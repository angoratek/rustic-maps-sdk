//! The map control cluster (docs/ReferenceAppUX.md §4.2): zoom −/+
//! (one step per tap, held repeat at the §2.4 cadence), the my-location
//! control, the 2D/3D pitch toggle, the scale bar (bottom-start) over the
//! contract's `ground_meters_per_screen_pixel`, and the compass (top-end,
//! visible only off-north, tap eases north up in the frame pump). The
//! controls float on the map's edges; the token table styles every surface
//! they draw.

import SwiftUI
import MapNativeCoreUI

/// The control cluster's platform-glue sizing (visual decisions no token
/// governs; all named).
enum MapControlClusterMetrics {
    /// The cluster buttons' square edge in points (the shared 40-point
    /// control diameter).
    static let controlButtonSize: CGFloat = AppLayout.mapControlDiameter
    /// The cluster stack's spacing (the §2.4 `stack-tight`).
    static let controlClusterSpacing: CGFloat = AppLayout.stackTight
    /// The scale bar's rule height in points.
    static let scaleBarRuleHeight: CGFloat = 6
    /// The SF Symbol glyph size the cluster's icon buttons render with.
    static let controlGlyphBodyPixels: CGFloat = 18
    /// The compact attribution info control's glyph size in points.
    static let controlGlyphCaptionPixels: CGFloat = 16
    /// The zoom −/+ hold buttons' glyph text.
    static let zoomInGlyph = "+"
    static let zoomOutGlyph = "−"
    /// The my-location control's SF Symbol glyph.
    static let myLocationGlyph = "location.fill"
    /// The compass control's north letter, the same mark every platform uses.
    static let compassNorthLetter = "N"
}

/// The map control cluster's floating vertical stack (§4.2, floating on
/// the map's end edge): zoom −, zoom +, my-location, 2D/3D.
struct MapControlClusterView: View {
    @ObservedObject var applicationModel: ReferenceApplicationModel

    @Environment(\.appTheme) private var theme

    var body: some View {
        VStack(spacing: MapControlClusterMetrics.controlClusterSpacing) {
            MapControlHoldButton(
                action: { applicationModel.userTappedZoomInControl() },
                label: MapControlClusterMetrics.zoomInGlyph,
                accessibilityIdentifier: "map-control-zoom-in"
            )
            MapControlHoldButton(
                action: { applicationModel.userTappedZoomOutControl() },
                label: MapControlClusterMetrics.zoomOutGlyph,
                accessibilityIdentifier: "map-control-zoom-out"
            )
            circularControlItemButton(
                glyph: MapControlClusterMetrics.myLocationGlyph,
                isActive: isRecentering
            ) {
                applicationModel.myLocationControlTapped()
            }
            .accessibilityIdentifier("map-control-my-location")
            .accessibilityLabel(myLocationControlLabel)
            // The long press opens the layer panel at its Device location
            // row (the specification's active-source gesture).
            .highPriorityGesture(
                LongPressGesture(minimumDuration: 0.55).onEnded { _ in
                    applicationModel.openMapLayersPanel(focusDeviceLocation: true)
                }
            )
            circularControlItemButton(label: pitchModeToggleLabel) {
                applicationModel.toggledThreeDimensionalPitch()
            }
            .accessibilityIdentifier("map-control-pitch-toggle")
        }
    }

    /// The my-location control's active re-center affordance (§4.2/§5.3):
    /// while a gesture-broken follow waits for an explicit re-engage, the
    /// control takes the cluster's primary look.
    private var isRecentering: Bool {
        applicationModel.isJourneyActive && applicationModel.isFollowBrokenByGesture
    }

    /// The my-location control's label names what its tap does: start the
    /// source when it is off, re-center while a position is live.
    private var myLocationControlLabel: String {
        if isRecentering || applicationModel.isDeviceLocationActive {
            return "Re-center on my location"
        }
        return "Show my location"
    }

    /// The 2D/3D toggle's label always names the pitch mode in force
    /// (§4.2: the toggle sets the §2.4 3D pitch or 0).
    private var pitchModeToggleLabel: String {
        applicationModel.isThreeDimensionalPitchEnabled ? "3D" : "2D"
    }

    /// One circular token-styled cluster button (the surface + border
    /// convention; the text-primary styling; the accent fill while active).
    private func circularControlItemButton(
        glyph: String? = nil,
        label: String? = nil,
        isActive: Bool = false,
        action: @escaping () -> Void
    ) -> some View {
        Button(action: action) {
            ClusterControlLabel(glyph: glyph, label: label, theme: theme)
                .frame(
                    width: MapControlClusterMetrics.controlButtonSize,
                    height: MapControlClusterMetrics.controlButtonSize
                )
                .background(
                    Circle().fill(
                        isActive
                            ? ReferenceAppUXColor.accent(theme)
                            : ReferenceAppUXColor.surface(theme)
                    )
                )
                .overlay(
                    Circle()
                        .strokeBorder(
                            isActive
                                ? ReferenceAppUXColor.accent(theme)
                                : ReferenceAppUXColor.surfaceBorder(theme),
                            lineWidth: 1
                        )
                )
                .contentShape(Circle())
        }
        .buttonStyle(.plain)
    }
}

/// The cluster control's inner label (an SF Symbol glyph or a text label,
/// styled per the token table).
private struct ClusterControlLabel: View {
    let glyph: String?
    let label: String?
    let theme: AppTheme

    var body: some View {
        if let glyph {
            Image(systemName: glyph)
                .font(.system(size: MapControlClusterMetrics.controlGlyphBodyPixels))
                .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
        } else if let label {
            Text(label)
                .font(ReferenceAppUXFont.bodyStrong())
                .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
        }
    }
}

/// A zoom control that repeats its step while held (§2.4's
/// `map-control-zoom-repeat-interval`): the first step fires on press-down
/// and the press-up ends the repeat — pure platform glue over the held-
/// repeat convention.
struct MapControlHoldButton: View {
    /// The step application the control runs (on press-down, then per
    /// repeat tick while held).
    let action: () -> Void
    let label: String
    let accessibilityIdentifier: String

    @Environment(\.appTheme) private var theme
    @State private var isPressActive = false
    @State private var holdRepeatTimer: Timer?

    var body: some View {
        Button {
            action()
        } label: {
            Text(label)
                .font(.system(size: MapControlClusterMetrics.controlGlyphBodyPixels))
                .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
                .frame(
                    width: MapControlClusterMetrics.controlButtonSize,
                    height: MapControlClusterMetrics.controlButtonSize
                )
                .background(Circle().fill(ReferenceAppUXColor.surface(theme)))
                .overlay(
                    Circle()
                        .strokeBorder(ReferenceAppUXColor.surfaceBorder(theme), lineWidth: 1)
                )
                .contentShape(Circle())
        }
        .buttonStyle(PressReportingButtonStyle(isPressed: $isPressActive))
        .onChange(of: isPressActive) { pressedNow in
            if pressedNow {
                beginHoldRepeat()
            } else {
                endHoldRepeat()
            }
        }
        .onDisappear {
            endHoldRepeat()
        }
        .accessibilityIdentifier(accessibilityIdentifier)
    }

    /// Starts the held repeat: the §2.4 cadence timer re-applies the
    /// control's step until the press ends.
    private func beginHoldRepeat() {
        guard holdRepeatTimer == nil else { return }
        let repeatedStepAction = action
        holdRepeatTimer = Timer.scheduledTimer(
            withTimeInterval: AppLayout.mapControlZoomRepeatInterval,
            repeats: true
        ) { _ in
            Task { @MainActor in
                repeatedStepAction()
            }
        }
    }

    private func endHoldRepeat() {
        holdRepeatTimer?.invalidate()
        holdRepeatTimer = nil
    }
}

/// The button style that reports the button's pressed state through a
/// binding (the plain style settles press only visually).
struct PressReportingButtonStyle: ButtonStyle {
    @Binding var isPressed: Bool

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .onChange(of: configuration.isPressed) { pressedNow in
                isPressed = pressedNow
            }
    }
}

/// The scale bar (§4.2, bottom-start): the §2.4 1-2-3-5-10 ladder over the
/// contract's `ground_meters_per_screen_pixel`, rendered at the honest
/// pixel width the distance measures at the drawn camera.
struct MapScaleBarView: View {
    @ObservedObject var applicationModel: ReferenceApplicationModel

    @Environment(\.appTheme) private var theme

    var body: some View {
        if let scaleBarReadout = applicationModel.scaleBarReadout {
            VStack(alignment: .leading, spacing: AppLayout.stackTight) {
                Text(scaleBarDistanceLabel(distanceMeters: scaleBarReadout.distanceMeters))
                .font(ReferenceAppUXFont.caption())
                .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
                RoundedRectangle(cornerRadius: AppLayout.radiusInline)
                    .strokeBorder(ReferenceAppUXColor.textPrimary(theme), lineWidth: 1)
                    .frame(
                        width: scaleBarReadout.barWidthPixels,
                        height: MapControlClusterMetrics.scaleBarRuleHeight
                    )
            }
            .padding(.horizontal, AppLayout.stackGroup)
            .padding(.vertical, AppLayout.stackTight)
            .background(
                RoundedRectangle(cornerRadius: AppLayout.radiusInline)
                    .fill(ReferenceAppUXColor.surface(theme))
            )
            .overlay(
                RoundedRectangle(cornerRadius: AppLayout.radiusInline)
                    .strokeBorder(ReferenceAppUXColor.surfaceBorder(theme), lineWidth: 1)
            )
            .accessibilityIdentifier("map-scale-bar")
        }
    }
}

/// The compass control (§4.2, top-end): appears only once the drawn
/// camera's bearing is off north by the §2.4 threshold; the needle
/// re-orients with the drawn bearing and the tap eases north up (§4.2's
/// re-north path, pump-eased).
struct MapCompassControl: View {
    @ObservedObject var applicationModel: ReferenceApplicationModel

    @Environment(\.appTheme) private var theme

    /// The compass's visibility read: the drawn camera's bearing (the
    /// attribute readback), off north by the §2.4 threshold.
    private var degreesOffNorth: Double {
        guard let drawnBearingDegrees = applicationModel.cameraAttributeReadback?.bearingDegrees
        else { return 0 }
        let normalizedBearing = wrappedBearingDegrees(drawnBearingDegrees)
        return min(normalizedBearing, Double(360) - normalizedBearing)
    }

    /// The needle's rotation: the drawn camera's bearing the needle points
    /// its north indicator against.
    private var drawnBearingDegrees: Double {
        wrappedBearingDegrees(applicationModel.cameraAttributeReadback?.bearingDegrees ?? 0)
    }

    var body: some View {
        if degreesOffNorth >= AppLayout.compassVisibilityBearingThresholdDegrees {
            Button {
                applicationModel.compassControlTapped()
            } label: {
                Text(MapControlClusterMetrics.compassNorthLetter)
                    .font(ReferenceAppUXFont.bodyStrong())
                    .foregroundStyle(ReferenceAppUXColor.accent(theme))
                    .frame(
                        width: MapControlClusterMetrics.controlButtonSize,
                        height: MapControlClusterMetrics.controlButtonSize
                    )
                    .background(Circle().fill(ReferenceAppUXColor.surface(theme)))
                    .overlay(
                        Circle()
                            .strokeBorder(ReferenceAppUXColor.surfaceBorder(theme), lineWidth: 1)
                    )
                    .rotationEffect(.degrees(-drawnBearingDegrees))
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .accessibilityIdentifier("map-compass-control")
        }
    }
}