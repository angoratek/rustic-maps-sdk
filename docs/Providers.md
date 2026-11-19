# Providers

The SDK draws maps and answers search, routing and traffic calls through a provider
layer built on vendor-neutral contracts. `platform/providers.json` describes every
provider: key, display name, credential shape, platform availability, attribution, terms
and fixture routes. The SDK registers nine provider keys (MapQuest, HERE, Google Maps,
TomTom, Mapbox, Radar, OSRM, Valhalla, Photon) and three tile sources (OpenStreetMap
Standard raster, OpenStreetMap US vector, OpenFreeMap vector). Applications select a
provider per capability, may mix vendors on one map, and never carry a credential in a
build.

## The registry

`platform/providers.json` is the single descriptor source for the reference
applications, development server, test harness and playground. It declares
`schema_version`, `fixture_base_path` (`/providers`),
`reference_app_user_agent_template`, `keyless_base_map` and `providers`.

### What a descriptor declares

The fields a settings surface renders, the dispatch reads and the harness serves:

```json
{
  "provider_key": "mapquest",
  "display_name": "MapQuest",
  "credential_label": "API key",
  "registration_available": {
    "android": true, "ios": true, "react_native": true, "wasm": true
  },
  "attribution_text": "Map data © OpenStreetMap contributors (ODbL) — tile imagery © MapQuest",
  "terms_note": "MapQuest terms apply: rate limits and a 30-day raster-tile caching ceiling.",
  "settings_hint": "Create a key in the MapQuest developer console.",
  "wasm_registration": "mapquest"
}
```

It also declares `served_contracts` and `fixture_routes` (method, path pattern, the
served `contract` the route answers and the committed fixture). A declared contract with
no committed payload is recorded in `fixtureless_contracts` with its `contract` and a
reviewed `reason`, so a live-only source stays explicit instead of silently uncovered.
Optional: `maximum_zoom_level`, `vector_styles`,
`style_resource_hosts`, `url_template`, `glyphs_url_template`, `raster_url_template`,
`vector_style`, `raster_tile_cache_ceiling_days`, `demo_base_url`, `demo_base_url_note`.
No credential value is stored; `platform/reference-scenes.json` carries no descriptor.

### The keyless base map

`keyless_base_map` names the provider key and style id a keyless application paints, for
example `{ "provider_key": "openfreemap", "style_id": "bright-light" }`. The application
registers the credential-less raster descriptor and installs this vector style once the
map view exists; an omitted field keeps the default raster start. The browser demo
starts raster and offers the vector bases one tap away.

### Consumers

The reference applications render one settings row per descriptor and use
`wasm_registration` for browser dispatch. `platform/web/serve.mjs` serves each
`fixture_routes` entry under `/providers/{provider_key}/{provider-relative path}`.
Adding a provider is a provider crate, a descriptor and a registration dispatch arm.

## Credentials

### Runtime entry on every target

A gear control near the readiness chip opens settings: provider credentials,
persistence, map SDK settings, attribution and terms. Each row shows display name,
credential label, a masked secure entry, status, Apply and Clear; a field is never
echoed in full, a confirmation shows at most the last four characters, paste is allowed
and there is no show-key affordance.

- Apply calls `configure_provider_set`, or `set_provider_credential_with_tile_cache`,
  flips the status and retries initialization; a refusal is shown verbatim.
- Clear removes the credential and UI state; there is no unregister operation, so a
  restart is required to become keyless.
- Without a key the application starts providerless, renders cached or recorded imagery,
  reports "Set a provider key to load live map data" and surfaces
  `ProviderNotConfigured` verbatim.
- An unregistered provider renders disabled-with-reason.

| Target | Entry seam | Persistence |
|---|---|---|
| Android (Compose) | `applyProviderCredential(providerKey, credential)` | Keystore AES/GCM private preferences |
| iOS (SwiftUI) | `applyProviderCredential(_:_:)` | Keychain, `kSecClassGenericPassword` |
| React Native | `setProviderCredential(providerKey, credential)` `@ReactMethod` | `getStoredCredential`/`setStoredCredential`/`clearStoredCredential` |
| WebAssembly | settings panel over `configureProvider` | Browser `localStorage`, explicit opt-in only |

The iOS Keychain service is `com.angoratek.rustic-maps.refapp.provider-credentials`;
React Native uses Keychain on iOS and Android Keystore AES/GCM on Android.

### Persistence rules

Session-only is the default. "Remember on this device" (default off) persists to the
secure store, so off means the value is gone at process end; a failed secure store is
reported honestly and falls back to session-only. The browser exception is
`localStorage` on the demo origin, opt-in and warned as readable; native keeps the
secure store, and automation injection is never persisted.

### Automation injection

Android accepts a launch-intent extra `providerApiKey` (`adb shell am start -e
providerApiKey …`, or `ActivityScenario`) through the settings registration path;
optional `providerKey` defaults to MapQuest. iOS accepts `PROVIDER_API_KEY`, optionally
with `PROVIDER_KEY`; React Native reads both. The value is never persisted, logged or
compiled; without it the run is keyless and provider-dependent cases skip. CI supplies
it only to the UI-test job.

```bash
# Android: the intent extra is forwarded from the host environment.
PROVIDER_API_KEY=<credential> ./gradlew :reference-application:connectedDebugAndroidTest \
  -Pandroid.testInstrumentationRunnerArguments.providerApiKey="$PROVIDER_API_KEY"

# iOS: TEST_RUNNER_ is required, because xcodebuild does not forward the
# shell environment to the UI-test runner; a bare PROVIDER_API_KEY skips.
TEST_RUNNER_PROVIDER_API_KEY=<credential> TEST_RUNNER_PROVIDER_KEY=mapquest \
xcodebuild test -project ReferenceApplication.xcodeproj \
  -scheme ReferenceApplication -sdk iphonesimulator \
  -destination 'platform=iOS Simulator,name=iPhone 17 Pro'
```

Keyed captures use the same seams for one session and publish no credential;
`Release.md` records commands and revision.

### The hosted-demo exception

The Appetize-hosted demo is the one artifact that may carry a credential, being a build
inside Appetize rather than a downloadable distribution.

- **Dedicated:** issued for the demo alone, rotated on schedule, never the fixture or
  live-smoke key.
- **Injected, never stored:** `scripts/inject-hosted-demo-credential.sh` writes
  `assets/hosted-demo-credential.json` into the Android and React Native Android apps
  from `HOSTED_DEMO_PROVIDER_API_KEY` and `HOSTED_DEMO_PROVIDER_KEY` (default MapQuest);
  iOS reads `HostedDemoProviderAPIKey` from `Info.plist`, expanded from the same build
  setting. Both paths are gitignored, produced only inside
  `.github/workflows/hosted-demo.yml`, and `--clean` removes the Android files.
- **Runtime unchanged:** applied through the settings registration path, never persisted
  or logged, gone at process end; without it the build is keyless.
- **Permissions off:** `appPermissions.adbConnect`, `networkProxy` and `debugLog` stay
  disabled and no API path serves the binary.

### What must never carry a credential

No credential in a build input, source file, fixture, doc page, log, diagnostic, error
detail, crash report, screenshot or the shared demo artifact; no build field, plist key,
`local.properties` line or `.xcconfig` carries one. GitHub Release, npm/Maven/SwiftPM
package, docs download, repository, fixture and screenshot stay keyless and
runtime-entered. The hosted-demo key is never the fixture-recording key and never enters
a release or package.

### Browser transport

Transport is `reqwest`/`tokio` natively and `window.fetch` on WebAssembly; key
injection, retries, pacing, timeouts and status mapping live in the shared client.

```rust
pub trait HttpTransport: Send + Sync {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, TransportError>;
}
```

`HttpRequest`/`HttpResponse` are byte and header records. The browser drops the
Fetch-forbidden names (`user-agent`, `cookie`, `host`, `origin`, `referer`,
`connection`) and cannot set `content-length`, `accept-encoding`, `proxy-*` or `sec-*`;
native still sends the declared user agent. A browser-entered key is visible to the page
and network panel; CORS and that exposure are inherent. The goal is to avoid shipping a
key by default, not to promise secrecy from the page's own user.

`scripts/smoke-providers.mjs` exercises every registry provider's served contracts in a
browser and `scripts/live-style-smoke.mjs` fetches a live style, sprite pair, glyph
range and tile; both read keys from `~/.everymap/config.toml` (provider smoke), never
print or write them, and never run in CI.

## Coverage matrix

Cell entries name the everymap type the SDK's bridge wraps; a dash means no registration
path, and registration is refused rather than stubbed.

| Provider | Key | Geocoding | Search and type-ahead | Routing | Matrix | Traffic | Raster tiles | Vector tiles | Credential |
|---|---|---|---|---|---|---|---|---|---|
| MapQuest | `mapquest` | `MapQuestGeocoder` | `MapQuestPlaceSearchProvider`, `MapQuestAutocompleteProvider` | `MapQuestRouter` | `MapQuestDistanceMatrix` | `MapQuestTrafficProvider` | `MapQuestRasterTileProvider` | — | API key, query parameter `key` |
| Photon | `photon` | `PhotonGeocoder` | `PhotonPlaceSearchProvider`, `PhotonAutocompleteProvider` | — | — | — | — | — | None, or a deployment URL |
| OSRM | `osrm` | — | — | `OsrmRouter` | — | — | — | — | None, or a deployment URL |
| Valhalla | `valhalla` | — | — | `ValhallaRouter` | — | — | — | — | None, or a deployment URL |
| HERE | `here` | `HereGeocoder` | — | `HereRouter` | — | `HereTraffic` | — | `HereTileProvider` | API key, query parameter `apiKey` |
| Google Maps | `google` | `GoogleGeocoder` | — | `GoogleRouter` | — | — | — | — | API key, query parameter `key` |
| TomTom | `tomtom` | `TomTomGeocoder` | — | `TomTomRouter` | — | `TomTomTraffic` | `TomTomTileProvider` | — | API key, query parameter `key` |
| Mapbox | `mapbox` | `MapBoxGeocoder` | — | `MapBoxRouter` | — | — | — | `MapBoxTileProvider` | Access token, query parameter `access_token` |
| Radar | `radar` | `RadarGeocoder` | — | `RadarRouter` | — | — | — | — | API key, `Authorization` header |
| OpenStreetMap Standard | `openstreetmap` | — | — | — | — | — | `UrlTemplateRasterTileProvider` | — | None |
| OpenStreetMap US | `osmus` | — | — | — | — | — | — | `UrlTemplateVectorTileProvider` | None |
| OpenFreeMap | `openfreemap` | — | — | — | — | — | — | `UrlTemplateVectorTileProvider` | None |

The three tile sources are base-map sources in the layers control, not capability-picker
rows, and serve no callable contract besides tiles.

### Credential shapes

One string in the credential slot:

| Shape | Providers | Value |
|---|---|---|
| Query-parameter key | MapQuest, HERE, Google Maps, TomTom, Mapbox | The issued API key; the provider crate names its parameter |
| Header key | Radar | The issued API key, sent as `Authorization` |
| Deployment address | OSRM, Valhalla | The deployment base URL; an empty value reaches the crate's local default, so a shared deployment must be named |
| Deployment address or public instance | Photon | The deployment base URL; an empty value reaches the public Photon instance |
| None | OpenStreetMap Standard, OpenStreetMap US, OpenFreeMap | Empty |

Defaults: `http://localhost:5000` (OSRM), `http://localhost:8002` (Valhalla),
`https://photon.komoot.io` (Photon). `demo_base_url` and `demo_base_url_note` name a
public server for keyless providers so a shared deployment is always named;
`credential_label` controls the field, and a blank label means keyless.

### Availability, attribution and terms

`registration_available`, `attribution_text` and `terms_note` come from the descriptor.
OpenStreetMap Standard is unavailable on WebAssembly; OpenStreetMap US and OpenFreeMap
are native-only. The HERE notice `© 20XX HERE, XXX` substitutes year and licensee and
must be refreshed within 24 hours.

### Registration model

- `configure_provider(contract, provider_key, credential)` fills one capability, leaving
  every other slot serving.
- `configure_provider_set(provider_key, credential)` fills every capability the catalog
  row serves.
- `set_provider_credential` is removed, because a catalog with several credentialed
  providers cannot infer the set; `set_provider_credential_with_tile_cache` remains.
- A provider that does not serve the contract is refused before any slot changes with
  `ProviderNotConfigured` on native and `unsupportedCapability` in the browser. An empty
  credential is legal only for a keyless provider.

### Per-capability selection

The registry holds one slot per contract; `configure_provider` replaces one, so a map
can mix a geocoding vendor, a URL-template tile source and another router. A refused key
leaves the previous provider and throws. `provider_registrations()` reports each
capability in stable order with serving key, display name and installation state:
Geocoding, Routing, Distance matrix, Place search, Traffic incidents, Traffic flow,
Elevation, Raster tiles, Vector tiles, Offline routing. Unconfigured reads "Not
configured"; a picker offers only descriptors declaring the capability for the platform,
and the choice joins the versioned preferences record. Eight contract families accept a
host implementation through the UDL `[Trait, WithForeign]` interface and
`register_*_from_foreign`, which writes the same slot, last write wins. Vector tiles and
offline routing keep dedicated calls.

### What makes a provider selectable

1. The crate is a workspace dependency of `rustic-maps-native-core` and
   `rustic-maps-web-bindings`.
2. The native catalog row has key, display name, credential requirement, served
   contracts, install and install-all calls, and tile ceilings.
3. The wasm dispatch row has the same key and display name.
4. `platform/providers.json` has a descriptor with display name, credential label,
   per-platform availability, attribution, terms, settings hint, `wasm_registration` and
   fixture routes.
5. The iOS resource mirror is refreshed and the platform-parity gate passes.
6. Android and web dispatch read the descriptor; iOS and React Native carry a
   registration arm per credentialed provider.
7. The provider-name guard passes.
8. The provider-capability pages, the add-a-provider guide and the Rust page describe
   it.

The catalog table is the only other place a provider name reaches the binding; compiling
a crate does not make it selectable.

### The provider layer behind the SDK

- **`everymap-core`** owns contracts and portable plumbing: transport seam, pacing,
  retry, timeouts, request observation and sixteen contract traits.
- **`everymap-providers-*`** owns vendor URLs, credentials, status mapping, attribution
  and terms; eleven crates are published.
- **`rustic-maps-core::everymap_bridge`** adapts eight families: geocoding, place search
  with type-ahead, routing, distance matrix, traffic incidents and flow, raster tiles
  and vector tiles.
- **`geospatial-traits`** is the in-repo contract for host implementations and UDL
  callbacks.

## Provider-backed tile sources

### Raster

`MapView` draws the registered `RasterTileProvider` slot. MapQuest and TomTom register
into it; a URL-template source registers over any `{z}/{x}/{y}` template. HERE raster is
a provider-crate addition, not an SDK concept, and that upstream is not yet carried. A
registered raster provider wins over a page's live URL-template fallback.

OpenStreetMap Standard is a live-only base map: its tiles are served by
`tile.openstreetmap.org` under the OpenStreetMap tile usage policy, and the registry
commits no tile for it. Its descriptor records `raster_tiles` in `fixtureless_contracts`
with that reason. No test serves another provider's recorded tile under its key: the
browser fallback case answers the intercepted live host with a generated placeholder
image.

### Vector

`MapView.set_provider_vector_tile_source(source)` installs a named tileset;
`ProviderVectorTileSource` names the `provider_key` and built-in `style_id`, and the
registry refuses an unregistered provider.
`MapView.set_provider_vector_tile_source_from_document(source, style_document_url)`
installs the provider's own style document; the committed-style member is the fallback
where a vendor's terms refuse hosted resources, and for host-registered providers.

Application code can serve vector tiles without a provider crate by implementing the
`VectorTileProvider` callback contract (a synchronous `source_descriptor()` and an
asynchronous `fetch_vector_tile`) and registering through
`MapSdk::register_vector_tile_provider_from_foreign`; the selection is recorded under
the application key `application`, and `vector_tile_provider_is_registered()` reports
the slot. Declared encoding and style hints are advisory: each `VectorTileData` reports
its own encoding and the decoder unwraps per tile, while the installed style id selects
the document. The browser has no foreign-callback seam; a page uses `configureProvider`
or the URL-template source.

The URL-template record carries terms text, `{s}` subdomains, per-request headers and
style hints; null or empty fields declare none and every value is handed to the provider
layer. The browser consults no named declaration, so extras are caller-declared; native
inherits a matching keyless tileset's recorded extras.

### Layers control and style families

A provider with `raster_tiles` becomes a raster base and one with `vector_tiles` a
vector base; the row states the credential state and attribution. A vendor family
installs through the document member once its credential is applied.

| Key | Base | Style family | Style ids | Style document | Max zoom |
|---|---|---|---|---|---|
| `openstreetmap` | Raster | — | — | `https://tile.openstreetmap.org/{z}/{x}/{y}.png` | 19 |
| `osmus` | Vector | `carto` | `carto-light`, `carto-dark` | Committed | 15 |
| `openfreemap` | Vector | `openmaptiles` | `bright-light`, `fiord-dark`, `positron-light` | Committed | 14 |
| `here` | Vector | `omv` | `omv-light` (HERE Base) | `https://assets.vector.hereapi.com/styles/berlin/base/mapbox/tilezen` | 17 |
| `mapbox` | Vector | `streets` | `streets-light` (Mapbox Streets) | `https://api.mapbox.com/styles/v1/mapbox/streets-v11` | 22 |

The keyless URL-template providers keep the committed OpenMapTiles and Carto documents:
Bright, Fiord, Positron and Carto, with style ids `bright-light`, `fiord-dark`,
`positron-light`, `carto-light` and `carto-dark`.

## Style documents and assets

### Loading the provider's own style document

The committed styles target OpenMapTiles and Carto; HERE serves OMV and Mapbox serves
Streets, so those tiles need the vendor's own style. The document is fetched through the
provider's authenticated style-resource client, registered beside the provider
(`VendorImplementations.style_resources`).
`rustic_maps_core::StyleResourceClient::fetch_style_resource(url)` is the SDK-side
contract; the vendor client applies its authentication provider, transport, token
bucket, retry policy, timeout and request observer, with credential query values
redacted from the observed URL. A `document_url` must be `https` and carry no query,
because the client appends the credential.

A compiled document is kept per provider key and address for the session, so a repeated
install costs no request; a malformed document is never remembered. A missing provider,
missing style-resource fetch, failed request or uncompilable document throws and leaves
the installed base in force.

### Glyphs

The install resolves the document's `glyphs` template and fetches the Latin blocks
through the same client: 256-codepoint blocks `0-255` and `256-511`, committed under
`fixtures/glyphs/`. The pair decodes into one `LabelFont`, cached per provider, document
address and font stack.

### Sprites

The synchronous `SpriteSource` means the install prefetches the style's `sprite` base at
`<base>.json` and `<base>.png`, decodes the atlas and injects it through
`with_prefetched_sprite_atlas`; the atlas is cached per provider, document and base
address. The parser keeps `default` and records a density-specific `@2x` address as
unsupported. The PNG decoder builds on wasm, so the browser decodes the sheet and packs
glyph ranges into a label font, cached per provider, document and resource; a relative
document address is made absolute against the page origin once.

### Host scoping

The credential's scope is the document's own origin plus every declared host:
`VendorImplementations::style_resource_hosts`, mirrored as `style_resource_hosts` in
`platform/providers.json` and carried with the client. Any other host refuses the
install before a request and names the host; a relative reference resolves against the
document directory or its origin when root-relative. A vendor URI scheme is rewritten by
its provider, then held to the same origin-and-declared-host check.

### Vendor URI schemes

The official Mapbox documents name resources with the vendor's URI scheme, which the
provider layer maps:

- `resolve_declared_style_resource_address`, the `StyleResourceClient` rewrite seam,
  returns `None` by default, leaving HERE's addresses to their own rule.
- `mapbox://sprites/{username}/{style_id}` maps to
  `https://api.mapbox.com/styles/v1/{username}/{style_id}/sprite`; a trailing version or
  cache segment is dropped from the Styles API path.
- `mapbox://fonts/{username}/{fontstack}/{range}` maps to
  `https://api.mapbox.com/fonts/v1/{username}/{fontstack}/{range}.pbf`, keeping
  `{fontstack}` and `{range}` for the renderer.

HERE appends `apiKey` and Mapbox appends `access_token` in the provider client.

### Fallbacks and degradation

A glyph or sprite failure degrades labels or icons, never the base:

- An address that cannot become a fetchable `http` or `https` template (a missing
  placeholder, an unrecognized scheme, a reference climbing above the document
  directory) is not a credential risk and is not refused; the install uses the embedded
  Latin range.
- A style with no endpoint or symbol font stack, or an unfetchable, undecodable or
  unpackable range, degrades the same way.
- A style without a sprite, or an unfetchable index or sheet, draws text without icons.
- Where terms refuse hosted resources, the provider keeps a committed style document in
  that schema.

### Style and tileset must agree

A style written for one schema paints fragments when served another schema's tiles: each
layer reads a source layer the tile lacks. `vector_style_source_layer_mismatches(tile,
style)` names each painted layer whose `source-layer` is missing and lists the layers
the tile carries; the vector-base install reports the mismatches through the status
readback or the diagnostics ring.

The Mapbox fixture pair (`fixtures/mapbox/vendor_style_document.json`,
`fixtures/mapbox/vendor_style_tile.pbf`) is a neutral document and tile with the Streets
schema's `road`, `water`, `place_label` and `poi_label` layers. The HERE pair
(`fixtures/here/vendor_style_document.json`, `fixtures/here/vendor_style_tile.mvt`)
carries the OMV schema's `water`, `roads` and `places` layers, and its place layer
carries names and POI icons. Both keep resources document-relative for the same-origin
fixture server; the `mapbox://` mapping is exercised by the provider and hosted-resource
tests and the live smoke.

### Terms and vendored assets

- **HERE.** A credential serves Vector Tile API v2 and its referenced assets. Caching is
  bounded by the caching headers, otherwise up to 30 days (24 hours recommended);
  building an asset repository or scaling one request across users is forbidden. Style
  documents, glyphs and sprites are HERE Materials: runtime fetch and on-device cache
  with the developer's credential are allowed, but shipping any in the repository, a
  package or a committed fixture is not.
- **Mapbox.** The Styles API serves style JSON, sprites and glyphs, all Licensed Map
  Content. Terms permit 30-day single-device caching populated directly from the API,
  with no distribution or proxying. Mobile access must use Mapbox Mobile SDKs and Studio
  styles may only run on a Mapbox Map, so the runtime path is web-first; mobile needs an
  Order. No Mapbox asset may be bundled.
- **Tests.** A committed vendor style document is redistribution, so the fixture is
  synthetic with the vendor's structure and neutral content; the live smoke exercises
  the live path.

### What a published style declares

The compiler applies the layout and paint properties a real document uses. Symbol
layout: `text-padding`, `text-pitch-alignment` (`auto` resolves to viewport for a
point-placed label and map for a line-placed one), `icon-text-fit` and
`icon-text-fit-padding`; symbol paint: `icon-opacity`; line paint: `line-translate` and
`line-translate-anchor`. `circle-pitch-alignment` and `circle-pitch-scale` were already
carried.

Every other unimplemented property keeps a named diagnostic in
`CompiledStyle::unsupported`; `fill-pattern` is the one deliberate degradation, drawing
its colour. A known unpaintable kind (`heatmap`, `fill-extrusion`, `hillshade`,
`color-relief`) compiles to `CompiledLayerKind::Other` with one named diagnostic per
layer. The canvas surfaces the list in diagnostics.

### Source binding and attribution

Every committed document declares one vector source keyed `base`. The
`StyleSourceBindings`/`VectorSourceBinding` policy carries zoom clamping, bounds, scheme
and the attribution rule: the style's `minzoom`, `maxzoom` and `attribution` reach the
base, and the provider descriptor fills the rest. The reference application passes the
registry's style attribution into its URL-template source because committed styles
declare none. No tile URL placeholder mapping is needed: the provider owns its URL, and
a style's `tiles`/`url` stays documentation.

## Caching

### Namespacing

Every tile cache is namespaced by the provider that produced its bytes, for raster
tiles, vector payloads and tile packs. One normalized `tile_source_namespace` runs
through the raster request descriptor, cached raster wrapper, pack membership and
installation:

- Catalog provider: the catalog `provider_key`.
- URL-template source: source name plus a hash of its URL template.
- Application provider: an application-supplied stable key, never a shared literal.

One namespaced cache file replaces one file per provider, plus a visible namespace
column on tile packs. The cached raster and vector wrappers share one handle type
(store, namespace, time to live, executor), so a cached provider cannot be built without
a namespace.

### Key rules

Nothing goes into a key that can leak: no credential value, no raw URL. Two credentials
on one provider share that provider's entries because the same provider at the same
address serves identical bytes. The payload key stays style-free; the styled cache is
per scheduler and carries the style revision. Style resources are keyed by provider and
address only. Without an installed cache the fetch goes straight to the provider.

### Time to live

Raster tiles, vector tiles and style resources share a 30-day default. The MapQuest
raster ceiling is 30 days, declared as `raster_tile_cache_ceiling_days` in the registry
and `MAPQUEST_RASTER_TILE_CACHE_CEILING_SECONDS` in the catalog. A tile pack's requested
time to live is clamped to the provider's declared ceiling, and the MapQuest raster
maximum zoom is 20 (`MAPQUEST_RASTER_TILE_MAXIMUM_ZOOM_LEVEL`). The wasm build installs
no disk tile cache and never reads either value.

### Migrations

Fingerprints are opaque bytes, so lookups need no column change. Pre-namespace rows
would stay pinned and unreachable, so a migration purges raster and pack rows once. Both
vector key builders fold into one namespace-aware builder, and the browser's memory
store follows the same key.

### The traffic cache

The traffic layer draws congestion-coloured flow polylines from whatever provider serves
`TrafficFlowProvider`; a settled camera asks for its own bounds, so every small pan and
revisit would pay without a cache. `rustic-maps-core::traffic_flow_cache` wraps any
traffic-flow provider with a grid-keyed, memory-bounded cache:

- **Cells, not boxes.** A request maps onto the web-mercator tile grid at zoom 12 (about
  10 km at the equator), so a city camera covers one or two cells.
- **One call per miss.** Missing or expired cells are fetched as their union bounding
  box in one provider call, stored per cell by each segment's first point. The caller
  gets what the provider answered for that ground plus fresh cells; the renderer clips
  any coarser difference.
- **Bounded.** At most 64 cells are kept, least recently used evicted, and an
  over-budget request is refused with a message naming the budget.
- **Fresh for five minutes** against the injected clock; a clock stepping backwards
  keeps entries fresh longer.
- **A host clock.** The browser's standard-library clock panics, so the caller supplies
  one: `Date.now()` in the browser, the system clock on native, a controllable counter
  in tests.
- **No lock across an await.** A request inspects cells under the lock, drops it for the
  provider call, and re-takes it to store the answer.

Both registration paths wrap the provider: `MapSdk`'s native
`register_traffic_flow_provider` and the browser's `configureProvider`. A rejected
configuration (an impossible grid zoom, a zero budget) leaves the provider serving
uncached. `trafficFlowCacheStatistics()` answers `{hitCount, missCount, entryCount,
evictionCount, segmentCount, gridZoomLevel, maximumEntryCount, timeToLiveSeconds}`, or
null while none is registered; the developer console shows those rows. The cache does
not cull overlay geometry on the CPU, prefetch neighbours, or share entries across
providers.

Tests: cell keying and clamping, a repeated request as a hit, a small pan inside one
cell as a hit, expiry by the injected clock, the entry budget with least-recently-used
eviction, the over-budget refusal and segment intersection; browser cases assert one
miss then one hit and that a traffic toggle plus a camera pan produce two refreshes but
one provider request.

## Tests and fixtures

### Committed payloads

Contract tests carry recorded wiremock payloads for every vendor, exported rather than
recorded live:

1. Copy each into `fixtures/{provider_key}/{purpose}.json` at the repository root;
   OpenStreetMap US fixtures live under `fixtures/openstreetmap-us/`, named by their
   descriptor.
2. Register the request pattern in the descriptor's `fixture_routes` with the vendor
   client's method and path pattern.
3. Serve it through `platform/web/serve.mjs` under
   `/providers/{provider_key}/{provider-relative path}`.
4. Point a native integration test at a loopback server carrying the same bytes; every
   vendor domain type exposes `with_base_url`.

A served contract with no committed payload is declared in `fixtureless_contracts`
instead of a route, as the live-only OpenStreetMap Standard raster base does.

The browser harness and native tests read the same bytes, so drift is one failing route.
A place-search route also pins the first suggestion, line and quality class, so the
browser suite asserts the mapping for every provider serving type-ahead.

| Provider | Geocoding | Routing | Traffic | Tiles |
|---|---|---|---|---|
| MapQuest | forward, reverse | directions, distance matrix | incidents, flow | raster (the shared rendered-tile set) |
| HERE | forward | route | incidents, flow | vector (a recorded tile shared with the other vector sources) |
| Google | forward | route | — | — |
| TomTom | forward | route | incidents, flow | raster (a recorded Map Display tile) |
| Mapbox | forward | route | — | vector (a recorded tile shared with the other vector sources) |
| Radar | forward | route | — | — |
| Photon | search, reverse | — | — | — |
| OSRM | — | route | — | — |
| Valhalla | — | route | — | — |
| OpenStreetMap Standard | — | — | — | live only, no committed fixture |

### Native bridge tests

`crates/rustic-maps-native-core/tests/vendor_provider_*.rs` drive an exported payload
through the vendor provider and the SDK bridge on a loopback server: geocoding for the
credentialed vendors, routing for the full-service and credentialed vendors, traffic for
vendors, traffic for the three traffic-serving providers, and tiles for TomTom's raster
fetch and HERE's and Mapbox's vector fetches. Keyless routing payloads run through the
browser harness.

The provider layer's suites prove each vendor requests the right URL and parses its
payload. The SDK tests only what this repository owns: bridge conversion and its added
or refused fields, catalog dispatch, registry descriptors and per-platform mirrors, and
the reference-application selection path from settings row to registered slot.

### Browser cases

Every case reads facts from `platform/providers.json` and touches no external network;
`network_guard.mjs` aborts any request leaving the harness origin.

- `live_provider.spec.mjs` registers a descriptor's provider against same-origin fixture
  routes and asserts the browser `fetch` transport parsed the response and filled the
  expected state.
- `keyless_place_provider.spec.mjs` registers the keyless place-search provider with a
  deployment address and asserts suggest and search map the recorded responses.
- `capability_provider_restore.spec.mjs` switches routing to the keyless provider, plans
  from the fixture, reloads and finds it serving.
- `live_raster_fallback.spec.mjs` proves the canvas asks the recorded same-origin
  fixtures first and reaches the live raster template only when an address has none.

Natively, the catalog constructs each key and reports its contracts; an unknown key
throws `ProviderNotConfigured` naming it; a refused credential leaves the previous
provider serving. The vector tests prove every provider declaring `vector_tiles`
installs on every target; two raster providers and two credentials on one cache file
never share a fingerprint, and the same holds for vector payloads and packs; a migration
test opens a pre-namespace cache, runs the purge and serves a fresh tile.

### The provider-name guard

`platform/web/tests/provider_name_guard.mjs` enforces that a provider name lives only in
its provider crate, fixture directory, the shared registry and a registration dispatch
arm. It scans the browser harness, the wasm sources, the shipped Android wrapper module
and the three native reference application shells, reading names from the registry.

`HERE`, `Google`, `Radar` and `Mapbox` are ordinary words: the guard matches those
display names case-sensitively and skips their lowercase keys, while every other name
keeps case-insensitive substring matching.

### Always-on gates

`node --test platform/web/tests/contract_sync.mjs` asserts the wasm TypeScript façade
against the UDL, `platform/web/tests/platform_parity_gate.mjs` enforces the
platform-parity floor, `node scripts/check_ux_parity.mjs` fails on token or error-text
drift, and the provider-name guard runs under Playwright's `globalSetup` and under `node
--test`.

## Related reading

- `Architecture.md` — the core, renderer and binding layout; `Browser.md` — the browser
  target, the wasm façade, the fixture server and the keyless raster fallback evidence.
- `Release.md` — package identities, distribution, artifact rules and the capture
  procedures.
