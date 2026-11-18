# Draw a provider's own map style

A provider that publishes a vector style document draws its tiles the way its
own cartographers designed them. This page installs that style as your base
map, so roads, water, place names and icons match the tiles exactly, and
explains what the SDK fetches, what it refuses, and what it keeps on the
device.

## What you get

- A base map painted with the style document the provider publishes, not a
  style that ships with the SDK.
- Place names drawn in the style's own font and icons drawn from its own
  sprite sheet.
- Route references drawn as shaped badges — a shield or rounded rectangle in
  the style's own colours, with the reference centred inside — when the style
  declares a shield block for its route-reference layer. A style whose tileset
  carries no route-reference layer declares no badge and paints no route
  reference.
- A status readback that reports the style id, the tileset name, the style's
  attribution line, every property the renderer could not apply, every asset
  that fell back, and how many of each source's layers matched and painted.
- The fetched style document, glyph ranges and sprite sheet kept in the
  installed tile cache for up to thirty days, so a reopened application paints
  without requesting them again. Without a tile cache they are held for the
  session.

## What you need

- A vector-tile provider whose registry entry declares a style document
  (`document_url`). HERE and Mapbox do today, under the style ids `omv-light`
  and `streets-light`.
- That provider's credential applied at runtime. The SDK never ships a key.
- A target that carries the provider: Android, iOS, React Native and the
  browser all do, and a Rust application reaches the same member.

## Steps

1. **Configure the provider with your credential.** Use the provider's own
   registration path, exactly as you would for its geocoding or routing
   contracts. The credential lives in the provider client from this point on.

   ```kotlin
   import uniffi.rustic_maps.MapSdk
   import uniffi.rustic_maps.ProviderContract

   val mapSdk = MapSdk()
   mapSdk.configureProvider(ProviderContract.VectorTiles, "here", providerApiKey)
   ```

2. **Install the style by provider key and style id.** The address is the
   provider's published document, as the registry lists it; it carries no
   credential, because the provider's client appends it.

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

3. **Read the status back.** `vectorTileSourceStatus()` reports the style id
   you installed, the tileset's name and the attribution line the style
   requires. Show that line wherever the map is shown. The same readback
   reports the unsupported-property count with a sorted, bounded list of
   names, the assets that fell back to a degraded draw, and each source's
   matched and drawn layer counts for the last resolved frame, so your UI can
   tell "still loading" from "this style asks for something the renderer does
   not paint".

4. **Reopen the application.** The same family installs again without a
   request while the kept resources are younger than thirty days and the
   device cache is installed.

## What the SDK fetches, and what it refuses

An install fetches four resources through the provider's authenticated
client: the style document, its Latin glyph ranges, its sprite index and its
sprite sheet. Each address resolves against the document's own address — an
absolute address, a path root-relative to the document's origin, or a path
relative to the document's directory.

- A provider whose published documents name their resources with the vendor's
  own URI scheme resolves them through that provider's API before the host
  check, so a document that declares its sprite and glyph endpoints this way
  still draws its icons and labels.
- The credential travels only to the document's own origin and to the hosts
  the provider declares for its published resources. A style document naming
  any other host is refused before a request is made, and the refusal names
  the host.
- An address the SDK cannot fetch — an unrecognized URI scheme, or a path that
  climbs above the document's directory — is not a credential risk. The
  install continues with the SDK's embedded Latin font and draws no icons, so
  a base never fails over an address the SDK cannot read.
- A style with no glyph endpoint or no sprite draws its labels in the embedded
  font and its icons not at all.
- The document's own labels, icons and line styling are applied as it declares
  them. A property or layer kind the renderer does not paint is reported as a
  diagnostic rather than dropped in silence; a fill pattern paints the
  layer's fill colour.
- A style document and the tileset it is installed over must use the same
  schema. A document that declares a vector source naming a tileset the
  provider does not serve is refused before it is installed, with a message
  that names the declared source and the served tileset, instead of painting
  fragments. A source of a kind the vector base cannot draw is reported by
  name as a degraded asset.
- A layer reading a source layer the tiles do not carry is counted as
  unmatched in `vectorTileSourceStatus()`, so the readback names the mismatch
  rather than leaving the fragment unexplained.

## What each target calls

| Target | Member |
|---|---|
| Rust | `MapView::set_provider_vector_tile_source_from_document` |
| Android | `MapView.setProviderVectorTileSourceFromDocument` |
| iOS | `MapView.setProviderVectorTileSourceFromDocument` |
| React Native | `setProviderVectorTileSourceFromDocument` on `MapView` |
| WebAssembly | `setProviderVectorTileSourceFromDocument` on `MapView` |

The committed styles — `bright-light`, `fiord-dark`, `positron-light`,
`carto-light`, `carto-dark` — install through the sibling member without a
credential: `set_provider_vector_tile_source` with the provider key and style
id.

## Next

- [Provider capabilities in Rust](provider-capabilities-rust.md) lists every
  provider, the style ids each declares, and the attribution each one
  requires.
- [Map layers and traffic](map-layers-and-traffic.md) shows the layers control
  that switches between these bases on every platform.
