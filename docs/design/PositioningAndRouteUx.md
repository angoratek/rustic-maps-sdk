# Positioning feedback and the route-planning flow

Date: 2026-11-01. Status: landed. The positioning audits and the plan-flow
audits are complete on Android, iOS, React Native and the web demo, and their
fixes ship. Owner report: in the iOS
reference app the round location control does not trigger the current
location, or gives no visible feedback when it fails; and the Android (or
React Native) plan flow is confusing next to iOS — the origin and destination
fields do not make it clear how to search, and nothing seems to work. Both
are review findings, so this design starts with the audit each one needs
before any code.

Companion designs: the on-map control, the device-location states and the
my-location chips are `docs/design/MapControlsAndTrafficLayer.md`; the
transport-mode and route-strategy control beside the plan action is
`docs/design/DirectionsModesAndStrategy.md`.

## Positioning: what the audit must establish

The contract and the shells already have the pieces — `PositionController`,
each shell's device location source, the map control cluster's my-location
button, and the M41 state machine (off, waiting for a fix, active, permission
denied, unavailable). The report says the UI does not show any of it on iOS.
The audit answers, per target, with evidence:

| Question | Where to look |
|---|---|
| Does the round control start the source when off? | iOS `MapControlClusterView` action, Android `MapControlCluster`, RN cluster handler, web `my-location-control` |
| Is a permission request issued, and what is shown while it is pending? | each shell's source implementation and its status mapping |
| Is a failure surfaced at all, and where? | source status → panel row / status line / readiness chip |
| Does the camera centre on the first fix, and does the indicator appear? | follow camera logic, location indicator state |
| Does turning the source off clear the indicator and stop the camera following? | the stop path |

The audit's outcome is a per-target checklist of missing links, each with a
UI test that would have caught it. Fixes then land against the M41 state
machine rather than inventing a second one.

Known facts at the audit's start, verified rather than assumed: the web slice
of M41 already starts the source from the round control and reports the panel
states. The iOS audit found three broken links — the round control never
started the source, `locationManagerDidChangeAuthorization` was absent so a
permission grant never started the source without a second tap, and
`lastFixCoordinate` was never assigned so re-centre was dead. All three are
fixed with tests. The Android plan-flow audit found the two-field structure in
place but the fields unlabelled, no visible per-field Search, no assignment
hint, and a plan action enabled on typed text; those are fixed too. The
Android, React Native and web positioning audits and the React Native
plan-flow audit are complete, and each fix carries its UI test.

## The two-mode location puck

The map draws the device position itself, and the same renderer geometry
appears on every target. The puck has two modes chosen by the layer's visual
kind and the fix's reported course:

| Mode | When it draws | Visual |
|---|---|---|
| Tracking | `TrackingDot`, or any mode without a reported course | Accent core disc, white ring, dark halo, plus the ground-sized accuracy ring when the fix's accuracy reaches the draw threshold |
| Navigation | `GuidedCourseArrow` with a reported course | An up-course dart with a white outline and the accent fill, inside the same white ring and halo, rotated with the course |

The dart's tip reaches 0.86 of the accent-core radius, its rear corners span
0.58 of the radius each side of the course axis at 0.56 down-course, and its
tail notch sits at 0.18 down-course; the white outline adds 2.5 screen pixels.
The white ring is 3 screen pixels wide and the dark halo 2 screen pixels
outside it, so the tracking puck's overall radius is the accent-core radius
plus 5. The tracking dot ignores the fix's course entirely: the visual kind,
not the state, chooses the form, and a navigation layer without a course
degrades to the tracking dot.

The puck is screen-anchored: its center stays on the fix's projected pixel
under zoom and pan, and it draws once per visible world-copy window. The
accuracy ring is ground-projected, so it foreshortens at pitch and splits at
world-copy boundaries. The whole layer stays the top draw layer, painting
over route lines and polygons. The geometry is authored in the renderer; no
third-party icon asset is used.

Every size is a named renderer constant, and the colours keep their defaults
in the style record with per-layer overrides:

| Style field | Meaning | Default |
|---|---|---|
| `radius_pixels` | Accent-core radius; the ring and halo are added outside it | 11 screen pixels |
| `rgba_color` | Accent fill of the core disc and navigation dart | Opaque blue |
| `ring_rgba_color` | Ring around the puck and the dart's outline | Opaque white |
| `halo_rgba_color` | Halo drawn under the puck | Near-black at 30% alpha |
| `show_accuracy_ring` | Whether the ground accuracy ring draws | On |

The shells switch the visual kind from their navigation state: tracking while
the user follows the device, navigation while a simulated trip is active,
back to tracking on arrival or exit. They pass no radius or colour overrides,
so the renderer defaults are what every reference app shows.

## The route-planning flow: one pattern on every target

iOS is the reference, by owner direction. The shared flow:

1. **Two fields, clearly labelled.** `Origin` and `Destination`, each with an
   inline search affordance (a field that searches as you type, or a labelled
   `Search` action when the field is empty).
2. **Results assign to the field that asked.** A result list appears under
   the fields; tapping a result assigns it to the field whose search produced
   the list, and the sheet states which field the next result will fill.
3. **One primary action.** `Plan route` is enabled only when both fields hold
   resolved places; while disabled it states the missing half in one line.
4. **My-location chips.** Each field carries the M41 `My location` chip.
5. **Simulation stays secondary.** `Start simulated navigation` is a
   secondary action above the plan, never the primary route action.

Android and React Native adopt this pattern; the existing identifiers used by
their UI tests are kept or renamed in one change with the test updates. The
web demo keeps its current layout (it already follows the two-field pattern).

The audit first establishes whether the reported Android behaviour is the
current build: the report suspects an old installed app. The audit records
the installed build's `get_sdk_compiled_version` string beside the screenshot
so a stale install cannot be mistaken for a regression.

## Evidence and tests

- One screenshot series per target, taken in the same scenario (typed origin,
  typed destination, resolved both, planned route) on one revision, with the
  version string visible.
- iOS XCUITest, Android Espresso and RN jest cases for: the disabled plan
  action naming the missing field, a result assigning to the searched field,
  and a my-location chip filling its own field.
- A positioning case per target: tapping the round control with a simulated
  fix centres the camera, shows the indicator, and the console (M43) reports
  the source status; a denied permission shows its reason without a crash.
