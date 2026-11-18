package com.rusticmaps.sdk.refapprn

/**
 * Named constants of the React Native reference application shell. Every
 * tunable value lives here with its source; the repository rule forbids magic
 * numbers. The strings mirror the Compose reference application's
 * `ReferenceApplicationConstants` (docs/ReferenceAppUX.md is the design
 * language source of truth) — the v2 sheet-and-drawer language, RN-shaped.
 */
object RefappConstants {
    // --- Readiness and draw loop (docs/ReferenceAppUX.md §6.1) ---
    /** The readiness text ends in an ellipsis while the ladder is loading. */
    const val READINESS_LOADING_SUFFIX: String = "…"
    const val LOADING_STATUS_TEXT: String = "Maps SDK starting"
    const val READY_STATUS_TEMPLATE: String = "Maps SDK gpu renderer active (version %s)"
    const val DEGRADED_STATUS_TEMPLATE: String = "Maps SDK surface active with a failed frame: %s"
    const val FAILED_STATUS_TEMPLATE: String = "Maps SDK failed to start: %s"

    // --- Simulated turn-by-turn demo (docs/ReferenceAppUX.md §7) ---
    // The recorded trace samples advance five simulated seconds apart; the demo
    // plays the walk back far faster than real time (5 s / this factor = 250 ms
    // per fix) — the Compose reference application's documented demo factor.
    const val DEMO_PLAYBACK_TIME_ACCELERATION_FACTOR: Double = 20.0
    // The planned-route trip's synthetic trace values (the Compose reference
    // application's `SimulatedTrace.fromRouteData` constants): the fixes'
    // accuracy report and the recorded sample cadence the replay keeps, and
    // the walk-scale speed below which the playback never divides by zero.
    const val SIMULATED_TRACE_HORIZONTAL_ACCURACY_METERS: Double = 8.0
    const val SIMULATED_TRACE_SAMPLE_INTERVAL_SECONDS: Double = 5.0
    /**
     * The accuracy a device fix reports when its source carries none; the
     * navigation reducer needs a graded fix to advance steps and judge
     * deviations, so the accepted fix is reported at the recorded trace's own
     * accuracy value.
     */
    const val DEVICE_NAVIGATION_FALLBACK_ACCURACY_METERS: Double = 8.0
    const val PLANNED_ROUTE_FALLBACK_SPEED_METERS_PER_SECOND: Double = 1.4
    /**
     * A route-following trip walks the route's own polyline through the
     * contract's `RouteSimulator`; the tick cadence below is how often a fix
     * is asked for (10 fixes a second, smooth at the demo's playback speeds).
     * The Compose reference application's constant.
     */
    const val ROUTE_WALK_TICK_INTERVAL_MILLISECONDS: Long = 100L
    /**
     * The drawer's selectable playback speed multipliers, applied to the
     * route's own average speed (the `SimulatedTripPace` multiplier). The user
     * picks one for the session; a route-following trip rebuilds its walker
     * at the new pace without moving the travelled position.
     */
    val SIMULATED_TRIP_SPEED_MULTIPLIER_CHOICES: List<Double> = listOf(1.0, 2.0, 4.0, 8.0)
    /** The camera's zoom during a simulated trip (§2.4's `navigation-follow-zoom`). */
    const val NAVIGATION_FOLLOW_ZOOM_LEVEL: Double = 17.0
    /** The follow zoom's clamp toward trip end (§2.4's `navigation-follow-zoom-maximum`). */
    const val NAVIGATION_FOLLOW_ZOOM_MAXIMUM_LEVEL: Double = 17.5
    /** The 3D follow camera's pitch (§2.4's `navigation-follow-pitch-3d`). */
    const val NAVIGATION_FOLLOW_PITCH_3D_DEGREES: Double = 45.0
    /**
     * The follow camera's bearing chase time constant (§5.3: the exponential
     * chase toward the median-smoothed course — the Compose reference
     * application's documented platform constant; §2.4 fixes the control
     * easings and leaves the chase's smoothing to the platform).
     */
    const val FOLLOW_BEARING_CHASE_TIME_CONSTANT_SECONDS: Double = 0.15
    /** The bearing chase stops chasing residual deltas smaller than this (the
     * Compose application's documented snap). */
    const val FOLLOW_BEARING_CHASE_SNAP_DEGREES: Double = 0.05
    /** The median course smoother's fix window (§5.3; the Compose constant). */
    const val COURSE_SMOOTHING_WINDOW_FIXES: Int = 5
    /**
     * The one eased-camera-move duration (§2.4's `compass-renorth-animation`
     * 300 ms; the follow camera's 2D/3D pitch swing rides the same cadence).
     */
    const val EASED_CAMERA_MOVE_DURATION_SECONDS: Double = 0.3
    /** The map camera's zoom bounds (the renderer's clamp, §5.1). */
    const val MINIMUM_ZOOM_LEVEL: Double = 0.0
    const val MAXIMUM_ZOOM_LEVEL: Double = 24.0
    /** The flat 2D camera plane's pitch (§4.2's plane toggle). */
    const val TWO_D_CAMERA_PITCH_DEGREES: Double = 0.0
    /** A north-up camera's bearing (§4.2's compass control). */
    const val NORTH_UP_BEARING_DEGREES: Double = 0.0
    /** The zoom controls' step (§2.4's `map-control-zoom-step`). */
    const val MAP_CONTROL_ZOOM_STEP: Double = 1.0

    // --- Demo-plan picker (§4.1; the Compose application's demo plans) ---
    /** The pick attempt while the simulated trip is running (the drawer owns the dock). */
    const val PLAN_SELECT_TRIP_RUNNING_TEXT: String =
        "the simulated navigation trip is running; exit it before picking a plan"
    /** The pick's trace-load refusal (the verbatim asset load failure). */
    const val PLAN_SELECT_TRACE_UNAVAILABLE_TEXT: String = "the demo walk trace could not be loaded"

    // --- Navigation drawer (§7.2 v2 docks) ---
    const val NAVIGATION_PHASE_LABEL_NAVIGATING: String = "Navigating"
    const val NAVIGATION_PHASE_LABEL_OFF_ROUTE: String = "Off route"
    const val NAVIGATION_PHASE_LABEL_ARRIVED: String = "Arrived"
    const val NAVIGATION_EXIT_BUTTON_TEXT: String = "Exit"

    // --- Turn-banner and progress composition (§7.1) ---
    /** The banner's honest no-text line on the `Unavailable` instruction form. */
    const val NO_INSTRUCTION_TEXT: String = "No instruction text provided"
    /** The "then" line's prefix (§7.1). */
    const val THEN_LINE_PREFIX: String = "Then: "
    /** The arrival card's positive-stated word (§7.1). */
    const val ARRIVED_TEXT: String = "Arrived"
    /** Distance phrase word of the `Immediate` form (§7.1: `Immediate` renders no number). */
    const val DISTANCE_PHRASE_IMMEDIATE_TEXT: String = "Now"
    /** Roundabout suffix appended to the banner text when an exit index is live. */
    const val ROUNDABOUT_SUFFIX_TEMPLATE: String = " (exit %d)"
    /** Deviation strip text (§7.5), warning-styled. */
    const val DEVIATION_TEXT_TEMPLATE: String =
        "Off route — %s from the route line; " +
            "guidance is frozen until a graded fix recovers or a reroute applies."

    // --- Distance and duration formatting (the Compose screen's helpers) ---
    const val METERS_PER_KILOMETER: Double = 1000.0
    const val SECONDS_PER_MINUTE: Double = 60.0

    // --- Spoken guidance (§4.1/§6.3; the speech contract) ---
    /** The speech language tag the demo's guidance text is authored in. */
    const val SPEECH_LANGUAGE_TAG: String = "en-US"

    // --- Statistics throttling (§4.1's offline/live tile line) ---
    /** One statistics event per N composed frames keeps the JS thread unswamped. */
    const val STATISTICS_EVENT_EVERY_N_FRAMES: Long = 30

    // --- Route preview framing (§4.1's fit over the planned route) ---
    // Insets the route-preview `fit_bounds` keeps clear of map content, in
    // physical surface pixels: the floating panels' occupied band on the start
    // and top edges (one gutter plus one panel padding each, the Compose
    // application's approximation), and the readiness chip's band on the top
    // edge (§1's top-start dock). The bottom inset clears the half-state home
    // sheet (§2.4's sheet-half-height fraction of the surface extent) —
    // computed per fit, not a constant.
    const val ROUTE_PREVIEW_INSET_LEFT_PIXELS: Double = 30.0
    const val ROUTE_PREVIEW_INSET_TOP_PIXELS: Double = 48.0
    const val ROUTE_PREVIEW_INSET_RIGHT_PIXELS: Double = 30.0
    /** The route-preview fit's zoom clamp (the SDK's own maximum; Compose constant). */
    const val MAXIMUM_FIT_BOUNDS_ZOOM_LEVEL: Double = 24.0

    // --- Offline maps (§4.1's card over the registered tile cache) ---
    // The tile cache's file inside the app's caches directory; the SDK's
    // tile-cache registration arm opens (or creates) the SQLite response
    // cache there (the Compose reference application's file).
    const val TILE_CACHE_DATABASE_FILE_NAME: String = "reference-tile-cache.sqlite"
    /** The cache's named 16 MiB byte budget (the Compose application's demo budget). */
    const val TILE_CACHE_ABSOLUTE_MAXIMUM_BYTES: ULong = 16777216UL

    // --- The offline archive route (§2.5's offline routing copy) ---
    /** The committed compiled routing archive the route control reads from the app assets. */
    const val OFFLINE_ROUTING_ARCHIVE_ASSET_NAME: String = "routing_grid.rmog"
    // The offline archive's demo request: the routing-grid fixture's straight
    // oneway street (the wasm seam's test pair). The contract call takes the
    // archive profile's defaults (automobile, fastest, metric).
    const val OFFLINE_ROUTE_START_LATITUDE_DEGREES: Double = 0.001
    const val OFFLINE_ROUTE_START_LONGITUDE_DEGREES: Double = 0.0
    const val OFFLINE_ROUTE_END_LATITUDE_DEGREES: Double = 0.001
    const val OFFLINE_ROUTE_END_LONGITUDE_DEGREES: Double = 0.002
    /** The route summary's title for an offline archive route (§2.5). */
    const val OFFLINE_ROUTE_TITLE_TEXT: String = "Offline demo route"

    // --- Search and route planning (§4.1's plan flow) ---
    /** The demo search shows at most this many provider result rows. */
    const val SEARCH_RESULT_ROW_LIMIT: Int = 5
    /** The trip source a search-planned route replays (its own shape, in order). */
    const val PLANNED_ROUTE_TRACE_NAME: String = "planned_route_shape"
    /** The route summary card's title fallback when no destination name crossed. */
    const val PLANNED_ROUTE_FALLBACK_NAME: String = "Planned route"
    /** The verbatim search attempt before the SDK objects were built. */
    const val SEARCH_BEFORE_SDK_STATE_TEXT: String = "the SDK has not started"
    /** The refusal when a second search overlaps the running one. */
    const val SEARCH_IN_FLIGHT_TEXT: String = "a search is already in flight"
    /** The route plan's unresolved-origin refusal (the artifact's demo position is the default). */
    const val PLAN_ORIGIN_UNRESOLVED_TEXT: String = "the plan origin was not resolved"

    /** The explicit Plan route action's destination-side refusal. */
    const val PLAN_DESTINATION_UNRESOLVED_TEXT: String = "the plan destination was not resolved"
    /** The refusal when a second route plan overlaps the running one. */
    const val PLAN_ALREADY_RUNNING_TEXT: String = "a route plan is already in flight"
    /**
     * The keyless plan's honest refusal when the artifact records no route
     * for the ordered pair (§2.5 `plan.no_recorded_route_hint`): the slots
     * are the origin and destination names.
     */
    const val NO_RECORDED_ROUTE_HINT_TEMPLATE: String =
        "No recorded demo route from %s to %s; apply a provider key for live routing."
    /** The state text the plan carries while its route calculates. */
    const val PLAN_IN_FLIGHT_TEXT: String = "Planning the route…"
    /** The refusal when the Directions choice names no contract enum value. */
    const val PLAN_UNKNOWN_DIRECTIONS_CHOICE_TEXT: String =
        "the plan's directions choice is not recognised"
    /** The plan flow's half sheet height fraction (§2.4's fit bottom inset input). */
    const val SHEET_HALF_HEIGHT_FRACTION: Double = 0.5

    // --- The shared behavior model's consumer copy
    // (docs/ReferenceAppUX.md; the same values theme.ts
    // carries) ---
    const val PLAN_ORIGIN_LABEL_TEXT: String = "Origin"
    const val PLAN_DESTINATION_LABEL_TEXT: String = "Destination"
    const val PLAN_SEARCH_ACTION_TEXT: String = "Search"
    const val PLAN_MY_LOCATION_ACTION_TEXT: String = "My location"
    const val PLAN_SWAP_ACTION_TEXT: String = "Swap origin and destination"
    const val PLAN_MISSING_ORIGIN_TEXT: String = "Enter an origin to plan the route."
    const val PLAN_MISSING_DESTINATION_TEXT: String = "Enter a destination to plan the route."
    const val PLAN_MISSING_BOTH_TEXT: String =
        "Enter an origin and a destination to plan the route."
    const val PLAN_MY_LOCATION_UNAVAILABLE_TEMPLATE: String =
        "My location is unavailable: %s"
    const val PLAN_TRIP_SOURCE_DEMO_PLAN_TEMPLATE: String =
        "Selected demo plan: %s"
    const val SETTINGS_PROVIDER_ROW_TEMPLATE: String = "%s · %s"
    const val SETTINGS_PROVIDER_STATE_READY_TEXT: String = "Ready"
    const val SETTINGS_PROVIDER_STATE_NEEDS_CREDENTIAL_TEMPLATE: String = "Needs %s"
    const val SETTINGS_PROVIDER_STATE_NOT_CONFIGURED_TEXT: String = "Not configured"
    const val SETTINGS_ROUTING_DEMO_ENDPOINT_TEMPLATE: String = "Demo server: %s"
    const val SETTINGS_ROUTING_UNREACHABLE_NEXT_STEP_TEXT: String =
        "The demo server did not answer. Retry, or enter your own deployment address."
    const val DIAGNOSTICS_SHOW_DEBUG_EVENTS_TEXT: String = "Show debug events"
    const val DIAGNOSTICS_HIDE_DEBUG_EVENTS_TEXT: String = "Hide debug events"
    const val DIAGNOSTICS_CLEAR_ACTION_TEXT: String = "Clear"
    const val DIAGNOSTICS_NO_EVENTS_TEXT: String = "No SDK events recorded yet."
    const val DIAGNOSTICS_STREAM_SCOPE_NOTE_TEXT: String =
        "Debug detail stays on until the app restarts."

    // --- Cache state (§4.1's two honest lines) ---
    const val CACHE_REGISTERED_TEMPLATE: String = "Cache registered: %s · %d bytes budget"
    const val CACHE_NOT_REGISTERED_TEXT: String = "Cache not registered"

    // --- Runtime provider credentials
    // (docs/ProviderCredentialsAndBrowserProviders.md §3–§6) ---
    /** The provider key the registration dispatch serves today. */
    const val MAPQUEST_PROVIDER_KEY: String = "mapquest"

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
     * this platform.
     */
    const val RUNTIME_PROVIDER_KEY_INTENT_EXTRA: String = "providerKey"
    /** The refusal before the SDK objects were built. */
    const val PROVIDER_CREDENTIAL_SDK_NOT_STARTED_TEXT: String = "the SDK has not started"
    /** The refusal when a provider has no registration arm yet. */
    const val PROVIDER_CREDENTIAL_UNSUPPORTED_TEMPLATE: String = "provider '%s' is not supported"
    /** The refusal of an empty credential at the native boundary. */
    const val PROVIDER_CREDENTIAL_EMPTY_TEXT: String = "the provider credential is empty"
    /** The refusal when the app's caches directory is unavailable. */
    const val PROVIDER_CREDENTIAL_CACHE_DIRECTORY_UNAVAILABLE_TEXT: String =
        "the app caches directory is unavailable"

    // --- Android secure store (the opt-in credential persistence, §5) ---
    /** The private preferences file the encrypted credentials live in. */
    const val PROVIDER_CREDENTIAL_PREFERENCES_FILE_NAME: String = "provider-credentials"
    /** The Android Keystore provider name. */
    const val PROVIDER_CREDENTIAL_ANDROID_KEYSTORE: String = "AndroidKeyStore"
    /** The one AES key alias the credential field encrypts under. */
    const val PROVIDER_CREDENTIAL_KEYSTORE_ALIAS: String = "refapp-provider-credential"
    /** The authenticated cipher the stored value uses (AES-256 in GCM). */
    const val PROVIDER_CREDENTIAL_CIPHER_TRANSFORMATION: String = "AES/GCM/NoPadding"
    /** The AES key's size in bits (AES-256). */
    const val PROVIDER_CREDENTIAL_KEY_SIZE_BITS: Int = 256
    /** The GCM authentication tag's size in bits. */
    const val PROVIDER_CREDENTIAL_GCM_TAG_SIZE_BITS: Int = 128
    /** The separator between the stored IV and ciphertext (both Base64). */
    const val PROVIDER_CREDENTIAL_IV_SEPARATOR: Char = ':'
    /** The preferences key prefix, one entry per provider. */
    const val PROVIDER_CREDENTIAL_PREFERENCE_KEY_PREFIX: String = "provider-credential."

    // --- Demo scenes (§4.1's chip carousel; the artifact's scene keys) ---
    /** The overlay scenes' keys in the artifact's presentation order. */
    val OVERLAY_SCENE_KEYS: List<String> = listOf("route", "polygon", "markers", "traffic")
}