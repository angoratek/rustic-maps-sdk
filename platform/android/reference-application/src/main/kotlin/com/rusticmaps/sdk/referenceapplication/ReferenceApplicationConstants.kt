package com.rusticmaps.sdk.referenceapplication

import uniffi.rustic_maps.maximumZoomLevel

/**
 * Named constants of the reference application. Every tunable value lives
 * here with its source; the repository rule forbids magic numbers.
 */
object ReferenceApplicationConstants {
    // --- Camera ranges (the camera limits the SDK clamps to) ---
    // The sliders present the same ranges the SDK camera clamps to, so a
    // slider value is always a usable camera value. The maximum zoom and the
    // fit-bounds ceiling are read from the contract, so a renderer change
    // reaches this screen without an edit here.
    const val MINIMUM_ZOOM_LEVEL: Double = 0.0
    val MAXIMUM_ZOOM_LEVEL: Double get() = maximumZoomLevel()
    const val MAXIMUM_PITCH_DEGREES: Double = 75.0
    val MAXIMUM_FIT_BOUNDS_ZOOM_LEVEL: Double get() = maximumZoomLevel()

    // --- Simulated turn-by-turn demo (docs/ReferenceAppUX.md §7) ---
    // The recorded trace samples advance five simulated seconds apart; the
    // demo plays the walk back far faster than real time by advancing
    // simulated time DEMO_PLAYBACK_TIME_ACCELERATION_FACTOR times per tick,
    // so the sample dispatch interval is the artifact's sample interval
    // divided by this factor (5 s / 20 = 250 ms). Documented demo factor,
    // not a navigation policy.
    const val DEMO_PLAYBACK_TIME_ACCELERATION_FACTOR: Double = 20.0
    // Fallback sample interval when the artifact does not carry one (the
    // artifact's `navigation_demo.simulated_trace.sample_interval_seconds`
    // is the live source; this documents the recorded generator default).
    const val SIMULATED_TRACE_SAMPLE_INTERVAL_SECONDS: Double = 5.0
    // The horizontal-accuracy report the recorded trace carries with every
    // fix (the trace files' generator metadata `horizontal_accuracy_meters`);
    // fixes without accuracy can neither advance steps nor raise deviations.
    // Read per-sample from the trace JSON; this constant names the recorded
    // value for documentation.
    const val SIMULATED_TRACE_HORIZONTAL_ACCURACY_METERS: Double = 8.0

    // --- Route-following walk and playback controls (M27) ---
    // A search-planned trip walks the route's own polyline through the
    // contract's RouteSimulator; the tick cadence below is how often a fix is
    // asked for (10 fixes a second, smooth at the demo's playback speeds).
    const val ROUTE_WALK_TICK_INTERVAL_MILLISECONDS: Long = 100L
    // How often a paused playback checks whether the user resumed it.
    const val SIMULATED_TRIP_PAUSED_POLL_INTERVAL_MILLISECONDS: Long = 100L
    // The baseline walk speed when a route declares no positive duration; the
    // contract refuses a non-positive pace.
    const val SIMULATED_TRIP_FALLBACK_SPEED_METERS_PER_SECOND: Double = 1.4
    // The drawer's selectable playback speed multipliers, applied to the
    // route's own average speed (the SimulatedTripPace multiplier). The user
    // picks one for the session; a route-following trip rebuilds its walker
    // at the new pace without moving the travelled position.
    val SIMULATED_TRIP_SPEED_MULTIPLIER_CHOICES: List<Double> = listOf(1.0, 2.0, 4.0, 8.0)

    /**
     * The navigation follow camera's values (round 10) are the design
     * language's §2.4 tokens (`navigation-follow-zoom` etc.), mirrored in
     * `ReferenceAppUXDimensions` — the round-9 platform constant
     * (`NAVIGATION_FOLLOW_ZOOM_LEVEL`, 17.25) is superseded by the table.
     */

    // --- Route preview framing ---
    // Insets the route-preview `fit_bounds` keeps clear of map content, in
    // physical pixels at the reference density (1x): the floating panels'
    // occupied band is approximated by one gutter plus one panel padding on
    // the start edge and the bottom dock height on the bottom edge.
    const val ROUTE_PREVIEW_INSET_LEFT_PIXELS: Double = 224.0
    // The top inset clears a route endpoint pin's head above its anchor tip
    // (the pin's height is about 2.6 head radii; 48 px clears it at the
    // default radius).
    const val ROUTE_PREVIEW_INSET_TOP_PIXELS: Double = 48.0
    const val ROUTE_PREVIEW_INSET_RIGHT_PIXELS: Double = 48.0

    // --- Route endpoint pins (§4.1's A/B teardrop markers) ---
    // The origin pin is green (A), the destination pin red (B); both draw
    // through the contract's `PointPins` overlay kind at its documented
    // default head radius. These mirror the web planner's colors exactly.
    const val ROUTE_ORIGIN_PIN_RED: Double = 0.13
    const val ROUTE_ORIGIN_PIN_GREEN: Double = 0.62
    const val ROUTE_ORIGIN_PIN_BLUE: Double = 0.29
    const val ROUTE_DESTINATION_PIN_RED: Double = 0.85
    const val ROUTE_DESTINATION_PIN_GREEN: Double = 0.16
    const val ROUTE_DESTINATION_PIN_BLUE: Double = 0.20
    const val ROUTE_PIN_ALPHA: Double = 1.0

    // The explicit Plan route action and the route summary's option line
    // (§4.1: the same option set on every shell).
    const val PLAN_ROUTE_BUTTON_TEXT: String = "Plan route"
    // The route summary's option line is derived from the Directions choices;
    // the slots are the mode and strategy in consumer words.
    const val ROUTE_OPTIONS_SUMMARY_TEMPLATE: String = "%s · %s"

    // --- Directions control (PRD.md §6 item 36) ---
    // The control beside the plan action: Car and Walk on day one, Fastest and
    // Shortest, with the shared identifiers every target uses.
    const val DIRECTIONS_GROUP_LABEL_TEXT: String = "Directions"
    const val DIRECTIONS_MODE_CAR_TEXT: String = "Car"
    const val DIRECTIONS_MODE_WALK_TEXT: String = "Walk"
    const val DIRECTIONS_STRATEGY_FASTEST_TEXT: String = "Fastest"
    const val DIRECTIONS_STRATEGY_SHORTEST_TEXT: String = "Shortest"
    const val DIRECTIONS_MODE_CAR_TEST_TAG: String = "directions-mode-car"
    const val DIRECTIONS_MODE_WALK_TEST_TAG: String = "directions-mode-walk"
    const val DIRECTIONS_STRATEGY_FASTEST_TEST_TAG: String = "directions-strategy-fastest"
    const val DIRECTIONS_STRATEGY_SHORTEST_TEST_TAG: String = "directions-strategy-shortest"
    // A provider that answered but cannot route for the mode says so; a
    // failed capability read reports its own verbatim reason instead.
    const val DIRECTIONS_MODE_UNAVAILABLE_TEMPLATE: String =
        "%s directions are not available from this provider."
    const val DIRECTIONS_STRATEGY_UNAVAILABLE_TEMPLATE: String =
        "%s route strategy is not available from this provider."
    // The plan action's disabled-with-reason lines while either field holds no
    // text (docs/ReferenceAppUX.md §4.1): each names the half the reader still has to enter.
    // These are the artifact's `plan.missing_*` consumer strings verbatim.
    const val PLAN_ROUTE_NEEDS_BOTH_FIELDS_TEXT: String =
        "Enter an origin and a destination to plan the route."
    const val PLAN_ROUTE_NEEDS_ORIGIN_TEXT: String =
        "Enter an origin to plan the route."
    const val PLAN_ROUTE_NEEDS_DESTINATION_TEXT: String =
        "Enter a destination to plan the route."

    // --- The reference-application behavior model's consumer copy ---
    // The artifact's `plan.*`, `settings.*` and `diagnostics.*` behavior rows
    // (docs/ReferenceAppUX.md): the field actions, the
    // trip-source line, the composed provider row and the diagnostics opt-in.
    const val PLAN_MY_LOCATION_ACTION_TEXT: String = "My location"
    // The chip's accessible name names its field, so two chips are never
    // "My location" and "My location" to TalkBack.
    const val PLAN_MY_LOCATION_ACCESSIBILITY_TEMPLATE: String = "Use my location as the %s"
    const val PLAN_SWAP_ACTION_TEXT: String = "Swap origin and destination"
    const val PLAN_MY_LOCATION_UNAVAILABLE_TEMPLATE: String =
        "My location is unavailable: %s"
    const val PLAN_TRIP_SOURCE_DEMO_PLAN_TEMPLATE: String = "Selected demo plan: %s"
    // The planned-route counterpart of the artifact's demo-plan source line.
    // The artifact names only the demo-plan form; a planned route states its
    // destination beside the same "trip source" label, so the two sources read
    // as one pair.
    const val PLAN_TRIP_SOURCE_PLANNED_ROUTE_TEMPLATE: String = "Planned route: %s"
    // A keyed field whose text resolves to no place reports itself in the
    // error strip; it is never silently ignored.
    const val PLAN_FIELD_NO_RESULT_TEMPLATE: String = "No place found for \"%s\"."
    const val SETTINGS_PROVIDER_ROW_TEMPLATE: String = "%s · %s"
    const val SETTINGS_PROVIDER_STATE_READY_TEXT: String = "Ready"
    const val SETTINGS_PROVIDER_STATE_NEEDS_CREDENTIAL_TEMPLATE: String = "Needs %s"
    const val SETTINGS_PROVIDER_STATE_NOT_CONFIGURED_TEXT: String = "Not configured"
    const val SETTINGS_ROUTING_DEMO_ENDPOINT_TEMPLATE: String = "Demo server: %s"
    const val SETTINGS_ROUTING_UNREACHABLE_NEXT_STEP_TEXT: String =
        "The demo server did not answer. Retry, or enter your own deployment address."
    // The reason a keyless provider without a registry demo address is not
    // selectable: the previous provider keeps serving and the row names the
    // missing descriptor value instead of calling the SDK with an empty
    // deployment address.
    const val SETTINGS_PROVIDER_NO_DEMO_ENDPOINT_TEMPLATE: String =
        "%s declares no demo deployment address; enter your own deployment address."

    // --- Diagnostics opt-in (docs/ReferenceAppUX.md) ---
    // One bounded stream at Info when the pane opens; one explicit switch
    // lowers it to Debug for the session; the display window and the backlog
    // flush share the artifact's bounded values.
    const val DIAGNOSTICS_SHOW_DEBUG_EVENTS_TEXT: String = "Show debug events"
    const val DIAGNOSTICS_HIDE_DEBUG_EVENTS_TEXT: String = "Hide debug events"
    const val DIAGNOSTICS_CLEAR_ACTION_TEXT: String = "Clear"
    const val DIAGNOSTICS_STREAM_SCOPE_NOTE_TEXT: String =
        "Debug detail stays on until the app restarts."
    const val DIAGNOSTICS_EVENT_WINDOW: Int = 100
    const val DIAGNOSTICS_REFRESH_CADENCE_MILLISECONDS: Long = 500L
    // The severity names the shared behavior tokens declare: the pane's one
    // stream starts at Info and the explicit switch lowers it to Debug.
    const val DIAGNOSTICS_MINIMUM_SEVERITY_NAME: String = "Info"
    const val DIAGNOSTICS_DEBUG_SEVERITY_NAME: String = "Debug"
    // The location puck never predicts motion between fixes
    // (`location-animation-dead-reckoning`, §2.4): a wrong prediction would
    // draw a place the visitor never was.
    const val LOCATION_ANIMATION_DEAD_RECKONING: String = "off"
    // The SDK's per-subsystem in-memory ring the pane reads; the pane's own
    // display window is smaller and bounded separately.
    const val DIAGNOSTICS_RING_CAPACITY: Int = 512

    // --- Stable per-platform flow hooks ---
    // The test tags `platform/reference-flows.json` names for Android, and the
    // flow identifiers the launch automation accepts. The values are the
    // schema's own hook strings, so the checker can find them in this source.
    const val HOME_SHEET_PEEK_SEARCH_TEST_TAG: String = "home-sheet-peek-search"
    const val PLAN_SEARCH_RESULTS_TEST_TAG: String = "search-results-list"
    const val START_SIMULATED_NAVIGATION_TEST_TAG: String = "start-simulated-navigation"
    const val PLAN_ORIGIN_MY_LOCATION_TEST_TAG: String = "origin-my-location"
    const val PLAN_DESTINATION_MY_LOCATION_TEST_TAG: String = "destination-my-location"
    const val PLAN_SWAP_ACTION_TEST_TAG: String = "plan-swap-action"
    const val SETTINGS_OPEN_TEST_TAG: String = "open-settings"
    const val SETTINGS_CLOSE_TEST_TAG: String = "settings-close"
    const val READINESS_CHIP_TEST_TAG: String = "readiness-chip"
    const val DIAGNOSTICS_SHEET_TEST_TAG: String = "diagnostics-sheet"
    const val DIAGNOSTICS_DEBUG_TOGGLE_TEST_TAG: String = "diagnostics-debug-toggle"
    const val DIAGNOSTICS_CLEAR_TEST_TAG: String = "diagnostics-clear"
    const val DIAGNOSTICS_CLOSE_TEST_TAG: String = "diagnostics-close"
    const val CAPABILITY_ROUTING_ROW_TEST_TAG: String = "capability-provider-row-routing"
    // One test tag per picker entry, suffixed with the registry's provider key
    // (`capability-provider-choice-osrm`, …), so a flow can select an exact
    // provider without naming it in the application source.
    const val CAPABILITY_PROVIDER_CHOICE_TEST_TAG_PREFIX: String = "capability-provider-choice-"
    // The search view's assignment hint: the result list says which field a
    // tapped row fills.
    const val SEARCH_ASSIGNMENT_HINT_TEMPLATE: String =
        "Tapping a result assigns it to the %s field."
    const val SEARCH_SUBMIT_LABEL_TEXT: String = "Search"
    // The two plan fields' explicit labels (the hint text stays their
    // placeholder).
    const val ORIGIN_FIELD_LABEL_TEXT: String = "Origin"
    const val DESTINATION_FIELD_LABEL_TEXT: String = "Destination"
    // The plan flow's shared UI-automation identifiers (the same spelling iOS
    // and React Native use).
    const val PLAN_ORIGIN_FIELD_TEST_TAG: String = "search-view-origin-field"
    const val PLAN_DESTINATION_FIELD_TEST_TAG: String = "search-view-destination-field"
    // The type-ahead lists, the one no-match row and the inline provider
    // sentence each field renders (docs/ReferenceAppUX.md §5.5).
    const val PLAN_ORIGIN_AUTOCOMPLETE_LIST_TEST_TAG: String = "search-view-origin-autocomplete"
    const val PLAN_DESTINATION_AUTOCOMPLETE_LIST_TEST_TAG: String =
        "search-view-destination-autocomplete"
    const val PLAN_ORIGIN_AUTOCOMPLETE_NO_MATCH_TEST_TAG: String =
        "search-view-origin-autocomplete-no-match"
    const val PLAN_DESTINATION_AUTOCOMPLETE_NO_MATCH_TEST_TAG: String =
        "search-view-destination-autocomplete-no-match"
    const val PLAN_ORIGIN_AUTOCOMPLETE_FAILURE_TEST_TAG: String =
        "search-view-origin-autocomplete-failure"
    const val PLAN_DESTINATION_AUTOCOMPLETE_FAILURE_TEST_TAG: String =
        "search-view-destination-autocomplete-failure"
    const val PLAN_ROUTE_BUTTON_TEST_TAG: String = "plan-route-button"
    const val PLAN_ROUTE_REASON_TEST_TAG: String = "plan-route-reason"
    const val SEARCH_ASSIGNMENT_HINT_TEST_TAG: String = "search-assignment-hint"

    // --- Readiness and draw loop (docs/ReferenceAppUX.md §6.1) ---
    // The readiness text ends in an ellipsis while the ladder is in the
    // loading state; this is the exact suffix the design language uses.
    const val READINESS_LOADING_SUFFIX: String = "…"
    // Status line texts name the SDK and the renderer mode, mirroring the
    // web refapp's ready line ("Maps SDK gpu renderer active (version X)").
    const val READY_STATUS_TEMPLATE: String = "Maps SDK gpu renderer active (version %s)"
    const val DEGRADED_STATUS_TEMPLATE: String = "Maps SDK surface active with a failed frame: %s"
    const val FAILED_STATUS_TEMPLATE: String = "Maps SDK failed to start: %s"
    const val LOADING_STATUS_TEXT: String = "Maps SDK starting"
    // The readiness chip's four states (§6.1). The chip reads one line: a
    // ready or loading word, or a state prefix followed by the ladder's own
    // detail. The fallback details stand in when the ladder reports none.
    const val READINESS_CHIP_READY_TEXT: String = "Ready"
    const val READINESS_CHIP_LOADING_TEXT: String = "Loading…"
    const val READINESS_CHIP_DEGRADED_PREFIX_TEXT: String = "Degraded: "
    const val READINESS_CHIP_FAILED_PREFIX_TEXT: String = "Failed: "
    const val READINESS_CHIP_DEGRADED_FALLBACK_DETAIL_TEXT: String = "a frame draw failed"
    const val READINESS_CHIP_FAILED_FALLBACK_DETAIL_TEXT: String = "unknown failure"

    // --- Route planning ---
    // The demo route plan is a two-waypoint route (origin and destination).
    const val ROUTE_WAYPOINT_COUNT: Int = 2

    // --- Offline maps (docs/ReferenceAppUX.md §4.1, round 9) ---
    // The tile cache's file inside the app's caches directory; the SDK's
    // tile-cache registration arm opens (or creates) the SQLite response
    // cache there.
    const val TILE_CACHE_DATABASE_FILE_NAME: String = "reference-tile-cache.sqlite"
    // The named byte budget of the tile cache: 16 MiB (16777216) absolute
    // maximum — a demo-scale budget that still holds the whole recorded
    // fixture set many times over. (A const initializer must be a literal;
    // the MiB arithmetic lives in the comment.)
    const val TILE_CACHE_ABSOLUTE_MAXIMUM_BYTES: ULong = 16777216UL

    // The demo-plan picker's rows (scenario, trace asset, label, description)
    // come from the shared artifact's `demo_plans` section
    // (`platform/reference-scenes.json`), never from app-side constants.
    // The trip source a search-planned route replays (its own shape, in
    // order). The name is internal; the route summary names the destination.
    const val PLANNED_ROUTE_TRACE_NAME: String = "planned_route_shape"
    // The route summary card's primary and subtle control texts (§4.2). The
    // peek's action row is the same start (§1: the route's Start / the trip's
    // Exit), so the two read as one flow.
    const val ROUTE_SUMMARY_START_BUTTON_TEXT: String = "Start simulated navigation"
    const val ROUTE_SUMMARY_EDIT_BUTTON_TEXT: String = "Edit"
    // The destination search field's consumer text (§4.2's "Where to?") and
    // its placeholder; the origin field reuses its hint as its placeholder.
    const val DESTINATION_FIELD_HINT_TEXT: String = "Where to?"
    const val DESTINATION_FIELD_PLACEHOLDER_TEXT: String = "Search a destination"
    const val ORIGIN_FIELD_HINT_TEXT: String = "Search an origin"
    // The search view's heading and its dismiss action (§4.1): the same pair
    // the iOS and React Native shells show, so the flow reads identically.
    const val PLAN_SEARCH_HEADING_TEXT: String = "Plan a route"
    // The device-location control and its honest states (M33).
    const val DEVICE_LOCATION_ACTION_TEXT: String = "Show my location"
    const val DEVICE_LOCATION_STOP_TEXT: String = "Hide my location"
    const val DEVICE_LOCATION_OFF_TEXT: String = "Location off."
    const val DEVICE_LOCATION_PERMISSION_DENIED_TEXT: String =
        "Location permission was denied. Allow it to show your position."
    const val DEVICE_LOCATION_UNAVAILABLE_TEXT: String =
        "Location is unavailable on this device right now."
    // The device-driven guidance control and its plan-first sentence (the
    // shared copy of the Location group's second control).
    const val DEVICE_NAVIGATION_ACTION_TEXT: String = "Navigate from my location"
    const val DEVICE_NAVIGATION_STOP_TEXT: String = "Stop device navigation"
    const val DEVICE_NAVIGATION_PLAN_FIRST_TEXT: String =
        "Plan a route first; device navigation follows the route you planned."
    // The accuracy a device fix reports when its source carries none; the
    // navigation reducer needs a graded fix to advance steps and judge
    // deviations, so the accepted fix is reported at the recorded trace's own
    // accuracy value.
    const val DEVICE_NAVIGATION_FALLBACK_ACCURACY_METERS: Double = 8.0
    // The UI-automation identifiers of the Location group's two controls
    // (the shared spelling of the cross-target control identifiers).
    const val DEVICE_LOCATION_CONTROL_TEST_TAG: String = "device-location-control"
    const val DEVICE_NAVIGATION_CONTROL_TEST_TAG: String = "device-navigation-control"
    const val PLAN_SEARCH_DONE_ACTION_TEXT: String = "Done"

    // --- Navigation drawer (§7.2 v2 docks) ---
    const val NAVIGATION_DRAWER_EXIT_BUTTON_TEXT: String = "Exit"
    const val NAVIGATION_DRAWER_PHASE_NOT_STARTED_TEXT: String = "Not started"
    // The contract's phase words as the drawer states them (§7.2).
    const val NAVIGATION_PHASE_LABEL_NAVIGATING: String = "Navigating"
    const val NAVIGATION_PHASE_LABEL_OFF_ROUTE: String = "Off route"
    const val NAVIGATION_PHASE_LABEL_ARRIVED: String = "Arrived"
    // The ETA-distance line's prefix and its two labelled values (§7.2).
    const val NAVIGATION_DRAWER_ETA_PREFIX_TEXT: String = "ETA"
    const val NAVIGATION_DRAWER_ETA_DISTANCE_LABEL_TEXT: String = "Distance remaining"
    const val NAVIGATION_DRAWER_ETA_DURATION_LABEL_TEXT: String = "Duration remaining"
    // The deviation alert (§7.5): one sentence, the deviation offset in the
    // slot, guidance frozen until a graded fix recovers or a reroute applies.
    const val DEVIATION_ALERT_TEXT_TEMPLATE: String =
        "Off route — %s from the route line; " +
            "guidance is frozen until a graded fix recovers or a reroute applies."
    // The playback controls (M27): the speed chips' group label, the pause
    // and resume states of the one control, and the chip label form.
    const val NAVIGATION_DRAWER_SPEED_LABEL_TEXT: String = "Speed"
    const val NAVIGATION_DRAWER_PAUSE_BUTTON_TEXT: String = "Pause"
    const val NAVIGATION_DRAWER_RESUME_BUTTON_TEXT: String = "Resume"
    const val NAVIGATION_SPEED_MULTIPLIER_LABEL_TEMPLATE: String = "%d×"
    const val NAVIGATION_DRAWER_SEEK_LABEL_TEXT: String = "Seek"
    const val SPEECH_TOGGLE_LABEL_TEXT: String = "Voice guidance"

    // --- Offline maps card (§4.2: two honest lines) ---
    const val OFFLINE_MAPS_CARD_TITLE_TEXT: String = "Offline maps"
    const val OFFLINE_MAPS_NOT_REGISTERED_TEXT: String = "Cache not registered"
    /** The offline card's tile line before the first drawn frame (keyless starts). */
    const val OFFLINE_TILES_PENDING_TEXT: String = "Tile accounting starts with the first frame…"
    /**
     * The committed compiled routing archive the offline card's route control
     * reads from the app's assets (§2.5 `offline.plan_route_action`).
     */
    const val OFFLINE_ROUTING_ARCHIVE_ASSET_NAME: String = "routing_grid.rmog"
    // The offline archive's demo request: the routing-grid fixture's straight
    // oneway street, the same pair the wasm seam's test and the book's
    // snippet use. The request itself takes the archive profile's defaults
    // (automobile, fastest, metric).
    const val OFFLINE_ROUTE_START_LATITUDE_DEGREES: Double = 0.001
    const val OFFLINE_ROUTE_START_LONGITUDE_DEGREES: Double = 0.0
    const val OFFLINE_ROUTE_END_LATITUDE_DEGREES: Double = 0.001
    const val OFFLINE_ROUTE_END_LONGITUDE_DEGREES: Double = 0.002
    /** The offline card's route control label (§2.5 `offline.plan_route_action`). */
    const val OFFLINE_ROUTE_ACTION_TEXT: String = "Plan offline route"
    /** The route summary's title for an offline archive route (§2.5). */
    const val OFFLINE_ROUTE_TITLE_TEXT: String = "Offline demo route"
    /** The route summary's honesty note for an offline archive route (§2.5). */
    const val OFFLINE_ROUTE_NOTE_TEXT: String =
        "Planned offline from a recorded test graph, not live data."

    // --- Offline region downloads (§2.5's offline-card labels) ---
    // The canonical labels live in `platform/reference-tokens.json`; the
    // parity gate compares every one verbatim. The browser has no disk store,
    // so its label is mirrored here for the same canonical copy even though
    // the Android card has no browser surface to render it on.
    /** The region-download control while no download runs (§2.5). */
    const val OFFLINE_DOWNLOAD_ACTION_TEXT: String = "Download offline region"
    /** The in-flight region-download line (§2.5). */
    const val OFFLINE_DOWNLOADING_STATUS_TEXT: String = "Downloading…"
    /** The control that stops the in-flight region download (§2.5). */
    const val OFFLINE_CANCEL_ACTION_TEXT: String = "Cancel"
    /** The terminal line when every region tile is stored (§2.5). */
    const val OFFLINE_COMPLETED_STATUS_TEXT: String = "Download complete"
    /** The terminal line when the region download stops on an error (§2.5). */
    const val OFFLINE_FAILED_STATUS_TEXT: String = "Download failed"
    /** The stored-region tile-count label (§2.5). */
    const val OFFLINE_STORED_TILES_LABEL_TEXT: String = "Stored tiles"
    /** The stored-region payload-size label (§2.5). */
    const val OFFLINE_STORED_BYTES_LABEL_TEXT: String = "Stored bytes"
    /** The control that removes a stored region and its tiles (§2.5). */
    const val OFFLINE_DELETE_ACTION_TEXT: String = "Delete region"
    /** The line after the stored region is removed (§2.5). */
    const val OFFLINE_DELETED_STATUS_TEXT: String = "Region deleted"
    /** The browser's honest limitation statement (§2.5). */
    const val OFFLINE_BROWSER_UNAVAILABLE_STATUS_TEXT: String =
        "Offline region downloads are not available in the browser."

    // The card's composed lines. The canonical phrases above lead each line;
    // these templates carry only the counts that follow them.
    /** The demo region row: display name and the tiles its definition covers. */
    const val OFFLINE_REGION_TILE_COUNT_TEMPLATE: String = "%s · %d tiles"
    /** The in-flight counts: completed / total and the network-versus-disk split. */
    const val OFFLINE_DOWNLOAD_PROGRESS_COUNTS_TEMPLATE: String =
        "%d / %d tiles · network %d · disk %d"
    /** The terminal line for a cancelled download (a successful partial result). */
    const val OFFLINE_CANCELLED_STATUS_TEXT: String = "Download cancelled"
    /** The cancelled line's stored count out of the region's members. */
    const val OFFLINE_CANCELLED_STATUS_TEMPLATE: String = "%s · %d / %d tiles stored"
    /** A stored region row's tile and byte footprint. */
    const val OFFLINE_STORED_REGION_SUMMARY_TEMPLATE: String = "%s %d · %s %d"
    /** The stored-regions list's heading. */
    const val OFFLINE_STORED_REGIONS_HEADING_TEXT: String = "Stored regions"
    // The region flow's UI-automation identifiers.
    const val OFFLINE_REGION_DOWNLOAD_TEST_TAG: String = "offline-region-download"
    const val OFFLINE_REGION_CANCEL_TEST_TAG: String = "offline-region-cancel"
    const val OFFLINE_REGION_STATUS_TEST_TAG: String = "offline-region-status"
    const val OFFLINE_REGION_DELETE_TEST_TAG_PREFIX: String = "offline-region-delete-"

    // --- Provider credentials and the settings surface (§6.5) ---
    // Keyless is a first-class state: the readiness dock states what to do
    // next in neutral product language, never as a renderer or startup
    // failure. Credentials are entered at runtime, never embedded at build
    // time.
    const val KEYLESS_CREDENTIAL_STATUS_TEXT: String = "Set a provider key to load live map data"

    /**
     * The launch-intent extra that supplies a runtime credential to UI
     * automation (`adb shell am start -e providerApiKey …`). It is applied
     * through the public registration path, never persisted by the injection
     * itself, and never logged; it is not a build-time key seam.
     */
    const val RUNTIME_CREDENTIAL_INTENT_EXTRA: String = "providerApiKey"

    /**
     * The launch-intent extra that names the provider the runtime credential
     * belongs to (`adb shell am start -e providerKey …`). Absent, the
     * injection targets the only provider the registry declares available on
     * Android, so the vendor name stays in the registry descriptor.
     */
    const val RUNTIME_PROVIDER_KEY_INTENT_EXTRA: String = "providerKey"

    /**
     * The launch-intent extra that runs the tutorial's worked example (M25):
     * `adb shell am start -e demoWalkthrough true`. The boolean extra is read
     * once at startup and drives the same steps the guides publish, so the
     * generated chapters describe code this application really executes.
     */
    const val WALKTHROUGH_INTENT_EXTRA: String = "demoWalkthrough"

    /**
     * The launch-intent extra that selects a base map at startup by provider
     * key (`adb shell am start -e demoBaseMap <provider-key>`). It opens the
     * app on a chosen source, so a keyless run can start on imagery rather
     * than an empty surface; no credential is involved.
     */
    const val DEMO_BASE_MAP_INTENT_EXTRA: String = "demoBaseMap"

    /**
     * The launch-intent extra that names the reference flow to drive
     * (`adb shell am start -e referenceFlowId plan_and_navigate`). The value is
     * read once at startup and recorded; the flow itself drives the same public
     * user-path methods the sheet's controls call, so the automation is a test
     * seam rather than a second implementation.
     */
    const val REFERENCE_FLOW_INTENT_EXTRA: String = "referenceFlowId"

    /** The four flow identifiers `platform/reference-flows.json` declares. */
    const val REFERENCE_FLOW_PLAN_AND_NAVIGATE: String = "plan_and_navigate"
    const val REFERENCE_FLOW_MY_LOCATION_FIELD: String = "my_location_field"
    const val REFERENCE_FLOW_ROUTING_PROVIDER_SWITCH: String = "routing_provider_switch"
    const val REFERENCE_FLOW_DIAGNOSTICS_OPT_IN: String = "diagnostics_opt_in"

    /** Every flow identifier this build knows, for the launch extra's validation. */
    val REFERENCE_FLOW_IDS: List<String> = listOf(
        REFERENCE_FLOW_PLAN_AND_NAVIGATE,
        REFERENCE_FLOW_MY_LOCATION_FIELD,
        REFERENCE_FLOW_ROUTING_PROVIDER_SWITCH,
        REFERENCE_FLOW_DIAGNOSTICS_OPT_IN,
    )

    /** The origin the walkthrough searches for; the demo corridor's start. */
    const val WALKTHROUGH_ORIGIN_SEARCH_QUERY: String = "Capitol Park, Sacramento, California"

    /** The destination the walkthrough searches for; the corridor's end. */
    const val WALKTHROUGH_DESTINATION_SEARCH_QUERY: String = "Tower Bridge, Sacramento, California"
    const val SETTINGS_TITLE_TEXT: String = "Settings"
    const val SETTINGS_GEAR_SYMBOL: String = "⚙"
    const val SETTINGS_OPEN_ACCESSIBILITY_TEXT: String = "Open settings"
    const val SETTINGS_CLOSE_TEXT: String = "Close"
    const val SETTINGS_RESET_REMEMBERED_CHOICES_TEXT: String = "Reset remembered choices"
    const val PROVIDER_CREDENTIAL_APPLY_TEXT: String = "Apply"
    const val PROVIDER_CREDENTIAL_CLEAR_TEXT: String = "Clear"
    const val PROVIDER_CREDENTIAL_REMEMBER_LABEL_TEXT: String = "Remember on this device"
    const val PROVIDER_CREDENTIAL_REMEMBER_HINT_TEXT: String =
        "Stores the key in this device's keystore. Off keeps it for this session only."
    // Per-row status lines; the provider display name fills the slot.
    const val PROVIDER_CREDENTIAL_APPLIED_TEMPLATE: String = "%s key applied"
    const val PROVIDER_CREDENTIAL_NOT_APPLIED_TEMPLATE: String = "%s key not applied"
    const val PROVIDER_CREDENTIAL_ENTER_HINT_TEXT: String = "Enter a key, then Apply."
    const val PROVIDER_CREDENTIAL_UNAVAILABLE_TEMPLATE: String =
        "%s cannot be configured on this platform yet."
    // Clear removes the stored copy and the entered value; the SDK registry
    // has no unregister, so the running session stays credentialed (§6.5).
    const val PROVIDER_CREDENTIAL_CLEAR_RESTART_NOTE_TEXT: String =
        "Cleared. This session keeps its providers until the app restarts."
    // Honest secure-store failure: the value is session-only, stated plainly.
    const val PROVIDER_CREDENTIAL_NOT_STORED_TEMPLATE: String =
        "Not stored on this device: %s"
    // Base map (M9's native base-map switch): the settings sheet's section
    // label and the row status of the serving source. The User-Agent a
    // credential-less source identifies this app with comes from the shared
    // registry's app-identity template, never a shell literal.
    const val BASE_MAP_SECTION_LABEL_TEXT: String = "Base map"
    const val BASE_MAP_ACTIVE_STATUS_TEXT: String = "Serving"
    const val BASE_MAP_INACTIVE_STATUS_TEXT: String = "Select"
    // A credential-registered base map is offered before its key is applied;
    // the row names what it needs (the descriptor's own credential label)
    // instead of disappearing, and selecting it points at that key field.
    const val BASE_MAP_NEEDS_CREDENTIAL_TEMPLATE: String = "Needs %s"
    // The offline card's honest line after a credential-less base map took the
    // raster slot: the tile cache went with the provider that installed it.
    const val BASE_MAP_UNCACHED_NOTE_TEXT: String =
        "Serving base map runs uncached; select a keyed base map to use the cache."
    // M22: the base-map dropdown's entries. The section becomes one row that
    // names the serving base and opens a menu listing every raster source plus
    // every committed vector style the registry declares.
    const val BASE_MAP_CHANGE_ACTION_TEXT: String = "Change"
    const val BASE_MAP_VECTOR_GROUP_LABEL_TEXT: String = "Vector styles"
    const val BASE_MAP_RASTER_GROUP_LABEL_TEXT: String = "Raster sources"
    // The honest line while a styled vector base serves: the styles paint
    // fills, lines and circles, their place labels draw from the SDK's
    // embedded glyph range, and route-reference shield badges paint wherever
    // the style declares them.
    const val BASE_MAP_VECTOR_LABELS_NOTE_TEXT: String =
        "Vector styles draw land, water, roads with their names, and Latin place names."
    // Selecting a vector style before the shipped MapView exists cannot
    // install anything; the row says what to wait for instead of failing.
    const val BASE_MAP_VIEW_NOT_READY_TEXT: String =
        "Map view not ready; the style applies after the first frame."
    // The honest tile-failure line under an active vector base: the frame
    // skipped the failed tiles and the next frame retries them.
    const val BASE_MAP_VECTOR_FAILED_STATUS_TEMPLATE: String =
        "%d vector base tiles failed this frame; the next frame retries."

    // Per-capability provider selection: one row per SDK readback entry, the
    // serving provider's name (or the honest unconfigured wording), and a
    // picker of the shared registry's eligible providers.
    const val CAPABILITY_SECTION_LABEL_TEXT: String = "Capabilities"
    const val CAPABILITY_PROVIDER_NOT_CONFIGURED_TEXT: String = "Not configured"
    // A provider that declares a credential the session has not applied yet
    // is offered with what it needs (the descriptor's own label) instead of
    // disappearing, and selecting it points at that key field.
    const val CAPABILITY_PROVIDER_NEEDS_CREDENTIAL_TEMPLATE: String = "Needs %s"
    // Raster and vector tiles are owned by the map's layer control, so their
    // rows state where the source changes rather than offering a picker.
    const val CAPABILITY_PROVIDER_READ_ONLY_NOTE_TEXT: String =
        "Changed from the map's layer control"
    const val CAPABILITY_PROVIDER_CHANGE_ACTION_TEXT: String = "Change"

    // --- Demo scenes / demo plans consumer rows (§4.1) ---
    const val DEMO_SCENES_ROW_TITLE_TEXT: String = "Demo scenes"
    const val OVERLAYS_CLEAR_ACTION_TEXT: String = "Clear overlays"
    const val DEMO_PLANS_ROW_TITLE_TEXT: String = "Demo plans"

    // --- Diagnostics sheet (§6.1: the renderer facts and probe) ---
    const val DIAGNOSTICS_RENDERER_MODE_TEXT: String =
        "Renderer: GPU surface renderer (software graphics stack on this emulator: SwiftShader over ANGLE — an expected emulator fact, not a failure)"
    const val DIAGNOSTICS_TITLE_TEXT: String = "Diagnostics"
    // The sheet's vector-base line: the serving style and the last frame's
    // vector tile accounting, plus the fact that the raster layer no longer
    // draws under it (the styled base replaces the raster base).
    const val DIAGNOSTICS_VECTOR_BASE_TEMPLATE: String =
        "Vector base: %s · drawn %d, failed %d · raster layer hidden"

    // --- Diagnostics recent events (round 10's subscribe/flush seam, §4.1) ---
    const val DIAGNOSTICS_RECENT_EVENTS_TITLE_TEXT: String = "Recent SDK events"
    // The artifact's `diagnostics.no_events` consumer string verbatim.
    const val DIAGNOSTICS_RECENT_EVENTS_NONE_TEXT: String = "No SDK events recorded yet."
    // The backlog flush's consumer line: the contract returns how many of the
    // ring's most recent events it drained into the listener (oldest first).
    const val DIAGNOSTICS_EVENTS_BACKLOG_TEMPLATE: String = "Backlog: %d events flushed"
    // The maximum ring events one backlog flush drains; the artifact's shared
    // bounded backlog value, deep enough to show a session's history and small
    // enough to never hold the pump (a synchronous contract call — kept out of
    // the frame path, §1's diagnostics rule).
    const val DIAGNOSTICS_BACKLOG_FLUSH_EVENT_LIMIT: UInt = 100u

    // --- Developer console (site/book-src/provider-console.md) ---
    // Console-specific copy. The shared reference-tokens artifact has no
    // console section; the console reuses its color, spacing and radius tokens
    // and keeps only its own words here.
    const val PROVIDER_CONSOLE_TITLE_TEXT: String = "Developer console"
    const val PROVIDER_CONSOLE_OPEN_ACCESSIBILITY_TEXT: String = "Open developer console"
    const val PROVIDER_CONSOLE_OPEN_WITH_ALERT_ACCESSIBILITY_TEXT: String =
        "Open developer console; new warnings or errors"
    const val PROVIDER_CONSOLE_CLOSE_TEXT: String = "Close"
    const val PROVIDER_CONSOLE_EXPORT_TEXT: String = "Export"
    const val PROVIDER_CONSOLE_FILTER_ERROR_TEXT: String = "Error"
    const val PROVIDER_CONSOLE_FILTER_WARNING_TEXT: String = "Warning"
    const val PROVIDER_CONSOLE_FILTER_INFO_TEXT: String = "Info"
    const val PROVIDER_CONSOLE_EMPTY_ERROR_TEXT: String = "No Error events recorded yet."
    const val PROVIDER_CONSOLE_EMPTY_WARNING_TEXT: String =
        "No Warning or Error events recorded yet."
    const val PROVIDER_CONSOLE_EMPTY_INFO_TEXT: String = "No SDK events recorded yet."
    const val PROVIDER_CONSOLE_DETAIL_BACK_TEXT: String = "Back"
    const val PROVIDER_CONSOLE_TILE_CACHE_EMPTY_TEXT: String =
        "Tile cache · Unavailable — no tile cache is installed."
    const val PROVIDER_CONSOLE_TILE_CACHE_TEMPLATE: String =
        "Tile cache · hits %d · misses %d · stored %s"
    const val PROVIDER_CONSOLE_TILE_CACHE_OCCUPANCY_UNAVAILABLE_TEXT: String = "unavailable"
    const val PROVIDER_CONSOLE_TILE_CACHE_OCCUPANCY_TEMPLATE: String = "%d bytes"
    const val PROVIDER_CONSOLE_TILE_FRAME_EMPTY_TEXT: String =
        "Last frame · No GPU frame settled yet."
    const val PROVIDER_CONSOLE_TILE_FRAME_TEMPLATE: String =
        "Last frame · requested %d · drawn %d · fallback %d · missing %d"
    const val PROVIDER_CONSOLE_TRAFFIC_CACHE_EMPTY_TEXT: String =
        "Traffic cache · Unavailable — no traffic provider is registered."
    const val PROVIDER_CONSOLE_TRAFFIC_CACHE_TEMPLATE: String =
        "Traffic cache · %s · hits %d · misses %d · held cells %d · evictions %d · " +
            "grid zoom %d · entry budget %d · TTL %d s"
    const val PROVIDER_CONSOLE_TRAFFIC_PROVIDER_FALLBACK_TEXT: String = "Provider"
    const val PROVIDER_CONSOLE_EXPORT_HEADER_TEXT: String = "Developer console"
    const val PROVIDER_CONSOLE_EXPORT_EVENTS_LABEL_TEXT: String = "Events"
    const val PROVIDER_CONSOLE_EXPORT_SUBJECT_TEXT: String = "Developer console export"
    const val PROVIDER_CONSOLE_SHARE_CHOOSER_TEXT: String = "Share developer console"
    const val PROVIDER_CONSOLE_EXPORT_TRUNCATION_NOTICE_TEXT: String =
        "[truncated at 256 KB]"
    const val PROVIDER_CONSOLE_EXCERPT_NOT_CAPTURED_TEXT: String = "Not captured"
    const val PROVIDER_CONSOLE_DETAIL_TIME_LABEL_TEXT: String = "Time"
    const val PROVIDER_CONSOLE_DETAIL_LEVEL_LABEL_TEXT: String = "Level"
    const val PROVIDER_CONSOLE_DETAIL_SUBSYSTEM_LABEL_TEXT: String = "Subsystem"
    const val PROVIDER_CONSOLE_DETAIL_PROVIDER_LABEL_TEXT: String = "Provider"
    const val PROVIDER_CONSOLE_DETAIL_PROVIDER_KEY_LABEL_TEXT: String = "Provider key"
    const val PROVIDER_CONSOLE_DETAIL_CONTRACT_LABEL_TEXT: String = "Contract"
    const val PROVIDER_CONSOLE_DETAIL_METHOD_LABEL_TEXT: String = "Method"
    const val PROVIDER_CONSOLE_DETAIL_PATH_LABEL_TEXT: String = "Request path"
    const val PROVIDER_CONSOLE_DETAIL_STATUS_LABEL_TEXT: String = "Status class"
    const val PROVIDER_CONSOLE_DETAIL_OUTCOME_LABEL_TEXT: String = "Outcome"
    const val PROVIDER_CONSOLE_DETAIL_DURATION_LABEL_TEXT: String = "Duration"
    const val PROVIDER_CONSOLE_DETAIL_BYTES_LABEL_TEXT: String = "Response bytes"
    const val PROVIDER_CONSOLE_DETAIL_EXCERPT_LABEL_TEXT: String = "Response excerpt"
    const val PROVIDER_CONSOLE_DETAIL_MESSAGE_LABEL_TEXT: String = "Message"
    // The console's stable UI-automation identifiers (the cross-target
    // spelling of the console control and sheet).
    const val DEVELOPER_CONSOLE_CONTROL_TEST_TAG: String = "developer-console-control"
    const val DEVELOPER_CONSOLE_WARNING_DOT_TEST_TAG: String = "developer-console-warning-dot"
    const val DEVELOPER_CONSOLE_SHEET_TEST_TAG: String = "developer-console"
    const val DEVELOPER_CONSOLE_CLOSE_TEST_TAG: String = "developer-console-close"
    const val DEVELOPER_CONSOLE_EXPORT_TEST_TAG: String = "developer-console-export"
    const val DEVELOPER_CONSOLE_FILTER_ERROR_TEST_TAG: String = "developer-console-filter-error"
    const val DEVELOPER_CONSOLE_FILTER_WARNING_TEST_TAG: String =
        "developer-console-filter-warning"
    const val DEVELOPER_CONSOLE_FILTER_INFO_TEST_TAG: String = "developer-console-filter-info"
    const val DEVELOPER_CONSOLE_DETAIL_TEST_TAG: String = "developer-console-detail"
    const val DEVELOPER_CONSOLE_DETAIL_BACK_TEST_TAG: String = "developer-console-detail-back"
    const val DEVELOPER_CONSOLE_ROW_TEST_TAG_PREFIX: String = "developer-console-row-"

    // --- Map control cluster (§4.2, round 10) ---
    const val MAP_CONTROL_ZOOM_OUT_SYMBOL: String = "−"
    const val MAP_CONTROL_ZOOM_IN_SYMBOL: String = "+"
    const val MAP_CONTROL_ZOOM_OUT_ACCESSIBILITY_TEXT: String = "Zoom out one map-control step"
    const val MAP_CONTROL_ZOOM_IN_ACCESSIBILITY_TEXT: String = "Zoom in one map-control step"
    const val MAP_CONTROL_MY_LOCATION_ACCESSIBILITY_TEXT: String = "Center on my location"
    /** The my-location control's label while a gesture-broken follow waits (§5.3). */
    const val MAP_CONTROL_RECENTER_ACCESSIBILITY_TEXT: String = "Re-center on my location"
    const val MAP_CONTROL_THREE_D_LABEL_TEXT: String = "3D"
    const val MAP_CONTROL_TWO_D_LABEL_TEXT: String = "2D"
    const val MAP_CONTROL_PLANE_ACCESSIBILITY_TEXT: String = "Toggle the 2D and 3D camera planes"
    const val MAP_CONTROL_COMPASS_NORTH_LETTER: String = "N"
    const val MAP_CONTROL_COMPASS_ACCESSIBILITY_TEXT: String = "Ease the camera north up"

    // --- On-map layer panel (docs/ReferenceAppUX.md §4.2) ---
    const val MAP_LAYERS_CONTROL_ACCESSIBILITY_TEXT: String = "Map layers"
    /** The collapsed control's value while a non-default layer is active. */
    const val MAP_LAYERS_ACTIVE_LAYER_VALUE_TEXT: String = "Non-default layer active"
    const val MAP_LAYERS_TRAFFIC_SECTION_LABEL_TEXT: String = "Traffic"
    const val MAP_LAYERS_OVERLAYS_SECTION_LABEL_TEXT: String = "Overlays"
    const val MAP_LAYERS_DEVICE_LOCATION_SECTION_LABEL_TEXT: String = "Device location"
    const val MAP_LAYERS_STOP_ACTION_TEXT: String = "Stop"
    // The device-location row's five states (the specification's exact words).
    const val DEVICE_LOCATION_PANEL_OFF_TEXT: String = "Off"
    const val DEVICE_LOCATION_PANEL_WAITING_TEXT: String = "Waiting for a fix"
    const val DEVICE_LOCATION_PANEL_ACTIVE_TEXT: String = "Active"
    const val DEVICE_LOCATION_PANEL_PERMISSION_DENIED_TEXT: String = "Permission denied"
    const val DEVICE_LOCATION_PANEL_UNAVAILABLE_TEXT: String = "Unavailable"
    // The capability name a registry descriptor declares to serve traffic
    // flow (the shared registry's `served_contracts` spelling).
    const val TRAFFIC_FLOW_CAPABILITY_NAME: String = "traffic_flow"
    /** How long the camera must stay still before one traffic request runs. */
    const val TRAFFIC_REFRESH_DEBOUNCE_MILLISECONDS: Long = 400L
    /** The drawn width of one traffic-flow line, in screen pixels. */
    const val TRAFFIC_FLOW_OVERLAY_WIDTH_PIXELS: Double = 4.0
    const val MAP_LAYERS_CAMERA_BOUNDS_UNAVAILABLE_TEXT: String =
        "The camera bounds are unavailable."
    const val MAP_LAYERS_TRAFFIC_REFRESH_FAILURE_TEMPLATE: String =
        "Traffic refresh failed: %s"
    // The panel's shared identifiers and row-identifier prefixes.
    const val MAP_LAYERS_CONTROL_TEST_TAG: String = "map-layers-control"
    const val MAP_LAYERS_PANEL_TEST_TAG: String = "map-layers-panel"
    const val MAP_LAYERS_CLOSE_TEST_TAG: String = "map-layers-close"
    const val MAP_LAYERS_STATUS_TEST_TAG: String = "map-layers-status"
    const val MAP_LAYER_TRAFFIC_TEST_TAG: String = "map-layer-traffic"
    const val MAP_LAYER_TRAFFIC_REASON_TEST_TAG: String = "map-layer-traffic-reason"
    const val MAP_LAYER_BASE_ROW_TEST_TAG_PREFIX: String = "map-layer-base-"
    const val MAP_LAYER_STYLE_ROW_TEST_TAG_PREFIX: String = "map-layer-style-"
    const val MAP_LAYER_SCENE_ROW_TEST_TAG_PREFIX: String = "map-layer-scene-"
    const val PANEL_DEVICE_LOCATION_STATUS_TEST_TAG: String = "device-location-status"
    const val PANEL_DEVICE_LOCATION_STOP_TEST_TAG: String = "device-location-stop"

    // --- Attribution info control (§1/§4.2, PRD §5) ---
    // The always-visible attribution line became a compact info control at
    // the bottom-start; its panel carries the registry descriptors' attribution
    // lines and terms notes.
    const val ATTRIBUTION_INFO_SYMBOL: String = "ⓘ"
    const val ATTRIBUTION_INFO_ACCESSIBILITY_TEXT: String = "Map attribution and terms"
    const val ATTRIBUTION_PANEL_TITLE_TEXT: String = "Map attribution and terms"

    // --- Theme row (§1's half-state theme row) ---
    const val THEME_ROW_TITLE_TEXT: String = "Theme"

    // --- Search and route consumer rows (§4.1) ---
    const val SEARCH_RESULTS_LABEL_TEXT: String = "Search results"
    // The type-ahead list's one row when the provider answered with no match
    // (docs/ReferenceAppUX.md §5.5): never an empty
    // list. The inline provider failure sentence itself is the contract's own
    // detail text, so no new error wording is invented here; the shared
    // `plan.autocomplete_unavailable` sentence is therefore omitted on this
    // target by the parity gate's declaration.
    const val AUTOCOMPLETE_NO_MATCH_TEXT: String = "No matching places."
    const val SEARCH_IN_FLIGHT_TEXT: String = "Searching…"
    const val ROUTE_PLAN_IN_FLIGHT_TEXT: String = "Planning the route…"
    const val EMPTY_RESULTS_HINT_TEXT: String = "No results yet; search a place above."
    // The keyless plan flow's honest words (§2.5 `plan.demo_places_heading`,
    // `plan.demo_no_match_hint`, `plan.recorded_route_note`,
    // `plan.no_recorded_route_hint`): the artifact's demo places are offered
    // as demo content, and a recorded pair is named as the recorded demo
    // route, never as a live result or a live route.
    const val DEMO_PLACES_HEADING_TEXT: String = "Demo places"
    const val DEMO_NO_MATCH_HINT_TEMPLATE: String =
        "No demo place matches the %s; apply a provider key in Settings for live search."
    // The artifact's recorded route is a walk, so its note keeps the shared
    // canonical wording whatever the Directions choices are: the choices
    // apply to live plan calls, not to recorded geometry.
    const val RECORDED_ROUTE_NOTE_TEXT: String =
        "Recorded demo route, pedestrian, fastest."
    const val NO_RECORDED_ROUTE_HINT_TEMPLATE: String =
        "No recorded demo route from %s to %s; apply a provider key for live routing."

    // --- Speech (the platform TTS seam, §4.1) ---
    // The TextToSpeech language the demo requests; the instruction payloads'
    // speech channel is provider-authored English. The engine's installed
    // voice is platform-owned; a refused language is logged, not fatal.
    const val SPEECH_LANGUAGE_TAG: String = "en-US"
}