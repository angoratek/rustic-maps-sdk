# Reference-application behavior

Status: landed on Android, iOS, React Native and the web demo. Date: 2026-11-03.
The flow is data in `platform/reference-flows.json`, and
`scripts/check-reference-flows.mjs` gates it against the four applications.

## What this gives you

Every reference application runs one scripted flow with the same order, the
same words and the same visible outcomes on Android, iOS, React Native and the
web demo:

1. Open the app on the shared demo map.
2. Search an origin.
3. Search a destination, or fill either field with **My location**.
4. Plan the route with one explicit action.
5. Read the route card: distance, duration, mode and strategy.
6. Start simulated driving.
7. Follow the turn banner, the ETA-distance line and the speed controls.

The flow itself is data. `platform/reference-flows.json` names each step, the
action a tester performs, the outcome the tester should see, the copy ID from
`platform/reference-tokens.json`, and the per-platform automation hook. One
checker proves the data agrees with the four applications, and the same file
drives the manual screenshot scenario.

Two cross-cutting rules make the flow trustworthy:

- **Diagnostics are opt-in.** No SDK event subscription and no rendering log
  reaches a console or a visible pane until the visitor opens the diagnostics
  surface. The stream is bounded and cleared per session.
- **Provider rows read the registry.** A provider is always rendered as its
  display name plus its credential state (`MapQuest · Needs API key`), never as
  a bare name or a bare state. Keyless routing providers use the public demo
  address the descriptor declares; an unreachable server is reported verbatim
  with no silent fallback.

## What you need

| Requirement | Source |
| --- | --- |
| Consumer copy and behavior constants | `platform/reference-tokens.json` |
| Demo places, routes, plans and camera presets | `platform/reference-scenes.json` |
| Provider display names, credential labels, public demo addresses | `platform/providers.json` |
| The design language and component vocabulary | `docs/ReferenceAppUX.md` |
| The pure reference-application math | `crates/map-refapp-core` |
| The four application shells | Android, iOS, React Native and the web playground |
| One device per native target | Android Pixel 9 emulator, iOS iPhone 17 Pro simulator |

## What exists today

### The shared artifacts

The four applications already share three machine-readable artifacts. The
flow schema is the fourth and follows the same copy rules:

| Artifact | Android | iOS | React Native | Web |
| --- | --- | --- | --- | --- |
| `reference-scenes.json` | Gradle copy task into app assets | `project.yml` resource | Metro `require` in `platform/rn/app-shell/src/referenceScenes.ts` | imported by `platform/web/overlay_scenes.mjs` and served at `/reference-scenes.json` |
| `reference-tokens.json` | Compose mirror checked by `scripts/check_ux_parity.mjs` | SwiftUI mirror, same gate | `platform/rn/app-shell/src/theme.ts`, same gate | `platform/web/ui_strings.mjs` and CSS, same gate |
| `providers.json` | Gradle copy into app assets | `project.yml` resource | Metro `require` in `platform/rn/app-shell/src/providerRegistry.ts` | served at `/providers.json` |
| `reference-flows.json` (new) | Gradle copy into app assets | `project.yml` resource | Metro `require` | served at `/reference-flows.json` |

### The flow, step by step

The table names the current implementation for each step. The audit below it
records the divergences; the rest of the document fixes them.

| Step | Android | iOS | React Native | Web |
| --- | --- | --- | --- | --- |
| Open | `ReferenceApplicationScreen` + `HomeSheet` peek | `ReferenceApplicationView` + `HomeSheetPeekContentView` | `App.tsx` home sheet peek | `#trip-planner` in the v1 panel |
| Search origin | `PlanSearchView` → `LabeledSearchField` "Origin" + per-field Search | `HomeSheetSearchContentView.searchFieldRow` for `.origin` + Search + My location chip | `PlanSearchView` labelled field + inline Search + My location chip | `#trip-origin-input` + `#trip-origin-search-control` + `#origin-my-location` |
| Search destination | same, "Destination" | same, `.destination` | same, "Destination" | `#trip-destination-input` + `#trip-destination-search-control` + `#destination-my-location` |
| Assign a result | `SearchPlaceResultRow` → `assignSearchResult` | result rows → `assignSearchResult` | result rows → `assignSearchRow` | `render_trip_results` → `assign_trip_place` |
| My location in a field | **Missing.** Only `DeviceLocationGroup` starts the global source. | `MapLayersPanelState.myLocationChipTapped(for:)` fills one field | `fillPlanFieldWithMyLocation` fills one field; the chip marks it and disables with the panel's reason | `my_location_field_fillers` fills one field |
| Plan route | `planRouteFromPlanFields`; `canPlanRoute` requires two **resolved** places | `planRouteFromFields`; `canPlanRoute` requires text, then resolves assigned place → exact demo place → first provider result | `planRouteFromFields`; text-gated; same resolution order; keyless recorded route unless a routing provider is registered | `resolve_trip_endpoint`; selected place → exact demo place → keyed first result |
| Route card | `RouteSummaryCard` | `RouteSummaryCardView` | `RouteSummaryCard` (facts, no destination title) | `#trip-route-result` + ETA lines |
| Start driving | `startSimulatedTrip`; planned route or selected demo plan | `startSimulatedJourney`; planned route or sample journey | `startSimulatedNavigation` | `#follow-demo-start-control` |
| Turn banner | `TurnBannerStack` | `NavigationTurnBannerView` + `NavigationThenLineView` | `TurnBannerStack` | `#navigation-banner-overlay` |
| ETA line | `NavigationDrawer` `formatEtaLine` | `NavigationDrawerView.etaDistanceLine` | `NavigationDrawer` `drawer-eta-line` | `#navigation-eta-line`, `#trip-navigation-eta-line` |
| Speed controls | `NavigationSpeedControl` | `NavigationDrawerView.playbackSpeedRow` | `NavigationDrawer` `navigation-speed-row` | `#follow-demo-speed-controls` |
| Provider selection | `CapabilitySettingsSection` → `CapabilityProviderSettingsRow` | `CapabilityProvidersSectionView` → `CapabilityProviderRowView` | `capabilityProviderState.ts` rows → `CapabilityProviderRowView` composed `{name} · {state}` | **Missing.** Only the credential rows exist. |
| Diagnostics | `ReadinessChip` → `DiagnosticsSheet`; Info on open | `ReadinessChipView` → `DiagnosticsSheetView`; Debug on open | `readiness-chip` → diagnostics sheet | `Advanced` → `#diagnostics-controls`; Debug subscription at startup |

### Android

Files: `platform/android/reference-application/src/main/kotlin/com/rusticmaps/sdk/referenceapplication/`.

- `ReferenceApplicationScreen.kt` implements the v2 sheet archetype:
  `HomeSheet`, `HomeSheetPeekContent`, `PlanSearchView`, `RouteSummaryCard`,
  `DeviceLocationGroup`, `NavigationDrawer`, `TurnBannerStack`, `ReadinessChip`,
  `ProviderSettingsSheet`, `CapabilitySettingsSection`,
  `CapabilityProviderSettingsRow`, `DiagnosticsSheet`.
- `ReferenceApplicationState.kt` holds the flow state. `runPlanSearch` filters
  `artifact.demoPlaces` when keyless or calls `forwardGeocode` when keyed;
  `assignSearchResult` fills the field that owns the result list;
  `planRouteFromPlanFields` plans only when `canPlanRoute` is true, and
  `canPlanRoute` is `originPlace != null && destinationPlace != null`.
  `startSimulatedTrip` drives `activeSimulatedTrace`, which is
  `plannedRoute?.simulatedTrace ?: selectedDemoPlan.trace`.
- The plan fields carry visible `Origin` and `Destination` labels and a
  per-field `Search` action (`LabeledSearchField`). They have **no My location
  chip**, and there is no swap action.
- `CapabilityProviderChoice.pickerLabel` returns `"Needs %s"` when the provider
  needs a credential and the display name otherwise. `CapabilityProviderRow`
  renders `servingStatusText`, which is the display name or the raw key. The
  two together produce the bare-name/bare-state mix the report describes.
- Diagnostics: `engageDiagnosticsEventStream` subscribes at `SdkLogSeverity.INFO`
  when the sheet first opens. Separately, `MainActivity.kt`,
  `ReferenceApplicationState.kt`, `SimulatedTripDriver.kt` and
  `SpeechGuidanceSpeaker.kt` write `android.util.Log` lines on startup, trip
  start, reroute and speech. Those lines are always on and are the Android
  source of the reported log noise.

### iOS

Files: `platform/ios/reference-application/Sources/`.

- `HomeSheetView.swift` (`HomeSheetPeekContentView`) and
  `SheetContentComponents.swift` (`HomeSheetSearchContentView`,
  `RouteSummaryCardView`) implement the same archetype.
- `HomeSheetSearchContentView.searchFieldRow` gives each field a Search button
  and a `myLocationChip`; `MapLayersPanelState.myLocationChipTapped(for:)`
  starts the device source when needed and fills only that field.
- `ReferenceApplicationModel.planRouteFromFields` accepts typed text and
  resolves each field in order: the assigned place, an exact demo-place name,
  then the provider's first geocoding result (`resolvePlanField`). This is the
  behavior the report calls smooth.
- The fields have no visible `Origin`/`Destination` labels and there is no swap
  action.
- `CapabilityProviderState.swift` renders a row with `servingText` (the display
  name only) and a picker whose `pickerLabel` is either the display name or
  `"Needs \(credentialLabel)"`. Selecting OSRM or Valhalla calls
  `applySelection`, which passes `credential = ""` for a keyless provider.
  `provider_catalog.rs` reads that empty credential as "use the crate default",
  so the request goes to `http://localhost:5000` or `http://localhost:8002`.
- Diagnostics: `ReferenceApplicationConfiguration.diagnosticsEventsMinimumSeverity`
  is `.debug`, and the feed starts only when `openDiagnosticsSheet()` runs. No
  default console output was found.

### React Native

Files: `platform/rn/app-shell/`.

- `App.tsx` implements the sheet, the search view, the route card, the
  navigation drawer and the turn-banner stack.
- `PlanSearchView` gives each field a visible label with its A/B marker, an
  inline Search action and its own My location chip. The chip fills only its
  field, carries a filled mark while that field holds the fix, and is disabled
  with the device-location panel's reason when the source cannot start. A swap
  action exchanges the two texts, their resolved places and the chip marks.
- Focusing a field moves the assignment target and opens the search view
  without clearing the list the last search filled; the result list belongs to
  the field searched last.
- `canPlanRoute` requires text. Plan route resolves each field in the shared
  order: the assigned place while the field still holds its text, an exact
  demo-place name, then the provider's first geocoding result when a
  credential is applied. Editing a field away from its assigned text clears
  the assignment, so the assigned row stops marking itself and Plan route
  re-resolves from the text.
- Keyless, the plan applies the artifact's recorded route for the resolved
  pair unless a routing provider is registered. A selected keyless routing
  provider registers one with its descriptor's public demo address, so the
  same Plan route action then plans through that provider.
- `capabilityProviderState.ts` composes each provider row as its display name
  plus its credential state. `applyCapabilityProviderSelection` passes the
  session's credential for a credentialed provider and the descriptor's demo
  address for a keyless one, and refuses a keyless descriptor that declares
  none instead of inventing a deployment address.
- Diagnostics open from the readiness chip and call `getDiagnosticsLines()`.
  Failures never mirror to the JavaScript console; they reach the error strip
  and the diagnostics sheet.

### Web

Files: `platform/web/`.

- `index.html` implements the v1 floating-panel web refapp. `#trip-planner`
  has both fields, per-field Search buttons, both My location chips, the
  suggestion group, the reason strip, the explicit `#trip-plan-route-control`
  and `#follow-demo-start-control`, the Directions controls and the result
  list.
- `playground.mjs` resolves each field with `resolve_trip_endpoint` in the
  same order as iOS: selected place, exact demo place, keyed first provider
  result. `refresh_trip_plan_route_state` enables Plan route when both fields
  hold text; the resolution runs on press, and an unresolved field reports in
  the provider status line.
- The settings dialog has provider credential rows, the provider-capability
  rows and the map-control pointer for layers and device location. A
  capability's provider is selected from its own row, so the web demo can
  switch routing to OSRM or Valhalla.
- Diagnostics: `playground.mjs` subscribes through the debug switch
  (`set_diagnostics_debug_enabled`) when the visitor turns it on, not at
  startup. `#readout` stays hidden until the Debug checkbox, and no rendering
  event is recorded before then.

### What the audit means

The platforms agree on the destination (the same map, the same demo data, the
same navigation cards) and disagree on the path:

| Divergence | Effect | Owner |
| --- | --- | --- |
| Android requires a resolved result tap; iOS accepts typed text | Android feels unresponsive and its Plan route stays disabled | Android parity |
| Android has no field My location chips | The fastest way to fill Android's field is missing | Android parity |
| Provider rows split the name and the state across different controls | A row reads `Valhalla` and a menu item reads `Needs API key` | Provider alignment |
| Keyless OSRM/Valhalla receive an empty deployment address | A selected provider routes to `localhost` and fails | Provider alignment |
| Diagnostics subscribe or log by default on Android and web | Rendering events reach a console or a pane without an opt-in | Diagnostics opt-in |
| Web has no provider-capability section | The browser cannot demonstrate per-capability routing | Web parity |

## The one behavior model

### States and transitions

The flow is a small deterministic machine. It is shared as vocabulary, not as
application state: the SDK handles, credentials, cameras, gestures and view
lifecycle stay in each platform's model.

| State | Entry | Allowed events | Exit |
| --- | --- | --- | --- |
| `home_peek` | app launch, trip exit | `open_search(origin)`, `open_search(destination)`, `start_trip` when a plan or demo plan stands | `plan_search` or `navigating` |
| `plan_search` | search view opened | `field_text_changed(field)`, `search_requested(field)`, `result_assigned(field)`, `my_location_requested(field)`, `swap_requested`, `plan_requested`, `done` | `plan_search`, `planning`, `home_peek` |
| `planning` | Plan route accepted | `plan_succeeded`, `plan_failed` | `route_planned`, `plan_search` with the error strip |
| `route_planned` | a Route is present | `start_trip`, `edit_route`, `plan_requested` | `navigating`, `plan_search` |
| `navigating` | trip started | `fix`, `speed_changed`, `pause`, `resume`, `exit`, `arrived` | `navigating`, `home_peek` |
| `arrived` | the contract reports `Arrived` | `exit` | `home_peek` |
| `provider_settings` | settings opened | `provider_selected`, `credential_applied`, `close` | previous state |
| `diagnostics` | diagnostics opened | `debug_detail_toggled`, `clear`, `close` | previous state |

Rules the machine encodes:

- **Assignment never plans.** Only `plan_requested` plans.
- **Plan route is enabled when both fields hold text.** On press it resolves
  each field from an assigned result, an exact demo-place name, or the
  provider's first result. A field that resolves to nothing reports itself in
  the error strip; the button is never dead while the user is typing.
- **One trip at a time.** Starting a simulated trip ends a device-driven trip
  first, and the reverse holds.
- **Start is honest about its source.** When no route is planned, the peek
  starts the selected demo plan and says so; when neither exists, Start is
  disabled with a reason.
- **Diagnostics are an overlay, not a flow state.** Opening diagnostics never
  changes where the user is in the plan or navigation flow.

### The flow schema

`platform/reference-flows.json` is the language-neutral script. Its top level
names the schema version, the artifact purpose and the flows. Every flow names
the prerequisite state, the ordered steps and the screenshot step. Every step
carries one action, one or more visible outcomes, the copy IDs it must render,
and the per-platform automation hook. The reference flow also names the four
published frames, and its `drive` step carries the navigation marker as
conditions: the first instruction is shown and the 1× speed chip is selected.

```json
{
  "schema_version": 1,
  "artifact": {
    "name": "reference-flows",
    "purpose": "The ordered reference-application flows every target must run, with the visible outcome, copy ID and automation hook of each step."
  },
  "flows": [
    {
      "flow_id": "plan_and_navigate",
      "label": "Search, plan and simulate",
      "scenario": "reference",
      "screenshot_step_id": "drive",
      "screenshot_frames": ["plan_sheet", "provider_section", "navigation", "vector_base"],
      "preconditions": ["keyless", "shared artifacts loaded"],
      "steps": [
        {
          "step_id": "open",
          "action": { "kind": "launch" },
          "outcomes": [
            { "kind": "state", "id": "home_peek" },
            { "kind": "copy", "id": "plan.destination_hint" },
            { "kind": "copy", "id": "plan.origin_hint" }
          ],
          "hooks": {
            "android": { "kind": "test_tag", "value": "home-sheet-peek-search" },
            "ios": { "kind": "accessibility_identifier", "value": "home-sheet-peek-search" },
            "react_native": { "kind": "test_id", "value": "home-sheet-peek-search" },
            "web": { "kind": "css_selector", "value": "#trip-destination-input" }
          }
        },
        {
          "step_id": "search_origin",
          "action": {
            "kind": "type_and_search",
            "field": "origin",
            "value": "Capitol Park"
          },
          "outcomes": [
            { "kind": "visible", "id": "origin_result_row" },
            { "kind": "copy", "id": "plan.demo_places_heading" }
          ],
          "hooks": {
            "android": { "kind": "test_tag", "value": "search-view-origin-field" },
            "ios": { "kind": "accessibility_identifier", "value": "search-view-origin-field" },
            "react_native": { "kind": "test_id", "value": "origin-search-field" },
            "web": { "kind": "css_selector", "value": "#trip-origin-input" }
          }
        },
        {
          "step_id": "assign_destination",
          "action": {
            "kind": "assign_result",
            "field": "destination",
            "value": "Tower Bridge"
          },
          "outcomes": [
            { "kind": "condition", "id": "plan_ready" },
            { "kind": "copy", "id": "plan.plan_route_action" }
          ],
          "hooks": {
            "android": { "kind": "test_tag", "value": "plan-search-results" },
            "ios": { "kind": "accessibility_identifier", "value": "search-results-list" },
            "react_native": { "kind": "test_id", "value": "search-results-list" },
            "web": { "kind": "css_selector", "value": "#trip-search-results" }
          }
        },
        {
          "step_id": "plan",
          "action": { "kind": "tap", "target": "plan_route_action" },
          "outcomes": [
            { "kind": "state", "id": "route_planned" },
            { "kind": "copy", "id": "route.start_action" }
          ],
          "hooks": {
            "android": { "kind": "test_tag", "value": "plan-route-button" },
            "ios": { "kind": "accessibility_identifier", "value": "plan-route-button" },
            "react_native": { "kind": "test_id", "value": "plan-route" },
            "web": { "kind": "css_selector", "value": "#trip-plan-route-control" }
          }
        },
        {
          "step_id": "drive",
          "action": { "kind": "tap", "target": "route_start_action" },
          "outcomes": [
            { "kind": "state", "id": "navigating" },
            { "kind": "visible", "id": "turn_banner" },
            { "kind": "visible", "id": "eta_line" },
            { "kind": "visible", "id": "speed_controls" },
            { "kind": "copy", "id": "navigation.speed_label" }
          ],
          "hooks": {
            "android": { "kind": "test_tag", "value": "start-simulated-navigation" },
            "ios": { "kind": "accessibility_identifier", "value": "home-sheet-peek-start-navigation" },
            "react_native": { "kind": "test_id", "value": "start-simulated-navigation" },
            "web": { "kind": "css_selector", "value": "#follow-demo-start-control" }
          }
        }
      ]
    }
  ]
}
```

The example is abbreviated. The real file carries four flows:

| Flow | Purpose | Screenshot |
| --- | --- | --- |
| `plan_and_navigate` | The reference scenario: launch, two searches, plan, drive | yes, at `drive` |
| `my_location_field` | My location fills the origin, then the destination, without planning | no |
| `routing_provider_switch` | Open settings, read the routing row, select OSRM or Valhalla, plan, see the endpoint state | no |
| `diagnostics_opt_in` | Launch with no listener, open the pane, read the bounded stream, toggle debug detail, clear | no |

A step's `hooks` entry is optional only when the platform genuinely has no
surface for that step, and then it carries
`{ "kind": "not_available", "reason": "..." }`. The checker fails on a missing
hook with no reason.

The first adoption adds the stable hooks the schema needs where a platform
lacks one: the peek search row, the two result lists, the plan action and the
simulated-start control. Today Android's peek row and start control, and the
iOS and React Native result lists, have no stable identifier. Adding one is
part of the parity work, not a test-only wrapper.

### The parity checker

`scripts/check-reference-flows.mjs` is the gate. It reads the flow schema,
`platform/reference-tokens.json`, `platform/providers.json` and the four
application sources, and fails on:

1. a copy ID that is not in `ui_labels`;
2. an outcome or component ID that is not in the schema's component vocabulary;
3. a flow or step without a screenshot decision;
4. a platform hook string that does not occur in that platform's declared
   source file;
5. a provider name or base URL literal in a reference-application source that
   is not also the registry's value;
6. a screenshot step that does not show the reference scenario (turn banner,
   route polyline, ETA line, speed controls);
7. a missing `reference-flows.json` copy in any of the four build inputs.

The checker joins the existing gates: `node --test
platform/web/tests/contract_sync.mjs`, the provider-name guard,
`platform/web/tests/platform_parity_gate.mjs`, `node
scripts/check_ux_parity.mjs` and `node scripts/check-docs-references.mjs`. The
Playwright `globalSetup` runs it with the others, so a broken flow cannot reach
a browser test.

### Launch automation

The same file drives the automated runs:

- **Android** gets a launch-intent extra naming the flow ID, mirroring the
  existing `demoWalkthrough` extra. The state records the requested ID, and
  the connected suite — the platform's flow runner — drives the steps
  through the same user-path methods (`openPlanSearch`, `runPlanSearch`,
  `assignSearchResult`, `planRouteFromPlanFields`, `startSimulatedTrip`).
- **iOS** gets a launch argument naming the flow ID, mirroring
  `ReferenceApplicationLaunchAutomation`'s existing keyless plan and journey
  flags. It drives the same model methods.
- **React Native** passes the flow ID through the existing launch intent or
  environment seam and drives the TypeScript state.
- **Web** exposes `window.runReferenceFlow(flowId)` in `playground.mjs`; the
  Playwright suite iterates the schema and asserts each step's outcomes.

The automation is a test seam, not a second implementation. A step that cannot
be driven through a public user path is a defect in the step, not a reason for
a private path.

### Screenshot capture

The set is the six captures per platform that `docs/DocsStyleGuide.md` §5
defines: four flow frames from one revision at phone width and the
layers-control pair. The four flow frames are the plan sheet with both
endpoints chosen, the provider section with every row's display name and
credential state, the `plan_and_navigate` `drive` step at the trip's first
instruction with the 1× playback speed, and the styled vector base with its
place labels. Every run injects one provider credential into the session
through the platform's runtime seam; the published frames and the capture log
never carry it. Captures land under `tmp/reference-screenshots/`; a reviewed
set from one revision is copied into `site/book-src/img/` and updated together.
`docs/ScreenshotCapture.md` holds the exact commands and the revision record.

- **Web**: `CAPTURE_REFERENCE_SCREENSHOTS=1 PROVIDER_API_KEY=… npx playwright
  test tests/capture_reference_screenshots.spec.mjs --workers=1` from
  `platform/web`. The opt-in spec applies the credential through the page's
  settings surface, drives the same controls a visitor uses, and writes the
  four flow frames under `?useProviderFixtures` so no request leaves the test
  harness.
- **Android**: `ReferenceApplicationScreenshotCaptureTest` applies the
  credential from the launch-intent extra (`providerApiKey`), selects the
  styled base through the layers row's own act, and drives the same user-path
  methods as the behavior suite when the instrumentation argument
  `captureReferenceScreenshots=true` is present. The host pulls
  `reference-screenshots/` from the app's external files directory.
- **iOS**: `ReferenceApplicationScreenshotCaptureTest` launches the installed
  application four times with the credential in its launch environment
  (`PROVIDER_API_KEY`), uses the capture launch flags for the plan sheet and
  the sample journey, and attaches the four flow frames to the Xcode result
  bundle. The runner needs the `TEST_RUNNER_` environment prefix.
- **React Native**: `scripts/capture_reference_screenshots.mjs` in the app
  shell launches the Android shell with the `providerApiKey` intent extra and
  drives the same controls through `uiautomator dump` and `adb shell input`,
  capturing with `adb exec-out screencap -p`.

## The shared Rust core

### Crate boundary

The behavior model keeps its pure decision math in one place: a pure crate,
a separate refapp-only binding crate, and no SDK dependency.

| Crate | Contents | Built into |
| --- | --- | --- |
| `crates/map-refapp-core` | follow-camera state machine, circular course median, smoothstep and exponential easing, scale-bar ladder, trace cursor, flow-state reducer | `rustic-maps-web-bindings` directly, and `map-refapp-native` |
| `crates/map-refapp-native` | `crates/map-refapp-native/src/map_refapp_core.udl`, FFI records, converters to and from `map-refapp-core` | refapp-only Kotlin and Swift targets |

The pure crate has no I/O, no clock, no logger and no SDK type. Its public
surface exchanges primitives and crate-local enums only. The wasm build pays
no UniFFI scaffolding; the native binding crate is the only UniFFI consumer.

### What moves

| Concern | Current copies | Moves to the crate |
| --- | --- | --- |
| Follow-camera phases and zoom ramp | `platform/android/reference-application/src/main/kotlin/com/rusticmaps/sdk/referenceapplication/map/FollowCamera.kt`, `platform/ios/reference-application/Sources/MapControlsSupport.swift`, `platform/web/map_controls.mjs` | yes |
| Circular course median | `CourseMedianSmoother` on Android, the sorted-middle on iOS, the circular median on web | yes, one circular median |
| Smoothstep and exponential easing | `platform/android/reference-application/src/main/kotlin/com/rusticmaps/sdk/referenceapplication/map/CameraEasing.kt`, `platform/ios/reference-application/Sources/MapControlsSupport.swift`, `platform/web/map_controls.mjs` | yes |
| Scale-bar 1-2-3-5-10 ladder | `platform/android/reference-application/src/main/kotlin/com/rusticmaps/sdk/referenceapplication/map/ScaleBarModel.kt`, `platform/ios/reference-application/Sources/MapControlsSupport.swift`, `platform/rn/app-shell/src/mapControls.ts`, `platform/web/map_controls.mjs` | yes |
| Trace cursor and dispatch interval | Android `SimulatedTripDriver`, web follow loop | yes, the cursor; the timer stays platform-side |
| Flow-state reducer | none, four ad-hoc implementations | yes |

### The flow-state reducer

The reducer is a pure function:

```
FlowTransition advance_flow(FlowState state, FlowEvent event)
```

`FlowState` and `FlowEvent` are the vocabulary in the state table above. A
`FlowTransition` carries the next state, an outcome ID matching the schema's
step ID, and the copy IDs the platform must render. The reducer is total:
every event from every state returns either a new state or an explicit
`unchanged` transition, and a property test proves no event panics.

The reducer is **not** the application state machine. It does not own the SDK
handle, the route record, the camera pose, the credential store, the speech
engine or the view lifecycle. It answers one question: given the flow state
and an event, what should the user see next? Each platform keeps its own model
and calls the reducer at the same points it already handles the event.

### wasm adoption

`rustic-maps-web-bindings` exports the crate directly. The page deletes the
mirrored math from `platform/web/map_controls.mjs` and its own follow state,
and reads the crate's values instead. The Playwright assertions stay
unchanged: the page's hooks and rendered copy are the contract, not the
implementation behind them.

### Native bindings

`crates/map-refapp-native/src/map_refapp_core.udl` declares the synchronous
surface:

- `scale_bar_selection(ground_meters_per_screen_pixel, minimum_width_pixels,
  maximum_width_pixels)`;
- `bearing_off_north_degrees(bearing_degrees)`;
- `trace_dispatch_interval_milliseconds(recorded_sample_interval_seconds,
  playback_acceleration_factor)`;
- the follow-camera state and targets;
- the course median, smoothstep easing, exponential chase and trace cursor.

`advance_flow` stays crate-only: the native shells run their flows through
their own view-model methods, so nothing would consume the reducer across the
UniFFI boundary yet.

`scripts/build-refapp-core.sh` generates Kotlin and Swift from that UDL into
refapp-only targets. Android adopts the whole surface and deletes its copies;
iOS adopts the scale ladder and the circular course median and keeps its
follow-camera zoom policy, which deliberately differs (a user-nudged zoom
persists and there is no end-of-trip ramp), so that behaviour is not a copy of
the crate. Both pin value parity in their own tests. The AAR and XCFramework
file-set assertions exclude both crates, so no demo policy ships in a
published SDK package.

### React Native

React Native keeps a TypeScript mirror of the shared reference-application
math (`locationPoseAnimator`, `followBearingMode`, `frameClock`), because
`scripts/build-refapp-core.sh` generates only Kotlin and Swift. Its contract
records, enums and ordinals come from the generated `rustic_maps` module that
`scripts/generate-rn-bindings.sh` produces (`scripts/build-rn.sh` runs it).
`check-reference-flows.mjs` plus
`check_ux_parity.mjs` pin the constants and the flow vocabulary against the
schema.

### What stays platform-side

Views and layout, gesture plumbing, camera-object construction, surface
attach/resize, the trace timer, TTS and audio focus, speech dedup and flush,
credentials, lifecycle, launch automation and diagnostics rendering. These are
the platform's product surface and its UI-test host, not shared math.

## Diagnostics policy

### Defaults

The SDK's diagnostics sink stays as it is: one process-wide bounded ring per
subsystem (`DIAGNOSTIC_RING_CAPACITY = 512`), an idle gate at
`DIAGNOSTIC_IDLE_RECORDING_SEVERITY = Info`, and no console writer. The ring
is in-memory only and is never printed.

The reference applications add one rule above it: **no subscription and no
console output before the visitor asks for diagnostics.**

| Target | Before this change | After |
| --- | --- | --- |
| Android | sheet subscription at Info on first open; `Log.i`/`Log.w` lines from startup, trip start, reroute and speech | no subscription before open; no SDK/rendering `Log.*`; failures go to the error strip |
| iOS | sheet subscription at Debug on first open; no console output found | unchanged subscription timing; no console output |
| React Native | sheet query on open; nine `console.log` failure mirrors | no console output; failures go to the error strip |
| Web | `subscribeToDiagnosticEvents(Debug)` at startup, before the closed Advanced panel opens | subscription only when the diagnostics pane opens |

### The opt-in

One visible door per target, in the existing component vocabulary:

- **Native:** the readiness chip opens the diagnostics sheet.
- **Web:** `Advanced` → `#diagnostics-controls`.

Opening the door does two things: it starts the live subscription at `Info`
and it flushes the bounded backlog. Inside the pane, one explicit **Show debug
events** switch lowers the subscription to `Debug`; the SDK gate never raises
back, so the detail is per-session and bounded. `Clear` empties the pane's
display list. Closing the pane stops the refresh cadence; it does not stop the
already-lowered gate, which is why the state is per-session and documented in
the pane.

The opt-in is not persisted. A relaunch starts with diagnostics off.

### The bounded stream

| Value | Native | Web | Source |
| --- | --- | --- | --- |
| SDK ring capacity per subsystem | 512 | 512 | `DIAGNOSTIC_RING_CAPACITY` |
| Display window | 100 events | 100 events | shared `diagnostics-event-window` |
| Backlog flush limit | 100 | 100 | shared `diagnostics-backlog-flush-limit` |
| Refresh cadence | 500 ms | 500 ms | shared `diagnostics-refresh-cadence-milliseconds` |
| Default pane severity | Info | Info | shared `diagnostics-minimum-severity` |
| Debug detail | explicit switch | explicit switch | shared `diagnostics-debug-severity` |

The numeric values land in `platform/reference-tokens.json` as behavior
tokens, so the four mirrors cannot drift. The Android and web constants that
differ today (`INFO` vs `Debug`, 20-event web window) move to the shared
values in the same change.

### What is removed

- `android.util.Log` lines that mirror SDK or rendering events, including the
  demo-plan load, trip start, reroute and speech lines.
- The nine `console.log` calls in `platform/rn/app-shell/App.tsx`.
- The web's startup `subscribeToDiagnosticEvents` call.
- Any `print`, `NSLog` or equivalent added later to a reference application's
  SDK path.

A failure that needs the developer still reaches the diagnostics pane through
the SDK's own event stream. A failure that needs the visitor reaches the error
strip. Neither reaches a console by default.

### Failure modes

| Situation | What the visitor sees | Where |
| --- | --- | --- |
| Diagnostics never opened | no pane, no console line, no subscription | all targets |
| Pane opened, no events | `diagnostics.no_events` | pane |
| Debug detail not enabled | Info and above only | pane |
| Stream longer than the window | the most recent window, oldest dropped | pane |
| SDK not started | the readiness chip states the failure; the pane still opens | readiness + pane |
| A platform's console writer remains | the flow checker fails on the call site | gate |

## Provider selection, descriptors and endpoints

### The registry is the only source

`platform/providers.json` owns every display name, credential label,
capability list, attribution line, terms note and public demo address. No
reference application may name a provider or spell a provider URL in its own
source. The existing provider-name guard already enforces the name half; the
flow checker adds the URL half.

The routing descriptors gain two fields:

```json
{
  "provider_key": "osrm",
  "display_name": "OSRM",
  "credential_label": "",
  "served_contracts": ["routing"],
  "demo_base_url": "https://router.project-osrm.org",
  "demo_base_url_note": "Public demo server, rate-limited and not for production use.",
  "terms_note": "A self-hosted OSRM deployment follows its operator's terms; public demo servers are rate-limited and not for production use."
}
```

Valhalla carries the same shape with its own public demo address. The exact
address is an owner decision (open question 1); the schema field is not.

### Row composition

A capability row is one composition, everywhere:

```
Routing
MapQuest · Needs API key            Change
```

The serving line and every picker entry are the same helper:

```
provider_row(descriptor, applied_credential) =
    "{display_name} · {state}"
```

| Condition | State |
| --- | --- |
| The descriptor declares no credential | `Ready` |
| The descriptor declares a credential and the session holds it | `Ready` |
| The descriptor declares a credential and the session lacks it | `Needs {credential_label}` |
| No provider is registered for the capability | `Not configured` |
| A base-map row is the active source | `Serving` |
| A base-map row is selectable but not active | `Select` |

`settings.provider_row` is the copy ID (`{provider} · {state}`). The bare
`Valhalla` / `Needs API key` split disappears. A row never prints a provider
key, a raw contract name, or a state without its provider.

### Demo endpoints

The contract's `configure_provider(contract, provider_key, credential)`
already reads a keyless provider's credential as its deployment address:
`deployment_base_url_or_default` in `provider_catalog.rs` returns the crate
default only for an empty value. The reference applications therefore pass the
descriptor's `demo_base_url` as that argument:

| Descriptor | Argument passed |
| --- | --- |
| MapQuest | the session's applied API key |
| OSRM with `demo_base_url` | the descriptor's `demo_base_url` |
| Valhalla with `demo_base_url` | the descriptor's `demo_base_url` |
| A self-hosted deployment the visitor entered | the entered absolute HTTP(S) URL |
| A keyless descriptor without `demo_base_url` | no selection; the row says why |

No reference application passes an empty string for OSRM or Valhalla. The web
keeps `setProviderBaseUrl` as the test-harness override only; production
selection goes through `configureProvider`. The browser's public demo address
comes from the same descriptor.

The settings row states the address and the descriptor's terms note:

```
Routing
OSRM · Ready                        Change
Demo server: https://router.project-osrm.org
Public demo server, rate-limited and not for production use.
```

### Honest failure

An unreachable server is never hidden and never replaced:

1. The provider's typed failure crosses the contract verbatim. OSRM and
   Valhalla return `NetworkRequestFailed` with the address in the detail.
2. The error strip shows the variant label and the full detail. It stays until
   the next success.
3. The routing row adds the next step from
   `settings.routing_unreachable_next_step`: retry, or enter a self-hosted
   deployment address.
4. The previous provider keeps serving. The SDK validates before it moves a
   slot, so a refused selection never leaves a broken slot.
5. There is no fallback to MapQuest, no fallback to a recorded route and no
   silent retry. The route card stays absent and Plan route re-enables.

### Failure modes

| Situation | What the visitor sees | Slot after |
| --- | --- | --- |
| OSRM/Valhalla demo server unreachable | `NetworkRequestFailed` verbatim, with the address | previous provider |
| Demo server returns a provider refusal | the provider's typed refusal verbatim | previous provider |
| Malformed deployment address | the crate's `NetworkRequestFailed` naming the address | previous provider |
| Keyless provider selected with no `demo_base_url` | `settings.provider_state_not_configured` and the missing-descriptor reason | unchanged |
| Credentialed provider selected without a key | `Needs {credential_label}`; no SDK call | unchanged |
| Provider switch during a trip | the trip keeps its planned route; the next plan uses the new provider | new provider |

## Search, route card and navigation UX

The interaction model follows the search-first shape of Google Maps and Apple
Maps while staying inside the `docs/ReferenceAppUX.md` token and component
system. No new color, spacing or type value is introduced.

### Home and search

- **The peek is the question.** The home sheet's peek is the destination field
  ("Where to?") and one action row. There is no second search entry hidden in a
  menu.
- **Two labeled fields.** The search view shows `Origin` and `Destination`
  labels with A/B markers, the destination placeholder "Search a destination",
  and the origin hint "Search an origin". Each field carries an inline
  **Search** action and a **My location** chip. A **swap** action exchanges the
  two fields and their resolved places.
- **Suggestions are rows.** The result list belongs to the field whose search
  ran last. A row shows the place name, a secondary detail and the quality; an
  assigned row marks itself with `accent`. A keyless session labels the rows
  `Demo places` and refuses an unmatched query honestly. No row is invented.
- **One primary action.** Plan route is the only action that plans. It stays
  disabled with a reason naming the missing field while either field is empty;
  with both filled it resolves on press and reports a field that resolves to
  nothing.
- **Typed text is resolved, not rejected.** Plan route resolves each field in
  the order: assigned place, exact demo-place name, provider's first result. A
  field that resolves to nothing names itself in the error strip. This is the
  iOS behavior the other targets adopt; it removes Android's "tap a result or
  nothing happens" dead end.

### Route card

The route card is the map's companion, not a second map:

```
[ Route ]
Destination name                         14px title
1.4 km · 25 min                           numeric readout
Pedestrian · Fastest                      caption
[ Start simulated navigation ]            primary
Edit                                      subtle
```

- Distance and duration come from the contract's `Route`, formatted per unit
  system. They are never paraphrased.
- The mode and strategy line names the same choices the Directions control
  carries. A recorded demo route says `Recorded demo route, pedestrian,
  fastest.` instead.
- Start is primary; Edit returns to the search view with both fields intact.
- The map draws the route polyline and the A/B pins, framed inside the sheet's
  clear area.

### Navigation

- **Turn banner at the top center.** The distance phrase and the provider's
  instruction text, with the then line under it. The banner is the first thing
  the driver reads.
- **Drawer at the bottom.** ETA prefix, distance remaining, duration remaining,
  phase, speed chips (1×, 2×, 4×, 8×), pause/resume, voice toggle and exit.
- **Speed changes do not teleport.** A speed change re-anchors at the travelled
  distance.
- **Arrival swaps the banner to the positive-stated arrival card** and hides
  the playback controls.
- **Deviation is a state.** The warning strip appears exactly while the
  contract reports `OffRoute`; a reroute is a normal transition, not an error.

### Sheets and hierarchy

- One surface role, one border, one accent. A second accent is a table change
  first.
- Peek is content-driven; half is `sheet-half-height` (0.55 at phone width);
  full is capped at 0.72. Content scrolls inside the sheet; the page never
  scrolls horizontally.
- The sheet's grabber is the only gesture target. The platform back affordance
  collapses toward peek.
- Touch targets are at least 48 dp. The sheet's controls keep
  `gutter-inline` from the viewport edge.

### Copy

Consumer strings exist only in `platform/reference-tokens.json` `ui_labels`.
The flow schema references their IDs, never their text. The behavior model
uses the existing IDs where they exist (`plan.origin_hint`,
`plan.destination_hint`, `plan.destination_placeholder`, `plan.search_heading`,
`plan.plan_route_action`, `plan.planning_in_flight`, `plan.searching_in_flight`,
`plan.empty_results_hint`, `plan.demo_places_heading`,
`plan.demo_no_match_hint`, `plan.recorded_route_note`,
`plan.no_recorded_route_hint`, `route.start_action`, `route.edit_action`,
`navigation.*`, `readiness.*`, `settings.*`).

The behavior model adds these IDs to the artifact in the same change as their
four mirrors:

| New ID | Text |
| --- | --- |
| `plan.origin_label` | Origin |
| `plan.destination_label` | Destination |
| `plan.search_action` | Search |
| `plan.my_location_action` | My location |
| `plan.swap_action` | Swap origin and destination |
| `plan.missing_origin` | Enter an origin to plan the route. |
| `plan.missing_destination` | Enter a destination to plan the route. |
| `plan.missing_both` | Enter an origin and a destination to plan the route. |
| `plan.my_location_unavailable` | My location is unavailable: {reason} |
| `plan.trip_source_demo_plan` | Selected demo plan: {plan} |
| `settings.provider_row` | {provider} · {state} |
| `settings.provider_state_ready` | Ready |
| `settings.provider_state_needs_credential` | Needs {credential} |
| `settings.provider_state_not_configured` | Not configured |
| `settings.routing_demo_endpoint` | Demo server: {url} |
| `settings.routing_unreachable_next_step` | The demo server did not answer. Retry, or enter your own deployment address. |
| `diagnostics.show_debug_events` | Show debug events |
| `diagnostics.hide_debug_events` | Hide debug events |
| `diagnostics.clear_action` | Clear |
| `diagnostics.no_events` | No SDK events recorded yet. |
| `diagnostics.stream_scope_note` | Debug detail stays on until the app restarts. |

A provider display name, a maneuver instruction, a speech sentence, an error
detail and a value format remain outside `ui_labels` under the existing rules.

### Accessibility and motion

- Every field has a visible label and an accessible name; the two My location
  chips name their field ("Use my location as the origin").
- The swap, search, plan, start, speed, pause, voice and exit controls expose
  their state (`aria-pressed`, selected chip, disabled-with-reason).
- The native shells carry the same semantics through TalkBack, VoiceOver and
  React Native accessibility props.
- The web keeps the focus order, the live status region and the reduced-motion
  rule in `docs/ReferenceAppUX.md` §5.4.
- Camera eases and sheet transitions are the platform's motion; a visitor who
  requests reduced motion gets the same outcome without the animation.

## Platform parity matrix

| Capability | Android | iOS | React Native | Web |
| --- | --- | --- | --- | --- |
| Visible Origin/Destination labels | present | add | add | add |
| Inline Search action per field | present | present | add | present |
| My location chip per field | add | present | add | present |
| Swap action | add | add | add | add |
| Typed-text resolution on Plan route | add | present | add | present |
| Explicit Plan route action | present | present | present | present |
| Route card with mode/strategy | present | present | present | present |
| Turn banner, then line, deviation | present | present | present | present |
| ETA-distance line | present | present | present | present |
| Speed chips, pause/resume, voice, exit | present | present | present | present |
| Provider-capability section | present | present | present | add |
| Descriptor-composed provider row | add | fix | fix | add |
| Keyless demo endpoint | add | add | add | add |
| Diagnostics off until opt-in | fix logs | present | fix logs | fix startup subscription |
| Bounded diagnostics window | fix to shared | fix to shared | present | fix to shared |

## Tests and gates

| Layer | What it proves | Where |
| --- | --- | --- |
| Flow schema check | copy IDs, component IDs, hooks, screenshot steps and registry URLs agree with the sources | `scripts/check-reference-flows.mjs` |
| Rust unit and property tests | the flow reducer is total; the follow camera, median, easing, ladder and cursor match the documented values | `crates/map-refapp-core` |
| Android Espresso | the flow steps, My location fill, typed-text plan, provider switch and diagnostics-off default | `platform/android/reference-application/src/androidTest/` |
| iOS XCUITest | the same steps through the same user paths | `platform/ios/reference-application/Tests/ReferenceApplicationUITests/` |
| React Native jest/Detox | the same steps, the provider row composition and the flow vocabulary | `platform/rn/app-shell/__tests__/` |
| Playwright | `window.runReferenceFlow(flowId)` and the rendered outcomes; the diagnostics pane opens with no prior subscription | `platform/web/tests/` |
| UX parity | consumer copy and behavior tokens across the four mirrors | `scripts/check_ux_parity.mjs` |
| Provider guard | no provider name or URL outside its crate, descriptor or dispatch arm | `platform/web/tests/provider_name_guard.mjs` |
| Docs references | every cited document exists and the design note is indexed | `scripts/check-docs-references.mjs` |

The tests never contact the network. A routing-provider test starts a loopback
server over the committed OSRM or Valhalla fixture, or points the descriptor's
`demo_base_url` at the same-origin fixture route. The live public endpoint is
never a test dependency.

## Screenshots and evidence

The screenshot scenario is `plan_and_navigate` at its `drive` step, per
`docs/DocsStyleGuide.md` §5: the turn banner naming the trip's first
instruction, the planned route polyline with its A/B pins, the ETA-distance
line and the playback-speed controls at 1×, in the same camera and the same
trip state.

- Four captures per platform in the order Android, iOS, React Native,
  WebAssembly: the plan sheet, the provider section, the navigation frame and
  the styled vector base.
- Every capture comes from one revision; the revision and the flow ID are
  recorded in the page and in `docs/ScreenshotCapture.md`.
- The platform chrome may differ; the map content, the guidance, the control
  cluster, the scale bar, the drawer/ETA content and the drawn place labels
  may not.
- Recapturing one platform alone is not allowed. A UI change that reaches one
  platform recaptures the set.

The evidence series stays under `tmp/refapp-*`, gitignored, with the failure
series kept honestly.

## Open decisions

These need a product-owner ruling. Each is echoed in the milestone report.

1. **Public demo server terms and rate limits.** May the reference
   applications use `https://router.project-osrm.org` and a public Valhalla
   demo instance as the keyless demo addresses, or must the demo point at
   self-hosted instances with a stated budget? What request rate and daily
   volume are acceptable, and which terms note must the settings row and the
   attribution panel carry?
2. **Default routing in the keyless demo.** Does OSRM or Valhalla replace
   MapQuest as the default routing provider when no credential is present, or
   does the keyless demo stay on the recorded demo routes with OSRM/Valhalla
   as explicit choices? The current recorded-route behavior is honest and
   network-free; a public demo endpoint makes the switch demonstrable but
   spends the owner's rate budget.
3. **Background and foreground policy.** Should a simulated trip pause when
   the app backgrounds and resume on foreground, or continue on a background
   timer? Should device-driven navigation request background location in the
   demo? The demo currently declares when-in-use authorization only and no
   background modes.
4. **Screenshot scope.** Confirm the four navigation captures are the
   published set, and whether the plan sheet, the route card and the provider
   row also get captures (and whether tablets are in scope). A larger set must
   still come from one revision and keep the platform order.
5. **Diagnostics persistence.** Should the Show debug events choice persist
   across launches (and if so, on which platforms), or stay per-session as this
   design assumes? A persisted choice is a stored preference and needs the
   same remembered-choices treatment as the theme.
6. **Deployment address entry.** Keyless providers currently overload the
   contract's credential argument as the deployment address. Should the
   settings surface expose an explicit Deployment URL field for OSRM and
   Valhalla, with the descriptor's demo address as the default, or keep the
   demo address implicit and let self-hosting be an SDK-level integration
   task?

## Where the code lives

| File | Change |
| --- | --- |
| `platform/reference-flows.json` | New artifact: the named flows, steps, outcomes, copy IDs, hooks and screenshot decision. |
| `scripts/check-reference-flows.mjs` | New gate; also called from `platform/web/tests/platform_parity_gate.mjs` and Playwright `globalSetup`. |
| `platform/reference-tokens.json` | New behavior tokens and `ui_labels` rows listed above. |
| `platform/providers.json` | `demo_base_url` and `demo_base_url_note` on the keyless routing descriptors. |
| `crates/map-refapp-core/` | New pure crate: flow reducer plus the M12 math. |
| `crates/map-refapp-native/` | New refapp-only UDL and converters. |
| `scripts/build-refapp-core.sh` | New binding generation script. |
| `platform/android/reference-application/src/main/kotlin/com/rusticmaps/sdk/referenceapplication/ReferenceApplicationScreen.kt` | Field labels, inline Search, My location chips, swap, provider row composition, diagnostics opt-in. |
| `platform/android/reference-application/src/main/kotlin/com/rusticmaps/sdk/referenceapplication/ReferenceApplicationState.kt` | Typed-text resolution, flow-state calls, log removal, endpoint argument, diagnostics subscription on open. |
| `platform/android/reference-application/src/main/kotlin/com/rusticmaps/sdk/referenceapplication/CapabilityProviderState.kt` | `provider_row` composition and descriptor demo address. |
| `platform/android/reference-application/src/main/kotlin/com/rusticmaps/sdk/referenceapplication/MainActivity.kt` | Flow-ID launch extra, startup log removal. |
| `platform/android/reference-application/src/main/kotlin/com/rusticmaps/sdk/referenceapplication/navigation/SimulatedTripDriver.kt` | Log removal. |
| `platform/android/reference-application/src/main/kotlin/com/rusticmaps/sdk/referenceapplication/speech/SpeechGuidanceSpeaker.kt` | Log removal. |
| `platform/ios/reference-application/Sources/HomeSheetView.swift` | Peek's trip-source line and start reason. |
| `platform/ios/reference-application/Sources/SheetContentComponents.swift` | Field labels, swap, My location chips. |
| `platform/ios/reference-application/Sources/ReferenceApplicationModel.swift` | Flow-state calls, endpoint argument, diagnostics opt-in. |
| `platform/ios/reference-application/Sources/CapabilityProviderState.swift` | `provider_row` composition and demo address. |
| `platform/ios/reference-application/Sources/ProviderSettingsView.swift` | Composed serving line and picker labels. |
| `platform/ios/reference-application/Sources/ReferenceApplicationConfiguration.swift` | Shared threshold and copy removal. |
| `platform/rn/app-shell/App.tsx` | Field labels, inline Search, My location chips, swap, typed-text plan, log removal, diagnostics opt-in. |
| `platform/rn/app-shell/src/capabilityProviderState.ts` | `provider_row` composition and demo address. |
| `platform/rn/app-shell/src/providerRegistry.ts` | `demo_base_url` field. |
| `platform/rn/app-shell/src/mapControls.ts` | Retained mirror until the second ubrn module lands. |
| `platform/web/index.html` | Provider-capability section, field labels, swap. |
| `platform/web/playground.mjs` | Diagnostics subscription moved to the pane, flow runner, provider selection from descriptors. |
| `platform/web/map_controls.mjs` | Deleted mirror math after the wasm adoption. |
| `platform/web/serve.mjs` | `/reference-flows.json` route. |
| `site/book-src/screenshots.md` | The one-revision capture set and the flow ID. |
| `docs/ReferenceAppUX.md` | Component vocabulary for the field chips, swap, provider row and diagnostics opt-in. |
