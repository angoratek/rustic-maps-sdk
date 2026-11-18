//! The provider console's SwiftUI surface (docs/Browser.md):
//! the floating control on the map with its unread Warning/Error dot, the
//! full-height sheet over the scrubbed stream, the level filter, the detail
//! view over the typed `ProviderRequestTrace` and the live cache readbacks,
//! and the export through the platform share sheet. The console is read-only
//! over the SDK's own stream and never issues a provider call.

import MapNativeCoreUI
import RusticMapsSDK
import SwiftUI
import UIKit

/// The console control's platform-glue metrics (visual decisions no shared
/// token governs; all named).
enum ProviderConsoleMetrics {
    /// The unread dot's diameter in points.
    static let unreadDotDiameter: CGFloat = 9
    /// The unread dot's inset from the control's top-trailing corner.
    static let unreadDotInset: CGFloat = 4
    /// The console glyph's SF Symbol name.
    static let consoleGlyph = "terminal"
    /// The glyph size in points (the map cluster's body glyph).
    static let consoleGlyphPixels: CGFloat = 18
}

/// The floating console control (docs/Browser.md): a small
/// terminal-glyph button above the layers control, carrying a Warning/Error
/// dot while a new one arrived since the console last opened.
struct ProviderConsoleControl: View {
    @ObservedObject var applicationModel: ReferenceApplicationModel

    /// The feed whose unread counts drive the dot (observed directly: the
    /// model's own object change never covers the feed's published counts).
    @ObservedObject var providerConsoleFeed: ProviderConsoleFeed

    @Environment(\.appTheme) private var theme

    var body: some View {
        ZStack(alignment: .topTrailing) {
            Button {
                applicationModel.openProviderConsole()
            } label: {
                Image(systemName: ProviderConsoleMetrics.consoleGlyph)
                    .font(
                        .system(
                            size: ProviderConsoleMetrics.consoleGlyphPixels
                        )
                    )
                    .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
                    .frame(
                        width: AppLayout.mapControlDiameter,
                        height: AppLayout.mapControlDiameter
                    )
                    .background(
                        Circle().fill(ReferenceAppUXColor.surface(theme))
                    )
                    .overlay(
                        Circle().strokeBorder(
                            ReferenceAppUXColor.surfaceBorder(theme),
                            lineWidth: 1
                        )
                    )
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .accessibilityIdentifier("provider-console-control")
            .accessibilityLabel(
                ReferenceApplicationConfiguration
                    .providerConsoleOpenAccessibilityLabel
            )
            .accessibilityValue(
                providerConsoleFeed.hasUnreadWarningOrError
                    ? ReferenceApplicationConfiguration
                        .providerConsoleUnreadAccessibilityValue
                    : ""
            )
            // The dot is the control's sibling, not part of the button's
            // label, so its own accessibility element stays queryable while
            // it never intercepts the button's tap.
            if providerConsoleFeed.hasUnreadWarningOrError {
                Circle()
                    .fill(
                        providerConsoleFeed.unreadDotCarriesError
                            ? ReferenceAppUXColor.danger(theme)
                            : ReferenceAppUXColor.warning(theme)
                    )
                    .frame(
                        width: ProviderConsoleMetrics.unreadDotDiameter,
                        height: ProviderConsoleMetrics.unreadDotDiameter
                    )
                    .padding(ProviderConsoleMetrics.unreadDotInset)
                    .allowsHitTesting(false)
                    .accessibilityElement(children: .ignore)
                    .accessibilityIdentifier("provider-console-unread-dot")
                    .accessibilityLabel(
                        ReferenceApplicationConfiguration
                            .providerConsoleUnreadAccessibilityValue
                    )
            }
        }
    }
}

/// The console sheet: the level filter, the excerpt capture switch, the
/// scrubbed rows newest-first and the export action. The rows push a detail
/// view carrying the typed trace fields and the live cache readbacks.
struct ProviderConsoleSheet: View {
    @ObservedObject var applicationModel: ReferenceApplicationModel
    @ObservedObject var providerConsoleFeed: ProviderConsoleFeed

    @Environment(\.appTheme) private var theme
    @State private var isShareSheetPresented = false

    /// The rows the current level filter shows.
    private var visibleEvents: [LogEvent] {
        ProviderConsoleFormatting.visibleEvents(
            providerConsoleFeed.events,
            filter: applicationModel.providerConsoleLevelFilter
        )
    }

    var body: some View {
        NavigationStack {
            VStack(alignment: .leading, spacing: AppLayout.stackGroup) {
                header
                levelFilter
                captureSwitch
                if visibleEvents.isEmpty {
                    emptyState
                } else {
                    eventList
                }
            }
            .padding(AppLayout.panelPadding)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
            .background(ReferenceAppUXColor.surface(theme))
            .toolbar(.hidden, for: .navigationBar)
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("provider-console")
        }
        .presentationDetents([.large])
        .presentationDragIndicator(.visible)
        .sheet(isPresented: $isShareSheetPresented) {
            ProviderConsoleShareSheet(
                activityItems: [exportText]
            )
        }
    }

    /// The custom token-styled header (title, export, close).
    private var header: some View {
        HStack(spacing: AppLayout.stackTight) {
            Text(ReferenceApplicationConfiguration.providerConsoleTitleText)
                .font(ReferenceAppUXFont.title())
                .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
            Spacer(minLength: AppLayout.stackTight)
            Button(
                ReferenceApplicationConfiguration
                    .providerConsoleExportActionText
            ) {
                isShareSheetPresented = true
            }
            .font(ReferenceAppUXFont.bodyStrong())
            .foregroundStyle(ReferenceAppUXColor.accent(theme))
            .accessibilityIdentifier("provider-console-export")
            Button(ReferenceApplicationConfiguration.diagnosticsCloseLabel) {
                applicationModel.closeProviderConsole()
            }
            .font(ReferenceAppUXFont.bodyStrong())
            .foregroundStyle(ReferenceAppUXColor.accent(theme))
            .accessibilityIdentifier("provider-console-close")
        }
    }

    /// The Error / Warning / Info level filter.
    private var levelFilter: some View {
        Picker(
            ReferenceApplicationConfiguration
                .providerConsoleLevelFilterAccessibilityLabel,
            selection: Binding(
                get: { applicationModel.providerConsoleLevelFilter },
                set: { applicationModel.setProviderConsoleLevelFilter($0) }
            )
        ) {
            ForEach(ProviderConsoleLevelFilter.allCases) { levelFilter in
                Text(levelFilter.label).tag(levelFilter)
            }
        }
        .pickerStyle(.segmented)
        .accessibilityIdentifier("provider-console-level-filter")
    }

    /// The response-excerpt capture switch: excerpts are opt-in per session
    /// and never uploaded.
    private var captureSwitch: some View {
        VStack(alignment: .leading, spacing: AppLayout.stackTight) {
            HStack(spacing: AppLayout.stackTight) {
                Text(
                    ReferenceApplicationConfiguration
                        .providerConsoleCaptureLabelText
                )
                .font(ReferenceAppUXFont.body())
                .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
                Spacer(minLength: AppLayout.stackTight)
                ReferenceSwitchControl(
                    isOn: Binding(
                        get: {
                            applicationModel
                                .providerConsoleCapturesResponseExcerpts
                        },
                        set: {
                            applicationModel
                                .setProviderConsoleCapturesResponseExcerpts($0)
                        }
                    )
                )
            }
            if applicationModel.providerConsoleCapturesResponseExcerpts {
                ReferenceCaptionText(
                    text: ReferenceApplicationConfiguration
                        .providerConsoleCaptureNoteText
                )
            }
        }
        .accessibilityIdentifier("provider-console-capture")
    }

    /// The honest empty state, naming the actions that issue a request.
    private var emptyState: some View {
        Text(ProviderConsoleFormatting.emptyStateText)
            .font(ReferenceAppUXFont.numericReadout())
            .foregroundStyle(ReferenceAppUXColor.textSecondary(theme))
            .fixedSize(horizontal: false, vertical: true)
            .accessibilityIdentifier("provider-console-empty")
    }

    /// The scrubbed rows, newest first, each pushing the detail view.
    private var eventList: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: AppLayout.stackTight) {
                ForEach(visibleEvents, id: \.self) { event in
                    NavigationLink {
                        ProviderConsoleDetailView(
                            applicationModel: applicationModel,
                            layersState: applicationModel.mapLayersState,
                            event: event
                        )
                    } label: {
                        Text(
                            ProviderConsoleFormatting.rowText(
                                for: event,
                                descriptors:
                                    applicationModel.providerDescriptors
                            )
                        )
                        .font(ReferenceAppUXFont.numericReadout())
                        .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
                        .lineLimit(3)
                        .fixedSize(horizontal: false, vertical: true)
                        .truncationMode(.tail)
                        .frame(maxWidth: .infinity, alignment: .leading)
                    }
                    .buttonStyle(.plain)
                    .accessibilityIdentifier("provider-console-row")
                }
            }
        }
        .accessibilityIdentifier("provider-console-rows")
    }

    /// The credential-free text the share sheet receives.
    private var exportText: String {
        ProviderConsoleFormatting.exportText(
            for: visibleEvents,
            descriptors: applicationModel.providerDescriptors,
            filter: applicationModel.providerConsoleLevelFilter
        )
    }
}

/// The trace detail: the typed request fields and the live tile-cache and
/// traffic-cache readbacks.
struct ProviderConsoleDetailView: View {
    @ObservedObject var applicationModel: ReferenceApplicationModel
    @ObservedObject var layersState: MapLayersPanelState
    let event: LogEvent

    @Environment(\.appTheme) private var theme

    private var sections: [ProviderConsoleDetailSection] {
        ProviderConsoleFormatting.detailSections(
            for: event,
            descriptors: applicationModel.providerDescriptors,
            tileCacheState: applicationModel.offlineTileCacheState,
            tileCacheUncachedNote: applicationModel.offlineTileCacheUncachedNote,
            tileStatistics: applicationModel.lastFrameTileStatistics,
            tileCacheStatistics: layersState.tileCacheStatistics,
            trafficCacheStatistics: layersState.trafficCacheStatistics
        )
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: AppLayout.stackGroup) {
                ForEach(sections) { section in
                    VStack(alignment: .leading, spacing: AppLayout.stackTight) {
                        Text(section.title)
                            .font(ReferenceAppUXFont.bodyStrong())
                            .foregroundStyle(
                                ReferenceAppUXColor.textSecondary(theme)
                            )
                        ForEach(section.rows) { row in
                            VStack(alignment: .leading, spacing: 2) {
                                Text(row.label)
                                    .font(ReferenceAppUXFont.caption())
                                    .foregroundStyle(
                                        ReferenceAppUXColor.textSecondary(theme)
                                    )
                                Text(row.value)
                                    .font(ReferenceAppUXFont.numericReadout())
                                    .foregroundStyle(
                                        ReferenceAppUXColor.textPrimary(theme)
                                    )
                                    .fixedSize(
                                        horizontal: false,
                                        vertical: true
                                    )
                                    .textSelection(.enabled)
                            }
                            .frame(maxWidth: .infinity, alignment: .leading)
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            .padding(AppLayout.panelPadding)
        }
        .background(ReferenceAppUXColor.surface(theme))
        .navigationTitle(
            ReferenceApplicationConfiguration.providerConsoleDetailTitleText
        )
        .navigationBarTitleDisplayMode(.inline)
        .toolbar(.visible, for: .navigationBar)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("provider-console-detail")
    }
}

/// The platform share sheet the export action opens.
private struct ProviderConsoleShareSheet: UIViewControllerRepresentable {
    let activityItems: [Any]

    func makeUIViewController(
        context: Context
    ) -> UIActivityViewController {
        let activityViewController = UIActivityViewController(
            activityItems: activityItems,
            applicationActivities: nil
        )
        if let popoverPresentationController =
            activityViewController.popoverPresentationController
        {
            popoverPresentationController.sourceView = activityViewController.view
            popoverPresentationController.sourceRect = CGRect(
                x: activityViewController.view.bounds.midX,
                y: activityViewController.view.bounds.midY,
                width: 0,
                height: 0
            )
            popoverPresentationController.permittedArrowDirections = []
        }
        return activityViewController
    }

    func updateUIViewController(
        _ uiViewController: UIActivityViewController,
        context: Context
    ) {}
}
