//! The Directions choice (PRD.md §6 item 36): the transport
//! mode and route strategy every plan carries. Car and Fastest are the
//! defaults; a serving provider's `routeCapabilities()` report disables a
//! choice it cannot honour, and both choices ride the versioned remembered
//! record. The state is exercised by hand-written capability reporters in the
//! unit tests; the application supplies the live `MapSdk`.

import Combine
import Foundation
import RusticMapsSDK

// MARK: - The two consumer choices

/// The transport modes the Directions control offers. The contract can
/// express more; this slice offers car and walk.
enum DirectionsMode: String, CaseIterable, Identifiable {
    /// Car routing (`TransportationMode.Automobile`).
    case car
    /// Walking routing (`TransportationMode.Pedestrian`).
    case walk

    /// One entry per mode.
    var id: String { rawValue }

    /// The value the contract's `RouteOptions` carries.
    var transportationMode: TransportationMode {
        switch self {
        case .car: return .automobile
        case .walk: return .pedestrian
        }
    }

    /// The consumer-facing label.
    var displayName: String {
        switch self {
        case .car: return "Car"
        case .walk: return "Walk"
        }
    }

    /// The value the remembered record stores (the contract's own spelling,
    /// shared with the other shells).
    var recordValue: String {
        switch self {
        case .car: return "automobile"
        case .walk: return "pedestrian"
        }
    }

    /// Decodes a remembered value; an absent or unknown value is nil, so the
    /// caller applies the default.
    init?(recordValue: String?) {
        switch recordValue {
        case DirectionsMode.car.recordValue: self = .car
        case DirectionsMode.walk.recordValue: self = .walk
        default: return nil
        }
    }

    /// The diagnostics spelling of a contract mode.
    static func diagnosticsName(
        for transportationMode: TransportationMode
    ) -> String {
        switch transportationMode {
        case .automobile: return "automobile"
        case .bicycle: return "bicycle"
        case .pedestrian: return "pedestrian"
        }
    }
}

/// The route strategies the Directions control offers.
enum DirectionsStrategy: String, CaseIterable, Identifiable {
    /// Minimize expected travel time (`RouteStrategy.Fastest`).
    case fastest
    /// Minimize traveled distance (`RouteStrategy.Shortest`).
    case shortest

    /// One entry per strategy.
    var id: String { rawValue }

    /// The value the contract's `RouteOptions` carries.
    var routeStrategy: RouteStrategy {
        switch self {
        case .fastest: return .fastest
        case .shortest: return .shortest
        }
    }

    /// The consumer-facing label.
    var displayName: String {
        switch self {
        case .fastest: return "Fastest"
        case .shortest: return "Shortest"
        }
    }

    /// The value the remembered record stores (the contract's own spelling).
    var recordValue: String { rawValue }

    /// Decodes a remembered value; an absent or unknown value is nil.
    init?(recordValue: String?) {
        switch recordValue {
        case DirectionsStrategy.fastest.recordValue: self = .fastest
        case DirectionsStrategy.shortest.recordValue: self = .shortest
        default: return nil
        }
    }

    /// The diagnostics spelling of a contract strategy.
    static func diagnosticsName(for routeStrategy: RouteStrategy) -> String {
        switch routeStrategy {
        case .fastest: return "fastest"
        case .shortest: return "shortest"
        }
    }
}

// MARK: - The capability seam

/// The SDK's route-capability report. It exists so the Directions state can
/// be exercised with a hand-written fake (the generated `MapSdk` class cannot
/// be instantiated against recorded answers).
protocol RouteCapabilityReporting: AnyObject {
    /// Reports the transport modes and route strategy the serving routing
    /// provider can honour.
    func routeCapabilities() throws -> RouteCapabilities
}

/// The shipped SDK satisfies the Directions capability seam directly.
extension MapSdk: RouteCapabilityReporting {}

// MARK: - The state

/// The Directions control's state: the selected mode and strategy, the
/// serving provider's capability report and its disabled reasons.
@MainActor
final class DirectionsChoiceState: ObservableObject {
    /// The selected transport mode (Car by default).
    @Published private(set) var mode: DirectionsMode

    /// The selected route strategy (Fastest by default).
    @Published private(set) var strategy: DirectionsStrategy

    /// The transport modes the serving provider reports; before a report (or
    /// without a routing provider) both offered modes stay selectable.
    @Published private(set) var supportedTransportationModes: Set<TransportationMode>

    /// Whether the serving provider reports shortest-route support.
    @Published private(set) var supportsShortestRouteStrategy: Bool

    /// The capability read's honest reason while it throws (for example
    /// `ProviderNotConfigured` in a keyless session).
    @Published private(set) var capabilityReasonText: String?

    /// Whether a successful capability report is in force.
    @Published private(set) var hasCapabilityReport = false

    /// The serving SDK, when the application attached one.
    private weak var capabilityReporter: RouteCapabilityReporting?

    /// The remembered-choices record both choices join.
    private let rememberedChoicesStore: RememberedChoicesStore

    /// The failure text the capability read reports verbatim.
    private let failureText: (Swift.Error) -> String

    /// Creates the state over the remembered record (both choices restore
    /// from it) and the SDK's failure wording.
    init(
        rememberedChoicesStore: RememberedChoicesStore = .standard,
        failureText: @escaping (Swift.Error) -> String =
            CapabilityProviderState.defaultFailureText
    ) {
        let remembered = rememberedChoicesStore.load()
        self.mode = remembered?.directionsMode ?? .car
        self.strategy = remembered?.directionsStrategy ?? .fastest
        self.rememberedChoicesStore = rememberedChoicesStore
        self.failureText = failureText
        self.supportedTransportationModes = Set(
            DirectionsMode.allCases.map(\.transportationMode)
        )
        self.supportsShortestRouteStrategy = true
    }

    /// Binds the state to the running SDK (nil in a keyless session before
    /// one exists).
    func attach(reporter: RouteCapabilityReporting?) {
        capabilityReporter = reporter
    }

    /// Reads the serving provider's report and disables what it cannot
    /// honour. A missing provider is stated rather than guessed: both
    /// choices stay available for the recorded demo route, and the reason is
    /// shown under the control.
    func refreshCapabilities() {
        guard let capabilityReporter else {
            return
        }
        do {
            let capabilities = try capabilityReporter.routeCapabilities()
            hasCapabilityReport = true
            supportedTransportationModes = Set(
                capabilities.supportedTransportationModes
            )
            supportsShortestRouteStrategy =
                capabilities.supportsShortestRouteStrategy
            capabilityReasonText = nil
            enforceSupportedSelection()
        } catch {
            // No routing provider is a neutral keyless state, not a refusal
            // of the choice itself: the recorded demo route still plans.
            hasCapabilityReport = false
            supportedTransportationModes = Set(
                DirectionsMode.allCases.map(\.transportationMode)
            )
            supportsShortestRouteStrategy = true
            capabilityReasonText = failureText(error)
        }
    }

    /// Whether the strategy row shows: strategy applies to automobile plans.
    var isStrategyRowVisible: Bool { mode == .car }

    /// The disabled reason for one mode, or nil while it is offered.
    func disabledReason(for offeredMode: DirectionsMode) -> String? {
        guard hasCapabilityReport,
              !supportedTransportationModes.contains(
                  offeredMode.transportationMode
              )
        else {
            return nil
        }
        return ReferenceApplicationConfiguration.directionsModeUnavailableReason(
            modeLabel: offeredMode.displayName
        )
    }

    /// The disabled reason for the Shortest choice, or nil while it is
    /// offered for the selected car plan.
    var shortestDisabledReason: String? {
        guard mode == .car, hasCapabilityReport,
              !supportsShortestRouteStrategy
        else {
            return nil
        }
        return ReferenceApplicationConfiguration
            .directionsShortestUnavailableReason
    }

    /// Applies one mode choice; a disabled mode is a no-op and a new choice
    /// joins the remembered record.
    func selectMode(_ nextMode: DirectionsMode) {
        guard disabledReason(for: nextMode) == nil, mode != nextMode else {
            return
        }
        mode = nextMode
        persistSelections()
    }

    /// Applies one strategy choice; a disabled strategy is a no-op.
    func selectStrategy(_ nextStrategy: DirectionsStrategy) {
        if nextStrategy == .shortest, shortestDisabledReason != nil {
            return
        }
        guard strategy != nextStrategy else { return }
        strategy = nextStrategy
        persistSelections()
    }

    /// The `RouteOptions` every plan carries: the selected mode and strategy
    /// with the caller's unit and avoidance choices.
    func routeOptions(
        unitSystem: UnitSystem,
        avoidOptions: AvoidOptions,
        departureTimestampSeconds: UInt64? = nil
    ) -> RouteOptions {
        RouteOptions(
            transportationMode: mode.transportationMode,
            routeStrategy: strategy.routeStrategy,
            unitSystem: unitSystem,
            avoidOptions: avoidOptions,
            departureTimestampSeconds: departureTimestampSeconds
        )
    }

    /// Restores the defaults without rewriting the record (the record reset
    /// act owns the clear).
    func resetToDefaults() {
        mode = .car
        strategy = .fastest
    }

    // MARK: Private acts

    /// Falls back to a choice the provider reports when the remembered mode
    /// or strategy is no longer offered, and records the correction.
    private func enforceSupportedSelection() {
        var didChangeSelection = false
        if !supportedTransportationModes.contains(mode.transportationMode) {
            if supportedTransportationModes.contains(.automobile) {
                mode = .car
                didChangeSelection = true
            } else if supportedTransportationModes.contains(.pedestrian) {
                mode = .walk
                didChangeSelection = true
            }
        }
        if mode == .car, !supportsShortestRouteStrategy, strategy == .shortest {
            strategy = .fastest
            didChangeSelection = true
        }
        if didChangeSelection {
            persistSelections()
        }
    }

    /// Writes both choices into the existing remembered record, preserving
    /// every other remembered choice.
    private func persistSelections() {
        var choices = rememberedChoicesStore.load() ?? RememberedChoices()
        choices.directionsMode = mode
        choices.directionsStrategy = strategy
        rememberedChoicesStore.store(choices)
    }
}
