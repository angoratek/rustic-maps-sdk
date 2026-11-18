//! The home sheet's half-state and full-state content components
//! (docs/ReferenceAppUX.md §4.1, §4.2): the route summary card, the offline
//! maps card, the camera preset chips, the theme row, and the search view.
//! Demo scenes live in the map layers panel; demo plans keep their data and
//! state without a visible picker row. All consumer rows name the feature,
//! never the seam (§1's product-language rule); renderer/adapter and fixture
//! provenance live in the diagnostics sheet.

import SwiftUI
import RusticMapsSDK
import MapNativeCoreUI

/// The route summary card (§4.1, §4.2): the route's distance phrase and
/// duration from the contract's `Route` records (formatted, never
/// paraphrased), a primary "Start simulated navigation" and a subtle
/// "Edit". The map's route line is its visual (the overlay the model
/// drew); a failed planning attempt keeps the card away and the error
/// strip above the sheet's grabber carries the report per §6.3.
struct RouteSummaryCardView: View {
    @ObservedObject var applicationModel: ReferenceApplicationModel

    @Environment(\.appTheme) private var theme

    var body: some View {
        if applicationModel.isRouteCalculationRunning {
            ReferenceControlGroup(groupLabel: "Route", disabledReason: nil) {
                Text(ReferenceApplicationConfiguration.routeSummaryPlanningText)
                    .font(ReferenceAppUXFont.body())
                    .foregroundStyle(ReferenceAppUXColor.textSecondary(theme))
            }
            .accessibilityIdentifier("route-summary-card")
        } else if let plannedRoute = applicationModel.plannedRoute {
            ReferenceControlGroup(groupLabel: "Route", disabledReason: nil) {
                VStack(alignment: .leading, spacing: AppLayout.stackTight) {
                    if applicationModel.plannedRouteIsOfflineArchiveRoute {
                        // The offline route's summary names the test graph it
                        // came from (§2.5's route title), not a live plan.
                        Text(ReferenceApplicationConfiguration.offlineRouteTitleText)
                            .font(ReferenceAppUXFont.bodyStrong())
                            .foregroundStyle(ReferenceAppUXColor.textSecondary(theme))
                            .accessibilityIdentifier("route-summary-offline-title")
                    }
                    HStack(alignment: .firstTextBaseline) {
                        Text(TripPhraseFormatting.metricDistancePhrase(
                            meters: plannedRoute.distanceMeters
                        ))
                        .font(ReferenceAppUXFont.title())
                        .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
                        Text(TripPhraseFormatting.durationPhrase(
                            seconds: plannedRoute.durationSeconds
                        ))
                        .font(ReferenceAppUXFont.body())
                        .foregroundStyle(ReferenceAppUXColor.textSecondary(theme))
                        Spacer()
                    }
                    if applicationModel.plannedRouteIsRecordedDemoRoute {
                        // The summary names the artifact's recorded route as
                        // such (§4.1): the same wording the web planner uses.
                        ReferenceCaptionText(
                            text: ReferenceApplicationConfiguration.recordedRouteNoteText
                        )
                        .accessibilityIdentifier("route-summary-recorded-note")
                    }
                    if applicationModel.plannedRouteIsOfflineArchiveRoute {
                        // The offline route is the bundled test graph's answer,
                        // never live data; the summary says so (§2.5).
                        ReferenceCaptionText(
                            text: ReferenceApplicationConfiguration.offlineRouteNoteText
                        )
                        .accessibilityIdentifier("route-summary-offline-note")
                    }
                    HStack(spacing: AppLayout.stackTight) {
                        ReferencePrimaryButton(
                            label: ReferenceApplicationConfiguration.startSimulatedNavigationLabel
                        ) {
                            applicationModel.startPlannedRouteNavigation()
                        }
                        .accessibilityIdentifier("route-summary-start")
                        ReferenceSubtleButton(
                            label: ReferenceApplicationConfiguration.routeEditLabel
                        ) {
                            applicationModel.searchFieldAssignmentTarget = .destination
                            applicationModel.homeSheetState = .full
                        }
                        .accessibilityIdentifier("route-summary-edit")
                    }
                }
            }
            .accessibilityIdentifier("route-summary-card")
        }
    }
}

/// The offline maps card (§4.1): the cache registration state (registered
/// path + byte budget, or the disabled-with-reason state, §6.2), the live
/// tile line fed by the per-draw `FrameStatistics`, and the offline region
/// flow (download with live progress and cancellation, the stored-region
/// list with its byte footprint, and explicit deletion).
struct OfflineMapsCardView: View {
    @ObservedObject var applicationModel: ReferenceApplicationModel

    @Environment(\.appTheme) private var theme

    var body: some View {
        VStack(alignment: .leading, spacing: AppLayout.stackTight) {
            ReferenceControlGroup(
                groupLabel: ReferenceApplicationConfiguration.offlineMapsGroupLabel,
                disabledReason: applicationModel.offlineMapsDisabledReason
            ) {
                cacheLines
                if let offlineRegionState = applicationModel.offlineRegionState {
                    OfflineRegionSectionView(regionState: offlineRegionState)
                }
            }
            // The offline route control plans over the bundled archive, so it
            // needs neither a provider key nor the tile cache: it stays
            // available while the cache group above is disabled-with-reason.
            ReferenceSubtleButton(
                label: ReferenceApplicationConfiguration.offlineRouteActionText
            ) {
                Task { await applicationModel.planOfflineRouteFromArchive() }
            }
            .disabled(applicationModel.isRouteCalculationRunning)
            .opacity(applicationModel.isRouteCalculationRunning ? 0.5 : 1)
            .accessibilityIdentifier("offline-route-button")
        }
    }

    /// The card's two honest cache lines (§4.1): the registration state and
    /// the live per-draw tile accounting.
    @ViewBuilder
    private var cacheLines: some View {
        VStack(alignment: .leading, spacing: AppLayout.stackTight) {
            if let cacheState = applicationModel.offlineTileCacheState {
                ReferenceCaptionText(text: String(
                    format: ReferenceApplicationConfiguration.offlineCardRegisteredFormat,
                    cacheState.cacheDatabasePath,
                    cacheState.budgetPhrase
                ))
                .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
                .accessibilityIdentifier("offline-cache-state")
            } else if let uncachedNote =
                applicationModel.offlineTileCacheUncachedNote
            {
                ReferenceCaptionText(text: uncachedNote)
                    .foregroundStyle(ReferenceAppUXColor.warning(theme))
                    .accessibilityIdentifier("offline-cache-state")
            }
            if applicationModel.offlineTileCacheState != nil
                || applicationModel.offlineTileCacheUncachedNote != nil
            {
                if let statistics = applicationModel.lastFrameTileStatistics {
                    ReferenceCaptionText(text: String(
                        format:
                            ReferenceApplicationConfiguration.offlineCardTileStatisticsFormat,
                        String(statistics.requestedTileCount),
                        String(statistics.presentTileCount + statistics.fallbackTileCount),
                        String(statistics.fallbackTileCount),
                        String(statistics.deferredTileCount),
                        String(statistics.unavailableTileCount),
                        String(statistics.overlayTriangleCount)
                    ))
                    .foregroundStyle(ReferenceAppUXColor.textSecondary(theme))
                    .accessibilityIdentifier("offline-tile-statistics")
                } else {
                    ReferenceCaptionText(
                        text: ReferenceApplicationConfiguration.offlineCardNoFrameYetText
                    )
                    .accessibilityIdentifier("offline-tile-statistics")
                }
            }
        }
    }
}

/// The offline card's region flow (docs/design/TilePackManager.md): the demo
/// region's name and enumerated tile count, the download with its live
/// progress line and Cancel control, the terminal line after completion,
/// cancellation or failure, and the stored-region list with explicit
/// deletion. The SDK's own no-cache refusal is stated verbatim instead of a
/// progress line the card could not honestly produce.
struct OfflineRegionSectionView: View {
    @ObservedObject var regionState: OfflineRegionState

    @Environment(\.appTheme) private var theme

    var body: some View {
        VStack(alignment: .leading, spacing: AppLayout.stackTight) {
            if let summaryText = regionState.demoRegionSummaryText {
                ReferenceCaptionText(text: summaryText)
                    .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
                    .accessibilityIdentifier("offline-region-summary")
            }
            if let refusalText = regionState.tileCacheRefusalText {
                ReferenceCaptionText(text: refusalText)
                    .foregroundStyle(ReferenceAppUXColor.warning(theme))
                    .accessibilityIdentifier("offline-region-unavailable")
            }
            downloadControl
            if let statusText = regionState.statusLineText {
                ReferenceCaptionText(text: statusText)
                    .accessibilityIdentifier("offline-region-status")
            }
            if let removedStatusText = regionState.removedStatusText {
                ReferenceCaptionText(text: removedStatusText)
                    .accessibilityIdentifier("offline-region-deleted-status")
            }
            storedRegionRows
        }
    }

    /// The download control: Download while idle or after a terminal state,
    /// Cancel while a download runs. Download stays available after a
    /// failure or a cancellation, so the SDK resumes from the stored tiles;
    /// it is not offered while the SDK states there is no tile cache.
    @ViewBuilder
    private var downloadControl: some View {
        if regionState.isDownloading {
            ReferenceSubtleButton(
                label: ReferenceApplicationConfiguration.offlineRegionCancelActionText
            ) {
                regionState.cancelDownload()
            }
            .accessibilityIdentifier("offline-region-cancel-button")
        } else {
            ReferenceSubtleButton(
                label: ReferenceApplicationConfiguration.offlineRegionDownloadActionText
            ) {
                regionState.download()
            }
            .disabled(
                regionState.demoRegion == nil || !regionState.isTileCacheAvailable
            )
            .opacity(
                regionState.demoRegion == nil || !regionState.isTileCacheAvailable
                    ? 0.5
                    : 1
            )
            .accessibilityIdentifier("offline-region-download-button")
        }
    }

    /// One row per stored region: its name, live tile count, payload bytes,
    /// effective retention and explicit deletion control.
    @ViewBuilder
    private var storedRegionRows: some View {
        if !regionState.storedRegions.isEmpty {
            VStack(alignment: .leading, spacing: AppLayout.stackTight) {
                ForEach(regionState.storedRegions) { storedRegion in
                    VStack(alignment: .leading, spacing: AppLayout.stackTight) {
                        Text(storedRegion.displayName)
                            .font(ReferenceAppUXFont.bodyStrong())
                            .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
                        ReferenceCaptionText(text: storedRegion.tilesLineText)
                        ReferenceCaptionText(text: storedRegion.bytesLineText)
                        ReferenceCaptionText(text: storedRegion.retentionLineText)
                        ReferenceSubtleButton(
                            label: ReferenceApplicationConfiguration
                                .offlineRegionDeleteActionText
                        ) {
                            Task { await regionState.deleteRegion(storedRegion) }
                        }
                        .accessibilityIdentifier(
                            "offline-region-delete-\(storedRegion.regionId)"
                        )
                    }
                    .accessibilityIdentifier(
                        "offline-region-stored-\(storedRegion.regionId)"
                    )
                }
            }
        }
    }
}

/// The camera preset chips (consumer-worded rows;
/// the preset cameras apply instantly — the animated travel surface is a
/// documented open item, §5.1).
struct CameraPresetCarousel: View {
    @ObservedObject var applicationModel: ReferenceApplicationModel

    var body: some View {
        ReferenceControlGroup(
            groupLabel: ReferenceApplicationConfiguration.cameraPresetsGroupLabel,
            disabledReason: nil
        ) {
            if let cameraPresets = applicationModel.catalog?.cameraPresets {
                LazyVGrid(
                    columns: [GridItem(.adaptive(minimum: 120), spacing: AppLayout.stackTight)],
                    alignment: .leading,
                    spacing: AppLayout.stackTight
                ) {
                    ForEach(cameraPresets, id: \.id) { preset in
                        ReferenceSubtleButton(
                            label: preset.label,
                            isSelected: applicationModel.selectedPresetIdentifier == preset.id
                        ) {
                            applicationModel.applyCameraPreset(preset)
                        }
                        .accessibilityIdentifier("camera-preset-chip")
                    }
                }
            } else {
                ReferenceCaptionText(text: "The shared demo content is still loading…")
            }
        }
    }
}

/// The theme row (§3): the toggle button's label names the choice it switches
/// to next, cycling light → dark → auto → light.
struct ThemeRowView: View {
    @ObservedObject var applicationModel: ReferenceApplicationModel

    @Environment(\.appTheme) private var theme

    var body: some View {
        ReferenceControlGroup(groupLabel: ReferenceApplicationConfiguration.themeRowLabel, disabledReason: nil) {
            ReferenceSubtleButton(label: applicationModel.themeChoice.switchControlLabel) {
                applicationModel.themeChoice = applicationModel.themeChoice.nextChoice
            }
            .accessibilityIdentifier("theme-toggle")
        }
    }
}

/// The search view (§4.1: the sheet's full state): the destination field
/// ("Where to?"), the origin field (the artifact's demo position is its
/// placeholder, freely editable), and one shared provider result list
/// under both. Result rows are the §6.3 provider results (name +
/// secondary detail + quality), never invented; a provider failure
/// reports itself in the error strip and the row list stays empty.
struct HomeSheetSearchContentView: View {
    @ObservedObject var applicationModel: ReferenceApplicationModel

    /// The destination field's focus (a focused field is the assignment
    /// target the result list assigns to).
    @FocusState private var isDestinationFieldFocused: Bool
    /// The origin field's focus.
    @FocusState private var isOriginFieldFocused: Bool

    @Environment(\.appTheme) private var theme

    var body: some View {
        // The sheet's full state scrolls as one surface: the plan rows, the
        // Directions choice and the result list share the sheet's height and
        // move together, so a new choice row never pushes results off-screen.
        ScrollView {
            VStack(alignment: .leading, spacing: AppLayout.stackGroup) {
                HStack(spacing: AppLayout.stackTight) {
                    ReferenceSubtleButton(
                        label: ReferenceApplicationConfiguration.searchViewCollapseLabel
                    ) {
                        applicationModel.homeSheetState = .half
                    }
                    .accessibilityIdentifier("search-view-back")
                    Text(ReferenceApplicationConfiguration.searchViewHeadingText)
                        .font(ReferenceAppUXFont.title())
                        .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
                        .accessibilityIdentifier("search-view-heading")
                    Spacer()
                    // An explicit keyboard dismissal keeps the result rows
                    // below the plan rows reachable while a field is focused.
                    if isOriginFieldFocused || isDestinationFieldFocused {
                        Button {
                            isOriginFieldFocused = false
                            isDestinationFieldFocused = false
                        } label: {
                            Image(systemName: "keyboard.chevron.compact.down")
                                .font(ReferenceAppUXFont.bodyStrong())
                        }
                        .accessibilityIdentifier("search-keyboard-dismiss")
                        .accessibilityLabel("Hide keyboard")
                    }
                }
            // The fields keep the documented order (§4.1): origin first,
            // destination second — the same pair Android, React Native and
            // the browser show.
            searchFieldRow(
                fieldIdentifier: "search-view-origin-field",
                fieldLabel: ReferenceApplicationConfiguration.originFieldLabelText,
                fieldMarker: ReferenceApplicationConfiguration.originFieldMarkerText,
                placeholderText: applicationModel.originFieldPlaceholderText
                    ?? ReferenceApplicationConfiguration.originFieldHintText,
                queryBinding: Binding(
                    get: { applicationModel.originSearchText },
                    set: { applicationModel.updateOriginSearchText($0) }
                ),
                isFieldFocused: $isOriginFieldFocused,
                assignmentTarget: .origin,
                currentResultIdentifier: applicationModel.originCoordinate,
                suggestionFieldPrefix: "origin",
                typeAheadController: applicationModel.originTypeAheadController
            )
            searchFieldRow(
                fieldIdentifier: "search-view-destination-field",
                fieldLabel: ReferenceApplicationConfiguration.destinationFieldLabelText,
                fieldMarker: ReferenceApplicationConfiguration.destinationFieldMarkerText,
                placeholderText:
                    ReferenceApplicationConfiguration.destinationFieldPlaceholderText,
                queryBinding: Binding(
                    get: { applicationModel.destinationSearchText },
                    set: { applicationModel.updateDestinationSearchText($0) }
                ),
                isFieldFocused: $isDestinationFieldFocused,
                assignmentTarget: .destination,
                currentResultIdentifier: applicationModel.destinationCoordinate,
                suggestionFieldPrefix: "destination",
                typeAheadController: applicationModel.destinationTypeAheadController
            )
            // The swap action exchanges the two fields and everything
            // resolved from them; it never plans by itself.
            ReferenceSubtleButton(
                label: ReferenceApplicationConfiguration.swapPlanFieldsActionText
            ) {
                applicationModel.swapPlanFields()
            }
            .accessibilityIdentifier("plan-swap-fields")
            ReferenceCaptionText(
                text: ReferenceApplicationConfiguration.searchAssignmentHintText(
                    target: applicationModel.searchFieldAssignmentTarget.rawValue
                )
            )
            .accessibilityIdentifier("search-assignment-hint")
            // The explicit Plan route action (§4.1): disabled with a reason
            // until both fields hold text; assigning a result never plans.
            if !applicationModel.planRouteReasonText.isEmpty {
                ReferenceCaptionText(text: applicationModel.planRouteReasonText)
                    .accessibilityIdentifier("plan-route-reason")
            }
            Button {
                Task { await applicationModel.planRouteFromFields() }
            } label: {
                Text(ReferenceApplicationConfiguration.planRouteButtonText)
                    .font(ReferenceAppUXFont.bodyStrong())
                    .frame(maxWidth: .infinity)
            }
            .buttonStyle(.borderedProminent)
            .disabled(!applicationModel.canPlanRoute
                || applicationModel.isRouteCalculationRunning)
            .accessibilityIdentifier("plan-route-button")
            // The Directions choice rides every plan action (the mode and,
            // for car plans, the strategy).
            DirectionsControlView(
                applicationModel: applicationModel,
                directionsState: applicationModel.directionsState
            )
            resultRows
            }
        }
        .accessibilityIdentifier("search-view-scroll")
        // Scrolling the plan surface dismisses the keyboard, so the result
        // rows below the Directions control are reachable while a field is
        // focused.
        .scrollDismissesKeyboard(.immediately)
        .onChange(of: isDestinationFieldFocused) { _ in
            applicationModel.searchFieldAssignmentTarget = .destination
        }
        .onChange(of: isOriginFieldFocused) { _ in
            applicationModel.searchFieldAssignmentTarget = .origin
        }
    }

    /// One search field row: the query field and its submit control (the
    /// shared provider run, §4.1), with the field's own "My location" chip
    /// beneath its leading edge.
    private func searchFieldRow(
        fieldIdentifier: String,
        fieldLabel: String,
        fieldMarker: String,
        placeholderText: String,
        queryBinding: Binding<String>,
        isFieldFocused: FocusState<Bool>.Binding,
        assignmentTarget: SearchFieldAssignmentTarget,
        currentResultIdentifier: GeographicCoordinate?,
        suggestionFieldPrefix: String,
        typeAheadController: SearchTypeAheadController
    ) -> some View {
        VStack(alignment: .leading, spacing: AppLayout.stackTight) {
            // Every field states what it is: a visible label plus its A/B
            // marker, matching the destination/origin pair on every target.
            HStack(spacing: AppLayout.stackTight) {
                Text(fieldMarker)
                    .font(ReferenceAppUXFont.caption())
                    .foregroundStyle(ReferenceAppUXColor.accent(theme))
                Text(fieldLabel)
                    .font(ReferenceAppUXFont.bodyStrong())
                    .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
                    .accessibilityIdentifier("\(fieldIdentifier)-label")
                Spacer()
            }
            HStack(spacing: AppLayout.stackTight) {
                TextField(placeholderText, text: queryBinding)
                    .font(ReferenceAppUXFont.body())
                    .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
                    .focused(isFieldFocused)
                    .submitLabel(.search)
                    .accessibilityIdentifier(fieldIdentifier)
                    .onSubmit {
                        applicationModel.searchFieldAssignmentTarget = assignmentTarget
                        Task { await applicationModel.runSearch() }
                    }
                Button(ReferenceApplicationConfiguration.searchSubmitLabel) {
                    applicationModel.searchFieldAssignmentTarget = assignmentTarget
                    Task { await applicationModel.runSearch() }
                }
                .font(ReferenceAppUXFont.bodyStrong())
                .foregroundStyle(ReferenceAppUXColor.accent(theme))
            }
            // This field's own type-ahead (`docs/design/Autocomplete.md`):
            // at most the shared row cap, one no-match row, or one inline
            // failure sentence; a shorter query shows nothing.
            SearchFieldSuggestionListView(
                controller: typeAheadController,
                fieldPrefix: suggestionFieldPrefix
            )
            myLocationChip(for: assignmentTarget)
        }
    }

    /// The field's "My location" chip: it starts the device source when it is
    /// off and fills only this field from the latest fix. The chip is
    /// disabled with the panel's reason when the source cannot start, and its
    /// accessibility label names the field so two chips stay distinct.
    private func myLocationChip(
        for field: SearchFieldAssignmentTarget
    ) -> some View {
        let layersState = applicationModel.mapLayersState
        let disabledReason = layersState.myLocationChipDisabledReason
        return VStack(alignment: .leading, spacing: AppLayout.stackTight / 2) {
            ReferenceSubtleButton(
                label: layersState.myLocationChipLabel(for: field),
                isSelected: layersState.hasMyLocationFill(for: field)
            ) {
                layersState.myLocationChipTapped(for: field)
            }
            .disabled(disabledReason != nil)
            .opacity(disabledReason == nil ? 1 : 0.45)
            .accessibilityIdentifier(
                field == .origin ? "origin-my-location" : "destination-my-location"
            )
            .accessibilityLabel(
                MapLayersPanelState.myLocationChipAccessibilityLabel(for: field)
            )
            if let disabledReason {
                ReferenceCaptionText(text: disabledReason)
            }
        }
    }

    /// The §6.3 provider result rows: name + secondary detail + quality;
    /// the assigned row marks itself subtly with the accent color.
    private var resultRows: some View {
        VStack(alignment: .leading, spacing: AppLayout.stackTight) {
            if applicationModel.isSearchRunning {
                ReferenceCaptionText(
                    text: ReferenceApplicationConfiguration.searchInFlightText
                )
                .accessibilityIdentifier("search-results-in-flight")
            } else if let keylessNoMatchText =
                applicationModel.keylessSearchNoMatchText
            {
                // The keyless filter's honest no-match line: it points at the
                // provider key for live search instead of a silent empty list.
                ReferenceCaptionText(text: keylessNoMatchText)
                    .accessibilityIdentifier("search-results-demo-no-match")
            } else if applicationModel.searchResults.isEmpty {
                ReferenceCaptionText(
                    text: ReferenceApplicationConfiguration.searchResultsEmptyText
                )
                .accessibilityIdentifier("search-results-empty")
            } else if applicationModel.searchResultsAreDemoPlaces {
                // The keyless filter's rows are the artifact's demo places:
                // the list says so, never presenting them as live search
                // results.
                ReferenceCaptionText(
                    text: ReferenceApplicationConfiguration.demoPlacesHeadingText
                )
                .accessibilityIdentifier("search-results-demo-heading")
            }
            ForEach(applicationModel.searchResults, id: \.structuredAddressText) {
                placeResult in
                Button(action: {
                    applicationModel.assignSearchResult(placeResult)
                }) {
                    VStack(alignment: .leading, spacing: 0) {
                        Text(placeResult.structuredAddressText)
                            .font(ReferenceAppUXFont.body())
                            .foregroundStyle(
                                isAssigned(placeResult)
                                    ? ReferenceAppUXColor.accent(theme)
                                    : ReferenceAppUXColor.textPrimary(theme)
                            )
                            .multilineTextAlignment(.leading)
                        ReferenceCaptionText(
                            text: placeResult.geocodeQuality.descriptionLine
                        )
                    }
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("search-result-row")
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("search-results-list")
    }

    private func isAssigned(_ placeResult: Place) -> Bool {
        applicationModel.searchFieldAssignmentTarget == .destination
            ? applicationModel.destinationCoordinate == placeResult.coordinate
            : applicationModel.originCoordinate == placeResult.coordinate
    }
}

/// One field's type-ahead list (`docs/design/Autocomplete.md`): the capped
/// suggestion rows while typing, one "No matches" row when the provider
/// found nothing, one inline sentence after a failure, and nothing while the
/// query is below the threshold. Choosing a row fills the field through the
/// controller's selection action and closes the list.
struct SearchFieldSuggestionListView: View {
    @ObservedObject var controller: SearchTypeAheadController
    /// The field's stable prefix (`origin`/`destination`), so two lists never
    /// share an accessibility identifier.
    let fieldPrefix: String

    @Environment(\.appTheme) private var theme

    var body: some View {
        switch controller.phase {
        case .idle:
            EmptyView()
        case let .failed(sentence):
            Text(sentence)
                .font(ReferenceAppUXFont.caption())
                .foregroundStyle(ReferenceAppUXColor.warning(theme))
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityIdentifier("\(fieldPrefix)-suggestion-error")
        case .noMatches:
            // One row, never an empty list.
            ReferenceCaptionText(text: SearchTypeAhead.noMatchesText)
                .accessibilityIdentifier("\(fieldPrefix)-suggestion-no-matches")
        case .suggestions:
            VStack(alignment: .leading, spacing: AppLayout.stackTight / 2) {
                ForEach(
                    Array(controller.suggestions.enumerated()),
                    id: \.offset
                ) { rowIndex, place in
                    Button {
                        controller.select(place)
                    } label: {
                        VStack(alignment: .leading, spacing: 0) {
                            Text(place.structuredAddressText)
                                .font(ReferenceAppUXFont.body())
                                .foregroundStyle(
                                    ReferenceAppUXColor.textPrimary(theme)
                                )
                                .multilineTextAlignment(.leading)
                            ReferenceCaptionText(
                                text: place.geocodeQuality.descriptionLine
                            )
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel(place.structuredAddressText)
                    .accessibilityIdentifier(
                        "\(fieldPrefix)-suggestion-row-\(rowIndex)"
                    )
                }
            }
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("\(fieldPrefix)-suggestion-list")
        }
    }
}

extension GeocodeQuality {
    /// The row's secondary detail line (the quality, named per the
    /// contract's enum).
    var descriptionLine: String {
        switch self {
        case .point: return "Point"
        case .streetAddress: return "Street address"
        case .intersection: return "Intersection"
        case .street: return "Street"
        case .postalCode: return "Postal code"
        case .city: return "City"
        case .county: return "County"
        case .state: return "State"
        case .country: return "Country"
        }
    }
}