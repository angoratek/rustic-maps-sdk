//! The React Native reference application's one SDK seam on iOS, mirroring the
//! Compose reference application's architecture and the Android shell's
//! `RefappDemoSession.kt`: the surface lifecycle (`attach`/`resize`/`detach`
//! on the host's layout ticks, the Metal layer via the surface-child view
//! pointer), the explicit per-frame draw pump (one serialized
//! `MapView.draw` per composed frame with the
//! `RenderingSurfaceUnavailable` backpressure retry), the SDK-owned camera
//! math (unprojection at the viewport center), the readiness ladder, the
//! verbatim error report, and the plan picker's recorded walk trace replay
//! through `Navigator`.
//!
//! Everything the React side sees travels through `eventSink` as one event
//! channel; every SDK call failure reaches the React side verbatim (§6.3: no
//! catch block ends without a state change or an error presentation). All
//! controller work runs on the main thread, like every other seam; the
//! suspend SDK calls (geocode, route calculation) hand their reactor-
//! dependent work to the crate-owned runtime behind the exported async
//! methods.

import Foundation
import QuartzCore
import Security
import MapsSdkRn
import os

/// The demo session (the Kotlin `RefappDemoSession` singleton mirror): the
/// reference shell's camera intent, trace replay, speech, secure credential
/// store and search/plan glue. The map lifecycle and the SDK are the package's.
/// The `@objc` annotations name the exact selectors the ObjC++ bridge glue
/// (the React module and view manager) calls.
@objc(RefappDemoSession)
final class RefappDemoSession: NSObject, MapsSdkMapHostViewObserver, @unchecked Sendable {

    /// The singleton seam the view manager and the React module share.
    @objc static let shared = RefappDemoSession()

    private static let sessionTag = "RefappDemoSession"
    private let log = Logger(
        subsystem: "com.rusticmaps.sdk.refapprn",
        category: "RefappDemoSession"
    )

    /// The bundled recorded trace the shell replays when no plan trace loads
    /// (the artifact's own trace name is the default; the built-in demo trace
    /// is the fallback).
    private static let builtInTraceAssetName = "sacramento_clean_walk.json"

    // ------------------------------------------------------------------
    // Eventing to the React side
    // ------------------------------------------------------------------

    /// The React module's event sink (set once the module is built).
    @objc var eventSink: ((String, [String: Any]) -> Void)?

    private func emitEvent(_ eventName: String, _ arguments: [String: Any]) {
        eventSink?(eventName, arguments)
    }

    private func emitReadiness(_ readyState: ReadinessState, _ detailText: String?) {
        // The chip's display text is composed from the shared templates (§6.1);
        // the JS side renders it verbatim and needs no SDK vocabulary.
        let displayText: String
        switch readyState {
        case .loading:
            displayText = RefappConstants.loadingStatusText + RefappConstants.readinessLoadingSuffix
        case .ready:
            displayText = String(
                format: RefappConstants.readyStatusTemplate,
                compiledVersion
            )
        case .degraded:
            displayText = String(
                format: RefappConstants.degradedStatusTemplate,
                detailText ?? "unknown failure"
            )
        case .failed:
            displayText = String(
                format: RefappConstants.failedStatusTemplate,
                detailText ?? "unknown failure"
            )
        }
        var arguments: [String: Any] = [
            "state": "\(readyState)".lowercased(),
            "text": displayText,
        ]
        if let detailText = detailText {
            arguments["detail"] = detailText
        }
        emitEvent("RefappReadiness", arguments)
    }

    private func emitErrorReport(variantLabel: String, detailText: String) {
        emitEvent(
            "RefappErrorReport",
            ["variantLabel": variantLabel, "detailText": detailText]
        )
    }

    private func emitFrameStatistics(_ statistics: FrameStatistics?) {
        var arguments: [String: Any] = [
            "live": statistics != nil,
            "framesDrawnCount": framesDrawnCount,
            "framesDeferredCount": framesDeferredCount,
            "framesFailedCount": framesFailedCount,
        ]
        if let statistics = statistics {
            arguments["requestedTileCount"] = Int(statistics.requestedTileCount)
            arguments["presentTileCount"] = Int(statistics.presentTileCount)
            arguments["fallbackTileCount"] = Int(statistics.fallbackTileCount)
            arguments["deferredTileCount"] = Int(statistics.deferredTileCount)
            arguments["unavailableTileCount"] = Int(statistics.unavailableTileCount)
            arguments["overlayTriangleCount"] = Int(statistics.overlayTriangleCount)
        }
        emitEvent("RefappFrameStatistics", arguments)
    }

    private func emitCameraReadback() {
        guard let camera = packageHostView()?.camera() else { return }
        var arguments: [String: Any] = [
            "text": String(
                format: "Center %.5f , %.5f · zoom %.2f · bearing %.0f° · pitch %.0f°",
                camera.latitudeDegrees,
                camera.longitudeDegrees,
                camera.zoomLevel,
                camera.bearingDegrees,
                camera.pitchDegrees
            ),
            // The control cluster's read-only state (§4.2): the plane label,
            // the compass visibility and the scale bar all read these values.
            "zoomLevel": camera.zoomLevel,
            "bearingDegrees": camera.bearingDegrees,
            "pitchDegrees": camera.pitchDegrees,
            "isThreeDCameraEnabled": isThreeDCameraEnabled,
            "hasLocationFix": latestLocationFix?.snappedCoordinate != nil,
            "isFollowing": isTraceReplayActive && !followBrokenByGesture,
        ]
        if let groundMetersPerScreenPixel = packageHostView()?
            .groundMetersPerScreenPixel() {
            arguments["groundMetersPerScreenPixel"] = groundMetersPerScreenPixel
        }
        emitEvent("RefappCameraReadback", arguments)
    }

    /// The plan flow's state event (§4.1): the route summary card's four
    /// presentation values, emitted after every plan-state transition (in
    /// flight, planned, refused, cleared).
    func emitPlanState() {
        let plannedRoute = plannedRouteTrace
        var arguments: [String: Any] = [
            "isPlanned": plannedRoute != nil,
            "isInFlight": isPlanInFlight,
            "titleText": plannedRouteNameText ?? RefappConstants.plannedRouteFallbackName,
            "isRecordedDemoRoute": plannedRouteIsRecordedDemoRoute,
            "isOfflineArchiveRoute": plannedRouteIsOfflineArchiveRoute,
        ]
        if let plannedRoute = plannedRoute {
            arguments["distanceText"] = formatDistanceMeters(plannedRoute.route.distanceMeters)
            arguments["durationText"] = formatDurationSeconds(plannedRoute.route.durationSeconds)
        }
        emitEvent("RefappPlanState", arguments)
    }

    /// The offline card's cache line (§4.1's two honest lines, first line).
    func emitCacheState() {
        emitEvent(
            "RefappCacheState",
            [
                "isRegistered": isTileCacheRegistered,
                "stateText": tileCacheStateText,
            ]
        )
    }

    // ------------------------------------------------------------------
    // Spoken guidance (§4.1/§6.3; the speech contract)
    // ------------------------------------------------------------------

    /// The app-owned speech engine (the platform voice; the Compose reference
    /// application's `SpeechGuidanceSpeaker` semantics), created once at the
    /// module's initialize. The React side's speech consumer owns the dedup,
    /// pacing and toggle decisions and drives this engine through the bridge
    /// module's speak/stop methods.
    private(set) var speechSpeaker: RefappSpeechSpeaker?

    /// Builds the speech engine once (the React module's initialize hop).
    @objc func initializeSpeechEngine() {
        if speechSpeaker != nil {
            return
        }
        speechSpeaker = RefappSpeechSpeaker { [weak self] isEngineReady, unavailableReasonText in
            self?.emitSpeechEngineState(
                isEngineReady: isEngineReady,
                unavailableReasonText: unavailableReasonText
            )
        }
    }

    /// The speech engine's honest ready-unavailable state (§6.2).
    private func emitSpeechEngineState(isEngineReady: Bool, unavailableReasonText: String?) {
        var arguments: [String: Any] = ["isEngineReady": isEngineReady]
        if let unavailableReasonText = unavailableReasonText {
            arguments["unavailableReasonText"] = unavailableReasonText
        }
        emitEvent("RefappSpeechState", arguments)
    }

    /// The speech topic's delivery crossing to the React side verbatim: the
    /// contract's monotonic utterance id travels as its decimal string (the
    /// bridge's number conversion is lossy above 2^53), and the trigger kind
    /// in its lowercase enum label.
    private func emitSpeechInstruction(_ speechInstruction: SpeechInstruction) {
        let kindText: String
        switch speechInstruction.instructionKind {
        case .maneuverApproach: kindText = "maneuverApproach"
        case .immediateManeuver: kindText = "immediateManeuver"
        case .offRouteRecovery: kindText = "offRouteRecovery"
        case .arrival: kindText = "arrival"
        }
        emitEvent(
            "RefappSpeechInstruction",
            [
                "speechText": speechInstruction.utteranceSpeechText,
                "speechInstructionId": String(speechInstruction.utteranceId),
                "instructionKind": kindText.lowercased(),
            ]
        )
    }

    /// One consumer-voiced line through the platform engine (the bridge hop).
    @objc(speakGuidanceUtteranceWithSpeechText:engineUtteranceId:)
    func speakGuidanceUtterance(_ speechText: String, _ engineUtteranceId: String) {
        speechSpeaker?.speak(speechText: speechText, engineUtteranceId: engineUtteranceId)
    }

    /// The drawer's toggle-off path: whatever speaks, stops.
    @objc func stopGuidanceSpeech() {
        speechSpeaker?.stopSpeech()
    }

    /// The speech engine's current initialization state (the mount readback).
    func speechEngineState() -> (isEngineReady: Bool, unavailableReasonText: String?) {
        guard let speechSpeaker = speechSpeaker else {
            return (false, nil)
        }
        return speechSpeaker.speechEngineState()
    }

    // ------------------------------------------------------------------
    // SDK objects (built once, on the first ensureSdkStarted call)
    // ------------------------------------------------------------------

    /// The compiled SDK version the diagnostics sheet reports from the probe.
    var compiledVersion: String { MapsSdkNativeRuntime.shared.compiledVersion() }

    /// The package-owned SDK (one native instance for the whole process).
    private var nativeSdk: MapSdk? {
        MapsSdkNativeRuntime.shared.mapSdk
    }
    private let navigationController = Navigator()

    /// The shared demo artifact, loaded at SDK start (§9's native half).
    private var scenesArtifact: RefappScenesArtifact?

    private lazy var navigationListener = RefappCollectingListener(
        onInstruction: { [weak self] instruction in
            self?.emitTraceUpdateFromInstruction(instruction)
        },
        onDeviationStatusChange: { [weak self] deviationStatusChange in
            self?.emitDeviationUpdate(deviationStatusChange)
        },
        onPhaseChange: { [weak self] phaseChange in
            self?.emitPhaseUpdate(phaseChange)
        },
        onSpeechInstructionDelivery: { [weak self] speechInstruction in
            self?.emitSpeechInstruction(speechInstruction)
        }
    )

    private lazy var traceDriver = RefappTraceDriver(
        navigationController: navigationController,
        listener: navigationListener,
        onProcessedFix: { [weak self] processedFix in
            self?.handleSimulatedProcessedFix(processedFix)
        },
        onArrival: { [weak self] in
            self?.onSimulatedPlaybackEnded()
        }
    )

    /// Whether the SDK objects are built (`createSdk`'s outcome, honest).
    private(set) var isSdkStarted = false

    /// Builds the SDK objects keyless — no provider is registered, so the
    /// no-key state is first-class and honest
    /// (docs/Providers.md (Credentials) — and loads the
    /// shared demo artifact's native half. A credential entered at runtime
    /// registers later through `setProviderCredential`. Every failure of the
    /// artifact reports verbatim without failing the SDK start itself (the
    /// rendering surface needs no provider). Returns the honest success.
    @objc func ensureSdkStarted() -> Bool {
        dispatchPrecondition(condition: .onQueue(.main))
        if isSdkStarted {
            return true
        }
        guard nativeSdk != nil else {
            return false
        }
        isSdkStarted = true
        isTileCacheRegistered = MapsSdkNativeRuntime.shared
            .isProviderCredentialApplied
        tileCacheStateText = MapsSdkNativeRuntime.shared.tileCacheStateText
        emitErrorReportCleared()
        loadScenesArtifact()
        emitCacheState()
        emitPlanState()
        emitTraceState()
        scheduleLaunchArgumentAutomation()
        return true
    }

    private func loadScenesArtifact() {
        do {
            scenesArtifact = try RefappScenesLoader.load()
        } catch {
            scenesArtifact = nil
            reportFailureToReact(error)
        }
    }

    /// Applies one runtime provider credential (§3's explicit Apply,
    /// docs/Providers.md): the `mapquest` arm
    /// registers the provider set with the tile cache wrapped around the
    /// raster-tile contract (the offline-maps demo) using the app's caches
    /// directory under the named byte budget, then re-emits the readiness and
    /// cache state. A refusal's verbatim detail is returned to the caller and
    /// published on the error channel; the credential itself is never logged.
    /// A future provider adds one dispatch arm, never a view rewrite (§9).
    /// The provider key an automation-injected credential targets when the
    /// launch environment names none: the only provider the registration
    /// dispatch serves. Kept here (the dispatch arm) so callers never spell a
    /// vendor.
    @objc func defaultRuntimeInjectionProviderKey() -> String {
        RefappConstants.mapquestProviderKey
    }

    @objc func setProviderCredential(providerKey: String, credential: String) -> [String: Any] {
        dispatchPrecondition(condition: .onQueue(.main))
        let runtime = MapsSdkNativeRuntime.shared
        let result = runtime.setProviderCredential(
            providerKey: providerKey,
            credential: credential,
            withTileCache: true
        )
        let isApplied = result["isApplied"] as? Bool ?? false
        let detailText = result["detailText"] as? String ?? ""
        isTileCacheRegistered = isApplied
        tileCacheStateText = runtime.tileCacheStateText
        if isApplied {
            emitErrorReportCleared()
            emitCacheState()
            return providerCredentialApplyResult(isApplied: true, detailText: "")
        }
        emitErrorReport(variantLabel: "ProviderCredentialRefused", detailText: detailText)
        emitCacheState()
        return providerCredentialApplyResult(
            isApplied: false,
            detailText: detailText
        )
    }

    /// The bridge result shape: the honest applied flag and the verbatim detail.
    private func providerCredentialApplyResult(
        isApplied: Bool,
        detailText: String
    ) -> [String: Any] {
        ["isApplied": isApplied, "detailText": detailText]
    }

    /// A native-boundary refusal: published verbatim on the error channel too.
    private func providerCredentialRefusal(_ detailText: String) -> [String: Any] {
        emitErrorReport(variantLabel: "ProviderCredentialRefused", detailText: detailText)
        return providerCredentialApplyResult(isApplied: false, detailText: detailText)
    }

    // ------------------------------------------------------------------
    // The opt-in Keychain store (§3/§5: kSecClassGenericPassword; the
    // credential is never logged)
    // ------------------------------------------------------------------

    /// The stored credential, or nil when nothing is stored or it cannot be read.
    @objc func getStoredProviderCredential(providerKey: String) -> String? {
        dispatchPrecondition(condition: .onQueue(.main))
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: RefappConstants.providerCredentialKeychainService,
            kSecAttrAccount as String: providerKey,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]
        var item: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &item)
        guard status == errSecSuccess,
              let storedData = item as? Data,
              let storedCredential = String(data: storedData, encoding: .utf8) else {
            return nil
        }
        return storedCredential
    }

    /// Persists the credential behind the Keychain; false on failure.
    @objc func storeProviderCredential(providerKey: String, credential: String) -> Bool {
        dispatchPrecondition(condition: .onQueue(.main))
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: RefappConstants.providerCredentialKeychainService,
            kSecAttrAccount as String: providerKey,
        ]
        let attributes: [String: Any] = [
            kSecValueData as String: Data(credential.utf8),
            kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,
        ]
        let updateStatus = SecItemUpdate(query as CFDictionary, attributes as CFDictionary)
        if updateStatus == errSecSuccess {
            return true
        }
        if updateStatus != errSecItemNotFound {
            return false
        }
        let insertAttributes = query.merging(attributes) { _, newValue in newValue }
        return SecItemAdd(insertAttributes as CFDictionary, nil) == errSecSuccess
    }

    /// Removes the stored credential; false when the Keychain is unavailable.
    @objc func clearStoredProviderCredential(providerKey: String) -> Bool {
        dispatchPrecondition(condition: .onQueue(.main))
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: RefappConstants.providerCredentialKeychainService,
            kSecAttrAccount as String: providerKey,
        ]
        let status = SecItemDelete(query as CFDictionary)
        return status == errSecSuccess || status == errSecItemNotFound
    }

    /// Whether the tile-cache registration succeeded (the offline card's state).
    private(set) var isTileCacheRegistered = false

    /// The cache line's text: the registered file and budget, or the verbatim refusal.
    private(set) var tileCacheStateText = RefappConstants.cacheNotRegisteredText

    private func emitErrorReportCleared() {
        emitErrorReport(variantLabel: "", detailText: "")
    }

    // ------------------------------------------------------------------
    // Readiness ladder (§6.1) and the error report (§6.3)
    // ------------------------------------------------------------------

    enum ReadinessState {
        case loading, ready, degraded, failed
    }

    private(set) var readinessState: ReadinessState = .loading
    private(set) var readinessDetail: String?

    private var framesDrawnCount = 0
    private var framesDeferredCount = 0
    private var framesFailedCount = 0
    private var latestTileStatistics: FrameStatistics?
    private var framesSinceStatisticsEmit = 0

    /// Only the error currently on the React strip is cleared back (no flood).
    private var lastReportedError: String?

    /// The verbatim detail text of any failure (§6.3's mapping).
    private func failureDetailTextOf(_ failure: Error) -> String {
        if let sdkError = failure as? SdkError {
            switch sdkError {
            case let .InvalidCoordinateInput(detail): return detail
            case let .NetworkRequestFailed(detail): return detail
            case let .ResponseParsingFailure(detail): return detail
            case let .ProviderRequestRejected(detail): return detail
            case .RateLimitExhausted: return "rate limit exhausted; retry after the next refill"
            case let .ProviderNotConfigured(contractName):
                return "no '\(contractName)' provider is configured"
            case let .InvalidNavigationInput(detail): return detail
            case let .InvalidRenderInput(detail): return detail
            case let .RenderingSurfaceUnavailable(detail): return detail
            // The cache and offline variants carry their own verbatim reason.
            case let .CacheOperationFailure(detail): return detail
            case let .OfflineRoutingFailure(detail): return detail
            }
        }
        return (failure as? LocalizedError)?.errorDescription
            ?? failure.localizedDescription
    }

    private func failureReportOf(_ failure: Error) -> (variantLabel: String, detailText: String) {
        let variantLabel = String(describing: type(of: failure))
        return (variantLabel, failureDetailTextOf(failure))
    }

    private func reportFailureToReact(_ failure: Error) {
        let failureReport = failureReportOf(failure)
        lastReportedError = failureReport.variantLabel
        log.warning("reported failure: \(failureReport.variantLabel, privacy: .public): \(failureReport.detailText, privacy: .public)")
        emitErrorReport(variantLabel: failureReport.variantLabel, detailText: failureReport.detailText)
    }

    private func clearErrorOnReact() {
        emitErrorReportCleared()
    }

    // ------------------------------------------------------------------
    // The package MapView (surface, pump, gestures and camera live there)
    // ------------------------------------------------------------------

    /// The host view the demo drives; it also receives the camera readback.
    private var observedHostView: MapsSdkMapHostView?

    /// Whether a camera command has landed before the host view existed.
    private var hasCameraIntent = false

    private func packageHostView() -> MapsSdkMapHostView? {
        let currentHostView = MapsSdkMapHostView.currentHostView()
        if currentHostView !== observedHostView {
            observedHostView?.removeObserver(self)
            observedHostView = currentHostView
            currentHostView?.addObserver(self)
            if let currentHostView, hasCameraIntent {
                currentHostView.setCamera(desiredCameraPose())
            }
        }
        return currentHostView
    }

    /// The user-facing attach retry (the package view's own retry path).
    @objc func retrySurfaceAttach() {
        packageHostView()?.retryAttach()
        clearErrorOnReact()
    }

    // ------------------------------------------------------------------
    // Camera state (the SDK owns the camera math; the app owns the values)
    // ------------------------------------------------------------------

    private var centerLatitudeDegrees = 0.0
    private var centerLongitudeDegrees = 0.0
    private var zoomLevel = 0.0
    private var bearingDegrees = 0.0
    private var pitchDegrees = 0.0

    /// The 2D/3D plane the camera targets (§4.2's plane toggle).
    private var isThreeDCameraEnabled = false

    /// Whether a gesture broke the follow camera; it stays broken until the
    /// my-location control re-engages it.
    private var followBrokenByGesture = false

    /// The camera pose the demo's controls last asked the package view for.
    private func desiredCameraPose() -> MapsSdkCameraPose {
        MapsSdkCameraPose(
            latitudeDegrees: centerLatitudeDegrees,
            longitudeDegrees: centerLongitudeDegrees,
            zoomLevel: zoomLevel,
            bearingDegrees: bearingDegrees,
            pitchDegrees: pitchDegrees
        )
    }

    /// Applies the desired pose to the package view (a set, or an eased move).
    private func applyDesiredCamera(animated: Bool = false) {
        hasCameraIntent = true
        guard let packageHostView = packageHostView() else { return }
        let pose = desiredCameraPose()
        if animated {
            packageHostView.animateCamera(
                pose,
                durationMilliseconds: RefappConstants.easedCameraMoveDurationSeconds
                    * GeometryConstants.millisecondsPerSecond
            )
        } else {
            packageHostView.setCamera(pose)
        }
    }

    /// Adopts the package view's current camera into the demo's intent state.
    private func syncCameraStateFromHost() {
        guard let camera = packageHostView()?.camera() else { return }
        centerLatitudeDegrees = camera.latitudeDegrees
        centerLongitudeDegrees = camera.longitudeDegrees
        zoomLevel = camera.zoomLevel
        bearingDegrees = camera.bearingDegrees
        pitchDegrees = camera.pitchDegrees
    }

    /// A camera change signal (the package view owns the math and the pump).
    private func markCameraRevision() {
    }

    /// Applies one camera preset (the artifact's camera presets, fed from the
    /// React side, which requires the JSON artifact — the Metro asset).
    @objc(applyCameraPresetWithLatitudeDegrees:longitudeDegrees:zoomLevel:bearingDegrees:pitchDegrees:)
    func applyCameraPreset(
        _ presetLatitudeDegrees: Double,
        _ presetLongitudeDegrees: Double,
        _ presetZoomLevel: Double,
        _ presetBearingDegrees: Double,
        _ presetPitchDegrees: Double
    ) {
        centerLatitudeDegrees = presetLatitudeDegrees
        centerLongitudeDegrees = presetLongitudeDegrees
        zoomLevel = presetZoomLevel
        bearingDegrees = presetBearingDegrees
        pitchDegrees = presetPitchDegrees
        applyDesiredCamera()
        clearErrorOnReact()
    }

    /// Drag-pan (§5.2): the pixel delta crosses the package view's contract
    /// camera member, so the RN shell applies the same gesture algebra as
    /// every other shell. (The iOS `simctl` seam cannot inject touches; the
    /// method is still the bridge seam's parity half.)
    func panByPixels(_ deltaXPixels: Double, _ deltaYPixels: Double) {
        breakFollowOnGesture()
        packageHostView()?.panByScreenPixels(
            deltaXPixels: deltaXPixels,
            deltaYPixels: deltaYPixels,
            isGestureDriven: true
        )
        syncCameraStateFromHost()
    }

    // ------------------------------------------------------------------
    // The map control cluster (§4.2)
    // ------------------------------------------------------------------

    /// One zoom control step (§2.4's `map-control-zoom-step`; +1 or −1).
    private func zoomByControlStep(_ stepSign: Double) {
        packageHostView()?.zoomByLevels(
            deltaZoomLevels: stepSign * RefappConstants.mapControlZoomStep
        )
        syncCameraStateFromHost()
    }

    @objc func zoomInControl() {
        zoomByControlStep(1.0)
    }

    @objc func zoomOutControl() {
        zoomByControlStep(-1.0)
    }

    /// The my-location control (§4.2): centers on the indicator's latest fix;
    /// during a trip it also re-engages the follow camera.
    @objc func centerOnLatestLocation() {
        guard let latestFix = latestLocationFix?.snappedCoordinate else { return }
        centerLatitudeDegrees = latestFix.latitude
        centerLongitudeDegrees = latestFix.longitude
        if isTraceReplayActive {
            followBrokenByGesture = false
        }
        applyDesiredCamera()
    }

    /// The 2D/3D toggle (§4.2): swings the pitch between the two planes.
    @objc func toggleCameraPlane() {
        isThreeDCameraEnabled.toggle()
        pitchDegrees = cameraPitchTargetDegrees()
        applyDesiredCamera(animated: true)
    }

    /// The compass tap (§2.4): eases north up over the token's 300 ms; a
    /// camera edit breaks the follow camera like any gesture.
    @objc func easeCameraNorthUp() {
        breakFollowOnGesture()
        bearingDegrees = RefappConstants.northUpBearingDegrees
        applyDesiredCamera(animated: true)
    }

    /// A user gesture (§5.3: drag/pinch/rotate) breaks follow immediately; it
    /// stays broken until the my-location control re-engages it — a later fix
    /// does not.
    @objc func breakFollowOnGesture() {
        if isTraceReplayActive {
            followBrokenByGesture = true
        }
    }

    private func cameraPitchTargetDegrees() -> Double {
        isThreeDCameraEnabled
            ? RefappConstants.navigationFollowPitch3DDegrees
            : RefappConstants.twoDCameraPitchDegrees
    }

    // MARK: MapsSdkMapHostViewObserver

    func mapHostViewDidChangeCamera(
        _ hostView: MapsSdkMapHostView,
        camera: MapsSdkCameraPose,
        isGestureDriven: Bool
    ) {
        centerLatitudeDegrees = camera.latitudeDegrees
        centerLongitudeDegrees = camera.longitudeDegrees
        zoomLevel = camera.zoomLevel
        bearingDegrees = camera.bearingDegrees
        pitchDegrees = camera.pitchDegrees
        if isGestureDriven {
            breakFollowOnGesture()
        }
        emitCameraReadback()
    }

    func mapHostViewDidBecomeReady(_ hostView: MapsSdkMapHostView) {
        emitCameraReadback()
    }

    // ------------------------------------------------------------------
    // Search (geocode) and route planning (§4.1's plan flow)
    // ------------------------------------------------------------------

    /// The promise callbacks the React bridge adapts (one resolve value or
    /// one verbatim reject code/message; the bridge maps them onto the
    /// platform promise blocks).
    typealias PromiseResolve = (Any?) -> Void
    typealias PromiseReject = (String, String) -> Void

    /// Whether a provider search is running (one search at a time).
    private(set) var isSearchInFlight = false

    /// Runs the plan search on one field's text: the result rows travel to
    /// the React side through the promise (a `structuredAddressText`-plus-
    /// coordinate row per provider result, the Compose app's row semantics) —
    /// never from guesses; a provider failure reports itself in the error
    /// strip and rejects the promise (§4.1).
    @objc(searchPlacesWithQueryText:resolve:reject:)
    func searchPlaces(_ queryText: String, _ resolve: @escaping PromiseResolve, _ reject: @escaping PromiseReject) {
        dispatchPrecondition(condition: .onQueue(.main))
        if !isSdkStarted {
            reject("searchUnavailable", RefappConstants.searchBeforeSdkStateText)
            return
        }
        if isSearchInFlight {
            reject("searchInFlight", RefappConstants.searchInFlightText)
            return
        }
        if queryText.trimmingCharacters(in: .whitespaces).isEmpty {
            resolve([] as [[String: Any]])
            return
        }
        guard let constructedSdk = nativeSdk else { return }
        isSearchInFlight = true
        Task { @MainActor [weak self] in
            defer {
                self?.isSearchInFlight = false
            }
            do {
                let results = try await constructedSdk.forwardGeocode(addressText: queryText)
                let resultRows: [[String: Any]] = results
                    .prefix(RefappConstants.searchResultRowLimit)
                    .map { placeResult in
                        [
                            "structuredAddressText": placeResult.structuredAddressText,
                            "latitudeDegrees": placeResult.coordinate.latitude,
                            "longitudeDegrees": placeResult.coordinate.longitude,
                            "qualityText": String(describing: placeResult.geocodeQuality).lowercased(),
                        ]
                    }
                self?.clearErrorOnReact()
                resolve(resultRows)
            } catch {
                // Native-internal failures are no SdkError variant, and a
                // failing search must report, never crash (§6.3).
                self?.reportFailureToReact(error)
                reject("searchFailed", self?.failureDetailTextOf(error) ?? "unknown failure")
            }
        }
    }

    /// The resolved origin coordinate, or nil while the demo-position default stands.
    private var originPlaceCoordinate: GeographicCoordinate?

    /// The resolved destination coordinate, or nil while none is picked.
    private var destinationPlaceCoordinate: GeographicCoordinate?

    /// The planned route display name (the summary card's title).
    private var plannedRouteNameText: String?

    /// The search-planned route's trip source, or nil while none is planned.
    private var plannedRouteTrace: RefappTrace?

    /// Whether the planned route is the shared artifact's recorded demo
    /// route rather than a live planning result (the summary card says so).
    private var plannedRouteIsRecordedDemoRoute = false

    /// Whether the planned route came from the bundled offline test graph:
    /// the route summary says it is recorded test data, not live data.
    private var plannedRouteIsOfflineArchiveRoute = false

    /// Whether a route calculation is running (one plan at a time).
    private(set) var isPlanInFlight = false

    /// Whether the running SDK session has a provider credential applied.
    @objc func isProviderCredentialApplied() -> Bool {
        MapsSdkNativeRuntime.shared.isProviderCredentialApplied
    }

    /// The active route the demo drives: the planned route, else the picker's
    /// trace.
    private var activeRoute: Route? {
        activeLoadedTrace?.route
    }

    /// Assigns one provider result row to the plan field the search serves;
    /// there is no default origin (§4.1: the two-field flow asks for one).
    /// Assignment alone never plans: the explicit Plan route action is the
    /// only path that plans, and the promise resolves `true` once assigned.
    @objc(assignSearchResultWithAssignToOrigin:placeNameText:latitudeDegrees:longitudeDegrees:resolve:reject:)
    func assignSearchResult(
        _ assignToOrigin: Bool,
        _ placeNameText: String,
        _ latitudeDegrees: Double,
        _ longitudeDegrees: Double,
        _ resolve: @escaping PromiseResolve,
        _ reject: @escaping PromiseReject
    ) {
        dispatchPrecondition(condition: .onQueue(.main))
        let assignedCoordinate = GeographicCoordinate(
            latitude: latitudeDegrees,
            longitude: longitudeDegrees
        )
        if assignToOrigin {
            originPlaceCoordinate = assignedCoordinate
        } else {
            destinationPlaceCoordinate = assignedCoordinate
            plannedRouteNameText = placeNameText
        }
        clearErrorOnReact()
        resolve(true)
    }

    /// The explicit Plan route action (§4.1): plans between the two assigned
    /// coordinates with the plan sheet's Directions choice
    /// (PRD.md §6 item 36). A missing field rejects with a
    /// reason naming it; nothing is invented.
    @objc(planRouteWithTransportationModeOrdinal:routeStrategyOrdinal:resolve:reject:)
    func planRoute(
        _ transportationModeOrdinal: Int,
        _ routeStrategyOrdinal: Int,
        _ resolve: @escaping PromiseResolve,
        _ reject: @escaping PromiseReject
    ) {
        dispatchPrecondition(condition: .onQueue(.main))
        guard let resolvedOriginCoordinate = originPlaceCoordinate else {
            reject("planUnavailable", RefappConstants.planOriginUnresolvedText)
            return
        }
        guard let resolvedDestinationCoordinate = destinationPlaceCoordinate else {
            reject("planUnavailable", RefappConstants.planDestinationUnresolvedText)
            return
        }
        guard
            let transportationMode = Self.transportationMode(
                fromOrdinal: transportationModeOrdinal
            ),
            let routeStrategy = Self.routeStrategy(fromOrdinal: routeStrategyOrdinal)
        else {
            reject("planUnavailable", RefappConstants.planUnknownDirectionsChoiceText)
            return
        }
        planRouteBetween(
            waypoints: [resolvedOriginCoordinate, resolvedDestinationCoordinate],
            planNameText: plannedRouteNameText ?? RefappConstants.plannedRouteFallbackName,
            routeOptions: Self.referenceRouteOptions(
                transportationMode: transportationMode,
                routeStrategy: routeStrategy
            ),
            resolve: resolve,
            reject: reject
        )
    }

    /// The keyless plan (§4.1): applies the shared artifact's recorded route
    /// for the ordered demo-place pair — no provider call and no invented
    /// geometry. A pair the artifact does not record reports the named reason
    /// and rejects, exactly as the explicit live plan reports a refusal.
    @objc(planRecordedDemoRouteWithOriginName:destinationName:resolve:reject:)
    func planRecordedDemoRoute(
        _ originName: String,
        _ destinationName: String,
        _ resolve: @escaping PromiseResolve,
        _ reject: @escaping PromiseReject
    ) {
        dispatchPrecondition(condition: .onQueue(.main))
        if isPlanInFlight {
            reject("planInFlight", RefappConstants.planAlreadyRunningText)
            return
        }
        let recordedRoute = scenesArtifact?.demoRoutes.first { demoRoute in
            demoRoute.originName == originName && demoRoute.destinationName == destinationName
        }
        guard let recordedRoute else {
            let detailText = String(
                format: RefappConstants.noRecordedRouteHintFormat,
                originName,
                destinationName
            )
            emitErrorReport(variantLabel: "MissingRecordedRoute", detailText: detailText)
            reject("noRecordedRoute", detailText)
            return
        }
        isPlanInFlight = true
        emitPlanState()
        plannedRouteTrace = RefappTrace.fromRouteData(
            route: recordedRoute.route,
            traceName: RefappConstants.plannedRouteTraceName
        )
        plannedRouteNameText = destinationName
        plannedRouteIsRecordedDemoRoute = true
        plannedRouteIsOfflineArchiveRoute = false
        applyRoutePreviewFraming(recordedRoute.route)
        pushOverlaysToSurface()
        clearErrorOnReact()
        isPlanInFlight = false
        emitPlanState()
        resolve(true)
    }

    /// The offline card's route control (§2.5 `offline.plan_route_action`):
    /// reads the committed compiled routing archive from the application
    /// bundle and plans the demo request over it through the contract's
    /// `offlineRouteFromArchiveBytes` free function. The route feeds the same
    /// summary card, overlay line and simulated trip as a live plan, and the
    /// summary says it is recorded test data, not live data. No provider and
    /// no network round trip are involved.
    @objc(planOfflineRouteFromArchiveWithResolve:reject:)
    func planOfflineRouteFromArchive(
        _ resolve: @escaping PromiseResolve,
        _ reject: @escaping PromiseReject
    ) {
        dispatchPrecondition(condition: .onQueue(.main))
        if isPlanInFlight {
            reject("planInFlight", RefappConstants.planAlreadyRunningText)
            return
        }
        guard let archiveURL = Bundle.main.url(
            forResource: RefappConstants.offlineRoutingArchiveResourceName,
            withExtension: RefappConstants.offlineRoutingArchiveResourceExtension
        ) else {
            let detailText =
                "the bundled offline routing archive is missing from the application bundle"
            emitErrorReport(variantLabel: "OfflineArchiveUnavailable", detailText: detailText)
            reject("offlineArchiveUnavailable", detailText)
            return
        }
        let archiveBytes: Data
        do {
            archiveBytes = try Data(contentsOf: archiveURL)
        } catch {
            // A missing or unreadable bundled archive reports verbatim; it is
            // a build-asset failure, never a silent no-op.
            reportFailureToReact(error)
            reject("offlineArchiveUnavailable", failureDetailTextOf(error))
            return
        }
        isPlanInFlight = true
        emitPlanState()
        Task { @MainActor [weak self] in
            do {
                let route = try await offlineRouteFromArchiveBytes(
                    archiveBytes: archiveBytes,
                    start: GeographicCoordinate(
                        latitude: RefappConstants.offlineRouteStartLatitudeDegrees,
                        longitude: RefappConstants.offlineRouteStartLongitudeDegrees
                    ),
                    end: GeographicCoordinate(
                        latitude: RefappConstants.offlineRouteEndLatitudeDegrees,
                        longitude: RefappConstants.offlineRouteEndLongitudeDegrees
                    )
                )
                let tripTrace = RefappTrace.fromRouteData(
                    route: route,
                    traceName: RefappConstants.plannedRouteTraceName
                )
                self?.plannedRouteTrace = tripTrace
                self?.plannedRouteNameText = RefappConstants.offlineRouteTitleText
                self?.plannedRouteIsRecordedDemoRoute = false
                self?.plannedRouteIsOfflineArchiveRoute = true
                self?.applyRoutePreviewFraming(route)
                self?.pushOverlaysToSurface()
                self?.clearErrorOnReact()
                resolve(true)
            } catch {
                let failureDetail = self?.failureDetailTextOf(error) ?? "unknown failure"
                // A refused archive (invalid bytes, an out-of-graph
                // coordinate, a disconnected pair) reports its verbatim
                // detail through the same error channel a live refusal uses.
                self?.plannedRouteTrace = nil
                self?.plannedRouteNameText = nil
                self?.plannedRouteIsRecordedDemoRoute = false
                self?.plannedRouteIsOfflineArchiveRoute = false
                self?.reportFailureToReact(error)
                reject("offlineRoutePlanFailed", failureDetail)
            }
            defer {
                // The one settle-time emission: it must run after the in-flight
                // flag clears, else the summary renders the planning line.
                self?.isPlanInFlight = false
                self?.emitPlanState()
            }
        }
    }

    /// Plans the route through the two resolved waypoints (§4.1): builds the
    /// simulated trip source over the route's own shape, fits the preview
    /// framing and pushes the overlay set — then emits the plan state the
    /// route summary card renders.
    private func planRouteBetween(
        waypoints: [GeographicCoordinate],
        planNameText: String,
        routeOptions: RouteOptions,
        resolve: @escaping PromiseResolve,
        reject: @escaping PromiseReject
    ) {
        if isPlanInFlight {
            reject("planInFlight", RefappConstants.planAlreadyRunningText)
            return
        }
        guard let constructedSdk = nativeSdk else {
            reject("planUnavailable", RefappConstants.searchBeforeSdkStateText)
            return
        }
        isPlanInFlight = true
        emitPlanState()
        Task { @MainActor [weak self] in
            do {
                let route = try await constructedSdk.calculateRoute(
                    waypoints: waypoints,
                    routeOptions: routeOptions
                )
                let tripTrace = RefappTrace.fromRouteData(
                    route: route,
                    traceName: RefappConstants.plannedRouteTraceName
                )
                self?.plannedRouteTrace = tripTrace
                self?.plannedRouteNameText = planNameText
                self?.plannedRouteIsRecordedDemoRoute = false
                self?.plannedRouteIsOfflineArchiveRoute = false
                self?.applyRoutePreviewFraming(route)
                self?.pushOverlaysToSurface()
                self?.clearErrorOnReact()
                resolve(true)
            } catch {
                let failureDetail = self?.failureDetailTextOf(error) ?? "unknown failure"
                // Native-internal failures are no SdkError variant, and a
                // failing routing must report, never crash (§6.3).
                self?.plannedRouteTrace = nil
                self?.plannedRouteNameText = nil
                self?.plannedRouteIsRecordedDemoRoute = false
                self?.plannedRouteIsOfflineArchiveRoute = false
                self?.reportFailureToReact(error)
                reject("routePlanFailed", failureDetail)
            }
            defer {
                // The one settle-time emission (both paths): it must run
                // AFTER the in-flight flag clears, else the summary card
                // renders the planning line forever.
                self?.isPlanInFlight = false
                self?.emitPlanState()
            }
        }
    }

    /// The routing options of one plan: the plan sheet's Directions choice
    /// with metric display units and no avoided road features.
    private static func referenceRouteOptions(
        transportationMode: TransportationMode,
        routeStrategy: RouteStrategy
    ) -> RouteOptions {
        RouteOptions(
            transportationMode: transportationMode,
            routeStrategy: routeStrategy,
            unitSystem: .metric,
            avoidOptions: AvoidOptions(
                tollRoads: false,
                limitedAccessHighways: false,
                ferries: false,
                unpavedRoads: false
            ),
            departureTimestampSeconds: nil
        )
    }

    /// The transport mode a generated enum ordinal names.
    private static func transportationMode(
        fromOrdinal ordinal: Int
    ) -> TransportationMode? {
        switch ordinal {
        case 0: return .automobile
        case 1: return .bicycle
        case 2: return .pedestrian
        default: return nil
        }
    }

    /// The route strategy a generated enum ordinal names.
    private static func routeStrategy(
        fromOrdinal ordinal: Int
    ) -> RouteStrategy? {
        switch ordinal {
        case 0: return .fastest
        case 1: return .shortest
        default: return nil
        }
    }

    /// Fits the planned route between the waypoints (§5.1 camera fitting):
    /// the bottom inset clears the half-state sheet so the preview line stays
    /// visible above it, and the fitted camera's values read back through the
    /// attribute getters.
    private func applyRoutePreviewFraming(_ route: Route) {
        guard let packageHostView = packageHostView() else { return }
        let viewportWidthPixels = packageHostView.viewportWidthPixels
        let viewportHeightPixels = packageHostView.viewportHeightPixels
        guard viewportWidthPixels > 0, viewportHeightPixels > 0 else { return }
        packageHostView.fitBounds(
            route.boundingBox,
            edgeInsets: EdgeInsets(
                left: RefappConstants.routePreviewInsetLeftPixels,
                top: RefappConstants.routePreviewInsetTopPixels,
                right: RefappConstants.routePreviewInsetRightPixels,
                bottom: Double(viewportHeightPixels)
                    * RefappConstants.sheetHalfHeightFraction
            ),
            maximumZoomLevel: RefappConstants.maximumFitBoundsZoomLevel
        )
        syncCameraStateFromHost()
        clearErrorOnReact()
    }

    // ------------------------------------------------------------------
    // Scene overlays (§9: the shared artifact feeds the SDK overlay API)
    // ------------------------------------------------------------------

    /// Toggle state per artifact overlay scene key (route/polygon/markers/traffic).
    private var sceneToggleSelections: [String: Bool] = Dictionary(
        uniqueKeysWithValues: RefappConstants.overlaySceneKeys.map { ($0, false) }
    )

    /// Count of overlays the last surface push carried (diagnostics fact).
    private(set) var pushedOverlayCount = 0

    /// Sets one scene chip's selection (§4.1's chip carousel) and repushes.
    @objc(setSceneOverlayToggleWithSceneKey:isSelected:) func setSceneOverlayToggle(_ sceneKey: String, _ isSelected: Bool) {
        guard sceneToggleSelections.keys.contains(sceneKey) else {
            return
        }
        sceneToggleSelections[sceneKey] = isSelected
        pushOverlaysToSurface()
    }

    /// The Clear control: every chip unselected and the surface overlay list empty.
    @objc func clearSceneOverlays() {
        for sceneKey in sceneToggleSelections.keys {
            sceneToggleSelections[sceneKey] = false
        }
        pushOverlaysToSurface()
    }

    private func sceneDescriptor(_ sceneKey: String, _ scene: RefappOverlayScene) -> MapOverlay? {
        if sceneToggleSelections[sceneKey] != true {
            return nil
        }
        if let coordinates = scene.coordinates {
            return MapOverlay(
                kind: .routePolyline,
                coordinates: coordinates,
                rgbaColor: nil,
                widthPixels: nil,
                ringCoordinates: nil,
                pointMarkerPositions: nil,
                pointMarkerRadiusPixels: nil,
                trafficFlowSegments: nil
            )
        }
        if let ringCoordinates = scene.ringCoordinates {
            return MapOverlay(
                kind: .filledPolygon,
                coordinates: nil,
                rgbaColor: nil,
                widthPixels: nil,
                ringCoordinates: ringCoordinates,
                pointMarkerPositions: nil,
                pointMarkerRadiusPixels: nil,
                trafficFlowSegments: nil
            )
        }
        if let positions = scene.positions {
            return MapOverlay(
                kind: .pointMarkers,
                coordinates: nil,
                rgbaColor: nil,
                widthPixels: nil,
                ringCoordinates: nil,
                pointMarkerPositions: positions,
                pointMarkerRadiusPixels: nil,
                trafficFlowSegments: nil
            )
        }
        if let trafficFlowSegments = scene.trafficFlowSegments {
            return MapOverlay(
                kind: .trafficFlow,
                coordinates: nil,
                rgbaColor: nil,
                widthPixels: nil,
                ringCoordinates: nil,
                pointMarkerPositions: nil,
                pointMarkerRadiusPixels: nil,
                trafficFlowSegments: trafficFlowSegments
            )
        }
        return nil
    }

    /// The whole overlay list of the current frame: the demo scenes of the
    /// shared artifact plus the active plan's route line. Overlay style falls
    /// to the documented rustic-maps-rendering defaults — GPU-surface paint is a
    /// renderer constant, not a UX token (§2.2).
    private func overlayDescriptorList() -> [MapOverlay] {
        var overlayDescriptors: [MapOverlay] = []
        for sceneKey in RefappConstants.overlaySceneKeys {
            if let scene = scenesArtifact?.overlayScenes[sceneKey],
               let descriptor = sceneDescriptor(sceneKey, scene) {
                overlayDescriptors.append(descriptor)
            }
        }
        if let activeRoute = activeRoute {
            overlayDescriptors.append(
                MapOverlay(
                    kind: .routePolyline,
                    coordinates: activeRoute.legs.flatMap { leg in leg.steps.flatMap { step in step.shape } },
                    rgbaColor: nil,
                    widthPixels: nil,
                    ringCoordinates: nil,
                    pointMarkerPositions: nil,
                    pointMarkerRadiusPixels: nil,
                    trafficFlowSegments: nil
                )
            )
            // The route's A/B endpoint pins (§4.1), the same additive overlay
            // kind and colors the other shells draw.
            if let originCoordinate = activeRoute.legs.first?.steps.first?.shape.first,
               let destinationCoordinate = activeRoute.legs.last?.steps.last?.shape.last {
                overlayDescriptors.append(
                    MapOverlay(
                        kind: .pointPins,
                        coordinates: nil,
                        rgbaColor: OverlayColor(red: 0.13, green: 0.62, blue: 0.29, alpha: 1.0),
                        widthPixels: nil,
                        ringCoordinates: nil,
                        pointMarkerPositions: [originCoordinate],
                        pointMarkerRadiusPixels: nil,
                        trafficFlowSegments: nil
                    )
                )
                overlayDescriptors.append(
                    MapOverlay(
                        kind: .pointPins,
                        coordinates: nil,
                        rgbaColor: OverlayColor(red: 0.85, green: 0.16, blue: 0.20, alpha: 1.0),
                        widthPixels: nil,
                        ringCoordinates: nil,
                        pointMarkerPositions: [destinationCoordinate],
                        pointMarkerRadiusPixels: nil,
                        trafficFlowSegments: nil
                    )
                )
            }
        }
        return overlayDescriptors
    }

    /// Pushes the current overlay list to the surface; a refusal reports verbatim.
    func pushOverlaysToSurface() {
        let overlayList = overlayDescriptorList()
        packageHostView()?.setOverlays(overlayList)
        pushedOverlayCount = overlayList.count
        clearErrorOnReact()
    }

    // ------------------------------------------------------------------
    // The renderer-owned location indicator (§4.2)
    // ------------------------------------------------------------------

    /// The latest simulated fix the indicator shows, or nil when clear (no
    /// fix renders nothing — never a stale last position, §4.2).
    private var latestLocationFix: RefappProcessedFix?

    /// Pushes the location indicator for one fix to the renderer (§4.2: the
    /// visuals are renderer-owned; the app only sets state/style): the guided
    /// trip draws the `guidedCourseArrow` wedge along the reported course,
    /// tracking draws the `trackingDot`, and the accuracy ring follows the
    /// fix's own report.
    private func pushLocationIndicator(guided: Bool, fix: RefappProcessedFix) {
        guard let snappedCoordinate = fix.snappedCoordinate else { return }
        packageHostView()?.setLocationIndicator(
            state: LocationIndicatorState(
                coordinate: GeographicCoordinate(
                    latitude: snappedCoordinate.latitude,
                    longitude: snappedCoordinate.longitude
                ),
                courseHeadingDegrees: guided ? fix.courseHeadingDegrees : nil,
                horizontalAccuracyMeters: fix.horizontalAccuracyMeters
            ),
            style: LocationIndicatorStyle(
                visualKind: guided ? .guidedCourseArrow : .trackingDot,
                radiusPixels: nil,
                rgbaColor: nil,
                ringRgbaColor: nil,
                haloRgbaColor: nil,
                showAccuracyRing: true
            )
        )
        latestLocationFix = fix
    }

    /// Clears the renderer's location layer (§4.2's no-fix state).
    private func clearLocationIndicatorOnSurface() {
        if latestLocationFix == nil {
            return
        }
        packageHostView()?.clearLocationIndicator()
        latestLocationFix = nil
    }

    // ------------------------------------------------------------------
    // The navigation follow camera (§5.3)
    // ------------------------------------------------------------------

    /// The camera values captured at trip start and restored at trip end.
    private struct PreTripCamera {
        let centerLatitudeDegrees: Double
        let centerLongitudeDegrees: Double
        let zoomLevel: Double
        let bearingDegrees: Double
        let pitchDegrees: Double
    }

    /// The captured pre-trip camera, or nil outside a trip.
    private var preTripCamera: PreTripCamera?

    /// The follow camera's bearing target: the median-smoothed course, or nil.
    private var followTargetBearingDegrees: Double?

    /// The median-smoothed course-over-ground (§5.3; the Compose constant
    /// window).
    private let courseMedianSmoother = CourseMedianSmoother(
        RefappConstants.courseSmoothingWindowFixes
    )

    /// One simulated fix of the active trip: the follow camera's targets
    /// (center, the median-smoothed bearing and the ramped zoom) update on
    /// every fix, but the camera only moves while follow is engaged — a
    /// gesture-broken follow keeps the user's panned view until the
    /// my-location control re-engages it (§5.3). The renderer-owned location
    /// indicator follows the fix in the guided wedge form regardless.
    private func handleSimulatedProcessedFix(_ processedFix: RefappProcessedFix) {
        guard let snappedCoordinate = processedFix.snappedCoordinate else { return }
        if isTraceReplayActive && traceDriver.isTripActive {
            if let courseHeadingDegrees = processedFix.courseHeadingDegrees {
                courseMedianSmoother.add(courseHeadingDegrees)
                followTargetBearingDegrees = courseMedianSmoother.median()
            }
            if !followBrokenByGesture {
                centerLatitudeDegrees = snappedCoordinate.latitude
                centerLongitudeDegrees = snappedCoordinate.longitude
                zoomLevel = followZoomTowardTripEnd(
                    remainingDistanceFractionOf(activeRoute?.distanceMeters ?? 0.0)
                )
                pitchDegrees = cameraPitchTargetDegrees()
                if let targetBearingDegrees = followTargetBearingDegrees {
                    bearingDegrees = targetBearingDegrees
                }
                applyDesiredCamera()
            }
        }
        // The guided wedge rides the *playing* trip (a trailing fix behind the
        // playback's end arrives during the arrived presentation, where §7.1
        // shows the tracking dot, not the guided wedge).
        pushLocationIndicator(guided: traceDriver.isTripActive, fix: processedFix)
        // The progress line advances with every fix (the drawer's ETA row).
        emitTraceUpdateFromListener()
    }

    /// The remaining-distance fraction of the active trip's route (the zoom
    /// ramp's input).
    private func remainingDistanceFractionOf(_ totalRouteDistanceMeters: Double) -> Double {
        let remainingMeters = traceDriver.latestTripState?.progress.distanceRemainingMeters
            ?? totalRouteDistanceMeters
        if totalRouteDistanceMeters > 0.0 {
            return remainingMeters / totalRouteDistanceMeters
        }
        return 1.0
    }

    /// The follow zoom toward trip end (§5.3): the fixed deep zoom at the
    /// trip's start, ramping linearly with the *travelled* fraction to the
    /// maximum at arrival (the Compose application's documented ramp).
    private func followZoomTowardTripEnd(_ remainingDistanceFraction: Double) -> Double {
        let clampedRemainingFraction = max(0.0, min(1.0, remainingDistanceFraction))
        let travelledFraction = 1.0 - clampedRemainingFraction
        return RefappConstants.navigationFollowZoomLevel
            + (RefappConstants.navigationFollowZoomMaximumLevel
                - RefappConstants.navigationFollowZoomLevel)
            * travelledFraction
    }

    /// Restores the captured pre-trip camera (§5.3: restored after).
    private func restorePreTripCamera() {
        guard let restorableCamera = preTripCamera else { return }
        centerLatitudeDegrees = restorableCamera.centerLatitudeDegrees
        centerLongitudeDegrees = restorableCamera.centerLongitudeDegrees
        zoomLevel = restorableCamera.zoomLevel
        bearingDegrees = restorableCamera.bearingDegrees
        pitchDegrees = restorableCamera.pitchDegrees
        followTargetBearingDegrees = nil
        courseMedianSmoother.reset()
        isThreeDCameraEnabled = false
        preTripCamera = nil
        applyDesiredCamera()
    }

    // ------------------------------------------------------------------
    // The demo-plan picker's traces (§4.1; the Compose application's demo
    // plans)
    // ------------------------------------------------------------------

    /// The loaded plan traces by their application-bundle name (one load per
    /// plan).
    private var loadedPlanTraces: [String: RefappTrace] = [:]

    /// The plan picker's selected recorded walk (the Compose application's
    /// picker default is the artifact's own trace; the picker stays on the
    /// artifact framing until the first pick).
    private var selectedPlanTrace: RefappTrace?

    /// The default plan's bundle name: the artifact's trace, or the built-in
    /// demo trace.
    private var defaultTraceAssetName: String {
        scenesArtifact?.traceAssetName ?? RefappDemoSession.builtInTraceAssetName
    }

    /// Loads (or takes from the cache) one plan trace by its bundle name.
    private func loadedTraceFor(_ traceAssetName: String) throws -> RefappTrace {
        if let cachedTrace = loadedPlanTraces[traceAssetName] {
            return cachedTrace
        }
        let loadedTrace = try RefappTraceLoader.load(traceAssetName: traceAssetName)
        loadedPlanTraces[traceAssetName] = loadedTrace
        return loadedTrace
    }

    /// Loads one picker plan and applies it (§4.1; the Compose application's
    /// `selectDemoPlan`): the selected plan becomes the trip source "Start
    /// simulated navigation" drives, a search-planned route is dropped, the
    /// route preview frames the plan's own route, and the overlay set repushes
    /// with the plan's route line. A pick while the simulated trip runs is
    /// refused (the drawer owns the dock during a trip) — never a silent
    /// start of a half-selected plan.
    @objc(selectTraceReplayPlanWithTraceAssetName:resolve:reject:)
    func selectTraceReplayPlan(
        _ traceAssetName: String,
        _ resolve: @escaping PromiseResolve,
        _ reject: @escaping PromiseReject
    ) {
        dispatchPrecondition(condition: .onQueue(.main))
        if isTraceReplayActive {
            reject("tripRunning", RefappConstants.planSelectTripRunningText)
            return
        }
        let trace: RefappTrace
        do {
            trace = try loadedTraceFor(traceAssetName)
        } catch {
            // A refused pick reports verbatim (§6.3), never a guessed plan.
            reportFailureToReact(error)
            reject(
                "traceUnavailable",
                RefappConstants.planSelectTraceUnavailableText + ": " + failureDetailTextOf(error)
            )
            return
        }
        selectedPlanTrace = trace
        plannedRouteTrace = nil
        plannedRouteNameText = nil
        plannedRouteIsRecordedDemoRoute = false
        plannedRouteIsOfflineArchiveRoute = false
        applyRoutePreviewFraming(trace.route)
        pushOverlaysToSurface()
        clearErrorOnReact()
        emitPlanState()
        emitTraceState()
        resolve(true)
    }

    /// The trip source "Start simulated navigation" drives: the
    /// search-planned route when one is planned, otherwise the picker's
    /// selected plan, otherwise the recorded artifact trace (loaded from the
    /// application bundle on first use — the Compose app's artifact-trace
    /// plan).
    private func activeRouteTrace() throws -> RefappTrace {
        if let plannedTrace = plannedRouteTrace {
            return plannedTrace
        }
        if let selectedTrace = selectedPlanTrace {
            return selectedTrace
        }
        return try loadedTraceFor(defaultTraceAssetName)
    }

    /// The loaded trace currently on the demo (the progress line's and the
    /// trace state's honest fallback): the planned route, the selected plan,
    /// or the loaded artifact trace (nil before the first start).
    private var activeLoadedTrace: RefappTrace? {
        plannedRouteTrace
            ?? selectedPlanTrace
            ?? loadedPlanTraces[defaultTraceAssetName]
    }

    /// Starts the simulated trip on the active plan (the route summary's
    /// primary act): the follow camera engages at trip start (§5.3) — the
    /// deep follow zoom applies immediately, the 3D pitch applies at start,
    /// and the bearing eases toward the median course once fixes report it.
    /// A start failure leaves the home sheet with the verbatim report, never
    /// a half-started navigation mode.
    @objc func startTraceReplay() {
        dispatchPrecondition(condition: .onQueue(.main))
        // One trip at a time: a device-driven trip ends before the replay
        // starts (the location source itself belongs to the React side and is
        // stopped by the shell).
        stopDeviceNavigation()
        let trace: RefappTrace
        do {
            trace = try activeRouteTrace()
        } catch {
            reportFailureToReact(error)
            return
        }
        // The pre-trip camera is captured at trip start and restored after.
        preTripCamera = PreTripCamera(
            centerLatitudeDegrees: centerLatitudeDegrees,
            centerLongitudeDegrees: centerLongitudeDegrees,
            zoomLevel: zoomLevel,
            bearingDegrees: bearingDegrees,
            pitchDegrees: pitchDegrees
        )
        isTraceReplayActive = true
        // The active trace's route and endpoint pins draw beneath the trip
        // even before a plan chip was tapped: the start action itself makes
        // the route the trip follows visible (the same route preview the
        // other shells show).
        pushOverlaysToSurface()
        followTargetBearingDegrees = nil
        courseMedianSmoother.reset()
        zoomLevel = RefappConstants.navigationFollowZoomLevel
        pitchDegrees = RefappConstants.navigationFollowPitch3DDegrees
        isThreeDCameraEnabled = true
        applyDesiredCamera()
        traceDriver.startTrip(trace: trace)
        if let startFailure = traceDriver.startFailure {
            reportFailureToReact(startFailure)
        } else {
            clearErrorOnReact()
        }
        emitTraceUpdateFromListener()
        emitTraceState()
        emitPlaybackState()
    }

    /// Whether guidance currently follows the device source's own fixes on the
    /// planned route (the Location group's second control).
    private(set) var isDeviceNavigationActive = false

    /// Starts guidance on the planned route from this device's fixes. The
    /// location source itself belongs to the React side, which starts it
    /// through the same permission and source path the location control uses;
    /// this method starts the trip and reuses the session's trip presentation,
    /// so the banner, the ETA line, the location indicator and the follow
    /// camera all read the controller's own stream. Without a planned route it
    /// answers `false` and starts nothing.
    @objc func startDeviceNavigation() -> Bool {
        dispatchPrecondition(condition: .onQueue(.main))
        if isDeviceNavigationActive {
            return true
        }
        guard let planned = plannedRouteTrace else {
            return false
        }
        // One trip at a time: the simulated presentation and its camera end
        // before the device-driven one engages.
        if isTraceReplayActive {
            exitTraceReplay()
        }
        // The pre-trip camera is captured at trip start and restored after.
        preTripCamera = PreTripCamera(
            centerLatitudeDegrees: centerLatitudeDegrees,
            centerLongitudeDegrees: centerLongitudeDegrees,
            zoomLevel: zoomLevel,
            bearingDegrees: bearingDegrees,
            pitchDegrees: pitchDegrees
        )
        traceDriver.startDeviceTrip(route: planned.route)
        if let startFailure = traceDriver.startFailure {
            reportFailureToReact(startFailure)
            restorePreTripCamera()
            emitTraceState()
            emitPlaybackState()
            emitDeviceNavigationState()
            return false
        }
        isTraceReplayActive = true
        isDeviceNavigationActive = true
        // The planned route's line and endpoint pins draw beneath the trip.
        pushOverlaysToSurface()
        followTargetBearingDegrees = nil
        courseMedianSmoother.reset()
        zoomLevel = RefappConstants.navigationFollowZoomLevel
        // The follow camera's 3D course-follow pitch is the plane toggle's: a
        // device-driven trip does not turn the toggle on by itself.
        pitchDegrees = cameraPitchTargetDegrees()
        applyDesiredCamera()
        clearErrorOnReact()
        emitTraceUpdateFromListener()
        emitTraceState()
        emitPlaybackState()
        emitDeviceNavigationState()
        return true
    }

    /// Ends device-driven guidance. The location source belongs to the React
    /// side, which stops it only when this mode started it.
    @objc func stopDeviceNavigation() {
        dispatchPrecondition(condition: .onQueue(.main))
        if !isDeviceNavigationActive {
            return
        }
        isDeviceNavigationActive = false
        traceDriver.stopTrip()
        speechSpeaker?.releaseJourneySpeech()
        isTraceReplayActive = false
        restorePreTripCamera()
        clearLocationIndicatorOnSurface()
        markCameraRevision()
        emitTraceState()
        emitPlaybackState()
        emitDeviceNavigationState()
    }

    /// One accepted device fix of a device-driven trip, delivered through the
    /// driver's same fix path as a simulated fix. A refused fix ends the mode
    /// honestly rather than leaving a stale trip on screen (§6.3).
    @objc(deliverDeviceNavigationFixWithLatitudeDegrees:longitudeDegrees:courseHeadingDegrees:horizontalAccuracyMeters:)
    func deliverDeviceNavigationFix(
        latitudeDegrees: Double,
        longitudeDegrees: Double,
        courseHeadingDegrees: NSNumber?,
        horizontalAccuracyMeters: NSNumber?
    ) {
        dispatchPrecondition(condition: .onQueue(.main))
        if !isDeviceNavigationActive {
            return
        }
        traceDriver.deliverDeviceFix(
            coordinate: GeographicCoordinate(
                latitude: latitudeDegrees,
                longitude: longitudeDegrees
            ),
            courseHeadingDegrees: courseHeadingDegrees?.doubleValue,
            horizontalAccuracyMeters: horizontalAccuracyMeters?.doubleValue
                ?? RefappConstants.deviceNavigationFallbackAccuracyMeters
        )
        if let deliveryFailure = traceDriver.tripDeliveryFailure {
            stopDeviceNavigation()
            reportFailureToReact(deliveryFailure)
        }
    }

    private func emitDeviceNavigationState() {
        emitEvent(
            "RefappDeviceNavigationState",
            ["isActive": isDeviceNavigationActive]
        )
    }

    /// The drawer's pause act: the playback holds its current fix (§7.2).
    @objc func pauseTraceReplay() {
        dispatchPrecondition(condition: .onQueue(.main))
        traceDriver.pauseTrip()
        emitPlaybackState()
    }

    /// The drawer's resume act: the playback continues from its current fix.
    @objc func resumeTraceReplay() {
        dispatchPrecondition(condition: .onQueue(.main))
        traceDriver.resumeTrip()
        emitPlaybackState()
    }

    /// The drawer's seek act: moves the presented trip to a travelled
    /// distance along the route through the walker, so the camera, the
    /// indicator, the ETA line and the arrival follow as on a normal fix.
    @objc(seekTraceReplay:)
    func seekTraceReplay(_ travelledMeters: Double) {
        dispatchPrecondition(condition: .onQueue(.main))
        traceDriver.seekToTravelledMeters(travelledMeters)
        emitTraceUpdateFromListener()
        emitPlaybackState()
    }

    /// The drawer's speed act: a route-following trip rebuilds its walker at
    /// the new pace and re-anchors the clock at the travelled distance, so the
    /// position never jumps; a recorded trace's replay cadence follows the
    /// choice.
    @objc(selectTraceReplaySpeedMultiplier:)
    func selectTraceReplaySpeedMultiplier(_ speedMultiplier: Double) {
        dispatchPrecondition(condition: .onQueue(.main))
        traceDriver.selectSpeedMultiplier(speedMultiplier)
        emitPlaybackState()
    }

    /// The drawer's exit control: stop the replay, restore the captured
    /// pre-trip camera (§5.3: restored after), clear the location indicator
    /// (the demo's simulated device stops reporting); the planned route and
    /// its summary card stay for the sheet's return to half.
    @objc func exitTraceReplay() {
        dispatchPrecondition(condition: .onQueue(.main))
        // The drawer's exit control ends whichever trip is presented; a
        // device-driven one stops through its own path (the shell stops the
        // location source only when this mode started it).
        if isDeviceNavigationActive {
            stopDeviceNavigation()
            return
        }
        traceDriver.stopTrip()
        // The trip's speech lifecycle ends: the audio session equivalent
        // abandons once the engine drained (an in-flight utterance keeps it
        // until its completion).
        speechSpeaker?.releaseJourneySpeech()
        isTraceReplayActive = false
        restorePreTripCamera()
        clearLocationIndicatorOnSurface()
        markCameraRevision()
        emitTraceState()
        emitPlaybackState()
    }

    /// The simulated replay's playback end without a user exit.
    ///
    /// An arrival keeps the destination presentation: the trip stays
    /// presented with the destination centred and the indicator on the
    /// tracking dot, and only the drawer's exit control empties it
    /// (§7.1/§7.2: the arrival state replaces the drawer's contents and is not
    /// a snap back). Any other end (a recorded trace whose samples drained
    /// behind a reroute) restores the captured pre-trip camera as before.
    private func onSimulatedPlaybackEnded() {
        if !isTraceReplayActive {
            return
        }
        // The trip's speech lifecycle ended with the playback: the audio
        // session equivalent abandons once the engine drained (an in-flight
        // utterance keeps it until its completion).
        speechSpeaker?.releaseJourneySpeech()
        if traceDriver.hasArrived {
            // The arrival state stays visible: the destination remains the
            // camera centre (the trip does not snap back to the pre-trip
            // framing) and the indicator turns into the standing tracking dot.
            if let arrivedCoordinate = latestLocationFix?.snappedCoordinate {
                centerLatitudeDegrees = arrivedCoordinate.latitude
                centerLongitudeDegrees = arrivedCoordinate.longitude
                applyDesiredCamera()
            }
            if let arrivedFix = latestLocationFix {
                pushLocationIndicator(guided: false, fix: arrivedFix)
            }
            emitPlaybackState()
            return
        }
        restorePreTripCamera()
        if let arrivedFix = latestLocationFix {
            pushLocationIndicator(guided: false, fix: arrivedFix)
        }
        emitTraceUpdateFromListener()
        emitPlaybackState()
    }

    /// Whether the trace replay is currently driven (the drawer's state).
    private(set) var isTraceReplayActive = false

    private func emitTraceState() {
        emitEvent(
            "RefappTraceState",
            [
                "isActive": isTraceReplayActive,
                "traceName": activeLoadedTrace?.traceName ?? "",
            ]
        )
    }

    /// The drawer's playback state (§7.2): whether a presented trip is paused,
    /// its selected speed multiplier and whether it has arrived. Kept apart
    /// from the trace state so a pause or a pace change never drops the last
    /// guidance update (the trace state listener clears it).
    private func emitPlaybackState() {
        emitEvent(
            "RefappPlaybackState",
            [
                "isActive": isTraceReplayActive,
                "isPaused": traceDriver.isTripPaused,
                "speedMultiplier": traceDriver.speedMultiplier,
                "hasArrived": traceDriver.hasArrived,
                "isSeekable": traceDriver.isSeekable,
                "totalMeters": traceDriver.seekableDistanceMeters,
            ]
        )
    }

    // ------------------------------------------------------------------
    // The banner and drawer presentation (§7.1/§7.2)
    // ------------------------------------------------------------------

    /// The banner display text of one maneuver (§7.1), composed by the
    /// contract's shared rule: the composed variant first, then the provider's
    /// own text, the honest no-text line on the unavailable form, and the
    /// roundabout exit suffix.
    private func bannerDisplayText(
        _ instructionText: ManeuverInstructionText,
        _ roundaboutExitIndex: UInt8?
    ) -> String {
        instructionBannerText(
            composedDisplayText: instructionText.composedDisplayText,
            providerDisplayText: instructionText.providerDisplayText,
            roundaboutExitIndex: roundaboutExitIndex
        )
    }

    /// Formats the distance phrase of the active maneuver (§7.1 live prefix).
    private func formatDistancePhrase(_ phrase: DistancePhrase) -> String {
        switch phrase.phraseKind {
        case .immediate:
            return RefappConstants.distancePhraseImmediateText
        case .quantity:
            return String(
                format: "%.0f %@",
                phrase.magnitude ?? 0.0,
                distanceUnitSymbol(phrase.magnitudeUnit ?? .meters)
            )
        case .suppressed:
            return RefappConstants.distancePhraseImmediateText
        }
    }

    private func distanceUnitSymbol(_ distanceUnit: DistanceUnit) -> String {
        switch distanceUnit {
        case .meters: return "m"
        case .kilometers: return "km"
        case .feet: return "ft"
        case .miles: return "mi"
        }
    }

    private func phaseLabelOf(_ phase: NavigationPhase?) -> String {
        switch phase {
        case .arrived: return RefappConstants.navigationPhaseLabelArrived
        case .offRoute: return RefappConstants.navigationPhaseLabelOffRoute
        case .navigating: return RefappConstants.navigationPhaseLabelNavigating
        case nil: return ""
        }
    }

    private func emitTraceUpdateFromInstruction(_ instruction: NavigationInstruction) {
        var arguments: [String: Any] = [
            "phase": phaseLabelOf(latestPhase()),
            "isArrival": instruction.isArrival,
            "isReroutingRequired": instruction.isReroutingRequired,
            "bannerHeadlineText": formatDistancePhrase(instruction.activeManeuver.distancePhrase),
            "bannerDisplayText": bannerDisplayText(
                instruction.activeManeuver.instruction,
                instruction.activeManeuver.roundaboutExitIndex
            ),
        ]
        if let upcoming = instruction.upcomingManeuver {
            arguments["upcomingDisplayText"] = RefappConstants.thenLinePrefix
                + bannerDisplayText(upcoming.instruction, upcoming.roundaboutExitIndex)
        }
        if let progress = traceDriver.latestTripState?.progress {
            arguments["progressLineText"] = "Distance remaining "
                + formatDistanceMeters(progress.distanceRemainingMeters)
                + " · Duration remaining "
                + formatDurationSeconds(progress.durationRemainingSeconds)
        } else if let trace = activeLoadedTrace {
            arguments["progressLineText"] = "Distance remaining "
                + formatDistanceMeters(trace.route.distanceMeters)
                + " · Duration remaining "
                + formatDurationSeconds(trace.route.durationSeconds)
        }
        arguments["travelledMeters"] = traceDriver.travelledDistanceMeters
        emitEvent("RefappTraceUpdate", arguments)
    }

    private func latestPhase() -> NavigationPhase? {
        if let latestPhaseChange = navigationListener.latestPhaseChange {
            switch latestPhaseChange {
            case .arrived: return .arrived
            case .offRouteEntered: return .offRoute
            case .offRouteCleared: return .navigating
            default: break
            }
        }
        return traceDriver.latestTripState?.phase
    }

    private func emitTraceUpdateFromListener() {
        guard let instruction = navigationListener.latestInstruction else { return }
        emitTraceUpdateFromInstruction(instruction)
    }

    private func emitDeviationUpdate(_ deviationStatusChange: DeviationStatusChange) {
        if deviationStatusChange.statusChangeKind != .offRoute {
            return
        }
        let deviationDistanceMeters = deviationStatusChange.deviationInfo?.deviationFromRouteMeters ?? 0.0
        emitEvent(
            "RefappDeviationAlert",
            [
                "text": String(
                    format: RefappConstants.deviationTextTemplate,
                    formatDistanceMeters(deviationDistanceMeters)
                )
            ]
        )
    }

    private func emitPhaseUpdate(_ phaseChange: NavigationPhaseChange) {
        // The phase chip is the state form (§7.2): recomputed from the
        // controller's read-only state after every phase-topic update.
        let phase: NavigationPhase?
        switch phaseChange {
        case .arrived: phase = .arrived
        case .offRouteEntered: phase = .offRoute
        case .offRouteCleared: phase = .navigating
        default: phase = traceDriver.latestTripState?.phase
        }
        emitEvent("RefappPhaseChange", ["phase": phaseLabelOf(phase)])
    }

    // ------------------------------------------------------------------
    // Diagnostics (§4.1: the readiness chip's sheet; §6.4 honesty)
    // ------------------------------------------------------------------
    // Diagnostics (§4.1: the readiness chip's sheet; §6.4 honesty)
    // ------------------------------------------------------------------

    /// The one live diagnostics listener, present only while the pane is open.
    private var diagnosticsListener: RefappDiagnosticsListener?

    /// Receives the SDK's structured events and forwards them to the bridge.
    private final class RefappDiagnosticsListener: NSObject, SdkDiagnosticEventListener, @unchecked Sendable {
        private let onEvent: (LogEvent) -> Void

        init(onEvent: @escaping (LogEvent) -> Void) {
            self.onEvent = onEvent
        }

        func onDiagnosticEvent(event: LogEvent) {
            onEvent(event)
        }
    }

    /// The severity's lower-case contract name (the bridge's payload word).
    private static func severityName(_ severity: SdkLogSeverity) -> String {
        switch severity {
        case .trace: return "trace"
        case .debug: return "debug"
        case .info: return "info"
        case .warn: return "warn"
        case .error: return "error"
        }
    }

    /// Registers the diagnostics listener at the ordinal's severity and
    /// flushes the bounded backlog. Called only when the pane opens (and
    /// again when the explicit debug switch lowers the gate).
    @objc func startDiagnosticEvents(
        minimumSeverityOrdinal: Int,
        maximumBacklogCount: Int
    ) {
        guard let sdk = nativeSdk else { return }
        stopDiagnosticEvents()
        let minimumSeverity: SdkLogSeverity
        switch minimumSeverityOrdinal {
        case 0: minimumSeverity = .trace
        case 1: minimumSeverity = .debug
        case 3: minimumSeverity = .warn
        case 4: minimumSeverity = .error
        default: minimumSeverity = .info
        }
        let listener = RefappDiagnosticsListener { [weak self] event in
            // The SDK delivers events from its own threads; the React event
            // emitter is touched on the main queue only.
            let timestampText = String(event.timestampMillisecondsSinceEpoch)
            let severityName = Self.severityName(event.severity)
            let subsystemTarget = event.subsystemTarget
            let messageLine = event.messageLine
            // The typed provider request trace crosses as a nested payload; the
            // record is already redacted at emission, so no credential travels.
            // Only scalar fields are captured, keeping the main-queue closure
            // free of a non-Sendable record.
            let traceProviderKey = event.providerRequestTrace?.providerKey
            let traceProviderContract = event.providerRequestTrace?.providerContract
            let traceHttpMethod = event.providerRequestTrace?.httpMethod
            let traceRequestPath = event.providerRequestTrace?.requestPath
            let traceStatusClass = event.providerRequestTrace?.statusClass
            let traceOutcome = event.providerRequestTrace?.outcome
            let traceDurationMilliseconds = event.providerRequestTrace.map {
                String($0.durationMilliseconds)
            }
            let traceResponseBytes = event.providerRequestTrace.map {
                String($0.responseBytes)
            }
            let traceResponseExcerpt = event.providerRequestTrace?.responseExcerpt
            DispatchQueue.main.async {
                var payload: [String: Any] = [
                    "timestampMillisecondsSinceEpoch": timestampText,
                    "severity": severityName,
                    "subsystemTarget": subsystemTarget,
                    "messageLine": messageLine,
                ]
                if let traceProviderKey, let traceProviderContract,
                   let traceHttpMethod, let traceRequestPath,
                   let traceStatusClass, let traceOutcome,
                   let traceDurationMilliseconds, let traceResponseBytes {
                    var providerRequestTrace: [String: Any] = [
                        "providerKey": traceProviderKey,
                        "providerContract": traceProviderContract,
                        "httpMethod": traceHttpMethod,
                        "requestPath": traceRequestPath,
                        "statusClass": traceStatusClass,
                        "outcome": traceOutcome,
                        "durationMilliseconds": traceDurationMilliseconds,
                        "responseBytes": traceResponseBytes,
                    ]
                    providerRequestTrace["responseExcerpt"] =
                        traceResponseExcerpt ?? NSNull()
                    payload["providerRequestTrace"] = providerRequestTrace
                }
                self?.emitEvent("RefappDiagnosticEvent", payload)
            }
        }
        diagnosticsListener = listener
        sdk.subscribeToDiagnosticEvents(
            listener: listener,
            minimumSeverity: minimumSeverity
        )
        _ = sdk.flushRecentDiagnosticEvents(
            listener: listener,
            minimumSeverity: minimumSeverity,
            maximumCount: UInt32(max(0, maximumBacklogCount))
        )
    }

    /// Removes the listener; the SDK's already-lowered gate stays for the session.
    @objc func stopDiagnosticEvents() {
        diagnosticsListener = nil
    }

    /// The diagnostics lines the sheet renders (read-only queries, no state).
    func diagnosticsLines() -> [String] {
        let cameraReadback: String
        if let camera = packageHostView()?.camera() {
            cameraReadback = String(
                format: "Camera %.5f , %.5f · zoom %.2f · bearing %.0f° · pitch %.0f°",
                camera.latitudeDegrees,
                camera.longitudeDegrees,
                camera.zoomLevel,
                camera.bearingDegrees,
                camera.pitchDegrees
            )
        } else {
            cameraReadback = "Camera: not built yet"
        }
        let tileLine: String
        if let statistics = latestTileStatistics {
            tileLine = "Tiles: requested \(statistics.requestedTileCount) · drawn "
                + "\(statistics.presentTileCount) · fallback \(statistics.fallbackTileCount) · "
                + "deferred \(statistics.deferredTileCount) · missing \(statistics.unavailableTileCount)"
        } else {
            tileLine = "Tile accounting starts with the first frame…"
        }
        let planLine: String
        if let plannedTrace = plannedRouteTrace {
            planLine = "Plan: \(plannedRouteNameText ?? RefappConstants.plannedRouteFallbackName) · "
                + formatDistanceMeters(plannedTrace.route.distanceMeters) + " · "
                + formatDurationSeconds(plannedTrace.route.durationSeconds)
        } else {
            planLine = "No planned route (a search assignment plans one)"
        }
        return [
            "Renderer: GPU surface renderer over the platform Metal backend (the honest device fact)",
            cameraReadback,
            tileLine,
            "Frames: drawn \(framesDrawnCount) · deferred \(framesDeferredCount) · failed \(framesFailedCount)",
            "Overlays pushed: \(pushedOverlayCount)",
            "Cache: \(tileCacheStateText)",
            planLine,
        ]
    }

    // ------------------------------------------------------------------
    // ObjC-readability helpers (the bridge glue's payload shapes)
    // ------------------------------------------------------------------

    /// The speech engine's state as the bridge payload dictionary (§6.2).
    @objc func speechEngineStateDictionary() -> NSDictionary {
        let engineState = speechEngineState()
        var stateDictionary: [String: Any] = ["isEngineReady": engineState.isEngineReady]
        if let unavailableReasonText = engineState.unavailableReasonText {
            stateDictionary["unavailableReasonText"] = unavailableReasonText
        }
        return stateDictionary as NSDictionary
    }

    /// The diagnostics lines as the bridge payload array (the bridge converts
    /// native arrays only — the Kotlin module's same lesson).
    @objc func diagnosticsLinesToArray() -> NSArray {
        diagnosticsLines() as NSArray
    }

    // ------------------------------------------------------------------
    // The launch-argument demo automation (the iOS-idiomatic UI-test seam)
    // ------------------------------------------------------------------

    /// Schedules the launch-argument demo automation after a successful SDK
    /// start: the same launcher flags the Compose iOS reference application
    /// consumes, driving the same user-path methods (`simctl` cannot inject
    /// touches). Order: plan apply → scene toggles → (start delay) trip
    /// start → (exit delay) trip exit.
    private func scheduleLaunchArgumentAutomation() {
        let launcherArguments = ProcessInfo.processInfo.arguments
        let automationArguments = AutomationArguments.parse(launcherArguments)
        guard automationArguments.wantsAutomation else { return }
        log.info("launch-argument automation scheduled (plan '\(automationArguments.traceAssetName ?? "default", privacy: .public)')")
        // 1. The demo plan applies after the automation's plan delay.
        DispatchQueue.main.asyncAfter(deadline: .now() + RefappConstants.automationPlanApplyDelaySeconds) { [weak self] in
            self?.applyAutomatedPlan(automationArguments)
        }
    }

    /// Applies the automated plan (the trace's asset name, or the artifact's
    /// default) and then the scene toggles, then the trip start/exit chain.
    private func applyAutomatedPlan(_ automationArguments: AutomationArguments) {
        let traceAssetName = automationArguments.traceAssetName ?? defaultTraceAssetName
        let planResolved: (Any?) -> Void = { [weak self] _ in
            guard let self else { return }
            // 2. The scene toggles apply after the plan applied.
            DispatchQueue.main.asyncAfter(
                deadline: .now() + RefappConstants.automationScenesApplyDelaySeconds
            ) { [weak self] in
                self?.applyAutomatedScenes(automationArguments)
            }
        }
        let planRejected: (String, String) -> Void = { [weak self] code, detailText in
            self?.emitErrorReport(variantLabel: code, detailText: detailText)
        }
        if let traceAssetName = automationArguments.traceAssetName {
            selectTraceReplayPlan(traceAssetName, planResolved, planRejected)
        } else {
            // The default trace: the picker path against the artifact's name.
            selectTraceReplayPlan(defaultTraceAssetName, planResolved, planRejected)
        }
    }

    private func applyAutomatedScenes(_ automationArguments: AutomationArguments) {
        for sceneKey in automationArguments.sceneKeys ?? [] {
            setSceneOverlayToggle(sceneKey, true)
        }
        // 3. The trip starts after the automation's start delay, when named.
        if let startDelaySeconds = automationArguments.startDelaySeconds {
            DispatchQueue.main.asyncAfter(deadline: .now() + startDelaySeconds) { [weak self] in
                self?.startTraceReplay()
                // 4. The trip exits after the automation's exit delay, when
                // named.
                if let exitDelaySeconds = automationArguments.exitDelaySeconds {
                    DispatchQueue.main.asyncAfter(deadline: .now() + exitDelaySeconds) { [weak self] in
                        self?.exitTraceReplay()
                    }
                }
            }
        }
    }

    /// The launch-argument automation flags (the named constants).
    private struct AutomationArguments {
        var traceAssetName: String?
        var sceneKeys: [String]?
        var startDelaySeconds: Double?
        var exitDelaySeconds: Double?

        /// Parses the launcher arguments into the automation request.
        static func parse(_ launcherArguments: [String]) -> AutomationArguments {
            func valueForArgument(_ argumentName: String) -> String? {
                if let argumentIndex = launcherArguments.firstIndex(of: "-\(argumentName)"),
                   argumentIndex + 1 < launcherArguments.count {
                    return launcherArguments[argumentIndex + 1]
                }
                if let equalityArg = launcherArguments.first(where: { $0.hasPrefix("\(argumentName)=") }) {
                    return String(equalityArg.dropFirst(argumentName.count + 1))
                }
                return nil
            }
            let traceAssetName = valueForArgument(RefappConstants.automationPlanArgumentName)
            let sceneList = valueForArgument(RefappConstants.automationScenesArgumentName)
            let startSeconds = valueForArgument(RefappConstants.automationStartSecondsArgumentName)
                .flatMap(Double.init)
            let exitSeconds = valueForArgument(RefappConstants.automationExitSecondsArgumentName)
                .flatMap(Double.init)
            return AutomationArguments(
                traceAssetName: traceAssetName,
                sceneKeys: sceneList.map { $0.split(separator: ",").map(String.init) },
                startDelaySeconds: startSeconds,
                exitDelaySeconds: exitSeconds
            )
        }

        var wantsAutomation: Bool {
            traceAssetName != nil || sceneKeys != nil || startDelaySeconds != nil
                || exitDelaySeconds != nil
        }
    }
}

/// The median-smoothed course-over-ground (§5.3): the circular median of the
/// named constant window of recent fixes (the Compose application's
/// `CourseMedianSmoother`).
final class CourseMedianSmoother {
    private var recentCourseDegrees: [Double] = []
    private let windowFixCount: Int

    init(_ windowFixCount: Int) {
        self.windowFixCount = windowFixCount
    }

    /// Adds one fix's course-over-ground, dropping the window's oldest.
    func add(_ courseDegreeValue: Double) {
        recentCourseDegrees.append(courseDegreeValue)
        while recentCourseDegrees.count > windowFixCount {
            recentCourseDegrees.removeFirst()
        }
    }

    /// The circular median, or nil with no windowed sample.
    func median() -> Double? {
        if recentCourseDegrees.isEmpty {
            return nil
        }
        let windowedList = recentCourseDegrees
        return windowedList.min { firstCandidateDegrees, secondCandidateDegrees in
            let firstTotal = windowedList.reduce(0.0) {
                $0 + angularDistanceDegrees(firstCandidateDegrees, $1)
            }
            let secondTotal = windowedList.reduce(0.0) {
                $0 + angularDistanceDegrees(secondCandidateDegrees, $1)
            }
            return firstTotal < secondTotal
        }
    }

    func reset() {
        recentCourseDegrees.removeAll()
    }

    private func angularDistanceDegrees(_ firstDegrees: Double, _ secondDegrees: Double) -> Double {
        let absoluteDifference = abs(firstDegrees - secondDegrees)
            .truncatingRemainder(dividingBy: GeometryConstants.fullCircleDegrees)
        if absoluteDifference > GeometryConstants.fullCircleDegrees / 2.0 {
            return GeometryConstants.fullCircleDegrees - absoluteDifference
        }
        return absoluteDifference
    }
}