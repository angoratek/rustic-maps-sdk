//! The error strip and the attribution notice (docs/ReferenceAppUX.md §4.2,
//! §6.3). The v2 layout's readiness signal is the chip (§4.2,
//! `ReadinessChipView`) and the §6.1 readout lines live in the diagnostics
//! sheet (`ReadinessAndDiagnosticsViews.swift`); this file keeps the two
//! docks shared by the sheet's and the drawer's states.

import SwiftUI
import RusticMapsSDK
import MapNativeCoreUI

/// The provider-echoing error strip (§6.3): a danger-bordered strip that
/// docks directly above the sheet's/drawer's grabber while a failure is
/// live; its label line names the failure and its body echoes the
/// provider's detail verbatim — never truncated (the wrap is the strip's
/// own).
struct ErrorReportView: View {
    let errorReport: ReferenceApplicationErrorReport

    @Environment(\.appTheme) private var theme

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(errorReport.labelLine)
                .font(ReferenceAppUXFont.bodyStrong())
                .foregroundStyle(ReferenceAppUXColor.danger(theme))
            Text(errorReport.detailText)
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
                .strokeBorder(ReferenceAppUXColor.danger(theme), lineWidth: 1)
        )
        .accessibilityIdentifier("error-report")
    }
}

/// The attribution info control (§1/§4.2, provider attribution): a compact
/// circular "ⓘ" at the map's bottom-end. Tapping it
/// opens the attribution panel (a popover, so a tap outside or Escape closes
/// it); the always-visible line it replaces no longer claims map space.
struct AttributionInfoControlView: View {
    @ObservedObject var applicationModel: ReferenceApplicationModel

    @Environment(\.appTheme) private var theme
    @State private var isPanelOpen = false

    /// The control's circular size (the shared 40-point control diameter).
    private let controlSize: CGFloat = AppLayout.mapControlDiameter

    var body: some View {
        if !applicationModel.attributionNoticeText.isEmpty {
            Button {
                isPanelOpen.toggle()
            } label: {
                Image(systemName: "info.circle")
                    .font(.system(size: MapControlClusterMetrics.controlGlyphCaptionPixels))
                    .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
                    .frame(width: controlSize, height: controlSize)
                    .background(
                        Circle().fill(
                            isPanelOpen
                                ? ReferenceAppUXColor.accent(theme)
                                : ReferenceAppUXColor.surface(theme)
                        )
                    )
                    .overlay(
                        Circle().strokeBorder(
                            isPanelOpen
                                ? ReferenceAppUXColor.accent(theme)
                                : ReferenceAppUXColor.surfaceBorder(theme),
                            lineWidth: 1
                        )
                    )
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .accessibilityIdentifier("attribution-info-control")
            .accessibilityLabel("Map attribution and terms")
            .popover(
                isPresented: $isPanelOpen,
                attachmentAnchor: .point(.topTrailing),
                arrowEdge: .bottom
            ) {
                AttributionPanelView(
                    attributionText: applicationModel.attributionNoticeText,
                    termsNotes: applicationModel.attributionTermsNotes
                )
                .presentationCompactAdaptation(.popover)
            }
        }
    }
}

/// The attribution panel: the shared provider registry's attribution lines
/// and terms notes (deduplicated, availability-filtered by the model), in
/// the token-styled surface. The text is selectable so compliance text stays
/// readable and copyable.
struct AttributionPanelView: View {
    /// The attribution line(s) the registry descriptors carry.
    let attributionText: String
    /// The descriptors' terms notes.
    let termsNotes: [String]

    @Environment(\.appTheme) private var theme

    /// The panel's readable maximum width in points (compact popover).
    private let panelMaximumWidth: CGFloat = 280

    var body: some View {
        VStack(alignment: .leading, spacing: AppLayout.stackTight) {
            Text("Map attribution and terms")
                .font(ReferenceAppUXFont.bodyStrong())
                .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
            Text(attributionText)
                .font(ReferenceAppUXFont.caption())
                .foregroundStyle(ReferenceAppUXColor.textSecondary(theme))
            ForEach(termsNotes, id: \.self) { termsNote in
                Text(termsNote)
                    .font(ReferenceAppUXFont.caption())
                    .foregroundStyle(ReferenceAppUXColor.textSecondary(theme))
            }
        }
        .padding(AppLayout.panelPadding)
        .frame(maxWidth: panelMaximumWidth, alignment: .leading)
        .background(
            RoundedRectangle(cornerRadius: AppLayout.radiusSurface)
                .fill(ReferenceAppUXColor.surface(theme))
        )
        .overlay(
            RoundedRectangle(cornerRadius: AppLayout.radiusSurface)
                .strokeBorder(ReferenceAppUXColor.surfaceBorder(theme), lineWidth: 1)
        )
        .textSelection(.enabled)
        .accessibilityIdentifier("attribution-panel")
    }
}