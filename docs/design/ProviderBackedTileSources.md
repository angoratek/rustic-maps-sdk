# Provider-backed tile sources

**Status:** partly landed, owner-directed 2026-11-05 (`PLAN.md` M77). The provider vector slot, the provider style document, the registry `document_url` rows and the layers control ship; HERE raster upstream remains open.
**Question it answers:** how does a registered provider serve a vector or raster base map through the same scheduler and style pipeline the URL-template sources use, without a second tile path?

## Outcome

A reference app selects HERE or Mapbox vector tiles, or HERE raster tiles, in the layers control. The base draws through the provider registry slot exactly as MapQuest raster tiles and URL-template sources do today. The capability rows list the tile contract for every provider that serves it. No vendor template, endpoint or schema name lives outside the provider crate and its descriptor.

## What exists

- **Raster.** `MapView` draws the registered `RasterTileProvider` slot. MapQuest and TomTom register into it; a URL-template source registers a provider over any `{z}/{x}/{y}` template. Adding HERE raster is a provider crate addition, not an SDK concept.
- **Vector.** `MapView.set_provider_vector_tile_source` installs a named provider tileset through the registry's vector slot, and `set_provider_vector_tile_source_from_document` installs the provider's own style document. HERE and Mapbox register as `VectorTiles` in the catalog; the URL-template member stays for any `{z}/{x}/{y}` source.
- **everymap.** `VectorTileProvider` and `VectorTileSourceDescriptor` already exist, and HERE and Mapbox implement them. Nothing new is needed upstream for vector beyond a way to name a source with the provider's own style constraints.

## Target shape

1. **Provider vector source record (addition-only).** A new dictionary names a provider key and one of the renderer's committed styles, with the zoom span and attribution the surface displays. It does not name a template.
2. **View member.** `MapView.setProviderVectorTileSource(source)` resolves the registry's vector slot for the provider key, reads its source descriptor, and feeds the same scheduler the URL-template path feeds. The registry refuses the selection when the provider is not registered for vector tiles.
3. **Registration.** `configure_provider(VectorTiles, key, credential)` installs the provider through the catalog. The bridge already adapts `VectorTileProvider`; the catalog gains the contract for HERE and Mapbox.
4. **HERE raster upstream.** `everymap-providers-here` gains a raster provider over HERE's tile endpoint, with the typed format and resolution the contract carries, committed fixtures and contract tests. The SDK catalog registers it exactly as TomTom's.
5. **Layers control.** The base-map rows come from the registry: a credentialed provider with `raster_tiles` becomes a raster base, one with `vector_tiles` becomes a vector base. The row states the credential state and the attribution line.
6. **Capability rows.** Vector Tiles appears for HERE and Mapbox, Raster Tiles for HERE, from the same descriptor list the other rows use.

## The style question (resolved 2026-11-06)

The renderer's committed styles target the OpenMapTiles schema. Mapbox serves the Mapbox Streets schema and HERE serves OMV; neither paints correctly through an OpenMapTiles style. Two options:

- **Ship a style per schema.** Add committed styles for Mapbox Streets and OMV to `rustic-maps-style`, with the golden coverage the other styles have. Largest work, truthful rendering.
- **Map known schemas onto the built-in styles.** A style adapter maps layer names per schema. Cheaper, but a mapping table is a new maintenance surface and hides schema differences.

The design recommended the first option and kept provider vector bases unavailable until their style existed, rather than drawing a wrong map. The owner then ruled for the provider's own published document instead, with a committed per-schema document as the fallback; the next section records that shape.

## Loading the provider's own style document

The 2026-11-06 ruling resolved the question above for the credentialed providers: load the style the provider publishes, which matches its tiles by construction. The mechanism, landed 2026-11-12:

- **Member.** `MapView.set_provider_vector_tile_source_from_document(source, style_document_url)` is the asynchronous sibling of `setProviderVectorTileSource`. The committed-style member stays for a provider painted with a style the SDK carries: the fallback where a vendor's terms refuse hosted resources, and host-registered providers.
- **Fetch.** The document is fetched through the provider's authenticated style-resource client, which the catalog registers beside the provider (`VendorImplementations.style_resources`). The credential stays in the vendor client; the SDK names only the address.
- **Cache.** A document that compiled is kept per provider key and address, so a repeated install of the same family costs no request. A malformed document is never remembered.
- **Refusals.** A missing provider, a provider without a style-resource fetch, a failed request and an uncompilable document all throw and leave the installed base in force.
- **Fixtures.** Each credentialed vector provider's descriptor serves a committed synthetic document under a `style_document` fixture route, so the browser and native suites never reach the network and the vendor's own file is never committed.

Also landed: each credentialed vector provider's `document_url` row in the registry, the served vector-tiles contracts, the layers control installing a vendor family through this member, and the provider-aware glyph and sprite fetches through the same style-resource client.

## Tests

- A raster test registers a provider and draws a committed tile through the scheduler.
- A vector test registers a provider, selects a committed style, and draws committed vector tiles through the same scheduler; a missing style is refused with a named reason.
- A catalog test proves every provider advertised for a tile contract can be installed and drawn.
- A registry/descriptor parity test proves the layers control rows match the catalog.
