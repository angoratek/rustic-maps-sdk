# Changelog

The SDK is pre-1.0. No versioned release is published yet; the entries below
describe the capabilities the current source tree carries, and the
[downloads page](downloads.md) lists the scripts that build each platform
artifact. The FFI contract is **addition-only**: existing members keep their
signature and meaning across an upgrade. The [versions and
support](versions-and-support.md) page explains the numbering and the release
tracks.

## Vector base maps and labels

- Install any `{z}/{x}/{y}` vector tileset as a styled base map. Five named
  styles ship: Bright and Positron (light), Fiord (dark) for
  OpenMapTiles-schema tilesets, and Carto Light and Carto Dark for the
  OpenStreetMap US Sourdough tileset. A style can be switched without
  refetching a tile: decoded geometry is restyled from the tile cache.
- A styled base replaces the raster tiles while installed and draws beneath
  every overlay. The native surface paints the base's background immediately
  and fills in tiles as they arrive; the browser resolves a frame's tiles
  concurrently.
- Place names and road names draw from an SDF glyph atlas on every surface.
  Point-placed names sit at their feature; road names rotate along their own
  lines. Labels are filtered by feature rank and class, collision-rejected and
  capped per frame.
- A route reference draws as a shaped badge where the style declares a shield
  block for its route-reference layer. The committed Carto styles carry that
  layer; Bright, Fiord and Positron do not.
- A status readback reports the configured source and style, the drawn and
  failed tile counts of the last resolved frame, and the last tile failure
  detail.

## Positioning

- The Rust `map-positioning` crate validates device fixes, ignores stationary
  repeats when averaging course, smooths the course around the circle, tracks
  staleness and records source failures.
- A simulated position source walks a route with course and accuracy for
  tests, demos and desktop applications.
- The controller and its fix and source-error records are part of the SDK
  contract, so Kotlin, Swift and TypeScript applications construct the same
  controller and read the same smoothed course. The browser demo uses it with
  the Geolocation API and reports denied, unavailable and timed-out sources
  honestly.

## MapView and the reference applications

- One `MapView` per platform owns the map surface, frame pump, lifecycle,
  gestures, camera, overlays, location indicator and listener callbacks.
- Frames are explicit: your display link, choreographer or render pump calls
  `draw` with a `MapCamera`. `draw` returns the frame's tile and overlay
  statistics, and `isFrameSettled` lets the pump skip an unchanged frame.
- Overlays are addressed by an identifier the view returns: add, update,
  remove, replace the whole list, or clear it. A rejected descriptor keeps the
  previous list. Overlay geometry is tessellated once.
- The camera is an immutable value with fit-bounds framing, projection and
  unprojection, and pan, zoom and rotate gestures that hold the point under
  your finger.
- Android (Compose), iOS (SwiftUI), React Native and the browser reference
  applications implement one shared design language: a bottom-sheet home, a
  five-control cluster, a follow camera, an offline tile-cache card, a
  readiness-chip diagnostics readout, and a Light / Dark / Auto theme whose
  Auto setting follows the operating system.

## Navigation and simulated trips

- Navigation state is deterministic: the same sequence of location updates
  produces the same phase, step and instruction. A trip moves through
  `Navigating`, `OffRoute` and `Arrived`.
- Step advance supports manual, distance-to-the-end-of-the-step and
  distance-from-the-step policies. A fix whose reported accuracy is worse than
  the policy's minimum never advances.
- Deviation is graded. A fully-off-route fix freezes progress and asks for a
  reroute; an off-step-but-on-route fix keeps counting down.
- Instruction text is the provider's own, never rephrased. The payload carries
  the active maneuver, the next maneuver, a unit-aware distance phrase and the
  roundabout exit index where one applies.
- Speech triggers use documented distance bands: each band announces at most
  once per step, with a minimum spacing and repeat distance to suppress
  chatter. The platform owns playback, and every utterance carries a stable,
  monotonic identifier.
- Reroute requests pass through a cooldown and movement threshold, with an
  in-flight gate and a readback. The defaults are a five-second cooldown and a
  fifty-meter threshold.
- `RouteSimulator` interpolates a trip along the route's polyline at a pace
  you choose and marks arrival exactly at the final coordinate.

## Search, routing and traffic

- `MapSdk` resolves every provider-backed call through one registry: geocoding,
  search prediction, full search, routing with ordered waypoints, the distance
  matrix, traffic incidents, traffic flow and raster tiles. A call into an
  unregistered contract throws `ProviderNotConfigured` with the contract name.
- Route options cover automobile, bicycle and pedestrian modes; fastest and
  shortest strategies; metric and imperial units; avoids for toll roads,
  limited-access highways, ferries and unpaved roads; and a departure time.
- A route carries distance, duration, its bounding box and one leg per waypoint
  pair, with maneuver steps inside each leg.
- `SearchEngine`, `RoutingEngine`, `OfflineManager` and `Diagnostics` expose
  the same members as thin facades.
- MapQuest implements the platform API set from one key with a token-bucket
  rate limiter, bounded retries for idempotent calls, and a status-to-error
  mapping.
- A URL-template provider serves any raster or vector tileset that publishes a
  `{z}/{x}/{y}` template, so a base map can come from a platform provider, an
  OpenStreetMap-based service or your own server.

## Offline maps

- A deterministic compiler turns an OpenStreetMap PBF extract into a
  self-describing `RMOG` routing archive: nodes, a per-edge pack with road
  class, speed and access columns, and turn restrictions.
- A validating reader opens the archive from memory-mapped storage or from
  in-memory bytes; the in-memory path runs in the browser.
- An offline routing provider answers routes with A\* over the graph, honors
  recorded turn restrictions, derives maneuvers from the bearing change, and
  stays honest where the archive carries no street names.
- An archive that fails validation throws `OfflineRoutingFailure`. The
  WebAssembly surface loads the compiled archive from in-memory bytes and
  answers the same route records and refusals.
- The disk response cache keeps raster imagery available when the network is
  not. The reference applications include an offline card that plans a route
  from the bundled archive.

## Camera and rendering

- The SDK draws maps with its own minimal `wgpu` renderer: web-mercator
  projection, exact frustum tile listing across the antimeridian, and raster
  tiles from a byte-bounded GPU texture cache.
- The tile scheduler deduplicates requests within a frame, falls back to the
  nearest available parent tile, and respects a per-frame fetch budget. A tile
  that fails to decode is skipped and evicted so a later frame can refetch it.
- Route, polygon, marker, pin and traffic-flow overlays draw over the tiles,
  and the location indicator draws above every overlay. High-density surfaces
  scale labels, line widths and marker radii by the device pixel ratio.
- The same renderer draws into live Android and iOS surfaces and on a WebGPU
  canvas. Platform recognizers forward screen deltas, and the SDK applies pan,
  focal-point zoom and focal-point rotation through the camera's ranges.

## Imagery, providers and caching

- Raster tiles come from MapQuest or any `{z}/{x}/{y}` raster source, in
  standard or retina resolution. A source is validated when registered, so a
  malformed template or inverted zoom range refuses immediately.
- A SQLite-backed response cache holds provider responses under an absolute
  byte budget, a share of free disk space, or both, with per-category
  time-to-live, LRU eviction, and cache-then-network behavior.
- Raster tiles cache for 30 days by default, the common ceiling under imagery
  provider terms. Each provider declares its own ceiling; a request above it is
  accepted and clamped, and the effective value, the ceiling and the default
  are readable.
- Ten provider contracts cover geocoding, routing, distance matrix, search,
  traffic incidents, traffic flow, elevation, raster tiles, offline routing and
  vector tiles. A provider is selected by registration, not by a call change.

## Provider credentials

- The SDK needs no provider key at build time. The reference applications start
  keyless: a URL-template base map draws without a credential, and provider
  calls answer once a credential is applied.
- Credentials are entered at runtime from a settings surface whose rows come
  from one shared provider registry. A key stays in memory unless the user opts
  in to the device's secure storage.
- Automated UI runs can inject a credential at launch; it is never persisted or
  logged.
- Registering a provider set again replaces every slot. Registering a
  URL-template base map also drops the installed tile cache, because its terms
  belong to the provider that installed it.

## Errors and diagnostics

- Every failure crosses as one `SdkError` variant; a variant that carries a
  detail string carries it verbatim. The reference applications render the same
  sentence for the same failure across platforms.
- A diagnostics layer keeps bounded per-subsystem rings. Coordinates are
  rounded before an event reaches your listener, and only events at or above
  the subscribed severity are delivered.
- A flush is bounded, synchronous and non-destructive, so the same event can be
  read again. A bounded per-object ring records failed calls to a
  host-implemented provider with the contract, method, time and verbatim
  failure text.

## Packaging and the live demo

- A tag-driven release pipeline builds and publishes the Android AARs, the iOS
  XCFramework and Swift package manifest, the React Native and WebAssembly npm
  tarballs, and the bare WebAssembly archive. Each release carries per-asset
  SHA-256 checksums, an aggregate manifest, Sigstore bundles, the notices and
  the license texts.
- The same WebAssembly build powers a keyless live demo with pan, zoom,
  bearing and pitch controls, recorded route planning, a simulated trip and
  offline routing, and a provider panel that accepts a runtime key. A hosted
  mobile demo runs the Android, iOS and React Native reference applications.

## Attribution

Each provider descriptor carries its own attribution line and terms note, and
the reference applications render the active provider's line from the shared
registry. Keep it visible in your application. Third-party dependency notices
are regenerated into [Third-party notices](third-party-notices.md).
