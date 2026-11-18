# Draw a provider's own map style

A provider that publishes a vector style document draws its tiles the way its own
cartographers designed them. This page installs that style as the base map and explains
what the SDK fetches, what it refuses and what it keeps.

## What you get

- A base map painted with the provider's published style document, not a style that
  ships with the SDK.
- Place names in the style's own font and icons from its own sprite sheet.
- Route references drawn as shaped badges, a shield or rounded rectangle in the style's
  own colours with the reference centred inside, when the style declares a shield block
  for its route-reference layer. A style whose tileset carries no route-reference layer
  declares no badge and paints no route reference.
- A status readback with the style id, the tileset name, the attribution line, every
  property the renderer could not apply, every asset that fell back, and each source's
  matched and painted layer counts.
- The fetched document, glyph ranges and sprite sheet kept in the installed tile cache
  for up to thirty days, or for the session when no cache is installed, so a reopened
  application paints without requesting them again.

## What you need

- A vector-tile provider whose registry entry declares a style document
  (`document_url`). HERE and Mapbox do, under `omv-light` and `streets-light`.
- That provider's credential applied at runtime. The SDK never ships a key.
- Android, iOS, React Native, the browser, or a Rust application.

## Steps

1. **Configure the provider with your credential.** Use its own registration path,
   exactly as for geocoding or routing. The credential lives in the provider client from
   this point on.

   ```kotlin
   import uniffi.rustic_maps.MapSdk
   import uniffi.rustic_maps.ProviderContract

   val mapSdk = MapSdk()
   mapSdk.configureProvider(ProviderContract.VectorTiles, "here", providerApiKey)
   ```

2. **Install the style by provider key and style id.** The address is the provider's
   published document as the registry lists it; it carries no credential, because the
   provider's client appends it.

   ```kotlin
   import uniffi.rustic_maps.ProviderVectorTileSource

   val source = ProviderVectorTileSource(
       providerKey = "here",
       styleId = "omv-light",
   )
   mapView.setProviderVectorTileSourceFromDocument(
       source = source,
       styleDocumentUrl = "https://assets.vector.hereapi.com/styles/berlin/base/mapbox/tilezen",
   )
   ```

3. **Read the status back.** `vectorTileSourceStatus()` reports the installed style id,
   the tileset name and the attribution line the style requires; show that line wherever
   the map is shown. It also reports the unsupported-property count with a sorted,
   bounded list of names, the assets that fell back to a degraded draw, and each
   source's matched and drawn layer counts for the last resolved frame, so your UI can
   tell "still loading" from "this style asks for something the renderer does not
   paint".

4. **Reopen the application.** The same family installs again without a request while
   the kept resources are younger than thirty days and the device cache is installed.

## What the SDK fetches, and what it refuses

An install fetches four resources through the provider's authenticated client: the style
document, its Latin glyph ranges, its sprite index and its sprite sheet. Each address
resolves against the document's own address: absolute, root-relative to its origin, or
relative to its directory.

- A provider whose documents name resources with the vendor's URI scheme resolves them
  through that provider's API before the host check.
- The credential travels only to the document's own origin and to the hosts the provider
  declares. A document naming any other host is refused before a request, and the
  refusal names the host.
- An address the SDK cannot fetch, an unrecognized URI scheme or a path above the
  document's directory, continues with the SDK's embedded Latin font and draws no icons,
  so a base never fails over an address the SDK cannot read.
- A style with no glyph endpoint or no sprite draws labels in the embedded font and its
  icons not at all.
- A property or layer kind the renderer does not paint is reported as a diagnostic; a
  fill pattern paints the layer's fill colour.
- A document and its tileset must use the same schema. A document naming a tileset the
  provider does not serve is refused before install, naming the declared source and the
  served tileset. A source kind the vector base cannot draw is reported by name as a
  degraded asset.
- A layer reading a source layer the tiles do not carry is counted as unmatched, so the
  readback names the mismatch.

## What each target calls

| Target | Member |
|---|---|
| Rust | `MapView::set_provider_vector_tile_source_from_document` |
| Android | `MapView.setProviderVectorTileSourceFromDocument` |
| iOS | `MapView.setProviderVectorTileSourceFromDocument` |
| React Native | `setProviderVectorTileSourceFromDocument` on `MapView` |
| WebAssembly | `setProviderVectorTileSourceFromDocument` on `MapView` |

The committed styles — `bright-light`, `fiord-dark`, `positron-light`, `carto-light`,
`carto-dark` — install through `set_provider_vector_tile_source` with the provider key
and style id, without a credential.

## Next

- [Provider capabilities in Rust](provider-capabilities-rust.md) lists every provider,
  the style ids each declares, and the attribution each one requires.
- [Map layers and traffic](map-layers-and-traffic.md) shows the layers control that
  switches between these bases on every platform.
