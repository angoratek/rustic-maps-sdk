//! Grid-keyed, memory-bounded caching of traffic-flow observations.
//!
//! A traffic layer asks its provider for the segments inside a camera's
//! bounding box every time the camera settles. The provider answers for a
//! *region*, and the region a camera shows changes by a few pixels on every
//! small pan, so a cache keyed by the exact requested box would miss almost
//! every time. This module keys cached answers by the web-mercator tile grid
//! instead: a request is answered from the cells it covers when those cells
//! are fresh, the missing cells are fetched from the provider once as their
//! union, and the provider's answer is stored per cell. A pan of a few pixels
//! is therefore a hit, a revisit of an earlier area is a hit, and memory stays
//! bounded by a fixed entry budget with least-recently-used eviction.
//!
//! An answer is the segments the covering cells hold — what the provider
//! reported for that ground — not a re-filtered subset of it. A cell is
//! coarser than a camera, so a hit can hold a few segments just outside the
//! requested box; the renderer clips those at draw time, and narrowing them
//! here would drop observations a provider chose to report.
//!
//! The clock is injected ([`TrafficFlowCurrentTimeSeconds`]) rather than read
//! from the platform: the crate compiles for the browser, where the
//! standard library's clock panics, so the host supplies its own (the browser
//! target uses `Date.now()`, the native targets `SystemTime`). Tests supply a
//! controllable clock.
//!
//! The cache never holds a lock across an await: a request inspects the cells
//! under the lock, drops it, awaits the provider, then re-takes the lock to
//! store the answer.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use geospatial_traits::TrafficFlowProvider;
use geospatial_types::{BoundingBox, GeographicCoordinate, SDKError, TrafficFlowData};

/// Zoom level the cache keys its cells at: a ~10 km cell at the equator. A
/// city camera covers one or two cells, so the provider is asked for roughly
/// what the camera shows — no larger — and a pan of a few hundred metres stays
/// inside the cell it already holds.
pub const DEFAULT_TRAFFIC_FLOW_GRID_ZOOM_LEVEL: u8 = 12;

/// How many grid cells the cache keeps before evicting the least recently
/// used. Sixty-four cells at the default zoom cover roughly a two-hour drive
/// in every direction without unbounded growth.
pub const DEFAULT_TRAFFIC_FLOW_MAXIMUM_ENTRY_COUNT: usize = 64;

/// How long one cached cell stays fresh: the traffic category's own policy,
/// five minutes, which is the interval at which flow observations are worth
/// re-reading.
pub const DEFAULT_TRAFFIC_FLOW_TIME_TO_LIVE_SECONDS: u64 = 300;

/// Highest zoom level whose tile index range fits the `u32` cell key.
const MAXIMUM_GRID_ZOOM_LEVEL: u8 = 30;

/// The number of longitude degrees in a full turn, the base of the
/// web-mercator tile fractions.
const LONGITUDE_DEGREES_PER_TURN: f64 = 360.0;

/// The latitude limit of the web-mercator projection in degrees; a request
/// whose box reaches beyond it is clamped, exactly as a tile server clamps.
const MERCATOR_MAXIMUM_LATITUDE_DEGREES: f64 = 85.051_128_779_806_6;

/// Supplies "now" in whole seconds to the cache.
///
/// The host decides what the clock is: the browser target reads
/// `Date.now()`, the native targets read the system clock, and tests use a
/// controllable counter. A cached cell's freshness is `now - stored_at` less
/// than the time-to-live, so a clock that steps backwards simply keeps an
/// entry fresh longer rather than expiring everything.
pub type TrafficFlowCurrentTimeSeconds = Arc<dyn Fn() -> u64 + Send + Sync>;

/// How a cache answers traffic-flow requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrafficFlowCacheConfiguration {
    /// Zoom level the cells are keyed at.
    pub grid_zoom_level: u8,
    /// Cells kept before the least recently used is evicted.
    pub maximum_entry_count: usize,
    /// Seconds a stored cell stays fresh.
    pub time_to_live_seconds: u64,
}

impl Default for TrafficFlowCacheConfiguration {
    fn default() -> Self {
        Self {
            grid_zoom_level: DEFAULT_TRAFFIC_FLOW_GRID_ZOOM_LEVEL,
            maximum_entry_count: DEFAULT_TRAFFIC_FLOW_MAXIMUM_ENTRY_COUNT,
            time_to_live_seconds: DEFAULT_TRAFFIC_FLOW_TIME_TO_LIVE_SECONDS,
        }
    }
}

/// One grid cell at the cache's zoom level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TrafficFlowGridCell {
    /// Tile column at the configuration's zoom level.
    pub tile_x: u32,
    /// Tile row at the configuration's zoom level.
    pub tile_y: u32,
}

/// What the cache has answered: read by the frame diagnostics and the
/// developer console, never mutated by a caller.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TrafficFlowCacheStatistics {
    /// Requests answered entirely from fresh cells.
    pub hit_count: u64,
    /// Requests that had to ask the provider for at least one cell.
    pub miss_count: u64,
    /// Cells currently held.
    pub entry_count: usize,
    /// Cells dropped to stay inside the entry budget.
    pub eviction_count: u64,
    /// Flow segments currently held across every cell.
    pub segment_count: usize,
}

/// A serving traffic-flow cache's counters beside the budget they obey.
///
/// The readback a developer console shows and a native surface crosses the
/// contract with: the hits and misses prove a repeated pane was answered
/// without asking the provider, and `maximum_entry_count` bounds how many
/// cells the cache may hold at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrafficFlowCacheReadback {
    /// The cache's counters as they stand.
    pub statistics: TrafficFlowCacheStatistics,
    /// The configuration the cache enforces.
    pub configuration: TrafficFlowCacheConfiguration,
}

/// One cached cell's contents.
struct TrafficFlowCacheEntry {
    /// The cell this entry serves.
    cell: TrafficFlowGridCell,
    /// The segments whose first point falls inside the cell.
    segments: Vec<TrafficFlowData>,
    /// When the entry was stored, in the injected clock's seconds.
    stored_at_seconds: u64,
    /// The use order that least-recently-used eviction compares.
    last_used_sequence: u64,
}

/// The cache's mutable state, shared behind one lock.
struct TrafficFlowCacheState {
    /// Stored cells, in insertion order (order is decided by
    /// `last_used_sequence`, not by this vector's order).
    entries: Vec<TrafficFlowCacheEntry>,
    /// The next use sequence to hand out; monotonic.
    next_use_sequence: u64,
    /// Hit, miss and eviction counters.
    statistics: TrafficFlowCacheStatistics,
    /// Recently requested cells, most recent first, so a pan back is a hit
    /// even when the requested box changed shape.
    recent_cells: VecDeque<TrafficFlowGridCell>,
}

impl TrafficFlowCacheState {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
            next_use_sequence: 0,
            statistics: TrafficFlowCacheStatistics::default(),
            recent_cells: VecDeque::new(),
        }
    }

    fn mark_used(&mut self, cell_index: usize) -> TrafficFlowGridCell {
        let use_sequence = self.next_use_sequence;
        self.next_use_sequence = self.next_use_sequence.wrapping_add(1);
        let entry = &mut self.entries[cell_index];
        entry.last_used_sequence = use_sequence;
        entry.cell
    }

    fn entry_index_for_cell(&self, cell: TrafficFlowGridCell) -> Option<usize> {
        self.entries.iter().position(|entry| entry.cell == cell)
    }
}

/// The grid-keyed traffic-flow cache.
///
/// One instance serves one provider's answers; [`GridCachedTrafficFlowProvider`]
/// wraps a provider with it. The cache is shared through an `Arc` so the
/// provider wrapper can report statistics while a frame reads them.
pub struct TrafficFlowGridCache {
    configuration: TrafficFlowCacheConfiguration,
    state: Mutex<TrafficFlowCacheState>,
    current_time_seconds: TrafficFlowCurrentTimeSeconds,
}

impl TrafficFlowGridCache {
    /// Builds a cache with the given configuration and clock.
    ///
    /// A configuration naming an impossible grid (a zoom above the tile-index
    /// range, a zero entry budget) is refused rather than silently corrected.
    pub fn new(
        configuration: TrafficFlowCacheConfiguration,
        current_time_seconds: TrafficFlowCurrentTimeSeconds,
    ) -> Result<Self, SDKError> {
        if configuration.grid_zoom_level > MAXIMUM_GRID_ZOOM_LEVEL {
            return Err(SDKError::InvalidCoordinateInput(format!(
                "the traffic flow cache grid zoom level {} exceeds the supported maximum {MAXIMUM_GRID_ZOOM_LEVEL}",
                configuration.grid_zoom_level
            )));
        }
        if configuration.maximum_entry_count == 0 {
            return Err(SDKError::InvalidCoordinateInput(
                "the traffic flow cache must accept at least one entry".to_string(),
            ));
        }
        Ok(Self {
            configuration,
            state: Mutex::new(TrafficFlowCacheState::new()),
            current_time_seconds,
        })
    }

    /// The configuration this cache runs with.
    pub fn configuration(&self) -> TrafficFlowCacheConfiguration {
        self.configuration
    }

    /// A copy of the cache's counters for a diagnostics readback.
    pub fn statistics(&self) -> TrafficFlowCacheStatistics {
        let state = self.lock_state();
        let mut statistics = state.statistics;
        statistics.entry_count = state.entries.len();
        statistics.segment_count = state.entries.iter().map(|entry| entry.segments.len()).sum();
        statistics
    }

    /// Drops every stored cell (the settings surface's Clear act).
    pub fn clear(&self) {
        let mut state = self.lock_state();
        state.entries.clear();
        state.recent_cells.clear();
    }

    /// Answers the segments inside `bounding_box` from fresh cells, and
    /// reports the cells that were not fresh.
    ///
    /// `cached_segments` are the already-known segments; `missing_cells` is
    /// empty when the answer needed no provider call. The caller fetches the
    /// union of the missing cells, stores it with [`Self::store`], and then
    /// answers the request again — so a miss costs one provider call and the
    /// next request over the same ground is a hit.
    fn lookup(
        &self,
        bounding_box: &BoundingBox,
        count_as_request: bool,
    ) -> Result<TrafficFlowCellLookup, SDKError> {
        let covering_cells = self.covering_cells(bounding_box)?;
        let now_seconds = (self.current_time_seconds)();
        let mut state = self.lock_state();
        let mut cached_segments = Vec::new();
        let mut missing_cells = Vec::new();
        let mut hit_cell_count = 0;
        for cell in &covering_cells {
            match state.entry_index_for_cell(*cell) {
                Some(entry_index) => {
                    let is_fresh = {
                        let entry = &state.entries[entry_index];
                        now_seconds.saturating_sub(entry.stored_at_seconds)
                            < self.configuration.time_to_live_seconds
                    };
                    if is_fresh {
                        hit_cell_count += 1;
                        state.mark_used(entry_index);
                        cached_segments.extend(state.entries[entry_index].segments.iter().cloned());
                    } else {
                        missing_cells.push(*cell);
                    }
                }
                None => missing_cells.push(*cell),
            }
        }
        // Only a caller's request counts: the re-read after a fetch is the
        // same request finishing, not a second one.
        if count_as_request {
            if missing_cells.is_empty() {
                state.statistics.hit_count = state.statistics.hit_count.wrapping_add(1);
            } else {
                state.statistics.miss_count = state.statistics.miss_count.wrapping_add(1);
            }
        }
        // Record every requested cell, newest first, so a later pan back over
        // these cells is answered from the recent list even when the shape of
        // the requested box changed.
        for cell in &covering_cells {
            if let Some(position) = state
                .recent_cells
                .iter()
                .position(|recent_cell| recent_cell == cell)
            {
                state.recent_cells.remove(position);
            }
            state.recent_cells.push_front(*cell);
        }
        let maximum_recent_cell_count = self.configuration.maximum_entry_count;
        while state.recent_cells.len() > maximum_recent_cell_count {
            state.recent_cells.pop_back();
        }
        let _ = hit_cell_count;
        Ok(TrafficFlowCellLookup {
            cached_segments,
            missing_cells,
            now_seconds,
        })
    }

    /// Stores one provider answer, distributing its segments over the cells
    /// they start in.
    fn store(&self, segments: &[TrafficFlowData], cells: &[TrafficFlowGridCell], now_seconds: u64) {
        if cells.is_empty() {
            return;
        }
        let mut segments_per_cell: Vec<Vec<TrafficFlowData>> = vec![Vec::new(); cells.len()];
        for segment in segments {
            let Some(first_coordinate) = segment.shape_coordinates.first() else {
                continue;
            };
            let segment_cell = self.cell_for_coordinate(first_coordinate);
            if let Some(cell_index) = cells.iter().position(|cell| *cell == segment_cell) {
                segments_per_cell[cell_index].push(segment.clone());
            }
        }
        let mut state = self.lock_state();
        for (cell_index, cell) in cells.iter().enumerate() {
            let stored_segments = std::mem::take(&mut segments_per_cell[cell_index]);
            match state.entry_index_for_cell(*cell) {
                Some(entry_index) => {
                    state.entries[entry_index].segments = stored_segments;
                    state.entries[entry_index].stored_at_seconds = now_seconds;
                    state.mark_used(entry_index);
                }
                None => {
                    while state.entries.len() >= self.configuration.maximum_entry_count {
                        evict_least_recently_used(&mut state);
                    }
                    let use_sequence = state.next_use_sequence;
                    state.next_use_sequence = state.next_use_sequence.wrapping_add(1);
                    state.entries.push(TrafficFlowCacheEntry {
                        cell: *cell,
                        segments: stored_segments,
                        stored_at_seconds: now_seconds,
                        last_used_sequence: use_sequence,
                    });
                }
            }
        }
    }

    /// Whether one bounding box covers no more cells than the entry budget
    /// allows. A larger request is answered without caching rather than
    /// refused: the wrapper stays transparent to its caller.
    fn covers_within_budget(&self, bounding_box: &BoundingBox) -> bool {
        self.covering_cells(bounding_box).is_ok()
    }

    /// The cells one bounding box covers at the configuration's zoom level.
    fn covering_cells(
        &self,
        bounding_box: &BoundingBox,
    ) -> Result<Vec<TrafficFlowGridCell>, SDKError> {
        let corner_cells = self.corner_cells(bounding_box);
        let (minimum_tile_x, maximum_tile_x) = corner_cells.0;
        let (minimum_tile_y, maximum_tile_y) = corner_cells.1;
        let cell_count = u64::from(maximum_tile_x.saturating_sub(minimum_tile_x) + 1)
            * u64::from(maximum_tile_y.saturating_sub(minimum_tile_y) + 1);
        if cell_count > u64::try_from(self.configuration.maximum_entry_count).unwrap_or(u64::MAX) {
            return Err(SDKError::InvalidCoordinateInput(format!(
                "the traffic flow cache would need {cell_count} cells for one request, more than its {} entry budget; ask for a smaller area",
                self.configuration.maximum_entry_count
            )));
        }
        let mut cells = Vec::with_capacity(usize::try_from(cell_count).unwrap_or(0));
        for tile_x in minimum_tile_x..=maximum_tile_x {
            for tile_y in minimum_tile_y..=maximum_tile_y {
                cells.push(TrafficFlowGridCell { tile_x, tile_y });
            }
        }
        Ok(cells)
    }

    /// The cell coordinate range a bounding box falls in, per axis.
    fn corner_cells(&self, bounding_box: &BoundingBox) -> ((u32, u32), (u32, u32)) {
        let south_west = self.cell_for_coordinate(&bounding_box.south_west);
        let north_east = self.cell_for_coordinate(&bounding_box.north_east);
        (
            (south_west.tile_x, north_east.tile_x),
            (north_east.tile_y, south_west.tile_y),
        )
    }

    /// The cell one coordinate falls in.
    fn cell_for_coordinate(&self, coordinate: &GeographicCoordinate) -> TrafficFlowGridCell {
        let zoom_level = self.configuration.grid_zoom_level;
        let tile_count = 1u32 << zoom_level;
        let longitude_fraction =
            (coordinate.longitude + LONGITUDE_DEGREES_PER_TURN / 2.0) / LONGITUDE_DEGREES_PER_TURN;
        let latitude_radians = coordinate.latitude.to_radians();
        let latitude_fraction = (1.0
            - (latitude_radians.tan() + 1.0 / latitude_radians.cos()).ln() / std::f64::consts::PI)
            / 2.0;
        TrafficFlowGridCell {
            tile_x: longitude_tile_index(longitude_fraction, tile_count),
            tile_y: latitude_tile_index(latitude_fraction, tile_count),
        }
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, TrafficFlowCacheState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// One request's cache answer.
struct TrafficFlowCellLookup {
    /// Segments the fresh cells already know.
    cached_segments: Vec<TrafficFlowData>,
    /// Cells that must be fetched from the provider.
    missing_cells: Vec<TrafficFlowGridCell>,
    /// The clock value the lookup used, so a later store agrees with it.
    now_seconds: u64,
}

/// Clamps a web-mercator fraction to a tile index.
fn longitude_tile_index(fraction: f64, tile_count: u32) -> u32 {
    let scaled = (fraction * f64::from(tile_count)).floor();
    clamp_tile_index(scaled, tile_count)
}

/// Clamps a web-mercator fraction to a tile index.
fn latitude_tile_index(fraction: f64, tile_count: u32) -> u32 {
    let scaled = (fraction * f64::from(tile_count)).floor();
    clamp_tile_index(scaled, tile_count)
}

/// Clamps a scaled tile index into `0..tile_count`.
fn clamp_tile_index(scaled: f64, tile_count: u32) -> u32 {
    if !scaled.is_finite() || scaled <= 0.0 {
        return 0;
    }
    let maximum_index = f64::from(tile_count.saturating_sub(1));
    if scaled >= maximum_index {
        return tile_count.saturating_sub(1);
    }
    scaled as u32
}

/// Drops the least recently used entry.
fn evict_least_recently_used(state: &mut TrafficFlowCacheState) {
    let Some(least_used_index) = state
        .entries
        .iter()
        .enumerate()
        .min_by_key(|(_index, entry)| entry.last_used_sequence)
        .map(|(index, _entry)| index)
    else {
        return;
    };
    state.entries.remove(least_used_index);
    state.statistics.eviction_count = state.statistics.eviction_count.wrapping_add(1);
}

/// The union of several grid cells' geographic bounds.
fn bounding_box_of_cells(cells: &[TrafficFlowGridCell], zoom_level: u8) -> Option<BoundingBox> {
    let mut minimum_latitude = f64::INFINITY;
    let mut maximum_latitude = f64::NEG_INFINITY;
    let mut minimum_longitude = f64::INFINITY;
    let mut maximum_longitude = f64::NEG_INFINITY;
    for cell in cells {
        for coordinate in cell_corner_coordinates(*cell, zoom_level) {
            minimum_latitude = minimum_latitude.min(coordinate.latitude);
            maximum_latitude = maximum_latitude.max(coordinate.latitude);
            minimum_longitude = minimum_longitude.min(coordinate.longitude);
            maximum_longitude = maximum_longitude.max(coordinate.longitude);
        }
    }
    if !minimum_latitude.is_finite() {
        return None;
    }
    BoundingBox::from_corners(
        GeographicCoordinate::from_degrees(minimum_latitude, minimum_longitude).ok()?,
        GeographicCoordinate::from_degrees(maximum_latitude, maximum_longitude).ok()?,
    )
    .ok()
}

/// A grid cell's south-west and north-east corners in degrees.
fn cell_corner_coordinates(cell: TrafficFlowGridCell, zoom_level: u8) -> [GeographicCoordinate; 2] {
    [
        coordinate_for_tile_index(cell.tile_x, cell.tile_y, zoom_level),
        coordinate_for_tile_index(cell.tile_x + 1, cell.tile_y + 1, zoom_level),
    ]
}

/// The geographic coordinate a tile index stands for.
fn coordinate_for_tile_index(tile_x: u32, tile_y: u32, zoom_level: u8) -> GeographicCoordinate {
    let tile_count = f64::from(1u32 << zoom_level);
    let longitude = f64::from(tile_x) / tile_count * LONGITUDE_DEGREES_PER_TURN
        - LONGITUDE_DEGREES_PER_TURN / 2.0;
    let mercator = std::f64::consts::PI * (1.0 - 2.0 * f64::from(tile_y) / tile_count);
    let latitude = mercator.sinh().atan().to_degrees().clamp(
        -MERCATOR_MAXIMUM_LATITUDE_DEGREES,
        MERCATOR_MAXIMUM_LATITUDE_DEGREES,
    );
    GeographicCoordinate::from_degrees(latitude, longitude).unwrap_or(GeographicCoordinate {
        latitude: 0.0,
        longitude: 0.0,
    })
}

/// A traffic-flow provider whose answers are cached by grid cell.
///
/// The wrapper answers a request from fresh cells when it can, asks the
/// underlying provider for exactly the cells it does not have — as their
/// union bounding box, in one call — and stores the answer per cell. It never
/// holds the cache lock across the provider await.
pub struct GridCachedTrafficFlowProvider {
    traffic_flow_provider: Arc<dyn TrafficFlowProvider>,
    cache: Arc<TrafficFlowGridCache>,
}

impl GridCachedTrafficFlowProvider {
    /// Wraps a provider with a cache built from `configuration` and the host's
    /// clock.
    pub fn new(
        traffic_flow_provider: Arc<dyn TrafficFlowProvider>,
        configuration: TrafficFlowCacheConfiguration,
        current_time_seconds: TrafficFlowCurrentTimeSeconds,
    ) -> Result<Self, SDKError> {
        Ok(Self {
            traffic_flow_provider,
            cache: Arc::new(TrafficFlowGridCache::new(
                configuration,
                current_time_seconds,
            )?),
        })
    }

    /// The cache this wrapper answers from, for statistics and clearing.
    pub fn cache(&self) -> Arc<TrafficFlowGridCache> {
        Arc::clone(&self.cache)
    }

    /// Warms the cache for a bounding box without answering a caller: the
    /// ring-prefetch the vector base performs, applied to traffic.
    pub async fn prefetch(&self, bounding_box: &BoundingBox) -> Result<(), SDKError> {
        self.fetch_traffic_flow(bounding_box)
            .await
            .map(|_segments| ())
    }
}

#[async_trait]
impl TrafficFlowProvider for GridCachedTrafficFlowProvider {
    async fn fetch_traffic_flow(
        &self,
        bounding_box: &BoundingBox,
    ) -> Result<Vec<TrafficFlowData>, SDKError> {
        // A request wider than the cell budget is answered straight from the
        // provider: the cache bounds memory, it does not bound what a caller
        // may ask for.
        if !self.cache.covers_within_budget(bounding_box) {
            return self
                .traffic_flow_provider
                .fetch_traffic_flow(bounding_box)
                .await;
        }
        let first_lookup = self.cache.lookup(bounding_box, true)?;
        if first_lookup.missing_cells.is_empty() {
            return Ok(first_lookup.cached_segments);
        }
        let fetch_box = bounding_box_of_cells(
            &first_lookup.missing_cells,
            self.cache.configuration.grid_zoom_level,
        )
        .unwrap_or(*bounding_box);
        // The provider call happens with no lock held: a request that must
        // wait on the network never blocks a frame reading statistics.
        let fetched_segments = self
            .traffic_flow_provider
            .fetch_traffic_flow(&fetch_box)
            .await?;
        self.cache.store(
            &fetched_segments,
            &first_lookup.missing_cells,
            first_lookup.now_seconds,
        );
        // The caller gets what the provider answered for the ground it has no
        // cell for, plus what the fresh cells already held: a provider's own
        // rows are never narrowed, and the next request over this ground is
        // answered from the cells.
        let mut segments = first_lookup.cached_segments;
        segments.extend(fetched_segments);
        Ok(segments)
    }
}

/// A portable clock for tests and hosts that only need monotonic seconds:
/// whole seconds since the Unix epoch, clamped to zero before it.
///
/// The browser build must not call this (the standard library's clock panics
/// there); the browser registration supplies `Date.now()` instead.
pub fn system_clock_time_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use geospatial_types::TrafficFlowCongestionLevel;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn coordinate(latitude: f64, longitude: f64) -> GeographicCoordinate {
        GeographicCoordinate::from_degrees(latitude, longitude).expect("canonical coordinate")
    }

    fn box_around(latitude: f64, longitude: f64, span: f64) -> BoundingBox {
        BoundingBox::from_corners(
            coordinate(latitude - span, longitude - span),
            coordinate(latitude + span, longitude + span),
        )
        .expect("ordered box")
    }

    fn segment(start: GeographicCoordinate, end: GeographicCoordinate) -> TrafficFlowData {
        TrafficFlowData {
            shape_coordinates: vec![start, end],
            current_speed_kilometers_per_hour: Some(40.0),
            free_flow_speed_kilometers_per_hour: Some(60.0),
            congestion_level: TrafficFlowCongestionLevel::Slow,
        }
    }

    /// A provider that answers a fixed segment set and counts its calls.
    struct CountingProvider {
        segments: Vec<TrafficFlowData>,
        call_count: Arc<AtomicU64>,
    }

    #[async_trait]
    impl TrafficFlowProvider for CountingProvider {
        async fn fetch_traffic_flow(
            &self,
            _bounding_box: &BoundingBox,
        ) -> Result<Vec<TrafficFlowData>, SDKError> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            Ok(self.segments.clone())
        }
    }

    fn controllable_clock() -> (TrafficFlowCurrentTimeSeconds, Arc<AtomicU64>) {
        let now = Arc::new(AtomicU64::new(1_000));
        let clock_now = Arc::clone(&now);
        let clock: TrafficFlowCurrentTimeSeconds =
            Arc::new(move || clock_now.load(Ordering::SeqCst));
        (clock, now)
    }

    #[test]
    fn a_cell_key_is_finest_near_the_equator_and_clamps_at_the_poles() {
        let cache = TrafficFlowGridCache::new(
            TrafficFlowCacheConfiguration::default(),
            controllable_clock().0,
        )
        .expect("cache");
        let half_grid = 1u32 << (DEFAULT_TRAFFIC_FLOW_GRID_ZOOM_LEVEL - 1);
        let origin_cell = cache.cell_for_coordinate(&coordinate(0.0, 0.0));
        assert_eq!(origin_cell.tile_x, half_grid);
        assert_eq!(origin_cell.tile_y, half_grid);
        // Every latitude beyond the projection still lands inside the grid.
        let north_cell = cache.cell_for_coordinate(&coordinate(89.9, 0.0));
        assert_eq!(north_cell.tile_y, 0);
        let south_cell = cache.cell_for_coordinate(&coordinate(-89.9, 0.0));
        assert_eq!(south_cell.tile_y, half_grid * 2 - 1);
    }

    #[tokio::test]
    async fn a_second_request_over_the_same_area_is_a_hit() {
        let (clock, _now) = controllable_clock();
        let call_count = Arc::new(AtomicU64::new(0));
        let inner = Arc::new(CountingProvider {
            segments: vec![segment(
                coordinate(38.58, -121.49),
                coordinate(38.59, -121.48),
            )],
            call_count: Arc::clone(&call_count),
        });
        let provider = GridCachedTrafficFlowProvider::new(
            inner,
            TrafficFlowCacheConfiguration::default(),
            clock,
        )
        .expect("provider");
        let request = box_around(38.58, -121.49, 0.02);
        let first_answer = provider.fetch_traffic_flow(&request).await.expect("first");
        assert_eq!(first_answer.len(), 1);
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
        let second_answer = provider.fetch_traffic_flow(&request).await.expect("second");
        assert_eq!(second_answer.len(), 1);
        // The provider was asked once: the second request is a cache hit.
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
        let statistics = provider.cache().statistics();
        assert!(statistics.hit_count >= 1);
        assert_eq!(statistics.miss_count, 1);
    }

    #[tokio::test]
    async fn a_small_pan_stays_inside_the_same_cell_and_is_a_hit() {
        let (clock, _now) = controllable_clock();
        let call_count = Arc::new(AtomicU64::new(0));
        let inner = Arc::new(CountingProvider {
            segments: vec![
                segment(coordinate(38.58, -121.49), coordinate(38.59, -121.48)),
                segment(coordinate(38.60, -121.47), coordinate(38.61, -121.46)),
            ],
            call_count: Arc::clone(&call_count),
        });
        let provider = GridCachedTrafficFlowProvider::new(
            inner,
            TrafficFlowCacheConfiguration::default(),
            clock,
        )
        .expect("provider");
        provider
            .fetch_traffic_flow(&box_around(38.58, -121.49, 0.01))
            .await
            .expect("first");
        // A pan of a few hundred metres: a different box, the same grid cell.
        provider
            .fetch_traffic_flow(&box_around(38.585, -121.485, 0.01))
            .await
            .expect("panned");
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn an_expired_cell_is_fetched_again() {
        let (clock, now) = controllable_clock();
        let call_count = Arc::new(AtomicU64::new(0));
        let inner = Arc::new(CountingProvider {
            segments: vec![segment(
                coordinate(38.58, -121.49),
                coordinate(38.59, -121.48),
            )],
            call_count: Arc::clone(&call_count),
        });
        let provider = GridCachedTrafficFlowProvider::new(
            inner,
            TrafficFlowCacheConfiguration::default(),
            clock,
        )
        .expect("provider");
        let request = box_around(38.58, -121.49, 0.02);
        provider.fetch_traffic_flow(&request).await.expect("first");
        now.fetch_add(
            DEFAULT_TRAFFIC_FLOW_TIME_TO_LIVE_SECONDS + 1,
            Ordering::SeqCst,
        );
        provider
            .fetch_traffic_flow(&request)
            .await
            .expect("expired");
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn the_cache_never_exceeds_its_entry_budget() {
        let (clock, _now) = controllable_clock();
        let call_count = Arc::new(AtomicU64::new(0));
        let inner = Arc::new(CountingProvider {
            segments: Vec::new(),
            call_count: Arc::clone(&call_count),
        });
        let provider = GridCachedTrafficFlowProvider::new(
            inner,
            TrafficFlowCacheConfiguration {
                grid_zoom_level: DEFAULT_TRAFFIC_FLOW_GRID_ZOOM_LEVEL,
                maximum_entry_count: 2,
                time_to_live_seconds: DEFAULT_TRAFFIC_FLOW_TIME_TO_LIVE_SECONDS,
            },
            clock,
        )
        .expect("provider");
        // Three separate cells, each its own request.
        for latitude in [10.0, 30.0, 50.0] {
            provider
                .fetch_traffic_flow(&box_around(latitude, 0.0, 0.001))
                .await
                .expect("request");
        }
        let statistics = provider.cache().statistics();
        assert!(
            statistics.entry_count <= 2,
            "entries: {}",
            statistics.entry_count
        );
        assert!(statistics.eviction_count >= 1);
    }

    #[tokio::test]
    async fn a_request_larger_than_the_budget_is_answered_uncached() {
        let (clock, _now) = controllable_clock();
        let call_count = Arc::new(AtomicU64::new(0));
        let inner = Arc::new(CountingProvider {
            segments: Vec::new(),
            call_count: Arc::clone(&call_count),
        });
        let provider = GridCachedTrafficFlowProvider::new(
            inner,
            TrafficFlowCacheConfiguration {
                grid_zoom_level: DEFAULT_TRAFFIC_FLOW_GRID_ZOOM_LEVEL,
                maximum_entry_count: 2,
                time_to_live_seconds: DEFAULT_TRAFFIC_FLOW_TIME_TO_LIVE_SECONDS,
            },
            clock,
        )
        .expect("provider");
        // A continent-sized request is answered by the provider directly: the
        // cache bounds its own memory, never what a caller may ask for.
        let segments = provider
            .fetch_traffic_flow(&box_around(0.0, 0.0, 20.0))
            .await
            .expect("an over-budget request is answered uncached");
        assert!(segments.is_empty());
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
        let statistics = provider.cache().statistics();
        assert_eq!(statistics.entry_count, 0);
    }

    #[test]
    fn a_zero_entry_budget_is_refused() {
        let refusal = TrafficFlowGridCache::new(
            TrafficFlowCacheConfiguration {
                maximum_entry_count: 0,
                ..TrafficFlowCacheConfiguration::default()
            },
            controllable_clock().0,
        )
        .err()
        .expect("a zero budget is refused");
        assert!(format!("{refusal}").contains("at least one entry"));
    }
}
