# Provider coverage across the SDK

**Status:** design, owner-directed 2026-11-05 (PRD §6 item 61). Work: `PLAN.md` M75.
**Question it answers:** which providers the SDK carries, what each one serves, and what makes a provider selectable, documented and tested on every target.

## Outcome

The target state: every provider crate the published provider layer carries is either registered and selectable on Android, iOS, React Native and WebAssembly, or documented as not registered with a stated reason. The reference applications and the web demo list the same providers for the same capabilities, a fixture-backed test proves one call per registered provider per served contract on native and on wasm, and no published page names a deleted crate, trait or type. Geocoding and routing payloads are exported for every registered vendor, traffic payloads for the three traffic-serving providers, raster tile payloads for the full-service and TomTom providers, and vector tile payloads for HERE and Mapbox; a selectable vendor vector base waits on a committed style for each vendor's schema.

## The two layers

- **`everymap-core`** owns the provider contracts and the portable plumbing: the transport seam, pacing, retry, timeouts, request observation and the sixteen domain traits.
- **`everymap-providers-*`** owns vendor knowledge: URLs, credentials, status mapping, attribution and terms. Eleven crates are published.
- **`rustic-maps-core::everymap_bridge`** adapts eight contracts to the SDK's own records: geocoding, place search with type-ahead, routing, distance matrix, traffic incidents and flow, raster tiles and vector tiles.
- **`geospatial-traits`** remains the SDK's in-repo contract for host implementations and for the UDL callbacks. It is being retired; new vendor providers implement the provider layer's contracts and cross the bridge instead.

## Coverage matrix

Cell entries name the everymap type the bridge wraps. A dash means the SDK does not register that contract for the provider, either because the vendor does not serve it or because the SDK has no registration path for it; a registration attempt is refused rather than filling the slot with a stub.

| Provider | Key | Geocoding | Search and type-ahead | Routing | Matrix | Traffic | Raster tiles | Vector tiles | Credential |
|---|---|---|---|---|---|---|---|---|---|
| MapQuest | `mapquest` | `MapQuestGeocoder` | `MapQuestPlaceSearchProvider`, `MapQuestAutocompleteProvider` | `MapQuestRouter` | `MapQuestDistanceMatrix` | `MapQuestTrafficProvider` | `MapQuestRasterTileProvider` | — | API key, query parameter `key` |
| Photon | `photon` | `PhotonGeocoder` | `PhotonPlaceSearchProvider`, `PhotonAutocompleteProvider` | — | — | — | — | — | none, or a deployment URL |
| OSRM | `osrm` | — | — | `OsrmRouter` | — | — | — | — | none, or a deployment URL |
| Valhalla | `valhalla` | — | — | `ValhallaRouter` | — | — | — | — | none, or a deployment URL |
| HERE | `here` | `HereGeocoder` | — | `HereRouter` | — | `HereTraffic` | — | — | API key, query parameter `apiKey` |
| Google | `google` | `GoogleGeocoder` | — | `GoogleRouter` | — | — | — | — | API key, query parameter `key` |
| TomTom | `tomtom` | `TomTomGeocoder` | — | `TomTomRouter` | — | `TomTomTraffic` | `TomTomTileProvider` | — | API key, query parameter `key` |
| Mapbox | `mapbox` | `MapBoxGeocoder` | — | `MapBoxRouter` | — | — | — | — | API key, query parameter `access_token` |
| Radar | `radar` | `RadarGeocoder` | — | `RadarRouter` | — | — | — | — | API key, `Authorization` header |
| URL-template raster | `openstreetmap` | — | — | — | — | — | `UrlTemplateRasterTileProvider` | — | none |
| URL-template vector | `osmus`, `openfreemap` | — | — | — | — | — | — | `UrlTemplateVectorTileProvider` | none |

The three tile sources stay base-map sources in the reference apps, selected by the layers control, and are not provider rows in a capability picker. They serve no callable contract besides tiles.

HERE and Mapbox also implement the provider layer's `VectorTileProvider`. Both the shared catalog and the bindings register that implementation through `configure_provider` for the vector-tiles contract; the registry's descriptors do not yet offer the capability, because no committed style paints either vendor's schema, so the reference apps have no way to show the base. One recorded vector tile is exported under each descriptor's fixture route and driven through the bridge by a native test; the style family and the served contract follow with the vector-source milestone.

## Credential shapes

A registration takes one string in the credential slot. Its meaning depends on the provider:

| Shape | Providers | Value |
|---|---|---|
| Query-parameter key | MapQuest, HERE, Google Maps, TomTom, Mapbox | The issued API key; the provider crate names its parameter |
| Header key | Radar | The issued API key, sent as `Authorization` |
| Deployment address | OSRM, Valhalla | The base URL of the deployment; an empty value reaches the crate's local default, so a shared deployment must be named |
| Deployment address or public instance | Photon | The base URL of the deployment; an empty value reaches the public Photon instance |
| None | OpenStreetMap Standard, OSM US, OpenFreeMap | Empty |

The descriptor's `credential_label` decides whether a settings surface renders a credential field. A blank label means keyless, and the surface offers the provider without a field.

## Registration model

- `configure_provider(contract, provider_key, credential)` fills exactly one capability and leaves every other slot serving.
- `configure_provider_set(provider_key, credential)` fills every capability the named provider's catalog row serves. The credential-only `set_provider_credential` is removed (owner ruling 2026-11-10): a catalog with several credentialed providers cannot infer which set a bare credential means.
- A provider that does not serve the requested contract is refused before any slot changes, with `ProviderNotConfigured` on native and the unsupported-capability code in the browser.

## Fixture export

The provider layer's contract tests carry recorded wiremock payloads for every vendor. The SDK exports those payloads rather than recording live responses:

1. Copy each payload into `fixtures/{provider_key}/{purpose}.json` at the repository root, where the fixture server resolves descriptor paths from.
2. Register the request pattern in the provider's descriptor under `fixture_routes`, with the method and the path pattern the vendor client uses.
3. Serve the route through `platform/web/serve.mjs`, which reads the descriptor.
4. Point a native integration test at a loopback server carrying the same bytes; every vendor domain type exposes `with_base_url` for exactly this.

The exported files are a shared artifact: the browser harness and the native tests read the same bytes, so a drift in either shows up as one failing route rather than two divergent fixtures. A place-search route also pins the first suggestion its payload must produce — the line and the quality class — so the browser suite asserts the mapping for every provider serving type-ahead, rather than for one provider's example.

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

The native integration tests under `crates/rustic-maps-native-core/tests/vendor_provider_*.rs` drive an exported payload through the vendor provider and the SDK bridge on a loopback server: geocoding for the five credentialed vendors, routing for the full-service and the five credentialed vendors, traffic for the three traffic-serving providers, and tiles for TomTom's raster fetch and HERE's and Mapbox's vector fetches. The keyless routing payloads are exercised through the browser harness.

## Why the SDK tests the seam again

The provider layer's suites prove that each vendor requests the right URL and parses its own payload. The SDK's records are different types with their own invariants: a `Coordinate` that validates latitude and longitude, a `RouteData` whose legs and maneuvers are the renderer's input, a `SearchResult` whose quality classes drive the suggestion UI. The SDK's tests prove the parts only this repository owns:

- the bridge conversion, including the fields the SDK adds or refuses;
- the catalog dispatch, so a provider key constructs the implementation the descriptor promises;
- the registry descriptors and their per-platform mirrors;
- the reference-app selection path, from the settings row to the registered slot.

A vendor parser regression is caught upstream; a bridge, dispatch or descriptor regression is caught here. The exported payloads are what keep both suites honest about the same wire shape.

## What makes a provider selectable

1. The crate dependency is in the workspace and in `rustic-maps-native-core` and `rustic-maps-web-bindings`.
2. The native catalog has one row: key, display name, credential requirement, served contracts, install and install-all calls, tile ceilings.
3. The wasm dispatch has one row with the same key and display name.
4. `platform/providers.json` has a descriptor with display name, credential label, per-platform availability, attribution, terms, settings hint, `wasm_registration` and fixture routes.
5. The iOS resource mirror is refreshed, and the platform-parity gate passes.
6. The reference apps' dispatch accepts the key: Android and web read the descriptor; iOS and the React Native native runtime carry a registration arm for each credentialed provider.
7. The provider-name guard passes, with ordinary-word display names handled by the rule above.
8. The provider-capability pages, the add-a-provider guide and the Rust page describe it.

## Open decisions

- **What an empty credential means.** The reference apps pass each keyless descriptor's `demo_base_url`, so a shared deployment is always named. On native, an empty credential still falls back to the crate's local default (OSRM `localhost:5000`, Valhalla `localhost:8002`); Photon alone falls back to its public instance. Decide whether the SDK keeps the crate defaults, refuses an empty credential for a deployment-addressed provider, or reads a descriptor default instead.
- **Fixture coverage.** Geocoding payloads are exported for the five new vendors. Decide which contracts must ship exported payloads (the proposal: geocoding, routing, traffic and raster tiles wherever the provider serves them) and who records the remainder.
- **Multi-credential launch injection.** With six credentialed providers, a launch-time default without an explicit key is ambiguous. Decide whether the default stays MapQuest or the injection refuses without a key.
- **Public demo servers.** A shipped reference app that selects a keyless provider reaches a public demo server under its rate limits. Decide whether the demo may do so by default or must name it explicitly.

## Risks

- **Name-guard collisions.** `HERE`, `Google`, `Radar` and `Mapbox` are ordinary words. The guard matches those display names case-sensitively and skips their lowercase keys, so prose does not trip it; every other name keeps case-insensitive substring matching, which catches vendor identifiers.
- **Credential default ambiguity.** Adding a second credentialed provider breaks the "only available provider" shortcut. The shortcut is removed and its tests updated in the same change.
- **Web credential Apply.** The browser's convenience export registers MapQuest by construction; the demo must pass the row's own key instead.
- **Binary size.** Five more vendor crates on every target. The catalog is compiled in; a feature gate that drops unused providers from a distribution is a separate decision.
- **Screenshots.** The settings provider section changes, so the published set must be recaptured from one revision across the four platforms, or the deferral recorded with its reason.

## Per-capability selection

The registry holds one slot per contract. `configure_provider(contract,
provider_key, credential)` replaces exactly one slot; every other slot keeps
serving, so one map can mix a vendor for geocoding, a URL-template source for
tiles and another engine for routing. Succeeding is the only way a slot
changes: a refused key or credential leaves the previous provider in place and
throws with the reason. An empty credential is legal for a keyless provider
(a URL-template source, a self-hosted OSRM server) and refused for a provider
whose descriptor declares a credential label.

`provider_registrations()` reports every capability in a stable order with the
serving key, the descriptor's display name and whether a provider is installed.
The reference apps render one settings row per capability from this list plus
the shared descriptors, ordered Geocoding, Routing, Place search, Distance
matrix, Traffic incidents, Traffic flow, Elevation, Raster tiles, Vector tiles,
Offline routing. A capability with no installed provider reads "Not
configured"; a picker is limited to descriptors that declare that capability
for the platform. The choice joins each app's versioned preferences record.

Eight contract families also accept a host implementation through the UDL's
`[Trait, WithForeign]` interface and `register_*_from_foreign`; it writes the
same slot as `configure_provider`, last write wins, and the readback reports
it. Vector tiles and offline routing keep their dedicated source and archive
calls rather than a foreign provider object.

The catalog table in `rustic-maps-native-core` is the only place a provider
name reaches the binding besides the descriptors and dispatch arms. Adding a
provider is a crate, a descriptor and a table row; compiling a crate into the
binding does not make it selectable by itself.

Tests: the catalog constructs each key and reports its contracts; an unknown
key throws `ProviderNotConfigured` naming it; a refused credential leaves the
previous provider serving; a browser case configures routing to a second
provider, plans from the fixture, reloads and sees the choice restored.
