//! Asynchronous tile fetching, retry back-off and the coalesced tile-arrival
//! repaint event.

use super::*;

/// Why a tile payload fetch produced no bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TilePayloadFetchFailure {
    /// Neither the recorded route nor a live source serves the address, so a
    /// retry cannot help while the sources stay unchanged.
    NoSource,
    /// A transport, HTTP or decode failure a later attempt can outlive.
    Transient,
}

/// The outcome of one tile payload fetch.
pub(super) enum TilePayloadFetch {
    /// The tile's encoded image bytes.
    Fetched(Vec<u8>),
    /// No bytes; the failure class decides whether the tile is retried.
    Failed(TilePayloadFetchFailure),
}

/// Announces that a detached prefetch delivered tile payloads, so the page's
/// on-demand render loop draws the frame that now includes them. The event is
/// the only channel a canvas has to the page's render scheduler; a page that
/// renders continuously simply ignores it.
pub(super) fn dispatch_map_tile_arrived(canvas: &HtmlCanvasElement) {
    if let Ok(tile_arrived_event) = web_sys::Event::new(MAP_TILE_ARRIVED_EVENT_NAME) {
        let _ = canvas.dispatch_event(&tile_arrived_event);
    }
}

/// The canvases a coalesced tile-arrival repaint can target, plus the targets
/// with a repaint already scheduled inside the current window.
///
/// The delivery callback a visible prefetch invokes must be `Send + Sync`, so
/// it cannot hold an [`HtmlCanvasElement`] itself. It carries only a target
/// index into this thread-local registry; the wasm task that runs the
/// delivery is on the same thread as the canvas, so the lookup is sound.
pub(super) struct MapTileArrivalDispatcher {
    /// One slot per live canvas, cleared when the canvas is dropped.
    targets: Vec<Option<HtmlCanvasElement>>,
    /// Target indices with a coalesced repaint timeout outstanding.
    scheduled_targets: HashSet<usize>,
}

thread_local! {
    static MAP_TILE_ARRIVAL_DISPATCHER: RefCell<MapTileArrivalDispatcher> =
        RefCell::new(MapTileArrivalDispatcher {
            targets: Vec::new(),
            scheduled_targets: HashSet::new(),
        });
}

/// Registers a canvas as a tile-arrival repaint target and returns its index.
pub(super) fn register_map_tile_arrival_target(canvas: &HtmlCanvasElement) -> usize {
    MAP_TILE_ARRIVAL_DISPATCHER.with(|dispatcher| {
        let mut dispatcher = dispatcher.borrow_mut();
        let target_index = dispatcher.targets.len();
        dispatcher.targets.push(Some(canvas.clone()));
        target_index
    })
}

/// Releases a dropped canvas' target slot; a timeout still outstanding finds
/// `None` and dispatches nothing.
pub(super) fn unregister_map_tile_arrival_target(target_index: usize) {
    MAP_TILE_ARRIVAL_DISPATCHER.with(|dispatcher| {
        if let Some(target_slot) = dispatcher.borrow_mut().targets.get_mut(target_index) {
            *target_slot = None;
        }
    });
}

/// Dispatches the arrival event to one registered target, if it is still live.
pub(super) fn dispatch_map_tile_arrived_for_target(target_index: usize) {
    let target = MAP_TILE_ARRIVAL_DISPATCHER.with(|dispatcher| {
        dispatcher
            .borrow()
            .targets
            .get(target_index)
            .and_then(Clone::clone)
    });
    if let Some(canvas) = target {
        dispatch_map_tile_arrived(&canvas);
    }
}

/// Schedules at most one tile-arrival repaint per target inside
/// [`MAP_TILE_ARRIVAL_COALESCE_MILLISECONDS`]: the first delivery starts a
/// timeout and every delivery before it fires folds into that same repaint.
pub(super) fn schedule_coalesced_map_tile_arrived(target_index: usize) {
    let repaint_already_scheduled = MAP_TILE_ARRIVAL_DISPATCHER.with(|dispatcher| {
        !dispatcher
            .borrow_mut()
            .scheduled_targets
            .insert(target_index)
    });
    if repaint_already_scheduled {
        return;
    }
    let Some(browser_window) = web_sys::window() else {
        finish_coalesced_map_tile_arrival(target_index);
        return;
    };
    let timeout_callback = Closure::once(move || {
        finish_coalesced_map_tile_arrival(target_index);
    });
    if browser_window
        .set_timeout_with_callback_and_timeout_and_arguments_0(
            timeout_callback.as_ref().unchecked_ref(),
            MAP_TILE_ARRIVAL_COALESCE_MILLISECONDS,
        )
        .is_ok()
    {
        // The timeout owns the callback; forgetting the closure lets the
        // timer call it and then collect it.
        timeout_callback.forget();
    } else {
        finish_coalesced_map_tile_arrival(target_index);
    }
}

/// Clears a target's pending mark and dispatches its one coalesced repaint.
pub(super) fn finish_coalesced_map_tile_arrival(target_index: usize) {
    MAP_TILE_ARRIVAL_DISPATCHER.with(|dispatcher| {
        dispatcher
            .borrow_mut()
            .scheduled_targets
            .remove(&target_index);
    });
    dispatch_map_tile_arrived_for_target(target_index);
}

impl BrowserMapView {
    /// Replaces the canvas' raster scheduler, staging provider, uploaded
    /// textures and cross-fade state with fresh ones, so every following
    /// frame plans its fetches from the current raster sources and no payload
    /// fetched from the previous source can draw again.
    pub(super) fn replace_raster_tile_state(&self, draw_state: &mut CanvasDrawState) {
        let tile_provider =
            std::sync::Arc::new(RecordedTilePayloadProvider::new(self.tile_resolution));
        draw_state.tile_scheduler = TileScheduler::new(
            tile_provider.clone(),
            TileTexturePool::new(TILE_PAYLOAD_POOL_MAXIMUM_BYTES),
            RasterImageFormat::Png,
            self.tile_resolution,
            MAXIMUM_TILE_FETCHES_PER_FRAME,
        );
        draw_state.tile_provider = tile_provider;
        draw_state.tile_textures.clear();
        draw_state.texture_cache_order.clear();
        draw_state.zoom_transition = ZoomTransition::new();
        draw_state.zoom_transition_clock_origin_milliseconds = browser_now_milliseconds();
    }

    /// Starts the fetches the frame's pooled-missing tiles need and returns
    /// immediately: a presenting frame never blocks on the network, and no
    /// fetch or image decode runs inside a presenting frame. Each
    /// fetch completes on its own task, stages its payload for the
    /// scheduler's fetch pass, and the page's settled pump picks the tile up
    /// on a later frame. Failures (route miss, fetch error, undecodable
    /// payload) mark the tile unavailable, so a camera over unrecorded ground
    /// stops re-requesting after the first attempt.
    pub(super) fn start_tile_prefetches(
        &self,
        draw_state: &CanvasDrawState,
        requested_tiles: &[TileKey],
    ) {
        let tile_provider = draw_state.tile_provider.clone();
        let tile_resolution = draw_state.tile_provider.tile_resolution;
        let recorded_tile_fixtures_are_served = self.recorded_tile_fixtures_are_served;
        let canvas = self.canvas.clone();
        // The revision of the pool this fetch belongs to. A result that
        // resolves after a provider change must not stage a payload, mark a
        // tile unavailable or schedule a retry for a source the page replaced;
        // the frame that applies the change updates the field first.
        let raster_registration_revision = self.raster_registration_revision.get();
        let mut tile_fetches_in_flight = self.tile_fetches_in_flight.borrow_mut();
        for tile_key in requested_tiles {
            if tile_fetches_in_flight.len() >= MAXIMUM_CONCURRENT_TILE_FETCHES {
                break;
            }
            if tile_fetches_in_flight.contains(tile_key)
                || draw_state.tile_scheduler.texture_pool().contains(tile_key)
                || tile_provider.tile_is_unavailable(tile_key)
                || tile_provider.tile_fetch_is_deferred(tile_key)
            {
                continue;
            }
            tile_fetches_in_flight.insert(*tile_key);
            let tile_key = *tile_key;
            let tile_provider = tile_provider.clone();
            let tile_fetches_in_flight = self.tile_fetches_in_flight.clone();
            let canvas = canvas.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let fetched_payload = fetch_tile_payload_bytes(
                    tile_key,
                    tile_resolution,
                    recorded_tile_fixtures_are_served,
                )
                .await;
                // The provider changed while the fetch ran: the pooled state
                // and the new provider own the tile now, so this result is
                // dropped silently.
                if crate::live_providers::raster_tile_provider_registration_revision()
                    != raster_registration_revision
                {
                    tile_fetches_in_flight.borrow_mut().remove(&tile_key);
                    return;
                }
                let staged = match fetched_payload {
                    TilePayloadFetch::Fetched(encoded_image_bytes) => {
                        tile_provider.preload_tile_payload(tile_key, &encoded_image_bytes);
                        tile_provider.clear_tile_retry_schedule(&tile_key);
                        true
                    }
                    TilePayloadFetch::Failed(TilePayloadFetchFailure::NoSource) => {
                        tile_provider.record_unavailable_tile(tile_key);
                        false
                    }
                    TilePayloadFetch::Failed(TilePayloadFetchFailure::Transient) => {
                        tile_provider.record_transient_tile_failure(tile_key);
                        false
                    }
                };
                tile_fetches_in_flight.borrow_mut().remove(&tile_key);
                if staged {
                    // A page rendering on demand needs to know a payload
                    // landed; the event is the only channel the canvas has to
                    // the page's render loop.
                    if let Ok(tile_arrived_event) = web_sys::Event::new(MAP_TILE_ARRIVED_EVENT_NAME)
                    {
                        let _ = canvas.dispatch_event(&tile_arrived_event);
                    }
                }
            });
        }
    }
}

/// Fetches one tile's encoded payload: when the page serves them, the
/// same-origin recorded fixture first, then the installed raster-tile provider
/// (a registered credential or a URL-template source), so panning anywhere in
/// the world still draws imagery. A page that serves no fixtures skips the
/// fixture probe and asks the provider directly, issuing no `/tiles` request.
/// `Failed` means the tile has no payload at this address.
pub(super) async fn fetch_tile_payload_bytes(
    tile_key: TileKey,
    tile_resolution: RasterTileResolution,
    recorded_tile_fixtures_are_served: bool,
) -> TilePayloadFetch {
    let fixture_outcome = if recorded_tile_fixtures_are_served {
        let payload_url = tile_fixture_url(&tile_key, tile_resolution);
        match fetch_same_origin_bytes(&payload_url).await {
            Ok(encoded_image_bytes) => return TilePayloadFetch::Fetched(encoded_image_bytes),
            Err(fixture_error) => Some(fixture_error),
        }
    } else {
        None
    };
    let Some(provider) = registered_raster_tile_provider() else {
        // With no live source, only a definitive route answer is permanent:
        // a transient transport failure must be retried once it can succeed.
        // A skipped probe is definitive by construction.
        let fixture_has_no_record = matches!(
            fixture_outcome,
            None | Some(SameOriginTileFetchError::NotRecorded)
        );
        return TilePayloadFetch::Failed(if fixture_has_no_record {
            TilePayloadFetchFailure::NoSource
        } else {
            TilePayloadFetchFailure::Transient
        });
    };
    match provider
        .fetch_raster_tile(
            tile_key.tile_x,
            tile_key.tile_y,
            tile_key.zoom_level,
            RasterImageFormat::Png,
            tile_resolution,
        )
        .await
    {
        Ok(tile_data) => TilePayloadFetch::Fetched(tile_data.image_bytes),
        // The live contract's error does not distinguish a refused address
        // from a transport failure, so it is retried with the back-off rather
        // than blacklisting a tile a later attempt may serve.
        Err(_) => TilePayloadFetch::Failed(TilePayloadFetchFailure::Transient),
    }
}
