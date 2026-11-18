//! The home sheet (docs/ReferenceAppUX.md §1, §4.2): the bottom sheet with
//! the grabber and its three fold states — the peek (the destination
//! search field and the primary action row, content-driven height per
//! `sheet-peek-content-rows`), the half (the working content at
//! `sheet-half-height`), and the full (the search view, capped at
//! `PANEL_MAXIMUM_HEIGHT_FRACTION`). The navigation drawer replaces the
//! sheet in place during a simulated trip (§1).

import SwiftUI
import RusticMapsSDK
import MapNativeCoreUI

/// The home sheet's surface and fold states; the contents are the state's
/// views (the peek's search field + action row, the half's working
/// content, the full's search view).
struct HomeSheetView: View {
    @ObservedObject var applicationModel: ReferenceApplicationModel

    /// The viewport size the fold fractions resolve against (passed in
    /// from the root's geometry).
    let viewportSize: CGSize

    /// The height the dock may take with the map controls and the top row
    /// still on-screen; the full state never grows past it.
    var maximumDockHeight: CGFloat = .infinity

    @Environment(\.appTheme) private var theme

    private var foldState: ReferenceAppUXSheetState {
        applicationModel.homeSheetState
    }

    /// The half state's height: `sheet-half-height` — the phone-width cap
    /// (`sheet-half-phone-height`) applies below the phone-width
    /// breakpoint so the map stays the larger half (§1).
    private var halfStateHeight: CGFloat {
        viewportSize.height * (
            isPhoneWidthViewport
                ? AppLayout.sheetHalfPhoneHeightFraction
                : AppLayout.sheetHalfHeightFraction
        )
    }

    /// The full state's height: the `PANEL_MAXIMUM_HEIGHT_FRACTION` cap
    /// (0.72 at phone width) so the attribution stays visible (§1).
    private var fullStateMaximumHeight: CGFloat {
        viewportSize.height * (
            isPhoneWidthViewport
                ? AppLayout.phonePanelMaximumHeightFraction
                : AppLayout.panelMaximumHeightFraction
        )
    }

    private var isPhoneWidthViewport: Bool {
        viewportSize.width < AppLayout.phoneWidthBreakpointPixels
    }

    var body: some View {
        VStack(spacing: 0) {
            ReferenceSheetGrabber { foldDirection in
                stepFoldState(foldDirection)
            }
            switch foldState {
            case .peek:
                HomeSheetPeekContentView(applicationModel: applicationModel)
            case .half:
                HomeSheetHalfContentView(applicationModel: applicationModel)
            case .full:
                HomeSheetSearchContentView(applicationModel: applicationModel)
            }
        }
        .padding(.horizontal, AppLayout.panelPadding)
        .padding(.bottom, AppLayout.panelPadding)
        .frame(
            height: foldStateHeight,
            alignment: .top
        )
        .background(
            UnevenRoundedRectangle(
                topLeadingRadius: AppLayout.radiusSurface,
                topTrailingRadius: AppLayout.radiusSurface
            )
            .fill(ReferenceAppUXColor.surface(theme))
            .shadow(
                color: AppLayout.elevationSurfaceColor,
                radius: AppLayout.elevationSurfaceRadius,
                x: 0,
                y: -AppLayout.elevationSurfaceOffsetY
            )
        )
        .overlay(
            UnevenRoundedRectangle(
                topLeadingRadius: AppLayout.radiusSurface,
                topTrailingRadius: AppLayout.radiusSurface
            )
            .strokeBorder(ReferenceAppUXColor.surfaceBorder(theme), lineWidth: 1)
        )
        .animation(.easeOut(duration: foldAnimationSeconds), value: foldState)
        // The sheet's container identifier must not clobber the children's
        // own accessibility identifiers (the peek rows', the grabber's):
        // the container becomes its own accessibility element that
        // contains them.
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("home-sheet")
    }

    /// The fold animation's duration — the sheet's platform glue easing,
    /// kept short enough to read as instant.
    private let foldAnimationSeconds: TimeInterval = 0.2

    /// The fold state's frame height: nil in peek (content-driven, never
    /// a viewport fraction); the half fraction; the full-state cap in
    /// full. The peek's frame height returns nil because the sheet grows
    /// with its two rows exactly (§1).
    private var foldStateHeight: CGFloat? {
        switch foldState {
        case .peek:
            return nil
        case .half:
            return halfStateHeight
        case .full:
            return min(fullStateMaximumHeight, maximumDockHeight)
        }
    }

    /// Steps the fold state one state in the drag's direction (the
    /// grabber's gesture; the state machine clamps at the ends, §4.2's
    /// back affordance collapses toward peek).
    private func stepFoldState(_ foldDirection: ReferenceSheetGrabber.SheetFoldDirection) {
        switch foldDirection {
        case .up:
            switch foldState {
            case .peek: applicationModel.homeSheetState = .half
            case .half: applicationModel.homeSheetState = .full
            case .full: break
            }
        case .down:
            switch foldState {
            case .peek, .half: applicationModel.homeSheetState = .peek
            case .full: applicationModel.homeSheetState = .half
            }
        }
    }
}

/// The peek content (§1: exactly `sheet-peek-content-rows` rows — the
/// destination search field "Where to?" and the one primary action row).
struct HomeSheetPeekContentView: View {
    @ObservedObject var applicationModel: ReferenceApplicationModel

    @Environment(\.appTheme) private var theme

    var body: some View {
        VStack(spacing: AppLayout.stackTight) {
            Button(action: {
                applicationModel.searchFieldAssignmentTarget = .destination
                applicationModel.homeSheetState = .full
            }) {
                HStack(spacing: AppLayout.stackTight) {
                    Text(ReferenceApplicationConfiguration.destinationSearchPlaceholder)
                        .font(ReferenceAppUXFont.body())
                        .foregroundStyle(ReferenceAppUXColor.textSecondary(theme))
                    Spacer()
                }
                .padding(.horizontal, AppLayout.stackGroup)
                .padding(.vertical, AppLayout.stackTight)
                .background(
                    RoundedRectangle(cornerRadius: AppLayout.radiusControl)
                        .fill(ReferenceAppUXColor.surface(theme))
                )
                .overlay(
                    RoundedRectangle(cornerRadius: AppLayout.radiusControl)
                        .strokeBorder(ReferenceAppUXColor.surfaceBorder(theme), lineWidth: 1)
                )
            }
            .buttonStyle(.plain)
            .accessibilityIdentifier("home-sheet-peek-search")
            // §1: the peek's action row starts the trip — the planned
            // route's when one was planned, the demo-plan picker's selected
            // recorded trace otherwise — beside the origin entry that opens
            // the search view armed for the origin. Both plan fields are
            // therefore one tap from the home sheet, the same pair the
            // Android and React Native peeks carry.
            HStack(spacing: AppLayout.stackTight) {
                ReferencePrimaryButton(
                    label: ReferenceApplicationConfiguration.startSimulatedNavigationLabel
                ) {
                    applicationModel.startSimulatedJourney()
                }
                .accessibilityIdentifier("home-sheet-peek-start-navigation")
                ReferenceSubtleButton(
                    label: ReferenceApplicationConfiguration.originFieldHintText
                ) {
                    applicationModel.searchFieldAssignmentTarget = .origin
                    applicationModel.homeSheetState = .full
                }
                .accessibilityIdentifier("home-sheet-peek-set-origin")
            }
            // Start is honest about its source: with no planned route the
            // peek starts the selected demo plan and says so.
            if applicationModel.plannedRoute == nil,
               let selectedDemoPlan = applicationModel.selectedDemoPlan {
                ReferenceCaptionText(
                    text: ReferenceApplicationConfiguration
                        .tripSourceDemoPlanText(plan: selectedDemoPlan.label)
                )
                .accessibilityIdentifier("peek-trip-source")
            }
        }
        .padding(.top, AppLayout.stackTight)
    }
}

/// The device-location group: the source's own status and Stop action
/// live in the map's layer panel, so this row is a one-line pointer there.
/// The second control drives the planned route's guidance from the same
/// source's fixes.
struct DeviceLocationRowView: View {
    @ObservedObject var applicationModel: ReferenceApplicationModel

    @Environment(\.appTheme) private var theme

    var body: some View {
        ReferenceControlGroup(groupLabel: "Location", disabledReason: nil) {
            VStack(alignment: .leading, spacing: AppLayout.stackTight) {
                ReferenceSubtleButton(
                    label: "Device location in Map layers"
                ) {
                    applicationModel.openMapLayersPanel(focusDeviceLocation: true)
                }
                .accessibilityIdentifier("device-location-pointer")
                ReferenceSubtleButton(
                    label: applicationModel.isDeviceNavigationActive
                        ? ReferenceApplicationConfiguration.deviceNavigationStopLabel
                        : ReferenceApplicationConfiguration.deviceNavigationActionLabel
                ) {
                    applicationModel.toggleDeviceNavigation()
                }
                .accessibilityIdentifier("device-navigation-control")
                if let statusText = applicationModel.deviceLocationStatusText {
                    ReferenceCaptionText(text: statusText)
                        .accessibilityIdentifier("device-navigation-status")
                }
            }
        }
    }
}

/// The half content (§1: search results → the route summary card, location,
/// offline maps, camera presets and theme; plus the plan flow's remaining
/// groups). Demo scenes live in the map layers panel.
struct HomeSheetHalfContentView: View {
    @ObservedObject var applicationModel: ReferenceApplicationModel

    @Environment(\.appTheme) private var theme

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: AppLayout.stackGroup) {
                RouteSummaryCardView(applicationModel: applicationModel)
                DeviceLocationRowView(applicationModel: applicationModel)
                OfflineMapsCardView(applicationModel: applicationModel)
                CameraPresetCarousel(applicationModel: applicationModel)
                themeRow
                simulatedNavigationRow
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("home-sheet-half-content")
    }

    /// The theme row (§2's appearance group in the sheet's language).
    private var themeRow: some View {
        ThemeRowView(applicationModel: applicationModel)
    }

    /// The simulated navigation's sample journey controls (the recorded
    /// trace demo; the planned route's "Start simulated navigation" lives
    /// on the route summary card).
    private var simulatedNavigationRow: some View {
        ReferenceControlGroup(groupLabel: ReferenceApplicationConfiguration.simulatedNavigationGroupLabel, disabledReason: nil) {
            VStack(alignment: .leading, spacing: AppLayout.stackTight) {
                HStack(spacing: AppLayout.stackTight) {
                    ReferenceSubtleButton(
                        label: ReferenceApplicationConfiguration.sampleJourneyLabel
                    ) {
                        applicationModel.startSampleJourney()
                    }
                    .accessibilityIdentifier("sample-journey-button")
                    HStack(spacing: AppLayout.stackTight) {
                        Text("Simulate off route")
                            .font(ReferenceAppUXFont.body())
                            .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
                        Spacer(minLength: AppLayout.stackTight)
                        ReferenceSwitchControl(
                            isOn: $applicationModel.isOffRouteSimulationActive
                        )
                    }
                    .accessibilityIdentifier("off-route-simulation-toggle")
                }
                ReferenceCaptionText(
                    text: ReferenceApplicationConfiguration.offRouteSimulationExplanation
                )
            }
        }
    }
}