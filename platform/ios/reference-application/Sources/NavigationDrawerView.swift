//! The navigation drawer (docs/ReferenceAppUX.md §1, §4.2, §7.2): the low
//! dock that replaces the home sheet in place during a simulated trip —
//! grabber, the ETA-distance line, the phase, the speech toggle and the
//! exit; it never scrolls (the §7 banner stack at the map's top carries
//! the content). The arrival state replaces the drawer's contents (§7.2).

import SwiftUI
import RusticMapsSDK
import MapNativeCoreUI

/// The navigation drawer (§4.2's navigating / arrived states).
struct NavigationDrawerView: View {
    @ObservedObject var applicationModel: ReferenceApplicationModel

    @Environment(\.appTheme) private var theme

    private var cardStore: NavigationStateCardStore {
        applicationModel.navigationCardStore
    }

    /// Whether the trip presents its arrival state: the phase topic's
    /// arrival, the instruction payload's arrival, or the walker's own
    /// report — all three keep the presentation until Exit.
    private var isArrived: Bool {
        cardStore.isTripArrived || applicationModel.hasSimulatedJourneyArrived
    }

    var body: some View {
        VStack(spacing: AppLayout.stackTight) {
            ReferenceSheetGrabber { _ in
                // The drawer has no fold states (§1: its content is fixed
                // by §7); the grabber is decorative here.
            }
            if isArrived {
                drawerArrivedContents
            } else {
                drawerTripContents
            }
        }
        .padding(.horizontal, AppLayout.panelPadding)
        .padding(.bottom, AppLayout.panelPadding)
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
        // The drawer's container identifier must not clobber the children's
        // own accessibility identifiers (the phase chip's, the exit's):
        // the container becomes its own accessibility element that
        // contains them.
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("navigation-drawer")
    }

    /// The trip contents: the ETA-distance line (the trip progress
    /// rendered as one phrase), the phase row (the phase text, never color
    /// alone, §7.1, with the speech toggle), and either the device-driven
    /// trip's location control or the simulated playback rows (Pause/Resume
    /// and Exit).
    private var drawerTripContents: some View {
        VStack(alignment: .leading, spacing: AppLayout.stackTight) {
            etaDistanceLine
            HStack(spacing: AppLayout.stackGroup) {
                Text(phaseText)
                    .font(ReferenceAppUXFont.bodyStrong())
                    .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
                    .accessibilityIdentifier("navigation-drawer-phase")
                Spacer()
                HStack(spacing: AppLayout.stackTight) {
                    Text(ReferenceApplicationConfiguration.drawerVoiceToggleLabel)
                        .font(ReferenceAppUXFont.body())
                        .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
                    ReferenceSwitchControl(isOn: $applicationModel.isSpeechEnabled)
                }
                .accessibilityIdentifier("navigation-drawer-voice-toggle")
            }
            if applicationModel.isDeviceNavigationActive {
                // A device-driven trip has no playback to pause and no pace
                // to choose; its Location control (the same control the half
                // sheet carries) stays reachable to end the trip.
                deviceNavigationRow
            } else {
                playbackSpeedRow
                if applicationModel.isJourneySeekable {
                    seekRow
                }
            }
            HStack(spacing: AppLayout.stackGroup) {
                Spacer()
                if !applicationModel.isDeviceNavigationActive {
                    Button(playbackControlText) {
                        if applicationModel.isJourneyPaused {
                            applicationModel.resumeSimulatedJourney()
                        } else {
                            applicationModel.pauseSimulatedJourney()
                        }
                    }
                    .font(ReferenceAppUXFont.bodyStrong())
                    .foregroundStyle(ReferenceAppUXColor.accent(theme))
                    .accessibilityIdentifier("navigation-playback-pause")
                }
                Button(ReferenceApplicationConfiguration.drawerExitLabel) {
                    applicationModel.stopSimulatedJourney()
                }
                .font(ReferenceAppUXFont.bodyStrong())
                .foregroundStyle(ReferenceAppUXColor.accent(theme))
                .accessibilityIdentifier("navigation-drawer-exit")
            }
        }
        .padding(.top, AppLayout.stackTight)
    }

    /// The device-driven trip's Location control (the half sheet's second
    /// Location control, kept reachable while the drawer replaces the sheet)
    /// and its honest status caption.
    private var deviceNavigationRow: some View {
        VStack(alignment: .leading, spacing: AppLayout.stackTight) {
            ReferenceSubtleButton(
                label: ReferenceApplicationConfiguration.deviceNavigationStopLabel
            ) {
                applicationModel.stopDeviceNavigation()
            }
            .accessibilityIdentifier("device-navigation-control")
            if let statusText = applicationModel.deviceLocationStatusText {
                ReferenceCaptionText(text: statusText)
                    // A distinct identifier from the half sheet's caption: both
                    // read the same state, and a duplicate identifier made the
                    // UI suite query a stale first match.
                    .accessibilityIdentifier("navigation-drawer-device-navigation-status")
            }
        }
    }

    /// The playback-speed row: the row label and one selectable
    /// chip per offered multiplier, the active one tinted. A route-following
    /// trip rebuilds its walker at the new pace without moving the travelled
    /// position; a recorded trace's replay cadence follows the same choice.
    private var playbackSpeedRow: some View {
        HStack(spacing: AppLayout.stackTight) {
            Text(ReferenceApplicationConfiguration.navigationSpeedLabelText)
                .font(ReferenceAppUXFont.body())
                .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
            ForEach(
                ReferenceApplicationConfiguration.simulatedTripSpeedMultiplierChoices,
                id: \.self
            ) { speedMultiplier in
                ReferenceSubtleButton(
                    label: speedMultiplierChipText(speedMultiplier),
                    isSelected: applicationModel.journeySpeedMultiplier == speedMultiplier,
                    action: {
                        applicationModel.selectSimulatedJourneySpeedMultiplier(speedMultiplier)
                    }
                )
                .accessibilityIdentifier(speedMultiplierChipIdentifier(speedMultiplier))
            }
            Spacer()
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("navigation-speed-row")
    }

    /// The seek row: one scrubber over the route's own length. Every
    /// movement asks the walker for that position, so the camera, the
    /// indicator, the ETA line and the arrival follow exactly as on a normal
    /// fix. The row stays visible in the arrival state so a backwards seek
    /// can leave it.
    private var seekRow: some View {
        HStack(spacing: AppLayout.stackTight) {
            Text(ReferenceApplicationConfiguration.navigationSeekLabelText)
                .font(ReferenceAppUXFont.body())
                .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
            Slider(
                value: Binding(
                    get: { applicationModel.journeyTravelledMeters },
                    set: { applicationModel.seekSimulatedJourney(toTravelledMeters: $0) }
                ),
                in: 0...max(
                    applicationModel.journeySeekableDistanceMeters,
                    ReferenceApplicationConfiguration.minimumSeekRangeMeters
                )
            )
            .accessibilityIdentifier("navigation-seek-slider")
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("navigation-seek-row")
    }

    /// One speed chip's label (the shared canonical `{multiplier}×`).
    private func speedMultiplierChipText(_ speedMultiplier: Double) -> String {
        String(
            format: ReferenceApplicationConfiguration.navigationSpeedMultiplierFormat,
            Int(speedMultiplier)
        )
    }

    /// One speed chip's accessibility identifier (multiplier-unique, so the
    /// UI suite taps a named pace).
    private func speedMultiplierChipIdentifier(_ speedMultiplier: Double) -> String {
        "navigation-speed-chip-\(Int(speedMultiplier))"
    }

    /// The pause control's label: the action the tap performs (Pause while
    /// running, Resume while paused).
    private var playbackControlText: String {
        applicationModel.isJourneyPaused
            ? ReferenceApplicationConfiguration.navigationResumeActionText
            : ReferenceApplicationConfiguration.navigationPauseActionText
    }

    /// The arrived contents (§7.2: the arrival state replaces the
    /// drawer's contents — the positive-stated line and the exit; the
    /// banner stack shows the arrival card). The playback controls stay
    /// hidden: only the exit tears the arrival presentation down and
    /// restores the pre-trip camera.
    private var drawerArrivedContents: some View {
        VStack(alignment: .leading, spacing: AppLayout.stackTight) {
            HStack(spacing: AppLayout.stackGroup) {
                Text(ReferenceApplicationConfiguration.drawerArrivedText)
                    .font(ReferenceAppUXFont.title())
                    .foregroundStyle(ReferenceAppUXColor.positive(theme))
                    .accessibilityIdentifier("navigation-drawer-arrived")
                Spacer()
                Button(ReferenceApplicationConfiguration.drawerExitLabel) {
                    applicationModel.stopSimulatedJourney()
                }
                .font(ReferenceAppUXFont.bodyStrong())
                .foregroundStyle(ReferenceAppUXColor.accent(theme))
                .accessibilityIdentifier("navigation-drawer-exit")
            }
            if applicationModel.isJourneySeekable {
                seekRow
            }
        }
        .padding(.top, AppLayout.stackTight)
    }

    /// The drawer's ETA-distance line: the trip's remaining duration and
    /// distance phrases (the §7.2 progress numbers as one phrase); while
    /// the payload has not arrived yet the line reads the honest loading
    /// text, and while the guidance is frozen off route the line renders
    /// the frozen last-on-route projection as-is (§7.1).
    private var etaDistanceLine: some View {
        HStack(alignment: .firstTextBaseline) {
            Text(etaDistancePhrase)
                .font(ReferenceAppUXFont.maneuverDistance())
                .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
                .accessibilityIdentifier("navigation-drawer-eta-line")
            Spacer()
        }
    }

    private var etaDistancePhrase: String {
        guard let tripState = cardStore.tripState else {
            return ReferenceApplicationConfiguration.drawerPreparingTripText
        }
        let distancePhrase = TripPhraseFormatting.metricDistancePhrase(
            meters: tripState.progress.distanceRemainingMeters
        )
        let durationPhrase = TripPhraseFormatting.durationPhrase(
            seconds: tripState.progress.durationRemainingSeconds
        )
        return "\(ReferenceApplicationConfiguration.etaPrefixText): "
            + "\(ReferenceApplicationConfiguration.etaDistanceLabelText) \(distancePhrase) · "
            + "\(ReferenceApplicationConfiguration.etaDurationLabelText) \(durationPhrase)"
    }

    private var phaseText: String {
        guard let tripState = cardStore.tripState else {
            return ReferenceApplicationConfiguration.phaseNotStartedText
        }
        switch tripState.phase {
        case .navigating: return ReferenceApplicationConfiguration.phaseNavigatingText
        case .offRoute: return ReferenceApplicationConfiguration.phaseOffRouteText
        case .arrived: return ReferenceApplicationConfiguration.drawerArrivedText
        }
    }
}