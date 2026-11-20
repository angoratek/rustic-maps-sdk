//! Frame statistics: the JavaScript status field vocabulary and the GPU
//! pass timestamp ring the readback reports.

use super::*;

/// Name of the frame statistics object field carrying the number of tiles
/// the camera listed at the fetch zoom.
pub(super) const STATUS_FIELD_REQUESTED_TILES: &str = "requestedTiles";
/// Name of the frame statistics object field carrying the number of drawn
/// tiles (quads with pooled imagery over their span this frame).
pub(super) const STATUS_FIELD_DRAWN_TILES: &str = "drawnTiles";
/// Name of the frame statistics object field carrying the number of drawn
/// world-copy fragments (the quads after fragment splitting).
pub(super) const STATUS_FIELD_DRAWN_FRAGMENTS: &str = "drawnFragments";
/// Name of the frame statistics object field carrying the number of
/// requested tiles with no pooled imagery over their span this frame.
pub(super) const STATUS_FIELD_MISSING_TILES: &str = "missingTiles";
/// Name of the frame statistics object field carrying the addresses
/// (`z/x/y`) of the missing tiles, bounded to
/// [`MAXIMUM_REPORTED_MISSING_TILE_ADDRESSES`].
pub(super) const STATUS_FIELD_MISSING_TILE_ADDRESSES: &str = "missingTileAddresses";
/// Most missing tile addresses one frame reports: a diagnostics panel names
/// the addresses that lagged without letting one wide viewport grow the frame
/// statistics without bound.
pub(super) const MAXIMUM_REPORTED_MISSING_TILE_ADDRESSES: usize = 16;
/// Name of the frame statistics object field reporting whether the frame
/// still owes cross-fade frames after a tile zoom-level change.
pub(super) const STATUS_FIELD_ZOOM_TRANSITION_RUNNING: &str = "zoomTransitionRunning";
/// Name of the frame statistics object field carrying the number of retired
/// tiles the frame drew for the zoom cross-fade.
pub(super) const STATUS_FIELD_RETAINED_TILES: &str = "retainedTiles";
/// Name of the frame statistics object field reporting whether a styled
/// vector base still owes tile-level cross-fade frames. It mirrors
/// `zoomTransitionRunning`, which also reports the raster cross-fade and a
/// style transition.
pub(super) const STATUS_FIELD_VECTOR_ZOOM_TRANSITION_RUNNING: &str = "vectorZoomTransitionRunning";
/// Name of the frame statistics object field carrying the number of
/// retired-level vector tiles the frame still drew for its cross-fade.
pub(super) const STATUS_FIELD_VECTOR_RETAINED_TILES: &str = "vectorRetainedTiles";
/// Name of the frame statistics object field carrying how many vector-base
/// tiles the frame neither drew nor failed (a fetch is in flight or has not
/// started yet). Zero while no vector base serves.
pub(super) const STATUS_FIELD_PENDING_VECTOR_TILES: &str = "pendingVectorTiles";
/// Name of the frame statistics object field carrying how many vector-base
/// tiles the frame drew at their own zoom. Zero while no vector base serves.
pub(super) const STATUS_FIELD_DRAWN_VECTOR_TILES: &str = "drawnVectorTiles";
/// Name of the frame statistics object field reporting that the raster tile
/// layer listed tiles, drew none, and has at least one retry scheduled: the
/// honest "waiting on the tile source" state, as distinct from a settled map
/// and from a source that has given up on the address.
pub(super) const STATUS_FIELD_RASTER_TILES_RETRYING: &str = "rasterTilesRetrying";
/// Name of the frame statistics object field carrying the fetch zoom the
/// frame scheduled at, clamped to the recorded fixtures.
pub(super) const STATUS_FIELD_TILE_ZOOM_LEVEL: &str = "tileZoomLevel";
/// Name of the frame statistics object field carrying the pixel resolution
/// the canvas fetches and samples its tiles at.
pub(super) const STATUS_FIELD_TILE_RESOLUTION: &str = "tileResolution";
/// Statistics text of the standard tile resolution.
pub(super) const STATUS_TILE_RESOLUTION_STANDARD_TEXT: &str = "standard";
/// Statistics text of the retina tile resolution (the `@2x` form).
pub(super) const STATUS_TILE_RESOLUTION_RETINA_TEXT: &str = "retina";
/// Name of the frame statistics object field carrying the number of solid
/// overlay triangles drawn over the tiles this frame.
pub(super) const STATUS_FIELD_OVERLAY_TRIANGLES: &str = "overlayTriangles";
/// Name of the frame statistics object field carrying the number of placed
/// labels the frame's label layer painted (zero without glyphs or markers).
pub(super) const STATUS_FIELD_LABEL_COUNT: &str = "labelCount";
/// Name of the frame statistics object field carrying the number of glyph
/// triangles (halo and text passes together) the label layer drew.
pub(super) const STATUS_FIELD_LABEL_GLYPH_TRIANGLES: &str = "labelGlyphTriangles";
/// Name of the frame statistics object field carrying the number of solid
/// triangles the label layer's route-reference badge pass drew.
pub(super) const STATUS_FIELD_LABEL_BADGE_TRIANGLES: &str = "labelBadgeTriangles";
/// Name of the frame statistics object field carrying the placed labels' text
/// in draw order, so a caller can see which names the frame painted.
pub(super) const STATUS_FIELD_LABEL_TEXTS: &str = "labelTexts";
/// Name of the frame statistics object field carrying how many placed labels
/// run along a line feature (the rotated road names), so a caller can tell
/// line placement from point placement.
pub(super) const STATUS_FIELD_LINE_LABEL_COUNT: &str = "lineLabelCount";
/// Name of the frame statistics object field carrying the texts of the labels
/// placed along line features, in draw order: the street names the frame
/// painted.
pub(super) const STATUS_FIELD_LINE_LABEL_TEXTS: &str = "lineLabelTexts";
/// Name of the frame statistics object field carrying how many GPU vertex and
/// index buffer allocations this canvas has made since initialization. A
/// steady-state frame reuses the buffers it already owns, so the value stops
/// growing once every layer's payload has been seen at its largest; a frame
/// that allocates again therefore signals a regression rather than normal
/// rendering.
pub(super) const STATUS_FIELD_GPU_BUFFER_ALLOCATIONS: &str = "gpuBufferAllocations";
/// Name of the frame statistics object field carrying the frame's own stage
/// spans in milliseconds (`{tilePipeline, overlayGeometry, encode, submit}`).
/// A frame-budget measurement reads them to attribute a slow frame to
/// geometry, encoding or submission instead of guessing; the page's
/// diagnostics render them as ordinary frame facts.
pub(super) const STATUS_FIELD_STAGE_MILLISECONDS: &str = "stageMilliseconds";
/// The tile stage of a frame: camera listing, same-origin fetches, the
/// scheduler's warm pass, quad assembly and texture resolution.
pub(super) const STAGE_TILE_PIPELINE: &str = "tilePipeline";
/// The overlay stage of a frame: the cached world-space overlay revision
/// check (re-tessellating only when the content changes), the frame uniform
/// write, and the per-frame location-indicator tessellation.
pub(super) const STAGE_OVERLAY_GEOMETRY: &str = "overlayGeometry";
/// The encoding stage of a frame: converting the assembled vertices into the
/// vertex and index buffers the draw consumes.
pub(super) const STAGE_ENCODE: &str = "encode";
/// The submission stage of a frame: surface acquisition, the render pass and
/// the queue submission.
pub(super) const STAGE_SUBMIT: &str = "submit";
/// Name of the frame statistics object field carrying the render pass's own
/// GPU time in milliseconds. The value comes from timestamp queries around
/// the pass and is reported one frame after it was written, so it describes
/// the previous frame's GPU work; a slow frame can then be attributed to the
/// GPU rather than the CPU. The field is absent when the drawing adapter
/// does not support timestamp queries or when no reading has completed yet,
/// and a frame never waits on the readback to present.
pub(super) const STATUS_FIELD_GPU_PASS_MILLISECONDS: &str = "gpuPassMilliseconds";
/// The vector base's cache and latency fields of a frame that drew one:
/// cumulative payload-cache hits and misses, fetches in flight at resolve,
/// the missing tiles a cached parent covered, the first-drawn-tile latency
/// (absent until a tile has painted), and whether the frame drew in
/// motion-graded mode.
pub(super) const STATUS_FIELD_VECTOR_BASE_PAYLOAD_CACHE_HIT_COUNT: &str =
    "vectorBasePayloadCacheHitCount";
pub(super) const STATUS_FIELD_VECTOR_BASE_PAYLOAD_CACHE_MISS_COUNT: &str =
    "vectorBasePayloadCacheMissCount";
pub(super) const STATUS_FIELD_VECTOR_BASE_IN_FLIGHT_FETCH_COUNT: &str =
    "vectorBaseInFlightTileFetchCount";
pub(super) const STATUS_FIELD_VECTOR_BASE_FALLBACK_TILE_COUNT: &str = "vectorBaseFallbackTileCount";
pub(super) const STATUS_FIELD_VECTOR_BASE_FIRST_DRAWN_TILE_MILLISECONDS: &str =
    "vectorBaseTimeToFirstDrawnTileMilliseconds";
pub(super) const STATUS_FIELD_VECTOR_BASE_MOTION_GRADED: &str = "motionGraded";
/// Tiles whose label candidates the vector base extracted this frame, and
/// tiles whose candidates it read from the per-tile cache. An unchanged
/// settle frame reports zero extractions.
pub(super) const STATUS_FIELD_VECTOR_BASE_LABEL_CANDIDATE_EXTRACTION_COUNT: &str =
    "vectorBaseLabelCandidateExtractionCount";
pub(super) const STATUS_FIELD_VECTOR_BASE_LABEL_CANDIDATE_CACHE_HIT_COUNT: &str =
    "vectorBaseLabelCandidateCacheHitCount";
/// Timestamps one frame's render pass writes: one when the pass begins and
/// one when it ends.
pub(super) const GPU_PASS_TIMESTAMP_COUNT: u32 = 2;
/// Query index of the render pass's beginning timestamp.
pub(super) const GPU_PASS_TIMESTAMP_BEGIN_INDEX: u32 = 0;
/// Query index of the render pass's ending timestamp.
pub(super) const GPU_PASS_TIMESTAMP_END_INDEX: u32 = 1;
/// Bytes one resolved timestamp tick count occupies (`u64`).
pub(super) const GPU_PASS_TIMESTAMP_BYTES: usize = std::mem::size_of::<u64>();
/// Bytes one frame's resolved timestamp pair occupies.
pub(super) const GPU_PASS_TIMESTAMP_RESULT_BYTES: u64 =
    (GPU_PASS_TIMESTAMP_COUNT as usize * GPU_PASS_TIMESTAMP_BYTES) as u64;
/// Readback buffers the timestamp results alternate through. Two are enough
/// for the one-frame-lagged readback: frame N writes into one while frame N+1
/// reads the other, and each slot is free again by the time its turn returns.
pub(super) const GPU_PASS_READBACK_SLOT_COUNT: usize = 2;
/// Nanoseconds in one millisecond, converting a resolved tick span (scaled by
/// the queue's timestamp period) into the statistics' millisecond unit.
pub(super) const NANOSECONDS_PER_MILLISECOND: f64 = 1_000_000.0;
/// Milliseconds in one second, converting the monotonic frame clock into the
/// seconds a style transition is timed in.
pub(super) const MILLISECONDS_PER_SECOND: f64 = 1_000.0;

/// Whether a drawing adapter can report GPU pass timestamps. Timestamp
/// queries are only requested when the adapter advertises the feature and is
/// not the noop backend or a CPU fallback adapter: a software rasterizer's
/// tick counts say nothing about a GPU and some driver paths leave them
/// unusable, so such an adapter keeps the absent statistic instead of
/// contributing misleading numbers.
pub(super) fn adapter_supports_gpu_pass_timestamps(adapter: &wgpu::Adapter) -> bool {
    let adapter_information = adapter.get_info();
    adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY)
        && adapter_information.backend != wgpu::Backend::Noop
        && adapter_information.device_type != wgpu::DeviceType::Cpu
}

/// State of one alternated timestamp readback slot.
///
/// A slot is only ever remapped once its previous map has been read and
/// unmapped: cancelling a pending map and immediately reusing the buffer makes
/// the browser reject the next map ("map is pending", "already mapped") and
/// eventually leaves the buffer mapped across a copy. A pending slot is
/// therefore skipped, never unmapped.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum GpuPassReadbackState {
    /// Unmapped and free: the slot may receive a copy and a map request.
    Idle,
    /// A map request is outstanding and its callback has not run yet.
    Pending,
    /// The callback reported success; the next frame that needs the slot
    /// reads it and unmaps it.
    Mapped,
}

/// One mapped-readback buffer the resolved pass timestamps alternate through,
/// with the state its asynchronous map request left behind. The state lives
/// behind an `Rc<Cell<_>>` because the map callback is registered with a
/// `'static` closure while the slot itself stays owned by the draw state.
pub(super) struct GpuPassReadbackSlot {
    buffer: wgpu::Buffer,
    state: Rc<Cell<GpuPassReadbackState>>,
}

/// The timestamp-query ring behind [`STATUS_FIELD_GPU_PASS_MILLISECONDS`].
///
/// A frame writes two timestamps around its render pass, resolves them into
/// a small buffer, copies the result into the current readback slot and maps
/// it without waiting. The next frame reads whatever that map completed,
/// unmaps the slot and reports the elapsed GPU time; a map that is still
/// pending is dropped rather than allowed to delay the presentation. The lag
/// is the point: awaiting a mapped readback inside the presenting frame made
/// later frames render black under headless Chromium, so no frame ever waits
/// on its own timestamps.
pub(super) struct GpuPassTimestamps {
    /// The pair of pass timestamps the frame's render pass writes.
    pub(super) query_set: wgpu::QuerySet,
    /// Resolve destination of the query set: query results cannot be copied
    /// straight to a map-readable buffer.
    resolve_buffer: wgpu::Buffer,
    /// Alternating map-readable copies of the resolved results.
    readback_slots: Vec<GpuPassReadbackSlot>,
    /// Slot the next frame writes its results into.
    next_write_slot: usize,
    /// The last completed reading, in milliseconds; absent until one lands.
    pub(super) last_completed_milliseconds: Option<f64>,
    /// Nanoseconds per timestamp tick, from the queue's timestamp period.
    timestamp_period_nanoseconds: f64,
}

impl GpuPassTimestamps {
    /// Builds the query set, resolve buffer and readback slots. Only called
    /// when [`adapter_supports_gpu_pass_timestamps`] accepted the adapter.
    pub(super) fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let query_set = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("rustic-maps-web-bindings-gpu-pass-timestamps"),
            ty: wgpu::QueryType::Timestamp,
            count: GPU_PASS_TIMESTAMP_COUNT,
        });
        let resolve_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rustic-maps-web-bindings-gpu-pass-timestamp-resolve"),
            size: GPU_PASS_TIMESTAMP_RESULT_BYTES,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback_slots = (0..GPU_PASS_READBACK_SLOT_COUNT)
            .map(|slot_index| GpuPassReadbackSlot {
                buffer: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(match slot_index {
                        0 => "rustic-maps-web-bindings-gpu-pass-timestamp-readback-0",
                        _ => "rustic-maps-web-bindings-gpu-pass-timestamp-readback-1",
                    }),
                    size: GPU_PASS_TIMESTAMP_RESULT_BYTES,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                state: Rc::new(Cell::new(GpuPassReadbackState::Idle)),
            })
            .collect();
        Self {
            query_set,
            resolve_buffer,
            readback_slots,
            next_write_slot: 0,
            last_completed_milliseconds: None,
            // The browser backend reports nanoseconds per tick (1.0); a
            // backend that reports zero yields no usable reading.
            timestamp_period_nanoseconds: f64::from(queue.get_timestamp_period()),
        }
    }

    /// Reads any completed reading, then answers the slot this frame may
    /// write into — or `None` when both slots are still mapped or pending, in
    /// which case the frame simply carries no GPU sample. A pending map is
    /// never unmapped: it is left to complete and read on a later frame.
    pub(super) fn begin_frame(&mut self) -> Option<usize> {
        self.read_completed_reading();
        let write_slot_index = self.next_write_slot;
        self.next_write_slot = (write_slot_index + 1) % GPU_PASS_READBACK_SLOT_COUNT;
        (self.readback_slots[write_slot_index].state.get() == GpuPassReadbackState::Idle)
            .then_some(write_slot_index)
    }

    /// Reads and releases every slot whose map has completed. The last
    /// completed value stays reported if this frame has no fresh one, so a
    /// slow readback shows the previous measurement rather than a gap.
    fn read_completed_reading(&mut self) {
        for readback_slot in &self.readback_slots {
            if readback_slot.state.get() != GpuPassReadbackState::Mapped {
                continue;
            }
            if let Ok(mapped_range) = readback_slot.buffer.get_mapped_range(..)
                && mapped_range.len() >= GPU_PASS_TIMESTAMP_RESULT_BYTES as usize
            {
                let begin_ticks = timestamp_tick_at(&mapped_range, 0);
                let end_ticks = timestamp_tick_at(&mapped_range, 1);
                let elapsed_nanoseconds = end_ticks.saturating_sub(begin_ticks) as f64
                    * self.timestamp_period_nanoseconds;
                self.last_completed_milliseconds =
                    Some(elapsed_nanoseconds / NANOSECONDS_PER_MILLISECOND);
            }
            readback_slot.buffer.unmap();
            readback_slot.state.set(GpuPassReadbackState::Idle);
        }
    }

    /// Records the timestamp resolve and its copy into the frame's readback
    /// slot, after the frame's render pass was encoded.
    pub(super) fn resolve_and_copy(
        &self,
        command_encoder: &mut wgpu::CommandEncoder,
        write_slot_index: usize,
    ) {
        command_encoder.resolve_query_set(
            &self.query_set,
            GPU_PASS_TIMESTAMP_BEGIN_INDEX..GPU_PASS_TIMESTAMP_END_INDEX + 1,
            &self.resolve_buffer,
            0,
        );
        command_encoder.copy_buffer_to_buffer(
            &self.resolve_buffer,
            0,
            &self.readback_slots[write_slot_index].buffer,
            0,
            GPU_PASS_TIMESTAMP_RESULT_BYTES,
        );
    }

    /// Requests the current slot's map after the frame was submitted. The
    /// callback only flips the slot's state; a later frame reads it and never
    /// waits for it. A failed map returns the slot to `Idle` so it can be
    /// reused without an explicit unmap.
    pub(super) fn request_readback(&mut self, write_slot_index: usize) {
        let readback_slot = &self.readback_slots[write_slot_index];
        let slot_state = Rc::clone(&readback_slot.state);
        readback_slot.state.set(GpuPassReadbackState::Pending);
        readback_slot
            .buffer
            .map_async(wgpu::MapMode::Read, .., move |map_result| {
                slot_state.set(match map_result {
                    Ok(()) => GpuPassReadbackState::Mapped,
                    Err(_) => GpuPassReadbackState::Idle,
                });
            });
    }
}

/// Reads the `timestamp_index`-th resolved 64-bit tick count from a mapped
/// query result, using the host's native byte order (the wasm target's is
/// little endian, matching the WebGPU result layout).
pub(super) fn timestamp_tick_at(resolved_results: &[u8], timestamp_index: usize) -> u64 {
    let tick_start = timestamp_index * GPU_PASS_TIMESTAMP_BYTES;
    let mut tick_bytes = [0u8; GPU_PASS_TIMESTAMP_BYTES];
    tick_bytes
        .copy_from_slice(&resolved_results[tick_start..tick_start + GPU_PASS_TIMESTAMP_BYTES]);
    u64::from_ne_bytes(tick_bytes)
}
