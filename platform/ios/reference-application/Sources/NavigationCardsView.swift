//! The simulated trip's §7 card set at the v2 placement (docs/Reference
//! AppUX.md §7.2): the turn banner, then line and deviation alert stack at
//! the map's top-center over the navigation drawer; the progress card's
//! numbers render in the drawer's ETA line (§4.2) — every value renders
//! the contract's payload verbatim, the banner rules per
//! `ManeuverInstructionPresentation` (the distance phrase, the display
//! text priority, the roundabout suffix, the honest unavailable line).

import SwiftUI
import RusticMapsSDK
import MapNativeCoreUI

/// The turn banner (§7.1, §4): accent-bordered surface card at top-center;
/// the banner switches to the warning state while
/// `is_rerouting_required` keeps the frozen guidance visible, and swaps to
/// the arrival card when the trip arrives.
struct NavigationTurnBannerView: View {
    @ObservedObject var cardStore: NavigationStateCardStore
    let showArrivalState: Bool

    @Environment(\.appTheme) private var theme

    private var bannerBorderColor: Color {
        let isFrozenWhileReroutingRequired = cardStore.instruction?.isReroutingRequired == true
        if isFrozenWhileReroutingRequired {
            return ReferenceAppUXColor.warning(theme)
        }
        return ReferenceAppUXColor.accent(theme)
    }

    var body: some View {
        if showArrivalState {
            NavigationArrivalBannerView()
        } else if let instruction = cardStore.instruction {
            HStack(alignment: .firstTextBaseline, spacing: AppLayout.stackTight) {
                if let distancePhraseText = ManeuverInstructionPresentation.distancePhraseText(ofActiveManeuver: instruction.activeManeuver) {
                    Text(distancePhraseText)
                        .font(ReferenceAppUXFont.maneuverDistance())
                        .foregroundStyle(ReferenceAppUXColor.textSecondary(theme))
                }
                Text(ManeuverInstructionPresentation.bannerText(ofActiveManeuver: instruction.activeManeuver))
                    .font(ReferenceAppUXFont.maneuver())
                    .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
                    .lineLimit(2)
            }
            .padding(AppLayout.panelPadding)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(
                RoundedRectangle(cornerRadius: AppLayout.radiusControl)
                    .fill(ReferenceAppUXColor.surface(theme))
                    .shadow(
                        color: AppLayout.elevationSurfaceColor,
                        radius: AppLayout.elevationSurfaceRadius,
                        x: 0,
                        y: AppLayout.elevationSurfaceOffsetY
                    )
            )
            .overlay(
                RoundedRectangle(cornerRadius: AppLayout.radiusControl)
                    .strokeBorder(bannerBorderColor, lineWidth: 1.5)
            )
            // The banner's container identifier must not clobber the
            // composed slots' own accessibility (the distance phrase and
            // the maneuver line): the container becomes its own
            // accessibility element that contains them.
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("navigation-turn-banner")
        }
    }
}

/// The arrival banner (§7.1: positive-stated "Arrived").
struct NavigationArrivalBannerView: View {
    @Environment(\.appTheme) private var theme

    var body: some View {
        Text(ReferenceApplicationConfiguration.drawerArrivedText)
            .font(ReferenceAppUXFont.maneuver())
            .foregroundStyle(ReferenceAppUXColor.positive(theme))
            .padding(AppLayout.panelPadding)
            .frame(maxWidth: .infinity, alignment: .center)
            .background(
                RoundedRectangle(cornerRadius: AppLayout.radiusControl)
                    .fill(ReferenceAppUXColor.surface(theme))
                    .shadow(
                        color: AppLayout.elevationSurfaceColor,
                        radius: AppLayout.elevationSurfaceRadius,
                        x: 0,
                        y: AppLayout.elevationSurfaceOffsetY
                    )
            )
            .overlay(
                RoundedRectangle(cornerRadius: AppLayout.radiusControl)
                    .strokeBorder(ReferenceAppUXColor.positive(theme), lineWidth: 1.5)
            )
            .accessibilityIdentifier("navigation-arrival-banner")
    }
}

/// The then line (§7.1): a single text-secondary body line below the
/// banner, hidden while the contract's upcoming maneuver is null or the
/// trip arrived.
struct NavigationThenLineView: View {
    @ObservedObject var cardStore: NavigationStateCardStore
    let showArrivalState: Bool

    @Environment(\.appTheme) private var theme

    var body: some View {
        if !showArrivalState, let upcomingManeuver = cardStore.instruction?.upcomingManeuver,
           let thenLineText = ManeuverInstructionPresentation.thenLineText(ofUpcomingManeuver: upcomingManeuver) {
            Text("Then: \(thenLineText)")
                .font(ReferenceAppUXFont.body())
                .foregroundStyle(ReferenceAppUXColor.textSecondary(theme))
                .padding(.horizontal, AppLayout.stackGroup)
                .padding(.vertical, AppLayout.stackTight)
                .background(
                    RoundedRectangle(cornerRadius: AppLayout.radiusInline)
                        .fill(ReferenceAppUXColor.surface(theme))
                )
        }
    }
}

/// The deviation alert (§7.4): the warning strip shown exactly while a
/// deviation is live, narrated from the deviation topic's payload; hidden
/// again when the topic reports on route.
struct NavigationDeviationAlertView: View {
    @ObservedObject var cardStore: NavigationStateCardStore

    @Environment(\.appTheme) private var theme

    private var deviationAlertText: String? {
        guard let deviationStatusChange = cardStore.deviationStatusChange else { return nil }
        switch deviationStatusChange.statusChangeKind {
        case .offRoute:
            let deviationMeters = deviationStatusChange.deviationInfo?.deviationFromRouteMeters ?? 0
            return String(
                format: ReferenceApplicationConfiguration.deviationAlertFormat,
                "\(deviationMeters.decimalReadout(1)) m"
            )
        case .onRoute:
            return nil
        }
    }

    var body: some View {
        if let deviationAlertText {
            HStack(alignment: .firstTextBaseline, spacing: AppLayout.stackTight) {
                Text(ReferenceApplicationConfiguration.phaseOffRouteText)
                    .font(ReferenceAppUXFont.bodyStrong())
                    .foregroundStyle(ReferenceAppUXColor.warning(theme))
                Text(deviationAlertText)
                    .font(ReferenceAppUXFont.caption())
                    .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
                    .fixedSize(horizontal: false, vertical: true)
            }
            .padding(AppLayout.stackTight)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(
                RoundedRectangle(cornerRadius: AppLayout.radiusInline)
                    .fill(ReferenceAppUXColor.surface(theme))
            )
            .overlay(
                RoundedRectangle(cornerRadius: AppLayout.radiusInline)
                    .strokeBorder(ReferenceAppUXColor.warning(theme), lineWidth: 1)
            )
            .accessibilityIdentifier("navigation-deviation-alert")
        }
    }
}

/// Whether the trip arrived (the banner swap state of §7.1) — the phase
/// topic's arrival or the instruction payload's `is_arrival` marks it. The
/// card store itself is in the UI wrapper; the judgment lives here because
/// the two payload sources feed both banners and cards.
extension NavigationStateCardStore {
    var isTripArrived: Bool {
        if tripState?.phase == .arrived {
            return true
        }
        return instruction?.isArrival == true
    }
}