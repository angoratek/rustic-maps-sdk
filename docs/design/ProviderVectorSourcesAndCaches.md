# Provider-backed vector sources and per-provider tile caches

**Status:** design, owner-directed 2026-11-06 (`PLAN.md` M78).
**Questions it answers:** why can only the URL-template providers serve a
vector base today; how do HERE and Mapbox join the same path with their own
tiles and a style that tells the truth; and how does a cache stop serving one
provider's tiles for another?

## Outcome

Every vector-capable provider — the keyless URL-template pair and the
credentialed HERE and Mapbox `VectorTileProvider` implementations — registers
through one path, selects one of its declared style families, and paints
through the same renderer. Every tile cache is namespaced by the provider that
produced its bytes, for raster tiles, vector payloads and tile packs alike.

## Why HERE and Mapbox cannot serve a vector base today

The rendering pipeline is already provider-neutral: `VectorBaseScheduler` takes
an `Arc<dyn VectorTileProvider>` and the adapter `EveryMapVectorTileProvider`
wraps any everymap provider. The gap is selection and registration:

- `rustic_maps_providers::VendorImplementations` carries no vector-tile slot,
  and the shared and native catalog tables omit `VectorTiles` from HERE's and
  Mapbox's served contracts, so `configureProvider(VectorTiles, …)` is refused
  before a provider is built.
- `ProviderRegistry` already owns a vector-tile slot and accessors, but
  `MapSdk` exposes only the raster accessors, so nothing can register or read
  it.
- The only reachable view member, `set_vector_tile_source`, takes the SDK's
  URL-template record, rebuilds `UrlTemplateVectorTileProvider` inline with
  empty headers, terms and style hints, and constructs the scheduler itself.
  A credentialed endpoint has no expression in that record.
- The registry JSON lists neither provider as serving `vector_tiles` and names
  no vector styles for them.

## One registration and one selection

`VendorImplementations` gains a `vector_tiles` slot; the HERE and Mapbox
implementations build `HereTileProvider` and `MapBoxTileProvider`; both catalog
tables and `platform/providers.json` list `vector_tiles` for them with their
declared style families. `MapSdk` gains
`register_vector_tile_provider`/`required_vector_tile_provider` on the existing
registry slot, mirrored in the UDL and the wasm façade.

The view gains one provider-backed member beside the URL-template one. Both
members resolve to the same internal selection — a provider, a style id, an
optional style source id and attribution overrides — and both construct the
same scheduler. The URL-template member stays as the frozen contract; the new
member is additive. Internally there is one `VectorSourceSelection` and one
scheduler entry, so no third path appears.

The URL-template path stops rebuilding the everymap declaration by hand: it
uses the everymap named declarations, so the recorded headers, terms text,
subdomains and style hints survive instead of being dropped.

### Application-provided vector tiles

Application code can serve vector tiles without a provider crate. It
implements the `VectorTileProvider` callback contract — a synchronous
`source_descriptor()` naming the tileset for its cached payloads, with its
attribution and zoom span, plus the wire encoding and style hints it declares
in the contract mirror, and an asynchronous `fetch_vector_tile` — and
registers it through `MapSdk::register_vector_tile_provider_from_foreign`. The
SDK's native provider descriptor carries the source name, attribution and zoom
span only. The declared encoding and style hints are advisory: each fetched
`VectorTileData` reports the encoding its own payload arrived in and the
decoder unwraps by that per-tile value, while the installed style id alone
selects the style document. The
registration records the selection under the application key `application`
and registers the adapter on the same registry slot a catalog provider uses,
so `MapView::set_provider_vector_tile_source` resolves it when the source names
that key. `vector_tile_provider_is_registered()` reports whether the slot is
filled. The browser surface has no foreign-callback seam: a page registers
through `configureProvider` or the URL-template source.

The URL-template record also carries the declaration's terms text, `{s}`
subdomains, per-request headers and style hints. A null or empty field declares
none, and the conversion hands every value to the provider layer instead of
dropping it. The browser surface consults no named declaration, so every extra
there is caller-declared; the native surface inherits a matching keyless
tileset's recorded extras when the caller omits them.

## Styles: the vendor's own document, with its glyphs and sprites

The committed Bright, Fiord, Positron and Carto documents are
OpenMapTiles/Carto-schema styles. HERE serves OMV and Mapbox serves Streets;
painting those tiles with an OpenMapTiles style yields a background and
fragments, not a map.

The owner ruled (2026-11-06) for the vendor's own style document: a
credentialed vector provider loads the style the vendor publishes, so the
layers, filters, glyphs and sprites match the tiles by construction. That
brings three obligations the renderer does not meet yet:

- **The style document itself.** Fetch it through the provider client, so the
  credential stays inside the client, cache it per provider, and pin a
  synthetic structurally equivalent document as the fixture so tests never
  touch the network. The fetch seam is recorded below the terms research.
- **Glyphs and sprites.** The vendor styles reference hosted glyph ranges and
  a sprite sheet. The SDK has a glyph pipeline with a range cache that already
  namespaces by glyph source; a sprite loader exists but is host-injected.
  Both extend to provider URLs fetched with the provider credential, with the
  provider key in the glyph-range namespace.
- **Terms and attribution.** Each vendor's terms must allow the SDK to fetch
  and cache the style, glyphs and sprites with the customer's credential; the
  attribution the vendor style carries becomes the one the view reports. Where
  a vendor's terms refuse it, that provider keeps a committed style document
  in that vendor's schema instead, and the capability row states which path it
  uses.

The keyless URL-template providers keep the committed OpenMapTiles/Carto
documents; nothing changes for them.

### Terms research, 2026-11-10

Public sources only; no credential was used and no credentialed endpoint was
called. "Redistribute" means the SDK vendor bundling or proxying assets; an
end user's runtime display under the application developer's own credential is
that developer's licensed use.

- **HERE.** A credential serves Vector Tile API v2
  (`https://vector.hereapi.com/v2/vectortiles/{map}/mc/{z}/{x}/{y}/omv`), the
  Mapbox-compatible style at
  `https://assets.vector.hereapi.com/styles/berlin/base/mapbox/tilezen` with
  the registered key in the query, and its referenced assets ([API
  reference](https://docs.here.com/map-rendering/reference/gettile-1),
  [Mapbox page](https://docs.here.com/map-rendering/docs/mapbox)). Caching is
  bounded: "as explicitly allowed by the caching headers", otherwise up to 30
  days, with 24 hours recommended; building a repository of assets or scaling
  one request across users is forbidden ([caching
  guide](https://docs.here.com/map-rendering/docs/cache-maps-vector-tile-api)).
  Sub-licensing, distribution, modification and derivative works are not
  granted, and style documents, glyphs and sprites are "HERE Materials"
  ([HERE Platform Terms v3.0, 14 Nov
  2026](https://assets.here.com/x7rx8ayph7ee/4zbYtdcDHM69wHXnSEWEcn/b650026a1c2528b35e102985c9192f26/HERE_PLATFORM_TERMS_11.14.2026.pdf)
  §2.b, §3.c, §3.p). Attribution is `© 20XX HERE, XXX`, refreshed every 24
  hours ([required
  notice](https://docs.here.com/map-rendering/docs/required-copyright-notice-vector-tile-api)).
  **Verdict:** runtime fetch and on-device cache with the developer's
  credential are allowed; shipping a HERE style document, glyph range, sprite
  sheet or tile in the repository, a package or a committed fixture is not.
- **Mapbox.** The Styles API serves style JSON, sprites and glyphs;
  "Licensed Map Content" includes all three, and the terms permit 30-day
  single-device caching populated directly from the API, with no distribution
  or proxying ([Mapbox Product Terms, 20 Oct
  2026](https://cdn.prod.website-files.com/609ed46055e27a02ffc0749b/6ad79764a359d21b46049ea4_Mapbox%20Product%20Terms%20(October%2020%2C%202026).pdf)
  §2.8.1, §3.32). Mobile access must go through Mapbox Mobile SDKs (§2.9.1),
  and Studio styles may only run on a Mapbox Map (§2.11). **Verdict:** the
  runtime-fetch path is web-first; mobile use needs an Order. No Mapbox asset
  may be bundled.
- **Consequence for tests.** A committed copy of a vendor style document is
  redistribution, so the fixture for these tests is a synthetic document with
  the vendor's structure and neutral content, not the vendor's file. The live
  path is exercised by a maintainer smoke, never by the committed suites.

### Fetch seam, 2026-11-12

The SDK needs no credential to load a vendor's style resources: the vendor
client already holds the credential and applies it. everymap's shared provider
client exposes `build_request(method, url)` and `request(request)` publicly,
and `request` applies the client's authentication provider, transport, token
bucket, retry policy, timeout and request observer, observing the URL with its
credential query values redacted. The SDK therefore names only the resource
address: no credential is retained by the SDK and no vendor URL shape enters
the core.

- `rustic_maps_core::StyleResourceClient::fetch_style_resource(url)` is the
  SDK-side contract.
- `rustic_maps_providers::VendorImplementations::style_resources` carries one
  implementation per vendor that publishes a hosted style. HERE's client
  appends `apiKey` and Mapbox's `access_token`, both taken from the
  registration credential.
- The native suite fetches a committed synthetic document per provider through
  a loopback server and asserts three things: the registered credential
  reached the request through the client, the bytes are the committed
  document, and the document compiles into a vector style. Neither the SDK nor
  the test constructs a credentialed URL.

This withdraws the earlier recorded dependency on an upstream everymap
accessor: the provider layer's `VectorTileProvider` trait still needs no
change, and no SDK-owned transport seam is introduced.

The document now loads at selection. `MapView.set_provider_vector_tile_source_from_document(source, style_document_url)`
is the asynchronous sibling of the committed-style member, and it is the one a
vendor family installs through: the fetch goes through the style-resource
client registered beside the provider, the document compiles in the renderer,
and a compiled document is kept per provider key and address for the session
so a repeated install costs no request. Persisting it under the terms'
thirty-day window is the disk-caching item below. A malformed document is never remembered, and every
refusal leaves the installed base in force. The committed-style member stays
for the fallback where a vendor's terms refuse hosted resources and for
host-registered providers.

The registry rows landed on 2026-11-12: HERE and Mapbox serve the
`vector_tiles` contract with a declared zoom span (17 and 22) and one style
each (`omv-light`, `streets-light`) whose `document_url` names the provider's
published document without a credential. A style entry names either a
committed style or a provider document; a document address must be `https`
and carry no query, because the provider client appends the credential. The
capability pages name both families and their style ids.

The style resources persisted on 2026-11-13: a document, a glyph range or a
sprite sheet is served from the tile cache's store under the terms' thirty-day
window before the provider is asked, keyed by the provider and the address
only, so no credential enters the key and a key rotation keeps the bytes
valid. Without an installed cache the fetch goes straight to the provider. The layers control installs a vendor family
through the sibling member on all four surfaces (2026-11-12), each listing
the family once its credential is applied.

The provider declares its schema family as a typed field on the everymap
vector source descriptor (`style_schema`: `openmaptiles`, `carto`, `streets`,
`omv`), so the SDK never matches opaque hint strings. The registry descriptor
names, per provider, the style document to load and the attribution to show.

Every committed document declares one vector source keyed `base`. The
scheduler binds the provider to that style source id through the existing
`StyleSourceBindings`/`VectorSourceBinding` policy, which already carries zoom
clamping, bounds, scheme and the attribution rule: the style's own `minzoom`,
`maxzoom` and `attribution` reach the base, and the provider descriptor fills
whatever the style leaves out. That replaced the scheduler's private copy of
the policy on 2026-11-13, and both surfaces' status readbacks now report the
style's line rather than the descriptor's. The reference application still
passes the registry's style attribution into its URL-template source, because
the committed styles declare no source attribution of their own; that copy
stays until those styles carry one. No tile URL placeholder mapping is needed:
the provider owns its URL, and a style's `tiles`/`url` stays documentation.

### Hosted glyphs and sprites, 2026-11-13

The glyph half is in: a provider-document install resolves the document's
`glyphs` template and fetches the Latin blocks through the same style-resource
client. The protocol serves 256-codepoint blocks, so the ranges are `0-255` and
`256-511`; the served payloads are committed under `fixtures/glyphs/`. The pair
decodes into one `LabelFont` the scheduler carries to the label pass, cached per
provider, document address and font stack.

The credential's scope is the document's own origin plus every host the
provider declares: `VendorImplementations::style_resource_hosts` names them in
the provider crate, `platform/providers.json` mirrors them, and the
registration carries them beside the style-resource client. A glyph address on
any other host refuses the install before a request, with the host named; a
relative reference is resolved against the document's own directory (or its
origin, when root-relative), as the style specification allows. A vendor URI
scheme is first rewritten by the provider that owns it, as recorded below. An
address the SDK still cannot turn into a fetchable `http`/`https` template —
a missing placeholder, an unrecognized scheme, a reference climbing above the
document's directory — is not a credential risk and so is not a refusal: the
install proceeds with the embedded Latin range. The same
holds for a style with no endpoint or no symbol font stack, and for a range
that cannot be fetched, decoded or packed: a glyph failure degrades labels,
never the base.

The sprite half takes the same shape. The renderer's `SpriteSource` is
synchronous, so the install prefetches the protocol's two addresses for the
style's `sprite` base — `<base>.json` and `<base>.png` — through the
style-resource client, decodes the atlas and injects it into the scheduler
(`with_prefetched_sprite_atlas`), cached per provider, document and base
address. The style parser keeps the declaration's `default` address and
records a density-specific one (`@2x`) as unsupported, so the install loads
the one sheet the style declares. One resolver holds the base to the
document's origin or a declared host and resolves a relative base against the
document's directory, exactly as it does for a glyph template, with the
provider's URI-scheme mapping applied first; an address the SDK cannot fetch
as `http`/`https` degrades rather than refuses, for the same reason. A style without a sprite, and an index or sheet that cannot be fetched
or decoded, draws its text without icons, which is the renderer's existing
degradation.

Both install paths run the same seams. The PNG decoder builds on wasm, so the
browser canvas prefetches and decodes the sheet in the page and packs the
document's glyph ranges into a label font, caching each per provider, document
and resource. A document address the page names relatively is made absolute
against the page's origin once, so the document's own resource references
resolve against a single address.

### Vendor URI schemes, 2026-11-16

The official Mapbox documents name their own resources with the vendor's URI
scheme: `mapbox://sprites/{username}/{style_id}` for the sprite base and
`mapbox://fonts/{username}/{fontstack}/{range}` for the glyph template. Those
are not web addresses, so the address resolver classified them as unfetchable
and the install dropped every icon, shield and POI symbol. The provider layer
now maps them, because the scheme and endpoints are the vendor's:

- `rustic_maps_core::StyleResourceClient` gains
  `resolve_declared_style_resource_address`. Its default returns `None` and
  leaves the declaration unchanged, so HERE's relative and absolute addresses
  keep their existing rule and a provider that publishes ordinary web
  addresses needs no mapping.
- The Mapbox implementation maps the sprite base to
  `https://api.mapbox.com/styles/v1/{username}/{style_id}/sprite` and the glyph
  template to
  `https://api.mapbox.com/fonts/v1/{username}/{fontstack}/{range}.pbf`, keeping
  the `{fontstack}` and `{range}` placeholders for the renderer to substitute.
  A trailing version or cache segment after the style id names the same sprite
  base and is dropped from the Styles API path.
- The rewritten address still goes through the same origin-and-declared-host
  check: the mapping runs before resolution, never around it. A mapping that
  lands on an undeclared host is refused before a request, exactly like an
  absolute address the document wrote itself.

The credential parameter stays the provider client's business and is unchanged:
HERE's client appends `apiKey`, Mapbox's appends `access_token`. The live smoke
(`scripts/live-style-smoke.mjs`) fetches a real style, sprite pair, glyph range
and tile through those parameters; the committed suites never call a provider.
The HERE required notice remains `© 20XX HERE, XXX`, with the year and the
licensee substituted and the line refreshed every 24 hours.

### What a real style declares, 2026-11-16

A published document uses layout and paint properties beyond the first pass.
The compiler now applies these and carries them into the resolved plans the
renderer reads:

- `text-padding` and `text-pitch-alignment` (with `auto` resolved to viewport
  for a point-placed label and map for a line-placed one) in the resolved
  symbol layout.
- `icon-text-fit` and `icon-text-fit-padding` in the resolved symbol layout,
  and `icon-opacity` in the resolved symbol paint.
- `line-translate` and `line-translate-anchor` in the resolved line paint.
- `circle-pitch-alignment` and `circle-pitch-scale` were already carried.

Every other unimplemented property keeps a named diagnostic in
`CompiledStyle::unsupported`; `fill-pattern` is the one deliberate degradation,
and its diagnostic says the fill draws its colour. A known layer kind the
overlay pipeline cannot paint (`heatmap`, `fill-extrusion`, `hillshade`,
`color-relief`) compiles to `CompiledLayerKind::Other` with one named
diagnostic per layer, so a real style never loses a whole kind in silence. The
canvas still owns surfacing the list in its diagnostics readout.

### Style and tileset must agree, 2026-11-16

A style written for one schema paints fragments when it is served another
schema's tiles: every layer reads a source layer the tile does not carry, and
the failure looks like a renderer defect. The renderer exposes
`vector_style_source_layer_mismatches(tile, style)`, which names each painted
layer whose `source-layer` the decoded tile lacks and lists the layers the tile
does carry. The vector-base install should call it once per source and tile
address and report the mismatches through the status readback or the
diagnostics ring instead of painting the fragments; the scheduler itself is
outside this change.

The committed fixtures now prove schema fidelity rather than "tiles drawn":

- `fixtures/mapbox/vendor_style_document.json` and
  `fixtures/mapbox/vendor_style_tile.pbf` are a neutral authored document and
  tile with the Streets schema's `road`, `water`, `place_label` and `poi_label`
  layers and current attributes. The committed document keeps its resources
  document-relative so the same-origin fixture server can serve them; the
  `mapbox://` mapping is exercised by the provider and hosted-resource tests
  and by the live smoke, which reaches the real API.
- `fixtures/here/vendor_style_document.json` and
  `fixtures/here/vendor_style_tile.mvt` are the same for the OMV schema's
  `water`, `roads` and `places` layers, with document-relative resources; the
  place layer carries both names and POI icons.
- Neither fixture carries vendor material. Regression tests assert painted
  road polylines, water fills, place text, POI icon candidates and visible
  sprite pixels, and a tile from another schema is asserted to report the
  named mismatch.

## Per-provider caches

The owner ruled (2026-11-06) for one namespaced cache file rather than one
file per provider, and for a visible namespace column on tile packs.

The raster disk cache fingerprint carries only zoom, tile index, image format
and resolution. One `SqliteResponseCache` is provider-neutral, so two providers
or two credentials opening the same cache file can serve each other's raster
tiles as valid data. Tile packs inherit the same key, so two providers sharing
a pack id overwrite each other's membership.

Vector payloads already include a source name, but the two vector key builders
disagree — one hashed descriptor, one plain string — and the native surface
uses only the second, so the same tile can be stored twice under different
keys. The browser's memory cache is shared across sources on one view and
relies on the source name alone.

The design adds one normalized `tile_source_namespace` to the raster request
descriptor, threaded through the cached raster wrapper, the pack membership and
the installation:

- Catalog provider: the catalog `provider_key`.
- URL-template source: the source name plus a hash of its URL template, since
  a caller-supplied name is not enforced unique.
- Application provider: an application-supplied stable key, never one shared
  literal.

Nothing goes into the key that can leak: no credential value, no raw URL. Two
credentials on one provider therefore share that provider's entries, which
is correct — the same provider at the same address serves identical bytes,
so a key rotation keeps the cache valid. Whether a credential must instead
partition the cache is the owner question recorded in `PLAN.md` §5. The
payload key stays style-free; the styled cache is per scheduler and already
carries the style revision.

The persisted format needs no column change for lookups — fingerprints are
opaque bytes — but pre-namespace rows would stay pinned and unreachable, so a
schema migration purges raster and pack rows once. Pack identity gains the
namespace in a column so a pack can be reported and removed as the provider's
own.

Both vector key builders fold into one namespace-aware builder, and the
browser's memory store follows the same key so a provider switch cannot read
the previous provider's payloads. The cached raster and vector wrappers share
one handle type — store, namespace, time to live, executor — so a cached
provider cannot be constructed without a namespace.

## Duplication this removes

- The `VectorTiles` refusal arms and their vendor tables in both bindings.
- The SDK's hand-built URL-template declaration and its dropped metadata.
- The scheduler's private zoom/attribution policy, replaced by the existing
  source-binding policy.
- The demo's third attribution copy.
- Two vector cache key builders and two near-identical cached wrappers.

## Tests

- Registration parity: every provider in the registry that declares
  `vector_tiles` installs on every target, and `configureProvider` accepts it.
- A neutral authored style document and a schema-matched tile per family paint
  their road, water, text and icon primitives, and the tests never touch the
  network. No vendor style document, glyph range, sprite sheet or tile is
  committed.
- Two raster providers and two credentials on one cache file never share a
  fingerprint; the same holds for vector payloads and packs.
- A migration test opens a pre-namespace cache, runs the purge, and serves a
  fresh tile.
- The layers control lists exactly the declared vector families and installs
  through the provider member.

## Open questions

The owner ruled on the style approach, the cache layout, the typed style schema
and provider-aware pack identity on 2026-11-06. The 2026-11-10 terms research
settles the fetch-and-cache path and leaves these with the vendors:

1. Does an SDK vendor need its own distribution or reseller agreement under
   HERE §2.b, or is the application developer solely the licensee?
2. May a third-party SDK embed or serve HERE's published styles, and which
   styles are licensed for non-HERE renderers?
3. Do style JSON, glyphs and sprites share the 30-day results cache window?
4. Is offline storage permitted for raw-API customers at all, and for how
   long?
5. Mapbox: is a third-party mobile renderer permissible without an Order, and
   does §2.8.1's cache permission cover non-tile style assets for it?
