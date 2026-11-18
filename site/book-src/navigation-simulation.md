# Turn-by-turn navigation

Start a trip on a route, deliver position fixes, and receive turn-by-turn
guidance: the banner instruction, speech triggers, deviation status, and
phase. This guide turns the route from [Search and routing](search-and-routing.md)
into a simulated Capitol Park to Tower Bridge drive. Every code block compiles
against the [FFI contract](udl-contract.md).

## Before you start

A `Route` from `calculateRoute` and the `mapSdk` and `mapView` from [Your first
map](first-map.md); [Show the device's location](positioning.md) obtains fixes.

## Rules that apply on every platform

- Subscribe before you start: `addNavigationListener` registers one
  `NavigationListener` for all four topics and returns a subscription; cancel
  it when the screen leaves. Dropping it does not unsubscribe. The four
  per-topic calls (`subscribeToNavigationInstructions`,
  `subscribeToSpeechInstructions`, `subscribeToDeviationStatus`,
  `subscribeToPhaseChanges`) exist for a screen that needs only some channels.
  The listener's `onNavigationInstruction`, `onSpeechInstruction`,
  `onDeviationStatusChange` and `onNavigationPhaseChange` carry the topics.
- Start with the default policies unless you measured different ones. The
  initial instruction dispatches before `startNavigation` returns, so the
  banner shows the first maneuver.
- Deliver one fix at a time with a reported accuracy; a fix without accuracy
  never advances a step and is never judged for deviation.
- Put the route polyline on the map once, then rebuild the follow camera from
  each accepted fix. Follow the smoothed course, not the raw fix-to-fix
  bearing, which jitters at low speed. When the user pans away, stop drawing
  the follow camera until they tap the location control.
- The SDK triggers speech; your application voices it. Hand `utteranceSpeechText`
  verbatim to the speech engine and use `utteranceId` to deduplicate.
- A trip graded fully off route asks for a replacement through
  `beginRerouteCalculation`; `applyReroute` applies it and
  `cancelRerouteCalculation` abandons it, while `isCalculatingNewRoute` reports
  the in-flight fact. The grade names are `FULLY_OFF_ROUTE` on Android,
  `.fullyOffRoute` in Swift and `FullyOffRoute` in React Native.
- `stopNavigation` clears the trip and its state; later fixes and reroutes are
  dropped until the next start, while subscriptions stay registered.

<div class="platform-tabs">
<div class="platform-tab-titles" role="tablist">
<button type="button" data-platform-tab="android">Android</button>
<button type="button" data-platform-tab="ios">iOS</button>
<button type="button" data-platform-tab="react-native">React Native</button>
<button type="button" data-platform-tab="wasm">WebAssembly</button>
</div>

<div class="platform-tab-panel" data-platform-panel="android">

### Start the trip

```kotlin
import uniffi.rustic_maps.defaultDeviationPolicy
import uniffi.rustic_maps.defaultStepAdvancePolicy
import uniffi.rustic_maps.UnitSystem

navigationController.startNavigation(
    route,
    UnitSystem.METRIC,
    defaultStepAdvancePolicy(),
    defaultDeviationPolicy(),
)
```

### Deliver position fixes

```kotlin
import uniffi.rustic_maps.NavigationLocationUpdate

suspend fun replayFixes(
    navigationController: Navigator,
    samples: Sequence<GeographicCoordinate>,
    sampleIntervalMilliseconds: Long,
) {
    for (sample in samples) {
        navigationController.updateLocation(
            NavigationLocationUpdate(
                coordinate = sample,
                horizontalAccuracyMeters = 8.0,
                courseHeadingDegrees = null,
                timestampSeconds = null,
            ),
        )
        delay(sampleIntervalMilliseconds)
    }
}
```

### Draw the trip and follow it

```kotlin
// Once, when the route arrives: the route geometry as one polyline.
val routeOverlay = MapOverlay(
    kind = MapOverlayKind.ROUTE_POLYLINE,
    coordinates = route.legs.flatMap { leg -> leg.steps.flatMap { step -> step.shape } },
    rgbaColor = OverlayColor(red = 0.16, green = 0.47, blue = 1.0, alpha = 0.95),
    widthPixels = 6.0,
    ringCoordinates = null,
    pointMarkerPositions = null,
    pointMarkerRadiusPixels = null,
    trafficFlowSegments = null,
)
mapView.setOverlays(listOf(routeOverlay))

// On every fix (or every instruction, if you follow instructions instead
// of raw fixes): a 3D course-up camera over the vehicle.
val followCamera = MapCameraPose(
    latitudeDegrees = fix.coordinate.latitude,
    longitudeDegrees = fix.coordinate.longitude,
    zoomLevel = followZoomLevel,
    bearingDegrees = smoothedCourseHeadingDegrees,
    pitchDegrees = followPitchDegrees,
)
mapView.setLocationIndicator(locationIndicatorState, locationIndicatorStyle)
mapView.setCamera(followCamera)
```
</div>

<div class="platform-tab-panel" data-platform-panel="ios">

### Start the trip

```swift
try navigationController.startNavigation(
    route: route,
    unitSystem: UnitSystem.metric,
    stepAdvancePolicy: defaultStepAdvancePolicy(),
    deviationPolicy: defaultDeviationPolicy()
)
```

### Deliver position fixes

```swift
func deliver(_ fix: RecordedFix) {
    navigationController.updateLocation(
        locationUpdate: NavigationLocationUpdate(
            coordinate: fix.coordinate,
            horizontalAccuracyMeters: fix.horizontalAccuracyMeters,
            courseHeadingDegrees: fix.courseHeadingDegrees,
            timestampSeconds: nil
        )
    )
    if let tripState = navigationController.currentTripState() {
        updateReadouts(tripState)
    }
}
```

### Draw the trip and follow it

```swift
let routeOverlay = MapOverlay(
    kind: .routePolyline,
    coordinates: route.legs.flatMap { $0.steps.flatMap { $0.shape } },
    rgbaColor: OverlayColor(red: 0.16, green: 0.47, blue: 1.0, alpha: 0.95),
    widthPixels: 6.0,
    ringCoordinates: nil,
    pointMarkerPositions: nil,
    pointMarkerRadiusPixels: nil,
    trafficFlowSegments: nil
)
_ = mapView.setOverlays([routeOverlay])

let followCamera = MapCameraPose(
    latitudeDegrees: fix.coordinate.latitude,
    longitudeDegrees: fix.coordinate.longitude,
    zoomLevel: followZoomLevel,
    bearingDegrees: smoothedCourseHeadingDegrees,
    pitchDegrees: followPitchDegrees
)
mapView.setLocationIndicator(state: locationIndicatorState, style: locationIndicatorStyle)
mapView.setCamera(followCamera)
```
</div>

<div class="platform-tab-panel" data-platform-panel="react-native">

### Start the trip

The package root exports `Navigator`, `UnitSystem`,
`defaultStepAdvancePolicy` and `defaultDeviationPolicy`.

```js
navigationController.startNavigation(
  route,
  UnitSystem.Metric,
  defaultStepAdvancePolicy(),
  defaultDeviationPolicy(),
);
```

### Deliver position fixes

```js
navigationController.updateLocation({
  coordinate: { latitude: sampleLatitude, longitude: sampleLongitude },
  horizontalAccuracyMeters: 8.0,
  courseHeadingDegrees: undefined,
  timestampSeconds: undefined,
});

const tripState = navigationController.currentTripState();
```

### Draw the trip and follow it

```js
import {
  LocationIndicatorVisualKind,
  MapOverlayKind,
} from '@angoratek/rustic-maps-sdk';

// Once, when the route arrives: one polyline over the whole route.
const routeOverlay = {
  kind: MapOverlayKind.RoutePolyline,
  coordinates: route.legs.flatMap(
    (leg) => leg.steps.flatMap((step) => step.shape),
  ),
  rgbaColor: { red: 0.16, green: 0.47, blue: 1.0, alpha: 0.95 },
  widthPixels: 6.0,
};
mapView.setOverlays([routeOverlay]);

// On every accepted fix: the 3D course-up follow camera (z17, 45° pitch,
// the shared demo values). The guided arrow sits over the vehicle.
const FOLLOW_ZOOM_LEVEL = 17.0;
const FOLLOW_PITCH_DEGREES = 45.0;
const followCamera = {
  latitudeDegrees: fix.latitude,
  longitudeDegrees: fix.longitude,
  zoomLevel: FOLLOW_ZOOM_LEVEL,
  bearingDegrees: smoothedCourseHeadingDegrees,
  pitchDegrees: FOLLOW_PITCH_DEGREES,
};
mapView.setLocationIndicator(
  {
    coordinate: { latitude: fix.latitude, longitude: fix.longitude },
    courseHeadingDegrees: smoothedCourseHeadingDegrees,
  },
  { visualKind: LocationIndicatorVisualKind.GuidedCourseArrow },
);
mapView.setCamera(followCamera);
```

</div>

<div class="platform-tab-panel" data-platform-panel="wasm">

### Start the trip and read the banner

The demo drives the same `Navigator` with a `RouteSimulator` fix;
`fixAtTravelledMeters` is the distance-driven equivalent,
`totalDistanceMeters()` and `totalDurationSeconds()` size the progress bar,
and `isArrived` turns true at the final coordinate.

```js
import { Navigator, RouteSimulator, UnitSystem } from './web/rustic_maps.js';

const navigationController = new Navigator();
navigationController.startNavigation(
  route,
  UnitSystem.Metric,
  defaultStepAdvancePolicy(),
  defaultDeviationPolicy(),
);

const navigationRouteSimulator = new RouteSimulator(route, {
  speedMetersPerSecond: 1.4,
  speedMultiplier: 20.0,
});

const navigationFix = navigationRouteSimulator.fixAtElapsedSeconds(0.0);
navigationController.updateLocation({
  coordinate: navigationFix.coordinate,
  horizontalAccuracyMeters: 8.0,
  courseHeadingDegrees: navigationFix.courseHeadingDegrees,
});

/** @type {any} */
const tripSnapshot = navigationController.currentTripState();

// One snapshot is all the card needs.
const phase = tripSnapshot.phase;
const distancePhrase = tripSnapshot.instruction.activeManeuver.distancePhraseText;
const bannerText = tripSnapshot.instruction.activeManeuver.providerDisplayText
  ?? tripSnapshot.instruction.activeManeuver.displayText;
const thenText = tripSnapshot.instruction.upcomingManeuver?.displayText ?? '';
const metersToManeuver =
  tripSnapshot.instruction.progress.distanceToNextManeuverMeters;
const metersRemaining = tripSnapshot.instruction.progress.distanceRemainingMeters;
const secondsRemaining = tripSnapshot.instruction.progress.durationRemainingSeconds;
const deviationMeters = tripSnapshot.deviation?.distanceFromRouteMeters ?? null;
```

The banner keeps two lines: `distancePhraseText` names the distance and
`providerDisplayText` is the provider's own sentence, so the distance is not
printed twice. `applyReroute` swaps the route without a restart.

</div>
</div>

## What you should see

The banner names the first maneuver before `startNavigation` returns, and the
phase and deviation callbacks follow the trip. `stopNavigation()` ends it;
[Offline maps](offline-maps.md) covers routing without a network.
