//! Shared vector-tile base pipeline: one scheduler that fetches a vector
//! tileset through the [`VectorTileProvider`] trait, decodes each tile once,
//! styles it, and answers the overlay primitives the frame draws beneath its
//! own overlays.
//!
//! Every target serves its vector base through this one implementation, so the
//! native surfaces and the browser cannot drift in what a tile paints. The
//! scheduler owns two caches:
//!
//! - **Decoded tiles**, keyed by the tile address: a tile's bytes are fetched
//!   and decoded once and never again while it stays cached; a style change
//!   restyles the decoded tile instead of refetching it.
//! - **Styled geometry**, keyed by the tile address, the style revision and the
//!   style's zoom bucket: the style's zoom span (`min_zoom`/`max_zoom`) and
//!   width stops are evaluated at the camera's zoom level, so a camera pan
//!   across a width stop repaints the tile from the style it already carries —
//!   no refetch, no decode. The zoom level quantizes to
//!   [`VECTOR_BASE_STYLE_ZOOM_QUANTIZATION_STEPS_PER_LEVEL`] steps per level so
//!   a continuous zoom gesture cannot grow the styled cache without bound.
//!
//! A tile that fails to fetch or decode does not fail the frame: the scheduler
//! skips it, keeps painting the tiles that succeeded, and reports the failure
//! in the frame's counts and its last failure detail. The base map is one layer
//! of a frame, never the frame's reason to fail.
//!
//! # Latency: payload cache, deliberate prefetch, progressive draw
//!
//! The camera-facing [`VectorBaseScheduler::cached_frame`] never waits on the
//! network: it paints the decoded tiles the cache holds, reports the rest as
//! pending or failed, and lets a detached prefetch fill the gaps. A missing
//! tile whose ancestor is cached draws that ancestor at the same camera zoom
//! (the raster path's parent-fallback semantics), so a new area blurs in at
//! coarse detail instead of flashing holes.
//!
//! Fetched wire payloads additionally pass through a
//! [`rustic_maps_core::CachedResponseStore`] keyed by the tileset's source name
//! and the tile address. The native surfaces install their disk store (TTL,
//! LRU and quota), so a region browsed once survives the next session; the
//! browser installs [`VectorTileMemoryCache`], the same policy in memory where
//! no SQLite exists.
//!
//! [`VectorBaseScheduler::prefetch_missing_tiles`] claims the visible misses;
//! [`VectorBaseScheduler::prefetch_ring_tiles`] then claims the ring around the
//! viewport (and the coarse parents that back a fallback) under a small budget.
//! Both return cancellable detached futures: a dropped future releases its
//! claims, so a scheduler that is replaced mid-fetch never strands a tile
//! address as permanently in flight.
//!
//! The scheduler is camera-agnostic: the caller lists the visible tile
//! addresses (through `MapCamera::visible_tile_keys` or the page's own grid)
//! and names the camera's zoom level; [`VectorBaseScheduler::served_zoom_level`]
//! clamps that listing to the tileset's declared zoom span.
//!
//! Each resolved frame also carries the drawn tiles' label candidates, read
//! from the active style's symbol layers. [`VectorBaseFrame::label_layer`]
//! places them at a camera pose through the SDK's embedded default glyph font,
//! so every platform surface draws the same place names without a
//! platform-specific asset. A frame without candidates yields `None` and
//! costs the renderer nothing.

use crate::camera::{MapCamera, ViewportPixels};
use crate::labels::{
    LabelFont, LabelLayer, MapLabelCandidate, PlacedLabel, default_label_font,
    label_candidates_for_compiled_layer_at_zoom_with_transition,
};
use crate::overlay::{MapOverlay, simplify_overlay_layers_for_style_zoom};
use crate::source_binding::{BoundVectorSource, VectorSourceBinding, vector_binding_policy};
use crate::sprite::{SpriteAtlas, SpriteSource};
use crate::tile_key::TileKey;
use crate::vector_style::{
    VectorStyleArtifactError, VectorTileAddress, vector_tile_overlay_layers_at_zoom,
    vector_tile_overlay_layers_at_zoom_with_transition, vector_tile_style_from_id,
    vector_tile_style_from_json,
};
use crate::zoom_transition::ZoomTransition;
use flate2::read::GzDecoder;
use futures::stream::{FuturesUnordered, StreamExt};
use geospatial_traits::{VectorTileProvider, VectorTileSourceDescriptor};
use geospatial_types::{VectorTileData, VectorTileEncoding};
use rustic_maps_core::CachedResponseStore;
use rustic_maps_style::{
    CompiledLayerKind, CompiledSource, CompiledSourceKind, CompiledStyle, UnsupportedProperty,
};
use rustic_maps_vector_tiles::{
    MAXIMUM_DECOMPRESSED_VECTOR_TILE_BYTES, VectorTile, decode_vector_tile,
};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::io::Read;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use web_time::{Duration, Instant};

/// The decoded-tile cache's default capacity. A 1080p frame asks for a few
/// dozen tiles, so the cache holds several camera positions' worth of decoded
/// geometry: a pan back over visited ground repaints without refetching.
pub const DEFAULT_VECTOR_BASE_MAXIMUM_CACHED_TILE_COUNT: usize = 512;

/// The styled-geometry cache's default capacity, in styled tiles. One decoded
/// tile can be styled once per zoom bucket the camera passed through, so this
/// cache is bounded on its own rather than only by decoded-tile eviction: four
/// buckets' worth of the decoded capacity covers a pan and a zoom while a
/// tile stays resident, and nothing beyond that is retained.
pub const DEFAULT_VECTOR_BASE_MAXIMUM_CACHED_STYLED_TILE_COUNT: usize =
    4 * DEFAULT_VECTOR_BASE_MAXIMUM_CACHED_TILE_COUNT;

/// How many style-zoom buckets one zoom level divides into. A bucket is the
/// granularity at which a restyle repaints: the camera zoom quantizes to the
/// nearest bucket, so a continuous zoom gesture restyles at most this many
/// times per level instead of once per frame.
pub const VECTOR_BASE_STYLE_ZOOM_QUANTIZATION_STEPS_PER_LEVEL: f64 = 4.0;

/// The default time-to-live of one persisted vector-tile payload: thirty
/// calendar days, the tile-imagery policy the provider crates assert against
/// the cache's `ResponseCategory::VectorTiles` default. A host that declares
/// a stricter ceiling passes its own value through
/// [`VectorBaseScheduler::with_payload_cache`].
pub const DEFAULT_VECTOR_TILE_PAYLOAD_TIME_TO_LIVE_SECONDS: u64 = 30 * 24 * 60 * 60;

/// The in-memory vector-tile payload cache's default capacity, in tiles. The
/// browser has no disk store, so the cache is process memory: a hundred-odd
/// wire payloads cover a viewport's worth of panning and a style switch
/// without holding a whole session's tiles decoded as well.
pub const DEFAULT_VECTOR_TILE_MEMORY_CACHE_MAXIMUM_TILE_COUNT: usize = 128;

/// How many ring tiles one detached prefetch plan may claim. The ring is an
/// anticipation, not a requirement: a few tiles around the viewport turn the
/// first pan into a cache hit without spending the service's low-volume
/// budget on a whole neighborhood.
pub const DEFAULT_VECTOR_BASE_PREFETCH_RING_TILE_BUDGET: usize = 32;

/// How many distinct failing tile addresses the scheduler keeps a failure
/// detail for. The map is a diagnostic aid, not a record: past this bound
/// each new failure evicts one existing entry, so a long session over failing
/// ground cannot grow it without limit.
const MAXIMUM_RECORDED_FAILURE_DETAILS: usize = 256;

/// The minimum interval between two ring prefetch plans. A plan is an
/// anticipation, so one every half second is enough to stay ahead of a
/// sustained pan while leaving the service and the frame loop alone; the
/// plan also waits until the visible set's own fetches have landed.
pub const DEFAULT_VECTOR_BASE_RING_PREFETCH_MINIMUM_INTERVAL_MILLISECONDS: u64 = 500;

/// How many visible-tile fetches one scheduler keeps in flight at once before
/// the next queued claim starts. A provider's token bucket declares the same
/// burst as its capacity; the scheduler keeps its claims whole (every visible
/// miss is queued) but starts them in bursts, so the service never sees more
/// simultaneous requests than it allowed and a fit's first tile lands without
/// waiting for the rest.
pub const DEFAULT_VECTOR_BASE_VISIBLE_PREFETCH_BURST_TILE_COUNT: usize = 8;

/// The lowest served zoom level at which [`VectorBaseScheduler::prefetch_ring_tiles`]
/// plans ahead. Below it a screen holds most of the world's tiles: the ring
/// costs a large fraction of the visible set and predicts the next camera far
/// worse than it does at city zoom, so a fit stays visible-only.
pub const DEFAULT_VECTOR_BASE_RING_PREFETCH_MINIMUM_ZOOM_LEVEL: u8 = 8;

/// The first byte offset and prime of the FNV-1a hash a frame's content
/// revision is built with; the revision only has to differ when the frame's
/// inputs differ, so a fast non-cryptographic mix is enough.
const FRAME_CONTENT_REVISION_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// See [`FRAME_CONTENT_REVISION_OFFSET_BASIS`].
const FRAME_CONTENT_REVISION_PRIME: u64 = 0x0000_0100_0000_01b3;

/// The eight neighbor offsets a viewport ring is grown with, in tile units.
const VECTOR_BASE_RING_NEIGHBOR_OFFSETS: [(i32, i32); 8] = [
    (-1, -1),
    (0, -1),
    (1, -1),
    (-1, 0),
    (1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
];

/// Milliseconds in one second, for converting a frame clock reading in
/// seconds to the cross-fade's millisecond timestamps.
const MILLISECONDS_PER_SECOND: f64 = 1_000.0;

/// One resolved frame's cache and latency accounting, read beside the tile
/// counts so a device measurement can answer "is it the server?" with data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VectorBaseFrameDiagnostics {
    /// Payload-cache lookups that hit, cumulative over this scheduler's
    /// fetches (zero while no payload cache is installed).
    pub payload_cache_hit_count: u64,
    /// Payload-cache lookups that missed (or failed to read, degrading to the
    /// network), cumulative over this scheduler's fetches.
    pub payload_cache_miss_count: u64,
    /// Tile addresses with a fetch or decode running when the frame resolved.
    pub in_flight_fetch_count: usize,
    /// Milliseconds from installing the scheduler to the first frame that
    /// painted any tile (own or fallback), or `None` while nothing has
    /// painted yet.
    pub time_to_first_drawn_tile_milliseconds: Option<u64>,
    /// How many tiles this frame styled from decoded geometry because the
    /// styled cache held no entry for the tile, style revision and zoom
    /// bucket. Zero on a warm refit of the same camera; one when a single new
    /// tile arrives. The count is the evidence that tile arrivals reuse the
    /// per-tile styled cache instead of restyling the composed base.
    pub tessellated_tile_count: usize,
    /// Tiles whose label candidates this frame extracted from the decoded
    /// geometry. A settled frame at an unchanged camera pose, style revision
    /// and zoom bucket extracts none: it reads the candidates the previous
    /// frame cached, so a steady frame pays no label walk.
    pub label_candidate_extraction_count: usize,
    /// Tiles whose label candidates this frame read from the per-tile cache
    /// instead of extracting them. Only a frame that assembles labels counts
    /// a hit.
    pub label_candidate_cache_hit_count: usize,
}

/// One resolved vector base frame: the overlay primitives to draw beneath the
/// caller's overlays plus the honest counts of what the frame drew.
#[derive(Debug, Clone, PartialEq)]
pub struct VectorBaseFrame {
    /// The base overlay primitives in global style order: every tile's layer 0
    /// first, then every tile's layer 1, and so on (later entries paint over
    /// earlier ones). Within one style layer the tiles keep their address
    /// order, with coarse ancestor stand-ins before the tiles they cover.
    ///
    /// Shared behind an [`Arc`]: cloning a frame clones this list's reference
    /// count, so a surface that clones the frame never copies tens of
    /// thousands of overlays. Compare [`Self::content_revision`] to decide
    /// whether the list changed, not the list itself.
    pub overlays: Arc<Vec<MapOverlay>>,
    /// The frame's label candidates, read from the drawn tiles' symbol layers
    /// under the active style, in tile-address order and then style-layer
    /// order. Empty when the style declares no symbol layer or no drawn tile
    /// carries one, so a frame without labels stays label-free.
    pub label_candidates: Vec<MapLabelCandidate>,
    /// The sprite atlas the style's `sprite` URL resolved to through the
    /// scheduler's injected source, when it declares one and the source served
    /// it. `None` leaves the frame's icon layers undrawn; their text still
    /// places.
    pub sprite_atlas: Option<Arc<SpriteAtlas>>,
    /// The distinct tile addresses the frame asked for, after clamping and
    /// deduplication.
    pub requested_tile_count: usize,
    /// How many of those tiles the frame drew at their own zoom.
    pub drawn_tile_count: usize,
    /// How many of those tiles the frame covered with a cached ancestor while
    /// their own detail is missing (in flight or failed). Each covered tile
    /// also appears in [`Self::pending_tile_count`] or
    /// [`Self::failed_tile_count`], because the ancestor is a stand-in, not
    /// the tile itself; the frame's painted coverage is `drawn + fallback`.
    pub fallback_tile_count: usize,
    /// How many of those tiles failed to fetch or decode this frame.
    pub failed_tile_count: usize,
    /// How many of those tiles have a fetch in flight (or not yet started) and
    /// were therefore neither drawn nor failed this frame. A frame resolved
    /// from the cache alone reports them here, so a caller can tell "loading"
    /// from "failed" and keep presenting while the prefetch runs.
    pub pending_tile_count: usize,
    /// The style the frame painted with.
    pub style_id: String,
    /// The style's canvas background, or `None` when the style paints its own
    /// full ground fill. A surface that hides the raster base while the vector
    /// base serves clears to this color beneath the overlays, so the gaps
    /// between the style's translucent fills read as the designed map rather
    /// than as the platform's clear color.
    pub background: Option<[f32; 4]>,
    /// The failure detail of the last tile that failed this frame, if any.
    pub last_failure_detail: Option<String>,
    /// Whether this frame drew in motion-graded mode: the camera was moving
    /// (a pan, zoom or rotate gesture), so the frame reused the last settled
    /// style zoom and skipped label assembly. The next still frame resolves
    /// both at the final camera pose and reports `false`.
    pub motion_graded: bool,
    /// Whether a tile-level cross-fade still owes frames: the previous level
    /// is drawn while the requested one fills, then dissolves out. A running
    /// transition keeps a presenting surface unsettled so its pump draws the
    /// fade out, exactly like the raster base's transition.
    pub zoom_transition_running: bool,
    /// How many retired-level tiles this frame still drew while the requested
    /// level fills (at their own address or as an ancestor stand-in). Zero
    /// when no transition retains a level or the requested level already
    /// paints the same tiles as ancestor stand-ins.
    pub retained_tile_count: usize,
    /// The retired tile zoom level the frame drew, when a transition retains
    /// one.
    pub retained_tile_zoom_level: Option<u8>,
    /// Opacity the retired level is drawn at: one during the hold, ramping to
    /// zero across the fade.
    pub retained_opacity: f32,
    /// The frame's per-source layer accounting, sorted by source id. A source
    /// matched a layer when the painted tileset carried the `source-layer` the
    /// layer reads; it drew a layer when that layer painted geometry or placed
    /// a label.
    pub source_layer_counts: Vec<VectorSourceLayerCount>,
    /// The frame's cache-hit, in-flight and first-drawn-tile accounting.
    pub diagnostics: VectorBaseFrameDiagnostics,
    /// The revision of the frame's content — its drawn and fallback tiles,
    /// style revision, style-zoom bucket, sprite atlas and label assembly.
    /// Two frames with the same revision paint the same overlays and carry
    /// the same label candidates, so a caller compares this integer instead
    /// of deep-comparing tens of thousands of overlays (and can share the
    /// frame behind an `Arc` instead of cloning it). A frame's diagnostic
    /// counters are deliberately not part of the revision: they change
    /// without changing a pixel.
    pub content_revision: u64,
}

impl VectorBaseFrame {
    /// Whether the frame drew every requested tile and none failed — the
    /// base map has nothing left to fetch for this camera.
    pub fn is_idle(&self) -> bool {
        self.failed_tile_count == 0
            && self.pending_tile_count == 0
            && self.fallback_tile_count == 0
            && self.drawn_tile_count == self.requested_tile_count
    }

    /// The frame's label layer at the camera pose, measured and rasterized
    /// through the SDK's embedded default glyph font ([`default_label_font`]).
    ///
    /// `None` when the frame carries no label candidates — a style without
    /// symbol layers, a raster-only view, or no drawn tile naming anything —
    /// so a caller that forwards the layer to the renderer costs no buffer,
    /// no atlas upload and no draw on label-free frames. A frame whose
    /// candidates all collide off-screen also yields an empty layer, which the
    /// renderer treats the same way.
    pub fn label_layer(
        &self,
        camera: &MapCamera,
        viewport: &ViewportPixels,
        field_of_view_degrees: f64,
    ) -> Option<LabelLayer> {
        self.label_layer_with_font(
            camera,
            viewport,
            field_of_view_degrees,
            default_label_font(),
        )
    }

    /// [`Self::label_layer`] for a surface whose drawing buffer is not one
    /// device pixel per point: the placed glyph sizes and the fragment edge
    /// gamma follow `device_pixel_ratio`, so the labels keep their declared
    /// point size on a 2× or 3× surface (the style numbers stay the source of
    /// truth for the halo and gamma math).
    pub fn label_layer_with_device_pixel_ratio(
        &self,
        camera: &MapCamera,
        viewport: &ViewportPixels,
        field_of_view_degrees: f64,
        device_pixel_ratio: f64,
    ) -> Option<LabelLayer> {
        self.label_layer_with_font_and_device_pixel_ratio(
            camera,
            viewport,
            field_of_view_degrees,
            default_label_font(),
            device_pixel_ratio,
        )
    }

    /// [`Self::label_layer_with_device_pixel_ratio`] carrying the previous
    /// frame's placed labels, so the names already on screen keep their spots
    /// while the camera moves instead of letting newcomers displace them; see
    /// [`LabelLayer::place_with_history`].
    pub fn label_layer_with_device_pixel_ratio_and_history(
        &self,
        camera: &MapCamera,
        viewport: &ViewportPixels,
        field_of_view_degrees: f64,
        device_pixel_ratio: f64,
        previous_labels: &[PlacedLabel],
    ) -> Option<LabelLayer> {
        self.label_layer_with_font_and_device_pixel_ratio_and_history(
            camera,
            viewport,
            field_of_view_degrees,
            default_label_font(),
            device_pixel_ratio,
            previous_labels,
        )
    }

    /// [`Self::label_layer`] against an explicit glyph font: the override for
    /// a caller that supplies its own glyph source instead of the SDK's
    /// embedded Latin range.
    pub fn label_layer_with_font(
        &self,
        camera: &MapCamera,
        viewport: &ViewportPixels,
        field_of_view_degrees: f64,
        font: &LabelFont,
    ) -> Option<LabelLayer> {
        self.label_layer_with_font_and_device_pixel_ratio(
            camera,
            viewport,
            field_of_view_degrees,
            font,
            1.0,
        )
    }

    /// [`Self::label_layer_with_font`] for a surface whose drawing buffer is
    /// not one device pixel per point; see
    /// [`Self::label_layer_with_device_pixel_ratio`].
    pub fn label_layer_with_font_and_device_pixel_ratio(
        &self,
        camera: &MapCamera,
        viewport: &ViewportPixels,
        field_of_view_degrees: f64,
        font: &LabelFont,
        device_pixel_ratio: f64,
    ) -> Option<LabelLayer> {
        self.label_layer_with_font_and_device_pixel_ratio_and_history(
            camera,
            viewport,
            field_of_view_degrees,
            font,
            device_pixel_ratio,
            &[],
        )
    }

    /// The history-carrying entry point: [`Self::label_layer_with_font_and_device_pixel_ratio`]
    /// with the previous frame's placed labels, so a name already on screen
    /// keeps its spot while the camera moves; see
    /// [`LabelLayer::place_with_history`].
    pub fn label_layer_with_font_and_device_pixel_ratio_and_history(
        &self,
        camera: &MapCamera,
        viewport: &ViewportPixels,
        field_of_view_degrees: f64,
        font: &LabelFont,
        device_pixel_ratio: f64,
        previous_labels: &[PlacedLabel],
    ) -> Option<LabelLayer> {
        if self.label_candidates.is_empty() {
            return None;
        }
        Some(LabelLayer::place_with_history_and_sprite(
            camera,
            viewport,
            field_of_view_degrees,
            &self.label_candidates,
            font,
            device_pixel_ratio,
            previous_labels,
            self.sprite_atlas.as_ref(),
        ))
    }
}

/// The styled-geometry cache key: one tile's paint under one style revision at
/// one style-zoom bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct StyledTileCacheKey {
    /// The tile the geometry belongs to.
    tile_key: TileKey,
    /// The style revision the overlay geometry was styled under.
    style_revision: u64,
    /// The style zoom bucket (the quantized camera zoom level).
    quantized_style_zoom: i64,
}

/// One tile's cached styled contents under one style revision and zoom
/// bucket: the overlay geometry and the label candidates read from the same
/// decoded tile at the same evaluated zoom. Caching both together is what
/// makes an unchanged settle frame free of the per-tile symbol walk.
#[derive(Debug, Clone)]
struct StyledTileContents {
    /// The styled overlay geometry, bucketed by compiled layer index.
    overlays: Vec<Vec<MapOverlay>>,
    /// The tile's label candidates, or `None` until a frame that assembles
    /// labels has extracted them. A motion-graded frame styles geometry but
    /// skips labels, so the settle that follows still extracts them once.
    label_candidates: Option<Vec<MapLabelCandidate>>,
    /// One flag per compiled layer: whether that layer placed at least one
    /// label from this tile. `None` until the tile's labels are extracted,
    /// matching [`Self::label_candidates`].
    label_contributed_layers: Option<Vec<bool>>,
}

/// The per-frame styling inputs every tile's append shares: the style-zoom
/// bucket that keys the styled cache, the zoom paint and labels evaluate at,
/// a running style transition, and whether the frame assembles labels at all.
#[derive(Clone, Copy)]
struct StyledTileFrame<'a> {
    /// The quantized camera zoom the styled cache keys on.
    quantized_style_zoom: i64,
    /// The fractional zoom paint expressions and labels evaluate at.
    evaluated_style_zoom: f64,
    /// The previous style and elapsed seconds while a transition blends.
    transition: Option<&'a (CompiledStyle, f64)>,
    /// Whether this frame reads the tile's label candidates. A motion-graded
    /// frame leaves them out; the settle frame extracts them.
    assemble_labels: bool,
}

/// The decoded-tile cache shared between the presenting frame and its detached
/// prefetch. The frame resolves from whatever the cache already holds, while a
/// spawned prefetch fetches every missing tile concurrently and inserts it, so
/// a first frame over unvisited ground never waits on the network. The cache
/// also remembers in-flight addresses (so a repaint does not start duplicate
/// fetches) and the last failure per address (so a frame can report it until
/// the next prefetch retries).
#[derive(Debug)]
struct DecodedTileCache {
    /// Decoded tiles by address (fetched and decoded once).
    tiles: HashMap<TileKey, Arc<VectorTile>>,
    /// The cache's insertion order, for eviction.
    insertion_order: VecDeque<TileKey>,
    /// Addresses whose fetch is running.
    in_flight: HashSet<TileKey>,
    /// The last failure detail per address, cleared by the retry.
    failure_details: HashMap<TileKey, String>,
    /// The cache's capacity.
    maximum_tile_count: usize,
}

impl DecodedTileCache {
    fn new(maximum_tile_count: usize) -> Self {
        Self {
            tiles: HashMap::new(),
            insertion_order: VecDeque::new(),
            in_flight: HashSet::new(),
            failure_details: HashMap::new(),
            maximum_tile_count,
        }
    }

    fn get(&self, tile_key: &TileKey) -> Option<Arc<VectorTile>> {
        self.tiles.get(tile_key).cloned()
    }

    /// Claims the address for a fetch; `false` when it is already cached or a
    /// fetch is running. Claiming clears any previous failure so the frame
    /// reports "pending", not a stale refusal, while the retry runs.
    fn begin_fetch(&mut self, tile_key: TileKey) -> bool {
        if self.tiles.contains_key(&tile_key) || !self.in_flight.insert(tile_key) {
            return false;
        }
        self.failure_details.remove(&tile_key);
        true
    }

    /// Releases an in-flight claim without storing the result, for a ring
    /// fetch whose tile the camera has already left.
    fn discard_fetch(&mut self, tile_key: TileKey) {
        self.in_flight.remove(&tile_key);
    }

    /// Records one finished fetch: the decoded tile on success, the detail on
    /// failure, and clears the in-flight mark either way.
    fn finish_fetch(&mut self, tile_key: TileKey, outcome: Result<Arc<VectorTile>, String>) {
        self.in_flight.remove(&tile_key);
        match outcome {
            Ok(decoded_tile) => {
                self.failure_details.remove(&tile_key);
                self.insert_decoded_tile(tile_key, decoded_tile);
            }
            Err(failure_detail) => {
                // The failure map is a diagnostic aid, not a record: it is
                // capped so a long session over failing ground cannot grow it
                // without bound.
                if !self.failure_details.contains_key(&tile_key)
                    && self.failure_details.len() >= MAXIMUM_RECORDED_FAILURE_DETAILS
                    && let Some(evicted_key) = self.failure_details.keys().next().copied()
                {
                    self.failure_details.remove(&evicted_key);
                }
                self.failure_details.insert(tile_key, failure_detail);
            }
        }
    }

    /// Inserts a decoded tile and evicts the oldest decoded tiles beyond the
    /// cache's capacity. Styled geometry is keyed by the address and a style
    /// revision, so it stays valid for a re-fetched address and is bounded by
    /// its own cache; eviction here never touches it.
    fn insert_decoded_tile(&mut self, tile_key: TileKey, decoded_tile: Arc<VectorTile>) {
        if self.tiles.insert(tile_key, decoded_tile).is_none() {
            self.insertion_order.push_back(tile_key);
        }
        while self.tiles.len() > self.maximum_tile_count {
            let Some(evicted_tile_key) = self.insertion_order.pop_front() else {
                break;
            };
            self.tiles.remove(&evicted_tile_key);
        }
    }
}

/// The in-memory vector-tile payload cache the browser installs where no
/// SQLite store exists: the same cache-then-network policy as the native disk
/// store (a fingerprint key, a time-to-live and a bounded capacity), held in
/// process memory with least-recently-used eviction.
///
/// The cache is shared across scheduler installs, so switching a style or
/// reinstalling the same tileset serves the region from memory instead of
/// refetching it. It never outlives the page.
#[derive(Debug)]
pub struct VectorTileMemoryCache {
    /// The cached payloads and their access order.
    state: Mutex<VectorTileMemoryCacheState>,
    /// The cache's capacity, in tiles.
    maximum_tile_count: usize,
}

#[derive(Debug, Default)]
struct VectorTileMemoryCacheState {
    /// Cached payloads by request fingerprint.
    entries: HashMap<String, VectorTileMemoryCacheEntry>,
    /// Monotonic access stamp, for least-recently-used eviction.
    access_sequence: u64,
}

#[derive(Debug)]
struct VectorTileMemoryCacheEntry {
    /// The stored wire payload.
    payload: Arc<VectorTileData>,
    /// When the entry expires.
    expires_at: Instant,
    /// The access stamp of the entry's last read or write.
    last_accessed: u64,
}

impl VectorTileMemoryCache {
    /// Builds an empty payload cache holding at most `maximum_tile_count`
    /// tiles.
    pub fn new(maximum_tile_count: usize) -> Self {
        Self {
            state: Mutex::new(VectorTileMemoryCacheState::default()),
            maximum_tile_count,
        }
    }

    /// The fingerprints the cache currently holds, for tests and diagnosis.
    pub fn cached_tile_count(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entries
            .len()
    }
}

impl CachedResponseStore<VectorTileData> for VectorTileMemoryCache {
    fn lookup_response(
        &self,
        request_fingerprint: &str,
    ) -> Result<Option<Arc<VectorTileData>>, geospatial_types::SDKError> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let access_stamp = {
            state.access_sequence += 1;
            state.access_sequence
        };
        let Some(entry) = state.entries.get_mut(request_fingerprint) else {
            return Ok(None);
        };
        if entry.expires_at <= Instant::now() {
            state.entries.remove(request_fingerprint);
            return Ok(None);
        }
        entry.last_accessed = access_stamp;
        Ok(Some(Arc::clone(&entry.payload)))
    }

    fn store_response(
        &self,
        request_fingerprint: &str,
        response_value: Arc<VectorTileData>,
        time_to_live_seconds: u64,
    ) -> Result<(), geospatial_types::SDKError> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.access_sequence += 1;
        let access_stamp = state.access_sequence;
        state.entries.insert(
            request_fingerprint.to_string(),
            VectorTileMemoryCacheEntry {
                payload: response_value,
                expires_at: Instant::now() + Duration::from_secs(time_to_live_seconds),
                last_accessed: access_stamp,
            },
        );
        while state.entries.len() > self.maximum_tile_count {
            let Some(least_recently_used_fingerprint) = state
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_accessed)
                .map(|(fingerprint, _)| fingerprint.clone())
            else {
                break;
            };
            state.entries.remove(&least_recently_used_fingerprint);
        }
        Ok(())
    }
}

/// The cache-then-network hit/miss counters a detached prefetch updates and a
/// frame reads. Shared by `Arc` because the prefetch outlives the frame that
/// started it.
#[derive(Debug, Default)]
struct VectorBasePayloadCacheCounters {
    /// Payload-cache lookups that hit.
    hit_count: AtomicU64,
    /// Payload-cache lookups that missed.
    miss_count: AtomicU64,
}

/// The fingerprint one vector-tile payload is stored under: the response
/// category prefix the disk store reads its TTL category from, the tileset's
/// source name (so two tilesets serving the same address never collide) and
/// the tile address. The builder lives in the core crate because the cached
/// vector-tile wrapper stores under the same key.
fn vector_tile_payload_cache_fingerprint(source_name: &str, tile_key: TileKey) -> String {
    rustic_maps_core::vector_tile_payload_fingerprint(
        source_name,
        tile_key.zoom_level,
        tile_key.tile_x,
        tile_key.tile_y,
    )
}

/// The guard of a detached prefetch: it clears the in-flight marks of every
/// claimed tile still undelivered when the future is dropped. Without it, a
/// cancelled prefetch (the runtime shutting down, or a replaced scheduler)
/// would report its addresses pending forever and no later plan would retry
/// them.
struct PrefetchClaimGuard {
    /// The cache the claims live in.
    decoded_tiles: Arc<Mutex<DecodedTileCache>>,
    /// The claimed addresses not yet finished.
    remaining_tile_keys: Vec<TileKey>,
}

impl PrefetchClaimGuard {
    fn new(decoded_tiles: Arc<Mutex<DecodedTileCache>>, claimed_tile_keys: Vec<TileKey>) -> Self {
        Self {
            decoded_tiles,
            remaining_tile_keys: claimed_tile_keys,
        }
    }

    /// Records one finished fetch, so a later drop only clears the claims that
    /// are still genuinely in flight.
    fn mark_finished(&mut self, tile_key: TileKey) {
        self.remaining_tile_keys.retain(|key| *key != tile_key);
    }
}

impl Drop for PrefetchClaimGuard {
    fn drop(&mut self) {
        let mut decoded_tiles = self
            .decoded_tiles
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        for tile_key in &self.remaining_tile_keys {
            decoded_tiles.in_flight.remove(tile_key);
        }
    }
}

/// The previous frame's painted tile set, ready to be retained across a tile
/// level change. It remembers the exact tiles the frame drew — own tiles and
/// ancestor stand-ins — plus the style revision and zoom bucket their geometry
/// was styled under, so the retired level is reconstructed from the styled
/// cache rather than fetched or copied every frame.
#[derive(Debug, Clone)]
struct LevelFrameSnapshot {
    /// The tile zoom level the frame drew.
    tile_zoom_level: u8,
    /// The style revision its geometry was styled under.
    style_revision: u64,
    /// The zoom bucket its geometry was styled at.
    quantized_style_zoom: i64,
    /// The style's compiled layer count when the frame was styled, so a
    /// retained fade keeps every bucket even if the next style has a
    /// different layer count.
    layer_count: usize,
    /// The tiles that contributed geometry to the frame.
    tile_keys: Vec<TileKey>,
}

/// The shared vector base pipeline: fetches, decodes, styles and caches one
/// vector tileset.
pub struct VectorBaseScheduler {
    /// The tileset the scheduler fetches through.
    provider: Arc<dyn VectorTileProvider>,
    /// The active style's identifier (the style name the status reports).
    style_id: String,
    /// The active style's compiled render plans.
    style: CompiledStyle,
    /// The style's own vector source, bound through the provider: the TileJSON
    /// policy the style declares (its zoom span and attribution, with the
    /// descriptor supplying whatever the style leaves out). `None` for a style
    /// that declares no vector source, which falls back to the descriptor.
    bound_base_source: Option<Arc<BoundVectorSource>>,
    /// The injected sprite loader the style's `sprite` URL is resolved
    /// through, when the host supplies one. The renderer never fetches.
    sprite_source: Option<Arc<dyn SpriteSource>>,
    /// The resolved sprite atlas, keyed by the URL it was loaded for; the
    /// inner option records a failed load so a frame does not retry it every
    /// draw.
    sprite_atlas: Option<(String, Option<Arc<SpriteAtlas>>)>,
    /// The glyph font the style's own text draws with, when the host fetched
    /// one from the style's glyph endpoint. `None` keeps the SDK's embedded
    /// Latin range ([`default_label_font`]).
    label_font: Option<Arc<LabelFont>>,
    /// Bumped on every style change; part of the styled cache key so a
    /// geometry styled under a previous style is never reused.
    style_revision: u64,
    /// The style being replaced, kept while its declared transition is
    /// still blending; `None` when no transition is running.
    previous_style: Option<CompiledStyle>,
    /// The monotonic clock reading, in seconds, of the first timed frame
    /// after a style change; `None` until such a frame arrives.
    transition_started_at_seconds: Option<f64>,
    /// The decoded-tile cache shared with the detached prefetch.
    decoded_tiles: Arc<Mutex<DecodedTileCache>>,
    /// Styled overlay geometry by tile, style revision and zoom bucket,
    /// bucketed by compiled layer index so frame assembly can paint in global
    /// style order across every tile.
    styled_tiles: HashMap<StyledTileCacheKey, StyledTileContents>,
    /// The styled cache's insertion order, for eviction.
    styled_tile_insertion_order: VecDeque<StyledTileCacheKey>,
    /// The styled cache's capacity.
    maximum_cached_styled_tile_count: usize,
    /// The persistent payload store the fetches pass through, or `None` for
    /// an uncached scheduler.
    payload_cache: Option<Arc<dyn CachedResponseStore<VectorTileData>>>,
    /// The payload store's time-to-live, in seconds.
    payload_cache_time_to_live_seconds: u64,
    /// The payload store's shared hit/miss counters.
    payload_cache_counters: Arc<VectorBasePayloadCacheCounters>,
    /// The ring prefetch's per-plan tile budget.
    maximum_prefetch_ring_tile_count: usize,
    /// The visible prefetch's concurrent-fetch burst. The provider's own
    /// pacing (a token bucket whose capacity is its burst) is installed here
    /// by the caller, so a first fit never opens more requests at once than
    /// the service allows; the claims still cover the whole visible set,
    /// queued behind the burst, and each completion paints on its own.
    visible_prefetch_burst_tile_count: usize,
    /// The lowest served zoom level at which the ring prefetch plans ahead.
    ring_prefetch_minimum_zoom_level: u8,
    /// The viewport's latest visible set, for dropping ring work that a pan
    /// has made irrelevant before it reaches the styled cache.
    latest_visible_tile_keys: Arc<Mutex<Vec<TileKey>>>,
    /// The last frame this scheduler assembled, shared with the caller while
    /// its content revision and diagnostics stand. An unchanged frame costs
    /// one `Arc` clone instead of a second assembly and a deep overlay
    /// compare.
    last_frame: Option<Arc<VectorBaseFrame>>,
    /// The minimum interval between two ring plans.
    ring_prefetch_minimum_interval: Duration,
    /// When the last ring plan was started, for the interval gate.
    last_ring_prefetch_at: Mutex<Option<Instant>>,
    /// When this scheduler was installed, for the first-drawn-tile latency.
    installed_at: Instant,
    /// The time to the first frame that painted any tile, once it happened.
    first_drawn_tile_milliseconds: Option<u64>,
    /// The last frame's last tile failure, for the status readback.
    last_failure_detail: Option<String>,
    /// The style-zoom bucket the last settled (ungraded) frame resolved at.
    /// A motion-graded frame reuses it instead of styling at the moving
    /// camera's own bucket, so a zoom gesture restyles nothing until it
    /// settles. `None` until the first settled frame, and then a graded frame
    /// falls back to the camera's current bucket.
    settled_style_zoom_bucket: Option<i64>,
    /// The tile-level cross-fade state. It has the same semantics as the
    /// raster base's transition: a level change holds the previous level
    /// while the new one fills, caps the hold at one second and dissolves the
    /// retained level over the fade once the new level is ready.
    zoom_transition: ZoomTransition,
    /// The retired level's painted tile set while a transition runs. Set
    /// when a level change is observed and dropped when the transition
    /// completes.
    retained_level: Option<LevelFrameSnapshot>,
    /// The last resolved frame's painted tile set, captured so a level change
    /// in the next frame can retain what was actually on screen.
    last_level_frame: Option<LevelFrameSnapshot>,
}

/// The source id the committed styles and the provider documents name their
/// tileset by.
pub const BASE_SOURCE_ID: &str = "base";

/// Binds the style's vector source to the scheduler's provider through the
/// shared TileJSON policy, so the style's own zoom span and attribution reach
/// the base and the provider descriptor fills whatever the style leaves out.
///
/// The style names its source `base`; a document that uses another id still
/// binds its first vector source, because the scheduler fetches exactly one
/// tileset whichever id names it. Every vector source the style's layers
/// reference is held to that one served tileset by
/// [`verify_declared_vector_sources`] before the scheduler is built, so the
/// single bound stream serves them all.
fn bound_base_vector_source(
    provider: &Arc<dyn VectorTileProvider>,
    style: &CompiledStyle,
) -> Option<Arc<BoundVectorSource>> {
    let descriptor = provider.source_descriptor();
    let vector_source = style
        .sources
        .iter()
        .find(|(source_id, _)| source_id.as_str() == BASE_SOURCE_ID)
        .or_else(|| style.sources.iter().next());
    let (source_id, source) = vector_source?;
    let rustic_maps_style::CompiledSource::Vector(vector_source) = source else {
        return None;
    };
    let binding = VectorSourceBinding::new(
        source_id.clone(),
        descriptor.source_name.clone(),
        Arc::clone(provider),
    );
    let policy = vector_binding_policy(&binding, vector_source);
    Some(Arc::new(BoundVectorSource {
        source_id: source_id.clone(),
        provider_key: descriptor.source_name,
        provider: Arc::clone(provider),
        policy,
    }))
}

/// The maximum number of distinct unsupported-property names a status
/// readback lists.
///
/// The companion count reports every diagnostic the style produced; only the
/// name list is bounded, so a large vendor document cannot grow the readback
/// without bound. Names are sorted before the cap is applied, so the same
/// document always reports the same slice.
pub const MAXIMUM_REPORTED_UNSUPPORTED_PROPERTY_NAMES: usize = 32;

/// The maximum number of degraded-asset names a status readback lists.
///
/// The names are sorted before the cap is applied, exactly like
/// [`MAXIMUM_REPORTED_UNSUPPORTED_PROPERTY_NAMES`].
pub const MAXIMUM_REPORTED_DEGRADED_ASSET_NAMES: usize = 16;

/// The style-fidelity diagnostics a vector base reports beside its frame
/// counts: how much of the document the renderer could not apply, which
/// declared assets fell back to a degraded draw, and which declared sources
/// the vector base cannot draw at all.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VectorBaseStyleFidelity {
    /// Every unsupported-property diagnostic the style produced, including a
    /// known layer kind this renderer cannot paint.
    pub unsupported_property_count: u32,
    /// The distinct unsupported properties, scope-qualified and sorted, capped
    /// at [`MAXIMUM_REPORTED_UNSUPPORTED_PROPERTY_NAMES`].
    pub unsupported_property_names: Vec<String>,
    /// The assets that fell back to a degraded draw (a declared sprite the
    /// install could not fetch, a declared glyph endpoint whose ranges did not
    /// pack, or a declared source of a kind the vector base cannot draw),
    /// sorted and capped at [`MAXIMUM_REPORTED_DEGRADED_ASSET_NAMES`].
    pub degraded_asset_names: Vec<String>,
}

/// One vector source's layer accounting for a resolved frame.
///
/// `matched_layer_count` counts the source's style layers whose `source-layer`
/// the painted tileset carries (a layer that names no source layer always
/// matches). `drawn_layer_count` counts the source's layers that painted
/// geometry or placed a label in the frame. The two differ when a layer reads
/// a source layer the tileset does not carry, when its zoom span or visibility
/// excludes the frame, or when its filter admits no feature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorSourceLayerCount {
    /// The style source id the counts belong to.
    pub source_id: String,
    /// The source's layers whose source layer the painted tileset carries.
    pub matched_layer_count: u32,
    /// The source's layers that painted in the frame.
    pub drawn_layer_count: u32,
}

/// The source ids the style's layers reference, in sorted order.
fn referenced_source_ids(style: &CompiledStyle) -> BTreeSet<&str> {
    style
        .layers
        .iter()
        .filter_map(|layer| layer.source.as_deref())
        .collect()
}

/// The name fragments that identify the tileset a descriptor serves, sorted.
///
/// The descriptor's `source_name` is `vendor/tileset` for every catalog
/// provider (`here/vector`, `mapbox/mapbox.mapbox-streets-v8`), so both the
/// full name and its tileset half identify the served tileset. The provider's
/// declared `style_hints` are additional identities: they are its own
/// statement of the tileset's schema or style family (`omv` and `base` for
/// HERE's vector tileset), never a guess, and a published style document may
/// name them in its source address where it names no tileset id.
fn served_tileset_identities(descriptor: &VectorTileSourceDescriptor) -> Vec<&str> {
    let mut identities = BTreeSet::new();
    identities.insert(descriptor.source_name.as_str());
    if let Some((_, tileset_id)) = descriptor.source_name.rsplit_once('/')
        && !tileset_id.is_empty()
    {
        identities.insert(tileset_id);
    }
    for style_hint in &descriptor.style_hints {
        let style_hint = style_hint.trim();
        if !style_hint.is_empty() {
            identities.insert(style_hint);
        }
    }
    identities.into_iter().collect()
}

/// Whether one declared source address names the served tileset.
///
/// The address is compared segment by segment after its scheme and any query
/// or fragment, so `mapbox://mapbox.mapbox-streets-v8` and a tile template
/// whose path carries the tileset id both match, while an address naming a
/// different tileset does not. Nothing is fetched: the provider binding owns
/// the fetch address, and the style's `url`/`tiles` is identity documentation.
fn declared_address_identifies_tileset(address: &str, served_identities: &[&str]) -> bool {
    let address = address.trim();
    let after_scheme = address
        .split_once("://")
        .map_or(address, |(_, remainder)| remainder);
    let path = after_scheme
        .split_once(['?', '#'])
        .map_or(after_scheme, |(path, _)| path);
    path.split('/').any(|segment| {
        served_identities
            .iter()
            .any(|identity| segment.eq_ignore_ascii_case(identity))
    })
}

/// Verifies that every vector source the style's layers reference identifies
/// the tileset the provider serves.
///
/// A style may declare several sources, and its layers may reference any of
/// them. The vector base fetches exactly one tileset — the provider's — so a
/// source whose declared address names a different tileset would paint a
/// background and fragments while every layer looked well-formed. The install
/// refuses that document instead, naming the declared source(s) and the served
/// tileset.
///
/// A source that declares no `url` and no `tiles` is host-bound: the provider
/// binding supplies its fetch, and it always matches. A declared address
/// matches when any of its path segments equals the descriptor's
/// `source_name`, its tileset half, or one of the provider's declared
/// `style_hints`. The hints are provider declarations, not guesses: the
/// provider states which schema or style family its tileset serves, so a
/// published document that names `base` or `omv` (HERE's vector tileset)
/// identifies the served tileset just as a tileset id does. A source of a
/// kind the vector base cannot draw (raster, GeoJSON or an opaque type) never
/// refuses the install; it is reported as a degraded asset through
/// [`VectorBaseStyleFidelity::degraded_asset_names`].
///
/// # Errors
/// [`VectorStyleArtifactError`] naming the declared source(s) and the served
/// tileset when a referenced vector source names a different tileset.
pub fn verify_declared_vector_sources(
    style: &CompiledStyle,
    descriptor: &VectorTileSourceDescriptor,
) -> Result<(), VectorStyleArtifactError> {
    let served_identities = served_tileset_identities(descriptor);
    let mut mismatched_declarations = Vec::new();
    for source_id in referenced_source_ids(style) {
        let Some(CompiledSource::Vector(vector_source)) = style.source(source_id) else {
            continue;
        };
        let declared_addresses = vector_source
            .url
            .iter()
            .chain(vector_source.tiles.iter())
            .collect::<Vec<_>>();
        if declared_addresses.is_empty() {
            continue;
        }
        let identifies_served_tileset = declared_addresses
            .iter()
            .any(|address| declared_address_identifies_tileset(address, &served_identities));
        if !identifies_served_tileset {
            let addresses = declared_addresses
                .iter()
                .map(|address| format!("{address:?}"))
                .collect::<Vec<_>>()
                .join(", ");
            mismatched_declarations.push(format!("{source_id:?} ({addresses})"));
        }
    }
    if mismatched_declarations.is_empty() {
        return Ok(());
    }
    Err(VectorStyleArtifactError {
        detail: format!(
            "the style document declares the vector source(s) {} but the served tileset is \
             {}; a vector base fetches exactly the tileset its provider serves, so installing \
             this document would paint fragments",
            mismatched_declarations.join(", "),
            descriptor.source_name
        ),
    })
}

/// The degraded-asset names a style produces for a scheduler whose declared
/// sprite, glyph endpoint and non-vector sources did not draw.
fn degraded_asset_names(
    style: &CompiledStyle,
    sprite_drawn: bool,
    glyphs_drawn: bool,
) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(sprite) = style.sprite.as_ref() {
        let any_icon_layer = style
            .layers
            .iter()
            .any(|layer| layer.layout.icon_image.is_some());
        if any_icon_layer && !sprite_drawn {
            names.push(format!("sprite: {}", sprite.url));
        }
    }
    if let Some(glyphs) = style.glyphs.as_ref() {
        let any_text_layer = style
            .layers
            .iter()
            .any(|layer| layer.kind == CompiledLayerKind::Symbol);
        if any_text_layer && !glyphs_drawn {
            names.push(format!("glyphs: {glyphs}"));
        }
    }
    for source_id in referenced_source_ids(style) {
        let Some(source) = style.source(source_id) else {
            continue;
        };
        let source_kind = source.kind();
        if source_kind != CompiledSourceKind::Vector {
            names.push(format!(
                "source {source_id:?} ({}): the vector base cannot draw this source kind",
                source_kind.as_str()
            ));
        }
    }
    names.sort();
    names.dedup();
    names.truncate(MAXIMUM_REPORTED_DEGRADED_ASSET_NAMES);
    names
}

/// The distinct unsupported properties, scope-qualified and sorted, capped at
/// [`MAXIMUM_REPORTED_UNSUPPORTED_PROPERTY_NAMES`].
fn unsupported_property_names(unsupported: &[UnsupportedProperty]) -> Vec<String> {
    let mut names = unsupported
        .iter()
        .map(|property| {
            format!(
                "{}: {}",
                property.scope.description(),
                property.property_name
            )
        })
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names.truncate(MAXIMUM_REPORTED_UNSUPPORTED_PROPERTY_NAMES);
    names
}

/// The frame's per-source layer accounting.
fn source_layer_counts_for_frame(
    style: &CompiledStyle,
    served_source_layers: &BTreeSet<String>,
    contributed_layers: &[bool],
) -> Vec<VectorSourceLayerCount> {
    let mut counts: BTreeMap<&str, (u32, u32)> = BTreeMap::new();
    for (layer_index, layer) in style.layers.iter().enumerate() {
        let Some(source_id) = layer.source.as_deref() else {
            continue;
        };
        let counts_for_source = counts.entry(source_id).or_insert((0, 0));
        if layer.source_kind != Some(CompiledSourceKind::Vector) {
            continue;
        }
        let matched = match layer.source_layer.as_deref() {
            None => true,
            Some(source_layer) => served_source_layers.contains(source_layer),
        };
        if matched {
            counts_for_source.0 = counts_for_source.0.saturating_add(1);
        }
        if contributed_layers
            .get(layer_index)
            .copied()
            .unwrap_or(false)
        {
            counts_for_source.1 = counts_for_source.1.saturating_add(1);
        }
    }
    counts
        .into_iter()
        .map(
            |(source_id, (matched_layer_count, drawn_layer_count))| VectorSourceLayerCount {
                source_id: source_id.to_string(),
                matched_layer_count,
                drawn_layer_count,
            },
        )
        .collect()
}

/// The tile's label candidates, plus one flag per compiled layer telling
/// whether that layer placed at least one of them.
///
/// The candidate list is exactly what [`crate::labels::label_candidates_for_vector_tile`]
/// produces (every compiled layer in order); the flags are the per-layer
/// evidence the frame's source accounting needs.
fn label_candidates_and_contributed_layers(
    tile: &VectorTile,
    tile_address: VectorTileAddress,
    style: &CompiledStyle,
    style_zoom_level: f64,
    transition: Option<(&CompiledStyle, f64)>,
) -> (Vec<MapLabelCandidate>, Vec<bool>) {
    let mut candidates = Vec::new();
    let mut contributed_layers = vec![false; style.layers.len()];
    for (layer_index, layer) in style.layers.iter().enumerate() {
        // A transitioning frame blends each symbol layer from its counterpart
        // in the outgoing style, matched by layer id.
        let layer_transition = transition.map(|(previous_style, elapsed_seconds)| {
            (
                previous_style.layer(&layer.id).unwrap_or(layer),
                style.transition,
                elapsed_seconds,
            )
        });
        let layer_candidates = label_candidates_for_compiled_layer_at_zoom_with_transition(
            tile,
            tile_address,
            layer,
            style_zoom_level,
            layer_transition,
        );
        if !layer_candidates.is_empty() {
            contributed_layers[layer_index] = true;
        }
        candidates.extend(layer_candidates);
    }
    (candidates, contributed_layers)
}

impl VectorBaseScheduler {
    /// Builds a scheduler over the provider and an explicit style definition.
    ///
    /// `style_id` names the style in status readbacks and in the styled cache's
    /// revision; the scheduler itself styles with `style`.
    pub fn new(
        provider: Arc<dyn VectorTileProvider>,
        style_id: impl Into<String>,
        style: CompiledStyle,
    ) -> Self {
        let bound_base_source = bound_base_vector_source(&provider, &style);
        Self {
            provider,
            style_id: style_id.into(),
            style,
            bound_base_source,
            sprite_source: None,
            sprite_atlas: None,
            label_font: None,
            style_revision: 0,
            previous_style: None,
            transition_started_at_seconds: None,
            decoded_tiles: Arc::new(Mutex::new(DecodedTileCache::new(
                DEFAULT_VECTOR_BASE_MAXIMUM_CACHED_TILE_COUNT,
            ))),
            styled_tiles: HashMap::new(),
            styled_tile_insertion_order: VecDeque::new(),
            maximum_cached_styled_tile_count: DEFAULT_VECTOR_BASE_MAXIMUM_CACHED_STYLED_TILE_COUNT,
            payload_cache: None,
            payload_cache_time_to_live_seconds: DEFAULT_VECTOR_TILE_PAYLOAD_TIME_TO_LIVE_SECONDS,
            payload_cache_counters: Arc::new(VectorBasePayloadCacheCounters::default()),
            maximum_prefetch_ring_tile_count: DEFAULT_VECTOR_BASE_PREFETCH_RING_TILE_BUDGET,
            visible_prefetch_burst_tile_count:
                DEFAULT_VECTOR_BASE_VISIBLE_PREFETCH_BURST_TILE_COUNT,
            ring_prefetch_minimum_zoom_level: DEFAULT_VECTOR_BASE_RING_PREFETCH_MINIMUM_ZOOM_LEVEL,
            latest_visible_tile_keys: Arc::new(Mutex::new(Vec::new())),
            last_frame: None,
            ring_prefetch_minimum_interval: Duration::from_millis(
                DEFAULT_VECTOR_BASE_RING_PREFETCH_MINIMUM_INTERVAL_MILLISECONDS,
            ),
            last_ring_prefetch_at: Mutex::new(None),
            installed_at: Instant::now(),
            first_drawn_tile_milliseconds: None,
            last_failure_detail: None,
            settled_style_zoom_bucket: None,
            zoom_transition: ZoomTransition::new(),
            retained_level: None,
            last_level_frame: None,
        }
    }

    /// Builds a scheduler over the provider and one of the built-in style
    /// artifacts, named by its style id (`bright-light`, `fiord-dark`, …).
    ///
    /// # Parameters
    /// - `provider` — the vector tile source the scheduler fetches from.
    /// - `style_id` — one of the exported `VECTOR_STYLE_ID_*` identifiers.
    ///
    /// # Errors
    /// An unknown id reports the artifact error rather than a guess.
    pub fn from_style_id(
        provider: Arc<dyn VectorTileProvider>,
        style_id: &str,
    ) -> Result<Self, VectorStyleArtifactError> {
        let style = vector_tile_style_from_id(style_id)?;
        Ok(Self::new(provider, style_id, style))
    }

    /// Builds a scheduler over the provider and a version-8 style document a
    /// provider publishes, parsing and compiling it here.
    ///
    /// A provider's own document names the layers, filters, glyphs and
    /// sprites its tiles were designed for, so a base painted with it matches
    /// the tiles by construction.
    ///
    /// # Parameters
    /// - `provider` — the vector tile source the scheduler fetches from.
    /// - `style_id` — the label the base status reports for this style.
    /// - `style_document_json` — the document as the provider served it.
    ///
    /// # Errors
    /// A malformed document reports the artifact error rather than falling
    /// back to another style, and a document whose referenced vector sources
    /// name a tileset the provider does not serve reports a refusal naming the
    /// declared source(s) and the served tileset.
    pub fn from_style_document_json(
        provider: Arc<dyn VectorTileProvider>,
        style_id: &str,
        style_document_json: &str,
    ) -> Result<Self, VectorStyleArtifactError> {
        let style = vector_tile_style_from_json(style_document_json)?;
        verify_declared_vector_sources(&style, &provider.source_descriptor())?;
        Ok(Self::new(provider, style_id, style))
    }

    /// Routes every fetched vector-tile payload through a response store
    /// before decoding it, and stores each fresh payload for
    /// `time_to_live_seconds`. A lookup hit skips the network entirely, so a
    /// region browsed once is served from the store on a later scheduler.
    ///
    /// The store is keyed by the tileset's declared source name and the tile
    /// address; a failing store degrades to network-only operation rather
    /// than failing a frame.
    ///
    /// # Parameters
    /// - `payload_cache` — the store the fetches consult and back-fill.
    /// - `time_to_live_seconds` — how long a stored payload stays valid.
    pub fn with_payload_cache(
        mut self,
        payload_cache: Arc<dyn CachedResponseStore<VectorTileData>>,
        time_to_live_seconds: u64,
    ) -> Self {
        self.payload_cache = Some(payload_cache);
        self.payload_cache_time_to_live_seconds = time_to_live_seconds;
        self
    }

    /// Injects the sprite loader that resolves a style's `sprite` URL.
    ///
    /// The loader receives the URL the style document declares and returns
    /// the decoded index and sheet. A test binds the URL to committed
    /// fixtures; an application to its own transport. Without a source, an
    /// icon layer's text still draws and its icons are skipped.
    pub fn with_sprite_source(mut self, sprite_source: Arc<dyn SpriteSource>) -> Self {
        self.sprite_source = Some(sprite_source);
        self.sprite_atlas = None;
        self
    }

    /// Injects the atlas an install already fetched for the style's sprite.
    ///
    /// The renderer's sprite seam is synchronous, so a host that fetches the
    /// index and sheet through an authenticated client decodes them once and
    /// hands the atlas here, labelled with the URL the style declares. A
    /// style replaced on the scheduler drops the atlas, because the next
    /// style names its own sprite. Without an atlas, an icon layer's text
    /// still draws and its icons are skipped.
    pub fn with_prefetched_sprite_atlas(
        mut self,
        sprite_url: impl Into<String>,
        sprite_atlas: Arc<SpriteAtlas>,
    ) -> Self {
        self.sprite_atlas = Some((sprite_url.into(), Some(sprite_atlas)));
        self
    }

    /// Injects the glyph font the style's own place names draw with.
    ///
    /// The host fetches the font from the style document's glyph endpoint
    /// through the provider's authenticated style-resource client and hands
    /// it here; the renderer never fetches. Without a font, the labels draw
    /// through the SDK's embedded Latin range ([`default_label_font`]).
    pub fn with_label_font(mut self, label_font: Arc<LabelFont>) -> Self {
        self.label_font = Some(label_font);
        self
    }

    /// The glyph font the style's own place names draw with, when the host
    /// supplied one.
    pub fn label_font(&self) -> Option<&Arc<LabelFont>> {
        self.label_font.as_ref()
    }

    /// The sprite atlas the current style resolves to, when the scheduler
    /// holds one: a source loaded it or an install prefetched it for the URL
    /// the style names. `None` without either, and after a style replaced the
    /// one the held atlas belongs to.
    pub fn sprite_atlas(&self) -> Option<Arc<SpriteAtlas>> {
        let sprite_url = &self.style.sprite.as_ref()?.url;
        let (cached_url, cached_atlas) = self.sprite_atlas.as_ref()?;
        (cached_url == sprite_url)
            .then(|| cached_atlas.clone())
            .flatten()
    }

    /// Resolves the active style's sprite atlas through the injected source,
    /// once per URL. A style without a sprite, a missing source and a failed
    /// load all yield `None`; a failed load is remembered so the next frame
    /// does not retry it.
    fn resolved_sprite_atlas(&mut self) -> Option<Arc<SpriteAtlas>> {
        let sprite_url = self.style.sprite.as_ref()?.url.clone();
        if let Some((cached_url, cached_atlas)) = &self.sprite_atlas
            && *cached_url == sprite_url
        {
            return cached_atlas.clone();
        }
        let loaded_atlas = self
            .sprite_source
            .as_ref()
            .and_then(|source| source.load_sprite(&sprite_url).ok())
            .map(Arc::new);
        self.sprite_atlas = Some((sprite_url, loaded_atlas.clone()));
        loaded_atlas
    }

    /// Overrides the ring prefetch's per-plan tile budget (the test seam and
    /// the host's tuning knob).
    pub fn with_maximum_prefetch_ring_tile_count(mut self, maximum_tile_count: usize) -> Self {
        self.maximum_prefetch_ring_tile_count = maximum_tile_count;
        self
    }

    /// Sets the concurrent-fetch burst the visible prefetch starts (the host
    /// installs its provider's declared token-bucket capacity here). The
    /// claims still cover the whole visible set; only their start is bounded,
    /// so one slow tile never delays the burst already in flight.
    pub fn with_visible_prefetch_burst_tile_count(mut self, burst_tile_count: usize) -> Self {
        self.visible_prefetch_burst_tile_count = burst_tile_count.max(1);
        self
    }

    /// Overrides the lowest served zoom at which the ring prefetch plans ahead
    /// (the test seam; the host keeps the default).
    pub fn with_ring_prefetch_minimum_zoom_level(mut self, minimum_zoom_level: u8) -> Self {
        self.ring_prefetch_minimum_zoom_level = minimum_zoom_level;
        self
    }

    /// Overrides the minimum interval between two ring prefetch plans (the
    /// test seam; the host keeps the default).
    pub fn with_ring_prefetch_minimum_interval(mut self, minimum_interval: Duration) -> Self {
        self.ring_prefetch_minimum_interval = minimum_interval;
        self
    }

    /// The active style's identifier.
    pub fn style_id(&self) -> &str {
        &self.style_id
    }

    /// The active style's compiled render plans.
    pub fn style(&self) -> &CompiledStyle {
        &self.style
    }

    /// The style's fidelity diagnostics: the unsupported-property count with
    /// its bounded, sorted name list, and the assets that fell back to a
    /// degraded draw.
    ///
    /// The names are computed from the installed style and the assets the
    /// install actually resolved, so a status readback states what the base
    /// cannot paint instead of leaving it silent.
    pub fn style_fidelity(&self) -> VectorBaseStyleFidelity {
        let sprite_drawn = self
            .sprite_atlas
            .as_ref()
            .is_some_and(|(_, atlas)| atlas.is_some());
        let glyphs_drawn = self.label_font.is_some();
        VectorBaseStyleFidelity {
            unsupported_property_count: u32::try_from(self.style.unsupported.len())
                .unwrap_or(u32::MAX),
            unsupported_property_names: unsupported_property_names(&self.style.unsupported),
            degraded_asset_names: degraded_asset_names(&self.style, sprite_drawn, glyphs_drawn),
        }
    }

    /// The tileset's descriptor (cache namespace, attribution, zoom span).
    pub fn source_descriptor(&self) -> VectorTileSourceDescriptor {
        self.provider.source_descriptor()
    }

    /// The attribution line this base requires wherever it is shown: the
    /// style's own source declaration when it carries one, and otherwise the
    /// tileset's descriptor.
    pub fn attribution_text(&self) -> String {
        match self.bound_base_source.as_ref() {
            Some(bound_base_source) => bound_base_source.attribution_text().to_string(),
            None => self.provider.source_descriptor().attribution_text,
        }
    }

    /// The zoom level the tileset actually serves for a requested zoom level:
    /// the request clamped into the descriptor's declared span, so a camera
    /// beyond either edge keeps a base map at the nearest served resolution.
    /// A descriptor declaring an inverted span clamps to its lower edge rather
    /// than panicking (the providers refuse such a declaration at
    /// construction; a hand-written implementor might not).
    pub fn served_zoom_level(&self, requested_zoom_level: u8) -> u8 {
        if let Some(bound_base_source) = self.bound_base_source.as_ref() {
            return bound_base_source.served_zoom_level(requested_zoom_level);
        }
        let descriptor = self.provider.source_descriptor();
        let lowest_served_zoom_level = descriptor.minimum_zoom_level;
        let highest_served_zoom_level = descriptor.maximum_zoom_level.max(lowest_served_zoom_level);
        requested_zoom_level
            .max(lowest_served_zoom_level)
            .min(highest_served_zoom_level)
    }

    /// The highest zoom level this base serves: the style's own declaration
    /// when its source carries one, and otherwise the tileset's descriptor.
    pub fn served_maximum_zoom_level(&self) -> u8 {
        match self.bound_base_source.as_ref() {
            Some(bound_base_source) => bound_base_source.policy.maximum_zoom_level,
            None => self.provider.source_descriptor().maximum_zoom_level,
        }
    }

    /// The last resolved frame's last tile failure detail, if that frame had
    /// one. Cleared by a frame whose tiles all resolved.
    pub fn last_failure_detail(&self) -> Option<&str> {
        self.last_failure_detail.as_deref()
    }

    /// Replaces the style with the built-in artifact a style id names, keeping
    /// every decoded tile: the next frame restyles the cached tiles under the
    /// new style without fetching them again. An unknown id reports the
    /// artifact error and keeps the current style in force.
    pub fn set_style(&mut self, style_id: &str) -> Result<(), VectorStyleArtifactError> {
        let style = vector_tile_style_from_id(style_id)?;
        self.set_compiled_style(style_id, style);
        Ok(())
    }

    /// Replaces the style with an already-compiled plan, keeping every decoded
    /// tile. This is the seam a host uses for a style document it parsed
    /// itself, and the one [`Self::set_style`] calls.
    ///
    /// When the incoming style declares a transition (`transition` at the
    /// document root or a `*-transition` paint block), the outgoing style is
    /// kept and the next timed frame starts blending from it; otherwise the
    /// change applies immediately. A transition is driven by the frame clock
    /// passed to [`Self::cached_frame_at_time`], never by a timer.
    ///
    /// # Parameters
    /// - `style_id` — the identifier status readbacks report.
    /// - `style` — the compiled render plans to install.
    pub fn set_compiled_style(&mut self, style_id: impl Into<String>, style: CompiledStyle) {
        let transition_lifetime_seconds = style.transition_lifetime_seconds();
        let outgoing_style = std::mem::replace(&mut self.style, style);
        self.style_id = style_id.into();
        self.style_revision += 1;
        // A different style never reuses the previous style's geometry.
        self.styled_tiles.clear();
        self.styled_tile_insertion_order.clear();
        // A different style may name a different sprite, so reload it.
        self.sprite_atlas = None;
        // A different style may name a different glyph endpoint, so the
        // previous style's font is dropped and the labels draw through the
        // embedded range until the host supplies the new style's font.
        self.label_font = None;
        // A style with no declared timing applies immediately; keeping the
        // outgoing plan would only hold values that never blend.
        self.previous_style = (transition_lifetime_seconds > 0.0).then_some(outgoing_style);
        self.transition_started_at_seconds = None;
        // The replacement style declares its own source, so its zoom span and
        // attribution apply from the next frame on.
        self.bound_base_source = bound_base_vector_source(&self.provider, &self.style);
    }

    /// Whether a style transition is still blending and therefore needs
    /// further timed frames.
    pub fn style_transition_running(&self) -> bool {
        self.previous_style.is_some()
    }

    /// Resolves the base map for the given visible tile addresses and camera
    /// zoom level: every missing tile is fetched and decoded once — the
    /// misses are fetched concurrently, so the frame waits for the slowest
    /// request rather than the sum of all of them — every tile is styled at
    /// the camera zoom's bucket (from the decoded cache when the bucket or
    /// the style changed), and the frame's overlay primitives are returned in
    /// tile-address order. A tile that fails to fetch or decode is skipped,
    /// counted, and covered by its nearest cached ancestor when one exists;
    /// the frame never fails as a whole.
    ///
    /// This is the blocking reference path (tests and captures that want a
    /// settled frame). A presenting surface instead resolves with
    /// [`Self::cached_frame`] and lets the returned detached prefetches fill
    /// the cache.
    pub async fn resolve_frame(
        &mut self,
        visible_tile_keys: &[TileKey],
        style_zoom_level: f64,
    ) -> VectorBaseFrame {
        self.finish_style_transition();
        if let Some(prefetch) = self.prefetch_missing_tiles(visible_tile_keys) {
            prefetch.await;
        }
        self.cached_frame(visible_tile_keys, style_zoom_level)
    }

    /// [`Self::resolve_frame`] with the frame's monotonic clock, in seconds,
    /// so a style change blends over its declared transition instead of
    /// applying immediately.
    ///
    /// # Parameters
    /// - `visible_tile_keys` — the camera's visible tile addresses.
    /// - `style_zoom_level` — the camera's fractional zoom for paint
    ///   evaluation.
    /// - `frame_time_seconds` — a monotonic clock reading; only differences
    ///   against the first timed frame after a style change matter, so any
    ///   stable origin is valid.
    pub async fn resolve_frame_at_time(
        &mut self,
        visible_tile_keys: &[TileKey],
        style_zoom_level: f64,
        frame_time_seconds: f64,
    ) -> VectorBaseFrame {
        if let Some(prefetch) = self.prefetch_missing_tiles(visible_tile_keys) {
            prefetch.await;
        }
        self.cached_frame_at_time(visible_tile_keys, style_zoom_level, frame_time_seconds)
    }

    /// Claims every visible tile the cache does not hold and answers the
    /// prefetch to spawn detached: it fetches (through the payload cache)
    /// the claimed tiles concurrently, decodes them and writes the results
    /// into the shared cache, returning how many tiles it delivered. `None`
    /// when every visible tile is cached or already being fetched, so a
    /// repaint never starts a duplicate request. The caller presents a
    /// [`Self::cached_frame`] immediately and lets this run.
    ///
    /// The future is cancellable by dropping it: its claims are released, so
    /// no address stays in flight forever.
    pub fn prefetch_missing_tiles(
        &self,
        visible_tile_keys: &[TileKey],
    ) -> Option<impl std::future::Future<Output = usize> + Send + 'static> {
        self.remember_visible_tiles(visible_tile_keys);
        self.prefetch_tile_keys(visible_tile_keys.to_vec(), None, false, None)
    }

    /// [`Self::prefetch_missing_tiles`] with per-tile delivery: the callback
    /// runs as soon as each visible tile has been fetched, decoded and written
    /// into the shared cache, so a presenting surface repaints on the first
    /// arrival instead of waiting for a `join_all` over the whole set. The
    /// returned future still resolves with the delivered count once every
    /// claim has finished, so a caller that does not need per-tile delivery
    /// keeps the original entry point.
    ///
    /// The claims cover the whole visible set; only their start is bounded by
    /// [`Self::with_visible_prefetch_burst_tile_count`]. The callback runs
    /// outside the cache lock and may schedule a frame; a surface that draws
    /// immediately coalesces the arrivals itself (several callbacks may run
    /// before its next frame).
    pub fn prefetch_missing_tiles_with_delivery<Delivery>(
        &self,
        visible_tile_keys: &[TileKey],
        on_tile_delivered: Delivery,
    ) -> Option<impl std::future::Future<Output = usize> + Send + 'static>
    where
        Delivery: Fn(TileKey) + Send + Sync + 'static,
    {
        self.remember_visible_tiles(visible_tile_keys);
        self.prefetch_tile_keys(
            visible_tile_keys.to_vec(),
            None,
            true,
            Some(Arc::new(on_tile_delivered)),
        )
    }

    /// Stores the latest visible set for the ring plan's staleness check.
    fn remember_visible_tiles(&self, visible_tile_keys: &[TileKey]) {
        let mut latest_visible_tile_keys = self
            .latest_visible_tile_keys
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        latest_visible_tile_keys.clear();
        latest_visible_tile_keys.extend(visible_tile_keys.iter().copied());
        latest_visible_tile_keys.sort();
        latest_visible_tile_keys.dedup();
    }

    /// Claims the ring around the viewport for a detached prefetch: the eight
    /// neighbors of every visible tile at the served zoom, plus each visible
    /// tile's coarse parent (the fallback that covers the area while the
    /// detailed tile is in flight), under
    /// [`Self::with_maximum_prefetch_ring_tile_count`]'s budget.
    ///
    /// Parents come first because they can cover a missing child immediately;
    /// the rest are ordered by distance from the viewport's center, so a
    /// budgeted plan grows outward instead of claiming a scattered corner.
    /// Nothing already cached, failed or in flight is claimed. `None` when
    /// the ring has nothing left to claim, when a visible tile's own fetch is
    /// still running (the view comes first), or when the previous plan ran
    /// inside the configured interval.
    pub fn prefetch_ring_tiles(
        &self,
        visible_tile_keys: &[TileKey],
    ) -> Option<impl std::future::Future<Output = usize> + Send + 'static> {
        let visible_tiles_are_fetching = {
            let decoded_tiles = self
                .decoded_tiles
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            visible_tile_keys
                .iter()
                .any(|tile_key| decoded_tiles.in_flight.contains(tile_key))
        };
        if visible_tiles_are_fetching {
            return None;
        }
        self.remember_visible_tiles(visible_tile_keys);
        // Below the low-zoom threshold one screen holds most of the pyramid:
        // a ring is a large fraction of the visible set and predicts the next
        // camera far worse than at city zoom, so a fit stays visible-only.
        let served_zoom_level = visible_tile_keys
            .iter()
            .map(|tile_key| tile_key.zoom_level)
            .max()
            .unwrap_or(0);
        if served_zoom_level < self.ring_prefetch_minimum_zoom_level {
            return None;
        }
        let candidates = self.ring_prefetch_candidates(visible_tile_keys);
        if candidates.is_empty() {
            return None;
        }
        {
            let mut last_ring_prefetch_at = self
                .last_ring_prefetch_at
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let now = Instant::now();
            if last_ring_prefetch_at.is_some_and(|last_plan_at| {
                now.duration_since(last_plan_at) < self.ring_prefetch_minimum_interval
            }) {
                return None;
            }
            *last_ring_prefetch_at = Some(now);
        }
        self.prefetch_tile_keys(
            candidates,
            Some(self.maximum_prefetch_ring_tile_count),
            true,
            None,
        )
    }

    /// The ring plan: coarse parents first, then same-zoom neighbors ordered
    /// by distance from the visible set's center. The plan is deduplicated
    /// against the visible set; the per-plan budget applies to the tiles the
    /// caches cannot already serve, so budgeted plans are not wasted on
    /// cached neighbors.
    fn ring_prefetch_candidates(&self, visible_tile_keys: &[TileKey]) -> Vec<TileKey> {
        let mut visible_tiles = visible_tile_keys.to_vec();
        visible_tiles.sort();
        visible_tiles.dedup();
        if visible_tiles.is_empty() {
            return Vec::new();
        }
        let served_zoom_level = visible_tiles
            .iter()
            .map(|tile_key| tile_key.zoom_level)
            .max()
            .unwrap_or(0);
        let tiles_per_axis = 1u64
            .checked_shl(u32::from(served_zoom_level))
            .unwrap_or(u64::MAX);
        let visible_tile_set = visible_tiles.iter().copied().collect::<HashSet<_>>();
        let center_tile_x = visible_tiles
            .iter()
            .map(|tile_key| f64::from(tile_key.tile_x))
            .sum::<f64>()
            / visible_tiles.len() as f64;
        let center_tile_y = visible_tiles
            .iter()
            .map(|tile_key| f64::from(tile_key.tile_y))
            .sum::<f64>()
            / visible_tiles.len() as f64;
        let distance_from_center = |tile_key: &TileKey| {
            let horizontal_distance = f64::from(tile_key.tile_x) - center_tile_x;
            let vertical_distance = f64::from(tile_key.tile_y) - center_tile_y;
            horizontal_distance * horizontal_distance + vertical_distance * vertical_distance
        };
        let mut parent_tile_keys = visible_tiles
            .iter()
            .filter_map(|tile_key| tile_key.parent_tile_key())
            .filter(|tile_key| !visible_tile_set.contains(tile_key))
            .collect::<Vec<_>>();
        let mut ring_tile_keys = Vec::new();
        for visible_tile in &visible_tiles {
            for (horizontal_offset, vertical_offset) in VECTOR_BASE_RING_NEIGHBOR_OFFSETS {
                let neighbor_tile_x = i64::from(visible_tile.tile_x) + i64::from(horizontal_offset);
                let neighbor_tile_y = i64::from(visible_tile.tile_y) + i64::from(vertical_offset);
                if neighbor_tile_x < 0 || neighbor_tile_y < 0 {
                    continue;
                }
                if neighbor_tile_x as u64 >= tiles_per_axis
                    || neighbor_tile_y as u64 >= tiles_per_axis
                {
                    continue;
                }
                let Ok(neighbor_tile_key) = TileKey::new(
                    neighbor_tile_x as u32,
                    neighbor_tile_y as u32,
                    served_zoom_level,
                ) else {
                    continue;
                };
                if !visible_tile_set.contains(&neighbor_tile_key) {
                    ring_tile_keys.push(neighbor_tile_key);
                }
            }
        }
        parent_tile_keys.sort_by(|first, second| {
            distance_from_center(first)
                .total_cmp(&distance_from_center(second))
                .then_with(|| first.cmp(second))
        });
        parent_tile_keys.dedup();
        ring_tile_keys.sort_by(|first, second| {
            distance_from_center(first)
                .total_cmp(&distance_from_center(second))
                .then_with(|| first.cmp(second))
        });
        let mut candidates = parent_tile_keys;
        candidates.extend(ring_tile_keys);
        let mut seen_tile_keys = HashSet::new();
        candidates
            .into_iter()
            .filter(|tile_key| seen_tile_keys.insert(*tile_key))
            .collect()
    }

    /// Claims the cache misses among `candidate_tile_keys`, up to
    /// `maximum_claim_count` when one is given, and answers the detached fetch
    /// future. Shared by the visible and ring prefetch plans: both
    /// deduplicate and claim under one short lock, so neither duplicates
    /// another plan's request. The candidate order is the claim priority, and
    /// the budget is applied after the cached and in-flight addresses are
    /// skipped.
    ///
    /// Fetches start in bursts of [`Self::with_visible_prefetch_burst_tile_count`]
    /// and each completion is written into the shared cache (and handed to
    /// `delivery`) immediately, so no `join_all` gates the first arrival. A
    /// `drop_stale` plan (the ring) discards a delivery the latest visible set
    /// no longer touches instead of inserting a tile the camera has left.
    fn prefetch_tile_keys(
        &self,
        candidate_tile_keys: Vec<TileKey>,
        maximum_claim_count: Option<usize>,
        drop_stale: bool,
        delivery: Option<Arc<dyn Fn(TileKey) + Send + Sync>>,
    ) -> Option<impl std::future::Future<Output = usize> + Send + 'static> {
        let claimed_tile_keys = {
            let mut decoded_tiles = self
                .decoded_tiles
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let mut seen_tile_keys = HashSet::new();
            candidate_tile_keys
                .into_iter()
                .filter(|tile_key| seen_tile_keys.insert(*tile_key))
                .filter(|tile_key| decoded_tiles.begin_fetch(*tile_key))
                .take(maximum_claim_count.unwrap_or(usize::MAX))
                .collect::<Vec<TileKey>>()
        };
        if claimed_tile_keys.is_empty() {
            return None;
        }
        let provider = Arc::clone(&self.provider);
        let decoded_tiles = Arc::clone(&self.decoded_tiles);
        let payload_cache = self.payload_cache.clone();
        let payload_cache_source_name = self.provider.source_descriptor().source_name;
        let payload_cache_time_to_live_seconds = self.payload_cache_time_to_live_seconds;
        let payload_cache_counters = Arc::clone(&self.payload_cache_counters);
        let latest_visible_tile_keys = Arc::clone(&self.latest_visible_tile_keys);
        let fetch_burst = self.visible_prefetch_burst_tile_count.max(1);
        Some(async move {
            let mut claim_guard =
                PrefetchClaimGuard::new(Arc::clone(&decoded_tiles), claimed_tile_keys.clone());
            let make_fetch = |tile_key: TileKey| {
                let provider = Arc::clone(&provider);
                let payload_cache = payload_cache.clone();
                let payload_cache_source_name = payload_cache_source_name.clone();
                let payload_cache_counters = Arc::clone(&payload_cache_counters);
                async move {
                    (
                        tile_key,
                        Self::fetch_and_decode_vector_tile(
                            provider.as_ref(),
                            payload_cache.as_deref(),
                            &payload_cache_source_name,
                            payload_cache_time_to_live_seconds,
                            &payload_cache_counters,
                            tile_key,
                        )
                        .await,
                    )
                }
            };
            let mut fetches = FuturesUnordered::new();
            let mut next_claim_index = 0;
            while next_claim_index < claimed_tile_keys.len() && fetches.len() < fetch_burst {
                fetches.push(make_fetch(claimed_tile_keys[next_claim_index]));
                next_claim_index += 1;
            }
            let mut delivered_tile_count = 0;
            while let Some((tile_key, fetched_tile)) = fetches.next().await {
                if next_claim_index < claimed_tile_keys.len() {
                    fetches.push(make_fetch(claimed_tile_keys[next_claim_index]));
                    next_claim_index += 1;
                }
                // Stale ring work is dropped, not inserted: the camera has
                // panned away and the tile would only occupy the cache.
                let is_stale = drop_stale && {
                    let latest_visible_tile_keys = latest_visible_tile_keys
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner);
                    !ring_tile_is_relevant(&tile_key, &latest_visible_tile_keys)
                };
                if is_stale {
                    decoded_tiles
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .discard_fetch(tile_key);
                    claim_guard.mark_finished(tile_key);
                    continue;
                }
                let delivered = fetched_tile.is_ok();
                decoded_tiles
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .finish_fetch(tile_key, fetched_tile);
                claim_guard.mark_finished(tile_key);
                if delivered {
                    delivered_tile_count += 1;
                    if let Some(delivery) = &delivery {
                        delivery(tile_key);
                    }
                }
            }
            delivered_tile_count
        })
    }

    /// Resolves one frame from the decoded cache alone — never the network:
    /// every cached tile is styled at the camera zoom's bucket (from the
    /// styled cache when the bucket or the style is unchanged) and its labels
    /// read, an address whose prefetch is running counts as pending, and a
    /// recorded failure counts as failed with its detail. A missing address
    /// whose ancestor is cached draws that ancestor's geometry once for the
    /// whole area it covers (the raster path's parent fallback), so the frame
    /// paints coarse detail instead of a hole.
    ///
    /// This is what a surface draws while a detached
    /// [`Self::prefetch_missing_tiles`] fills the cache, so a first frame over
    /// unvisited ground presents the style's background or the cached coarse
    /// detail instead of blocking on every tile's latency.
    pub fn cached_frame(
        &mut self,
        visible_tile_keys: &[TileKey],
        style_zoom_level: f64,
    ) -> VectorBaseFrame {
        // The untimed path applies a style change immediately: no transition
        // is started and any pending one is dropped, so its output is exactly
        // the settled style's.
        self.finish_style_transition();
        let tile_zoom_level = resolved_tile_zoom_level(
            visible_tile_keys,
            self.zoom_transition.observed_tile_zoom_level(),
        );
        (*self.cached_frame_content(
            visible_tile_keys,
            style_zoom_level,
            None,
            false,
            tile_zoom_level,
            None,
        ))
        .clone()
    }

    /// [`Self::cached_frame`] behind a shared [`Arc`]: an unchanged frame is
    /// the same allocation the previous call answered, so a presenting
    /// surface can compare [`VectorBaseFrame::content_revision`] and reuse the
    /// frame without cloning its overlay list. Cloning the returned value is
    /// one reference-count increment.
    pub fn cached_frame_shared(
        &mut self,
        visible_tile_keys: &[TileKey],
        style_zoom_level: f64,
    ) -> Arc<VectorBaseFrame> {
        self.finish_style_transition();
        let tile_zoom_level = resolved_tile_zoom_level(
            visible_tile_keys,
            self.zoom_transition.observed_tile_zoom_level(),
        );
        self.cached_frame_content(
            visible_tile_keys,
            style_zoom_level,
            None,
            false,
            tile_zoom_level,
            None,
        )
    }

    /// [`Self::cached_frame`] with the frame's monotonic clock, in seconds, so
    /// a style change blends from the previous style over its declared
    /// transition. The first timed frame after a change starts the clock; the
    /// same frame clock keeps the blend advancing until the transition's
    /// lifetime elapses, after which the previous style is dropped.
    ///
    /// # Parameters
    /// - `visible_tile_keys` — the camera's visible tile addresses.
    /// - `style_zoom_level` — the camera's fractional zoom for paint
    ///   evaluation.
    /// - `frame_time_seconds` — a monotonic clock reading. A non-finite value
    ///   applies the change immediately rather than freezing a blend.
    pub fn cached_frame_at_time(
        &mut self,
        visible_tile_keys: &[TileKey],
        style_zoom_level: f64,
        frame_time_seconds: f64,
    ) -> VectorBaseFrame {
        self.cached_frame_at_time_in_motion(
            visible_tile_keys,
            style_zoom_level,
            frame_time_seconds,
            false,
        )
    }

    /// [`Self::cached_frame_at_time`] with the frame graded by camera motion.
    ///
    /// While `motion_graded` is true the camera is mid-gesture (a pan, zoom or
    /// rotate by input), so the frame is assembled entirely from what the
    /// caches already hold: decoded tiles, styled geometry at the last settled
    /// style-zoom bucket, and no label candidates. Styling a tile the camera
    /// has newly revealed still happens once, because there is no earlier
    /// geometry to reuse; what is skipped is the repeated assembly — the
    /// per-bucket restyle a continuous zoom would otherwise pay every frame
    /// and the per-tile label extraction plus placement the settled frame
    /// resolves. A style transition does not blend on a graded frame for the
    /// same reason; it resumes (or completes) at the settle.
    ///
    /// The next call with `motion_graded` false is the settle: it resolves at
    /// the camera's own style bucket and carries the frame's labels, exactly
    /// as a direct [`Self::cached_frame_at_time`] at that pose would.
    ///
    /// # Parameters
    /// - `visible_tile_keys` — the camera's visible tile addresses.
    /// - `style_zoom_level` — the camera's fractional zoom for paint
    ///   evaluation.
    /// - `frame_time_seconds` — a monotonic clock reading.
    /// - `motion_graded` — true while a camera gesture is active.
    pub fn cached_frame_at_time_in_motion(
        &mut self,
        visible_tile_keys: &[TileKey],
        style_zoom_level: f64,
        frame_time_seconds: f64,
        motion_graded: bool,
    ) -> VectorBaseFrame {
        (*self.cached_frame_at_time_in_motion_shared(
            visible_tile_keys,
            style_zoom_level,
            frame_time_seconds,
            motion_graded,
        ))
        .clone()
    }

    /// [`Self::cached_frame_at_time_in_motion`] behind a shared [`Arc`]; see
    /// [`Self::cached_frame_shared`] for the sharing rule.
    pub fn cached_frame_at_time_in_motion_shared(
        &mut self,
        visible_tile_keys: &[TileKey],
        style_zoom_level: f64,
        frame_time_seconds: f64,
        motion_graded: bool,
    ) -> Arc<VectorBaseFrame> {
        let transition = self.active_style_transition(frame_time_seconds);
        let tile_zoom_level = resolved_tile_zoom_level(
            visible_tile_keys,
            self.zoom_transition.observed_tile_zoom_level(),
        );
        self.cached_frame_content(
            visible_tile_keys,
            style_zoom_level,
            transition.as_ref(),
            motion_graded,
            tile_zoom_level,
            Some(frame_time_seconds),
        )
    }

    /// The transition a timed frame should blend with, or `None` when none is
    /// running. Starts the clock on the first timed frame after a change and
    /// drops the previous style once the change's lifetime has elapsed.
    fn active_style_transition(&mut self, frame_time_seconds: f64) -> Option<(CompiledStyle, f64)> {
        self.previous_style.as_ref()?;
        if !frame_time_seconds.is_finite() {
            self.finish_style_transition();
            return None;
        }
        let started_at_seconds = *self
            .transition_started_at_seconds
            .get_or_insert(frame_time_seconds);
        let elapsed_seconds = (frame_time_seconds - started_at_seconds).max(0.0);
        if elapsed_seconds >= self.style.transition_lifetime_seconds() {
            self.finish_style_transition();
            return None;
        }
        self.previous_style
            .clone()
            .map(|previous_style| (previous_style, elapsed_seconds))
    }

    /// Drops any in-flight style transition so the next frame draws the new
    /// style's values immediately.
    fn finish_style_transition(&mut self) {
        self.previous_style = None;
        self.transition_started_at_seconds = None;
    }

    /// The shared frame-resolution body. `transition` carries the previous
    /// style and the elapsed seconds while a change blends; `None` styles
    /// from the current style alone and uses the styled-geometry cache.
    /// `motion_graded` freezes the style-zoom bucket at the last settled
    /// frame's, drops any transition blend and skips label assembly.
    /// `tile_zoom_level` is the level this frame's addresses belong to, and
    /// `frame_time_seconds` the frame's monotonic clock when it has one.
    fn cached_frame_content(
        &mut self,
        visible_tile_keys: &[TileKey],
        style_zoom_level: f64,
        transition: Option<&(CompiledStyle, f64)>,
        motion_graded: bool,
        tile_zoom_level: u8,
        frame_time_seconds: Option<f64>,
    ) -> Arc<VectorBaseFrame> {
        // A level change retires what the last frame drew: capture it before
        // this frame's geometry replaces it, so the retired level can stay on
        // screen while the new one fills. It is never fetched again.
        if self
            .zoom_transition
            .observed_tile_zoom_level()
            .is_some_and(|observed_tile_zoom_level| observed_tile_zoom_level != tile_zoom_level)
        {
            self.retained_level = self.last_level_frame.take();
        }
        let mut requested_tile_keys = visible_tile_keys.to_vec();
        requested_tile_keys.sort();
        requested_tile_keys.dedup();
        let requested_style_zoom = quantized_style_zoom(style_zoom_level);
        // A graded frame paints from the geometry the last settled frame
        // resolved, so a continuous zoom gesture does not restyle every tile
        // once per style bucket. A style transition cannot reuse cached
        // geometry (its paint changes every frame), so a graded frame paints
        // the current style's cache and the blend resumes at the settle.
        let quantized_style_zoom = if motion_graded {
            // Freeze on the last settled bucket; a gesture that began before
            // any frame settled freezes on this frame's own bucket instead,
            // so the following graded frames still reuse one style pass.
            *self
                .settled_style_zoom_bucket
                .get_or_insert(requested_style_zoom)
        } else {
            requested_style_zoom
        };
        let effective_transition = if motion_graded { None } else { transition };
        let evaluated_style_zoom =
            quantized_style_zoom as f64 / VECTOR_BASE_STYLE_ZOOM_QUANTIZATION_STEPS_PER_LEVEL;
        let sprite_atlas = self.resolved_sprite_atlas();
        // Read the cache's verdict for every address under one lock, then
        // style from the cloned tiles without holding it.
        let (cached_tiles, in_flight_fetch_count) = {
            let decoded_tiles = self
                .decoded_tiles
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let cached_tiles = requested_tile_keys
                .iter()
                .copied()
                .map(|tile_key| {
                    (
                        tile_key,
                        decoded_tiles.get(&tile_key),
                        decoded_tiles.failure_details.get(&tile_key).cloned(),
                    )
                })
                .collect::<Vec<(TileKey, Option<Arc<VectorTile>>, Option<String>)>>();
            (cached_tiles, decoded_tiles.in_flight.len())
        };
        let mut drawable_tiles: Vec<(TileKey, Arc<VectorTile>)> = Vec::new();
        let mut missing_tile_keys = Vec::new();
        let mut failed_tile_count = 0_usize;
        let mut pending_tile_count = 0_usize;
        let mut last_failure_detail: Option<String> = None;
        for (tile_key, decoded_tile, failure_detail) in cached_tiles {
            match decoded_tile {
                Some(decoded_tile) => drawable_tiles.push((tile_key, decoded_tile)),
                None => {
                    if let Some(failure_detail) = failure_detail {
                        failed_tile_count += 1;
                        last_failure_detail = Some(failure_detail);
                    } else {
                        // Neither cached nor failed: either its prefetch is
                        // running or the caller has not requested it yet. Both
                        // are pending detail a later frame draws, so the frame
                        // reports them honestly and keeps presenting.
                        pending_tile_count += 1;
                    }
                    missing_tile_keys.push(tile_key);
                }
            }
        }
        // A missing tile whose nearest cached ancestor exists is painted from
        // that ancestor once per ancestor, not once per child: one coarse tile
        // covers four missing children without stacking its translucent fills
        // four times.
        let (fallback_ancestors, covered_tile_count) = {
            let decoded_tiles = self
                .decoded_tiles
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let mut fallback_ancestors: Vec<(TileKey, Arc<VectorTile>)> = Vec::new();
            let mut seen_ancestor_tile_keys = HashSet::new();
            let mut covered_tile_count = 0;
            for missing_tile_key in &missing_tile_keys {
                let mut ancestor_tile_key = missing_tile_key.parent_tile_key();
                while let Some(candidate_tile_key) = ancestor_tile_key {
                    let Some(ancestor_tile) = decoded_tiles.get(&candidate_tile_key) else {
                        ancestor_tile_key = candidate_tile_key.parent_tile_key();
                        continue;
                    };
                    covered_tile_count += 1;
                    if seen_ancestor_tile_keys.insert(candidate_tile_key) {
                        fallback_ancestors.push((candidate_tile_key, ancestor_tile));
                    }
                    break;
                }
            }
            (fallback_ancestors, covered_tile_count)
        };
        let drawable_tile_keys = drawable_tiles
            .iter()
            .map(|(tile_key, _)| *tile_key)
            .collect::<HashSet<_>>();
        // The source-layer names the painted tileset carries: the frame's own
        // tiles and the stand-in ancestors, which are painted from the same
        // tileset. The per-source matched counts compare each style layer's
        // `source-layer` against this set.
        let served_source_layers: BTreeSet<String> = fallback_ancestors
            .iter()
            .chain(drawable_tiles.iter())
            .flat_map(|(_, decoded_tile)| {
                decoded_tile
                    .layers
                    .iter()
                    .map(|tile_layer| tile_layer.name.clone())
            })
            .collect();
        // The tile-level cross-fade mirrors the raster base's state machine:
        // a level change holds the retired level while the requested one
        // fills, the hold expires after one second, and the retired level
        // dissolves over the fade once at least 85% of the requested level is
        // present. The state is resolved before the content revision so a
        // fade's changing opacity invalidates a reused frame.
        let transition_frame = match frame_time_seconds {
            Some(frame_time_seconds) if frame_time_seconds.is_finite() => {
                let ready_tile_fraction = if requested_tile_keys.is_empty() {
                    1.0
                } else {
                    drawable_tiles.len() as f64 / requested_tile_keys.len() as f64
                };
                Some(self.zoom_transition.update(
                    tile_zoom_level,
                    frame_time_seconds * MILLISECONDS_PER_SECOND,
                    ready_tile_fraction,
                ))
            }
            _ => {
                // A frame without a clock cannot animate the fade, so it
                // observes the level and draws the requested one alone.
                self.zoom_transition
                    .observe_without_transition(tile_zoom_level);
                self.retained_level = None;
                None
            }
        };
        let retained_transition = transition_frame.and_then(|transition_frame| {
            transition_frame
                .retained_tile_zoom_level
                .map(|retained_tile_zoom_level| {
                    (retained_tile_zoom_level, transition_frame.retained_opacity)
                })
        });
        // The content revision names everything the frame's pixels depend on:
        // the requested addresses (so a metadata-only pending/failed change
        // still invalidates a reused frame), the tiles painted and their paint
        // order, the style revision, the zoom bucket, the sprite atlas, the
        // label assembly, a running style transition's elapsed time and the
        // tile cross-fade's retained level and opacity. A frame with an equal
        // revision is the same frame; the caller compares one integer instead
        // of the overlay list itself.
        let content_revision = frame_content_revision(
            self.style_revision,
            quantized_style_zoom,
            motion_graded,
            effective_transition.map(|(_, elapsed_seconds)| *elapsed_seconds),
            sprite_atlas.as_ref().map_or(0, |atlas| atlas.revision()),
            &requested_tile_keys,
            &drawable_tiles,
            &fallback_ancestors,
            &drawable_tile_keys,
            transition_frame.map(|transition_frame| transition_frame.is_running),
            retained_transition,
        );
        let payload_cache_hit_count = self
            .payload_cache_counters
            .hit_count
            .load(Ordering::Relaxed);
        let payload_cache_miss_count = self
            .payload_cache_counters
            .miss_count
            .load(Ordering::Relaxed);
        // An unchanged frame is the previous allocation: no second assembly,
        // no overlay clone and no deep compare. Diagnostics are part of the
        // reuse check because a frame carries them, and they can move without
        // changing a pixel (a failed retry elsewhere, a payload-cache hit); a
        // mismatch rebuilds rather than serving stale counters.
        if let Some(last_frame) = &self.last_frame {
            let last_diagnostics = last_frame.diagnostics;
            if last_frame.content_revision == content_revision
                && last_diagnostics.payload_cache_hit_count == payload_cache_hit_count
                && last_diagnostics.payload_cache_miss_count == payload_cache_miss_count
                && last_diagnostics.in_flight_fetch_count == in_flight_fetch_count
                && last_diagnostics.time_to_first_drawn_tile_milliseconds
                    == self.first_drawn_tile_milliseconds
                // The pending/failed split and the failure detail are frame
                // metadata, not content: a retry can fail where it was pending
                // without changing a drawn tile, and the reused frame must
                // still report the newer verdict.
                && last_frame.failed_tile_count == failed_tile_count
                && last_frame.pending_tile_count == pending_tile_count
                && last_frame.last_failure_detail == last_failure_detail
                // A frame that just styled tiles reports their count; the next
                // resolve of the same content does no work and carries zero,
                // which is the frame later settles share. Likewise, a frame
                // that just extracted label candidates is rebuilt once so the
                // next frame can report the cache hits that replace the walk.
                && last_diagnostics.tessellated_tile_count == 0
                && last_diagnostics.label_candidate_extraction_count == 0
            {
                return Arc::clone(last_frame);
            }
        }
        let mut frame = VectorBaseFrame {
            overlays: Arc::new(Vec::new()),
            label_candidates: Vec::new(),
            sprite_atlas,
            requested_tile_count: requested_tile_keys.len(),
            drawn_tile_count: drawable_tiles.len(),
            fallback_tile_count: covered_tile_count,
            failed_tile_count,
            pending_tile_count,
            style_id: self.style_id.clone(),
            background: match effective_transition {
                Some((previous_style, elapsed_seconds)) => {
                    self.style.background_at_with_transition(
                        evaluated_style_zoom,
                        Some((previous_style, *elapsed_seconds)),
                    )
                }
                None => self.style.background_at(evaluated_style_zoom),
            },
            last_failure_detail,
            motion_graded,
            zoom_transition_running: transition_frame
                .is_some_and(|transition_frame| transition_frame.is_running),
            retained_tile_count: 0,
            retained_tile_zoom_level: None,
            retained_opacity: 0.0,
            source_layer_counts: Vec::new(),
            diagnostics: VectorBaseFrameDiagnostics::default(),
            content_revision,
        };
        let mut tessellated_tile_count = 0_usize;
        let mut label_candidate_extraction_count = 0_usize;
        let mut label_candidate_cache_hit_count = 0_usize;
        // One flag per compiled layer: whether any painted tile contributed
        // geometry or a label from it. Grouped by source after the tile loops
        // for the frame's per-source drawn counts.
        let mut contributed_layers = vec![false; self.style.layer_count()];
        // Style layers paint in global document order across the whole visible
        // tile set: bucket every tile's primitives by compiled layer index and
        // flatten the buckets at the end, so a later tile's layer-0 fill can
        // never paint over an earlier tile's layer-1 line.
        let mut layer_batches: Vec<Vec<MapOverlay>> = vec![Vec::new(); self.style.layer_count()];
        // The tiles this frame actually paints, own and stand-in, so a
        // retired level that repeats one of them is not drawn twice.
        let mut painted_tile_keys: Vec<TileKey> = Vec::new();
        // Coarse stand-ins paint beneath the frame's own tiles within every
        // layer, so a sibling drawn at full detail is never overpainted by its
        // parent's coarse fills. An ancestor already drawn at its own address
        // is skipped: it is in the list once already.
        for (ancestor_tile_key, ancestor_tile) in fallback_ancestors {
            if drawable_tile_keys.contains(&ancestor_tile_key) {
                continue;
            }
            painted_tile_keys.push(ancestor_tile_key);
            self.append_styled_tile_contents(
                &mut frame,
                &mut layer_batches,
                &mut contributed_layers,
                ancestor_tile_key,
                &ancestor_tile,
                StyledTileFrame {
                    quantized_style_zoom,
                    evaluated_style_zoom,
                    transition: effective_transition,
                    assemble_labels: !motion_graded,
                },
                &mut tessellated_tile_count,
                &mut label_candidate_extraction_count,
                &mut label_candidate_cache_hit_count,
            );
        }
        for (tile_key, decoded_tile) in drawable_tiles {
            painted_tile_keys.push(tile_key);
            self.append_styled_tile_contents(
                &mut frame,
                &mut layer_batches,
                &mut contributed_layers,
                tile_key,
                &decoded_tile,
                StyledTileFrame {
                    quantized_style_zoom,
                    evaluated_style_zoom,
                    transition: effective_transition,
                    assemble_labels: !motion_graded,
                },
                &mut tessellated_tile_count,
                &mut label_candidate_extraction_count,
                &mut label_candidate_cache_hit_count,
            );
        }
        frame.source_layer_counts =
            source_layer_counts_for_frame(&self.style, &served_source_layers, &contributed_layers);
        let mut overlays: Vec<MapOverlay> = layer_batches.into_iter().flatten().collect();
        // Keep this frame's painted tiles ready for the next level change.
        // The geometry itself stays in the styled cache, so retaining a level
        // costs no per-frame copy and no fetch. The snapshot is taken before
        // the retained level is appended, so a second level change during a
        // fade retires the requested level at full opacity, exactly like the
        // raster transition. A blending style has no cached geometry, so its
        // frame is not retainable and a level change during the blend keeps
        // drawing the new level alone.
        self.last_level_frame = effective_transition.is_none().then(|| LevelFrameSnapshot {
            tile_zoom_level,
            style_revision: self.style_revision,
            quantized_style_zoom,
            layer_count: self.style.layer_count(),
            tile_keys: painted_tile_keys.clone(),
        });
        // Append the retired level's geometry at the transition's opacity,
        // skipping any tile the requested level already painted.
        if let Some((retained_tile_zoom_level, retained_opacity)) = retained_transition
            && let Some(retained_level) = self.retained_level.as_ref()
            && retained_level.tile_zoom_level == retained_tile_zoom_level
        {
            let retained_tile_count = append_retained_level_overlays(
                &mut overlays,
                &self.styled_tiles,
                retained_level,
                retained_opacity,
                &painted_tile_keys,
            );
            if retained_tile_count > 0 {
                frame.retained_tile_count = retained_tile_count;
                frame.retained_tile_zoom_level = Some(retained_tile_zoom_level);
                frame.retained_opacity = retained_opacity;
            }
        }
        if !frame.zoom_transition_running {
            self.retained_level = None;
        }
        frame.overlays = Arc::new(overlays);
        // The settle frame's bucket is what a following gesture freezes on;
        // a graded frame never moves it.
        if !motion_graded {
            self.settled_style_zoom_bucket = Some(quantized_style_zoom);
        }
        if self.first_drawn_tile_milliseconds.is_none()
            && (frame.drawn_tile_count > 0 || frame.fallback_tile_count > 0)
        {
            self.first_drawn_tile_milliseconds =
                Some(u64::try_from(self.installed_at.elapsed().as_millis()).unwrap_or(u64::MAX));
        }
        frame.diagnostics = VectorBaseFrameDiagnostics {
            payload_cache_hit_count,
            payload_cache_miss_count,
            in_flight_fetch_count,
            time_to_first_drawn_tile_milliseconds: self.first_drawn_tile_milliseconds,
            tessellated_tile_count,
            label_candidate_extraction_count,
            label_candidate_cache_hit_count,
        };
        self.last_failure_detail = frame.last_failure_detail.clone();
        tracing::debug!(
            target: "vector_base",
            requested_tile_count = frame.requested_tile_count,
            drawn_tile_count = frame.drawn_tile_count,
            fallback_tile_count = frame.fallback_tile_count,
            failed_tile_count = frame.failed_tile_count,
            pending_tile_count = frame.pending_tile_count,
            retained_tile_count = frame.retained_tile_count,
            zoom_transition_running = frame.zoom_transition_running,
            tessellated_tile_count = frame.diagnostics.tessellated_tile_count,
            label_candidate_extraction_count = frame.diagnostics.label_candidate_extraction_count,
            label_candidate_cache_hit_count = frame.diagnostics.label_candidate_cache_hit_count,
            payload_cache_hit_count = frame.diagnostics.payload_cache_hit_count,
            payload_cache_miss_count = frame.diagnostics.payload_cache_miss_count,
            in_flight_fetch_count = frame.diagnostics.in_flight_fetch_count,
            time_to_first_drawn_tile_milliseconds =
                frame.diagnostics.time_to_first_drawn_tile_milliseconds,
            content_revision = frame.content_revision,
            "vector base frame resolved"
        );
        let frame = Arc::new(frame);
        self.last_frame = Some(Arc::clone(&frame));
        frame
    }

    /// Appends one tile's styled overlays and label candidates to the frame
    /// at the camera zoom's style bucket, styling the tile into the styled
    /// cache first when the bucket or the style is new. Shared by the tiles
    /// drawn at their own address and by the ancestors that stand in for
    /// missing children.
    #[allow(clippy::too_many_arguments)]
    fn append_styled_tile_contents(
        &mut self,
        frame: &mut VectorBaseFrame,
        layer_batches: &mut [Vec<MapOverlay>],
        contributed_layers: &mut [bool],
        tile_key: TileKey,
        decoded_tile: &VectorTile,
        styling: StyledTileFrame<'_>,
        tessellated_tile_count: &mut usize,
        label_candidate_extraction_count: &mut usize,
        label_candidate_cache_hit_count: &mut usize,
    ) {
        let tile_address = VectorTileAddress {
            zoom_level: tile_key.zoom_level,
            tile_x: tile_key.tile_x,
            tile_y: tile_key.tile_y,
        };
        if let Some((previous_style, elapsed_seconds)) = styling.transition {
            // A blending frame's paint differs every frame, so it never
            // enters the styled-geometry cache; the settled style does. Its
            // labels come from the same blend and are not cached either.
            let mut styled_layers = vector_tile_overlay_layers_at_zoom_with_transition(
                decoded_tile,
                tile_address,
                &self.style,
                previous_style,
                *elapsed_seconds,
                styling.evaluated_style_zoom,
            );
            simplify_overlay_layers_for_style_zoom(
                &mut styled_layers,
                styling.evaluated_style_zoom,
            );
            *tessellated_tile_count += 1;
            for (layer_index, styled_overlays) in styled_layers.into_iter().enumerate() {
                if !styled_overlays.is_empty() {
                    contributed_layers[layer_index] = true;
                }
                layer_batches[layer_index].extend(styled_overlays);
            }
            if styling.assemble_labels {
                *label_candidate_extraction_count += 1;
                let (label_candidates, label_contributed_layers) =
                    label_candidates_and_contributed_layers(
                        decoded_tile,
                        tile_address,
                        &self.style,
                        styling.evaluated_style_zoom,
                        styling.transition.map(|(previous_style, elapsed_seconds)| {
                            (previous_style, *elapsed_seconds)
                        }),
                    );
                for (layer_index, contributed) in label_contributed_layers.into_iter().enumerate() {
                    if contributed {
                        contributed_layers[layer_index] = true;
                    }
                }
                frame.label_candidates.extend(label_candidates);
            }
            return;
        }
        let styled_key = StyledTileCacheKey {
            tile_key,
            style_revision: self.style_revision,
            quantized_style_zoom: styling.quantized_style_zoom,
        };
        if !self.styled_tiles.contains_key(&styled_key) {
            let mut styled_layers = vector_tile_overlay_layers_at_zoom(
                decoded_tile,
                tile_address,
                &self.style,
                styling.evaluated_style_zoom,
            );
            // Low-zoom LOD: simplify rings to a screen-space tolerance and
            // cull sub-pixel features before the geometry enters the
            // per-tile cache, so every arrival and refit reuses the
            // reduced form rather than re-simplifying or re-tessellating.
            simplify_overlay_layers_for_style_zoom(
                &mut styled_layers,
                styling.evaluated_style_zoom,
            );
            *tessellated_tile_count += 1;
            self.store_styled_tile(
                styled_key,
                StyledTileContents {
                    overlays: styled_layers,
                    label_candidates: None,
                    label_contributed_layers: None,
                },
            );
        }
        // The label candidates share the styled cache's key: tile address,
        // style revision and zoom bucket. An unchanged settle frame reads
        // them instead of walking the tile's symbol layers again, which is
        // what keeps a steady frame's label cost at zero. A motion-graded
        // frame stores geometry only, so the following settle extracts the
        // labels once and caches them for every frame after.
        if styling.assemble_labels {
            let labels_are_cached = self
                .styled_tiles
                .get(&styled_key)
                .is_some_and(|styled_contents| styled_contents.label_candidates.is_some());
            if labels_are_cached {
                *label_candidate_cache_hit_count += 1;
            } else {
                let (label_candidates, label_contributed_layers) =
                    label_candidates_and_contributed_layers(
                        decoded_tile,
                        tile_address,
                        &self.style,
                        styling.evaluated_style_zoom,
                        None,
                    );
                if let Some(styled_contents) = self.styled_tiles.get_mut(&styled_key) {
                    styled_contents.label_candidates = Some(label_candidates);
                    styled_contents.label_contributed_layers = Some(label_contributed_layers);
                }
                *label_candidate_extraction_count += 1;
            }
        }
        if let Some(styled_contents) = self.styled_tiles.get(&styled_key) {
            for (layer_index, styled_overlays) in styled_contents.overlays.iter().enumerate() {
                if !styled_overlays.is_empty() {
                    contributed_layers[layer_index] = true;
                }
                layer_batches[layer_index].extend(styled_overlays.iter().cloned());
            }
            if let Some(label_contributed_layers) = &styled_contents.label_contributed_layers {
                for (layer_index, contributed) in label_contributed_layers.iter().enumerate() {
                    if *contributed {
                        contributed_layers[layer_index] = true;
                    }
                }
            }
            if styling.assemble_labels
                && let Some(label_candidates) = &styled_contents.label_candidates
            {
                frame
                    .label_candidates
                    .extend(label_candidates.iter().cloned());
            }
        }
    }

    /// The decoded tile for the address, consulted through the payload cache
    /// first and fetched from the provider only on a miss. Kept free of the
    /// scheduler's caches so the prefetch can fetch every missing tile
    /// concurrently and insert the results afterwards.
    async fn fetch_and_decode_vector_tile(
        provider: &dyn VectorTileProvider,
        payload_cache: Option<&dyn CachedResponseStore<VectorTileData>>,
        payload_cache_source_name: &str,
        payload_cache_time_to_live_seconds: u64,
        payload_cache_counters: &VectorBasePayloadCacheCounters,
        tile_key: TileKey,
    ) -> Result<Arc<VectorTile>, String> {
        let payload_cache_fingerprint =
            vector_tile_payload_cache_fingerprint(payload_cache_source_name, tile_key);
        let decode = |tile_data: &VectorTileData| -> Result<Arc<VectorTile>, String> {
            Ok(Arc::new(decode_wire_vector_tile(tile_data).map_err(
                |decode_error| {
                    format!(
                        "the vector base tile {}/{}/{} failed to decode: {decode_error}",
                        tile_key.zoom_level, tile_key.tile_x, tile_key.tile_y
                    )
                },
            )?))
        };
        if let Some(payload_cache) = payload_cache {
            match payload_cache.lookup_response(&payload_cache_fingerprint) {
                Ok(Some(cached_payload)) => {
                    payload_cache_counters
                        .hit_count
                        .fetch_add(1, Ordering::Relaxed);
                    return decode(cached_payload.as_ref());
                }
                // A miss is a fetch; a store read failure degrades to the
                // same path (the cache is an accelerator, never a gate).
                Ok(None) | Err(_) => {
                    payload_cache_counters
                        .miss_count
                        .fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        let tile_data = provider
            .fetch_vector_tile(tile_key.tile_x, tile_key.tile_y, tile_key.zoom_level)
            .await
            .map_err(|fetch_error| {
                format!(
                    "the vector base tile {}/{}/{} failed to fetch: {fetch_error}",
                    tile_key.zoom_level, tile_key.tile_x, tile_key.tile_y
                )
            })?;
        let decoded_tile = decode(&tile_data)?;
        if let Some(payload_cache) = payload_cache {
            // Storage is best-effort: a failing store must not lose the
            // freshly fetched tile.
            let _ = payload_cache.store_response(
                &payload_cache_fingerprint,
                Arc::new(tile_data),
                payload_cache_time_to_live_seconds,
            );
        }
        Ok(decoded_tile)
    }

    /// Inserts a decoded tile and evicts the oldest decoded tiles beyond the
    /// cache's capacity. Styled geometry is keyed by the address and a style
    /// revision and is bounded by its own cache, so decoded-tile eviction
    /// never touches it.
    ///
    /// The decoded-tile eviction below drops a tile's styled entries, but a
    /// tile stays decoded for many frames while the camera may style it once
    /// per zoom bucket; this cache is therefore bounded on its own too.
    fn store_styled_tile(
        &mut self,
        styled_key: StyledTileCacheKey,
        styled_contents: StyledTileContents,
    ) {
        if self
            .styled_tiles
            .insert(styled_key, styled_contents)
            .is_none()
        {
            self.styled_tile_insertion_order.push_back(styled_key);
        }
        while self.styled_tiles.len() > self.maximum_cached_styled_tile_count {
            let Some(evicted_styled_key) = self.styled_tile_insertion_order.pop_front() else {
                break;
            };
            self.styled_tiles.remove(&evicted_styled_key);
        }
    }
}

/// Whether a ring-plan tile still touches the latest visible set: it is a
/// visible address, a visible tile's parent (the coarse fallback the plan
/// prefetches) or one of the eight same-zoom neighbors of a visible tile. A
/// completion that answers `false` is stale — the camera panned away while the
/// request was in flight — and is dropped instead of cached.
fn ring_tile_is_relevant(tile_key: &TileKey, visible_tile_keys: &[TileKey]) -> bool {
    visible_tile_keys.iter().any(|visible_tile_key| {
        if visible_tile_key == tile_key
            || visible_tile_key.parent_tile_key().as_ref() == Some(tile_key)
        {
            return true;
        }
        if visible_tile_key.zoom_level != tile_key.zoom_level {
            return false;
        }
        let horizontal_distance = i64::from(tile_key.tile_x) - i64::from(visible_tile_key.tile_x);
        let vertical_distance = i64::from(tile_key.tile_y) - i64::from(visible_tile_key.tile_y);
        (-1..=1).contains(&horizontal_distance) && (-1..=1).contains(&vertical_distance)
    })
}

/// Hashes everything a resolved frame's pixels depend on, in paint order: the
/// requested addresses, the style revision, the style-zoom bucket, a running
/// transition's elapsed time, the sprite atlas revision, the motion-graded and
/// label-assembly flags, the tiles drawn at their own address and the coarse
/// ancestors painted beneath them. The revision is the caller's cheap
/// alternative to deep-comparing a frame's overlay list; equal revisions mean
/// equal overlays and equal label candidates.
#[allow(clippy::too_many_arguments)]
fn frame_content_revision(
    style_revision: u64,
    quantized_style_zoom: i64,
    motion_graded: bool,
    transition_elapsed_seconds: Option<f64>,
    sprite_revision: u64,
    requested_tile_keys: &[TileKey],
    drawable_tiles: &[(TileKey, Arc<VectorTile>)],
    fallback_ancestors: &[(TileKey, Arc<VectorTile>)],
    drawable_tile_keys: &HashSet<TileKey>,
    zoom_transition_running: Option<bool>,
    retained_level: Option<(u8, f32)>,
) -> u64 {
    let mut revision = FRAME_CONTENT_REVISION_OFFSET_BASIS;
    let mut mix = |value: u64| {
        revision ^= value;
        revision = revision.wrapping_mul(FRAME_CONTENT_REVISION_PRIME);
    };
    let mix_tile_key = |mix: &mut dyn FnMut(u64), tile_key: &TileKey| {
        mix((u64::from(tile_key.zoom_level) << 48)
            ^ (u64::from(tile_key.tile_x) << 24)
            ^ u64::from(tile_key.tile_y));
    };
    mix(style_revision);
    mix(quantized_style_zoom as u64);
    mix(u64::from(motion_graded));
    match transition_elapsed_seconds {
        Some(elapsed_seconds) => {
            mix(1);
            mix(elapsed_seconds.to_bits());
        }
        None => mix(0),
    }
    match zoom_transition_running {
        Some(is_running) => {
            mix(1);
            mix(u64::from(is_running));
        }
        None => mix(0),
    }
    match retained_level {
        Some((retained_tile_zoom_level, retained_opacity)) => {
            mix(1);
            mix(u64::from(retained_tile_zoom_level));
            mix(u64::from(retained_opacity.to_bits()));
        }
        None => mix(0),
    }
    mix(sprite_revision);
    mix(requested_tile_keys.len() as u64);
    for tile_key in requested_tile_keys {
        mix_tile_key(&mut mix, tile_key);
    }
    mix(drawable_tiles.len() as u64);
    for (tile_key, _) in drawable_tiles {
        mix_tile_key(&mut mix, tile_key);
    }
    mix(fallback_ancestors.len() as u64);
    for (tile_key, _) in fallback_ancestors {
        if !drawable_tile_keys.contains(tile_key) {
            mix_tile_key(&mut mix, tile_key);
        }
    }
    revision
}

/// Rebuilds the retired level's geometry from the styled cache at the
/// snapshot's style revision and zoom bucket and appends it at `opacity`, in
/// global style order. A tile the requested level already paints — at its own
/// address or as an ancestor stand-in — is skipped, so a zoom-in never lays
/// the same translucent geometry over itself. Returns how many retired tiles
/// were appended.
fn append_retained_level_overlays(
    overlays: &mut Vec<MapOverlay>,
    styled_tiles: &HashMap<StyledTileCacheKey, StyledTileContents>,
    retained_level: &LevelFrameSnapshot,
    opacity: f32,
    requested_level_painted_tile_keys: &[TileKey],
) -> usize {
    let mut retained_layer_batches: Vec<Vec<MapOverlay>> =
        vec![Vec::new(); retained_level.layer_count];
    let mut retained_tile_count = 0;
    for tile_key in &retained_level.tile_keys {
        if requested_level_painted_tile_keys.contains(tile_key) {
            continue;
        }
        let styled_key = StyledTileCacheKey {
            tile_key: *tile_key,
            style_revision: retained_level.style_revision,
            quantized_style_zoom: retained_level.quantized_style_zoom,
        };
        let Some(styled_contents) = styled_tiles.get(&styled_key) else {
            continue;
        };
        retained_tile_count += 1;
        for (layer_batch, tile_overlays) in retained_layer_batches
            .iter_mut()
            .zip(styled_contents.overlays.iter())
        {
            layer_batch.extend(
                tile_overlays
                    .iter()
                    .map(|overlay| overlay.with_opacity_multiplier(opacity)),
            );
        }
    }
    overlays.extend(retained_layer_batches.into_iter().flatten());
    retained_tile_count
}

/// The tile zoom level a frame's addresses belong to: every address the
/// caller lists is generated at one level, so the first names it. An empty
/// list keeps the level the transition last observed, so an empty viewport
/// never looks like a level change.
fn resolved_tile_zoom_level(
    visible_tile_keys: &[TileKey],
    observed_tile_zoom_level: Option<u8>,
) -> u8 {
    visible_tile_keys
        .first()
        .map_or(observed_tile_zoom_level.unwrap_or(0), |tile_key| {
            tile_key.zoom_level
        })
}

/// Quantizes a camera zoom level to the styled cache's bucket (the nearest
/// [`VECTOR_BASE_STYLE_ZOOM_QUANTIZATION_STEPS_PER_LEVEL`]-th of a level).
fn quantized_style_zoom(style_zoom_level: f64) -> i64 {
    (style_zoom_level * VECTOR_BASE_STYLE_ZOOM_QUANTIZATION_STEPS_PER_LEVEL).round() as i64
}

/// Unwraps the wire envelope and decodes the tile: a gzip-wrapped body is
/// decompressed first, so the decoder only ever sees plain MVT protobuf. The
/// record is borrowed because the caller keeps the payload for its cache.
pub(crate) fn decode_wire_vector_tile(tile_data: &VectorTileData) -> Result<VectorTile, String> {
    let plain_tile_bytes = match tile_data.tile_encoding {
        VectorTileEncoding::MapboxVectorTile => tile_data.tile_bytes.clone(),
        VectorTileEncoding::GzippedMapboxVectorTile => {
            let gzip_decoder = GzDecoder::new(tile_data.tile_bytes.as_slice());
            let mut unwrapped_bytes = Vec::new();
            let mut limited_decoder =
                gzip_decoder.take(MAXIMUM_DECOMPRESSED_VECTOR_TILE_BYTES as u64 + 1);
            limited_decoder
                .read_to_end(&mut unwrapped_bytes)
                .map_err(|decompress_error| {
                    format!(
                        "the gzip-wrapped vector tile payload failed to decompress: \
                         {decompress_error}"
                    )
                })?;
            if unwrapped_bytes.len() > MAXIMUM_DECOMPRESSED_VECTOR_TILE_BYTES {
                return Err(format!(
                    "the gzip-wrapped vector tile payload decompressed beyond the {MAXIMUM_DECOMPRESSED_VECTOR_TILE_BYTES}-byte limit"
                ));
            }
            unwrapped_bytes
        }
    };
    decode_vector_tile(&plain_tile_bytes).map_err(|decode_error| decode_error.to_string())
}

#[cfg(test)]
mod gzip_bomb_tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use geospatial_types::VectorTileEncoding;
    use std::io::Write;

    #[test]
    fn a_gzip_wrapped_tile_is_unwrapped_before_decoding() {
        // A provider declares its tiles gzip-wrapped; the scheduler must read
        // the per-tile encoding and decompress before the MVT decoder sees the
        // bytes, rather than handing it the gzip envelope.
        let plain_tile_bytes: &[u8] =
            include_bytes!("../../../fixtures/openstreetmap-us/sourdough_tile_z6_x18_y27.mvt");
        let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
        encoder
            .write_all(plain_tile_bytes)
            .expect("the encoder writes");
        let gzip_wrapped_bytes = encoder.finish().expect("the encoder finishes");
        let tile_data = VectorTileData {
            tile_x: 18,
            tile_y: 27,
            zoom_level: 6,
            tile_encoding: VectorTileEncoding::GzippedMapboxVectorTile,
            tile_bytes: gzip_wrapped_bytes,
        };
        let decoded = decode_wire_vector_tile(&tile_data)
            .expect("a gzip-wrapped tile is unwrapped and decoded");
        assert_eq!(
            decoded.layers.len(),
            11,
            "the unwrapped fixture decodes to its Sourdough layers"
        );
        assert_eq!(
            decoded.layers[0].name, "aeroways",
            "the unwrapped fixture keeps its wire order"
        );
    }

    #[test]
    fn a_decompression_bomb_is_refused_instead_of_allocating() {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
        encoder
            .write_all(&vec![0_u8; MAXIMUM_DECOMPRESSED_VECTOR_TILE_BYTES + 4096])
            .expect("the encoder writes");
        let compressed_bomb = encoder.finish().expect("the encoder finishes");
        assert!(
            compressed_bomb.len() < MAXIMUM_DECOMPRESSED_VECTOR_TILE_BYTES,
            "the fixture is a real bomb: a small payload expanding past the limit"
        );
        let tile_data = VectorTileData {
            tile_x: 0,
            tile_y: 0,
            zoom_level: 0,
            tile_encoding: VectorTileEncoding::GzippedMapboxVectorTile,
            tile_bytes: compressed_bomb,
        };
        let refusal =
            decode_wire_vector_tile(&tile_data).expect_err("a payload past the limit is refused");
        assert!(refusal.contains("decompressed beyond"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES;
    use crate::vector_style::{
        VECTOR_STYLE_ID_BRIGHT_LIGHT, VECTOR_STYLE_ID_OPENMAPTILES_DARK,
        VECTOR_STYLE_ID_OPENMAPTILES_LIGHT,
    };
    use async_trait::async_trait;
    use geospatial_types::{GeographicCoordinate, SDKError};
    use rustic_maps_test_support::{
        SACRAMENTO_CAMERA_ZOOM_LEVEL, SACRAMENTO_LATITUDE_DEGREES, SACRAMENTO_LONGITUDE_DEGREES,
    };
    use std::collections::HashMap as StdHashMap;
    use std::sync::Mutex;

    /// A one-layer style over the OpenMapTiles water layer, with an optional
    /// root `transition` duration.
    fn transition_test_style(
        fill_color: &str,
        fill_opacity: f64,
        transition_milliseconds: Option<f64>,
    ) -> CompiledStyle {
        let transition = transition_milliseconds
            .map(|duration| format!(r#", "transition": {{ "duration": {duration} }}"#))
            .unwrap_or_default();
        crate::vector_style::vector_tile_style_from_json(&format!(
            r#"{{
                "version": 8,
                "sources": {{ "base": {{ "type": "vector" }} }}{transition},
                "layers": [
                    {{
                        "id": "water",
                        "type": "fill",
                        "source": "base",
                        "source-layer": "water",
                        "paint": {{ "fill-color": "{fill_color}", "fill-opacity": {fill_opacity} }}
                    }}
                ]
            }}"#
        ))
        .expect("the transition fixture style parses")
    }

    /// The first filled polygon's resolved colour, or a panic when the frame
    /// paints none.
    fn first_fill_rgba(frame: &VectorBaseFrame) -> [f32; 4] {
        frame
            .overlays
            .iter()
            .find_map(|overlay| match overlay {
                MapOverlay::FilledPolygon(fill) => Some(fill.rgba_color),
                _ => None,
            })
            .expect("the frame paints at least one fill")
    }

    /// The committed Basic Latin range packed as the label font the tests
    /// inject.
    fn committed_label_font() -> Arc<LabelFont> {
        let font_stack = rustic_maps_glyphs::decode_glyph_font_stack(include_bytes!(
            "../../../fixtures/glyphs/noto_sans_regular_0-255.pbf"
        ))
        .expect("the committed Latin range decodes");
        Arc::new(
            LabelFont::from_glyph_font_stack(&font_stack)
                .expect("the committed Latin range packs into the label atlas"),
        )
    }

    #[test]
    fn a_scheduler_keeps_the_injected_label_font_until_the_style_changes() {
        let provider = ScriptedVectorTileProvider::serving_tile(
            OPENFREEMAP_TILE_KEY,
            OPENFREEMAP_TILE_BYTES.to_vec(),
        );
        let scheduler = VectorBaseScheduler::new(
            provider,
            "label-font",
            transition_test_style("#ff0000", 1.0, None),
        );
        assert!(
            scheduler.label_font().is_none(),
            "a scheduler without an injected font draws with the embedded range"
        );
        let label_font = committed_label_font();
        let mut scheduler = scheduler.with_label_font(Arc::clone(&label_font));
        assert!(
            scheduler
                .label_font()
                .is_some_and(|retained| Arc::ptr_eq(retained, &label_font)),
            "the injected font is retained for the base's labels"
        );
        scheduler.set_compiled_style(
            "label-font-replaced",
            transition_test_style("#0000ff", 1.0, None),
        );
        assert!(
            scheduler.label_font().is_none(),
            "a different style drops the previous style's font"
        );
    }

    #[test]
    fn a_scheduler_draws_a_prefetched_sprite_atlas_until_the_style_changes() {
        let sprite_url = "fixture://icon-sprite";
        let provider = ScriptedVectorTileProvider::serving_tile(
            OPENFREEMAP_TILE_KEY,
            OPENFREEMAP_TILE_BYTES.to_vec(),
        );
        let mut scheduler = VectorBaseScheduler::new(
            provider,
            "prefetched-sprite",
            crate::vector_style::vector_tile_style_from_json(&format!(
                r##"{{
                    "version": 8,
                    "sprite": "{sprite_url}",
                    "sources": {{ "base": {{ "type": "vector" }} }},
                    "layers": [
                        {{
                            "id": "water",
                            "type": "fill",
                            "source": "base",
                            "source-layer": "water",
                            "paint": {{ "fill-color": "#ff0000" }}
                        }}
                    ]
                }}"##
            ))
            .expect("the sprite fixture style parses"),
        );
        assert!(
            scheduler.resolved_sprite_atlas().is_none(),
            "a scheduler without an injected atlas draws no icons"
        );
        let sprite_atlas = Arc::new(
            SpriteAtlas::from_index_and_sheet_png(
                include_bytes!("../../rustic-maps-style/fixtures/sprites/icon-sprite.json"),
                include_bytes!("../../rustic-maps-style/fixtures/sprites/icon-sprite.png"),
            )
            .expect("the committed sprite fixtures decode"),
        );
        scheduler = scheduler.with_prefetched_sprite_atlas(sprite_url, Arc::clone(&sprite_atlas));
        assert!(
            scheduler
                .resolved_sprite_atlas()
                .is_some_and(|resolved| Arc::ptr_eq(&resolved, &sprite_atlas)),
            "the style's own URL resolves to the atlas the install fetched"
        );
        scheduler.set_compiled_style(
            "prefetched-sprite-replaced",
            transition_test_style("#0000ff", 1.0, None),
        );
        assert!(
            scheduler.resolved_sprite_atlas().is_none(),
            "a style without a sprite drops the previous style's atlas"
        );
    }

    #[tokio::test]
    async fn a_style_transition_blends_from_the_previous_style_over_its_declared_duration() {
        let provider = ScriptedVectorTileProvider::serving_tile(
            OPENFREEMAP_TILE_KEY,
            OPENFREEMAP_TILE_BYTES.to_vec(),
        );
        let mut scheduler = VectorBaseScheduler::new(
            provider,
            "transition-a",
            transition_test_style("#ff0000", 0.6, None),
        );
        let tile_keys = [OPENFREEMAP_TILE_KEY];
        let settled_style_a = scheduler
            .resolve_frame(&tile_keys, SACRAMENTO_CAMERA_ZOOM_LEVEL)
            .await;
        let style_a_rgba = first_fill_rgba(&settled_style_a);
        assert!((style_a_rgba[0] - 1.0).abs() < f32::EPSILON);
        assert!((style_a_rgba[3] - 0.6).abs() < f32::EPSILON);

        scheduler.set_compiled_style(
            "transition-b",
            transition_test_style("#0000ff", 0.2, Some(400.0)),
        );
        assert!(scheduler.style_transition_running());

        // The first timed frame starts the clock; its blend is still fully
        // the previous style's values.
        let at_start = scheduler.cached_frame_at_time(&tile_keys, 14.0, 10.0);
        assert_eq!(first_fill_rgba(&at_start), style_a_rgba);
        // Two hundred milliseconds later exactly half the blend has run.
        let at_half = scheduler.cached_frame_at_time(&tile_keys, 14.0, 10.2);
        let half_rgba = first_fill_rgba(&at_half);
        assert!((half_rgba[0] - 0.5).abs() < 1e-6, "{half_rgba:?}");
        assert!((half_rgba[2] - 0.5).abs() < 1e-6, "{half_rgba:?}");
        assert!((half_rgba[3] - 0.4).abs() < 1e-6, "{half_rgba:?}");
        // At the declared duration the change has landed and the transition
        // is no longer running.
        let at_end = scheduler.cached_frame_at_time(&tile_keys, 14.0, 10.4);
        let end_rgba = first_fill_rgba(&at_end);
        assert!((end_rgba[0]).abs() < 1e-6, "{end_rgba:?}");
        assert!((end_rgba[2] - 1.0).abs() < 1e-6, "{end_rgba:?}");
        assert!((end_rgba[3] - 0.2).abs() < 1e-6, "{end_rgba:?}");
        assert!(!scheduler.style_transition_running());
    }

    /// A background-only style with an optional root transition block.
    fn background_transition_style(
        color: &str,
        opacity: f64,
        transition_milliseconds: Option<f64>,
    ) -> CompiledStyle {
        let transition = transition_milliseconds
            .map(|duration| format!(r#", "transition": {{ "duration": {duration} }}"#))
            .unwrap_or_default();
        crate::vector_style::vector_tile_style_from_json(&format!(
            r#"{{
                "version": 8,
                "sources": {{}}{transition},
                "layers": [
                    {{
                        "id": "canvas",
                        "type": "background",
                        "paint": {{
                            "background-color": "{color}",
                            "background-opacity": {opacity}
                        }}
                    }}
                ]
            }}"#
        ))
        .expect("the background transition style parses")
    }

    #[tokio::test]
    async fn a_background_transition_blends_from_the_previous_background() {
        let provider = ScriptedVectorTileProvider::serving_tile(
            OPENFREEMAP_TILE_KEY,
            OPENFREEMAP_TILE_BYTES.to_vec(),
        );
        let mut scheduler = VectorBaseScheduler::new(
            provider,
            "background-a",
            background_transition_style("#ff0000", 1.0, None),
        );
        let tile_keys = [OPENFREEMAP_TILE_KEY];
        let settled = scheduler
            .resolve_frame(&tile_keys, SACRAMENTO_CAMERA_ZOOM_LEVEL)
            .await;
        assert_eq!(settled.background, Some([1.0, 0.0, 0.0, 1.0]));
        scheduler.set_compiled_style(
            "background-b",
            background_transition_style("#0000ff", 0.5, Some(400.0)),
        );
        // The first timed frame starts the clock; the next one is half way.
        let _at_start = scheduler.cached_frame_at_time(&tile_keys, 14.0, 10.0);
        let at_half = scheduler.cached_frame_at_time(&tile_keys, 14.0, 10.2);
        let background = at_half
            .background
            .expect("the frame carries its canvas background");
        assert!((background[0] - 0.5).abs() < 1e-6, "{background:?}");
        assert!(background[1].abs() < 1e-6, "{background:?}");
        assert!((background[2] - 0.5).abs() < 1e-6, "{background:?}");
        assert!((background[3] - 0.75).abs() < 1e-6, "{background:?}");
    }

    #[tokio::test]
    async fn the_untimed_frame_path_draws_a_style_change_immediately() {
        let provider = ScriptedVectorTileProvider::serving_tile(
            OPENFREEMAP_TILE_KEY,
            OPENFREEMAP_TILE_BYTES.to_vec(),
        );
        let mut scheduler = VectorBaseScheduler::new(
            provider,
            "transition-a",
            transition_test_style("#ff0000", 0.6, None),
        );
        let tile_keys = [OPENFREEMAP_TILE_KEY];
        scheduler
            .resolve_frame(&tile_keys, SACRAMENTO_CAMERA_ZOOM_LEVEL)
            .await;
        scheduler.set_compiled_style(
            "transition-b",
            transition_test_style("#0000ff", 0.2, Some(400.0)),
        );
        // The untimed path applies the change at once and drops the pending
        // blend, so its frame is exactly the settled style's.
        let untimed = scheduler.cached_frame(&tile_keys, 14.0);
        let untimed_rgba = first_fill_rgba(&untimed);
        assert!((untimed_rgba[0]).abs() < 1e-6, "{untimed_rgba:?}");
        assert!((untimed_rgba[2] - 1.0).abs() < 1e-6, "{untimed_rgba:?}");
        assert!((untimed_rgba[3] - 0.2).abs() < 1e-6, "{untimed_rgba:?}");
        assert!(!scheduler.style_transition_running());
    }

    /// A document whose own source declares the zoom span and attribution a
    /// style can carry, so the scheduler's policy has something to prefer over
    /// the provider descriptor.
    const STYLE_SOURCE_POLICY_DOCUMENT_JSON: &str = r##"{
        "version": 8,
        "sources": {
            "base": {
                "type": "vector",
                "attribution": "Style attribution",
                "minzoom": 3,
                "maxzoom": 9
            }
        },
        "layers": [
            {
                "id": "water",
                "type": "fill",
                "source": "base",
                "source-layer": "water",
                "paint": { "fill-color": "#0000ff" }
            }
        ]
    }"##;

    #[tokio::test]
    async fn the_styles_own_source_policy_wins_over_the_provider_descriptor() {
        let provider = ScriptedVectorTileProvider::serving_tile(
            SOURDOUGH_TILE_KEY,
            SOURDOUGH_TILE_BYTES.to_vec(),
        );
        let mut scheduler = VectorBaseScheduler::from_style_document_json(
            provider,
            "policy-style",
            STYLE_SOURCE_POLICY_DOCUMENT_JSON,
        )
        .expect("the policy document compiles");
        assert_eq!(
            scheduler.attribution_text(),
            "Style attribution",
            "the style's own declaration is the line the base reports"
        );
        assert_eq!(
            scheduler.served_zoom_level(15),
            9,
            "the style's declared span clamps above the descriptor's"
        );
        assert_eq!(scheduler.served_zoom_level(1), 3);

        // A style that declares neither falls back to the tileset's own
        // descriptor, so replacing a style never loses the provider line.
        scheduler.set_compiled_style("plain-style", transition_test_style("#ff0000", 1.0, None));
        assert_eq!(scheduler.attribution_text(), "Scripted vector tiles");
        assert_eq!(scheduler.served_zoom_level(15), 14);
        assert_eq!(scheduler.served_zoom_level(1), 1);
    }

    /// The bundled Sourdough tile the styling tests pin (z6, x18,
    /// y27, the Bahamas area of the tileset).
    const SOURDOUGH_TILE_BYTES: &[u8] =
        include_bytes!("../../../fixtures/openstreetmap-us/sourdough_tile_z6_x18_y27.mvt");
    /// The Sourdough tile's address.
    const SOURDOUGH_TILE_KEY: TileKey = TileKey {
        tile_x: 18,
        tile_y: 27,
        zoom_level: 6,
    };

    /// The bundled OpenFreeMap z14 vector tile over Sacramento: the demo tile
    /// whose place layer names Sacramento, West Sacramento, Mikon and Bryte.
    const OPENFREEMAP_TILE_BYTES: &[u8] =
        include_bytes!("../../../fixtures/openfreemap/openmaptiles_tile_z14_x2661_y6286.pbf");
    /// The OpenFreeMap demo tile's address.
    const OPENFREEMAP_TILE_KEY: TileKey = TileKey {
        tile_x: 2661,
        tile_y: 6286,
        zoom_level: 14,
    };
    /// Edge of the square viewport the label placement tests project into, in
    /// screen pixels.
    const LABEL_TEST_VIEWPORT_EDGE_PIXELS: f64 = 512.0;

    /// A provider serving scripted tiles and counting every fetch.
    struct ScriptedVectorTileProvider {
        served_tile_bytes: StdHashMap<TileKey, Vec<u8>>,
        failed_tile_keys: Vec<TileKey>,
        fetch_count: Mutex<usize>,
        descriptor: VectorTileSourceDescriptor,
    }

    impl ScriptedVectorTileProvider {
        fn serving_tile(tile_key: TileKey, tile_bytes: Vec<u8>) -> Arc<Self> {
            Arc::new(Self {
                served_tile_bytes: [(tile_key, tile_bytes)].into_iter().collect(),
                failed_tile_keys: Vec::new(),
                fetch_count: Mutex::new(0),
                descriptor: VectorTileSourceDescriptor {
                    source_name: "scripted".to_string(),
                    attribution_text: "Scripted vector tiles".to_string(),
                    minimum_zoom_level: 0,
                    maximum_zoom_level: 14,
                    style_hints: Vec::new(),
                },
            })
        }

        fn fetch_count(&self) -> usize {
            *self.fetch_count.lock().unwrap()
        }
    }

    #[async_trait]
    impl VectorTileProvider for ScriptedVectorTileProvider {
        fn source_descriptor(&self) -> VectorTileSourceDescriptor {
            self.descriptor.clone()
        }

        async fn fetch_vector_tile(
            &self,
            tile_x: u32,
            tile_y: u32,
            zoom_level: u8,
        ) -> Result<VectorTileData, SDKError> {
            *self.fetch_count.lock().unwrap() += 1;
            let tile_key = TileKey::new(tile_x, tile_y, zoom_level)?;
            if self.failed_tile_keys.contains(&tile_key) {
                return Err(SDKError::ResponseParsingFailure(format!(
                    "the scripted provider refuses {zoom_level}/{tile_x}/{tile_y}"
                )));
            }
            let tile_bytes = self
                .served_tile_bytes
                .get(&tile_key)
                .cloned()
                .ok_or_else(|| {
                    SDKError::ResponseParsingFailure(format!(
                        "the scripted provider serves no {zoom_level}/{tile_x}/{tile_y}"
                    ))
                })?;
            VectorTileData::from_wire_bytes(tile_x, tile_y, zoom_level, tile_bytes)
        }
    }

    /// A provider serving the same fixture bytes at every requested address,
    /// for the ring and prefetch plans whose tiles are not individually
    /// scripted.
    struct RepeatingVectorTileProvider {
        tile_bytes: Vec<u8>,
        fetch_count: Mutex<usize>,
    }

    impl RepeatingVectorTileProvider {
        fn serving_bytes(tile_bytes: Vec<u8>) -> Arc<Self> {
            Arc::new(Self {
                tile_bytes,
                fetch_count: Mutex::new(0),
            })
        }

        fn fetch_count(&self) -> usize {
            *self.fetch_count.lock().unwrap()
        }
    }

    #[async_trait]
    impl VectorTileProvider for RepeatingVectorTileProvider {
        fn source_descriptor(&self) -> VectorTileSourceDescriptor {
            VectorTileSourceDescriptor {
                source_name: "repeating".to_string(),
                attribution_text: "Repeating vector tiles".to_string(),
                minimum_zoom_level: 0,
                maximum_zoom_level: 14,
                style_hints: Vec::new(),
            }
        }

        async fn fetch_vector_tile(
            &self,
            tile_x: u32,
            tile_y: u32,
            zoom_level: u8,
        ) -> Result<VectorTileData, SDKError> {
            *self.fetch_count.lock().unwrap() += 1;
            VectorTileData::from_wire_bytes(tile_x, tile_y, zoom_level, self.tile_bytes.clone())
        }
    }

    /// A provider that serves every tile except one, which never answers: the
    /// stalled-tile case a fit's per-tile delivery must survive.
    struct OneStalledVectorTileProvider {
        stalled_tile_key: TileKey,
        tile_bytes: Vec<u8>,
    }

    #[async_trait]
    impl VectorTileProvider for OneStalledVectorTileProvider {
        fn source_descriptor(&self) -> VectorTileSourceDescriptor {
            VectorTileSourceDescriptor {
                source_name: "one-stalled".to_string(),
                attribution_text: "One stalled vector tile".to_string(),
                minimum_zoom_level: 0,
                maximum_zoom_level: 14,
                style_hints: Vec::new(),
            }
        }

        async fn fetch_vector_tile(
            &self,
            tile_x: u32,
            tile_y: u32,
            zoom_level: u8,
        ) -> Result<VectorTileData, SDKError> {
            let tile_key = TileKey::new(tile_x, tile_y, zoom_level)?;
            if tile_key == self.stalled_tile_key {
                std::future::pending::<()>().await;
            }
            VectorTileData::from_wire_bytes(tile_x, tile_y, zoom_level, self.tile_bytes.clone())
        }
    }

    /// A provider whose fetches never answer, modelling a stalled network the
    /// cancellability test parks on.
    struct StalledVectorTileProvider;

    #[async_trait]
    impl VectorTileProvider for StalledVectorTileProvider {
        fn source_descriptor(&self) -> VectorTileSourceDescriptor {
            VectorTileSourceDescriptor {
                source_name: "stalled".to_string(),
                attribution_text: "Stalled vector tiles".to_string(),
                minimum_zoom_level: 0,
                maximum_zoom_level: 14,
                style_hints: Vec::new(),
            }
        }

        async fn fetch_vector_tile(
            &self,
            _tile_x: u32,
            _tile_y: u32,
            _zoom_level: u8,
        ) -> Result<VectorTileData, SDKError> {
            loop {
                std::future::pending::<()>().await;
            }
        }
    }

    /// A `std` task waker that never wakes anything: enough to drive one poll
    /// of a future to its first await point.
    fn noop_waker() -> std::task::Waker {
        use std::task::{RawWaker, RawWakerVTable, Waker};
        const NOOP_WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
            |_| RawWaker::new(std::ptr::null::<()>(), &NOOP_WAKER_VTABLE),
            |_| {},
            |_| {},
            |_| {},
        );
        // SAFETY: every vtable function is a no-op operating on a null data
        // pointer, which the waker contract permits since nothing ever
        // dereferences it.
        unsafe { Waker::from_raw(RawWaker::new(std::ptr::null::<()>(), &NOOP_WAKER_VTABLE)) }
    }

    /// A one-layer style painting the sample tile's `highways` lines, with a
    /// width stop at each zoom level so a zoom change must repaint every line.
    fn highways_style_with_width_stops() -> CompiledStyle {
        crate::vector_style::vector_tile_style_from_json(
            r#"{
                "version": 8,
                "sources": { "base": { "type": "vector" } },
                "layers": [
                    {
                        "id": "highways",
                        "type": "line",
                        "source": "base",
                        "source-layer": "highways",
                        "paint": {
                            "line-color": [229.5, 153, 51, 1],
                            "line-width": ["interpolate", ["linear"], ["zoom"], 6, 2, 14, 10]
                        }
                    }
                ]
            }"#,
        )
        .expect("the test style document compiles")
    }

    fn first_line_width(frame: &VectorBaseFrame) -> f64 {
        frame
            .overlays
            .iter()
            .find_map(|overlay| match overlay {
                MapOverlay::RoutePolyline(route_overlay) => Some(route_overlay.width_pixels),
                _ => None,
            })
            .expect("the fixture's highways layer paints at least one line")
    }

    #[tokio::test]
    async fn a_resolved_frame_fetches_each_visible_tile_once_and_caches_it() {
        let provider = ScriptedVectorTileProvider::serving_tile(
            SOURDOUGH_TILE_KEY,
            SOURDOUGH_TILE_BYTES.to_vec(),
        );
        let mut scheduler = VectorBaseScheduler::new(
            Arc::clone(&provider) as Arc<dyn VectorTileProvider>,
            "test-highways",
            highways_style_with_width_stops(),
        );
        let first_frame = scheduler.resolve_frame(&[SOURDOUGH_TILE_KEY], 6.0).await;
        assert_eq!(first_frame.requested_tile_count, 1);
        assert_eq!(first_frame.drawn_tile_count, 1);
        assert_eq!(first_frame.failed_tile_count, 0);
        assert!(first_frame.is_idle());
        assert!(!first_frame.overlays.is_empty());
        assert_eq!(provider.fetch_count(), 1);

        let second_frame = scheduler.resolve_frame(&[SOURDOUGH_TILE_KEY], 6.0).await;
        assert_eq!(second_frame.overlays, first_frame.overlays);
        assert_eq!(
            provider.fetch_count(),
            1,
            "a cached tile resolves without a second fetch"
        );
    }

    #[tokio::test]
    async fn a_cached_frame_never_fetches_and_reports_pending_tiles() {
        let provider = ScriptedVectorTileProvider::serving_tile(
            SOURDOUGH_TILE_KEY,
            SOURDOUGH_TILE_BYTES.to_vec(),
        );
        let mut scheduler = VectorBaseScheduler::new(
            Arc::clone(&provider) as Arc<dyn VectorTileProvider>,
            "test-highways",
            highways_style_with_width_stops(),
        );
        let pending_frame = scheduler.cached_frame(&[SOURDOUGH_TILE_KEY], 6.0);
        assert_eq!(pending_frame.requested_tile_count, 1);
        assert_eq!(pending_frame.drawn_tile_count, 0);
        assert_eq!(pending_frame.failed_tile_count, 0);
        assert_eq!(pending_frame.pending_tile_count, 1);
        assert!(!pending_frame.is_idle());
        assert_eq!(provider.fetch_count(), 0, "a cached frame never fetches");

        let prefetch = scheduler
            .prefetch_missing_tiles(&[SOURDOUGH_TILE_KEY])
            .expect("the missing tile is claimed by the prefetch");
        let in_flight_frame = scheduler.cached_frame(&[SOURDOUGH_TILE_KEY], 6.0);
        assert_eq!(in_flight_frame.pending_tile_count, 1);
        assert_eq!(provider.fetch_count(), 0, "the prefetch runs detached");

        prefetch.await;
        let drawn_frame = scheduler.cached_frame(&[SOURDOUGH_TILE_KEY], 6.0);
        assert_eq!(drawn_frame.drawn_tile_count, 1);
        assert_eq!(drawn_frame.pending_tile_count, 0);
        assert_eq!(drawn_frame.failed_tile_count, 0);
        assert!(drawn_frame.is_idle());
        assert_eq!(provider.fetch_count(), 1);
        assert!(
            scheduler
                .prefetch_missing_tiles(&[SOURDOUGH_TILE_KEY])
                .is_none(),
            "a cached tile is never claimed again"
        );
    }

    #[tokio::test]
    async fn a_failed_prefetch_is_reported_and_retried_by_the_next_frame() {
        let provider = Arc::new(ScriptedVectorTileProvider {
            served_tile_bytes: StdHashMap::new(),
            failed_tile_keys: vec![SOURDOUGH_TILE_KEY],
            fetch_count: Mutex::new(0),
            descriptor: VectorTileSourceDescriptor {
                source_name: "scripted".to_string(),
                attribution_text: "Scripted vector tiles".to_string(),
                minimum_zoom_level: 0,
                maximum_zoom_level: 14,
                style_hints: Vec::new(),
            },
        });
        let mut scheduler = VectorBaseScheduler::new(
            Arc::clone(&provider) as Arc<dyn VectorTileProvider>,
            "test-highways",
            highways_style_with_width_stops(),
        );
        scheduler
            .prefetch_missing_tiles(&[SOURDOUGH_TILE_KEY])
            .expect("the missing tile is claimed")
            .await;
        let failed_frame = scheduler.cached_frame(&[SOURDOUGH_TILE_KEY], 6.0);
        assert_eq!(failed_frame.failed_tile_count, 1);
        assert_eq!(failed_frame.pending_tile_count, 0);
        assert!(
            failed_frame
                .last_failure_detail
                .as_deref()
                .unwrap_or_default()
                .contains("failed to fetch"),
            "the frame reports the provider's refusal"
        );

        // The next prefetch retries the address rather than caching the
        // failure forever.
        scheduler
            .prefetch_missing_tiles(&[SOURDOUGH_TILE_KEY])
            .expect("a failed address is retried")
            .await;
        assert_eq!(provider.fetch_count(), 2);
    }

    #[tokio::test]
    async fn a_motion_graded_frame_draws_from_cache_and_its_settle_equals_the_direct_frame() {
        let provider = ScriptedVectorTileProvider::serving_tile(
            OPENFREEMAP_TILE_KEY,
            OPENFREEMAP_TILE_BYTES.to_vec(),
        );
        let mut scheduler = VectorBaseScheduler::from_style_id(
            Arc::clone(&provider) as Arc<dyn VectorTileProvider>,
            VECTOR_STYLE_ID_BRIGHT_LIGHT,
        )
        .expect("the committed Bright style loads");
        let start_frame = scheduler
            .resolve_frame(&[OPENFREEMAP_TILE_KEY], SACRAMENTO_CAMERA_ZOOM_LEVEL)
            .await;
        assert_eq!(start_frame.failed_tile_count, 0);
        assert!(
            !start_frame.label_candidates.is_empty(),
            "the settled start frame carries the tile's labels"
        );
        let styled_tile_count_before_motion = scheduler.styled_tiles.len();

        // A zoom gesture crosses a style-zoom bucket. Graded, the frame keeps
        // painting the last settled bucket's styled geometry: the overlays are
        // the start frame's, no new styled entry is produced and no label
        // candidate is read.
        let moved_zoom_level = SACRAMENTO_CAMERA_ZOOM_LEVEL + 0.5;
        let motion_frame = scheduler.cached_frame_at_time_in_motion(
            &[OPENFREEMAP_TILE_KEY],
            moved_zoom_level,
            0.0,
            true,
        );
        assert!(motion_frame.motion_graded);
        assert_eq!(
            motion_frame.overlays, start_frame.overlays,
            "a graded frame paints the last settled style bucket's cached geometry"
        );
        assert!(
            motion_frame.label_candidates.is_empty(),
            "a graded frame runs no label assembly"
        );
        assert_eq!(
            scheduler.styled_tiles.len(),
            styled_tile_count_before_motion,
            "a graded frame must not style a new zoom bucket"
        );

        // The settle resolves the new bucket and the labels, and must equal
        // the frame a scheduler that never graded draws at the same pose.
        let settle_frame = scheduler.cached_frame_at_time_in_motion(
            &[OPENFREEMAP_TILE_KEY],
            moved_zoom_level,
            0.0,
            false,
        );
        assert!(!settle_frame.motion_graded);
        assert!(
            !settle_frame.label_candidates.is_empty(),
            "the settle frame resolves the labels"
        );

        let direct_provider = ScriptedVectorTileProvider::serving_tile(
            OPENFREEMAP_TILE_KEY,
            OPENFREEMAP_TILE_BYTES.to_vec(),
        );
        let mut direct_scheduler = VectorBaseScheduler::from_style_id(
            direct_provider as Arc<dyn VectorTileProvider>,
            VECTOR_STYLE_ID_BRIGHT_LIGHT,
        )
        .expect("the committed Bright style loads");
        let direct_frame = direct_scheduler
            .resolve_frame(&[OPENFREEMAP_TILE_KEY], moved_zoom_level)
            .await;

        assert_eq!(
            settle_frame.overlays, direct_frame.overlays,
            "the settle frame must paint exactly what the ungraded frame paints"
        );
        assert_eq!(
            settle_frame.label_candidates, direct_frame.label_candidates,
            "the settle frame must carry exactly the ungraded frame's labels"
        );
        assert_eq!(settle_frame.background, direct_frame.background);
        assert_eq!(settle_frame.style_id, direct_frame.style_id);
    }

    #[tokio::test]
    async fn a_symbol_style_frame_carries_its_place_labels() {
        let provider = ScriptedVectorTileProvider::serving_tile(
            OPENFREEMAP_TILE_KEY,
            OPENFREEMAP_TILE_BYTES.to_vec(),
        );
        let mut scheduler = VectorBaseScheduler::from_style_id(
            Arc::clone(&provider) as Arc<dyn VectorTileProvider>,
            VECTOR_STYLE_ID_BRIGHT_LIGHT,
        )
        .expect("the committed Bright style loads");
        let frame = scheduler
            .resolve_frame(&[OPENFREEMAP_TILE_KEY], SACRAMENTO_CAMERA_ZOOM_LEVEL)
            .await;
        assert_eq!(frame.failed_tile_count, 0);
        assert!(
            frame.background.is_some(),
            "the Bright style's canvas background must travel with the frame, so a \
             surface that hides the raster base can clear to it"
        );
        assert!(
            frame
                .label_candidates
                .iter()
                .any(|candidate| candidate.text == "Sacramento"),
            "the Bright style's symbol layer must read Sacramento from the demo tile"
        );

        let camera = MapCamera::new(
            GeographicCoordinate::from_degrees(
                SACRAMENTO_LATITUDE_DEGREES,
                SACRAMENTO_LONGITUDE_DEGREES,
            )
            .unwrap(),
            SACRAMENTO_CAMERA_ZOOM_LEVEL,
            0.0,
            0.0,
        );
        let viewport = ViewportPixels::new(
            LABEL_TEST_VIEWPORT_EDGE_PIXELS,
            LABEL_TEST_VIEWPORT_EDGE_PIXELS,
        )
        .unwrap();
        let label_layer = frame
            .label_layer(&camera, &viewport, DEFAULT_FIELD_OF_VIEW_DEGREES)
            .expect("a frame with candidates yields a label layer");
        assert!(
            label_layer
                .labels()
                .iter()
                .any(|label| label.text == "Sacramento"),
            "the Sacramento label anchors at the frame center"
        );
        assert!(
            !label_layer.quad_vertices(&viewport).text.is_empty(),
            "the embedded default font rasterizes the placed labels"
        );
    }

    /// The two-layer style the cross-tile ordering test paints: a water fill
    /// first in document order, then a highway line.
    fn fill_then_line_style() -> CompiledStyle {
        crate::vector_style::vector_tile_style_from_json(
            r#"{
                "version": 8,
                "sources": { "base": { "type": "vector" } },
                "layers": [
                    {
                        "id": "water-fill",
                        "type": "fill",
                        "source": "base",
                        "source-layer": "water",
                        "paint": { "fill-color": [51, 127.5, 229.5, 0.4] }
                    },
                    {
                        "id": "highway-line",
                        "type": "line",
                        "source": "base",
                        "source-layer": "highways",
                        "paint": { "line-color": [229.5, 153, 51, 0.9], "line-width": 2 }
                    }
                ]
            }"#,
        )
        .expect("the test style document compiles")
    }

    #[tokio::test]
    async fn a_later_tiles_fill_cannot_paint_over_an_earlier_tiles_line() {
        // Two adjacent tiles decode the committed Sourdough payload, so both
        // paint water fills and highway lines. Under tile-by-tile flattening
        // the second tile's fills land after the first tile's lines; global
        // style order must make every fill — from either tile — paint first.
        let provider = RepeatingVectorTileProvider::serving_bytes(SOURDOUGH_TILE_BYTES.to_vec());
        let mut scheduler = VectorBaseScheduler::new(
            Arc::clone(&provider) as Arc<dyn VectorTileProvider>,
            "cross-tile-order",
            fill_then_line_style(),
        );
        let west_tile_key = SOURDOUGH_TILE_KEY;
        let east_tile_key = TileKey {
            tile_x: SOURDOUGH_TILE_KEY.tile_x + 1,
            tile_y: SOURDOUGH_TILE_KEY.tile_y,
            zoom_level: SOURDOUGH_TILE_KEY.zoom_level,
        };
        // Paint order, not low-zoom LOD: style at z9 so no feature is
        // simplified or culled and the per-tile counts stay comparable.
        let frame = scheduler
            .resolve_frame(&[west_tile_key, east_tile_key], 9.0)
            .await;
        assert_eq!(frame.drawn_tile_count, 2);

        let fill_count = frame
            .overlays
            .iter()
            .filter(|overlay| matches!(overlay, MapOverlay::FilledPolygon(_)))
            .count();
        let line_count = frame
            .overlays
            .iter()
            .filter(|overlay| matches!(overlay, MapOverlay::RoutePolyline(_)))
            .count();
        let last_fill_index = frame
            .overlays
            .iter()
            .rposition(|overlay| matches!(overlay, MapOverlay::FilledPolygon(_)))
            .expect("the fixture paints water fills");
        let first_line_index = frame
            .overlays
            .iter()
            .position(|overlay| matches!(overlay, MapOverlay::RoutePolyline(_)))
            .expect("the fixture paints highway lines");
        assert!(
            last_fill_index < first_line_index,
            "every tile's fill (layer 0) must paint before every tile's line (layer 1); \
             last fill index {last_fill_index}, first line index {first_line_index}"
        );

        // Both tiles contributed both kinds, so the ordering assertion above
        // is genuinely cross-tile rather than a one-tile accident.
        let decoded_tile = decode_vector_tile(SOURDOUGH_TILE_BYTES).expect("the fixture decodes");
        let style = fill_then_line_style();
        let per_tile_fill_count = crate::vector_style::vector_tile_overlays(
            &decoded_tile,
            VectorTileAddress {
                zoom_level: west_tile_key.zoom_level,
                tile_x: west_tile_key.tile_x,
                tile_y: west_tile_key.tile_y,
            },
            &style,
        )
        .iter()
        .filter(|overlay| matches!(overlay, MapOverlay::FilledPolygon(_)))
        .count();
        let per_tile_line_count = crate::vector_style::vector_tile_overlays(
            &decoded_tile,
            VectorTileAddress {
                zoom_level: west_tile_key.zoom_level,
                tile_x: west_tile_key.tile_x,
                tile_y: west_tile_key.tile_y,
            },
            &style,
        )
        .iter()
        .filter(|overlay| matches!(overlay, MapOverlay::RoutePolyline(_)))
        .count();
        assert!(per_tile_fill_count > 0 && per_tile_line_count > 0);
        assert_eq!(fill_count, per_tile_fill_count * 2);
        assert_eq!(line_count, per_tile_line_count * 2);
    }

    #[test]
    fn the_styled_cache_is_bounded_on_its_own() {
        // One decoded tile can be styled once per zoom bucket, so the styled
        // cache cannot rely on decoded-tile eviction alone: storing more
        // entries than the cap drops the oldest.
        let provider = ScriptedVectorTileProvider::serving_tile(
            SOURDOUGH_TILE_KEY,
            SOURDOUGH_TILE_BYTES.to_vec(),
        );
        let mut scheduler = VectorBaseScheduler::new(
            Arc::clone(&provider) as Arc<dyn VectorTileProvider>,
            "test-highways",
            highways_style_with_width_stops(),
        );
        scheduler.maximum_cached_styled_tile_count = 3;
        for style_revision in 0_u64..6 {
            scheduler.store_styled_tile(
                StyledTileCacheKey {
                    tile_key: SOURDOUGH_TILE_KEY,
                    style_revision,
                    quantized_style_zoom: 140,
                },
                StyledTileContents {
                    overlays: Vec::new(),
                    label_candidates: None,
                    label_contributed_layers: None,
                },
            );
        }
        assert_eq!(
            scheduler.styled_tiles.len(),
            3,
            "the styled cache never exceeds its own capacity"
        );
        assert!(
            !scheduler
                .styled_tiles
                .keys()
                .any(|key| key.style_revision == 0),
            "the oldest styled entry is the one evicted"
        );
    }

    #[tokio::test]
    async fn the_schema_styles_carry_place_and_road_labels_from_the_committed_tile() {
        // Both OpenMapTiles-schema artifacts declare the same symbol set as
        // the named styles: place names, road names along their lines and the
        // closely zoomed points of interest. Without them a descriptor that
        // offers only the schema style would paint a map with no text.
        for style_id in [
            VECTOR_STYLE_ID_OPENMAPTILES_LIGHT,
            VECTOR_STYLE_ID_OPENMAPTILES_DARK,
        ] {
            let provider = ScriptedVectorTileProvider::serving_tile(
                OPENFREEMAP_TILE_KEY,
                OPENFREEMAP_TILE_BYTES.to_vec(),
            );
            let mut scheduler = VectorBaseScheduler::from_style_id(
                Arc::clone(&provider) as Arc<dyn VectorTileProvider>,
                style_id,
            )
            .expect("the committed OpenMapTiles style loads");
            let frame = scheduler
                .resolve_frame(&[OPENFREEMAP_TILE_KEY], SACRAMENTO_CAMERA_ZOOM_LEVEL)
                .await;
            assert_eq!(frame.failed_tile_count, 0, "{style_id} draws its tile");
            assert!(
                frame
                    .label_candidates
                    .iter()
                    .any(|candidate| candidate.text == "Sacramento"),
                "{style_id} must name the place from the committed tile"
            );
            assert!(
                frame
                    .label_candidates
                    .iter()
                    .any(|candidate| !candidate.line_coordinates.is_empty()),
                "{style_id} must place road names along their lines"
            );
        }
    }

    #[tokio::test]
    async fn a_style_without_symbol_layers_yields_no_label_layer() {
        let provider = ScriptedVectorTileProvider::serving_tile(
            SOURDOUGH_TILE_KEY,
            SOURDOUGH_TILE_BYTES.to_vec(),
        );
        let mut scheduler = VectorBaseScheduler::new(
            Arc::clone(&provider) as Arc<dyn VectorTileProvider>,
            "test-highways",
            highways_style_with_width_stops(),
        );
        let frame = scheduler.resolve_frame(&[SOURDOUGH_TILE_KEY], 6.0).await;
        assert!(
            frame.label_candidates.is_empty(),
            "a style with no symbol layer reads no labels"
        );
        let camera = MapCamera::new(
            GeographicCoordinate::from_degrees(
                SACRAMENTO_LATITUDE_DEGREES,
                SACRAMENTO_LONGITUDE_DEGREES,
            )
            .unwrap(),
            6.0,
            0.0,
            0.0,
        );
        let viewport = ViewportPixels::new(
            LABEL_TEST_VIEWPORT_EDGE_PIXELS,
            LABEL_TEST_VIEWPORT_EDGE_PIXELS,
        )
        .unwrap();
        assert!(
            frame
                .label_layer(&camera, &viewport, DEFAULT_FIELD_OF_VIEW_DEGREES)
                .is_none(),
            "a label-free frame costs the renderer no layer"
        );
    }

    #[tokio::test]
    async fn a_style_change_restyles_the_cached_tile_without_refetching() {
        let provider = ScriptedVectorTileProvider::serving_tile(
            SOURDOUGH_TILE_KEY,
            SOURDOUGH_TILE_BYTES.to_vec(),
        );
        let mut scheduler = VectorBaseScheduler::new(
            Arc::clone(&provider) as Arc<dyn VectorTileProvider>,
            "test-highways",
            highways_style_with_width_stops(),
        );
        scheduler.resolve_frame(&[SOURDOUGH_TILE_KEY], 6.0).await;

        scheduler
            .set_style(crate::vector_style::VECTOR_STYLE_ID_CARTO_DARK)
            .expect("the committed dark style loads");
        let restyled_frame = scheduler.resolve_frame(&[SOURDOUGH_TILE_KEY], 6.0).await;
        assert_eq!(provider.fetch_count(), 1);
        assert_eq!(
            restyled_frame.style_id,
            crate::vector_style::VECTOR_STYLE_ID_CARTO_DARK
        );
        assert_eq!(restyled_frame.drawn_tile_count, 1);
        assert!(
            !restyled_frame.overlays.is_empty(),
            "the dark style paints the fixture's schema layers"
        );
    }

    #[tokio::test]
    async fn crossing_a_width_stop_restyles_the_cached_tile_without_refetching() {
        let provider = ScriptedVectorTileProvider::serving_tile(
            SOURDOUGH_TILE_KEY,
            SOURDOUGH_TILE_BYTES.to_vec(),
        );
        let mut scheduler = VectorBaseScheduler::new(
            Arc::clone(&provider) as Arc<dyn VectorTileProvider>,
            "test-highways",
            highways_style_with_width_stops(),
        );
        let near_frame = scheduler.resolve_frame(&[SOURDOUGH_TILE_KEY], 6.0).await;
        let far_frame = scheduler.resolve_frame(&[SOURDOUGH_TILE_KEY], 14.0).await;
        assert_eq!(
            provider.fetch_count(),
            1,
            "the same tile restyles across a zoom stop without a refetch"
        );
        assert_eq!(first_line_width(&near_frame), 2.0);
        assert_eq!(first_line_width(&far_frame), 10.0);
    }

    #[tokio::test]
    async fn a_zoom_within_one_bucket_reuses_the_styled_geometry() {
        let provider = ScriptedVectorTileProvider::serving_tile(
            SOURDOUGH_TILE_KEY,
            SOURDOUGH_TILE_BYTES.to_vec(),
        );
        let mut scheduler = VectorBaseScheduler::new(
            Arc::clone(&provider) as Arc<dyn VectorTileProvider>,
            "test-highways",
            highways_style_with_width_stops(),
        );
        let first_frame = scheduler.resolve_frame(&[SOURDOUGH_TILE_KEY], 6.0).await;
        let same_bucket_frame = scheduler.resolve_frame(&[SOURDOUGH_TILE_KEY], 6.1).await;
        assert_eq!(first_frame.overlays, same_bucket_frame.overlays);
        assert_eq!(provider.fetch_count(), 1);
    }

    #[tokio::test]
    async fn a_failing_tile_is_counted_and_does_not_fail_the_frame() {
        let provider = Arc::new(ScriptedVectorTileProvider {
            served_tile_bytes: [(SOURDOUGH_TILE_KEY, SOURDOUGH_TILE_BYTES.to_vec())]
                .into_iter()
                .collect(),
            failed_tile_keys: vec![SOURDOUGH_TILE_KEY],
            fetch_count: Mutex::new(0),
            descriptor: VectorTileSourceDescriptor {
                source_name: "scripted".to_string(),
                attribution_text: "Scripted vector tiles".to_string(),
                minimum_zoom_level: 0,
                maximum_zoom_level: 14,
                style_hints: Vec::new(),
            },
        });
        let mut scheduler = VectorBaseScheduler::new(
            Arc::clone(&provider) as Arc<dyn VectorTileProvider>,
            "test-highways",
            highways_style_with_width_stops(),
        );
        let frame = scheduler.resolve_frame(&[SOURDOUGH_TILE_KEY], 6.0).await;
        assert_eq!(frame.failed_tile_count, 1);
        assert_eq!(frame.drawn_tile_count, 0);
        assert!(frame.overlays.is_empty());
        assert!(!frame.is_idle());
        assert!(
            frame
                .last_failure_detail
                .as_deref()
                .unwrap_or_default()
                .contains("failed to fetch")
        );
        assert_eq!(
            scheduler.last_failure_detail(),
            frame.last_failure_detail.as_deref()
        );
    }

    #[test]
    fn the_served_zoom_level_clamps_into_the_tileset_span() {
        let provider = ScriptedVectorTileProvider::serving_tile(
            SOURDOUGH_TILE_KEY,
            SOURDOUGH_TILE_BYTES.to_vec(),
        );
        let scheduler = VectorBaseScheduler::new(
            Arc::clone(&provider) as Arc<dyn VectorTileProvider>,
            "test-highways",
            highways_style_with_width_stops(),
        );
        assert_eq!(scheduler.served_zoom_level(6), 6);
        assert_eq!(scheduler.served_zoom_level(18), 14);
        assert_eq!(scheduler.attribution_text(), "Scripted vector tiles");
    }

    #[test]
    fn an_unknown_style_id_is_refused_and_keeps_the_current_style() {
        let provider = ScriptedVectorTileProvider::serving_tile(
            SOURDOUGH_TILE_KEY,
            SOURDOUGH_TILE_BYTES.to_vec(),
        );
        let mut scheduler = VectorBaseScheduler::new(
            Arc::clone(&provider) as Arc<dyn VectorTileProvider>,
            "test-highways",
            highways_style_with_width_stops(),
        );
        assert!(scheduler.set_style("no-such-style").is_err());
        assert_eq!(scheduler.style_id(), "test-highways");
    }

    #[tokio::test]
    async fn a_payload_cache_serves_a_second_scheduler_without_refetching() {
        let provider = ScriptedVectorTileProvider::serving_tile(
            SOURDOUGH_TILE_KEY,
            SOURDOUGH_TILE_BYTES.to_vec(),
        );
        let payload_cache = Arc::new(VectorTileMemoryCache::new(16));
        let mut first_scheduler = VectorBaseScheduler::new(
            Arc::clone(&provider) as Arc<dyn VectorTileProvider>,
            "test-highways",
            highways_style_with_width_stops(),
        )
        .with_payload_cache(
            payload_cache.clone(),
            DEFAULT_VECTOR_TILE_PAYLOAD_TIME_TO_LIVE_SECONDS,
        );
        let first_frame = first_scheduler
            .resolve_frame(&[SOURDOUGH_TILE_KEY], 6.0)
            .await;
        assert_eq!(first_frame.drawn_tile_count, 1);
        assert_eq!(first_frame.diagnostics.payload_cache_miss_count, 1);
        assert_eq!(first_frame.diagnostics.payload_cache_hit_count, 0);
        assert_eq!(provider.fetch_count(), 1);

        // A fresh scheduler (a style reinstall or a later session on the
        // native disk store) serves the same tile from the payload cache.
        let mut second_scheduler = VectorBaseScheduler::new(
            Arc::clone(&provider) as Arc<dyn VectorTileProvider>,
            "test-highways",
            highways_style_with_width_stops(),
        )
        .with_payload_cache(
            payload_cache.clone(),
            DEFAULT_VECTOR_TILE_PAYLOAD_TIME_TO_LIVE_SECONDS,
        );
        let second_frame = second_scheduler
            .resolve_frame(&[SOURDOUGH_TILE_KEY], 6.0)
            .await;
        assert_eq!(second_frame.drawn_tile_count, 1);
        assert_eq!(
            second_frame.overlays, first_frame.overlays,
            "the cached payload decodes to the same geometry"
        );
        assert_eq!(
            provider.fetch_count(),
            1,
            "the second scheduler never refetches a payload the cache holds"
        );
        assert_eq!(second_frame.diagnostics.payload_cache_hit_count, 1);
        assert_eq!(second_frame.diagnostics.payload_cache_miss_count, 0);
        assert_eq!(payload_cache.cached_tile_count(), 1);
    }

    #[tokio::test]
    async fn a_memory_payload_cache_expires_entries_after_their_time_to_live() {
        use geospatial_types::VectorTileData;

        let provider = RepeatingVectorTileProvider::serving_bytes(SOURDOUGH_TILE_BYTES.to_vec());
        let payload_cache = Arc::new(VectorTileMemoryCache::new(16));
        let fingerprint = vector_tile_payload_cache_fingerprint("repeating", SOURDOUGH_TILE_KEY);
        payload_cache
            .store_response(
                &fingerprint,
                Arc::new(
                    VectorTileData::from_wire_bytes(
                        SOURDOUGH_TILE_KEY.tile_x,
                        SOURDOUGH_TILE_KEY.tile_y,
                        SOURDOUGH_TILE_KEY.zoom_level,
                        SOURDOUGH_TILE_BYTES.to_vec(),
                    )
                    .expect("the fixture payload is a valid wire record"),
                ),
                0,
            )
            .expect("the in-memory cache accepts a store");
        assert_eq!(payload_cache.cached_tile_count(), 1);

        let mut scheduler = VectorBaseScheduler::new(
            Arc::clone(&provider) as Arc<dyn VectorTileProvider>,
            "test-highways",
            highways_style_with_width_stops(),
        )
        .with_payload_cache(payload_cache.clone(), 0);
        scheduler.resolve_frame(&[SOURDOUGH_TILE_KEY], 6.0).await;
        assert_eq!(
            provider.fetch_count(),
            1,
            "a zero time-to-live entry is expired and the tile is fetched"
        );
        assert_eq!(payload_cache.cached_tile_count(), 1);
    }

    #[tokio::test]
    async fn the_ring_prefetch_claims_the_parent_and_the_neighbors_under_a_budget() {
        let provider = RepeatingVectorTileProvider::serving_bytes(SOURDOUGH_TILE_BYTES.to_vec());
        let mut scheduler = VectorBaseScheduler::new(
            Arc::clone(&provider) as Arc<dyn VectorTileProvider>,
            "test-highways",
            highways_style_with_width_stops(),
        )
        .with_maximum_prefetch_ring_tile_count(3)
        // This fixture exercises the plan itself; the low-zoom gate is covered
        // by `a_low_zoom_fit_plans_no_ring`.
        .with_ring_prefetch_minimum_zoom_level(5);
        let visible_tile_key = TileKey::new(16, 16, 5).unwrap();

        let ring_prefetch = scheduler
            .prefetch_ring_tiles(&[visible_tile_key])
            .expect("the ring has uncached candidates");
        assert_eq!(
            ring_prefetch.await,
            3,
            "the plan claims exactly its budget, parents first"
        );
        assert_eq!(provider.fetch_count(), 3);

        // The parent is the first claim, so it is cached while the visible
        // child is not: the frame paints the parent as the child's fallback.
        let frame = scheduler.cached_frame(&[visible_tile_key], 5.0);
        assert_eq!(frame.drawn_tile_count, 0);
        assert_eq!(frame.pending_tile_count, 1);
        assert_eq!(
            frame.fallback_tile_count, 1,
            "the coarse parent stands in for the missing child"
        );
        assert!(!frame.is_idle(), "the child's own detail is still owed");
        assert!(
            !frame.overlays.is_empty(),
            "the fallback parent paints its geometry"
        );
        assert!(
            frame
                .diagnostics
                .time_to_first_drawn_tile_milliseconds
                .is_some(),
            "the fallback coverage counts as the first drawn tile"
        );
    }

    #[tokio::test]
    async fn a_failed_child_draws_its_cached_parent() {
        let child_tile_key = SOURDOUGH_TILE_KEY;
        let parent_tile_key = child_tile_key
            .parent_tile_key()
            .expect("the fixture tile has a parent");
        let provider = Arc::new(ScriptedVectorTileProvider {
            served_tile_bytes: [(parent_tile_key, SOURDOUGH_TILE_BYTES.to_vec())]
                .into_iter()
                .collect(),
            failed_tile_keys: vec![child_tile_key],
            fetch_count: Mutex::new(0),
            descriptor: VectorTileSourceDescriptor {
                source_name: "scripted".to_string(),
                attribution_text: "Scripted vector tiles".to_string(),
                minimum_zoom_level: 0,
                maximum_zoom_level: 14,
                style_hints: Vec::new(),
            },
        });
        let mut scheduler = VectorBaseScheduler::new(
            Arc::clone(&provider) as Arc<dyn VectorTileProvider>,
            "test-highways",
            highways_style_with_width_stops(),
        );
        let parent_frame = scheduler.resolve_frame(&[parent_tile_key], 6.0).await;
        assert_eq!(parent_frame.drawn_tile_count, 1);
        let parent_overlay_count = parent_frame.overlays.len();
        assert!(parent_overlay_count > 0);

        scheduler
            .prefetch_missing_tiles(&[child_tile_key])
            .expect("the failing child is claimed")
            .await;
        let frame = scheduler.cached_frame(&[child_tile_key], 6.0);
        assert_eq!(frame.drawn_tile_count, 0);
        assert_eq!(frame.failed_tile_count, 1);
        assert_eq!(frame.fallback_tile_count, 1);
        assert_eq!(
            frame.overlays.len(),
            parent_overlay_count,
            "the cached parent paints once for the failed child"
        );
        assert!(frame.last_failure_detail.is_some());
    }

    #[tokio::test]
    async fn a_cancelled_prefetch_releases_its_in_flight_claims() {
        use std::task::{Context, Poll};

        let stalled_tile_key = TileKey::new(9, 3, 6).unwrap();
        let mut scheduler = VectorBaseScheduler::new(
            Arc::new(StalledVectorTileProvider),
            "test-highways",
            highways_style_with_width_stops(),
        );
        let prefetch = scheduler
            .prefetch_missing_tiles(&[stalled_tile_key])
            .expect("the stalled tile is claimed");
        let mut prefetch_future = Box::pin(prefetch);
        let never_waking_waker = noop_waker();
        let mut never_waking_context = Context::from_waker(&never_waking_waker);
        assert_eq!(
            prefetch_future.as_mut().poll(&mut never_waking_context),
            Poll::Pending,
            "the stalled fetch parks the prefetch at its await point"
        );
        drop(prefetch_future);

        let frame = scheduler.cached_frame(&[stalled_tile_key], 6.0);
        assert_eq!(frame.pending_tile_count, 1);
        assert_eq!(
            frame.diagnostics.in_flight_fetch_count, 0,
            "a dropped prefetch must not leave its address in flight forever"
        );
        assert!(
            scheduler
                .prefetch_missing_tiles(&[stalled_tile_key])
                .is_some(),
            "the released address is fetchable again"
        );
    }

    #[tokio::test]
    async fn a_warm_refit_tessellates_no_tiles_and_one_arrival_tessellates_one() {
        let provider = RepeatingVectorTileProvider::serving_bytes(SOURDOUGH_TILE_BYTES.to_vec());
        let mut scheduler = VectorBaseScheduler::new(
            Arc::clone(&provider) as Arc<dyn VectorTileProvider>,
            "test-highways",
            highways_style_with_width_stops(),
        );
        let first_tile_key = TileKey::new(9, 3, 6).unwrap();
        let second_tile_key = TileKey::new(10, 3, 6).unwrap();
        let cold_frame = scheduler.resolve_frame(&[first_tile_key], 6.0).await;
        assert_eq!(
            cold_frame.diagnostics.tessellated_tile_count, 1,
            "a first tile is styled once"
        );

        let warm_frame = scheduler.resolve_frame(&[first_tile_key], 6.0).await;
        assert_eq!(
            warm_frame.diagnostics.tessellated_tile_count, 0,
            "a warm refit reuses the styled cache and tessellates nothing"
        );
        assert_eq!(
            warm_frame.content_revision, cold_frame.content_revision,
            "reused geometry carries the same content revision"
        );

        let one_arrival_frame = scheduler
            .resolve_frame(&[first_tile_key, second_tile_key], 6.0)
            .await;
        assert_eq!(
            one_arrival_frame.diagnostics.tessellated_tile_count, 1,
            "one newly arrived tile tessellates exactly itself"
        );
        assert_ne!(
            one_arrival_frame.content_revision, warm_frame.content_revision,
            "a new tile changes the frame content"
        );
    }

    #[tokio::test]
    async fn an_unchanged_frame_is_shared_instead_of_reassembled() {
        let provider = RepeatingVectorTileProvider::serving_bytes(SOURDOUGH_TILE_BYTES.to_vec());
        let mut scheduler = VectorBaseScheduler::new(
            Arc::clone(&provider) as Arc<dyn VectorTileProvider>,
            "test-highways",
            highways_style_with_width_stops(),
        );
        let tile_key = TileKey::new(9, 3, 6).unwrap();
        scheduler.resolve_frame(&[tile_key], 6.0).await;
        let first_frame = scheduler.cached_frame_shared(&[tile_key], 6.0);
        let second_frame = scheduler.cached_frame_shared(&[tile_key], 6.0);
        assert!(
            Arc::ptr_eq(&first_frame, &second_frame),
            "an unchanged frame is the same allocation, not a copy"
        );
        assert_eq!(first_frame.content_revision, second_frame.content_revision);
    }

    #[test]
    fn a_low_zoom_fit_plans_no_ring() {
        let provider = RepeatingVectorTileProvider::serving_bytes(SOURDOUGH_TILE_BYTES.to_vec());
        let scheduler = VectorBaseScheduler::new(
            Arc::clone(&provider) as Arc<dyn VectorTileProvider>,
            "test-highways",
            highways_style_with_width_stops(),
        );
        let low_zoom_tile_key = TileKey::new(64, 44, 7).unwrap();
        assert!(
            scheduler
                .prefetch_ring_tiles(&[low_zoom_tile_key])
                .is_none(),
            "a z7 fit stays visible-only"
        );
        let city_zoom_tile_key = TileKey::new(128, 88, 8).unwrap();
        assert!(
            scheduler
                .prefetch_ring_tiles(&[city_zoom_tile_key])
                .is_some(),
            "the ring plans again at city zoom"
        );
    }

    #[tokio::test]
    async fn a_stalled_visible_tile_does_not_hold_back_the_other_fourteen() {
        let stalled_tile_key = TileKey::new(60, 44, 7).unwrap();
        let provider = Arc::new(OneStalledVectorTileProvider {
            stalled_tile_key,
            tile_bytes: SOURDOUGH_TILE_BYTES.to_vec(),
        });
        let scheduler = VectorBaseScheduler::new(
            Arc::clone(&provider) as Arc<dyn VectorTileProvider>,
            "test-highways",
            highways_style_with_width_stops(),
        )
        .with_visible_prefetch_burst_tile_count(8);
        let mut visible_tile_keys = Vec::new();
        for tile_x in 60..65 {
            for tile_y in 44..47 {
                visible_tile_keys.push(TileKey::new(tile_x, tile_y, 7).unwrap());
            }
        }
        assert_eq!(visible_tile_keys.len(), 15);
        let (delivery_sender, mut delivery_receiver) = tokio::sync::mpsc::unbounded_channel();
        let prefetch = scheduler
            .prefetch_missing_tiles_with_delivery(&visible_tile_keys, move |tile_key| {
                let _ = delivery_sender.send(tile_key);
            })
            .expect("the visible misses are claimed");
        let prefetch_task = tokio::spawn(prefetch);
        let deliveries = tokio::time::timeout(Duration::from_millis(500), async {
            let mut delivered_tile_keys = Vec::new();
            while delivered_tile_keys.len() < 14 {
                let delivered_tile_key = delivery_receiver
                    .recv()
                    .await
                    .expect("the delivery channel stays open while a tile stalls");
                delivered_tile_keys.push(delivered_tile_key);
            }
            delivered_tile_keys
        })
        .await
        .expect("the fourteen servable tiles paint inside the 500 ms budget");
        assert!(
            !deliveries.contains(&stalled_tile_key),
            "the stalled tile is the one still owed"
        );
        prefetch_task.abort();
    }

    #[tokio::test]
    async fn a_tile_level_change_holds_the_retired_level_and_dissolves_it_after_the_hold() {
        use crate::zoom_transition::{
            ZOOM_TRANSITION_FADE_MILLISECONDS, ZOOM_TRANSITION_MAXIMUM_HOLD_MILLISECONDS,
        };

        // Serve the z7 tile only. Zooming out to its z6 parent has no cached
        // ancestor to fall back to, so the retired z7 level is the only thing
        // on screen until the new level arrives.
        let zoomed_tile_key = TileKey::new(
            SOURDOUGH_TILE_KEY.tile_x * 2,
            SOURDOUGH_TILE_KEY.tile_y * 2,
            SOURDOUGH_TILE_KEY.zoom_level + 1,
        )
        .expect("the fixture tile has a first child");
        let provider = ScriptedVectorTileProvider::serving_tile(
            zoomed_tile_key,
            SOURDOUGH_TILE_BYTES.to_vec(),
        );
        let mut scheduler = VectorBaseScheduler::new(
            Arc::clone(&provider) as Arc<dyn VectorTileProvider>,
            "test-highways",
            highways_style_with_width_stops(),
        );
        let settled = scheduler.resolve_frame(&[zoomed_tile_key], 7.0).await;
        assert_eq!(settled.drawn_tile_count, 1);
        assert!(!settled.zoom_transition_running);
        let retired_overlay_count = settled.overlays.len();
        assert!(retired_overlay_count > 0);
        let fetch_count_after_settle = provider.fetch_count();

        // The new level's tile is not cached, so the retired level is held at
        // full opacity while the requested one fills.
        let holding = scheduler.cached_frame_at_time(&[SOURDOUGH_TILE_KEY], 6.0, 0.0);
        assert!(holding.zoom_transition_running);
        assert_eq!(holding.retained_tile_zoom_level, Some(7));
        assert_eq!(holding.retained_tile_count, 1);
        assert_eq!(holding.retained_opacity, 1.0);
        assert_eq!(
            holding.overlays.len(),
            retired_overlay_count,
            "the frame draws the retired geometry over the empty new level"
        );
        assert_eq!(
            provider.fetch_count(),
            fetch_count_after_settle,
            "a transition never fetches the retired level"
        );

        // The hold expires at its cap; the fade starts from full opacity.
        let fade_start = scheduler.cached_frame_at_time(
            &[SOURDOUGH_TILE_KEY],
            6.0,
            ZOOM_TRANSITION_MAXIMUM_HOLD_MILLISECONDS / MILLISECONDS_PER_SECOND,
        );
        assert!(fade_start.zoom_transition_running);
        assert_eq!(fade_start.retained_opacity, 1.0);
        assert_eq!(fade_start.retained_tile_zoom_level, Some(7));

        let halfway = scheduler.cached_frame_at_time(
            &[SOURDOUGH_TILE_KEY],
            6.0,
            (ZOOM_TRANSITION_MAXIMUM_HOLD_MILLISECONDS + ZOOM_TRANSITION_FADE_MILLISECONDS / 2.0)
                / MILLISECONDS_PER_SECOND,
        );
        assert!((halfway.retained_opacity - 0.5).abs() < 1.0e-6);
        assert!(halfway.zoom_transition_running);

        let completed = scheduler.cached_frame_at_time(
            &[SOURDOUGH_TILE_KEY],
            6.0,
            (ZOOM_TRANSITION_MAXIMUM_HOLD_MILLISECONDS + ZOOM_TRANSITION_FADE_MILLISECONDS)
                / MILLISECONDS_PER_SECOND,
        );
        assert!(!completed.zoom_transition_running);
        assert_eq!(completed.retained_tile_count, 0);
        assert_eq!(completed.retained_tile_zoom_level, None);
        assert!(completed.overlays.is_empty());
    }

    #[tokio::test]
    async fn a_camera_jump_and_a_level_change_during_a_fade_retire_the_requested_level() {
        use crate::zoom_transition::ZOOM_TRANSITION_FADE_MILLISECONDS;

        // Every address resolves, so the new level is its own geometry (no
        // ancestor fallback) and the retired level is genuinely distinct.
        let provider = RepeatingVectorTileProvider::serving_bytes(SOURDOUGH_TILE_BYTES.to_vec());
        let mut scheduler = VectorBaseScheduler::new(
            Arc::clone(&provider) as Arc<dyn VectorTileProvider>,
            "test-highways",
            highways_style_with_width_stops(),
        );
        scheduler.resolve_frame(&[SOURDOUGH_TILE_KEY], 6.0).await;

        // A jump of six levels still retires exactly the level it left.
        let jumped_tile_key = TileKey::new(18 * 64, 27 * 64, 12).unwrap();
        let jumped = scheduler
            .resolve_frame_at_time(&[jumped_tile_key], 12.0, 0.0)
            .await;
        assert_eq!(jumped.drawn_tile_count, 1);
        assert_eq!(jumped.retained_tile_zoom_level, Some(6));
        assert_eq!(jumped.retained_tile_count, 1);
        assert_eq!(jumped.retained_opacity, 1.0);

        // The new level was ready at once, so the fade is already under way.
        let fading = scheduler.cached_frame_at_time(
            &[jumped_tile_key],
            12.0,
            ZOOM_TRANSITION_FADE_MILLISECONDS / 2.0 / MILLISECONDS_PER_SECOND,
        );
        assert!((fading.retained_opacity - 0.5).abs() < 1.0e-6);
        assert!(fading.zoom_transition_running);

        // A level change mid-fade retires the requested level (12) at full
        // opacity, exactly like the raster transition.
        let returned_back = scheduler.cached_frame_at_time(
            &[SOURDOUGH_TILE_KEY],
            6.0,
            (ZOOM_TRANSITION_FADE_MILLISECONDS / 2.0 + 1.0) / MILLISECONDS_PER_SECOND,
        );
        assert_eq!(returned_back.retained_tile_zoom_level, Some(12));
        assert_eq!(returned_back.retained_tile_count, 1);
        assert_eq!(returned_back.retained_opacity, 1.0);
        assert!(returned_back.zoom_transition_running);
    }

    #[tokio::test]
    async fn a_zoom_in_does_not_double_the_ancestor_the_new_level_already_paints() {
        let provider = ScriptedVectorTileProvider::serving_tile(
            SOURDOUGH_TILE_KEY,
            SOURDOUGH_TILE_BYTES.to_vec(),
        );
        let mut scheduler = VectorBaseScheduler::new(
            Arc::clone(&provider) as Arc<dyn VectorTileProvider>,
            "test-highways",
            highways_style_with_width_stops(),
        );
        let settled = scheduler.resolve_frame(&[SOURDOUGH_TILE_KEY], 6.0).await;
        let retired_overlay_count = settled.overlays.len();
        assert!(retired_overlay_count > 0);

        // The z7 child is missing, so the new frame paints its z6 parent as an
        // ancestor stand-in. The transition must not lay the same translucent
        // geometry over itself a second time.
        let child_tile_key = TileKey::new(36, 54, 7).unwrap();
        let holding = scheduler.cached_frame_at_time(&[child_tile_key], 7.0, 0.0);
        assert!(holding.zoom_transition_running);
        assert_eq!(holding.fallback_tile_count, 1);
        assert_eq!(holding.retained_tile_count, 0);
        assert_eq!(holding.retained_tile_zoom_level, None);
        assert_eq!(
            holding.overlays.len(),
            retired_overlay_count,
            "the ancestor fallback is the retired level's geometry, drawn once"
        );
    }

    #[tokio::test]
    async fn a_warm_refit_at_the_same_level_starts_no_transition() {
        let provider = ScriptedVectorTileProvider::serving_tile(
            SOURDOUGH_TILE_KEY,
            SOURDOUGH_TILE_BYTES.to_vec(),
        );
        let mut scheduler = VectorBaseScheduler::new(
            Arc::clone(&provider) as Arc<dyn VectorTileProvider>,
            "test-highways",
            highways_style_with_width_stops(),
        );
        scheduler.resolve_frame(&[SOURDOUGH_TILE_KEY], 6.0).await;
        let warm = scheduler.cached_frame_at_time(&[SOURDOUGH_TILE_KEY], 6.0, 1.0);
        assert!(!warm.zoom_transition_running);
        assert_eq!(warm.retained_tile_count, 0);
        assert_eq!(warm.retained_tile_zoom_level, None);
        assert_eq!(warm.drawn_tile_count, 1);
    }

    #[tokio::test]
    async fn an_unchanged_settle_frame_extracts_no_label_candidates() {
        let provider = ScriptedVectorTileProvider::serving_tile(
            OPENFREEMAP_TILE_KEY,
            OPENFREEMAP_TILE_BYTES.to_vec(),
        );
        let mut scheduler = VectorBaseScheduler::from_style_id(
            Arc::clone(&provider) as Arc<dyn VectorTileProvider>,
            VECTOR_STYLE_ID_BRIGHT_LIGHT,
        )
        .expect("the committed Bright style loads");
        let first = scheduler
            .resolve_frame(&[OPENFREEMAP_TILE_KEY], SACRAMENTO_CAMERA_ZOOM_LEVEL)
            .await;
        assert!(!first.label_candidates.is_empty());
        assert_eq!(first.diagnostics.label_candidate_extraction_count, 1);
        assert_eq!(first.diagnostics.label_candidate_cache_hit_count, 0);

        // Every later settle at the same revision, address and zoom bucket
        // reads the cached candidates: the symbol walk runs zero times.
        for _ in 0..4 {
            let unchanged =
                scheduler.cached_frame(&[OPENFREEMAP_TILE_KEY], SACRAMENTO_CAMERA_ZOOM_LEVEL);
            assert_eq!(unchanged.diagnostics.label_candidate_extraction_count, 0);
            assert_eq!(unchanged.diagnostics.label_candidate_cache_hit_count, 1);
            assert_eq!(unchanged.label_candidates, first.label_candidates);
        }
    }

    #[tokio::test]
    async fn a_motion_graded_frame_defers_label_extraction_to_the_settle() {
        let provider = ScriptedVectorTileProvider::serving_tile(
            OPENFREEMAP_TILE_KEY,
            OPENFREEMAP_TILE_BYTES.to_vec(),
        );
        let mut scheduler = VectorBaseScheduler::from_style_id(
            Arc::clone(&provider) as Arc<dyn VectorTileProvider>,
            VECTOR_STYLE_ID_BRIGHT_LIGHT,
        )
        .expect("the committed Bright style loads");
        // Decode the tile without resolving a settled frame, so the graded
        // frame is the first to style it.
        scheduler
            .prefetch_missing_tiles(&[OPENFREEMAP_TILE_KEY])
            .expect("the tile is claimed")
            .await;

        let graded = scheduler.cached_frame_at_time_in_motion(
            &[OPENFREEMAP_TILE_KEY],
            SACRAMENTO_CAMERA_ZOOM_LEVEL,
            0.0,
            true,
        );
        assert!(graded.label_candidates.is_empty());
        assert_eq!(graded.diagnostics.label_candidate_extraction_count, 0);

        let settle = scheduler.cached_frame_at_time_in_motion(
            &[OPENFREEMAP_TILE_KEY],
            SACRAMENTO_CAMERA_ZOOM_LEVEL,
            0.1,
            false,
        );
        assert!(!settle.label_candidates.is_empty());
        assert_eq!(settle.diagnostics.label_candidate_extraction_count, 1);

        let repeated = scheduler.cached_frame_at_time_in_motion(
            &[OPENFREEMAP_TILE_KEY],
            SACRAMENTO_CAMERA_ZOOM_LEVEL,
            0.2,
            false,
        );
        assert_eq!(repeated.diagnostics.label_candidate_extraction_count, 0);
        assert_eq!(repeated.diagnostics.label_candidate_cache_hit_count, 1);
    }

    #[tokio::test]
    async fn a_style_change_or_a_new_zoom_bucket_invalidates_the_label_cache() {
        let provider = ScriptedVectorTileProvider::serving_tile(
            OPENFREEMAP_TILE_KEY,
            OPENFREEMAP_TILE_BYTES.to_vec(),
        );
        let mut scheduler = VectorBaseScheduler::from_style_id(
            Arc::clone(&provider) as Arc<dyn VectorTileProvider>,
            VECTOR_STYLE_ID_BRIGHT_LIGHT,
        )
        .expect("the committed Bright style loads");
        scheduler
            .resolve_frame(&[OPENFREEMAP_TILE_KEY], SACRAMENTO_CAMERA_ZOOM_LEVEL)
            .await;
        assert_eq!(
            scheduler
                .cached_frame(&[OPENFREEMAP_TILE_KEY], SACRAMENTO_CAMERA_ZOOM_LEVEL)
                .diagnostics
                .label_candidate_extraction_count,
            0
        );

        // A new zoom bucket has its own label geometry.
        let other_bucket =
            scheduler.cached_frame(&[OPENFREEMAP_TILE_KEY], SACRAMENTO_CAMERA_ZOOM_LEVEL + 0.5);
        assert_eq!(other_bucket.diagnostics.label_candidate_extraction_count, 1);

        // A style revision bump restyles and re-extracts.
        scheduler.set_compiled_style("other-highways", highways_style_with_width_stops());
        let restyled =
            scheduler.cached_frame(&[OPENFREEMAP_TILE_KEY], SACRAMENTO_CAMERA_ZOOM_LEVEL + 0.5);
        assert_eq!(restyled.diagnostics.label_candidate_extraction_count, 1);
    }

    /// The committed synthetic Mapbox Streets-schema style document.
    const MAPBOX_VENDOR_STYLE_DOCUMENT_JSON: &str =
        include_str!("../../../fixtures/mapbox/vendor_style_document.json");
    /// The committed synthetic Mapbox Streets-schema tile the document paints.
    const MAPBOX_VENDOR_TILE_BYTES: &[u8] =
        include_bytes!("../../../fixtures/mapbox/vendor_style_tile.pbf");
    /// The committed tile's address.
    const MAPBOX_VENDOR_TILE_KEY: TileKey = TileKey {
        tile_x: 2661,
        tile_y: 6286,
        zoom_level: 14,
    };
    /// The served tileset name the committed tile and style are authored for.
    const MAPBOX_SERVED_TILESET: &str = "mapbox/mapbox.mapbox-streets-v8";

    /// The committed synthetic HERE OMV-schema style document.
    const HERE_VENDOR_STYLE_DOCUMENT_JSON: &str =
        include_str!("../../../fixtures/here/vendor_style_document.json");
    /// The committed synthetic HERE OMV-schema tile the document paints.
    const HERE_VENDOR_TILE_BYTES: &[u8] =
        include_bytes!("../../../fixtures/here/vendor_style_tile.mvt");
    /// The committed HERE tile's address.
    const HERE_VENDOR_TILE_KEY: TileKey = TileKey {
        tile_x: 18,
        tile_y: 27,
        zoom_level: 6,
    };
    /// The served tileset name HERE's descriptor declares.
    const HERE_SERVED_TILESET: &str = "here/vector";
    /// The style families HERE's descriptor declares for its vector tileset.
    const HERE_SERVED_TILESET_STYLE_HINTS: [&str; 2] = ["omv", "base"];

    /// A scripted provider serving one committed tile under a chosen
    /// descriptor, for the vendor-document tests.
    fn vendor_style_provider(
        source_name: &str,
        tile_key: TileKey,
        tile_bytes: &[u8],
    ) -> Arc<ScriptedVectorTileProvider> {
        vendor_style_provider_with_style_hints(source_name, &[], tile_key, tile_bytes)
    }

    /// A scripted provider whose descriptor also declares the tileset's style
    /// families, as the catalog providers do.
    fn vendor_style_provider_with_style_hints(
        source_name: &str,
        style_hints: &[&str],
        tile_key: TileKey,
        tile_bytes: &[u8],
    ) -> Arc<ScriptedVectorTileProvider> {
        Arc::new(ScriptedVectorTileProvider {
            served_tile_bytes: [(tile_key, tile_bytes.to_vec())].into_iter().collect(),
            failed_tile_keys: Vec::new(),
            fetch_count: Mutex::new(0),
            descriptor: VectorTileSourceDescriptor {
                source_name: source_name.to_string(),
                attribution_text: "Vendor fixture tiles".to_string(),
                minimum_zoom_level: 0,
                maximum_zoom_level: 22,
                style_hints: style_hints
                    .iter()
                    .map(|style_hint| style_hint.to_string())
                    .collect(),
            },
        })
    }

    #[test]
    fn a_document_naming_a_tileset_the_provider_does_not_serve_is_refused_by_name() {
        let provider = vendor_style_provider(
            MAPBOX_SERVED_TILESET,
            MAPBOX_VENDOR_TILE_KEY,
            MAPBOX_VENDOR_TILE_BYTES,
        );
        let document = r#"{
            "version": 8,
            "sources": {
                "base": { "type": "vector", "url": "mapbox://mapbox.mapbox-streets-v8" },
                "terrain": { "type": "vector", "url": "mapbox://mapbox.mapbox-terrain-v2" }
            },
            "layers": [
                { "id": "water", "type": "fill", "source": "base", "source-layer": "water" },
                { "id": "terrain", "type": "fill", "source": "terrain", "source-layer": "contour" }
            ]
        }"#;
        let refusal =
            match VectorBaseScheduler::from_style_document_json(provider, "vendor-style", document)
            {
                Ok(_) => panic!("a source naming another tileset must refuse"),
                Err(refusal) => refusal,
            };
        let detail = refusal.to_string();
        assert!(
            detail.contains("\"terrain\""),
            "the refusal names the declared source: {detail}"
        );
        assert!(
            detail.contains("mapbox.mapbox-terrain-v2"),
            "the refusal names the declared address: {detail}"
        );
        assert!(
            detail.contains(MAPBOX_SERVED_TILESET),
            "the refusal names the served tileset: {detail}"
        );
        assert!(
            detail.contains("served tileset"),
            "the refusal states the mismatch: {detail}"
        );
    }

    #[test]
    fn a_document_naming_the_served_tileset_installs() {
        let provider = vendor_style_provider(
            MAPBOX_SERVED_TILESET,
            MAPBOX_VENDOR_TILE_KEY,
            MAPBOX_VENDOR_TILE_BYTES,
        );
        let document = r#"{
            "version": 8,
            "sources": {
                "base": {
                    "type": "vector",
                    "url": "mapbox://mapbox.mapbox-streets-v8",
                    "tiles": ["https://tiles.example.test/mapbox.mapbox-streets-v8/{z}/{x}/{y}.mvt"]
                }
            },
            "layers": [
                { "id": "water", "type": "fill", "source": "base", "source-layer": "water" }
            ]
        }"#;
        VectorBaseScheduler::from_style_document_json(provider, "vendor-style", document)
            .expect("a source naming the served tileset installs");
    }

    #[tokio::test]
    async fn a_supported_vendor_fixture_reports_fidelity_and_painted_source_counts() {
        let provider = vendor_style_provider(
            MAPBOX_SERVED_TILESET,
            MAPBOX_VENDOR_TILE_KEY,
            MAPBOX_VENDOR_TILE_BYTES,
        );
        let mut scheduler = VectorBaseScheduler::from_style_document_json(
            provider,
            "vendor-style",
            MAPBOX_VENDOR_STYLE_DOCUMENT_JSON,
        )
        .expect("the committed vendor document installs");
        let fidelity = scheduler.style_fidelity();
        assert_eq!(
            fidelity.unsupported_property_count, 0,
            "the schema-matched vendor fixture applies every property it declares"
        );
        assert_eq!(fidelity.unsupported_property_names, Vec::<String>::new());
        assert!(
            fidelity
                .degraded_asset_names
                .iter()
                .any(|name| name == "sprite: sprite"),
            "a declared sprite the install could not fetch is named: {:?}",
            fidelity.degraded_asset_names
        );
        assert!(
            fidelity
                .degraded_asset_names
                .iter()
                .any(|name| name == "glyphs: glyphs/{fontstack}/{range}.pbf"),
            "a declared glyph endpoint that did not pack is named: {:?}",
            fidelity.degraded_asset_names
        );

        let frame = scheduler
            .resolve_frame(
                &[MAPBOX_VENDOR_TILE_KEY],
                MAPBOX_VENDOR_TILE_KEY.zoom_level as f64,
            )
            .await;
        assert_eq!(frame.drawn_tile_count, 1);
        let base_count = frame
            .source_layer_counts
            .iter()
            .find(|source_layer_count| source_layer_count.source_id == "base")
            .expect("the frame reports its declared source");
        assert_eq!(
            base_count.matched_layer_count, 4,
            "water, road, place_label and poi_label all match the served tile"
        );
        assert_eq!(
            base_count.drawn_layer_count, 4,
            "every matched layer paints geometry or places a label"
        );
    }

    #[tokio::test]
    async fn a_here_shaped_source_matches_the_providers_declared_style_hints() {
        // HERE's published style names its source with a tiles address whose
        // path carries the schema and map family (`base`, `mc`, `omv`), not
        // the descriptor's tileset id. The provider declares those families
        // as style hints, so the document must install and paint rather than
        // be refused as a foreign tileset.
        let provider = vendor_style_provider_with_style_hints(
            HERE_SERVED_TILESET,
            &HERE_SERVED_TILESET_STYLE_HINTS,
            HERE_VENDOR_TILE_KEY,
            HERE_VENDOR_TILE_BYTES,
        );
        let mut here_document: serde_json::Value =
            serde_json::from_str(HERE_VENDOR_STYLE_DOCUMENT_JSON)
                .expect("the committed HERE document is valid JSON");
        here_document["sources"]["base"]["tiles"] = serde_json::json!([
            "https://vector.hereapi.com/v2/vectortiles/base/mc/{z}/{x}/{y}/omv"
        ]);
        let document =
            serde_json::to_string(&here_document).expect("the HERE-shaped document serializes");
        let mut scheduler =
            VectorBaseScheduler::from_style_document_json(provider, "here-vendor-style", &document)
                .expect("the base/mc/omv source address matches the descriptor's style hints");
        let frame = scheduler
            .resolve_frame(
                &[HERE_VENDOR_TILE_KEY],
                HERE_VENDOR_TILE_KEY.zoom_level as f64,
            )
            .await;
        assert_eq!(frame.drawn_tile_count, 1);
        let base_count = frame
            .source_layer_counts
            .iter()
            .find(|source_layer_count| source_layer_count.source_id == "base")
            .expect("the frame reports its declared source");
        assert_eq!(
            base_count.matched_layer_count, 3,
            "water, roads and places all match the served HERE tile"
        );
        assert_eq!(
            base_count.drawn_layer_count, 3,
            "every matched layer paints geometry or places a label"
        );
    }

    #[test]
    fn unsupported_properties_are_counted_and_their_names_are_bounded() {
        let unknown_properties = (0..40)
            .map(|index| format!(r#""unknown-{index}": 1"#))
            .collect::<Vec<_>>()
            .join(", ");
        let document = format!(
            r#"{{
                "version": 8,
                "sources": {{ "base": {{ "type": "vector" }} }},
                "layers": [
                    {{
                        "id": "water",
                        "type": "fill",
                        "source": "base",
                        "source-layer": "water",
                        "paint": {{ {unknown_properties} }}
                    }}
                ]
            }}"#
        );
        let provider = vendor_style_provider(
            "fixture-tiles",
            MAPBOX_VENDOR_TILE_KEY,
            MAPBOX_VENDOR_TILE_BYTES,
        );
        let scheduler =
            VectorBaseScheduler::from_style_document_json(provider, "vendor-style", &document)
                .expect("a document with unknown properties still installs");
        let fidelity = scheduler.style_fidelity();
        assert_eq!(
            fidelity.unsupported_property_count, 40,
            "the count reports every unknown property, not only the listed ones"
        );
        assert_eq!(
            fidelity.unsupported_property_names.len(),
            MAXIMUM_REPORTED_UNSUPPORTED_PROPERTY_NAMES,
            "the name list is capped at the documented bound"
        );
        let mut sorted_names = fidelity.unsupported_property_names.clone();
        sorted_names.sort();
        assert_eq!(
            fidelity.unsupported_property_names, sorted_names,
            "the listed names are sorted"
        );
        assert!(
            fidelity
                .unsupported_property_names
                .iter()
                .all(|name| name.starts_with("layer \"water\": ")),
            "each name carries its layer scope"
        );
    }

    #[test]
    fn a_referenced_non_vector_source_is_reported_as_degraded_by_name() {
        let document = r#"{
            "version": 8,
            "sources": {
                "base": { "type": "vector" },
                "satellite": {
                    "type": "raster",
                    "tiles": ["https://tiles.example.test/{z}/{x}/{y}.png"]
                }
            },
            "layers": [
                { "id": "water", "type": "fill", "source": "base", "source-layer": "water" },
                { "id": "imagery", "type": "raster", "source": "satellite" }
            ]
        }"#;
        let provider = vendor_style_provider(
            "fixture-tiles",
            MAPBOX_VENDOR_TILE_KEY,
            MAPBOX_VENDOR_TILE_BYTES,
        );
        let scheduler =
            VectorBaseScheduler::from_style_document_json(provider, "vendor-style", document)
                .expect("a style with an undrawable source still installs");
        let fidelity = scheduler.style_fidelity();
        assert!(
            fidelity.degraded_asset_names.iter().any(|name| {
                name.contains("source \"satellite\"")
                    && name.contains("raster")
                    && name.contains("cannot draw")
            }),
            "the undrawable source is named: {:?}",
            fidelity.degraded_asset_names
        );
    }
}
