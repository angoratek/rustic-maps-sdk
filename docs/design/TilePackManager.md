# Offline tile packs

Status: implemented. The pack surface is reachable through `OfflineManager`,
and the pack byte budget is ruled below. Date: 2026-11-03; byte-budget ruling
2026-11-17.

## What this gives you

An application can define an offline region, download its raster tiles with
live progress, cancel and resume the download, list the regions stored on
disk with their byte footprint and effective retention, and delete a region.
The tiles live in the same SQLite response cache the live map already uses,
so a packed tile and a cache tile are the same store entry: the renderer's
scheduler reads both through the registered raster-tile slot with no
integration.

A download reports honest counts of tiles fetched from the provider versus
tiles the store already held. A provider or network failure keeps the tiles
that did arrive and the membership that names the missing ones, so the next
call resumes from exactly what is present. Pack membership is pinned against
the cache's ordinary least-recently-used eviction; only the region's explicit
deletion, or the entry's own time-to-live, removes it. The browser has no
SDK-owned disk store, so the pack surface is honestly absent there.

## What exists today

`crates/rustic-maps-cache/src/tile_packs.rs` implements the whole storage
behaviour:

| Piece | What it does |
|---|---|
| `TilePackDescription` | Region id, display name, bounding box, zoom range, image format, resolution, requested retention. |
| `enumerate_region_tiles` | Web-mercator tile enumeration, coarse zoom first, bounded by `MAXIMUM_TILE_PACK_TILE_COUNT` (65,536) and the structural zoom maximum. |
| `TilePackManager::prepare_tile_pack` | Defines the pack and its membership, then downloads through the cached provider, reporting `TilePackProgress` once up front and once per completed tile. |
| `TilePackManager::tile_pack_state` / `tile_pack_states` | On-disk record readback: member count, live tile count, payload bytes, effective retention. |
| `TilePackManager::evict_tile_pack` | Explicit removal of the pack, its membership and every member entry no other live pack still claims. |
| `TilePackManager::with_maximum_zoom_level` | Refuses a pack deeper than the installed provider's declared maximum. |
| `TilePackManager::with_pack_byte_budget_bytes` / `pack_byte_budget_bytes` | Reserves and reads the payload-byte budget each pack may hold beside the general cache. The default is unbounded (`u64::MAX`), so the structural tile cap is the only size bound until the application reserves one. A download that would cross a configured budget stops, withdraws the crossing tile, keeps the partial pack and its membership, and reports a cache write refusal; a later call under a raised budget resumes it. |
| `TilePackManager::effective_time_to_live_seconds` | Requested retention clamped to the provider's declared caching-terms ceiling. |

The manager is reachable from the SDK: `OfflineManager` exposes the region
members below and builds a `TilePackManager` from the installed tile cache for
each call. No application constructs one directly, because the store and the
provider handles are private to `MapSdk`, and a pack must use the installed
store to share fingerprints and eviction.

`MapSdk` owns the installed tile cache. `set_provider_credential_with_tile_cache`
opens a `SqliteResponseCache`, wraps the raster-tile provider in
`CachedRasterTileProvider`, and records a private `TileCacheInstallation`
(the uncached provider, the store, the current time-to-live and the declared
terms ceiling). `OfflineManager` in `crates/rustic-maps-native-core/src/engines.rs`
delegates `tile_cache_terms` and `set_tile_time_to_live` to that installation.
`ProviderCatalogEntry` carries both the terms ceiling and the provider's
declared maximum zoom level; `MAPQUEST_RASTER_TILE_MAXIMUM_ZOOM_LEVEL` (20)
lives beside them in `crates/rustic-maps-native-core/src/provider_catalog.rs`
and reaches the cache path.

## The integration decision

| Option | Verdict | Reason |
|---|---|---|
| Store one `TilePackManager` on `TileCacheInstallation` | Rejected | `set_raster_tile_cache_time_to_live` rebuilds the cached wrapper; a stored manager would hold the previous wrapper and its time-to-live. |
| Build the manager on demand from the installation snapshot | Chosen | The manager is `Arc`s plus two scalars. A snapshot always carries the current wrapper time-to-live, the blocking executor and the provider facts, with no extra lifecycle. |
| Construct the manager in application code | Rejected | The store and the provider handles are private to `MapSdk`, and a pack must use the installed store to share fingerprints and eviction. |
| Give packs a second store | Rejected | It would duplicate the schema, quota, eviction policy and fingerprint scheme that make a packed tile indistinguishable from a cache tile. |

### Where the manager lives

The installation snapshot lives in `crates/rustic-maps-native-core/src/provider.rs`:

- `TileCacheInstallation` gains `raster_tile_maximum_zoom_level: Option<u8>` and
  `tile_pack_byte_budget_bytes: u64`.
- A helper `tile_pack_manager()` clones the uncached provider and the store,
  builds a `CachedRasterTileProvider` with the installation's current
  time-to-live and `crate::runtime::blocking_work_executor()`, constructs
  `TilePackManager::new(..., provider_terms_ceiling_seconds)`, applies
  `.with_pack_byte_budget_bytes(tile_pack_byte_budget_bytes)` and
  `.with_maximum_zoom_level(...)` when the provider declares one.
- `MapSdk` gains `set_tile_pack_byte_budget(byte_budget)` and
  `tile_pack_byte_budget()`, which validate and update or read the
  installation. `OfflineManager` delegates both. A zero budget is refused with
  `InvalidCoordinateInput`; a missing tile cache is `ProviderNotConfigured`
  naming `TileCache`.
- The helper is called at the start of each exported pack member, so a
  time-to-live or budget override is observed by the next call with no rebuild
  hook.

Cancellation state is separate from the installation. `MapSdk` gains a
native-only field, `region_downloads: Mutex<HashMap<String, RegionDownloadCancellation>>`,
holding the cancellation signal and the completion state for each in-flight
region. Keeping it on `MapSdk`, not on the installation, means an
`OfflineManager` constructed over the same `MapSdk` sees the same in-flight
downloads. Replacing the installation (a new credential or cache
registration) signals every in-flight download to cancel and clears the map,
so a download never writes into a store the SDK has stopped serving.

The wasm build has no `TileCacheInstallation`; every pack member answers the
same `ProviderNotConfigured` naming `TileCache` that the existing terms
surface answers. The UDL members still compile for wasm (the generated
scaffolding is shared); the hand-maintained TypeScript façade neither declares
nor pretends to serve them.

### Provider facts

- `ProviderCatalogEntry` gains `raster_tile_maximum_zoom_level: Option<u8>`.
  The MapQuest entry supplies `mapquest_provider::tiles::MAPQUEST_RASTER_TILE_MAXIMUM_ZOOM_LEVEL`
  (20). An entry without a declared maximum leaves the manager's structural
  bound in force and lets the provider refuse a deeper tile at fetch time.
- The terms ceiling already reaches the manager through
  `TileCacheInstallation::provider_terms_ceiling_seconds`.
- A region's `image_format` and `tile_resolution` must equal the pair the
  base map's scheduler requests; the fingerprint is built from the requested
  pair, and a mismatch makes packed tiles invisible to the scheduler. The
  credentialed catalog path honors the requested pair
  (`MapQuestRasterTileProvider` encodes the format and resolution into the
  request path). Registering a URL-template raster source drops the installed
  tile cache, so packs do not run over that path today; if they ever do, the
  region pair must also equal the source's declared pair.

### Blocking and concurrency

The manager's store work must leave the async reactor exactly as the cached
wrapper's does:

- `TilePackManager` gains `blocking_work_executor: Arc<dyn BlockingWorkExecutor>`
  (default `InlineBlockingWorkExecutor`) and `with_blocking_work_executor`,
  mirroring `CachedRasterTileProvider`.
- `prepare_tile_pack` routes `define_tile_pack`,
  `refresh_member_tile_freshnesses` and the final record read through
  `rustic_maps_core::blocking_work::run_blocking`.
- The binding layer routes the manager's synchronous `tile_pack_states` and
  `evict_tile_pack` calls through `run_blocking` inside its `[Async]` members;
  a foreign caller therefore never runs SQLite on its own thread.

One download per region id at a time. A second `download_region` for the same
id while one is in flight is refused with `CacheOperationFailure` naming the
region. Different regions may run concurrently: each defines only its own
membership, the store is a connection pool, and the provider's token bucket
serializes the network anyway. The manager holds no lock across an await.

## The contract surface

The addition is on `OfflineManager`, using the region vocabulary the public
API already adopted (`docs/PublicApiV2.md` uses `downloadRegion`,
`RegionDefinition` and region deletion). "Tile pack" stays an internal name.

```udl
dictionary RegionDefinition {
    string region_id;
    string display_name;
    BoundingBox bounding_box;
    u8 minimum_zoom_level;
    u8 maximum_zoom_level;
    RasterImageFormat image_format;
    RasterTileResolution tile_resolution;
    u64? time_to_live_seconds;
};

dictionary RegionRecord {
    string region_id;
    string display_name;
    BoundingBox bounding_box;
    u8 minimum_zoom_level;
    u8 maximum_zoom_level;
    RasterImageFormat image_format;
    RasterTileResolution tile_resolution;
    u64 effective_time_to_live_seconds;
    u64 member_tile_count;
    u64 present_tile_count;
    u64 present_payload_bytes;
};

enum RegionDownloadPhase {
    "Downloading",
    "Completed",
    "Cancelled",
    "Failed",
};

dictionary RegionDownloadProgress {
    string region_id;
    u64 total_tile_count;
    u64 completed_tile_count;
    u64 network_fetched_tile_count;
    u64 disk_served_tile_count;
    RegionDownloadPhase phase;
};

dictionary RegionRemovalReport {
    u64 removed_entry_count;
    u64 removed_payload_bytes;
};

[Trait, WithForeign]
interface RegionDownloadProgressListener {
    void on_region_download_progress(RegionDownloadProgress progress);
};

interface OfflineManager {
    constructor(MapSdk map_sdk);

    TileCacheTerms tile_cache_terms();
    void set_tile_time_to_live(u64 time_to_live_seconds);

    // Addition-only pack members.
    [Throws=SdkError]
    void set_tile_pack_byte_budget(u64 byte_budget);

    [Throws=SdkError]
    u64 tile_pack_byte_budget();

    [Throws=SdkError]
    u64 region_tile_count(RegionDefinition region);

    [Async, Throws=SdkError]
    RegionRecord download_region(
        RegionDefinition region,
        RegionDownloadProgressListener progress_listener
    );

    void cancel_region_download(string region_id);

    [Async, Throws=SdkError]
    sequence<RegionRecord> region_records();

    [Async, Throws=SdkError]
    RegionRecord? region_record(string region_id);

    [Async, Throws=SdkError]
    RegionRemovalReport delete_region(string region_id);
};
```

`region_tile_count` is synchronous: it only validates the definition and
enumerates tiles, so a settings surface can warn before a large download.
Every member that touches SQLite is `[Async]`, so the generated Kotlin is a
`suspend` function, the Swift an `async` function and the React Native call a
promise. Per-member `[Async, Throws=SdkError]` is the only form the pinned
UniFFI line accepts.

### The listener

`RegionDownloadProgressListener` follows the existing `[Trait, WithForeign]`
pattern (`SdkDiagnosticEventListener`, `NavigationListener`): the binding
crate defines the real `Send + Sync` trait, and the foreign class is passed as
an `Arc`.

- The listener receives one `Downloading` report with zero completions before
  the first tile, one per completed tile, and exactly one terminal report:
  `Completed`, `Cancelled` or `Failed`.
- Callbacks run on an SDK worker thread (the crate-owned runtime), never on
  the foreign UI thread. They must not block and must not call back into the
  same `OfflineManager`; a platform surface marshals to its UI thread.
- The counts are the manager's own honest counts: `network_fetched` versus
  `disk_served` distinguish work from a re-prepare that reused the store.

### Cancellation and resume

The contract adds no cancellation token type and no new `SdkError` variant.
`cancel_region_download(region_id)` signals the in-flight download; the
binding races the prepare future against that signal with `tokio::select!`
inside the runtime work. Dropping `prepare_tile_pack` is cancellation-safe by
design: each tile fetch is atomic, the store writes are transactional, and
the membership already records the missing tiles. `cancel_region_download` is
idempotent and a no-op for an unknown or finished region.

Cancellation is a successful partial result, not an error:

- the terminal progress phase is `Cancelled`;
- `download_region` returns the region's current `RegionRecord`, whose
  `present_tile_count` is below `member_tile_count`;
- if cancellation arrived before the prepare defined the pack, the binding
  defines it first so the partial region is visible and resumable;
- a later `download_region` with the same definition serves present tiles
  from disk and fetches only the missing ones.

The freshness refresh runs in batches of 256 tiles. A cancellation between
batches can leave up to 255 fetched tiles on the wrapper's time-to-live until
the next prepare; they are present and pinned, and the resume refreshes them.

## Storage, budget and eviction

- **Same store, same keys.** Membership uses
  `cached_providers::tile_request_descriptor(...).to_fingerprint()`, exactly
  the cached wrapper's scheme. A prepared tile is a live entry the scheduler
  reads through the registered slot. `rustic-maps-rendering/tests/tile_pack_scheduler_integration.rs`
  pins that equivalence.
- **Pinning.** Every member fingerprint present in `tile_pack_members` is
  excluded from the store's least-recently-used deletion. Budget pressure
  evicts ordinary entries first; pack entries leave only through
  `delete_region` or their own expiry.
- **Pack byte budget.** Each pack has its own payload-byte budget reserved
  beside the general cache's `CacheQuota`. The application sets it through
  `set_tile_pack_byte_budget`; the default is
  `DEFAULT_TILE_PACK_BYTE_BUDGET_BYTES` (`u64::MAX`), so until one is set the
  structural 65,536-tile cap is the only size limit. Enforcement is per tile as
  the download stores it: a tile that would take the pack past the budget is
  withdrawn again, the download stops with `CacheOperationFailure` /
  `WriteRejected`, and the membership keeps naming the missing tile. A pack
  already over a newly lowered budget refuses immediately and changes nothing.
  Raising the budget and calling `download_region` again resumes the remaining
  tiles. Ordinary cache eviction still never removes a present pack row, so the
  reserve is not reclaimed by general pressure. Pack bytes are observable
  through the record's `present_payload_bytes`.
- **Retention.** A pack's effective retention is the requested value or the
  raster-tiles category default, clamped to the provider's declared ceiling.
  A zero is refused. Pack entries follow that value; an expired member is a
  cache miss on the next prepare and is fetched and refreshed.
- **Re-prepare of a present region.** Idempotent: nothing is re-downloaded,
  the effective retention is refreshed, and the record reports disk-served
  tiles.
- **Overlapping packs.** A tile two packs claim is fetched once, is pinned
  while either pack lives, and is deleted only when the last claiming pack is
  deleted.
- **Redefinition.** Re-defining an id with a different shape replaces the
  membership. Dropped memberships unpin the old tiles; the entries are never
  deleted by the redefinition, so ordinary eviction reclaims them.
- **Reopen.** Pack records and membership survive a store reopen; an
  interrupted download resumes after a process restart.

## Failure modes

| Situation | What the caller sees | On-disk state |
|---|---|---|
| No tile cache installed | `ProviderNotConfigured`, contract name `TileCache` | Nothing written |
| Unknown region on `delete_region` | `InvalidCoordinateInput` naming the id | Unchanged |
| Unknown region on `region_record` | `null` | Unchanged |
| Inverted or unrepresentable zoom range | `InvalidCoordinateInput` | Nothing written |
| Maximum zoom above the provider's declared maximum | `InvalidCoordinateInput` | Nothing defined |
| Region above the 65,536-tile cap | `InvalidCoordinateInput` | Nothing written |
| Zero requested retention | `InvalidCoordinateInput` | Nothing defined |
| Zero pack byte budget | `InvalidCoordinateInput` | Nothing defined; the previous budget stays in force |
| Pack byte budget would be exceeded | `CacheOperationFailure` with `WriteRejected` | The crossing tile is withdrawn; the partial pack and membership are kept and resume under a raised budget |
| Pack already above a newly lowered budget | `CacheOperationFailure` with `WriteRejected` | Unchanged; raising the budget resumes it |
| Provider or network failure mid-download | The provider's typed error | Fetched tiles and membership kept; resumable |
| Store write failure | `CacheOperationFailure` naming the missing tile count | Partial pack kept; resumable |
| Cancellation | Success, terminal phase `Cancelled` | Partial pack kept; resumable |
| Corrupted pack row | `CacheOperationFailure` with `RecordCorrupted` | Reported, never clamped |
| A region already downloading | `CacheOperationFailure` naming the region | The in-flight download continues |
| Browser build | `ProviderNotConfigured`, contract name `TileCache` | No disk store exists |

A prepare that returns without error but leaves `present_tile_count` below
`member_tile_count` is reported as `CacheOperationFailure`, not success: the
store's write path is best-effort by design (a cache write failure never
fails a provider call), so the pack manager is the layer that turns an
incomplete pack into an honest failure.

## Platform exposure

| Target | What ships |
|---|---|
| Android (Kotlin) | Generated `OfflineManager` members (`suspend` functions) and the synchronous pack byte-budget pair; the app implements `RegionDownloadProgressListener` and hops to the main thread. The reference app calls the generated surface directly through `MapsSdkFacade`'s `nativeSdk`. |
| iOS (Swift) | Generated `OfflineManager` members (`async`) and the synchronous pack byte-budget pair; the app implements the listener and hops to `@MainActor`. |
| React Native | Hand-written bridge: `MapsSdkNativeModule` / `MapsSdkNativeRuntime` start the generated download, forward progress through the existing `DeviceEventEmitter`, expose `cancelRegionDownload`, and resolve the promise with the final record. `platform/rn/src/MapsSdk.ts` wraps it with a typed `downloadRegion` / `cancelRegionDownload`. The generated contract binding carries the pack byte-budget pair; the curated `MapsSdk` proxy does not add a wrapper for it. |
| WebAssembly | No pack surface. The hand-maintained TypeScript façade stays without it; the published offline-maps page states the limitation. `contract_sync.mjs` is addition-tolerant, so the UDL growth is not a mirror failure. |

## Reference application

The three native reference apps extend the existing Offline maps card
(`ReferenceAppUX.md` §4.1) with the same scenario and values:

1. A region row loaded from `platform/reference-scenes.json`
   (`offline_regions`: id, display name, bounding box, zoom range, retention).
   The demo region is small enough to be one tile column at the demo zoom, so
   a device proof is quick and cheap.
2. **Download offline region** starts `download_region`; the card shows
   `completed / total tiles`, the network-versus-disk split, and a **Cancel**
   control while in flight.
3. Completion shows the stored tile count, payload bytes and effective
   retention. Failure shows the §6.3 verbatim error strip.
4. **Delete region** calls `delete_region` and returns the card to the
   store-empty state.
5. The existing live tile line and the recorded-graph route control stay.

New user-facing labels go in `platform/reference-tokens.json` under
`ui_labels`, and `scripts/check_ux_parity.mjs` keeps the three mirrors
identical. The browser card is unchanged and continues to state that it has
no tile-cache or pack accounting.

## Tests and proofs

| Layer | What it proves | Where |
|---|---|---|
| Manager unit tests | Enumeration, ceiling, resume, pinning, overlap, redefinition, reopen, retention clamp; the existing suite. | `crates/rustic-maps-cache/src/tile_packs.rs` |
| Manager additions | Store calls route through the blocking executor; cancellation drops mid-download and resumes; an incomplete prepare is a typed failure; the provider zoom ceiling comes from configuration. | same module |
| Fixture-backed integration | A region prepared over committed tile fixtures served by a loopback HTTP server, then read back after a store reopen and resolved by the scheduler with no further provider request. | a new test beside `crates/rustic-maps-rendering/tests/tile_pack_scheduler_integration.rs`, reusing the loopback pattern in `crates/rustic-maps-native-core/tests/url_template_raster_tile_source.rs` |
| Binding tests | No-cache refusal, `region_tile_count` validation, zoom-ceiling refusal, list/read/delete, duplicate-download refusal, cancellation and phase sequence, wasm refusal; a test-only tile-cache installation over a scripted raster provider keeps these network-free. | `crates/rustic-maps-native-core/tests/` |
| Contract gate | The UDL additions regenerate Kotlin and Swift; the wasm mirror still agrees. | `scripts/build-android.sh`, `scripts/build-ios.sh`, `node --test platform/web/tests/contract_sync.mjs` |
| React Native tests | Progress event → typed callback, promise resolution, cancellation call, JSON record round trip. | `platform/rn/app-shell/__tests__/` |
| Device proof | A clean external app installs a cache, downloads the region with live progress, force-quits, relaunches with the network unavailable, renders the region, deletes it, and returns to network rendering. Evidence recorded. | `tmp/offline-pack-proof/` |

The device proof is the milestone's verification: it exercises the production
installation path rather than a test seam, and no test reaches the external
network.

## Open decisions

Items 1 and 2 are resolved; the rest need a product-owner ruling. Each
unresolved item is echoed in the milestone report.

1. **Pack byte budget — resolved 2026-11-17.** Each pack has a separate
   payload-byte budget beside the general cache, configured by the application
   through `set_tile_pack_byte_budget`. A download that would cross it stops,
   keeps the partial pack and its membership, and reports
   `CacheOperationFailure`; a later call under a raised budget resumes it. The
   default is unbounded (`u64::MAX`) until the application sets one.
2. **Pack size cap — resolved 2026-11-17.** The structural 65,536-tile cap
   remains the only tile-count limit, and the default pack byte budget is
   unbounded. An application that wants a lower byte bound sets one; the SDK
   adds no lower product cap of its own.
3. **Re-download and refresh.** A pack's entries expire at the provider's
   terms ceiling (thirty days for MapQuest). Should the SDK refresh a pack
   automatically before expiry, or must the application call
   `download_region` again? Would an automatic bulk refresh risk the
   provider's terms?
4. **Background downloads.** The design runs a download only while the app
   process is alive and resumes after relaunch. Must iOS and Android continue
   a pack download while the app is backgrounded or terminated
   (`BGTaskScheduler`, `WorkManager`, a foreground service), or is resumable
   foreground-only acceptable for this milestone?
5. **Browser scope.** Confirm the browser surface stays without a pack
   manager (no IndexedDB or OPFS store) and states the limitation in the
   published page, or is a browser pack store in scope?
6. **Cancellation contract shape.** Confirm cancellation is a successful
   partial result (terminal phase `Cancelled`, partial pack kept and
   resumable) rather than a new `SdkError` variant. This keeps the error
   surface addition-only.
7. **Concurrent downloads.** Allow different regions to download
   concurrently (the provider's rate limiter serializes the network), or
   serialize all pack downloads in the SDK? Same-region concurrency is
   refused either way.
8. **Redefinition policy.** Keep the current "redefine replaces membership,
   old entries stay until ordinary eviction", or refuse a shape change for an
   existing region id and require an explicit delete and re-download?

## Where the code lives

| File | Change |
|---|---|
| `crates/rustic-maps-cache/src/tile_packs.rs` | Blocking executor, cancellation-safe completion, incomplete-pack detection. |
| `crates/rustic-maps-cache/src/sqlite_store.rs` | No schema change; the pack tables and pinning already exist. `entry_payload_size_bytes` reads the stored size of the tile the budget check measures. |
| `crates/rustic-maps-native-core/src/provider.rs` | Installation zoom ceiling and pack byte budget, `tile_pack_manager()` snapshot helper, cancellation registry, installation-replacement cancellation. |
| `crates/rustic-maps-native-core/src/provider_catalog.rs` | `raster_tile_maximum_zoom_level` per catalog entry. |
| `crates/rustic-maps-native-core/src/engines.rs` | `OfflineManager` pack members, the pack byte-budget delegation and the listener adapter. |
| `crates/rustic-maps-native-core/src/records.rs` | `RegionDefinition`, `RegionRecord`, `RegionDownloadProgress`, `RegionDownloadPhase`, `RegionRemovalReport`. |
| `crates/rustic-maps-native-core/src/rustic_maps.udl` | The addition-only contract wave above. |
| `crates/rustic-maps-native-core/src/runtime.rs` | No change; the existing `drive` and `blocking_work_executor` seams serve the new calls. |
| `platform/providers.json` | Declare the raster maximum zoom for the credentialed provider if the registry is the shared source of the value. |
| `platform/reference-scenes.json` | `offline_regions` demo data. |
| `platform/reference-tokens.json` | New offline-card labels. |
| `platform/android/.../ReferenceApplicationScreen.kt`, `.../ReferenceApplicationState.kt` | Card controls, progress, cancel, delete. |
| `platform/ios/reference-application/Sources/ReferenceApplicationModel.swift`, `HomeSheetView.swift` | Same scenario. |
| `platform/rn/src/MapsSdk.ts`, `platform/rn/android/.../MapsSdkNativeModule.kt`, `platform/rn/ios/MapsSdkNativeRuntime.swift`, `platform/rn/app-shell/App.tsx` | Bridge methods, progress events, card controls. |
| `site/book-src/offline-maps.md` | Download, progress, cancel, list, delete and restart-offline steps on the native tabs; the browser limitation. |
| `docs/PublicApiV2.md` | Reconcile the pack section with the landed shape. |
| `docs/ReferenceAppUX.md` | The card's new states and labels. |
