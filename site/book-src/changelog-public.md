# Changelog

The SDK is pre-1.0. No versioned release is published yet; the entries below
describe the capabilities the current source tree carries, and the
[downloads page](downloads.md) lists the scripts that build each platform
artifact. They are grouped by area, with the most recently completed work
first.

The FFI contract is **addition-only**: existing members keep their signature
and meaning across an upgrade, so a release adds to the surface rather than
rewriting your calls. The [versions and support](versions-and-support.md) page
explains the numbering and the release tracks.

## Vector base maps and labels

- Install any `{z}/{x}/{y}` vector tileset as a styled base map. Five named
  styles ship: Bright and Positron (light), Fiord (dark) for
  OpenMapTiles-schema tilesets, and Carto Light and Carto Dark for the
  OpenStreetMap US Sourdough tileset. A style can be switched without
  refetching a tile: decoded geometry is restyled from the tile cache.
- A styled base replaces the raster tiles while it is installed and draws
  beneath every overlay, so markers, routes and the location indicator stay on
  top. The native surface paints the base's background immediately and fills in
  tiles as they arrive; the browser resolves a frame's tiles concurrently rather
  than one after another.
- Place names and road names draw from an SDF glyph atlas on every surface.
  Point-placed names sit at their feature; road names rotate along their own
  lines. Labels are filtered by feature rank and class, collision-rejected and
  capped per frame.
- A route reference draws as a shaped badge — a shield or rounded rectangle in
  the style's own fill, outline and text colours — where the style declares a
  shield block for its route-reference layer. The committed Carto styles carry
  that layer; Bright, Fiord and Positron do not, so those bases draw no
  shields.
- A status readback reports the configured source and style, the drawn and
  failed tile counts of the last resolved frame, and the last tile failure
  detail, so an application can show an honest loading or unavailable state.
- A Mapbox Vector Tile v2.1 decoder turns fetched tiles into layers, features
  and geometries, with typed errors on every malformed path.

## Positioning

- The Rust `map-positioning` crate validates the fixes a device source reports,
  ignores stationary repeats when averaging course, smooths the course around
  the circle (so 350° and 10° average to north), tracks staleness and records
  source failures, so the location indicator, follow camera and navigation
  controller read one consistent stream.
- A simulated position source walks a route with course and accuracy for tests,
  demos and desktop applications before a platform source is wired.
- The positioning controller and its fix and source-error records are part of
  the SDK contract, so Kotlin, Swift and TypeScript applications construct the
  same controller, feed it fixes and read the smoothed course and staleness.
- The browser demo uses it with the Geolocation API: its Location settings show
  the device's position, draw the tracking dot and centre the map, reporting
  denied, unavailable and timed-out sources honestly. Other hosts supply fixes
  from their own location API, so permission and lifecycle stay where the
  operating system requires them.

## MapView and the reference applications

- One `MapView` per platform owns the map surface, the frame pump, the
  lifecycle, gestures, the camera, overlays, the location indicator and the
  listener callbacks. Above it sit `MapSdk`, `Navigator` and the small engine
  objects.
- Frames are explicit: your display link, choreographer or render pump calls
  `draw` with a `MapCamera`. `draw` returns the frame's tile and overlay
  statistics, and `isFrameSettled` lets the pump skip a frame when nothing has
  changed.
- Overlays are addressed by an identifier the view returns: add, update,
  remove, replace the whole list, or clear it. A rejected descriptor keeps the
  previous list in force. Overlay geometry is tessellated once, not rebuilt
  every frame.
- The camera is an immutable value with fit-bounds framing, projection and
  unprojection, and pan, zoom and rotate gestures that hold the point under
  your finger. The reference applications animate camera presets and frame a
  route preview with the same insets-aware constructor.
- The renderer owns the location indicator: a tracking dot with an accuracy
  ring, or a guided course arrow in navigation follow mode.
- Android (Compose), iOS (SwiftUI), React Native and the browser reference
  applications implement one shared design language: a bottom-sheet home, a
  five-control cluster, a follow camera, an offline tile-cache card, a
  readiness-chip diagnostics readout, and a Light / Dark / Auto theme whose Auto
  setting follows the operating system.

## Navigation and simulated trips

- Navigation state is deterministic: the same sequence of location updates
  produces the same phase, step and instruction. A trip moves through
  `Navigating`, `OffRoute` and `Arrived`.
- Step advance supports manual, distance-to-the-end-of-the-step and
  distance-from-the-step policies. A fix whose reported accuracy is worse than
  the policy's minimum never advances.
- Deviation is graded. A fully-off-route fix freezes progress and asks for a
  reroute; an off-step-but-on-route fix keeps counting down and reconciles to
  the projected step.
- Instruction text is the provider's own, never rephrased. The instruction
  payload carries the active maneuver, the next maneuver, a unit-aware distance
  phrase (immediate, a rounded quantity, or suppressed while progress is
  frozen), and the roundabout exit index where one applies.
- The SDK publishes speech triggers over documented distance bands. Each band
  announces at most once per step, with a minimum spacing and repeat distance to
  suppress chatter. The platform owns playback and synthesis, and every
  utterance carries a stable, monotonic identifier.
- Reroute requests pass through a cooldown and a movement threshold, with an
  in-flight gate and a readback that says whether a recalculation is running.
  The defaults are a five-second cooldown and a fifty-meter movement threshold.
- A subscription delivers one topic: instructions, deviation status, phase
  changes or speech. A composite subscription delivers all four, and `cancel`
  is explicit and idempotent.
- `RouteSimulator` interpolates a trip along the planned route's own polyline
  at a pace you choose, answers a fix at a travelled distance or an elapsed
  time, and marks arrival exactly when the fix reaches the final coordinate.

## Search, routing and traffic

- `MapSdk` resolves every provider-backed call through one registry. The calls
  cover forward and reverse geocoding, search prediction, full search, routing
  with ordered waypoints, the distance matrix, traffic incidents, traffic flow
  and raster tiles. A call into an unregistered contract throws
  `ProviderNotConfigured` with the contract name.
- Route options cover automobile, bicycle and pedestrian modes; fastest and
  shortest strategies; metric and imperial units; avoiding toll roads,
  limited-access highways, ferries and unpaved roads; and a departure time.
- A route carries distance, duration, its bounding box and one leg per waypoint
  pair, with the maneuver steps inside each leg.
- `SearchEngine`, `RoutingEngine`, `OfflineManager` and `Diagnostics` expose
  the same members as thin facades, so each capability has one object to hold.
- MapQuest implements the platform API set from one key: geocoding, search
  prediction, routing, the route matrix, traffic incidents, traffic flow and
  raster tiles in standard and retina resolutions. Requests share a token-bucket
  rate limiter, bounded retries for idempotent calls, and a status-to-error
  mapping.
- A URL-template provider serves any raster or vector tileset that publishes a
  `{z}/{x}/{y}` template, so a base map can come from a platform provider, an
  OpenStreetMap-based service or your own server without changing a call.
- Distance matrix and traffic results are available on the native, React Native
  and WebAssembly surfaces.

## Offline maps

- A deterministic compiler turns an OpenStreetMap PBF extract into a
  self-describing `RMOG` routing archive: nodes, a per-edge pack with road
  class, speed and access columns, and turn restrictions.
- A validating reader opens the archive from memory-mapped storage or from
  in-memory bytes. The in-memory path runs in the browser.
- An offline routing provider answers routes with A\* over the graph. It honors
  recorded turn restrictions and derives turn maneuvers from the bearing change
  in the geometry, and its instruction text stays honest where the archive
  carries no street names.
- When the online provider fails, an offline fallback answers from the graph.
  An archive that fails validation throws `OfflineRoutingFailure`.
- The same graph runs in the browser: the WebAssembly surface loads the
  compiled archive from in-memory bytes and answers the same route records and
  refusals as the native targets.
- The disk response cache keeps raster imagery available when the network is
  not. The reference applications include an offline card that plans a route
  from the bundled archive.

## Camera and rendering

- The SDK draws maps with its own minimal `wgpu` renderer. It implements
  web-mercator projection, lists exactly the tiles the camera covers (including
  across the antimeridian), and draws raster tiles from a byte-bounded GPU
  texture cache.
- The tile scheduler deduplicates requests within a frame, falls back to the
  nearest available parent tile, and respects a per-frame fetch budget. A tile
  that fails to decode is skipped and evicted so a later frame can refetch it.
- Route, polygon, marker, pin and traffic-flow overlays draw over the tiles, and
  the location indicator draws above every overlay.
- The same renderer draws into live Android and iOS surfaces and on a WebGPU
  canvas in the browser. High-density surfaces scale labels, line widths and
  marker radii by the surface's device pixel ratio, so point sizes stay as
  designed.
- The camera math is the SDK's: platform recognizers forward ordinary screen
  deltas, and the SDK applies pan, focal-point zoom and focal-point rotation
  through the camera's own ranges.

## Imagery, providers and caching

- Raster tiles come from MapQuest or from any `{z}/{x}/{y}` raster source, in
  standard or retina resolution. A source is validated when it is registered,
  so a malformed template or an inverted zoom range refuses immediately.
- A SQLite-backed response cache holds provider responses under an absolute
  byte budget, a share of free disk space, or both. It provides per-category
  time-to-live, LRU eviction, and cache-then-network behavior around every
  provider contract.
- Raster tiles cache for 30 days by default, the common ceiling under imagery
  provider terms. Each provider declares its own ceiling; a requested
  time-to-live above that ceiling is accepted and clamped, and the effective
  value, the ceiling and the category default are all readable.
- The provider surface is vendor-neutral. Ten provider contracts cover
  geocoding, routing, distance matrix, search, traffic incidents, traffic flow,
  elevation, raster tiles, offline routing and vector tiles, and a provider is
  selected by registration rather than by a change to the call.

## Provider credentials

- The SDK needs no provider key at build time. The reference applications start
  keyless: a URL-template base map draws without a credential, and provider
  calls answer once a credential is applied.
- Credentials are entered at runtime from a settings surface whose rows come
  from one shared provider registry. A key stays in memory unless the user opts
  in to the device's secure storage (Keychain on iOS, Keystore on Android).
- Automated UI runs can inject a credential at launch. It is never persisted or
  logged.
- Registering a provider set again replaces every provider slot. Registering a
  URL-template base map also drops the installed tile cache, because the cache's
  terms belong to the provider that installed it, and a fresh cache starts at
  its category default.

## Errors and diagnostics

- Every failure crosses as one `SdkError` variant; a variant that carries a
  detail string carries it verbatim. The reference applications render the same
  sentence for the same failure across platforms.
- A diagnostics layer keeps bounded per-subsystem rings. Coordinates are
  rounded before an event reaches your listener, and only events at or above the
  severity you subscribe to are delivered.
- The native and WebAssembly surfaces provide subscribe and flush calls. A flush
  is bounded, synchronous and non-destructive, so the same event can be read
  again.
- A bounded per-object ring records failed calls to a provider you implemented
  in your own code, classified as a mapped contract error or an unclassified
  callback failure, with the contract, the method, the time and the verbatim
  failure text.

## Packaging and the live demo

- A tag-driven release pipeline builds and publishes, when a version tag is
  pushed:
  - the Android bindings AAR and the `MapView` wrapper AAR;
  - the iOS XCFramework zip and a generated Swift package manifest;
  - the React Native npm tarball and the WebAssembly npm tarball;
  - the bare WebAssembly archive.

  Each release carries per-asset SHA-256 checksums, an aggregate checksum
  manifest, Sigstore signature bundles, the third-party notices, the license
  texts and an install README.
- The same WebAssembly build powers a keyless live demo. It draws on a WebGPU
  canvas with pan, zoom, bearing and pitch controls, in-page overlays, recorded
  route planning, a simulated trip with the SDK's own navigator, and offline
  routing from a compiled archive. Its provider panel accepts a runtime key for
  live geocoding, routing and tile probes.
- A hosted mobile demo runs the Android, iOS and React Native reference
  applications on a device.
- `getSdkCompiledVersion` answers the loaded core's version on every surface, so
  a diagnostics screen can compare it with the artifact you shipped.

## Attribution

Each provider descriptor carries its own attribution line and terms note, and
the reference applications render the active provider's line from the shared
registry. The keyless demo draws OpenStreetMap data from the OpenFreeMap vector
tileset in the Bright style, with the attribution that style requires. Keep the
active provider's attribution visible in your application. Third-party
dependency notices are regenerated into
[Third-party notices](third-party-notices.md).
