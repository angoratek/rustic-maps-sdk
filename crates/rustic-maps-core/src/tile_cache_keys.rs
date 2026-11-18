//! Stable cache keys for tile payloads.
//!
//! Raster and vector tile payloads share one cache store, and nothing in a
//! tile address says which provider or source produced the bytes. Every key
//! therefore carries a `tile_source_namespace`: the provider key for a
//! catalog provider, a stable caller-supplied key for an application
//! provider. The namespace is part of the key, so two sources serving the
//! same address can never read each other's payloads.
//!
//! The vector-tile key lives here rather than in the storage crate because
//! the renderer builds the same key before handing a fetch to a store, and
//! the renderer must stay free of a storage backend.

/// The response-category prefix of every vector-tile payload key.
///
/// The prefix matches the vector-tiles category's storage name, which the
/// disk store reads to pick the entry's retention policy.
pub const VECTOR_TILES_FINGERPRINT_PREFIX: &str = "vector_tiles";

/// The key one vector-tile payload is stored under.
///
/// # Parameters
/// - `tile_source_namespace` — the provider or source that produces the
///   tiles; never a credential and never a raw URL.
/// - `zoom_level` — the web-mercator zoom level.
/// - `tile_x` — the tile column at that zoom level.
/// - `tile_y` — the tile row at that zoom level.
///
/// # Returns
/// A stable key of the form `vector_tiles:<namespace>/<zoom>/<x>/<y>`.
pub fn vector_tile_payload_fingerprint(
    tile_source_namespace: &str,
    zoom_level: u8,
    tile_x: u32,
    tile_y: u32,
) -> String {
    format!(
        "{VECTOR_TILES_FINGERPRINT_PREFIX}:{tile_source_namespace}/{zoom_level}/{tile_x}/{tile_y}"
    )
}

#[cfg(test)]
mod tests {
    use super::{VECTOR_TILES_FINGERPRINT_PREFIX, vector_tile_payload_fingerprint};

    #[test]
    fn the_key_names_the_source_and_the_address() {
        assert_eq!(
            vector_tile_payload_fingerprint("openfreemap", 14, 2662, 6285),
            "vector_tiles:openfreemap/14/2662/6285"
        );
        assert!(
            vector_tile_payload_fingerprint("openfreemap", 14, 2662, 6285)
                .starts_with(VECTOR_TILES_FINGERPRINT_PREFIX)
        );
    }

    #[test]
    fn two_sources_at_one_address_never_share_a_key() {
        assert_ne!(
            vector_tile_payload_fingerprint("here", 14, 2662, 6285),
            vector_tile_payload_fingerprint("mapbox", 14, 2662, 6285)
        );
    }

    #[test]
    fn the_address_components_stay_distinct() {
        assert_ne!(
            vector_tile_payload_fingerprint("here", 14, 2662, 6285),
            vector_tile_payload_fingerprint("here", 15, 2662, 6285)
        );
        assert_ne!(
            vector_tile_payload_fingerprint("here", 14, 2663, 6285),
            vector_tile_payload_fingerprint("here", 14, 2662, 6285)
        );
        assert_ne!(
            vector_tile_payload_fingerprint("here", 14, 2662, 6286),
            vector_tile_payload_fingerprint("here", 14, 2662, 6285)
        );
    }
}
