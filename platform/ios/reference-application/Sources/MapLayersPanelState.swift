//! The on-map layer panel's state (docs/ReferenceAppUX.md §4.2): the
//! base-map rows, the traffic layer's debounced capability calls, the
//! device-location status row and the route fields' "My location" chips.
//!
//! The type owns every decision the panel makes and reaches the rest of the
//! application only through injected seams, so its tests drive it with
//! hand-written schedulers and a traffic-flow provider registered on a real
//! `MapSdk` (never the network). The model supplies the production seams: the
//! SDK's `fetch_traffic_flow` capability, the shipped map view's camera
//! bounds, the base-map selection path and the existing remembered record.

import Combine
import Foundation
import MapNativeCoreUI
import RusticMapsSDK

// MARK: - Errors

/// A local failure of the panel's traffic seam (the SDK is not running yet).
enum MapLayersTrafficError: Swift.Error, LocalizedError {
    /// The session has no SDK to ask (the keyless start before a provider).
    case sdkUnavailable

    var errorDescription: String? {
        "The SDK is not running yet."
    }
}

// MARK: - Value types

/// The panel's device-location row states, exactly the panel table's wording.
enum DeviceLocationPanelStatus: String, Equatable, CaseIterable {
    /// No source is running.
    case off
    /// The source is running and no fix arrived yet.
    case waitingForFix
    /// A fix arrived; the row reads Active.
    case active
    /// The platform refused the permission.
    case permissionDenied
    /// Location services are unavailable on this device.
    case unavailable

    /// The panel row's presentation text.
    var displayText: String {
        switch self {
        case .off: return "Off"
        case .waitingForFix: return "Waiting for a fix"
        case .active: return "Active"
        case .permissionDenied: return "Permission denied"
        case .unavailable: return "Unavailable"
        }
    }

    /// Whether the source is updating (the row's Stop action is shown).
    var isSourceRunning: Bool {
        switch self {
        case .waitingForFix, .active: return true
        case .off, .permissionDenied, .unavailable: return false
        }
    }

    /// The reason the source cannot start, or nil while it can.
    var cannotStartReasonText: String? {
        switch self {
        case .permissionDenied, .unavailable: return displayText
        case .off, .waitingForFix, .active: return nil
        }
    }
}

/// The result of asking the application to start the device location source.
enum DeviceLocationStartResult: Equatable {
    /// Location updates began; the first fix is still pending.
    case started
    /// The platform refused the permission.
    case permissionDenied
    /// Location services are unavailable.
    case unavailable
}

/// One base-map row of the layers panel: a raster descriptor or one committed
/// vector style. The identifier is the specification's shared test handle.
struct MapLayerBaseRow: Identifiable, Equatable {
    /// Which selection the row performs.
    enum Selection: Equatable {
        /// A registry descriptor's raster source.
        case raster(providerKey: String)
        /// A registry descriptor's committed vector style.
        case vector(choiceKey: String)
    }

    /// The row's accessibility identifier (`map-layer-base-…` or
    /// `map-layer-style-…`).
    let identifier: String
    /// The consumer-facing row name.
    let displayName: String
    /// The selection the row performs.
    let selection: Selection
    /// Whether the row's base currently serves the map.
    let isServing: Bool
    /// The disabled-with-reason text, or nil while the row is usable.
    let disabledReasonText: String?

    /// One row per identifier.
    var id: String { identifier }

    /// The row's rendered button label.
    var buttonLabel: String {
        if let disabledReasonText {
            return "\(displayName) — \(disabledReasonText)"
        }
        return isServing ? "\(displayName) — Serving" : displayName
    }
}

/// The traffic switch's availability, resolved from the shared registry.
enum MapLayerTrafficAvailability: Equatable {
    /// A descriptor serves the capability with the credentials in force.
    case available(providerKey: String, attributionText: String)
    /// A declared descriptor still needs its credential.
    case needsCredential(reasonText: String)
    /// No descriptor declares the capability.
    case notConfigured(reasonText: String)

    /// Whether the switch can be turned on.
    var isAvailable: Bool {
        if case .available = self { return true }
        return false
    }

    /// The disabled reason the switch renders, or nil while it is available.
    var reasonText: String? {
        switch self {
        case .available: return nil
        case let .needsCredential(reasonText): return reasonText
        case let .notConfigured(reasonText): return reasonText
        }
    }

    /// The serving descriptor's attribution line while it is available.
    var attributionText: String? {
        if case let .available(_, attributionText) = self { return attributionText }
        return nil
    }
}

// MARK: - Debounce scheduling

/// One scheduled debounce action the panel can cancel.
@MainActor
protocol MapLayersScheduledAction: AnyObject {
    /// Cancels the action; it must not run afterwards.
    func cancel()
}

/// Schedules the traffic layer's debounced refresh. The production
/// implementation sleeps on the main actor; tests use a manual scheduler so
/// the debounce is asserted without waiting.
@MainActor
protocol MapLayersDebounceScheduling: AnyObject {
    /// Schedules `action` after `seconds`, replacing nothing on its own; the
    /// caller cancels the previous action.
    func schedule(
        afterSeconds seconds: Double,
        action: @escaping @MainActor () -> Void
    ) -> MapLayersScheduledAction
}

/// The production scheduler: a main-actor task that sleeps for the debounce
/// and runs the action unless it was cancelled.
@MainActor
final class TaskMapLayersDebounceScheduler: MapLayersDebounceScheduling {
    /// The number of nanoseconds in one second.
    private static let nanosecondsPerSecond: UInt64 = 1_000_000_000

    /// A nonisolated initializer keeps the type usable as a default
    /// argument; the scheduling itself stays main-actor isolated.
    nonisolated init() {}

    func schedule(
        afterSeconds seconds: Double,
        action: @escaping @MainActor () -> Void
    ) -> MapLayersScheduledAction {
        let task = Task { @MainActor in
            try? await Task.sleep(
                nanoseconds: UInt64(seconds * Double(Self.nanosecondsPerSecond))
            )
            guard !Task.isCancelled else { return }
            action()
        }
        return TaskMapLayersScheduledAction(task: task)
    }
}

/// The task-backed scheduled action.
@MainActor
private final class TaskMapLayersScheduledAction: MapLayersScheduledAction {
    private let task: Task<Void, Never>

    init(task: Task<Void, Never>) {
        self.task = task
    }

    func cancel() {
        task.cancel()
    }
}

// MARK: - The panel state

/// The on-map layer panel's state machine. One instance per application
/// session; the model owns it and renders it through `MapLayersPanelView`.
@MainActor
final class MapLayersPanelState: ObservableObject {
    // MARK: Named constants

    /// The capability name a registry descriptor declares to serve traffic
    /// flow (the shared registry's `served_contracts` spelling).
    static let trafficFlowCapabilityName = "traffic_flow"

    /// How long the camera must stay still before the traffic layer asks the
    /// capability for the settled bounds. Every camera change restarts it, so
    /// one settle yields one request.
    static let trafficRefreshDebounceSeconds: Double = 0.4

    /// The drawn width of one traffic-flow line, in screen pixels.
    static let trafficFlowOverlayWidthPixels: Double = 4

    /// The display format a device fix writes into a route field.
    static let deviceFixDisplayFormat = "%.5f, %.5f"

    /// The disabled reason a credentialed row renders.
    static let needsCredentialFormat = "Needs %@"

    /// The text the traffic switch renders when no descriptor serves it.
    static let notConfiguredText = "Not configured"

    /// The panel status line when the camera cannot report bounds.
    static let cameraBoundsUnavailableText = "The camera bounds are unavailable."

    /// The panel status line for a failed traffic refresh.
    static let trafficRefreshFailureFormat = "Traffic refresh failed: %@"

    /// The mark a chip carries once its field holds a device fix.
    static let myLocationChipFilledMark = "·"

    /// The chip's consumer label.
    static let myLocationChipText =
        ReferenceApplicationConfiguration.myLocationActionText

    /// The chip's accessibility label (the field name keeps two chips
    /// distinguishable).
    static func myLocationChipAccessibilityLabel(
        for field: SearchFieldAssignmentTarget
    ) -> String {
        "Use my location as the \(field.rawValue)"
    }

    // MARK: Published state

    /// The base-map rows, in the specification's order: raster descriptors
    /// first, then every committed vector style.
    @Published private(set) var baseRows: [MapLayerBaseRow] = []

    /// The traffic switch's availability.
    @Published private(set) var trafficAvailability: MapLayerTrafficAvailability =
        .notConfigured(reasonText: notConfiguredText)

    /// Whether the traffic layer is on.
    @Published private(set) var isTrafficEnabled = false

    /// Whether a traffic request is in flight (the debounce's one-request cap).
    @Published private(set) var isTrafficRefreshing = false

    /// The panel's status line (a failed refresh, a refusal or a location
    /// reason), or nil while nothing needs saying.
    @Published private(set) var statusLineText: String?

    /// The device location row's state.
    @Published private(set) var locationStatus: DeviceLocationPanelStatus = .off

    /// The latest device fix's display text, or nil before the first fix.
    @Published private(set) var latestFixDisplayText: String?

    /// The route fields a "My location" chip filled this session.
    @Published private(set) var filledFields: Set<SearchFieldAssignmentTarget> = []

    /// The route fields waiting for the next device fix.
    @Published private(set) var pendingChipFields: Set<SearchFieldAssignmentTarget> = []

    /// The number of traffic capability calls this session (the test handle
    /// for the one-request-in-flight rule).
    @Published private(set) var trafficFetchCount = 0

    /// The serving traffic-flow cache's counters and budget, read after each
    /// settled refresh: the diagnostics sheet shows them and a suite reads
    /// them. Nil while the layer is off or the provider answers uncached.
    @Published private(set) var trafficCacheStatistics: TrafficFlowCacheStatistics?

    /// The installed tile payload cache's exact counters, read when the
    /// developer console opens: both the raster base and the vector base fetch
    /// through the same store. Nil while no tile cache is installed.
    @Published private(set) var tileCacheStatistics: TileCacheStatistics?

    /// The number of traffic overlays installed this session.
    @Published private(set) var trafficOverlayInstallCount = 0

    // MARK: Seams

    /// The debounce scheduler (production: a main-actor sleep).
    private let debounceScheduler: MapLayersDebounceScheduling

    /// The remembered-choices record the traffic choice joins.
    private let rememberedChoicesStore: RememberedChoicesStore

    /// The capability call the layer performs.
    private let fetchTrafficFlow: (BoundingBox) async throws -> [TrafficFlowData]

    /// The settled camera's geographic bounds.
    private let cameraGeographicBounds: () -> BoundingBox?

    /// Installs the one traffic overlay from the capability's rows.
    private let installTrafficOverlay: ([TrafficFlowData]) -> Void

    /// Reads the serving traffic-flow cache's counters and budget.
    private let readTrafficCacheStatistics: () -> TrafficFlowCacheStatistics?

    /// Reads the installed tile payload cache's exact counters.
    private let readTileCacheStatistics: () -> TileCacheStatistics?

    /// Removes the traffic overlay.
    private let clearTrafficOverlay: () -> Void

    /// Starts the device location source and reports whether it began.
    private let startDeviceLocation: () -> DeviceLocationStartResult

    /// Writes one route field from a device fix.
    private let fillRouteField: (SearchFieldAssignmentTarget, String) -> Void

    /// Records the traffic choice in the remembered record.
    private let persistTrafficChoice: () -> Void

    // MARK: Debounce and request bookkeeping

    /// The pending debounce action, if any.
    private var debounceAction: MapLayersScheduledAction?

    /// The request currently in flight, if any.
    private var refreshTask: Task<Void, Never>?

    /// Whether a camera change arrived while a request was in flight.
    private var hasPendingRefresh = false

    /// The camera pose key of the last requested refresh, so an unchanged
    /// pose does not schedule another request.
    private var lastRequestedCameraPoseKey: String?

    /// Invalidates answers that belong to a layer state the user already
    /// left (a toggle-off, or an off/on pair while a request was in flight).
    private var trafficRequestGeneration = 0

    /// Creates the state over the production seams.
    init(
        debounceScheduler: MapLayersDebounceScheduling = TaskMapLayersDebounceScheduler(),
        rememberedChoicesStore: RememberedChoicesStore = .standard,
        fetchTrafficFlow: @escaping (BoundingBox) async throws -> [TrafficFlowData],
        cameraGeographicBounds: @escaping () -> BoundingBox?,
        installTrafficOverlay: @escaping ([TrafficFlowData]) -> Void,
        readTrafficCacheStatistics: @escaping () -> TrafficFlowCacheStatistics?,
        readTileCacheStatistics: @escaping () -> TileCacheStatistics?,
        clearTrafficOverlay: @escaping () -> Void,
        startDeviceLocation: @escaping () -> DeviceLocationStartResult,
        fillRouteField: @escaping (SearchFieldAssignmentTarget, String) -> Void,
        persistTrafficChoice: @escaping () -> Void
    ) {
        self.debounceScheduler = debounceScheduler
        self.rememberedChoicesStore = rememberedChoicesStore
        self.fetchTrafficFlow = fetchTrafficFlow
        self.cameraGeographicBounds = cameraGeographicBounds
        self.installTrafficOverlay = installTrafficOverlay
        self.readTrafficCacheStatistics = readTrafficCacheStatistics
        self.readTileCacheStatistics = readTileCacheStatistics
        self.clearTrafficOverlay = clearTrafficOverlay
        self.startDeviceLocation = startDeviceLocation
        self.fillRouteField = fillRouteField
        self.persistTrafficChoice = persistTrafficChoice
    }

    // MARK: Base map

    /// Rebuilds the base-map rows from the current choices and selection.
    /// Raster descriptors come first, then every committed vector style; a
    /// credentialed descriptor whose key is not applied (raster or vector) is
    /// disabled with `Needs <credential label>`.
    func refreshBaseRows(
        rasterChoices: [BaseMapChoice],
        vectorChoices: [VectorBaseMapChoice],
        activeRasterProviderKey: String?,
        activeVectorChoiceKey: String?
    ) {
        var rows: [MapLayerBaseRow] = rasterChoices.map { choice in
            MapLayerBaseRow(
                identifier: "map-layer-base-\(choice.providerKey)",
                displayName: choice.descriptor.displayName,
                selection: .raster(providerKey: choice.providerKey),
                isServing: activeVectorChoiceKey == nil
                    && choice.providerKey == activeRasterProviderKey,
                disabledReasonText: choice.isServable
                    ? nil
                    : Self.needsCredentialText(
                        credentialLabel: choice.descriptor.credentialLabel
                    )
            )
        }
        rows.append(contentsOf: vectorChoices.map { choice in
            MapLayerBaseRow(
                identifier: "map-layer-style-\(choice.style.styleId)",
                displayName: choice.displayName,
                selection: .vector(choiceKey: choice.choiceKey),
                isServing: choice.choiceKey == activeVectorChoiceKey,
                disabledReasonText: choice.isServable
                    ? nil
                    : Self.needsCredentialText(
                        credentialLabel: choice.descriptor.credentialLabel
                    )
            )
        })
        baseRows = rows
    }

    /// The disabled reason for a credentialed descriptor without its key.
    static func needsCredentialText(credentialLabel: String) -> String {
        String(format: needsCredentialFormat, credentialLabel)
    }

    // MARK: Traffic configuration

    /// Resolves the traffic switch from the shared registry: the first
    /// descriptor declaring the capability serves it; a credential it does
    /// not yet have, or no declaration at all, states the reason plainly.
    func updateTrafficConfiguration(
        providerDescriptors: [ProviderDescriptor],
        appliedProviderKeys: Set<String>
    ) {
        let descriptor = providerDescriptors.first { candidate in
            candidate.servedContracts.contains(Self.trafficFlowCapabilityName)
                && candidate.registrationAvailable.ios
        }
        guard let descriptor else {
            trafficAvailability = .notConfigured(reasonText: Self.notConfiguredText)
            turnTrafficOffIfEnabled(remember: false)
            return
        }
        let requiresCredential = !descriptor.credentialLabel.trimmingCharacters(
            in: .whitespacesAndNewlines
        ).isEmpty && descriptor.registrationAvailable.ios
        if requiresCredential
            && !appliedProviderKeys.contains(descriptor.providerKey)
        {
            trafficAvailability = .needsCredential(
                reasonText: Self.needsCredentialText(
                    credentialLabel: descriptor.credentialLabel
                )
            )
            turnTrafficOffIfEnabled(remember: false)
            return
        }
        trafficAvailability = .available(
            providerKey: descriptor.providerKey,
            attributionText: descriptor.attributionText
        )
    }

    /// Restores the remembered traffic choice once a descriptor that serves
    /// the capability is registered; a keyless session keeps the choice for a
    /// later launch instead of pretending the layer is on.
    func restoreRememberedTrafficIfAvailable() {
        guard !isTrafficEnabled,
              trafficAvailability.isAvailable,
              rememberedChoicesStore.load()?.trafficLayerEnabled == true
        else {
            return
        }
        setTrafficEnabled(true, remember: false)
    }

    /// Turns the layer on or off. A turn-on that the registry cannot serve
    /// states the reason instead of pretending; turning off clears the
    /// overlay and any scheduled refresh.
    func setTrafficEnabled(_ enabled: Bool, remember: Bool = true) {
        if enabled {
            guard trafficAvailability.isAvailable else {
                statusLineText = trafficAvailability.reasonText
                return
            }
        }
        isTrafficEnabled = enabled
        trafficRequestGeneration += 1
        if remember {
            persistTrafficChoice()
        }
        if enabled {
            scheduleTrafficRefresh()
        } else {
            cancelScheduledRefresh()
            clearTrafficOverlay()
            lastRequestedCameraPoseKey = nil
            statusLineText = nil
            trafficCacheStatistics = nil
        }
    }

    /// Reads the installed tile payload cache's exact counters for the
    /// developer console. Read-only: it never issues a provider call.
    func refreshTileCacheStatistics() {
        tileCacheStatistics = readTileCacheStatistics()
    }

    /// Notes the camera's settled pose: an unchanged pose schedules nothing,
    /// a changed one restarts the debounce. Called from every camera path.
    func noteCameraPoseChanged(poseKey: String) {
        guard isTrafficEnabled else { return }
        guard poseKey != lastRequestedCameraPoseKey else { return }
        lastRequestedCameraPoseKey = poseKey
        scheduleTrafficRefresh()
    }

    /// Schedules one refresh after the debounce; a newer schedule replaces
    /// the older action.
    private func scheduleTrafficRefresh() {
        guard isTrafficEnabled else { return }
        debounceAction?.cancel()
        debounceAction = debounceScheduler.schedule(
            afterSeconds: Self.trafficRefreshDebounceSeconds
        ) { [weak self] in
            self?.debounceAction = nil
            self?.beginTrafficRefresh()
        }
    }

    /// Cancels the pending debounce and drops a pending refresh.
    private func cancelScheduledRefresh() {
        debounceAction?.cancel()
        debounceAction = nil
        hasPendingRefresh = false
    }

    /// Starts one capability call unless another is in flight; a call that
    /// arrives while one is running is coalesced into one follow-up.
    private func beginTrafficRefresh() {
        guard isTrafficEnabled else { return }
        if isTrafficRefreshing {
            hasPendingRefresh = true
            return
        }
        guard let bounds = cameraGeographicBounds() else {
            statusLineText = Self.cameraBoundsUnavailableText
            return
        }
        isTrafficRefreshing = true
        trafficFetchCount += 1
        let requestGeneration = trafficRequestGeneration
        refreshTask = Task { @MainActor [weak self] in
            guard let self else { return }
            do {
                let trafficFlowSegments = try await self.fetchTrafficFlow(bounds)
                // A toggle-off (or a new toggle-on) while the request was in
                // flight drops its answer: the overlay it would install no
                // longer belongs to the layer state in force.
                if self.isTrafficEnabled,
                   requestGeneration == self.trafficRequestGeneration
                {
                    self.installTrafficOverlay(trafficFlowSegments)
                    self.trafficOverlayInstallCount += 1
                    self.statusLineText = nil
                    // The call settled, so the cache's counters now describe
                    // it: a repeat over the same ground reads as a hit.
                    self.trafficCacheStatistics = self.readTrafficCacheStatistics()
                }
            } catch {
                if requestGeneration == self.trafficRequestGeneration {
                    let failureText = Self.failureDetailText(error)
                    self.statusLineText = String(
                        format: Self.trafficRefreshFailureFormat,
                        failureText
                    )
                    if Self.isProviderNotConfigured(error) {
                        // The switch turns off with the capability's own
                        // reason; its overlay and pending work go with it.
                        self.setTrafficEnabled(false)
                        self.statusLineText = failureText
                    }
                }
            }
            self.isTrafficRefreshing = false
            self.refreshTask = nil
            if self.hasPendingRefresh {
                self.hasPendingRefresh = false
                self.scheduleTrafficRefresh()
            }
        }
    }

    /// Turns the layer off without recording the change (a configuration
    /// change, not a user choice).
    private func turnTrafficOffIfEnabled(remember: Bool) {
        guard isTrafficEnabled else { return }
        setTrafficEnabled(false, remember: remember)
    }

    /// Whether a thrown error is the contract's missing-provider variant.
    private static func isProviderNotConfigured(_ error: Error) -> Bool {
        if let sdkError = error as? SdkError,
           case .ProviderNotConfigured = sdkError
        {
            return true
        }
        // The generated error's description is its own variant name (its
        // `LocalizedError` implementation reflects the case). The fallback
        // keeps the classification correct when the binding module is linked
        // into both the application and its test host, whose separately
        // linked enum copies carry distinct type metadata.
        return String(reflecting: error).contains("ProviderNotConfigured")
    }

    /// The verbatim detail of a thrown error, never paraphrased.
    private static func failureDetailText(_ error: Error) -> String {
        if let localizedError = error as? LocalizedError,
           let errorDescription = localizedError.errorDescription
        {
            return errorDescription
        }
        if let sdkError = error as? SdkError {
            return String(describing: sdkError)
        }
        return String(describing: error)
    }

    // MARK: Device location

    /// Updates the row after the source reports a state change.
    func setLocationStatus(_ status: DeviceLocationPanelStatus) {
        locationStatus = status
        switch status {
        case .permissionDenied, .unavailable:
            statusLineText = status.displayText
        case .off:
            pendingChipFields.removeAll()
            statusLineText = nil
        case .waitingForFix, .active:
            break
        }
    }

    /// Records one accepted device fix: the row reads Active, the chip fill
    /// targets receive the fix's display text, and the waiting set clears.
    func noteDeviceFix(coordinate: GeographicCoordinate) {
        let displayText = String(
            format: Self.deviceFixDisplayFormat,
            coordinate.latitude,
            coordinate.longitude
        )
        latestFixDisplayText = displayText
        locationStatus = .active
        for field in pendingChipFields {
            fillRouteField(field, displayText)
            filledFields.insert(field)
        }
        pendingChipFields.removeAll()
    }

    /// Clears the row when the source stops.
    func noteDeviceLocationStopped() {
        setLocationStatus(.off)
    }

    /// The serving descriptor's attribution line while the layer is on: the
    /// map's attribution notice appends it for as long as the layer draws.
    var trafficAttributionText: String? {
        guard isTrafficEnabled else { return nil }
        return trafficAvailability.attributionText
    }

    /// The reason a "My location" chip is disabled, or nil while it works.
    var myLocationChipDisabledReason: String? {
        locationStatus.cannotStartReasonText
    }

    /// One chip's tap: starts the source when it is off, then fills that
    /// field alone from the latest fix (waiting for the next fix while none
    /// arrived yet).
    func myLocationChipTapped(for field: SearchFieldAssignmentTarget) {
        if let disabledReason = myLocationChipDisabledReason {
            statusLineText = disabledReason
            return
        }
        if !locationStatus.isSourceRunning {
            let startResult = startDeviceLocation()
            switch startResult {
            case .started:
                locationStatus = .waitingForFix
            case .permissionDenied:
                locationStatus = .permissionDenied
                statusLineText = DeviceLocationPanelStatus.permissionDenied.displayText
                return
            case .unavailable:
                locationStatus = .unavailable
                statusLineText = DeviceLocationPanelStatus.unavailable.displayText
                return
            }
        }
        pendingChipFields.insert(field)
        if let latestFixDisplayText {
            fillRouteField(field, latestFixDisplayText)
            filledFields.insert(field)
            pendingChipFields.remove(field)
        }
    }

    /// Whether the field currently holds a fix the chip filled.
    func hasMyLocationFill(for field: SearchFieldAssignmentTarget) -> Bool {
        filledFields.contains(field)
    }

    /// One chip's rendered label.
    func myLocationChipLabel(for field: SearchFieldAssignmentTarget) -> String {
        hasMyLocationFill(for: field)
            ? "\(Self.myLocationChipText) \(Self.myLocationChipFilledMark)"
            : Self.myLocationChipText
    }

    /// Drops a field's fill mark when the user edits it.
    func clearMyLocationFill(for field: SearchFieldAssignmentTarget) {
        filledFields.remove(field)
        pendingChipFields.remove(field)
    }

    /// Exchanges the two fields' fill marks with the swap action, so the
    /// chip state follows the text it filled.
    func swapMyLocationFills() {
        let originWasFilled = filledFields.contains(.origin)
        let destinationWasFilled = filledFields.contains(.destination)
        if originWasFilled {
            filledFields.insert(.destination)
        } else {
            filledFields.remove(.destination)
        }
        if destinationWasFilled {
            filledFields.insert(.origin)
        } else {
            filledFields.remove(.origin)
        }
        pendingChipFields.removeAll()
    }
}
