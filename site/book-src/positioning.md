# Show the device's location

Show the user's position on the map: a tracking dot with the source's accuracy
ring, a camera centred on the fix, and an honest line when the permission is
denied or the service is unavailable.

The host owns the operating system's location API and its permission prompt.
Every host pushes its fixes into the same Rust `PositionController`, which
validates them, ignores stationary repeats when smoothing course and tracks
staleness. The renderer draws the dot; the camera and the navigation controller
read the same stream.

## The shared scenario

- Start the source after the permission is granted.
- Draw the tracking dot with `showAccuracyRing` and the controller's smoothed
  course.
- Centre the camera on each fix at follow zoom 15.
- Stop the source when the control is turned off and clear the layer.
- State a denied permission, a disabled service, an unavailable source and a
  timeout in the same words on every platform.

## Android

```kotlin
import com.rusticmaps.sdk.DeviceLocationSource
import uniffi.rustic_maps.LocationIndicatorState
import uniffi.rustic_maps.LocationIndicatorStyle
import uniffi.rustic_maps.LocationIndicatorVisualKind
import uniffi.rustic_maps.PositionController

val positionController = PositionController()
val deviceLocationSource = DeviceLocationSource(
    context = context,
    positionController = positionController,
    onFix = { fix ->
        mapView.setLocationIndicator(
            state = LocationIndicatorState(
                coordinate = fix.coordinate,
                courseHeadingDegrees = positionController.smoothedCourseDegrees(),
                horizontalAccuracyMeters = fix.horizontalAccuracyMeters,
            ),
            style = LocationIndicatorStyle(
                visualKind = LocationIndicatorVisualKind.TRACKING_DOT,
                radiusPixels = null,
                rgbaColor = null,
                showAccuracyRing = true,
            ),
        )
    },
)
// Request ACCESS_FINE_LOCATION at runtime first; then:
if (!deviceLocationSource.start()) {
    // Read positionController.sourceStatus() for the honest reason.
}
// Later, when the control is turned off:
deviceLocationSource.stop()
```

## iOS

<!-- docs-snippet: skip — the snippet uses the MapNativeCoreUI view's location layer and its DeviceLocationSource, which the contract-only Swift snippet check does not mirror. -->
```swift
import MapNativeCoreUI
import RusticMapsSDK

let positionController = PositionController()
let deviceLocationSource = DeviceLocationSource(
    positionController: positionController,
    onFix: { fix in
        mapView.setLocationIndicator(
            state: LocationIndicatorState(
                coordinate: fix.coordinate,
                courseHeadingDegrees: positionController.smoothedCourseDegrees(),
                horizontalAccuracyMeters: fix.horizontalAccuracyMeters
            ),
            style: LocationIndicatorStyle(
                visualKind: .trackingDot,
                radiusPixels: nil,
                rgbaColor: nil,
                showAccuracyRing: true
            )
        )
    }
)
// The Info.plist declares NSLocationWhenInUseUsageDescription.
deviceLocationSource.requestWhenInUseAuthorization()
deviceLocationSource.start()
// Later:
deviceLocationSource.stop()
```

## React Native

```ts
import { MapsSdk } from '@angoratek/rustic-maps-sdk';

const sdk = await MapsSdk.create();
const started = await sdk.startDeviceLocation();
if (started) {
  const fix = await sdk.latestDeviceFix();
  const smoothedCourse = await sdk.deviceLocationSmoothedCourseDegrees();
  // Present fix and smoothedCourse through the <MapView> handle's
  // setLocationIndicator and setCamera calls.
}
// The failure name, when the source could not start:
const sourceStatus = await sdk.deviceLocationSourceStatus();
await sdk.stopDeviceLocation();
```

Ask for the Android runtime permission with `PermissionsAndroid` before
`startDeviceLocation`; the iOS authorization prompt is raised by the source.

## WebAssembly

```ts
import { PositionController, PositionSourceError } from './web/rustic_maps.js';

const positionController = new PositionController();
const watchIdentifier = navigator.geolocation.watchPosition(
  position => {
    positionController.feedFix({
      coordinate: {
        latitude: position.coords.latitude,
        longitude: position.coords.longitude,
      },
      horizontalAccuracyMeters: position.coords.accuracy,
    });
    // Draw the tracking dot with the controller's smoothed course and centre
    // the camera through BrowserMapView.setLocationIndicator and setCamera.
  },
  error => {
    positionController.sourceFailed(
      error.code === 1
        ? PositionSourceError.PermissionDenied
        : PositionSourceError.Unavailable,
    );
  },
);
// Later:
navigator.geolocation.clearWatch(watchIdentifier);
```

## Rust

```rust
use rustic_maps_navigation::SimulatedTripPace;
use rustic_maps_positioning::{PositionController, SimulatedPositionSource};

let source = SimulatedPositionSource::new(&route, pace, 5.0)?;
let mut controller = PositionController::new();
controller.feed_fix(source.fix_at_elapsed_seconds(0.0))?;
```

A Rust application supplies fixes from its own OS source (or the simulated
source above) and reads `latest_fix`, `smoothed_course_heading_degrees` and
`is_stale`. See [Use the SDK from Rust](rust.md).
