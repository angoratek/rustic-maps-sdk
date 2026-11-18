# Providers

The SDK draws maps and answers search, routing and traffic calls through a
provider layer built on vendor-neutral contracts. One shared registry,
`platform/providers.json`, describes every provider: its key, display name,
credential shape, platform availability, attribution, terms and fixture
routes. The SDK registers nine provider keys (MapQuest, HERE, Google Maps,
TomTom, Mapbox, Radar, OSRM, Valhalla and Photon) and serves three tile
sources as base maps (OpenStreetMap Standard raster, OpenStreetMap US vector
and OpenFreeMap vector). Applications select a provider per capability, may
mix vendors on one map, and never carry a credential in a build.

## The registry

`platform/providers.json` is the single descriptor source consumed by the
reference applications, the development server, the test harness and the
playground. It declares:

- `schema_version` and `fixture_base_path` (`/providers`).
- `reference_app_user_agent_template`, the identifying user agent the
  reference applications send.
- `keyless_base_map`, the styled vector base a keyless reference application
  installs once its map view exists.
- `providers`, one descriptor per provider key.

### What a descriptor declares

Each descriptor carries the fields a settings surface renders, the
registration dispatch reads and the test harness serves:

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

The descriptor also declares `served_contracts` (the capabilities the SDK
registers) and `fixture_routes` (method, path pattern and committed fixture
per served contract). A provider may additionally declare
`maximum_zoom_level`, `vector_styles`, `style_resource_hosts`,
`url_template`, `glyphs_url_template`, `raster_url_template`, `vector_style`,
`raster_tile_cache_ceiling_days`, `demo_base_url` and `demo_base_url_note`.

The registry holds descriptors only, never credential values. The demo
artifact `platform/reference-scenes.json` carries no provider descriptors.

### The keyless base map

The top-level `keyless_base_map` names the provider key and style id a keyless
reference application paints, for example
`{ "provider_key": "openfreemap", "style_id": "bright-light" }`. A keyless
application still registers the credential-less raster descriptor so the
shipped surface can attach, then installs this vector style over it once the
map view exists. A registry that omits the field keeps the default raster
start; an explicit base-map choice wins over it. The browser demo starts
raster and offers the vector bases one tap away.

### Consumers

The reference applications render one settings row per descriptor and use
`wasm_registration` for the browser dispatch. `platform/web/serve.mjs` serves
each `fixture_routes` entry under
`/providers/{provider_key}/{provider-relative path}`. The provider-name guard
reads the registry's own keys, so a new provider is guarded automatically.

Adding a provider is a provider crate, a descriptor and a registration
dispatch arm. It must not require rewriting the live demo, the fixture
server, the UI suites or a reference application.

## Credentials

### Runtime entry on every target

Every reference application exposes a settings view opened from a gear
control near the readiness chip. It has four areas: provider credentials,
persistence, map SDK settings, and attribution and terms.

The provider credentials area renders one row per registry descriptor:
display name, credential label, a masked secure entry, a status line, Apply
and Clear. A credential field is never echoed in full; a short confirmation
may show only the last four characters. Paste is allowed and there is no
show-key affordance.

- Apply registers the providers through the SDK's registration calls
  (`configure_provider_set`, and `set_provider_credential_with_tile_cache`
  for the tile-cache registration). Success flips the credential status and
  retries initialization. A refusal is shown verbatim.
- Clear removes the stored credential and the UI state. The SDK registry has
  no unregister operation, so the application states that a restart is
  required for the running session to become keyless again.
- No key is a first-class state. The application starts without providers,
  renders cached or recorded imagery where available, reports "Set a
  provider key to load live map data", and offers the settings action.
  Operations that need a provider surface the typed `ProviderNotConfigured`
  error verbatim.
- The view renders the shared descriptors. A provider the SDK does not
  register yet renders disabled-with-reason rather than pretending to work.

| Target | Entry seam | Persistence |
|---|---|---|
| Android (Compose) | `applyProviderCredential(providerKey, credential)` | Keystore AES/GCM private preferences |
| iOS (SwiftUI) | `applyProviderCredential(_:_:)` | Keychain, `kSecClassGenericPassword` |
| React Native | `setProviderCredential(providerKey, credential)` `@ReactMethod` | `getStoredCredential`/`setStoredCredential`/`clearStoredCredential` |
| WebAssembly | settings panel over `configureProvider` | Browser `localStorage`, explicit opt-in only |

The native iOS Keychain service is
`com.angoratek.rustic-maps.refapp.provider-credentials`. React Native uses
Keychain on iOS and Android Keystore AES/GCM private preferences on Android.

### Persistence rules

Session-only is the default. A "Remember on this device" toggle, default off,
persists the credential in the platform secure store. Off means the value is
gone when the process ends. Secure-store availability is reported honestly
when it fails, and the application falls back to session-only.

The browser has no platform secure store, so its one exception is
`localStorage` on the demo's own origin, behind an explicit opt-in and a
visible warning that the value is stored in readable form and that a shared
computer should not use the option. Native targets keep the secure store and
the off default. Automation injection is session-only and never persisted.

### Automation injection

Local and CI UI automation can run keyed without a build-time credential.
Each reference application supports a documented runtime injection seam:

- **Android.** A launch-intent extra `providerApiKey`
  (`adb shell am start -e providerApiKey …`, or an `ActivityScenario` intent)
  is applied through the same registration path as the settings surface. The
  optional `providerKey` extra names its provider, defaulting to MapQuest, the
  documented full-service provider.
- **iOS.** The `PROVIDER_API_KEY` launch-environment variable is applied at
  application start, optionally paired with `PROVIDER_KEY`.
- **React Native.** The native controller reads both the Android extras and
  the iOS launch environment at SDK start and applies them through
  `setProviderCredential`.

The injected value is never persisted by the injection, never logged, and is
not compiled into any artifact. A run without the variable stays keyless;
provider-dependent cases skip rather than fail. CI supplies the variable only
to the UI-test job when a key is configured.

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

Keyed screenshot captures use the same seams for one session and publish no
credential. `Release.md` records the commands and the published revision
record.

### The hosted-demo exception

The Appetize-hosted demo is the one artifact that may carry a credential: a
visitor comparing SDKs should see a real map rather than a mock scene, and a
build living inside Appetize's service is not a downloadable distribution.
The exception is bounded:

- **Dedicated.** The key is issued for the demo alone, rotated on a schedule,
  and never the key the fixture tools or the live-smoke example use.
- **Injected, never stored.**
  `scripts/inject-hosted-demo-credential.sh` writes
  `assets/hosted-demo-credential.json` into the Android reference
  application and the React Native Android app on the build machine from
  `HOSTED_DEMO_PROVIDER_API_KEY` and `HOSTED_DEMO_PROVIDER_KEY` (default
  MapQuest). The iOS app reads `HostedDemoProviderAPIKey` from `Info.plist`,
  expanded from the same build setting. These paths are gitignored and
  produced only inside `.github/workflows/hosted-demo.yml`; `--clean` removes
  the Android files so a local build cannot silently stay keyed.
- **Runtime semantics unchanged.** The application applies it through the
  same registration path as the settings surface and the automation seam. It
  is never persisted to the Keystore or Keychain, never logged, and gone when
  the process ends. A build without the injected value is keyless and still
  works.
- **Permissions stay off.** The hosted apps keep `appPermissions.adbConnect`,
  `networkProxy` and `debugLog` disabled, so a visitor cannot attach to the
  running build, and no API path serves the binary.

### What must never carry a credential

No credential appears in a build input, source file, fixture, documentation
page, log, diagnostic line, error detail, crash report, screenshot or the
shared demo artifact. Redaction is grep-gated where practical, and there must
be no build field, plist key, `local.properties` line or `.xcconfig` entry
carrying one. Any credential in a *distributed* artifact is still forbidden:
GitHub Release, npm/Maven/SwiftPM package, docs download, repository, fixture
or screenshot. Those stay keyless and runtime-entered. The hosted-demo key is
never the fixture-recording key, never appears in a release or package, and
is bounded by the rules above.

### Browser transport

The provider layer selects its HTTP transport per target: `reqwest` and
`tokio` natively, `window.fetch` on WebAssembly. Key injection, bounded
retries, pacing, timeouts and status mapping live in the shared provider
client rather than in a vendor crate.

```rust
pub trait HttpTransport: Send + Sync {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, TransportError>;
}
```

`HttpRequest` and `HttpResponse` are plain byte and header records. The
browser transport drops the Fetch-standard forbidden request names
(`user-agent`, `cookie`, `host`, `origin`, `referer`, `connection`) before
building a request; the Fetch standard additionally forbids a page from
setting `content-length`, `accept-encoding`, the `proxy-*` family and the
`sec-*` family. The native transport still sends the provider's declared user
agent.

A key entered into a browser page is visible to the page and in the network
panel by construction. The goal is to avoid shipping one by default, not to
promise secrecy from the page's own user. CORS and key exposure in the
browser are documented as inherent, not hidden.

Live provider verification runs outside the test suites:
`scripts/smoke-providers.mjs` exercises every registry provider's served
contracts in a real browser and `scripts/live-style-smoke.mjs` fetches a live
style, sprite pair, glyph range and tile. Both read provider keys from the
machine's own configuration (`~/.everymap/config.toml` for the provider
smoke), never print or write them, and never run in CI.

## Coverage matrix

Cell entries name the everymap type the SDK's bridge wraps. A dash means the
SDK does not register that contract for the provider, either because the
vendor does not serve it or because the SDK has no registration path for it;
a registration attempt is refused rather than filling the slot with a stub.

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

The three tile sources stay base-map sources in the reference applications,
selected by the layers control, and are not provider rows in a capability
picker. They serve no callable contract besides tiles.

### Credential shapes

A registration takes one string in the credential slot. Its meaning depends
on the provider:

| Shape | Providers | Value |
|---|---|---|
| Query-parameter key | MapQuest, HERE, Google Maps, TomTom, Mapbox | The issued API key; the provider crate names its parameter |
| Header key | Radar | The issued API key, sent as `Authorization` |
| Deployment address | OSRM, Valhalla | The base URL of the deployment; an empty value reaches the crate's local default, so a shared deployment must be named |
| Deployment address or public instance | Photon | The base URL of the deployment; an empty value reaches the public Photon instance |
| None | OpenStreetMap Standard, OpenStreetMap US, OpenFreeMap | Empty |

The crate defaults are `http://localhost:5000` for OSRM,
`http://localhost:8002` for Valhalla and `https://photon.komoot.io` for
Photon. The registry declares a `demo_base_url` and `demo_base_url_note` for
the keyless providers that offer a public server, and the reference
applications pass it so a shared deployment is always named.

The descriptor's `credential_label` decides whether a settings surface
renders a credential field. A blank label means keyless, and the surface
offers the provider without a field.

### Platform availability, attribution and terms

| Provider | Key | Android | iOS | React Native | WebAssembly | Attribution | Terms |
|---|---|---|---|---|---|---|---|
| MapQuest | `mapquest` | Yes | Yes | Yes | Yes | Map data © OpenStreetMap contributors (ODbL) — tile imagery © MapQuest | Rate limits and a 30-day raster-tile caching ceiling |
| HERE | `here` | Yes | Yes | Yes | Yes | © 20XX HERE, XXX | Per-plan transaction limits and a caching policy that bounds stored results |
| Google Maps | `google` | Yes | Yes | Yes | Yes | Map data © Google | Billing per SKU, caching restrictions, no permanent storage of most content |
| TomTom | `tomtom` | Yes | Yes | Yes | Yes | © TomTom | Per-plan request quotas and a caching policy |
| Mapbox | `mapbox` | Yes | Yes | Yes | Yes | © Mapbox © OpenStreetMap contributors | Usage-based billing; Mapbox and OpenStreetMap attribution wherever data is shown |
| Radar | `radar` | Yes | Yes | Yes | Yes | © Radar | Per-plan request quotas; no permanent storage of geocoding results |
| OSRM | `osrm` | Yes | Yes | Yes | Yes | Map data © OpenStreetMap contributors (ODbL) — routing by OSRM | Public demo server: reasonable, non-commercial, one request per second, no guarantee; self-hosted follows the operator |
| Valhalla | `valhalla` | Yes | Yes | Yes | Yes | Map data © OpenStreetMap contributors (ODbL) — routing by Valhalla | Public demo server: one request per second, identifying User-Agent, attribution, no production use; self-hosted follows the operator |
| Photon | `photon` | Yes | Yes | Yes | Yes | Map data © OpenStreetMap contributors (ODbL) — search by Photon | Public instance: reasonable, non-commercial, no guarantee; self-hosted follows the operator |
| OpenStreetMap Standard | `openstreetmap` | Yes | Yes | Yes | No | Map data © OpenStreetMap contributors (ODbL) — raster tiles © OpenStreetMap contributors | Tile usage policy: low-volume use only, with attribution |
| OpenStreetMap US | `osmus` | No | No | No | No | Map data © OpenStreetMap contributors (ODbL) — vector tiles © OpenStreetMap US | Free for low-volume, non-commercial use with attribution |
| OpenFreeMap | `openfreemap` | No | No | No | No | Map data © OpenStreetMap contributors (ODbL) — vector tiles © OpenFreeMap and OpenMapTiles | Free public use with attribution; bulk downloads limited to 3,000 tiles or 50 MB |

The HERE notice `© 20XX HERE, XXX` substitutes the current year and the
licensee and must be refreshed within 24 hours.

### Registration model

- `configure_provider(contract, provider_key, credential)` fills exactly one
  capability and leaves every other slot serving.
- `configure_provider_set(provider_key, credential)` fills every capability
  the named provider's catalog row serves.
- The credential-only `set_provider_credential` is removed: a catalog with
  several credentialed providers cannot infer which set a bare credential
  means. `set_provider_credential_with_tile_cache` remains for the tile-cache
  registration.
- A provider that does not serve the requested contract is refused before any
  slot changes, with `ProviderNotConfigured` on native and the
  `unsupportedCapability` code in the browser. An empty credential is legal
  for a keyless provider and refused for one whose descriptor declares a
  credential label.

### Per-capability selection

The registry holds one slot per contract. `configure_provider` replaces
exactly one slot; every other slot keeps serving, so one map can mix a vendor
for geocoding, a URL-template source for tiles and another engine for
routing. Succeeding is the only way a slot changes: a refused key or
credential leaves the previous provider in place and throws with the reason.

`provider_registrations()` reports every capability in a stable order with
the serving key, the descriptor's display name and whether a provider is
installed: Geocoding, Routing, Distance matrix, Place search, Traffic
incidents, Traffic flow, Elevation, Raster tiles, Vector tiles, Offline
routing. A capability with no installed provider reads "Not configured"; a
picker is limited to descriptors that declare that capability for the
platform. The choice joins each application's versioned preferences record.

Eight contract families also accept a host implementation through the UDL's
`[Trait, WithForeign]` interface and `register_*_from_foreign`. It writes the
same slot as `configure_provider`, last write wins, and the readback reports
it. Vector tiles and offline routing keep their dedicated calls instead.

### What makes a provider selectable

1. The crate dependency is in the workspace and in `rustic-maps-native-core`
   and `rustic-maps-web-bindings`.
2. The native catalog has one row: key, display name, credential
   requirement, served contracts, install and install-all calls, tile
   ceilings.
3. The wasm dispatch has one row with the same key and display name.
4. `platform/providers.json` has a descriptor with display name, credential
   label, per-platform availability, attribution, terms, settings hint,
   `wasm_registration` and fixture routes.
5. The iOS resource mirror is refreshed, and the platform-parity gate passes.
6. The reference applications' dispatch accepts the key: Android and web read
   the descriptor; iOS and the React Native native runtime carry a
   registration arm for each credentialed provider.
7. The provider-name guard passes.
8. The provider-capability pages, the add-a-provider guide and the Rust page
   describe it.

The catalog table in `rustic-maps-native-core` is the only place a provider
name reaches the binding besides the descriptors and dispatch arms. Compiling
a crate into the binding does not make it selectable by itself.

### The provider layer behind the SDK

- **`everymap-core`** owns the contracts and portable plumbing: the transport
  seam, pacing, retry, timeouts, request observation and sixteen contract
  traits.
- **`everymap-providers-*`** owns vendor knowledge: URLs, credentials, status
  mapping, attribution and terms. Eleven crates are published.
- **`rustic-maps-core::everymap_bridge`** adapts eight contract families to
  the SDK's records: geocoding, place search with type-ahead, routing,
  distance matrix, traffic incidents and flow, raster tiles and vector tiles.
- **`geospatial-traits`** remains the SDK's in-repo contract for host
  implementations and UDL callbacks. New vendor providers implement the
  provider layer's contracts and cross the bridge instead.

## Provider-backed tile sources

### Raster

`MapView` draws the registered `RasterTileProvider` slot. MapQuest and TomTom
register into it; a URL-template source registers a provider over any
`{z}/{x}/{y}` template. Adding HERE raster is a provider-crate addition, not
an SDK concept; the HERE raster upstream is not yet carried. A registered
raster provider wins over a page's live URL-template fallback, whatever order
the page's asynchronous registrations run in.

### Vector

`MapView.set_provider_vector_tile_source(source)` installs a named provider
tileset through the registry's vector slot.
`ProviderVectorTileSource` names the `provider_key` and the built-in
`style_id`; the registry refuses the selection when the provider is not
registered for vector tiles.

`MapView.set_provider_vector_tile_source_from_document(source,
style_document_url)` is the asynchronous sibling that installs the provider's
own style document. The committed-style member stays for a provider painted
with a style the SDK carries: the fallback where a vendor's terms refuse
hosted resources, and for host-registered providers.

Application code can serve vector tiles without a provider crate. It
implements the `VectorTileProvider` callback contract (a synchronous
`source_descriptor()` and an asynchronous `fetch_vector_tile`) and registers
through `MapSdk::register_vector_tile_provider_from_foreign`. The selection
is recorded under the application key `application`, and
`vector_tile_provider_is_registered()` reports whether the slot is filled.
Declared encoding and style hints are advisory: each fetched
`VectorTileData` reports its own payload's encoding and the decoder unwraps
by that per-tile value, while the installed style id alone selects the
document. The browser surface has no foreign-callback seam; a page registers
through `configureProvider` or the URL-template source.

The URL-template record carries the declaration's terms text, `{s}`
subdomains, per-request headers and style hints. A null or empty field
declares none, and the conversion hands every value to the provider layer
instead of dropping it. The browser surface consults no named declaration, so
every extra there is caller-declared; the native surface inherits a matching
keyless tileset's recorded extras when the caller omits them.

### Layers control and style families

The base-map rows come from the registry: a provider with `raster_tiles`
becomes a raster base, one with `vector_tiles` becomes a vector base. The row
states the credential state and the attribution line. A vendor family
installs through the document member once its credential is applied, and each
family is listed once.

| Key | Base | Style family | Style ids | Style document | Max zoom |
|---|---|---|---|---|---|
| `openstreetmap` | Raster | — | — | `https://tile.openstreetmap.org/{z}/{x}/{y}.png` | 19 |
| `osmus` | Vector | `carto` | `carto-light`, `carto-dark` | Committed | 15 |
| `openfreemap` | Vector | `openmaptiles` | `bright-light`, `fiord-dark`, `positron-light` | Committed | 14 |
| `here` | Vector | `omv` | `omv-light` (HERE Base) | `https://assets.vector.hereapi.com/styles/berlin/base/mapbox/tilezen` | 17 |
| `mapbox` | Vector | `streets` | `streets-light` (Mapbox Streets) | `https://api.mapbox.com/styles/v1/mapbox/streets-v11` | 22 |

The keyless URL-template providers keep the committed OpenMapTiles and Carto
documents. The committed styles are Bright, Fiord, Positron and Carto, with
style ids `bright-light`, `fiord-dark`, `positron-light`, `carto-light` and
`carto-dark`.

## Style documents and assets

### Loading the provider's own style document

The renderer's committed styles target the OpenMapTiles and Carto schemas.
HERE serves OMV and Mapbox serves Streets; painting those tiles with an
OpenMapTiles style yields a background and fragments, not a map. A
credentialed vector provider therefore loads the style the vendor publishes,
which matches its tiles by construction.

The document is fetched through the provider's authenticated style-resource
client, which the catalog registers beside the provider
(`VendorImplementations.style_resources`). The SDK names only the address;
the credential stays in the vendor client.

`rustic_maps_core::StyleResourceClient::fetch_style_resource(url)` is the
SDK-side contract. The vendor client applies its authentication provider,
transport, token bucket, retry policy, timeout and request observer,
observing the URL with its credential query values redacted. A `document_url`
must be `https` and carry no query, because the provider client appends the
credential.

A compiled document is kept per provider key and address for the session, so
a repeated install of the same family costs no request; a malformed document
is never remembered. A missing provider, a provider without a style-resource
fetch, a failed request and an uncompilable document all throw and leave the
installed base in force.

### Glyphs

A provider-document install resolves the document's `glyphs` template and
fetches the Latin blocks through the same style-resource client. The protocol
serves 256-codepoint blocks, so the ranges are `0-255` and `256-511`, and the
payloads are committed under `fixtures/glyphs/`. The pair decodes into one
`LabelFont` the scheduler carries to the label pass, cached per provider,
document address and font stack.

### Sprites

The renderer's `SpriteSource` is synchronous, so the install prefetches the
style's `sprite` base at `<base>.json` and `<base>.png` through the
style-resource client, decodes the atlas and injects it through
`with_prefetched_sprite_atlas`. The atlas is cached per provider, document
and base address. The parser keeps the declaration's `default` address and
records a density-specific `@2x` address as unsupported, so the install loads
the one sheet the style declares.

Both install paths run the same seams on native and on wasm. The PNG decoder
builds on wasm, so the browser canvas decodes the sheet in the page and packs
the document's glyph ranges into a label font, caching each per provider,
document and resource. A document address the page names relatively is made
absolute against the page's origin once.

### Host scoping

The credential's scope is the document's own origin plus every host the
provider declares. `VendorImplementations::style_resource_hosts` names them
in the provider crate, `platform/providers.json` mirrors them under
`style_resource_hosts`, and the registration carries them with the client.

A glyph or sprite address on any other host refuses the install before a
request, with the host named. A relative reference is resolved against the
document's own directory, or its origin when root-relative, as the style
specification allows. A vendor URI scheme is first rewritten by the provider
that owns it, then held to the same origin-and-declared-host check; a mapping
that lands on an undeclared host is refused before a request.

### Vendor URI schemes

The official Mapbox documents name their resources with the vendor's URI
scheme. The provider layer maps them, because the scheme and endpoints are
the vendor's:

- The `StyleResourceClient` rewrite seam,
  `resolve_declared_style_resource_address`, returns `None` by default and
  leaves the declaration unchanged, so HERE's relative and absolute addresses
  keep their own rule.
- `mapbox://sprites/{username}/{style_id}` maps to
  `https://api.mapbox.com/styles/v1/{username}/{style_id}/sprite`. A trailing
  version or cache segment after the style id names the same sprite base and
  is dropped from the Styles API path.
- `mapbox://fonts/{username}/{fontstack}/{range}` maps to
  `https://api.mapbox.com/fonts/v1/{username}/{fontstack}/{range}.pbf`,
  keeping the `{fontstack}` and `{range}` placeholders for the renderer to
  substitute.

The credential parameter stays the provider client's business and is
unchanged: HERE's client appends `apiKey`, Mapbox's appends `access_token`.

### Fallbacks and degradation

A glyph or sprite failure degrades labels or icons, never the base:

- An address the SDK cannot turn into a fetchable `http` or `https`
  template (a missing placeholder, an unrecognized scheme, a reference
  climbing above the document's directory) is not a credential risk and so
  is not a refusal. The install proceeds with the embedded Latin range for
  glyphs.
- A style with no endpoint or no symbol font stack, or a range that cannot be
  fetched, decoded or packed, degrades the same way.
- A style without a sprite, or an index or sheet that cannot be fetched or
  decoded, draws text without icons.
- Where a vendor's terms refuse hosted resources, that provider keeps a
  committed style document in that vendor's schema instead, and the
  capability row states which path it uses.

### Style and tileset must agree

A style written for one schema paints fragments when it is served another
schema's tiles: every layer reads a source layer the tile does not carry, and
the failure looks like a renderer defect. The renderer exposes
`vector_style_source_layer_mismatches(tile, style)`, which names each painted
layer whose `source-layer` the decoded tile lacks and lists the layers the
tile does carry. The vector-base install reports the mismatches through the
status readback or the diagnostics ring instead of painting the fragments.

The committed fixtures prove schema fidelity. The Mapbox pair
(`fixtures/mapbox/vendor_style_document.json`,
`fixtures/mapbox/vendor_style_tile.pbf`) is a neutral authored document and
tile with the Streets schema's `road`, `water`, `place_label` and
`poi_label` layers. The HERE pair (`fixtures/here/vendor_style_document.json`,
`fixtures/here/vendor_style_tile.mvt`) is the same for the OMV schema's
`water`, `roads` and `places` layers, and its place layer carries both names
and POI icons. Both keep their resources document-relative so the
same-origin fixture server can serve them; the `mapbox://` mapping is
exercised by the provider and hosted-resource tests and by the live smoke.
Neither fixture carries vendor material.

### Terms and vendored assets

- **HERE.** A credential serves Vector Tile API v2 and its referenced assets.
  Caching is bounded by the caching headers, otherwise up to 30 days, with 24
  hours recommended; building an asset repository or scaling one request
  across users is forbidden. Style documents, glyphs and sprites are HERE
  Materials: runtime fetch and on-device cache with the developer's
  credential are allowed, but shipping any of them in the repository, a
  package or a committed fixture is not.
- **Mapbox.** The Styles API serves style JSON, sprites and glyphs, all
  Licensed Map Content. The terms permit 30-day single-device caching
  populated directly from the API, with no distribution or proxying. Mobile
  access must go through Mapbox Mobile SDKs and Studio styles may only run on
  a Mapbox Map, so the runtime-fetch path is web-first; mobile use needs an
  Order. No Mapbox asset may be bundled.
- **Consequence for tests.** A committed vendor style document is
  redistribution, so the fixture is a synthetic document with the vendor's
  structure and neutral content. The live path is exercised by the live
  smoke, never by the committed suites.

### What a published style declares

The compiler applies the layout and paint properties a real document uses and
carries them into the resolved plans the renderer reads. In the symbol
layout: `text-padding` and `text-pitch-alignment` (with `auto` resolved to
viewport for a point-placed label and map for a line-placed one),
`icon-text-fit` and `icon-text-fit-padding`; in the symbol paint:
`icon-opacity`; in the line paint: `line-translate` and
`line-translate-anchor`. `circle-pitch-alignment` and `circle-pitch-scale`
were already carried.

Every other unimplemented property keeps a named diagnostic in
`CompiledStyle::unsupported`; `fill-pattern` is the one deliberate
degradation, and its diagnostic says the fill draws its colour. A known layer
kind the overlay pipeline cannot paint (`heatmap`, `fill-extrusion`,
`hillshade`, `color-relief`) compiles to `CompiledLayerKind::Other` with one
named diagnostic per layer, so a real style never loses a whole kind in
silence. The canvas surfaces the list in its diagnostics readout.

### Source binding and attribution

Every committed document declares one vector source keyed `base`. The
scheduler binds the provider to that source id through the
`StyleSourceBindings`/`VectorSourceBinding` policy, which carries zoom
clamping, bounds, scheme and the attribution rule: the style's own
`minzoom`, `maxzoom` and `attribution` reach the base, and the provider
descriptor fills whatever the style leaves out. Both surfaces' status
readbacks report the style's line. The reference application still passes
the registry's style attribution into its URL-template source, because the
committed styles declare none of their own. No tile URL placeholder mapping
is needed: the provider owns its URL, and a style's `tiles`/`url` stays
documentation.

## Caching

### Namespacing

Every tile cache is namespaced by the provider that produced its bytes, for
raster tiles, vector payloads and tile packs alike. One normalized
`tile_source_namespace` runs through the raster request descriptor, the
cached raster wrapper, the pack membership and the installation:

- Catalog provider: the catalog `provider_key`.
- URL-template source: the source name plus a hash of its URL template, since
  a caller-supplied name is not enforced unique.
- Application provider: an application-supplied stable key, never one shared
  literal.

One namespaced cache file is used rather than one file per provider, plus a
visible namespace column on tile packs. Pack identity gains the namespace so
a pack can be reported and removed as the provider's own. The cached raster
and vector wrappers share one handle type (store, namespace, time to live,
executor), so a cached provider cannot be constructed without a namespace.

### Key rules

Nothing goes into a key that can leak: no credential value, no raw URL. Two
credentials on one provider share that provider's entries, which is correct:
the same provider at the same address serves identical bytes, so a key
rotation keeps the cache valid. The payload key stays style-free; the styled
cache is per scheduler and already carries the style revision.

Style resources follow the same rule. A document, glyph range or sprite sheet
is keyed by the provider and the address only, so no credential enters the
key and a key rotation keeps the bytes valid. Without an installed cache the
fetch goes straight to the provider.

### Time to live

Raster tiles, vector tiles and style resources share a 30-day default window.
The MapQuest raster ceiling is 30 days, declared as
`raster_tile_cache_ceiling_days` in the registry and
`MAPQUEST_RASTER_TILE_CACHE_CEILING_SECONDS` in the catalog. A tile pack's
requested time to live is clamped to the provider's declared ceiling, and the
MapQuest raster maximum zoom is 20
(`MAPQUEST_RASTER_TILE_MAXIMUM_ZOOM_LEVEL`). The wasm build installs no disk
tile cache and never reads either value.

### Migrations

The persisted format needs no column change for lookups because fingerprints
are opaque bytes. Pre-namespace rows would stay pinned and unreachable, so a
schema migration purges raster and pack rows once. Both vector key builders
fold into one namespace-aware builder, and the browser's memory store follows
the same key so a provider switch cannot read the previous payloads.

### The traffic cache

The traffic layer draws congestion-coloured flow polylines from whatever
provider serves `TrafficFlowProvider`. A settled camera asks for the segments
inside its own bounds, so without a cache every small pan is a provider
request and every revisit pays again. `rustic-maps-core::traffic_flow_cache`
wraps any traffic-flow provider with a grid-keyed, memory-bounded cache:

- **Cells, not boxes.** A request maps onto the web-mercator tile grid at
  zoom 12 (about 10 km at the equator), so a city camera covers one or two
  cells and a pan of a few hundred metres stays inside the cells already held.
- **One call per miss.** Missing or expired cells are fetched as their union
  bounding box in a single provider call, and the answer is stored per cell by
  each segment's first point. The caller receives exactly what the provider
  answered for that ground plus what fresh cells already held; a hit answers
  from cells a few kilometres coarser than the camera, and the renderer clips
  the difference at draw time.
- **Bounded.** At most 64 cells are kept, the least recently used is evicted,
  and a request needing more cells than the budget is refused with a message
  naming the budget rather than silently truncated.
- **Fresh for five minutes**, measured against the injected clock. A clock
  that steps backwards keeps an entry fresh longer instead of expiring
  everything.
- **A host clock.** The crate compiles for the browser, where the standard
  library's clock panics, so the caller supplies one: `Date.now()` in the
  browser, the system clock on native, a controllable counter in tests.
- **No lock across an await.** A request inspects its cells under the lock,
  drops it for the provider call, and re-takes it to store the answer, so a
  frame reading statistics never waits on the network.

Both registration paths wrap the provider — `MapSdk`'s native
`register_traffic_flow_provider` and the browser's `configureProvider` — so
the cache exists on every target without a reference app knowing about it. A
rejected configuration (an impossible grid zoom, a zero budget) leaves the
provider serving uncached rather than failing registration.

`trafficFlowCacheStatistics()` answers `{hitCount, missCount, entryCount,
evictionCount, segmentCount, gridZoomLevel, maximumEntryCount,
timeToLiveSeconds}`, or null while no traffic provider is registered; the
developer console shows those rows. The cache deliberately does not cull
overlay geometry on the CPU, prefetch neighbouring cells, or share entries
across providers: world-space vertices are clipped by the GPU, traffic ages
quickly, and re-registering a provider replaces its cache.

Tests: cell keying and clamping, a repeated request as a hit, a small pan
inside one cell as a hit, expiry by the injected clock, the entry budget with
least-recently-used eviction, the over-budget refusal and segment
intersection; browser cases assert one miss then one hit and that a traffic
toggle plus a camera pan produce two refreshes but one provider request.

## Tests and fixtures

### Committed payloads

The provider layer's contract tests carry recorded wiremock payloads for every
vendor. The SDK exports those payloads rather than recording live responses:

1. Copy each payload into `fixtures/{provider_key}/{purpose}.json` at the
   repository root, where the fixture server resolves descriptor paths. The
   OpenStreetMap US fixtures live under `fixtures/openstreetmap-us/`, which
   their descriptor names directly.
2. Register the request pattern in the provider's descriptor under
   `fixture_routes`, with the method and path pattern the vendor client uses.
3. Serve the route through `platform/web/serve.mjs`, which reads the
   descriptor and serves it under
   `/providers/{provider_key}/{provider-relative path}`.
4. Point a native integration test at a loopback server carrying the same
   bytes; every vendor domain type exposes `with_base_url` for this.

The exported files are a shared artifact: the browser harness and the native
tests read the same bytes, so a drift in either shows up as one failing route
rather than two divergent fixtures. A place-search route also pins the first
suggestion its payload must produce, the line and quality class, so the
browser suite asserts the mapping for every provider serving type-ahead.

Exported payloads, by contract:

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

### Native bridge tests

The native integration tests under
`crates/rustic-maps-native-core/tests/vendor_provider_*.rs` drive an exported
payload through the vendor provider and the SDK bridge on a loopback server:
geocoding for the credentialed vendors, routing for the full-service and the
credentialed vendors, traffic for the three traffic-serving providers, and
tiles for TomTom's raster fetch and HERE's and Mapbox's vector fetches. The
keyless routing payloads are exercised through the browser harness.

The provider layer's own suites prove that each vendor requests the right URL
and parses its own payload. The SDK's records carry their own invariants (a
validated `Coordinate`, a `RouteData` the renderer consumes, a `SearchResult`
whose quality classes drive the suggestion UI), so the SDK tests the parts
only this repository owns: the bridge conversion and the fields it adds or
refuses, the catalog dispatch, the registry descriptors and their
per-platform mirrors, and the reference-application selection path from the
settings row to the registered slot.

### Browser cases

Every browser case reads its provider facts from `platform/providers.json`
and touches no external network. `network_guard.mjs` aborts any request that
leaves the harness origin.

- `live_provider.spec.mjs` registers a descriptor's provider against
  same-origin fixture routes and asserts the browser `fetch` transport
  parsed the response and filled the expected state.
- `keyless_place_provider.spec.mjs` registers the keyless place-search
  provider with a deployment address and asserts suggest and search map the
  recorded responses.
- `capability_provider_restore.spec.mjs` switches routing to the keyless
  routing provider, plans from the fixture, reloads and finds it still
  serving.
- `live_raster_fallback.spec.mjs` proves the canvas asks the recorded
  same-origin fixtures first and reaches the live raster template only when
  an address has none.

The catalogue also asserts, natively: the catalog constructs each key and
reports its contracts; an unknown key throws `ProviderNotConfigured` naming
it; a refused credential leaves the previous provider serving. The vector
tests prove every provider declaring `vector_tiles` installs on every target;
two raster providers and two credentials on one cache file never share a
fingerprint, and the same holds for vector payloads and packs; a migration
test opens a pre-namespace cache, runs the purge and serves a fresh tile.

### The provider-name guard

`platform/web/tests/provider_name_guard.mjs` enforces that a provider name
lives only in its provider crate, its fixture directory, the shared registry
and a registration dispatch arm. It scans the browser harness, the wasm
binding sources, the shipped Android wrapper module and the three native
reference application shells, and fails when a provider name appears
anywhere else. The names are read from the registry, so a newly added
provider is guarded automatically with no edit.

`HERE`, `Google`, `Radar` and `Mapbox` are ordinary words. The guard matches
those display names case-sensitively and skips their lowercase keys, so prose
does not trip it; every other name keeps case-insensitive substring matching,
which catches vendor identifiers.

### Always-on gates

The always-on gates ride along with the provider suites:
`node --test platform/web/tests/contract_sync.mjs` asserts the wasm
TypeScript façade against the UDL,
`platform/web/tests/platform_parity_gate.mjs` enforces the platform-parity
floor, `node scripts/check_ux_parity.mjs` fails on token or error-text drift,
and the provider-name guard runs under Playwright's `globalSetup` and under
`node --test`.

## Related reading

- `Architecture.md` — the core, renderer and binding layout.
- `Browser.md` — the browser target, the wasm façade and the fixture server.
- `Release.md` — package identities, distribution and artifact rules.
- `Browser.md` — the keyless raster fallback and its evidence.
- `Release.md` — the per-platform capture procedures and the published
  revision record.
