import CoreLocation
import RusticMapsSDK

/// What the application uses position fixes for; selects the accuracy Core
/// Location is asked for.
public enum LocationConsumerMode {
    /// Showing the device on the map and filling route fields: ten-meter
    /// accuracy, enough for the indicator without draining the battery.
    case idle
    /// Turn-by-turn guidance: the highest accuracy Core Location offers for
    /// navigation, which the heading and along-route presentation need.
    case navigation
}

/// Feeds Core Location into the contract's `PositionController`.
///
/// The application owns the authorization prompt (its `Info.plist` declares
/// `NSLocationWhenInUseUsageDescription`) and calls ``start()`` after the user
/// grants it. The source reports every accepted fix to the controller with the
/// platform's accuracy, course and timestamp; authorization and service
/// failures are reported to the controller and to the optional failure
/// closure, so the controller's state and the UI agree.
///
/// The source accepts every fix whose horizontal accuracy is non-negative and
/// whose age is at or below the controller's staleness bound. A fix with a
/// negative horizontal accuracy, or one older than that bound, is dropped
/// before it reaches the controller; every other fix is forwarded, and a
/// controller refusal is reported through the failure callback.
///
/// A `.locationUnknown` failure is transient — Core Location retries on its
/// own — so it neither fails the session nor marks the source unavailable.
///
/// Background updates are opt-in through
/// ``init(positionController:consumerMode:enablesBackgroundLocationUpdates:onPositioningFailure:onFix:onAuthorizationResolved:)``
/// and are enabled only when the application actually declares them: the app
/// must add the `location` value to its `UIBackgroundModes` array, declare
/// `NSLocationAlwaysAndWhenInUseUsageDescription`, and the visitor must grant
/// Always authorization. Without all three the source leaves background
/// updates off instead of requesting a capability the bundle cannot carry.
///
/// One controller per map session is shared by the location indicator, the
/// follow camera and the navigator, so this source takes it rather than
/// creating one.
public final class DeviceLocationSource: NSObject, CLLocationManagerDelegate {
    /// The controller every fix and failure is reported to.
    public let positionController: PositionController

    /// The platform's location manager.
    private let locationManager: CLLocationManager

    /// Whether the application's bundle declares the `location` background
    /// mode; production reads `Info.plist`, tests inject it.
    private let declaresLocationBackgroundMode: () -> Bool

    /// Receives a refused fix's verbatim detail, when the application wants it.
    private let onPositioningFailure: ((String) -> Void)?

    /// Receives every accepted fix, when the application wants to present it.
    private let onFix: ((PositionFix) -> Void)?

    /// Receives a resolved authorization status, so the application can start
    /// the source the visitor asked for once the prompt is answered.
    private let onAuthorizationResolved: ((CLAuthorizationStatus) -> Void)?

    /// Whether the application asked for updates that continue while the app
    /// is in the background.
    private let wantsBackgroundUpdates: Bool

    /// Whether the caller has asked for updates. The prompt is answered
    /// asynchronously, so the request outlives the first ``start()``.
    private var wantsUpdates = false

    /// Whether the source is currently updating.
    public private(set) var isActive = false

    /// The consumer mode in force; changing it updates Core Location's
    /// requested accuracy in place, so a running session follows the switch
    /// from map browsing to navigation and back.
    public var consumerMode: LocationConsumerMode {
        didSet {
            guard consumerMode != oldValue else { return }
            applyDesiredAccuracy()
        }
    }

    /// Whether updates are configured to continue in the background: true
    /// only when the application requested it, declares the location
    /// background mode, and the visitor granted Always authorization.
    public private(set) var isBackgroundLocationConfigured = false

    /// The desired accuracy the running manager is configured with (internal
    /// so the simulator tests can assert the per-mode choice).
    var configuredDesiredAccuracy: CLLocationAccuracy {
        locationManager.desiredAccuracy
    }

    /// Builds the source over a controller, without starting it.
    ///
    /// - Parameters:
    ///   - positionController: the controller fixes and failures are reported
    ///     to.
    ///   - consumerMode: the accuracy the first session asks for; idle until
    ///     navigation starts.
    ///   - enablesBackgroundLocationUpdates: requests background updates. The
    ///     source enables them only when the application declares the
    ///     `location` background mode and the visitor grants Always
    ///     authorization; otherwise the request changes nothing.
    ///   - onPositioningFailure: receives a refused or failed fix's verbatim
    ///     detail.
    ///   - onFix: receives every accepted fix.
    ///   - onAuthorizationResolved: receives a resolved authorization status.
    public convenience init(
        positionController: PositionController,
        consumerMode: LocationConsumerMode = .idle,
        enablesBackgroundLocationUpdates: Bool = false,
        onPositioningFailure: ((String) -> Void)? = nil,
        onFix: ((PositionFix) -> Void)? = nil,
        onAuthorizationResolved: ((CLAuthorizationStatus) -> Void)? = nil
    ) {
        self.init(
            positionController: positionController,
            consumerMode: consumerMode,
            enablesBackgroundLocationUpdates: enablesBackgroundLocationUpdates,
            locationManager: CLLocationManager(),
            declaresLocationBackgroundMode: {
                Self.applicationDeclaresLocationBackgroundMode()
            },
            onPositioningFailure: onPositioningFailure,
            onFix: onFix,
            onAuthorizationResolved: onAuthorizationResolved
        )
    }

    /// Builds the source over an injected location manager and background-mode
    /// declaration, so tests exercise the authorization transitions without
    /// touching a device.
    init(
        positionController: PositionController,
        consumerMode: LocationConsumerMode = .idle,
        enablesBackgroundLocationUpdates: Bool = false,
        locationManager: CLLocationManager,
        declaresLocationBackgroundMode: @escaping () -> Bool,
        onPositioningFailure: ((String) -> Void)? = nil,
        onFix: ((PositionFix) -> Void)? = nil,
        onAuthorizationResolved: ((CLAuthorizationStatus) -> Void)? = nil
    ) {
        self.positionController = positionController
        self.consumerMode = consumerMode
        self.wantsBackgroundUpdates = enablesBackgroundLocationUpdates
        self.locationManager = locationManager
        self.declaresLocationBackgroundMode = declaresLocationBackgroundMode
        self.onPositioningFailure = onPositioningFailure
        self.onFix = onFix
        self.onAuthorizationResolved = onAuthorizationResolved
        super.init()
        locationManager.delegate = self
        applyDesiredAccuracy()
        locationManager.distanceFilter = Self.minimumDistanceMeters
    }

    /// The current authorization status.
    public var authorizationStatus: CLAuthorizationStatus {
        locationManager.authorizationStatus
    }

    /// Asks for when-in-use authorization. The application calls this before
    /// ``start()`` while the status is `.notDetermined`. The resolved status is
    /// reported to ``onAuthorizationResolved`` even though location updates
    /// have not been requested yet — that callback is what the documented
    /// prompt-then-start flow waits for.
    public func requestWhenInUseAuthorization() {
        isAwaitingAuthorizationResolution = true
        locationManager.requestWhenInUseAuthorization()
    }

    /// Whether the application has asked for authorization and the prompt has
    /// not resolved yet. The delegate reports the resolved status exactly once
    /// per request, whether it came from ``requestWhenInUseAuthorization()``
    /// or from ``start()``.
    private var isAwaitingAuthorizationResolution = false

    /// Starts location updates. Answers `false` when the permission is
    /// undetermined (the prompt was requested and the caller should retry
    /// after it resolves), denied or restricted, or when location services are
    /// disabled; the controller records the failure.
    @discardableResult
    public func start() -> Bool {
        wantsUpdates = true
        if isActive {
            return true
        }
        switch locationManager.authorizationStatus {
        case .notDetermined:
            isAwaitingAuthorizationResolution = true
            locationManager.requestWhenInUseAuthorization()
            return false
        case .denied, .restricted:
            positionController.sourceFailed(error: .permissionDenied)
            return false
        case .authorizedAlways, .authorizedWhenInUse:
            break
        @unknown default:
            positionController.sourceFailed(error: .unavailable)
            return false
        }
        guard CLLocationManager.locationServicesEnabled() else {
            positionController.sourceFailed(error: .serviceDisabled)
            return false
        }
        configureBackgroundUpdatesIfAuthorized()
        locationManager.startUpdatingLocation()
        isActive = true
        return true
    }

    /// Stops location updates; a later ``start()`` resumes the same controller.
    public func stop() {
        wantsUpdates = false
        locationManager.stopUpdatingLocation()
        isActive = false
    }

    /// Starts the updates the visitor asked for once the authorization prompt
    /// resolves, and reports the resolved status to the application. A
    /// permission refusal is reported through the controller's source status.
    public func locationManagerDidChangeAuthorization(_ manager: CLLocationManager) {
        // Core Location reports the current status when the delegate is
        // installed; only a status that answers a request this source made is
        // the application's to act on. The request can come from
        // `requestWhenInUseAuthorization()` directly (the documented
        // prompt-then-start flow), not only from `start()`.
        let resolvedStatus = manager.authorizationStatus
        if isAwaitingAuthorizationResolution, resolvedStatus != .notDetermined {
            isAwaitingAuthorizationResolution = false
            onAuthorizationResolved?(resolvedStatus)
        }
        guard wantsUpdates else { return }
        if isActive {
            // A change while updates run is the system revoking, restricting
            // or upgrading the permission; the separate Core Location error
            // is not guaranteed to arrive after an already-granted session.
            switch resolvedStatus {
            case .denied, .restricted:
                locationManager.stopUpdatingLocation()
                isActive = false
                positionController.sourceFailed(error: .permissionDenied)
                onAuthorizationResolved?(resolvedStatus)
            case .authorizedAlways, .authorizedWhenInUse:
                // An Always upgrade enables background updates; a downgrade
                // to WhenInUse turns them off again.
                configureBackgroundUpdatesIfAuthorized()
            case .notDetermined:
                break
            @unknown default:
                locationManager.stopUpdatingLocation()
                isActive = false
                positionController.sourceFailed(error: .unavailable)
                onAuthorizationResolved?(resolvedStatus)
            }
            return
        }
        switch resolvedStatus {
        case .authorizedAlways, .authorizedWhenInUse:
            start()
        case .denied, .restricted:
            positionController.sourceFailed(error: .permissionDenied)
        case .notDetermined:
            break
        @unknown default:
            positionController.sourceFailed(error: .unavailable)
        }
    }

    /// Reports one platform fix to the controller after refusing the fixes the
    /// SDK's consumers cannot use.
    public func locationManager(
        _ manager: CLLocationManager,
        didUpdateLocations locations: [CLLocation]
    ) {
        guard let location = locations.last else { return }
        // Core Location uses a negative accuracy for "unknown"; such a fix has
        // no usable position envelope and is not a position report.
        guard location.horizontalAccuracy >= 0 else { return }
        // A fix older than the controller's staleness bound is already stale
        // for every consumer, so it is refused at the source instead of being
        // fed and immediately discarded downstream.
        let fixAgeSeconds = Date().timeIntervalSince(location.timestamp)
        guard fixAgeSeconds <= positionController.staleAfterSeconds() else { return }
        let positionFix = PositionFix(
            coordinate: GeographicCoordinate(
                latitude: location.coordinate.latitude,
                longitude: location.coordinate.longitude
            ),
            horizontalAccuracyMeters: location.horizontalAccuracy,
            courseHeadingDegrees: location.course >= 0 ? location.course : nil,
            timestampSeconds: location.timestamp.timeIntervalSince1970
        )
        do {
            try positionController.feedFix(fix: positionFix)
            onFix?(positionFix)
        } catch {
            onPositioningFailure?(String(describing: error))
        }
    }

    /// Reports a source failure to the controller. `.locationUnknown` is
    /// transient (Core Location keeps retrying and the next fix clears it), so
    /// it is deliberately not recorded as a source failure.
    public func locationManager(
        _ manager: CLLocationManager,
        didFailWithError error: Error
    ) {
        if let coreLocationError = error as? CLError,
           coreLocationError.code == .locationUnknown {
            return
        }
        positionController.sourceFailed(error: Self.sourceError(for: error))
        onPositioningFailure?(error.localizedDescription)
    }

    /// Applies the accuracy the consumer mode asks for.
    private func applyDesiredAccuracy() {
        switch consumerMode {
        case .idle:
            locationManager.desiredAccuracy = Self.idleDesiredAccuracyMeters
        case .navigation:
            locationManager.desiredAccuracy = Self.navigationDesiredAccuracyMeters
        }
    }

    /// Enables background updates only when the bundle declares them and the
    /// visitor granted Always authorization; otherwise the flags stay off.
    /// Core Location raises when `allowsBackgroundLocationUpdates` is set for
    /// a bundle without the `location` background mode, so the declaration is
    /// checked first.
    private func configureBackgroundUpdatesIfAuthorized() {
        guard wantsBackgroundUpdates,
              locationManager.authorizationStatus == .authorizedAlways,
              declaresLocationBackgroundMode()
        else {
            isBackgroundLocationConfigured = false
            locationManager.allowsBackgroundLocationUpdates = false
            return
        }
        locationManager.allowsBackgroundLocationUpdates = true
        // Navigation must not be paused by the system when it believes the
        // visitor stopped moving; the source is the trip's fix supply.
        locationManager.pausesLocationUpdatesAutomatically = false
        isBackgroundLocationConfigured = true
    }

    /// Whether the application's `Info.plist` declares the `location` value of
    /// `UIBackgroundModes`.
    private static func applicationDeclaresLocationBackgroundMode() -> Bool {
        let backgroundModes = Bundle.main.object(
            forInfoDictionaryKey: "UIBackgroundModes"
        ) as? [String]
        return backgroundModes?.contains(locationBackgroundModeValue) ?? false
    }

    /// Maps a Core Location failure onto the contract's source status.
    private static func sourceError(for error: Error) -> PositionSourceError {
        guard let coreLocationError = error as? CLError else {
            return .unavailable
        }
        switch coreLocationError.code {
        case .denied:
            return .permissionDenied
        default:
            return .unavailable
        }
    }

    /// The shortest movement between delivered fixes, in meters.
    private static let minimumDistanceMeters: CLLocationDistance = 1

    /// The accuracy the idle consumer asks for: ten meters, enough for the
    /// map indicator and the route fields.
    private static let idleDesiredAccuracyMeters: CLLocationAccuracy =
        kCLLocationAccuracyNearestTenMeters

    /// The accuracy the navigation consumer asks for: Core Location's
    /// navigation grade.
    private static let navigationDesiredAccuracyMeters: CLLocationAccuracy =
        kCLLocationAccuracyBestForNavigation

    /// The `UIBackgroundModes` value that declares background location.
    private static let locationBackgroundModeValue = "location"
}
