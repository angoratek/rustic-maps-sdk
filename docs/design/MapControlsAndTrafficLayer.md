# Map controls: the layer switcher, traffic and device location

Date: 2026-11-01. Status: landed on Android, iOS, React Native and the web
demo. The floating control, the traffic layer, the device-location states, the
my-location chips and the settings pointers ship on all four targets. This
document answers the
owner's direction of the same day: make the base map switchable from the map
itself with a floating control above the attribution line, expose the traffic
layer there, and drive device positioning and "my location" route endpoints
from the map's round location control instead of the settings sheet. Provider
selection for every other capability stays in settings.

Rulings recorded the same day (`PRD.md` §6 item 35): the traffic layer draws
congestion-colored flow polylines from the `TrafficFlowProvider` data; the
floating control carries base map, traffic and the demo overlay scenes; the
route endpoints get one "My location" chip beside each of the origin and
destination fields.

## Landed per platform

| Target | Landed | Open |
|---|---|---|
| Browser | The floating control on the playground with Base map, Traffic, Overlays and Device location sections; Escape closes and refocuses; the traffic overlay fed from camera bounds with a 400 ms debounce and one request in flight; the round control starts the source; both chips. `tests/map_layers_and_traffic.spec.mjs` carries 8 cases. The settings dialog points to the map control. | — |
| iOS | The panel with the same four sections; the round control starts the source, a permission grant starts it without a second tap, first-fix re-centre works, failures render in the panel, and the chips fill their own field. 12 unit tests and 6 XCUITest cases. The settings sheet points to the map control. | — |
| Android | The panel with the same four sections; the traffic switch, the round control's states, the first-fix re-centre and both chips; the plan-flow fixes from the positioning audit (labelled fields, per-field Search, assignment hint, resolved-place gating). The settings sheet points to the map control. | — |
| React Native | The panel with the same four sections; the traffic switch, the round control's states and both chips. The settings sheet points to the map control. | — |

## Why this is a UI slice, not a renderer build

The contract and the renderer already carry everything the three features
need:

| Need | Existing surface |
|---|---|
| Base map switch | `register_url_template_raster_tile_source`, `MapView.set_vector_tile_source`, `clear_vector_tile_source` |
| Traffic drawing | `MapOverlayKind.TrafficFlow` with `traffic_flow_segments` and congestion colors |
| Traffic data | `fetch_traffic_flow(BoundingBox)` on the `TrafficFlow` capability |
| Device positioning | `PositionController` + each shell's device location source (M33) |
| Remembered choices | each app's versioned preferences record |

So the work is one control, one data path per shell, and no contract change.
A provider that serves traffic joins the toggle by declaring the capability;
nothing in the shells names a vendor.

## The floating control

**Placement.** The map's end edge (bottom-right), docked directly above the
attribution notice, in the same position on all four applications. It sits
outside the present dock's area and never overlaps the attribution line. On a
phone it opens upward as a panel clamped to 70% of the viewport height; on a
wide viewport it opens beside the button.

**Collapsed state.** One round button with the layers glyph, labelled "Map
layers" for assistive technology. The button carries a dot when a non-default
layer is active (a style other than the default, traffic on, or scenes
enabled).

**Expanded panel.** Three sections, in this order:

1. **Base map** — one row per tile source the platform can serve, read from
   the shared descriptors: the raster sources first, then every committed
   vector style of every vector descriptor. The active row carries the
   `Serving` mark; a provider whose credential is missing shows
   `Needs <credential label>` and stays disabled. Selecting a row performs
   the same call the settings Base map section performs today.
2. **Traffic** — one switch. On turn-on the shell asks the traffic-flow
   capability for the current camera bounds, debounced to the camera's settled
   state, and installs a `TrafficFlow` overlay; panning and zooming refresh it
   at the same debounce. The switch renders `Needs <credential label>` when no
   credential serves the capability and `Not configured` when no provider
   declares it, so the control never pretends a layer is on.
3. **Overlays** — one switch per demo scene key of the shared scenes artifact,
   mirroring the settings Overlay scenes section and writing the same
   remembered choice.

**One choice, one control.** The settings sheet keeps the per-capability
provider rows (M40), but its Base map and Overlay scenes sections become a
one-line pointer to the map control — a capability that has an interactive
control on the map is not also a menu in settings. The settings sheet keeps
credentials and the capabilities the map control does not cover: geocoding,
routing, distance matrix, place search, traffic data, elevation and offline
routing.

## Traffic

The layer is provider-neutral by construction:

1. Resolve the camera's geographic bounds.
2. `fetch_traffic_flow(bounds)` through the capability.
3. Install one `TrafficFlow` overlay with the returned segments.

Refreshes are debounced to the camera settling, capped to one request in
flight, and dropped when the toggle goes off. A failed refresh keeps the
previous segments and shows the failure detail in the panel's status line; a
`ProviderNotConfigured` failure turns the switch off with its reason.
Attribution for the layer comes from the descriptor that serves it, appended
to the attribution notice while the layer is on.

The committed flow fixture (`fixtures/mapquest/traffic_flow_new_york.json`)
is the test payload; no test reaches the network.

## Device location

**The round control.** The my-location button in the map control cluster
gains the off state:

| State | Tap | Long press |
|---|---|---|
| Source off, no fix | Request permission, start the source, center on the first fix | — |
| Source on, following | Re-center and resume following after a gesture | Open the layers panel at the Device location row |
| Source on, permission denied | Open the layers panel at the Device location row with the reason | — |

The layers panel's Device location row is the status and stop surface: `Off`,
`Waiting for a fix`, `Active`, `Permission denied`, `Unavailable`, with a
`Stop` action when on. The settings sheet's device-location row becomes a
one-line pointer to this control.

**Route endpoints.** Each of the origin and destination fields carries a small
"My location" chip beneath the field's leading edge. Tapping it:

1. Starts the source when it is off (same path as the round control), and
2. fills that field with the latest fix's display text as soon as a fix
   arrives, leaving the other field untouched.

The chip is disabled with the same reason text as the panel when the source
cannot start. Once a field holds a fix, the chip reads `My location` with a
filled mark, and its accessibility label names the field it fills so two chips
are never indistinguishable.

## Accessibility and test identifiers

| Item | Identifier |
|---|---|
| The collapsed control | `map-layers-control` |
| One base row | `map-layer-base-<provider_key>` or `map-layer-style-<style_id>` |
| The traffic switch | `map-layer-traffic` |
| One scene switch | `map-layer-scene-<scene_key>` |
| The device location row | `device-location-status` |
| The origin chip | `origin-my-location` |
| The destination chip | `destination-my-location` |

On the web the control is a button with `aria-expanded` and `aria-controls`
over a labelled panel; rows are real buttons; switches are `role="switch"`
with `aria-checked`; the panel closes on Escape and returns focus to the
button; the attribution notice's live region announces the layer change.

## Tests

- Browser: open, switch a raster source to a vector style, toggle traffic
  from the committed fixture and count the overlay installs, toggle a scene,
  reload and see the choices restored; the control stays keyboard operable and
  Escape returns focus.
- Android Espresso and iOS XCUITest: open the panel, switch the base, toggle
  traffic to its needs-credential state keyless, tap the round location
  control with an injected fix and see the camera center and the panel's
  status read Active, tap each my-location chip and see the matching field
  fill.
- React Native jest: the same panel state machine and chip behavior against
  the mocked module.
- Parity: the identifiers and section order above are the shared contract;
  the parity gate asserts they exist in all four shells.

## Documentation and screenshots

A new chapter, *Map layers and traffic*, shows the control, the traffic
toggle and the location flows for each platform. `ReferenceAppUX.md` §4 gains
the component and retires the settings Base map and Overlay scenes sections to
pointers. The screenshot set is recaptured once, in the same scenario, with
the panel closed on the landing state and open in a second shot; no platform's
screenshots update alone.

## Open questions

None blocking; the panel's wide-viewport placement is an implementation
choice the first slice settles against the existing dock geometry.
