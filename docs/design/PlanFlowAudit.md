# Plan-flow audit

Status: audit, complete. Date: 2026-11-04.

## What this gives you

The React Native plan flow read against the iOS reference pattern, one step at
a time, with the evidence for every verdict and one dated evidence entry per
target. It answers three questions:

- Does the React Native shell run the same plan flow as the iOS reference
  application, with the same fields, the same resolution order and the same
  visible outcomes?
- Where it does not, what is the smallest honest change, and what remains a
  recorded residual?
- Which revision, version string, device and test produced the evidence on
  Android, iOS, React Native and the web?

The flow itself is the shared artifact in `platform/reference-flows.json`, the
design language is `docs/ReferenceAppUX.md` §4.1 and §5, and the behavior
contract is `docs/design/ReferenceApplicationBehavior.md`.

## Evidence header

| Field | Value |
| --- | --- |
| Audit date | 2026-11-04 |
| SDK version | `0.1.0` (workspace `Cargo.toml`) |
| Android shell | `0.1.0` (`versionName`, `platform/android/reference-application/build.gradle.kts`) |
| iOS shell | `1.0` build `1` (`CFBundleShortVersionString`, `platform/ios/reference-application/Sources/Info.plist`) |
| React Native shell | `MapsSdkRefappRn` `0.0.1` (`platform/rn/app-shell/package.json`); package `@angoratek/rustic-maps-sdk` `0.1.0` |
| Web shell | the wasm playground has no independent version; it serves SDK `0.1.0` |
| Base revision | `95a0057` (`main`) |
| React Native fix revision | `a8bba53` (peek trip-source line) and `6b16d0c` (route card and failure-strip tests), on `docs/m44-audit` |
| Audit document revision | the commit that adds this page (`git log -1 -- docs/design/PlanFlowAudit.md`) |

The version strings are the ones the shells report, not a plan number. The
React Native package and the SDK share `0.1.0` because the shell consumes the
package by `file:..`; the shell's own bundle version is `0.0.1`.

## The iOS reference pattern

The iOS reference application is the pattern because it is the shell whose
plan flow the behavior contract describes in its final form: typed text is a
valid input, assignment never plans, and Plan route is the one explicit
action. The pattern below is what the React Native state is measured against.

### 1. Search fields

`HomeSheetSearchContentView.searchFieldRow` renders both fields with:

- a visible label and its A/B marker (`Origin` / `Destination`);
- the field's input with `submitLabel(.search)` and its own inline `Search`
  action;
- the field's own type-ahead list (`SearchFieldSuggestionListView`);
- a `My location` chip that fills only its own field.

The search view's heading is `Plan a route`; `Done` returns to the half sheet.
Each field carries its own inline Search; the result list belongs to the field
searched last (the assignment target follows focus).

### 2. Typed resolution

`ReferenceApplicationModel.resolvePlanField(for:)` resolves a field in one
order:

1. the assigned place, when the field still holds its text;
2. an exact demo-place name, keyless or keyed;
3. the provider's first geocoding result, when a credential is applied.

A field that resolves to nothing names itself in the error strip. Plan route
resolves origin first, then destination, and only then plans.

### 3. Assignment

`assignSearchResult(_:)` fills the assignment target's field, sets its
coordinate and assigned text, and never plans by itself. The assigned row
marks itself subtly; editing the field away from the assigned text clears the
coordinate so Plan route re-resolves from the text.

### 4. Swap

`swapPlanFields()` exchanges the two texts, both coordinates, both assigned
texts and the My location fill state, and clears both type-ahead lists. A swap
never plans.

### 5. Mode and strategy

`DirectionsChoiceState` offers Car and Walk, and Fastest and Shortest for car
plans. Car and Fastest are the defaults; the choices are remembered per device
and sent to the provider with the plan. A keyless recorded route carries its
own recorded profile, which the route note states.

### 6. Route card facts

`RouteSummaryCardView` renders the contract `Route`'s distance phrase and
duration (formatted, never paraphrased), the recorded-route note when the
route is the artifact's, and the primary `Start simulated navigation` and
subtle `Edit` actions.

### 7. Source line

Before a route is planned, the peek's `peek-trip-source` line names the demo
plan Start will replay (`Selected demo plan: …`). After a route is planned the
line is absent: Start now replays the planned route.

### 8. Failure states

A provider refusal is carried verbatim in the error strip above the sheet. An
unresolved field names itself; a keyless pair without a recorded route states
that fact and points at the provider key; a keyless search with no demo match
says so instead of showing a silent empty list.

### 9. Relaunch restore

The remembered record restores the two search texts, the Directions choices
and the per-capability provider choices. It never restores a route result or
an assignment; Plan route re-resolves after a relaunch.

## React Native against the reference

The current React Native shell is `platform/rn/app-shell/App.tsx` with its
pure helpers in `platform/rn/app-shell/src/`. The state below is the shipped
state at the revisions in the evidence header; the older React Native
paragraph in `docs/design/ReferenceApplicationBehavior.md` describes an
earlier tree and is recorded as a documentation residual below.

| Step | iOS reference | React Native state | Verdict | Evidence |
| --- | --- | --- | --- | --- |
| Search fields | visible labels + A/B markers, inline Search, per-field My location chip, heading, Done | `PlanFieldSearchControl` and `PlanSearchView`: `origin-field-marker`/`destination-field-marker`, `planOriginLabelText`/`planDestinationLabelText`, `origin-search-action`/`destination-search-action`, `origin-my-location`/`destination-my-location`, `search-heading`, `search-done` | Match | Jest `the plan fields carry visible labels, inline Search actions and per-field My location chips` |
| Typed resolution | assigned place → exact demo name → provider first result | `resolvePlanField` uses the same order, keyless exact demo name and keyed provider first result | Match | Jest `typed text that is not a demo place resolves through the provider first result`, `typing both demo places and pressing Plan route applies the recorded route`; `keylessPlan.test.ts` |
| Assignment | fills the assignment target, marks the row, never plans | `assignSearchRow` → `assignPlanFieldRow`; keyless fills directly, keyed assigns through the native session first; never plans | Match | Jest `an unresolved keyless field names the field in the error strip`, `typing both demo places and pressing Plan route applies the recorded route`; App `a keyless search offers the artifact demo places and says so` |
| Swap | texts, coordinates, assigned texts, fills | `swapPlanFields`: texts, assigned texts and places, active field, native re-assignment when keyed | Match on fields and places; residual: no My location filled-state to swap | Jest `swap exchanges the two fields and their resolved places` |
| Mode and strategy | Car/Fastest defaults, Walk hides strategy, capability reasons, remembered | `directionsSelection.ts` and `DirectionsControls`: same defaults, capability reasons, remembered; keyed plan sends the selection | Match | Jest `directionsSelection.test.ts` (7 cases); App `the plan sheet Directions default to Car and Fastest and a Walk plan sends pedestrian`, `the Directions choices survive a relaunch through the record` |
| Route card facts | distance + duration, recorded note, Start/Edit | `RouteSummaryCard`: native `distanceText`/`durationText`, recorded note, Start/Edit; the native hosts format the contract route | Match | Jest `the route card renders the native distance and duration facts` |
| Source line | `peek-trip-source` names the demo plan before a route | **was missing**; the string existed unused. Now renders `peek-trip-source` while `!planState.isPlanned` | **Fixed** in `a8bba53` | Jest `the peek names the demo plan Start will replay until a route is planned` |
| Failure states | error strip, named unresolved field, keyless no-recorded-route, verbatim refusal | error strip `error-strip`; `reportUnresolvedPlanField` names the field; keyless no-recorded-route and no-match lines; autocomplete keeps the text with one sentence | Match; residual: the My location chip reports failure through the strip instead of rendering disabled-with-reason | Jest `an unresolved keyless field names the field in the error strip`, `planSearchAutocomplete.test.tsx` failure case; `keylessPlan.test.ts` unrecorded pair |
| Relaunch restore | texts, Directions and provider choices; never a route | `preferences.ts` restores the same record; a route result is never restored; Plan route re-resolves | Match | Jest `a remembered preferences record restores choices without planning a route`, `preferences.test.ts` (7 cases) |

### The one code change

`a8bba53` adds the peek's trip-source line to the React Native shell:
`selectedDemoPlan` is looked up from the remembered plan asset name, and the
line renders while no route is planned, exactly as the iOS
`HomeSheetPeekContentView` and the Android `tripSourceText` do. `6b16d0c`
pins the route card facts and the failure strip so those two steps have direct
React Native evidence, not only source parity.

## Residuals

These are the divergences that remain. None blocks the plan flow; each has a
proposed fix and its visible effect.

| Residual | Effect | Proposed fix |
| --- | --- | --- |
| The My location chip has no disabled-with-reason or filled state | iOS disables the chip with the location panel's reason and marks the field it filled; React Native keeps the chip enabled and reports a failed start in the error strip | Wire the device-source status into `PlanFieldSearchControl` (a disabled reason) and a filled marker, with Jest cases for denied, unavailable and filled |
| Focusing a field clears the shared result list | `onFocus` calls `openSearchView`, which clears `searchRows`; iOS focus only moves the assignment target and keeps the last result list | Split a `focusPlanField` action (assignment target + full sheet) from `openSearchView` (which also clears the list); add a Jest case |
| The assigned-row marker can go stale after an edit | React Native keeps `assignedOriginText`/`assignedDestinationText` after the field text changes, so the old row can still read as assigned; iOS clears the coordinate on edit | Clear the assignment when the field text differs from its assigned text, mirroring `updateOriginSearchText`; add a Jest case |
| A selected keyless routing provider is not used by the plan action | React Native's keyless branch always applies the artifact's recorded route; iOS plans through a registered keyless routing provider (its public demo address) | Track whether the routing capability has a registered provider and choose the provider plan in that case, mirroring the iOS branch; add a Jest case. This is adjacent to the nine steps, not one of them |
| The React Native route card shows a destination title | iOS uses a `Route` group label and no destination title; React Native renders `planState.titleText` above the facts | Align the card shape with iOS, or add the title to iOS and the web; the facts themselves already match |
| `ReferenceApplicationBehavior.md`'s React Native paragraph is behind the code | It says the fields have no visible labels, inline Search, My location chips or swap; the shipped shell and its Jest suite have all four | Revise that paragraph (or retire its "What exists today" detail in favor of this audit) in a documentation change |

## Evidence series

Every entry is dated 2026-11-04 and produced on the same day as this audit.
Each target ran the plan flow under the device and command named; the evidence
paths hold the captured test reports and result bundles. Screenshots were not
recaptured: the published four-frame set is a separate, credentialed capture
(`docs/ScreenshotCapture.md`) and none of this audit's changes affect the
frames' content on Android, iOS or the web.

### Android — 2026-11-04

- Revision: `95a0057` (the Android sources are unchanged by this audit).
- Version string: shell `0.1.0`, SDK `0.1.0`.
- Device: Pixel_9 emulator (`sdk_gphone64_arm64`), Android 16 (API 36),
  `emulator-5554`.
- Commands:
  - `./gradlew :reference-application:testDebugUnitTest --tests 'com.rusticmaps.sdk.referenceapplication.PlanFieldResolverTest' --tests 'com.rusticmaps.sdk.referenceapplication.DirectionsStateTest'`
  - `./gradlew :reference-application:assembleDebug :reference-application:assembleDebugAndroidTest`
  - `adb shell am instrument -w -e class 'com.rusticmaps.sdk.referenceapplication.ReferenceApplicationAndroidTest#thePlanAndNavigateFlowResolvesTypedTextAndStartsTheTrip' com.rusticmaps.sdk.referenceapplication.test/androidx.test.runner.AndroidJUnitRunner`
- Evidence paths:
  - `tmp/plan-flow-audit/android/gradle-unit-tests.log` (`PlanFieldResolverTest` 5/5, `DirectionsStateTest` 13/13)
  - `tmp/plan-flow-audit/android/TEST-*.xml` (the JUnit results)
  - `tmp/plan-flow-audit/android/gradle-assemble.log`
  - `tmp/plan-flow-audit/android/instrumentation-plan-flow.log` (`OK (1 test)`, 13.4 s)
- What the flow test asserts: the peek names the demo-plan fallback, typed text
  enables Plan route, swap exchanges the pair without planning, Plan route
  resolves the typed pair, the source line switches to the planned route, and
  the simulated trip starts.

### iOS — 2026-11-04

- Revision: `95a0057` (the iOS sources are unchanged by this audit).
- Version string: shell `1.0` build `1`, SDK `0.1.0`.
- Device: iPhone 17 Pro simulator, iOS 26.5, id
  `BF087F6F-3BE2-47B5-94C5-561EE4F3880A`.
- Commands:
  - `xcodebuild test -project ReferenceApplication.xcodeproj -scheme ReferenceApplication -sdk iphonesimulator -destination 'platform=iOS Simulator,id=BF087F6F-3BE2-47B5-94C5-561EE4F3880A' -only-testing:ReferenceApplicationTests/ReferenceApplicationBehaviorTests -only-testing:ReferenceApplicationTests/DirectionsChoiceStateTests`
  - the same command with `-only-testing` for `testThePlanFlowGatesItsRouteActionUntilBothFieldsHoldText`, `testEachMyLocationChipFillsOnlyItsOwnField`, `testDirectionsDefaultToCarAndWalkChangesThePlansMode`, `testTypedDemoPlaceNamesPlanTheRecordedRouteWithoutAResultTap`, `testRememberedChoicesSurviveRelaunch` and `testTheKeylessDemoSearchAndRecordedRouteNeedNoProviderKey`
- Evidence paths:
  - `tmp/plan-flow-audit/ios/xcodebuild-unit-tests.log` (12 tests, 0 failures)
  - `tmp/plan-flow-audit/ios/xcodebuild-ui-tests.log` (6 UI tests, 0 failures)
  - `tmp/plan-flow-audit/ios/ReferenceApplicationPlanFlow.xcresult`
  - `tmp/plan-flow-audit/ios/ReferenceApplicationPlanFlowUI.xcresult`
  - `tmp/plan-flow-audit/ios/build-ios.log` (the generated bindings were
    regenerated first; the checkout carried stale generated Swift against the
    committed UDL)
- What the UI tests drive: the keyless demo search and recorded route, the
  Plan route gate, one chip per field, the Car/Walk mode change, typed demo
  names without a result tap, and the remembered choices across a relaunch.

### React Native — 2026-11-04

- Revision: `a8bba53` plus `6b16d0c` on `docs/m44-audit` (the shell fix and
  its pinning tests); base `95a0057`.
- Version string: shell `MapsSdkRefappRn` `0.0.1`, package and SDK `0.1.0`.
- Device: host Jest run (Node `v25.8.2`, Darwin 26.4); the shell's standing
  device remains the Pixel_9 emulator, used by the capture runner rather than
  by the component suite.
- Command:
  `cd platform/rn/app-shell && npx jest --runInBand`
- Evidence paths:
  - `tmp/plan-flow-audit/rn/jest-full.log` (27 suites, 195 tests, 0 failures)
  - `tmp/plan-flow-audit/rn/jest-reference-flows.log` (8 flow tests)
  - `tmp/plan-flow-audit/rn/jest-plan-flow.log` (the plan-flow subset)
- What the suite covers: labeled fields with markers, inline Search actions,
  per-field My location chips, swap, typed provider resolution, the recorded
  keyless route, the route card facts, the failure strip, the Directions
  defaults and relaunch restore, and the new peek trip-source line.

### Web — 2026-11-04

- Revision: `95a0057` (the web sources are unchanged by this audit).
- Version string: no independent harness version; SDK `0.1.0`.
- Device: Playwright `1.64.1`, system Chromium channel, viewport 640×480 at
  device scale 1, Darwin 26.4.
- Command:
  `cd platform/web && npx playwright test tests/reference_flows.spec.mjs tests/trip_planner.spec.mjs tests/directions_controls.spec.mjs tests/demo_preferences.spec.mjs --workers=1 --reporter=line`
- Evidence path:
  `tmp/plan-flow-audit/web/playwright-plan-flow.log` (19 tests, 0 failures).
- What the specs drive: the plan-and-navigate flow on the labelled fields,
  the my-location flow, the swap, the routing capability picker, an
  unreachable deployment reported verbatim, the keyless recorded trip, the
  Directions choices, and the reload/reset behavior.

## Gates

| Gate | Command | Result |
| --- | --- | --- |
| Reference-flow contract | `node scripts/check-reference-flows.mjs` | PASS: 4 flows and 24 steps |
| UX parity | `node scripts/check_ux_parity.mjs` | PASS: every extracted consumer string matches its canonical value |
| Docs references | `node scripts/check-docs-references.mjs` | PASS (every `docs/` page indexed and every referenced path resolves) |
| Doc snippets | `node scripts/check-doc-snippets.mjs` | PASS (no generated snippets in this page) |
| React Native tests | `cd platform/rn/app-shell && npx jest --runInBand` | 195 passed |

## Reproduction notes

- The React Native package and its generated bindings are gitignored build
  artifacts. A fresh worktree needs `platform/rn/src/index.ts`,
  `platform/rn/src/NativeMapsSdkRn.ts` and `platform/rn/src/generated/` from a
  generated checkout (or `scripts/generate-rn-bindings.sh`) before the Jest
  suite resolves `maps-sdk-rn`; copying them, not symlinking, keeps one module
  identity for the package-root test.
- The iOS build needs `scripts/build-ios.sh` after any UDL change. A checkout
  can otherwise carry generated Swift that predates the committed contract;
  the first audit attempt failed on `MapView.reloadTileSource` for exactly
  that reason and passed after regeneration.
- The web suite needs the wasm `platform/web/pkg`; the worktree links the
  generated package from the main checkout.
- Evidence under `tmp/plan-flow-audit/` is gitignored by design; the committed
  record is this page's commands, test names and verdicts.
