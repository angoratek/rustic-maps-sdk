# Turn-by-turn navigation

Start a trip on a route, deliver position fixes, and receive turn-by-turn
guidance: the banner instruction, speech triggers, deviation status, and
phase. This guide turns the route you planned in [Search and
routing](search-and-routing.md) into a simulated Capitol Park to Tower Bridge
drive. Every code block compiles against the [FFI contract](udl-contract.md).

## Rules that apply on every platform

- Subscribe before you start the trip, so you receive the initial instruction
  dispatch. `addNavigationListener` subscribes one listener to all four
  topics and returns a subscription; cancel it with its handle when the screen
  leaves. Dropping the reference does not unsubscribe. The four per-topic
  subscribe calls exist for a screen that needs only some channels.
- Start with the default policies unless you measured different ones. The
  initial instruction payload dispatches before the call returns, so the
  banner shows the first maneuver.
- Deliver one fix at a time. A fix without a reported accuracy never advances
  a step and is never judged for deviation, so a simulated source must carry
  its accuracy like a real fix does.
- Put the route polyline on the map once, then rebuild the follow camera from
  each accepted fix. Follow the smoothed course, not the raw fix-to-fix
  bearing: a fix-to-fix course jitters at low speed. When the user pans away,
  stop drawing the follow camera until they tap the location control.
- The SDK triggers speech; your application voices it. Hand
  `utteranceSpeechText` verbatim to the platform's speech engine.
  `utteranceId` is a stable identity for deduplication.
- A deviating trip asks for a replacement route through
  `beginRerouteCalculation`; `applyReroute` applies it and
  `cancelRerouteCalculation` abandons it. The trip state's
  `isCalculatingNewRoute` reports the in-flight fact.
- `stopNavigation` clears the active trip and its state; later fixes and
  reroutes are dropped until the next start, and subscriptions stay
  registered.

<div class="platform-tabs">
<div class="platform-tab-titles" role="tablist">
<button type="button" data-platform-tab="android">Android</button>
<button type="button" data-platform-tab="ios">iOS</button>
<button type="button" data-platform-tab="react-native">React Native</button>
<button type="button" data-platform-tab="wasm">WebAssembly</button>
</div>

<div class="platform-tab-panel" data-platform-panel="android">

### Build the controller and subscribe

The listener implements the four callbacks the contract carries:

```kotlin
import uniffi.rustic_maps.Navigator
import uniffi.rustic_maps.NavigationListener

class GuidanceListener(
    private val onInstruction: (NavigationInstruction) -> Unit,
    private val onSpeech: (SpeechInstruction) -> Unit,
) : NavigationListener {
    override fun onNavigationInstruction(instruction: NavigationInstruction) {
        onInstruction(instruction)
    }

    override fun onSpeechInstruction(speechInstruction: SpeechInstruction) {
        onSpeech(speechInstruction)
    }

    override fun onDeviationStatusChange(deviationStatusChange: DeviationStatusChange) {
        // Update your reroute affordance here.
    }

    override fun onNavigationPhaseChange(navigationPhaseChange: NavigationPhaseChange) {
        // Drive the navigation drawer's phase state here.
    }
}

val navigationController = Navigator()
val listener = GuidanceListener(onInstruction = ::showBanner, onSpeech = ::speak)

// One call subscribes the listener to all four topics; the returned
// subscription cancels every one of them.
val subscription = navigationController.addNavigationListener(listener)

// The four per-topic calls remain available when a screen needs only
// some of the channels:
//   navigationController.subscribeToNavigationInstructions(listener)
//   navigationController.subscribeToSpeechInstructions(listener)
//   navigationController.subscribeToDeviationStatus(listener)
//   navigationController.subscribeToPhaseChanges(listener)
```

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

### Drive the trip from the device

Feed the `Navigator` the fixes your location source produces and the same
banner, ETA line and deviation judgment follow the real journey:

```kotlin
navigationController.startNavigation(route, UnitSystem.METRIC,
    defaultStepAdvancePolicy(), defaultDeviationPolicy())
// On every accepted device fix:
navigationController.updateLocation(
    NavigationLocationUpdate(
        coordinate = fix.coordinate,
        horizontalAccuracyMeters = fix.horizontalAccuracyMeters,
        courseHeadingDegrees = positionController.smoothedCourseDegrees(),
        timestampSeconds = null,
    ),
)
```

The reference applications put this behind **Navigate from my location** and
**Stop device navigation**, which starts the source when needed, ends a
simulated trip first, and stops only the source it started. With no route it
states *Plan a route first; device navigation follows the route you planned.*
See [Show the device's location](positioning.md) for obtaining the fixes.

### Draw the trip on the map

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

### Voice the speech channel

```kotlin
val textToSpeechEngine = TextToSpeech(context, this)
// onSpeechInstruction:
textToSpeechEngine.speak(
    speechInstruction.utteranceSpeechText,
    TextToSpeech.QUEUE_ADD,
    null,
    speechInstruction.utteranceId.toString(),
)
```

### Reroute when guidance freezes

Keep the state machine in your own coordinator:

```kotlin
if (tripState.deviation?.gradeKind == DeviationGradeKind.FULLY_OFF_ROUTE &&
    navigationController.beginRerouteCalculation()
) {
    coroutineScope.launch {
        val replacement = mapSdk.calculateRoute(
            listOf(currentFixCoordinate, originalDestination),
            routeOptions,
        )
        navigationController.applyReroute(replacement, defaultStepAdvancePolicy())
    }
} else {
    navigationController.cancelRerouteCalculation()
}
```

### End the trip

```kotlin
navigationController.stopNavigation()
```

</div>

<div class="platform-tab-panel" data-platform-panel="ios">

### Build the controller and subscribe

The same four callbacks arrive on a Swift object that conforms to
`NavigationListener`:

```swift
import RusticMapsSDK

final class GuidanceListener: NavigationListener {
    func onNavigationInstruction(instruction: NavigationInstruction) {
        // Update the banner instruction.
    }

    func onSpeechInstruction(speechInstruction: SpeechInstruction) {
        // Hand the verbatim text to your speech engine.
    }

    func onDeviationStatusChange(deviationStatusChange: DeviationStatusChange) {
        // Update the reroute affordance.
    }

    func onNavigationPhaseChange(navigationPhaseChange: NavigationPhaseChange) {
        // Drive the phase state.
    }
}

let navigationController = Navigator()
let listener = GuidanceListener()

// One call subscribes all four topics; cancel() removes all four.
let subscription = navigationController.addNavigationListener(listener: listener)
```

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

### Draw the trip on the map

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

### Voice the speech channel

```swift
let speechEngine = AVSpeechSynthesizer()
// In onSpeechInstruction:
let utterance = AVSpeechUtterance(string: speechInstruction.utteranceSpeechText)
utterance.voice = AVSpeechSynthesisVoice(language: "en-US")
speechEngine.speak(utterance)
```

### Reroute when guidance freezes

```swift
if tripState.deviation?.gradeKind == .fullyOffRoute {
    if navigationController.beginRerouteCalculation() {
        let replacement = try await mapSdk.calculateRoute(
            waypoints: [currentFixCoordinate, originalDestination],
            routeOptions: routeOptions
        )
        try navigationController.applyReroute(
            route: replacement,
            stepAdvancePolicy: defaultStepAdvancePolicy()
        )
    } else {
        navigationController.cancelRerouteCalculation()
    }
}
```

### End the trip

```swift
navigationController.stopNavigation()
```

</div>

<div class="platform-tab-panel" data-platform-panel="react-native">

### Build the controller and subscribe

```js
import {
  Navigator,
  UnitSystem,
  defaultStepAdvancePolicy,
  defaultDeviationPolicy,
} from '@angoratek/rustic-maps-sdk';

const navigationController = new Navigator();

// One listener implements the four callbacks the contract carries; one call
// subscribes all four, and cancel() removes them together.
const listener = {
  onNavigationInstruction(instruction) {
    setNextManeuver({
      instructionText: instruction.activeManeuver.instruction.composedDisplayText,
      distancePhrase: instruction.activeManeuver.distancePhrase,
      isReroutingRequired: instruction.isReroutingRequired,
    });
  },
  onSpeechInstruction(speechInstruction) {
    speak(speechInstruction.utteranceSpeechText);
  },
  onDeviationStatusChange(deviationStatusChange) {
    setDeviation(deviationStatusChange);
  },
  onNavigationPhaseChange(navigationPhaseChange) {
    setPhase(navigationPhaseChange);
  },
};

const subscription = navigationController.addNavigationListener(listener);
```

Each callback delivers the same payloads the native platforms receive, in
lower camel case.

### Start the trip

```js
navigationController.startNavigation(
  route,
  UnitSystem.Metric,
  defaultStepAdvancePolicy(),
  defaultDeviationPolicy(),
);
```

### Deliver position fixes

Deliver one fix per sample of your location source or its recorded trace:

```js
navigationController.updateLocation({
  coordinate: { latitude: sampleLatitude, longitude: sampleLongitude },
  horizontalAccuracyMeters: 8.0,
  courseHeadingDegrees: undefined,
  timestampSeconds: undefined,
});

const tripState = navigationController.currentTripState();
```

### Draw the trip on the map

Put the route on once, then drive the next-maneuver card from the listener
and the follow camera from each accepted fix:

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

### Voice the speech channel

Hand the verbatim text to the platform's speech engine, such as Expo's Speech,
and use the delivery's `utteranceId` for deduplication:

```js
function onSpeechInstruction(speechInstruction) {
  Speech.speak(speechInstruction.utteranceSpeechText, {
    language: 'en-US',
    onDone: () => markUtteranceComplete(speechInstruction.utteranceId),
  });
}
```

### Reroute when guidance freezes

```js
import { DeviationGradeKind } from '@angoratek/rustic-maps-sdk';

const tripState = navigationController.currentTripState();
if (
  tripState?.deviation?.gradeKind === DeviationGradeKind.FullyOffRoute &&
  navigationController.beginRerouteCalculation()
) {
  const replacement = await mapSdk.calculateRoute(
    [
      { latitude: fix.latitude, longitude: fix.longitude },
      destinationPlace.coordinate,
    ],
    routeOptions,
  );
  navigationController.applyReroute(
    replacement,
    defaultStepAdvancePolicy(),
  );
} else {
  navigationController.cancelRerouteCalculation();
}
```

### End the trip

```js
navigationController.stopNavigation();
```

</div>

<div class="platform-tab-panel" data-platform-panel="wasm">

### Start the navigator on the route

The browser `Navigator` carries `startNavigation`, `updateLocation`,
`applyReroute`, `stopNavigation`, `currentTripState` and its typed
subscriptions. Create the navigator on the planned route, start it in metric
units with the default policies, and drive it with the same walker the native
tabs use. `updateLocation` feeds the fix; `currentTripState` reads the trip
snapshot:

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
printed twice. `applyReroute` swaps the route without restarting the navigator.

### Simulate the trip and follow it

Ask the simulator for a fix at an elapsed time. Draw the route, place the
location indicator on the fix, and present a course-up camera frame:

```js
import { RouteSimulator } from './web/rustic_maps.js';

/** @type {any} */
const plannedRoute = route;
const routeOverlay = {
  kind: 'routePolyline',
  coordinates: plannedRoute.legs.flatMap(
    (leg) => leg.steps.flatMap((step) => step.shape),
  ),
  color: { red: 0.16, green: 0.47, blue: 1.0, alpha: 0.95 },
  widthPixels: 6.0,
};

const ROUTE_SIMULATOR_SPEED_METERS_PER_SECOND = 1.4;
const ROUTE_SIMULATOR_SPEED_MULTIPLIER = 20.0;
const FOLLOW_ZOOM_LEVEL = 17.0;
const FOLLOW_PITCH_DEGREES = 45.0;
const fieldOfViewDegrees = 60.0;
const elapsedSeconds = 0.0;

const routeSimulator = new RouteSimulator(plannedRoute, {
  speedMetersPerSecond: ROUTE_SIMULATOR_SPEED_METERS_PER_SECOND,
  speedMultiplier: ROUTE_SIMULATOR_SPEED_MULTIPLIER,
});
const fix = routeSimulator.fixAtElapsedSeconds(elapsedSeconds);

mapView.setOverlays([routeOverlay]);
mapView.setLocationIndicator(
  { coordinate: fix.coordinate, courseHeadingDegrees: fix.courseHeadingDegrees },
  { visualKind: 'guidedCourseArrow' },
);
await mapView.renderFrame(
  new MapCamera(
    fix.coordinate.latitude,
    fix.coordinate.longitude,
    FOLLOW_ZOOM_LEVEL,
    fix.courseHeadingDegrees ?? 0.0,
    FOLLOW_PITCH_DEGREES,
  ),
  fieldOfViewDegrees,
);
```

Advance `elapsedSeconds` on your own tick and call `fixAtElapsedSeconds` again
for each frame; `fixAtTravelledMeters` is the distance-driven equivalent.
`totalDistanceMeters()` and `totalDurationSeconds()` size the progress bar, and
`isArrived` turns true exactly at the final coordinate.

</div>

</div>
