//! The exported map surface view: the FFI face of the `rustic-maps-rendering`
//! surface renderer, attaching a platform window surface, driving its
//! lifecycle and drawing camera-tiled frames with the overlay list over
//! them.
//!
//! # The view-state locking discipline
//!
//! `draw` awaits the tile fetches, so no lock may be held across the await
//! while the exported future must be `Send`. The view therefore keeps its
//! surface state (renderer, tile scheduler, extent) in a dedicated slot the
//! whole draw *owns* for its duration: `draw` takes the session out of the
//! slot under one brief lock, awaits and renders with no lock held, and
//! restores it afterwards — under a second brief lock that also honors a
//! `detach` racing the draw. The same take-out serves `resize`, so resize
//! works while a fetch is in flight unless its own brief configure collides
//! with a render in progress. Overlays live in their own lock, taken only
//! to clone the shared snapshot the frame draws. The consequences are
//! documented on the methods (serialized draws, a mid-draw resize surfacing
//! the busy state).
//!
//! # Cancellation-safe ownership
//!
//! The borrowed session and vector-base scheduler move into the spawned frame
//! work, so the restore cannot live on the caller's future: a caller that
//! drops its future mid-draw would otherwise strand the slot `Busy`. A
//! [`FrameOwnership`] guard inside the work restores the borrowed state when
//! the frame ends — normally, on an early error, or while unwinding from a
//! panic — holding `Arc` handles to the view's slots so it can write them
//! back. A racing `detach` wins: the guard drops the session and the slot
//! stays honestly detached. The spawned work always runs to completion on the
//! crate-owned runtime even when the awaiting future is gone, so the slot is
//! recoverable after every way a frame can end.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use geospatial_traits::RasterTileProvider;
use rustic_maps_rendering::{
    CLEAR_COLOR_RGBA, DEFAULT_FIELD_OF_VIEW_DEGREES, HostedGlyphRanges, LabelFont, LabelLayer,
    LocationIndicatorLayer, MapOverlay as RenderingMapOverlay, PlacedLabel, PreparedTile,
    RetainedTileLevel, SurfaceFrameStatistics, SurfaceRenderError, SurfaceRenderTarget,
    SurfaceTileRenderer, TilePrefetchPlanner, TileScheduler, TileTexturePool, VectorBaseFrame,
    VectorBaseScheduler, ViewportPixels as RenderingViewportPixels,
    WgpuSurfaceGraphicsContextSource, ZoomTransition, requested_level_ready_tile_fraction,
};
#[cfg(not(target_arch = "wasm32"))]
use rustic_maps_rendering::{SpriteAtlas, fetch_hosted_sprite_atlas};

use crate::camera::MapCamera;
use crate::error::SdkError;
use crate::platform_surface;
use crate::projection::validated_viewport_pixels;
use crate::provider::MapSdk;
use crate::records::{
    BoundingBox, EdgeInsets, FrameStatistics, GeographicCoordinate, LocationIndicatorState,
    LocationIndicatorStyle, MapOverlay, ProviderVectorTileSource, RasterImageFormat,
    RasterTileResolution, SurfaceHandle, UrlTemplateVectorTileSource, VectorTileSourceLayerCount,
    VectorTileSourceStatus, ViewportSize,
};
use crate::runtime::{drive, spawn_detached};

/// Name the surface seam reports for itself in diagnostics.
const SURFACE_SEAM_LABEL: &str = "map surface view";

/// Byte budget of the surface tile payload pool — one standard-resolution
/// encoded tile is tens of kilobytes, so the pool comfortably holds the
/// tiles of a far wider neighborhood than the default view. Mirror of the
/// wasm canvas' pool budget (`TILE_PAYLOAD_POOL_MAXIMUM_BYTES`).
const SURFACE_TILE_PAYLOAD_POOL_MAXIMUM_BYTES: u64 = 64 << 20;

/// Missing tiles one frame fetches from the surface-attached provider; the
/// rest resolve later. Mirror of the wasm canvas' per-frame fetch budget
/// (`MAXIMUM_TILE_FETCHES_PER_FRAME`).
const SURFACE_MAXIMUM_TILE_FETCHES_PER_FRAME: usize = 32;

/// Encoded image format the attached provider serves the surface with.
const SURFACE_REQUESTED_IMAGE_FORMAT: RasterImageFormat = RasterImageFormat::Png;

/// Pixel resolution the attached provider serves the surface with.
const SURFACE_REQUESTED_TILE_RESOLUTION: RasterTileResolution = RasterTileResolution::Standard;

/// The detached-view refusal every surface method reports.
const SURFACE_DETACHED_DETAIL: &str = "the map surface view is detached; attach a surface first";

/// Builds the scheduler a frame draws raster tiles through: a fresh payload
/// pool and warm-fetch registry over the resolved provider, at the surface's
/// requested image format, resolution and per-frame fetch budget. A reload
/// replaces the scheduler through this constructor, so the previous source's
/// pooled payloads and in-flight warm fetches can never reach a later frame.
fn new_raster_tile_scheduler(raster_tile_provider: Arc<dyn RasterTileProvider>) -> TileScheduler {
    TileScheduler::new(
        raster_tile_provider,
        TileTexturePool::new(SURFACE_TILE_PAYLOAD_POOL_MAXIMUM_BYTES),
        SURFACE_REQUESTED_IMAGE_FORMAT.to_native_image_format(),
        SURFACE_REQUESTED_TILE_RESOLUTION.to_native_tile_resolution(),
        SURFACE_MAXIMUM_TILE_FETCHES_PER_FRAME,
    )
}

/// The device pixels one point/CSS pixel covers before the platform reports
/// the surface's own ratio: a one-device-pixel-per-point drawing buffer. The
/// committed styles declare point sizes, so this identity ratio draws them
/// unscaled exactly as the headless golden does.
const DEFAULT_DEVICE_PIXEL_RATIO: f64 = 1.0;

/// Latitude of the view's camera before any host intent, in decimal degrees:
/// the same neutral origin the platform wrappers start from.
const DEFAULT_CAMERA_LATITUDE_DEGREES: f64 = 0.0;

/// Longitude of the view's camera before any host intent, in decimal degrees.
const DEFAULT_CAMERA_LONGITUDE_DEGREES: f64 = 0.0;

/// Zoom level of the view's camera before any host intent: the whole-world
/// framing the platform wrappers start at.
const DEFAULT_CAMERA_ZOOM_LEVEL: f64 = 2.0;

/// The view's camera pose together with the attached viewport it frames.
/// The viewport travels beside the pose because it must stay readable while
/// a draw owns the surface slot, and clears with detach exactly like the
/// platform wrappers' own viewport readback.
struct ViewCameraState {
    /// The pose the view holds and reports.
    camera: MapCamera,
    /// The attached viewport in physical pixels, or `None` before a surface
    /// attaches and after it detaches.
    viewport: Option<ViewportSize>,
}

impl ViewCameraState {
    /// The neutral pose and detached viewport a freshly built view holds.
    fn new() -> Self {
        let neutral_camera = match MapCamera::new(
            GeographicCoordinate {
                latitude: DEFAULT_CAMERA_LATITUDE_DEGREES,
                longitude: DEFAULT_CAMERA_LONGITUDE_DEGREES,
            },
            DEFAULT_CAMERA_ZOOM_LEVEL,
            0.0,
            0.0,
        ) {
            Ok(neutral_camera) => neutral_camera,
            Err(_) => unreachable!("the neutral default camera pose is canonical"),
        };
        Self {
            camera: neutral_camera,
            viewport: None,
        }
    }
}

/// The geographic bounds of `viewport` at `camera`'s pose, delegating to the
/// renderer's one bounds helper and rebuilding the mirror record the contract
/// hands out by value.
fn geographic_bounds_of_camera(camera: &MapCamera, viewport: ViewportSize) -> Option<BoundingBox> {
    let viewport_pixels = validated_viewport_pixels(viewport).ok()?;
    rustic_maps_rendering::camera_geographic_bounds(
        camera.native_camera(),
        &viewport_pixels,
        DEFAULT_FIELD_OF_VIEW_DEGREES,
    )
    .map(|bounding_box| BoundingBox::from_native_bounding_box(&bounding_box))
}

/// The presented state one settled-check compares against: the camera the
/// last frame drew (exactly), the overlay list revision as drawn, the
/// location layer, the vector base source it drew (if any), and the last
/// frame's pending tile counts (fallback, deferred or unavailable raster
/// tiles and failed vector base tiles may still change pixels when their
/// detail arrives).
#[derive(Debug, Clone)]
struct LastDrawnFrame {
    /// The camera the last frame drew, compared exactly with
    /// `is_frame_settled`'s camera.
    camera: rustic_maps_rendering::MapCamera,
    /// Whether the last frame still owed cross-fade frames: a running
    /// zoom transition keeps the presented pixels changing without any tile
    /// left pending.
    zoom_transition_running: bool,
    /// The overlay store's mutation revision when the frame snapshotted its
    /// list: any later mutation makes the presented state unsettled until the
    /// next frame draws, so the check compares one integer instead of the
    /// whole overlay list.
    overlay_revision: u64,
    /// The location layer the last frame drew.
    location_indicator: Option<LocationIndicatorLayer>,
    /// The device pixel ratio the last frame drew its labels and overlay
    /// screen sizes at; a ratio change resizes both, so the frame is not
    /// settled against a different one.
    device_pixel_ratio: f64,
    /// The vector base source the last frame drew (no source drew none),
    /// compared with the source installed at the settle check.
    vector_base_source: Option<VectorBaseSourceSummary>,
    /// How many of the last frame's requested tiles may still change pixels
    /// (the fallback, deferred or unavailable counts' sum, plus the vector
    /// base's failed tiles): a settled frame draws none of them.
    pending_tile_count: u32,
    /// Whether the last frame was drawn in the vector base's motion-graded
    /// mode. A graded frame deliberately skipped style and label assembly, so
    /// the presented pixels are not the resolved frame even when the camera is
    /// back at rest: the settle check keeps the pump drawing until a still
    /// frame resolves them.
    motion_graded: bool,
}

impl LastDrawnFrame {
    /// Whether the frame's presented pixels may still change: a pending
    /// tile's detail, a cross-fade frame still to draw, or a motion-graded
    /// frame whose settled assembly is still owed.
    fn drawn_pixels_may_still_change(&self) -> bool {
        self.pending_tile_count > 0 || self.zoom_transition_running || self.motion_graded
    }
}

/// Prefix of every identifier `add_overlay` generates: the identifier is
/// this prefix plus the view's monotonically increasing identifier index.
const OVERLAY_IDENTIFIER_PREFIX: &str = "overlay-";

/// Index of the first identifier `add_overlay` generates (`overlay-1`). The
/// counter only ever increases, so one view never reuses an identifier.
const FIRST_OVERLAY_IDENTIFIER_INDEX: u64 = 1;

/// Milliseconds in one second, converting the cross-fade clock's elapsed
/// duration into the milliseconds its state machine reads.
const MILLISECONDS_PER_SECOND: f64 = 1_000.0;

/// The refusal detail `update_overlay` throws for an identifier that does
/// not address any current overlay; the identifier itself is appended.
const UNKNOWN_OVERLAY_IDENTIFIER_DETAIL_PREFIX: &str = "no overlay is addressed by identifier ";

/// The view's overlay state: the drawn list in draw order plus the
/// identifier generator. `set_overlays` replaces the whole list and drops
/// every identifier (the documented bulk-call semantics), while
/// `add_overlay` appends and hands out the next identifier; the counter
/// survives both calls (and detach/attach) so an identifier is never reused
/// within one view's lifetime.
///
/// The drawn overlays live behind one [`Arc`]: a frame's snapshot is an `Arc`
/// clone, so an unchanged overlay list costs no payload copy per frame. A
/// mutation after the snapshot was taken copies the list once
/// ([`Arc::make_mut`]), leaving the in-flight frame drawing what it snapshotted.
#[derive(Debug)]
struct OverlayStore {
    /// The validated overlays drawn in every frame, in draw order.
    drawn_overlays: Arc<Vec<RenderingMapOverlay>>,
    /// The identifier addressing each drawn overlay at the same position;
    /// `None` for entries supplied through `set_overlays`.
    overlay_identifiers: Vec<Option<String>>,
    /// The index the next `add_overlay` identifier carries.
    next_identifier_index: u64,
    /// A monotonically increasing stamp bumped by every mutation, so the
    /// settle check compares one integer instead of the whole list.
    revision: u64,
}

/// The revision a store that has never been mutated carries.
const FIRST_OVERLAY_REVISION: u64 = 0;

impl Default for OverlayStore {
    /// Builds an empty store whose first generated identifier is
    /// `overlay-1` and whose revision is [`FIRST_OVERLAY_REVISION`].
    fn default() -> Self {
        Self {
            drawn_overlays: Arc::new(Vec::new()),
            overlay_identifiers: Vec::new(),
            next_identifier_index: FIRST_OVERLAY_IDENTIFIER_INDEX,
            revision: FIRST_OVERLAY_REVISION,
        }
    }
}

impl OverlayStore {
    /// The shared snapshot of the native overlays in draw order — the list a
    /// frame draws. Cloning the snapshot is cloning one reference count, so a
    /// frame over unchanged overlays never copies the payload.
    fn drawn_overlays(&self) -> Arc<Vec<RenderingMapOverlay>> {
        Arc::clone(&self.drawn_overlays)
    }

    /// The current mutation revision; the settle check compares it with the
    /// revision the last drawn frame snapshotted.
    fn revision(&self) -> u64 {
        self.revision
    }

    /// Advances the mutation revision, saturating at [`u64::MAX`] rather than
    /// wrapping back to a revision an earlier frame recorded.
    fn advance_revision(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }

    /// Replaces the whole drawn list with unaddressed entries (the
    /// `set_overlays` bulk semantics): every identifier generated earlier
    /// stops addressing anything.
    fn replace_all(&mut self, overlays: Vec<RenderingMapOverlay>) {
        self.overlay_identifiers = vec![None; overlays.len()];
        self.drawn_overlays = Arc::new(overlays);
        self.advance_revision();
    }

    /// Appends one addressed overlay and answers its generated identifier.
    fn add(&mut self, overlay: RenderingMapOverlay) -> String {
        let overlay_identifier =
            format!("{OVERLAY_IDENTIFIER_PREFIX}{}", self.next_identifier_index);
        self.next_identifier_index = self.next_identifier_index.saturating_add(1);
        Arc::make_mut(&mut self.drawn_overlays).push(overlay);
        self.overlay_identifiers
            .push(Some(overlay_identifier.clone()));
        self.advance_revision();
        overlay_identifier
    }

    /// Replaces the overlay the identifier addresses, keeping its position
    /// in the draw order; an identifier that addresses nothing refuses with
    /// a precise detail (an addressed-overlay input failure, reported as
    /// `InvalidRenderInput` like every overlay-addressing input refusal).
    fn update(
        &mut self,
        overlay_identifier: &str,
        overlay: RenderingMapOverlay,
    ) -> Result<(), SdkError> {
        let Some(overlay_index) = self
            .overlay_identifiers
            .iter()
            .position(|entry_identifier| entry_identifier.as_deref() == Some(overlay_identifier))
        else {
            return Err(SdkError::InvalidRenderInput {
                detail: format!(
                    "{UNKNOWN_OVERLAY_IDENTIFIER_DETAIL_PREFIX}`{overlay_identifier}`; \
                     `add_overlay` answers the identifiers in force"
                ),
            });
        };
        Arc::make_mut(&mut self.drawn_overlays)[overlay_index] = overlay;
        self.advance_revision();
        Ok(())
    }

    /// Removes the overlay the identifier addresses and answers whether it
    /// was present.
    fn remove(&mut self, overlay_identifier: &str) -> bool {
        let Some(overlay_index) = self
            .overlay_identifiers
            .iter()
            .position(|entry_identifier| entry_identifier.as_deref() == Some(overlay_identifier))
        else {
            return false;
        };
        Arc::make_mut(&mut self.drawn_overlays).remove(overlay_index);
        self.overlay_identifiers.remove(overlay_index);
        self.advance_revision();
        true
    }

    /// Clears the whole drawn list (addressed and unaddressed entries alike).
    fn clear(&mut self) {
        Arc::make_mut(&mut self.drawn_overlays).clear();
        self.overlay_identifiers.clear();
        self.advance_revision();
    }
}

/// The frame-drawing seam of a [`SurfaceSession`]: the platform surface
/// renderer in production builds, and a GPU-free double in the crate's tests
/// so the draw path's ownership, cancellation and panic behavior is
/// exercised deterministically without a platform surface.
trait SurfaceFrameRenderer: Send {
    /// Reconfigures the drawing extent in physical pixels.
    fn configure_extent(
        &mut self,
        width_pixels: u32,
        height_pixels: u32,
    ) -> Result<(), SurfaceRenderError>;

    /// Sets the frame pass's clear color.
    fn set_clear_color(&mut self, clear_color: [f32; 4]);

    /// Renders the frame's four layers, scaling the composed overlay layers'
    /// screen sizes by `overlay_device_pixel_ratio` at emission.
    // One argument per frame layer keeps the draw order explicit at the call
    // site, mirroring the renderer's own signature.
    #[allow(clippy::too_many_arguments)]
    fn render_frame_layers_and_labels(
        &mut self,
        camera: &rustic_maps_rendering::MapCamera,
        viewport: &RenderingViewportPixels,
        prepared_tiles: &[PreparedTile],
        pool: &mut TileTexturePool,
        overlay_layers: &[&[RenderingMapOverlay]],
        overlay_device_pixel_ratio: f64,
        location_indicator_layer: Option<&LocationIndicatorLayer>,
        label_layer: Option<&LabelLayer>,
    ) -> Result<SurfaceFrameStatistics, SurfaceRenderError>;

    /// Renders the frame with a retired tile zoom level drawn over the
    /// requested one, for the cross-fade a zoom that crosses a tile level
    /// uses. The default drops the retired level and draws the frame exactly
    /// as [`Self::render_frame_layers_and_labels`] does.
    // One argument per frame layer keeps the draw order explicit at the call
    // site, mirroring the renderer's own signature.
    #[allow(clippy::too_many_arguments)]
    fn render_frame_layers_and_labels_with_retained_level(
        &mut self,
        camera: &rustic_maps_rendering::MapCamera,
        viewport: &RenderingViewportPixels,
        prepared_tiles: &[PreparedTile],
        pool: &mut TileTexturePool,
        overlay_layers: &[&[RenderingMapOverlay]],
        overlay_device_pixel_ratio: f64,
        location_indicator_layer: Option<&LocationIndicatorLayer>,
        label_layer: Option<&LabelLayer>,
        retained_tile_level: Option<RetainedTileLevel>,
    ) -> Result<SurfaceFrameStatistics, SurfaceRenderError> {
        let _ = retained_tile_level;
        self.render_frame_layers_and_labels(
            camera,
            viewport,
            prepared_tiles,
            pool,
            overlay_layers,
            overlay_device_pixel_ratio,
            location_indicator_layer,
            label_layer,
        )
    }
}

impl SurfaceFrameRenderer for SurfaceTileRenderer {
    fn configure_extent(
        &mut self,
        width_pixels: u32,
        height_pixels: u32,
    ) -> Result<(), SurfaceRenderError> {
        SurfaceTileRenderer::configure_extent(self, width_pixels, height_pixels)
    }

    fn set_clear_color(&mut self, clear_color: [f32; 4]) {
        SurfaceTileRenderer::set_clear_color(self, clear_color);
    }

    fn render_frame_layers_and_labels(
        &mut self,
        camera: &rustic_maps_rendering::MapCamera,
        viewport: &RenderingViewportPixels,
        prepared_tiles: &[PreparedTile],
        pool: &mut TileTexturePool,
        overlay_layers: &[&[RenderingMapOverlay]],
        overlay_device_pixel_ratio: f64,
        location_indicator_layer: Option<&LocationIndicatorLayer>,
        label_layer: Option<&LabelLayer>,
    ) -> Result<SurfaceFrameStatistics, SurfaceRenderError> {
        SurfaceTileRenderer::render_frame_layers_and_labels_with_overlay_layers(
            self,
            camera,
            viewport,
            prepared_tiles,
            pool,
            overlay_layers,
            overlay_device_pixel_ratio,
            location_indicator_layer,
            label_layer,
        )
    }

    fn render_frame_layers_and_labels_with_retained_level(
        &mut self,
        camera: &rustic_maps_rendering::MapCamera,
        viewport: &RenderingViewportPixels,
        prepared_tiles: &[PreparedTile],
        pool: &mut TileTexturePool,
        overlay_layers: &[&[RenderingMapOverlay]],
        overlay_device_pixel_ratio: f64,
        location_indicator_layer: Option<&LocationIndicatorLayer>,
        label_layer: Option<&LabelLayer>,
        retained_tile_level: Option<RetainedTileLevel>,
    ) -> Result<SurfaceFrameStatistics, SurfaceRenderError> {
        SurfaceTileRenderer::render_frame_layers_and_labels_with_retained_level(
            self,
            camera,
            viewport,
            prepared_tiles,
            pool,
            overlay_layers,
            overlay_device_pixel_ratio,
            location_indicator_layer,
            label_layer,
            retained_tile_level,
        )
    }
}

/// The per-view drawing state: the surface renderer with its tile
/// scheduler, and the physical-pixel extent the surface is configured to.
/// Owned by whichever draw or resize call holds it out of [`MapView`]'s
/// slot lock; see the module documentation on the locking discipline.
struct SurfaceSession {
    /// The surface renderer drawing into the platform surface.
    surface_renderer: Box<dyn SurfaceFrameRenderer>,
    /// The camera-agnostic scheduler feeding the pool from the
    /// surface-attached raster provider.
    tile_scheduler: TileScheduler,
    /// Configured width in physical pixels.
    width_pixels: u32,
    /// Configured height in physical pixels.
    height_pixels: u32,
    /// The labels the previous frame placed, carried into the next frame's
    /// collision pass so the names already on screen keep their spots while
    /// the camera moves instead of letting newcomers displace them.
    previous_label_placements: Vec<PlacedLabel>,
    /// The tile-level cross-fade state: which retired zoom level stays on
    /// screen while the new level's tiles arrive, and at what opacity.
    zoom_transition: ZoomTransition,
    /// Monotonic origin for the cross-fade's timestamps.
    zoom_transition_clock_origin: std::time::Instant,
}

impl SurfaceSession {
    /// Replaces the raster scheduler with a fresh one over
    /// `raster_tile_provider` and drops the retained cross-fade level, so the
    /// frame after a source reload re-plans every fetch and never draws a
    /// payload fetched from the previous source.
    fn replace_raster_tile_scheduler(&mut self, raster_tile_provider: Arc<dyn RasterTileProvider>) {
        self.tile_scheduler = new_raster_tile_scheduler(raster_tile_provider);
        self.zoom_transition = ZoomTransition::new();
        self.zoom_transition_clock_origin = std::time::Instant::now();
    }
}

/// The occupancy of the view's slot: the idle drawing state, a draw that
/// took the state out and will restore it, or the detached state.
enum SurfaceSlot {
    // Boxed: the drawing state is many hundreds of bytes (the renderer's
    // pipelines and caches), several times the empty variants' size, and the
    // slot switches states without the state usually being read — the
    // box keeps the enum dispatch cheap.
    Idle(Box<SurfaceSession>),
    /// A draw holds the state; its restore only writes back into this
    /// state, so a racing `detach` keeps the draw's state dropped.
    Busy,
    /// No surface state: the view renders nothing until the next attach.
    Detached,
}

/// The occupancy of the vector base source's slot: the installed
/// scheduler, a draw that took it out and will restore it, or no source.
/// Unlike [`SurfaceSlot`], an install or clear racing a draw is not refused:
/// it is remembered in the view's pending-change slot and applied when the
/// draw restores the scheduler.
enum VectorBaseSlot {
    // Boxed for the same reason the surface state is: the scheduler's caches
    // dwarf the empty variants.
    Installed(Box<VectorBaseScheduler>),
    /// A draw holds the scheduler for its duration.
    Busy,
    /// No vector base source: frames draw raster tiles and overlays alone.
    Absent,
}

/// A base-map switch that landed while a draw owned the vector base slot.
/// The draw's restore applies it once the frame has drawn, so the request
/// succeeds instead of being refused; the newest request replaces any
/// earlier one.
enum PendingVectorBaseChange {
    /// Drop the base when the frame finishes.
    Clear,
    /// Install this scheduler and summary when the frame finishes.
    Install(Box<VectorBaseScheduler>, VectorBaseSourceSummary),
}

/// What the view draws a vector base with, independent of the scheduler's
/// occupancy: the status readback answers this while a draw holds the
/// scheduler out of its slot. Written when a source is installed, dropped
/// when it is cleared.
#[derive(Debug, Clone, PartialEq, Eq)]
struct VectorBaseSourceSummary {
    /// The active style id.
    style_id: String,
    /// The tileset's name.
    source_name: String,
    /// The attribution line the tileset's terms require.
    attribution_text: String,
    /// Highest zoom level the tileset serves.
    maximum_zoom_level: u8,
    /// The installed style's unsupported-property count.
    unsupported_property_count: u32,
    /// The distinct unsupported properties, bounded and sorted.
    unsupported_property_names: Vec<String>,
    /// The assets that fell back to a degraded draw, bounded and sorted.
    degraded_asset_names: Vec<String>,
}

impl VectorBaseSourceSummary {
    /// Reads the summary of an installed scheduler, including the style
    /// fidelity diagnostics it reports.
    fn from_scheduler(style_id: String, vector_base_scheduler: &VectorBaseScheduler) -> Self {
        let fidelity = vector_base_scheduler.style_fidelity();
        Self {
            style_id,
            source_name: vector_base_scheduler.source_descriptor().source_name,
            // The style's own source declaration wins over the tileset's
            // descriptor, exactly as it does in the drawn frame.
            attribution_text: vector_base_scheduler.attribution_text(),
            maximum_zoom_level: vector_base_scheduler.served_maximum_zoom_level(),
            unsupported_property_count: fidelity.unsupported_property_count,
            unsupported_property_names: fidelity.unsupported_property_names,
            degraded_asset_names: fidelity.degraded_asset_names,
        }
    }
}

/// The last resolved vector base frame's accounting, for the status
/// readback while a draw holds the scheduler.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct LastVectorBaseFrame {
    /// Tile addresses the frame drew.
    drawn_tile_count: u32,
    /// Tile addresses the frame skipped after a failed fetch or decode.
    failed_tile_count: u32,
    /// The frame's last tile failure detail, if any.
    last_failure_detail: Option<String>,
    /// The frame's per-source layer accounting, sorted by source id.
    source_layer_counts: Vec<VectorTileSourceLayerCount>,
}

impl LastVectorBaseFrame {
    /// The accounting of one resolved frame.
    fn from_vector_base_frame(vector_base_frame: &VectorBaseFrame) -> Self {
        Self {
            drawn_tile_count: u32::try_from(vector_base_frame.drawn_tile_count).unwrap_or(u32::MAX),
            failed_tile_count: u32::try_from(vector_base_frame.failed_tile_count)
                .unwrap_or(u32::MAX),
            last_failure_detail: vector_base_frame.last_failure_detail.clone(),
            source_layer_counts: vector_base_frame
                .source_layer_counts
                .iter()
                .map(|source_layer_count| VectorTileSourceLayerCount {
                    source_id: source_layer_count.source_id.clone(),
                    matched_layer_count: source_layer_count.matched_layer_count,
                    drawn_layer_count: source_layer_count.drawn_layer_count,
                })
                .collect(),
        }
    }
}

/// The view state a draw hands to the spawned frame work: `Arc` handles to
/// the slots the frame borrows and the accounting it writes.
///
/// The spawned work owns these handles for the frame's whole lifetime, which
/// is what makes the ownership transfer cancellation-safe: the work restores
/// the borrowed session and scheduler through [`FrameOwnership`] even when the
/// foreign caller drops the future awaiting it. Cloning this struct clones
/// reference counts only.
#[derive(Clone)]
struct FrameSharedState {
    /// The SDK state, for the provider-selection accounting a base-map switch
    /// writes.
    map_sdk: Arc<MapSdk>,
    /// The surface state's occupancy.
    surface_slot: Arc<Mutex<SurfaceSlot>>,
    /// The vector base scheduler's occupancy.
    vector_base: Arc<Mutex<VectorBaseSlot>>,
    /// What the view draws a vector base with.
    vector_base_summary: Arc<Mutex<Option<VectorBaseSourceSummary>>>,
    /// The last resolved vector base frame's accounting.
    last_vector_base_frame: Arc<Mutex<Option<LastVectorBaseFrame>>>,
    /// The base-map switch a call requested while a draw owned the scheduler.
    pending_vector_base_change: Arc<Mutex<Option<PendingVectorBaseChange>>>,
    /// The raster provider a `reload_tile_source` call resolved while the
    /// frame owned the surface state, applied by the restore.
    pending_raster_tile_reload: Arc<Mutex<Option<Arc<dyn RasterTileProvider>>>>,
    /// The presented state the settle check compares against.
    last_drawn_frame: Arc<Mutex<Option<LastDrawnFrame>>>,
}

impl FrameSharedState {
    /// Records the resolved vector base frame's accounting for the status
    /// readback. Taken before the scheduler restore, so a base-map switch
    /// applied there resets it instead of pairing the new source's summary
    /// with the previous source's tile counts.
    fn record_vector_base_frame(&self, vector_base_frame: &VectorBaseFrame) {
        *crate::panic_boundary::state_lock_guard(
            &self.last_vector_base_frame,
            "the surface view's last vector base frame",
        ) = Some(LastVectorBaseFrame::from_vector_base_frame(
            vector_base_frame,
        ));
    }

    /// Records the presented state one completed frame drew: the camera it
    /// drew (exactly), the overlay revision it snapshotted, the location
    /// layer and device pixel ratio it drew, the vector base source it drew,
    /// and the pending tile counts whose later detail may still change pixels.
    #[allow(clippy::too_many_arguments)]
    fn record_last_drawn_frame(
        &self,
        camera: rustic_maps_rendering::MapCamera,
        overlay_revision: u64,
        location_indicator: Option<LocationIndicatorLayer>,
        device_pixel_ratio: f64,
        vector_base_source: Option<VectorBaseSourceSummary>,
        frame_statistics: &FrameStatistics,
        vector_base_frame: Option<&VectorBaseFrame>,
        zoom_transition_running: bool,
    ) {
        let vector_base_failed_tile_count =
            u32::try_from(vector_base_frame.map_or(0, |frame| frame.failed_tile_count))
                .unwrap_or(u32::MAX);
        let vector_base_pending_tile_count =
            u32::try_from(vector_base_frame.map_or(0, |frame| frame.pending_tile_count))
                .unwrap_or(u32::MAX);
        let motion_graded =
            vector_base_frame.is_some_and(|vector_base_frame| vector_base_frame.motion_graded);
        *crate::panic_boundary::state_lock_guard(
            &self.last_drawn_frame,
            "the surface view's last drawn frame",
        ) = Some(LastDrawnFrame {
            camera,
            zoom_transition_running,
            overlay_revision,
            location_indicator,
            device_pixel_ratio,
            vector_base_source,
            pending_tile_count: frame_statistics.fallback_tile_count
                + frame_statistics.deferred_tile_count
                + frame_statistics.unavailable_tile_count
                + vector_base_failed_tile_count
                + vector_base_pending_tile_count,
            motion_graded,
        });
    }

    /// Restores the drawn session into the slot, but only when the slot still
    /// expects it: a racing `detach` left the slot [`SurfaceSlot::Detached`]
    /// and the drawn state stays dropped. A reload requested while the frame
    /// owned the state is applied first: the restored session takes a fresh
    /// scheduler over the resolved provider, and the presented frame is
    /// marked unsettled so the pump draws the re-planned tiles.
    fn restore_surface_session(&self, mut surface_session: Box<SurfaceSession>) {
        let pending_raster_tile_reload = crate::panic_boundary::state_lock_guard(
            &self.pending_raster_tile_reload,
            "the surface view's pending raster-tile reload",
        )
        .take();
        let mut slot_state =
            crate::panic_boundary::state_lock_guard(&self.surface_slot, "the surface view's slot");
        if !matches!(*slot_state, SurfaceSlot::Busy) {
            return;
        }
        if let Some(raster_tile_provider) = pending_raster_tile_reload {
            surface_session.replace_raster_tile_scheduler(raster_tile_provider);
            *crate::panic_boundary::state_lock_guard(
                &self.last_drawn_frame,
                "the surface view's last drawn frame",
            ) = None;
        }
        *slot_state = SurfaceSlot::Idle(surface_session);
    }

    /// Restores the drawn scheduler into the slot and applies the base-map
    /// switch a call requested while the draw owned it: a pending clear
    /// leaves the slot absent and drops what the draw borrowed, a pending
    /// install replaces the drawn scheduler and its summary. The pending
    /// change is taken in the same step, so it can never reach a later draw.
    /// Without a pending change the drawn scheduler is written back, but only
    /// while the slot still expects it.
    fn restore_vector_base_scheduler(
        &self,
        vector_base_scheduler: Option<Box<VectorBaseScheduler>>,
    ) {
        let mut slot_state = crate::panic_boundary::state_lock_guard(
            &self.vector_base,
            "the surface view's vector base",
        );
        if !matches!(*slot_state, VectorBaseSlot::Busy) {
            return;
        }
        match crate::panic_boundary::state_lock_guard(
            &self.pending_vector_base_change,
            "the surface view's pending vector base change",
        )
        .take()
        {
            Some(PendingVectorBaseChange::Clear) => {
                *slot_state = VectorBaseSlot::Absent;
                *crate::panic_boundary::state_lock_guard(
                    &self.vector_base_summary,
                    "the surface view's vector base summary",
                ) = None;
                *crate::panic_boundary::state_lock_guard(
                    &self.last_vector_base_frame,
                    "the surface view's last vector base frame",
                ) = None;
                self.map_sdk
                    .clear_provider_selection(crate::records::ProviderContract::VectorTiles);
            }
            Some(PendingVectorBaseChange::Install(
                pending_vector_base_scheduler,
                pending_source_summary,
            )) => {
                self.map_sdk.record_provider_selection(
                    crate::records::ProviderContract::VectorTiles,
                    &pending_source_summary.source_name,
                    &pending_source_summary.source_name,
                );
                *crate::panic_boundary::state_lock_guard(
                    &self.vector_base_summary,
                    "the surface view's vector base summary",
                ) = Some(pending_source_summary);
                *crate::panic_boundary::state_lock_guard(
                    &self.last_vector_base_frame,
                    "the surface view's last vector base frame",
                ) = None;
                *slot_state = VectorBaseSlot::Installed(pending_vector_base_scheduler);
            }
            None => {
                *slot_state = match vector_base_scheduler {
                    Some(vector_base_scheduler) => VectorBaseSlot::Installed(vector_base_scheduler),
                    None => VectorBaseSlot::Absent,
                };
            }
        }
    }
}

/// Owns the surface session and vector base scheduler for one frame and
/// restores them into the view's shared slots when the frame ends.
///
/// The guard runs *inside* the spawned frame work, so the ownership transfer
/// survives every way a frame can end: a completed frame, an early error, a
/// racing detach (which leaves the slot honestly [`SurfaceSlot::Detached`]),
/// and a panic that unwinds the work. The caller's future may be dropped
/// without the frame stranding the slot [`SurfaceSlot::Busy`]; the spawned
/// work still runs to completion and drops this guard.
struct FrameOwnership {
    /// The shared slots the restore writes into.
    shared: FrameSharedState,
    /// The borrowed session; taken by the restore so a second call is a
    /// no-op.
    surface_session: Option<Box<SurfaceSession>>,
    /// The borrowed vector base scheduler (`None`: no base was installed).
    vector_base_scheduler: Option<Box<VectorBaseScheduler>>,
    /// Whether the restore already ran, so [`Drop`] stays idempotent.
    restored: bool,
}

impl FrameOwnership {
    /// Takes ownership of the borrowed session and scheduler for one frame.
    fn new(
        shared: FrameSharedState,
        surface_session: Box<SurfaceSession>,
        vector_base_scheduler: Option<Box<VectorBaseScheduler>>,
    ) -> Self {
        Self {
            shared,
            surface_session: Some(surface_session),
            vector_base_scheduler,
            restored: false,
        }
    }

    /// The borrowed parts the frame draws through; the guard keeps the
    /// ownership so a panic still restores them.
    fn borrowed_parts(&mut self) -> (&mut SurfaceSession, Option<&mut VectorBaseScheduler>) {
        (
            crate::panic_boundary::required_invariant(
                self.surface_session.as_deref_mut(),
                "the frame ownership guard holds the session until it restores",
            ),
            self.vector_base_scheduler.as_deref_mut(),
        )
    }

    /// Restores the session and scheduler into their slots. Idempotent, so the
    /// frame work calls it on its normal path and [`Drop`] calls it while
    /// unwinding from a panic.
    fn restore(&mut self) {
        if self.restored {
            return;
        }
        self.restored = true;
        if let Some(surface_session) = self.surface_session.take() {
            self.shared.restore_surface_session(surface_session);
        }
        self.shared
            .restore_vector_base_scheduler(self.vector_base_scheduler.take());
    }
}

impl Drop for FrameOwnership {
    /// Restores the borrowed state if the frame work did not reach its own
    /// restore, which is the panic path.
    fn drop(&mut self) {
        self.restore();
    }
}

/// The exported surface view: draws the camera-driven tiled map plus the
/// overlay list onto the attached platform surface.
pub struct MapView {
    /// The SDK state the surface resolves its raster provider through.
    map_sdk: Arc<MapSdk>,
    /// The surface state's occupancy (see [`SurfaceSlot`]). Shared with the
    /// spawned frame work, whose ownership guard restores the session.
    surface_slot: Arc<Mutex<SurfaceSlot>>,
    /// The overlay state drawn over the tiles in every following frame: the
    /// bulk list supplied through `set_overlays` plus the individually
    /// addressed overlays `add_overlay` manages, with the descriptors
    /// validated into native overlays at insertion time, so a draw never
    /// validates. Outlives attach/detach cycles.
    overlays: Mutex<OverlayStore>,
    /// The drawing buffer's device pixels per point/CSS pixel: the
    /// labels and the overlay screen sizes are point-sized by style, so the
    /// draw multiplies both by this ratio. Outlives attach/detach cycles like
    /// the overlay list; the platform shell reports it on attach, resize and
    /// after an SDK swap.
    device_pixel_ratio: Mutex<f64>,
    /// The pose the view holds and the attached viewport it frames; see
    /// [`ViewCameraState`]. The viewport clears with detach, matching the
    /// platform wrappers' own viewport readback.
    camera_state: Mutex<ViewCameraState>,
    /// The renderer-owned location layer drawn as the frame's third pass
    /// (after tiles and overlays); `None` until
    /// `set_location_indicator` supplies state. Outlives attach/detach
    /// cycles like the overlay list.
    location_indicator: Mutex<Option<LocationIndicatorLayer>>,
    /// The presented state the idle-settled discipline compares against
    /// (`is_frame_settled`'s inputs), rewritten by the spawned frame work
    /// after every completed non-failing draw.
    last_drawn_frame: Arc<Mutex<Option<LastDrawnFrame>>>,
    /// The vector base source's occupancy (see [`VectorBaseSlot`]); outlives
    /// attach/detach cycles like the overlay list. Shared with the spawned
    /// frame work, whose ownership guard restores the scheduler.
    vector_base: Arc<Mutex<VectorBaseSlot>>,
    /// What the view draws a vector base with, for the status readback while
    /// a draw holds the scheduler. A source that is only scheduled for a
    /// following frame is not written here, so the readback keeps reporting
    /// the source the frame in flight is drawing.
    vector_base_summary: Arc<Mutex<Option<VectorBaseSourceSummary>>>,
    /// The last resolved vector base frame's accounting.
    last_vector_base_frame: Arc<Mutex<Option<LastVectorBaseFrame>>>,
    /// The base-map switch a call requested while a draw owned the scheduler
    /// (see [`PendingVectorBaseChange`]), applied when the draw restores it.
    /// Only set while the slot is [`VectorBaseSlot::Busy`], and consumed by
    /// the restore, so it never reaches a later draw.
    pending_vector_base_change: Arc<Mutex<Option<PendingVectorBaseChange>>>,
    /// The raster provider a `reload_tile_source` call resolved while a draw
    /// owned the surface state, applied by the draw's restore: the following
    /// frames build their scheduler and pool from it. Only set while the slot
    /// is [`SurfaceSlot::Busy`], and consumed by the restore, so it never
    /// reaches a later draw.
    pending_raster_tile_reload: Arc<Mutex<Option<Arc<dyn RasterTileProvider>>>>,
    /// The provider-published style documents fetched so far, keyed by the
    /// provider key and the address they came from. A document is kept only
    /// after it compiled, so a repeated install of the same family costs no
    /// request and a malformed document is never remembered as usable.
    provider_style_documents: Arc<Mutex<HashMap<(String, String), String>>>,
    /// The glyph font each provider style document's symbol layers draw
    /// with, keyed by the provider key, the document address and the font
    /// stack. A font is kept only after its ranges decoded and packed, so a
    /// repeated install of one document costs no request; a failed fetch is
    /// not remembered, so the next install retries it.
    provider_label_fonts: Arc<Mutex<ProviderLabelFontCache>>,
    /// The decoded sprite atlas each provider style document's sprite
    /// resolves to, keyed by the provider key, the document address and the
    /// base address the style declares. An atlas is kept only after its index
    /// and sheet decoded, so a repeated install of one document costs no
    /// request; a failed fetch is not remembered, so the next install retries
    /// it.
    #[cfg(not(target_arch = "wasm32"))]
    provider_sprite_atlases: Arc<Mutex<ProviderSpriteAtlasCache>>,
}

/// What names one provider style document's glyph font: the provider key, the
/// document address and the font stack its symbol layers name.
type ProviderLabelFontKey = (String, String, String);

/// The glyph fonts fetched so far, keyed by what names them.
type ProviderLabelFontCache = HashMap<ProviderLabelFontKey, Arc<LabelFont>>;

/// What names one provider style document's sprite: the provider key, the
/// document address and the base address the style declares.
#[cfg(not(target_arch = "wasm32"))]
type ProviderSpriteAtlasKey = (String, String, String);

/// The sprite atlases fetched so far, keyed by what names them.
#[cfg(not(target_arch = "wasm32"))]
type ProviderSpriteAtlasCache = HashMap<ProviderSpriteAtlasKey, Arc<SpriteAtlas>>;

impl MapView {
    /// Builds the view over the SDK state it draws from.
    pub fn new(map_sdk: Arc<MapSdk>) -> Self {
        Self {
            map_sdk,
            surface_slot: Arc::new(Mutex::new(SurfaceSlot::Detached)),
            overlays: Mutex::new(OverlayStore::default()),
            device_pixel_ratio: Mutex::new(DEFAULT_DEVICE_PIXEL_RATIO),
            camera_state: Mutex::new(ViewCameraState::new()),
            location_indicator: Mutex::new(None),
            last_drawn_frame: Arc::new(Mutex::new(None)),
            vector_base: Arc::new(Mutex::new(VectorBaseSlot::Absent)),
            vector_base_summary: Arc::new(Mutex::new(None)),
            last_vector_base_frame: Arc::new(Mutex::new(None)),
            pending_vector_base_change: Arc::new(Mutex::new(None)),
            pending_raster_tile_reload: Arc::new(Mutex::new(None)),
            provider_style_documents: Arc::new(Mutex::new(HashMap::new())),
            provider_label_fonts: Arc::new(Mutex::new(HashMap::new())),
            #[cfg(not(target_arch = "wasm32"))]
            provider_sprite_atlases: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// The slot state behind its lock; the slot's guard regions hold only
    /// panic-free state swaps (the module documentation's locking
    /// discipline), so poisoning is the panic boundary's internal-invariant
    /// case (see [`crate::panic_boundary`]).
    fn surface_slot_lock(&self) -> MutexGuard<'_, SurfaceSlot> {
        crate::panic_boundary::state_lock_guard(&self.surface_slot, "the surface view's slot")
    }

    /// The overlay state behind its lock; see [`Self::surface_slot_lock`].
    fn overlays_lock(&self) -> MutexGuard<'_, OverlayStore> {
        crate::panic_boundary::state_lock_guard(&self.overlays, "the surface view's overlay state")
    }

    /// The device pixel ratio behind its lock; see [`Self::surface_slot_lock`].
    fn device_pixel_ratio_lock(&self) -> MutexGuard<'_, f64> {
        crate::panic_boundary::state_lock_guard(
            &self.device_pixel_ratio,
            "the surface view's device pixel ratio",
        )
    }

    /// The camera pose and attached viewport behind one lock; see
    /// [`Self::surface_slot_lock`].
    fn camera_state_lock(&self) -> MutexGuard<'_, ViewCameraState> {
        crate::panic_boundary::state_lock_guard(
            &self.camera_state,
            "the surface view's camera state",
        )
    }

    /// The location layer behind its lock; see [`Self::surface_slot_lock`].
    fn location_indicator_lock(&self) -> MutexGuard<'_, Option<LocationIndicatorLayer>> {
        crate::panic_boundary::state_lock_guard(
            &self.location_indicator,
            "the surface view's location layer",
        )
    }

    /// The presented frame behind its lock; see [`Self::surface_slot_lock`].
    fn last_drawn_frame_lock(&self) -> MutexGuard<'_, Option<LastDrawnFrame>> {
        crate::panic_boundary::state_lock_guard(
            &self.last_drawn_frame,
            "the surface view's last drawn frame",
        )
    }

    /// The vector base slot behind its lock; see [`Self::surface_slot_lock`].
    fn vector_base_lock(&self) -> MutexGuard<'_, VectorBaseSlot> {
        crate::panic_boundary::state_lock_guard(&self.vector_base, "the surface view's vector base")
    }

    /// The vector base source summary behind its lock; see
    /// [`Self::surface_slot_lock`].
    fn vector_base_summary_lock(&self) -> MutexGuard<'_, Option<VectorBaseSourceSummary>> {
        crate::panic_boundary::state_lock_guard(
            &self.vector_base_summary,
            "the surface view's vector base summary",
        )
    }

    /// The last resolved vector base frame behind its lock; see
    /// [`Self::surface_slot_lock`].
    fn last_vector_base_frame_lock(&self) -> MutexGuard<'_, Option<LastVectorBaseFrame>> {
        crate::panic_boundary::state_lock_guard(
            &self.last_vector_base_frame,
            "the surface view's last vector base frame",
        )
    }

    /// The pending base-map change behind its lock; see
    /// [`Self::surface_slot_lock`].
    fn pending_vector_base_change_lock(&self) -> MutexGuard<'_, Option<PendingVectorBaseChange>> {
        crate::panic_boundary::state_lock_guard(
            &self.pending_vector_base_change,
            "the surface view's pending vector base change",
        )
    }

    /// The pending raster-tile reload behind its lock; see
    /// [`Self::surface_slot_lock`].
    fn pending_raster_tile_reload_lock(
        &self,
    ) -> MutexGuard<'_, Option<Arc<dyn RasterTileProvider>>> {
        crate::panic_boundary::state_lock_guard(
            &self.pending_raster_tile_reload,
            "the surface view's pending raster-tile reload",
        )
    }

    /// Reads the pose the view holds (`camera`'s documented UDL method). The
    /// pose starts neutral and every accepted set, fit or transform updates
    /// it.
    pub fn camera(&self) -> Arc<MapCamera> {
        Arc::new(self.camera_state_lock().camera)
    }

    /// Adopts the pose every following readback reports (`set_camera`'s
    /// documented UDL method). The pose is already validated by its
    /// constructor, so the call cannot fail and changes nothing else.
    #[allow(clippy::needless_pass_by_value)]
    pub fn set_camera(&self, camera: Arc<MapCamera>) {
        self.camera_state_lock().camera = *camera;
    }

    /// Frames the box inside the attached viewport's inset region and adopts
    /// the fitted camera (`fit_bounds`' documented UDL method). Answers
    /// `None`, changing nothing, while no viewport is attached.
    pub fn fit_bounds(
        &self,
        bounding_box: BoundingBox,
        edge_insets: EdgeInsets,
        maximum_zoom_level: f64,
    ) -> Result<Option<Arc<MapCamera>>, SdkError> {
        let Some(viewport) = self.camera_state_lock().viewport else {
            return Ok(None);
        };
        let fitted_camera =
            MapCamera::fit_bounds(bounding_box, viewport, edge_insets, maximum_zoom_level)?;
        self.camera_state_lock().camera = fitted_camera;
        Ok(Some(Arc::new(fitted_camera)))
    }

    /// The attached viewport's geographic bounds at the current pose
    /// (`camera_geographic_bounds`' documented UDL method); `None` while no
    /// viewport is attached or the projection cannot answer.
    pub fn camera_geographic_bounds(&self) -> Option<BoundingBox> {
        let camera_state = self.camera_state_lock();
        let viewport = camera_state.viewport?;
        geographic_bounds_of_camera(&camera_state.camera, viewport)
    }

    /// Attaches the platform surface (`attach`'s documented UDL method),
    /// building the whole drawing machinery — surface target, renderer,
    /// configured extent, tile scheduler — before installing it, so a busy
    /// slot races nothing. Attaching an attached view is refused.
    pub fn attach(
        &self,
        surface_handle: SurfaceHandle,
        width_pixels: u32,
        height_pixels: u32,
    ) -> Result<(), SdkError> {
        let surface_session =
            self.build_surface_session(surface_handle, width_pixels, height_pixels)?;
        let mut slot_state = self.surface_slot_lock();
        match *slot_state {
            // Replacing an attached view is refused: the platform glue's
            // lifecycle is detach-then-attach.
            SurfaceSlot::Idle(_) => {
                return Err(SdkError::RenderingSurfaceUnavailable {
                    detail: "the map surface view is already attached; detach first".to_string(),
                });
            }
            // A draw holding the state will restore it; attaching mid-draw
            // loses and retries after the draw (and, if intended, a detach).
            SurfaceSlot::Busy => {
                return Err(SdkError::RenderingSurfaceUnavailable {
                    detail: "a frame draw is in flight; attach after the frame draws".to_string(),
                });
            }
            // The only installable state.
            SurfaceSlot::Detached => {}
        }
        *slot_state = SurfaceSlot::Idle(Box::new(surface_session));
        drop(slot_state);
        self.camera_state_lock().viewport = Some(ViewportSize {
            width_pixels: f64::from(width_pixels),
            height_pixels: f64::from(height_pixels),
        });
        Ok(())
    }

    /// Reconfigures the surface to the new physical-pixel extent (both
    /// non-zero). Works while a fetch is in flight — the scheduler is not
    /// touched — and surfaces the busy state only against an active render.
    pub fn resize(&self, width_pixels: u32, height_pixels: u32) -> Result<(), SdkError> {
        if width_pixels == 0 || height_pixels == 0 {
            return Err(SdkError::InvalidRenderInput {
                detail: format!(
                    "resize extent {width_pixels}x{height_pixels} must be \
                     non-zero; a platform view between layouts waits for the \
                     next laid out size"
                ),
            });
        }
        let mut slot_state = self.surface_slot_lock();
        match &mut *slot_state {
            SurfaceSlot::Idle(surface_session) => {
                surface_session
                    .surface_renderer
                    .configure_extent(width_pixels, height_pixels)
                    .map_err(|surface_error| rendering_failure_to_sdk_error(&surface_error))?;
                surface_session.width_pixels = width_pixels;
                surface_session.height_pixels = height_pixels;
                drop(slot_state);
                self.camera_state_lock().viewport = Some(ViewportSize {
                    width_pixels: f64::from(width_pixels),
                    height_pixels: f64::from(height_pixels),
                });
                Ok(())
            }
            SurfaceSlot::Busy => Err(SdkError::RenderingSurfaceUnavailable {
                detail: "a frame draw is in flight; resize after the frame draws".to_string(),
            }),
            SurfaceSlot::Detached => Err(detached_surface_error()),
        }
    }

    /// Detaches (`detach`'s documented UDL method): releases the surface
    /// state until the next attach. Idempotent, safe against a draw still
    /// in flight — the draw's restore honors the detached slot and drops
    /// the state without reinstalling it (see [`SurfaceSlot::Busy`]).
    pub fn detach(&self) {
        let mut slot_state = self.surface_slot_lock();
        *slot_state = SurfaceSlot::Detached;
        drop(slot_state);
        self.camera_state_lock().viewport = None;
    }

    /// Records the drawing buffer's device pixels per point/CSS pixel
    /// (`set_device_pixel_ratio`'s documented UDL method): the labels and
    /// the overlay screen sizes are declared in points by the committed
    /// styles, so the draw multiplies both by this ratio and they keep their
    /// declared point size on a 2× or 3× surface. The ratio outlives
    /// attach/detach cycles like the overlay list; a non-finite or
    /// non-positive ratio is refused with `InvalidRenderInput` and leaves the
    /// previous ratio in force.
    pub fn set_device_pixel_ratio(&self, device_pixel_ratio: f64) -> Result<(), SdkError> {
        if !device_pixel_ratio.is_finite() || device_pixel_ratio <= 0.0 {
            return Err(SdkError::InvalidRenderInput {
                detail: format!(
                    "device pixel ratio {device_pixel_ratio} must be finite and \
                     positive; the platform shell reports the drawing buffer's \
                     pixels per point"
                ),
            });
        }
        *self.device_pixel_ratio_lock() = device_pixel_ratio;
        Ok(())
    }

    /// The stored device pixel ratio; [`DEFAULT_DEVICE_PIXEL_RATIO`] until the
    /// platform shell reports the surface's own.
    fn stored_device_pixel_ratio(&self) -> f64 {
        *self.device_pixel_ratio_lock()
    }

    /// Replaces the drawn overlay list in one call. Validating the whole
    /// list first keeps the previous list in force when any descriptor is
    /// invalid — the wasm canvas' all-or-nothing semantics. This bulk call
    /// replaces the whole drawn list, including overlays added through
    /// `add_overlay`, and drops the addressed bookkeeping (the UDL's
    /// documented interaction): identifiers returned before this call no
    /// longer address anything.
    pub fn set_overlays(&self, overlay_descriptors: Vec<MapOverlay>) -> Result<(), SdkError> {
        let mut validated_overlays = Vec::with_capacity(overlay_descriptors.len());
        for (overlay_index, overlay_descriptor) in overlay_descriptors.into_iter().enumerate() {
            validated_overlays.push(
                overlay_descriptor
                    .into_native_overlay()
                    .map_err(|failure| invalid_overlay_entry(failure, overlay_index))?,
            );
        }
        self.overlays_lock().replace_all(validated_overlays);
        Ok(())
    }

    /// Adds one overlay and answers the generated identifier that addresses
    /// it. Validation is `set_overlays`' all-or-nothing validation for the
    /// single descriptor: a refused descriptor throws, adds nothing and
    /// consumes no identifier.
    pub fn add_overlay(&self, overlay: MapOverlay) -> Result<String, SdkError> {
        let native_overlay = overlay
            .into_native_overlay()
            .map_err(invalid_added_overlay)?;
        Ok(self.overlays_lock().add(native_overlay))
    }

    /// Replaces the overlay the identifier addresses, keeping its draw
    /// position. Validating the descriptor before touching the store keeps
    /// the previously addressed overlay in force when the descriptor is
    /// invalid; an identifier that addresses nothing throws with a precise
    /// detail.
    pub fn update_overlay(
        &self,
        overlay_identifier: String,
        overlay: MapOverlay,
    ) -> Result<(), SdkError> {
        let native_overlay = overlay
            .into_native_overlay()
            .map_err(invalid_added_overlay)?;
        self.overlays_lock()
            .update(&overlay_identifier, native_overlay)
    }

    /// Removes the overlay the identifier addresses and answers whether that
    /// identifier was present. An unknown identifier is not an error.
    pub fn remove_overlay(&self, overlay_identifier: String) -> bool {
        self.overlays_lock().remove(&overlay_identifier)
    }

    /// Clears the whole drawn overlay list — the bulk list and every
    /// addressed overlay — so following frames draw the tiles alone until
    /// `set_overlays` or `add_overlay` supplies new content.
    pub fn clear_overlays(&self) {
        self.overlays_lock().clear();
    }

    /// Sets the single location layer drawn as the frame's third pass
    /// (`set_location_indicator`'s documented UDL method), replacing any
    /// earlier state. Validating both records before the store keeps the
    /// earlier indicator in force when either is invalid — `set_overlays`'
    /// all-or-nothing semantics.
    pub fn set_location_indicator(
        &self,
        state: LocationIndicatorState,
        style: LocationIndicatorStyle,
    ) -> Result<(), SdkError> {
        let validated_state = state.into_native_location_indicator_state()?;
        let validated_style = style.into_native_location_indicator_style()?;
        *self.location_indicator_lock() = Some(LocationIndicatorLayer::new(
            validated_state,
            validated_style,
        ));
        Ok(())
    }

    /// Clears the location layer; following frames draw tiles and overlays
    /// alone until `set_location_indicator` supplies new state.
    pub fn clear_location_indicator(&self) {
        *self.location_indicator_lock() = None;
    }

    /// Reads whether the view's presented state would not change if another
    /// frame drew right now (`is_frame_settled`'s documented UDL method): the
    /// camera equals the last drawn camera exactly, the overlay list carries
    /// the revision the last frame drew, and the location layer, device pixel
    /// ratio and vector base source are unchanged since that draw, and the
    /// last frame resolved no pending pixels (raster or vector base). Any
    /// overlay mutation since the last draw marks the frame unsettled, even
    /// when it restores equal content, until the next frame draws. A pure
    /// query — it never draws, fetches or mutates state. It answers `false`
    /// while a draw is in flight (the presented state is then changing) and
    /// before a frame has ever drawn, and throws when the view is detached.
    pub fn is_frame_settled(&self, camera: Arc<MapCamera>) -> Result<bool, SdkError> {
        match &*self.surface_slot_lock() {
            SurfaceSlot::Detached => {
                return Err(SdkError::RenderingSurfaceUnavailable {
                    detail: SURFACE_DETACHED_DETAIL.to_string(),
                });
            }
            SurfaceSlot::Busy => return Ok(false),
            SurfaceSlot::Idle(_) => {}
        }
        let Some(last_drawn_frame) = self.last_drawn_frame_lock().clone() else {
            // No frame has drawn yet: the presented state is whatever the
            // platform left, and the first draw would change it.
            return Ok(false);
        };
        let drawn_overlay_revision = self.overlays_lock().revision();
        let location_indicator = self.location_indicator_lock().clone();
        let vector_base_source = self.vector_base_summary_lock().clone();
        if last_drawn_frame.camera != *camera.native_camera()
            || drawn_overlay_revision != last_drawn_frame.overlay_revision
            || location_indicator != last_drawn_frame.location_indicator
            || self.stored_device_pixel_ratio() != last_drawn_frame.device_pixel_ratio
            || vector_base_source != last_drawn_frame.vector_base_source
        {
            return Ok(false);
        }
        Ok(!last_drawn_frame.drawn_pixels_may_still_change())
    }

    /// Re-resolves the raster-tile provider and drops the scheduler's pooled
    /// raster state (`reload_tile_source`'s documented UDL method), so a
    /// provider, credential or source change reaches the following frames
    /// without an attach/detach cycle.
    ///
    /// The next draw builds a fresh scheduler and payload pool over the
    /// current provider: pooled payloads, in-flight warm fetches and the
    /// retained cross-fade level from the previous source are dropped, so no
    /// tile fetched from it can draw again, and the frame reports its
    /// previously pooled tiles as missing while it re-plans them. Overlays,
    /// the location layer, the camera, the device pixel ratio and the vector
    /// base survive the reload. The view is left unsettled so the platform's
    /// pump draws the re-planned frame.
    ///
    /// A call made while a frame draws is applied when that frame finishes,
    /// and the following frames use the new provider; a later call replaces
    /// the earlier one. While detached there is no pooled state to clear and
    /// the next attach resolves the current provider, so the call succeeds
    /// and changes nothing.
    ///
    /// # Errors
    /// Throws `ProviderNotConfigured` when the view is attached and no
    /// raster-tile provider is registered; the previous source stays in
    /// force.
    pub fn reload_tile_source(&self) -> Result<(), SdkError> {
        // The pending slot is taken before the surface slot, the same order
        // the frame restore uses, so a reload racing a restore can never
        // deadlock on the two locks.
        let mut pending_raster_tile_reload = self.pending_raster_tile_reload_lock();
        let mut slot_state = self.surface_slot_lock();
        match &*slot_state {
            SurfaceSlot::Detached => return Ok(()),
            SurfaceSlot::Busy | SurfaceSlot::Idle(_) => {}
        }
        let raster_tile_provider = self.map_sdk.required_raster_tile_provider()?;
        match &mut *slot_state {
            SurfaceSlot::Busy => {
                *pending_raster_tile_reload = Some(raster_tile_provider);
            }
            SurfaceSlot::Idle(surface_session) => {
                surface_session.replace_raster_tile_scheduler(raster_tile_provider);
                // The presented pixels will change when the re-planned tiles
                // arrive, so no earlier frame can answer settled.
                *self.last_drawn_frame_lock() = None;
            }
            SurfaceSlot::Detached => unreachable!("the detached slot returned above"),
        }
        Ok(())
    }

    /// Installs a vector base map (`set_vector_tile_source`'s documented UDL
    /// method): the source's URL template, descriptor and style id build
    /// the shared [`VectorBaseScheduler`] the draw resolves every frame, and
    /// replace any base already installed. The whole source builds before the
    /// slot moves, so a refused call (a malformed template, an inverted zoom
    /// range or an unknown style id) leaves the previously installed base in
    /// force. Installing while a draw is in flight succeeds and is deferred:
    /// the draw's restore installs it, and the following frame draws the new
    /// base.
    pub fn set_vector_tile_source(
        &self,
        source: UrlTemplateVectorTileSource,
    ) -> Result<(), SdkError> {
        let (provider_source, style_id) = source.into_provider_source()?;
        let source_descriptor = provider_source.source_descriptor.clone();
        let url_template_provider =
            everymap_providers_url_template_vector::UrlTemplateVectorTileProvider::new(
                provider_source,
            )
            .map_err(|error| {
                SdkError::from(rustic_maps_core::everymap_bridge::map_provider_error(error))
            })?;
        let vector_base_scheduler = self.vector_base_scheduler_from_provider(
            Arc::new(
                rustic_maps_core::everymap_bridge::EveryMapVectorTileProvider::new(Arc::new(
                    url_template_provider,
                )),
            ),
            &style_id,
            &source_descriptor.source_name,
        )?;
        let source_summary =
            VectorBaseSourceSummary::from_scheduler(style_id, &vector_base_scheduler);
        self.install_vector_base_scheduler(
            vector_base_scheduler,
            source_summary,
            Some((
                source_descriptor.source_name.clone(),
                source_descriptor.source_name,
            )),
        )
    }

    /// Installs a vector base map served by a provider configured through
    /// `MapSdk::configure_provider_set` (`set_provider_vector_tile_source`'s
    /// documented UDL method). The provider's descriptor supplies the
    /// tileset name, attribution and zoom span; the source record chooses the
    /// provider and the committed style the base paints with. Refusals (no
    /// vector-tile provider, a record naming another provider, or an unknown
    /// style) leave any installed base in force, and an install landing while
    /// a frame draws is deferred exactly like the URL-template path.
    pub fn set_provider_vector_tile_source(
        &self,
        source: ProviderVectorTileSource,
    ) -> Result<(), SdkError> {
        if let Some((installed_provider_key, _)) = self
            .map_sdk
            .selected_provider(crate::records::ProviderContract::VectorTiles)
            && installed_provider_key != source.provider_key
        {
            return Err(SdkError::ProviderNotConfigured {
                contract_name: format!(
                    "vector tiles from provider {}; the installed vector-tile provider is {}",
                    source.provider_key, installed_provider_key
                ),
            });
        }
        let vector_tile_provider = self.map_sdk.required_vector_tile_provider()?;
        let source_descriptor = vector_tile_provider.source_descriptor();
        let vector_base_scheduler = self.vector_base_scheduler_from_provider(
            vector_tile_provider,
            &source.style_id,
            &source_descriptor.source_name,
        )?;
        let source_summary =
            VectorBaseSourceSummary::from_scheduler(source.style_id, &vector_base_scheduler);
        // The provider was recorded when it was configured, with the display
        // name its descriptor carries; re-recording here would replace that
        // name with the tileset name.
        self.install_vector_base_scheduler(vector_base_scheduler, source_summary, None)
    }

    /// Installs a vector base map served by a configured provider and painted
    /// with the style document the provider publishes
    /// (`set_provider_vector_tile_source_from_document`'s documented UDL
    /// method).
    ///
    /// The document is fetched through the provider's authenticated
    /// style-resource fetch, so the credential stays in the provider client
    /// and the request carries the provider's own transport, pacing and retry
    /// policy. A document that compiled is kept per provider and address, so
    /// installing the same family again costs no request; a document that
    /// fails to fetch, decode or compile throws and leaves the previous base
    /// in force.
    ///
    /// # Parameters
    /// - `source` — the configured provider key and the style label the
    ///   status reports.
    /// - `style_document_url` — the absolute address of the provider's style
    ///   document, without a credential.
    ///
    /// # Errors
    /// Throws when no vector-tile provider serves the requested key, when the
    /// provider registers no style-resource fetch, when the request fails, or
    /// when the served document is not a compilable style.
    pub async fn set_provider_vector_tile_source_from_document(
        &self,
        source: ProviderVectorTileSource,
        style_document_url: String,
    ) -> Result<(), SdkError> {
        if let Some((installed_provider_key, _)) = self
            .map_sdk
            .selected_provider(crate::records::ProviderContract::VectorTiles)
            && installed_provider_key != source.provider_key
        {
            return Err(SdkError::ProviderNotConfigured {
                contract_name: format!(
                    "vector tiles from provider {}; the installed vector-tile provider is {}",
                    source.provider_key, installed_provider_key
                ),
            });
        }
        let vector_tile_provider = self.map_sdk.required_vector_tile_provider()?;
        let style_resources = self.style_resource_client(&source.provider_key)?;
        let source_descriptor = vector_tile_provider.source_descriptor();
        let style_document = match self
            .cached_provider_style_document(&source.provider_key, &style_document_url)
        {
            Some(style_document) => style_document,
            None => {
                let document_url = style_document_url.clone();
                let document_bytes =
                    drive(async move { style_resources.fetch_style_resource(&document_url).await })
                        .await?;
                String::from_utf8(document_bytes).map_err(|utf8_error| {
                    SdkError::ResponseParsingFailure {
                        detail: format!(
                            "the style document the vector base source {} served is not UTF-8 \
                             text: {utf8_error}",
                            source_descriptor.source_name
                        ),
                    }
                })?
            }
        };
        let vector_base_scheduler = VectorBaseScheduler::from_style_document_json(
            vector_tile_provider,
            &source.style_id,
            &style_document,
        )
        .map_err(|style_error| SdkError::ResponseParsingFailure {
            detail: format!(
                "the style document the vector base source {} served does not compile: \
                     {style_error}",
                source_descriptor.source_name
            ),
        })?;
        // The style's own labels draw through the glyph endpoint it declares,
        // fetched with the provider's credential. A style that declares no
        // endpoint, or whose ranges the install could not complete, keeps the
        // embedded Latin range; a glyph address that resolves off the
        // document's origin refuses the install before any request.
        let vector_base_scheduler = match self
            .hosted_label_font(
                &source.provider_key,
                &style_document_url,
                &vector_base_scheduler,
            )
            .await?
        {
            Some(label_font) => vector_base_scheduler.with_label_font(label_font),
            None => vector_base_scheduler,
        };
        // The style's own icons draw from the sprite it declares, fetched the
        // same way. The renderer's sprite seam is synchronous, so the install
        // prefetches and hands over the decoded atlas; a style without a
        // sprite, or one the install could not complete, draws its text
        // without icons, and an off-origin sprite base refuses the install.
        #[cfg(not(target_arch = "wasm32"))]
        let vector_base_scheduler = match self
            .hosted_sprite_atlas(
                &source.provider_key,
                &style_document_url,
                &vector_base_scheduler,
            )
            .await?
        {
            Some((sprite_url, sprite_atlas)) => {
                vector_base_scheduler.with_prefetched_sprite_atlas(sprite_url, sprite_atlas)
            }
            None => vector_base_scheduler,
        };
        // Remember the document only after it compiled, so a malformed
        // document is fetched again rather than remembered as usable.
        self.remember_provider_style_document(
            source.provider_key.clone(),
            style_document_url,
            style_document,
        );
        let vector_base_scheduler =
            self.vector_base_scheduler_with_payload_cache(vector_base_scheduler);
        let source_summary =
            VectorBaseSourceSummary::from_scheduler(source.style_id, &vector_base_scheduler);
        self.install_vector_base_scheduler(vector_base_scheduler, source_summary, None)
    }

    /// The compiled-once style document a provider and address served
    /// earlier, when one is cached.
    fn cached_provider_style_document(
        &self,
        provider_key: &str,
        style_document_url: &str,
    ) -> Option<String> {
        self.provider_style_documents_lock()
            .get(&(provider_key.to_string(), style_document_url.to_string()))
            .cloned()
    }

    /// Keeps a provider's style document after it compiled.
    fn remember_provider_style_document(
        &self,
        provider_key: String,
        style_document_url: String,
        style_document: String,
    ) {
        self.provider_style_documents_lock()
            .insert((provider_key, style_document_url), style_document);
    }

    /// The cached-document lock; its guard regions hold only map reads and
    /// writes, so poisoning is the panic boundary's internal-invariant case.
    fn provider_style_documents_lock(&self) -> MutexGuard<'_, HashMap<(String, String), String>> {
        crate::panic_boundary::state_lock_guard(
            &self.provider_style_documents,
            "the view's provider style documents",
        )
    }

    /// The glyph font a provider's style document names for its symbol
    /// layers.
    ///
    /// The font is built from the style's `glyphs` endpoint over the Latin
    /// blocks the label pass packs, fetched through the provider's
    /// authenticated style-resource client and cached per provider, document
    /// address and font stack.
    ///
    /// # Parameters
    /// - `provider_key` — the provider the document came from, which names
    ///   the cached font and the glyph requests' namespace.
    /// - `style_document_url` — the document's address, whose origin bounds
    ///   every glyph request.
    /// - `vector_base_scheduler` — the scheduler built from the compiled
    ///   document, whose endpoint and symbol font stacks the font is built
    ///   from.
    ///
    /// # Returns
    /// The packed font, or `None` when the style names no glyph endpoint or
    /// no symbol font stack, when the address cannot be fetched as written,
    /// and when the ranges could not be fetched, decoded or packed — the
    /// labels then draw through the SDK's embedded Latin range.
    ///
    /// An address the SDK cannot turn into a fetchable template is no
    /// credential risk, so it degrades rather than refuses: a vendor may name
    /// its glyphs in its own URI scheme, and a family must not lose its base
    /// over an address the SDK cannot fetch.
    ///
    /// # Errors
    /// Throws when no style-resource fetch is registered, and when the
    /// style's glyph address resolves to an origin other than the document's:
    /// a style document must not be able to aim the provider credential at a
    /// host the provider did not declare, so the install is refused before
    /// any request.
    async fn hosted_label_font(
        &self,
        provider_key: &str,
        style_document_url: &str,
        vector_base_scheduler: &VectorBaseScheduler,
    ) -> Result<Option<Arc<LabelFont>>, SdkError> {
        let style = vector_base_scheduler.style();
        let Some(glyph_url_template) = style.glyphs.as_deref() else {
            return Ok(None);
        };
        let Some(font_stack_name) = style.symbol_font_stacks().into_iter().next() else {
            return Ok(None);
        };
        let cache_key = (
            provider_key.to_string(),
            style_document_url.to_string(),
            font_stack_name.clone(),
        );
        if let Some(cached_label_font) = self.provider_label_fonts_lock().get(&cache_key) {
            return Ok(Some(Arc::clone(cached_label_font)));
        }
        let style_resources = self.style_resource_client(provider_key)?;
        let declared_hosts = self.map_sdk.vector_tile_style_resource_hosts();
        let mut hosted_glyph_ranges = match HostedGlyphRanges::new(
            style_resources,
            provider_key,
            style_document_url,
            glyph_url_template,
            &declared_hosts,
        ) {
            Ok(hosted_glyph_ranges) => hosted_glyph_ranges,
            Err(geospatial_types::SDKError::ResponseParsingFailure(_)) => return Ok(None),
            Err(refusal) => return Err(refusal.into()),
        };
        let fetched_font_stack =
            drive(async move { hosted_glyph_ranges.latin_font_stack(&font_stack_name).await })
                .await;
        let Ok(font_stack) = fetched_font_stack else {
            return Ok(None);
        };
        let Ok(label_font) = LabelFont::from_glyph_font_stack(&font_stack) else {
            return Ok(None);
        };
        let label_font = Arc::new(label_font);
        self.provider_label_fonts_lock()
            .insert(cache_key, Arc::clone(&label_font));
        Ok(Some(label_font))
    }

    /// The cached-label-font lock; its guard regions hold only map reads and
    /// writes, so poisoning is the panic boundary's internal-invariant case.
    fn provider_label_fonts_lock(&self) -> MutexGuard<'_, ProviderLabelFontCache> {
        crate::panic_boundary::state_lock_guard(
            &self.provider_label_fonts,
            "the view's provider label fonts",
        )
    }

    /// The style's sprite atlas, decoded once per provider, document and base
    /// address.
    ///
    /// Returns `None` when the style declares no sprite, when its base
    /// address cannot be fetched as written, and when the index or sheet
    /// could not be fetched or decoded — the icons are then skipped and the
    /// labels still draw.
    ///
    /// # Errors
    /// Throws when no style-resource fetch is registered, and when the
    /// sprite's resolved addresses lie outside the style document's declared
    /// origin: a style document must not be able to aim the provider
    /// credential at a host the provider did not declare, so the install is
    /// refused before any request.
    #[cfg(not(target_arch = "wasm32"))]
    async fn hosted_sprite_atlas(
        &self,
        provider_key: &str,
        style_document_url: &str,
        vector_base_scheduler: &VectorBaseScheduler,
    ) -> Result<Option<(String, Arc<SpriteAtlas>)>, SdkError> {
        let Some(sprite) = vector_base_scheduler.style().sprite.as_ref() else {
            return Ok(None);
        };
        let sprite_url = sprite.url.clone();
        let cache_key = (
            provider_key.to_string(),
            style_document_url.to_string(),
            sprite_url.clone(),
        );
        if let Some(cached_sprite_atlas) = self.provider_sprite_atlases_lock().get(&cache_key) {
            return Ok(Some((sprite_url, Arc::clone(cached_sprite_atlas))));
        }
        let style_resources = self.style_resource_client(provider_key)?;
        let declared_hosts = self.map_sdk.vector_tile_style_resource_hosts();
        let fetched_sprite_atlas = fetch_hosted_sprite_atlas(
            style_resources,
            style_document_url,
            &sprite_url,
            &declared_hosts,
        )
        .await;
        let sprite_atlas = match fetched_sprite_atlas {
            Ok(sprite_atlas) => Arc::new(sprite_atlas),
            Err(geospatial_types::SDKError::ProviderRequestRejected(detail)) => {
                return Err(SdkError::ProviderRequestRejected { detail });
            }
            Err(_) => return Ok(None),
        };
        self.provider_sprite_atlases_lock()
            .insert(cache_key, Arc::clone(&sprite_atlas));
        Ok(Some((sprite_url, sprite_atlas)))
    }

    /// The cached-sprite-atlas lock; its guard regions hold only map reads
    /// and writes, so poisoning is the panic boundary's internal-invariant
    /// case.
    #[cfg(not(target_arch = "wasm32"))]
    fn provider_sprite_atlases_lock(&self) -> MutexGuard<'_, ProviderSpriteAtlasCache> {
        crate::panic_boundary::state_lock_guard(
            &self.provider_sprite_atlases,
            "the view's provider sprite atlases",
        )
    }

    /// Builds the scheduler a vector base draws through: the provider adapted
    /// to the rendering seam and the committed style it paints with. The
    /// vector payloads share the installed disk tile cache, so a region
    /// browsed once is served from disk on the next session under the
    /// vector-tiles category's thirty-day policy, and the cache's quota and
    /// LRU bound both payload kinds together. No installed cache (a keyless
    /// build) leaves the base on the network-only path.
    fn vector_base_scheduler_from_provider(
        &self,
        vector_tile_provider: Arc<dyn geospatial_traits::VectorTileProvider>,
        style_id: &str,
        source_name: &str,
    ) -> Result<VectorBaseScheduler, SdkError> {
        let vector_base_scheduler =
            VectorBaseScheduler::from_style_id(vector_tile_provider, style_id).map_err(
                |style_error| SdkError::InvalidRenderInput {
                    detail: format!(
                        "the vector base source {source_name} names a style the SDK does not \
                         carry: {style_error}"
                    ),
                },
            )?;
        Ok(self.vector_base_scheduler_with_payload_cache(vector_base_scheduler))
    }

    /// Attaches the shared vector payload cache to a scheduler built for a
    /// provider-backed base, so a region browsed once is served from disk on
    /// the next session under the vector-tiles category's thirty-day policy.
    /// The style-resource fetch a provider-document install uses: the
    /// provider's authenticated fetch, wrapped in the tile cache's disk store
    /// when one is installed, so a reopened application serves the document,
    /// its glyph ranges and its sprite sheet under the terms' thirty-day
    /// window instead of requesting them again. Without an installed cache the
    /// fetch goes straight to the provider.
    ///
    /// # Parameters
    /// - `provider_key` — the provider the resources belong to, which
    ///   namespaces the cached entries.
    ///
    /// # Errors
    /// Throws when no style-resource fetch is registered for the serving
    /// provider.
    #[cfg(not(target_arch = "wasm32"))]
    fn style_resource_client(
        &self,
        provider_key: &str,
    ) -> Result<Arc<dyn rustic_maps_core::StyleResourceClient>, SdkError> {
        let style_resources = self.map_sdk.required_vector_tile_style_resources()?;
        match self.map_sdk.style_resource_cache_store() {
            Some(response_store) => {
                Ok(Arc::new(rustic_maps_cache::CachedStyleResourceClient::new(
                    style_resources,
                    response_store,
                    provider_key,
                    rustic_maps_cache::ResponseCategory::StyleResources
                        .default_time_to_live_seconds(),
                    crate::runtime::blocking_work_executor(),
                )))
            }
            None => Ok(style_resources),
        }
    }

    fn vector_base_scheduler_with_payload_cache(
        &self,
        vector_base_scheduler: VectorBaseScheduler,
    ) -> VectorBaseScheduler {
        #[cfg(not(target_arch = "wasm32"))]
        match self.map_sdk.vector_tile_payload_cache_store() {
            Some(payload_cache) => vector_base_scheduler.with_payload_cache(
                payload_cache,
                rustic_maps_cache::ResponseCategory::VectorTiles.default_time_to_live_seconds(),
            ),
            None => vector_base_scheduler,
        }
        #[cfg(target_arch = "wasm32")]
        vector_base_scheduler
    }

    /// Writes an installed scheduler and its summary into the view, deferring
    /// to the drawing frame when one owns the slot. `selection` names the
    /// provider to record for a source that is not itself a configured
    /// provider; the configured path passes `None`.
    fn install_vector_base_scheduler(
        &self,
        vector_base_scheduler: VectorBaseScheduler,
        source_summary: VectorBaseSourceSummary,
        selection: Option<(String, String)>,
    ) -> Result<(), SdkError> {
        let mut slot_state = self.vector_base_lock();
        if matches!(*slot_state, VectorBaseSlot::Busy) {
            // A frame owns the scheduler for its duration: remember the
            // install so the draw's restore applies it, replacing any earlier
            // pending change. The status keeps reporting the source being
            // drawn until then, because the summary is not written here.
            *self.pending_vector_base_change_lock() = Some(PendingVectorBaseChange::Install(
                Box::new(vector_base_scheduler),
                source_summary,
            ));
            return Ok(());
        }
        if let Some((provider_key, display_name)) = selection {
            self.map_sdk.record_provider_selection(
                crate::records::ProviderContract::VectorTiles,
                &provider_key,
                &display_name,
            );
        }
        *self.vector_base_summary_lock() = Some(source_summary);
        *self.last_vector_base_frame_lock() = None;
        *slot_state = VectorBaseSlot::Installed(Box::new(vector_base_scheduler));
        Ok(())
    }

    /// Removes the vector base map (`clear_vector_tile_source`'s documented
    /// UDL method): following frames draw raster tiles and overlays
    /// alone. Like installing, clearing while a draw is in flight succeeds
    /// and is deferred: the draw's restore drops what it borrowed, and the
    /// following frame draws no vector base.
    pub fn clear_vector_tile_source(&self) -> Result<(), SdkError> {
        let mut slot_state = self.vector_base_lock();
        if matches!(*slot_state, VectorBaseSlot::Busy) {
            // A frame owns the scheduler for its duration: remember the clear
            // so the draw's restore drops what it borrowed, replacing any
            // earlier pending change.
            *self.pending_vector_base_change_lock() = Some(PendingVectorBaseChange::Clear);
            return Ok(());
        }
        *slot_state = VectorBaseSlot::Absent;
        *self.vector_base_summary_lock() = None;
        *self.last_vector_base_frame_lock() = None;
        self.map_sdk
            .clear_provider_selection(crate::records::ProviderContract::VectorTiles);
        Ok(())
    }

    /// Reads the vector base source's status (`vector_tile_source_status`'s
    /// documented UDL method): the installed source's style, name,
    /// attribution and zoom ceiling, its style fidelity (unsupported
    /// properties and degraded assets), and the last resolved frame's drawn
    /// and failed tile counts plus its per-source layer accounting. A pure
    /// query that never throws; with no source installed it answers an honest
    /// not-configured status.
    pub fn vector_tile_source_status(&self) -> VectorTileSourceStatus {
        let Some(source_summary) = self.vector_base_summary_lock().clone() else {
            return VectorTileSourceStatus::not_configured();
        };
        let last_vector_base_frame = self
            .last_vector_base_frame_lock()
            .clone()
            .unwrap_or_default();
        VectorTileSourceStatus {
            is_configured: true,
            style_id: source_summary.style_id,
            source_name: source_summary.source_name,
            attribution_text: source_summary.attribution_text,
            maximum_zoom_level: source_summary.maximum_zoom_level,
            drawn_tile_count: last_vector_base_frame.drawn_tile_count,
            failed_tile_count: last_vector_base_frame.failed_tile_count,
            last_failure_detail: last_vector_base_frame.last_failure_detail,
            unsupported_property_count: source_summary.unsupported_property_count,
            unsupported_property_names: source_summary.unsupported_property_names,
            degraded_asset_names: source_summary.degraded_asset_names,
            source_layer_counts: last_vector_base_frame.source_layer_counts,
        }
    }

    /// Takes the vector base scheduler out of its slot for the whole draw (the
    /// same take-out discipline as [`Self::take_surface_session`]); `None`
    /// when no source is installed, and the restore writes back what it took.
    fn take_vector_base_scheduler(&self) -> Option<Box<VectorBaseScheduler>> {
        match std::mem::replace(&mut *self.vector_base_lock(), VectorBaseSlot::Busy) {
            VectorBaseSlot::Installed(vector_base_scheduler) => Some(vector_base_scheduler),
            // The surface slot already refuses a second concurrent draw, so
            // these variants mean no source is installed.
            VectorBaseSlot::Busy | VectorBaseSlot::Absent => None,
        }
    }

    /// The shared slot handles one frame's spawned work needs. Cloning this
    /// struct clones reference counts only.
    fn frame_shared_state(&self) -> FrameSharedState {
        FrameSharedState {
            map_sdk: Arc::clone(&self.map_sdk),
            surface_slot: Arc::clone(&self.surface_slot),
            vector_base: Arc::clone(&self.vector_base),
            vector_base_summary: Arc::clone(&self.vector_base_summary),
            last_vector_base_frame: Arc::clone(&self.last_vector_base_frame),
            pending_vector_base_change: Arc::clone(&self.pending_vector_base_change),
            pending_raster_tile_reload: Arc::clone(&self.pending_raster_tile_reload),
            last_drawn_frame: Arc::clone(&self.last_drawn_frame),
        }
    }

    /// Restores the drawn scheduler into the slot and applies the base-map
    /// switch a call requested while the draw owned it (see
    /// [`FrameSharedState::restore_vector_base_scheduler`]). The draw path
    /// restores through the frame ownership guard; this wrapper is the test
    /// seam that exercises the same restore deterministically.
    #[cfg(test)]
    fn restore_vector_base_scheduler(
        &self,
        vector_base_scheduler: Option<Box<VectorBaseScheduler>>,
    ) {
        self.frame_shared_state()
            .restore_vector_base_scheduler(vector_base_scheduler);
    }

    /// Draws one frame for the camera (`draw`'s documented UDL method):
    /// lists the camera's visible tiles, resolves them straight from the
    /// pool's state (no await on the network), plans the detached warm fetch
    /// of the misses — the visible set plus the prefetch planner's ahead
    /// ring — and spawns it fire-and-forget through the crate-owned
    /// runtime, then renders tiles, overlays and the location layer, and
    /// presents. A never-awaited frame, with pending detail arriving on
    /// later frames' resolves (which `is_frame_settled`'s pump resumes on).
    /// Returns the frame's tile and overlay statistics so the platform
    /// render loop can report exactly what a frame drew (the round-9
    /// contract ask), and `is_frame_settled` can compare the pending counts.
    ///
    /// The frame's work hands to the crate-owned runtime through [`drive`],
    /// together with the taken-out session, the shared slot handles and the
    /// overlay and indicator snapshots, so the poller awaits only a
    /// runtime-independent handback. The spawned work owns the session and
    /// scheduler through a [`FrameOwnership`] guard that restores them when
    /// the frame ends, so dropping the caller's future mid-draw cannot strand
    /// the slot busy: the work still runs to completion and restores the
    /// session (or leaves the slot honestly detached when a `detach` raced).
    /// A panicking frame surfaces as the same panic on a caller still awaiting
    /// it, and the guard's drop during unwinding still restores the slot.
    pub async fn draw(&self, camera: Arc<MapCamera>) -> Result<FrameStatistics, SdkError> {
        let native_camera = *camera.native_camera();
        let surface_session = self.take_surface_session()?;
        let (overlays, overlay_revision) = {
            let overlay_store = self.overlays_lock();
            (overlay_store.drawn_overlays(), overlay_store.revision())
        };
        let location_indicator = self.location_indicator_lock().clone();
        let device_pixel_ratio = self.stored_device_pixel_ratio();
        let drawn_vector_base_source = self.vector_base_summary_lock().clone();
        // The camera is the platform's gesture seam: a frame drawn at a pose
        // other than the last presented one is a frame in motion (a pointer
        // pan, a pinch, a rotation or an eased camera move), so the vector
        // base grades its assembly and the settle check below keeps the pump
        // drawing until a still frame resolves it.
        let motion_graded = self
            .last_drawn_frame_lock()
            .as_ref()
            .is_some_and(|last_drawn_frame| last_drawn_frame.camera != native_camera);
        let vector_base_scheduler = self.take_vector_base_scheduler();
        let shared_state = self.frame_shared_state();
        drive(async move {
            let mut frame_ownership = FrameOwnership::new(
                FrameSharedState::clone(&shared_state),
                surface_session,
                vector_base_scheduler,
            );
            let draw_outcome = {
                let (surface_session, vector_base_scheduler) = frame_ownership.borrowed_parts();
                Self::draw_frame(
                    surface_session,
                    &native_camera,
                    vector_base_scheduler,
                    &overlays,
                    location_indicator.as_ref(),
                    device_pixel_ratio,
                    motion_graded,
                )
                .await
            };
            // The drawn frame's accounting describes the source that frame
            // drew. Recording it before the restore lets a base-map switch
            // applied there reset it, instead of pairing the new source's
            // summary with the previous source's tile counts.
            if let Ok((_, Some(drawn_vector_base_frame), _)) = &draw_outcome {
                shared_state.record_vector_base_frame(drawn_vector_base_frame);
            }
            // The presented state is recorded before the restore too: a
            // raster reload applied there drops it, so the restored view
            // answers unsettled and the pump draws the re-planned frame.
            if let Ok((frame_statistics, vector_base_frame, zoom_transition_running)) =
                &draw_outcome
            {
                shared_state.record_last_drawn_frame(
                    native_camera,
                    overlay_revision,
                    location_indicator,
                    device_pixel_ratio,
                    drawn_vector_base_source,
                    frame_statistics,
                    vector_base_frame.as_deref(),
                    *zoom_transition_running,
                );
            }
            frame_ownership.restore();
            draw_outcome
        })
        .await
        .map(|(frame_statistics, _vector_base_frame, _zoom_transition_running)| frame_statistics)
    }

    /// Takes the surface session out of the slot for the whole draw,
    /// replacing an idle state with [`SurfaceSlot::Busy`] (or refusing when
    /// the slot is busy or detached) — the serialized-draw rule of the UDL
    /// documentation.
    fn take_surface_session(&self) -> Result<Box<SurfaceSession>, SdkError> {
        let mut slot_state = self.surface_slot_lock();
        match std::mem::replace(&mut *slot_state, SurfaceSlot::Busy) {
            SurfaceSlot::Idle(surface_session) => Ok(surface_session),
            SurfaceSlot::Busy => Err(SdkError::RenderingSurfaceUnavailable {
                detail: "a frame draw is in flight; the next frame retries".to_string(),
            }),
            SurfaceSlot::Detached => Err(detached_surface_error()),
        }
    }

    /// One frame against the taken-out session: with no vector base, the
    /// camera's visible tiles listed GPU-free, the frame's readiness resolved
    /// from the pool's state alone (`resolve_frame` — no await on the network;
    /// parent fallbacks cover the gaps), the background warm fetch planned over
    /// the prefetch planner's request list (visible set first, then the
    /// ahead ring, then the coarse fallback) and spawned detached — the
    /// frame's own verdicts report the in-flight tiles `Deferred`. With a
    /// vector base installed, the raster layer is hidden and the vector
    /// scheduler resolves the styled base instead. Then tiles, overlays, the
    /// vector base's place labels and the location layer
    /// rendered to the surface. The render itself is synchronous within the
    /// handback task, so the frame never spreads a borrow of the view across
    /// its await. (The navigation
    /// corridor leg of the prefetch planner stays unwired here: the surface
    /// view has no navigation session to pull a corridor from; the
    /// ring-ahead and fallback layers still prefetch.)
    async fn draw_frame(
        surface_session: &mut SurfaceSession,
        native_camera: &rustic_maps_rendering::MapCamera,
        vector_base_scheduler: Option<&mut VectorBaseScheduler>,
        overlays: &[RenderingMapOverlay],
        location_indicator_layer: Option<&LocationIndicatorLayer>,
        device_pixel_ratio: f64,
        motion_graded: bool,
    ) -> Result<(FrameStatistics, Option<Arc<VectorBaseFrame>>, bool), SdkError> {
        let viewport = RenderingViewportPixels::new(
            f64::from(surface_session.width_pixels),
            f64::from(surface_session.height_pixels),
        )
        .map_err(|failure_message| SdkError::RenderingSurfaceUnavailable {
            detail: format!(
                "{SURFACE_SEAM_LABEL} surface extent became unusable: {failure_message}"
            ),
        })?;
        // A styled vector base replaces the raster base instead of tinting it:
        // while one is installed the raster layer is hidden, so the imagery and
        // its baked labels cannot show through the style's translucent fills.
        // The browser canvas takes the same decision (`setBaseMapVisible`).
        let raster_base_serves = vector_base_scheduler.is_none();
        let (prepared_tiles, retained_tile_level, zoom_transition_running) = if raster_base_serves {
            let tile_zoom_level = native_camera.tile_zoom_level();
            let visible_tile_keys = native_camera.visible_tile_keys(
                &viewport,
                DEFAULT_FIELD_OF_VIEW_DEGREES,
                tile_zoom_level,
            );
            // The warm plan first, resolve second: the plan marks its fetches
            // in-flight, so this frame's verdicts honestly report them
            // `Deferred` and the render uses the pooled truth the resolve
            // produced.
            let warm_request_tile_keys = TilePrefetchPlanner::new().prefetch_request_tiles(
                native_camera,
                &viewport,
                DEFAULT_FIELD_OF_VIEW_DEGREES,
                // A plain map view carries no navigation corridor geometry.
                None,
                surface_session.tile_scheduler.texture_pool(),
                &surface_session.tile_scheduler.warming_tile_keys(),
            );
            let warm_task = surface_session
                .tile_scheduler
                .plan_warm_tiles(&warm_request_tile_keys);
            if warm_task.fetch_count() > 0 {
                spawn_detached(warm_task.run());
            }
            let prepared_tiles = surface_session
                .tile_scheduler
                .resolve_frame(&visible_tile_keys);
            // A zoom that crossed a tile level keeps the retired level on
            // screen (resolved from the pool alone — a transition never
            // fetches) and dissolves it into the new level as its tiles
            // arrive. The transition also stays unsettled, so the platform's
            // pump draws the fade out instead of leaving it half way.
            let ready_tile_fraction = requested_level_ready_tile_fraction(&prepared_tiles);
            let now_milliseconds = surface_session
                .zoom_transition_clock_origin
                .elapsed()
                .as_secs_f64()
                * MILLISECONDS_PER_SECOND;
            let transition_frame = surface_session.zoom_transition.update(
                tile_zoom_level,
                now_milliseconds,
                ready_tile_fraction,
            );
            let retained_tile_level =
                transition_frame
                    .retained_tile_zoom_level
                    .map(|retained_tile_zoom_level| RetainedTileLevel {
                        tile_zoom_level: retained_tile_zoom_level,
                        opacity: transition_frame.retained_opacity,
                    });
            (
                prepared_tiles,
                retained_tile_level,
                transition_frame.is_running,
            )
        } else {
            (Vec::new(), None, false)
        };
        // The vector base resolves in the same handback task as the raster
        // plan (when one serves) and composes beneath the caller's overlays.
        let mut style_transition_running = false;
        // The glyph font the style's own place names draw with, when the
        // install fetched one from the style document's glyph endpoint.
        let mut style_label_font: Option<Arc<LabelFont>> = None;
        let vector_base_frame = match vector_base_scheduler {
            Some(vector_base_scheduler) => {
                style_label_font = vector_base_scheduler.label_font().cloned();
                let vector_base_zoom_level =
                    vector_base_scheduler.served_zoom_level(native_camera.tile_zoom_level());
                let vector_base_tile_keys = native_camera.visible_tile_keys(
                    &viewport,
                    DEFAULT_FIELD_OF_VIEW_DEGREES,
                    vector_base_zoom_level,
                );
                // The fetch runs detached and the frame draws the cache, so a
                // first frame over unvisited ground presents the style's
                // background at once instead of blocking on every tile's
                // latency (the black keyless start's fix, PLAN §2).
                // The visible plan delivers each tile the moment it lands, so
                // a later pump frame paints it without waiting for the whole
                // visible set; the native pump already redraws while a frame
                // is unsettled, so the callback needs no wake of its own.
                if let Some(prefetch) = vector_base_scheduler
                    .prefetch_missing_tiles_with_delivery(&vector_base_tile_keys, |_| {})
                {
                    spawn_detached(prefetch);
                }
                // The style's declared transitions blend from whichever style
                // this one replaced; the frame clock is the same monotonic
                // origin the raster cross-fade uses, never a platform timer.
                let frame_time_seconds = surface_session
                    .zoom_transition_clock_origin
                    .elapsed()
                    .as_secs_f64();
                // The shared frame is the previous allocation when nothing
                // changed, and `content_revision` names its content; the
                // surface never clones the overlay list to compare or draw it.
                let vector_base_frame = vector_base_scheduler
                    .cached_frame_at_time_in_motion_shared(
                        &vector_base_tile_keys,
                        native_camera.zoom_level,
                        frame_time_seconds,
                        motion_graded,
                    );
                style_transition_running = vector_base_scheduler.style_transition_running();
                // The ring around the viewport (and the coarse parents that
                // back a parent fallback) warms after the presentable frame
                // is built, so the next pan starts from the cache. The plan
                // is detached and cancellable; it never runs inside a draw.
                if let Some(ring_prefetch) =
                    vector_base_scheduler.prefetch_ring_tiles(&vector_base_tile_keys)
                {
                    spawn_detached(ring_prefetch);
                }
                Some(vector_base_frame)
            }
            None => None,
        };
        // The style-derived base primitives paint first and the caller's
        // overlay list paints over them. Both layers stay borrowed: the
        // renderer scales each declared point/CSS size at emission, so an
        // unchanged overlay list is never deep-cloned per frame and every
        // route line, marker, pin and traffic width keeps its declared point
        // size on a 2× or 3× surface.
        let base_overlays: &[RenderingMapOverlay] =
            vector_base_frame.as_ref().map_or(&[], |vector_base_frame| {
                vector_base_frame.overlays.as_slice()
            });
        let overlay_layers = [base_overlays, overlays];
        // The vector base's place names draw over its geometry through the
        // label-aware entry point, using the style-derived candidates the
        // frame resolved and the glyph font the style's own endpoint served.
        // A style whose install fetched no font — no glyph endpoint, no
        // symbol font stack, or a fetch the install could not complete —
        // draws through the SDK's embedded Latin range instead. A raster
        // base, a style without symbol layers, or a frame whose labels all
        // collide yields `None`, and the renderer then creates no label
        // buffer, no atlas upload and no draw — the unlabeled frame stays
        // identical.
        let label_layer = vector_base_frame
            .as_ref()
            .and_then(|drawn_vector_base_frame| {
                let previous_label_placements = &surface_session.previous_label_placements;
                match style_label_font.as_deref() {
                    Some(label_font) => drawn_vector_base_frame
                        .label_layer_with_font_and_device_pixel_ratio_and_history(
                            native_camera,
                            &viewport,
                            DEFAULT_FIELD_OF_VIEW_DEGREES,
                            label_font,
                            device_pixel_ratio,
                            previous_label_placements,
                        ),
                    None => drawn_vector_base_frame
                        .label_layer_with_device_pixel_ratio_and_history(
                            native_camera,
                            &viewport,
                            DEFAULT_FIELD_OF_VIEW_DEGREES,
                            device_pixel_ratio,
                            previous_label_placements,
                        ),
                }
            });
        // Hand this frame's placements to the next one: the names on screen
        // keep their spots through a pan instead of flickering as newcomers
        // compete for the same space. A motion-graded frame assembled no
        // labels, so it leaves the last settled placements in hand; the
        // settle then places against the pose the gesture started from rather
        // than against an empty history, which keeps its label set equal to
        // the same camera reached without a gesture.
        if !vector_base_frame
            .as_ref()
            .is_some_and(|vector_base_frame| vector_base_frame.motion_graded)
        {
            surface_session.previous_label_placements = label_layer
                .as_ref()
                .map(|layer| layer.labels().to_vec())
                .unwrap_or_default();
        }
        // The style's own canvas background fills the gaps the overlays leave
        // once the raster base is hidden; without it those gaps would read as
        // the platform's dark clear instead of the map the style was designed
        // on. A raster base keeps the shared clear.
        let surface_clear_color = vector_base_frame
            .as_ref()
            .and_then(|frame| frame.background)
            .unwrap_or(CLEAR_COLOR_RGBA);
        surface_session
            .surface_renderer
            .set_clear_color(surface_clear_color);
        surface_session
            .surface_renderer
            .render_frame_layers_and_labels_with_retained_level(
                native_camera,
                &viewport,
                &prepared_tiles,
                surface_session.tile_scheduler.texture_pool_mut(),
                &overlay_layers,
                device_pixel_ratio,
                location_indicator_layer,
                label_layer.as_ref(),
                retained_tile_level,
            )
            .map(|rendered_frame_statistics| {
                // A running vector cross-fade keeps the view unsettled exactly
                // like the raster one, so the platform's pump draws the held
                // level out instead of leaving the fade half way.
                let vector_zoom_transition_running = vector_base_frame
                    .as_ref()
                    .is_some_and(|vector_base_frame| vector_base_frame.zoom_transition_running);
                (
                    FrameStatistics::from_native_frame_statistics(&rendered_frame_statistics),
                    vector_base_frame,
                    zoom_transition_running
                        || vector_zoom_transition_running
                        || style_transition_running,
                )
            })
            .map_err(|surface_error| rendering_failure_to_sdk_error(&surface_error))
    }

    /// Builds the whole drawing machinery for one attach: the platform
    /// handle lowers to a surface target, the renderer acquires its
    /// context and negotiates its format, the extent configures, and the
    /// raster provider resolves through the SDK registry before the
    /// scheduler takes it.
    fn build_surface_session(
        &self,
        surface_handle: SurfaceHandle,
        width_pixels: u32,
        height_pixels: u32,
    ) -> Result<SurfaceSession, SdkError> {
        if width_pixels == 0 || height_pixels == 0 {
            return Err(SdkError::InvalidRenderInput {
                detail: format!(
                    "attach extent {width_pixels}x{height_pixels} must be non-zero; \
                     a platform view not laid out yet waits for resize"
                ),
            });
        }
        let surface_target = platform_surface::surface_target_from_handle(&surface_handle)?;
        let render_target = SurfaceRenderTarget::from_unsafe_target(surface_target)
            .map_err(|surface_error| rendering_failure_to_sdk_error(&surface_error))?;
        let mut surface_renderer =
            SurfaceTileRenderer::new(render_target, &WgpuSurfaceGraphicsContextSource)
                .map_err(|surface_error| rendering_failure_to_sdk_error(&surface_error))?;
        surface_renderer
            .configure_extent(width_pixels, height_pixels)
            .map_err(|surface_error| rendering_failure_to_sdk_error(&surface_error))?;
        let raster_tile_provider = self.map_sdk.required_raster_tile_provider()?;
        Ok(SurfaceSession {
            surface_renderer: Box::new(surface_renderer),
            tile_scheduler: new_raster_tile_scheduler(raster_tile_provider),
            width_pixels,
            height_pixels,
            previous_label_placements: Vec::new(),
            zoom_transition: rustic_maps_rendering::ZoomTransition::new(),
            zoom_transition_clock_origin: std::time::Instant::now(),
        })
    }
}
/// The detached-view refusal every surface method reports.
fn detached_surface_error() -> SdkError {
    SdkError::RenderingSurfaceUnavailable {
        detail: SURFACE_DETACHED_DETAIL.to_string(),
    }
}

/// Maps the surface renderer's failures onto the binding error model: the
/// frame's own input problems (an overflowing draw list, a mismatched
/// viewport) are render-input failures, while everything about the surface
/// and graphics context itself is a rendering-surface failure. Both carry
/// the renderer's verbatim detail.
fn rendering_failure_to_sdk_error(surface_render_error: &SurfaceRenderError) -> SdkError {
    match surface_render_error {
        SurfaceRenderError::DrawListTooLarge {
            quad_count,
            maximum_quad_count,
        } => SdkError::InvalidRenderInput {
            detail: format!(
                "the frame's draw list of {quad_count} quads exceeds the \
                 supported {maximum_quad_count}; reduce the viewport extent \
                 or the fetch zoom"
            ),
        },
        SurfaceRenderError::ViewportExtentMismatch {
            viewport_extent,
            configured_extent,
        } => SdkError::InvalidRenderInput {
            detail: format!(
                "the frame's viewport extent {viewport_extent:?} does not \
                 match the configured surface extent {configured_extent:?}"
            ),
        },
        other_surface_failure => SdkError::RenderingSurfaceUnavailable {
            detail: other_surface_failure.to_string(),
        },
    }
}

/// Prefixes a per-entry overlay conversion failure with the failed entry's
/// position, mirroring the wasm canvas' `overlay <index> is invalid` prefix.
fn invalid_overlay_entry(failure: SdkError, overlay_index: usize) -> SdkError {
    overlay_failure_with_prefix(failure, format!("overlay {overlay_index} is invalid"))
}

/// Prefixes one added or replaced overlay's conversion failure: there is no
/// list position to name, so the subject is the descriptor itself.
fn invalid_added_overlay(failure: SdkError) -> SdkError {
    overlay_failure_with_prefix(failure, "the overlay is invalid".to_string())
}

/// Prefixes an overlay conversion failure with its subject. The prefix rides
/// on the failed descriptor's own detail, so the wasm-seam parity messages
/// keep their variant: a coordinate-boundary failure stays a coordinate
/// failure, prefixed.
fn overlay_failure_with_prefix(failure: SdkError, prefix: String) -> SdkError {
    match failure {
        SdkError::InvalidRenderInput { detail } => SdkError::InvalidRenderInput {
            detail: format!("{prefix}: {detail}"),
        },
        SdkError::InvalidCoordinateInput { detail } => SdkError::InvalidCoordinateInput {
            detail: format!("{prefix}: {detail}"),
        },
        other_failure => SdkError::InvalidRenderInput {
            detail: format!("{prefix}: {other_failure}"),
        },
    }
}

#[cfg(test)]
mod tests;
