# Provider genericity at every layer

The owner's north-star directive (2026-10-29): the SDK supports multiple
providers in Rust, but the **reference-app and test layers still leak MapQuest
specifics**. This page records the audit, the target design and the migration
milestone (M15). The rule is in `CLAUDE.md` under "North star: provider
genericity".

## Why this matters

Adding HERE, Google, TomTom, Mapbox, Radar or any URL-template source must be a
provider crate plus a registry descriptor plus one registration arm. The five
vendor crates now exist in the published provider layer, so adding one here is
a descriptor, a catalog row and a dispatch arm. If the
playground, the fixture server, the UI suites or a refapp shell name a vendor,
every new provider becomes a cross-platform edit — and the refapps drift from
the SDK's trait-based design.

## Audit (2026-10-29)

Provider-specific leakage found outside the provider crates:

| Layer | Leak | Location |
|---|---|---|
| Web dev server | MapQuest geocoding/directions/tiles fixture routes and path shapes are hardcoded; the `/mapquest/...` base is structural, not descriptor-driven | `platform/web/serve.mjs:70-160` |
| Web playground | `register_mapquest_providers`/`clear_mapquest_providers` calls, a "MapQuest API key" label, and a hardcoded attribution line | `platform/web/index.html:496,541,732,740`; `platform/web/main.mjs:19-20,46-47` |
| Web tests | Fixture base `/mapquest`, vendor path assertions, MapQuest-named exports | `platform/web/tests/live_provider.spec.mjs`, `contract_sync.mjs` |
| Wasm seam | The registration export is vendor-named and has no generic dispatch (`register_mapquest_providers`); live calls assume the single registered MapQuest set | `crates/rustic-maps-web-bindings/src/live_providers.rs` |
| Refapp shells | Attribution and some key labels are hardcoded strings rather than descriptor fields | Android `ReferenceApplicationConstants.kt:65,141,149`; iOS `ReferenceApplicationConfiguration.swift:12-14`; RN `App.tsx:132` |
| Automation | ~~The launch-argument key name is `mapquestApiKey`~~ — **resolved**: the Android/RN launch-intent extra and the iOS/RN environment variable are provider-neutral (`providerApiKey`/`PROVIDER_API_KEY`), and the keyed Android suite is verified with them (round 68) | Android/RN `RefappConstants`, `docs/ProviderCredentialsAndBrowserProviders.md` |
| Fixtures | Provider fixtures live under `fixtures/mapquest/…` (fine as a provider-owned directory), but the dev server maps them through vendor-specific code | `serve.mjs` |

**Status (2026-10-31): the table above is the dated 2026-10-29 audit, and
M15 resolved every row.** `serve.mjs` resolves fixtures through the shared
registry descriptors under `/providers/{provider_key}/…`; the playground and
its tests name providers only through registry data (`setProviderBaseUrl` /
`configureProviderSet`, and `clearProviderRegistrations`); the wasm seam
serves the contract-shaped `configureProvider` and `providerRegistrations`
over the same per-capability registry the native targets use; the reference-app
labels and attribution render the descriptors; and automation uses the
provider-neutral `providerApiKey`/`PROVIDER_API_KEY` names (verified keyed on
the Android suite, round 68). `platform/web/tests/provider_name_guard.mjs` is
the standing gate. The table stays for history; the current design is below.

Correctly provider-specific and staying so: the `everymap-providers-mapquest`
crate, its fixture directory, the `configureProviderSet` and
`configureProvider` registration path, and the refapp's registration dispatch
arm. Vendor code belongs in exactly those places.

## Target design

### 1. One shared provider registry

`platform/providers.json` (or a `providers` section in the shared artifact)
holds one descriptor per provider:

```json
{
  "provider_key": "mapquest",
  "display_name": "MapQuest",
  "credential_label": "API key",
  "registration_available": { "android": true, "ios": true, "react_native": true, "wasm": true },
  "attribution_text": "© MapQuest · Map data © OpenStreetMap contributors (ODbL)",
  "terms_note": "Provider-specific rate limits and caching terms.",
  "wasm_registration": "mapquest",
  "fixture_routes": [
    { "method": "GET", "path_pattern": "^geocoding/v1/address$", "fixture": "fixtures/mapquest/geocode_address_city.json" },
    { "method": "GET", "path_pattern": "^directions/v2/route$", "fixture": "fixtures/mapquest/directions_route_two_waypoints.json" },
    { "method": "GET", "path_pattern": "^tiles/v1/render/latest/[a-z0-9_-]+/(\\d+)/(\\d+)/(\\d+)/(256|256@2x)/(png|jpeg)$", "fixture_kind": "tile_directory" }
  ]
}
```

Descriptors contain no credential values, ever.

### 2. Generic fixture routing

The dev server serves `/providers/{provider_key}/{provider-relative path}` and
resolves the request by looking up that provider's `fixture_routes`. A live
test registers the provider with `base_url = origin + "/providers/" + key`, so
the provider's own path shape appends unchanged. Tests name a provider key,
not a vendor path; the fixture files stay in the provider's own directory.

### 3. Generic wasm registration

The contract's `MapSdk` builds for wasm32 since round 117, so the
contract-shaped calls exist on that surface, not a vendor-shaped copy:

- `configureProvider(contract, providerKey, credential)` selects one
  capability and leaves every other slot serving.
- `providerRegistrations()` reports, in a stable order, which key serves each
  capability.
- `configureProviderSet(providerKey, credential)` fills every capability the
  named provider serves from one credential.

The module the demo loads (the generated `wasm2` package, built by
`scripts/build-wasm2-package.sh`) serves the same registry: `MapSdk.configureProvider`,
`MapSdk.providerRegistrations`, `MapSdk.configureProviderSet(providerKey, credential)`
and the canvas glue's `clearProviderRegistrations` are the browser calls, and the
legacy `registerProvider` / `clearProvider` pair is gone. No snake_case export reaches
JavaScript: every Rust export sets `js_name`, so the callable names are
lower-camel. The playground reads the descriptor rows, renders
labels/attribution from them, and calls the same per-capability registry.

### 4. Descriptor-driven refapp chrome

Attribution, credential labels and hints come from the descriptors in every
shell (Android, iOS, RN, web); the registration dispatch arm is the only
provider-named code. The automation seam's key name becomes a generic
`providerApiKey`/`providerKey` pair, with the vendor name only in the
descriptor.

### 5. A guard test

A repository test asserts that provider names do not appear outside their
provider crate, their fixture directory, the shared registry artifact and the
registration dispatch arms (an allowlist analogous to the docs scrub). It
keeps the north star from eroding.

### 6. A vendor-neutral URL-template source

The provider layer's URL-template crates close FR-D's "any URL/template
source": `everymap-providers-url-template-vector` serves a vector tileset over
a template containing `{z}/{x}/{y}`, naming no vendor and needing no registry
descriptor of its own — the descriptor (source name, attribution, zoom span)
is supplied by the integrator, who also owns the template. The crates share
the provider layer's transport, limiter and retry loop, so a new source cannot
re-introduce a private copy of the provider plumbing.

The raster half of the same family is
`everymap-providers-url-template-raster`: it implements the typed raster
contract over the same validated template, and the integrator's source
declaration adds what a raster tileset has and a vector one does not — the
image format and pixel resolution it actually serves, and the `User-Agent` its
requests identify the application with (public tile services refuse clients
that send nothing). The template validator and the tile-source status
vocabulary live once, in the vector crate, and the raster crate reuses both,
so the two contracts can never disagree about what a valid template is or how
a failed tile request classifies. The exported
`register_url_template_raster_tile_source` call is how a native application
registers one at runtime, straight from a registry descriptor: the base-map
switch in the three native reference apps is that call plus a settings row.
`rustic-maps-core`'s bridge adapts the provider layer's tile records and
failures to the SDK's contract at one seam.

## Milestone M15 (phases)

0. **Registry** — add the shared `providers.json` descriptors (seeded with
   MapQuest) and wire the four refapps to read attribution/labels from it.
1. **Generic fixture routing** — `serve.mjs` becomes a descriptor-driven
   `/providers/{key}/…` router; migrate the MapQuest routes and the web tests.
2. **Generic wasm registration** — `configureProvider` / `providerRegistrations`
   on the contract-shaped `MapSdk` surface, with the browser seam dispatching
   through the same per-capability registry; the playground uses descriptors.
3. **Automation naming** — generic `providerApiKey` intent/env keys plus a
   provider-key argument, MapQuest only in descriptors.
4. **Guard test** — the provider-name allowlist test, wired into CI.
5. **Second provider proof** — before/with the first non-MapQuest provider,
   demonstrate the full path (descriptor + catalog row + dispatch + fixture
   routes) with one additional provider; the OSM US URL-template vector source
   and the keyless Photon and OSRM providers are the shipped proof.

## Risks

| Risk | Mitigation |
|---|---|
| Descriptor schema churn | Keep it data-only; schema additions are tolerant in consumers; version the artifact |
| Fixture-path generality | Patterns are per-provider data; the server never interprets a vendor path |
| Wasm dispatch table grows with provider crates | One match arm per provider; compilation already per crate |
| Test churn | Migrate tests with the router; both vendor-relative and registry paths exercised during the transition |
| Attribution differences | Descriptor text is the single source; shells render it verbatim |
