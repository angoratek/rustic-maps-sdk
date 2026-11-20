//! Pod copy of `platform/ios/Sources/MapNativeCoreUI/MapView.swift` (the
//! `MapsSdkRusticMapView` widget), renamed to leave the generated binding's
//! `MapView` type unambiguous inside the package module. The two copies are
//! kept in sync by the iOS packaging slice; the SwiftPM package and this pod
//! intentionally expose the same widget.
//! The lifecycle-aware iOS map view shipped in the `MapNativeCoreUI`
//! package: it owns the Metal-backed surface, the surface-pointer
//! handoff, the `CADisplayLink` frame pump, resize, the camera value store
//! and animation, gestures, the overlay and location-layer bookkeeping and
//! the delegate callbacks. The application owns only its widgets and its
//! own camera intent.
//!
//! ## Drawing
//!
//! There is deliberately no SDK-internal render loop: this view pumps
//! explicit frames from a `CADisplayLink` and follows the documented
//! idle-settled discipline (the public API guide §10):
//!
//! 1. resize when the capped pixel extent changed, never every frame;
//! 2. ask `isFrameSettled`; when it answers true the pump pauses entirely
//!    instead of burning frames on an unchanged, fully presented view;
//! 3. otherwise draw with at most one draw in flight
//!    (`RenderingSurfaceUnavailable` retries on the next tick);
//! 4. report statistics, readiness and failures.
//!
//! The pump resumes on a camera set or animation, a gesture, an overlay or
//! location mutation, a surface attach/resize, window attach or visibility,
//! `setRenderingPaused(false)`, `retryAttach` and a one-second watchdog.
//! When only unavailable tiles remain with an unchanged pose, the pump
//! drops to that one-second retry cadence rather than spinning against the
//! provider.
//!
//! ## Lifecycle
//!
//! The display link starts in `didMoveToWindow` when the view gains a
//! window and is invalidated when it loses one; the surface detaches at the
//! same moment. The SwiftUI wrapper calls `teardown()` from
//! `dismantleUIView`. Detaching never happens in `deinit`: the Rust side
//! must release its surface while the Metal layer still exists.
//!
//! The view also observes the application's lifecycle. Entering the
//! background releases the GPU surface and pauses the pump, so the adapter's
//! resources are not held while nothing is visible; returning to the
//! foreground resumes the pump, which re-attaches the surface on the next
//! display tick. A memory warning releases the surface too, and the idle
//! watchdog retries the attach so a foreground map restores itself.
//! `setRenderingPaused(true)` follows the same rule: a paused host releases
//! the surface instead of leaving it allocated.

import QuartzCore
import UIKit

/// The camera values a `MapsSdkRusticMapView` reads back from the SDK's anchored
/// camera. The SDK owns the math; this plain value is what the wrapper
/// exposes so an integrator never holds a `MapCamera` handle.
public struct MapsSdkCameraPose: Equatable {
    /// Camera center latitude in decimal degrees.
    public var latitudeDegrees: Double
    /// Camera center longitude in decimal degrees.
    public var longitudeDegrees: Double
    /// Zoom level.
    public var zoomLevel: Double
    /// Bearing in degrees clockwise from true north.
    public var bearingDegrees: Double
    /// Pitch in degrees; zero is top-down.
    public var pitchDegrees: Double

    /// Builds one camera pose.
    public init(
        latitudeDegrees: Double,
        longitudeDegrees: Double,
        zoomLevel: Double,
        bearingDegrees: Double,
        pitchDegrees: Double
    ) {
        self.latitudeDegrees = latitudeDegrees
        self.longitudeDegrees = longitudeDegrees
        self.zoomLevel = zoomLevel
        self.bearingDegrees = bearingDegrees
        self.pitchDegrees = pitchDegrees
    }
}

/// Which `MapsSdkRusticMapView` operation produced a `MapsSdkMapViewFailure`.
public enum MapsSdkMapViewFailurePhase: Sendable {
    /// Acquiring the surface or attaching it.
    case surface
    /// Reconfiguring an attached surface.
    case resize
    /// Drawing a frame or querying idle settlement.
    case draw
    /// Reading, building or transforming the camera.
    case camera
    /// Pushing the overlay list.
    case overlay
    /// Pushing or clearing the location layer.
    case locationIndicator
    /// Installing, clearing or reading the vector base source.
    case baseMap
    /// Unprojecting a tap.
    case tap
    /// An application delegate callback failed. The Swift delegate protocol
    /// cannot throw, so the shipped wrapper never produces this phase; it is
    /// declared so every native target carries the same phase set and a
    /// consumer that switches over the phases compiles against one contract.
    case listener
}

/// One refused `MapsSdkRusticMapView` operation carrying its verbatim SDK or platform
/// failure. The wrapper never swallows a failure: every one reaches
/// `MapsSdkMapViewDelegate.mapView(_:didFailWith:)` with the phase it occurred in
/// and the original error, so an app can present the SDK's own message.
public struct MapsSdkMapViewFailure: Error {
    /// The wrapper operation that failed.
    public let phase: MapsSdkMapViewFailurePhase
    /// The verbatim SDK or platform failure.
    public let cause: Error

    /// Builds one failure.
    public init(phase: MapsSdkMapViewFailurePhase, cause: Error) {
        self.phase = phase
        self.cause = cause
    }
}

/// One application-lifecycle event the map view reacts to.
enum MapViewLifecycleEvent {
    /// The application entered the background.
    case didEnterBackground
    /// The application will return to the foreground.
    case willEnterForeground
    /// The system reported memory pressure.
    case didReceiveMemoryWarning
    /// The host paused or resumed rendering explicitly.
    case renderingPaused(Bool)
}

/// The pump and surface action one lifecycle event asks for.
enum MapViewLifecycleAction: Equatable {
    /// Pause the frame pump and release the GPU surface; a later resume
    /// re-attaches it.
    case releaseSurfaceAndPausePump
    /// Release the GPU surface under memory pressure, then let the idle
    /// watchdog retry the attach (a background app waits for the foreground
    /// instead).
    case releaseSurfaceAndRetryAfterMemoryPressure
    /// Resume the frame pump, whose next tick attaches the released surface.
    case resumeAndAttach
}

/// The map view's application-lifecycle policy.
///
/// Backgrounding releases the surface and waits for the foreground; a memory
/// warning releases it too but lets the idle watchdog retry the attach, so a
/// foreground map restores itself. An explicit pause releases the surface, and
/// a host that pauses the view frees its resources rather than only stopping
/// the display link.
enum MapViewLifecyclePolicy {
    /// The action an event asks for.
    ///
    /// - Parameter event: the lifecycle event.
    /// - Returns: the pump and surface action to apply.
    static func action(for event: MapViewLifecycleEvent) -> MapViewLifecycleAction {
        switch event {
        case .didEnterBackground:
            return .releaseSurfaceAndPausePump
        case .didReceiveMemoryWarning:
            return .releaseSurfaceAndRetryAfterMemoryPressure
        case .willEnterForeground, .renderingPaused(false):
            return .resumeAndAttach
        case .renderingPaused(true):
            return .releaseSurfaceAndPausePump
        }
    }
}

/// The callbacks a `MapsSdkRusticMapView` delivers on the main thread.
///
/// Every member has an empty default in the protocol extension: an
/// integrator overrides only what it needs.
@MainActor
public protocol MapsSdkMapViewDelegate: AnyObject {
    /// The first frame drew and presented successfully.
    func mapViewDidBecomeReady(_ mapView: MapsSdkRusticMapView)

    /// The camera changed: a set, an animation step, a gesture or a fit.
    ///
    /// - Parameters:
    ///   - camera: the adopted pose.
    ///   - isGestureDriven: true when a pan, pinch or rotate produced the
    ///     change, which is the hook a follow camera uses to break on
    ///     gesture.
    func mapView(
        _ mapView: MapsSdkRusticMapView,
        didChangeCamera camera: MapsSdkCameraPose,
        isGestureDriven: Bool
    )

    /// A tap unprojected onto the ground plane; a miss reports nothing.
    func mapView(
        _ mapView: MapsSdkRusticMapView,
        didClickCoordinate coordinate: GeographicCoordinate
    )

    /// An SDK or platform operation failed; the failure is never swallowed.
    func mapView(_ mapView: MapsSdkRusticMapView, didFailWith failure: MapsSdkMapViewFailure)

    /// One presented frame's tile and overlay accounting.
    func mapView(_ mapView: MapsSdkRusticMapView, didDrawFrame statistics: FrameStatistics)

    /// A frame was skipped because a draw or resize held the render state
    /// (`RenderingSurfaceUnavailable` backpressure); the next tick retries.
    func mapViewDidDeferFrame(_ mapView: MapsSdkRusticMapView)

    /// The pump paused because `isFrameSettled` answered true.
    func mapViewDidSettleFrame(_ mapView: MapsSdkRusticMapView)
}

public extension MapsSdkMapViewDelegate {
    func mapViewDidBecomeReady(_ mapView: MapsSdkRusticMapView) {}
    func mapView(
        _ mapView: MapsSdkRusticMapView,
        didChangeCamera camera: MapsSdkCameraPose,
        isGestureDriven: Bool
    ) {}
    func mapView(
        _ mapView: MapsSdkRusticMapView,
        didClickCoordinate coordinate: GeographicCoordinate
    ) {}
    func mapView(_ mapView: MapsSdkRusticMapView, didFailWith failure: MapsSdkMapViewFailure) {}
    func mapView(_ mapView: MapsSdkRusticMapView, didDrawFrame statistics: FrameStatistics) {}
    func mapViewDidDeferFrame(_ mapView: MapsSdkRusticMapView) {}
    func mapViewDidSettleFrame(_ mapView: MapsSdkRusticMapView) {}
}

/// The Metal-backed, lifecycle-aware map view shipped in the package.
///
/// The layer class is `CAMetalLayer`, so the surface handle the renderer
/// lowers through `SurfaceHandle.iosViewPointer` always carries a Metal
/// layer; the view computes that pointer itself, so user code never does
/// pointer math. The view caps its drawable at the downlevel texture limit
/// while keeping its point size: the rendered pixels scale up to the view
/// without a layout change (an unguarded full-screen buffer on a tall
/// phone is rejected by the downlevel adapter).
///
/// All members are expected on the main thread.
@MainActor
public final class MapsSdkRusticMapView: UIView {
    // MARK: Constants

    /// The maximum drawable pixel extent per axis the wgpu surface
    /// configure accepts on these adapters: `wgpu::Limits::downlevel_defaults()`
    /// sets `max_texture_dimension_2d` to 2048, and a larger surface
    /// buffer is rejected. The view keeps its full point size; only the
    /// Metal layer's drawable is capped, so the drawn pixels scale up to
    /// the view without any layout change.
    private static let maximumDrawableExtentPixels: CGFloat = 2048

    /// The display-scale floor when the trait collection has not reported
    /// a scale yet (the first ticks before the window scales resolve).
    private static let minimumDisplayScale: CGFloat = 1.0

    /// The SDK renderer's default perspective field of view, read from the
    /// contract so the host cannot drift from the renderer.
    private nonisolated static var defaultCameraFieldOfViewDegrees: Double {
        defaultFieldOfViewDegrees()
    }

    /// One double tap's zoom step (the design language's §5.2): the gesture
    /// zooms exactly one level around the tapped point through the contract's
    /// focal zoom member, the same seam the pinch recognizer feeds.
    private static let doubleTapZoomStep: Double = 1.0

    /// The cheap idle insurance cadence while the pump is settled, backing
    /// off or retrying an attach: one second.
    private static let watchdogIntervalSeconds: TimeInterval = 1.0

    /// Consecutive unavailable-only, pose-unchanged frames before the pump
    /// drops to the watchdog's one-second retry cadence: a provider that
    /// keeps rejecting tiles must not become a 60 fps retry spin.
    private static let unavailableBackoffFrameCount: Int = 3

    /// The default camera animation duration (the design language's §5.3
    /// eased move duration).
    public nonisolated static let defaultCameraAnimationDurationMilliseconds: Double = 300

    /// The SDK's maximum zoom level, the default fit-bounds ceiling.
    public nonisolated static var defaultFitBoundsMaximumZoomLevel: Double {
        maximumZoomLevel()
    }

    /// Milliseconds in one second (the animation duration's unit
    /// conversion).
    private static let millisecondsPerSecond: Double = 1000

    /// The initial pose before any host camera intent.
    public nonisolated(unsafe) static let defaultCameraPose = MapsSdkCameraPose(
        latitudeDegrees: 0,
        longitudeDegrees: 0,
        zoomLevel: 2,
        bearingDegrees: 0,
        pitchDegrees: 0
    )

    // MARK: Public state

    /// The SDK whose registered raster-tile provider the view draws from.
    /// Setting it creates the native view; setting it while attached
    /// rebuilds the native view and re-attaches on the next pump tick.
    /// Nil draws nothing (the black canvas stands).
    public var sdk: MapSdk? {
        didSet {
            if let sdk, let oldValue, sdk === oldValue {
                return
            }
            if sdk == nil && oldValue == nil {
                return
            }
            rebuildNativeMapView()
        }
    }

    /// The callbacks, held weakly.
    ///
    /// Liveness policy (the same on Android): a delegate commonly captures
    /// the host that owns the view, so a strong reference here would close a
    /// retain cycle and keep both alive after the host released them. The
    /// weak reference breaks that cycle; the host keeps its delegate alive
    /// for as long as it wants callbacks, and a delegate no longer referenced
    /// elsewhere is deallocated and its callbacks stop. `teardown()` clears
    /// the reference regardless.
    public weak var delegate: MapsSdkMapViewDelegate?

    /// Whether touch gestures pan, pinch, rotate and tap the map.
    public var isGestureEnabled: Bool = true {
        didSet {
            for gestureRecognizer in gestureRecognizersList {
                gestureRecognizer.isEnabled = isGestureEnabled
            }
        }
    }

    /// The camera the pump draws with; a projection of the contract camera
    /// value the SDK holds, so the wrapper stores no pose of its own.
    public var camera: MapsSdkCameraPose {
        storedCamera.map(readCameraPose) ?? MapsSdkRusticMapView.defaultCameraPose
    }

    /// The render viewport in pixels, or nil before the surface attached.
    /// This is the capped extent the frames render at, not the raw view
    /// extent; gestures and taps scale between the two.
    public var viewportSize: ViewportSize? {
        guard attachedWidthPixels > 0, attachedHeightPixels > 0 else {
            return nil
        }
        return ViewportSize(
            widthPixels: Double(attachedWidthPixels),
            heightPixels: Double(attachedHeightPixels)
        )
    }

    /// The render viewport width in pixels (zero before attach).
    public var viewportWidthPixels: Int { attachedWidthPixels }

    /// The render viewport height in pixels (zero before attach).
    public var viewportHeightPixels: Int { attachedHeightPixels }

    /// The capped physical-pixel extent the drawable is configured to, or
    /// nil before the first laid-out pump tick.
    public var viewportPixelExtent: CGSize? {
        renderPixelExtent == .zero ? nil : renderPixelExtent
    }

    /// The effective drawable scale (points to rendered pixels), clamped
    /// by the downlevel texture limit.
    public var displayScale: CGFloat { renderScale }

    // MARK: Surface and native view state

    private var nativeMapView: MapView?
    /// The vector base source installed before the native view existed; the
    /// native view is rebuilt on every SDK swap, and the contract keeps the
    /// source across detach/attach, so the wrapper re-applies it like the
    /// overlay list. The record also carries the rollback target for a change
    /// queued behind a frame draw.
    /// The vector base source last asked for. The contract keeps the source
    /// across detach and attach, so this value is only re-applied after a
    /// native-view rebuild; a change made while a frame draws is applied by
    /// the SDK when that frame finishes, which is why no queue is needed.
    private var requestedVectorTileSource: UrlTemplateVectorTileSource?
    private var requiresAttach = false
    private var isAttached = false
    private var attachedWidthPixels = 0
    private var attachedHeightPixels = 0
    private var renderPixelExtent: CGSize = .zero
    private var renderScale: CGFloat = 1

    // MARK: Camera state

    /// The contract camera the view holds. It is the single source of the
    /// camera state: every readback, gesture and animation replaces it, and
    /// the native view receives the same value through `setCamera(_:)`. Nil
    /// until the first camera intent, which reads as the contract's neutral
    /// default pose.
    private var storedCamera: MapCamera?
    private var cameraRevision: UInt64 = 0
    private var cameraAnimation: CameraAnimation?

    // MARK: Overlay and location state

    private var overlayDescriptors: [MapOverlay] = []
    private var overlayDescriptorsByIdentifier: [String: MapOverlay] = [:]
    private var overlayIdentifierOrder: [String] = []
    private var locationIndicatorState: LocationIndicatorState?
    private var locationIndicatorStyle: LocationIndicatorStyle?

    // MARK: Pump state

    private var displayLink: CADisplayLink?
    private var watchdogTimer: Timer?
    private var isDrawInFlight = false
    /// The render generation. Every change that makes an in-flight draw
    /// result obsolete — attach, resize, detach, native-view rebuild, teardown
    /// or a newly adopted pose — invalidates it; a finished draw publishes only
    /// when its captured generation is still current, so a stale result never
    /// reports statistics, readiness or a settle against the newer state.
    private var drawGenerationGate = RenderGenerationGate()
    private var isBackoffActive = false
    private var consecutiveUnavailableOnlyFrames = 0
    private var hasReportedMapReady = false
    private var isRenderingPaused = false
    private var isTornDown = false
    private var gestureRecognizersList: [UIGestureRecognizer] = []

    /// The last lifecycle action the view applied. Internal so the simulator
    /// tests can post the application notifications and assert the mapping.
    private(set) var lastAppliedLifecycleAction: MapViewLifecycleAction?

    // MARK: Gesture state

    private var lastPanTranslationPixels: CGPoint = .zero

    // MARK: Construction

    public override class var layerClass: AnyClass { CAMetalLayer.self }

    public override init(frame: CGRect) {
        super.init(frame: frame)
        configureView()
    }

    /// Storyboard/XIB construction: the shipped view is layout-capable from
    /// a nib, unlike the app-side view it replaces.
    public required init?(coder: NSCoder) {
        super.init(coder: coder)
        configureView()
    }

    private func configureView() {
        // The canvas is opaque black under both themes (the renderer's
        // clear color is a Rust-side rendering constant, not a UI token).
        backgroundColor = UIColor.black
        isOpaque = true
        isMultipleTouchEnabled = true
        configureGestures()
        observeApplicationLifecycle()
    }

    /// The view's raw object address, laid as the unsigned pointer value
    /// the `SurfaceHandle.iosViewPointer` field carries. Computed inside
    /// the view so user code never does pointer math.
    private var iosViewPointerValue: UInt64 {
        let opaquePointer = Unmanaged.passUnretained(self).toOpaque()
        return UInt64(UInt(bitPattern: Int(bitPattern: opaquePointer)))
    }

    /// The view's backing Metal layer (its layer class is `CAMetalLayer`).
    private var metalLayer: CAMetalLayer? {
        layer as? CAMetalLayer
    }

    // MARK: Camera API

    /// Sets the camera immediately; a refused pose reports and keeps the
    /// old one.
    public func setCamera(_ cameraPose: MapsSdkCameraPose) {
        cancelCameraAnimation()
        guard let adoptedCamera = cameraFromPoseOrNil(cameraPose) else { return }
        adoptCamera(adoptedCamera, isGestureDriven: false)
    }

    /// Eases the camera to `cameraPose` over `durationMilliseconds` through
    /// the contract's interpolation curve. A non-positive duration is an
    /// immediate set. A gesture cancel drops the animation where it stands.
    public func animateCamera(
        _ cameraPose: MapsSdkCameraPose,
        durationMilliseconds: Double = MapsSdkRusticMapView.defaultCameraAnimationDurationMilliseconds
    ) {
        guard let targetCamera = cameraFromPoseOrNil(cameraPose) else { return }
        cancelCameraAnimation()
        guard durationMilliseconds > 0 else {
            adoptCamera(targetCamera, isGestureDriven: false)
            return
        }
        guard let startCamera = cameraFromPoseOrNil(camera) else { return }
        cameraAnimation = CameraAnimation(
            startTime: CACurrentMediaTime(),
            durationSeconds: durationMilliseconds / MapsSdkRusticMapView.millisecondsPerSecond,
            fromCamera: startCamera,
            toCamera: targetCamera
        )
        resumePump(resetBackoff: true)
    }

    /// Frames `boundingBox` inside the attached viewport's inset region
    /// through the contract's `fitBounds` member (top-down and north-up by
    /// construction) and adopts the fitted camera. Without an attached
    /// viewport the call changes nothing.
    public func fitBounds(
        boundingBox: BoundingBox,
        edgeInsets: EdgeInsets = MapsSdkRusticMapView.zeroEdgeInsets,
        maximumZoomLevel: Double = MapsSdkRusticMapView.defaultFitBoundsMaximumZoomLevel
    ) {
        guard let nativeMapView else { return }
        do {
            guard let fittedCamera = try nativeMapView.fitBounds(
                boundingBox: boundingBox,
                edgeInsets: edgeInsets,
                maximumZoomLevel: maximumZoomLevel
            ) else {
                return
            }
            adoptCamera(
                fittedCamera,
                isGestureDriven: false,
                syncToNativeView: false
            )
        } catch {
            reportFailure(phase: .camera, cause: error)
        }
    }

    /// Ground meters represented by one screen pixel at the camera center
    /// (the scale bar's helper), or nil before the surface attached.
    public func groundMetersPerScreenPixel() -> Double? {
        guard let viewport = viewportSize,
              let currentCamera = currentCameraOrNil()
        else {
            return nil
        }
        do {
            return try currentCamera.groundMetersPerScreenPixel(
                viewport: viewport,
                fieldOfViewDegrees: MapsSdkRusticMapView.defaultCameraFieldOfViewDegrees
            )
        } catch {
            reportFailure(phase: .camera, cause: error)
            return nil
        }
    }

    /// The geographic bounds of the attached viewport's current camera pose,
    /// or nil before the surface attached or when the projection cannot
    /// answer.
    ///
    /// The contract unprojects the four viewport corners through the current
    /// camera; a steep pitch can put the far corner below the horizon, in
    /// which case the camera's own ground scale bounds a box around its
    /// center. The traffic layer asks for these bounds once the camera
    /// settles.
    public func cameraGeographicBounds() -> BoundingBox? {
        nativeMapView?.cameraGeographicBounds()
    }

    /// Pans by a screen-pixel delta through the contract camera's
    /// `panByScreenPixels` (the programmatic twin of a drag; the
    /// content-follows-pointer convention).
    public func panByScreenPixels(
        deltaXPixels: Double,
        deltaYPixels: Double,
        isGestureDriven: Bool = true
    ) {
        transformCamera(
            panXPixels: deltaXPixels,
            panYPixels: deltaYPixels,
            isGestureDriven: isGestureDriven
        )
    }

    /// Zooms by `deltaZoomLevels` around the viewport's center — the
    /// control cluster's zoom buttons go through the same contract member
    /// as a pinch.
    public func zoomByLevels(deltaZoomLevels: Double) {
        transformCamera(
            zoomLevels: deltaZoomLevels,
            focalPoint: viewportCenterScreenPoint,
            isGestureDriven: false
        )
    }

    /// Rotates the content clockwise by `clockwiseDegrees` around the
    /// viewport center (the content-follows-fingers convention: a
    /// clockwise finger turn turns the content clockwise, which lowers the
    /// bearing).
    public func rotateByDegrees(clockwiseDegrees: Double) {
        rotateCamera(
            clockwiseDegrees: clockwiseDegrees,
            focalPoint: viewportCenterScreenPoint,
            isGestureDriven: true
        )
    }

    // MARK: Overlay API (addressed by caller identifier)

    /// Adds or replaces the overlay held under `identifier`; answers
    /// whether the SDK accepted the list. A refusal leaves the previous
    /// bookkeeping and list in force.
    @discardableResult
    public func addOverlay(identifier: String, overlay: MapOverlay) -> Bool {
        var nextByIdentifier = overlayDescriptorsByIdentifier
        var nextOrder = overlayIdentifierOrder
        if nextByIdentifier[identifier] == nil {
            nextOrder.append(identifier)
        }
        nextByIdentifier[identifier] = overlay
        return replaceOverlayBookkeeping(byIdentifier: nextByIdentifier, order: nextOrder)
    }

    /// Replaces an existing overlay under `identifier`; an unknown
    /// identifier answers false and changes nothing (use `addOverlay` for
    /// a new identifier).
    @discardableResult
    public func updateOverlay(identifier: String, overlay: MapOverlay) -> Bool {
        guard overlayDescriptorsByIdentifier[identifier] != nil else {
            return false
        }
        var nextByIdentifier = overlayDescriptorsByIdentifier
        nextByIdentifier[identifier] = overlay
        return replaceOverlayBookkeeping(
            byIdentifier: nextByIdentifier,
            order: overlayIdentifierOrder
        )
    }

    /// Removes the overlay under `identifier` (an unknown identifier is a
    /// no-op); answers whether the SDK accepted the list.
    @discardableResult
    public func removeOverlay(identifier: String) -> Bool {
        guard overlayDescriptorsByIdentifier[identifier] != nil else {
            return true
        }
        var nextByIdentifier = overlayDescriptorsByIdentifier
        nextByIdentifier.removeValue(forKey: identifier)
        let nextOrder = overlayIdentifierOrder.filter { $0 != identifier }
        return replaceOverlayBookkeeping(byIdentifier: nextByIdentifier, order: nextOrder)
    }

    /// Replaces the whole overlay list and resets the identifier
    /// bookkeeping: later `addOverlay`/`updateOverlay`/`removeOverlay`
    /// calls start from an empty identifier map. A refusal restores the
    /// previous list and bookkeeping.
    @discardableResult
    public func setOverlays(_ overlays: [MapOverlay]) -> Bool {
        let previousByIdentifier = overlayDescriptorsByIdentifier
        let previousOrder = overlayIdentifierOrder
        overlayDescriptorsByIdentifier.removeAll()
        overlayIdentifierOrder.removeAll()
        if pushOverlays(overlays) {
            return true
        }
        overlayDescriptorsByIdentifier = previousByIdentifier
        overlayIdentifierOrder = previousOrder
        return false
    }

    /// Clears every overlay and the identifier bookkeeping; answers
    /// whether the SDK accepted the list. A refusal restores both.
    @discardableResult
    public func clearOverlays() -> Bool {
        setOverlays([])
    }

    // MARK: Location layer API

    /// Sets the renderer-owned location layer; a refused push reports.
    public func setLocationIndicator(
        state: LocationIndicatorState,
        style: LocationIndicatorStyle
    ) {
        locationIndicatorState = state
        locationIndicatorStyle = style
        guard let nativeMapView else { return }
        do {
            try nativeMapView.setLocationIndicator(state: state, style: style)
            resumePump(resetBackoff: true)
        } catch {
            reportFailure(phase: .locationIndicator, cause: error)
        }
    }

    /// Clears the location layer; a refused clear reports.
    public func clearLocationIndicator() {
        locationIndicatorState = nil
        locationIndicatorStyle = nil
        guard let nativeMapView else { return }
        nativeMapView.clearLocationIndicator()
        resumePump(resetBackoff: true)
    }

    // MARK: Vector base map API

    /// Installs a vector base map: a provider-neutral vector tileset painted
    /// with one of the SDK's committed styles, drawn over the raster tiles and
    /// beneath every overlay. The source outlives attach/detach cycles, so an
    /// install before the surface exists is carried into the first frame.
    /// Answers whether the SDK accepted it; a refusal reports through
    /// `MapsSdkMapViewDelegate.mapView(_:didFailWith:)` and leaves the previously
    /// installed base in force.
    @discardableResult
    public func setVectorTileSource(
        _ source: UrlTemplateVectorTileSource
    ) -> Bool {
        // The stored source survives a native-view rebuild (an SDK swap).
        requestedVectorTileSource = source
        guard let nativeMapView else { return true }
        do {
            try nativeMapView.setVectorTileSource(source: source)
            resumePump(resetBackoff: true)
            return true
        } catch {
            reportFailure(phase: .baseMap, cause: error)
            return false
        }
    }

    /// Installs a provider-backed base painted with the style document the
    /// provider publishes. The provider must be registered for vector tiles
    /// first, and the document is fetched through that provider's
    /// authenticated client, so the application never handles the credential.
    /// Answers whether the SDK accepted it; a refusal reports through
    /// `MapsSdkMapViewDelegate.mapView(_:didFailWith:)` and leaves the
    /// installed base in force.
    @discardableResult
    public func setProviderVectorTileSourceFromDocument(
        _ source: ProviderVectorTileSource,
        styleDocumentURL: String
    ) async -> Bool {
        guard let nativeMapView else { return false }
        do {
            try await nativeMapView.setProviderVectorTileSourceFromDocument(
                source: source,
                styleDocumentUrl: styleDocumentURL
            )
            resumePump(resetBackoff: true)
            return true
        } catch {
            reportFailure(phase: .baseMap, cause: error)
            return false
        }
    }

    /// Removes the vector base map; answers whether the SDK accepted the
    /// clear.
    @discardableResult
    public func clearVectorTileSource() -> Bool {
        requestedVectorTileSource = nil
        guard let nativeMapView else { return true }
        do {
            try nativeMapView.clearVectorTileSource()
            resumePump(resetBackoff: true)
            return true
        } catch {
            reportFailure(phase: .baseMap, cause: error)
            return false
        }
    }

    /// The vector base source's status — the configured style, name and
    /// attribution plus the last frame's drawn and failed tile counts — or
    /// nil before the native view exists.
    public func vectorTileSourceStatus() -> VectorTileSourceStatus? {
        nativeMapView?.vectorTileSourceStatus()
    }

    // MARK: Lifecycle API

    /// Pauses or resumes drawing explicitly (a host may release the GPU
    /// surface in the background). Pausing releases the surface as well as
    /// stopping the pump; resuming re-attaches it on the next display tick.
    public func setRenderingPaused(_ paused: Bool) {
        guard isRenderingPaused != paused else { return }
        isRenderingPaused = paused
        applyLifecycleAction(
            MapViewLifecyclePolicy.action(for: .renderingPaused(paused))
        )
    }

    /// Whether this view has drawn and presented its first frame since the
    /// current native surface attached. The `mapViewDidBecomeReady` delegate
    /// callback fires at the same moment, so a host that subscribes after the
    /// callback can read this state instead of waiting for a second event.
    public var hasDrawnFirstFrame: Bool {
        hasReportedMapReady
    }

    /// Retries a failed attach; the error surfaced through
    /// `MapsSdkMapViewDelegate.mapView(_:didFailWith:)`.
    public func retryAttach() {
        guard !isTornDown, nativeMapView != nil else { return }
        requiresAttach = true
        resumePump(resetBackoff: true)
    }

    /// Re-resolves the SDK's raster-tile provider and drops every pooled
    /// tile, so a provider, credential or source change reaches the next
    /// frame without detaching the surface. The attached view holds its
    /// provider and keeps its tiles by coordinate, so without this a base-map
    /// switch leaves the previous imagery on screen until the camera moves
    /// onto unfetched tiles. The camera, overlays, location layer and
    /// listeners keep their state, unlike an attach/detach cycle; a failure
    /// surfaces through `MapsSdkMapViewDelegate.mapView(_:didFailWith:)`.
    public func reloadTileSource() {
        guard !isTornDown, let nativeMapView else { return }
        do {
            try nativeMapView.reloadTileSource()
            resumePump(resetBackoff: true)
        } catch {
            reportFailure(phase: .baseMap, cause: error)
        }
    }

    /// Releases the display link, the watchdog and the native surface.
    /// Called by the SwiftUI wrapper's `dismantleUIView`; the view is not
    /// reusable afterwards.
    public func teardown() {
        guard !isTornDown else { return }
        isTornDown = true
        stopDisplayLink()
        cancelWatchdog()
        stopObservingApplicationLifecycle()
        detachSurface()
        nativeMapView = nil
        cameraAnimation = nil
        storedCamera = nil
        delegate = nil
    }

    // MARK: Application lifecycle

    /// Observes the application notifications whose transitions release or
    /// restore the GPU surface.
    private func observeApplicationLifecycle() {
        let notificationCenter = NotificationCenter.default
        notificationCenter.addObserver(
            self,
            selector: #selector(applicationDidEnterBackground),
            name: UIApplication.didEnterBackgroundNotification,
            object: nil
        )
        notificationCenter.addObserver(
            self,
            selector: #selector(applicationWillEnterForeground),
            name: UIApplication.willEnterForegroundNotification,
            object: nil
        )
        notificationCenter.addObserver(
            self,
            selector: #selector(applicationDidReceiveMemoryWarning),
            name: UIApplication.didReceiveMemoryWarningNotification,
            object: nil
        )
    }

    private func stopObservingApplicationLifecycle() {
        NotificationCenter.default.removeObserver(self)
    }

    @objc private func applicationDidEnterBackground() {
        applyLifecycleAction(
            MapViewLifecyclePolicy.action(for: .didEnterBackground)
        )
    }

    @objc private func applicationWillEnterForeground() {
        applyLifecycleAction(
            MapViewLifecyclePolicy.action(for: .willEnterForeground)
        )
    }

    @objc private func applicationDidReceiveMemoryWarning() {
        applyLifecycleAction(
            MapViewLifecyclePolicy.action(for: .didReceiveMemoryWarning)
        )
    }

    /// Applies one lifecycle action. Releasing pauses the display link and
    /// detaches the platform surface; a teardown or a window detach already
    /// releases it, so both actions are safe to repeat.
    private func applyLifecycleAction(_ action: MapViewLifecycleAction) {
        lastAppliedLifecycleAction = action
        switch action {
        case .releaseSurfaceAndPausePump:
            displayLink?.isPaused = true
            cancelWatchdog()
            detachSurface()
        case .releaseSurfaceAndRetryAfterMemoryPressure:
            // The watchdog is the retry: it re-attaches on a later tick, or
            // waits for the foreground when the app is in the background.
            pausePump()
            detachSurface()
        case .resumeAndAttach:
            resumePump(resetBackoff: true)
        }
    }

    // MARK: View lifecycle overrides

    public override func didMoveToWindow() {
        super.didMoveToWindow()
        if window != nil {
            startDisplayLink()
            resumePump(resetBackoff: true)
        } else {
            stopDisplayLink()
            cancelWatchdog()
            detachSurface()
        }
    }

    /// The layout pass also resumes the pump: a rotated or split-view
    /// resize arrives here before the next display tick, and the tick's
    /// extent check reconfigures the surface.
    public override func layoutSubviews() {
        super.layoutSubviews()
        resumePump(resetBackoff: true)
    }

    // MARK: Frame pump

    private func canPump() -> Bool {
        !isTornDown
            && nativeMapView != nil
            && window != nil
            && !isRenderingPaused
            // A backgrounded application must not re-attach the surface it
            // released; the foreground notification resumes the pump.
            && UIApplication.shared.applicationState != .background
    }

    private func startDisplayLink() {
        guard displayLink == nil else { return }
        let newDisplayLink = CADisplayLink(
            target: self,
            selector: #selector(displayLinkFrameTick)
        )
        // Born paused: a keyless view (no SDK yet) must not tick at the
        // display cadence. `resumePump` unpauses it once the pump can run.
        newDisplayLink.isPaused = true
        newDisplayLink.add(to: .main, forMode: .common)
        displayLink = newDisplayLink
    }

    private func stopDisplayLink() {
        displayLink?.invalidate()
        displayLink = nil
    }

    private func resumePump(resetBackoff: Bool) {
        guard canPump() else { return }
        cancelWatchdog()
        if resetBackoff {
            isBackoffActive = false
            consecutiveUnavailableOnlyFrames = 0
        }
        startDisplayLink()
        displayLink?.isPaused = false
    }

    /// Pauses the tick but keeps the display link alive (the pump's cheap
    /// idle state). Any resume trigger restarts it, and the watchdog
    /// re-checks cheaply.
    private func pausePump() {
        displayLink?.isPaused = true
        scheduleWatchdog()
    }

    private func pauseForSettled() { pausePump() }

    private func pauseForBackoff() { pausePump() }

    private func pauseForRetry() { pausePump() }

    private func scheduleWatchdog() {
        cancelWatchdog()
        let timer = Timer(
            timeInterval: MapsSdkRusticMapView.watchdogIntervalSeconds,
            target: self,
            selector: #selector(watchdogFired),
            userInfo: nil,
            repeats: false
        )
        RunLoop.main.add(timer, forMode: .common)
        watchdogTimer = timer
    }

    private func cancelWatchdog() {
        watchdogTimer?.invalidate()
        watchdogTimer = nil
    }

    @objc private func watchdogFired() {
        resumePump(resetBackoff: false)
    }

    @objc private func displayLinkFrameTick() {
        pumpOneFrame()
    }

    private func pumpOneFrame() {
        guard canPump() else { return }
        guard updateRenderExtent() else { return }
        let pixelExtent = renderPixelExtent
        if requiresAttach {
            tryAttach(pixelExtent: pixelExtent)
            return
        }
        guard isAttached, let nativeMapView else { return }
        if attachedWidthPixels != Int(pixelExtent.width)
            || attachedHeightPixels != Int(pixelExtent.height) {
            tryResize(nativeMapView: nativeMapView, pixelExtent: pixelExtent)
            return
        }
        // A base-map change queued by a tap that landed mid-frame applies
        // here, before this tick's frame resolves against the scheduler.
        advanceCameraAnimation()
        guard let camera = currentCameraOrNil() else { return }
        let isSettledNow: Bool
        do {
            isSettledNow = try nativeMapView.isFrameSettled(camera: camera)
        } catch {
            reportFailure(phase: .draw, cause: error)
            pauseForRetry()
            return
        }
        if isSettledNow {
            delegate?.mapViewDidSettleFrame(self)
            pauseForSettled()
            return
        }
        guard !isDrawInFlight else { return }
        isDrawInFlight = true
        let drawnCameraRevision = cameraRevision
        let drawnDrawGeneration = drawGenerationGate.beginDraw()
        let frameCamera = camera
        Task { @MainActor [weak self] in
            guard let self else { return }
            defer { self.isDrawInFlight = false }
            do {
                let statistics = try await nativeMapView.draw(camera: frameCamera)
                guard self.drawGenerationGate.isCurrent(drawnDrawGeneration) else { return }
                self.handleDrawnFrame(
                    statistics,
                    drawnCameraRevision: drawnCameraRevision
                )
            } catch let sdkError as SdkError {
                guard self.drawGenerationGate.isCurrent(drawnDrawGeneration) else { return }
                if case .RenderingSurfaceUnavailable = sdkError {
                    // Documented backpressure of the serialized-draw rule:
                    // retry next tick.
                    self.consecutiveUnavailableOnlyFrames = 0
                    self.delegate?.mapViewDidDeferFrame(self)
                } else {
                    self.reportFailure(phase: .draw, cause: sdkError)
                    self.pauseForRetry()
                }
            } catch {
                guard self.drawGenerationGate.isCurrent(drawnDrawGeneration) else { return }
                self.reportFailure(phase: .draw, cause: error)
                self.pauseForRetry()
            }
        }
    }

    /// Invalidates any in-flight draw result: the next completion sees a newer
    /// generation and drops its result instead of publishing it.
    private func invalidateInFlightDraw() {
        drawGenerationGate.invalidateInFlightDraw()
    }

    private func handleDrawnFrame(
        _ statistics: FrameStatistics,
        drawnCameraRevision: UInt64
    ) {
        delegate?.mapView(self, didDrawFrame: statistics)
        if !hasReportedMapReady {
            hasReportedMapReady = true
            delegate?.mapViewDidBecomeReady(self)
        }
        let nothingDeferred = statistics.deferredTileCount == 0
        let somethingUnavailable = statistics.unavailableTileCount > 0
        // "Unchanged pose" is judged against the camera this frame actually
        // drew, not a gesture that landed while the draw awaited tiles.
        let poseUnchanged = drawnCameraRevision == cameraRevision
        if nothingDeferred && somethingUnavailable && poseUnchanged {
            if isBackoffActive {
                pauseForBackoff()
                return
            }
            consecutiveUnavailableOnlyFrames += 1
            if consecutiveUnavailableOnlyFrames >= MapsSdkRusticMapView.unavailableBackoffFrameCount {
                isBackoffActive = true
                pauseForBackoff()
            }
        } else {
            consecutiveUnavailableOnlyFrames = 0
            isBackoffActive = false
        }
    }

    // MARK: Attach / resize / detach

    /// Recomputes the capped drawable extent from the laid-out point size
    /// and the trait collection's scale; false while the view has no
    /// non-zero laid-out extent yet.
    private func updateRenderExtent() -> Bool {
        let laidOutPointExtent = bounds.size
        guard laidOutPointExtent.width > 0, laidOutPointExtent.height > 0 else {
            return false
        }
        let displayScale = max(
            traitCollection.displayScale,
            MapsSdkRusticMapView.minimumDisplayScale
        )
        // The drawable is capped at the adapter's maximum texture
        // dimension: the capped effective scale keeps the aspect and never
        // exceeds the configure validation.
        let cappedDisplayScale = min(
            displayScale,
            MapsSdkRusticMapView.maximumDrawableExtentPixels
                / max(laidOutPointExtent.width, laidOutPointExtent.height)
        )
        let pixelExtent = CGSize(
            width: (laidOutPointExtent.width * cappedDisplayScale).rounded(),
            height: (laidOutPointExtent.height * cappedDisplayScale).rounded()
        )
        guard pixelExtent.width >= 1, pixelExtent.height >= 1 else {
            return false
        }
        renderScale = cappedDisplayScale
        renderPixelExtent = pixelExtent
        metalLayer?.drawableSize = pixelExtent
        return true
    }

    private func tryAttach(pixelExtent: CGSize) {
        guard let nativeMapView else { return }
        let widthPixels = UInt32(pixelExtent.width)
        let heightPixels = UInt32(pixelExtent.height)
        guard widthPixels > 0, heightPixels > 0 else { return }
        let surfaceHandle = SurfaceHandle(
            appkitViewPointer: nil,
            iosViewPointer: iosViewPointerValue,
            androidNativeWindowPointer: nil
        )
        do {
            try nativeMapView.attach(
                surfaceHandle: surfaceHandle,
                widthPixels: widthPixels,
                heightPixels: heightPixels
            )
            attachedWidthPixels = Int(widthPixels)
            attachedHeightPixels = Int(heightPixels)
            requiresAttach = false
            isAttached = true
            invalidateInFlightDraw()
            applyStoredStateToNativeView()
            resumePump(resetBackoff: true)
        } catch {
            // A failed attach reports, then waits for the watchdog's next
            // retry (a registration may arrive meanwhile) instead of
            // spinning every display tick.
            requiresAttach = true
            reportFailure(phase: .surface, cause: error)
            pauseForRetry()
        }
    }

    private func tryResize(
        nativeMapView: MapView,
        pixelExtent: CGSize
    ) {
        let widthPixels = UInt32(pixelExtent.width)
        let heightPixels = UInt32(pixelExtent.height)
        guard widthPixels > 0, heightPixels > 0 else { return }
        do {
            try nativeMapView.resize(
                widthPixels: widthPixels,
                heightPixels: heightPixels
            )
            attachedWidthPixels = Int(widthPixels)
            attachedHeightPixels = Int(heightPixels)
            invalidateInFlightDraw()
            applyDevicePixelRatio(to: nativeMapView, failurePhase: .resize)
            resumePump(resetBackoff: true)
        } catch let sdkError as SdkError {
            if case .RenderingSurfaceUnavailable = sdkError {
                // A mid-draw resize is documented backpressure: the next
                // tick retries.
                return
            }
            reportFailure(phase: .resize, cause: sdkError)
            pauseForRetry()
        } catch {
            reportFailure(phase: .resize, cause: error)
            pauseForRetry()
        }
    }

    /// Detaches the platform surface. A view that still owns a native view
    /// (a window detach, not a teardown) re-attaches on the next pump tick
    /// when it returns to a window.
    private func detachSurface() {
        invalidateInFlightDraw()
        if isAttached {
            nativeMapView?.detach()
            isAttached = false
        }
        requiresAttach = nativeMapView != nil
        attachedWidthPixels = 0
        attachedHeightPixels = 0
    }

    private func rebuildNativeMapView() {
        invalidateInFlightDraw()
        if isAttached {
            nativeMapView?.detach()
            isAttached = false
        }
        attachedWidthPixels = 0
        attachedHeightPixels = 0
        requiresAttach = false
        hasReportedMapReady = false
        nativeMapView = nil
        guard let sdk, !isTornDown else { return }
        nativeMapView = MapView(mapSdk: sdk)
        requiresAttach = true
        applyStoredStateToNativeView()
        resumePump(resetBackoff: true)
    }

    /// Re-pushes the stored camera, overlays and location layer onto a fresh
    /// native view (the SDK state outlives the view's attach cycles, but a
    /// rebuilt native view starts empty).
    private func applyStoredStateToNativeView() {
        guard let nativeMapView else { return }
        applyDevicePixelRatio(to: nativeMapView, failurePhase: .surface)
        // The fresh native view starts at the contract default; hand it the
        // pose the wrapper holds so its readback and following frames match.
        if let storedCamera {
            nativeMapView.setCamera(camera: storedCamera)
        }
        do {
            try nativeMapView.setOverlays(overlayDescriptors: overlayDescriptors)
        } catch {
            reportFailure(phase: .overlay, cause: error)
        }
        if let state = locationIndicatorState,
           let style = locationIndicatorStyle {
            do {
                try nativeMapView.setLocationIndicator(state: state, style: style)
            } catch {
                reportFailure(phase: .locationIndicator, cause: error)
            }
        }
        if let vectorTileSource = requestedVectorTileSource {
            do {
                try nativeMapView.setVectorTileSource(source: vectorTileSource)
            } catch {
                reportFailure(phase: .baseMap, cause: error)
            }
        }
    }

    /// Reports the capped drawable scale to the SDK: the labels and overlay
    /// screen sizes are declared in points, so the surface scales them by the
    /// drawable's device pixels per point. Called whenever the effective scale
    /// is applied — at attach, after a resize and on a rebuilt native view.
    private func applyDevicePixelRatio(
        to nativeMapView: MapView,
        failurePhase: MapsSdkMapViewFailurePhase
    ) {
        do {
            try nativeMapView.setDevicePixelRatio(
                devicePixelRatio: Double(renderScale)
            )
        } catch {
            reportFailure(phase: failurePhase, cause: error)
        }
    }

    // MARK: Camera internals

    private var viewportCenterScreenPoint: ScreenPoint? {
        guard let viewport = viewportSize else { return nil }
        return ScreenPoint(
            x: viewport.widthPixels / 2,
            y: viewport.heightPixels / 2
        )
    }

    /// Builds the contract camera for `pose`. The constructor clamps zoom
    /// and pitch and wraps the bearing, so this is the only normalization
    /// the wrapper performs.
    private func cameraFromPoseOrNil(_ pose: MapsSdkCameraPose) -> MapCamera? {
        do {
            return try MapCamera(
                center: GeographicCoordinate(
                    latitude: pose.latitudeDegrees,
                    longitude: pose.longitudeDegrees
                ),
                zoomLevel: pose.zoomLevel,
                bearingDegrees: pose.bearingDegrees,
                pitchDegrees: pose.pitchDegrees
            )
        } catch {
            reportFailure(phase: .camera, cause: error)
            return nil
        }
    }

    private func readCameraPose(_ camera: MapCamera) -> MapsSdkCameraPose {
        let center = camera.center()
        return MapsSdkCameraPose(
            latitudeDegrees: center.latitude,
            longitudeDegrees: center.longitude,
            zoomLevel: camera.zoomLevel(),
            bearingDegrees: camera.bearingDegrees(),
            pitchDegrees: camera.pitchDegrees()
        )
    }

    /// Makes `camera` the wrapper's camera state and hands it to the native
    /// view, which keeps its own copy. The readback and the callback are
    /// projections of this one value.
    private func adoptCamera(
        _ camera: MapCamera,
        isGestureDriven: Bool,
        syncToNativeView: Bool = true
    ) {
        if syncToNativeView {
            nativeMapView?.setCamera(camera: camera)
        }
        storedCamera = camera
        cameraRevision &+= 1
        // A new pose makes any draw that started before it obsolete: its
        // statistics and settle answer the old camera.
        invalidateInFlightDraw()
        delegate?.mapView(
            self,
            didChangeCamera: readCameraPose(camera),
            isGestureDriven: isGestureDriven
        )
        resumePump(resetBackoff: true)
    }

    /// The camera state the pump draws with. Before the first camera intent
    /// it lazily adopts the contract's neutral default, which is the pose the
    /// native view itself starts from.
    private func currentCameraOrNil() -> MapCamera? {
        if let storedCamera {
            return storedCamera
        }
        guard let defaultCamera = cameraFromPoseOrNil(MapsSdkRusticMapView.defaultCameraPose) else {
            return nil
        }
        storedCamera = defaultCamera
        return defaultCamera
    }

    private func transformCamera(
        panXPixels: Double = 0,
        panYPixels: Double = 0,
        zoomLevels: Double = 0,
        rotateDegrees: Double = 0,
        focalPoint: ScreenPoint? = nil,
        isGestureDriven: Bool = true
    ) {
        guard let viewport = viewportSize else { return }
        let hasPan = panXPixels != 0 || panYPixels != 0
        let hasZoom = zoomLevels != 0 && focalPoint != nil
        let hasRotate = rotateDegrees != 0 && focalPoint != nil
        guard hasPan || hasZoom || hasRotate else { return }
        guard let startingCamera = currentCameraOrNil() else { return }
        do {
            var workingCamera = startingCamera
            if hasPan {
                workingCamera = try workingCamera.panByScreenPixels(
                    deltaXPixels: panXPixels,
                    deltaYPixels: panYPixels,
                    viewport: viewport,
                    fieldOfViewDegrees: MapsSdkRusticMapView.defaultCameraFieldOfViewDegrees
                )
            }
            if hasZoom, let focalPoint {
                workingCamera = try workingCamera.zoomByLevelsAroundScreenPoint(
                    deltaZoomLevels: zoomLevels,
                    focalPoint: focalPoint,
                    viewport: viewport,
                    fieldOfViewDegrees: MapsSdkRusticMapView.defaultCameraFieldOfViewDegrees
                )
            }
            if hasRotate, let focalPoint {
                workingCamera = try workingCamera.rotateByDegreesAroundScreenPoint(
                    deltaBearingDegrees: rotateDegrees,
                    focalPoint: focalPoint,
                    viewport: viewport,
                    fieldOfViewDegrees: MapsSdkRusticMapView.defaultCameraFieldOfViewDegrees
                )
            }
            adoptCamera(workingCamera, isGestureDriven: isGestureDriven)
        } catch {
            reportFailure(phase: .camera, cause: error)
        }
    }

    private func rotateCamera(
        clockwiseDegrees: Double,
        focalPoint: ScreenPoint?,
        isGestureDriven: Bool
    ) {
        // The content-follows-fingers convention's rotational face: the
        // drag delta arrives clockwise-positive, while the contract's
        // bearing delta adds clockwise to the camera bearing — so the sign
        // flips across the seam and the camera answers the wrapped bearing.
        transformCamera(
            rotateDegrees: -clockwiseDegrees,
            focalPoint: focalPoint,
            isGestureDriven: isGestureDriven
        )
    }

    /// Drops a running camera animation; the adopted pose stays where it is.
    /// Every gesture's begun phase calls this seam; it is internal so the
    /// simulator tests can prove a cancel stops the move.
    func cancelCameraAnimation() {
        cameraAnimation = nil
    }

    /// Advances a running animation to the pose the contract interpolates
    /// for `currentTime`.
    private func advanceCameraAnimation(
        at currentTime: CFTimeInterval = CACurrentMediaTime()
    ) {
        guard let animation = cameraAnimation else { return }
        let elapsedSeconds = currentTime - animation.startTime
        let fraction = min(max(elapsedSeconds / animation.durationSeconds, 0), 1)
        applyCameraAnimation(animation: animation, fraction: fraction)
    }

    /// Applies one animation frame at a deterministic progress; internal so
    /// the simulator tests can assert the delegated interpolation midpoint.
    func applyCameraAnimationFraction(_ fraction: Double) {
        guard let animation = cameraAnimation else { return }
        applyCameraAnimation(
            animation: animation,
            fraction: min(max(fraction, 0), 1)
        )
    }

    private func applyCameraAnimation(
        animation: CameraAnimation,
        fraction: Double
    ) {
        do {
            let nextCamera = try MapCamera.interpolate(
                startCamera: animation.fromCamera,
                targetCamera: animation.toCamera,
                fraction: fraction
            )
            if fraction >= 1 {
                cameraAnimation = nil
            }
            adoptCamera(nextCamera, isGestureDriven: false)
        } catch {
            reportFailure(phase: .camera, cause: error)
            cameraAnimation = nil
        }
    }

    // MARK: Overlay internals

    private var orderedOverlayDescriptors: [MapOverlay] {
        overlayIdentifierOrder.compactMap { overlayDescriptorsByIdentifier[$0] }
    }

    /// Installs one identifier bookkeeping snapshot, rolling it back when the
    /// SDK refuses the resulting list (a refused overlay is never cached as
    /// accepted).
    private func replaceOverlayBookkeeping(
        byIdentifier: [String: MapOverlay],
        order: [String]
    ) -> Bool {
        let previousByIdentifier = overlayDescriptorsByIdentifier
        let previousOrder = overlayIdentifierOrder
        overlayDescriptorsByIdentifier = byIdentifier
        overlayIdentifierOrder = order
        if pushOverlays(orderedOverlayDescriptors) {
            return true
        }
        overlayDescriptorsByIdentifier = previousByIdentifier
        overlayIdentifierOrder = previousOrder
        return false
    }

    private func pushOverlays(_ descriptors: [MapOverlay]) -> Bool {
        guard let nativeMapView else {
            overlayDescriptors = descriptors
            return true
        }
        do {
            try nativeMapView.setOverlays(overlayDescriptors: descriptors)
            overlayDescriptors = descriptors
            resumePump(resetBackoff: true)
            return true
        } catch {
            reportFailure(phase: .overlay, cause: error)
            return false
        }
    }

    // MARK: Gestures

    private func configureGestures() {
        let panRecognizer = UIPanGestureRecognizer(
            target: self,
            action: #selector(handlePanGesture)
        )
        panRecognizer.maximumNumberOfTouches = 1
        addGestureRecognizer(panRecognizer)

        let pinchRecognizer = UIPinchGestureRecognizer(
            target: self,
            action: #selector(handlePinchGesture)
        )
        addGestureRecognizer(pinchRecognizer)

        let rotationRecognizer = UIRotationGestureRecognizer(
            target: self,
            action: #selector(handleRotationGesture)
        )
        addGestureRecognizer(rotationRecognizer)

        let tapRecognizer = UITapGestureRecognizer(
            target: self,
            action: #selector(handleTapGesture)
        )
        addGestureRecognizer(tapRecognizer)

        // A double tap zooms; the single tap waits for it to fail so one
        // double tap never also reports a map click.
        let doubleTapRecognizer = UITapGestureRecognizer(
            target: self,
            action: #selector(handleDoubleTapGesture)
        )
        doubleTapRecognizer.numberOfTapsRequired = 2
        addGestureRecognizer(doubleTapRecognizer)
        tapRecognizer.require(toFail: doubleTapRecognizer)

        panRecognizer.delegate = self
        pinchRecognizer.delegate = self
        rotationRecognizer.delegate = self
        tapRecognizer.delegate = self
        doubleTapRecognizer.delegate = self
        gestureRecognizersList = [
            panRecognizer, pinchRecognizer, rotationRecognizer, tapRecognizer,
            doubleTapRecognizer,
        ]
    }

    /// The render pixel coordinate of one point in the view.
    private func renderPixelPoint(of pointInView: CGPoint) -> ScreenPoint {
        ScreenPoint(
            x: Double(pointInView.x * renderScale),
            y: Double(pointInView.y * renderScale)
        )
    }

    @objc private func handlePanGesture(_ recognizer: UIPanGestureRecognizer) {
        guard isGestureEnabled else { return }
        switch recognizer.state {
        case .began:
            // A gesture is a new camera intent: it takes over from a running
            // animation rather than fighting it frame by frame.
            cancelCameraAnimation()
            lastPanTranslationPixels = .zero
            resumePump(resetBackoff: true)
        case .changed:
            let translationPixels = CGPoint(
                x: recognizer.translation(in: self).x * renderScale,
                y: recognizer.translation(in: self).y * renderScale
            )
            let deltaXPixels = translationPixels.x - lastPanTranslationPixels.x
            let deltaYPixels = translationPixels.y - lastPanTranslationPixels.y
            lastPanTranslationPixels = translationPixels
            guard deltaXPixels != 0 || deltaYPixels != 0 else { return }
            panByScreenPixels(
                deltaXPixels: Double(deltaXPixels),
                deltaYPixels: Double(deltaYPixels),
                isGestureDriven: true
            )
        case .ended, .cancelled, .failed:
            lastPanTranslationPixels = .zero
        default:
            break
        }
    }

    @objc private func handlePinchGesture(_ recognizer: UIPinchGestureRecognizer) {
        guard isGestureEnabled else { return }
        switch recognizer.state {
        case .began:
            cancelCameraAnimation()
            resumePump(resetBackoff: true)
        case .changed:
            let scale = recognizer.scale
            recognizer.scale = 1
            guard scale > 0 else { return }
            let zoomLevels = log2(Double(scale))
            guard zoomLevels != 0 else { return }
            transformCamera(
                zoomLevels: zoomLevels,
                focalPoint: renderPixelPoint(of: recognizer.location(in: self)),
                isGestureDriven: true
            )
        default:
            break
        }
    }

    @objc private func handleRotationGesture(
        _ recognizer: UIRotationGestureRecognizer
    ) {
        guard isGestureEnabled else { return }
        switch recognizer.state {
        case .began:
            cancelCameraAnimation()
            resumePump(resetBackoff: true)
        case .changed:
            let rotationRadians = recognizer.rotation
            recognizer.rotation = 0
            let rotationDegrees = Double(rotationRadians) * degreesPerRadian
            guard rotationDegrees != 0 else { return }
            rotateCamera(
                clockwiseDegrees: rotationDegrees,
                focalPoint: renderPixelPoint(of: recognizer.location(in: self)),
                isGestureDriven: true
            )
        default:
            break
        }
    }

    @objc private func handleTapGesture(_ recognizer: UITapGestureRecognizer) {
        guard isGestureEnabled, recognizer.state == .ended else { return }
        guard let viewport = viewportSize,
              let camera = currentCameraOrNil()
        else {
            return
        }
        do {
            let coordinate = try camera.screenToGroundCoordinate(
                screenPoint: renderPixelPoint(of: recognizer.location(in: self)),
                viewport: viewport,
                fieldOfViewDegrees: MapsSdkRusticMapView.defaultCameraFieldOfViewDegrees
            )
            if let coordinate {
                delegate?.mapView(self, didClickCoordinate: coordinate)
            }
        } catch {
            reportFailure(phase: .tap, cause: error)
        }
    }

    @objc private func handleDoubleTapGesture(_ recognizer: UITapGestureRecognizer) {
        guard isGestureEnabled, recognizer.state == .ended else { return }
        transformCamera(
            zoomLevels: MapsSdkRusticMapView.doubleTapZoomStep,
            focalPoint: renderPixelPoint(of: recognizer.location(in: self)),
            isGestureDriven: true
        )
    }

    // MARK: Helpers

    private func reportFailure(phase: MapsSdkMapViewFailurePhase, cause: Error) {
        delegate?.mapView(
            self,
            didFailWith: MapsSdkMapViewFailure(phase: phase, cause: cause)
        )
    }

    /// One eased camera move in flight (the pump advances it every tick).
    /// The endpoints are contract camera values, so a frame is one call into
    /// the SDK's interpolation curve.
    private struct CameraAnimation {
        let startTime: CFTimeInterval
        let durationSeconds: Double
        let fromCamera: MapCamera
        let toCamera: MapCamera
    }

    /// The zero edge insets of `fitBounds`' natural default.
    public nonisolated static let zeroEdgeInsets = EdgeInsets(
        left: 0,
        top: 0,
        right: 0,
        bottom: 0
    )

    /// Radians-to-degrees conversion of the two-finger rotation angle.
    private var degreesPerRadian: Double { 180 / Double.pi }
}

extension MapsSdkRusticMapView: UIGestureRecognizerDelegate {
    /// The pinch and rotation recognizers run together, so a two-finger
    /// move can zoom and turn at once (the map's own gesture algebra
    /// composes both members). The tap pair never recognizes together: the
    /// single tap is required to wait for the double tap to fail (§5.2).
    public func gestureRecognizer(
        _ gestureRecognizer: UIGestureRecognizer,
        shouldRecognizeSimultaneouslyWith otherGestureRecognizer: UIGestureRecognizer
    ) -> Bool {
        let tapRecognizers = gestureRecognizersList.filter { recognizer in
            recognizer is UITapGestureRecognizer
        }
        let bothAreTaps = tapRecognizers.contains(gestureRecognizer)
            && tapRecognizers.contains(otherGestureRecognizer)
        return !bothAreTaps
    }
}

/// The generation gate an asynchronous draw result checks before publishing.
///
/// A draw task captures the generation it started under; any state change
/// that makes the result obsolete advances the generation, and the completion
/// compares its captured value before it reports statistics, readiness, a
/// deferred frame or a settle. A stale result is dropped silently; clearing the
/// in-flight flag is the caller's unconditional `defer`.
struct RenderGenerationGate: Equatable {
    private var currentGeneration: UInt64 = 0

    /// Captures the generation one draw starts under.
    mutating func beginDraw() -> UInt64 {
        currentGeneration
    }

    /// Advances the generation after a change that invalidates in-flight work.
    mutating func invalidateInFlightDraw() {
        currentGeneration &+= 1
    }

    /// Whether a result captured under `generation` is still current.
    func isCurrent(_ generation: UInt64) -> Bool {
        generation == currentGeneration
    }
}
