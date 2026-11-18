/**
 * The reference application's design tokens and consumer-language strings
 * (docs/ReferenceAppUX.md §2's token tables). The theme follows §3's owner
 * ruling: Light, Dark and Auto, with Auto the default on a device that never
 * chose; Auto follows the operating system live. SDK vocabulary stays out of
 * every consumer-facing row: the strings name product concepts (map, route,
 * walk demo), never seam names.
 *
 * Naming: the §2.1 rows keep the document's token names in camel case
 * (`surface`, `surfaceBorder`, `textPrimary`, …), the §2.3 type scale keeps
 * its exact row names (`typeCaption`, `typeBody`, `typeBodyStrong`,
 * `typeTitle`, `typeManeuver`, `typeManeuverDistance`), and the §2.4 rows are
 * mirrored either here (spacing, radii, framing) or in the behavior-token
 * modules they belong to (`mapControls.ts` for the map-control rows,
 * `sheetLayout.ts` for the home-sheet rows). A platform value that §2 does
 * not name is documented as platform-derived rather than presented as a
 * design-table row (§2.1's rule: a token exists if and only if it is in the
 * table).
 */

// --- The theme model (§3: Light, Dark and Auto; Auto is the default) ---

/** A resolved presentation theme: the colour table the tokens come from. */
export type ThemeName = 'dark' | 'light';

/** The three-way choice the appearance row cycles through (§3). */
export type ThemeChoice = 'light' | 'dark' | 'auto';

/** The choice of a device that never chose (§3: Auto is the default). */
export const defaultThemeChoice: ThemeChoice = 'auto';

/** The choice a stored value names, or Auto when it names none (§3). */
export function themeChoiceFromStoredValue(
  storedValue: string | null | undefined,
): ThemeChoice {
  return storedValue === 'light' || storedValue === 'dark' || storedValue === 'auto'
    ? storedValue
    : defaultThemeChoice;
}

/** The choice the toggle cycles to (light → dark → auto → light). */
export function nextThemeChoice(themeChoice: ThemeChoice): ThemeChoice {
  switch (themeChoice) {
    case 'light':
      return 'dark';
    case 'dark':
      return 'auto';
    case 'auto':
      return 'light';
  }
}

/**
 * The appearance row's label, always naming the choice it switches to next
 * ("Switch to auto theme", §3) — the same words the Android and iOS rows
 * render.
 */
export function themeChoiceSwitchLabel(themeChoice: ThemeChoice): string {
  return `Switch to ${nextThemeChoice(themeChoice)} theme`;
}

/**
 * The resolved presentation theme: the choice itself for Light and Dark, the
 * operating system's scheme while Auto. A platform that reports no scheme
 * (a test renderer, a browser without a preference) resolves to light, the
 * system default.
 */
export function resolvedThemeName(
  themeChoice: ThemeChoice,
  systemThemeName: ThemeName | null | undefined,
): ThemeName {
  if (themeChoice === 'auto') {
    return systemThemeName === 'dark' ? 'dark' : 'light';
  }
  return themeChoice;
}

// --- Color tokens (§2.1 rows, dark theme) ---
export const darkThemeColors = {
  surface: 'rgba(20, 24, 30, 0.86)',
  surfaceBorder: 'rgba(255, 255, 255, 0.14)',
  textPrimary: '#f2f4f8',
  textSecondary: '#9aa4b2',
  accent: '#4c8dff',
  positive: '#34c07c',
  warning: '#e0a03a',
  danger: '#e05656',
  /**
   * Platform-derived, not a §2.1 row: the selected-chip/primary-action tint
   * is the `accent` row at the dark theme's tint alpha (0.18).
   */
  accentSoft: 'rgba(76, 141, 255, 0.18)',
  /**
   * Platform-derived, not a §2.1 row: §4.1's sheets dim the map with this
   * theme-independent modal scrim (the iOS reference application's
   * diagnostics backdrop is the same 0.3–0.35 black).
   */
  scrim: 'rgba(0, 0, 0, 0.35)',
  /**
   * Platform-derived, not a §2.1 row: the platform screen's own ground behind
   * the GPU map surface. §2.1's own note keeps the canvas clear color a
   * renderer constant; this is only the layer a platform paints before the
   * map surface attaches.
   */
  screenBackdrop: '#0c0f14',
} as const satisfies ThemeColorTokens;

// --- Color tokens (§2.1 rows, light theme) ---
export const lightThemeColors = {
  surface: 'rgba(255, 255, 255, 0.92)',
  surfaceBorder: 'rgba(0, 0, 0, 0.12)',
  textPrimary: '#1c1e21',
  textSecondary: '#565c66',
  accent: '#2563eb',
  positive: '#1d8a4e',
  warning: '#a07016',
  danger: '#c22f2f',
  /** Platform-derived: the `accent` row at the light theme's tint alpha (0.14). */
  accentSoft: 'rgba(37, 99, 235, 0.14)',
  /** Platform-derived: the theme-independent modal scrim (see the dark table). */
  scrim: 'rgba(0, 0, 0, 0.35)',
  /** Platform-derived: the light screen ground behind the map surface. */
  screenBackdrop: '#e8ebf0',
} as const satisfies ThemeColorTokens;

/**
 * The active theme's color values: the §2.1 rows plus the documented
 * platform-derived values above (literal types widened).
 */
export interface ThemeColorTokens {
  surface: string;
  surfaceBorder: string;
  textPrimary: string;
  textSecondary: string;
  accent: string;
  positive: string;
  warning: string;
  danger: string;
  accentSoft: string;
  scrim: string;
  screenBackdrop: string;
}

/** The §2 token tables, addressed by the owned theme name. */
export const themeColorsByThemeName: Record<ThemeName, ThemeColorTokens> = {
  dark: darkThemeColors,
  light: lightThemeColors,
};


// --- Spacing tokens (§2.4) ---
export const themeSpacing = {
  gutterInline: 16,
  gutterBlock: 16,
  panelPadding: 14,
  stackTight: 8,
  stackGroup: 12,
  /** The diameter every circular map control shares (`map-control-diameter`). */
  mapControlDiameter: 40,
} as const;

// --- Shape tokens (§2.4) ---
export const themeRadii = {
  surface: 12,
  control: 8,
  inline: 6,
} as const;

/**
 * `elevation-surface` (§2.4): floating panels carry `0 1px 4px
 * rgba(0, 0, 0, 0.25)`, decomposed into the shadow parameters React Native
 * platforms accept (iOS `shadow*`, Android `elevation`).
 */
export const themeElevationSurface = {
  color: '#000000',
  offsetY: 1,
  blurRadius: 4,
  opacity: 0.25,
} as const;

/**
 * The §2.4 framing rows RN's native controller consumes through its own
 * seams (the JS side only names them; the camera work happens natively).
 */
export const themeFramingTokens = {
  /** `field-of-view-degrees`: the archetypal framed camera (60). */
  fieldOfViewDegrees: 60,
  /** `navigation-follow-zoom`: the follow camera's base zoom (17). */
  navigationFollowZoom: 17,
  /** `navigation-follow-zoom-maximum`: the follow zoom's end clamp (17.5). */
  navigationFollowZoomMaximum: 17.5,
  /** `navigation-follow-pitch-3d`: the 3D follow camera's pitch (45 degrees). */
  navigationFollowPitch3dDegrees: 45,
  /** `autocomplete-minimum-query-characters`: type-ahead starts at three. */
  autocompleteMinimumQueryCharacters: 3,
  /** `autocomplete-debounce-milliseconds`: the type-ahead quiet period. */
  autocompleteDebounceMilliseconds: 250,
  /** `autocomplete-maximum-results`: the type-ahead row cap. */
  autocompleteMaximumResults: 5,
} as const;

/**
 * The diagnostics behavior tokens (docs/ReferenceAppUX.md
 * §Diagnostics policy): the shared bounds the pane and the SDK ring use. The
 * values mirror `platform/reference-tokens.json` one for one.
 */
export const themeDiagnosticsTokens = {
  /** `diagnostics-event-window`: the pane keeps the most recent 100 events. */
  diagnosticsEventWindow: 100,
  /** `diagnostics-backlog-flush-limit`: at most 100 buffered events on open. */
  diagnosticsBacklogFlushLimit: 100,
  /** `diagnostics-refresh-cadence-milliseconds`: the pane's 500 ms refresh. */
  diagnosticsRefreshCadenceMilliseconds: 500,
  /** `diagnostics-ring-capacity`: the SDK's per-subsystem ring size. */
  diagnosticsRingCapacity: 512,
  /** `diagnostics-minimum-severity`: the subscription starts at Info. */
  diagnosticsMinimumSeverity: 'Info',
  /** `diagnostics-debug-severity`: the explicit switch lowers it to Debug. */
  diagnosticsDebugSeverity: 'Debug',
} as const;

// --- Type scale (§2.3: the document's row names verbatim) ---
export const themeTypography = {
  /** `type-caption`: status line, attribution, reasons. */
  typeCaption: { fontSize: 12, lineHeight: 16 },
  /** `type-body`: panel body, control labels. */
  typeBody: { fontSize: 13, lineHeight: 18 },
  /** `type-body-strong`: group labels, toggle labels. */
  typeBodyStrong: { fontSize: 13, lineHeight: 18, fontWeight: '600' },
  /** `type-title`: panel titles. */
  typeTitle: { fontSize: 14, lineHeight: 20, fontWeight: '600' },
  /** `type-maneuver`: the turn banner's instruction text. */
  typeManeuver: { fontSize: 22, lineHeight: 28, fontWeight: '600' },
  /** `type-maneuver-distance`: the distance phrase (lighter at the same size). */
  typeManeuverDistance: { fontSize: 22, lineHeight: 28, fontWeight: '400' },
} as const;

// --- Consumer-language strings (product words; §6.3 verbatim errors) ---
export const appStrings = {
  /** The one readiness signal (§6.1). */
  diagnosticsTitle: 'Diagnostics',
  /** The diagnostics sheet's recent-events block (§4.1). */
  diagnosticsRecentEventsTitleText: 'Recent events',
  /** The sheet's honest line when the native diagnostics seam did not load. */
  diagnosticsLinesUnavailableText:
    'Diagnostics are not available in this build; the native map module did not load.',
  /** The explicit opt-in that lowers the stream to Debug (§2.5 `diagnostics.show_debug_events`). */
  diagnosticsShowDebugEventsText: 'Show debug events',
  /** The same control once debug detail is on (§2.5 `diagnostics.hide_debug_events`). */
  diagnosticsHideDebugEventsText: 'Hide debug events',
  /** The pane's Clear action (§2.5 `diagnostics.clear_action`). */
  diagnosticsClearActionText: 'Clear',
  /** The pane's honest empty line (§2.5 `diagnostics.no_events`). */
  diagnosticsNoEventsText: 'No SDK events recorded yet.',
  /** The per-session scope note (§2.5 `diagnostics.stream_scope_note`). */
  diagnosticsStreamScopeNoteText: 'Debug detail stays on until the app restarts.',
  /** The chip's neutral text while no readiness state has been reported yet. */
  readinessUnknownText: 'Map status not reported yet',
  /** The ready line the package MapView's first frame reports. */
  readinessReadyPrefixText: 'Maps SDK gpu renderer active (version ',
  /** The degraded line a failed frame reports; the verbatim detail follows. */
  readinessDegradedPrefixText: 'Maps SDK surface active with a failed frame: ',
  /** The failed line a refused start reports; the verbatim detail follows. */
  readinessFailedPrefixText: 'Maps SDK failed to start: ',
  /** The chip's short ready line (§1: one short line; §2.5 `readiness.chip_ready`). */
  readinessChipReadyText: 'Ready',
  /** The chip's short loading line. */
  readinessChipLoadingText: 'Loading…',
  /** The chip's short degraded prefix; the detail follows it. */
  readinessChipDegradedPrefix: 'Degraded: ',
  /** The chip's degraded fallback when the ladder supplied no detail. */
  readinessChipDegradedFallbackText: 'a frame draw failed',
  /** The chip's short failed prefix; the detail follows it. */
  readinessChipFailedPrefix: 'Failed: ',
  /** The chip's failed fallback when the ladder supplied no detail. */
  readinessChipFailedFallbackText: 'unknown failure',
  retrySurfaceText: 'Retry surface',
  /** The bottom sheet's section headers (product words, not seam names). */
  cameraSectionHeader: 'Camera presets',
  /** The turn card's honest empty text when no guidance has arrived yet. */
  awaitingGuidanceText: 'Waiting for guidance…',
  // --- The v2 plan flow (§4.1; the Compose reference application's rows) ---
  /** The peek state's search field row (the plan flow's entry). */
  whereToText: 'Where to?',
  /** The peek's action row: the route's start (§2.5 `peek.primary_action`). */
  peekPrimaryActionText: 'Start simulated navigation',
  /** The route summary card's primary control (§4.2's primary act). */
  routeSummaryStartButtonText: 'Start simulated navigation',
  /** The route summary card's subtle control (back to the search view). */
  routeSummaryEditButtonText: 'Edit',
  /** The route summary's meta line while its route calculates (§4.1). */
  routeSummaryPlanningText: 'Planning the route…',
  /** The search view's field labels (§4.2's field pair; §2.5 `plan.origin_hint`). */
  originFieldHintText: 'Search an origin',
  /** The destination field's placeholder (§2.5 `plan.destination_placeholder`). */
  destinationFieldHintText: 'Search a destination',
  // --- The behavior model's field vocabulary (ReferenceApplicationBehavior.md) ---
  /** The origin field's visible label (§2.5 `plan.origin_label`). */
  planOriginLabelText: 'Origin',
  /** The destination field's visible label (§2.5 `plan.destination_label`). */
  planDestinationLabelText: 'Destination',
  /** One field's inline search action (§2.5 `plan.search_action`). */
  planSearchActionText: 'Search',
  /** One field's my-location chip (§2.5 `plan.my_location_action`). */
  planMyLocationActionText: 'My location',
  /** The field-pair swap action (§2.5 `plan.swap_action`). */
  planSwapActionText: 'Swap origin and destination',
  /** The missing-field plan reasons (§2.5 `plan.missing_origin`, …_destination, …_both). */
  planMissingOriginText: 'Enter an origin to plan the route.',
  planMissingDestinationText: 'Enter a destination to plan the route.',
  planMissingBothText: 'Enter an origin and a destination to plan the route.',
  /** The my-location chip's honest unavailable line (§2.5 `plan.my_location_unavailable`). */
  planMyLocationUnavailableFormat: 'My location is unavailable: {reason}',
  /** The trip source the peek starts when it falls back to a demo plan (§2.5 `plan.trip_source_demo_plan`). */
  planTripSourceDemoPlanFormat: 'Selected demo plan: {plan}',
  searchResultsLabelText: 'Search results',
  /** The search view's heading (§2.5 `plan.search_heading`). */
  searchHeadingText: 'Plan a route',
  searchInFlightText: 'Searching…',
  searchDoneText: 'Done',
  /** The explicit route action of the two-field plan flow (§4.1). */
  planRouteButtonText: 'Plan route',
  /** The plan action's disabled-with-reason lines, naming the missing field. */
  planRouteMissingBothText: 'Enter an origin and a destination to plan the route.',
  planRouteMissingOriginText: 'Enter an origin to plan the route.',
  planRouteMissingDestinationText: 'Enter a destination to plan the route.',
  /** The two-field flow's empty-list hint (§4.1; §2.5 `plan.empty_results_hint`). */
  searchHintText: 'No results yet; search a place above.',
  /**
   * The result list's heading while the keyless demo filter answers
   * (§2.5 `plan.demo_places_heading`): the rows are demo places, never live
   * search results.
   */
  demoPlacesHeadingText: 'Demo places',
  /** The keyless search's no-match line (§2.5 `plan.demo_no_match_hint`). */
  demoNoMatchHintFormat:
    'No demo place matches the {field}; apply a provider key in Settings for live search.',
  /** The type-ahead's one-row answer when the provider matched nothing. */
  planAutocompleteNoMatchesText: 'No matching places.',
  /** The type-ahead's one inline sentence when the provider failed. */
  planAutocompleteFailedText:
    'Suggestions are unavailable; keep typing to try again.',
  /** The route summary's note for the artifact's recorded route (§2.5 `plan.recorded_route_note`). */
  recordedDemoRouteNoteText: 'Recorded demo route, pedestrian, fastest.',
  /** The keyless plan's honest refusal for an unrecorded pair (§2.5 `plan.no_recorded_route_hint`). */
  noRecordedDemoRouteHintFormat:
    'No recorded demo route from {origin} to {destination}; apply a provider key for live routing.',
  /** The demo scenes row (§4.1's chip carousel). */
  demoScenesRowTitleText: 'Demo scenes',
  /** The demo-scenes clear action (§2.5 `overlays.clear_action`). */
  demoScenesClearText: 'Clear overlays',
  demoScenesInfoText: 'Info',
  /** The info affordance's honest empty text before any scene is selected. */
  demoScenesInfoEmptyText: 'Select a scene to see its description.',
  /**
   * §6.2's disabled-with-reason lead-in for the scene group: the overlay
   * capability needs the live map surface, and the readiness status follows.
   */
  demoScenesUnavailableReasonText:
    'Scene overlays draw on the live map surface only; the map is not ready yet: ',
  /**
   * The demo plans row (§4.1's picker of the recorded demo walks; the labels
   * and info-affordance descriptions mirror the Compose reference
   * application's plan consumer language — the rows name the feature, never
   * the seam or the fixture paths).
   */
  demoPlansRowTitleText: 'Demo plans',
  demoPlansInfoText: 'Info',
  /** The offline maps card (§4.1's two honest lines). */
  offlineMapsCardTitleText: 'Offline maps',
  offlineTilesUnavailableText: 'Tile statistics arrive with the first drawn frame.',
  /** The offline card's route control (§2.5 `offline.plan_route_action`). */
  offlineRouteActionText: 'Plan offline route',
  /** The route summary's title for an offline archive route (§2.5). */
  offlineRouteTitleText: 'Offline demo route',
  /** The route summary's honesty note for an offline archive route (§2.5). */
  offlineRouteNoteText: 'Planned offline from a recorded test graph, not live data.',
  /** The offline card's region-download control (§2.5 `offline.download_action`). */
  offlineDownloadActionText: 'Download offline region',
  /** The in-flight line; the counts follow it (§2.5 `offline.downloading_status`). */
  offlineDownloadingStatusText: 'Downloading…',
  /** The control that stops the in-flight download (§2.5 `offline.cancel_action`). */
  offlineCancelActionText: 'Cancel',
  /** The terminal line when every region tile is stored (§2.5). */
  offlineCompletedStatusText: 'Download complete',
  /** The terminal line when the download stops on an error (§2.5). */
  offlineFailedStatusText: 'Download failed',
  /** The stored-region tile-count label (§2.5). */
  offlineStoredTilesLabelText: 'Stored tiles',
  /** The stored-region payload-size label (§2.5). */
  offlineStoredBytesLabelText: 'Stored bytes',
  /** The control that removes a stored region (§2.5). */
  offlineDeleteActionText: 'Delete region',
  /** The line shown after a stored region is removed (§2.5). */
  offlineDeletedStatusText: 'Region deleted',
  /** The honest browser limitation (§2.5; the RN shell never renders it). */
  offlineBrowserUnavailableStatusText:
    'Offline region downloads are not available in the browser.',
  /**
   * The demo region's enumerated-tile unit. The canonical tables declare no
   * bare unit row; this is the platform's own count label.
   */
  offlineRegionTileUnitText: 'tiles',
  /** The theme row (§3: the toggle names the next choice). */
  themeRowTitleText: 'Theme',
  /**
   * The guided walk's unavailable reasons (§6.2, parity audit D8): the action
   * is disabled-with-reason rather than looking permanently enabled.
   */
  guidedWalkUnavailableNoNativeText:
    'The guided walk demo needs the native map module, which is not available in this build.',
  guidedWalkUnavailableNoPlanText:
    'Choose a demo plan to start the guided walk.',
  /** The navigation drawer (§7.2). */
  navigationDrawerExitButtonText: 'Exit',
  navigationDrawerEtaLabelText: 'ETA',
  /** The drawer's phase before the trip's first update (§2.5 `navigation.phase_not_started`). */
  phaseNotStartedText: 'Not started',
  /** The playback-speed row's label (§2.5 `navigation.speed_label`). */
  navigationSpeedLabelText: 'Speed',
  /** The playback control while the trip runs (§2.5 `navigation.pause_action`). */
  navigationPauseActionText: 'Pause',
  /** The playback control while the trip is paused (§2.5 `navigation.resume_action`). */
  navigationResumeActionText: 'Resume',
  /** One speed chip's label (§2.5 `navigation.speed_multiplier_label`). */
  navigationSpeedMultiplierFormat: '{multiplier}×',
  /** The route-position seek control's label (§2.5 `navigation.seek_label`). */
  navigationSeekLabelText: 'Seek',
  // --- The drawer's speech row (§4.2's speech toggle; the Compose string) ---
  speechToggleLabelText: 'Voice guidance',
  /** The §6.2 disabled-with-reason line while the engine is unavailable. */
  speechToggleUnavailableLeadInText: 'unavailable:',
  speechEngineStartingText: 'the engine is starting…',
  // --- The map control cluster (§4.2; the Compose accessibility strings) ---
  mapControlZoomOutSymbol: '−',
  mapControlZoomInSymbol: '+',
  mapControlZoomOutAccessibilityText: 'Zoom out one map-control step',
  mapControlZoomInAccessibilityText: 'Zoom in one map-control step',
  mapControlMyLocationAccessibilityText: 'Center on my location',
  /** The my-location control's label while a gesture-broken follow waits (§5.3). */
  mapControlRecenterAccessibilityText: 'Re-center on my location',
  mapControlPlaneAccessibilityText: 'Toggle the 2D and 3D camera planes',
  mapControlTwoDLabelText: '2D',
  mapControlThreeDLabelText: '3D',
  mapControlCompassNorthLetter: 'N',
  mapControlCompassAccessibilityText: 'Ease the camera north up',
  // --- The attribution info control (§1/§4.2) ---
  attributionInfoSymbol: 'ⓘ',
  attributionInfoAccessibilityText: 'Map attribution and terms',
  attributionPanelTitleText: 'Map attribution and terms',
  // --- The arrival presentation (§7.1). ---
  arrivedText: 'Arrived',
  // --- The settings surface (docs/Providers.md
  // §3): the gear beside the readiness chip and the descriptor-driven
  // provider credential rows. The strings name product concepts, never seam
  // names, and never echo a credential. ---
  settingsGearGlyph: '⚙',
  settingsGearAccessibilityText: 'Open settings',
  settingsTitleText: 'Settings',
  /** The settings surface's close control (§2.5 `settings.close_action`). */
  settingsDoneText: 'Close',
  /** The settings surface's reset row: clears the remembered choices record. */
  settingsResetPreferencesText: 'Reset remembered choices',
  settingsResetPreferencesNoteText:
    'Clears the theme, base map, demo plan, camera and search choices remembered on this device. Provider keys stay as they are.',
  // --- The base-map switch (M9; the Compose reference application's words) ---
  baseMapSectionHeaderText: 'Base map',
  baseMapActiveStatusText: 'Serving',
  baseMapInactiveStatusText: 'Select',
  /**
   * A credential provider is offered before its key is applied; the row names
   * what it needs (the descriptor's own credential label) instead of
   * disappearing, and selecting it points at that key field.
   */
  baseMapNeedsCredentialPrefixText: 'Needs ',
  /**
   * The offline card's honest line after a credential-less base map took the
   * raster slot: the tile cache went with the provider that installed it.
   */
  baseMapUncachedNoteText:
   'Serving base map runs uncached; select a keyed base map to use the cache.',
  // --- The styled vector base (the committed styles) ---
  baseMapVectorGroupLabelText: 'Vector styles',
  /**
   * The honest line while a styled vector base serves: the styles' place and
   * road names draw from the SDK's embedded glyph range, and route-reference
   * shield badges draw where the style declares them.
   */
  baseMapLabelsNoteText:
    'Vector styles draw land, water, roads with their names, and Latin place names.',
  /** Selecting a style before the map view exists cannot install anything. */
  baseMapViewNotReadyText:
    'Map view not ready; the style applies after the first frame.',
  /** The honest tile-failure line under an active vector base. */
  baseMapVectorFailedPrefixText:
    'Vector base tiles failed this frame: ',
  /** The credential row's hint when a not-yet-servable base map points at it. */
  providerCredentialEnterHintText: 'Enter a key, then Apply.',
  // --- The per-capability provider rows (docs/Providers.md) ---
  /** The settings section that chooses the provider serving each capability. */
  capabilityProviderSectionHeaderText: 'Providers by capability',
  /** A capability with no installed provider reads honestly, never blank. */
  capabilityProviderNotConfiguredText: 'Not configured',
  /** The one provider-row composition (§2.5 `settings.provider_row`). */
  settingsProviderRowFormat: '{provider} · {state}',
  /** A provider whose credential is satisfied, or needs none (§2.5 `settings.provider_state_ready`). */
  settingsProviderStateReadyText: 'Ready',
  /** A credentialed provider whose key is not applied (§2.5 `settings.provider_state_needs_credential`). */
  settingsProviderStateNeedsCredentialFormat: 'Needs {credential}',
  /** No provider is registered for the capability (§2.5 `settings.provider_state_not_configured`). */
  settingsProviderStateNotConfiguredText: 'Not configured',
  /** A keyless routing provider's public demo address (§2.5 `settings.routing_demo_endpoint`). */
  settingsRoutingDemoEndpointFormat: 'Demo server: {url}',
  /** The honest next step after an unreachable demo server (§2.5 `settings.routing_unreachable_next_step`). */
  settingsRoutingUnreachableNextStepText:
    'The demo server did not answer. Retry, or enter your own deployment address.',
  /**
   * The read-only tile rows' note: the map's layer control owns the base map
   * source, so the settings section must not offer a second selection path.
   */
  capabilityProviderReadOnlyNoteText:
    'The map’s layer control changes the base map source.',
  /** The picker's reason when the provider's key has not been applied yet. */
  capabilityProviderNeedsCredentialTemplate: 'Needs {credential}.',
  /** The picker's reason when the registry does not offer the chosen provider. */
  capabilityProviderNotOfferedTemplate: 'The provider {provider} is not offered for this capability.',
  // --- The plan sheet's Directions control (PRD.md §6 item 36) ---
  /** The Directions group's title in the plan sheet. */
  directionsTitleText: 'Directions',
  directionsModeRowTitleText: 'Mode',
  directionsStrategyRowTitleText: 'Strategy',
  directionsModeCarText: 'Car',
  directionsModeWalkText: 'Walk',
  directionsStrategyFastestText: 'Fastest',
  directionsStrategyShortestText: 'Shortest',
  /** A mode or strategy the serving provider cannot honour is disabled with its reason. */
  directionsCarUnsupportedText: 'The serving provider does not support car routes.',
  directionsWalkUnsupportedText: 'The serving provider does not support walking routes.',
  directionsShortestUnsupportedText:
    'The serving provider does not support shortest routes.',
  providerCredentialsSectionHeaderText: 'Provider credentials',
  /**
   * The shared consumer sentence for a capability with no provider
   * (`strings.provider_not_configured_detail_template`); the call site
   * substitutes the contract name.
   */
  providerNotConfiguredDetailTemplateText:
    "no '{provider}' provider is configured",
  /** The honest keyless state (§3: a first-class state, never an error). */
  providerCredentialKeylessNeutralText: 'Set a provider key to load live map data',
  /**
   * The not-applied status (§2.5 `settings.credential_not_applied`); the call
   * site substitutes `{provider}`.
   */
  providerCredentialKeylessStatusText: '{provider} key not applied',
  /**
   * The applied status (§2.5 `settings.credential_applied`); the call site
   * substitutes `{provider}`.
   */
  providerCredentialAppliedStatusText: '{provider} key applied',
  providerCredentialRememberedSuffixText: ' · remembered on this device',
  providerCredentialSessionOnlySuffixText: ' · this session only',
  /** The verbatim refusal's lead-in (§6.3 honesty; the detail follows). */
  providerCredentialRefusedLeadInText: 'Key refused: ',
  /** The secure-store failure's lead-in (the availability is never hidden). */
  providerCredentialStorageFailureLeadInText: 'Could not use the secure store: ',
  providerCredentialRememberLabelText: 'Remember on this device',
  providerCredentialApplyButtonText: 'Apply',
  providerCredentialClearButtonText: 'Clear',
  /**
   * §3's clear-is-honest rule: the registry has no unregister operation, so
   * the running session cannot become keyless without a restart.
   */
  providerCredentialRestartNoteText:
    'Clearing a key removes it from this device. Restart the app for the running session to become keyless again.',
  /** A row the SDK does not register on this platform renders disabled-with-reason. */
  providerCredentialUnavailableReasonText:
    'Registration is not available on this platform yet',
  /** The honest detail when the package SDK handle has not been created. */
  nativeSdkUnavailableText: 'the native SDK is unavailable',
} as const;