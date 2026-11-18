# Device positioning research and design

**Status:** research for M33 (owner-directed 2026-10-31). Sources are MapLibre
Native's Android and iOS documentation and source, the browser Geolocation
API, and the two maintained Rust positioning crates. This page records what
the platforms offer, what the SDK should own, and the recommended split
between Rust and each host.

## 1. What MapLibre Native does

### Android

- **`LocationComponent`** (`org.maplibre.android.location`) is the public
  entry point. An application activates it with
  `LocationComponentActivationOptions` and then calls
  `setLocationComponentEnabled(true)`. It owns the location engine, the
  request parameters, the compass and the render mode.
- **`LocationEngine`** is the abstraction behind it: `requestLocationUpdates`,
  `removeLocationUpdates`, `getLastLocation`, with an engine callback and a
  request/result pair. Since MapLibre Native Android 10.0.0 the default is
  `MapLibreFusedLocationEngineImpl`, a FLOSS engine over `android.location`
  `LocationManager` with local fusion; Google Play Services'
  `FusedLocationProviderClient` engine was removed and is now an application-
  supplied `LocationEngine` implementation.
- **Render modes**: `NORMAL` (position only), `COMPASS` (bearing from the
  compass), `GPS` (bearing from the location fix). The puck is a style layer
  (`LocationIndicatorLayer`) with accuracy radius, accuracy color, bearing,
  top/bearing/shadow images, tilt displacement and perspective compensation.
- **Permissions and lifecycle**: the application requests `ACCESS_FINE_LOCATION`
  (and coarse) and owns start/stop; the component requests and removes
  updates as it is enabled/disabled and as the lifecycle changes.

### iOS

- **`MLNMapView.showsUserLocation`** turns Core Location tracking on. The
  default manager requests authorization when the status is undetermined
  (`shouldRequestAuthorizationToUseLocationServices`), and the application
  must provide `NSLocationWhenInUseUsageDescription` (or Always).
- **`MLNLocationManager`** is the injection point: a protocol with
  `delegate`, `authorizationStatus`, `headingOrientation`, `desiredAccuracy`,
  `distanceFilter`, authorization requests and start/stop updates. An
  application can substitute any implementation (MapLibre's sample wraps
  `CLLocationManager`), set before enabling `showsUserLocation`.
- **Callbacks**: `mapView:didUpdateUserLocation:` fires while tracking and on
  heading/course updates; `mapView:didFailToLocateUserWithError:` reports
  failures. Background updates are not supported by the view.

### Common shape

Both platforms expose the same three concerns, which is the shape the SDK
should mirror:

1. **A source** that produces fixes (position, accuracy, course, timestamp)
   and needs a permission decision the host owns.
2. **A render surface** for the fix: accuracy circle, course arrow, pulsing
   disc, plus follow-camera modes.
3. **A lifecycle** — start/stop with the host's foreground/background rules.

## 2. Browser and Rust

- **Browser**: `navigator.geolocation.watchPosition` / `getCurrentPosition`
  provide position, accuracy, altitude, heading and speed, with a permission
  prompt owned by the browser. Errors are typed codes (permission denied,
  position unavailable, timeout). Secure contexts only.
- **Rust**: `robius-location` abstracts Core Location, Android `jni`,
  Windows Geolocation and the Linux XDG desktop portal/GeoClue behind one
  API; `waterkit-location` offers an async API with explicit backend
  introspection and typed permission errors. Both confirm the same three
  concerns (source, permission, availability) and both require platform
  packaging (Info.plist keys, Android permissions, GeoClue service).
- A pure-Rust desktop source is possible through either crate, but it is an
  *optional* platform detail, not something the rendering core should depend
  on.

## 3. Recommended architecture

### Rust owns the pipeline, hosts own the system API

```
host source (Android / iOS / RN / browser / desktop)
        │  PositionFix { coordinate, accuracyMeters, courseDegrees?, timestampSeconds? }
        ▼
PositionController (Rust)
  - validates and de-duplicates fixes
  - smooths course (circular median over N fixes)
  - applies accuracy and staleness rules
  - exposes the latest fix + a change stream
        │
        ├── renderer location indicator (accuracy ring, course arrow)
        ├── follow camera (center/bearing chase, the existing camera mode)
        └── navigator update (NavigationLocationUpdate)
```

- **`PositioningSource`** is the host-side producer. It is not a UniFFI
  foreign trait at first: the contract exposes a `PositionController` that the
  host feeds with `positioningSourceFixed(PositionFix)` /
  `positioningSourceFailed(PositionSourceError)` calls, and the host owns the
  platform API and permission prompt. This keeps permissions, lifecycle and
  threading where the OS requires them while Rust keeps one validated stream
  and one policy.
- **`PositionController`** is the Rust-owned state: latest fix, smoothed
  course, accuracy ring radius, staleness, follow mode, and a subscription
  for the reference apps' status row. It reuses the existing
  `NavigationLocationUpdate` conversion instead of inventing a second fix
  record.
- A **`SimulatedPositioningSource`** ships in Rust for tests, desktop demos
  and the existing recorded-trace demo; it is the default where no OS source
  is wired.
- Permission is deliberately **not** in the contract's render path: hosts
  expose their own permission state (Android runtime permission result, iOS
  `authorizationStatus`, browser permission result) and report the resulting
  source errors to `PositionController`.

### Platform implementations

| Target | Source | Permission owner | Reference-app control |
| --- | --- | --- | --- |
| Rust | `robius-location`/`waterkit-location`, or the simulated source | host/OS | example binary |
| Android | `android.location.LocationManager` with an optional fused engine behind a `LocationEngine`-shaped interface | runtime permission helper in the Kotlin SDK | "Show my location" |
| iOS | `CLLocationManager` behind an injectable protocol | `Info.plist` + authorization request | "Show my location" |
| React Native | native bridges on both hosts; JS consumer feeds the controller | host | "Show my location" |
| WebAssembly | `navigator.geolocation.watchPosition` wrapper | browser prompt | "Show my location" |

### Contract additions (addition-only)

- `PositionFix` dictionary: coordinate, horizontal accuracy meters, optional
  course degrees, optional timestamp seconds, optional altitude.
- `PositionSourceError` enum: permission denied, service disabled, source
  unavailable, timeout.
- `PositionController` interface: `sourceFixed(fix)`,
  `sourceFailed(error)`, `latestFix()`, `smoothedCourseDegrees()`,
  `isStale(nowSeconds)`, `startFollowing(mode)`, `stopFollowing()`, plus a
  listener for status rows.
- No `PositioningSource` foreign trait in the first slice; add it only if a
  Rust-side consumer genuinely needs to poll a foreign source rather than
  receive pushes.

## 4. Open questions to confirm with the owner

1. Should the Android SDK bundle a Google Play Services fused engine, or ship
   the FLOSS `LocationManager` source and document the fused option as an
   application-supplied implementation? (MapLibre's own direction is FLOSS by
   default.)
2. Does positioning include a foreground service / background updates, or only
   while the app is visible? (MapLibre's view stops in background; a
   navigation app may need background updates later.)
3. For Rust desktop, is an OS-backed source required for the first release, or
   is the simulated source enough until a real desktop consumer appears?

## 5. Verification plan

- Rust unit tests for validation, de-duplication, course smoothing, staleness
  and simulated-fix delivery.
- Android instrumentation: permission grant/denial, a stubbed engine feeding a
  fix, the indicator and follow camera reacting.
- iOS XCUITest: authorization states via launch arguments, a stubbed
  `MLNLocationManager`-style source, indicator and camera.
- Browser Playwright: stubbed `navigator.geolocation`, permission/error states,
  indicator and follow camera.
- One tutorial page per platform with the same scenario and wording; the
  parity floor asserts each target's positioning path.
