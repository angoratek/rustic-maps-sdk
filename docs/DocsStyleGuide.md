# Documentation style guide

**Status:** adopted 2026-10-31. This is the standard every
published page, API reference, screenshot caption and public documentation
comment is checked against. The reference sites for tone and structure are
Stripe's documentation and HERE's SDK guides: direct, concrete, outcome-first,
and free of storytelling about the team that built the product.

## 1. The reader and the promise

Write for an application developer who has a map, a route or a position to
ship and is evaluating or using the SDK. Assume competence, not context: the
reader has not read the plan, does not know the internal vocabulary and should
never need to.

Every page answers, in this order:

1. **What you get** — the outcome in one or two sentences.
2. **What you need** — SDK artifact, minimum OS, permissions, provider key.
3. **How to do it** — numbered, complete steps with runnable code.
4. **What you should see** — the expected result, including failure states.
5. **Where to go next** — one or two links, not a wall of related reading.

## 2. Voice

- Second person, present tense, imperative for steps: "Create a `MapView`",
  not "The developer can create a `MapView`".
- Short sentences. One idea per sentence. Prefer the concrete noun to the
  abstract one.
- State facts plainly. No "simply", "just", "of course", "it should be noted",
  "as you may know", "in order to", "we are excited to".
- No hedging or self-praise: not "powerful", "seamless", "state-of-the-art",
  "robust", "blazing fast". Show the number or drop the adjective.
- No narrative about the project's process: no milestones, rounds, plan
  numbers, owner names, internal tickets, "this slice", "for now", "future
  work", "we plan to". Describe what exists and how to use it.
- Parentheses carry data or a short clarification only. Never put an insider
  aside in parentheses (for example a note about hostnames, repository paths
  or how the site is hosted). If the reader does not need it, delete it.
- American English, serial comma, sentence case headings. Product names keep
  their exact spelling (`MapView`, `MapCamera`, `MapQuest`, `OpenFreeMap`).

## 3. Page structure

- One `#` title per page, then short sections with `##`. A page longer than a
  screen needs a one-paragraph summary at the top.
- Steps are numbered; reference material is bulleted or tabulated.
- Every code block states its language, is complete enough to run, and is
  checked by `scripts/check-doc-snippets.mjs`. No `...` placeholders in a
  quickstart.
- Every platform-specific page shows the same scenario with the same values,
  so a reader can compare platforms line by line.
- Link text names the destination ("Install the Android artifact"), never
  "here".

## 4. Platform order and names

The order is **Rust, Android, iOS, React Native, WebAssembly** on every
matrix, download table, tab set and screenshot gallery. Rust is first once
its libraries ship.

One concept keeps one name across platforms: `MapView`, `MapCamera`,
`search`, `calculateRoute`, `startNavigation`, `PositionController`. State a
platform spelling rule once (Kotlin `UPPER_SNAKE` enum constants, Swift
lowerCamel cases, TypeScript string unions) instead of repeating it per tab.

Shared prose lives above the tab set. State the rule, the prerequisite, the
ordering and the expected result once for the page; a tab carries only the code,
the artifact name and the facts that genuinely differ on that platform. Never
repeat the same paragraph inside two tabs, and never restate the page's outcome
at the start of each one.

Platform tabs are one page-wide choice. The selected platform is written to
the URL fragment (`first-map.html#android`) with `history.replaceState`, a deep
link wins over the remembered choice, and a link to a heading inside a panel
opens that panel. Panel ids are the platform names; tab buttons carry
`aria-controls` and the panels `aria-labelledby`. The on-page contents list
relabels each panel's headings `Android · Draw a map surface` whenever mdBook
rebuilds it. Without JavaScript every panel prints in full, so the page stays
readable.

## 5. Screenshots

- One set per platform: **six captures**, in the platform order, all from one
  revision. The set is the four flow frames from the reference scenario plus a
  layers-control pair from one keyed session.
- The four flow frames are the **plan sheet** with both endpoints chosen, the
  **provider section** with every row's display name and credential state,
  **simulated navigation** with the turn-by-turn banner, the planned route
  polyline, the ETA line and the playback-speed controls, and the **vector
  base** with its place labels. They come from one session with the same
  camera, the same route and the same trip state. A screenshot that shows a
  different mode teaches a different app.
- The layers-control pair is the map with the control closed over the applied
  provider's raster base, and the same session and camera with the layers
  panel open on its lower rows. Both frames come from one keyed session.
- The navigation frame is the same moment on every platform: the banner names
  the trip's first instruction and the playback speed is set to **1×**.
- Every capture run injects one provider credential into the session through
  the platform's runtime seam; the credential is never stored in the build,
  the page, the image or the capture log. The provider section proves the
  applied state with the composed `{display name} · {state}` rows.
- Captures are phone widths only; tablets are out of scope for the set.
- Captions name what the reader sees (readiness chip, banner, polyline,
  scale bar, attribution, place labels) with the shared vocabulary.
- A capture must be verifiably from the platform it claims: record the
  revision and the scenario in the page and the capture log
  (`docs/Release.md`), and keep the evidence file names in the
  round record.
- Recapture the whole set whenever a UI change reaches any platform; never
  update one platform or one frame alone.

Download artifacts follow one model:

- When a version is published, artifacts use the site's versioned path
  (`/downloads/vX.Y.Z/…`) and are mirrored to an immutable release record. A
  new version never rewrites an existing file.
- Every file carries a `.sha256` sidecar and a Sigstore bundle; the version
  page prints the exact verification identity for its tag.
- A file is mirrored to the site itself while it stays under the host's
  per-file limit (100 MB per file, 1 GB site). A file near the limit moves to
  the release record if the limit tightens.

## 6. API reference pages

- One page per surface: Rust, Android/Kotlin, iOS/Swift, React Native and
  WebAssembly.
- For each public type: purpose, construction, members with parameters,
  return values, errors, threading/async rules and one minimal example.
- Generated material (types, signatures, error variants) is generated, not
  hand-copied; hand-written prose is linted against the generated surface.
- Link every platform page from the API reference landing page and from the
  matching guide.

## 7. Documentation comments

Public Rust documentation comments are the source of the HTML and the
generated reference. They describe the problem the item solves, its
parameters, its return value and its failure modes. They never mention
milestones, rounds, plan identifiers, owner names, internal teams, review
history or "this slice".

```rust
/// Projects a geographic coordinate into the normalized world square.
///
/// # Parameters
/// - `coordinate` — a WGS 84 coordinate inside the valid latitude range.
///
/// # Returns
/// The coordinate's position in the `[0, 1]` world square, with the origin at
/// the north-west corner.
///
/// # Errors
/// Returns [`SDKError::InvalidCoordinateInput`] when the latitude is outside
/// `-90..=90` or any component is not finite.
```

## 8. Banned vocabulary in published material

| Do not write | Write instead |
| --- | --- |
| slice, round, milestone, plan item, M-number | (delete; describe the feature) |
| owner, owner ruling, PRD §, audit round | (delete) |
| refapp, harness, playground (as product words) | reference application, test suite, demo |
| fixture (for a reader-facing artifact) | recorded sample data |
| "for now", "to come", "not yet wired", "pending seam" | what ships today, or omit |
| parenthetical hosting/path/internal notes | (delete) |

## 9. Review checklist

A page is ready when each answer is yes:

- Does the first paragraph say what the reader gets?
- Can every code block run as written, and is it compile-checked?
- Are the platform tabs complete, in order, and showing the same scenario?
- Does the page avoid every banned word and every internal identifier?
- Are all figures, counts and version numbers derived from the artifact or a
  run, rather than remembered?
- Do the links name their destinations and resolve?
- Are screenshots current, from one revision, and in the reference scenario?

## 10. Link and publication hygiene

1. Link text names the destination ("Install the Android artifact"), never
   "here" or "see below".
2. Relative links between site chapters stay relative and are verified by the
   mdBook build (`create-missing = false` catches renamed chapters).
3. Private paths (`PLAN.md`, `PRD.md`, `tmp/` research notes) never appear in
   site chapters. The publish scrub refuses the build when they do. The
   published package identities are the one allowed exception to the
   `com.angoratek` content denial: the scrub permits the Maven coordinate form
   (`com.angoratek:rustic-maps-sdk[:version]`), the artifact package form
   (`com.angoratek.rustic-maps-sdk…`) and the npm scopes in installation
   snippets, while a bare `com.angoratek` (a private bundle id) stays refused
   everywhere (`scripts/build-docs-site.sh`, self-checked).
4. Numbers, versions, file paths and API names are facts: copy them from
   source files, never from memory.
5. Pages whose content is generated (the UDL contract chapter, the third-party
   notices) are never hand-edited; the build script's byte-equality assertions
   refuse a desynced stage.
6. When you change source code that a site chapter documents, update the
   chapter in the same commit so no published page lags its source.

## 11. Code conventions the documentation cites

These rules apply to the code samples in every page and to the generated
binding surfaces.

- **No abbreviations in public identifiers.** Spell every public identifier
  out: `GeographicCoordinate`, not `GeoCoord`; `instruction_display_text`, not
  `instrDispTxt`; `time_to_live_seconds`, not `ttl`. The standard forms `SDK`,
  `URL`, `HTTP`, `wasm` and `MVT` are accepted in prose, never as identifier
  shorthand.

| Principle | Rust core | Kotlin binding surface | Swift binding surface |
|---|---|---|---|
| Single responsibility | one crate per concern | one class per screen/feature | one struct per screen/feature |
| Open–closed | a new vendor means a new `*Provider` implementation; the core stays untouched | a new capability means a new composable or view model | `final class` plus protocol; no forks in existing code |
| Liskov substitution | mocks in `geospatial-traits::mock` must satisfy every tested consumer | subclasses and reference applications honor suspend contracts | SwiftUI view models honor async throws contracts |
| Interface segregation | narrow provider contracts, not one mega-trait | role-targeted protocols, not god objects | role-targeted protocols |
| Dependency inversion | the SDK adapts the published provider layer at one bridge; core crates never depend on a vendor crate | view models depend on injected SDK protocols | dependency injection through initializers |

- **Errors stay typed.** Rust failures are `geospatial_types::SDKError`, with
  no `unwrap`, no `expect` and no raw `String` errors; panics never cross the
  FFI boundary. Kotlin and Swift surface the contract's typed exceptions as
  sealed UI states and thrown async errors.
- **Property- and fixture-first testing.** Coordinate, camera and projection
  math gets `proptest` suites; every external payload gets a recorded fixture
  and a parser test; reference applications double as UI-test hosts.

## 12. Domain vocabulary for repository documents

The pages under `docs/` are internal. They use the project's terms
consistently; published site pages must not use the internal ones (see §8).

| Term | Meaning in this repository | Do not write |
| --- | --- | --- |
| tile | One image tile keyed by `TileKey` (zoom, x, y; 256/512 px source) | map image, chunk, texture (when it is a texture, "texture" is the GPU term) |
| tile fixture | The recorded committed tiles the render tests serve | sample imagery, test images |
| golden | A committed reference image rendered with a fixed camera | snapshot, baseline render |
| fix | One location result from the location source (position + accuracy + time) | GPS point, location update |
| pose | The camera's full state: center, zoom, bearing, pitch | camera position (incomplete), viewpoint |
| leg | A route segment between consecutive waypoints | route portion |
| step | One maneuver-bearing unit inside a leg | instruction |
| maneuver | The turn action derived from graph or provider geometry | turn instruction (reserve that for the display text) |
| banner instruction / spoken instruction | The two output channels of an instruction; speech is triggered by the SDK, voiced by the platform | TTS text, voice guidance |
| deviation | The graded state of leaving the active route or step | reroute (that is the remedy, not the condition) |
| reroute | The act of computing a replacement route after deviation | navigation reset |
| trip session | The active `NavigationSession` value | navigation, route |
| contract | The frozen UniFFI UDL file, the single source of truth for bindings | interface, API (too broad) |
| surface | The FFI face a binding layer sees (native, wasm) | boundary layer |
| fixture | A recorded external response (or tile) a parser or test replays | mock (a mock is a behavioral stand-in; a fixture is recorded data) |
| cache terms | A provider contract's declared cache time-to-live and budget | caching policy (vague) |
| readiness | The diagnostics readout a reference app shows on its home card | health check |
