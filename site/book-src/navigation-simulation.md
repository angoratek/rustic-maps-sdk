# Turn-by-turn navigation

Start a trip on a route, deliver position fixes, and receive turn-by-turn
guidance: the banner instruction, speech triggers, deviation status, and
phase. This guide turns the route you planned in [Search and
routing](search-and-routing.md) into a simulated Capitol Park to Tower Bridge
drive. Every code block compiles against the [FFI contract](udl-contract.md).

<div class="platform-tabs">
<div class="platform-tab-titles" role="tablist">
<button type="button" data-platform-tab="android">Android</button>
<button type="button" data-platform-tab="ios">iOS</button>
<button type="button" data-platform-tab="react-native">React Native</button>
<button type="button" data-platform-tab="wasm">WebAssembly</button>
</div>

<div class="platform-tab-panel" data-platform-panel="android">

### Build the controller and subscribe first

Subscribe before you start the trip, so you receive the initial instruction
dispatch. The listener implements the four callbacks the contract carries:

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

A subscription is cancelled with its handle when your screen leaves;
dropping the reference does not unsubscribe.

### Start the trip

Use the default policies unless you measured different ones for your
product:

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

The initial instruction payload dispatches before the call returns; the
banner shows the first maneuver.

### Deliver position fixes

Deliver one fix at a time, as a live location source or a recorded trace
does:

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

Fixes without a reported accuracy never advance a step and are never
judged for deviation, so a simulated source must carry its accuracy
like a real fix does.

### Drive the trip from the device

A trip does not have to be simulated. Feed the `Navigator` the fixes your
location source produces and the same banner, ETA line and deviation
judgment follow the real journey. Start the trip on the planned route, then
call `updateLocation` with each accepted fix's coordinate, its horizontal
accuracy and the `PositionController`'s smoothed course:

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

The reference applications put this behind one control in the Location
group, **Navigate from my location** and **Stop device navigation**, which
starts the location source when needed, ends a simulated trip first, and
stops only the source it started. It states *Plan a route first; device
navigation follows the route you planned.* when no route exists. See
[Show the device's location](positioning.md) for obtaining the fixes.

### Draw the trip on the map

Navigation state is half the experience; the map also has to show the route
and follow the vehicle. Two calls do it: the route polyline goes on as an
overlay, and each fix rebuilds the camera.

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

Keep the bearing smoothed rather than raw: a fix-to-fix course jitters at
low speed, and a camera that follows it reads as shaky. When the user pans
away, stop drawing the follow camera until they tap the location control to
re-center.

### Voice the speech channel

The platform owns playback; the SDK owns when a line is newsworthy.
When `onSpeechInstruction` delivers a `SpeechInstruction`, hand its
verbatim text to the platform's speech engine:

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

The utterance id is the contract's monotonic id, so your dedup keeps
its identity across repeats.

### Reroute when guidance freezes

A deviating trip asks for a replacement route through the controller's
checks. Keep the state machine in your own coordinator:

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
// Clears the active trip and its state; later fixes and reroutes are
// dropped until the next startNavigation. Subscriptions stay registered
// and serve the next trip.
navigationController.stopNavigation()
```

</div>

<div class="platform-tab-panel" data-platform-panel="ios">

### Build the controller and subscribe first

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

Put the route on the map once and follow it with every fix:

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

The camera follows the smoothed course, not the raw fix-to-fix bearing, and a
pan gesture hands control back to the user until the location control is
tapped again.

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

The trip state readback (`currentTripState()`) carries the phase, the
progress, and `isCalculatingNewRoute` so your UI can show the
recalculation honestly.

### End the trip

```swift
// Clears the active trip and its state; subscriptions stay registered.
navigationController.stopNavigation()
```

</div>

<div class="platform-tab-panel" data-platform-panel="react-native">

### Build the controller and subscribe first

Subscribe before you start the trip, so you receive the initial instruction
dispatch. The package exposes the same navigation controller to JavaScript:

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

Each callback delivers the same payloads the native platforms receive, with
lower camel case field names. Cancel the subscription with its handle when
your screen leaves; dropping the reference does not unsubscribe.

### Start the trip

Use the default policies unless you measured different ones for your
product:

```js
navigationController.startNavigation(
  route,
  UnitSystem.Metric,
  defaultStepAdvancePolicy(),
  defaultDeviationPolicy(),
);
```

The initial instruction payload dispatches before the call returns; the
banner shows the first maneuver.

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

Fixes without a reported accuracy never advance a step and are never judged
for deviation, so a simulated source must carry its accuracy like a real fix
does.

### Draw the trip on the map

The package's `MapView` handle exposes the overlay, camera and
location-indicator members the contract carries. Put the route on once, then
drive the next-maneuver card from the listener subscribed above and the follow
camera from each accepted fix:

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
// the shared demo values). The handle takes the same five-value pose the
// other tabs use, and the guided arrow sits over the vehicle.
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

The camera follows the smoothed course, not the raw fix-to-fix bearing, and
a pan gesture hands control back to the user until the location control is
tapped again.

### Voice the speech channel

The SDK triggers; your application voices. In the speech callback, hand
the verbatim text to the platform's speech engine (Expo's Speech, or
your own TTS wrapper), and use the delivery's `utteranceId` for dedup:

```js
function onSpeechInstruction(speechInstruction) {
  Speech.speak(speechInstruction.utteranceSpeechText, {
    language: 'en-US',
    onDone: () => markUtteranceComplete(speechInstruction.utteranceId),
  });
}
```

### Reroute when guidance freezes

A deviating trip asks for a replacement route through the controller's
reroute checks. Keep the state machine in your own coordinator:

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

The trip state readback carries the phase, the progress and
`isCalculatingNewRoute`, so your UI can show the recalculation honestly.

### End the trip

```js
// Clears the active trip and its state; subscriptions stay registered.
navigationController.stopNavigation();
```

</div>

<div class="platform-tab-panel" data-platform-panel="wasm">

### Start the navigator on the route

The browser `Navigator` carries the driving and readback calls:
`startNavigation`, `updateLocation`, `applyReroute`, `stopNavigation`, and
`currentTripState`, plus `hasActiveTrip`. `addNavigationListener` registers a
JavaScript callback that receives one event per topic: instruction, phase,
deviation, speech, or stop. `removeNavigationListener` removes it by id.

Create the navigator on the route you planned and drive it with the same
walker the native tabs use. Each `updateLocation` returns the trip snapshot;
the phase, the instruction payload, the progress numbers, and the deviation
all come from that one call:

```js
import { Navigator, RouteSimulator } from './web/rustic_maps_web_bindings.js';

const navigationController = new Navigator();
navigationController.startNavigation(route, 'metric');

const navigationRouteSimulator = new RouteSimulator(route, {
  speedMetersPerSecond: 1.4,
  speedMultiplier: 20.0,
});

const navigationFix = navigationRouteSimulator.fixAtElapsedSeconds(0.0);
const tripSnapshot = navigationController.updateLocation({
  coordinate: navigationFix.coordinate,
  horizontalAccuracyMeters: 8.0,
  courseHeadingDegrees: navigationFix.courseHeadingDegrees,
  timestampSeconds: null,
});

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

A fix without an accuracy is never judged and never advances a step, so a
simulated source carries its accuracy like a real fix. `stopNavigation` ends
the trip, and `applyReroute` swaps the route without restarting the
navigator. The banner keeps two lines: `distancePhraseText` names the
distance and `providerDisplayText` is the provider's own sentence, so the
distance is not printed twice.

### Simulate the trip and follow it

Create the simulator once from the planned route and a pace, then ask it for
a fix at an elapsed time. The fix carries a `coordinate`, the
`courseHeadingDegrees`, the travelled and remaining meters, and
`isArrived`. Draw the route, place the location indicator on the fix, and
present a course-up camera frame:

```js
import { RouteSimulator } from './web/rustic_maps_web_bindings.js';

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

Advance `elapsedSeconds` on your own tick and call `fixAtElapsedSeconds`
again for each frame; `fixAtTravelledMeters` is the distance-driven
equivalent. `totalDistanceMeters()` and `totalDurationSeconds()` size the
trip's progress bar, and `isArrived` turns true exactly at the final
coordinate. The route and step records are the same
`Route`/`RouteLeg`/`RouteStep` objects the native surfaces return, so the
polyline extraction is identical.

</div>

</div>
