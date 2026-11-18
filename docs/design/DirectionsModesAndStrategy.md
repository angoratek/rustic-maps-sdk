# Directions modes and route strategy

Date: 2026-11-01. Status: landed (round 128). `RouteCapabilities` is in the
contract and all four targets expose transport mode and route strategy beside
the plan action. Owner direction: choosing a
transport mode should be possible at every level, starting with car and
pedestrian and extensible to truck, public transit and bicycle; and the route
strategy most engines offer (fastest or shortest) should be abstracted the
same way. The direction came from a real defect: in the iOS reference app a
San Francisco to Oakland plan through OSRM or Valhalla returned a ferry route
where the vendor provider took the Bay Bridge, because the reference apps
hardcode the pedestrian mode.

Companion designs: the plan-flow pattern and the positioning audits are
`docs/design/PositioningAndRouteUx.md`; the on-map control and the
device-location states are `docs/design/MapControlsAndTrafficLayer.md`.

## What already exists

The contract carries both choices on every route request, and the capability
readback that tells a surface what a provider can honour:

| Field | Values today | Providers |
|---|---|---|
| `RouteOptions.transportation_mode` | `Automobile`, `Bicycle`, `Pedestrian` | MapQuest maps all three; OSRM selects the `driving`/`cycling`/`walking` profile; Valhalla sets the `auto`/`bicycle`/`pedestrian` costing |
| `RouteOptions.route_strategy` | `Fastest`, `Shortest` | MapQuest maps both for automobile; Valhalla sets its `shortest` cost factor for automobile; OSRM has no route-service equivalent and documents that it uses `Fastest` |
| `RouteCapabilities` | supported modes plus `supports_shortest_route_strategy` | MapQuest [Automobile, Bicycle, Pedestrian] and shortest; Valhalla all supported; OSRM all modes, no shortest |

`MapSdk.route_capabilities()` answers it for the serving routing provider; an
empty routing slot throws `ProviderNotConfigured`. The record is also carried
on `RoutingEngine`. A provider reports what it can honour rather than what the
SDK can express.

Before this change the contract had the fields but the surface did not: every
reference app planned with `TransportationMode.Pedestrian` hardcoded, no app
exposed the strategy, and nothing told the reader when the serving provider
could not honour a choice. The ferry defect came from that gap.

## The change

### Contract

No breaking change. One addition-only record landed so a surface can be
honest about what a provider supports:

```
dictionary RouteCapabilities {
    sequence<TransportationMode> supported_transportation_modes;
    boolean supports_shortest_route_strategy;
}
```

`MapSdk.route_capabilities()` answers it for the serving routing provider;
an empty routing slot throws `ProviderNotConfigured`. A provider reports what
it can honour rather than what the SDK can express. MapQuest:
`[Automobile, Bicycle, Pedestrian]`, shortest supported. OSRM: all three
modes, shortest not supported. Valhalla: all three modes, shortest supported
for automobile only.

The provider-side seam is one trait method with a default, so no existing
provider must change to keep compiling:

```rust
pub trait RoutingProvider {
    fn route_capabilities(&self) -> RouteCapabilities {
        RouteCapabilities::all_supported()
    }
    // calculate_route stays as it is
}
```

### Reference applications

Every app carries a **Directions** control next to the plan action, in the same
order everywhere:

1. **Mode** — a two-value segmented control, Car and Walk, with Car as the
   default. A mode the serving provider does not support is shown disabled
   with its reason.
2. **Strategy** — a two-value choice, Fastest and Shortest, with Fastest as
   the default, shown only for automobile plans and disabled with a reason
   when the provider reports no shortest support (OSRM today).

Both choices are remembered in the existing preferences record
(`directionsTransportationMode`, `directionsRouteStrategy`), so a relaunch
keeps them, and both are applied to every planned route in the app: the demo
plan, the typed plan, the walkthrough and the navigation the app starts from
the plan.

Web, Android, iOS and React Native consume the same contract calls; the
identifier set is shared (`directions-mode-car`, `directions-mode-walk`,
`directions-strategy-fastest`, `directions-strategy-shortest`).

Per target at round 128: the browser exposes the control with its two Playwright
cases; Android carries `DirectionsState` and the control with JVM and
instrumented cases; iOS carries `DirectionsControlView` and
`DirectionsChoiceState`; React Native exposes the choice through
`directionsSelection` and the curated module. Both choices are in each app's
preferences record.

### Extensibility

Truck and public transit are not expressible by all three shipped providers,
so they stay out of this slice. When they land, `TransportationMode` gains
variants under an owner ruling: a new enum value is source-breaking for
exhaustive `switch` statements in generated Swift, so the ruling, the
regenerated bindings, the provider mappings and the capability report ship in
one change. Until then the capability list is what tells the UI what it may
offer, which is why it exists now rather than later.

## Mode and strategy honouring

The contract can express three transport modes and two strategies. A provider
reports what it can honour through `route_capabilities()`, and a reference
application reads that answer before it renders a choice or sends a request.
The shipped answers are:

| Provider | Car | Walk | Bicycle | Fastest | Shortest |
|---|---|---|---|---|---|
| MapQuest | yes | yes | yes | yes | automobile only |
| Valhalla | yes | yes | yes | yes | automobile only |
| OSRM | yes | yes | yes | yes, from the profile's ranking | no |

Shortest is an automobile-only choice on every surface: a walk plan always
sends Fastest, and a remembered Shortest choice waits for the next car plan.
OSRM has no route-service switch for the ranking, so both strategies produce
the same request and the deployment's profile decides the path.

### What happens when a choice is unsupported

On Android, iOS and React Native, a choice the serving provider reports as
unsupported is disabled with a reason; it is never sent and silently ignored.
The reason line is the same sentence the shell uses elsewhere, and the
selection falls back without changing the remembered value. WebAssembly is
the one exception described in the sentence table: its page does not gate the
control on the report. The fallbacks are:

- An unsupported mode pick changes nothing. A remembered mode the provider
  cannot serve falls back to the first offered supported mode, Car before
  Walk. With no supported mode at all, the selection is kept and no provider
  call follows.
- An unsupported Shortest pick changes nothing, and a remembered Shortest
  falls back to Fastest. Returning to a provider that supports it restores the
  remembered choice.
- On Android, iOS and React Native, a failed capability read (no routing
  provider installed, or the read refused) disables both mode choices and
  Shortest. The line is the provider failure verbatim; the contract's routing
  slot reports `no 'RoutingProvider' provider is configured`.

The exact disabled-with-reason sentences the shells show:

| Surface | Unsupported mode | Unsupported Shortest |
|---|---|---|
| Android | `{mode} directions are not available from this provider.` (`Walk directions are not available from this provider.`, `Car directions are not available from this provider.`) | `Shortest route strategy is not available from this provider.` |
| iOS | `{mode} is not offered by the serving routing provider.` (`Walk is not offered by the serving routing provider.`) | `Shortest is not offered by the serving routing provider.` |
| React Native | `The serving provider does not support walking routes.` and `The serving provider does not support car routes.` | `The serving provider does not support shortest routes.` |
| WebAssembly | No reason line. The browser API exposes `routeCapabilities()`, but the demo does not gate the control on it; the selected pair is sent and the route card states the mode and strategy. | No reason line for the same reason; the selected strategy is sent and the provider's documented ranking applies. |

The reason belongs to the control, not to the plan result: a plan is refused
only by the provider, with the provider's own detail. The fallback values are
what the plan call carries (`effectiveTransportationMode` and
`effectiveRouteStrategy` in the shared reference-application state).

### Ferry crossings

OSRM's maneuver vocabulary has no ferry entry. It marks a crossing on the
route step's `mode` field, and a ferry step also carries a `notification`
maneuver type. The provider classifies any step whose `mode` is `ferry` as
`ManeuverType::Ferry` before consulting the maneuver-type table, so a
crossing is never downgraded to `Continue`. Both instruction strings read
`Take the ferry`; OSRM returns no prose, so the provider authors both forms.

The provider crate's hand-authored `san_francisco_oakland_ferry_route.json`
fixture follows the route-service response shape for a crossing with a
driving step, a ferry step and an arrival step. The provider test plans it and
asserts the ferry classification and both instruction strings, that the
automobile choice requested the driving profile, and that Fastest and
Shortest produce the same request. The browser reference-flow suite selects
the provider that owns the fixture through the settings picker, plans the
fixture's own endpoints, and asserts the same display and speech strings
survive both strategies.

## Tests

- Rust: each provider's `route_capabilities` answer; a routing slot with no
  provider throws `ProviderNotConfigured`; a request with each mode and
  strategy builds the documented provider request (OSRM profile path,
  Valhalla costing, MapQuest route type).
- Reference apps: a plan with Car and Fastest carries `Automobile`/`Fastest`
  to the SDK; switching to Walk changes the request; a provider without
  shortest support shows the strategy disabled with its reason; both choices
  survive a relaunch.
- The ferry defect gets a regression case in the provider crate and in the
  browser reference-flow suite: with Car selected, the committed OSRM ferry
  fixture is requested with the `driving` profile, the ferry step's display
  and speech strings both read `Take the ferry`, and the Fastest and Shortest
  requests are identical.
