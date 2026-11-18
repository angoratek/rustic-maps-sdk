// Hand-written WebAssembly surface projections.
//
// The declarations below describe values the WebAssembly surface composes,
// renames or flattens from the contract records in `rustic_maps.d.ts`, or
// carries only on the browser. `scripts/generate-web-facade.mjs` appends this
// file verbatim to the generated mirror; keep the declarations here when a
// value cannot be derived from the frozen UniFFI contract.

/**
 * The active deviation of a trip snapshot. Mirrors the UDL
 * `DeviationStatusChange` shape where the snapshot flattens it: the snapshot
 * is `null` on route, so a present deviation always reports the `OffRoute`
 * status kind, and the measured distance replaces the nested deviation-info
 * record. The contract's wrong-step grade (which leaves the phase
 * `Navigating`) is guidance feedback and is not surfaced here.
 */
export interface NavigatorDeviation {
    /** Which form the status update takes (the contract's enum spelling). */
    readonly statusChangeKind: DeviationStatusChangeKind;
    /** Distance, in meters, from the raw fix to the remaining route line. */
    readonly distanceFromRouteMeters: number;
}

/**
 * The composed turn-by-turn instruction payload of a trip snapshot, mirroring
 * the UDL `NavigationInstruction` record: the active maneuver's two banner
 * lines, the following maneuver when the route has one, progress along the
 * remaining trip, and the arrival/rerouting markers.
 */
export interface NavigatorInstruction {
    /** The banner maneuver currently being traversed. */
    readonly activeManeuver: {
        /** Canonical distance phrase text (for example `"130 meters"`, or
         * `"now"` when the maneuver is immediate). */
        readonly distancePhraseText: string;
        /** Composed instruction text: the live distance prefix plus the
         * provider-authored display text. */
        readonly displayText: string;
        /** Provider-authored banner text, verbatim, or `null` when the
         * provider authored none. The native cards show this beside the
         * separately phrased distance; `displayText` repeats the distance
         * inside the sentence. */
        readonly providerDisplayText: string | null;
        /** The text-to-speech variant (the speech text with the same distance
         * prefix); empty when the provider authored no text. */
        readonly speechText: string;
    };
    /** The maneuver after the active one, or `null` on the final step and
     * after arrival. */
    readonly upcomingManeuver: {
        /** Composed instruction text of the following step. */
        readonly displayText: string;
        /** The text-to-speech variant of the following step. */
        readonly speechText: string;
    } | null;
    /** Progress along the remaining trip. */
    readonly progress: {
        /** Distance, in meters, to the end of the current step (the next
         * maneuver point). */
        readonly distanceToNextManeuverMeters: number;
        /** Total remaining distance, in meters. */
        readonly distanceRemainingMeters: number;
        /** Total remaining travel time, in seconds. */
        readonly durationRemainingSeconds: number;
    };
    /** True exactly while the trip has arrived with zero remaining progress. */
    readonly isArrival: boolean;
    /** True exactly while the trip is off route: the instruction must not be
     * presented as current guidance. */
    readonly isReroutingRequired: boolean;
}

/**
 * The trip snapshot the wasm `Navigator`'s `currentTripState()` and
 * `updateLocation()` methods answer, projecting the UDL
 * `TripNavigationState`/`NavigationInstruction` pair into the fields the
 * browser turn-by-turn card consumes.
 */
export interface NavigatorTripState {
    /** High-level trip phase (the contract's `NavigationPhase`). */
    readonly phase: NavigationPhase;
    /** The composed instruction payload of the current state. */
    readonly instruction: NavigatorInstruction;
    /** The active deviation, or `null` while the trip is on route. */
    readonly deviation: NavigatorDeviation | null;
    /** Position snapped onto the current step, or `null` until the first
     * location update has been processed. */
    readonly snappedCoordinate: GeographicCoordinate | null;
    /** True exactly while the trip has arrived. */
    readonly isArrived: boolean;
    /** Traversal-ordered index of the step currently traversed. */
    readonly currentStepIndex: number;
}

/**
 * One device position fix the wasm `PositionController` accepted: the Rust
 * `rustic-maps-positioning` record in the surface's lower-camel spelling. Reported
 * back by `latestFix`; the controller validates and smooths the same fixes
 * the native targets carry through the contract.
 */
export interface DevicePositionFix {
    /** Latitude in decimal degrees. */
    readonly latitude: number;
    /** Longitude in decimal degrees. */
    readonly longitude: number;
    /** The 68% horizontal-accuracy radius in meters, or null. */
    readonly horizontalAccuracyMeters: number | null;
    /** Direction of travel in degrees clockwise from true north, or null. */
    readonly courseHeadingDegrees: number | null;
    /** The fix instant in seconds since the Unix epoch, or null. */
    readonly timestampSeconds: number | null;
}

/**
 * One place row the WebAssembly geocoding and search calls answer: the
 * contract's `Place` record flattened to the surface's scalar field names.
 * Returned by `forwardGeocode`, `suggestPlaces`, `suggestPlacesInBoundary`,
 * `searchPlaces` and `searchPlacesInBoundary`.
 */
export interface PlaceRow {
    /** Display string the provider produced for this place. */
    readonly structuredAddressText: string;
    /** Resolved latitude in decimal degrees. */
    readonly latitudeDegrees: number;
    /** Resolved longitude in decimal degrees. */
    readonly longitudeDegrees: number;
    /** Structural precision reached for this resolution (the contract enum's name). */
    readonly qualityText: string;
}

/**
 * The plain options object the WebAssembly `calculateRouteWithOptions` call
 * accepts: the contract's `RouteOptions` and `AvoidOptions` records with
 * lower-camel field names. Every field is optional; an omitted field takes
 * the contract default (automobile, fastest, metric, no avoids, depart now).
 * Enum fields take the contract variant name or its lowercase spelling.
 */
export interface RouteOptionsObject {
    /** Mode of transport: automobile, bicycle or pedestrian. */
    readonly transportationMode?: string;
    /** Optimization target: fastest or shortest. */
    readonly routeStrategy?: string;
    /** Display units: metric or imperial. */
    readonly unitSystem?: string;
    /** Features to avoid; every flag defaults to false. */
    readonly avoidOptions?: {
        readonly tollRoads?: boolean;
        readonly limitedAccessHighways?: boolean;
        readonly ferries?: boolean;
        readonly unpavedRoads?: boolean;
    };
    /** Departure instant in seconds since the Unix epoch; omit to depart now. */
    readonly departureTimestampSeconds?: number;
}

/**
 * One navigation event delivered to a listener registered through the wasm
 * `Navigator.addNavigationListener`: the contract's foreign
 * `NavigationListener` topics modelled over wasm-bindgen. `instruction`
 * arrives for every dispatched fix, `phase` once per transition (with its
 * `phaseChange` name), `deviation` when the deviation judgment changes,
 * `speech` when a new non-empty instruction carries speech text, and the
 * wasm-only `stop` when the trip ends (with a null snapshot).
 */
export type NavigatorEventTopic =
    | "instruction"
    | "phase"
    | "deviation"
    | "speech"
    | "stop";

/**
 * One navigation event's shape. `snapshot` is the plain trip snapshot the
 * `currentTripState` read returns, or `null` for the stop topic.
 */
export interface NavigatorEvent {
    readonly topic: NavigatorEventTopic;
    readonly snapshot: NavigatorTripState | null;
    /** Present on a `speech` event: the contract's speech text. */
    readonly speechText?: string;
    /** Present on a `phase` event when the transition has a name. */
    readonly phaseChange?: NavigationPhaseChange;
}
