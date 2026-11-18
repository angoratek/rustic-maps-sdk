//! Frame-driven raster tile scheduling with parent fallback.
//!
//! Each frame the scheduler receives the XYZ tiles the camera needs (usually
//! from [`crate::projection::tiles_covering`] on the camera footprint). For
//! every requested tile it reports one readiness:
//!
//! * `Present` — pooled at the requested zoom;
//! * `FallsBackTo` — a nearest pooled ancestor stands in while the detailed
//!   tile loads or while its fetch failed, so the map blurs instead of
//!   flashing holes;
//! * `Unavailable` — the provider rejected the tile this frame; a later frame
//!   may retry.
//!
//! Fetches are deduplicated within a frame (one provider call per distinct
//! tile), ordered most-detailed-first, and capped by the configured per-frame
//! fetch budget; requests beyond the cap report `Deferred`.
//!
//! # The idle render-state signal
//!
//! An idle state means no more tile loads remain, so a capture made then
//! cannot race a late fetch. This module exposes render-state queries:
//! [`TileScheduler::is_idle`] over the fetch state, and
//! [`prepared_frame_is_idle`] over a captured frame's readiness verdicts.
//!
//! What counts as pending is stated over the fetch state, because the
//! scheduler itself is frame-driven: [`TileScheduler::prepare_frame`] awaits
//! every fetch it issues, so a fetch is never observable through this API
//! mid-flight. The pending states are therefore exactly the readiness
//! verdicts whose pixels a later successful frame can still change:
//!
//! * `Deferred` — the fetch budget spent; a later frame fetches the tile.
//! * `Unavailable` — the provider rejected the tile; a later frame retries
//!   it.
//! * `FallsBackTo` — a coarser ancestor covers the area while the detailed
//!   tile loads or its fetch failed; when the fetch lands, the area re-renders
//!   at its own detail level.
//!
//! A frame is idle when every requested tile resolved `Present`: no
//! scheduled work can change its pixels. An empty request set is idle by
//! vacuity. Idle is a *query*, not an event: it can flip back to pending
//! when the pool loses a requested payload — least-recently-used budget
//! pressure, or the renderer evicting an undecodable payload after a draw —
//! until the next prepare frame refetches it.
//!
//! Explicitly not part of the verdict: camera animations
//! (`crate::camera::CameraAnimation`), which are composed entirely outside
//! the renderer; GPU upload state, which never spans calls on the headless
//! render path because uploads happen synchronously inside one frame render
//! (the WebAssembly canvas, in contrast, keeps browser fetches in flight
//! across a render frame); and evictions of payloads the current request set
//! does not ask for.
//!
//! # Synchronous resolve and detached warm fetches
//!
//! The frame path never awaits the network: [`TileScheduler::resolve_frame`]
//! answers every requested tile's readiness from the pool's current state
//! alone (plus the in-flight warming set it reports as `Deferred`), purely
//! synchronously, and the misses are fetched by [`TileScheduler::warm_tiles`]
//! (or its detached twin [`TileScheduler::plan_warm_tiles`], whose
//! [`TileWarmTask`] runs on the caller's background runtime) so detail
//! arrives in a later frame. The blocking [`TileScheduler::prepare_frame`]
//! awaits every fetch it issues; both paths report the same readiness
//! verdicts over the same pool.
//!
//! Diagnostics emit per-frame resolve and warm events at the Debug level and
//! per-tile fetch events at the Trace level under the `tile_fetch` target,
//! behind the diagnostics severity gate.

use crate::tile_key::TileKey;
use crate::tile_texture_pool::TileTexturePool;
use futures::StreamExt;
use futures::stream::FuturesUnordered;
use geospatial_traits::RasterTileProvider;
use geospatial_types::{RasterImageFormat, RasterTileResolution};
use std::collections::HashSet;
use std::future::Future;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};

/// Target the scheduler's per-frame and per-tile fetch events emit under.
const TILE_FETCH_TRACING_TARGET: &str = "tile_fetch";

/// The number of tile fetches one warm pass or blocking frame keeps in flight.
///
/// A viewport holds a dozen or more tiles, so a serial pass spends most of its
/// time waiting on one round trip at a time and the map fills in visibly tile
/// by tile. Eight concurrent fetches fill a normal viewport within one provider
/// round trip while staying inside what a public tile service and a browser's
/// per-host connection pool comfortably serve; the provider's token bucket is
/// sized to match (`DEFAULT_TOKEN_BUCKET_CAPACITY_TOKENS` and
/// `DEFAULT_REFILL_TOKENS_PER_SECOND` in the URL-template providers).
pub const TILE_FETCH_CONCURRENCY: usize = 8;

/// How a requested tile resolves for one frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileReadiness {
    /// Pooled at the requested zoom level.
    Present,
    /// Not pooled; the stated ancestor tile shows the same area at lower
    /// detail while this tile is fetched or was rejected.
    FallsBackTo {
        /// Address of the pooled ancestor tile.
        fallback_tile_key: TileKey,
        /// Zoom levels between the requested tile and its fallback.
        zoom_level_difference: u16,
    },
    /// The provider rejected the tile this frame.
    Unavailable,
    /// Left for a later frame because the per-frame fetch budget was spent.
    Deferred,
}

/// One requested tile and how it resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreparedTile {
    /// The tile the camera asked for.
    pub requested_tile_key: TileKey,
    /// How the scheduler resolved it.
    pub readiness: TileReadiness,
}

/// Schedules raster tile fetches into a [`TileTexturePool`].
pub struct TileScheduler {
    tile_provider: Arc<dyn RasterTileProvider>,
    texture_pool: TileTexturePool,
    requested_image_format: RasterImageFormat,
    requested_tile_resolution: RasterTileResolution,
    maximum_fetches_per_frame: usize,
    /// The shared registry of this scheduler's in-flight background warm
    /// tasks (see [`Self::plan_warm_tiles`]); resolve_frame reports its
    /// tiles as `Deferred` and drains its deliveries into the pool.
    warm_fetch_registry: Arc<WarmFetchRegistry>,
    /// The receiving half of the warm tasks' delivery channel; only the
    /// scheduler takes outcomes from it.
    warm_delivery_receiver: Receiver<WarmFetchOutcome>,
}

/// One background warm fetch's outcome, delivered back to the scheduler: the
/// pool payload on success, `None` on a provider failure (the tile stays
/// missing; later resolves keep reporting its ancestor or `Unavailable`).
struct WarmFetchOutcome {
    tile_key: TileKey,
    delivered_payload: Option<Vec<u8>>,
}

/// The shared in-flight state between a scheduler and its detached warm
/// tasks: the warming set every resolver reads (it decides the `Deferred`
/// verdicts), and the delivery channel the tasks' fetch outcomes flow
/// through. Locks stay inside short set operations — never across an await.
struct WarmFetchRegistry {
    warming_tile_keys: Mutex<HashSet<TileKey>>,
    delivery_sender: Sender<WarmFetchOutcome>,
    /// Monotonic count of payloads a warm task delivered to the pool: the
    /// change signal a redraw pump reads to resume a settled view (the
    /// `on_tiles_arrived` counter of the idle-settled discipline).
    delivered_tiles_counter: AtomicU64,
}

impl WarmFetchRegistry {
    /// Reports whether a tile is currently being warmed in the background.
    fn is_warming(&self, tile_key: &TileKey) -> bool {
        self.warming_tile_keys
            .lock()
            .expect("the warming set lock is not poisoned")
            .contains(tile_key)
    }

    /// Marks tiles as in-flight warm fetches.
    fn mark_warming(&self, tile_keys: &[TileKey]) {
        let mut warming_tile_keys = self
            .warming_tile_keys
            .lock()
            .expect("the warming set lock is not poisoned");
        warming_tile_keys.extend(tile_keys.iter().copied());
    }

    /// Clears one tile's in-flight flag (the scheduler does this when it
    /// drains the fetch's delivery).
    fn unmark_warming(&self, tile_key: &TileKey) {
        self.warming_tile_keys
            .lock()
            .expect("the warming set lock is not poisoned")
            .remove(tile_key);
    }

    /// The warm tasks' delivered-payments counter.
    fn delivered_tile_count(&self) -> u64 {
        self.delivered_tiles_counter.load(Ordering::Relaxed)
    }
}

impl TileScheduler {
    /// Builds a scheduler reading tiles from the provider into the pool at
    /// the requested image format and pixel resolution.
    pub fn new(
        tile_provider: Arc<dyn RasterTileProvider>,
        texture_pool: TileTexturePool,
        requested_image_format: RasterImageFormat,
        requested_tile_resolution: RasterTileResolution,
        maximum_fetches_per_frame: usize,
    ) -> Self {
        let (delivery_sender, delivery_receiver) = channel();
        Self {
            tile_provider,
            texture_pool,
            requested_image_format,
            requested_tile_resolution,
            maximum_fetches_per_frame,
            warm_fetch_registry: Arc::new(WarmFetchRegistry {
                warming_tile_keys: Mutex::new(HashSet::new()),
                delivery_sender,
                delivered_tiles_counter: AtomicU64::new(0),
            }),
            warm_delivery_receiver: delivery_receiver,
        }
    }

    /// Read-only access to the pool (recency is not touched).
    pub fn texture_pool(&self) -> &TileTexturePool {
        &self.texture_pool
    }

    /// The idle render-state signal over a request set: whether no pending
    /// tile work remains, judged against the pool's current contents — every
    /// distinct requested tile sits pooled at its own zoom level. This is the
    /// fetch-state half of the idle signal (see the module documentation for
    /// the full semantics).
    ///
    /// Compared with [`Self::prepare_frame`], which fetches misses, this
    /// query never fetches: it answers from the state a previous prepare
    /// frame left behind, or from a pool the caller filled directly.
    /// It reports false for a budget-deferred tile mid-fetch, for a tile
    /// whose fetch failed, and for a tile relying on a parent fallback, and
    /// it flips back to pending when the pool loses a requested payload.
    pub fn is_idle(&self, requested_tiles: &[TileKey]) -> bool {
        requested_tiles
            .iter()
            .all(|tile_key| self.texture_pool.contains(tile_key))
    }

    /// Borrows the pool mutably, e.g. for a renderer to upload and drain it.
    pub fn texture_pool_mut(&mut self) -> &mut TileTexturePool {
        &mut self.texture_pool
    }

    /// A snapshot of the tiles with warm fetches in flight (see
    /// [`Self::plan_warm_tiles`]): the set a prefetch planner deduplicates
    /// its own request list against, so in-flight fetches neither reappear
    /// in the plan nor consume its priority slots.
    pub fn warming_tile_keys(&self) -> std::collections::HashSet<TileKey> {
        self.warm_fetch_registry
            .warming_tile_keys
            .lock()
            .expect("the warming set lock is not poisoned")
            .clone()
    }

    /// The number of payloads this scheduler's background warm tasks have
    /// delivered into the pool: the `on_tiles_arrived` counter of the
    /// idle-settled redraw discipline (see the module documentation), which
    /// a pump pairs with [`Self::is_idle`] and the frame verdicts to resume
    /// a settled view when pending detail arrives.
    pub fn delivered_warm_tile_count(&self) -> u64 {
        self.warm_fetch_registry.delivered_tile_count()
    }

    /// Resolves every requested tile's readiness for one frame from the
    /// pool's current state alone — the await-free half of the frame path
    /// (see the module documentation): purely synchronous, GPU-free, and
    /// never touching the provider, so an uncovered camera pose reports its
    /// fallback verdicts immediately instead of stalling the frame on the
    /// network. Completed background warm deliveries (see
    /// [`Self::plan_warm_tiles`]) drain into the pool first, so the freshest
    /// fetched detail resolves into this frame.
    ///
    /// Duplicate requests keep one verdict per request (deduplication is a
    /// fetch concern, not a display one — the renderer draws per request).
    pub fn resolve_frame(&mut self, requested_tiles: &[TileKey]) -> Vec<PreparedTile> {
        self.drain_warm_fetch_deliveries();
        tracing::debug!(
            target: TILE_FETCH_TRACING_TARGET,
            requested_tile_count = requested_tiles.len(),
            pooled_tile_count = requested_tiles
                .iter()
                .filter(|tile_key| self.texture_pool.contains(tile_key))
                .count(),
            "resolve_frame answered from pool state"
        );
        requested_tiles
            .iter()
            .map(|tile_key| PreparedTile {
                requested_tile_key: *tile_key,
                readiness: self.resolve_readiness(tile_key, &[]),
            })
            .collect()
    }

    /// Fetches the requested tiles' pooled misses — the awaited half of the
    /// frame path: deduplication, the most-detailed-first order and the
    /// per-call budget (see the module documentation), with the fetches'
    /// outcomes delivering through the shared registry whose
    /// [`Self::resolve_frame`] drains them into the pool. Designed for
    /// callers that await (tests, the blocking reference path); a frame path
    /// that must not block plans the detached twin with
    /// [`Self::plan_warm_tiles`] and runs it on its own background task.
    /// Returns the number of fetches the call issued.
    pub async fn warm_tiles(&mut self, requested_tiles: &[TileKey]) -> usize {
        let warm_task = self.plan_warm_tiles(requested_tiles);
        let fetch_count = warm_task.run().await;
        self.drain_warm_fetch_deliveries();
        fetch_count
    }

    /// Plans a detached warm fetch of the requested tiles' pooled misses:
    /// the owned [`TileWarmTask`] to run on a background runtime (fire and
    /// forget — the frame path never awaits it). The plan marks its tiles
    /// in the shared registry, so every [`Self::resolve_frame`] until the
    /// delivery drains reports them `Deferred` (the pixels may change when
    /// the detail arrives), and the task's outcomes deliver back through
    /// the scheduler's channel. Returns a task fetching nothing when
    /// nothing needs warming.
    pub fn plan_warm_tiles(&mut self, requested_tiles: &[TileKey]) -> TileWarmTask {
        self.drain_warm_fetch_deliveries();
        let unique_warm_candidates: Vec<TileKey> = {
            let mut seen_tiles = HashSet::new();
            requested_tiles
                .iter()
                .copied()
                .filter(|tile_key| seen_tiles.insert(*tile_key))
                .filter(|tile_key| !self.texture_pool.contains(tile_key))
                .filter(|tile_key| !self.warm_fetch_registry.is_warming(tile_key))
                .collect()
        };
        // Most-detailed tiles first: detail arriving last would keep the map
        // visibly blurry for the whole warm pass. Stable within a zoom level
        // so callers keep their own input priority order.
        let mut warm_candidates = unique_warm_candidates;
        warm_candidates.sort_by_key(|tile_key| std::cmp::Reverse(tile_key.zoom_level));
        let fetch_budget = self.maximum_fetches_per_frame.min(warm_candidates.len());
        let fetch_tile_keys = warm_candidates[..fetch_budget].to_vec();
        self.warm_fetch_registry.mark_warming(&fetch_tile_keys);
        tracing::debug!(
            target: TILE_FETCH_TRACING_TARGET,
            planned_fetch_count = fetch_tile_keys.len(),
            deferred_candidate_count = warm_candidates.len() - fetch_tile_keys.len(),
            "warm pass planned"
        );
        TileWarmTask {
            tile_provider: Arc::clone(&self.tile_provider),
            requested_image_format: self.requested_image_format,
            requested_tile_resolution: self.requested_tile_resolution,
            fetch_tile_keys,
            warm_fetch_registry: Arc::clone(&self.warm_fetch_registry),
        }
    }

    /// Moves completed warm tasks' fetch outcomes into the pool (clearing
    /// their in-flight flags): every finished fetch's payload becomes
    /// available to this and later frames. Returns the number of delivered
    /// payloads.
    fn drain_warm_fetch_deliveries(&mut self) -> usize {
        let mut delivered_count = 0;
        while let Ok(warm_fetch_outcome) = self.warm_delivery_receiver.try_recv() {
            self.warm_fetch_registry
                .unmark_warming(&warm_fetch_outcome.tile_key);
            if let Some(delivered_payload) = warm_fetch_outcome.delivered_payload {
                self.texture_pool
                    .insert(warm_fetch_outcome.tile_key, delivered_payload);
                delivered_count += 1;
            }
        }
        delivered_count
    }

    /// Resolves every requested tile for this frame, fetching misses.
    ///
    /// Duplicate requests appear once per request in the returned frame but
    /// cost one fetch. The pool is consulted before fetching so pooled
    /// ancestors serve as fallbacks immediately.
    ///
    /// The blocking reference path (see the module documentation): every
    /// fetch it issues is awaited inside the call, so a never-covered camera
    /// pose costs the network through this method — the split's
    /// [`Self::resolve_frame`] + [`Self::warm_tiles`]/[`Self::plan_warm_tiles`]
    /// never does. Completed detached warm deliveries drain first, so a
    /// background fetch's payload lands in the pool before this frame would
    /// refetch it; in-flight warming tiles are left to their own tasks
    /// rather than double-fetched.
    pub async fn prepare_frame(&mut self, requested_tiles: &[TileKey]) -> Vec<PreparedTile> {
        self.drain_warm_fetch_deliveries();
        let unique_requested_tiles: Vec<TileKey> = {
            let mut seen_tiles = HashSet::new();
            requested_tiles
                .iter()
                .copied()
                .filter(|tile_key| seen_tiles.insert(*tile_key))
                .collect()
        };

        let mut fetch_candidates = unique_requested_tiles
            .iter()
            .copied()
            .filter(|tile_key| !self.texture_pool.contains(tile_key))
            .filter(|tile_key| !self.warm_fetch_registry.is_warming(tile_key))
            .collect::<Vec<TileKey>>();
        // Most-detailed tiles first: detail arriving last would keep the map
        // visibly blurry for the whole frame. Stable within a zoom level so
        // callers keep their own input priority order.
        fetch_candidates.sort_by_key(|tile_key| std::cmp::Reverse(tile_key.zoom_level));

        let fetch_budget = self.maximum_fetches_per_frame.min(fetch_candidates.len());
        if fetch_budget > 0 {
            let tile_provider = Arc::clone(&self.tile_provider);
            let requested_image_format = self.requested_image_format;
            let requested_tile_resolution = self.requested_tile_resolution;
            let budgeted_tile_keys = fetch_candidates[..fetch_budget].to_vec();
            // The blocking path keeps the same in-flight window as the
            // detached warm pass, so a reference or golden run fills a
            // viewport in one provider round trip instead of serially.
            let fetched_tiles =
                futures::stream::iter(budgeted_tile_keys.into_iter().map(|tile_key| {
                    let tile_provider = Arc::clone(&tile_provider);
                    async move {
                        tracing::trace!(
                            target: TILE_FETCH_TRACING_TARGET,
                            zoom_level = tile_key.zoom_level,
                            tile_x = tile_key.tile_x,
                            tile_y = tile_key.tile_y,
                            "prepare_frame tile fetch issued"
                        );
                        let outcome = tile_provider
                            .fetch_raster_tile(
                                tile_key.tile_x,
                                tile_key.tile_y,
                                tile_key.zoom_level,
                                requested_image_format,
                                requested_tile_resolution,
                            )
                            .await;
                        (tile_key, outcome)
                    }
                }))
                .buffer_unordered(TILE_FETCH_CONCURRENCY)
                .collect::<Vec<_>>()
                .await;
            for (tile_key, outcome) in fetched_tiles {
                match outcome {
                    Ok(fetched_tile) => {
                        self.texture_pool.insert(tile_key, fetched_tile.image_bytes);
                    }
                    Err(error) => {
                        // Provider failures are surfaced per-tile; the frame
                        // keeps resolving so fallbacks can cover the gap. The
                        // trace is the only signal on this blocking path, so
                        // name the tile and the failure rather than swallowing
                        // it.
                        tracing::trace!(
                            target: TILE_FETCH_TRACING_TARGET,
                            zoom_level = tile_key.zoom_level,
                            tile_x = tile_key.tile_x,
                            tile_y = tile_key.tile_y,
                            failure = %error,
                            "blocking tile fetch failed"
                        );
                    }
                }
            }
        }

        requested_tiles
            .iter()
            .copied()
            .map(|tile_key| PreparedTile {
                requested_tile_key: tile_key,
                readiness: self.resolve_readiness(&tile_key, &fetch_candidates[fetch_budget..]),
            })
            .collect()
    }

    fn resolve_readiness(
        &self,
        requested_tile_key: &TileKey,
        deferred_tiles: &[TileKey],
    ) -> TileReadiness {
        if self.texture_pool.contains(requested_tile_key) {
            return TileReadiness::Present;
        }
        if self.warm_fetch_registry.is_warming(requested_tile_key) {
            return TileReadiness::Deferred;
        }
        if deferred_tiles.contains(requested_tile_key) {
            return TileReadiness::Deferred;
        }
        // Nearest pooled ancestor stands in while this tile loads or failed.
        let mut ancestor_tile_key = requested_tile_key.parent_tile_key();
        let mut zoom_level_difference: u16 = 1;
        while let Some(candidate) = ancestor_tile_key {
            if self.texture_pool.contains(&candidate) {
                return TileReadiness::FallsBackTo {
                    fallback_tile_key: candidate,
                    zoom_level_difference,
                };
            }
            zoom_level_difference += 1;
            ancestor_tile_key = candidate.parent_tile_key();
        }
        TileReadiness::Unavailable
    }
}

/// One planned detached warm fetch: the owned inputs (the provider handle,
/// the request formats) plus the shared in-flight registry, runnable as its
/// own background task. Built by [`TileScheduler::plan_warm_tiles`]; the
/// frame path runs it fire-and-forget so no draw ever awaits the network
/// (see the module documentation).
pub struct TileWarmTask {
    tile_provider: Arc<dyn RasterTileProvider>,
    requested_image_format: RasterImageFormat,
    requested_tile_resolution: RasterTileResolution,
    /// The deduplicated, most-detailed-first, budget-capped misses to fetch.
    fetch_tile_keys: Vec<TileKey>,
    warm_fetch_registry: Arc<WarmFetchRegistry>,
}

/// The panic guard of [`TileWarmTask::run`]: it holds the not-yet-delivered
/// planned keys, and its `Drop` clears every one of them from the warming
/// set — so a task that unwinds mid-fetch (or is cancelled because its
/// runtime dropped the future) cannot leave stale in-flight marks in the
/// registry, which would otherwise report those tiles `Deferred` forever and
/// double-fetch nothing in later warm passes. Delivered keys are removed from
/// the owned list before (or regardless of) the guard's drop, and the
/// registry's own drain clears each delivered key at resolve.
struct WarmTaskUnwindGuard {
    warm_fetch_registry: Arc<WarmFetchRegistry>,
    remaining_tile_keys: Vec<TileKey>,
}

impl Drop for WarmTaskUnwindGuard {
    fn drop(&mut self) {
        let mut warming_tile_keys = self
            .warm_fetch_registry
            .warming_tile_keys
            .lock()
            .expect("the warming set lock is not poisoned");
        for tile_key in &self.remaining_tile_keys {
            warming_tile_keys.remove(tile_key);
        }
    }
}

impl TileWarmTask {
    /// The planned fetches' count (an empty task fetches nothing).
    pub fn fetch_count(&self) -> usize {
        self.fetch_tile_keys.len()
    }

    /// Runs the planned fetches to completion, delivering every outcome —
    /// payload or provider failure — back to the scheduler's channel, and
    /// returns the number of payloads delivered. The pass keeps up to
    /// [`TILE_FETCH_CONCURRENCY`] provider calls in flight, refilling as each
    /// completes, so a viewport fills within a round trip instead of one tile
    /// at a time; the scheduler drains the deliveries into its pool at its
    /// next resolve. The unwind guard (see [`WarmTaskUnwindGuard`]) keeps a
    /// dropped or panicking run from stranding stale warming marks: a key is
    /// removed from the guard when its outcome is delivered, and every key
    /// still in flight or unstarted is unmarked if the run is dropped.
    pub async fn run(self) -> usize {
        let Self {
            tile_provider,
            requested_image_format,
            requested_tile_resolution,
            warm_fetch_registry,
            fetch_tile_keys,
        } = self;
        let mut unwind_guard = WarmTaskUnwindGuard {
            warm_fetch_registry: Arc::clone(&warm_fetch_registry),
            remaining_tile_keys: fetch_tile_keys.clone(),
        };
        let mut pending_tile_keys = fetch_tile_keys.into_iter();
        let mut in_flight = FuturesUnordered::new();
        for _ in 0..TILE_FETCH_CONCURRENCY {
            let Some(tile_key) = pending_tile_keys.next() else {
                break;
            };
            in_flight.push(warm_fetch_future(
                Arc::clone(&tile_provider),
                requested_image_format,
                requested_tile_resolution,
                tile_key,
            ));
        }
        let mut delivered_count = 0;
        while let Some((tile_key, delivered_payload)) = in_flight.next().await {
            if delivered_payload.is_some() {
                warm_fetch_registry
                    .delivered_tiles_counter
                    .fetch_add(1, Ordering::Relaxed);
                delivered_count += 1;
            }
            let _ = warm_fetch_registry.delivery_sender.send(WarmFetchOutcome {
                tile_key,
                delivered_payload,
            });
            unwind_guard
                .remaining_tile_keys
                .retain(|remaining| *remaining != tile_key);
            if let Some(next_tile_key) = pending_tile_keys.next() {
                in_flight.push(warm_fetch_future(
                    Arc::clone(&tile_provider),
                    requested_image_format,
                    requested_tile_resolution,
                    next_tile_key,
                ));
            }
        }
        delivered_count
    }
}

/// Fetches one tile and answers its delivery outcome: the payload, or `None`
/// on a provider failure (the tile stays missing and a later warm pass
/// retries it). Shared by every slot of the warm pass's in-flight window.
fn warm_fetch_future(
    tile_provider: Arc<dyn RasterTileProvider>,
    requested_image_format: RasterImageFormat,
    requested_tile_resolution: RasterTileResolution,
    tile_key: TileKey,
) -> impl Future<Output = (TileKey, Option<Vec<u8>>)> + Send + 'static {
    tracing::trace!(
        target: TILE_FETCH_TRACING_TARGET,
        zoom_level = tile_key.zoom_level,
        tile_x = tile_key.tile_x,
        tile_y = tile_key.tile_y,
        "detached warm tile fetch issued"
    );
    async move {
        let delivered_payload = match tile_provider
            .fetch_raster_tile(
                tile_key.tile_x,
                tile_key.tile_y,
                tile_key.zoom_level,
                requested_image_format,
                requested_tile_resolution,
            )
            .await
        {
            Ok(fetched_tile) => Some(fetched_tile.image_bytes),
            Err(_fetch_error) => {
                tracing::trace!(
                    target: TILE_FETCH_TRACING_TARGET,
                    zoom_level = tile_key.zoom_level,
                    tile_x = tile_key.tile_x,
                    tile_y = tile_key.tile_y,
                    "detached warm tile fetch failed"
                );
                None
            }
        };
        (tile_key, delivered_payload)
    }
}

/// The idle render-state signal over a captured frame's readiness verdicts:
/// whether every requested tile of the frame resolved `Present` at its own
/// zoom level. The verdict-level twin of [`TileScheduler::is_idle`], kept
/// GPU-free so callers (golden tests, the idle-gated snapshot of
/// [`crate::headless`]) can assert determinism before any capture.
///
/// The two forms agree for a frame checked immediately after the
/// `prepare_frame` that produced it; they can diverge only if the pool
/// changed in between — an eviction leaves the recorded verdicts resolved
/// while `is_idle` reports pending again. The verdict form is exactly the
/// gate a deterministic capture needs at capture time; the pool form is what
/// a caller re-checks before a later frame.
pub fn prepared_frame_is_idle(prepared_tiles: &[PreparedTile]) -> bool {
    prepared_tiles
        .iter()
        .all(|prepared_tile| prepared_tile.readiness == TileReadiness::Present)
}

#[cfg(test)]
mod tests {
    use super::{PreparedTile, TileReadiness, TileScheduler, prepared_frame_is_idle};
    use crate::tile_key::TileKey;
    use crate::tile_scheduler::TILE_FETCH_CONCURRENCY;
    use crate::tile_texture_pool::TileTexturePool;
    use async_trait::async_trait;
    use geospatial_traits::RasterTileProvider;
    use geospatial_types::{RasterImageFormat, RasterTileData, RasterTileResolution, SDKError};
    use std::collections::HashMap;
    use std::future::pending;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    /// Provider stub returning canned tiles and recording every request.
    struct ScriptedTileProvider {
        scripted_responses: Mutex<HashMap<TileKey, Result<Vec<u8>, String>>>,
        requested_tiles: Mutex<Vec<TileKey>>,
    }

    impl ScriptedTileProvider {
        fn serving_tiles(served_tiles: impl IntoIterator<Item = (TileKey, Vec<u8>)>) -> Arc<Self> {
            Arc::new(Self {
                scripted_responses: Mutex::new(
                    served_tiles
                        .into_iter()
                        .map(|(tile_key, image_bytes)| (tile_key, Ok(image_bytes)))
                        .collect(),
                ),
                requested_tiles: Mutex::new(Vec::new()),
            })
        }

        fn failing_tiles(
            served_tiles: impl IntoIterator<Item = (TileKey, Vec<u8>)>,
            failed_tiles: impl IntoIterator<Item = TileKey>,
        ) -> Arc<Self> {
            Arc::new(Self {
                scripted_responses: Mutex::new(
                    served_tiles
                        .into_iter()
                        .map(|(tile_key, image_bytes)| (tile_key, Ok(image_bytes)))
                        .chain(
                            failed_tiles
                                .into_iter()
                                .map(|tile_key| (tile_key, Err("unroutable".to_string()))),
                        )
                        .collect(),
                ),
                requested_tiles: Mutex::new(Vec::new()),
            })
        }

        fn requested_tile_keys(&self) -> Vec<TileKey> {
            self.requested_tiles.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl RasterTileProvider for ScriptedTileProvider {
        async fn fetch_raster_tile(
            &self,
            tile_x: u32,
            tile_y: u32,
            zoom_level: u8,
            _image_format: RasterImageFormat,
            _tile_resolution: RasterTileResolution,
        ) -> Result<RasterTileData, SDKError> {
            let requested_tile_key = TileKey::new(tile_x, tile_y, zoom_level).unwrap();
            self.requested_tiles
                .lock()
                .unwrap()
                .push(requested_tile_key);
            let scripted_response = self
                .scripted_responses
                .lock()
                .unwrap()
                .get(&requested_tile_key)
                .cloned();
            match scripted_response {
                Some(Ok(image_bytes)) => Ok(RasterTileData {
                    tile_x,
                    tile_y,
                    zoom_level,
                    image_format: RasterImageFormat::Png,
                    tile_resolution: RasterTileResolution::Standard,
                    image_bytes,
                }),
                Some(Err(detail)) => Err(SDKError::NetworkRequestFailed(detail)),
                None => Err(SDKError::ResponseParsingFailure(
                    "tile not scripted".to_string(),
                )),
            }
        }
    }

    fn tile_key(tile_x: u32, tile_y: u32, zoom_level: u8) -> TileKey {
        TileKey::new(tile_x, tile_y, zoom_level).unwrap()
    }

    fn readiness_of(prepared_tiles: &[PreparedTile], requested_tile_key: TileKey) -> TileReadiness {
        prepared_tiles
            .iter()
            .find(|prepared_tile| prepared_tile.requested_tile_key == requested_tile_key)
            .map(|prepared_tile| prepared_tile.readiness)
            .expect("requested tile missing from the prepared frame")
    }

    #[tokio::test]
    async fn pooled_tiles_render_without_a_second_fetch() {
        let detailed_tile = tile_key(3, 4, 5);
        let tile_provider = ScriptedTileProvider::serving_tiles([(detailed_tile, vec![1; 64])]);
        let mut tile_scheduler = TileScheduler::new(
            tile_provider.clone(),
            TileTexturePool::new(1 << 20),
            RasterImageFormat::Png,
            RasterTileResolution::Standard,
            8,
        );
        let first_frame = tile_scheduler.prepare_frame(&[detailed_tile]).await;
        assert_eq!(
            readiness_of(&first_frame, detailed_tile),
            TileReadiness::Present
        );
        let second_frame = tile_scheduler.prepare_frame(&[detailed_tile]).await;
        assert_eq!(
            readiness_of(&second_frame, detailed_tile),
            TileReadiness::Present
        );
        assert_eq!(tile_provider.requested_tile_keys().len(), 1);
    }

    #[tokio::test]
    async fn duplicated_requests_in_one_frame_fetch_once() {
        let detailed_tile = tile_key(1, 2, 3);
        let tile_provider = ScriptedTileProvider::serving_tiles([(detailed_tile, vec![1; 64])]);
        let mut tile_scheduler = TileScheduler::new(
            tile_provider.clone(),
            TileTexturePool::new(1 << 20),
            RasterImageFormat::Png,
            RasterTileResolution::Standard,
            8,
        );
        let frame = tile_scheduler
            .prepare_frame(&[detailed_tile, detailed_tile, detailed_tile])
            .await;
        assert_eq!(frame.len(), 3);
        assert_eq!(tile_provider.requested_tile_keys().len(), 1);
    }

    #[tokio::test]
    async fn failed_detail_falls_back_to_the_pooled_parent() {
        let parent_tile = tile_key(1, 1, 2);
        let child_tile = tile_key(5, 5, 4);
        // The child is scripted to fail; the parent serves.
        let tile_provider =
            ScriptedTileProvider::failing_tiles([(parent_tile, vec![2; 64])], [child_tile]);
        let mut tile_scheduler = TileScheduler::new(
            tile_provider.clone(),
            TileTexturePool::new(1 << 20),
            RasterImageFormat::Png,
            RasterTileResolution::Standard,
            8,
        );
        let frame = tile_scheduler
            .prepare_frame(&[child_tile, parent_tile])
            .await;
        assert_eq!(readiness_of(&frame, parent_tile), TileReadiness::Present);
        match readiness_of(&frame, child_tile) {
            TileReadiness::FallsBackTo {
                fallback_tile_key,
                zoom_level_difference,
            } => {
                assert_eq!(fallback_tile_key, parent_tile);
                assert_eq!(zoom_level_difference, 2);
            }
            other => panic!("expected a parent fallback, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn fetch_budget_defers_the_surplus() {
        let first_tile = tile_key(0, 0, 3);
        let second_tile = tile_key(1, 0, 3);
        let tile_provider = ScriptedTileProvider::serving_tiles([
            (first_tile, vec![1; 64]),
            (second_tile, vec![1; 64]),
        ]);
        let mut tile_scheduler = TileScheduler::new(
            tile_provider.clone(),
            TileTexturePool::new(1 << 20),
            RasterImageFormat::Png,
            RasterTileResolution::Standard,
            1,
        );
        let frame = tile_scheduler
            .prepare_frame(&[first_tile, second_tile])
            .await;
        assert_eq!(readiness_of(&frame, first_tile), TileReadiness::Present);
        assert_eq!(readiness_of(&frame, second_tile), TileReadiness::Deferred);
        assert_eq!(tile_provider.requested_tile_keys().len(), 1);
        // The deferred tile completes on the next frame.
        let next_frame = tile_scheduler
            .prepare_frame(&[first_tile, second_tile])
            .await;
        assert_eq!(
            readiness_of(&next_frame, second_tile),
            TileReadiness::Present
        );
    }

    #[tokio::test]
    async fn unrouted_tiles_report_unavailable_per_frame() {
        let failing_tile = tile_key(2, 2, 4);
        let tile_provider = ScriptedTileProvider::failing_tiles([], [failing_tile]);
        let mut tile_scheduler = TileScheduler::new(
            tile_provider.clone(),
            TileTexturePool::new(1 << 20),
            RasterImageFormat::Png,
            RasterTileResolution::Standard,
            8,
        );
        let frame = tile_scheduler.prepare_frame(&[failing_tile]).await;
        assert_eq!(
            readiness_of(&frame, failing_tile),
            TileReadiness::Unavailable
        );
        assert_eq!(tile_provider.requested_tile_keys().len(), 1);
        // A later frame retries the tile.
        tile_scheduler.prepare_frame(&[failing_tile]).await;
        assert_eq!(tile_provider.requested_tile_keys().len(), 2);
    }

    #[tokio::test]
    async fn the_fetch_budget_leaves_the_frame_pending() {
        // Mid-fetch state made observable: the budget stops the second tile's
        // fetch, so it reports `Deferred` and the frame is not idle — later
        // frames still change its pixels even though its fetch never left the
        // scheduler in flight.
        let first_tile = tile_key(0, 0, 3);
        let deferred_tile = tile_key(1, 0, 3);
        let tile_provider = ScriptedTileProvider::serving_tiles([
            (first_tile, vec![1; 64]),
            (deferred_tile, vec![1; 64]),
        ]);
        let mut tile_scheduler = TileScheduler::new(
            tile_provider.clone(),
            TileTexturePool::new(1 << 20),
            RasterImageFormat::Png,
            RasterTileResolution::Standard,
            1,
        );
        let frame = tile_scheduler
            .prepare_frame(&[first_tile, deferred_tile])
            .await;
        assert_eq!(readiness_of(&frame, deferred_tile), TileReadiness::Deferred);
        assert!(
            !tile_scheduler.is_idle(&[first_tile, deferred_tile]),
            "a budget-deferred tile keeps the frame pending"
        );
        assert!(!prepared_frame_is_idle(&frame));
        // The fetch completes on the next frame and the signal flips to idle.
        let next_frame = tile_scheduler
            .prepare_frame(&[first_tile, deferred_tile])
            .await;
        assert!(tile_scheduler.is_idle(&[first_tile, deferred_tile]));
        assert!(prepared_frame_is_idle(&next_frame));
    }

    #[tokio::test]
    async fn a_failed_fetch_keeps_the_frame_pending() {
        let failing_tile = tile_key(2, 2, 4);
        let tile_provider = ScriptedTileProvider::failing_tiles([], [failing_tile]);
        let mut tile_scheduler = TileScheduler::new(
            tile_provider.clone(),
            TileTexturePool::new(1 << 20),
            RasterImageFormat::Png,
            RasterTileResolution::Standard,
            8,
        );
        let frame = tile_scheduler.prepare_frame(&[failing_tile]).await;
        assert_eq!(
            readiness_of(&frame, failing_tile),
            TileReadiness::Unavailable
        );
        assert!(
            !tile_scheduler.is_idle(&[failing_tile]),
            "a provider failure keeps the frame pending: a later frame retries it"
        );
        assert!(!prepared_frame_is_idle(&frame));
    }

    #[tokio::test]
    async fn a_fallback_frame_is_pending_until_its_detail_arrives() {
        let parent_tile = tile_key(1, 1, 2);
        let failing_child_tile = tile_key(5, 5, 4);
        let tile_provider =
            ScriptedTileProvider::failing_tiles([(parent_tile, vec![2; 64])], [failing_child_tile]);
        let mut tile_scheduler = TileScheduler::new(
            tile_provider.clone(),
            TileTexturePool::new(1 << 20),
            RasterImageFormat::Png,
            RasterTileResolution::Standard,
            8,
        );
        let frame = tile_scheduler
            .prepare_frame(&[failing_child_tile, parent_tile])
            .await;
        assert!(
            !tile_scheduler.is_idle(&[failing_child_tile]),
            "a parent fallback still owes the child's own detail"
        );
        assert!(!prepared_frame_is_idle(&frame));
        // The parent-only request set *is* idle: nothing is owed for it.
        assert!(tile_scheduler.is_idle(&[parent_tile]));
    }

    #[test]
    fn the_readiness_verdicts_decide_the_frame_verdict_alone() {
        let fallback_child_tile = tile_key(5, 5, 4);
        let prepared_tile = |tile_readiness| PreparedTile {
            requested_tile_key: fallback_child_tile,
            readiness: tile_readiness,
        };
        for (tile_readiness, expected_idle) in [
            (TileReadiness::Present, true),
            (
                TileReadiness::FallsBackTo {
                    // (5, 5, 4)'s own parent.
                    fallback_tile_key: fallback_child_tile
                        .parent_tile_key()
                        .expect("the fallback child is not the root tile and has a parent"),
                    zoom_level_difference: 1,
                },
                false,
            ),
            (TileReadiness::Unavailable, false),
            (TileReadiness::Deferred, false),
        ] {
            assert_eq!(
                prepared_frame_is_idle(&[prepared_tile(tile_readiness)]),
                expected_idle,
                "verdict {tile_readiness:?}"
            );
        }
        // An empty request set is idle by vacuity, matching `is_idle`.
        assert!(prepared_frame_is_idle(&[]));
    }

    #[tokio::test]
    async fn an_evicted_tile_flips_idleness_back_to_pending() {
        let first_tile = tile_key(0, 0, 3);
        let second_tile = tile_key(1, 0, 3);
        let third_tile = tile_key(2, 0, 3);
        let tile_provider = ScriptedTileProvider::serving_tiles([
            (first_tile, vec![1; 64]),
            (second_tile, vec![1; 64]),
            (third_tile, vec![1; 64]),
        ]);
        let mut tile_scheduler = TileScheduler::new(
            tile_provider.clone(),
            TileTexturePool::new(1 << 20),
            RasterImageFormat::Png,
            RasterTileResolution::Standard,
            8,
        );
        tile_scheduler
            .prepare_frame(&[first_tile, second_tile, third_tile])
            .await;
        assert!(tile_scheduler.is_idle(&[first_tile, second_tile, third_tile]));
        // An eviction of a requested payload — budget pressure, or the
        // renderer's corrupt-payload eviction — turns the signal pending
        // again; the next prepare frame refetches it.
        assert!(tile_scheduler.texture_pool_mut().evict(&second_tile));
        assert!(!tile_scheduler.is_idle(&[first_tile, second_tile, third_tile]));
        let refetched_frame = tile_scheduler
            .prepare_frame(&[first_tile, second_tile, third_tile])
            .await;
        assert_eq!(
            readiness_of(&refetched_frame, second_tile),
            TileReadiness::Present
        );
        assert!(tile_scheduler.is_idle(&[first_tile, second_tile, third_tile]));
    }

    #[tokio::test]
    async fn an_eviction_outside_the_request_set_does_not_disturb_idleness() {
        let requested_tile = tile_key(0, 0, 3);
        let unrelated_tile = tile_key(6, 6, 3);
        let tile_provider = ScriptedTileProvider::serving_tiles([
            (requested_tile, vec![1; 64]),
            (unrelated_tile, vec![1; 64]),
        ]);
        let mut tile_scheduler = TileScheduler::new(
            tile_provider.clone(),
            TileTexturePool::new(1 << 20),
            RasterImageFormat::Png,
            RasterTileResolution::Standard,
            8,
        );
        tile_scheduler
            .prepare_frame(&[requested_tile, unrelated_tile])
            .await;
        assert!(tile_scheduler.texture_pool_mut().evict(&unrelated_tile));
        assert!(
            tile_scheduler.is_idle(&[requested_tile]),
            "an eviction of a payload the request set does not ask for leaves the frame idle"
        );
    }

    /// Provider stub whose fetches never answer: it models a stalled
    /// network the detached warm run parks on, so its future can be
    /// cancelled mid-await deterministically.
    struct StalledTileProvider;

    #[async_trait]
    impl RasterTileProvider for StalledTileProvider {
        async fn fetch_raster_tile(
            &self,
            _tile_x: u32,
            _tile_y: u32,
            _zoom_level: u8,
            _image_format: RasterImageFormat,
            _tile_resolution: RasterTileResolution,
        ) -> Result<RasterTileData, SDKError> {
            // A break-less loop over a never-yielding pending future is the
            // permanent stall itself: the run parks at its await point until
            // it is cancelled.
            loop {
                pending::<()>().await;
            }
        }
    }

    /// Provider stub that panics on fetch for one tile and serves a canned
    /// payload for every other, modelling an unexpected provider bug.
    struct PanickingTileProvider {
        panicking_tile_key: TileKey,
    }

    #[async_trait]
    impl RasterTileProvider for PanickingTileProvider {
        async fn fetch_raster_tile(
            &self,
            tile_x: u32,
            tile_y: u32,
            zoom_level: u8,
            _image_format: RasterImageFormat,
            _tile_resolution: RasterTileResolution,
        ) -> Result<RasterTileData, SDKError> {
            let requested_tile_key = TileKey::new(tile_x, tile_y, zoom_level).unwrap();
            if requested_tile_key == self.panicking_tile_key {
                panic!("the provider bug fires");
            }
            Ok(RasterTileData {
                tile_x,
                tile_y,
                zoom_level,
                image_format: RasterImageFormat::Png,
                tile_resolution: RasterTileResolution::Standard,
                image_bytes: vec![1; 64],
            })
        }
    }

    /// The vtable and data pointer of the never-waking waker: every vtable
    /// call is a no-op, so a manual poll never touches the data pointer.
    const NOOP_WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
        |_| RawWaker::new(std::ptr::null::<()>(), &NOOP_WAKER_VTABLE),
        |_| {},
        |_| {},
        |_| {},
    );

    /// A `std` task waker that never wakes anything: enough to drive one
    /// `poll` of a future to its first await point.
    fn noop_waker() -> Waker {
        // SAFETY: every vtable function is a no-op operating on a null data
        // pointer, which the waker contract permits since nothing ever
        // dereferences it.
        unsafe { Waker::from_raw(RawWaker::new(std::ptr::null::<()>(), &NOOP_WAKER_VTABLE)) }
    }

    #[tokio::test]
    async fn the_detached_warm_run_delivers_its_detail_through_the_resolve() {
        let warm_tile = tile_key(5, 6, 7);
        let tile_provider = ScriptedTileProvider::serving_tiles([(warm_tile, vec![1; 64])]);
        let mut tile_scheduler = TileScheduler::new(
            tile_provider.clone(),
            TileTexturePool::new(1 << 20),
            RasterImageFormat::Png,
            RasterTileResolution::Standard,
            8,
        );
        let resolved_before_warm = tile_scheduler.resolve_frame(&[warm_tile]);
        assert_eq!(
            readiness_of(&resolved_before_warm, warm_tile),
            TileReadiness::Unavailable
        );
        let warm_task = tile_scheduler.plan_warm_tiles(&[warm_tile]);
        let resolved_while_warming = tile_scheduler.resolve_frame(&[warm_tile]);
        assert_eq!(
            readiness_of(&resolved_while_warming, warm_tile),
            TileReadiness::Deferred,
            "a tile with a warm fetch in flight reports its in-flight state"
        );
        tokio::spawn(warm_task.run())
            .await
            .expect("the scripted warm run never panics");
        let resolved_after_warm = tile_scheduler.resolve_frame(&[warm_tile]);
        assert_eq!(
            readiness_of(&resolved_after_warm, warm_tile),
            TileReadiness::Present
        );
        assert_eq!(tile_scheduler.delivered_warm_tile_count(), 1);
        // The pooled detail is never refetched, not even by the blocking
        // reference path.
        let _ = tile_scheduler.prepare_frame(&[warm_tile]).await;
        assert_eq!(tile_provider.requested_tile_keys(), vec![warm_tile]);
    }

    /// A provider that only answers once the pool's whole window is in
    /// flight, so a serial pass would park forever; the test proves the warm
    /// run keeps [`TILE_FETCH_CONCURRENCY`] fetches alive at once.
    struct WindowedWarmProvider {
        barrier: tokio::sync::Barrier,
        fetches_in_flight: std::sync::atomic::AtomicUsize,
        maximum_fetches_in_flight: std::sync::atomic::AtomicUsize,
    }

    impl WindowedWarmProvider {
        fn new(window: usize) -> Arc<Self> {
            Arc::new(Self {
                barrier: tokio::sync::Barrier::new(window),
                fetches_in_flight: std::sync::atomic::AtomicUsize::new(0),
                maximum_fetches_in_flight: std::sync::atomic::AtomicUsize::new(0),
            })
        }
    }

    #[async_trait]
    impl RasterTileProvider for WindowedWarmProvider {
        async fn fetch_raster_tile(
            &self,
            tile_x: u32,
            tile_y: u32,
            zoom_level: u8,
            _image_format: RasterImageFormat,
            _tile_resolution: RasterTileResolution,
        ) -> Result<RasterTileData, SDKError> {
            use std::sync::atomic::Ordering;
            let in_flight = self.fetches_in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.maximum_fetches_in_flight
                .fetch_max(in_flight, Ordering::SeqCst);
            self.barrier.wait().await;
            self.fetches_in_flight.fetch_sub(1, Ordering::SeqCst);
            Ok(RasterTileData {
                tile_x,
                tile_y,
                zoom_level,
                image_format: RasterImageFormat::Png,
                tile_resolution: RasterTileResolution::Standard,
                image_bytes: vec![0x5a; 64],
            })
        }
    }

    #[tokio::test]
    async fn the_detached_warm_run_fills_the_fetch_pool_window() {
        use std::sync::atomic::Ordering;
        let windowed_tile_keys: Vec<TileKey> = (0..(TILE_FETCH_CONCURRENCY as u32 * 2))
            .map(|tile_x| tile_key(tile_x, 10, 6))
            .collect();
        let tile_provider = WindowedWarmProvider::new(TILE_FETCH_CONCURRENCY);
        let mut tile_scheduler = TileScheduler::new(
            tile_provider.clone(),
            TileTexturePool::new(1 << 20),
            RasterImageFormat::Png,
            RasterTileResolution::Standard,
            64,
        );
        let warm_task = tile_scheduler.plan_warm_tiles(&windowed_tile_keys);
        assert_eq!(warm_task.fetch_count(), windowed_tile_keys.len());
        let delivered_count =
            tokio::time::timeout(std::time::Duration::from_secs(5), warm_task.run())
                .await
                .expect("the warm pass fills the pool window instead of parking");
        assert_eq!(delivered_count, windowed_tile_keys.len());
        assert_eq!(
            tile_provider
                .maximum_fetches_in_flight
                .load(Ordering::SeqCst),
            TILE_FETCH_CONCURRENCY,
            "the warm pass keeps exactly the pool's window in flight"
        );
    }

    #[tokio::test]
    async fn a_warm_run_with_provider_failures_resolves_the_state_honestly() {
        let failing_tile = tile_key(2, 3, 6);
        let tile_provider = ScriptedTileProvider::failing_tiles([], [failing_tile]);
        let mut tile_scheduler = TileScheduler::new(
            tile_provider.clone(),
            TileTexturePool::new(1 << 20),
            RasterImageFormat::Png,
            RasterTileResolution::Standard,
            8,
        );
        let warm_task = tile_scheduler.plan_warm_tiles(&[failing_tile]);
        let joined = tokio::spawn(warm_task.run()).await.expect("no panic");
        assert_eq!(joined, 0, "a failed fetch delivers no payload");
        assert_eq!(tile_scheduler.delivered_warm_tile_count(), 0);
        // The failed tile's warming flag cleared at the drain, so a later
        // warm pass may retry it.
        let retried_warm_task = tile_scheduler.plan_warm_tiles(&[failing_tile]);
        assert_eq!(retried_warm_task.fetch_count(), 1);
    }

    #[tokio::test]
    async fn a_cancelled_warm_run_unmarks_its_remaining_tiles() {
        let stalled_tile = tile_key(9, 3, 6);
        let mut tile_scheduler = TileScheduler::new(
            Arc::new(StalledTileProvider),
            TileTexturePool::new(1 << 20),
            RasterImageFormat::Png,
            RasterTileResolution::Standard,
            8,
        );
        let warm_task = tile_scheduler.plan_warm_tiles(&[stalled_tile]);
        let mut run_future = Box::pin(warm_task.run());
        let never_waking_waker = noop_waker();
        let mut never_waking_context = Context::from_waker(&never_waking_waker);
        assert_eq!(
            run_future.as_mut().poll(&mut never_waking_context),
            Poll::Pending,
            "the stalled fetch parks the run at its await point"
        );
        // Dropping the run future mid-fetch (the detached task's runtime
        // shutting down, or a panic in a later poll) must leave no stale
        // warming mark behind.
        drop(run_future);
        let resolved_after_cancel = tile_scheduler.resolve_frame(&[stalled_tile]);
        assert_eq!(
            readiness_of(&resolved_after_cancel, stalled_tile),
            TileReadiness::Unavailable,
            "a cancelled warm fetch must not leave the tile marked warming"
        );
        assert_eq!(
            tile_scheduler
                .plan_warm_tiles(&[stalled_tile])
                .fetch_count(),
            1,
            "the unmarked tile is fetchable again"
        );
    }

    #[tokio::test]
    async fn a_panicking_warm_run_unmarks_its_remaining_tiles_in_the_registry() {
        let panicking_tile = tile_key(7, 3, 6);
        let served_tile = tile_key(8, 3, 6);
        let mut tile_scheduler = TileScheduler::new(
            Arc::new(PanickingTileProvider {
                panicking_tile_key: panicking_tile,
            }),
            TileTexturePool::new(1 << 20),
            RasterImageFormat::Png,
            RasterTileResolution::Standard,
            8,
        );
        let warm_task = tile_scheduler.plan_warm_tiles(&[panicking_tile, served_tile]);
        let joined = tokio::spawn(warm_task.run()).await;
        assert!(
            joined.is_err(),
            "the panicking fetch must end the run with a panic"
        );
        // The unwind guard unmarked every still-undelivered key, so both
        // tiles resolve without in-flight marks and are fetchable again.
        let resolved_after_panic = tile_scheduler.resolve_frame(&[panicking_tile, served_tile]);
        assert_eq!(
            readiness_of(&resolved_after_panic, panicking_tile),
            TileReadiness::Unavailable
        );
        assert_eq!(
            readiness_of(&resolved_after_panic, served_tile),
            TileReadiness::Unavailable
        );
        assert_eq!(
            tile_scheduler
                .plan_warm_tiles(&[panicking_tile, served_tile])
                .fetch_count(),
            2,
            "the unmarked tiles are fetchable again"
        );
    }
}
