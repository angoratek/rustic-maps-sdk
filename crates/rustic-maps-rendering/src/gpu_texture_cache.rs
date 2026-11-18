//! Byte-bounded LRU cache of tiles already uploaded to GPU memory.
//!
//! Uploads are expensive — every pooled payload is decoded to its full RGBA8
//! image and copied onto the device — and a steady camera keeps the same
//! tiles visible for many consecutive frames. This cache stores what
//! [`crate::headless::HeadlessTileRenderer`] uploaded, keyed by tile and
//! stamped with the pool's payload revision (see
//! [`crate::tile_texture_pool::TileTexturePool::payload_revision`]): a pool
//! insert under the same key replaces the bytes, so a cached texture whose
//! revision no longer matches is stale and must never be drawn from. Frames
//! prune stale entries first, then draw from whatever survives.
//!
//! Eviction order is least-recently-used, and the budget is a mirror of the
//! encoded-payload pool's byte budget idea: as with the pool, a single
//! oversized texture is kept alone rather than flash the map.

use crate::tile_key::TileKey;
use std::collections::HashMap;
use wgpu::{BindGroup, Texture};

/// Default GPU byte budget, sized as a heuristic for one complete 16 × 16
/// tile grid of 256-px RGBA8 tiles — the payload size platform raster tile
/// sources typically produce. Budgets size the cache, not the textures: the
/// upload path textures whatever a payload decodes to, so a 512-px payload
/// holds half as many tiles in the same byte budget.
pub const GPU_TILE_TEXTURE_CACHE_DEFAULT_MAXIMUM_BYTES: u64 =
    256 * TILE_TEXTURE_PIXELS * TILE_TEXTURE_PIXELS * PIXEL_BYTES;

/// Assumed texel edge of the tile textures the default budget heuristic is
/// sized for; the actual texture size comes from the decoded payload.
const TILE_TEXTURE_PIXELS: u64 = 256;

/// Bytes of one RGBA8 texel.
const PIXEL_BYTES: u64 = 4;

/// One tile's uploaded state: the GPU texture, the bind group that draws it
/// with the renderer's pipeline, and the identity data that decides whether
/// the upload is still valid.
#[derive(Debug)]
struct GpuTileTexture {
    pool_revision: u64,
    texture: Texture,
    bind_group: BindGroup,
    texture_bytes: u64,
    last_accessed_recency: u64,
}

impl GpuTileTexture {
    /// Whether this upload still matches the pool's payload: it is stale when
    /// the payload under the same tile key was replaced, reinserted, or
    /// evicted since.
    fn matches_pool_revision(&self, pool_revision: u64) -> bool {
        self.pool_revision == pool_revision
    }
}

/// LRU cache of uploaded tile textures, bounded in GPU bytes.
#[derive(Debug)]
pub struct GpuTileTextureCache {
    maximum_stored_bytes: u64,
    stored_bytes: u64,
    recency_counter: u64,
    /// Diagnostics: uploads ever inserted (each insert decodes and copies a
    /// payload onto the device), and lookups served by an existing upload.
    total_upload_count: u64,
    total_cache_hit_count: u64,
    entries: HashMap<TileKey, GpuTileTexture>,
}

impl GpuTileTextureCache {
    /// Creates an empty cache bounded by `maximum_stored_bytes`.
    pub fn new(maximum_stored_bytes: u64) -> Self {
        Self {
            maximum_stored_bytes,
            stored_bytes: 0,
            recency_counter: 0,
            total_upload_count: 0,
            total_cache_hit_count: 0,
            entries: HashMap::new(),
        }
    }

    /// Resizes the byte budget; entries beyond the new budget (keeping an
    /// oversized lone entry) evict immediately, instead of waiting for the
    /// next insertion that may never come on a steady all-hit scene.
    pub fn set_maximum_stored_bytes(&mut self, maximum_stored_bytes: u64) {
        self.maximum_stored_bytes = maximum_stored_bytes;
        self.evict_until_within_budget();
    }

    /// Refreshes an entry's recency without counting it as a lookup; reports
    /// whether the tile is currently uploaded. The renderer touches every
    /// tile about to be drawn before uploading, so one insert's eviction pass
    /// inside a frame cannot remove an entry a later quad of the same frame
    /// reuses.
    pub(crate) fn touch(&mut self, tile_key: &TileKey) -> bool {
        if let Some(cache_entry) = self.entries.get_mut(tile_key) {
            self.recency_counter += 1;
            cache_entry.last_accessed_recency = self.recency_counter;
            true
        } else {
            false
        }
    }

    /// Borrows the upload for the tile, but only when it matches the pool's
    /// current payload revision — a stale entry stays hidden for pruning.
    /// Refreshes the entry's recency and the hit diagnostics on a hit.
    pub(crate) fn lookup(
        &mut self,
        tile_key: &TileKey,
        pool_revision: u64,
    ) -> Option<(&Texture, &BindGroup)> {
        let cache_entry = self.entries.get_mut(tile_key)?;
        if !cache_entry.matches_pool_revision(pool_revision) {
            return None;
        }
        self.recency_counter += 1;
        cache_entry.last_accessed_recency = self.recency_counter;
        self.total_cache_hit_count += 1;
        Some((&cache_entry.texture, &cache_entry.bind_group))
    }

    /// Stores a fresh upload, evicting least-recently-used entries until the
    /// budget is honored. A single upload larger than the budget is still
    /// kept — dropping the only visible tile would flash the map rather than
    /// save the memory (exactly the pool's rule).
    pub(crate) fn insert(
        &mut self,
        tile_key: TileKey,
        pool_revision: u64,
        texture: Texture,
        bind_group: BindGroup,
        texture_bytes: u64,
    ) {
        self.recency_counter += 1;
        self.total_upload_count += 1;
        if let Some(previous_entry) = self.entries.insert(
            tile_key,
            GpuTileTexture {
                pool_revision,
                texture,
                bind_group,
                texture_bytes,
                last_accessed_recency: self.recency_counter,
            },
        ) {
            self.stored_bytes -= previous_entry.texture_bytes;
        }
        self.stored_bytes += texture_bytes;
        self.evict_until_within_budget();
    }

    /// Drops every upload whose payload revision no longer matches — the
    /// prune that keeps GPU bytes from pinning tiles whose bytes the pool
    /// replaced or no longer holds. Called by the renderer once per frame
    /// against the pool's current revisions.
    pub(crate) fn retain_matching(&mut self, mut matches: impl FnMut(&TileKey, u64) -> bool) {
        self.entries.retain(|tile_key, cache_entry| {
            let keep = matches(tile_key, cache_entry.pool_revision);
            if !keep {
                self.stored_bytes -= cache_entry.texture_bytes;
            }
            keep
        });
    }

    fn evict_until_within_budget(&mut self) {
        while self.stored_bytes > self.maximum_stored_bytes && self.entries.len() > 1 {
            let eviction_candidate = self
                .entries
                .iter()
                .min_by_key(|(_, cache_entry)| cache_entry.last_accessed_recency)
                .map(|(tile_key, _)| *tile_key);
            match eviction_candidate {
                Some(candidate) => {
                    if let Some(evicted_entry) = self.entries.remove(&candidate) {
                        self.stored_bytes -= evicted_entry.texture_bytes;
                    }
                }
                None => break,
            }
        }
    }

    /// Total GPU bytes currently held by cached uploads.
    pub fn stored_bytes(&self) -> u64 {
        self.stored_bytes
    }

    /// Number of tile uploads currently cached.
    pub fn stored_texture_count(&self) -> usize {
        self.entries.len()
    }

    /// Uploads ever inserted: each one paid a full decode plus device copy.
    pub fn total_upload_count(&self) -> u64 {
        self.total_upload_count
    }

    /// Lookups served by an existing upload instead of a new one.
    pub fn total_cache_hit_count(&self) -> u64 {
        self.total_cache_hit_count
    }
}

#[cfg(test)]
mod tests {
    use super::{GPU_TILE_TEXTURE_CACHE_DEFAULT_MAXIMUM_BYTES, TILE_TEXTURE_PIXELS};

    #[test]
    fn the_default_budget_holds_a_full_grid_of_tile_textures() {
        assert_eq!(
            GPU_TILE_TEXTURE_CACHE_DEFAULT_MAXIMUM_BYTES,
            TILE_TEXTURE_PIXELS * TILE_TEXTURE_PIXELS * 4 * 256
        );
    }
}
