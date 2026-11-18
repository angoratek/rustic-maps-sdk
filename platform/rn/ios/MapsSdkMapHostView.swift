import UIKit

/// One native observer of the host view's listener events (the reference
/// shell's demo session mirrors the camera readback and breaks its follow
/// camera on a gesture; the JavaScript event channel is always registered).
/// The camera value is a Swift struct, so this protocol stays native rather
/// than Objective-C.
public protocol MapsSdkMapHostViewObserver: AnyObject {
    /// The adopted camera changed (a set, animation step, gesture or fit).
    func mapHostViewDidChangeCamera(
        _ hostView: MapsSdkMapHostView,
        camera: MapsSdkCameraPose,
        isGestureDriven: Bool
    )
    /// The first frame drew and presented successfully.
    func mapHostViewDidBecomeReady(_ hostView: MapsSdkMapHostView)
}

public extension MapsSdkMapHostViewObserver {
    func mapHostViewDidBecomeReady(_ hostView: MapsSdkMapHostView) {}
}

/// The React Native package's native map host on iOS.
///
/// A `UIView` embedding the shared `MapsSdkRusticMapView` (the same
/// Metal-backed widget the SwiftUI reference application ships), inset by the
/// safe-area insets JavaScript passes, drawing from the package's one
/// `MapsSdkNativeRuntime` SDK object. The widget owns the surface, the display
/// link, gestures, camera animation, overlays and the location layer; this
/// class owns only the bridge glue.
@objc(MapsSdkMapHostView)
public final class MapsSdkMapHostView: UIView, MapsSdkMapViewDelegate {

    /// Live hosts keyed by the JavaScript-side stable identifier.
    private static let liveHostViews = NSMapTable<NSString, MapsSdkMapHostView>(
        keyOptions: .strongMemory,
        valueOptions: .weakMemory
    )

    /// The host registered under the identifier, or nil.
    @objc public static func find(viewIdentifier: String) -> MapsSdkMapHostView? {
        liveHostViews.object(forKey: viewIdentifier as NSString)
    }

    /// Attaches the package SDK to every live host (the SDK may start after
    /// the React view was created).
    public static func attachSdkToLiveHostViews(_ sdk: MapSdk?) {
        let enumerator = liveHostViews.objectEnumerator()
        while let hostView = enumerator?.nextObject() as? MapsSdkMapHostView {
            hostView.attachPackageSdk(sdk)
        }
    }

    /// The most recently registered live host (the reference shell's view).
    @objc public static func currentHostView() -> MapsSdkMapHostView? {
        let enumerator = liveHostViews.objectEnumerator()
        var currentHostView: MapsSdkMapHostView?
        while let hostView = enumerator?.nextObject() as? MapsSdkMapHostView {
            currentHostView = hostView
        }
        return currentHostView
    }

    /// The shared Metal-backed map widget.
    public let rusticMapView: MapsSdkRusticMapView

    private var observers: [MapsSdkMapHostViewObserver] = []

    private var insetLeftPixels: CGFloat = 0
    private var insetTopPixels: CGFloat = 0
    private var insetRightPixels: CGFloat = 0
    private var insetBottomPixels: CGFloat = 0

    /// The render-pause state the JavaScript prop requested. The effective
    /// pause also covers a view outside a window, so an explicit pause
    /// survives the window attach that follows the prop application.
    private var isRenderingPaused = false
    /// Whether the view has ever been in a window. Removing such a view from
    /// its superview is React Native's unmount signal.
    private var hasBeenAttachedToWindow = false
    /// Guards `teardown()` against the window and manager teardown paths.
    private var isTornDown = false

    /// The JavaScript-side identifier events and commands are keyed on.
    @objc public var viewIdentifier: String? {
        didSet {
            if let oldValue {
                Self.liveHostViews.removeObject(forKey: oldValue as NSString)
            }
            if let updatedIdentifier = viewIdentifier {
                Self.liveHostViews.setObject(self, forKey: updatedIdentifier as NSString)
            }
        }
    }

    /// Whether touch gestures pan, pinch, rotate and tap the map.
    @objc public var isGestureEnabled: Bool {
        get { rusticMapView.isGestureEnabled }
        set { rusticMapView.isGestureEnabled = newValue }
    }

    /// Registers one native observer (the JavaScript channel is independent).
    public func addObserver(_ observer: MapsSdkMapHostViewObserver) {
        if !observers.contains(where: { existing in existing === observer }) {
            observers.append(observer)
        }
    }

    /// Removes one native observer.
    public func removeObserver(_ observer: MapsSdkMapHostViewObserver) {
        observers.removeAll { existing in existing === observer }
    }

    public override init(frame: CGRect) {
        rusticMapView = MapsSdkRusticMapView(frame: .zero)
        super.init(frame: frame)
        addSubview(rusticMapView)
        rusticMapView.delegate = self
        attachPackageSdk(MapsSdkNativeRuntime.shared.mapSdk)
    }

    @available(*, unavailable)
    public required init?(coder: NSCoder) {
        fatalError("MapsSdkMapHostView is constructed in code only")
    }

    /// Attaches the package-owned SDK the widget draws from.
    public func attachPackageSdk(_ sdk: MapSdk?) {
        if rusticMapView.sdk !== sdk {
            rusticMapView.sdk = sdk
        }
    }

    // MARK: Safe-area insets (physical pixels)

    @objc public func setTopBarInsetPixels(_ value: CGFloat) {
        insetTopPixels = value
        setNeedsLayout()
    }

    @objc public func setBottomBarInsetPixels(_ value: CGFloat) {
        insetBottomPixels = value
        setNeedsLayout()
    }

    @objc public func setLeftBarInsetPixels(_ value: CGFloat) {
        insetLeftPixels = value
        setNeedsLayout()
    }

    @objc public func setRightBarInsetPixels(_ value: CGFloat) {
        insetRightPixels = value
        setNeedsLayout()
    }

    public override func layoutSubviews() {
        super.layoutSubviews()
        let displayScale = max(traitCollection.displayScale, 1)
        rusticMapView.frame = CGRect(
            x: insetLeftPixels / displayScale,
            y: insetTopPixels / displayScale,
            width: max(bounds.width - (insetLeftPixels + insetRightPixels) / displayScale, 0),
            height: max(bounds.height - (insetTopPixels + insetBottomPixels) / displayScale, 0)
        )
    }

    // MARK: Imperative calls (the package module addresses the view)

    public func setCamera(_ camera: MapsSdkCameraPose) {
        rusticMapView.setCamera(camera)
    }

    public func animateCamera(
        _ camera: MapsSdkCameraPose,
        durationMilliseconds: Double
    ) {
        rusticMapView.animateCamera(
            camera,
            durationMilliseconds: durationMilliseconds
        )
    }

    public func fitBounds(
        _ boundingBox: BoundingBox,
        edgeInsets: EdgeInsets,
        maximumZoomLevel: Double
    ) {
        rusticMapView.fitBounds(
            boundingBox: boundingBox,
            edgeInsets: edgeInsets,
            maximumZoomLevel: maximumZoomLevel
        )
    }

    @objc public func panByScreenPixels(
        deltaXPixels: Double,
        deltaYPixels: Double,
        isGestureDriven: Bool
    ) {
        rusticMapView.panByScreenPixels(
            deltaXPixels: deltaXPixels,
            deltaYPixels: deltaYPixels,
            isGestureDriven: isGestureDriven
        )
    }

    @objc public func zoomByLevels(deltaZoomLevels: Double) {
        rusticMapView.zoomByLevels(deltaZoomLevels: deltaZoomLevels)
    }

    @discardableResult
    public func addOverlay(
        overlayIdentifier: String,
        overlay: MapOverlay
    ) -> Bool {
        rusticMapView.addOverlay(identifier: overlayIdentifier, overlay: overlay)
    }

    @discardableResult
    public func updateOverlay(
        overlayIdentifier: String,
        overlay: MapOverlay
    ) -> Bool {
        rusticMapView.updateOverlay(identifier: overlayIdentifier, overlay: overlay)
    }

    @discardableResult
    @objc public func removeOverlay(overlayIdentifier: String) -> Bool {
        rusticMapView.removeOverlay(identifier: overlayIdentifier)
    }

    @discardableResult
    public func setOverlays(_ overlays: [MapOverlay]) -> Bool {
        rusticMapView.setOverlays(overlays)
    }

    @discardableResult
    @objc public func clearOverlays() -> Bool {
        rusticMapView.clearOverlays()
    }

    public func setLocationIndicator(
        state: LocationIndicatorState,
        style: LocationIndicatorStyle
    ) {
        rusticMapView.setLocationIndicator(state: state, style: style)
    }

    @objc public func clearLocationIndicator() {
        rusticMapView.clearLocationIndicator()
    }

    // MARK: Vector base map

    /// Installs one styled vector base; answers whether the SDK accepted it.
    @discardableResult
    @objc public func setVectorTileSourceJSON(
        _ sourceJson: String
    ) -> Bool {
        guard
            let sourceDictionary = MapsSdkNativeJson.dictionary(from: sourceJson),
            let source = MapsSdkNativeJson.vectorTileSource(from: sourceDictionary)
        else {
            return false
        }
        return rusticMapView.setVectorTileSource(source)
    }

    /// Installs a provider-backed base painted with the provider's published
    /// style document; answers through `completion` once the SDK settled.
    @objc public func setProviderVectorTileSourceFromDocumentJSON(
        _ sourceJson: String,
        styleDocumentURL: String,
        completion: @escaping (Bool) -> Void
    ) {
        guard
            let sourceDictionary = MapsSdkNativeJson.dictionary(from: sourceJson),
            let source = MapsSdkNativeJson.providerVectorTileSource(from: sourceDictionary)
        else {
            completion(false)
            return
        }
        Task { @MainActor in
            let accepted = await rusticMapView
                .setProviderVectorTileSourceFromDocument(
                    source,
                    styleDocumentURL: styleDocumentURL
                )
            completion(accepted)
        }
    }

    /// Removes the vector base; answers whether the SDK accepted the clear.
    @discardableResult
    @objc public func clearVectorTileSource() -> Bool {
        rusticMapView.clearVectorTileSource()
    }

    /// The vector base status as its JSON readback, or `{}` when the native
    /// view does not exist yet.
    @objc public func vectorTileSourceStatusJSON() -> String {
        guard let status = rusticMapView.vectorTileSourceStatus() else {
            return "{}"
        }
        var statusObject: [String: Any] = [
            "isConfigured": status.isConfigured,
            "styleId": status.styleId,
            "sourceName": status.sourceName,
            "attributionText": status.attributionText,
            "maximumZoomLevel": Int(status.maximumZoomLevel),
            "drawnTileCount": Int(status.drawnTileCount),
            "failedTileCount": Int(status.failedTileCount),
            "unsupportedPropertyCount": Int(status.unsupportedPropertyCount),
            "unsupportedPropertyNames": status.unsupportedPropertyNames,
            "degradedAssetNames": status.degradedAssetNames,
            "sourceLayerCounts": status.sourceLayerCounts.map { sourceLayerCount -> [String: Any] in
                [
                    "sourceId": sourceLayerCount.sourceId,
                    "matchedLayerCount": Int(sourceLayerCount.matchedLayerCount),
                    "drawnLayerCount": Int(sourceLayerCount.drawnLayerCount),
                ]
            },
        ]
        if let lastFailureDetail = status.lastFailureDetail {
            statusObject["lastFailureDetail"] = lastFailureDetail
        }
        guard
            let data = try? JSONSerialization.data(withJSONObject: statusObject),
            let encoded = String(data: data, encoding: .utf8)
        else {
            return "{}"
        }
        return encoded
    }

    @objc public func retryAttach() {
        rusticMapView.retryAttach()
    }

    /// Re-resolves the raster-tile provider and drops the pooled tiles.
    @objc public func reloadTileSource() {
        rusticMapView.reloadTileSource()
    }

    /// Applies the JavaScript render-pause prop. The request is remembered so
    /// a later window attach cannot resume a view the caller paused.
    @objc public func setRenderingPaused(_ paused: Bool) {
        isRenderingPaused = paused
        applyRenderingPause()
    }

    /// The widget's first-frame state. It answers the same readiness the
    /// `mapReady` event reports, so a JavaScript subscriber that attaches
    /// after the event can still learn the map is ready.
    @objc public var hasDrawnFirstFrame: Bool {
        rusticMapView.hasDrawnFirstFrame
    }

    /// Forwards the effective pause: an explicitly paused view stays paused
    /// when it attaches, and a view outside a window never pumps.
    private func applyRenderingPause() {
        rusticMapView.setRenderingPaused(isRenderingPaused || window == nil)
    }

    /// The last adopted camera pose.
    public func camera() -> MapsSdkCameraPose {
        rusticMapView.camera
    }

    // MARK: JSON command seam (the Objective-C++ module's entry points)

    @objc public func setCameraJSON(_ cameraJson: String) {
        if let dictionary = MapsSdkNativeJson.dictionary(from: cameraJson),
           let camera = MapsSdkNativeJson.camera(from: dictionary) {
            setCamera(camera)
        }
    }

    @objc public func animateCameraJSON(
        _ cameraJson: String,
        durationMilliseconds: Double
    ) {
        if let dictionary = MapsSdkNativeJson.dictionary(from: cameraJson),
           let camera = MapsSdkNativeJson.camera(from: dictionary) {
            animateCamera(camera, durationMilliseconds: durationMilliseconds)
        }
    }

    @objc public func fitBoundsJSON(
        _ boundingBoxJson: String,
        edgeInsetsJson: String,
        maximumZoomLevel: Double
    ) {
        guard
            let boundingBoxDictionary = MapsSdkNativeJson.dictionary(from: boundingBoxJson),
            let boundingBox = MapsSdkNativeJson.boundingBox(from: boundingBoxDictionary),
            let edgeInsetsDictionary = MapsSdkNativeJson.dictionary(from: edgeInsetsJson)
        else {
            return
        }
        fitBounds(
            boundingBox,
            edgeInsets: MapsSdkNativeJson.edgeInsets(from: edgeInsetsDictionary),
            maximumZoomLevel: maximumZoomLevel
        )
    }

    @objc public func addOverlayJSON(
        _ overlayIdentifier: String,
        overlayJson: String
    ) -> Bool {
        guard
            let dictionary = MapsSdkNativeJson.dictionary(from: overlayJson),
            let overlay = MapsSdkNativeJson.overlay(from: dictionary)
        else {
            return false
        }
        return addOverlay(overlayIdentifier: overlayIdentifier, overlay: overlay)
    }

    @objc public func updateOverlayJSON(
        _ overlayIdentifier: String,
        overlayJson: String
    ) -> Bool {
        guard
            let dictionary = MapsSdkNativeJson.dictionary(from: overlayJson),
            let overlay = MapsSdkNativeJson.overlay(from: dictionary)
        else {
            return false
        }
        return updateOverlay(overlayIdentifier: overlayIdentifier, overlay: overlay)
    }

    @objc public func setOverlaysJSON(_ overlaysJson: String) -> Bool {
        guard let overlayDictionaries = MapsSdkNativeJson.dictionaryArray(from: overlaysJson) else {
            return false
        }
        let overlays = overlayDictionaries.compactMap { dictionary in
            MapsSdkNativeJson.overlay(from: dictionary)
        }
        guard overlays.count == overlayDictionaries.count else {
            return false
        }
        return setOverlays(overlays)
    }

    @objc public func setLocationIndicatorJSON(
        _ stateJson: String,
        styleJson: String
    ) {
        guard
            let stateDictionary = MapsSdkNativeJson.dictionary(from: stateJson),
            let styleDictionary = MapsSdkNativeJson.dictionary(from: styleJson),
            let state = MapsSdkNativeJson.locationIndicatorState(from: stateDictionary),
            let style = MapsSdkNativeJson.locationIndicatorStyle(from: styleDictionary)
        else {
            return
        }
        setLocationIndicator(state: state, style: style)
    }

    @objc public func getCameraJSON() -> String {
        var cameraObject: [String: Any] = [:]
        let camera = camera()
        cameraObject["latitudeDegrees"] = camera.latitudeDegrees
        cameraObject["longitudeDegrees"] = camera.longitudeDegrees
        cameraObject["zoomLevel"] = camera.zoomLevel
        cameraObject["bearingDegrees"] = camera.bearingDegrees
        cameraObject["pitchDegrees"] = camera.pitchDegrees
        guard
            let data = try? JSONSerialization.data(withJSONObject: cameraObject),
            let encoded = String(data: data, encoding: .utf8)
        else {
            return "{}"
        }
        return encoded
    }

    /// Ground meters represented by one screen pixel, or nil before attach.
    /// Optional `Double` is not Objective-C representable, so the bridge
    /// reads the boxed form.
    @objc public func groundMetersPerScreenPixelNumber() -> NSNumber? {
        rusticMapView.groundMetersPerScreenPixel().map { NSNumber(value: $0) }
    }

    /// The render viewport width in pixels (zero before attach).
    @objc public var viewportWidthPixels: Int {
        Int(rusticMapView.viewportSize?.widthPixels ?? 0)
    }

    /// The render viewport height in pixels (zero before attach).
    @objc public var viewportHeightPixels: Int {
        Int(rusticMapView.viewportSize?.heightPixels ?? 0)
    }

    /// Releases the display link, the surface and the native objects. The
    /// window lifecycle calls it when React Native unmounts the view, and the
    /// view manager may call it through its dismantle hook.
    @objc public func teardown() {
        guard !isTornDown else { return }
        isTornDown = true
        viewIdentifier = nil
        observers.removeAll()
        rusticMapView.delegate = nil
        rusticMapView.teardown()
        removeFromSuperview()
    }

    public override func didMoveToWindow() {
        super.didMoveToWindow()
        if window != nil {
            hasBeenAttachedToWindow = true
        }
        applyRenderingPause()
        // React Native unmounts a managed view by removing it from its
        // superview. A view that is only detached from a window keeps its
        // state and resumes when it attaches again.
        if window == nil && superview == nil && hasBeenAttachedToWindow {
            teardown()
        }
    }

    // MARK: MapsSdkMapViewDelegate

    public func mapViewDidBecomeReady(_ mapView: MapsSdkRusticMapView) {
        observers.forEach { observer in
            observer.mapHostViewDidBecomeReady(self)
        }
        emitEvent("mapReady", payload: [:])
    }

    public func mapView(
        _ mapView: MapsSdkRusticMapView,
        didChangeCamera camera: MapsSdkCameraPose,
        isGestureDriven: Bool
    ) {
        observers.forEach { observer in
            observer.mapHostViewDidChangeCamera(
                self,
                camera: camera,
                isGestureDriven: isGestureDriven
            )
        }
        emitEvent(
            "cameraChanged",
            payload: [
                "camera": cameraDictionary(camera),
                "isGestureDriven": isGestureDriven,
            ]
        )
    }

    public func mapView(
        _ mapView: MapsSdkRusticMapView,
        didClickCoordinate coordinate: GeographicCoordinate
    ) {
        emitEvent(
            "mapClick",
            payload: [
                "coordinate": [
                    "latitude": coordinate.latitude,
                    "longitude": coordinate.longitude,
                ],
            ]
        )
    }

    public func mapView(
        _ mapView: MapsSdkRusticMapView,
        didFailWith failure: MapsSdkMapViewFailure
    ) {
        emitEvent(
            "error",
            payload: [
                "failurePhase": failurePhaseLabel(failure.phase),
                "failureMessage": (failure.cause as NSError).localizedDescription,
            ]
        )
    }

    public func mapView(
        _ mapView: MapsSdkRusticMapView,
        didDrawFrame statistics: FrameStatistics
    ) {
        emitEvent(
            "frameStatistics",
            payload: [
                "statistics": [
                    "requestedTileCount": Int(statistics.requestedTileCount),
                    "presentTileCount": Int(statistics.presentTileCount),
                    "fallbackTileCount": Int(statistics.fallbackTileCount),
                    "deferredTileCount": Int(statistics.deferredTileCount),
                    "unavailableTileCount": Int(statistics.unavailableTileCount),
                    "overlayTriangleCount": Int(statistics.overlayTriangleCount),
                ],
            ]
        )
    }

    public func mapViewDidDeferFrame(_ mapView: MapsSdkRusticMapView) {
        emitEvent("frameDeferred", payload: [:])
    }

    public func mapViewDidSettleFrame(_ mapView: MapsSdkRusticMapView) {
        emitEvent("frameSettled", payload: [:])
    }

    // MARK: Helpers

    private func emitEvent(_ eventName: String, payload: [String: Any]) {
        guard let viewIdentifier else { return }
        var eventPayload = payload
        eventPayload["viewIdentifier"] = viewIdentifier
        eventPayload["event"] = eventName
        MapsSdkNativeEventChannel.emit(eventPayload)
    }

    private func cameraDictionary(_ camera: MapsSdkCameraPose) -> [String: Any] {
        [
            "latitudeDegrees": camera.latitudeDegrees,
            "longitudeDegrees": camera.longitudeDegrees,
            "zoomLevel": camera.zoomLevel,
            "bearingDegrees": camera.bearingDegrees,
            "pitchDegrees": camera.pitchDegrees,
        ]
    }

    private func failurePhaseLabel(_ phase: MapsSdkMapViewFailurePhase) -> String {
        switch phase {
        case .surface: return "surface"
        case .resize: return "resize"
        case .draw: return "draw"
        case .camera: return "camera"
        case .overlay: return "overlay"
        case .locationIndicator: return "locationIndicator"
        case .baseMap: return "baseMap"
        case .tap: return "tap"
        }
    }
}
