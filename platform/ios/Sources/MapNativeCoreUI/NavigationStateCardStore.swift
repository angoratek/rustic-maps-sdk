//! The main-actor glue between the contract's `NavigationListener`
//! callbacks and SwiftUI (the reference applications' "listener dispatch to
//! the main actor"). The SDK dispatches on its own broadcaster thread and owns
//! the re-entrancy rules; the listeners below do nothing but hop to the main
//! actor and update the published card model the turn-by-turn UI renders from.
//! The read-only controller queries seed an initial state; the listener topics
//! are the live data source (§7.2).

import Combine
import Foundation
import RusticMapsSDK

/// The published turn-by-turn card model (design document §7.1): the live
/// payload values the card set renders, updated only by the listener topics
/// (plus the seeding call from the read-only queries).
@MainActor
public final class NavigationStateCardStore: ObservableObject {
    /// The latest composed instruction payload (the instructions topic);
    /// nil before the first dispatched payload arrives.
    @Published public private(set) var instruction: NavigationInstruction?

    /// The latest trip state snapshot, seeded from the controller's
    /// read-only query and then updated only through the phase topic's
    /// accompanying state? The contract's phase topic carries the phase
    /// transition alone, so the progress card renders the last seeded or
    /// queried `TripNavigationState` and is refreshed from the controller
    /// query after each phase change.
    @Published public private(set) var tripState: TripNavigationState?

    /// The latest deviation-status update (the deviation topic): off-route
    /// while a deviation persists (re-broadcast per fix), on-route when it
    /// clears.
    @Published public private(set) var deviationStatusChange: DeviationStatusChange?

    /// The latest phase transition (the phase topic): entered/cleared/
    /// reroute/arrived narratives.
    @Published public private(set) var phaseChange: NavigationPhaseChange?

    public init() {}

    /// Seeds the store from the controller's read-only queries (§7.2:
    /// "use the read-only queries only to seed an initial state").
    public func seed(
        fromInstruction instruction: NavigationInstruction?,
        tripState: TripNavigationState?
    ) {
        self.instruction = instruction
        self.tripState = tripState
    }

    /// Removes every card value: the stopped-journey state between trips.
    public func reset() {
        instruction = nil
        tripState = nil
        deviationStatusChange = nil
        phaseChange = nil
    }

    /// Records one instruction payload; called only through the glue.
    func record(instruction: NavigationInstruction) {
        self.instruction = instruction
    }

    /// Records one deviation-status update; called only through the glue.
    func record(deviationStatusChange: DeviationStatusChange) {
        self.deviationStatusChange = deviationStatusChange
    }

    /// Records one phase transition and refreshes the progress snapshot
    /// through the read-only query hook (the phase topic carries the
    /// transition alone; the progress card renders the controller's state).
    func record(
        phaseChange: NavigationPhaseChange,
        refreshedTripState: TripNavigationState?
    ) {
        self.phaseChange = phaseChange
        if let refreshedTripState {
            tripState = refreshedTripState
        }
    }
}

/// The main-actor hop of the contract's listener: implements the foreign
/// callback protocol verbatim and forwards each payload onto the main
/// actor's card store. The listener never re-enters the controller except
/// for the read-only queries the phase transition refreshes through
/// (documented above) — the re-entrancy guard is the SDK broadcaster's
/// single-in-flight rule either way.
public final class MainActorNavigationStateListener: NavigationListener {
    /// The card store updated on the main actor. The contract's listener
    /// protocol requires `Sendable`; this weak back-reference is written once
    /// during construction and only read on the main actor, so it is marked
    /// `nonisolated(unsafe)` rather than pretending the weak box is immutable.
    nonisolated(unsafe) private weak var cardStore: NavigationStateCardStore?

    /// The read-only seed hook the phase topic's refresh goes through, so
    /// the progress card follows the phase change it rides with. Runs on
    /// the main actor.
    private let tripStateRefresh: @MainActor @Sendable () -> TripNavigationState?

    /// The speech-trigger hook the speech-instruction topic rides through
    /// (the SDK owns *when* a line is newsworthy; the platform owns
    /// playback). Runs on the main actor; nil for consumers that never
    /// subscribe the speech topic.
    private let speechTriggerHandler: (@MainActor @Sendable (SpeechInstruction) -> Void)?

    /// Builds the listener over the store it forwards onto.
    public init(
        cardStore: NavigationStateCardStore,
        tripStateRefresh: @escaping @MainActor @Sendable () -> TripNavigationState?,
        speechTriggerHandler: (@MainActor @Sendable (SpeechInstruction) -> Void)? = nil
    ) {
        self.cardStore = cardStore
        self.tripStateRefresh = tripStateRefresh
        self.speechTriggerHandler = speechTriggerHandler
    }

    /// The speech-instruction topic: each delivery is the verbatim
    /// composed speech text of one utterance trigger (a crossed distance
    /// band, the immediate band, an off-route recovery, or the arrival).
    /// The caller speaks it verbatim; the trigger *when* is the SDK's.
    public func onSpeechInstruction(speechInstruction: SpeechInstruction) {
        let forwardedSpeechInstruction = speechInstruction
        Task { @MainActor [weak self] in
            self?.speechTriggerHandler?(forwardedSpeechInstruction)
        }
    }

    public func onDeviationStatusChange(deviationStatusChange: DeviationStatusChange) {
        let forwardedDeviationStatusChange = deviationStatusChange
        Task { @MainActor [weak self] in
            self?.cardStore?.record(deviationStatusChange: forwardedDeviationStatusChange)
        }
    }

    public func onNavigationInstruction(instruction: NavigationInstruction) {
        let forwardedInstruction = instruction
        Task { @MainActor [weak self] in
            self?.cardStore?.record(instruction: forwardedInstruction)
        }
    }

    public func onNavigationPhaseChange(navigationPhaseChange: NavigationPhaseChange) {
        let forwardedPhaseChange = navigationPhaseChange
        Task { @MainActor [weak self] in
            self?.cardStore?.record(
                phaseChange: forwardedPhaseChange,
                refreshedTripState: self?.tripStateRefresh()
            )
        }
    }
}