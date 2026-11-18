# Traffic layer caching

The traffic layer draws congestion-coloured flow polylines from whatever
provider serves `TrafficFlowProvider`. A camera asks for the segments inside
its own bounds every time it settles, so without a cache every small pan is a
provider request, and every revisit pays again. This note records how the
traffic layer's data is cached, bounded and released.

## What was wrong

- A refresh asks the provider for the current camera's bounding box. That box
  changes by a few hundred metres on every small pan, so a cache keyed by the
  exact box never hits.
- The page keeps the last answer as one overlay and replaces it on the next
  settle. While a request is in flight — and through the refresh debounce —
  the previous segments keep drawing, which reads as "nothing was cleaned up".
- On native targets the only traffic cache is the disk response store, keyed
  by the exact request fingerprint: a revisit of the same box hits, anything
  else misses, and a browser has no disk store at all.

## The cache

`rustic-maps-core::traffic_flow_cache` holds a grid-keyed, memory-bounded
cache that wraps any traffic-flow provider:

- **Cells, not boxes.** A request is mapped onto the web-mercator tile grid at
  zoom 12 (a cell is about 10 km at the equator). A city camera covers one or
  two cells, so the provider is asked for roughly what the camera shows, and a
  pan of a few hundred metres stays inside the cell already held.
- **One call per miss.** The cells that are missing or expired are fetched as
  their union bounding box in a single provider call, and the answer is stored
  per cell by each segment's first point. The caller receives exactly what the
  provider answered for that ground, plus what the fresh cells already held —
  a provider's own rows are never narrowed, so a canned or partial answer is
  preserved. A hit answers from the cells, which are coarser than a camera by
  a few kilometres; the renderer clips the difference at draw time.
- **Bounded.** At most 64 cells are kept (roughly a two-hour drive in every
  direction); the least recently used cell is evicted when the budget is full,
  and a request that would need more cells than the budget is refused with a
  message naming the budget rather than silently truncating.
- **Fresh for five minutes.** The entry's age against the injected clock
  decides freshness; the value matches the traffic category's own policy. A
  clock that steps backwards keeps an entry fresh longer instead of expiring
  everything.
- **A host clock.** The crate compiles for the browser, where the standard
  library's clock panics, so the caller supplies the clock: `Date.now()` in
  the browser, the system clock on native, a controllable counter in tests.
- **No lock across an await.** A request inspects its cells under the lock,
  drops it for the provider call, and re-takes it to store the answer, so a
  frame reading statistics never waits on the network.

Both registration paths wrap the traffic-flow provider — `MapSdk`'s native
`register_traffic_flow_provider` and the browser's `configureProvider` — so
the cache is present on every target without the reference apps knowing about
it. A rejected configuration (an impossible grid zoom, a zero budget) leaves
the provider serving uncached rather than failing registration.

## Reading it back

The browser export `trafficFlowCacheStatistics()` answers
`{hitCount, missCount, entryCount, evictionCount, segmentCount, gridZoomLevel,
maximumEntryCount, timeToLiveSeconds}`, or null while no traffic provider is
registered. The developer console shows those rows; the native surfaces read
the same counters through their own readback when the console lands.

## What it deliberately does not do

- **No renderer-side culling of overlay geometry.** World-space overlay
  vertices are clipped by the GPU, so a segment off-screen costs vertex
  processing but no fill. Re-tessellating per camera to cull would defeat the
  tessellation cache and cost more than it saves at city scale.
- **No ring prefetch.** `GridCachedTrafficFlowProvider::prefetch` exists for a
  caller that wants to warm the neighbouring cells; the reference apps warm
  only what the camera shows, because traffic data ages quickly.
- **No cross-provider sharing.** The cache belongs to one registered
  provider; re-registering replaces it, which is the settings surface's Clear
  act for traffic.

## Verification

- Unit tests in `traffic_flow_cache.rs`: cell keying and clamping, a repeated
  request as a hit, a small pan inside one cell as a hit, expiry by the
  injected clock, the entry budget with least-recently-used eviction, the
  over-budget refusal, and segment intersection.
- Browser: `live_provider.spec.mjs` covers the flow call and
  `provider_contract_surface.spec.mjs` asserts one miss then one hit with the
  cells' segments; `map_layers_and_traffic.spec.mjs` proves the real path — a
  traffic toggle and a camera pan produce two refreshes but **one** provider
  request, with the cache reporting one hit and one miss.
