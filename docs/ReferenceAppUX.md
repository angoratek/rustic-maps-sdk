# Reference-Application UX Language (`docs/ReferenceAppUX.md`)

The single design-language source of truth for the reference applications
(PRD §4.9, FR-I): the one uniform UX language the Kotlin/Compose, Swift/
SwiftUI and React Native refapps implement idiomatically, and that the wasm
playground (`platform/web/index.html`, the web refapp of record) already
implements.

Naming policy follows the repository rules: no abbreviations, no personal or
brand-identifying names, every tunable value a named constant. Everything in
§2 is therefore a token table; anything not listed there is not a design
decision and must not be invented ad hoc per platform.

The native applications implement the v2 form: one bottom sheet, a
search-first flow, a navigation drawer and diagnostics behind a chip. The web
playground keeps the v1 floating-panel form (§10) while implementing the same
consumer flows.

---

## 1. Layout archetype

One archetype everywhere. The v2 revision (round 9, 2026-10-28) reorganizes
it around a single principle: **the map is the product; the interface is one
bottom sheet that folds away.** The v1 form this replaces — floating panel
stacks pinned to both edges, the readout and status always on screen — is
what the *web playground* keeps (§10); the native refapps implement v2:

- **The map fills the viewport.** The map surface is the screen's background
  layer; the sheet floats above it. Never a map inside a box.
- **One home sheet** docked at the viewport's bottom edge, rounded top
  corners (`radius-surface`), with a grabber. It spans start-end minus the
  gutter at wide widths and full-bleed (`gutter-inline` on its own padding)
  at phone width. Its three states:
  - **peek** — the fold. Content is exactly `SHEET_PEEK_CONTENT_ROWS` rows:
    the destination search field ("Where to?") and one action row — the
    route's Start / the trip's Exit beside the **Search an origin** entry
    that opens the search view armed for the origin, so both plan fields are
    one tap from the home sheet on every platform. Height is content-driven,
    never a viewport fraction.
  - **half** — the working content (search results → the route summary card,
    location, offline maps, camera presets, theme), grown to
    `sheet-half-height`. Demo scenes live in the map layers panel; the native
    half sheet carries no demo-plan or demo-scene rows.
  - **full** — scrolled content, capped at `PANEL_MAXIMUM_HEIGHT_FRACTION`
    of the viewport height (0.72 at phone width) so the attribution and any
    docked dock stay visible.
- **The navigation drawer** replaces the home sheet *in place* during a
  simulated trip: a low dock (grabber + ETA-distance line + phase + speech
  + exit). Everything §7 drives (the turn banner, then line, deviation
  alert) sits at the map's top; the drawer only carries progress and
  control.
- **The turn banner** (the first thing the driver must read) sits at the
  *top-center* of the map in navigation mode.
- **The readiness chip** docks at the viewport's top-start edge: one short
  line in §6.1's four states ("Ready", "Loading…", "Degraded: …",
  "Failed: …"), tappable — it opens the diagnostics sheet. Every refapp has
  exactly one readiness signal, and this is it.
- **The attribution notice** docks at the viewport's bottom-end edge
  immediately above whatever dock (sheet or drawer) is present (PRD §5
  Compliance: MapQuest attribution + the ODbL/OSM notice are part of the
  default map view, not an about screen).
- **The error strip** (§6.3) docks directly above the sheet's grabber when
  an error is live; its raise is the sheet's tallest state. It wraps the
  full detail — never truncated.
- **The diagnostics sheet** opens from the readiness chip: the §6.1
  readout's lines (renderer mode, adapter, center/zoom/bearing/pitch, the
  per-draw tile statistics, the compiled version), plus the demo's frame
  facts. Diagnostics are one tap away and zero screens from the fold —
  a consumer run should never need them; they exist so honesty about the
  renderer never depends on a debug build. The diagnostics sheet and the
  developer console (§4.2) are the opt-in surfaces: no SDK event subscription
  and no rendering log reaches a console or a pane before one of them opens,
  and the stream is bounded and cleared per session. This applies to all four
  surfaces, including the web demo's Advanced section.
- **Product language for consumer rows.** Consumer-facing strings name the
  *feature*, never the seam: "Demo scenes", "Offline maps", "Simulated
  navigation", "Theme". No "FFI", "wasm seam", "overlay renderer active",
  "fixture" in a consumer row — renderer/adapter detail and fixture
  provenance stay in the diagnostics sheet. The one place SDK jargon is
  *required* verbatim is the error strip and diagnostics detail (§6.3).

**Responsive rule (phone width).** Below the named constant
`PHONE_WIDTH_BREAKPOINT_PIXELS = 560` the sheet is full-bleed, the half
state grows to `sheet-half-height` (≤ 0.55 of the viewport so the map stays
the larger half), the full state keeps the 0.72 cap, the navigation drawer
keeps its low-dock height, and the turn banner spans the full width. No
layout ever introduces horizontal scrolling: the viewport holds the map,
the sheet scrolls *inside* itself, the drawer never scrolls (its content is
fixed by §7).

## 2. Design tokens

**Single source and gate (2026-10-30).** `platform/reference-tokens.json` is
the machine-readable copy of this section — §2.1's color roles, §2.3's scale,
§2.4's constants, and the unified SDK error-detail strings. Every platform
mirror (Compose, SwiftUI, React Native, the web playground) reproduces its
names and values, and `scripts/check_ux_parity.mjs` fails when one drifts; a
token change lands in the artifact and every mirror in the same change.

### 2.1 Color system (semantic)

Colors are defined as semantic roles, each carrying a dark-theme and a
light-theme value. Hex values are the locked reference values; the CSS block
in the playground implements exactly this table (§9), and native platforms
mirror the names verbatim (see the naming column).

| Token name | Dark theme | Light theme | Role |
| --- | --- | --- | --- |
| `surface` | `rgba(20, 24, 30, 0.86)` | `rgba(255, 255, 255, 0.92)` | panel background |
| `surface-border` | `rgba(255, 255, 255, 0.14)` | `rgba(0, 0, 0, 0.12)` | panel + divider borders |
| `text-primary` | `#f2f4f8` | `#1c1e21` | primary panel text |
| `text-secondary` | `#9aa4b2` | `#565c66` | labels, hints, value lines |
| `accent` | `#4c8dff` | `#2563eb` | interactive: sliders, links, focus, route imagery |
| `positive` | `#34c07c` | `#1d8a4e` | readiness "ready", arrival |
| `warning` | `#e0a03a` | `#a07016` | readiness "degraded", the pending-reason strips |
| `danger` | `#e05656` | `#c22f2f` | readiness "failed", errors, the off-route alert |

Rules the table encodes:

- **The map canvas is dark-friendly in both themes.** The renderer's clear
  color is opaque black (a Rust-side rendering constant, not a UI token), so
  the themes differ only in the panels: dark theme dark-tints panels over
  the dark canvas ("glass on glass"), light theme uses nearly-opaque light
  panels ("paper on glass"). Both must hold a WCAG AA contrast ratio for
  `text-primary` on `surface` — the chosen pairs do.
- There is no third accent. If a platform needs a color the table does not
  name, add it to this table first (both themes), then use it.
- Interaction-state tints a toolkit derives from the named roles —
  hover/pressed surface tints, an elevated surface, an accent-tinted fill and
  its `on-accent` text — are platform-derived and stay out of the shared
  schema; `scripts/check_ux_parity.mjs` ignores them while still failing on
  any named role's drift.

### 2.2 Canvas-mirror rule

Interactive 2D-canvas paints (the degraded scene background and the debug tile
grid) and the location indicator passed to the GPU surface cannot read CSS
`var()` strings — the browser-loop
constraint documented in `docs/reference/BrowserLoop.md`. Those paints therefore use
**literal JS constants that mirror this table by name**, grouped in one
block (`SCENE_BACKGROUND_BY_THEME`, `TILE_FILL_BY_THEME`,
`TILE_STROKE_BY_THEME`, …) in `platform/web/playground.mjs`, each with a comment
naming the token it mirrors. When a token value changes the mirror constant
changes in the same change. The rule for other platforms: rendering *inside
the GPU map surface* colors (route line, markers, traffic congestion
colors) are SDK/renderer constants, not UX tokens — only paints a platform
layer itself performs (the 2D fallback above) mirror the tokens.

**The vector base's clear color is style data, not a UX token (M21 note,
2026-10-30).** Each named vector style carries its own `background` in the
committed style artifact (`rustic-maps-rendering`'s `fixtures/render/vector-style-*.json`),
and the page clears the canvas to the active style's background so the style
was the color it was designed on. The browser loop cannot read those artifact
values as CSS either, so `platform/web/playground.mjs` mirrors them in one
block (`VECTOR_BASE_CLEAR_COLOR_BY_STYLE`, one comment per style naming its
artifact) exactly like the token mirrors above — but the source of truth is the
style artifact, and a shell never redefines a style's ground color. A new style
brings its own background with it; `check_ux_parity.mjs` does not compare these
values because they are not token-backed.

### 2.3 Typography

One font stack per platform is deliberately not shared; the *scale* is.

| Token name | Size | Weight | Use |
| --- | --- | --- | --- |
| `type-caption` | 12px | 400 | status line, attribution, reasons |
| `type-body` | 13px | 400 | panel body, control labels |
| `type-body-strong` | 13px | 600 | group labels, toggle labels |
| `type-title` | 14px | 600 | panel titles |
| `type-maneuver` | 22px | 600 | the turn banner's instruction text |
| `type-maneuver-distance` | 22px | 400 | the turn banner's distance phrase |

`type-maneuver-distance` is intentionally lighter-weight than the
instruction text set at the same size: distance is context, text is
command. Numerals in the readout use the platform's default monospace stack
(`--font-numeric-readout` token, CSS value
`ui-monospace, 'SF Mono', monospace`); nothing else is monospace.

Map labels are **style data, not UX tokens**: their size, colour and halo
come from the active vector style, as the upstream style declares them. A
route reference is not a road name: a style that declares a shield block for a
symbol layer draws the reference as a shaped badge — a shield or rounded
rectangle in the style's own fill, outline and text colours, with the
reference centred inside — and a style without one paints no route reference
rather than a misleading name. A tileset that carries no route-reference
source layer (the OpenMapTiles schema these styles use) yields no badge and no
route reference; the committed Carto regional frame is the badge proof. The
renderer's rules for them are fixed rather than themed: the glyph edge sits
on the signed-distance field's 0.75 contour, the anti-aliasing band is 0.84
device pixels at every text size and pixel ratio, and a halo's width is the
style's own width (never a fixed threshold offset); the drawing buffer is
always the device-pixel size of its laid-out surface, so labels are never
upscaled by the compositor. The rendering crate's label tests and the committed
z14 goldens pin the MapLibre-parity result.

### 2.4 Spacing, radii, gutter

| Token name | Value | Use |
| --- | --- | --- |
| `gutter-inline` | 16px | viewport edge → panel edge |
| `gutter-block` | 16px | viewport edge → panel edge, vertical |
| `panel-padding` | 14px | panel inner padding |
| `stack-tight` | 8px | between controls inside a group |
| `stack-group` | 12px | between groups inside a panel |
| `radius-surface` | 12px | panels |
| `radius-control` | 8px | cards, embedded boxes, buttons |
| `radius-inline` | 6px | status line, chips, small reasons |
| `elevation-surface` | `0 1px 4px rgba(0, 0, 0, 0.25)` | floating panels |
| `sheet-half-height` | `0.5` of the viewport height | the home sheet's half state |
| `sheet-half-phone-height` | `0.55` of the viewport height | the half state's phone-width cap |
| `sheet-peek-content-rows` | 2 | peek-state rows (search field + one action row); peek height is content-driven, never a viewport fraction |
| `field-of-view-degrees` | 60 | the archetypal framed camera (mirrors `rustic-maps-rendering`'s default) |
| `navigation-follow-zoom` | 17 | the navigation follow camera's base zoom (§5.3); mapbox's nav zoom ladder tops at 16.35 — locked deeper per product direction so turn geometry is legible |
| `navigation-follow-zoom-maximum` | 17.5 | the fixed follow zoom's clamp toward trip end (§5.3) |
| `navigation-follow-pitch-3d` | 45 degrees | the 3D follow camera's pitch; the 2D/3D toggle sets it to 0 in 2D |
| `map-control-zoom-step` | 1 zoom level | one tap of the zoom +/− control |
| `map-control-zoom-repeat-interval` | 250 ms | holding a zoom button repeats its step at this cadence |
| `compass-visibility-bearing-threshold` | 1 degree | the compass control appears only while the camera's bearing is off north by at least this much |
| `compass-renorth-animation` | 300 ms | the eased "north up" return when the compass is tapped |
| `location-animation-duration-milliseconds` | 1000 ms | the longest motion one fix may start; the actual duration is the interval between fixes, clamped to this ceiling |
| `location-animation-minimum-milliseconds` | 120 ms | the shortest motion one fix starts, so a fast fix stream still animates instead of teleporting |
| `location-animation-dead-reckoning` | `off` | whether the puck predicts motion between fixes; off, because a wrong prediction draws a place the visitor never was |
| `scale-bar-width-range` | 50–120 px | the scale bar targets a width in this range, then picks a clean distance from the 1-2-3-5-10 ladder |

### 2.5 Consumer copy

**Rule: a consumer string exists if and only if it is in `ui_labels`.** The
canonical source is the `ui_labels` section of `platform/reference-tokens.json`;
every reference surface (Compose, SwiftUI, React Native including its two
native hosts, and the web playground's `platform/web/ui_strings.mjs`) mirrors
those values. `scripts/check_ux_parity.mjs` reads both sides, normalizes each
platform's interpolation (`%s`, `%@`, `${…}`, `\(…)`) to the artifact's one
placeholder syntax, and reports every omission and divergence. A
`platform_overrides` entry is explicit and allow-listed, and carries its reason
in the artifact; there is no implicit per-platform wording.

The table below is checked against the artifact by the same gate, so it cannot
drift. Deliberately outside this table, per the copy audit (§3.3): contract text
(maneuver instructions, speech, provider display names, verbatim error
details), value formatting (`m`/`km`/`ft`/`mi`, pluralization, locale), SF
Symbol and system-back glyphs, diagnostics jargon, registry rows
(`platform/providers.json`), and content labels (`platform/reference-scenes.json`).

| ID | Canonical text | Platform overrides |
| --- | --- | --- |
| `peek.primary_action` | Start simulated navigation | — |
| `plan.origin_hint` | Search an origin | — |
| `plan.destination_hint` | Where to? | — |
| `plan.destination_placeholder` | Search a destination | — |
| `plan.search_heading` | Plan a route | — |
| `plan.plan_route_action` | Plan route | — |
| `plan.planning_in_flight` | Planning the route… | — |
| `plan.searching_in_flight` | Searching… | — |
| `plan.empty_results_hint` | No results yet; search a place above. | — |
| `plan.no_matches` | No matching places. | — |
| `plan.autocomplete_unavailable` | Suggestions are unavailable; keep typing to try again. | — |
| `plan.demo_places_heading` | Demo places | — |
| `plan.demo_no_match_hint` | No demo place matches the {field}; apply a provider key in Settings for live search. | — |
| `plan.recorded_route_note` | Recorded demo route, pedestrian, fastest. | — |
| `plan.no_recorded_route_hint` | No recorded demo route from {origin} to {destination}; apply a provider key for live routing. | — |
| `plan.origin_label` | Origin | — |
| `plan.destination_label` | Destination | — |
| `plan.search_action` | Search | — |
| `plan.my_location_action` | My location | — |
| `plan.swap_action` | Swap origin and destination | — |
| `plan.missing_origin` | Enter an origin to plan the route. | — |
| `plan.missing_destination` | Enter a destination to plan the route. | — |
| `plan.missing_both` | Enter an origin and a destination to plan the route. | — |
| `plan.my_location_unavailable` | My location is unavailable: {reason} | — |
| `plan.trip_source_demo_plan` | Selected demo plan: {plan} | — |
| `route.start_action` | Start simulated navigation | — |
| `route.edit_action` | Edit | — |
| `navigation.exit_action` | Exit | — |
| `navigation.phase_navigating` | Navigating | — |
| `navigation.phase_off_route` | Off route | — |
| `navigation.phase_arrived` | Arrived | — |
| `navigation.phase_not_started` | Not started | — |
| `navigation.eta_prefix` | ETA | — |
| `navigation.eta_distance_label` | Distance remaining | — |
| `navigation.eta_duration_label` | Duration remaining | — |
| `navigation.deviation_alert` | Off route — {offset} from the route line; guidance is frozen until a graded fix recovers or a reroute applies. | — |
| `navigation.voice_toggle` | Voice guidance | — |
| `navigation.speed_label` | Speed | — |
| `navigation.pause_action` | Pause | — |
| `navigation.resume_action` | Resume | — |
| `navigation.speed_multiplier_label` | {multiplier}× | — |
| `navigation.seek_label` | Seek | — |
| `readiness.chip_ready` | Ready | — |
| `readiness.chip_loading` | Loading… | — |
| `readiness.chip_degraded_prefix` | Degraded:  | — |
| `readiness.chip_failed_prefix` | Failed:  | — |
| `settings.open_accessibility` | Open settings | — |
| `settings.close_action` | Close | — |
| `settings.credential_applied` | {provider} key applied | — |
| `settings.credential_not_applied` | {provider} key not applied | — |
| `settings.keyless_status` | Set a provider key to load live map data | — |
| `settings.provider_row` | {provider} · {state} | — |
| `settings.provider_state_ready` | Ready | — |
| `settings.provider_state_needs_credential` | Needs {credential} | — |
| `settings.provider_state_not_configured` | Not configured | — |
| `settings.routing_demo_endpoint` | Demo server: {url} | — |
| `settings.routing_unreachable_next_step` | The demo server did not answer. Retry, or enter your own deployment address. | — |
| `overlays.clear_action` | Clear overlays | — |
| `scenes.heading` | Demo scenes | — |
| `plans.heading` | Demo plans | — |
| `camera.presets_heading` | Camera presets | — |
| `theme.heading` | Theme | — |
| `offline.heading` | Offline maps | — |
| `offline.plan_route_action` | Plan offline route | — |
| `offline.recorded_graph_route_title` | Offline demo route | — |
| `offline.recorded_graph_note` | Planned offline from a recorded test graph, not live data. | — |
| `offline.download_action` | Download offline region | — |
| `offline.downloading_status` | Downloading… | — |
| `offline.cancel_action` | Cancel | — |
| `offline.completed_status` | Download complete | — |
| `offline.failed_status` | Download failed | — |
| `offline.stored_tiles_label` | Stored tiles | — |
| `offline.stored_bytes_label` | Stored bytes | — |
| `offline.delete_action` | Delete region | — |
| `offline.deleted_status` | Region deleted | — |
| `offline.browser_unavailable_status` | Offline region downloads are not available in the browser. | — |
| `diagnostics.title` | Diagnostics | — |
| `diagnostics.show_debug_events` | Show debug events | — |
| `diagnostics.hide_debug_events` | Hide debug events | — |
| `diagnostics.clear_action` | Clear | — |
| `diagnostics.no_events` | No SDK events recorded yet. | — |
| `diagnostics.stream_scope_note` | Debug detail stays on until the app restarts. | — |

## 3. Theme model

Owner ruling, 2026-10-30 (PRD §6 item 9): the theme is a three-way choice —
**Light, Dark and Auto** — and **Auto is the default**. This overturns the
earlier "never read the OS preference" rule.

- The **choice** is `light`, `dark` or `auto`. The **resolved theme** is the
  choice itself for Light/Dark, and the operating system's
  `prefers-color-scheme` while the choice is Auto. The resolved theme lands
  as a **presentation attribute on the root element**
  (`data-theme="dark" | "light"` on web; the platform root's equivalent) at
  first paint; the choice itself is published beside it
  (`data-theme-choice` on web) so diagnostics can tell the two apart.
- **Auto follows the OS live**: a change of the system preference re-resolves
  the theme (and repaints the vector base in the matching style) while Auto
  is selected. An explicit Light or Dark choice ignores the system from then
  on.
- The toggle cycles `light → dark → auto → light`; its label always names the
  choice it switches to next ("Switch to auto theme"). Native counterparts
  render the same three choices in their appearance control.
- An explicit choice is **remembered on the device** (web: local storage);
  Auto is the state of a device that never chose. Nothing about the theme
  leaves the device.
- The **vector base** takes its own background from the style the resolved
  theme selects (Bright's paper, Fiord's slate, Positron's grey), so the
  canvas clear color is the style's, never an unrelated constant.
- **All four surfaces implement this** (round 46): iOS (`ThemeChoice` +
  `resolvedTheme(systemColorScheme:)`), web (`data-theme-choice` plus
  `prefers-color-scheme`), Android (`ThemeChoice`/`ThemeChoiceStore` with
  `isSystemInDarkTheme`) and React Native (`theme.ts` + `useColorScheme`,
  persisted through the native bridge). Android and React Native previously
  kept a two-way dark-default toggle; the parity audit is what caught it.

## 4. Component inventory

Two inventories share the tokens: §4.1's **v2 native refapp set** (the
consumer revision's components) and §4.3's **v1 playground set** (the web
panels, unchanged). A component present on both behaves identically
regardless of where it docks.

### 4.1 The everyday flows (v2, native refapps)

- **Home (peek → half).** First paint: the map centered on the shared
  artifact's default camera preset, the readiness chip resolving (§6.1),
  the home sheet in peek. The peek's search field and its action row are
  the entire interface; a drag up (or a tap on the field) grows the sheet
  to half.
- **Plan (two searches → explicit Plan route → confirm).** Tapping the
  peek's destination field or its origin entry opens the search view (the
  sheet's full state): **two fields, origin first and destination second**
  (`plan.origin_hint`, `plan.destination_placeholder`), the same pair and
  order every route planner offers — the origin no longer defaults to the
  demo position. The search view's heading is **Plan a route** and its
  dismiss action **Done** on the three native shells. Each field carries its
  own visible label, its own inline **Search** action and its own **My
  location** chip that fills only that field; a **swap** action exchanges the
  pair and both resolved places. Each field owns its own search and its own
  result list (the list belongs to the field that was searched last); the
  demo-place suggestion rows fill whichever field is focused. Plan route
  resolves each field from an assigned result, an exact demo-place name or
  the provider's first result, so typed text is a valid input on every
  platform rather than a dead end.
  Result rows are the §6.3 provider results (name + secondary detail +
  quality), never invented; a provider failure reports itself in the error
  strip and the row list is never filled with guesses. **Plan route** is the
  explicit action on every platform: it stays disabled with a reason naming
  the missing field until both fields hold text, and it is the only
  assignment-triggered path that plans (a field assignment never plans by
  itself). Planning draws the route's step-shape polyline plus **A/B
  teardrop pins** (origin green A, destination red B, anchored at their
  tips), frames the route's bounding box with edge margins that keep both
  pins and the line clear of the control sheet, and returns to half showing
  the **route summary card** — the route's distance and duration (the
  contract's `Route` records, formatted, never paraphrased) with the route
  options named in the same copy on every platform (pedestrian, fastest),
  a primary "Start simulated trip" and a subtle "Edit" button. The map is
  context in this flow, not a control: tapping it selects nothing (§5.2).
- **Show my location.** The home sheet's **Location** group holds one control
  (**Show my location** / **Hide my location**) and, when it has something to
  say, one status line. Starting it asks for the platform permission (the
  Android runtime permission, the iOS authorization prompt, the browser's own
  prompt), then draws the renderer-owned tracking dot with its accuracy ring
  and centres the camera on each fix; the smoothed course comes from the Rust
  controller, not the raw source. A denied permission, a disabled service, an
  unavailable source and a timeout each state themselves in product words; the
  control returns to the idle label when stopped. The browser demo's Location
  group lives in the settings sheet, the native groups in the half sheet; the
  React Native shell follows with the same control and wording.
- **Navigate from my location.** The Location group's second control
  (**Navigate from my location** / **Stop device navigation**) drives the
  planned route's guidance from the device source's own fixes instead of the
  recorded trace: the turn banner, the ETA line, the deviation judgment and
  the §5.3 follow camera read the same `Navigator` stream a simulated trip
  reads, each fix carrying its own accuracy and the Rust controller's
  smoothed course. The control starts the location source when it is off,
  ends a simulated trip first (one trip runs at a time), and states
  *Plan a route first; device navigation follows the route you planned.*
  when no route exists. Stopping ends the trip and turns off only the
  location source this control started; a source the visitor started by hand
  keeps running. The camera follows the trip's fixed deep zoom and the plane
  toggle's pitch, exactly as during a simulated trip. While the trip runs the
  native shells carry the same control and label in the navigation drawer,
  because the drawer replaces the home sheet there; the browser's settings
  sheet stays reachable throughout.

- **Navigate (§7's card set at v2 placement).** Start swaps the sheet for
  the navigation drawer and applies the framing camera per §5.1, then the
  §5.3 follow camera takes over: fixed deep zoom, course-follow 3D,
  broken by gestures and re-engaged at the next fix. The §4.2 map control
  cluster floats over the map throughout. During
  the trip: turn banner top-center, then line under it, deviation alert
  under that (all §7); the drawer carries the ETA-distance line, the
  phase, the speech toggle, and exit. The trip ends by the drawer's exit
  control or by arrival (§7.1); either returns the sheet to peek. Speech
  is default-on with a speaker toggle in the drawer; what is spoken comes
  **verbatim from the contract's speech channel** (the §6.3 verbatim rule
  applies to spoken guidance); the voice itself is platform-owned
  (`AVSpeechSynthesizer` / Android `TextToSpeech`).
- **Demo plans.** The native refapps keep the shared artifact's recorded plans
  as state — the artifact's trace is the default selection and the peek names
  it — without a visible picker row. The web playground's Advanced panel keeps
  the picker: its **Demo plans** row offers the artifact's recorded plans as
  chips (label and description are the artifact's own, never invented).
  Selecting a plan loads its
  committed navigation trace, draws and frames the recorded route through
  the same overlay composer and fitted camera the planner uses, marks the
  chip selected, and states the plan's description. Starting the simulated
  trip replays the trace's own fixes through the wasm `Navigator` at the
  recorded cadence scaled by the playback speed choice — the native
  `SimulatedTripDriver`'s recorded-trace path — so the §7 card set follows
  honestly: the clean walk reaches Arrived, the deviation-and-recovery trace
  freezes and raises the off-route alert before recovering, and the
  deviation-and-reroute trace applies the recorded replacement route the
  first time guidance freezes (§7.5). A recorded trace replays its samples
  verbatim, so the route-position seek row is hidden for its duration (the
  native `isSeekable` rule); pause, resume and the speed choices stay.
  Planning a route supersedes the selected plan, and **Clear plan** returns
  the panel to its idle state.
- **Demo scenes.** The native refapps' map layers panel is the scene home: its
  Overlays section carries one switch per shared artifact scene plus a Clear
  overlays action, and the half sheet carries no scene rows. The web
  playground keeps its chip carousel: each chip selects or unselects its
  scene; each scene's description is behind an info affordance, not a wall of
  prose under the controls; a scene that cannot run right now renders
  disabled-with-reason per §6.2 on its own row. The imagery (route line,
  markers, polygons, traffic) is the SDK overlay API; no per-platform styling.
- **Offline maps.** The half state's "Offline maps" card owns two honest
  lines: the cache registration state (registered cache path + byte
  budget once the tile-cache registration ran; otherwise
  disabled-with-reason per §6.2) and a live tile line fed by the per-draw
  `FrameStatistics` (requested / drawn / missing / …). A network loss
  is presented as a *state* through the readiness ladder (degraded, with
  the reason naming cached tiles), never a hidden fallback.
- **Diagnostics.** The readiness chip opens the diagnostics sheet (a
  dismissible half-height scroll): renderer mode and adapter detail (when
  any), center / zoom / bearing / pitch (read back through the camera's
  attribute getters — no "no zoom readback" honesty note is needed on
  native), the per-draw tile statistics, the compiled version probe, and
  the demo's frame facts (§6.1's numbers). Nothing here is required to use
  the product; it exists so §6's honesty never depends on a debug build.

### 4.2 The v2 component set (native refapps)

| Component | States | Convention |
| --- | --- | --- |
| Home sheet | peek / half / full; hidden during a trip | §1's states; drag the grabber to move between them; taps land on the controls inside it; the platform back affordance collapses toward peek |
| Sheet grabber | — | the sheet's only gesture target, inset `gutter-inline` from each top corner |
| Origin field | empty / focused / filled / filled by My location | the search view's first field, with its visible "Origin" label, inline Search action and My location chip; its search button and Enter key run that field's search |
| Destination search field | empty / focused / filled / filled by My location | the peek's row ("Destination") and the search view's second field, with its visible "Destination" label, inline Search action and My location chip; the result list belongs to the field searched last |
| My location chip | idle / filling / disabled-with-reason | one per plan field; starts the device source when off and fills only that field from the latest fix; never plans |
| Swap action | idle | exchanges the two plan fields and their resolved places in one tap |
| Plan route action | disabled-with-reason / enabled / planning… | the explicit route action on every platform; disabled until both fields hold text, with the reason naming the missing field; assignment alone never plans |
| Base-map rows | serving / selecting / blocked-on-key | the map layers panel's **Base map** section: the registry's raster sources first, then one row per committed vector style of every vector descriptor (OpenStreetMap US's Carto Light/Carto Dark and OpenFreeMap's Bright, Fiord and Positron), each with `Serving` / `Select` / `Needs <credential label>`. Selecting a vector style installs the SDK's styled base over the raster tiles and beneath the app's overlays; selecting a raster source removes it. Attribution follows the active entry, and the active vector base carries the label note its styles earn: *Vector styles draw land, water, roads with their names, and Latin place names.* A style that declares a shield block on its route-reference layer draws each route reference as a shaped badge; Bright, Fiord and Positron carry no route-reference layer, so those bases draw no shields. The settings surface carries no base-map row |
| Route endpoint pin | A (origin) / B (destination) | one additive overlay kind: a teardrop anchored at its tip with a hollow head center; A green, B red; drawn over the route line |
| Search result row | listed / assigned | a §6.3 provider result (name, secondary detail, quality); an assigned row marks itself subtly with `accent` |
| Route summary card | planning… / planned / failed per §6.3 | distance phrase + duration + "Start simulated navigation" + "Edit"; the map's route line is its visual |
| Navigation drawer | navigating / arrived | the low dock; ETA-distance line, phase, speech toggle, exit; never scrolls (the §7 banner stack carries the content) |
| Location indicator | tracking / navigation / no-fix | **renderer-owned, not a platform widget**: the map shows the device's position itself — tracking mode is a soft dot (accent core, white ring and dark halo, plus the ground-sized accuracy ring when the fix carries one); navigation mode is an up-course dart with a white outline and the accent fill inside the same ring and halo, rotating with the course. Colours and sizes are renderer constants overridable through the layer style. No fix: nothing renders (never a stale last position) |
| Map control cluster | always visible on the map | floating on the map's end edge: zoom + / − (zoom-in above zoom-out), my-location (centers on the indicator; re-engages the follow camera during a trip), 2D/3D toggle (§2.4's `navigation-follow-pitch-3d`), scale bar (bottom-start), compass (top-end, appears only when the camera is off-north by the §2.4 threshold; tap eases north up). The zoom, my-location, 2D/3D, layers and attribution controls share one 40 dp/40 pt diameter on the native refapps |
| Map layers control | collapsed (default layer / non-default dot) / open (Base map, Traffic, Overlays, Device location) | the round control docked directly above the attribution control on the map's end edge, with the accessible name "Map layers"; it opens a scrollable panel capped at `PANEL_MAXIMUM_HEIGHT_FRACTION`, with Close and Escape returning focus to the control. Base map rows switch the base or vector style; the traffic switch debounces 400 ms to the settled camera and states `Needs <credential label>` or `Not configured`; one scene switch per shared artifact scene plus Clear overlays; the Device location row states Off / Waiting for a fix / Active / Permission denied / Unavailable with Stop. A dot marks a non-default layer. The control names no provider and holds no credential |
| Speech toggle | on / off | default on; spoken text verbatim from the payload's speech channel; the voice is platform-owned |
| Readiness chip | §6.1's four states | top-start dock; tap opens the diagnostics sheet |
| Diagnostics sheet | closed (no subscription) / open (Info) / open with debug detail | half-height scroll; §6.1's readout lines + the compiled version; the event stream and any console output start only when the sheet or the developer console opens; the stream is bounded and cleared per session |
| Developer console | closed (Warning/Error dot) / open (level filter, rows, detail) | the floating terminal-glyph control in the map control area; read-only over the SDK's scrubbed stream; the Error / Warning / Info thresholds; a provider row carries the registry display name, contract, method and redacted path, status class, duration and, while capture is on, a bounded response excerpt; the detail view carries the typed request fields and the live tile- and traffic-cache rows; Capture response excerpts is per session and off by default; Export shares the filtered list (a JSON download in the browser, the platform share sheet on native) |
| Provider capability row | serving / needs-credential / not-configured / selectable | one row per capability from `providerRegistrations()`; its serving line and every picker entry read the registry descriptor and compose `{display_name} · {credential state}`, never a bare provider name or a bare state |
| Demo scene switch | on / off | the map layers panel's Overlays section: one row per artifact scene plus Clear overlays; the web playground keeps its chip carousel |
| Offline maps card | registered / disabled-with-reason | cache path + budget line + the live `FrameStatistics` line |
| Error strip | hidden / shown | docks above the sheet's grabber (§6.3: verbatim, full detail, wrapping) |
| Attribution control | static / panel open | compact info button at the map's end edge, labelled "Map attribution and terms"; its panel carries the serving base map's attribution line and terms note, plus the traffic layer's attribution while that layer draws; a layer change is announced in the live region; `type-caption`, `text-secondary` (PRD §5) |
| Reason strip | hidden / shown | §6.2 — the *why* behind any disabled group, card or chip, consumer worded with the capability detail verbatim |

### 4.3 The v1 playground inventory (web, unchanged)

| Component | States | Convention |
| --- | --- | --- |
| Panel (`#controls`, the primary surface) | open (always) | Token-styled floating surface; groups stacked inside with `stack-group` |
| Group label (`h2.group-label`) | static | `type-body-strong`, `text-secondary`, sentence case |
| Labeled slider | enabled / value updating | Label + a *value line* showing the live value; value always updated by gesture (wheel/pan) too, sliders never show a value the camera disagrees with |
| Labeled toggle | checked / unchecked / **disabled-with-reason** | Checkbox + label row; a full group disables as a unit with one reason; see §6.2 |
| Control group (`fieldset.control-group`) | enabled / disabled-with-reason | A labeled box around a related feature; disabling is a group-level act with a visible reason |
| Button | primary / subtle | `accent` background for the one primary action in a group (e.g. Clear overlays), `surface`+border for secondary ones (e.g. theme, preset chips) |
| Chip / preset button | selected / selectable | Camera presets are subtle buttons in a wrapped row, selected preset gets `accent` text |
| Status line (`#status-line`) | `loading` / `ready` / `degraded` / `failed` | see §6.1; carries `data-readiness` on web |
| Error report (`#error-report`) | hidden / shown | see §6.3 |
| Readout (`#readout`) | gpu / overlay | see §6.1 |
| Attribution notice (`#attribution-notice`) | static | `type-caption`, `text-secondary` |
| Turn banner | Navigating / OffRoute (frozen) / Arrived | see §7 |
| Progress lines | live / frozen | see §7 |
| Deviation alert | entered / cleared | see §7.5; `warning` styling, honest text |
| Reason strip (`*-reason`) | hidden / shown | `warning` color, `type-caption`; the *why* behind any disabled/pending group |

## 5. Interaction patterns

### 5.1 SDK-driven vs platform glue

The dividing line the doc commits to: **the SDK owns camera math and
renderer plumbing; the platform owns gesture plumbing and widget
construction.**

| Concern | Owner | Mechanism |
| --- | --- | --- |
| Pan: pixel delta → ground delta | SDK (`MapCamera.screen_to_ground_coordinate` deltas) | the panned under-pointer ground point becomes the new center |
| Wheel/trackpad zoom | SDK zoom semantics clamped to `MINIMUM_ZOOM_LEVEL=0`,`MAXIMUM_ZOOM_LEVEL=20` | platform forwards deltas |
| Camera fitting / presets | SDK (`MapCamera` + `fit_bounds` with `EdgeInsets`) | presets are plain camera values applied via the camera object |
| Animated framing (`flyTo`) | SDK (`fly_to_animation`) | **not on the wasm seam yet** — presets apply instantly there; native refapps animate (deferral noted in §12) |
| Gesture plumbing (pointer capture, momentum, pinch → these deltas) | **platform glue** | `CameraInteraction` (crates/rustic-maps-rendering/src/camera_interaction.rs, the M3 open item landed) carries the delta seam — pan by screen pixels, focal zoom, focal rotation — and the web playground's pointer/wheel loop forwards its deltas to it; recognizers, wheel-to-zoom shaping, the fixture zoom range and inertia stay JS-side, and M5's camera plumbing wires the native recognizers onto the same methods |

The direction contract the split locks in (2026-10-29, the owner's
reversed-pan finding): **map content follows the pointer — a pointer's
ground point at gesture start must land under the pointer after the drag
(a rightward drag moves the content right, a downward drag moves it down);**
the same convention's other faces are a wheel-up zooming in, spread fingers
zooming in, and a clockwise two-finger turn rotating the content clockwise.
Every gesture layer must obey it: the web playground's drag path (pinned by
`platform/web/tests/map_pan_direction.spec.mjs`), Android `panByPixels`
(pinned by the Espresso `theDragPanMovesTheMapContentWithThePointer` test),
and iOS `userDraggedMapPixels`/`userRotatedMapDegrees` (pinned by the
`DemoGesturePanContent`/`DemoGestureRotateContent` launch-argument direction
demos and their XCUITests).

Per-platform gesture note (PRD §4.9 "idiomatic per platform"): the Compose
and SwiftUI refapps consume the SDK `MapCamera` the same way the playground
does (build camera → screen math → camera object), but wire their own
Compose/SwiftUI gesture modifiers until `CameraInteraction` lands; the
gesture surface is the M3→M5 open item, not a per-platform divergence in
where the math lives. This must not be relitigated per platform — when the
`CameraInteraction` seam exists, all three implementations shrink to it.

### 5.3 The navigation follow camera (round 10)

During a guided trip the camera follows a **fixed deep zoom
(`§2.4`'s `navigation-follow-zoom`, clamped by
`navigation-follow-zoom-maximum`)** with **course-follow bearing in 3D**:
bearing eases toward a median-smoothed course-over-ground, and pitch is
`navigation-follow-pitch-3d` (45°) — set to 0 in 2D by the control cluster's
toggle. A user gesture (drag, pinch, rotate) **breaks follow** immediately;
follow **re-engages automatically at the next location fix** — no button
tap required (the my-location control also re-centers explicitly). At trip
start the zoom is applied once and restored after; the framing camera of
§4.1's Navigate flow is the pre-trip preset, the follow camera is the
in-trip state. The demo trace is a walk — directionality is mild; with real
GPS the same smoothing carries it.

### 5.2 The gestures themselves (web, and the reference shape)

- **drag-pan**: pointer down/move over the map; the pixel delta is inverted
  through `screen_to_ground_coordinate` at the viewport center so map
  content tracks the pointer exactly; pointer capture keeps the gesture
  alive outside the canvas.
- **wheel zoom**: zoom clamped by the range the fixture/camera supports;
  sliders and readout follow.
- **double click / double tap** (round 36, every platform): zooms exactly one
  level around the pointer under the gesture through the contract's focal
  zoom member (`zoom_by_levels_around_screen_point`), so the ground point the
  visitor pointed at stays under it. The web recognizes a two-click pair and
  a touch double tap (a pointer pair within 350 ms and 24 px); Android uses
  `GestureDetector.onDoubleTap` with single-tap-*confirmed* reporting; iOS
  (the shipped `MapView` and the RN host view) adds a two-tap recognizer the
  single tap is required to wait on. One double tap never also reports a
  map click.
- **tap**: selects nothing by default (no selection story yet — do not
  attach one to the map in the refapps).
- **base map and content do not compete**: a vector base is a *base layer*,
  so the scene toggles and a planned route compose over it rather than being
  disabled or cleared while it draws.

### 5.4 Keyboard and assistive-technology contract (web)

The browser demo is usable without a pointer (owner direction, 2026-10-31).
The map surface (`#map-overlay`) is one focus stop: `tabindex="0"`,
`role="application"` with `aria-roledescription="interactive map"`, an
accessible name, `aria-keyshortcuts`, and `aria-describedby` pointing at a
visually hidden instruction paragraph. The GPU canvas beneath it is
`aria-hidden`. A skip link is the document's first focus stop and lands on
the control sheet (`#controls`, `tabindex="-1"`).

| Key | Effect |
| --- | --- |
| Arrow keys | Pan by 100 CSS pixels (scaled to the canvas backing store, the same ratio pointer drags use) |
| `+` / `=` | Zoom in one level around the viewport centre, clamped to the fixture range |
| `-` / `_` | Zoom out one level |
| Shift + Left/Right | Rotate the bearing by 5° around the viewport centre |
| Shift + Up/Down | Tilt the pitch by 5° within `[0, 60]` |
| Home | Ease north-up and flat, preserving the zoom level |

Every camera mutation crosses the wasm seam exactly as the pointer
equivalent does; the page owns only the key-to-step mapping. A keyboard
camera move breaks an engaged follow trip just like a drag. Ctrl/Cmd/Alt
chords are left to the browser and the OS. Each move announces its result in
a polite visually hidden live region (`#map-status`) — centre coordinates,
zoom, bearing or pitch — debounced to one message per burst. The map's focus
ring is drawn inside the viewport so it never clips, every button and input
has an accessible name, the 2D/3D toggle exposes `aria-pressed`, and a
visitor who requests reduced motion gets the camera eases applied
immediately along with the stylesheet's transitions disabled.

The hosted mobile-demo page follows the ARIA tabs pattern: a named tablist
with a roving tabindex, Left/Right/Home/End moving selection, a tabpanel
labelled by the selected tab, a session link whose accessible name says it
opens a new tab and which takes focus when it replaces Start, and Stop
returning focus to Start.

The same control set and accessible names are the parity target for the
native shells through their platform accessibility services (TalkBack,
VoiceOver, RN accessibility props); keyboard is a web-only input, the
semantics and wording are not.

### 5.5 Type-ahead in the plan fields

Both endpoint fields suggest places while the visitor types, on every target,
under one rule: three characters
(`autocomplete-minimum-query-characters`), a 250 ms quiet window
(`autocomplete-debounce-milliseconds`), at most five rows
(`autocomplete-maximum-results`), one request in flight per field, a dropped
answer when a newer query supersedes it, and a chosen row that fills the
field, closes the list and holds the place for Plan route. The provider comes
from the settings surface's credential state: the credentialed provider in a
keyed session, the registry's keyless search descriptor otherwise; no shell
names a provider. Fewer than three characters clears the list without a
request, no matches shows one row, and a provider failure keeps the text with
one inline sentence.

### 5.6 The location puck's motion

A trip's position disc and its accuracy ring move continuously between fixes
instead of stepping by the distance between them. The motion comes from one
pure animator in the shared Rust model: a shell hands it a fix and asks it for
the pose to draw at the current frame time. React Native has no seam to the
shared crate, so its shell uses the documented TypeScript mirror of the same
model (`platform/rn/src/locationPoseAnimator.ts`); the Rust animator remains the
reference and both read the same §2.4 tokens.

The rules the four shells follow:

1. **Position interpolates linearly** between the pose the animator is drawing
   and the new fix. A fix is a measurement; easing with overshoot would draw
   the puck where the visitor never was.
2. **A fix that lands mid-flight retargets from the drawn pose**, never from the
   previous fix, so the drawn position is continuous however irregular the fix
   cadence.
3. **The duration is the interval between fixes**, clamped to the
   `location-animation-minimum-milliseconds` (120 ms) floor and the
   `location-animation-duration-milliseconds` (1000 ms) ceiling. The first fix
   of a session draws immediately. A stream slower than the ceiling makes the
   puck arrive early and wait; a faster stream still animates rather than
   teleporting.
4. **The course takes the shorter arc** through the SDK's signed angular
   difference, eased with the existing smoothstep, so 359° → 1° turns one
   degree forward rather than a full turn backwards. A fix without a course
   keeps the previous bearing and does not animate it.
5. **The accuracy radius eases linearly**, so a shrinking ring reads as a fix
   getting better instead of flashing.
6. **Idle costs nothing.** When no motion runs the animator answers the latest
   pose and reports that it is not animating, so a shell can stop redrawing.
7. **Dead reckoning stays out** (`location-animation-dead-reckoning` is `off`),
   because a wrong prediction is worse than a still puck.

Each shell drives the animator from the frame pump it already owns: the
browser's `requestAnimationFrame`, the Android Compose frame clock, the iOS
`TimelineView` and the React Native module's frame callback. The follow camera
consumes fixes, not interpolated poses (§5.3), so the puck's motion never drags
the camera. The motion model and its measured cadences are recorded in
`PRD.md §6 item 66`.

### 5.7 North-up during navigation

The compass is a mode toggle while a guided trip runs. The follow camera carries
two bearing modes:

- **Course-up** is the default at trip start: the bearing eases toward the
  median-smoothed course over ground, as §5.3 describes.
- **North-up** is the visitor's choice: the target bearing is pinned at 0 and
  every following fix keeps it there. The compass stays visible while north-up
  holds so the visitor can leave the mode.

Choosing course-up again clears the pin, and the next fix resumes the eased
course chase. A gesture breaks follow but does not change the chosen mode, so
the my-location control's explicit re-engage preserves it. A trip's end
restores the default course-up mode together with the captured pre-trip camera;
the mode is trip state and is not remembered across trips.

React Native uses the documented TypeScript mirror of the same model
(`platform/rn/src/followBearingMode.ts`), including these mode transitions; the
shared Rust `FollowCamera` in `map-refapp-core` is the reference. The compass
visibility threshold and the re-north animation are §2.4 tokens, and the
control's placement and states are in §4.2.

## 6. Status, readiness, and provider-echoing errors

### 6.1 Readiness ladder (first-class, never hidden)

Every refapp presents one line — the status line — with exactly these four
states, in order of the ladder the app walks at start:

| State | `data-readiness` | Color | Meaning |
| --- | --- | --- | --- |
| Loading | `loading` | `text-secondary` | SDK module/`start_sdk` still resolving; text ends in an ellipsis |
| Ready | `ready` | `positive` | "Maps SDK gpu renderer active (version X)" — live guidance that the actual renderer mode is active |
| Degraded | `degraded` | `warning` | A documented capability substitute fired: e.g. "Maps SDK overlay renderer active … WebGPU unavailable: <detail>" — the 2D tile-grid path the SDK reports, presented as a *state*, not a hidden fallback |
| Failed | `failed` | `danger` | The SDK failed to start; the detail must name the failure verbatim |

The readout (the web playground's readout panel; the native refapps'
diagnostics sheet per §4.1) reports the same reality with more numbers:
mode gpu/overlay
+ renderer detail (e.g. GPU adapter name), center, zoom/bearing/pitch, tile
statistics (requested/drawn/missing/fragments, fetch zoom level, tile
resolution, overlay triangles). In degraded mode it reports visible-tile
count instead of GPU statistics. **Nothing about renderer health is shown
only visually in the canvas** — a user must read availability, not infer it.

### 6.2 Disabled-with-reason

Any control group whose feature cannot run right now is disabled as a *unit*
with one visible reason strip (`warning`-colored, inside the group). The
reason states the capability, not the blame: "Scene overlays draw on the
WebGPU surface only; this session uses the 2D tile grid (WebGPU
unavailable: …)". Reasons are never empty on a disabled group and the group
never silently re-enables: the enabling is simultaneous with hiding the
reason. This is the pattern the overlay groups already use, and it generalizes
to: offline pack download while offline, provider error states that remove
capability, and any pending contract surface.

### 6.3 Provider-echoing error states (FR-I)

Errors are presented honestly and structurally — **the error block echoes
the provider's/SDK's own words**:

- A dedicated error report (the `#error-report` strip, `danger`-bordered)
  appears above the status line and is visible until the next successful
  action clears it.
- Its body echoes the failure detail **verbatim** — the SDK contract's
  `SdkError` variants and detail strings cross the binding unchanged, and
  the UI never paraphrases, truncates or swallows them.
- On the wasm seam today failures cross as plain detail strings (the
  `failure` helper in `rustic-maps-web-bindings`); the structured
  `variant`-carrying shape of the generated
  `platform/web/generated/rustic_maps.d.ts` is the contract
  the UI targets, and the error block's *label line* shows the variant name
  when one is available (native refapps) or severity + origin when only the
  detail is (wasm today).
- Silent failure is a bug: no catch block in any refapp is allowed to end
  without either a state change or an error presentation.

### 6.4 Loading / empty states

- Loading: the readiness ladder above, plus per-surface loading text ended
  with an ellipsis; no spinners in v1 (deterministic text over a live canvas).
- Empty: when a scene/preset produces no content (e.g. no recorded fixture
  tiles at an extreme preset), the renderer's honest `missingTiles`
  statistic is surfaced in the readout — the empty map is an *announced*
  state with counts, not a blank screen of lies.

### 6.5 Provider credentials and the settings surface (round 18)

Provider credentials are **never embedded in a refapp build**. Every refapp
starts keyless and offers a settings surface, opened by a gear control beside
the readiness chip:

- **Rows from the shared registry.** The settings view renders the provider
  descriptors in `platform/providers.json` (display name, credential label,
  availability per platform, attribution, hint). A new provider is a
  descriptor row plus a registration dispatch arm, never a view rewrite.
  Every capability row and every picker entry composes the descriptor's
  display name with its state (`MapQuest · Needs API key`), so a provider is
  never shown as a bare name or a bare state. A keyless routing descriptor
  declares the public demo address the application passes as its deployment
  address; no reference application hardcodes that address, and an unreachable
  server is reported verbatim with the descriptor's terms and a retry or
  own-deployment next step.
- **Masked entry, explicit apply.** The credential field is a secure text
  field; Apply registers the providers through the SDK and retries
  initialization; a refusal is shown verbatim. The value is never echoed in
  logs, diagnostics, error text, screenshots or the artifact.
- **Persistence is opt-in.** "Remember on this device" defaults off. On
  means the platform secure store (Keychain / Android Keystore AES/GCM);
  off means the value is gone with the process. Clearing removes both copies
  and states that the running session needs a restart to become keyless
  (the SDK registry has no unregister).
- **Keyless is a first-class state.** Without a credential the app keeps the
  settings action available and anything that needs a provider surfaces the
  typed `ProviderNotConfigured` error verbatim. What the map itself shows is
  registry-driven: when the shared registry declares a credential-less base map
  (the URL-template raster descriptor), the app registers it at startup so the
  shipped surface can attach, and the offline card states that such a base runs
  uncached. When the registry also declares a `keyless_base_map` (a provider key
  plus a committed style id), that styled vector base is installed over the
  raster source once the map view exists, so a keyless start opens on styled
  OpenFreeMap Bright rather than the plain OSM raster (owner ruling 2026-10-31,
  PRD §6 item 18). The raster source stays registered but hidden while the
  vector base serves; an explicit base-map choice always wins. The chip then
  reads the ready line, because a live base map really is serving. A registry
  with no credential-less source falls back to cached/recorded imagery and the
  neutral prompt "Set a provider key to load live map data". The UI suites
  assert whichever of the cases this build's registry implies, never a blank or
  a renderer failure.
- **Automation injection is runtime-only.** Local/CI UI automation may supply
  a credential through the `providerApiKey` launch-intent extra (Android/RN)
  or the `PROVIDER_API_KEY` launch environment (iOS/RN), optionally paired
  with a provider key (`providerKey` / `PROVIDER_KEY`); the value goes through
  the public registration path, is never persisted by the injection and is
  never logged. Tests that need provider/surface data skip when it is absent.
- **Browser caveat.** A key entered in the browser page is visible to that
  page and its network panel by construction; the rule there is only that no
  key ships by default.
- **Base map (M9's native switch, round 35).** The map layers panel's
  **Base map** section carries every descriptor that either serves through an
  applied credential or publishes a `raster_url_template`, one row each,
  showing the descriptor's display name and a `Serving` / `Select` status.
  Selecting a row registers that source on
  the SDK's raster-tile slot and asks the map view to re-resolve its
  provider; a refusal is shown verbatim and the serving base map stays. The
  rows and the attribution line both come from the registry, so the band
  always attributes the imagery actually on screen — never a provider the
  app merely could register. A credential-less source identifies the app
  through a `User-Agent` naming the app and the SDK version, because public
  tile services refuse anonymous clients. The settings surface carries no
  base-map row; it keeps credentials, per-capability providers and the
  remembered-choices reset.
- **Styled vector base (M22, 2026-10-30).** The same panel section lists the
  registry's committed vector styles. Selecting one calls the
  shipped map view's additive vector base member (`MapView.set_vector_tile_source`
  on the contract), which installs the descriptor's vector tileset with that
  style: the SDK resolves the camera's visible tiles through one shared Rust
  scheduler (fetch once, decode once, restyle from the cached tile when the
  style changes or the camera crosses a style zoom stop), draws the styled
  base over the raster tiles and beneath every overlay, and reports its style,
  attribution and last-frame tile counts back through
  `vector_tile_source_status`. The section names the style with its own
  attribution line and states what the styles paint: land, water and roads
  with their Latin place and road names. A style that declares a shield block
  on its route-reference layer draws each road or transit reference as a
  shaped badge; Bright, Fiord and Positron carry no route-reference layer, so
  those bases draw no shields.
  A raster selection clears the vector base, so the two never paint over each
  other.

## 7. Simulated turn-by-turn live UI (`rustic-maps-navigation` payloads)

The TBT UI is specified directly against the frozen contract's payload
shapes (`crates/rustic-maps-native-core/src/rustic_maps.udl`, the round-7
navigation extension). Field names below are the payload's field names —
the UI never has to ask what a value means.

### 7.1 The card set

```
[ TURN BANNER  (accent-bordered surface card, top on driven layouts) ]
  distance phrase      composed display text
  ("250 m")            ("Turn left onto the fixture's north street")

[ THEN LINE  (single `text-secondary` body line, hidden after
              arrival / when the contract's upcoming_maneuver is null) ]
  Then: "Turn right onto the fixture's final street"

[ PROGRESS CARD ]
  Distance to maneuver   < TripProgress.distance_to_next_maneuver_meters >
  Distance remaining     < TripProgress.distance_remaining_meters >
  Duration remaining     < TripProgress.duration_remaining_seconds >
  Phase chip             < NavigationPhase: Navigating | OffRoute | Arrived >

[ DEVIATION ALERT (warning strip, shown exactly while a deviation is live) ]
  Off route entered — < DeviationInfo.deviation_from_route_meters > m from
  the route line; guidance is frozen.
```

- **Turn banner** renders `NavigationInstruction.active_maneuver`:
  `distance_phrase` (per `DistancePhraseKind` — `Immediate` renders no
  number), and the text in the priority the contract defines:
  `composed_display_text` (provider text never rewritten, only prefixed)
  — falling back to `provider_display_text` — and for
  `ManeuverInstructionTextKind.Unavailable` the banner must say so honestly
  ("No instruction text provided"). A roundabout
  (`roundabout_exit_index != null`) appends "exit <index>".
- **Then line** renders `upcoming_maneuver.instruction` (display channel),
  suppressed while null.
- **Progress card** renders `TripNavigationState.progress` verbatim
  (meters and seconds, formatted per unit system on native; the demo is
  metric). Frozen while `OffRoute` — the payload itself is the frozen
  last-on-route projection and the UI shows the frozen values as-is.
- **Phase chip** renders `NavigationPhase` as text, never as a color alone.
- **Arrival state**: `phase === "Arrived"` (equivalently `is_arrival`) swaps
  the banner for the arrival card: `positive`-stated "Arrived", remaining
  progress rendered as zero with the distance bands' zero phrase, then line
  hidden.

### 7.2 v2 placement (native refapps)

The card set and payload bindings above are unchanged by the v2 revision;
only their docks move:

- the turn banner docks top-center on the map (full-width at phone width),
  the then line directly under it, the deviation alert under that;
- the progress card's numbers appear as the navigation drawer's
  ETA-distance line, the phase chip in the drawer's state slot;
- the arrival state replaces the drawer's contents (the banner stack
  swaps to the arrival card per §7.1);
- the error strip and readiness chip keep their §1 docks throughout.

The web playground renders the same card content in its v1 sidebar layout,
from the live trip snapshot (§7.4); its Playwright assertions (§10.1) lock
that v1 form, and the M5 native suites assert these v2 docks.

### 7.3 Live-data wiring (per platform)

The payloads arrive two ways: read-only queries
(`Navigator.current_trip_state() /
current_instructions()`) and the per-topic listener
(`NavigationListener`: `on_navigation_instruction`,
`on_deviation_status_change`, `on_navigation_phase_change`). The refapps
*drive* the UI from the listener topics (the contract's broadcast is the
data source) and use the read-only queries only to seed an
initial state. Re-entrancy is the SDK's concern (the broadcaster's
single-in-flight guard); listeners never re-enter the controller.

### 7.4 Wasm status (live since round 43)

The `Navigator` is exposed on the wasm seam as a **driving and readback**
subset: `startNavigation`, `updateLocation`, `applyReroute`,
`stopNavigation`, `currentTripState`, plus a wasm-only `hasActiveTrip`.
Since round 61 the seam also models the contract's foreign
`NavigationListener` topics over wasm-bindgen:
`addNavigationListener(callback)` receives `instruction` events for every
dispatched fix, `phase` events naming the native transition, `deviation`
updates, `speech` events carrying the contract's speech text, and the
wasm-only `stop` event; `removeNavigationListener(id)` unregisters. Each
event carries the same plain trip snapshot `currentTripState` answers, so a
consumer can drive the card from the listener instead of polling. The
reroute-recalculation members remain native (they are driving calls, not
events); the frozen UDL is untouched, and `contract_sync.mjs` asserts the
wasm class against the UDL `Navigator` members it serves, in both
directions, with the event bridge documented as wasm-only.

The web refapp's "Simulated navigation" card is therefore **live**: the
same fix that moves the follow camera is delivered to the navigator, and
every row — phase chip, distance phrase, provider banner text, the `Then:`
line, the three progress rows and the deviation alert — comes from the
returned trip snapshot. The two banner lines mirror the native cards: the
phrase line carries the distance (`distancePhraseText`), the banner shows
the provider's own sentence (`providerDisplayText`, falling back to the
composed text when the provider authored none) so the distance is not
printed twice. Without a planned route the card is idle and says so; a
route the navigator refuses shows the refusal in the reason strip. No
sample values are rendered anywhere.

### 7.5 Deviation alert semantics

The deviation alert is driven by the deviation topic
(`DeviationStatusChange`): `OffRoute` kind shows the strip with the
`deviation_info.deviation_from_route_meters` distance; `OnRoute` clears it.
The phase topic's `OffRouteEntered`/`OffRouteCleared` transitions and the
phase chip (the drawer's state slot per §7.2) carry the same truth; the
strip is the *narrative* form
(entered/cleared with a reason), the chip is the *state* form. A reroute
(`RerouteApplied`) never shows as an error — it is a normal state
transition the phase chip reflects; only `is_rerouting_required` on the
instruction payload keeps the frozen banner's warning state visible.

## 8. Uniformize: how each platform consumes the tokens

- **Web (CSS)** — the table lives as CSS custom properties under `:root`
  (dark) overridden under `:root[data-theme='light']`; tokens keep their
  exact token names as variable names (`--surface-color`,
  `--text-primary-color`, `--type-body-size`, `--panel-padding`, …). The
  canonical, complete token block is the one in
  `platform/web/styles/playground.css`; the platform refs copy it.
- **Kotlin/Compose** — one `MaterialTheme(colorScheme = …, typography =
  …)` composed from the table: `surface`/`onSurface` ← `surface` +
  `text-primary`, `outlineVariant` ← `surface-border`, `primary` ←
  `accent`, `error` ← `danger`, error-adjacent warning/positive map to
  themed secondary colors *before* any Material default is used. The
  spacing/radius table becomes named constants in a `ReferenceAppUXDimensions`
  object; `type-maneuver*` map onto the headline slot. No other color slot
  is themed.
- **Swift/SwiftUI** — the table becomes asset-catalog colors and text
  styles (any appearance variant), exposed through a `ReferenceAppUXColor`
  and `ReferenceAppUXFont` extension; radii/spacings become an
  `AppLayout` enum of named constants. SwiftUI `@Environment(\.colorScheme)`
  is driven by the refapp's explicit theme state, not the system.
- **React Native (future wave)** — the same table as a `StyleSheet` of
  tokens behind a theme hook; the mapping notes above apply directly.
  (Deferred work: none blocking — the table above is all RN needs.)

Rule: **a token exists if and only if it is in this table.** A new named
constant appears in all platforms in the same change (add to the table
first).

## 9. Shared demo-scene artifact

`platform/reference-scenes.json` is the language-neutral demo content every
refapp loads, so the three apps present identical demo data. Provider
descriptors live in the separate shared registry `platform/providers.json`
(`platform/providers.json`), not this artifact. Sections:

| Section | Contents |
| --- | --- |
| `camera_presets` | the Sacramento demo locations: id, label, and the full camera (latitude/longitude/zoom/bearing/pitch in degrees) |
| `overlay_scenes` | the route/polygon/markers/traffic demo scenes keyed by their scene key, each with `label`, `description`, the wasm `set_overlays` kind string, and coordinates (`latitude_degrees`/`longitude_degrees`) |
| `navigation_demo` | the demo trace reference (which recorded synthetic trace drives the simulated trip, with its fixture path and provenance) and the wasm seam status (available since round 43); `layout_preview` is retained as historical sample data and no longer rendered |
| `demo_plans` | the recorded-walk picker rows in display order: `scenario`, `fixture_path` (the bundled trace derives its asset name from it), the consumer-facing `label`, and the info-affordance `description` |
| `demo_routes` | every ordered pair of the five demo places (20 records): `origin_name`, `destination_name`, the two coordinates, and the provider's contract-shaped `Route` between them, recorded with the shared route options (pedestrian, fastest). The keyless planner answers any of these pairs from the recording and refuses any other pair honestly; no key appears anywhere in the file |

Consumption rules:

- **Web**: `overlay_scenes.mjs` imports the artifact (JSON module) and
  builds the wasm descriptors; the playground and `golden.mjs` both consume
  it unchanged. The trip planner reads `demo_routes` keyless and draws the
  recorded route's step shapes through the same overlay descriptors the
  live path produces. `serve.mjs` serves the artifact at
  `/reference-scenes.json` (the only address allowed outside the web root).
- **Kotlin/Compose + Swift/SwiftUI**: the file is bundled as an app asset
  and decoded at first launch; the camera presets feed the refapp's preset
  picker, the overlay scenes feed the SDK overlay API once the
  `MapSdk`/overlay exposure per platform is available to it, the
  navigation section feeds the simulated trip (loading the referenced
  synthetic trace through the SDK's trace format on native, where the trace
  loader exists), and `demo_plans` builds the recorded-walk picker's rows —
  labels, descriptions and replay order are never restated app-side.
- **RN**: the file ships in the Turbo-Module example app bundle and is
  `require`d directly (JSON is a supported Metro asset); `demo_plans` builds
  the picker rows exactly as on native.
- The artifact's scene geometry is mirrored coordinate-for-coordinate from
  `crates/rustic-maps-rendering/tests/overlay_golden.rs` so the native golden
  fixtures, the browser golden compare and all refapps render the same
  scenes. When a scene changes there, `platform/reference-scenes.json`
  changes in the same change.

## 10. The wasm playground as the web refapp

`platform/web/index.html` implements this language on the web seam and is
the refapp of record for the wasm target. **It deliberately keeps the v1
floating-panel interface of §4.3 — v2's sheet-and-drawer revision (§1,
§4.1–§4.2) applies to the native refapps — while every v2 consumer flow is
present inside that layout (plan flow, route summary, live guidance with
the on-map turn banner, demo plans, offline route, theme and base map).**
Its remaining seam asks are listed in §12. It implements:

- Design tokens exact per §2 (dark + light), theme toggle per §3.
- The Settings sheet keeps Provider credentials and Theme; base maps live in
  the map layers panel on the native shells and in this page's settings
  sheet.
- The primary panel with the plan flow (origin then destination, explicit
  Plan route), the offline-maps card, demo plans, camera presets, scene
  overlays (from the shared artifact, disabled-with-reason per §6.2) and
  Debug. The vector base is installed through the canvas
  (`MapView.setVectorTileSource`), which fetches, styles and *labels* the
  tiles itself with the SDK's embedded font — the page loads no glyph range
  and composes no vector geometry. Simulated navigation keeps its control
  card (phase, voice, the drawer's one ETA-distance line) while the turn
  banner, then line and deviation alert sit at the map's top-center per
  §1/§7.1.
- Status line / readiness ladder / error report per §6; readout per §6.1;
  attribution docked per §1.
- The canvas 2D mirrors per §2.2 (literal constants with token-mapping
  comments; theme-aware degraded scene).
- The renderer-mode, frame-statistic and readiness window hooks stay
  as the UX-test host contract (§10.1).

### 10.1 UX-test host contract

The Playwright suite locks the language:

| Locked behavior | Hook |
| --- | --- |
| Theme toggle flips the root attribute | `#theme-toggle-control`, `html[data-theme]` |
| Readiness ladder resolves to a state + mode text | `#status-line[data-readiness]`, `#harness-status` |
| Scene overlays resolve to enabled OR disabled-with-reason only after readiness | `#scene-overlay-controls`, `#scene-overlay-reason` |
| Camera preset applies the shared artifact's camera | `#camera-preset-controls [data-camera-preset-id]`, `window.mapCameraState` |
| Simulated navigation presents the pending-seam notice + labeled preview | `#navigation-demo-panel`, `#navigation-demo-reason`, `navigation-demo-card` |
| Phone-width layout stays inside one scroll axis | default viewport + a 360px-width pass |
| Keyboard map model: focus, pan/zoom/rotate/tilt/Home, spoken status | `#map-overlay[role=application]`, `#map-status`, `window.mapCameraState` |
| Skip link and focus rings reach every control | `.skip-link`, `#controls[tabindex]`, computed `outline` |
| Mobile-demo tablist follows the ARIA tabs pattern | `[role=tablist]`, `[role=tabpanel]`, roving `tabindex` |
| Offline-maps card plans over the committed routing graph | `#offline-maps-controls`, `#offline-route-control`, `#offline-route-result` |
| Camera presets ease every pose component to the artifact pose | `#camera-preset-controls [data-camera-preset-id]`, `window.mapCameraState` |
| Navigation card and trip card carry the drawer's one ETA-distance line | `#navigation-eta-line`, `#navigation-eta-distance-value`, `#navigation-eta-duration-value`, `#trip-navigation-eta-line` |
| The turn banner sits at the map's top-center during guidance | `#navigation-banner-overlay`, `#banner-instruction-text` |
| The base-map flow lives in the web settings sheet; the native shells keep it in the map layers panel | `#settings-dialog #base-map-controls` |
| Device positioning uses the browser source and the Rust controller | `#device-location-control`, `window.mapDeviceLocationState` |
| Device navigation follows the planned route from device fixes | `#device-navigation-control`, `window.mapDeviceLocationState().isNavigating`, `#navigation-banner-overlay` |
| Demo plans render the artifact plans and replay a recorded trace | `#demo-plans-controls`, `#demo-plan-chips [data-demo-plan-scenario]`, `window.mapDemoPlanState`, `window.mapNavigationSnapshot` |
| Voice guidance speaks the snapshot's speech channel | `#navigation-voice-control`, `window.mapNavigationSnapshot().instruction.activeManeuver.speechText` |

Playwright's `globalSetup` also runs the platform-parity floor
(`platform/web/tests/platform_parity_gate.mjs`): binding-generation paths,
the UDL interfaces the apps consume, the wasm capability floor, the
three-way Auto-default theme on all four surfaces, the keyboard/ARIA markup
above, the iOS registry mirror and every registry fixture path. It is a
fail-closed source check, not a behaviour proof.

The native refapp UI-test suites (Espresso/XCUITest/Detox, M5) assert the
same behaviors against *their* platform equivalents of these hooks; the
table above is the shared assertion list those suites should mirror (the
v2 mappings: the readiness chip ↔ status line, the diagnostics sheet ↔
readout, the home/search/offline sheet controls ↔ panels, the navigation
drawer ↔ the preview's card set).

## 11. What is deliberately out of scope here

Refapp features that PRD §4.9 lists but that the wasm seam does not expose
yet: reverse geocoding and offline pack download. Forward
geocoding, autocomplete and settled search, two-waypoint routing with the full
route options object, distance matrix, traffic incidents/flow and offline
routing from archive bytes are live on the seam (M13/M19/M31;
`platform/web/tests/live_provider.spec.mjs` and the offline-routing suite
exercise them). The missing panels' *presentation*
is fixed by this language (§4's button/card/reason/error components + §6.3),
but their data awaits the `MapSdk` exposure on the wasm seam (Kotlin and
Swift bind it already; see §12 and the four-platform parity audit).

Two `MapSdk` members are **native-only by design**, not gaps to close: the
raster-tile cache terms/TTL, because the browser target has no persistent tile
cache (PRD §4.7 — an IndexedDB store is a separate backlog item), and
`latest_provider_failure_diagnostics`, whose native classifier is not exported
by the wasm seam; the browser's diagnostics readout shows the provider-request
events the seam already records. The browser also cannot show the native offline card's tile
cache/tile accounting, because it has no disk cache: its card states the
recorded-graph route and its provenance, which is its whole offline story.
Structured `SdkError` variants on the wasm throw path remain an open parity
slice (the seam throws verbatim strings today).

**The navigation demo standard.** Every refapp demos the navigation simulation
end to end within this language: a search flow for origin and destination
points, a calculated route revealed on the map with a confirmation control,
and the navigator launched on that route with §7's card set driven by the live
listener topics. The platform TTS engine speaks the contract's *speech* text
verbatim, never paraphrased; the §6.3 verbatim-echo rule applies to spoken
guidance too. An offline maps or cache demo presents through §6.2's
disabled-with-reason and the readiness ladder: an offline state is a *state*,
never a hidden fallback. Everything visual stays inside the §2 token table; no
per-platform invention.

## 12. Open asks to the contract / seam owners (deferrals)

1. **`Navigator` on the wasm seam** (`rustic-maps-web-bindings`) — **resolved round 43**
   for the driving/readback core: the card set (§7) renders live snapshots.
   Still open: the `NavigationListener` subscription topics and the speech
   and reroute-recalculation members, which need a wasm foreign-listener
   contract. The `TripNavigationState`/`NavigationInstruction` projections
   are mirrored in `platform/web/generated/rustic_maps.d.ts`.
2. **`MapSdk` on the wasm seam**: geocode search, route planning and
   offline pack download have no wasm surface; their refapp panels are
   specified but unimplemented on web. Ask: expose `MapSdk` with its async
   provider surface.
3. **Camera animation across a preset (`fly_to_animation`) on the wasm
   seam**: presets today jump instantly on web. Ask: an animated apply on
   the wasm camera path or a documentable instant-apply contract.
4. **The idle render-state signal + `snapshot()`** (`rustic-maps-rendering`
   backlog): the readiness ladder's "Ready" currently keys off the
   reported renderer mode and the first settled frame statistics; a real
   idle signal would replace the frame-count heuristic. (Slice 0/heads
   own this.)