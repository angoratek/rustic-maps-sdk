//! The shared panel primitives of the design language: the translucent
//! rounded surfaces (§1), the group boxes with their labels (§4), the
//! reason strips and the chip row.

import SwiftUI
import MapNativeCoreUI

/// A floating panel surface: token background, border, radius and the
/// elevation shadow, with the panel padding inside.
struct ReferencePanelSurface: ViewModifier {
    @Environment(\.appTheme) private var theme

    private var panelShadowColor: Color {
        AppLayout.elevationSurfaceColor
    }

    func body(content: Content) -> some View {
        content
            .padding(AppLayout.panelPadding)
            .background(
                RoundedRectangle(cornerRadius: AppLayout.radiusSurface)
                    .fill(ReferenceAppUXColor.surface(theme))
                    .shadow(
                        color: panelShadowColor,
                        radius: AppLayout.elevationSurfaceRadius,
                        x: 0,
                        y: AppLayout.elevationSurfaceOffsetY
                    )
            )
            .overlay(
                RoundedRectangle(cornerRadius: AppLayout.radiusSurface)
                    .strokeBorder(ReferenceAppUXColor.surfaceBorder(theme), lineWidth: 1)
            )
    }
}

extension View {
    /// The panel surface presentation (§1's translucent rounded panels).
    func referencePanelSurface() -> some View {
        modifier(ReferencePanelSurface())
    }
}

/// A control group: the labeled box around a related feature (§4's
/// `fieldset.control-group`), with the reason strip a disabled-with-reason
/// group presents (§6.2).
struct ReferenceControlGroup<Content: View>: View {
    let groupLabel: String
    /// The reason strip text while the group is disabled-with-reason;
    /// never empty on a disabled group (§6.2), and the group content is
    /// disabled as a unit when set.
    let disabledReason: String?

    @Environment(\.appTheme) private var theme
    @ViewBuilder let content: () -> Content

    var body: some View {
        VStack(alignment: .leading, spacing: AppLayout.stackTight) {
            Text(groupLabel)
                .font(ReferenceAppUXFont.bodyStrong())
                .foregroundStyle(ReferenceAppUXColor.textSecondary(theme))
            content()
                .disabled(disabledReason != nil)
                .opacity(disabledReason == nil ? 1 : 0.5)
            if let disabledReason {
                Text(disabledReason)
                    .font(ReferenceAppUXFont.caption())
                    .foregroundStyle(ReferenceAppUXColor.warning(theme))
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .overlay(
            RoundedRectangle(cornerRadius: AppLayout.radiusControl)
                .strokeBorder(ReferenceAppUXColor.surfaceBorder(theme), lineWidth: 1)
        )
    }
}

/// A primary button (§4's "the one primary action in a group"): the
/// `accent` background with the surface-contrast label — the route
/// summary's and peek's "Start simulated navigation" affordance.
struct ReferencePrimaryButton: View {
    let label: String
    let action: () -> Void

    @Environment(\.appTheme) private var theme

    var body: some View {
        Button(action: action) {
            Text(label)
                .font(ReferenceAppUXFont.bodyStrong())
                .foregroundStyle(ReferenceAppUXColor.surface(theme))
                .lineLimit(1)
                .padding(.horizontal, AppLayout.stackGroup)
                .padding(.vertical, AppLayout.stackTight)
                .frame(maxWidth: .infinity)
                .background(
                    RoundedRectangle(cornerRadius: AppLayout.radiusControl)
                        .fill(ReferenceAppUXColor.accent(theme))
                )
        }
        .buttonStyle(.plain)
    }
}

/// The sheet's grabber (§4.2: "the sheet's only gesture target, inset
/// `gutter-inline` from each top corner"): the fold gesture's handle, not
/// itself a control.
struct ReferenceSheetGrabber: View {
    let foldAction: (SheetFoldDirection) -> Void

    /// The fold gesture's directions (up grows the sheet, down collapses
    /// it).
    enum SheetFoldDirection {
        case up
        case down
    }

    @Environment(\.appTheme) private var theme

    var body: some View {
        CenterAlignedGrabberCapsule()
            .frame(width: ReferenceApplicationConfiguration.sheetGrabberWidth,
                   height: ReferenceApplicationConfiguration.sheetGrabberHeight)
            .padding(.top, AppLayout.stackTight)
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 1)
                    .onEnded { dragValue in
                        if dragValue.translation.height < 0 {
                            foldAction(.up)
                        } else if dragValue.translation.height > 0 {
                            foldAction(.down)
                        }
                    }
            )
            .accessibilityIdentifier("sheet-grabber")
    }

    private struct CenterAlignedGrabberCapsule: View {
        @Environment(\.appTheme) private var theme

        var body: some View {
            Capsule()
                .fill(ReferenceAppUXColor.surfaceBorder(theme))
                .scaledToFit()
        }
    }
}

/// A subtle button (§4's secondary buttons: surface + border).
struct ReferenceSubtleButton: View {
    let label: String
    var isSelected: Bool = false
    let action: () -> Void

    @Environment(\.appTheme) private var theme

    var body: some View {
        Button(action: action) {
            Text(label)
                .font(ReferenceAppUXFont.body())
                .lineLimit(2)
                .multilineTextAlignment(.leading)
                .foregroundStyle(
                    isSelected
                        ? ReferenceAppUXColor.accent(theme)
                        : ReferenceAppUXColor.textPrimary(theme)
                )
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
    }
}
/// The token-styled switch control: an `accent` capsule track when on, the
/// theme `surface` capsule with its border when off, and a `text-primary`
/// knob in both — the native switch renders a light track that reads as an
/// invented white pill inside the dark glass panels (§2.1: no color slot
/// exists outside the token table).
struct ReferenceSwitchControl: View {
    /// The switch's two-way state binding.
    @Binding var isOn: Bool

    @Environment(\.appTheme) private var theme

    private enum SwitchMetrics {
        /// The switch's capsule width (matches the native switch footprint).
        static let switchWidth: CGFloat = 44
        /// The switch's capsule height.
        static let switchHeight: CGFloat = 26
        /// The knob's inset on every side (the track's border must stay
        /// visible around the knob's rest ends).
        static let knobInset: CGFloat = 3
        /// The knob's diameter (the capsule height minus both insets).
        static let knobDiameter: CGFloat = switchHeight - 2 * knobInset
    }

    var body: some View {
        Button {
            // The tap flips the binding the same way the native toggle
            // did; the label stays in the caller's row.
            isOn.toggle()
        } label: {
            ZStack(alignment: .leading) {
                Capsule()
                    .fill(
                        isOn
                            ? ReferenceAppUXColor.accent(theme)
                            : ReferenceAppUXColor.surface(theme)
                    )
                    .overlay(
                        Capsule()
                            .strokeBorder(ReferenceAppUXColor.surfaceBorder(theme), lineWidth: 1)
                    )
                Capsule()
                    .fill(ReferenceAppUXColor.textPrimary(theme))
                    .frame(
                        width: SwitchMetrics.knobDiameter,
                        height: SwitchMetrics.knobDiameter
                    )
                    .offset(
                        x: isOn
                            ? SwitchMetrics.switchWidth
                                - SwitchMetrics.knobDiameter
                                - SwitchMetrics.knobInset
                            : SwitchMetrics.knobInset
                    )
            }
            .frame(
                width: SwitchMetrics.switchWidth,
                height: SwitchMetrics.switchHeight
            )
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}

/// The caption text of a group (type-caption, text-secondary).
struct ReferenceCaptionText: View {
    let text: String

    @Environment(\.appTheme) private var theme

    var body: some View {
        Text(text)
            .font(ReferenceAppUXFont.caption())
            .foregroundStyle(ReferenceAppUXColor.textSecondary(theme))
            .fixedSize(horizontal: false, vertical: true)
    }
}