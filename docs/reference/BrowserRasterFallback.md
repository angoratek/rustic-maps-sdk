# The keyless raster fallback and the configured provider

**Reader outcome.** You will know why the demo once requested OpenStreetMap
tiles in a session with a MapQuest key, what the fallback is allowed to do,
and how the browser target keeps a configured provider authoritative.

## The failure

A session with a provider credential could not draw live raster tiles after
switching from a vector base back to the raster base. The console showed
cross-origin failures for `tile.openstreetmap.org` and no imagery outside the
recorded fixtures.

Two independent faults combined.

1. **Two writers, one slot.** The page registers the keyless OpenStreetMap
   `{z}/{x}/{y}` source during renderer start-up, through
   `registerUrlTemplateRasterTileSource`. Applying a credential calls
   `setProviderCredential`, which installs MapQuest for the same
   `RasterTiles` registry slot. Both calls are asynchronous page code; the
   remembered-key restore runs before the renderer's own registration, so the
   fallback lands last and replaces MapQuest. The canvas then fetches every
   unrecorded address from OpenStreetMap.
2. **A forbidden request header.** `UrlTemplateRasterTileProvider` attaches
   `User-Agent` when its source declares one, which is correct for native
   hosts. The browser transport forwarded every header to `fetch`, so the
   cross-origin tile request asked for a header the tile host does not allow;
   its preflight failed and every tile with it.

The MapQuest provider itself was never at fault.

## The rule

A live `{z}/{x}/{y}` source registered by a page is a **fallback**. It serves
only while nothing else serves the raster-tile contract. A configured
provider always outranks it, whatever order the two registrations happen in.

## Design

### Registry

The browser provider state keeps the fallback beside the per-capability
registry, not inside its `RasterTiles` slot:

- `registerUrlTemplateRasterTileSource` validates and stores the fallback
  only. It never replaces a configured raster provider.
- The canvas resolves the raster provider as: the registry's configured
  `RasterTiles` provider first; the fallback second; the recorded-fixture
  provider for the demo area, as today.
- `clearProviderRegistrations` clears the configured providers and leaves
  the fallback in place, so a cleared page keeps drawing live tiles. A
  dedicated `clearUrlTemplateRasterTileSource` removes the fallback when a
  page wants no cross-origin traffic at all.
- Registering the fallback again replaces the previous fallback, so a page
  keeps one.

The wasm façade (`platform/web/main.mjs`) carries the new clear call;
`contract_sync.mjs` and the platform parity gate see it as an addition. The
frozen UDL is unchanged.

### Transport

The browser transport filters the Fetch specification's forbidden request
header names before building the request — at least `user-agent`, `host`,
`content-length`, `connection`, `cookie`, `origin`, `referer`,
`accept-encoding` and anything beginning with `sec-`. The native transport
is unchanged and continues to send the provider's declared user agent, which
OpenStreetMap's tile policy expects from a server.

### Tile failures and retries

A tile request that answers 404 or 410 is definitive: the address has no
recorded tile and no live source serves it, so the canvas stops asking. A
transport or server failure is not definitive: the canvas retries the address
after 2 seconds, doubling the wait to a 30-second cap across consecutive
failures, so a temporary outage recovers without a reload. The three memories
behind these rules (unavailable addresses, staged payloads and retry
schedules) are bounded, so a long pan neither grows them without limit nor
blacklists a tile for the session.

Every browser request also carries a 30-second deadline. A stalled connection
is aborted instead of holding one of the in-flight request slots.

### Demo

- The page stops passing `userAgent`; a browser supplies its own.
- The fallback is registered once, unconditionally, and now cannot displace a
  configured provider.
- The key restore and the renderer start-up may run in either order; the
  resolution rule makes the outcome the same.

## Verification

- A keyed session makes zero requests to the fallback host and all raster
  requests to the configured provider.
- A keyless session outside the recorded fixtures still draws live tiles.
- Clearing registrations returns to the fallback without a reload.
- A browser request for a cross-origin tile carries no forbidden header.
- Native provider tests still observe the declared user agent.
