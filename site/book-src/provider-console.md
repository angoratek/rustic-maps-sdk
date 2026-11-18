# See provider requests in the demo

The developer console in every reference application and in the browser demo
shows the SDK's own scrubbed event stream: one row per event in that stream,
with the provider request rows naming which provider served a call, which
capability it asked for, the method and path, the status, the duration and the
outcome. Use it to tell a provider problem from a map problem without opening
the browser's developer tools or attaching a debugger.

## What you get

- One row per event, newest first, filtered by the Error, Warning and Info
  levels. Info shows every console level.
- Provider request rows prefixed with the provider's display name, so a search
  that failed against one provider reads as that provider's failure rather than
  a blank map.
- A floating control with a Warning/Error dot that reports a severe event
  arriving while the console is closed.
- A detail view for every row: the event's severity, target and message, and
  for a provider request every typed field plus the live cache rows.
- Cache counters for the base map's tiles and for the traffic overlay's cache,
  so a repeated request can be read as a cache hit rather than a new network
  call.
- An opt-in response-excerpt capture switch where the console offers one, and
  an **Export** action that carries the filtered stream with no credential.
- A console that is read-only over the SDK's own stream: it keeps only the
  filter and the selected row, never clears the SDK's diagnostics ring, and
  never issues a provider call.

## Open the console

1. Select the floating console control in the map's control area. On Android
   it sits on the start edge above the scale bar, on iOS it sits above the
   layers control, in React Native it floats above the attribution notice,
   and in the browser demo it sits beside the layers control.
2. Issue a request: search for a place, plan a route, or turn on a provider
   tile or traffic layer.
3. The console lists the event within about a second of the response settling.

The console stays open over the map. Close it with its close action, or press
Escape in the browser demo.

### The Warning/Error dot

While the console is closed, a Warning or an Error that reaches the console's
stream lights a dot on the floating control. The dot takes the more severe
color when both arrive, and opening the console clears it.

The dot can report only the events the stream has seen. Diagnostics are opt-in:
on Android, iOS and React Native the stream starts the first time you open the
console or the diagnostics pane, so an event raised before that first open
cannot light the dot. The browser demo starts the same stream with the page.

## Read the stream

A level filter sits at the top: **Error** shows errors, **Warning** shows
warnings and errors, and **Info** shows every console level. The browser demo
adds filters for provider and outcome; the native consoles keep the level
filter alone.

A non-provider row shows the severity, the subsystem target and the scrubbed
message. A provider request row shows:

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

### Outcomes

| Outcome | What happened | What to check |
| --- | --- | --- |
| `success` | The provider returned a response the SDK accepted. | Nothing. |
| `redirect` | The provider redirected the request. | The configured base address, or the provider's own routing. |
| `client-error` | The provider refused the request. | The credential, the provider's plan tier, or the request's coordinates. |
| `server-error` | The provider failed while handling the request. | Retry; if it persists, use the row in a report to the provider. |
| `transport-failure` | The request never reached the provider. | The network, the browser's CORS policy, or the configured base URL. |
| `unclassified` | No status class was recorded. | Treat it as a transport problem and keep the row in a report. |

## Select a row

Selecting a row opens its detail view:

- the severity, the subsystem target and the message line, whatever the row
  is;
- for a provider request, the provider's display name and key, the contract,
  the method, the redacted path, the status class, the outcome, the duration,
  the response byte count and the response excerpt when capture is on;
- the live cache rows described below.

The detail view reads the typed fields the SDK delivered. It never parses the
composed message line to recover a provider fact.

## Read the cache rows

Two read-only lines report the current session. They answer one question
without opening the developer tools: is the SDK asking the network again for
something it already holds? The counters are never uploaded and are not part
of the exported stream.

| Line | What it counts |
| --- | --- |
| **Tile cache** | The installed tile payload cache on Android, iOS and React Native: `hits` answered without a provider request, `misses` that went to the provider, and `stored` payload bytes. Both the raster base and the vector base fetch through the same store, so the counters cover every payload lookup. The browser demo has no disk cache; while it serves a vector base, its tile row shows that base's `payload cache hits` and `payload cache misses` instead. |
| **Last frame** | The base map's tiles for the last settled frame: `requested` tiles the camera listed, `drawn` tiles with imagery, `fallback` tiles covered by a cached ancestor, `deferred` tiles still loading and `unavailable` tiles with no imagery yet. It describes one frame, not the cache, so it stays a separate row. |
| **Traffic cache** | The congestion overlay's cache: `hits`, `misses`, `held cells`, `evictions`, `grid zoom`, `entry budget` and `time to live`. The browser demo, Android and React Native head the line with the display name of the provider serving traffic; the serving provider always comes from the same registry the settings surface uses. |

The rows describe different layers. Tile cache counters follow the installed
payload store: a hit is a payload the store already held, and a miss is one it
had to fetch. The last-frame row follows the base map: a drawn tile is one the
SDK had in memory or fetched, and a missing tile is one it asked for but could
not draw this frame. Traffic counters follow the congestion overlay, which asks
its provider for the ground the camera shows each time the map settles.

### What the traffic counters mean

| Counter | Meaning |
| --- | --- |
| `hits` | A refresh answered from memory. The provider was not called. |
| `misses` | A refresh that needed ground the cache did not hold fresh, so the provider was called. |
| `held cells` | The grid cells the cache holds now. |
| `evictions` | Cells dropped to stay inside the budget. The least recently used cell goes first. |
| `grid zoom` | The zoom level of the grid that a cell covers. A higher zoom means smaller cells. |
| `entry budget` | The maximum number of cells the cache keeps. |
| `time to live` | How long a held cell counts as fresh. |

A first traffic request is a miss; that is the expected cost of filling the
cache. A short pan stays inside a held cell, so the next refresh is a hit and
no request leaves the device. Evictions rising during a long drive is normal:
the cache is bounded on purpose, so it forgets the oldest ground instead of
growing without limit. A miss rate that stays high while the camera sits still
means the cache is not being reused, and is worth including in a failure
report.

To watch a hit:

1. Open the console.
2. Turn on the traffic layer, then read the counters: one miss, no hits.
3. Pan the map a short distance and wait for the overlay to refresh.
4. The hit count rises and the miss count stays where it was.

While no traffic provider is registered, the traffic line says that no traffic
provider is registered. It shows no counters rather than zeros, because no
cache exists yet. On Android, iOS and React Native, while no tile cache is
installed the tile cache line says so instead of showing zeros. Until the first
frame draws, the last-frame row says that no frame has settled.

## Capture a response excerpt

Response bodies are opt-in because they can contain provider data you did not
type. In the browser demo and the iOS reference application, select **Capture
response excerpts** in the console. On Android and React Native, the
application turns the same process-wide setting on through the SDK
(`setResponseExcerptCapture(true)` on the SDK object), then the next request is
captured. In every case, issue the request again after enabling capture.

The captured prefix appears beneath the row and in the detail view. It is
capped at 512 bytes, cut on a character boundary, and credential-bearing JSON
members and `name=value` assignments are replaced with `***`. The setting is
off whenever the application or page starts; turning it off stops new captures
but does not erase rows already captured.

## Export the stream

**Export** carries the filtered stream with no credential by construction: the
stream is scrubbed before the console reads it, paths show parameter names with
the values replaced, and a captured excerpt has credential-bearing members and
assignments replaced.

- In the browser demo, Export downloads the filtered rows as a JSON file. A
  provider row carries its typed request object beside the event envelope.
- On Android, iOS and React Native, Export hands the filtered list to the
  platform share sheet as plain text. The text is capped at 256 KB and says so
  when it truncates.

The response excerpt is included only while capture is on. The cache lines are
live reads of the map and are not part of any export.

## Empty states

An empty list is never blank:

- With no provider registered, the console says no provider is registered and
  points to the settings surface, then names the actions that issue a request.
- With providers registered and no request yet, it says no provider request has
  been made and names the actions that issue one.
- A filter that excludes every row says that no event matches the selected
  filters.

## Report a provider failure

1. Open the console and issue the action that failed.
2. Filter to the level and, in the browser demo, the provider and outcome you
   need.
3. Select **Export**.
4. Attach the file or the shared text to your report, and name the action you
   took.

The export contains only the console's own fields: no credential and no request
header. With capture off it carries no response body; with capture on it carries
the redacted prefix, so review the excerpt before you share it if the provider's
answer contains data you would rather not pass on.

## Where the console ships

| Target | Open it | Export |
| --- | --- | --- |
| Android | The developer console control in the map control area, on the start edge above the scale bar. | The filtered list through the platform share sheet. |
| iOS | The developer console control above the layers control. | The filtered list through the platform share sheet. |
| React Native | The developer console control floating above the attribution notice. | The filtered list through the platform share sheet. |
| WebAssembly | The developer console control beside the layers control in the browser demo. | The filtered rows as a JSON file download. |

A Rust application reads the same scrubbed stream through the diagnostics
listener and renders its own view; the SDK ships no Rust console interface.

## Next

- [Provider capabilities in the browser](provider-capabilities-wasm.md)
- [Mix providers per capability](mix-providers-per-capability.md)
- [FAQ and troubleshooting](faq.md)
