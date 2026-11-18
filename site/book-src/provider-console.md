# See provider requests in the demo

The developer console in every reference application and in the browser demo shows the
SDK's own scrubbed event stream: one row per event, with provider request rows naming
the provider, capability, method and path, status, duration and outcome. It reads
without the browser's developer tools or a debugger. It gives you a level filter,
provider rows prefixed with the provider's display name, a floating control with a
Warning/Error dot, a detail view with every typed field and the live cache rows, tile
and traffic cache counters, an opt-in response-excerpt capture, and an **Export** action
that carries the filtered stream with no credential. The console is read-only over the
SDK's stream: it keeps only the filter and the selected row, never clears the
diagnostics ring and never issues a provider call.

## Open the console

1. Select the floating console control. On Android it sits on the start edge above the
   scale bar, on iOS above the layers control, in React Native above the attribution
   notice, and in the browser demo beside the layers control.
2. Issue a request: search for a place, plan a route, or turn on a provider tile or
   traffic layer.
3. The console lists the event within about a second of the response settling.

The console stays open over the map; close it with its close action, or press Escape in
the browser demo. A Warning or an Error that arrives while it is closed lights a dot on
the floating control, and opening the console clears it. Diagnostics are opt-in: on
Android, iOS and React Native the stream starts the first time you open the console or
the diagnostics pane, so an earlier event cannot light the dot; the browser demo starts
the stream with the page. An empty list is never blank: with no provider registered it
points to the settings surface and names the actions that issue a request; with
providers registered and no request yet it says no provider request has been made; a
filter that excludes every row says that no event matches.

## Read the stream

**Error** shows errors, **Warning** shows warnings and errors, and **Info** shows every
console level. The browser demo adds provider and outcome filters. A provider request
row reads like this:

```
MapQuest · GeocodingProvider · GET /geocoding/v1/address · 2xx · 142 ms
```

| Part | Meaning |
| --- | --- |
| Provider | The provider's display name from the provider registry. |
| Contract | The contract the request served: `GeocodingProvider`, `RoutingProvider`, `RasterTileProvider`, and so on. |
| Method and path | The HTTP method and the request path, with credential values replaced by `<redacted>`. |
| Status | The status class the response fell into: `2xx`, `3xx`, `4xx`, `5xx`, or `transport-failure` when no response arrived. |
| Duration | The time from sending the request to the response settling. |
| Response excerpt | With capture on, the first 512 bytes of the response, redacted and shown beneath the row. With capture off, nothing. |

Each provider request carries one of these outcomes:

| Outcome | What happened | What to check |
| --- | --- | --- |
| `success` | The provider returned a response the SDK accepted. | Nothing. |
| `redirect` | The provider redirected the request. | The configured base address, or the provider's own routing. |
| `client-error` | The provider refused the request. | The credential, the provider's plan tier, or the request's coordinates. |
| `server-error` | The provider failed while handling the request. | Retry; if it persists, use the row in a report to the provider. |
| `transport-failure` | The request never reached the provider. | The network, the browser's CORS policy, or the configured base URL. |
| `unclassified` | No status class was recorded. | Treat it as a transport problem and keep the row in a report. |

Selecting a row opens its detail view: the severity, subsystem target and message, and
for a provider request the provider's display name and key, the contract, the method,
the redacted path, the status class, the outcome, the duration, the response byte count,
and the response excerpt when capture is on. The view reads the typed fields; it never
parses the composed message line.

## Read the cache rows

Two read-only lines report the current session. The counters are never uploaded and are
not part of the exported stream.

| Line | What it counts |
| --- | --- |
| **Tile cache** | The installed tile payload cache on Android, iOS and React Native: `hits` answered without a provider request, `misses` that went to the provider, and `stored` payload bytes. Both the raster base and the vector base fetch through the same store. The browser demo has no disk cache; while it serves a vector base, its tile row shows that base's `payload cache hits` and `payload cache misses`. |
| **Last frame** | The base map's tiles for the last settled frame: `requested` tiles the camera listed, `drawn` tiles with imagery, `fallback` tiles covered by a cached ancestor, `deferred` tiles still loading and `unavailable` tiles with no imagery yet. It describes one frame, not the cache. |
| **Traffic cache** | The congestion overlay's cache: `hits`, `misses`, `held cells`, `evictions`, `grid zoom`, `entry budget` and `time to live`. The browser demo, Android and React Native head the line with the display name of the provider serving traffic. |

A tile-cache hit is a payload the store already held; a miss is one it had to fetch. The
last-frame row follows the base map, and the traffic counters follow the congestion
overlay.

The traffic counters mean:

| Counter | Meaning |
| --- | --- |
| `hits` | A refresh answered from memory. The provider was not called. |
| `misses` | A refresh that needed ground the cache did not hold fresh, so the provider was called. |
| `held cells` | The grid cells the cache holds now. |
| `evictions` | Cells dropped to stay inside the budget. The least recently used cell goes first. |
| `grid zoom` | The zoom level of the grid that a cell covers. A higher zoom means smaller cells. |
| `entry budget` | The maximum number of cells the cache keeps. |
| `time to live` | How long a held cell counts as fresh. |

A first traffic request is a miss, the expected cost of filling the cache. A short pan
stays inside a held cell, so the next refresh is a hit; evictions rising during a long
drive is normal because the cache is bounded. A miss rate that stays high while the
camera sits still means the cache is not being reused. While no traffic provider is
registered, the traffic line says so and shows no counters; while no tile cache is
installed, the tile cache line says so; until the first frame draws, the last-frame row
says that no frame has settled.

## Capture and export the stream

Response bodies are opt-in because they can contain provider data you did not type. In
the browser demo and the iOS reference application, select **Capture response
excerpts**. On Android and React Native, turn the same process-wide setting on through
the SDK (`setResponseExcerptCapture(true)`), then issue the request again. The captured
prefix appears beneath the row and in the detail view, capped at 512 bytes and cut on a
character boundary, with credential-bearing JSON members and `name=value` assignments
replaced by `***`. The setting is off whenever the application or page starts; turning
it off stops new captures but keeps captured rows.

**Export** carries the filtered stream with no credential by construction: the stream is
scrubbed before the console reads it, paths show parameter names with the values
replaced, and a captured excerpt has credential-bearing members and assignments
replaced. The browser demo downloads the filtered rows as a JSON file, with each
provider row carrying its typed request object beside the event envelope. Android, iOS
and React Native hand the filtered list to the platform share sheet as plain text,
capped at 256 KB with a notice when it truncates. The response excerpt is included only
while capture is on; the cache lines are live reads and are not part of any export.

To report a failure, issue the action that failed, filter to the level and, in the
browser demo, the provider and outcome you need, select **Export**, and attach it to
your report with the action you took. The export contains only the console's own fields:
no credential and no request header. With capture on it carries the redacted prefix, so
review the excerpt before you share it. A Rust application reads the same scrubbed
stream through the diagnostics listener and renders its own view; the SDK ships no Rust
console interface.

## Next

- [Provider capabilities in the browser](provider-capabilities-wasm.md)
- [Mix providers per capability](mix-providers-per-capability.md)
- [FAQ and troubleshooting](faq.md)
