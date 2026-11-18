//! The static configuration of the reference application: named constants
//! for every tunable value (the repository's no-magic-numbers rule), the
//! readiness-ladder and consumer-string texts, and the attribution notice.

import Foundation
import RusticMapsSDK

/// The reference application's named constants and fixed strings.
enum ReferenceApplicationConfiguration {
    // MARK: Provider credentials (docs/ReferenceAppUX.md §6.5)

    /// The provider key the registration dispatch arm answers to (the shared
    /// provider registry's descriptor key).
    static let mapquestProviderKey = "mapquest"

    /// The launch-environment variable that supplies an automation credential
    /// at runtime (local/CI UI tests). It is applied through the public
    /// registration path, never persisted by the injection and never logged;
    /// it is not a build-time key seam.
    static let runtimeCredentialEnvironmentKey = "PROVIDER_API_KEY"

    /// The launch-environment variable that names the provider the automation
    /// credential belongs to. Absent, the injection targets the only provider
    /// the registry declares available on iOS.
    static let runtimeProviderKeyEnvironmentKey = "PROVIDER_KEY"

    /// The launch-environment variable that replaces the registry's keyless
    /// search provider deployment address (the type-ahead automation seam).
    /// It is not a credential, is never persisted, and is read only to point
    /// an automated session at a same-origin fixture server; every ordinary
    /// session uses the registry's declared demo address.
    static let typeAheadDeploymentBaseURLEnvironmentKey = "SEARCH_PROVIDER_BASE_URL"

    /// The registry's keyless search provider deployment override for this
    /// process, or nil when the registry's own demo address serves.
    static func typeAheadDeploymentBaseURLOverride() -> String? {
        ProcessInfo.processInfo.environment[
            typeAheadDeploymentBaseURLEnvironmentKey
        ].flatMap { value in value.isEmpty ? nil : value }
    }

    /// The launch-environment variable that replaces the registry's keyless
    /// routing provider deployment address (the routing automation seam). It
    /// is not a credential, is never persisted, and is read only to point an
    /// automated session at a loopback fixture server; every ordinary session
    /// uses the registry's declared demo address.
    static let routingDeploymentBaseURLEnvironmentKey = "ROUTING_PROVIDER_BASE_URL"

    /// The registry's keyless routing provider deployment override for this
    /// process, or nil when the registry's own demo address serves.
    static func routingDeploymentBaseURLOverride() -> String? {
        ProcessInfo.processInfo.environment[
            routingDeploymentBaseURLEnvironmentKey
        ].flatMap { value in value.isEmpty ? nil : value }
    }

    /// The `Info.plist` key that carries the hosted demo's own credential.
    /// The build expands it from the
    /// `HOSTED_DEMO_PROVIDER_API_KEY` build setting, which the hosted-demo
    /// pipeline passes from a secret on the build machine; every ordinary
    /// build expands it to the empty string, so a distributed app carries no
    /// key. See `hostedDemoBundleCredential()`.
    static let hostedDemoProviderAPIKeyInfoKey = "HostedDemoProviderAPIKey"

    /// The `Info.plist` key naming the provider the hosted credential belongs
    /// to, expanded from the `HOSTED_DEMO_PROVIDER_KEY` build setting.
    static let hostedDemoProviderKeyInfoKey = "HostedDemoProviderKey"

    /// The hosted demo's credential, when this build carries one. Returns
    /// `nil` for every ordinary build and for an unexpanded placeholder; the
    /// caller applies the value through the public registration path, which
    /// never persists and never logs it.
    static func hostedDemoBundleCredential() -> (providerKey: String?, credential: String)? {
        let apiKey = (Bundle.main.object(forInfoDictionaryKey: hostedDemoProviderAPIKeyInfoKey) as? String)
            .flatMap { value in value.isEmpty || value.hasPrefix("$(") ? nil : value }
        guard let apiKey else {
            return nil
        }
        let providerKey = (Bundle.main.object(forInfoDictionaryKey: hostedDemoProviderKeyInfoKey) as? String)
            .flatMap { value in value.isEmpty || value.hasPrefix("$(") ? nil : value }
        return (providerKey, apiKey)
    }

    /// The keyless state's honest line: no credential is configured, so the
    /// application loads no live map data until one is entered.
    static let keylessReadinessText = "Set a provider key to load live map data"

    /// The readiness chip's short keyless line (the full line lives in the
    /// settings view and the model's readiness text).
    static let readinessChipKeylessText = "Set provider key"

    /// The settings surface's title.
    static let settingsTitleText = "Settings"

    /// The settings surface's close label.
    static let settingsCloseLabel = "Close"

    /// The settings surface's reset row: clears the shared remembered choices
    /// while leaving the opt-in credential store untouched.
    static let resetRememberedChoicesLabel = "Reset remembered choices"

    /// The settings gear control's accessibility label.
    static let settingsControlLabel = "Open settings"

    /// The provider-credentials group's label.
    static let providerCredentialsGroupLabel = "Provider credentials"

    // MARK: Provider capabilities (one provider per capability)

    /// The settings sheet's per-capability group label.
    static let capabilityProvidersGroupLabel = "Provider capabilities"

    /// A capability row's serving line while no provider is installed.
    static let capabilityNotConfiguredText = "Not configured"

    /// A capability row's picker action.
    static let capabilityChangeActionText = "Change"

    /// The composed provider row: the provider's display name and its
    /// credential state, never one without the other
    /// (`settings.provider_row`: `{provider} · {state}`).
    static let providerRowFormat = "%@ · %@"

    /// The provider row's state while its credential is in force or it
    /// declares none (`settings.provider_state_ready`).
    static let providerStateReadyText = "Ready"

    /// One composed provider row.
    static func providerRowText(provider: String, state: String) -> String {
        String(format: providerRowFormat, provider, state)
    }

    /// A row's state while a keyless provider declares no public demo
    /// deployment address, so the selection cannot reach any server.
    static func missingDemoEndpointReason(provider: String) -> String {
        "\(provider) declares no demo deployment address."
    }

    /// The routing row's demo deployment address line
    /// (`settings.routing_demo_endpoint`: `Demo server: {url}`).
    static let routingDemoEndpointFormat = "Demo server: %@"

    /// One routing row's demo deployment address line.
    static func routingDemoEndpointText(url: String) -> String {
        String(format: routingDemoEndpointFormat, url)
    }

    /// The next step under a routing provider whose demo server did not
    /// answer (`settings.routing_unreachable_next_step`).
    static let routingUnreachableNextStepText =
        "The demo server did not answer. Retry, or enter your own deployment address."

    /// The read-only raster- and vector-tile rows' note: their provider
    /// follows the map's layer control rather than this section.
    static let capabilityLayerControlNoteText =
        "Changed from the map's layer control"

    /// A credential-requiring provider's state label
    /// (`settings.provider_state_needs_credential`: `Needs {credential}`).
    static let providerStateNeedsCredentialFormat = "Needs %@"

    /// A capability picker's option label while the provider's credential is
    /// not applied (the descriptor's own credential label fills the slot).
    static func capabilityNeedsCredentialText(credentialLabel: String) -> String {
        String(format: providerStateNeedsCredentialFormat, credentialLabel)
    }

    /// The credential row's Apply label.
    static let applyCredentialLabel = "Apply"

    /// The credential row's Clear label.
    static let clearCredentialLabel = "Clear"

    /// The credential row's not-applied status line (`%@` is the provider's
    /// display name; the shared canonical `{provider} key not applied`).
    static let credentialNotAppliedFormat = "%@ key not applied"

    /// The credential row's applied status line (`%@` is the provider's
    /// display name; the shared canonical `{provider} key applied`).
    static let credentialAppliedFormat = "%@ key applied"

    /// The credential row's applied status line when the credential is also
    /// stored on the device (the canonical applied line plus the opt-in's
    /// device-local note).
    static let credentialAppliedAndRememberedFormat =
        "%@ key applied · remembered on this device"

    /// The credential row's empty-entry refusal.
    static let credentialEntryRequiredText = "Enter the provider key before applying it."

    /// The credential row's prompt after a base-map row asked for this
    /// provider's key (the reason is a next step, never a dead control).
    static let credentialEnterHintText = "Enter a key, then Apply."

    /// The credential row's status while the SDK is not running.
    static let credentialSdkNotRunningText =
        "The SDK is not running; the readiness ladder states why."

    /// The credential row's status when the running session no longer holds
    /// the key but the SDK registry cannot be unregistered at runtime.
    static let credentialClearedRestartRequiredText = """
        Key cleared. Restart the app for this session to run without a \
        provider key; the running SDK keeps the registration until then.
        """

    /// The credential row's disabled-with-reason line when the application
    /// has no registration arm for the provider yet (§6.2).
    static func providerNotRegisteredReason(displayName: String) -> String {
        "\(displayName) is not registered by this application yet."
    }

    /// The credential row's status when the applier has no arm for the
    /// provider key.
    static func unknownProviderReason(providerKey: String) -> String {
        "This application has no registration path for the provider key '\(providerKey)'."
    }

    /// The honest secure-store failure line: the Keychain refused, so the
    /// credential stays session-only (never a plaintext fallback).
    static func credentialStoreUnavailableReason(detail: String) -> String {
        "The Keychain could not store this key: \(detail) The key stays available for this session only."
    }

    /// The honest secure-store failure line when removing the stored copy
    /// failed.
    static func credentialStoreClearFailedReason(detail: String) -> String {
        "The Keychain could not remove the stored key: \(detail) The running session is unchanged."
    }

    /// The "Remember on this device" row's label.
    static let rememberCredentialLabel = "Remember on this device"

    /// The "Remember on this device" row's explanatory line.
    static let rememberCredentialExplanation = """
        Stores the key in this device's Keychain. Off keeps it only until the \
        app quits.
        """

    /// The settings view's line naming the secure store the opt-in uses.
    static let credentialStoreExplanation = """
        Remembered keys are stored in this device's Keychain; nothing is \
        written to the demo artifact or the application bundle.
        """

    // MARK: Base map (M9's native base-map switch)

    /// The map layers panel's base-map group label.
    static let baseMapGroupLabel = "Base map"

    /// The serving base map row's status label.
    static let baseMapServingStatusText = "Serving"

    /// A non-serving base map row's status label (the row's own tap act).
    static let baseMapSelectStatusText = "Select"

    /// The offline card's honest line after a credential-less base map took
    /// the raster slot: the tile cache went with the provider that installed
    /// it.
    static let baseMapUncachedNoteText = """
        Serving base map runs uncached; select a keyed base map to use the \
        cache.
        """

    // MARK: Base-map styled vector note

    /// The limit line while a styled vector base serves: the committed styles
    /// paint fills, lines and circles, plus Latin place names and road names
    /// from the SDK's embedded glyph range and route-reference shield badges
    /// where the style declares them.
    static let vectorLabelsNoteText = """
        Vector styles draw land, water, roads with their names, and Latin \
        place names.
        """
    /// Selecting a vector style before the shipped MapView exists cannot
    /// install anything; the row says what to wait for instead of failing.
    static let baseMapViewNotReadyText = """
        Map view not ready; the style applies after the first frame.
        """
    /// The honest tile-failure line under an active vector base: the frame
    /// skipped the failed tiles and the next frame retries them.
    static func vectorBaseFailedStatusText(failedTileCount: UInt32) -> String {
        "\(failedTileCount) vector base tiles failed this frame; the next "
            + "frame retries."
    }

    /// The lowest zoom level a credential-less raster base map declares (a
    /// URL-template source covers its published range from level zero).
    static let baseMapMinimumZoomLevel: UInt8 = 0

    // MARK: Directions (PRD.md §6 item 36)

    /// The Directions section's group label.
    static let directionsGroupLabel = "Directions"
    /// The mode row's label.
    static let directionsModeRowLabel = "Mode"
    /// The strategy row's label.
    static let directionsStrategyRowLabel = "Strategy"
    /// The disabled reason for a mode the serving provider cannot route for.
    static func directionsModeUnavailableReason(modeLabel: String) -> String {
        "\(modeLabel) is not offered by the serving routing provider."
    }
    /// The disabled reason for the Shortest strategy when the serving
    /// provider reports no shortest-route support.
    static let directionsShortestUnavailableReason =
        "Shortest is not offered by the serving routing provider."
    /// The diagnostics block's title for the last plan's options.
    static let directionsDiagnosticsBlockTitle = "Directions"

    // MARK: Readiness ladder (docs/ReferenceAppUX.md §6.1)

    /// The loading text shown while the SDK and the surface resolve (ends
    /// in an ellipsis, no spinners).
    static let startingReadinessText = "Starting the Maps SDK…"
    /// The loading text for the surface-attach phase.
    static let attachingReadinessText = "Preparing the map surface…"
    /// The ready text: the live guidance that the GPU renderer is active
    /// (the `%@` carries the compiled SDK version).
    static let readyReadinessFormat = "Maps SDK gpu renderer active (version %@)"
    /// The iOS reference application has no documented capability
    /// substitute today (the wasm 2D tile-grid path has no native
    /// counterpart in this slice), so `degraded` is reserved for later
    /// capability-substitution failures; the state exists in the ladder.
    static let degradedReadinessText = "Maps SDK degraded renderer active (no capability substitute is documented for the native surface path)"

    /// The readiness chip's short ready line (§4.2's chip states; the full
    /// readiness text stays in the diagnostics sheet).
    static let readinessChipReadyText = "Ready"
    /// The readiness chip's short loading line.
    static let readinessChipLoadingText = "Loading…"
    /// The prefix the chip lines of the degraded state carry.
    static let readinessChipDegradedPrefix = "Degraded: "
    /// The prefix the chip lines of the failed state carry.
    static let readinessChipFailedPrefix = "Failed: "

    // MARK: Sheet layout glue (docs/ReferenceAppUX.md §1, §4.2)

    /// The sheet grabber's capsule width in points (the §4.2 grabber's
    /// platform-glue sizing; its corner insets are the `gutter-inline`
    /// token).
    static let sheetGrabberWidth: CGFloat = 48

    /// The sheet grabber's capsule height in points.
    static let sheetGrabberHeight: CGFloat = 5

    /// The fold-drag translation (points) that steps the sheet one fold
    /// state.
    static let sheetFoldGestureThresholdPixels: CGFloat = 24

    // MARK: Offline maps (docs/ReferenceAppUX.md §4.1)

    /// The absolute byte budget the demo registers its raster tile cache
    /// with — a demo-sized budget, named constants all the way (16 MiB).
    static let offlineTileCacheAbsoluteMaximumBytes: UInt64 = 16 * 1024 * 1024

    /// The caches-directory subfolder the demo's tile-cache database lives
    /// in (the cache never enters the application bundle or git).
    static let offlineTileCacheFolderName = "tile-cache"

    /// The tile-cache database file name inside the caches subfolder.
    static let offlineTileCacheDatabaseFileName = "tile-cache.db"

    /// The binary unit factor (1024) and name the diagnostics and the
    /// offline card format byte budgets with.
    static let bytesPerBinaryMegabyte: UInt64 = 1024 * 1024

    /// The offline card's line while the tile cache is registered.
    static let offlineCardRegisteredFormat = """
        Tile caching offline: %@ (%@ budget)
        """

    /// The offline card's live tile line (the per-draw `FrameStatistics`
    /// values, formatted as one line).
    static let offlineCardTileStatisticsFormat = """
        Tiles last frame: %@ requested · %@ drawn (%@ from the parent level) · \
        %@ deferred · %@ unavailable · overlay triangles %@
        """

    /// The offline card's line before the first drawn frame.
    static let offlineCardNoFrameYetText = "Tiles last frame: the first map frame has not drawn yet…"

    // MARK: Offline region downloads (crates/rustic-maps-cache/src/tile_packs.rs)

    /// The offline card's region-download control while no download runs
    /// (§2.5 `offline.download_action`).
    static let offlineRegionDownloadActionText = "Download offline region"

    /// The in-flight line's leading text; the tile counts and the
    /// network-versus-disk split follow it (§2.5 `offline.downloading_status`).
    static let offlineRegionDownloadingStatusText = "Downloading…"

    /// The in-flight line's control that stops the download
    /// (§2.5 `offline.cancel_action`).
    static let offlineRegionCancelActionText = "Cancel"

    /// The terminal line when every region tile is stored
    /// (§2.5 `offline.completed_status`).
    static let offlineRegionCompletedStatusText = "Download complete"

    /// The terminal line when the download stops on an error
    /// (§2.5 `offline.failed_status`).
    static let offlineRegionFailedStatusText = "Download failed"

    /// The stored-region row's tile-count label
    /// (§2.5 `offline.stored_tiles_label`).
    static let offlineRegionStoredTilesLabel = "Stored tiles"

    /// The stored-region row's payload-size label
    /// (§2.5 `offline.stored_bytes_label`).
    static let offlineRegionStoredBytesLabel = "Stored bytes"

    /// The stored-region row's removal control
    /// (§2.5 `offline.delete_action`).
    static let offlineRegionDeleteActionText = "Delete region"

    /// The line after a stored region is removed
    /// (§2.5 `offline.deleted_status`).
    static let offlineRegionDeletedStatusText = "Region deleted"

    /// The browser's honest limitation statement, carried in the shared copy
    /// table (§2.5 `offline.browser_unavailable_status`); the native card
    /// never renders it because the native surface has a disk store.
    static let offlineRegionBrowserUnavailableStatusText =
        "Offline region downloads are not available in the browser."

    /// The in-flight line's full format: the leading status text, completed
    /// and total tile counts, and the network-versus-disk split. All slots
    /// are strings because the counts are 64-bit.
    static let offlineRegionDownloadProgressFormat = """
        %@ %@/%@ tiles · %@ fetched from the network · %@ from disk
        """

    /// The terminal line after a cancellation: the partial region stays on
    /// disk and a later download resumes it.
    static let offlineRegionCancelledStatusText =
        "Download cancelled; the stored tiles are kept for a later download."

    /// The demo region row's summary while its tile count is known.
    static let offlineRegionDemoSummaryFormat = "%@ · zoom %@–%@ · %@ tiles"

    /// The demo region row's summary before the tile count can be read (no
    /// tile cache installed).
    static let offlineRegionDemoSummaryWithoutCountFormat = "%@ · zoom %@–%@"

    /// A stored-region row's tile-count line.
    static let offlineRegionStoredTilesLineFormat = "%@ %@ / %@"

    /// A stored-region row's payload-size line.
    static let offlineRegionStoredBytesLineFormat = "%@ %@"

    /// A stored-region row's retention line (whole days).
    static let offlineRegionRetentionLineFormat = "Retained %@ days"

    /// The contract name of the tile-cache capability in a
    /// `ProviderNotConfigured` refusal.
    static let tileCacheContractName = "TileCache"

    /// The contract's canonical missing-provider sentence
    /// (`strings.provider_not_configured_detail_template` in
    /// `platform/reference-tokens.json`); the generated error's own
    /// `errorDescription` reflects the enum case instead, so the reference
    /// surfaces render this sentence.
    static func providerNotConfiguredDetailText(contractName: String) -> String {
        "no '\(contractName)' provider is configured"
    }

    /// The seconds in one day, for the retention phrase.
    static let secondsPerDay: UInt64 = 86_400

    /// The bytes in one binary kilobyte (1024), for payload phrases below a
    /// megabyte.
    static let bytesPerBinaryKilobyte: UInt64 = 1_024

    /// Formats a payload byte count as a binary-size phrase: MiB with one
    /// decimal at and above a megabyte, KiB below it.
    static func binaryByteFootprintPhrase(bytes: UInt64) -> String {
        if bytes >= bytesPerBinaryMegabyte {
            return String(
                format: "%.1f MiB",
                Double(bytes) / Double(bytesPerBinaryMegabyte)
            )
        }
        return String(
            format: "%.1f KiB",
            Double(bytes) / Double(bytesPerBinaryKilobyte)
        )
    }

    // MARK: The offline archive route (§2.5's offline routing copy)

    /// The committed compiled routing archive the offline card's route
    /// control reads from the application bundle (the same test graph the
    /// wasm seam and the contract tests use).
    static let offlineRoutingArchiveResourceName = "routing_grid"
    static let offlineRoutingArchiveResourceExtension = "rmog"
    // The offline archive's demo request: the routing-grid fixture's straight
    // oneway street, the same pair the wasm seam's test and the book's
    // snippet use. The contract call takes the archive profile's defaults
    // (automobile, fastest, metric).
    static let offlineRouteStartLatitudeDegrees = 0.001
    static let offlineRouteStartLongitudeDegrees = 0.0
    static let offlineRouteEndLatitudeDegrees = 0.001
    static let offlineRouteEndLongitudeDegrees = 0.002
    /// The offline card's route control label (§2.5 `offline.plan_route_action`).
    static let offlineRouteActionText = "Plan offline route"
    /// The route summary's title for an offline archive route (§2.5).
    static let offlineRouteTitleText = "Offline demo route"
    /// The route summary's honesty note for an offline archive route (§2.5).
    static let offlineRouteNoteText =
        "Planned offline from a recorded test graph, not live data."

    // MARK: Demo plans (docs/ReferenceAppUX.md §4.1: the recorded trace picker)
    // The picker's rows — scenarios, fixture paths, labels and descriptions —
    // come from the shared artifact's `demo_plans` section
    // (`platform/reference-scenes.json`), never from app-side constants.

    /// The home sheet's demo-plans group label (§4.2).
    static let demoPlansGroupLabel = "Demo plans"

    // MARK: Framing and demo-scope constants

    /// The fetch-zoom ceiling of the recorded demo raster fixture
    /// neighborhood (the recorded golden zoom, 13–14 near Sacramento): camera
    /// fitting clamps to it, because tiles beyond it do not exist in the
    /// demo fixture set and every such tile would render honestly missing.
    static let demoMaximumTileZoomLevel: Double = 14

    /// The maximum pitch the camera exposes, under the renderer's
    /// property-tested pitch ceiling (the 55-degree anchor the camera
    /// property tests pin).
    static let maximumDemoPitchDegrees: Double = 55

    /// The zoom range the camera exposes (the §5.1 zoom clamp:
    /// `MINIMUM_ZOOM_LEVEL` = 0, `MAXIMUM_ZOOM_LEVEL` = 20).
    static let minimumDemoZoomLevel: Double = 0
    static let maximumDemoZoomLevel: Double = 20

    /// The edge inset (in points) the SDK fit-bounds camera keeps the
    /// planned route inside on every viewport side (comfortably wider than
    /// the sheet gutters so the sheet never covers the route).
    static let routePreviewEdgeInsetPoints: Double = 64

    /// The meters in one degree of latitude (the spherical approximation
    /// the demo's off-route offset uses — a demo-scale constant, not
    /// navigation math).
    static let metersPerDegreeLatitude: Double = 111_320

    // MARK: Draw delegate readback

    /// The shortest interval between two draw-frame readback publications,
    /// in seconds. A frame whose statistics and camera match the last
    /// published ones publishes nothing; a changed frame publishes at most
    /// once per this window, so a busy or animating map cannot drive a
    /// SwiftUI change on every presented frame.
    static let frameReadbackPublishMinimumIntervalSeconds: TimeInterval = 0.1

    // MARK: Simulated navigation (docs/ReferenceAppUX.md §7)

    /// The demo acceleration factor of the simulated journey: the recorded
    /// trace's 5-second sample cadence replays at one tenth of real time,
    /// so the ~7-minute walk demonstrates in about 40 seconds.
    static let simulatedJourneySpeedFactor: Double = 10

    /// The §2.4 `navigation-follow-zoom` the follow camera locks at trip
    /// start (§5.3: the navigation profile's zoom ladder tops at 16.35 —
    /// locked deeper per product direction so turn geometry is legible).
    static let navigationFollowZoomLevel: Double = 17

    /// The §2.4 `navigation-follow-zoom-maximum` clamp: a user-nudged zoom
    /// during a trip never deepens past it (§5.3).
    static let navigationFollowZoomMaximumLevel: Double = 17.5

    /// The §2.4 `navigation-follow-pitch-3d` (degrees): the 3D follow
    /// camera's pitch; the 2D/3D toggle sets 0 in 2D. The camera's own
    /// ceiling is `maximumDemoPitchDegrees`.
    static let navigationFollowPitchDegrees3D: Double = 45

    /// The number of fixes the §5.3 course-follow bearing median-smooths
    /// the course over (a short window so GPS jitter does not swing the
    /// camera; the demo trace's mild directionality carries fine).
    static let courseFollowSmoothingFixes: Int = 5

    /// The minimum fix displacement (meters) below which a fix is too
    /// stationary to produce a new course-over-ground sample (the previous
    /// sample persists).
    static let courseFixMinimumDisplacementMeters: Double = 1

    // MARK: Diagnostics events (the sheet's recent-events readout)

    /// The minimum severity the diagnostics pane subscribes at when it
    /// opens. The pane starts at Info; the explicit Show debug events switch
    /// lowers it to `diagnosticsEventsDebugSeverity` for the session.
    static let diagnosticsEventsMinimumSeverity: SdkLogSeverity = .info

    /// The artifact's own spelling of the pane's starting severity
    /// (`diagnostics-minimum-severity`: `Info`), pinned beside the SDK enum
    /// the subscription uses.
    static let diagnosticsMinimumSeverityName = "Info"

    /// The severity the explicit Show debug events switch lowers the pane's
    /// one subscription to.
    static let diagnosticsEventsDebugSeverity: SdkLogSeverity = .debug

    /// The artifact's own spelling of the debug opt-in severity
    /// (`diagnostics-debug-severity`: `Debug`), pinned beside the SDK enum.
    static let diagnosticsDebugSeverityName = "Debug"

    /// The SDK's per-subsystem diagnostics ring capacity
    /// (`diagnostics-ring-capacity`): the backlog the pane reads through
    /// `flushRecentDiagnosticEvents`.
    static let diagnosticsRingCapacity: Int = 512

    /// The Show debug events switch's label while detail is off
    /// (`diagnostics.show_debug_events`).
    static let diagnosticsShowDebugEventsText = "Show debug events"

    /// The switch's label while detail is on (`diagnostics.hide_debug_events`).
    static let diagnosticsHideDebugEventsText = "Hide debug events"

    /// The pane's Clear action (`diagnostics.clear_action`).
    static let diagnosticsClearActionText = "Clear"

    /// The pane's per-session scope note (`diagnostics.stream_scope_note`).
    static let diagnosticsStreamScopeNoteText =
        "Debug detail stays on until the app restarts."

    /// The readout's cadence coalescing (seconds): the live stream runs at
    /// several events per presented frame, so the buffer merges its
    /// deliveries into the published list at this slower cadence and keeps
    /// the sheet (and the frame pump) off a per-event SwiftUI churn.
    static let diagnosticsRefreshCadenceMilliseconds: Int = 500
    static let diagnosticsEventsRefreshCadenceSeconds: TimeInterval =
        Double(diagnosticsRefreshCadenceMilliseconds) / 1000

    /// The bounded display window: the pane shows the most recent this-many
    /// events and drops the oldest.
    static let diagnosticsEventWindowCount: Int = 100

    /// The maximum count the sheet's backlog flush delivers (the ring may
    /// hold more; the readout's scroll absorbs the answer).
    static let diagnosticsBacklogFlushLimit: Int = 100
    static let diagnosticsEventsFlushMaximumCount: UInt32 =
        UInt32(diagnosticsBacklogFlushLimit)

    /// The buffered readout's capacity (events older than this are dropped
    /// from the sheet's oldest-first list; the live stream keeps flowing).
    static let diagnosticsEventsBufferedCapacity: Int =
        diagnosticsEventWindowCount

    /// The time format the event rows render their wall-clock timestamps
    /// with (local time, millisecond precision).
    static let diagnosticsEventTimeFormat = "HH:mm:ss.SSS"

    /// The severity-label rendering of an event row (`SdkLogSeverity`
    /// lowercased, e.g. "info").
    static func diagnosticsSeverityLabel(_ severity: SdkLogSeverity) -> String {
        switch severity {
        case .trace: return "trace"
        case .debug: return "debug"
        case .info: return "info"
        case .warn: return "warn"
        case .error: return "error"
        }
    }

    /// The ordered rank of one severity (trace lowest, error highest), so
    /// the pane can apply its display filter without an enum comparison.
    static func diagnosticsSeverityRank(_ severity: SdkLogSeverity) -> Int {
        switch severity {
        case .trace: return 0
        case .debug: return 1
        case .info: return 2
        case .warn: return 3
        case .error: return 4
        }
    }

    /// Whether one severity meets the pane's display minimum.
    static func diagnosticsSeverityMeets(
        _ severity: SdkLogSeverity,
        minimum: SdkLogSeverity
    ) -> Bool {
        diagnosticsSeverityRank(severity)
            >= diagnosticsSeverityRank(minimum)
    }

    /// The events block's empty line while no event has been delivered
    /// (the backlog flush and the live stream fill it;
    /// `diagnostics.no_events`).
    static let diagnosticsNoEventsText = "No SDK events recorded yet."

    /// The wall-clock formatter the event rows render their timestamps
    /// with (the configured §2.4 pattern, reused across rows).
    static let diagnosticsEventTimeFormatter: DateFormatter = {
        let eventTimeFormatter = DateFormatter()
        eventTimeFormatter.dateFormat = diagnosticsEventTimeFormat
        return eventTimeFormatter
    }()

    // MARK: Provider console (M58)

    /// The console sheet's title.
    static let providerConsoleTitleText = "Provider console"

    /// The console detail view's title.
    static let providerConsoleDetailTitleText = "Request detail"

    /// The console's export action (opens the share sheet).
    static let providerConsoleExportActionText = "Export"

    /// The floating control's accessible name.
    static let providerConsoleOpenAccessibilityLabel = "Provider console"

    /// The floating control's accessible value while the unread dot shows.
    static let providerConsoleUnreadAccessibilityValue =
        "New warnings or errors"

    /// The console's honest empty state; it names the actions that issue a
    /// provider request.
    static let providerConsoleEmptyText =
        "No console events yet. Search, plan a route or load a tile to see one."

    /// The response-excerpt capture switch's label.
    static let providerConsoleCaptureLabelText = "Capture response excerpts"

    /// The capture switch's scope note.
    static let providerConsoleCaptureNoteText =
        "Excerpts may contain provider content and are never uploaded."

    /// The level filter's accessible name.
    static let providerConsoleLevelFilterAccessibilityLabel = "Event level"

    /// The three level filter labels (Error / Warning / Info; Info shows
    /// all three).
    static let providerConsoleLevelErrorsLabel = "Error"
    static let providerConsoleLevelWarningsLabel = "Warning"
    static let providerConsoleLevelInformationLabel = "Info"

    /// The console's bounded row count (the feed keeps the most recent
    /// this-many events; the SDK ring keeps its own bounded backlog).
    static let providerConsoleMaximumRows: Int = 100

    /// The characters a console row shows from the response excerpt before
    /// the detail view shows the full captured prefix.
    static let providerConsoleRowExcerptCharacterLimit: Int = 120

    /// The exported text's UTF-8 byte budget (256 KB); a longer export is
    /// truncated on a character boundary with the note below.
    static let providerConsoleExportMaximumUTF8ByteCount: Int = 256 * 1024

    /// The note a truncated export ends with.
    static let providerConsoleExportTruncationNote = "\n\n[truncated at 256 KB]"

    /// The export header and its two summary lines.
    static let providerConsoleExportHeaderText = "Provider console"
    static let providerConsoleExportFilterPrefix = "Level: "
    static let providerConsoleExportRowCountPrefix = "Events: "

    /// The excerpt placeholder while the capture switch is off.
    static let providerConsoleResponseExcerptOffText =
        "Response excerpt capture is off."

    /// The tile-cache detail rows' honest texts. The cache reports exact
    /// payload hits and misses; the last frame's tile pipeline counts are a
    /// separate row and are labelled as such.
    static let providerConsoleTileCacheUnregisteredText =
        "No tile cache is registered."
    static let providerConsoleTileCacheUnavailableValueText =
        "unavailable"
    static let providerConsoleTileFrameRowLabel = "Last frame"

    /// The traffic-cache detail rows' absence state.
    static let providerConsoleTrafficCacheUnavailableText =
        "Unavailable — no traffic provider is registered."

    /// The launch-argument key that opens the provider console at startup
    /// (`-DemoOpenProviderConsole YES`): the automated evidence pass and the
    /// UI suite drive the console's own open path without touch injection.
    static let launchArgumentProviderConsoleOpen = "DemoOpenProviderConsole"

    /// The launch-argument key that records one probe Warning through the
    /// console feed (`-DemoConsoleProbeWarning YES`): the UI suite's seam for
    /// the unread Warning/Error dot. A normal launch never records it.
    static let launchArgumentProviderConsoleProbeWarning =
        "DemoConsoleProbeWarning"

    /// The sample cadence the recorded trace fixtures were recorded at, in
    /// fixture seconds (the recorded replay's cadence divides it by the demo
    /// acceleration and the selected speed multiplier).
    static let simulatedJourneySampleIntervalSeconds: Double = 5

    /// One route-following walk tick (seconds of real time): the walker's
    /// clock advances `routeWalkTickIntervalSeconds ×
    /// simulatedJourneySpeedFactor` simulated seconds per fix, so the
    /// position moves continuously between the route's shape vertices.
    static let routeWalkTickIntervalSeconds: Double = 0.1

    /// The shortest replay dispatch interval (seconds), so a fast speed
    /// multiplier never spins the timer.
    static let minimumJourneyDispatchIntervalSeconds: Double = 0.001

    /// The playback speed multipliers the drawer offers: the
    /// walker's pace is the route's own average speed times the choice, and
    /// a recorded trace's replay cadence divides by the same choice.
    static let simulatedTripSpeedMultiplierChoices: [Double] = [1, 2, 4, 8]

    /// The horizontal accuracy (meters) the simulated journeys report per
    /// fix (the same accuracy class the recorded trace's generator fixes
    /// carry).
    static let simulatedJourneyFixAccuracyMeters: Double = 8

    /// The mean Earth radius (meters, spherical) of the recorded replay's
    /// fix-to-fix course measure.
    static let meanEarthRadiusMeters: Double = 6_371_000

    /// The fallback walking speed (meters per second) the planned-route
    /// journey falls back to when a planned route carries no usable
    /// duration estimate.
    static let plannedRouteFallbackWalkSpeedMetersPerSecond: Double = 1.4

    /// How far east (in meters) the off-route simulation displaces the
    /// simulated fixes while its toggle is on — well beyond the default
    /// 25-meter deviation threshold widened by the simulated fix accuracy,
    /// so the deviation always fires.
    static let offRouteSimulationOffsetMeters: Double = 60

    /// The trip's own time formatting: the seconds above which the drawer's
    /// ETA line switches from raw seconds to whole minutes (60).

    /// The distance threshold the drawer's ETA line switches meters to
    /// kilometers at (the metric phrase boundary, 1000 m).
    static let tripEtaKilometerThresholdMeters: Double = 1000

    /// The route summary card's duration formatting: the threshold above
    /// which whole hours carry the minutes as a second value (60).

    // MARK: Launch automation (evidence-capture support)

    /// The launch-argument key that auto-starts the simulated journey once
    /// the application finishes starting (`-DemoStartSimulatedJourney
    /// YES`): the planned route drives the trip when one was planned, the
    /// recorded trace fixture otherwise — the automated evidence pass
    /// drives the search → route → navigate flow without touch injection
    /// (simctl cannot tap).
    static let launchArgumentSimulatedJourneyAutostart = "DemoStartSimulatedJourney"

    /// The launch-argument key that selects the light theme at startup
    /// (`-DemoSelectLightTheme YES`): the automated evidence pass captures
    /// the light-theme presentation without touching the toggle.
    static let launchArgumentLightThemeSelection = "DemoSelectLightTheme"

    /// The launch-argument key that clears the remembered choices at startup
    /// (`-DemoResetRememberedChoices YES`): the UI suite uses it to isolate
    /// its relaunch cases without another session's choices leaking in.
    static let launchArgumentResetRememberedChoices = "DemoResetRememberedChoices"

    /// The launch-argument key that names a shared reference flow to run at
    /// startup (`-DemoReferenceFlow <flow-id>`): the same user-path methods
    /// the sheet's controls call, so the flow schema's four flows are
    /// driveable without touch injection (simctl cannot tap).
    static let launchArgumentReferenceFlow = "DemoReferenceFlow"

    /// The shared flow schema's flow identifiers.
    static let referenceFlowPlanAndNavigate = "plan_and_navigate"
    static let referenceFlowMyLocationField = "my_location_field"
    static let referenceFlowRoutingProviderSwitch = "routing_provider_switch"
    static let referenceFlowDiagnosticsOptIn = "diagnostics_opt_in"

    /// The launch-argument key that runs the route-planning demo at
    /// startup (`-DemoPlanRoute YES`): geocodes the destination query,
    /// selects its first result and plans the route with the SDK
    /// fit-bounds framing — the automated evidence pass captures the
    /// search-and-route flow without touch injection. The canned query is
    /// the demo city's main station, chosen so the live geocoder resolves
    /// it reliably.
    static let launchArgumentRoutePlanningDemo = "DemoPlanRoute"

    /// The canned destination geocode query of the route-planning launch
    /// automation (the origin defaults to the demo position).
    static let routePlanningDemoDestinationQuery = "Sutter's Fort, Sacramento"

    /// The launch-argument key that runs the keyless demo search
    /// (`-DemoKeylessSearch YES`): filters the shared artifact's
    /// `demo_places` by the first demo place's name and leaves the full
    /// search view showing the demo rows — no provider credential and no
    /// provider call.
    static let launchArgumentKeylessSearchDemo = "DemoKeylessSearch"

    /// The launch-argument key that runs the keyless demo plan
    /// (`-DemoKeylessPlan YES`): searches and assigns the first recorded
    /// `demo_routes` pair's endpoints and runs the explicit Plan route
    /// action, so the route summary presents the recorded demo route
    /// without a provider credential.
    static let launchArgumentKeylessPlanDemo = "DemoKeylessPlan"

    /// The launch-argument key that fills both plan fields with the first
    /// recorded `demo_routes` pair and leaves the plan sheet open at full
    /// (`-DemoFillPlanEndpoints YES`) — the published plan-sheet capture,
    /// with no provider credential and no route planned.
    static let launchArgumentFillPlanEndpoints = "DemoFillPlanEndpoints"

    /// The launch-argument key that fills both plan fields with the first
    /// recorded `demo_routes` pair's endpoint names and leaves the plan sheet
    /// open at full (`-DemoCapturePlanEndpoints YES`), without a search
    /// provider call. The keyed screenshot capture uses it so the published
    /// plan sheet shows the reference endpoints rather than a live provider's
    /// best guess.
    static let launchArgumentCapturePlanEndpoints = "DemoCapturePlanEndpoints"

    /// The launch-argument key that selects a recorded demo plan at startup
    /// (`-DemoPlanScenario <scenario-name>`): the picker's own selection act,
    /// so the recorded plan's route-preview framing and route line are
    /// driveable without touch injection. An unknown scenario name leaves the
    /// default selection in force.
    static let launchArgumentDemoPlanScenario = "DemoPlanScenario"

    /// The launch-argument key that runs the tutorial's worked example at
    /// startup (`-DemoWalkthrough YES`): the same steps the guides publish,
    /// so the generated chapters describe code this application really
    /// executes (`docs/reference/DocumentationSnippets.md`).
    static let launchArgumentWalkthroughDemo = "DemoWalkthrough"

    /// The launch-argument key that selects a base map at startup by provider
    /// key (`-DemoBaseMap <provider-key>`). The hosted demo uses it to open on a
    /// keyless source, so the first screen is imagery rather than an empty
    /// surface; no credential is involved.
    static let launchArgumentDemoBaseMap = "DemoBaseMap"

    /// The origin the walkthrough searches for; the demo corridor's start.
    static let walkthroughOriginSearchQuery = "Capitol Park, Sacramento, California"

    /// The destination the walkthrough searches for; the corridor's end.
    static let walkthroughDestinationSearchQuery = "Tower Bridge, Sacramento, California"

    /// The launch-argument key that runs the simulated journey with the
    /// off-route displacement active (`-DemoRunOffRoute YES`): the journey
    /// starts with the deviation toggle on, so every sampled fix sits
    /// beyond the deviation threshold and the deviation alert (§7.2) is
    /// captured while the guidance freezes — the automated evidence pass
    /// without touch injection.
    static let launchArgumentOffRouteSimulation = "DemoRunOffRoute"

    /// The launch-argument key that opens the diagnostics sheet at startup
    /// (`-DemoOpenDiagnostics YES`): the automated evidence pass captures
    /// the diagnostics sheet (the readiness chip's destination) without
    /// touch injection.
    static let launchArgumentDiagnosticsOpen = "DemoOpenDiagnostics"

    /// The launch-argument key that grows the home sheet to its half state
    /// at startup (`-DemoSetSheetHalf YES`): the automated evidence pass
    /// captures the half state (demo scenes, offline maps, theme) without
    /// touch injection.
    static let launchArgumentSheetHalfState = "DemoSetSheetHalf"

    /// The launch-argument key that opens the search view (the sheet's
    /// full state) at startup (`-DemoOpenSearch YES`).
    static let launchArgumentSearchOpen = "DemoOpenSearch"

    /// The launch-argument key that starts the recorded sample journey even
    /// when a planned route exists (`-DemoStartSampleJourney YES`): the
    /// recorded trace drives the trip instead of the planned route, so the
    /// recorded-trace journey replay stays evidence-captured on the same flags.
    static let launchArgumentSampleJourneyAutostart = "DemoStartSampleJourney"

    /// The launch-argument key that starts a route-following trip over the
    /// selected demo plan's committed route (`-DemoStartRouteFollowingJourney
    /// YES`): the contract's `RouteSimulator` walks the route's own polyline
    /// without the network, so the shared walker's speed, pause and arrival
    /// behaviour is driveable by the UI suite. The recorded-trace path stays
    /// untouched for the demo scenarios.
    static let launchArgumentRouteFollowingJourneyAutostart =
        "DemoStartRouteFollowingJourney"

    // MARK: Round-10 launch automation (evidence-capture support)

    /// The launch-argument key that applies one zoom-in step through the
    /// same user-path method the control cluster's zoom + control calls
    /// (`-DemoTapZoomIn YES`).
    static let launchArgumentZoomInTapDemo = "DemoTapZoomIn"

    /// The launch-argument key that applies one zoom-out step through the
    /// same user-path method the control cluster's zoom − control calls
    /// (`-DemoTapZoomOut YES`).
    static let launchArgumentZoomOutTapDemo = "DemoTapZoomOut"

    /// The launch-argument key that toggles the 2D/3D pitch mode through
    /// the same user-path method the cluster's 2D/3D control calls
    /// (`-DemoToggleThreeDimensionalPitch YES`).
    static let launchArgumentThreeDimensionalPitchToggleDemo =
        "DemoToggleThreeDimensionalPitch"

    /// The launch-argument key that demonstrates the compass re-north
    /// (`-DemoRenorthCompass YES`): the camera turns to the demo's
    /// off-north bearing, settles, then the compass control's same
    /// user-path action eases north up — captured without touch injection.
    static let launchArgumentCompassRenorthDemo = "DemoRenorthCompass"

    /// The launch-argument key that starts the device location source
    /// (`-DemoStartDeviceLocation YES`): the same user-path method the
    /// Location group's control calls, for a live-fix evidence run.
    static let launchArgumentDeviceLocationAutostart = "DemoStartDeviceLocation"

    /// The off-north bearing (degrees) the compass demo turns the camera to
    /// before the eased re-north runs (a demo camera value, any non-north
    /// value above the §2.4 visibility threshold).
    static let compassRenorthDemoBearingDegrees: Double = 42

    /// The seconds the compass demo lets the off-north camera settle before
    /// the compass tap fires (the ease afterwards needs a settled start).
    static let compassRenorthDemoSettleSeconds: TimeInterval = 0.8

    /// The launch-argument key that breaks the navigation follow camera
    /// with a scripted drag mid-journey (`-DemoGestureBreakFollow YES`):
    /// the same user-path drag method a finger's pan calls runs
    /// `gestureBreakFollowDelaySeconds` into the journey, so the evidence
    /// pass captures the §5.3 gesture break and the next fix's re-engage
    /// without touch injection.
    static let launchArgumentGestureBreakFollowDemo = "DemoGestureBreakFollow"

    /// The seconds into the journey the scripted drag break fires at.
    static let gestureBreakFollowDelaySeconds: TimeInterval = 1.0

    /// The quiet period after the last gesture-driven camera change before
    /// the remembered-choice record is written once. A drag reports a camera
    /// change every frame; the record is a choice snapshot, not a frame log.
    static let cameraGesturePersistDebounceSeconds: TimeInterval = 0.35

    /// The launch-argument key that pans the map one rightward drag through
    /// the drag gesture's same user-path method (`-DemoGesturePanContent
    /// YES`): the §5.1 content-follows-pointer direction's evidence read —
    /// a rightward drag moves the map content right, so the camera center
    /// moves west and the diagnostics' center row drops below the launch
    /// preset's longitude. Captured without touch injection.
    static let launchArgumentGesturePanDemo = "DemoGesturePanContent"

    /// The drag delta (points) the pan demo's user-path act pans by (the
    /// direction is the assertion's subject; a pure-rightward delta also
    /// pins the latitude's standstill).
    static let gesturePanDemoDragDeltaPoints: CGSize = CGSize(width: 100, height: 0)

    /// The seconds the gesture demos wait before their user-path act fires
    /// (the pan and rotation camera members need an attached viewport —
    /// they no-op before the map surface configures).
    static let gestureDemoConfigureDelaySeconds: TimeInterval = 1.0

    /// The launch-argument key that turns the map through the rotation
    /// gesture's same user-path method (`-DemoGestureRotateContent YES`):
    /// the act is scheduled past the map's configure delay because the
    /// contract rotation member needs an attached viewport.
    /// the §5.1 content-follows-fingers direction's evidence read — the
    /// demo's clockwise delta must rotate the content clockwise, so the
    /// drawn bearing readback ends at the delta's reflection (launch 0° −
    /// the demo's degrees), never at the sum.
    static let launchArgumentTwoFingerRotateDemo = "DemoGestureRotateContent"

    /// The clockwise rotation (degrees) the rotate demo's user-path act
    /// turns by.
    static let twoFingerRotateDemoDegrees: Double = 45

    /// The nanoseconds in one second (the `Task.sleep` unit conversion the
    /// automation's delays format through).
    static let nanosecondsPerSecond: Double = 1_000_000_000

    /// The milliseconds in one second (the camera-animation duration unit
    /// conversion).
    static let millisecondsPerSecond: Double = 1000

    /// The drag delta (points) the scripted follow-break drag pans by (a
    /// small pan the simulator evidence camera sees as a visible break).
    static let gestureBreakFollowDragDeltaPoints: CGSize = CGSize(width: 90, height: 40)

    /// The launch-argument key that places a simulated tracking fix at the
    /// demo position (`-DemoStartTrackingFix YES`): the idle session's
    /// tracking-disc + accuracy-ring state for the renderer-owned location
    /// indicator — a demo fix on the same simulated-fix path the journeys
    /// ride, not a device fix.
    static let launchArgumentTrackingFixDemo = "DemoStartTrackingFix"

    // MARK: Consumer strings (docs/ReferenceAppUX.md §1: rows name the
    // feature, never the seam)

    /// The peek's destination search field hint ("Where to?").
    static let destinationSearchPlaceholder = "Where to?"

    /// The plan flow's destination field placeholder (`plan.destination_
    /// placeholder`: the empty field's prompt inside the full search view).
    static let destinationFieldPlaceholderText = "Search a destination"

    /// The plan flow's origin field hint (`plan.origin_hint`: the empty
    /// field's prompt; the shared canonical value).
    static let originFieldHintText = "Search an origin"

    /// The plan flow's visible origin field label (`plan.origin_label`).
    static let originFieldLabelText = "Origin"

    /// The plan flow's visible destination field label
    /// (`plan.destination_label`).
    static let destinationFieldLabelText = "Destination"

    /// The swap action that exchanges the two plan fields and their resolved
    /// places (`plan.swap_action`).
    static let swapPlanFieldsActionText = "Swap origin and destination"

    /// The plan fields' visible A/B markers, ordered origin then
    /// destination.
    static let originFieldMarkerText = "A"
    static let destinationFieldMarkerText = "B"

    /// The "My location" field chip (`plan.my_location_action`).
    static let myLocationActionText = "My location"

    /// The chip's honest failure line when the device position cannot be
    /// obtained (`plan.my_location_unavailable`: `{reason}`).
    static let myLocationUnavailableFormat = "My location is unavailable: %@"

    /// One chip's honest failure line.
    static func myLocationUnavailableText(reason: String) -> String {
        String(format: myLocationUnavailableFormat, reason)
    }

    /// Plan route's disabled-with-reason lines, one per missing field
    /// (`plan.missing_origin`, `plan.missing_destination`,
    /// `plan.missing_both`).
    static let planMissingOriginText = "Enter an origin to plan the route."
    static let planMissingDestinationText =
        "Enter a destination to plan the route."
    static let planMissingBothText =
        "Enter an origin and a destination to plan the route."

    /// The peek's trip-source line while no route is planned and the
    /// selected demo plan will supply the trip (`plan.trip_source_demo_plan`).
    static let tripSourceDemoPlanFormat = "Selected demo plan: %@"

    /// One trip-source line.
    static func tripSourceDemoPlanText(plan: String) -> String {
        String(format: tripSourceDemoPlanFormat, plan)
    }

    /// The plan flow's search-in-flight line while a geocode request runs
    /// (`plan.searching_in_flight`; loading text ends with an ellipsis, §6.4).
    static let searchInFlightText = "Searching…"

    /// The route summary's and the peek's "Start simulated navigation"
    /// primary label (§4.1).
    static let startSimulatedNavigationLabel = "Start simulated navigation"

    /// The route summary's edit affordance (§4.1: a subtle "Edit" that
    /// reopens the search view).
    static let routeEditLabel = "Edit"

    /// The home sheet's demo-scenes group label (§4.2).
    static let demoScenesGroupLabel = "Demo scenes"

    /// The demo-scenes group's clear action label.
    static let clearOverlaysActionText = "Clear overlays"

    /// The home sheet's offline group's label (§4.2).
    static let offlineMapsGroupLabel = "Offline maps"

    /// The home sheet's camera presets' label (the preset rows,
    /// consumer-worded).
    static let cameraPresetsGroupLabel = "Camera presets"

    /// The home sheet's theme row's label (§3).
    static let themeRowLabel = "Theme"

    /// The simulated navigation group's label (§4.2's consumer wording).
    static let simulatedNavigationGroupLabel = "Simulated navigation"

    /// The sample journey's label (the recorded trace demo over the
    /// shared artifact's navigation section).
    static let sampleJourneyLabel = "Sample journey"

    /// The search view's heading (the sheet's full state).
    static let searchViewHeadingText = "Plan a route"

    // The device-location control and its honest states.
    static let deviceLocationOffText = "Location off."
    static let deviceLocationPermissionRequestedText =
        "Allow location access to show your position."
    static let deviceLocationPermissionDeniedText =
        "Location permission was denied. Allow it in Settings to show your position."
    static let deviceLocationUnavailableText =
        "Location is unavailable on this device right now."
    /// The camera zoom applied when the device position first centres the map.
    static let deviceLocationZoomLevel: Double = 15.0

    // Device-driven navigation, the same control and wording as the
    // other targets. Its fixes drive the planned route's guidance, so the
    // turn banner, ETA line and follow camera read the navigator stream.
    static let deviceNavigationActionLabel = "Navigate from my location"
    static let deviceNavigationStopLabel = "Stop device navigation"
    static let deviceNavigationPlanFirstText =
        "Plan a route first; device navigation follows the route you planned."
    static let deviceNavigationWaitingText =
        "Waiting for your position to start guidance."

    /// The search view's back affordance (§4.2: the back affordance
    /// collapses toward peek; the search view's step collapses to half's
    /// summary first).
    static let searchViewCollapseLabel = "Done"

    /// The search view's assignment hint line (which field a tapped
    /// result assigns to).
    static func searchAssignmentHintText(target: String) -> String {
        "Tapping a result assigns it to the \(target) field."
    }

    /// The search view's submit control label.
    static let searchSubmitLabel = "Search"

    /// The explicit route action of the two-field plan flow (§4.1).
    static let planRouteButtonText = "Plan route"

    /// The route summary's planning-state line (loading text ends with an
    /// ellipsis, §6.4).
    static let routeSummaryPlanningText = "Planning the route…"

    /// The type-ahead's one row when the provider matched nothing
    /// (`plan.no_matches` in `platform/reference-tokens.json`).
    static let planAutocompleteNoMatchesText = "No matching places."

    /// The type-ahead's inline sentence for a provider failure with no detail
    /// (`plan.autocomplete_unavailable`).
    static let planAutocompleteUnavailableText =
        "Suggestions are unavailable; keep typing to try again."

    /// The navigation drawer's voice toggle label (§4.2's speech toggle).
    static let drawerVoiceToggleLabel = "Voice guidance"

    /// The navigation drawer's exit label (§4.2).
    static let drawerExitLabel = "Exit"

    /// The navigation drawer's navigating phase word.
    static let phaseNavigatingText = "Navigating"

    /// The navigation drawer's off-route phase word (the consumer wording,
    /// never the contract's `RoutePhase` enum spelling).
    static let phaseOffRouteText = "Off route"

    /// The navigation drawer's phase before a trip begins.
    static let phaseNotStartedText = "Not started"

    /// The navigation drawer's ETA line prefix.
    static let etaPrefixText = "ETA"

    /// The navigation drawer's remaining-distance label.
    static let etaDistanceLabelText = "Distance remaining"

    /// The navigation drawer's remaining-duration label.
    static let etaDurationLabelText = "Duration remaining"

    /// The deviation alert's sentence (`%@` carries the offset phrase from
    /// the route line; the shared canonical `{offset}` value).
    static let deviationAlertFormat = """
        Off route — %@ from the route line; guidance is frozen until a graded \
        fix recovers or a reroute applies.
        """

    /// The navigation drawer's playback-speed row label (§7.2's playback
    /// controls; the shared canonical `navigation.speed_label`).
    static let navigationSpeedLabelText = "Speed"

    /// The playback control while the simulated trip runs (the shared
    /// canonical `navigation.pause_action`).
    static let navigationPauseActionText = "Pause"

    /// The playback control while the simulated trip is paused (the shared
    /// canonical `navigation.resume_action`).
    static let navigationResumeActionText = "Resume"

    /// One playback-speed chip's label (`%d` carries the integer
    /// multiplier; the shared canonical `navigation.speed_multiplier_label`).
    static let navigationSpeedMultiplierFormat = "%d×"

    /// The drawer's route-position seek control's label (the shared
    /// canonical `navigation.seek_label`).
    static let navigationSeekLabelText = "Seek"

    /// The seek control's smallest range, in meters: a zero-length route still
    /// renders a valid slider range (its only value is the destination).
    static let minimumSeekRangeMeters: Double = 1

    /// The navigation drawer's arrived line (the phase rendered as text;
    /// the banner stack carries the arrival card, §7.1).
    static let drawerArrivedText = "Arrived"

    /// The navigation drawer's ETA line while no trip state arrived yet
    /// (loading text ends with an ellipsis, §6.4).
    static let drawerPreparingTripText = "Preparing the trip…"

    /// The diagnostics sheet's close label.
    static let diagnosticsCloseLabel = "Close"

    /// The diagnostics sheet's title.
    static let diagnosticsTitleText = "Diagnostics"

    /// The offline card's disabled-with-reason line for the session where
    /// the tile-cache registration never ran (§6.2).
    static let offlineCardUnavailableReason = """
        The tile cache is not registered for this session; the readiness \
        ladder states why the SDK is not running.
        """

    /// The offline card's disabled-with-reason line for a keyless session:
    /// the SDK runs, but no provider registration installed the cache.
    static let offlineCardKeylessReason = """
        Set a provider key in Settings to enable the tile cache for live \
        map data.
        """

    /// The result list's empty-state hint (the list is never filled with
    /// guesses, §4.1).
    static let searchResultsEmptyText = "No results yet; search a place above."

    /// The result list's heading while the keyless demo filter answers
    /// (`plan.demo_places_heading`: the rows are demo places, never live
    /// search results).
    static let demoPlacesHeadingText = "Demo places"

    /// The keyless search's no-match line
    /// (`plan.demo_no_match_hint`): the format's one slot is the field's
    /// consumer label ("origin" or "destination").
    static let demoNoMatchHintFormat =
        "No demo place matches the %@; apply a provider key in Settings for live search."

    /// The route summary's note when the planned route is the artifact's
    /// recorded one (`plan.recorded_route_note`).
    static let recordedRouteNoteText = "Recorded demo route, pedestrian, fastest."

    /// The keyless plan's honest refusal when no recorded route answers the
    /// pair (`plan.no_recorded_route_hint`): the slots are the origin and
    /// destination names.
    static let noRecordedRouteHintFormat =
        "No recorded demo route from %@ to %@; apply a provider key for live routing."

    // MARK: Fixed strings

    /// The diagnostics' renderer-mode row value: the surface path
    /// negotiated the GPU renderer (the mode the ready ladder reports).
    static let rendererModeReadout = "gpu surface renderer"
    /// The diagnostics' renderer-mode row value before a surface attached.
    static let rendererModeUnpreparedReadout = "not negotiated yet"

    /// The diagnostics' adapter row: the contract does not expose the
    /// renderer's adapter detail yet, and the sheet says that honestly
    /// instead of inventing a name (§6.1's adapter row, "when any").
    static let rendererAdapterRowText = "adapter detail: not exposed by the map-surface contract"

    /// The diagnostics' camera row before the first drawn frame.
    static let cameraReadoutUnpreparedText = "no camera drawn yet…"

    /// The diagnostics' per-draw statistics row before the first drawn
    /// frame.
    static let tileStatisticsUnpreparedText = "no frame drawn yet…"

    /// The diagnostics traffic-cache block's title and its line while the
    /// traffic layer is off (the contract readback is null then).
    static let trafficCacheDiagnosticsBlockTitle = "Traffic cache"
    static let trafficCacheUnpreparedText = "the traffic layer is off"

    /// The off-route simulation's explanatory line (the toggle applies to
    /// the journey that starts while it is on — the sheet's group is
    /// reachable before a journey starts, §4.2).
    static let offRouteSimulationExplanation = """
        The off-route simulation displaces a journey's simulated fixes; \
        it applies to the journey that starts while it is on.
        """
}