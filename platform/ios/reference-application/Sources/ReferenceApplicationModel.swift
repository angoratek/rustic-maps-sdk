//! The reference application model: the readiness ladder
//! (docs/ReferenceAppUX.md §6.1) and its readiness chip, the
//! provider-echoing error report (§6.3), the v2 sheet-and-drawer state
//! machine (§1: the home sheet's peek/half/full, the navigation drawer),
//! the plan flow (§4.1: search → route → confirm) over the async provider
//! surface, the offline-tile-cache registration and its live per-draw
//! statistics, the camera state ( gestures, presets, the SDK fit-bounds
//! path with the attribute-getter readback), the overlay scene
//! toggles, and the simulated turn-by-turn journeys driven by the replay
//! plans (the recorded trace fixture, §7; a planned route's own shape
//! walk) with the platform voice speaking the contract's speech channel
//! verbatim. Every fallible SDK path ends in the error report or a
//! readiness transition — no catch block here ends silently (§6.3).

import Combine
import CoreLocation
import Foundation
import RusticMapsSDK
import MapNativeCoreUI

/// The readiness ladder's states, exactly the §6.1 table plus the neutral
/// keyless state of §6.5: with no provider credential the application has
/// not failed — it waits for a credential instead.
enum ReadinessState: String {
    /// SDK module/provider registration still resolving.
    case loading
    /// The GPU surface renderer is active.
    case ready
    /// A documented capability substitute fired (none exists on this
    /// native path today; the state exists per the ladder).
    case degraded
    /// No provider credential is configured; the settings surface waits for
    /// one (§6.5: a first-class state, never a renderer failure).
    case keyless
    /// The SDK failed to start; the report names the failure verbatim.
    case failed
}

/// The applied status of one provider-credential row (§6.5): what the
/// settings surface says after Apply and Clear.
enum ProviderCredentialStatus: Equatable {
    /// No credential has been applied to this running session.
    case notApplied
    /// The provider registration succeeded with the entered credential.
    case applied
    /// Applying the credential failed; the detail is the SDK's or the
    /// Keychain's own text, shown verbatim.
    case failed(detailText: String)
    /// The credential row's prompt after a base-map row asked for this
    /// provider's key: the reason is a next step, never a dead control.
    case needsCredential(detailText: String)
    /// The credential was cleared; the SDK registry has no unregister
    /// operation, so the running session keeps it until a restart.
    case clearedRestartRequired
}

/// One reported failure in the error report (§6.3): the provider-echoing
/// label line (the `SdkError` variant name when the failure crossed the
/// contract, the failure origin otherwise) and the verbatim detail.
struct ReferenceApplicationErrorReport: Equatable {
    /// The label line (variant name, or severity + origin).
    let labelLine: String
    /// The failure detail echoed verbatim — never paraphrased, truncated or
    /// swallowed.
    let detailText: String
}

/// The camera values the gestures and presets own; the shipped map view
/// adopts them and reports the SDK's normalized readback (a fit, preset or
/// gesture lifts the previous framing).
struct ReferenceCameraValues: Equatable {
    var centerLatitudeDegrees: Double
    var centerLongitudeDegrees: Double
    var zoomLevel: Double
    var bearingDegrees: Double
    var pitchDegrees: Double
}

/// What the diagnostics sheet renders back of the last drawn frame's
/// camera — read through the contract camera's own attribute getters
/// (center, zoom, bearing and pitch all read back exactly).
struct CameraAttributeReadback: Equatable {
    let centerLatitudeDegrees: Double
    let centerLongitudeDegrees: Double
    let zoomLevel: Double
    let bearingDegrees: Double
    let pitchDegrees: Double
}

/// The offline card's cache registration state (the path and budget the
/// registration ran with).
struct OfflineTileCacheState: Equatable {
    /// The registered cache database's path (honest, full).
    let cacheDatabasePath: String
    /// The registered absolute byte budget, formatted.
    let budgetPhrase: String
}

/// Which search field a tapped provider result assigns to (§4.1: the
/// origin field and the destination field share one result list).
enum SearchFieldAssignmentTarget: String {
    /// The result becomes the route's destination ("Where to?").
    case destination
    /// The result becomes the route's origin.
    case origin
}

/// The application model (one per session).
@MainActor
final class ReferenceApplicationModel: ObservableObject {
    // MARK: Presentation state

    /// The theme choice in force (§3: Light, Dark or Auto; Auto on a device
    /// that never chose). An explicit choice is remembered on the device, and
    /// Auto follows the operating system's scheme live.
    @Published var themeChoice: ThemeChoice =
        RememberedChoicesStore.standard.load()?.theme ?? ThemeChoice.storedChoice()
    {
        didSet {
            themeChoice.store()
            persistRememberedChoices()
        }
    }

    /// The device-local store of the explicit choices besides the theme.
    private let rememberedChoicesStore: RememberedChoicesStore

    /// The debounce of a gesture-driven choice persistence: a drag changes the
    /// camera on every frame, and each of those changes is not a separate
    /// user choice. The record is written once the gesture has been still for
    /// ``ReferenceApplicationConfiguration/cameraGesturePersistDebounceSeconds``.
    private var scheduledChoicePersistTask: Task<Void, Never>?

    /// Creates the model over one remembered-choice store.
    init(rememberedChoicesStore: RememberedChoicesStore = .standard) {
        self.rememberedChoicesStore = rememberedChoicesStore
    }

    /// Schedules one choice-record write after the camera gesture settles,
    /// replacing any still-pending write from an earlier frame.
    func schedulePersistRememberedChoicesAfterCameraGesture() {
        scheduledChoicePersistTask?.cancel()
        scheduledChoicePersistTask = Task { @MainActor [weak self] in
            try? await Task.sleep(
                for: .seconds(
                    ReferenceApplicationConfiguration.cameraGesturePersistDebounceSeconds
                )
            )
            guard !Task.isCancelled else { return }
            self?.persistRememberedChoices()
        }
    }

    /// Writes a pending gesture-settled choice record immediately, cancelling
    /// the scheduled write. The application calls this when the scene stops
    /// being active, so the last gesture before backgrounding is not lost.
    func flushPendingChoicePersist() {
        guard scheduledChoicePersistTask != nil else { return }
        scheduledChoicePersistTask?.cancel()
        scheduledChoicePersistTask = nil
        persistRememberedChoices()
    }

    /// True while startup values are being adopted, so they are not rewritten.
    private var isRestoringRememberedChoices = false

    /// True while the startup opens its default base, which is not a user choice.
    private var isApplyingAutomaticBaseMap = false

    /// The base map the user explicitly chose (or the record restored), if any.
    private var rememberedBaseMapProviderKey: String?
    private var rememberedBaseMapStyleId: String?

    /// The demo plan the user explicitly chose (the startup default is not one).
    private var rememberedDemoPlanScenarioName: String?

    /// The home sheet's fold state (§1's v2 archetype: peek / half /
    /// full; the navigation drawer replaces it in place during a trip).
    @Published var homeSheetState: ReferenceAppUXSheetState = .peek

    /// Whether the diagnostics sheet is open (§4.1: the readiness chip
    /// opens it). The sheet's diagnostic feed follows this flag: opening
    /// arms the lazy subscription and the merge cadence, closing stops both.
    @Published var isDiagnosticsSheetOpen = false {
        didSet {
            guard isDiagnosticsSheetOpen != oldValue else { return }
            if isDiagnosticsSheetOpen {
                startDiagnosticsEventsFeed()
            } else {
                stopDiagnosticsEventsFeed()
            }
        }
    }

    /// The readiness ladder state and its live text (the full text lives
    /// in the diagnostics sheet; the chip carries the short line).
    @Published private(set) var readinessState: ReadinessState = .loading
    @Published private(set) var readinessText: String =
        ReferenceApplicationConfiguration.startingReadinessText

    /// The provider-echoing error report, visible until the next
    /// successful action clears it (§6.3).
    @Published private(set) var errorReport: ReferenceApplicationErrorReport?

    // MARK: Provider credentials (§6.5)

    /// Whether the settings surface is open (the gear control beside the
    /// readiness chip opens it).
    @Published var isProviderSettingsSheetOpen = false

    /// The shared provider registry's descriptors: one settings row each,
    /// never a hard-coded provider field.
    @Published private(set) var providerCredentialDescriptors: [ProviderDescriptor] = []

    /// The per-capability provider section's state: the SDK's registration
    /// readback, the registry options each capability offers, and the choices
    /// the device remembers. The settings view observes it directly, so a
    /// selection repaints its row without republishing the whole model.
    let capabilityProviderState = CapabilityProviderState()

    /// Every descriptor of the shared provider registry, in artifact order
    /// (the base-map choice list reads it; a keyless descriptor carries no
    /// credential row of its own).
    @Published private(set) var providerDescriptors: [ProviderDescriptor] = []

    /// The registry's base-map `User-Agent` template (its `%s` carries the
    /// compiled SDK version); blank when the artifact declares none, in
    /// which case the SDK sends no header.
    private(set) var referenceAppUserAgentTemplate = ""

    /// The registry's declared keyless styled base, or nil when the artifact
    /// declares none (the keyless start then keeps the credential-less raster
    /// source).
    private(set) var keylessBaseMap: KeylessBaseMapDescriptor?

    /// The keyless styled base still awaiting the shipped map view: the
    /// startup registers the credential-less raster source first (the surface
    /// attaches through the raster provider contract), and the styled base is
    /// installed as soon as a view is ready. Cleared by an explicit base-map
    /// choice or a credential application, which both win over the default.
    private var pendingKeylessVectorBaseChoiceKey: String?

    /// The remembered styled base still awaiting the shipped map view. It
    /// wins over the keyless default and over nothing else: an explicit
    /// selection clears it.
    private var pendingRememberedVectorBaseChoiceKey: String?

    /// The base map the user picked this session; nil follows the active
    /// base map's registry-order default.
    @Published private(set) var selectedBaseMapProviderKey: String?

    /// The styled vector base the user picked this session, while the
    /// registry still offers it; nil while the raster base serves (the
    /// default).
    @Published private(set) var selectedVectorBaseChoiceKey: String?

    /// The last vector base status the view reported (its honest
    /// readback): the configured style, its attribution and the last frame's
    /// tile counts.
    @Published private(set) var vectorBaseStatus: VectorTileSourceStatus?

    /// The unapplied credential drafts, keyed by provider key (the masked
    /// secure fields' text; never echoed anywhere else).
    @Published var providerCredentialDrafts: [String: String] = [:]

    /// The applied status of each provider row, keyed by provider key.
    @Published private(set) var providerCredentialStatuses:
        [String: ProviderCredentialStatus] = [:]

    /// Whether "Remember on this device" is on (default off; §6.5's opt-in
    /// persistence). Read through the settings view; changed through
    /// `setRememberProviderCredential(_:)`.
    @Published private(set) var remembersProviderCredential = false

    /// The honest secure-store note shown in the settings view when the
    /// Keychain refuses a request (nil while nothing failed).
    @Published private(set) var providerCredentialPersistenceNote: String?

    /// The Keychain store behind the opt-in toggle (no plaintext fallback).
    private let providerCredentialStore = ProviderCredentialStore()

    /// The credentials applied to the running session in memory, keyed by
    /// provider key. This is what the remember toggle persists and what
    /// Clear drops; the values are never logged or echoed.
    private var appliedProviderCredentials: [String: String] = [:]

    /// Whether any provider registration succeeded in this running
    /// session. It is deliberately one-way: the SDK registry has no
    /// unregister operation, so clearing the displayed credential keeps
    /// the already-registered surface attached (§6.5's restart rule).
    private var hasRegisteredProviderThisSession = false

    /// The provider-credential settings surface's accessibility identifier.
    static let providerSettingsSheetIdentifier = "provider-settings-sheet"

    // MARK: Camera state

    /// The live camera values (gestures, readout, presets agree with the
    /// drawn camera; the shipped `MapView` owns the camera store and
    /// reports every adopted pose back through its delegate).
    @Published private(set) var cameraValues = ReferenceCameraValues(
        centerLatitudeDegrees: 38.5766,
        centerLongitudeDegrees: -121.4944,
        zoomLevel: 14,
        bearingDegrees: 0,
        pitchDegrees: 0
    )

    /// The selected camera-preset identifier, cleared by direct camera
    /// manipulation (§4: the selected preset gets the accent text).
    @Published private(set) var selectedPresetIdentifier: String?

    /// The camera the last successful frame drew with (the diagnostics
    /// readback).
    @Published private(set) var cameraAttributeReadback: CameraAttributeReadback?

    /// The confirmed single taps the shipped view reported this session
    /// (§5.2: a double tap zooms and reports no click of its own). The
    /// diagnostics sheet renders the count beside the camera readback, and
    /// the UI suite asserts one tap per confirmed click.
    @Published private(set) var mapClickCount: Int = 0

    // MARK: SDK state

    /// The SDK instance of the session.
    private(set) var mapSdk: MapSdk?

    /// The shipped map view of the session (`MapNativeCoreUI.MapView`):
    /// it owns the surface, the display-link pump, resize, gestures
    /// and the camera store. Nil until the SwiftUI wrapper creates it.
    private(set) var mapView: MapNativeCoreUI.MapView?

    /// Whether the surface is currently attached (set on the first
    /// succeeded frame; the diagnostics sheet reports the mode from it).
    @Published private(set) var isSurfaceAttached = false

    // MARK: Device positioning

    /// Whether the device location source is updating.
    @Published private(set) var isDeviceLocationActive = false

    /// The honest line beside the location control, when it has something to say.
    @Published private(set) var deviceLocationStatusText: String?

    /// The contract controller the source feeds (one per map session).
    private var devicePositionController: PositionController?

    /// The running Core Location source, or nil while location is off.
    private var deviceLocationSource: DeviceLocationSource?

    /// Whether the planned route's guidance follows the device source's fixes
    /// (the Location group's second control) rather than the simulated trip.
    @Published private(set) var isDeviceNavigationActive = false

    /// Whether starting device navigation also started the location source,
    /// so stopping the mode leaves a source the visitor started by hand
    /// running.
    private var deviceNavigationStartedLocationSource = false

    /// Whether the status caption currently states why device navigation
    /// could not start. The refusal outranks a live position update, so the
    /// visitor can read what the control needs instead of losing the line to
    /// the next fix; planning a route clears it.
    private var showsDeviceNavigationPlanFirstRefusal = false

    // MARK: The on-map layer control

    /// Whether the map's layer panel is presented.
    @Published var isMapLayersPanelOpen = false

    /// Whether an open of the layer panel should bring the Device location
    /// row into view (the round control's long press, a refused source).
    @Published var shouldFocusMapLayersDeviceLocation = false

    /// The traffic layer's flow rows currently drawn; empty while the layer
    /// is off. The descriptor is appended to the overlay list by
    /// `activeOverlayDescriptors`.
    private(set) var trafficFlowSegments: [TrafficFlowData] = []

    /// Whether the next accepted device fix centres the camera (set when the
    /// round control starts the source, cleared by the first fix).
    private var shouldCenterOnNextFix = false

    /// Whether the device location choice belongs in the remembered record:
    /// true while the visitor's source is on, false after they stop it.
    private var remembersDeviceLocationEnabled = false

    /// Whether a remembered device-location choice waits for the shipped map
    /// view before its source can start.
    private var pendingRememberedDeviceLocationStart = false

    /// The layer panel's state machine. It reaches the SDK, the map view's
    /// camera bounds, the base-map selection path and the remembered record
    /// only through the seams built here.
    lazy var mapLayersState: MapLayersPanelState = makeMapLayersPanelState()

    /// Builds the panel state over the application's production seams.
    private func makeMapLayersPanelState() -> MapLayersPanelState {
        MapLayersPanelState(
            rememberedChoicesStore: rememberedChoicesStore,
            fetchTrafficFlow: { [weak self] boundingBox in
                guard let sdk = self?.mapSdk else {
                    throw MapLayersTrafficError.sdkUnavailable
                }
                return try await sdk.fetchTrafficFlow(boundingBox: boundingBox)
            },
            cameraGeographicBounds: { [weak self] in
                self?.mapView?.cameraGeographicBounds()
            },
            installTrafficOverlay: { [weak self] trafficFlowSegments in
                self?.trafficFlowSegments = trafficFlowSegments
                self?.rebuildOverlays()
            },
            readTrafficCacheStatistics: { [weak self] in
                self?.mapSdk?.trafficFlowCacheStatistics()
            },
            readTileCacheStatistics: { [weak self] in
                self?.mapSdk?.tileCacheStatistics()
            },
            clearTrafficOverlay: { [weak self] in
                self?.trafficFlowSegments = []
                self?.rebuildOverlays()
            },
            startDeviceLocation: { [weak self] in
                self?.startDeviceLocationSource() ?? .unavailable
            },
            fillRouteField: { [weak self] field, displayText in
                self?.fillRouteField(field, with: displayText)
            },
            persistTrafficChoice: { [weak self] in
                self?.persistRememberedChoices()
            }
        )
    }

    // MARK: Directions

    /// The Directions choice every plan carries. It reads its two remembered
    /// choices from the versioned record when it is first used.
    lazy var directionsState: DirectionsChoiceState = makeDirectionsState()

    /// The options the last plan action carried. The diagnostics sheet reads
    /// them, so an automated pass can witness the mode a plan used (the
    /// record itself stays free of transient route data).
    @Published private(set) var lastPlannedRouteOptions: RouteOptions?

    /// Whether the last plan action reached a provider. A keyless session
    /// plans the artifact's recorded route and states so.
    @Published private(set) var lastPlanUsedProvider = false

    /// Builds the Directions state over the shared remembered record and the
    /// application's contract-error wording.
    private func makeDirectionsState() -> DirectionsChoiceState {
        DirectionsChoiceState(
            rememberedChoicesStore: rememberedChoicesStore,
            failureText: { [weak self] error in
                guard let self else {
                    return CapabilityProviderState.defaultFailureText(error)
                }
                if let sdkError = error as? SdkError {
                    return self.sdkFailureDetailText(sdkError)
                }
                return CapabilityProviderState.defaultFailureText(error)
            }
        )
    }

    /// The `RouteOptions` every plan the application performs carries: the
    /// selected mode and strategy with the demo's unit and avoidance
    /// choices.
    func currentRouteOptions() -> RouteOptions {
        directionsState.routeOptions(
            unitSystem: .metric,
            avoidOptions: AvoidOptions(
                tollRoads: false,
                limitedAccessHighways: false,
                ferries: false,
                unpavedRoads: false
            )
        )
    }

    /// Binds the Directions state to the running SDK and reads its capability
    /// report. The plan sheet, a credential change and a routing-provider
    /// selection all call this.
    func refreshDirectionsCapabilities() {
        directionsState.attach(reporter: mapSdk)
        directionsState.refreshCapabilities()
    }

    /// The diagnostics sheet's Directions block: the selected choices and
    /// the last plan action's actual options.
    var directionsDiagnosticsText: String {
        let selectedModeName = directionsState.mode.recordValue
        let selectedStrategyName = directionsState.strategy.recordValue
        guard let lastPlannedRouteOptions else {
            return "selected \(selectedModeName) · \(selectedStrategyName); no plan yet"
        }
        let plannedModeName = DirectionsMode.diagnosticsName(
            for: lastPlannedRouteOptions.transportationMode
        )
        let plannedStrategyName = DirectionsStrategy.diagnosticsName(
            for: lastPlannedRouteOptions.routeStrategy
        )
        let provenance = lastPlanUsedProvider
            ? "provider request"
            : "recorded route; no provider request"
        return "last plan \(plannedModeName) · \(plannedStrategyName) · \(provenance)"
    }

    // MARK: Demo content

    /// The shared demo artifact (§9).
    private(set) var catalog: ReferenceScenesCatalog?

    /// The enabled overlay scenes (the artifact's scene keys).
    @Published var enabledSceneKeys: Set<String> = []

    /// The demo position the plan flow's origin field defaults to (§4.1:
    /// "the shared artifact's demo position is its placeholder").
    private(set) var demoPositionCoordinate: GeographicCoordinate?

    /// The demo-plan picker's plans (the recorded traces loaded at start,
    /// §4.1).
    @Published private(set) var demoPlans: [ReferenceDemoPlan] = []

    /// The demo plan the demo-plan picker has selected (the artifact's
    /// trace at start; a selection drops a planned route, §4.1).
    @Published private(set) var selectedDemoPlan: ReferenceDemoPlan?

    /// Whether a demo-plan chip act applied the plan's route-line overlay
    /// (the chip act's same overlay push the Android and RN pickers run;
    /// the default-selected plan carries no line before its own selection
    /// act, mirroring their startup).
    private var isDemoPlanRouteLineApplied = false

    /// The recorded journey fixture (decoded once at start; the sample
    /// journey's replay source).
    private(set) var recordedJourney: RecordedTraceJourney?

    // MARK: Offline maps (§4.1)

    /// The registered tile-cache state (path + budget), set once the
    /// tile-cache provider registration ran; nil before it (the card
    /// shows the disabled-with-reason state, §6.2).
    @Published private(set) var offlineTileCacheState: OfflineTileCacheState?

    /// The honest note shown in place of a registered-cache line after a
    /// credential-less base map took the raster slot (the installation went
    /// with the provider that installed it); nil while a cache is registered
    /// or before any registration.
    @Published private(set) var offlineTileCacheUncachedNote: String?

    /// The last drawn frame's tile statistics (the offline card's live
    /// tile line, the diagnostics sheet's per-draw block, §4.1).
    @Published private(set) var lastFrameTileStatistics: FrameStatistics?

    /// The offline card's region-download state (the shared artifact's demo
    /// region, its live progress, the stored-region list and deletion).
    @Published private(set) var offlineRegionState: OfflineRegionState?

    /// The draw delegate's publication gate: an identical frame publishes
    /// nothing, and a changed frame publishes at most once per cadence
    /// window, so the display cadence cannot drive a SwiftUI change per
    /// presented frame.
    private var framePublicationGate = MapFramePublicationGate(
        minimumIntervalSeconds:
            ReferenceApplicationConfiguration.frameReadbackPublishMinimumIntervalSeconds
    )

    // MARK: Plan flow (§4.1: search → route → confirm)

    /// The origin field's type-ahead (its own debounce, list and states;
    /// `docs/ReferenceAppUX.md §5.5`).
    lazy var originTypeAheadController = makeTypeAheadController(
        assignmentTarget: .origin
    )

    /// The destination field's type-ahead.
    lazy var destinationTypeAheadController = makeTypeAheadController(
        assignmentTarget: .destination
    )

    /// The destination field's query text ("Where to?").
    @Published var destinationSearchText = "" {
        didSet { persistRememberedChoices() }
    }

    /// The origin field's query text (empty until typed or assigned; there
    /// is no default origin — the two-field flow asks for one, §4.1).
    @Published var originSearchText = "" {
        didSet { persistRememberedChoices() }
    }

    /// The last text submitted to the plan search (never a provider payload).
    @Published private(set) var lastSearchQueryText = ""

    /// The geocode results of the last search (one shared result list
    /// under both fields).
    @Published private(set) var searchResults: [Place] = []

    /// Whether `searchResults` holds the shared artifact's demo places (the
    /// keyless search, §4.1): the result list's heading then says so.
    @Published private(set) var searchResultsAreDemoPlaces = false

    /// The keyless search's honest no-match line, nil while a demo match
    /// stands or before a search ran.
    @Published private(set) var keylessSearchNoMatchText: String?

    /// Whether a geocode request is in flight (the search view's in-flight
    /// line, §4.1).
    @Published private(set) var isSearchRunning = false

    /// The in-flight plan search, if any. A newer search cancels it and waits
    /// for it to finish, so two provider calls never overlap; the canceled
    /// call's result is dropped because its task is marked cancelled.
    private var planSearchTask: Task<Void, Never>?

    /// The text the origin field held when its result was assigned; the
    /// field's current text equals it while the assignment still stands.
    @Published private(set) var originAssignedText: String?

    /// The text the destination field held when its result was assigned.
    @Published private(set) var destinationAssignedText: String?

    /// Which field a tapped result assigns to (the last focused one).
    @Published var searchFieldAssignmentTarget: SearchFieldAssignmentTarget = .destination

    /// The route's origin coordinate (resolved from the origin field by the
    /// explicit Plan route action, §4.1).
    @Published private(set) var originCoordinate: GeographicCoordinate?

    /// The route's destination coordinate (resolved from the destination
    /// field by the explicit Plan route action).
    @Published private(set) var destinationCoordinate: GeographicCoordinate?

    /// The planned route, drawn as the route polyline overlay; the route
    /// summary card renders its records. A plan supersedes the device
    /// navigation control's plan-first refusal line.
    @Published private(set) var plannedRoute: Route? {
        didSet {
            if plannedRoute != nil {
                clearDeviceNavigationPlanFirstRefusal()
            }
        }
    }

    /// Whether the planned route is the shared artifact's recorded demo
    /// route rather than a live planning result (the summary card says so).
    @Published private(set) var plannedRouteIsRecordedDemoRoute = false

    /// Whether the planned route came from the bundled offline routing
    /// archive: the summary says it is recorded test data, not live data.
    @Published private(set) var plannedRouteIsOfflineArchiveRoute = false

    /// Whether a route calculation is in flight (the summary card's
    /// planning state).
    @Published private(set) var isRouteCalculationRunning = false

    // MARK: Simulated navigation (§7, §4.2)

    /// The turn-by-turn card store the listener topics drive.
    let navigationCardStore = NavigationStateCardStore()

    /// The platform voice (the contract's speech channel, spoken
    /// verbatim); the drawer's speech toggle drives it.
    let speechEngine = NavigationSpeechEngine()

    /// Whether the current speech is on (§4.2: default on; toggled in the
    /// drawer).
    @Published var isSpeechEnabled: Bool = true {
        didSet {
            speechEngine.isVoiceEnabled = isSpeechEnabled
            if !isSpeechEnabled {
                speechEngine.stopVoice()
            }
        }
    }

    /// Whether a simulated journey is running (the drawer replaces the
    /// home sheet while it is).
    @Published private(set) var isJourneyActive = false

    /// Whether the running journey's playback is paused at its current fix.
    @Published private(set) var isJourneyPaused = false

    /// Whether the running journey reached its destination. The arrival
    /// presentation outlives the playback (the destination stays centred and
    /// only the drawer's exit empties it, §7.1/§7.2).
    @Published private(set) var hasSimulatedJourneyArrived = false

    /// The playback speed multiplier the journey advances at — one of
    /// `simulatedTripSpeedMultiplierChoices`. The choice persists across
    /// trips (the user picked it for the session).
    @Published private(set) var journeySpeedMultiplier: Double =
        ReferenceApplicationConfiguration.simulatedTripSpeedMultiplierChoices[0]

    /// Whether the off-route simulation displaces the simulated fixes
    /// (the deviation demo, applied to the journey that starts while it
    /// is on — §6.2's disabled-with-reason governs the toggle).
    @Published var isOffRouteSimulationActive: Bool = false

    /// Whether the platform voice has spoken the trip's guidance (the
    /// diagnostics' and evidence pass's honest TTS-exercise signal).
    @Published private(set) var hasSpokenGuidance = false

    /// The navigation controller of the running journey.
    private var navigationController: Navigator?

    /// The topic subscriptions of the running journey (canceled on stop —
    /// snapshot cancel semantics, so already-enqueued events still
    /// deliver).
    private var topicSubscriptions: [RusticMapsSDK.Subscription] = []

    /// The replay plan the running journey consumes (recorded trace or
    /// planned-route walk).
    private var journeyReplayPlan: JourneyReplayPlan?

    /// The route walker of a route-following journey, or nil for a recorded
    /// trace.
    private var routeSimulator: RouteSimulator?

    /// The route-following walk's clock, in simulated seconds since the trip
    /// start (re-anchored at the travelled distance after a pace change).
    private var elapsedSimulatedSeconds: Double = 0

    /// The distance the running journey has travelled, in meters. The
    /// drawer's seek control reads it, and a pace change or a seek anchors
    /// the walk's clock at it.
    @Published private(set) var journeyTravelledMeters: Double = 0

    /// The running route-following journey's total length, in meters (the
    /// seek control's range); 0 while the trip cannot be seeked.
    @Published private(set) var journeySeekableDistanceMeters: Double = 0

    /// Whether the presented journey can be seeked: it walks the route's own
    /// geometry through a `RouteSimulator` (a recorded demo trace replays its
    /// own samples and has no route position to seek to).
    @Published private(set) var isJourneySeekable = false

    /// Whether the route-following walk delivered its first fix (the start
    /// fix): later ticks advance the clock.
    private var hasDeliveredRouteWalkStartFix = false

    /// The replay timer driving the accelerated location updates.
    private var journeyTimer: Timer?

    /// The index of the next replay sample.
    private var journeySampleIndex = 0

    // MARK: Navigation follow camera (§5.3) and the location indicator (§4.2)

    /// Whether the follow camera's writes are suspended by a user gesture
    /// (§5.3: a drag/pinch/rotate breaks follow immediately; it stays broken
    /// until the my-location control re-engages it — a later fix does not).
    @Published private(set) var isFollowBrokenByGesture = false

    /// Whether the visitor chose north-up during the trip: the follow
    /// camera then holds bearing 0 across every fix until course-up returns.
    private(set) var isHoldingNorthUp = false

    /// The follow zoom the running trip carries (§2.4's base zoom, locked
    /// at trip start; a user-nudged zoom during the trip persists under the
    /// §2.4 maximum clamp).
    private var followingZoomLevel: Double = 0

    /// The pre-trip camera snapshot — the values (a fitted framing's
    /// readback included) and the selected preset — restored after the trip
    /// (§5.3: "applied at trip start, restored after").
    private var preTripCameraSnapshot: PreTripCameraSnapshot?

    /// The generated circular median over the fixes' course-over-ground
    /// history (the §5.3 smoothing window: the camera's bearing follows the
    /// median of the last `courseFollowSmoothingFixes` computed courses).
    private var courseMedianSmoother: CourseMedianSmoother?

    /// The last fix the follow camera saw (course computation + indicator).
    private var lastFixCoordinate: GeographicCoordinate?

    /// The smoothed course-over-ground the indicator's guided wedge points
    /// along (nil until two displaced fixes produced a sample).
    private var lastFixCourseHeadingDegrees: Double?

    /// The location puck's interpolation: every fix starts an eased
    /// motion toward it, and a 60 Hz timer draws the pose while the motion
    /// runs, so the disc glides instead of hopping the fix distance.
    private let locationPoseAnimator = LocationPoseAnimator()

    /// Whether the current fix draws the guided wedge rather than the dot.
    private var latestLocationGuided = false

    /// The redraw timer that runs while a motion is in flight.
    private var locationAnimationTimer: Timer?

    /// Whether the map's pitch mode is 3D (the §2.4 45° follow pitch) —
    /// the cluster's 2D/3D toggle owns the state; a trip starts on 3D
    /// (§5.3) and restores the pre-trip mode after.
    @Published var isThreeDimensionalPitchEnabled = false

    /// The scale bar's last readout, computed from the drawn camera
    /// (§4.2: the §2.4 width range and the 1-2-3-5-10 ladder over
    /// `ground_meters_per_screen_pixel`).
    @Published private(set) var scaleBarReadout: ScaleBarReadout?

    /// Whether a simulated tracking fix is placed at the demo position
    /// (the idle session's tracking-disc state for the indicator; journeys
    /// switch the layer to the guided wedge while they run).
    @Published private(set) var isTrackingFixPlaced = false

    // MARK: Diagnostics events

    /// The buffered recent diagnostic events the sheet's readout renders.
    let diagnosticsEventsBuffer = SdkDiagnosticEventsBuffer()

    /// The listener the live subscription and the backlog flush deliver
    /// through (one object, so live events append after the flushed ones
    /// in the same oldest-first buffer).
    private lazy var diagnosticsEventListener = MainActorDiagnosticEventListener(
        buffer: diagnosticsEventsBuffer
    )

    /// Whether the one live subscription is armed (the first sheet open arms
    /// it; later opens reuse it — a second subscribe would double-deliver).
    private var isDiagnosticsEventsSubscriptionArmed = false

    /// The severity the armed subscription currently gates at, so the
    /// explicit Show debug events switch lowers it exactly once and the gate
    /// is never raised back within the session.
    private var diagnosticsEventsSubscriptionSeverity: SdkLogSeverity =
        ReferenceApplicationConfiguration.diagnosticsEventsMinimumSeverity

    /// Whether the pane shows debug-severity events. Off by default and never
    /// persisted: a relaunch starts with diagnostics off, and once the switch
    /// lowers the SDK gate the detail stays on until the app restarts.
    @Published private(set) var diagnosticsShowsDebugEvents = false

    // MARK: Provider console (M58)

    /// The console's read-only feed over the SDK's diagnostics stream. It is
    /// a second subscriber beside the diagnostics pane's and issues no
    /// provider call of its own.
    let providerConsoleFeed = ProviderConsoleFeed()

    /// The listener the console's subscription and backlog flush deliver
    /// through.
    private lazy var providerConsoleEventListener =
        ProviderConsoleDiagnosticEventListener(feed: providerConsoleFeed)

    /// Whether the console's one live subscription is armed (the first
    /// console open arms it; later opens reuse it — a second subscribe would
    /// double-deliver).
    private var isProviderConsoleSubscriptionArmed = false

    /// Whether the provider console sheet is open. Opening arms the lazy
    /// subscription and the merge cadence; closing keeps the warning/error
    /// stream for the unread dot and stops the cadence.
    @Published var isProviderConsoleOpen = false {
        didSet {
            guard isProviderConsoleOpen != oldValue else { return }
            if isProviderConsoleOpen {
                startProviderConsoleFeed()
            } else {
                stopProviderConsoleFeed()
            }
        }
    }

    /// The console's level filter (Error / Warning / Info; Info shows all).
    @Published var providerConsoleLevelFilter: ProviderConsoleLevelFilter =
        .information

    /// Whether the bounded response excerpt is captured. Off by default and
    /// never persisted; the switch states that excerpts may contain provider
    /// content and are never uploaded.
    @Published private(set) var providerConsoleCapturesResponseExcerpts = false

    // MARK: Readiness ladder (§6.1) and sheet lifecycle

    /// Starts the application keyless: loads the demo content, builds the
    /// SDK, and restores an opted-in stored credential (registering the
    /// providers over the tile cache) or settles on the neutral keyless
    /// state. No provider key is embedded in the build; the settings
    /// surface applies one at runtime (§6.5). Every failure lands in the
    /// ladder or the error report, never swallowed.
    func startApplication() async {
        // The UI-test probe runs before the slower session start, so the
        // floating control's unread Warning/Error dot is observable without
        // waiting for provider registration or a provider call.
        recordProviderConsoleProbeIfRequested()
        do {
            try loadDemoContent()
        } catch {
            setReadiness(.failed, String(describing: error))
            return
        }
        let sessionSdk = MapSdk()
        mapSdk = sessionSdk
        directionsState.attach(reporter: sessionSdk)
        offlineRegionState = makeOfflineRegionState()
        restoreRememberedProviderCredentials(into: sessionSdk)
        applyRuntimeInjectedCredentialIfPresent()
        // The type-ahead provider follows the credential state: the provider
        // a credential registered when one is applied, the registry's keyless
        // provider otherwise. The capability rows below read its registration.
        configureTypeAheadProvider()
        // The capability rows read the registration state the credential
        // application above just produced; the remembered per-capability
        // choices apply after it through `restoreRememberedChoices()`.
        capabilityProviderState.attach(
            sdk: sessionSdk,
            descriptors: providerDescriptors,
            appliedCredential: { [weak self] providerKey in
                self?.appliedProviderCredentials[providerKey]
            },
            failureText: { [weak self] error in
                self?.capabilityProviderFailureText(error)
                    ?? CapabilityProviderState.defaultFailureText(error)
            }
        )
        if surfaceSdk == nil {
            setReadiness(
                .keyless,
                ReferenceApplicationConfiguration.keylessReadinessText
            )
        }
        // The explicit choices from a previous run apply before the first
        // frame: theme and base map restore silently, the demo plan and
        // scenes fill their pickers, and the remembered search pair fills
        // the fields without planning a route.
        restoreRememberedChoices()
        selectKeylessBaseMapWhenUncredentialed()
        // The remembered base map applies after that default, because the
        // keyless raster registration replaces the vector slot. A remembered
        // choice always wins over the application's default.
        restoreRememberedBaseMap()
        // The base map registrations above own the two tile capabilities;
        // the rows read the registry back once they have run.
        capabilityProviderState.refresh()
        // A remembered or restored capability choice may have moved place
        // search; the type-ahead readback follows whatever now serves.
        configureTypeAheadProvider()
        // A routing-provider selection in the settings surface changes what
        // the serving provider can honour, so the Directions choices re-read
        // the capability report.
        capabilityProviderState.onProviderSelectionChanged = { [weak self] contract in
            switch contract {
            case .routing:
                self?.refreshDirectionsCapabilities()
            case .placeSearch:
                // The settings surface owns the type-ahead provider: the
                // selection's registration is what the fields now call.
                self?.configureTypeAheadProvider()
            default:
                break
            }
        }
        // The layer panel reads the same choices and registrations: its base
        // rows and the traffic switch's availability resolve now, and a
        // remembered traffic choice applies only once the capability's
        // credential is in force (a keyless session keeps it for a later
        // launch).
        refreshMapLayersState()
        mapLayersState.restoreRememberedTrafficIfAvailable()
        refreshDirectionsCapabilities()
        // The base-map and credential registrations above may have installed
        // or dropped the tile cache; the region card reads its truth now.
        await offlineRegionState?.refresh()
        await applyLaunchAutomation()
    }

    /// Rebuilds the layer panel's base rows and traffic availability from the
    /// current choices and applied credentials. Every base-map or credential
    /// change calls this, so the panel never renders a stale serving mark or
    /// a missing-credential row that is now servable.
    private func refreshMapLayersState() {
        mapLayersState.refreshBaseRows(
            rasterChoices: baseMapChoices,
            vectorChoices: vectorBaseMapChoices,
            activeRasterProviderKey: activeVectorBase == nil
                ? activeBaseMap?.providerKey
                : nil,
            activeVectorChoiceKey: activeVectorBase?.choiceKey
        )
        mapLayersState.updateTrafficConfiguration(
            providerDescriptors: providerDescriptors,
            appliedProviderKeys: Set(appliedProviderCredentials.keys)
        )
    }

    /// The automation credential seam (§6.5): a launch-environment variable
    /// supplies a provider credential at runtime for local and CI UI
    /// automation, optionally paired with a provider key. It goes through the
    /// public registration path, is never persisted by the injection and is
    /// never logged; it is not a build-time key seam.
    private func applyRuntimeInjectedCredentialIfPresent() {
        let environment = ProcessInfo.processInfo.environment
        let environmentCredential = environment[
            ReferenceApplicationConfiguration.runtimeCredentialEnvironmentKey
        ].flatMap { value in value.isEmpty ? nil : value }
        // A build made for the hosted demo carries its own dedicated
        // credential in the Info.plist; every ordinary build expands
        // that key to nothing, so this stays nil outside the hosted pipeline.
        // The environment seam keeps precedence so a local run can override it.
        let hostedDemoCredential = environmentCredential == nil
            ? ReferenceApplicationConfiguration.hostedDemoBundleCredential()
            : nil
        guard let runtimeCredential = environmentCredential ?? hostedDemoCredential?.credential else {
            return
        }
        let requestedProviderKey = environment[
            ReferenceApplicationConfiguration.runtimeProviderKeyEnvironmentKey
        ].flatMap { providerKey in
            providerKey.isEmpty ? nil : providerKey
        } ?? hostedDemoCredential?.providerKey
        // Absent an explicit provider key, the only provider the registry
        // declares available on iOS is the target (never a vendor guess).
        let availableDescriptors = providerCredentialDescriptors.filter {
            descriptor in descriptor.registrationAvailable.ios
        }
        let defaultProviderKey = availableDescriptors.count == 1
            ? availableDescriptors[0].providerKey
            : nil
        guard let providerKey = requestedProviderKey ?? defaultProviderKey else {
            return
        }
        applyProviderCredential(
            providerKey: providerKey,
            credential: runtimeCredential
        )
    }

    /// The attribution notice the bottom dock renders: the serving base
    /// map's own line. A styled vector base carries its style's design line
    /// (the registry's `vector_styles` attribution), which replaces the
    /// provider's general line while that base serves. A descriptor that is
    /// not the active base map claims nothing, so the notice never names a
    /// provider the map is not serving. Empty when the registry declares no
    /// line (the notice renders nothing rather than a guess).
    var attributionNoticeText: String {
        let baseAttributionText = activeVectorBase?.attributionText
            ?? activeBaseMap?.attributionText
            ?? ""
        guard let trafficAttributionText = mapLayersState.trafficAttributionText,
              !trafficAttributionText.trimmingCharacters(
                  in: .whitespacesAndNewlines
              ).isEmpty
        else {
            return baseAttributionText
        }
        guard !baseAttributionText.isEmpty else {
            return trafficAttributionText
        }
        return "\(baseAttributionText) · \(trafficAttributionText)"
    }

    /// The terms note the attribution panel lists under the attribution
    /// line: the serving base map's (vector base first) own terms note, or
    /// empty when the registry declares none (the panel then renders the
    /// attribution line alone rather than a guess).
    var attributionTermsNotes: [String] {
        let servingDescriptor = activeVectorBase?.descriptor ?? activeBaseMap
        guard let termsNote = servingDescriptor?.termsNote,
              !termsNote.trimmingCharacters(
                  in: .whitespacesAndNewlines
              ).isEmpty
        else {
            return []
        }
        return [termsNote]
    }

    /// The tile-cache database path the demo registers its raster tile
    /// cache at: inside the application's caches directory (never the
    /// bundle, never git), in a named subfolder.
    private func offlineTileCacheDatabaseURL() -> URL {
        let cachesDirectoryURL = FileManager.default.urls(
            for: .cachesDirectory, in: .userDomainMask
        )[0]
        let cacheFolderURL = cachesDirectoryURL.appendingPathComponent(
            ReferenceApplicationConfiguration.offlineTileCacheFolderName
        )
        try? FileManager.default.createDirectory(
            at: cacheFolderURL, withIntermediateDirectories: true
        )
        return cacheFolderURL.appendingPathComponent(
            ReferenceApplicationConfiguration.offlineTileCacheDatabaseFileName
        )
    }

    /// Builds the offline card's region state over the shared artifact's demo
    /// region and the session's `MapSdk`. The artifact's region definition is
    /// converted fallibly: a region whose tile form the contract does not
    /// know is reported rather than silently mismatched against the base
    /// map's raster request.
    private func makeOfflineRegionState() -> OfflineRegionState? {
        guard let regionRecord = catalog?.offlineRegions.first else {
            return nil
        }
        let definition: RegionDefinition
        do {
            definition = try regionRecord.contractRegionDefinition()
        } catch {
            reportLocalError(
                origin: "offline region",
                detail: String(describing: error)
            )
            return nil
        }
        return OfflineRegionState(
            demoRegion: definition,
            serviceProvider: { [weak self] in
                guard let self, let sessionSdk = self.mapSdk else {
                    throw OfflineRegionServiceError.sdkUnavailable
                }
                return SdkOfflineRegionService(
                    manager: OfflineManager(mapSdk: sessionSdk)
                )
            },
            reportFailure: { [weak self] error in
                self?.reportOfflineRegionFailure(error)
            }
        )
    }

    /// Echoes an offline-region failure into the §6.3 error report: a
    /// contract failure by its own variant name and verbatim detail, a local
    /// failure by its origin.
    private func reportOfflineRegionFailure(_ error: Error) {
        if let sdkError = error as? SdkError {
            reportSdkError(sdkError)
        } else {
            reportLocalError(
                origin: "offline region",
                detail: String(describing: error)
            )
        }
    }

    /// Loads the demo content from the bundle resources: the shared demo
    /// artifact and the recorded trace fixture (both bundled from their
    /// single sources). The demo position (the plan flow's origin
    /// default) is the artifact's first camera preset's center.
    private func loadDemoContent() throws {
        let loadedCatalog = try ReferenceScenesCatalog.loadedFromApplicationBundle()
        catalog = loadedCatalog
        // The provider rows come from the shared registry, not the demo
        // artifact: one descriptor source for every refapp and the dev
        // server.
        let loadedProviderRegistry = try ProviderRegistryCatalog
            .loadedFromPackageResource()
        // A keyless descriptor (blank credential label) renders no settings
        // row, so a provider registered elsewhere never paints an empty field.
        providerDescriptors = loadedProviderRegistry.providers
        referenceAppUserAgentTemplate =
            loadedProviderRegistry.referenceAppUserAgentTemplate
        keylessBaseMap = loadedProviderRegistry.keylessBaseMap
        providerCredentialDescriptors = providerDescriptors
            .filter { descriptor in
                !descriptor.credentialLabel.trimmingCharacters(
                    in: .whitespacesAndNewlines
                ).isEmpty
            }
        if let firstPreset = loadedCatalog.cameraPresets.first {
            cameraValues = ReferenceCameraValues(
                centerLatitudeDegrees: firstPreset.latitudeDegrees,
                centerLongitudeDegrees: firstPreset.longitudeDegrees,
                zoomLevel: firstPreset.zoomLevel,
                bearingDegrees: firstPreset.bearingDegrees,
                pitchDegrees: firstPreset.pitchDegrees
            )
            selectedPresetIdentifier = firstPreset.id
            demoPositionCoordinate = GeographicCoordinate(
                latitude: firstPreset.latitudeDegrees,
                longitude: firstPreset.longitudeDegrees
            )
        }
        // The artifact's own navigation demo trace is also the recorded
        // journey the home sheet previews; both its name and its asset path
        // come from the artifact, never from an app-side literal.
        let artifactTraceAssetName = (
            loadedCatalog.navigationDemo.simulatedTrace.fixturePath as NSString
        ).lastPathComponent
        guard let fixtureURL = Bundle.main.url(
            forResource: (artifactTraceAssetName as NSString).deletingPathExtension,
            withExtension: (artifactTraceAssetName as NSString).pathExtension
        ) else {
            throw ReferenceScenesCatalog.ReferenceScenesCatalogError.catalogResourceMissing(
                bundleDescription: "the application bundle (recorded trace fixture)"
            )
        }
        recordedJourney = try RecordedTraceJourney.decoded(from: Data(contentsOf: fixtureURL))
        // The loader's own failure renders verbatim in the ladder
        // (§6.3: never paraphrased, never swallowed).
        let loadedDemoPlans = try ReferenceDemoPlanLoader.plansFromBundle(
            catalog: loadedCatalog
        )
        demoPlans = loadedDemoPlans
        // The artifact's own navigation demo trace is the picker's
        // default selection (the demo plans rule; an absent artifact
        // plan falls to the picker's first plan, still explicit).
        selectedDemoPlan = loadedDemoPlans.first(where: { $0.isArtifactTrace })
            ?? loadedDemoPlans.first
    }

    private func setReadiness(_ state: ReadinessState, _ text: String) {
        readinessState = state
        readinessText = text
    }

    // MARK: Remembered choices (PRD.md §6 item 67)

    /// Adopts the record read at construction, once the demo content exists
    /// and before the first frame or plan. The theme is already in force; the
    /// base map and camera preset restore silently, the demo plan and scenes
    /// fill their pickers, the speed returns to the chosen pace, and the
    /// remembered search pair fills the fields without planning a route — the
    /// user presses Plan route.
    func restoreRememberedChoices() {
        guard let remembered = rememberedChoicesStore.load() else { return }
        isRestoringRememberedChoices = true
        defer { isRestoringRememberedChoices = false }
        themeChoice = remembered.theme
        rememberedBaseMapProviderKey = remembered.baseMapProviderKey
        rememberedBaseMapStyleId = remembered.baseMapStyleId
        rememberedDemoPlanScenarioName = remembered.demoPlanScenarioName
        if let scenarioName = remembered.demoPlanScenarioName,
           let rememberedPlan = demoPlans.first(where: { demoPlan in
               demoPlan.scenarioName == scenarioName
           })
        {
            // The plan fills the picker; its route preview is not applied so
            // the remembered camera preset (or the default framing) stands.
            selectedDemoPlan = rememberedPlan
            isDemoPlanRouteLineApplied = true
        }
        if let catalog {
            let availableSceneKeys = Set(catalog.overlayScenes.keys)
            enabledSceneKeys = remembered.sceneKeys.intersection(availableSceneKeys)
        }
        if let presetIdentifier = remembered.cameraPresetIdentifier,
           let preset = catalog?.cameraPresets.first(where: { cameraPreset in
               cameraPreset.id == presetIdentifier
           })
        {
            applyCameraPreset(preset)
        }
        if let speedMultiplier = remembered.simulatedTripSpeedMultiplier,
           ReferenceApplicationConfiguration.simulatedTripSpeedMultiplierChoices
               .contains(speedMultiplier)
        {
            journeySpeedMultiplier = speedMultiplier
        }
        originSearchText = remembered.originSearchText
        destinationSearchText = remembered.destinationSearchText
        lastSearchQueryText = remembered.lastSearchQueryText
        // The per-capability choices apply after the credentials above, so a
        // remembered credentialed provider finds its credential; a restore
        // never rewrites the record.
        capabilityProviderState.restoreRememberedChoices()
        rebuildOverlays()
        // The device-location choice restores once the shipped view exists:
        // its first fix is what centres the camera, so a restored style or
        // plan framing cannot override it.
        remembersDeviceLocationEnabled = remembered.deviceLocationEnabled
        pendingRememberedDeviceLocationStart = remembered.deviceLocationEnabled
        restoreRememberedDeviceLocationIfPermitted()
    }

    /// Applies the remembered base map after the keyless default registered
    /// its raster source. The keyless default is not a user choice and must not
    /// remove a remembered styled base; this call therefore runs last.
    func restoreRememberedBaseMap() {
        guard let remembered = rememberedChoicesStore.load() else { return }
        isRestoringRememberedChoices = true
        defer { isRestoringRememberedChoices = false }
        applyRememberedBaseMap(remembered)
    }

    /// Applies the remembered base map: a styled vector base is staged for the
    /// shipped view (explicit selections and the keyless default always defer
    /// to it), while a raster provider that can serve is registered now. A
    /// remembered credentialed base whose credential is not on this device
    /// stays unapplied and keeps its record for a later launch.
    private func applyRememberedBaseMap(_ remembered: RememberedChoices) {
        guard let providerKey = remembered.baseMapProviderKey else { return }
        if let styleId = remembered.baseMapStyleId {
            guard let vectorChoice = vectorBaseMapChoices.first(where: { choice in
                choice.descriptor.providerKey == providerKey
                    && choice.style.styleId == styleId
            }) else {
                return
            }
            pendingRememberedVectorBaseChoiceKey = vectorChoice.choiceKey
            applyPendingRememberedVectorBase()
            return
        }
        guard let rasterChoice = baseMapChoices.first(where: { choice in
            choice.providerKey == providerKey && choice.isServable
        }) else {
            return
        }
        selectBaseMap(providerKey: rasterChoice.providerKey)
    }

    /// The complete current record: the explicit choices, never transient state.
    private func currentRememberedChoices() -> RememberedChoices {
        RememberedChoices(
            theme: themeChoice,
            baseMapProviderKey: rememberedBaseMapProviderKey,
            baseMapStyleId: rememberedBaseMapStyleId,
            demoPlanScenarioName: rememberedDemoPlanScenarioName,
            sceneKeys: enabledSceneKeys,
            trafficLayerEnabled: mapLayersState.isTrafficEnabled,
            deviceLocationEnabled: remembersDeviceLocationEnabled,
            directionsMode: directionsState.mode,
            directionsStrategy: directionsState.strategy,
            cameraPresetIdentifier: selectedPresetIdentifier,
            simulatedTripSpeedMultiplier: journeySpeedMultiplier,
            originSearchText: originSearchText,
            destinationSearchText: destinationSearchText,
            lastSearchQueryText: lastSearchQueryText,
            capabilityProviders: capabilityProviderState.rememberedProviderKeys
        )
    }

    /// Writes every explicit choice after one changes.
    private func persistRememberedChoices() {
        guard !isRestoringRememberedChoices, !isApplyingAutomaticBaseMap else {
            return
        }
        rememberedChoicesStore.store(currentRememberedChoices())
    }

    /// Installs the remembered styled base once the shipped map view exists.
    /// The install is a restore, not a new choice, so it never rewrites the
    /// record.
    private func applyPendingRememberedVectorBase() {
        guard let pendingChoiceKey = pendingRememberedVectorBaseChoiceKey else {
            return
        }
        guard mapView != nil else { return }
        pendingRememberedVectorBaseChoiceKey = nil
        guard vectorBaseMapChoices.contains(where: { choice in
            choice.choiceKey == pendingChoiceKey
        }) else {
            return
        }
        isApplyingAutomaticBaseMap = true
        defer { isApplyingAutomaticBaseMap = false }
        selectVectorBase(choiceKey: pendingChoiceKey)
    }

    /// The settings surface's "Reset remembered choices" act: clears the
    /// record and returns the in-memory choices to their defaults. The
    /// provider credential store is untouched; a running session keeps the
    /// providers it already registered.
    func resetRememberedChoices() {
        isRestoringRememberedChoices = true
        themeChoice = .auto
        ThemeChoice.clearStoredChoice()
        rememberedBaseMapProviderKey = nil
        rememberedBaseMapStyleId = nil
        rememberedDemoPlanScenarioName = nil
        pendingRememberedVectorBaseChoiceKey = nil
        selectedPresetIdentifier = nil
        enabledSceneKeys.removeAll()
        selectedDemoPlan = demoPlans.first(where: { demoPlan in
            demoPlan.isArtifactTrace
        }) ?? demoPlans.first
        isDemoPlanRouteLineApplied = false
        journeySpeedMultiplier =
            ReferenceApplicationConfiguration.simulatedTripSpeedMultiplierChoices[0]
        originSearchText = ""
        destinationSearchText = ""
        lastSearchQueryText = ""
        originTypeAheadController.clear()
        destinationTypeAheadController.clear()
        capabilityProviderState.reset()
        mapLayersState.setTrafficEnabled(false, remember: false)
        pendingRememberedDeviceLocationStart = false
        if isDeviceLocationActive {
            stopDeviceLocation()
        } else {
            remembersDeviceLocationEnabled = false
        }
        mapLayersState.clearMyLocationFill(for: .origin)
        mapLayersState.clearMyLocationFill(for: .destination)
        directionsState.resetToDefaults()
        isRestoringRememberedChoices = false
        rememberedChoicesStore.clear()
        rebuildOverlays()
    }

    /// The readiness chip's short line (§4.2: the four states; the full
    /// readiness text lives in the diagnostics sheet).
    var readinessChipText: String {
        switch readinessState {
        case .loading:
            return ReferenceApplicationConfiguration.readinessChipLoadingText
        case .ready:
            return ReferenceApplicationConfiguration.readinessChipReadyText
        case .degraded:
            return ReferenceApplicationConfiguration.readinessChipDegradedPrefix
                + readinessText
        case .keyless:
            return ReferenceApplicationConfiguration.readinessChipKeylessText
        case .failed:
            return ReferenceApplicationConfiguration.readinessChipFailedPrefix
                + readinessText
        }
    }

    // MARK: Error report (§6.3)

    /// Labels the contract's error variants by their own name (the label
    /// line of §6.3: "the error block's label line shows the variant name
    /// when one is available").
    private func sdkErrorVariantName(_ sdkError: SdkError) -> String {
        switch sdkError {
        case .InvalidCoordinateInput: return "InvalidCoordinateInput"
        case .NetworkRequestFailed: return "NetworkRequestFailed"
        case .ResponseParsingFailure: return "ResponseParsingFailure"
        case .ProviderRequestRejected: return "ProviderRequestRejected"
        case .RateLimitExhausted: return "RateLimitExhausted"
        case .ProviderNotConfigured: return "ProviderNotConfigured"
        case .InvalidNavigationInput: return "InvalidNavigationInput"
        case .InvalidRenderInput: return "InvalidRenderInput"
        case .RenderingSurfaceUnavailable: return "RenderingSurfaceUnavailable"
        case .CacheOperationFailure: return "CacheOperationFailure"
        case .OfflineRoutingFailure: return "OfflineRoutingFailure"
        }
    }

    /// The verbatim detail text of a contract error (the detail string the
    /// SDK contract carries across the binding unchanged). The two sentences
    /// the contract does not carry as a detail string are the shared
    /// reference-tokens artifact's `strings.rate_limit_exhausted_detail` and
    /// `strings.provider_not_configured_detail_template`
    /// (platform/reference-tokens.json); scripts/check_ux_parity.mjs gates the
    /// text against that artifact.
    func sdkFailureDetailText(_ sdkError: SdkError) -> String {
        switch sdkError {
        case .InvalidCoordinateInput(let detail): return detail
        case .NetworkRequestFailed(let detail): return detail
        case .ResponseParsingFailure(let detail): return detail
        case .ProviderRequestRejected(let detail): return detail
        // platform/reference-tokens.json strings.rate_limit_exhausted_detail
        case .RateLimitExhausted: return "rate limit exhausted; retry after the next refill"
        // platform/reference-tokens.json strings.provider_not_configured_detail_template
        case .ProviderNotConfigured(let contractName):
            return ReferenceApplicationConfiguration.providerNotConfiguredDetailText(
                contractName: contractName
            )
        case .InvalidNavigationInput(let detail): return detail
        case .InvalidRenderInput(let detail): return detail
        case .RenderingSurfaceUnavailable(let detail): return detail
        // The cache and offline variants carry their own verbatim reason.
        case .CacheOperationFailure(let detail): return detail
        case .OfflineRoutingFailure(let detail): return detail
        }
    }

    /// Echoes a contract failure into the error report.
    func reportSdkError(_ sdkError: SdkError) {
        errorReport = ReferenceApplicationErrorReport(
            labelLine: sdkErrorVariantName(sdkError),
            detailText: sdkFailureDetailText(sdkError)
        )
    }

    /// Echoes a local (non-contract) failure into the error report: the
    /// label line names the origin, the detail echoes verbatim.
    func reportLocalError(origin: String, detail: String) {
        errorReport = ReferenceApplicationErrorReport(
            labelLine: origin,
            detailText: detail
        )
    }

    /// Clears the error report on the next successful action (§6.3).
    private func clearErrorReport() {
        errorReport = nil
    }
}

// MARK: - Provider credentials (docs/ReferenceAppUX.md §6.5)

extension ReferenceApplicationModel {
    /// The row's status, defaulting to not-applied before any action.
    func providerCredentialStatus(
        forProviderKey providerKey: String
    ) -> ProviderCredentialStatus {
        providerCredentialStatuses[providerKey] ?? .notApplied
    }

    /// Whether the running session has a provider credential applied (the
    /// keyless state's inverse; the offline card reads it).
    var hasAppliedProviderCredential: Bool {
        !appliedProviderCredentials.isEmpty
    }

    /// The offline card's disabled-with-reason line (§6.2): a keyless
    /// session names the missing credential instead of implying the SDK
    /// failed to start. An uncached-but-serving base map is not a disabled
    /// state — the card carries its own honest note instead.
    var offlineMapsDisabledReason: String? {
        guard offlineTileCacheState == nil,
              offlineTileCacheUncachedNote == nil else {
            return nil
        }
        guard mapSdk != nil, !hasAppliedProviderCredential else {
            return ReferenceApplicationConfiguration.offlineCardUnavailableReason
        }
        return ReferenceApplicationConfiguration.offlineCardKeylessReason
    }

    /// Applies a provider credential entered in the settings surface: the
    /// per-provider dispatch arm registers the SDK's providers (MapQuest
    /// through the tile-cache registration, the offline demo's path), then
    /// the first successful application builds the map surface and retries
    /// initialization. A refusal is shown verbatim (§6.5).
    func applyProviderCredential(providerKey: String, credential: String) {
        let enteredCredential = credential.trimmingCharacters(
            in: .whitespacesAndNewlines
        )
        guard let descriptor = providerCredentialDescriptors.first(
            where: { $0.providerKey == providerKey }
        ) else {
            providerCredentialStatuses[providerKey] = .failed(
                detailText: ReferenceApplicationConfiguration.unknownProviderReason(
                    providerKey: providerKey
                )
            )
            return
        }
        guard descriptor.registrationAvailable.ios else {
            providerCredentialStatuses[providerKey] = .failed(
                detailText: ReferenceApplicationConfiguration
                    .providerNotRegisteredReason(
                        displayName: descriptor.displayName
                    )
            )
            return
        }
        guard !enteredCredential.isEmpty else {
            providerCredentialStatuses[providerKey] = .failed(
                detailText: ReferenceApplicationConfiguration
                    .credentialEntryRequiredText
            )
            return
        }
        guard let mapSdk else {
            providerCredentialStatuses[providerKey] = .failed(
                detailText: ReferenceApplicationConfiguration
                    .credentialSdkNotRunningText
            )
            return
        }
        do {
            try registerProviders(
                forProviderKey: providerKey,
                credential: enteredCredential,
                on: mapSdk
            )
        } catch let sdkError as SdkError {
            providerCredentialStatuses[providerKey] = .failed(
                detailText: sdkFailureDetailText(sdkError)
            )
            reportSdkError(sdkError)
            return
        } catch {
            providerCredentialStatuses[providerKey] = .failed(
                detailText: String(describing: error)
            )
            reportLocalError(
                origin: "provider credentials", detail: String(describing: error)
            )
            return
        }
        appliedProviderCredentials[providerKey] = enteredCredential
        // A credential wins over the keyless styled base: its registration
        // must not end hidden under the default vector base.
        pendingKeylessVectorBaseChoiceKey = nil
        hasRegisteredProviderThisSession = true
        providerCredentialStatuses[providerKey] = .applied
        providerCredentialDrafts[providerKey] = ""
        clearErrorReport()
        if mapView?.sdk == nil {
            // The keyless session had no surface (attach needs the raster
            // provider); the first credential gives the shipped view its
            // SDK and it retries initialization on the next pump tick.
            mapView?.sdk = surfaceSdk
            setReadiness(
                .loading,
                ReferenceApplicationConfiguration.attachingReadinessText
            )
        }
        persistAppliedCredentialIfRemembered(
            providerKey: providerKey, credential: enteredCredential
        )
        // The newly registered providers fill their capability slots; the
        // rows and the pickers' credential reasons both follow the change.
        capabilityProviderState.clearMessages()
        capabilityProviderState.refresh()
        // The traffic capability now has its credential: the switch becomes
        // available and a remembered traffic choice can apply.
        refreshMapLayersState()
        mapLayersState.restoreRememberedTrafficIfAvailable()
        refreshDirectionsCapabilities()
        // The credential's registration also serves place search; the
        // type-ahead re-reads which provider now answers.
        configureTypeAheadProvider()
    }

    /// The verbatim text a capability selection's thrown error carries: the
    /// contract's own detail when the error crossed the contract, the local
    /// error's own description otherwise.
    func capabilityProviderFailureText(_ error: Swift.Error) -> String {
        if let sdkError = error as? SdkError {
            return sdkFailureDetailText(sdkError)
        }
        return CapabilityProviderState.defaultFailureText(error)
    }

    /// Clears a provider credential: the in-memory value and the stored
    /// copy both go, the row states the restart requirement (the SDK
    /// registry has no unregister operation, §6.5), and the running
    /// session's registration is unchanged until a restart.
    func clearProviderCredential(providerKey: String) {
        appliedProviderCredentials.removeValue(forKey: providerKey)
        providerCredentialDrafts[providerKey] = ""
        do {
            try providerCredentialStore.deleteCredential(
                forProviderKey: providerKey
            )
        } catch {
            providerCredentialPersistenceNote =
                ReferenceApplicationConfiguration.credentialStoreClearFailedReason(
                    detail: error.localizedDescription
                )
        }
        providerCredentialStatuses[providerKey] = .clearedRestartRequired
        if appliedProviderCredentials.isEmpty {
            remembersProviderCredential = false
        }
        clearErrorReport()
        // The cleared credential leaves the row's availability behind; the
        // registered capability itself stays for the running session.
        refreshMapLayersState()
        refreshDirectionsCapabilities()
    }

    /// Turns the opt-in persistence on or off. On stores every applied
    /// credential in the Keychain (and arms the next Apply); off deletes
    /// every stored copy. A Keychain refusal is reported honestly and the
    /// toggle returns to off — the credential stays session-only, never a
    /// plaintext fallback.
    func setRememberProviderCredential(_ shouldRemember: Bool) {
        guard shouldRemember != remembersProviderCredential else { return }
        guard shouldRemember else {
            remembersProviderCredential = false
            removeStoredProviderCredentials()
            return
        }
        guard !appliedProviderCredentials.isEmpty else {
            // Nothing applied yet: the preference governs the next Apply.
            remembersProviderCredential = true
            providerCredentialPersistenceNote = nil
            return
        }
        do {
            for (providerKey, credential) in appliedProviderCredentials {
                try providerCredentialStore.save(
                    credential: credential, forProviderKey: providerKey
                )
            }
            remembersProviderCredential = true
            providerCredentialPersistenceNote = nil
        } catch {
            remembersProviderCredential = false
            providerCredentialPersistenceNote =
                ReferenceApplicationConfiguration.credentialStoreUnavailableReason(
                    detail: error.localizedDescription
                )
        }
    }

    /// Restores the credentials the user opted to remember: a stored
    /// credential registers its provider before the first frame, so a
    /// remembered session starts on live map data exactly as if the key
    /// had just been applied.
    private func restoreRememberedProviderCredentials(into sdk: MapSdk) {
        var didRestoreCredential = false
        for descriptor in providerCredentialDescriptors
        where descriptor.registrationAvailable.ios {
            do {
                guard let storedCredential = try providerCredentialStore
                    .credential(forProviderKey: descriptor.providerKey)
                else { continue }
                do {
                    try registerProviders(
                        forProviderKey: descriptor.providerKey,
                        credential: storedCredential,
                        on: sdk
                    )
                } catch let sdkError as SdkError {
                    providerCredentialStatuses[descriptor.providerKey] = .failed(
                        detailText: sdkFailureDetailText(sdkError)
                    )
                    reportSdkError(sdkError)
                    continue
                } catch {
                    providerCredentialStatuses[descriptor.providerKey] = .failed(
                        detailText: String(describing: error)
                    )
                    reportLocalError(
                        origin: "provider credentials",
                        detail: String(describing: error)
                    )
                    continue
                }
                appliedProviderCredentials[descriptor.providerKey] =
                    storedCredential
                hasRegisteredProviderThisSession = true
                providerCredentialStatuses[descriptor.providerKey] = .applied
                remembersProviderCredential = true
                providerCredentialPersistenceNote = nil
                didRestoreCredential = true
            } catch {
                providerCredentialPersistenceNote =
                    ReferenceApplicationConfiguration
                        .credentialStoreUnavailableReason(
                            detail: error.localizedDescription
                        )
            }
        }
        guard didRestoreCredential else { return }
        setReadiness(
            .loading,
            ReferenceApplicationConfiguration.attachingReadinessText
        )
        mapView?.sdk = surfaceSdk
    }

    /// Every contract the shared registry can name, in the contract's stable
    /// order, paired with the enum the SDK call takes.
    private static let providerContractsByRegistryName: [(String, ProviderContract)] = [
        ("geocoding", .geocoding),
        ("routing", .routing),
        ("distance_matrix", .distanceMatrix),
        ("place_search", .placeSearch),
        ("traffic_incidents", .trafficIncidents),
        ("traffic_flow", .trafficFlow),
        ("elevation", .elevation),
        ("raster_tiles", .rasterTiles),
        ("vector_tiles", .vectorTiles),
        ("offline_routing", .offlineRouting),
    ]

    /// The per-provider registration dispatch arm (§9: the applier is the
    /// only provider-specific switch).
    private func registerProviders(
        forProviderKey providerKey: String,
        credential: String,
        on sdk: MapSdk
    ) throws {
        switch providerKey {
        case ReferenceApplicationConfiguration.mapquestProviderKey:
            try registerMapquestProviders(on: sdk, credential: credential)
        default:
            // Every other provider registers through the catalog: one
            // capability per call, for exactly the contracts its descriptor
            // declares.
            guard let descriptor = providerDescriptors.first(
                where: { $0.providerKey == providerKey }
            ) else {
                throw ProviderCredentialApplicationError.unknownProvider(
                    providerKey: providerKey
                )
            }
            var registeredContract = false
            for (registryName, contract) in Self.providerContractsByRegistryName
            where descriptor.servedContracts.contains(registryName) {
                try sdk.configureProvider(
                    contract: contract,
                    providerKey: providerKey,
                    credential: credential
                )
                registeredContract = true
            }
            guard registeredContract else {
                throw ProviderCredentialApplicationError.unknownProvider(
                    providerKey: providerKey
                )
            }
        }
    }

    /// Registers the seven MapQuest providers over the demo's disk tile
    /// cache: the raster-tile slot wraps the cache opened at the path the
    /// model knows, so the offline demo's cache and the credential apply
    /// are one registration.
    private func registerMapquestProviders(
        on sdk: MapSdk,
        credential: String
    ) throws {
        let tileCacheDatabaseURL = offlineTileCacheDatabaseURL()
        try sdk.setProviderCredentialWithTileCache(
            providerKey: ReferenceApplicationConfiguration.mapquestProviderKey,
            credential: credential,
            tileCacheDatabasePath: tileCacheDatabaseURL.path,
            tileCacheAbsoluteMaximumBytes: ReferenceApplicationConfiguration
                .offlineTileCacheAbsoluteMaximumBytes
        )
        offlineTileCacheState = OfflineTileCacheState(
            cacheDatabasePath: tileCacheDatabaseURL.path,
            budgetPhrase: offlineTileCacheBudgetPhrase
        )
        offlineTileCacheUncachedNote = nil
        // The registration installed the cache the region card reads; reread
        // its demo-region count and stored list.
        Task { [weak self] in
            await self?.offlineRegionState?.refresh()
        }
    }

    /// The offline card's budget phrase (the named byte budget in binary
    /// megabytes).
    private var offlineTileCacheBudgetPhrase: String {
        String(
            format: "%.0f MiB",
            Double(
                ReferenceApplicationConfiguration
                    .offlineTileCacheAbsoluteMaximumBytes
            ) / Double(
                ReferenceApplicationConfiguration.bytesPerBinaryMegabyte
            )
        )
    }

    /// Persists the just-applied credential when the user opted in; a
    /// Keychain refusal turns the toggle back off and notes the reason.
    private func persistAppliedCredentialIfRemembered(
        providerKey: String,
        credential: String
    ) {
        guard remembersProviderCredential else {
            providerCredentialPersistenceNote = nil
            return
        }
        do {
            try providerCredentialStore.save(
                credential: credential, forProviderKey: providerKey
            )
            providerCredentialPersistenceNote = nil
        } catch {
            remembersProviderCredential = false
            providerCredentialPersistenceNote =
                ReferenceApplicationConfiguration.credentialStoreUnavailableReason(
                    detail: error.localizedDescription
                )
        }
    }

    /// Deletes every stored provider credential (the remember toggle's off
    /// path); a refusal is reported honestly.
    private func removeStoredProviderCredentials() {
        for descriptor in providerCredentialDescriptors {
            do {
                try providerCredentialStore.deleteCredential(
                    forProviderKey: descriptor.providerKey
                )
            } catch {
                providerCredentialPersistenceNote =
                    ReferenceApplicationConfiguration
                        .credentialStoreClearFailedReason(
                            detail: error.localizedDescription
                        )
            }
        }
    }
}

// MARK: - Base map (M9's native base-map switch)

/// One base-map row of the settings sheet: the shared registry's descriptor
/// plus whether it can serve right now. A credential-registered provider is
/// offered before its key is applied and carries `isServable == false` until
/// then, so the row can say plainly what it needs instead of vanishing.
struct BaseMapChoice: Identifiable {
    /// The descriptor the row renders and registers from.
    let descriptor: ProviderDescriptor
    /// Whether selecting the row can register a source today.
    let isServable: Bool

    /// The registry key the row and its selection are addressed by.
    var providerKey: String { descriptor.providerKey }

    /// One row per provider key.
    var id: String { providerKey }
}

/// One styled vector base the settings surface offers: a registry
/// descriptor's vector tileset painted with one of the committed styles the
/// same descriptor lists. Identity is the pair — one provider serves several
/// styles — so the choice key names both halves.
struct VectorBaseMapChoice: Identifiable {
    /// The descriptor whose tileset and zoom ceiling the base uses.
    let descriptor: ProviderDescriptor
    /// The committed style the base paints with.
    let style: VectorStyleDescriptor
    /// Whether the choice can install now. False while a credentialed
    /// family's key is not applied: the row is listed with its `Needs
    /// <credential label>` state, exactly as a credential-less raster row is.
    var isServable: Bool = true

    /// Stable identity of this choice (`provider/style`).
    var choiceKey: String { "\(descriptor.providerKey)/\(style.styleId)" }

    /// One row per style of a provider.
    var id: String { choiceKey }

    /// The consumer-facing name the base-map row shows.
    var displayName: String { "\(descriptor.displayName) · \(style.displayName)" }

    /// The attribution line the base must carry: the style's own design line.
    var attributionText: String {
        style.attributionText.trimmingCharacters(
            in: .whitespacesAndNewlines
        ).isEmpty ? descriptor.attributionText : style.attributionText
    }
}

extension ReferenceApplicationModel {
    /// The base maps the settings surface offers, in registry order: every
    /// descriptor that either publishes a `{z}/{x}/{y}` raster URL template
    /// the SDK serves itself, or carries a credential row (the provider's
    /// raster slot comes from its registration, so it is offered before the
    /// key is applied and says plainly that it needs one).
    var baseMapChoices: [BaseMapChoice] {
        providerDescriptors.compactMap { descriptor -> BaseMapChoice? in
            if descriptor.isCredentiallessBaseMapAvailableOnIOS {
                return BaseMapChoice(descriptor: descriptor, isServable: true)
            }
            if descriptor.registrationAvailable.ios,
               !descriptor.credentialLabel.trimmingCharacters(
                   in: .whitespacesAndNewlines
               ).isEmpty {
                return BaseMapChoice(
                    descriptor: descriptor,
                    isServable: appliedProviderCredentials[
                        descriptor.providerKey
                    ] != nil
                )
            }
            return nil
        }
    }

    /// The styled vector base maps the settings surface offers, in
    /// registry order and style order: every descriptor publishing a vector
    /// tileset URL template plus a `vector_styles` list contributes one row
    /// per style, and a provider whose styles come from documents it publishes
    /// contributes its rows too. A credentialed family whose key is not
    /// applied is listed with `isServable = false`, so its row states `Needs
    /// <credential label>` instead of disappearing. The SDK serves these
    /// through the shipped `MapView`'s additive vector base members.
    var vectorBaseMapChoices: [VectorBaseMapChoice] {
        // Keyless families first (the demo's default base among them), then
        // the credential-gated families, each in registry order.
        let keylessDescriptors = providerDescriptors.filter { $0.offersVectorBase }
        let hostedDescriptors = providerDescriptors.filter { descriptor in
            !descriptor.offersVectorBase && descriptor.offersHostedVectorBase
        }
        return (keylessDescriptors + hostedDescriptors).flatMap { descriptor in
            let isServable = descriptor.offersVectorBase
                || appliedProviderCredentials[descriptor.providerKey] != nil
            return descriptor.vectorStyles.map { style in
                VectorBaseMapChoice(
                    descriptor: descriptor,
                    style: style,
                    isServable: isServable
                )
            }
        }
    }

    /// The registry's declared keyless styled base, resolved among the
    /// offered vector choices; nil when the registry declares none or the
    /// declaration names a provider/style
    /// this build does not offer (the keyless start then keeps the
    /// credential-less raster source).
    var keylessVectorBaseChoice: VectorBaseMapChoice? {
        guard let declared = keylessBaseMap else { return nil }
        return vectorBaseMapChoices.first { choice in
            choice.isServable
                && choice.descriptor.providerKey == declared.providerKey
                && choice.style.styleId == declared.styleId
        }
    }

    /// The active styled vector base, or nil while the raster base serves.
    var activeVectorBase: VectorBaseMapChoice? {
        guard let selectedVectorBaseChoiceKey else { return nil }
        return vectorBaseMapChoices.first {
            $0.choiceKey == selectedVectorBaseChoiceKey
        }
    }

    /// The note the base-map section carries while a styled vector base is
    /// active: it names what the styles draw (land, water, roads, Latin place
    /// and road names, and route-reference shield badges where the style
    /// declares them) so the absence of non-Latin text is not read as a
    /// failure.
    var vectorBaseLabelsNote: String? {
        activeVectorBase == nil
            ? nil
            : ReferenceApplicationConfiguration.vectorLabelsNoteText
    }

    /// The styled-vector-base row's act: installs the registry
    /// descriptor's vector tileset with the chosen committed style on the
    /// shipped `MapView`, so every following frame draws the styled base over
    /// the raster tiles and beneath the app's own overlays. A refusal reports
    /// verbatim and leaves the previously serving base in place; the selection
    /// records only a base the SDK accepted.
    func selectVectorBase(choiceKey: String) {
        guard let choice = vectorBaseMapChoices.first(
            where: { $0.choiceKey == choiceKey }
        ) else {
            return
        }
        guard let mapView else {
            providerCredentialStatuses[choice.descriptor.providerKey] =
                .needsCredential(
                    detailText: ReferenceApplicationConfiguration
                        .baseMapViewNotReadyText
                )
            return
        }
        if !choice.style.documentUrl.trimmingCharacters(
            in: .whitespacesAndNewlines
        ).isEmpty {
            // The provider publishes the style: the install fetches it
            // through the provider's authenticated client, so the app never
            // handles the credential.
            Task { @MainActor in
                let accepted = await mapView
                    .setProviderVectorTileSourceFromDocument(
                        ProviderVectorTileSource(
                            providerKey: choice.descriptor.providerKey,
                            styleId: choice.style.styleId
                        ),
                        styleDocumentURL: choice.style.documentUrl
                    )
                if accepted {
                    recordVectorBaseSelection(mapView: mapView, choice: choice)
                }
            }
            return
        }
        let accepted = mapView.setVectorTileSource(
            UrlTemplateVectorTileSource(
                urlTemplate: choice.descriptor.vectorUrlTemplate,
                sourceName: choice.descriptor.providerKey,
                attributionText: choice.attributionText,
                styleId: choice.style.styleId,
                minimumZoomLevel: ReferenceApplicationConfiguration
                    .baseMapMinimumZoomLevel,
                maximumZoomLevel: UInt8(
                    clamping: choice.descriptor.maximumZoomLevel
                )
            )
        )
        guard accepted else {
            // The view reported the refusal through its delegate; the
            // selection keeps the previous base, so this call changes nothing.
            return
        }
        recordVectorBaseSelection(mapView: mapView, choice: choice)
        // Selecting a base map is not a startup: the surface is already
        // presenting, and the pump only draws on demand, so a loading state
        // entered here would have no frame to leave it. The ladder
        // therefore keeps whatever the presented surface earned.
    }

    /// Records one accepted styled-vector-base install on the app's state.
    private func recordVectorBaseSelection(
        mapView: MapNativeCoreUI.MapView,
        choice: VectorBaseMapChoice
    ) {
        selectedVectorBaseChoiceKey = choice.choiceKey
        vectorBaseStatus = mapView.vectorTileSourceStatus()
        if !isApplyingAutomaticBaseMap {
            pendingRememberedVectorBaseChoiceKey = nil
            rememberedBaseMapProviderKey = choice.descriptor.providerKey
            rememberedBaseMapStyleId = choice.style.styleId
        }
        clearErrorReport()
        refreshMapLayersState()
        persistRememberedChoices()
    }

    /// The base map actually serving the map: the user's selection while it
    /// can still serve, otherwise the first choice that can. Nil when nothing
    /// can serve — a keyless session whose registry declares no
    /// credential-less source — and the band then claims no provider at all.
    var activeBaseMap: ProviderDescriptor? {
        // The active styled vector base never claims the raster descriptor's
        // attribution line: `attributionNoticeText` prefers its own line.
        let servableChoices = baseMapChoices.filter { $0.isServable }
        if let selectedProviderKey = selectedBaseMapProviderKey,
           let selectedChoice = servableChoices.first(
               where: { $0.providerKey == selectedProviderKey }
           )
        {
            return selectedChoice.descriptor
        }
        return servableChoices.first?.descriptor
    }

    /// The base-map row's act: registers the chosen source onto the SDK's
    /// raster-tile slot — a credential-less descriptor through the
    /// provider-neutral URL-template registration, a credentialed one by
    /// re-applying its credential — then asks the map view to re-resolve its
    /// provider so the switch shows on the next frame. A refusal reports
    /// verbatim and leaves the previously serving source in place: the SDK
    /// validates the whole declaration before the slot moves, and the
    /// selection only records a source the SDK accepted.
    ///
    /// A row whose provider has not been credentialed yet is not selectable:
    /// the act points at that provider's key field instead, so the reason is
    /// a next step rather than a dead control.
    /// Opens the map on the registry's keyless base map when nothing else is
    /// serving one.
    ///
    /// A keyless run (the hosted demo, a first launch, any build without a
    /// credential) would otherwise draw an empty surface. The choice is
    /// registry-driven and provider-neutral: the first descriptor that declares
    /// a credential-less raster template and is servable on this platform. A
    /// base map the application already selected always wins.
    func selectKeylessBaseMapWhenUncredentialed() {
        guard selectedBaseMapProviderKey == nil else {
            return
        }
        // A credentialed session already has a serving raster slot: its
        // registration installed the provider's tiles and, with them, the
        // tile cache. Selecting the credential-less source here would replace
        // that registration and silently drop the cache (the keyed launch
        // ended on the keyless raster base with `runs uncached` before this
        // guard existed).
        guard appliedProviderCredentials.isEmpty else {
            return
        }
        guard let keylessChoice = baseMapChoices.first(where: { choice in
            choice.isServable
                && !choice.descriptor.rasterUrlTemplate.trimmingCharacters(
                    in: .whitespacesAndNewlines
                ).isEmpty
        }) else {
            return
        }
        // The keyless start is the application's default, not a user choice:
        // it must not overwrite a remembered base map in the record.
        isApplyingAutomaticBaseMap = true
        selectBaseMap(providerKey: keylessChoice.providerKey)
        isApplyingAutomaticBaseMap = false
        // The registry's styled vector base opens the map: the
        // credential-less raster source above stays registered so the surface
        // can attach, and the styled base paints over it once a view is
        // ready. A remembered styled base
        // waits for the same view and wins over this default.
        if pendingRememberedVectorBaseChoiceKey == nil {
            pendingKeylessVectorBaseChoiceKey = keylessVectorBaseChoice?.choiceKey
        }
        applyPendingRememberedVectorBase()
        applyPendingKeylessVectorBase()
    }

    /// Installs the registry's keyless styled base once the shipped map view
    /// exists. The startup path calls this too, because a view that was
    /// already adopted when the keyless selection ran needs no second wait;
    /// whichever call comes second finds nothing pending. An applied
    /// credential or an explicit selection always wins.
    private func applyPendingKeylessVectorBase() {
        guard let pendingChoiceKey = pendingKeylessVectorBaseChoiceKey else {
            return
        }
        guard appliedProviderCredentials.isEmpty,
              selectedVectorBaseChoiceKey == nil
        else {
            pendingKeylessVectorBaseChoiceKey = nil
            return
        }
        // No view yet: keep the default staged for onMapViewReady rather
        // than reporting a not-ready refusal.
        guard mapView != nil else {
            return
        }
        pendingKeylessVectorBaseChoiceKey = nil
        guard vectorBaseMapChoices.contains(where: {
            $0.choiceKey == pendingChoiceKey
        }) else {
            return
        }
        // The keyless default is not a user choice: it must not overwrite a
        // remembered base map in the record.
        isApplyingAutomaticBaseMap = true
        defer { isApplyingAutomaticBaseMap = false }
        selectVectorBase(choiceKey: pendingChoiceKey)
    }

    func selectBaseMap(providerKey: String) {
        guard let choice = baseMapChoices.first(
            where: { $0.providerKey == providerKey }
        ), let mapSdk else {
            return
        }
        guard choice.isServable else {
            providerCredentialStatuses[providerKey] = .needsCredential(
                detailText: ReferenceApplicationConfiguration
                    .credentialEnterHintText
            )
            return
        }
        let descriptor = choice.descriptor
        let isCredentiallessSource = !descriptor.rasterUrlTemplate
            .trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        do {
            if isCredentiallessSource {
                try mapSdk.registerUrlTemplateRasterTileSource(
                    source: UrlTemplateRasterTileSource(
                        urlTemplate: descriptor.rasterUrlTemplate,
                        sourceName: descriptor.providerKey,
                        minimumZoomLevel: ReferenceApplicationConfiguration
                            .baseMapMinimumZoomLevel,
                        maximumZoomLevel: UInt8(
                            clamping: descriptor.maximumZoomLevel
                        ),
                        imageFormat: .png,
                        tileResolution: .standard,
                        userAgent: baseMapUserAgent()
                    )
                )
            } else if let appliedCredential =
                appliedProviderCredentials[descriptor.providerKey]
            {
                // Re-registration replaces every slot, the tile-cache
                // installation with them; the iOS registration path wraps
                // the cache in the same call.
                try registerProviders(
                    forProviderKey: descriptor.providerKey,
                    credential: appliedCredential,
                    on: mapSdk
                )
            } else {
                // A credentialed descriptor is servable only while its
                // credential is applied; an inconsistent choice changes
                // nothing.
                return
            }
        } catch let sdkError as SdkError {
            reportSdkError(sdkError)
            return
        } catch {
            reportLocalError(
                origin: "base map", detail: String(describing: error)
            )
            return
        }
        if isCredentiallessSource {
            // A credential-less source serves uncached: registering it
            // dropped the installation, whose terms belonged to the previous
            // provider. The offline card must say so rather than keep the
            // old promise.
            offlineTileCacheState = nil
            offlineTileCacheUncachedNote = ReferenceApplicationConfiguration
                .baseMapUncachedNoteText
            // The installation went with the provider; the region card states
            // the SDK's own no-cache refusal instead of offering a download.
            Task { [weak self] in
                await self?.offlineRegionState?.refresh()
            }
        }
        // A credential-less source is the session's first provider
        // registration, so the surface SDK is handed to the view here (the
        // keyless session started without one).
        hasRegisteredProviderThisSession = true
        selectedBaseMapProviderKey = descriptor.providerKey
        // A raster selection replaces a styled vector base: the vector base
        // draws over the raster tiles, so leaving it installed would keep
        // painting over the newly selected imagery.
        mapView?.clearVectorTileSource()
        selectedVectorBaseChoiceKey = nil
        vectorBaseStatus = nil
        // An explicit raster choice wins over the keyless styled base; the
        // startup path sets the pending default again after its own call.
        pendingKeylessVectorBaseChoiceKey = nil
        if !isApplyingAutomaticBaseMap {
            pendingRememberedVectorBaseChoiceKey = nil
            rememberedBaseMapProviderKey = descriptor.providerKey
            rememberedBaseMapStyleId = nil
        }
        clearErrorReport()
        if mapView?.sdk == nil {
            mapView?.sdk = surfaceSdk
            setReadiness(
                .loading,
                ReferenceApplicationConfiguration.attachingReadinessText
            )
        }
        // The attached view holds the provider it resolved and keeps its
        // tiles by coordinate: without this the previous imagery would stay
        // on screen until the camera reached new tiles.
        mapView?.reloadTileSource()
        // The raster capability follows the base-map slot; the read-only row
        // states the provider now serving it.
        capabilityProviderState.refresh()
        refreshMapLayersState()
        persistRememberedChoices()
    }

    /// The `User-Agent` a credential-less base map identifies this app with,
    /// formatted from the shared registry's template with the compiled SDK
    /// version. Public tile services (the OpenStreetMap usage policy among
    /// them) require a client that names itself and a way to reach its
    /// operator. Blank when the artifact declares no template — the SDK then
    /// sends no header rather than an invented one.
    private func baseMapUserAgent() -> String {
        let template = referenceAppUserAgentTemplate
        guard !template.trimmingCharacters(
            in: .whitespacesAndNewlines
        ).isEmpty else {
            return ""
        }
        return template.replacingOccurrences(
            of: "%s",
            with: getSdkCompiledVersion()
        )
    }
}

/// The applier's failure when no registration arm exists for a provider
/// key (the artifact may name a provider this application cannot register
/// yet; §6.5's disabled-with-reason rule).
enum ProviderCredentialApplicationError: Swift.Error, LocalizedError {
    /// The provider key has no dispatch arm.
    case unknownProvider(providerKey: String)

    var errorDescription: String? {
        switch self {
        case let .unknownProvider(providerKey):
            return ReferenceApplicationConfiguration.unknownProviderReason(
                providerKey: providerKey
            )
        }
    }
}

// MARK: - The shipped map view (surface, pump, gestures, camera store)

extension ReferenceApplicationModel {
    /// The SDK the shipped view draws from: nil until a provider
    /// registration succeeds in this session, because attaching without a
    /// raster provider is an honest `ProviderNotConfigured` failure — the
    /// keyless session is a neutral state, not a renderer failure (§6.5).
    var surfaceSdk: MapSdk? {
        hasRegisteredProviderThisSession ? mapSdk : nil
    }

    /// Adopts the view the SwiftUI wrapper created: applies the current
    /// camera values, re-pushes the overlays and restores a standing
    /// location fix (a recomposition can recreate the view).
    func onMapViewReady(_ view: MapNativeCoreUI.MapView) {
        mapView = view
        view.delegate = self
        view.sdk = surfaceSdk
        pushCameraImmediately()
        rebuildOverlays()
        restoreLocationIndicatorIfNeeded()
        applyPendingRememberedVectorBase()
        applyPendingKeylessVectorBase()
        refreshMapLayersState()
        // A remembered device-location source starts once the map view is
        // ready; its first accepted fix centres the camera.
        restoreRememberedDeviceLocationIfPermitted()
        // A traffic layer restored before the surface attached gets its
        // first camera settle now; an unchanged pose schedules nothing.
        mapLayersState.noteCameraPoseChanged(
            poseKey: Self.cameraPoseKey(for: cameraValues)
        )
    }

    /// The current camera values as the shipped view's plain pose.
    func currentCameraPose() -> MapCameraPose {
        MapCameraPose(
            latitudeDegrees: cameraValues.centerLatitudeDegrees,
            longitudeDegrees: cameraValues.centerLongitudeDegrees,
            zoomLevel: cameraValues.zoomLevel,
            bearingDegrees: cameraValues.bearingDegrees,
            pitchDegrees: cameraValues.pitchDegrees
        )
    }

    /// Applies the current camera values immediately (no ease).
    private func pushCameraImmediately() {
        mapView?.setCamera(currentCameraPose())
    }

    /// The diagnostics sheet's viewport row (the demo's frame facts).
    var configuredViewportReadoutText: String? {
        guard let mapView, let pixelExtent = mapView.viewportPixelExtent else {
            return nil
        }
        return String(
            format: "surface %d × %d px · drawable scale %.2f",
            Int(pixelExtent.width), Int(pixelExtent.height),
            mapView.displayScale
        )
    }

    /// Re-applies the standing location fix onto a recreated view (§4.2:
    /// a guided fix during a trip, the plain tracking disc otherwise).
    private func restoreLocationIndicatorIfNeeded() {
        guard let lastFixCoordinate else { return }
        applyLocationIndicator(
            coordinate: lastFixCoordinate,
            courseHeadingDegrees: isJourneyActive ? lastFixCourseHeadingDegrees : nil,
            horizontalAccuracyMeters:
                ReferenceApplicationConfiguration.simulatedJourneyFixAccuracyMeters,
            guided: isJourneyActive
        )
    }
}

// MARK: - MapViewDelegate (the shipped view's callbacks)

extension ReferenceApplicationModel: MapViewDelegate {
    /// The first presented frame: the ladder resolves to ready (§6.1) and
    /// an honest startup reports nothing (§6.3).
    func mapViewDidBecomeReady(_ mapView: MapNativeCoreUI.MapView) {
        clearErrorReport()
        isSurfaceAttached = true
        if readinessState != .ready {
            settleReadinessOnPresentedFrame()
        }
    }

    /// Moves the ladder to ready with the full startup line.
    ///
    /// The shipped view reports its first presented frame once per native
    /// view, so an operation that re-enters the loading state afterwards
    /// (selecting another base map) would otherwise leave the chip on the
    /// loading line while the new source serves. The Android shell settles on
    /// every frame; the drawn-frame callback below mirrors that.
    private func settleReadinessOnPresentedFrame() {
        setReadiness(
            .ready,
            String(
                format: ReferenceApplicationConfiguration.readyReadinessFormat,
                getSdkCompiledVersion()
            )
        )
    }

    /// The view adopted a camera: the values follow it exactly (gestures,
    /// presets, fits and the follow camera all read back from here). A
    /// gesture-driven change is one of §5.3's following-breaking gestures.
    func mapView(
        _ mapView: MapNativeCoreUI.MapView,
        didChangeCamera camera: MapCameraPose,
        isGestureDriven: Bool
    ) {
        cameraValues = ReferenceCameraValues(
            centerLatitudeDegrees: camera.latitudeDegrees,
            centerLongitudeDegrees: camera.longitudeDegrees,
            zoomLevel: camera.zoomLevel,
            bearingDegrees: camera.bearingDegrees,
            pitchDegrees: camera.pitchDegrees
        )
        if isGestureDriven {
            breakNavigationFollowFromGesture()
            // A direct camera gesture drops the selected preset's accent
            // (§4: only a preset's own act marks it selected). The record is
            // written once the gesture settles, not on every frame.
            selectedPresetIdentifier = nil
            schedulePersistRememberedChoicesAfterCameraGesture()
        }
        refreshScaleBarReadout()
        // Every camera path (drag, wheel, keyboard, preset, fit, device fix)
        // reaches the traffic layer through the same pose note; the debounce
        // turns a settle into one request.
        mapLayersState.noteCameraPoseChanged(
            poseKey: Self.cameraPoseKey(for: cameraValues)
        )
    }

    /// The traffic layer's settle key: a pose change restarts its debounce.
    static func cameraPoseKey(for cameraValues: ReferenceCameraValues) -> String {
        String(
            format: "%.6f|%.6f|%.3f|%.2f|%.2f",
            cameraValues.centerLatitudeDegrees,
            cameraValues.centerLongitudeDegrees,
            cameraValues.zoomLevel,
            cameraValues.bearingDegrees,
            cameraValues.pitchDegrees
        )
    }

    /// A tap unprojected onto the ground plane. The reference application
    /// counts the confirmed taps the diagnostics sheet reports; the shipped
    /// view's single-tap recognizer waits for the double tap to fail, so a
    /// double tap zooms without also counting as a click.
    func mapView(
        _ mapView: MapNativeCoreUI.MapView,
        didClickCoordinate coordinate: GeographicCoordinate
    ) {
        mapClickCount += 1
    }

    /// One presented frame: its statistics feed the offline card and the
    /// diagnostics block, and the drawn camera's readback updates (§4.1).
    ///
    /// The publication is gated: a frame whose statistics and camera match the
    /// last published pair changes nothing, and a changed frame publishes at
    /// most once per cadence window. The provider-status readback runs only
    /// when the tile statistics changed, so a camera-only frame skips it.
    func mapView(
        _ mapView: MapNativeCoreUI.MapView,
        didDrawFrame statistics: FrameStatistics
    ) {
        let drawnCamera = mapView.camera
        let readback = CameraAttributeReadback(
            centerLatitudeDegrees: drawnCamera.latitudeDegrees,
            centerLongitudeDegrees: drawnCamera.longitudeDegrees,
            zoomLevel: drawnCamera.zoomLevel,
            bearingDegrees: drawnCamera.bearingDegrees,
            pitchDegrees: drawnCamera.pitchDegrees
        )
        guard framePublicationGate.shouldPublish(
            statistics: statistics,
            camera: readback,
            now: ProcessInfo.processInfo.systemUptime
        ) else {
            return
        }
        if errorReport != nil {
            clearErrorReport()
        }
        if !isSurfaceAttached {
            isSurfaceAttached = true
        }
        // A frame that presents is proof the surface works, so a ladder left
        // on loading by a base-map change settles here (see
        // `settleReadinessOnPresentedFrame`); failed and keyless states carry
        // their own meaning and are left alone.
        if readinessState == .loading {
            settleReadinessOnPresentedFrame()
        }
        let statisticsChanged = lastFrameTileStatistics != statistics
        if statisticsChanged {
            lastFrameTileStatistics = statistics
        }
        // The vector base's honest accounting follows the frame, so the
        // base-map section can say when tiles failed and the next frame
        // retries them. Only a changed tile count can change the status.
        if activeVectorBase != nil, statisticsChanged {
            let nextStatus = mapView.vectorTileSourceStatus()
            if vectorBaseStatus != nextStatus {
                vectorBaseStatus = nextStatus
            }
        }
        if cameraAttributeReadback != readback {
            cameraAttributeReadback = readback
        }
        refreshScaleBarReadout()
    }

    /// A refused operation: the failure echoes verbatim (§6.3); a surface
    /// attach failure moves the ladder to failed (§6.1), every other phase
    /// keeps the ready state and reports.
    func mapView(
        _ mapView: MapNativeCoreUI.MapView,
        didFailWith failure: MapViewFailure
    ) {
        if let sdkError = failure.cause as? SdkError {
            reportSdkError(sdkError)
            if failure.phase == .surface {
                setReadiness(
                    .failed,
                    "\(sdkErrorVariantName(sdkError)): \(sdkFailureDetailText(sdkError))"
                )
            }
        } else {
            reportLocalError(
                origin: mapViewFailureOrigin(failure.phase),
                detail: String(describing: failure.cause)
            )
            if failure.phase == .surface {
                setReadiness(.failed, String(describing: failure.cause))
            }
        }
    }

    /// The local-failure label for a wrapper phase (the §6.3 label line
    /// names the origin when no contract variant crossed).
    private func mapViewFailureOrigin(
        _ phase: MapViewFailurePhase
    ) -> String {
        switch phase {
        case .surface: return "map surface attach"
        case .resize: return "map surface resize"
        case .draw: return "map frame draw"
        case .camera: return "map camera"
        case .overlay: return "overlay set"
        case .locationIndicator: return "location indicator"
        case .baseMap: return "vector base map"
        case .tap: return "map tap"
        }
    }
}

// MARK: - Camera, presets and fit bounds

extension ReferenceApplicationModel {
    /// Applies a camera interaction from the control cluster (the §5.1
    /// division: the platform owns gesture plumbing, the shipped view owns
    /// the camera store — the change is pushed immediately, and the view's
    /// delegate reports the adopted pose back).
    private func applyDirectCameraChange(
        _ mutateValues: (inout ReferenceCameraValues) -> Void
    ) {
        var changedValues = cameraValues
        mutateValues(&changedValues)
        cameraValues = changedValues
        selectedPresetIdentifier = nil
        pushCameraImmediately()
        persistRememberedChoices()
    }

    /// Drag-pan glue (§5.1): the drag delta crosses the shipped view's
    /// contract camera `panByScreenPixels` member — the
    /// content-follows-pointer convention: the ground point under the
    /// finger at gesture start lands under the finger after the drag, so
    /// dragging right moves the map content **right**. The member drops the
    /// whole delta when its ray misses the ground (the seam's whole-delta
    /// rule). The gesture breaks the follow camera's writes until the next
    /// fix (§5.3).
    func userDraggedMapPixels(_ dragDeltaPixels: CGSize) {
        breakNavigationFollowFromGesture()
        mapView?.panByScreenPixels(
            deltaXPixels: dragDeltaPixels.width,
            deltaYPixels: dragDeltaPixels.height
        )
    }

    /// Applies one of the shared artifact's camera presets (§9: the
    /// presets feed the preset picker; preset cameras apply instantly —
    /// the animated `flyTo` surface is a documented M3 open item).
    func applyCameraPreset(_ preset: ReferenceSceneCameraPreset) {
        cameraValues = ReferenceCameraValues(
            centerLatitudeDegrees: preset.latitudeDegrees,
            centerLongitudeDegrees: preset.longitudeDegrees,
            zoomLevel: preset.zoomLevel,
            bearingDegrees: preset.bearingDegrees,
            pitchDegrees: preset.pitchDegrees
        )
        selectedPresetIdentifier = preset.id
        pushCameraImmediately()
        persistRememberedChoices()
    }

    /// Frames the planned route through the shipped view's `fitBounds`
    /// (§5.1 camera fitting): the view adopts the fitted camera and
    /// reports its exact center, zoom and top-down/north-up framing back
    /// through the delegate (no recovery through projection).
    func fitCameraToRoute(route plannedRouteData: Route) {
        guard let mapView else { return }
        let insetPixels = ReferenceApplicationConfiguration.routePreviewEdgeInsetPoints
            * mapView.displayScale
        mapView.fitBounds(
            boundingBox: plannedRouteData.boundingBox,
            edgeInsets: EdgeInsets(
                left: insetPixels,
                top: insetPixels,
                right: insetPixels,
                bottom: insetPixels
            ),
            maximumZoomLevel: ReferenceApplicationConfiguration.demoMaximumTileZoomLevel
        )
        selectedPresetIdentifier = nil
        persistRememberedChoices()
    }
}

/// The whole-number formatting of a Double for the numeric readout columns
/// (the design language's monospaced numeric stack).
extension Double {
    func decimalReadout(_ fractionDigits: Int) -> String {
        String(format: "%.\(fractionDigits)f", self)
    }
}

// MARK: - Plan flow (§4.1: search → route → confirm)

extension ReferenceApplicationModel {
    /// The origin field's display text when untouched (the demo position
    /// is its §4.1 placeholder, resolved at load time).
    var originFieldPlaceholderText: String? {
        ReferenceApplicationConfiguration.originFieldHintText
    }

    /// The plan search, run for the assignment target's field (the one
    /// shared result list under both fields). A keyless session filters the
    /// shared artifact's demo places by the typed text — never a provider
    /// call, never an invented result — and the list says it is demo
    /// content. A keyed session asks the provider; failures echo verbatim
    /// (§6.3).
    func runSearch() async {
        let searchQueryText = searchFieldAssignmentTarget == .destination
            ? destinationSearchText
            : originSearchText
        guard !searchQueryText.isEmpty else {
            reportLocalError(
                origin: "search",
                detail: "the search field is empty; type a place to geocode"
            )
            return
        }
        lastSearchQueryText = searchQueryText
        persistRememberedChoices()
        if !hasAppliedProviderCredential {
            searchDemoPlaces(queryText: searchQueryText)
            return
        }
        guard mapSdk != nil else {
            reportLocalError(
                origin: "search",
                detail: "the SDK is not running; the readiness ladder states why"
            )
            return
        }
        // A newer search supersedes the one in flight: cancel it and wait for
        // its FFI call to return before starting the next, so searches never
        // overlap and a superseded result can never publish.
        if let previousSearch = planSearchTask {
            previousSearch.cancel()
            await previousSearch.value
        }
        let searchTask = Task { @MainActor [weak self] in
            guard let self else { return }
            await self.performProviderSearch(queryText: searchQueryText)
        }
        planSearchTask = searchTask
        await searchTask.value
    }

    /// Runs one provider geocode request and publishes its result only if the
    /// task was not superseded (cancelled) while it awaited the SDK.
    private func performProviderSearch(queryText: String) async {
        guard let mapSdk else {
            reportLocalError(
                origin: "search",
                detail: "the SDK is not running; the readiness ladder states why"
            )
            return
        }
        isSearchRunning = true
        searchResultsAreDemoPlaces = false
        keylessSearchNoMatchText = nil
        defer { isSearchRunning = false }
        do {
            let foundPlaces = try await mapSdk.forwardGeocode(addressText: queryText)
            guard !Task.isCancelled else { return }
            searchResults = foundPlaces
            clearErrorReport()
            if foundPlaces.isEmpty {
                reportLocalError(
                    origin: "search",
                    detail: "the geocoder returned no results for '\(queryText)'"
                )
            }
        } catch let sdkError as SdkError {
            guard !Task.isCancelled else { return }
            reportSdkError(sdkError)
        } catch {
            guard !Task.isCancelled else { return }
            reportLocalError(origin: "search", detail: String(describing: error))
        }
    }

    /// Filters the artifact's demo places by the typed text (the keyless
    /// search): the matched rows are demo places, and a query the artifact
    /// cannot answer says so honestly instead of failing silently.
    private func searchDemoPlaces(queryText: String) {
        guard let catalog else {
            reportLocalError(
                origin: "search",
                detail: "the shared demo artifact is not loaded"
            )
            return
        }
        let queryLowercase = queryText.lowercased()
        let matchingDemoPlaces = catalog.demoPlaces.filter { demoPlace in
            demoPlace.name.lowercased().contains(queryLowercase)
        }
        searchResults = matchingDemoPlaces.map(placeFromDemoPlace)
        searchResultsAreDemoPlaces = true
        keylessSearchNoMatchText = matchingDemoPlaces.isEmpty
            ? String(
                format: ReferenceApplicationConfiguration.demoNoMatchHintFormat,
                searchFieldLabel(for: searchFieldAssignmentTarget)
            )
            : nil
        clearErrorReport()
    }

    /// The consumer field word a no-match line names.
    private func searchFieldLabel(for target: SearchFieldAssignmentTarget) -> String {
        target == .destination ? "destination" : "origin"
    }

    /// One artifact demo place as a result row: its name is the display text
    /// and its coordinate the resolved position. The quality is `.point`
    /// because a demo place is a named feature point, never a geocoded
    /// street address.
    private func placeFromDemoPlace(_ demoPlace: ReferenceSceneDemoPlace) -> Place {
        Place(
            structuredAddressText: demoPlace.name,
            coordinate: demoPlace.coordinate,
            geocodeQuality: .point
        )
    }

    /// The artifact demo place whose name matches the text exactly
    /// (case-insensitive).
    private func demoPlaceNamed(_ placeNameText: String) -> ReferenceSceneDemoPlace? {
        let trimmedText = placeNameText.trimmingCharacters(in: .whitespacesAndNewlines)
        return catalog?.demoPlaces.first { demoPlace in
            demoPlace.name.compare(trimmedText, options: .caseInsensitive) == .orderedSame
        }
    }

    /// Assigns a provider result to the assignment target's field. The
    /// assignment fills the field and never plans by itself: the explicit
    /// Plan route action is the only path that plans (§4.1).
    func assignSearchResult(_ placeResult: Place) {
        switch searchFieldAssignmentTarget {
        case .destination:
            destinationTypeAheadController.clear()
            destinationCoordinate = placeResult.coordinate
            destinationAssignedText = placeResult.structuredAddressText
            destinationSearchText = placeResult.structuredAddressText
        case .origin:
            originTypeAheadController.clear()
            originCoordinate = placeResult.coordinate
            originAssignedText = placeResult.structuredAddressText
            originSearchText = placeResult.structuredAddressText
        }
        clearErrorReport()
    }

    /// Edits the origin field's text; editing away from the assigned place
    /// clears the assignment, so Plan route re-resolves from the text.
    func updateOriginSearchText(_ searchText: String) {
        originSearchText = searchText
        mapLayersState.clearMyLocationFill(for: .origin)
        if searchText != originAssignedText {
            originCoordinate = nil
        }
        originTypeAheadController.queryChanged(searchText)
    }

    /// Edits the destination field's text (see `updateOriginSearchText`).
    func updateDestinationSearchText(_ searchText: String) {
        destinationSearchText = searchText
        mapLayersState.clearMyLocationFill(for: .destination)
        if searchText != destinationAssignedText {
            destinationCoordinate = nil
        }
        destinationTypeAheadController.queryChanged(searchText)
    }

    /// Whether Plan route is available: both fields hold text (§4.1).
    var canPlanRoute: Bool {
        !originSearchText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            && !destinationSearchText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    /// The disabled-with-reason text under Plan route, empty when enabled.
    var planRouteReasonText: String {
        let originIsEmpty = originSearchText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        let destinationIsEmpty =
            destinationSearchText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        switch (originIsEmpty, destinationIsEmpty) {
        case (true, true):
            return ReferenceApplicationConfiguration.planMissingBothText
        case (true, false):
            return ReferenceApplicationConfiguration.planMissingOriginText
        case (false, true):
            return ReferenceApplicationConfiguration.planMissingDestinationText
        case (false, false): return ""
        }
    }

    /// The swap action (`plan.swap_action`): exchanges the two fields and
    /// everything resolved from them, so correcting a reversed pair never
    /// means retyping both. Assignment never plans.
    func swapPlanFields() {
        swap(&originSearchText, &destinationSearchText)
        swap(&originCoordinate, &destinationCoordinate)
        swap(&originAssignedText, &destinationAssignedText)
        mapLayersState.swapMyLocationFills()
        originTypeAheadController.clear()
        destinationTypeAheadController.clear()
        clearErrorReport()
    }

    /// The explicit Plan route action (§4.1): resolves both fields — the
    /// assigned place when the field still holds its text, otherwise the
    /// provider's first result for the typed text — then plans through the
    /// two coordinates; a field that resolves to nothing refuses with a
    /// reason naming it.
    func planRouteFromFields() async {
        guard canPlanRoute, !isRouteCalculationRunning else { return }
        do {
            guard try await resolvePlanField(for: .origin) else {
                reportUnresolvedPlanField(.origin)
                return
            }
            guard try await resolvePlanField(for: .destination) else {
                reportUnresolvedPlanField(.destination)
                return
            }
        } catch let sdkError as SdkError {
            reportSdkError(sdkError)
            return
        } catch {
            reportLocalError(origin: "search", detail: String(describing: error))
            return
        }
        if !hasAppliedProviderCredential
            && !capabilityProviderState.hasRegisteredRoutingProvider
        {
            // A keyless session with no routing provider plans the artifact's
            // recorded route for the pair — never a provider call, never
            // invented geometry. A pair the artifact does not record refuses
            // with a named reason. Selecting a keyless routing provider (its
            // public demo deployment or an automation fixture server)
            // registers one, so the same action then plans through that
            // provider.
            let originName = originAssignedText ?? originSearchText
            let destinationName = destinationAssignedText ?? destinationSearchText
            guard let recordedRoute = recordedDemoRoute(
                originName: originName,
                destinationName: destinationName
            ) else {
                reportLocalError(
                    origin: "route planning",
                    detail: String(
                        format: ReferenceApplicationConfiguration
                            .noRecordedRouteHintFormat,
                        originName,
                        destinationName
                    )
                )
                return
            }
            applyRecordedDemoRoute(recordedRoute)
            // The recorded route carries its own recorded profile; the
            // selected choices are what the live plan action would send, and
            // the diagnostics state that honestly.
            lastPlannedRouteOptions = currentRouteOptions()
            lastPlanUsedProvider = false
            return
        }
        await planRoute()
    }

    /// Resolves one plan field from its text when no assignment stands;
    /// answers whether the field now holds a coordinate. An exact demo-place
    /// name resolves without a provider, keyless or not (the web planner's
    /// same rule).
    private func resolvePlanField(for target: SearchFieldAssignmentTarget) async throws -> Bool {
        switch target {
        case .origin:
            if originCoordinate != nil { return true }
        case .destination:
            if destinationCoordinate != nil { return true }
        }
        let queryText = (target == .origin ? originSearchText : destinationSearchText)
            .trimmingCharacters(in: .whitespacesAndNewlines)
        guard !queryText.isEmpty else { return false }
        if let demoPlace = demoPlaceNamed(queryText) {
            assignResolvedPlanField(
                target,
                coordinate: demoPlace.coordinate,
                displayText: demoPlace.name
            )
            return true
        }
        guard hasAppliedProviderCredential, let mapSdk else { return false }
        let results = try await mapSdk.forwardGeocode(addressText: queryText)
        guard let firstResult = results.first else { return false }
        assignResolvedPlanField(
            target,
            coordinate: firstResult.coordinate,
            displayText: firstResult.structuredAddressText
        )
        return true
    }

    /// Reports a plan field that resolved to no place, naming the field. A
    /// keyless refusal points at the demo places and the key instead of
    /// claiming a provider looked and found nothing.
    private func reportUnresolvedPlanField(_ target: SearchFieldAssignmentTarget) {
        let fieldLabel = searchFieldLabel(for: target)
        let detailText = hasAppliedProviderCredential
            ? "the provider found no place for the \(fieldLabel)"
            : String(
                format: ReferenceApplicationConfiguration.demoNoMatchHintFormat,
                fieldLabel
            )
        reportLocalError(origin: "route planning", detail: detailText)
    }

    /// Stores one resolved route endpoint in its field and its field's text.
    private func assignResolvedPlanField(
        _ target: SearchFieldAssignmentTarget,
        coordinate: GeographicCoordinate,
        displayText: String
    ) {
        switch target {
        case .origin:
            originCoordinate = coordinate
            originAssignedText = displayText
            originSearchText = displayText
        case .destination:
            destinationCoordinate = coordinate
            destinationAssignedText = displayText
            destinationSearchText = displayText
        }
    }

    /// The artifact's recorded route for the ordered demo-place pair, or nil.
    private func recordedDemoRoute(
        originName: String,
        destinationName: String
    ) -> ReferenceSceneDemoRoute? {
        catalog?.demoRoutes.first { demoRoute in
            demoRoute.originName == originName
                && demoRoute.destinationName == destinationName
        }
    }

    /// Applies one recorded demo route as the planned route: the artifact's
    /// geometry feeds the overlay line, the summary card and the simulated
    /// trip exactly as a live plan does — no provider call.
    private func applyRecordedDemoRoute(_ recordedRoute: ReferenceSceneDemoRoute) {
        let contractRoute = recordedRoute.route.contractRoute
        plannedRoute = contractRoute
        plannedRouteIsRecordedDemoRoute = true
        plannedRouteIsOfflineArchiveRoute = false
        rebuildOverlays()
        fitCameraToRoute(route: contractRoute)
        clearErrorReport()
        homeSheetState = .half
    }

    /// The offline card's route control (§2.5 `offline.plan_route_action`):
    /// reads the committed compiled routing archive from the application
    /// bundle and plans the demo request over it through the contract's
    /// `offlineRouteFromArchiveBytes` free function. The route feeds the same
    /// summary card, overlay line and simulated trip as a live plan, and the
    /// summary states that it is a recorded test graph, not live data. No
    /// provider and no network round trip are involved.
    func planOfflineRouteFromArchive() async {
        guard !isRouteCalculationRunning else { return }
        guard let archiveURL = Bundle.main.url(
            forResource: ReferenceApplicationConfiguration.offlineRoutingArchiveResourceName,
            withExtension: ReferenceApplicationConfiguration.offlineRoutingArchiveResourceExtension
        ) else {
            reportLocalError(
                origin: "offline route planning",
                detail: "the bundled offline routing archive is missing from the application bundle"
            )
            return
        }
        isRouteCalculationRunning = true
        defer { isRouteCalculationRunning = false }
        do {
            let archiveBytes = try Data(contentsOf: archiveURL)
            let offlineRoute = try await offlineRouteFromArchiveBytes(
                archiveBytes: archiveBytes,
                start: GeographicCoordinate(
                    latitude: ReferenceApplicationConfiguration.offlineRouteStartLatitudeDegrees,
                    longitude: ReferenceApplicationConfiguration.offlineRouteStartLongitudeDegrees
                ),
                end: GeographicCoordinate(
                    latitude: ReferenceApplicationConfiguration.offlineRouteEndLatitudeDegrees,
                    longitude: ReferenceApplicationConfiguration.offlineRouteEndLongitudeDegrees
                )
            )
            plannedRoute = offlineRoute
            plannedRouteIsRecordedDemoRoute = false
            plannedRouteIsOfflineArchiveRoute = true
            rebuildOverlays()
            fitCameraToRoute(route: offlineRoute)
            clearErrorReport()
            homeSheetState = .half
        } catch let sdkError as SdkError {
            // A refused archive (invalid bytes, an out-of-graph coordinate, a
            // disconnected pair) reports its verbatim detail (§6.3).
            reportSdkError(sdkError)
        } catch {
            reportLocalError(
                origin: "offline route planning",
                detail: String(describing: error)
            )
        }
    }

    /// The launch-argument planning demo's origin: the artifact's demo
    /// position, applied programmatically (never a UI default), then the
    /// normal explicit plan runs.
    func planRouteFromDemoPosition() async {
        guard let demoPositionCoordinate else { return }
        originCoordinate = demoPositionCoordinate
        originAssignedText = "Demo position"
        originSearchText = "Demo position"
        await planRoute()
    }

    /// Plans the route from the origin coordinate to the destination over
    /// the async provider surface with the Directions control's selected
    /// mode and strategy, draws the route polyline over the map surface,
    /// frames it with the SDK fit-bounds camera, and folds the sheet back
    /// to half with the route summary card.
    func planRoute() async {
        guard let mapSdk else {
            reportLocalError(
                origin: "route planning",
                detail: "the SDK is not running; the readiness ladder states why"
            )
            return
        }
        guard let originCoordinate, let destinationCoordinate else {
            reportLocalError(
                origin: "route planning",
                detail: "no origin or destination assigned yet; assign one result to each field"
            )
            return
        }
        isRouteCalculationRunning = true
        defer { isRouteCalculationRunning = false }
        let waypoints = [originCoordinate, destinationCoordinate]
        let routeOptions = currentRouteOptions()
        do {
            let plannedRouteData = try await mapSdk.calculateRoute(
                waypoints: waypoints, routeOptions: routeOptions
            )
            // The plan that served carries exactly these options.
            lastPlannedRouteOptions = routeOptions
            lastPlanUsedProvider = true
            plannedRoute = plannedRouteData
            plannedRouteIsRecordedDemoRoute = false
            plannedRouteIsOfflineArchiveRoute = false
            rebuildOverlays()
            fitCameraToRoute(route: plannedRouteData)
            clearErrorReport()
            homeSheetState = .half
        } catch let sdkError as SdkError {
            reportRoutingPlanError(sdkError)
        } catch {
            reportLocalError(origin: "route planning", detail: String(describing: error))
        }
    }

    /// Echoes a route-planning failure. A routing provider that could not be
    /// reached is reported verbatim with the documented next step (retry, or
    /// enter a self-hosted deployment address); the previous provider keeps
    /// serving and no fallback or recorded route replaces the failure.
    private func reportRoutingPlanError(_ sdkError: SdkError) {
        let isNetworkFailure = sdkErrorVariantName(sdkError) == "NetworkRequestFailed"
        let detailText = sdkFailureDetailText(sdkError)
            + (isNetworkFailure
                ? " "
                    + ReferenceApplicationConfiguration
                        .routingUnreachableNextStepText
                : "")
        errorReport = ReferenceApplicationErrorReport(
            labelLine: sdkErrorVariantName(sdkError),
            detailText: detailText
        )
    }
}

// MARK: - Type-ahead (docs/ReferenceAppUX.md §5.5)

extension ReferenceApplicationModel {
    /// Builds one field's controller over the running SDK's suggest call.
    private func makeTypeAheadController(
        assignmentTarget: SearchFieldAssignmentTarget
    ) -> SearchTypeAheadController {
        let controller = SearchTypeAheadController(
            suggest: { [weak self] queryText in
                guard let self, let sessionSdk = self.mapSdk else {
                    throw SearchTypeAheadError.sdkUnavailable
                }
                return try await sessionSdk.suggest(
                    queryText: queryText,
                    searchBoundary: nil
                )
            },
            failureText: { [weak self] error in
                guard let self else {
                    return SearchTypeAhead.failureSentence(
                        detail: String(describing: error)
                    )
                }
                return SearchTypeAhead.failureSentence(
                    detail: self.capabilityProviderFailureText(error)
                )
            }
        )
        controller.onSelect = { [weak self] place in
            self?.assignTypeAheadSuggestion(place, to: assignmentTarget)
        }
        return controller
    }

    /// Fills one field from a chosen suggestion exactly as its submitted
    /// text would: the display line becomes the field's text and its
    /// coordinate becomes the resolved endpoint, so the explicit Plan route
    /// action proceeds without a second lookup when the provider supplied
    /// one.
    func assignTypeAheadSuggestion(
        _ place: Place,
        to target: SearchFieldAssignmentTarget
    ) {
        searchFieldAssignmentTarget = target
        switch target {
        case .destination:
            destinationCoordinate = place.coordinate
            destinationAssignedText = place.structuredAddressText
            destinationSearchText = place.structuredAddressText
        case .origin:
            originCoordinate = place.coordinate
            originAssignedText = place.structuredAddressText
            originSearchText = place.structuredAddressText
        }
        clearErrorReport()
    }

    /// Installs or re-reads the provider that serves type-ahead: the SDK's
    /// registered place-search provider when one serves (the credentialed
    /// provider after a credential is applied), otherwise the shared
    /// registry's keyless provider. Provider names never appear here; the
    /// registry descriptor and the SDK readback are the only sources.
    func configureTypeAheadProvider() {
        guard let sessionSdk = mapSdk else { return }
        let choice = SearchTypeAheadProviderChooser.resolve(
            descriptors: providerDescriptors,
            appliedCredentialKeys: Set(appliedProviderCredentials.keys),
            registrations: sessionSdk.providerRegistrations(),
            deploymentBaseURLOverride: ReferenceApplicationConfiguration
                .typeAheadDeploymentBaseURLOverride()
        )
        if case let .register(descriptor, deploymentBaseURL) = choice {
            do {
                try sessionSdk.configureProvider(
                    contract: .placeSearch,
                    providerKey: descriptor.providerKey,
                    credential: deploymentBaseURL
                )
            } catch {
                reportLocalError(
                    origin: "type-ahead",
                    detail: capabilityProviderFailureText(error)
                )
            }
        }
        // A provider change supersedes every pending request and answer.
        originTypeAheadController.clear()
        destinationTypeAheadController.clear()
    }
}

// MARK: - Overlay scenes

extension ReferenceApplicationModel {
    /// The overlay descriptors currently in force: the enabled demo scenes
    /// from the shared artifact, plus the planned route polyline, plus the
    /// destination marker.
    private var activeOverlayDescriptors: [MapOverlay] {
        var descriptors: [MapOverlay] = []
        // The traffic layer draws over the base and under the route line,
        // the same stacking order the browser demo applies.
        if !trafficFlowSegments.isEmpty {
            descriptors.append(
                MapOverlay(
                    kind: .trafficFlow,
                    coordinates: nil,
                    rgbaColor: nil,
                    widthPixels: MapLayersPanelState.trafficFlowOverlayWidthPixels,
                    ringCoordinates: nil,
                    pointMarkerPositions: nil,
                    pointMarkerRadiusPixels: nil,
                    trafficFlowSegments: trafficFlowSegments
                )
            )
        }
        if let catalog {
            for sceneKey in catalog.overlayScenes.keys.sorted() where enabledSceneKeys.contains(sceneKey) {
                guard let scene = catalog.overlayScenes[sceneKey] else { continue }
                do {
                    descriptors.append(try scene.sdkOverlayDescriptor())
                } catch {
                    reportLocalError(
                        origin: "demo scene '\(sceneKey)'",
                        detail: error.localizedDescription
                    )
                }
            }
        }
        if let plannedRoute {
            descriptors.append(routePolylineDescriptor(forRoute: plannedRoute))
        } else if isDemoPlanRouteLineApplied, let selectedDemoPlan {
            // The selected plan's recorded route line rides the overlay
            // push behind the planned route's own line.
            descriptors.append(
                routePolylineDescriptor(forRoute: selectedDemoPlan.journey.routeData)
            )
        }
        if let plannedRoute {
            descriptors.append(contentsOf: routeEndpointPinDescriptors(forRoute: plannedRoute))
        }
        return descriptors
    }

    /// The planned route's A/B endpoint pins (§4.1): origin green A,
    /// destination red B, each a teardrop anchored at its tip through the
    /// additive `PointPins` overlay kind. The colors mirror the web planner's
    /// and the Android refapp's named values.
    private func routeEndpointPinDescriptors(forRoute plannedRouteData: Route) -> [MapOverlay] {
        guard let originCoordinate = plannedRouteData.legs.first?.steps.first?.shape.first,
              let destinationCoordinate = plannedRouteData.legs.last?.steps.last?.shape.last
        else { return [] }
        return [
            endpointPinDescriptor(
                anchorCoordinate: originCoordinate,
                pinColor: OverlayColor(red: 0.13, green: 0.62, blue: 0.29, alpha: 1.0)
            ),
            endpointPinDescriptor(
                anchorCoordinate: destinationCoordinate,
                pinColor: OverlayColor(red: 0.85, green: 0.16, blue: 0.20, alpha: 1.0)
            ),
        ]
    }

    /// One endpoint pin descriptor at the documented default head radius.
    private func endpointPinDescriptor(
        anchorCoordinate: GeographicCoordinate,
        pinColor: OverlayColor
    ) -> MapOverlay {
        MapOverlay(
            kind: .pointPins,
            coordinates: nil,
            rgbaColor: pinColor,
            widthPixels: nil,
            ringCoordinates: nil,
            pointMarkerPositions: [anchorCoordinate],
            pointMarkerRadiusPixels: nil,
            trafficFlowSegments: nil
        )
    }

    /// The planned route's polyline descriptor: the traversal-order shape
    /// across every leg and step (consecutive equal coordinates — the leg
    /// end / next leg start duplicates — skip their degenerate segments,
    /// which the SDK's polyline conversion tolerates too).
    private func routePolylineDescriptor(forRoute plannedRouteData: Route) -> MapOverlay {
        var routeShapeCoordinates: [GeographicCoordinate] = []
        for routeLeg in plannedRouteData.legs {
            for routeStep in routeLeg.steps {
                routeShapeCoordinates.append(contentsOf: routeStep.shape)
            }
        }
        return MapOverlay(
            kind: .routePolyline,
            coordinates: routeShapeCoordinates,
            rgbaColor: nil,
            widthPixels: nil,
            ringCoordinates: nil,
            pointMarkerPositions: nil,
            pointMarkerRadiusPixels: nil,
            trafficFlowSegments: nil
        )
    }

    /// Rebuilds and applies the whole overlay list (the all-or-nothing
    /// semantics live on the SDK side; a failure keeps the previous list
    /// in force and echoes here).
    func rebuildOverlays() {
        guard let mapView else { return }
        // The shipped view reports a refused push through its delegate;
        // the all-or-nothing semantics keep the previous list in force.
        if mapView.setOverlays(activeOverlayDescriptors) {
            clearErrorReport()
        }
    }

    /// Selects a demo plan in the half sheet's picker (§4.1): the
    /// selection drops a planned route, applies the §5.1 route-preview
    /// framing over the plan's recorded route (the shipped view's
    /// `fitBounds`) and pushes the plan's route line onto the overlay list —
    /// the chip act's live planned-route demo with the recorded fixture as
    /// the route, no live provider call.
    func selectDemoPlan(_ demoPlan: ReferenceDemoPlan) {
        selectedDemoPlan = demoPlan
        rememberedDemoPlanScenarioName = demoPlan.scenarioName
        plannedRoute = nil
        plannedRouteIsRecordedDemoRoute = false
        plannedRouteIsOfflineArchiveRoute = false
        isDemoPlanRouteLineApplied = true
        fitCameraToRoute(route: demoPlan.journey.routeData)
        rebuildOverlays()
        persistRememberedChoices()
    }

    /// Toggles one demo scene and reapplies the list.
    func toggleScene(sceneKey: String, isEnabled: Bool) {
        if isEnabled {
            enabledSceneKeys.insert(sceneKey)
        } else {
            enabledSceneKeys.remove(sceneKey)
        }
        rebuildOverlays()
        persistRememberedChoices()
    }

    /// Clears every overlay (the scene carousel's clear action).
    func clearOverlays() {
        guard let mapView else { return }
        mapView.clearOverlays()
        enabledSceneKeys.removeAll()
        clearErrorReport()
        persistRememberedChoices()
    }
}

// MARK: - Simulated turn-by-turn journeys (docs/ReferenceAppUX.md §7, §4.2)

extension ReferenceApplicationModel {
    /// Starts the simulated navigation on the planned route (the plan
    /// flow's primary, §4.1: "Start simulated navigation"): the trip runs
    /// over the planned route's own shape with the live provider's steps,
    /// distances and payload channels.
    func startPlannedRouteNavigation() {
        guard let plannedRoute else { return }
        startJourney(replayPlan: JourneyReplayPlan.fromPlannedRoute(plannedRoute))
    }

    /// Starts a journey that follows a route's own polyline through the
    /// contract's `RouteSimulator` (the planned-route path; the launch
    /// automation drives it from a committed route so the walk is testable
    /// without the network).
    func startRouteFollowingJourney(routeData: Route) {
        startJourney(replayPlan: JourneyReplayPlan.fromPlannedRoute(routeData))
    }

    /// Starts the sample journey over the recorded trace fixture
    /// (§9: the artifact's navigation demo section names the trace).
    func startSampleJourney() {
        guard let recordedJourney else { return }
        startJourney(replayPlan: JourneyReplayPlan.fromRecordedTrace(recordedJourney))
    }

    /// Starts the simulated journey: the planned route drives the trip
    /// when one was planned (the search → route → navigate path), the
    /// demo-plan picker's selected recorded trace otherwise (§4.1).
    func startSimulatedJourney() {
        if plannedRoute != nil {
            startPlannedRouteNavigation()
        } else if let selectedDemoPlan {
            startJourney(replayPlan: JourneyReplayPlan.fromRecordedTrace(selectedDemoPlan.journey))
        } else {
            startSampleJourney()
        }
    }

    /// Starts a journey on a replay plan: the controller subscribes the
    /// four listener topics (instructions, phases, deviations, and the
    /// speech triggers), starts the trip with the contract's
    /// default policies, and the replay timer feeds the plan's fixes at
    /// the accelerated demo cadence. The route draws as an overlay; the
    /// card store seeds from the read-only queries (§7.2) before the
    /// topics take over; the sheet folds away for the navigation drawer;
    /// the §5.3 follow camera engages (fixed deep zoom, course-follow
    /// bearing, the §2.4 pitch) and restores the pre-trip camera after.
    ///
    /// `feedsDeviceFixes` starts the same trip without a walker or replay
    /// timer: the device location source supplies every fix, and the
    /// caller feeds them through `deliverJourneyFix`. One trip runs at a
    /// time, so either mode replaces the other.
    private func startJourney(
        replayPlan: JourneyReplayPlan,
        feedsDeviceFixes: Bool = false
    ) {
        if feedsDeviceFixes {
            // A device-driven trip ends the simulated one; the source it
            // needs was started by its caller first.
            if isJourneyActive {
                finishSimulatedJourney()
            }
        } else {
            // Starting the simulated trip ends a device-driven one; a
            // location source the visitor started by hand keeps running.
            stopDeviceNavigation()
        }
        guard navigationController == nil else { return }
        // One walker per route-following trip. It is built before the
        // controller starts, so a refused pace or a route with no usable
        // geometry never leaves a half-started trip behind. A
        // device-driven trip has no walker: its fixes come from the device.
        let journeySimulator: RouteSimulator?
        if replayPlan.followsRouteGeometry && !feedsDeviceFixes {
            do {
                journeySimulator = try makeRouteSimulator(
                    route: replayPlan.routeData,
                    speedMultiplier: journeySpeedMultiplier
                )
            } catch let sdkError as SdkError {
                reportSdkError(sdkError)
                return
            } catch {
                reportLocalError(
                    origin: "simulated journey",
                    detail: String(describing: error)
                )
                return
            }
        } else {
            journeySimulator = nil
        }
        let journeyController = Navigator()
        let tripListener = MainActorNavigationStateListener(
            cardStore: navigationCardStore,
            tripStateRefresh: { [weak self] in
                self?.navigationController?.currentTripState()
            },
            speechTriggerHandler: { [weak self] speechInstruction in
                self?.handleSpeechInstruction(speechInstruction)
            }
        )
        topicSubscriptions = [
            journeyController.subscribeToNavigationInstructions(listener: tripListener),
            journeyController.subscribeToPhaseChanges(listener: tripListener),
            journeyController.subscribeToDeviationStatus(listener: tripListener),
            journeyController.subscribeToSpeechInstructions(listener: tripListener),
        ]
        do {
            try journeyController.startNavigation(
                route: replayPlan.routeData,
                unitSystem: .metric,
                stepAdvancePolicy: defaultStepAdvancePolicy(),
                deviationPolicy: defaultDeviationPolicy()
            )
        } catch let sdkError as SdkError {
            // The topics are already subscribed to a controller that never
            // starts; cancel them before reporting (a stopped controller
            // must not keep its subscriptions).
            cancelTopicSubscriptions()
            reportSdkError(sdkError)
            return
        } catch {
            cancelTopicSubscriptions()
            reportLocalError(origin: "simulated journey", detail: String(describing: error))
            return
        }
        navigationController = journeyController
        journeyReplayPlan = replayPlan
        routeSimulator = journeySimulator
        journeySeekableDistanceMeters =
            journeySimulator?.totalDistanceMeters() ?? 0
        isJourneySeekable = journeySimulator != nil
        elapsedSimulatedSeconds = 0
        journeyTravelledMeters = 0
        hasDeliveredRouteWalkStartFix = false
        isJourneyPaused = false
        hasSimulatedJourneyArrived = false
        navigationCardStore.seed(
            fromInstruction: journeyController.currentInstructions(),
            tripState: journeyController.currentTripState()
        )
        rebuildJourneyRouteOverlay()
        // The §5.3 follow camera engages at trip start (the §4.1 Navigate
        // flow's fit-bounds framing is the pre-trip state, snapshotted and
        // restored when the journey ends).
        savePreTripCameraSnapshot()
        if feedsDeviceFixes {
            // A device-driven trip keeps the visitor's plane toggle: the
            // follow camera takes its 3D course pitch only when 3D is on.
            isFollowBrokenByGesture = false
            // The trip starts on the planned route's origin; the first
            // device fix retargets the camera from there.
            lastFixCoordinate = replayPlan.routeData.legs.first?.steps.first?
                .shape.first
        } else {
            isThreeDimensionalPitchEnabled = true
            isFollowBrokenByGesture = false
            let startFix = journeySimulator?.fixAtElapsedSeconds(elapsedSeconds: 0)
            lastFixCoordinate = startFix?.coordinate ?? replayPlan.samples.first?.coordinate
        }
        courseMedianSmoother = nil
        lastFixCourseHeadingDegrees = nil
        followingZoomLevel = ReferenceApplicationConfiguration.navigationFollowZoomLevel
        applyInitialFollowCameraState()
        isJourneyActive = true
        journeySampleIndex = 0
        isDiagnosticsSheetOpen = false
        homeSheetState = .peek
        // A device-driven trip consumes the source's fixes; no replay timer
        // may walk the route underneath them.
        if !feedsDeviceFixes {
            startJourneyTimer()
        }
    }

    /// Pauses the running journey's playback at its current fix (the
    /// presentation holds; the arrival state is already still).
    func pauseSimulatedJourney() {
        guard isJourneyActive, !hasSimulatedJourneyArrived else { return }
        isJourneyPaused = true
        stopJourneyTimer()
    }

    /// Resumes a paused journey from its current fix.
    func resumeSimulatedJourney() {
        guard isJourneyActive, isJourneyPaused, !hasSimulatedJourneyArrived else { return }
        isJourneyPaused = false
        startJourneyTimer()
    }

    /// Selects the playback speed multiplier — one of
    /// `simulatedTripSpeedMultiplierChoices`. A route-following journey
    /// rebuilds its walker at the new pace and re-anchors the clock at the
    /// travelled distance, so a mid-trip pace change never teleports the
    /// position. A refusal stops the playback honestly (§6.3).
    func selectSimulatedJourneySpeedMultiplier(_ speedMultiplier: Double) {
        let isOfferedChoice = ReferenceApplicationConfiguration
            .simulatedTripSpeedMultiplierChoices.contains(speedMultiplier)
        guard isOfferedChoice, speedMultiplier != journeySpeedMultiplier else { return }
        if let journeyReplayPlan, journeyReplayPlan.followsRouteGeometry, isJourneyActive {
            do {
                let rebuiltSimulator = try makeRouteSimulator(
                    route: journeyReplayPlan.routeData,
                    speedMultiplier: speedMultiplier
                )
                // Keep the travelled distance continuous: the rebuilt
                // walker's clock restarts at the distance already covered.
                let anchorFix = rebuiltSimulator.fixAtTravelledMeters(
                    travelledMeters: journeyTravelledMeters
                )
                routeSimulator = rebuiltSimulator
                elapsedSimulatedSeconds = anchorFix.elapsedSeconds
            } catch let sdkError as SdkError {
                reportSdkError(sdkError)
                concludeSimulatedJourneyKeepingCards()
                return
            } catch {
                reportLocalError(
                    origin: "simulated journey",
                    detail: String(describing: error)
                )
                concludeSimulatedJourneyKeepingCards()
                return
            }
        }
        journeySpeedMultiplier = speedMultiplier
        persistRememberedChoices()
        if isJourneyActive && !isJourneyPaused && !hasSimulatedJourneyArrived {
            // A recorded trace's cadence divides by the multiplier; the
            // route walker's tick stays fixed.
            startJourneyTimer()
        }
    }

    /// Moves the presented journey to `targetMeters` along the route's own
    /// geometry (the seek). The walker supplies the fix, so the camera, the
    /// location indicator, the ETA line and the arrival all read exactly what
    /// a normal fix at that distance produces.
    ///
    /// A paused journey stays paused; a running journey continues from the new
    /// position. Seeking backwards out of the arrival restarts the navigation
    /// session at the seek position, because the contract's reducer ignores
    /// fixes once it reports `Arrived` — without the restart the ETA line and
    /// guidance would stay frozen at the destination.
    func seekSimulatedJourney(toTravelledMeters targetMeters: Double) {
        guard isJourneyActive, let journeyReplayPlan,
            journeyReplayPlan.followsRouteGeometry,
            let routeSimulator
        else {
            return
        }
        let targetFix = routeSimulator.fixAtTravelledMeters(
            travelledMeters: min(
                max(targetMeters, 0),
                routeSimulator.totalDistanceMeters()
            )
        )
        if hasSimulatedJourneyArrived && !targetFix.isArrived {
            guard let navigationController else { return }
            // The arrival state's cards come from a session the reducer has
            // closed; reset them and start the same route again so the seek
            // position drives live progress and guidance.
            navigationCardStore.reset()
            do {
                try navigationController.startNavigation(
                    route: journeyReplayPlan.routeData,
                    unitSystem: .metric,
                    stepAdvancePolicy: defaultStepAdvancePolicy(),
                    deviationPolicy: defaultDeviationPolicy()
                )
            } catch let sdkError as SdkError {
                reportSdkError(sdkError)
                concludeSimulatedJourneyKeepingCards()
                return
            } catch {
                reportLocalError(
                    origin: "simulated journey",
                    detail: String(describing: error)
                )
                concludeSimulatedJourneyKeepingCards()
                return
            }
            hasSimulatedJourneyArrived = false
        }
        journeyTravelledMeters = targetFix.travelledMeters
        elapsedSimulatedSeconds = targetFix.elapsedSeconds
        hasDeliveredRouteWalkStartFix = true
        deliverJourneyFix(
            coordinate: targetFix.coordinate,
            horizontalAccuracyMeters: ReferenceApplicationConfiguration
                .simulatedJourneyFixAccuracyMeters,
            courseHeadingDegrees: targetFix.courseHeadingDegrees,
            timestampSeconds: targetFix.elapsedSeconds
        )
        if targetFix.isArrived {
            presentSimulatedJourneyArrival()
            return
        }
        // The timer stops at the arrival; a backward seek resumes it unless
        // the visitor paused the journey.
        if !isJourneyPaused && journeyTimer == nil {
            startJourneyTimer()
        }
    }

    /// Stops the simulated journey (the drawer's exit): cancels the topic
    /// subscriptions, stops the timer, drops the controller and clears
    /// the cards; the sheet returns to peek (§4.1). A device-driven trip
    /// takes the same exit: its navigation ends and only the location
    /// source it started stops with it.
    func stopSimulatedJourney() {
        if isDeviceNavigationActive {
            stopDeviceNavigation()
            return
        }
        finishSimulatedJourney()
        homeSheetState = .peek
    }

    /// Stops a journey whose playback ended without an arrival (a recorded
    /// trace whose samples drained behind a reroute, or a refused pace
    /// change) — the same cleanup as the drawer's exit (one entry point),
    /// keeping the card store's payload; the follow camera's zoom, bearing
    /// and pitch restore off the journey (§5.3). An arrival takes the
    /// presentation-preserving path instead (`presentSimulatedJourneyArrival`).
    private func concludeSimulatedJourneyKeepingCards() {
        cancelTopicSubscriptions()
        stopJourneyTimer()
        navigationController = nil
        isJourneyActive = false
        isJourneyPaused = false
        hasSimulatedJourneyArrived = false
        isOffRouteSimulationActive = false
        journeySampleIndex = 0
        clearRouteWalkingState()
        // The speech topic is gone; a line still in flight finishes (the
        // audio focus-equivalent releases only after drain).
        speechEngine.releaseAudioSessionAfterDrain()
        teardownFollowCameraState()
    }

    /// The full teardown of a stopped (not concluded) journey: also clears
    /// the card store; the follow camera restores off the journey (§5.3).
    private func finishSimulatedJourney() {
        cancelTopicSubscriptions()
        stopJourneyTimer()
        navigationController = nil
        navigationCardStore.reset()
        isJourneyActive = false
        isJourneyPaused = false
        hasSimulatedJourneyArrived = false
        isOffRouteSimulationActive = false
        journeySampleIndex = 0
        clearRouteWalkingState()
        // The speech topic is gone; a line still in flight finishes (the
        // audio focus-equivalent releases only after drain).
        speechEngine.releaseAudioSessionAfterDrain()
        // The journey route leaves the overlay list when the trip stops.
        rebuildOverlays()
        teardownFollowCameraState()
    }

    /// The post-trip teardown both stop paths share: the camera restores
    /// to the pre-trip snapshot (§5.3's "applied at trip start, restored
    /// after"), 2D/3D returns to the pre-trip mode, and the location
    /// layer clears — the trip's last fix is stale and the indicator never
    /// renders one (§4.2: no fix, nothing).
    private func teardownFollowCameraState() {
        restorePreTripCameraSnapshot()
        if isTrackingFixPlaced, let demoPositionCoordinate {
            // The demo's standing tracking fix places the layer back into
            // its idle disc state; without it the layer stays cleared.
            applyLocationIndicator(
                coordinate: demoPositionCoordinate,
                horizontalAccuracyMeters: ReferenceApplicationConfiguration
                    .simulatedJourneyFixAccuracyMeters,
                guided: false
            )
        } else {
            clearLocationIndicatorLayer()
        }
    }

    private func cancelTopicSubscriptions() {
        for subscription in topicSubscriptions {
            subscription.cancel()
        }
        topicSubscriptions = []
    }

    /// Starts the replay timer: a route-following trip ticks at the fixed
    /// walk interval (the walker owns the pace), a recorded trace at its
    /// sample cadence divided by the demo acceleration and the selected
    /// multiplier.
    private func startJourneyTimer() {
        stopJourneyTimer()
        guard let journeyReplayPlan, isJourneyActive, !isJourneyPaused else { return }
        let dispatchInterval: TimeInterval
        if journeyReplayPlan.followsRouteGeometry {
            dispatchInterval = ReferenceApplicationConfiguration.routeWalkTickIntervalSeconds
        } else {
            dispatchInterval = max(
                journeyReplayPlan.sampleIntervalSeconds
                    / (ReferenceApplicationConfiguration.simulatedJourneySpeedFactor
                        * journeySpeedMultiplier),
                ReferenceApplicationConfiguration.minimumJourneyDispatchIntervalSeconds
            )
        }
        journeyTimer = Timer.scheduledTimer(
            withTimeInterval: dispatchInterval,
            repeats: true
        ) { [weak self] _ in
            Task { @MainActor [weak self] in
                self?.advanceJourneyPlayback()
            }
        }
    }

    private func stopJourneyTimer() {
        journeyTimer?.invalidate()
        journeyTimer = nil
    }

    /// Advances the playback one tick: a route-following trip asks its
    /// `RouteSimulator` for the fix at the advanced simulated clock; a
    /// recorded trace delivers its next sample. The delivered fix carries
    /// the position, the course and the accuracy to the controller, the
    /// card model, the renderer-owned indicator and the §5.3 follow camera.
    /// An arrival stops the playback while keeping the presentation; sample
    /// exhaustion without an arrival concludes the trip. Speech is
    /// topic-driven: this pump never speaks.
    private func advanceJourneyPlayback() {
        guard let journeyReplayPlan, navigationController != nil, isJourneyActive else {
            stopSimulatedJourney()
            return
        }
        guard !isJourneyPaused, !hasSimulatedJourneyArrived else { return }
        if journeyReplayPlan.followsRouteGeometry {
            guard let routeSimulator else {
                // A route-following trip always builds its walker before the
                // timer starts; a missing one ends the playback rather than
                // leaving the trip stuck as "active".
                concludeSimulatedJourneyKeepingCards()
                return
            }
            if hasDeliveredRouteWalkStartFix {
                elapsedSimulatedSeconds +=
                    ReferenceApplicationConfiguration.routeWalkTickIntervalSeconds
                    * ReferenceApplicationConfiguration.simulatedJourneySpeedFactor
            }
            hasDeliveredRouteWalkStartFix = true
            let fix = routeSimulator.fixAtElapsedSeconds(
                elapsedSeconds: elapsedSimulatedSeconds
            )
            journeyTravelledMeters = fix.travelledMeters
            deliverJourneyFix(
                coordinate: fix.coordinate,
                horizontalAccuracyMeters:
                    ReferenceApplicationConfiguration.simulatedJourneyFixAccuracyMeters,
                courseHeadingDegrees: fix.courseHeadingDegrees,
                timestampSeconds: fix.elapsedSeconds
            )
            // The walker's own arrival report lands the last fix exactly on
            // the destination, whether or not the controller's phase had
            // caught up yet.
            if fix.isArrived {
                presentSimulatedJourneyArrival()
            }
            return
        }
        guard journeySampleIndex < journeyReplayPlan.samples.count else {
            if hasSimulatedJourneyArrived || navigationCardStore.isTripArrived {
                stopJourneyTimer()
            } else {
                // The samples ran out without arrival (a reroute shortened
                // them): the replay stops and the pre-trip camera returns.
                concludeSimulatedJourneyKeepingCards()
            }
            return
        }
        let replayedSample = journeyReplayPlan.samples[journeySampleIndex]
        journeySampleIndex += 1
        let replayedFix = isOffRouteSimulationActive
            ? offRouteSimulatedFix(from: replayedSample.coordinate)
            : replayedSample.coordinate
        deliverJourneyFix(
            coordinate: replayedFix,
            horizontalAccuracyMeters: replayedSample.horizontalAccuracyMeters,
            courseHeadingDegrees: smoothedCourseHeadingDegrees(toFix: replayedFix),
            timestampSeconds: replayedSample.timestampOffsetSeconds
        )
    }

    /// Feeds one fix to the controller and its presentation: the trip state
    /// refreshes from the read-only queries (§7.2's seeding), the
    /// renderer-owned indicator points along the fix's course (§4.2), the
    /// §5.3 follow camera follows the fix, and the controller's arrival
    /// phase swaps the drawer's contents.
    private func deliverJourneyFix(
        coordinate: GeographicCoordinate,
        horizontalAccuracyMeters: Double?,
        courseHeadingDegrees: Double?,
        timestampSeconds: Double?
    ) {
        guard let navigationController else { return }
        navigationController.updateLocation(
            locationUpdate: NavigationLocationUpdate(
                coordinate: coordinate,
                horizontalAccuracyMeters: horizontalAccuracyMeters,
                courseHeadingDegrees: courseHeadingDegrees,
                timestampSeconds: timestampSeconds
            )
        )
        navigationCardStore.seed(
            fromInstruction: navigationController.currentInstructions(),
            tripState: navigationController.currentTripState()
        )
        lastFixCoordinate = coordinate
        if let courseHeadingDegrees {
            lastFixCourseHeadingDegrees = courseHeadingDegrees
        }
        // The location layer shows the guided wedge pointing along the
        // course (§4.2: GuidedCourseArrow while guided).
        applyLocationIndicator(
            coordinate: coordinate,
            courseHeadingDegrees: courseHeadingDegrees,
            horizontalAccuracyMeters: horizontalAccuracyMeters,
            guided: true
        )
        // §5.3: each fix keeps the follow targets (center, the §2.4 zoom, the
        // course bearing, the 2D/3D pitch) current, but a follow broken by a
        // gesture stays broken — only the my-location control re-engages it,
        // so a later fix never snaps the user's panned view back.
        applyFollowCameraFix(center: coordinate, courseHeadingDegrees: courseHeadingDegrees)
        if navigationController.currentTripState()?.phase == .arrived
            || navigationCardStore.isTripArrived
        {
            presentSimulatedJourneyArrival()
        }
    }

    /// The arrival presentation (§7.1/§7.2): the playback stops, but the
    /// trip stays presented — the destination remains the camera centre and
    /// the indicator turns into the standing tracking dot — until the
    /// drawer's exit tears the journey down and restores the pre-trip
    /// camera.
    private func presentSimulatedJourneyArrival() {
        guard !hasSimulatedJourneyArrived else { return }
        hasSimulatedJourneyArrived = true
        stopJourneyTimer()
        // A route-following trip can reach the controller's arrival phase a
        // fix or two before the walker's final tick. The arrival presentation
        // lands on the walker's end fix, so the destination — never a fix
        // short of it — is the held centre.
        let destinationFix = routeDestinationFix()
        var arrivedCoordinate = lastFixCoordinate
        var arrivedCourseHeadingDegrees = lastFixCourseHeadingDegrees
        if let destinationFix {
            arrivedCoordinate = destinationFix.coordinate
            arrivedCourseHeadingDegrees =
                destinationFix.courseHeadingDegrees ?? arrivedCourseHeadingDegrees
            journeyTravelledMeters = destinationFix.travelledMeters
            lastFixCoordinate = destinationFix.coordinate
            lastFixCourseHeadingDegrees = arrivedCourseHeadingDegrees
            // The controller's own state lands on the destination too, so its
            // progress and snapped fix agree with the held presentation.
            navigationController?.updateLocation(
                locationUpdate: NavigationLocationUpdate(
                    coordinate: destinationFix.coordinate,
                    horizontalAccuracyMeters:
                        ReferenceApplicationConfiguration.simulatedJourneyFixAccuracyMeters,
                    courseHeadingDegrees: destinationFix.courseHeadingDegrees,
                    timestampSeconds: destinationFix.elapsedSeconds
                )
            )
            if let navigationController {
                navigationCardStore.seed(
                    fromInstruction: navigationController.currentInstructions(),
                    tripState: navigationController.currentTripState()
                )
            }
        }
        // The arrival re-centres even after a gesture broke the follow: the
        // destination is the trip's final presentation.
        isFollowBrokenByGesture = false
        if let arrivedCoordinate {
            applyFollowCameraFix(
                center: arrivedCoordinate,
                courseHeadingDegrees: arrivedCourseHeadingDegrees
            )
            applyLocationIndicator(
                coordinate: arrivedCoordinate,
                courseHeadingDegrees: arrivedCourseHeadingDegrees,
                horizontalAccuracyMeters:
                    ReferenceApplicationConfiguration.simulatedJourneyFixAccuracyMeters,
                guided: false
            )
        }
        // A line still in flight finishes (the audio focus-equivalent
        // releases only after drain).
        speechEngine.releaseAudioSessionAfterDrain()
    }

    /// The route-following walk's end fix (the destination and its final
    /// segment's bearing), or nil for a recorded trace.
    private func routeDestinationFix() -> SimulatedTripFix? {
        guard let journeyReplayPlan, journeyReplayPlan.followsRouteGeometry,
            let routeSimulator
        else {
            return nil
        }
        return routeSimulator.fixAtTravelledMeters(
            travelledMeters: routeSimulator.totalDistanceMeters()
        )
    }

    /// Builds a walker over `route` at the app's chosen pace: the route's
    /// own average speed is the baseline (a walk replays at the route's own
    /// duration at 1×), and the multiplier is the drawer's speed choice.
    private func makeRouteSimulator(
        route: Route,
        speedMultiplier: Double
    ) throws -> RouteSimulator {
        try RouteSimulator(
            route: route,
            pace: SimulatedTripPace(
                speedMetersPerSecond: routeSpeedMetersPerSecond(route),
                speedMultiplier: speedMultiplier
            )
        )
    }

    /// The baseline speed of a planned route, in meters per second. A route
    /// with no positive distance or duration (a single point, or a fixture
    /// that declares no travel time) falls back to the demo walk speed; the
    /// contract refuses a non-positive pace.
    private func routeSpeedMetersPerSecond(_ route: Route) -> Double {
        if route.distanceMeters > 0 && route.durationSeconds > 0 {
            return route.distanceMeters / route.durationSeconds
        }
        return ReferenceApplicationConfiguration.plannedRouteFallbackWalkSpeedMetersPerSecond
    }

    /// Clears the route-walking state both teardown paths share.
    private func clearRouteWalkingState() {
        routeSimulator = nil
        elapsedSimulatedSeconds = 0
        journeyTravelledMeters = 0
        journeySeekableDistanceMeters = 0
        isJourneySeekable = false
        hasDeliveredRouteWalkStartFix = false
    }

    /// The speech-instruction topic's handler: the SDK decided *when*
    /// the line is newsworthy; the platform speaks the payload's verbatim
    /// text through the engine policy (no queue, a newer line
    /// interrupts a lagging one, nothing repeats). The payload's
    /// `utterance_id` rides along: the engine
    /// dedups a re-delivery of the same emission by id — dedup and flush
    /// stay platform decisions.
    private func handleSpeechInstruction(_ speechInstruction: SpeechInstruction) {
        speechEngine.speakGuidance(
            speechTextVerbatim: speechInstruction.utteranceSpeechText,
            stableStepSpeechText: nil,
            utteranceId: speechInstruction.utteranceId
        )
        if !hasSpokenGuidance, speechEngine.lastSpokenText != nil {
            hasSpokenGuidance = true
        }
    }

    /// The off-route displacement: shifts the simulated fix east by the
    /// demo offset meters (at Sacramento's latitude, longitude-offset degrees
    /// over the latitude-circle meter length), beyond the default
    /// deviation threshold widened by the simulated fix accuracy.
    private func offRouteSimulatedFix(from simulatedFix: GeographicCoordinate) -> GeographicCoordinate {
        let offsetLatitudeMeters = ReferenceApplicationConfiguration.offRouteSimulationOffsetMeters
        let offsetDegrees = offsetLatitudeMeters
            / ReferenceApplicationConfiguration.metersPerDegreeLatitude
        return GeographicCoordinate(
            latitude: simulatedFix.latitude,
            longitude: simulatedFix.longitude + offsetDegrees
        )
    }

    /// The smoothed course-over-ground carried onto the fix (§5.3): the
    /// fix-to-fix bearing (clockwise from north) over a displaced move,
    /// appended to the median-smoothing window and trimmed to it.
    private func smoothedCourseHeadingDegrees(toFix fix: GeographicCoordinate) -> Double? {
        defer { lastFixCoordinate = fix }
        guard let lastFixCoordinate else { return nil }
        let displacementMeters = JourneyReplayCourseMeasure.displacementMeters(
            from: lastFixCoordinate, to: fix
        )
        guard displacementMeters
            >= ReferenceApplicationConfiguration.courseFixMinimumDisplacementMeters
        else {
            return lastFixCourseHeadingDegrees
        }
        let fixCourseDegrees = JourneyReplayCourseMeasure.bearingDegrees(
            from: lastFixCoordinate, to: fix
        )
        let smoother: CourseMedianSmoother
        if let existingSmoother = courseMedianSmoother {
            smoother = existingSmoother
        } else {
            let newSmoother = CourseMedianSmoother(
                windowFixCount: UInt32(
                    ReferenceApplicationConfiguration.courseFollowSmoothingFixes
                )
            )
            courseMedianSmoother = newSmoother
            smoother = newSmoother
        }
        smoother.add(courseDegrees: fixCourseDegrees)
        lastFixCourseHeadingDegrees = smoother.median()
        return lastFixCourseHeadingDegrees
    }

    /// Draws the journey's own route as the route polyline (it rides
    /// beside the scene toggles and the planned-route overlay).
    private func rebuildJourneyRouteOverlay() {
        guard let journeyReplayPlan else { return }
        guard let mapView else { return }
        var journeyOverlays = activeOverlayDescriptors
        if journeyOverlays.first(where: { $0.kind == .routePolyline }) == nil {
            journeyOverlays.append(
                routePolylineDescriptor(forRoute: journeyReplayPlan.routeData)
            )
        }
        mapView.setOverlays(journeyOverlays)
    }
}
// MARK: - Navigation follow camera, location indicator, map controls (docs/ReferenceAppUX.md §4.2, §5.3)

extension ReferenceApplicationModel {
    // MARK: Follow camera (§5.3)

    /// Snapshots the pre-trip camera the §5.3 follow camera restores after
    /// the trip (the values — including a fitted framing, whose readback
    /// the camera values carry — the selected preset and the 2D/3D mode).
    private func savePreTripCameraSnapshot() {
        preTripCameraSnapshot = PreTripCameraSnapshot(
            cameraValues: cameraValues,
            selectedPresetIdentifier: selectedPresetIdentifier,
            isThreeDimensionalPitchEnabled: isThreeDimensionalPitchEnabled
        )
    }

    /// Restores the pre-trip camera snapshot (§5.3's "applied at trip
    /// start, restored after"), leaving presets as the trip found them.
    private func restorePreTripCameraSnapshot() {
        isHoldingNorthUp = false
        guard let restoredSnapshot = preTripCameraSnapshot else {
            followingZoomLevel = ReferenceApplicationConfiguration.navigationFollowZoomLevel
            return
        }
        cameraValues = restoredSnapshot.cameraValues
        selectedPresetIdentifier = restoredSnapshot.selectedPresetIdentifier
        isThreeDimensionalPitchEnabled = restoredSnapshot.isThreeDimensionalPitchEnabled
        preTripCameraSnapshot = nil
        pushCameraImmediately()
    }

    /// The trip's initial follow-camera state (§5.3): the first fix
    /// centers, the §2.4 zoom engages, the 2D/3D pitch applies; the
    /// bearing keeps the trip's framing until the first displaced fix
    /// produces a smoothed course sample.
    private func applyInitialFollowCameraState() {
        isHoldingNorthUp = false
        if let firstFix = lastFixCoordinate {
            followingZoomLevel = ReferenceApplicationConfiguration.navigationFollowZoomLevel
            applyFollowCameraFix(
                center: firstFix,
                courseHeadingDegrees: nil
            )
        }
    }

    /// The follow camera's fix application (§5.3): center the fix, hold
    /// the trip's follow zoom (the §2.4 base, or the user-nudged level
    /// under the §2.4 maximum), bear along the smoothed course, pitch per
    /// the 2D/3D toggle. A broken follow stays broken until the caller
    /// re-engages.
    private func applyFollowCameraFix(
        center fix: GeographicCoordinate,
        courseHeadingDegrees: Double?
    ) {
        guard !isFollowBrokenByGesture else { return }
        let followZoomLevel = followingZoomLevel
        let followPitchDegrees = isThreeDimensionalPitchEnabled
            ? ReferenceApplicationConfiguration.navigationFollowPitchDegrees3D : 0
        var changedValues = cameraValues
        changedValues.centerLatitudeDegrees = fix.latitude
        changedValues.centerLongitudeDegrees = fix.longitude
        changedValues.zoomLevel = followZoomLevel
        if isHoldingNorthUp {
            changedValues.bearingDegrees = 0
        } else if let courseHeadingDegrees {
            changedValues.bearingDegrees = wrappedBearingDegrees(courseHeadingDegrees)
        }
        changedValues.pitchDegrees = followPitchDegrees
        cameraValues = changedValues
        pushCameraImmediately()
    }

    /// A user map gesture (drag/pinch/rotate) breaks the follow camera's
    /// writes immediately (§5.3); it stays broken until the my-location
    /// control re-engages it — a later fix does not.
    func breakNavigationFollowFromGesture() {
        guard isJourneyActive else { return }
        isFollowBrokenByGesture = true
    }

    // MARK: The location indicator (§4.2: renderer-owned visuals)

    /// Applies one fix to the renderer-owned location layer (§4.2):
    /// the contract's state (position, optional course, optional
    /// accuracy) and the style's visual kind — the guided wedge while a
    /// journey runs, the plain tracking disc otherwise. The style keeps
    /// the rendered defaults (no radius/color/ring override).
    private func applyLocationIndicator(
        coordinate: GeographicCoordinate,
        courseHeadingDegrees: Double? = nil,
        horizontalAccuracyMeters: Double?,
        guided: Bool
    ) {
        latestLocationGuided = guided
        // The fix enters the animator; the drawn pose comes from `poseAt`, so
        // the first fix draws at once and later ones glide (the web pump's
        // rule, from the same shared crate).
        locationPoseAnimator.pushFix(
            latitudeDegrees: coordinate.latitude,
            longitudeDegrees: coordinate.longitude,
            courseDegrees: courseHeadingDegrees,
            horizontalAccuracyMeters: horizontalAccuracyMeters,
            receivedAtMilliseconds: locationAnimationNowMilliseconds()
        )
        drawLocationPose()
    }

    /// The monotonic clock the puck's motion reads, in milliseconds.
    private func locationAnimationNowMilliseconds() -> Double {
        ProcessInfo.processInfo.systemUptime * 1000
    }

    /// Draws the interpolated pose for this instant and keeps the animation
    /// timer running while the motion lasts. The shipped view reports a
    /// refused push through its delegate.
    private func drawLocationPose() {
        guard let mapView,
              let pose = locationPoseAnimator.poseAt(
                nowMilliseconds: locationAnimationNowMilliseconds()
              )
        else { return }
        mapView.setLocationIndicator(
            state: LocationIndicatorState(
                coordinate: GeographicCoordinate(
                    latitude: pose.latitudeDegrees,
                    longitude: pose.longitudeDegrees
                ),
                courseHeadingDegrees: pose.courseDegrees,
                horizontalAccuracyMeters: pose.horizontalAccuracyMeters
            ),
            style: LocationIndicatorStyle(
                visualKind: latestLocationGuided ? .guidedCourseArrow : .trackingDot,
                radiusPixels: nil,
                rgbaColor: nil,
                ringRgbaColor: nil,
                haloRgbaColor: nil,
                showAccuracyRing: nil
            )
        )
        if pose.isAnimating {
            startLocationAnimationTimer()
        } else {
            stopLocationAnimationTimer()
        }
    }

    /// Starts the 60 Hz timer that redraws the puck during a motion.
    private func startLocationAnimationTimer() {
        guard locationAnimationTimer == nil else { return }
        let timer = Timer(timeInterval: 1.0 / 60.0, repeats: true) { [weak self] _ in
            Task { @MainActor in
                self?.drawLocationPose()
            }
        }
        RunLoop.main.add(timer, forMode: .common)
        locationAnimationTimer = timer
    }

    /// Stops the redraw timer once the motion settles.
    private func stopLocationAnimationTimer() {
        locationAnimationTimer?.invalidate()
        locationAnimationTimer = nil
    }

    // MARK: Device positioning

    /// Starts or stops the device location source. Starting asks for
    /// authorization while the status is undetermined; the source then starts
    /// itself once the prompt is answered, so the visitor grants once and the
    /// round control's promise holds.
    func toggleDeviceLocation() {
        // The visitor's own source action supersedes the control's refusal
        // line; the source's own status (live position, off, refused) wins.
        showsDeviceNavigationPlanFirstRefusal = false
        if isDeviceLocationActive {
            stopDeviceLocation()
            return
        }
        switch startDeviceLocationSource() {
        case .started:
            shouldCenterOnNextFix = true
            mapLayersState.setLocationStatus(
                mapLayersState.latestFixDisplayText == nil
                    ? .waitingForFix
                    : .active
            )
            deviceLocationStatusText = deviceLocationSource?.authorizationStatus
                == .notDetermined
                ? ReferenceApplicationConfiguration.deviceLocationPermissionRequestedText
                : nil
        case .permissionDenied:
            deviceLocationStatusText =
                ReferenceApplicationConfiguration.deviceLocationPermissionDeniedText
        case .unavailable:
            deviceLocationStatusText =
                ReferenceApplicationConfiguration.deviceLocationUnavailableText
        }
    }

    /// Starts the device location source and answers what happened: updates
    /// began, the authorization prompt is pending (treated as a start the
    /// source completes once granted), or the platform refused. The panel's
    /// round control and the route fields' chips share this path.
    @discardableResult
    func startDeviceLocationSource(remember: Bool = true) -> DeviceLocationStartResult {
        showsDeviceNavigationPlanFirstRefusal = false
        if isDeviceLocationActive {
            return .started
        }
        guard mapView != nil else {
            mapLayersState.setLocationStatus(.unavailable)
            return .unavailable
        }
        let controller = devicePositionController ?? {
            let createdController = PositionController()
            devicePositionController = createdController
            return createdController
        }()
        let source = DeviceLocationSource(
            positionController: controller,
            consumerMode: isDeviceNavigationActive ? .navigation : .idle,
            onPositioningFailure: { [weak self] detail in
                self?.deviceLocationStatusText = detail
                self?.mapLayersState.setLocationStatus(.unavailable)
            },
            onFix: { [weak self] fix in
                self?.applyDevicePositionFix(fix)
            },
            onAuthorizationResolved: { [weak self] authorizationStatus in
                self?.deviceLocationAuthorizationResolved(authorizationStatus)
            }
        )
        deviceLocationSource = source
        if source.start() {
            isDeviceLocationActive = true
            rememberDeviceLocationChoice(remember: remember)
            return .started
        }
        isDeviceLocationActive = false
        switch source.authorizationStatus {
        case .notDetermined:
            // The prompt is pending; the source starts itself on grant.
            rememberDeviceLocationChoice(remember: remember)
            return .started
        case .denied, .restricted:
            mapLayersState.setLocationStatus(.permissionDenied)
            return .permissionDenied
        default:
            mapLayersState.setLocationStatus(.unavailable)
            return .unavailable
        }
    }

    /// Adopts a resolved authorization: grant starts the updates the visitor
    /// asked for, and a refusal reaches the panel row and the status line.
    private func deviceLocationAuthorizationResolved(
        _ authorizationStatus: CLAuthorizationStatus
    ) {
        switch authorizationStatus {
        case .authorizedAlways, .authorizedWhenInUse:
            isDeviceLocationActive = true
            deviceLocationStatusText = nil
            mapLayersState.setLocationStatus(
                mapLayersState.latestFixDisplayText == nil
                    ? .waitingForFix
                    : .active
            )
        case .denied, .restricted:
            isDeviceLocationActive = false
            deviceLocationStatusText =
                ReferenceApplicationConfiguration.deviceLocationPermissionDeniedText
            mapLayersState.setLocationStatus(.permissionDenied)
        case .notDetermined:
            break
        @unknown default:
            isDeviceLocationActive = false
            mapLayersState.setLocationStatus(.unavailable)
        }
    }

    /// Writes one plan field from a "My location" chip, leaving the other
    /// field untouched.
    private func fillRouteField(
        _ field: SearchFieldAssignmentTarget,
        with displayText: String
    ) {
        switch field {
        case .origin:
            updateOriginSearchText(displayText)
        case .destination:
            updateDestinationSearchText(displayText)
        }
    }

    /// Stops the source, clears the layer and reports the off state.
    func stopDeviceLocation() {
        deviceLocationSource?.stop()
        deviceLocationSource = nil
        isDeviceLocationActive = false
        shouldCenterOnNextFix = false
        remembersDeviceLocationEnabled = false
        clearLocationIndicatorLayer()
        deviceLocationStatusText = ReferenceApplicationConfiguration.deviceLocationOffText
        mapLayersState.noteDeviceLocationStopped()
        persistRememberedChoices()
    }

    /// Records the visitor's running device-location source in the remembered
    /// record; a restore pass suppresses the write.
    private func rememberDeviceLocationChoice(remember: Bool) {
        remembersDeviceLocationEnabled = true
        if remember {
            persistRememberedChoices()
        }
    }

    /// Restores the remembered device-location source once the shipped map
    /// view exists. The restart happens silently only when the device already
    /// holds the location permission; otherwise the choice stays in the
    /// record for a later visit rather than prompting at launch.
    private func restoreRememberedDeviceLocationIfPermitted() {
        guard pendingRememberedDeviceLocationStart, mapView != nil else { return }
        pendingRememberedDeviceLocationStart = false
        let authorizationStatus = CLLocationManager().authorizationStatus
        guard authorizationStatus == .authorizedAlways
            || authorizationStatus == .authorizedWhenInUse
        else {
            return
        }
        switch startDeviceLocationSource(remember: false) {
        case .started:
            shouldCenterOnNextFix = true
            mapLayersState.setLocationStatus(.waitingForFix)
        case .permissionDenied, .unavailable:
            break
        }
    }

    // MARK: Device-driven navigation

    /// Starts or stops guidance on the planned route from the device
    /// source's fixes (the Location group's second control).
    func toggleDeviceNavigation() {
        if isDeviceNavigationActive {
            stopDeviceNavigation()
            return
        }
        startDeviceNavigation()
    }

    /// Starts guidance on the planned route from this device's fixes. The
    /// location source is started when it is off; the simulated trip ends
    /// first, because one trip runs at a time. Without a planned route the
    /// status line says what the mode needs instead of faking a trip.
    private func startDeviceNavigation() {
        guard let plannedRoute else {
            showsDeviceNavigationPlanFirstRefusal = true
            deviceLocationStatusText =
                ReferenceApplicationConfiguration.deviceNavigationPlanFirstText
            return
        }
        // The source is the trip's fix supply: a refused authorization or an
        // unavailable service leaves its own honest caption and starts
        // nothing.
        let startedLocationSource = !isDeviceLocationActive
        if startedLocationSource {
            toggleDeviceLocation()
            guard isDeviceLocationActive else { return }
        }
        deviceNavigationStartedLocationSource = startedLocationSource
        // Guidance needs the navigation-grade accuracy stream; the source
        // switches in place when it was already running for the indicator.
        deviceLocationSource?.consumerMode = .navigation
        startJourney(
            replayPlan: JourneyReplayPlan.fromPlannedRoute(plannedRoute),
            feedsDeviceFixes: true
        )
        guard navigationController != nil else {
            // The navigation refused to start; the source it needed must not
            // linger on the mode's behalf.
            if deviceNavigationStartedLocationSource {
                deviceNavigationStartedLocationSource = false
                stopDeviceLocation()
            }
            return
        }
        isDeviceNavigationActive = true
        showsDeviceNavigationPlanFirstRefusal = false
        deviceLocationStatusText =
            ReferenceApplicationConfiguration.deviceNavigationWaitingText
    }

    /// Ends device-driven guidance. The location source stops only when this
    /// mode started it; a source the visitor started by hand stays running
    /// and its next fix keeps the position caption current.
    func stopDeviceNavigation() {
        guard isDeviceNavigationActive else { return }
        isDeviceNavigationActive = false
        finishSimulatedJourney()
        homeSheetState = .peek
        if deviceNavigationStartedLocationSource {
            deviceNavigationStartedLocationSource = false
            stopDeviceLocation()
        } else {
            // A source the visitor started by hand keeps running for the
            // map indicator, which only needs the idle accuracy.
            deviceLocationSource?.consumerMode = .idle
        }
    }

    /// Clears the plan-first refusal line once a plan exists, so the status
    /// caption returns to the source's own position updates.
    private func clearDeviceNavigationPlanFirstRefusal() {
        guard showsDeviceNavigationPlanFirstRefusal else { return }
        showsDeviceNavigationPlanFirstRefusal = false
        deviceLocationStatusText = nil
    }

    /// Shows the device fix through the renderer's tracking dot and centres the
    /// camera, keeping the smoothed course the controller carries. During a
    /// device-driven trip the same fix feeds the journey delivery instead, so
    /// the banner, ETA line, guided indicator and follow camera read the
    /// navigator stream rather than the standalone tracking presentation.
    private func applyDevicePositionFix(_ fix: PositionFix) {
        let smoothedCourse = devicePositionController?.smoothedCourseDegrees()
        // The standalone source keeps the controller's last fix, so the round
        // control's re-center and the chip's field fill both read a position
        // the renderer already drew.
        lastFixCoordinate = fix.coordinate
        lastFixCourseHeadingDegrees = smoothedCourse
        mapLayersState.noteDeviceFix(coordinate: fix.coordinate)
        if isDeviceNavigationActive {
            deliverJourneyFix(
                coordinate: fix.coordinate,
                horizontalAccuracyMeters: fix.horizontalAccuracyMeters,
                courseHeadingDegrees: smoothedCourse,
                timestampSeconds: nil
            )
            deviceLocationStatusText = String(
                format: "Position %.5f, %.5f.",
                fix.coordinate.latitude,
                fix.coordinate.longitude
            )
            return
        }
        mapView?.setLocationIndicator(
            state: LocationIndicatorState(
                coordinate: fix.coordinate,
                courseHeadingDegrees: smoothedCourse ?? fix.courseHeadingDegrees,
                horizontalAccuracyMeters: fix.horizontalAccuracyMeters
            ),
            style: LocationIndicatorStyle(
                visualKind: .trackingDot,
                radiusPixels: nil,
                rgbaColor: nil,
                ringRgbaColor: nil,
                haloRgbaColor: nil,
                showAccuracyRing: true
            )
        )
        // The round control promises to centre on the first fix; later fixes
        // keep the indicator current without moving a camera the visitor may
        // have panned.
        if shouldCenterOnNextFix {
            shouldCenterOnNextFix = false
            cameraValues.centerLatitudeDegrees = fix.coordinate.latitude
            cameraValues.centerLongitudeDegrees = fix.coordinate.longitude
            cameraValues.zoomLevel = max(
                cameraValues.zoomLevel,
                ReferenceApplicationConfiguration.deviceLocationZoomLevel
            )
            pushCameraImmediately()
        }
        if !showsDeviceNavigationPlanFirstRefusal {
            deviceLocationStatusText = String(
                format: "Position %.5f, %.5f.",
                fix.coordinate.latitude,
                fix.coordinate.longitude
            )
        }
    }

    /// Clears the renderer-owned location layer (§4.2: with no fix nothing
    /// renders — never a stale last position).
    private func clearLocationIndicatorLayer() {
        guard let mapView else { return }
        mapView.clearLocationIndicator()
        lastFixCoordinate = nil
        lastFixCourseHeadingDegrees = nil
        isTrackingFixPlaced = false
    }

    /// Places a simulated tracking fix at the demo position (§4.2's
    /// tracking-disc state on the demo's own fix — the demo's stand-in for
    /// the device fix; the same simulated-fix path the journeys ride).
    func placeDemoTrackingFix() {
        guard let demoPositionCoordinate else { return }
        lastFixCoordinate = demoPositionCoordinate
        lastFixCourseHeadingDegrees = nil
        isTrackingFixPlaced = true
        applyLocationIndicator(
            coordinate: demoPositionCoordinate,
            horizontalAccuracyMeters:
                ReferenceApplicationConfiguration.simulatedJourneyFixAccuracyMeters,
            guided: false
        )
    }

    // MARK: The map control cluster (§4.2)

    /// The zoom −/+ control's step application (§4.2: one §2.4
    /// `map-control-zoom-step` per tap; a trip's nudged zoom persists
    /// under the §2.4 maximum clamp).
    private func applyZoomControlStep(_ zoomStep: Double) {
        applyDirectCameraChange { values in
            values.zoomLevel = clampedZoomLevel(values.zoomLevel + zoomStep)
        }
        if isJourneyActive {
            followingZoomLevel = cameraValues.zoomLevel
        }
    }

    func userTappedZoomInControl() {
        applyZoomControlStep(AppLayout.mapControlZoomStep)
    }

    func userTappedZoomOutControl() {
        applyZoomControlStep(-AppLayout.mapControlZoomStep)
    }

    /// The zoom range's clamp: the §5.1 documented 0–20 during idle
    /// sessions; a running trip clamps at the §2.4 follow maximum.
    private func clampedZoomLevel(_ zoomLevel: Double) -> Double {
        var clampedZoomLevel = min(
            max(
                zoomLevel,
                ReferenceApplicationConfiguration.minimumDemoZoomLevel
            ),
            ReferenceApplicationConfiguration.maximumDemoZoomLevel
        )
        if isJourneyActive {
            clampedZoomLevel = min(
                clampedZoomLevel,
                ReferenceApplicationConfiguration.navigationFollowZoomMaximumLevel
            )
        }
        return clampedZoomLevel
    }

    /// The my-location control (§4.2): centers on the indicator's fix;
    /// during a trip it re-engages the follow camera right away.
    func myLocationControlTapped() {
        if isJourneyActive {
            isFollowBrokenByGesture = false
            if let lastFixCoordinate {
                applyFollowCameraFix(
                    center: lastFixCoordinate,
                    courseHeadingDegrees: lastFixCourseHeadingDegrees
                )
            }
            return
        }
        if isDeviceLocationActive {
            // The on state re-centers; with no fix yet the source is already
            // running and the first fix will centre.
            if let lastFixCoordinate {
                applyDirectCameraChange { values in
                    values.centerLatitudeDegrees = lastFixCoordinate.latitude
                    values.centerLongitudeDegrees = lastFixCoordinate.longitude
                }
            } else {
                mapLayersState.setLocationStatus(.waitingForFix)
            }
            return
        }
        // The off state starts the source (asking for permission), and the
        // first fix centres the camera. A refusal is never silent: its reason
        // lands on the panel row and the panel opens at that row.
        switch startDeviceLocationSource() {
        case .started:
            shouldCenterOnNextFix = true
            mapLayersState.setLocationStatus(
                mapLayersState.latestFixDisplayText == nil
                    ? .waitingForFix
                    : .active
            )
        case .permissionDenied:
            deviceLocationStatusText =
                ReferenceApplicationConfiguration.deviceLocationPermissionDeniedText
            openMapLayersPanel(focusDeviceLocation: true)
        case .unavailable:
            deviceLocationStatusText =
                ReferenceApplicationConfiguration.deviceLocationUnavailableText
            openMapLayersPanel(focusDeviceLocation: true)
        }
    }

    /// Opens the layer panel, optionally bringing the Device location row
    /// into view (the round control's long press and a refused source).
    func openMapLayersPanel(focusDeviceLocation: Bool = false) {
        shouldFocusMapLayersDeviceLocation = focusDeviceLocation
        isMapLayersPanelOpen = true
    }

    /// Forwards the traffic switch's act to the panel state.
    func setTrafficLayerEnabled(_ isEnabled: Bool) {
        mapLayersState.setTrafficEnabled(isEnabled)
    }

    /// The 2D/3D toggle (§4.2): swaps the pitch between the §2.4
    /// `navigation-follow-pitch-3d` and 0 — applied to the follow camera
    /// during a trip, to the standing camera otherwise.
    func toggledThreeDimensionalPitch() {
        isThreeDimensionalPitchEnabled.toggle()
        if isJourneyActive {
            isFollowBrokenByGesture = false
            if let lastFixCoordinate {
                applyFollowCameraFix(
                    center: lastFixCoordinate,
                    courseHeadingDegrees: lastFixCourseHeadingDegrees
                )
            }
        } else {
            applyDirectCameraChange { values in
                values.pitchDegrees = self.isThreeDimensionalPitchEnabled
                    ? ReferenceApplicationConfiguration.navigationFollowPitchDegrees3D
                    : 0
            }
        }
    }

    /// The compass control's tap (§4.2): eases the camera north up over
    /// the §2.4 duration through the shipped view's `animateCamera` (the
    /// shortest-path bearing interpolation), so the camera values publish
    /// the eased states the drawn frames carry.
    func compassControlTapped() {
        // During a guided trip the compass is a mode toggle: choosing
        // north-up pins the follow camera's bearing at 0 across every
        // following fix, and a second tap resumes the eased course chase.
        if isJourneyActive {
            isHoldingNorthUp.toggle()
        }
        guard let drawnBearingDegrees = cameraAttributeReadback?.bearingDegrees else {
            return
        }
        let currentBearingDegrees = wrappedBearingDegrees(drawnBearingDegrees)
        let degreesOffNorth = min(
            currentBearingDegrees,
            Double(360) - currentBearingDegrees
        )
        // A tapped compass returns to north from an off-north camera; while
        // north-up holds there is nothing to ease, and the mode toggle above
        // is the whole action.
        guard degreesOffNorth
            >= AppLayout.compassVisibilityBearingThresholdDegrees
        else { return }
        var targetPose = currentCameraPose()
        targetPose.bearingDegrees = currentBearingDegrees > 180 ? 360 : 0
        mapView?.animateCamera(
            targetPose,
            durationMilliseconds: AppLayout.compassRenorthAnimationSeconds
                * ReferenceApplicationConfiguration.millisecondsPerSecond
        )
    }

    /// The rotation gesture's bearing change (§5.3's rotate): the
    /// content-follows-pointer convention's rotational face (§5.1) — the
    /// map content turns with the fingers, and the delta arrives
    /// clockwise-positive. The shipped view crosses the contract camera's
    /// `rotateByDegreesAroundScreenPoint` member about the viewport center,
    /// whose bearing delta adds clockwise, so the gesture's sign flips
    /// across the seam and the camera answers the wrapped bearing. A user
    /// gesture breaks the follow camera's writes until the next fix (§5.3).
    func userRotatedMapDegrees(_ rotationDeltaDegrees: Double) {
        breakNavigationFollowFromGesture()
        mapView?.rotateByDegrees(clockwiseDegrees: rotationDeltaDegrees)
    }

    /// The scale bar's readout refresh (§4.2): the shipped view answers
    /// the drawn camera's `ground_meters_per_screen_pixel` for the
    /// attached viewport, and the 1-2-3-5-10 ladder maps it to the §2.4
    /// width range.
    private func refreshScaleBarReadout() {
        guard let groundMetersPerPixel = mapView?.groundMetersPerScreenPixel() else {
            return
        }
        let nextReadout = ScaleBarReadout.readout(
            groundMetersPerPixel: groundMetersPerPixel,
            minimumWidthPixels: AppLayout.scaleBarMinimumWidthPixels,
            maximumWidthPixels: AppLayout.scaleBarMaximumWidthPixels
        )
        if scaleBarReadout != nextReadout {
            scaleBarReadout = nextReadout
        }
    }

    // MARK: Diagnostics events

    /// Opens the diagnostics sheet; the flag's observer starts the feed.
    func openDiagnosticsSheet() {
        isDiagnosticsSheetOpen = true
    }

    /// The severity the pane currently displays: Info until the visitor
    /// explicitly asks for debug events.
    var diagnosticsVisibleMinimumSeverity: SdkLogSeverity {
        diagnosticsShowsDebugEvents
            ? ReferenceApplicationConfiguration.diagnosticsEventsDebugSeverity
            : ReferenceApplicationConfiguration.diagnosticsEventsMinimumSeverity
    }

    /// Lowers the pane's one subscription to Debug for the session. The SDK
    /// gate never raises back, so an off switch only hides debug rows; it
    /// does not unsubscribe or raise the threshold.
    func setDiagnosticsShowsDebugEvents(_ showsDebugEvents: Bool) {
        diagnosticsShowsDebugEvents = showsDebugEvents
        guard showsDebugEvents, let mapSdk else { return }
        subscribeToDiagnosticEventsIfNeeded(
            mapSdk: mapSdk,
            minimumSeverity: ReferenceApplicationConfiguration
                .diagnosticsEventsDebugSeverity
        )
        _ = mapSdk.flushRecentDiagnosticEvents(
            listener: diagnosticsEventListener,
            minimumSeverity: ReferenceApplicationConfiguration
                .diagnosticsEventsDebugSeverity,
            maximumCount:
                ReferenceApplicationConfiguration.diagnosticsEventsFlushMaximumCount
        )
    }

    /// Clears the pane's display list without touching the SDK's ring or the
    /// session's subscription.
    func clearDiagnosticsEvents() {
        diagnosticsEventsBuffer.clear()
    }

    /// Starts the sheet's recent-events feed: arms the lazy subscription (the
    /// first open subscribes; later opens reuse it or lower it), starts the
    /// cadence merge, clears the previous readout and flushes the ring's
    /// backlog oldest-first at the pane's current severity.
    private func startDiagnosticsEventsFeed() {
        guard let mapSdk else { return }
        subscribeToDiagnosticEventsIfNeeded(
            mapSdk: mapSdk,
            minimumSeverity: diagnosticsVisibleMinimumSeverity
        )
        diagnosticsEventsBuffer.startRefreshCadence()
        diagnosticsEventsBuffer.clear()
        _ = mapSdk.flushRecentDiagnosticEvents(
            listener: diagnosticsEventListener,
            minimumSeverity: diagnosticsVisibleMinimumSeverity,
            maximumCount:
                ReferenceApplicationConfiguration.diagnosticsEventsFlushMaximumCount
        )
    }

    /// Stops the sheet's feed: no later delivery is recorded and the merge
    /// timer is invalidated. The frozen contract has no unsubscribe member, so
    /// the registration itself stays on the SDK; the app-side feed — the
    /// staging list and the timer — is what this releases.
    private func stopDiagnosticsEventsFeed() {
        diagnosticsEventsBuffer.stopRefreshCadence()
    }

    /// Arms the sheet's one live diagnostic subscription on its first open,
    /// not at application start: an application that never opens the sheet
    /// never subscribes. Later opens reuse the registration; the explicit
    /// Show debug events switch re-subscribes once at Debug, and the gate is
    /// never raised back.
    private func subscribeToDiagnosticEventsIfNeeded(
        mapSdk: MapSdk,
        minimumSeverity: SdkLogSeverity
    ) {
        guard !isDiagnosticsEventsSubscriptionArmed
            || diagnosticsEventsSubscriptionSeverity != minimumSeverity
        else {
            return
        }
        mapSdk.subscribeToDiagnosticEvents(
            listener: diagnosticsEventListener,
            minimumSeverity: minimumSeverity
        )
        isDiagnosticsEventsSubscriptionArmed = true
        diagnosticsEventsSubscriptionSeverity = minimumSeverity
    }

    // MARK: Provider console

    /// Opens the provider console; the flag's observer starts the feed.
    func openProviderConsole() {
        isProviderConsoleOpen = true
    }

    /// Closes the provider console; the flag's observer stops the cadence and
    /// keeps the warning/error stream for the unread dot.
    func closeProviderConsole() {
        isProviderConsoleOpen = false
    }

    /// Applies the console's level filter.
    func setProviderConsoleLevelFilter(
        _ levelFilter: ProviderConsoleLevelFilter
    ) {
        providerConsoleLevelFilter = levelFilter
    }

    /// Enables or disables the bounded response-excerpt capture. The switch
    /// is process-wide and off by default; the console's rows and export
    /// carry the excerpt only while it is on.
    func setProviderConsoleCapturesResponseExcerpts(_ enabled: Bool) {
        providerConsoleCapturesResponseExcerpts = enabled
        mapSdk?.setResponseExcerptCapture(enabled: enabled)
    }

    /// Starts the console's feed: empties the display list, opens the feed,
    /// arms the one subscription (the first open only) and replays the ring's
    /// backlog oldest-first through the same listener.
    private func startProviderConsoleFeed() {
        providerConsoleFeed.clear()
        providerConsoleFeed.open()
        mapLayersState.refreshTileCacheStatistics()
        guard let mapSdk else { return }
        if !isProviderConsoleSubscriptionArmed {
            mapSdk.subscribeToDiagnosticEvents(
                listener: providerConsoleEventListener,
                minimumSeverity:
                    ReferenceApplicationConfiguration
                        .diagnosticsEventsMinimumSeverity
            )
            isProviderConsoleSubscriptionArmed = true
        }
        _ = mapSdk.flushRecentDiagnosticEvents(
            listener: providerConsoleEventListener,
            minimumSeverity:
                ReferenceApplicationConfiguration.diagnosticsEventsMinimumSeverity,
            maximumCount:
                ReferenceApplicationConfiguration
                    .diagnosticsEventsFlushMaximumCount
        )
        providerConsoleFeed.mergePendingDeliveries()
    }

    /// Stops the console's cadence; the subscription itself stays on the SDK
    /// (the frozen contract has no unsubscribe member), and the feed keeps
    /// only warnings and errors for the unread dot.
    private func stopProviderConsoleFeed() {
        providerConsoleFeed.close()
    }

    /// The launch-argument probe seam: records one Warning before the slower
    /// session start when the UI-test flag is set. A normal launch never
    /// fabricates a console row.
    private func recordProviderConsoleProbeIfRequested() {
        guard UserDefaults.standard.bool(
            forKey: ReferenceApplicationConfiguration
                .launchArgumentProviderConsoleProbeWarning
        ) else {
            return
        }
        recordProviderConsoleProbeEvent()
    }

    /// The UI-test seam: records one clearly labeled probe event through the
    /// same feed path the SDK listener uses, so the floating control's unread
    /// dot and the row/detail rendering are driveable without a provider
    /// call. Only the launch automation invokes it; a normal launch never
    /// fabricates a console row.
    func recordProviderConsoleProbeEvent() {
        let probeProviderKey =
            providerDescriptors.first?.providerKey ?? "application"
        let probeTrace = ProviderRequestTrace(
            providerKey: probeProviderKey,
            providerContract: "RoutingProvider",
            httpMethod: "GET",
            requestPath: "/route/v1/probe?key=<redacted>",
            statusClass: "4xx",
            outcome: "client-error",
            durationMilliseconds: 42,
            responseBytes: 2,
            responseExcerpt: "{\"error\":\"probe\"}"
        )
        providerConsoleFeed.record(
            event: LogEvent(
                timestampMillisecondsSinceEpoch: UInt64(
                    Date().timeIntervalSince1970 * 1000
                ),
                severity: .warn,
                subsystemTarget: "provider_observation",
                messageLine: "probe provider request warning",
                providerRequestTrace: probeTrace
            )
        )
    }
}
