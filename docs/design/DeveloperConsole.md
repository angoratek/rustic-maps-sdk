# Developer console on every target

Date: 2026-11-01. Status: landed on Android, iOS, React Native and the web
demo. The provider-request observation seam, the `LogEvent` trace field and
the console surface ship. Owner direction: there is no
way to read logs or debug from a reference app — at least not in the iOS app —
so every reference app and demo should carry a developer console that shows
the key events: which provider serves a call, the provider requests and
responses (truncated, with a way to see more and to download), cache use
(tile cache, routing cache), warnings and errors, at Error, Warning and Info
levels.

## What already exists

The SDK already owns a structured log stream, delivered to foreign code:

| Surface | Shape |
|---|---|
| `MapSdk.subscribe_to_diagnostic_events(listener, minimum_severity)` | Fire-and-forget delivery to a `SdkDiagnosticEventListener` |
| `MapSdk.flush_recent_diagnostic_events(listener, minimum_severity, maximum_count)` | Reads back the recent buffer |
| `Diagnostics` facade | `add_listener`, `flush_recent` over the same objects |
| `LogEvent` | `timestamp_milliseconds_since_epoch`, `severity`, `subsystem_target`, `message_line` |
| `SdkLogSeverity` | `Trace`, `Debug`, `Info`, `Warn`, `Error` |

Events are already scrubbed: coordinates are rounded and API keys never
appear. Each console reads the same stream: the web demo, Android, iOS and
React Native render it behind the diagnostics control.

The observation seam behind the provider trace also ships:
`crates/rustic-maps-core/src/provider_observation.rs` builds a redacted
`ProviderRequestObservation` per attempt (path with query values removed except
the documented shape parameters, status class, duration, byte count, a
512-byte snippet on a character boundary), and `ObservedTransport` wraps a
provider client's transport once so retries are observed per attempt. The
MapQuest client and the five vendor clients report through it today; OSRM,
Valhalla, Photon and the URL-template sources have no SDK-attached observer
yet. The binding installs the observer and bridges it into the diagnostics
ring, `LogEvent.provider_request_trace` carries the typed record, and every
console surface renders it.

## What the console shows

One scrollable list, newest first, one line per event:
`hh:mm:ss · level · subsystem · message`. A level filter (Error, Warning,
Info; Info shows all three) sits at the top, next to a search field. Tapping a
line opens a detail sheet with the full message and, for a provider trace, the
structured fields below. A footer row shows the cache summary. Export writes
the filtered list to a plain-text file and opens the platform's share sheet;
the file is capped at 256 KB and says so when it truncates.

### Provider request and response trace

A new event family, emitted by the provider layer's observer so every provider gets it
without naming a vendor. The observation record already carries these fields;
the contract addition below is what makes them visible to an application:

| Field | Content |
|---|---|
| `provider_contract` | `RoutingProvider`, `GeocodingProvider`, … |
| `provider_key` | the registry key, or `application` for a foreign implementation |
| `http_method` | the method |
| `request_path` | the path only; query values are dropped except documented non-secret parameters |
| `status_class` | `2xx`, `4xx`, `5xx`, or `transport-failure` |
| `duration_milliseconds` | elapsed |
| `response_bytes` | payload size |
| `response_snippet` | the first 512 bytes, truncated at a character boundary |

Never included: the credential, the credential label's value, authorization
headers, and any query parameter the transport classifies as secret. The
redaction rule is the same one the failure diagnostics already use and is
tested by a transport test that feeds a credential through a request and
asserts the emitted event contains neither the credential nor the header.

Cache events join the same stream as `Info`: a tile-cache hit, a tile-cache
miss that went to the network, a routing-cache hit and a routing-cache miss,
each with the cache kind and the entry key. The console's footer aggregates
the counts per session.

## Contract additions

Addition-only:

```
dictionary ProviderRequestTrace {
    string provider_contract;
    string provider_key;
    string http_method;
    string request_path;
    string status_class;
    u64 duration_milliseconds;
    u64 response_bytes;
    string response_snippet;
};

// on LogEvent, appended and null for non-provider events
ProviderRequestTrace? provider_request_trace;
```

`LogEvent` gaining an optional field is addition-only for every generated
binding. Emission is gated by the subscription's minimum severity: traces are
`Info`, failures stay `Warn`/`Error`.

### How the transport learns which provider a request belongs to

The transport sees a URL and a method, not the contract or the registry key,
so `HttpRequest` gains two fields the provider clients fill in when they build
the request: `provider_contract` and `provider_key`. Each provider already
has one request-building site per API, so the change is one line per site and
no provider gains a vendor special case. The observer is one process-wide sink
installed by the native binding (and by the wasm façade), which converts an
observation into a `LogEvent` carrying the trace and appends it to the same
ring the console reads. When no sink is installed — unit tests, a bare
provider crate — the transport skips the observation with no allocation.

Redaction happens in the transport before the observation is built: query
values are dropped except an allow-list of non-secret parameters
(`format`, `overview`, `geometries`, `units`), the path keeps its structure,
and the snippet excludes anything the transport classifies as secret. A
credential supplied as a query parameter never appears in `request_path` or
the snippet because the value is removed while the URL is split, not filtered
afterwards.

## The console surface

A small floating button (a terminal glyph) above the attribution notice, on
the opposite edge from the map layer control, with the identifier
`developer-console-control`. It opens a full-height sheet on a phone and a
side panel on a wide viewport, with the identifier `developer-console`. The
button carries a dot when a Warning or Error arrived since the console was
last opened.

The console is present in every reference app and in the browser demo; it is
read-only over the SDK's own stream, keeps no state of its own beyond the
filter, and never issues a provider call. It lives behind the existing
readiness/diagnostics area rather than the settings sheet so that it is
reachable in one tap from the map.

## Retention and mobile constraints

- The SDK keeps the most recent 512 events per subsystem in its diagnostics
  rings. The console reads through `flush_recent` and keeps no second copy.
- A line renders at most three visual lines before truncation; the detail
  sheet shows the rest.
- The trace snippet is truncated at 512 bytes at emission, so a large raster
  payload never enters the buffer.
- Export streams the filtered list to a temporary file, capped at 256 KB, and
  the share sheet owns the file afterwards. Nothing is written to a
  credential-bearing location, and the file is deleted on next launch.

## Tests

- Rust: a transport request through a fake server emits one trace with the
  path, status class, duration and a snippet truncated at 512 bytes; a
  credential-bearing request emits neither the value nor the header; a
  non-provider event carries no trace; each ring caps at 512 events.
- Browser: the console opens from its button, filters by level, opens a
  detail sheet with the trace fields, exports and asserts the download name
  and that the file omits a credential passed by the test.
- Android, iOS and React Native: the console opens, renders Info events from
  an injected fake listener, and the export action invokes the share sheet
  with a file whose content matches the filtered list.

## Provider request console

Beyond the log list, every target carries a provider request console that
streams one row per provider request. Each row names the provider, the
capability, the method, the redacted path, the status, the duration and a
bounded response excerpt. Rows filter by provider, capability and outcome, copy
as text and export as JSON. The panel reads the same observation stream; it is
a second subscriber beside the diagnostics pane and adds nothing to the request
path. The provider list comes from the registry, never a hardcoded list.

The event shape is the typed `ProviderRequestTrace` on `LogEvent` (above)
plus one optional upstream field. `everymap-core` gains an optional
`response_excerpt` on the provider observation: the first N bytes of the body,
captured only when the host enables it, with the same redaction applied to
query values and known token fields. Each surface forwards it into the typed
trace, so every provider gets one excerpt and one redaction rule. The excerpt
is capped by a named constant, truncated on a character boundary and never
includes a credential.

- **Excerpt switch.** Response excerpts are opt-in per session; the switch
  states that excerpts may contain provider content and are never uploaded.
- **Row and filters.** Newest first, bounded to a named maximum. A row shows
  the captured excerpt beneath the request line. The provider list comes from
  the registry.
- **Copy and export.** Copy writes the visible rows as text; export downloads
  them as JSON. Both contain no credential by construction.
- **Empty state.** Says that no provider request has been made yet and names
  the action that issues one.
- **Redaction.** Path redaction is the provider layer's own; the excerpt
  reuses the same token vocabulary (`key`, `apiKey`, `access_token`, `session`,
  `signature`). A credential carried in a header never exposes the header.

Tests: a unit test feeds observations, including a refused request, and
asserts the row fields and filters; a redaction test proves a
credential-bearing path and a token-shaped excerpt are removed; a browser case
issues one request per capability against fixtures and asserts the rows name
the serving provider and the outcome; an export test parses the JSON and
asserts no credential is present.
