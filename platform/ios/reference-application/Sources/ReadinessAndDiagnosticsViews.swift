//! The readiness chip (docs/ReferenceAppUX.md §4.2, §6.1) and the
//! diagnostics sheet (§4.1, §6): the chip docks top-start and opens the
//! diagnostics on tap; the sheet is a dismissible half-height scroll
//! carrying the renderer detail, the camera's attribute-getter readback,
//! the per-draw tile statistics, the compiled version, the demo's frame
//! facts and the recorded journey's provenance — the one place where the
//! renderer/SDK jargon is required verbatim (§1's product-language rule).

import SwiftUI
import RusticMapsSDK
import MapNativeCoreUI

/// The readiness chip (§6.1's four states, one short line each; tappable —
/// it opens the diagnostics sheet).
struct ReadinessChipView: View {
    @ObservedObject var applicationModel: ReferenceApplicationModel

    @Environment(\.appTheme) private var theme

    private var readinessColor: Color {
        switch applicationModel.readinessState {
        case .loading: return ReferenceAppUXColor.textSecondary(theme)
        case .ready: return ReferenceAppUXColor.positive(theme)
        case .degraded: return ReferenceAppUXColor.warning(theme)
        case .keyless: return ReferenceAppUXColor.textSecondary(theme)
        case .failed: return ReferenceAppUXColor.danger(theme)
        }
    }

    var body: some View {
        Button(action: {
            applicationModel.openDiagnosticsSheet()
        }) {
            HStack(spacing: AppLayout.stackTight) {
                Circle()
                    .fill(readinessColor)
                    .frame(width: 8, height: 8)
                Text(applicationModel.readinessChipText)
                    .font(ReferenceAppUXFont.body())
                    .foregroundStyle(ReadinessChipTextColor(theme: theme))
                    .lineLimit(1)
                    .truncationMode(.tail)
            }
            .padding(.vertical, AppLayout.stackTight)
            .padding(.horizontal, AppLayout.stackGroup)
            .background(
                RoundedRectangle(cornerRadius: AppLayout.radiusInline)
                    .fill(ReferenceAppUXColor.surface(theme))
            )
            .overlay(
                RoundedRectangle(cornerRadius: AppLayout.radiusInline)
                    .strokeBorder(ReferenceAppUXColor.surfaceBorder(theme), lineWidth: 1)
            )
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier("readiness-chip")
    }

    /// The chip's text color: the ladder state's color worded per §6.1
    /// (the text itself stays in the theme's primary, the dot carries the
    /// state).
    private func ReadinessChipTextColor(theme: AppTheme) -> Color {
        ReferenceAppUXColor.textPrimary(theme)
    }
}

/// The diagnostics sheet (§4.1: a dismissible half-height scroll): the
/// §6.1 readout's lines — renderer mode and adapter detail (when any),
/// the camera's attribute readback, the per-draw tile statistics, the
/// compiled version probe, and the demo's frame facts — plus the demo
/// content's provenance (§1: fixture jargon lives in the diagnostics
/// detail).
struct DiagnosticsSheetView: View {
    @ObservedObject var applicationModel: ReferenceApplicationModel

    /// The viewport height the half-height scroll resolves against.
    let viewportSize: CGSize

    @Environment(\.appTheme) private var theme

    var body: some View {
        VStack(alignment: .leading, spacing: AppLayout.stackTight) {
            HStack {
                Text(ReferenceApplicationConfiguration.diagnosticsTitleText)
                    .font(ReferenceAppUXFont.title())
                    .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
                Spacer()
                Button(ReferenceApplicationConfiguration.diagnosticsCloseLabel) {
                    applicationModel.isDiagnosticsSheetOpen = false
                }
                .font(ReferenceAppUXFont.bodyStrong())
                .foregroundStyle(ReferenceAppUXColor.accent(theme))
                .accessibilityIdentifier("diagnostics-close")
            }
            ScrollView {
                VStack(alignment: .leading, spacing: AppLayout.stackTight) {
                    // The events readout leads the scroll: the sheet is a
                    // fixed half-height fold, so the events readout sits
                    // in the fold and the §6.1 blocks keep their order
                    // under it.
                    diagnosticsEventsBlock
                    diagnosticsBlock("Renderer", rendererRows)
                    diagnosticsBlock("Camera", cameraRows)
                    // The confirmed-tap count beside the camera readback:
                    // the diagnostics sheet's honest witness that a double
                    // tap zoomed without also reporting a click (§5.2).
                    Text("map taps \(applicationModel.mapClickCount)")
                        .font(ReferenceAppUXFont.numericReadout())
                        .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
                        .accessibilityIdentifier("diagnostics-map-tap-count")
                    diagnosticsBlock("Tiles", tileRows)
                    diagnosticsBlock(
                        ReferenceApplicationConfiguration.trafficCacheDiagnosticsBlockTitle,
                        trafficCacheRows
                    )
                    directionsDiagnosticsBlock
                    diagnosticsBlock("Demo", demoRows)
                }
            }
        }
        .padding(AppLayout.panelPadding)
        .frame(
            height: viewportSize.height * AppLayout.sheetHalfHeightFraction,
            alignment: .top
        )
        .background(
            RoundedRectangle(cornerRadius: AppLayout.radiusSurface)
                .fill(ReferenceAppUXColor.surface(theme))
                .shadow(
                    color: AppLayout.elevationSurfaceColor,
                    radius: AppLayout.elevationSurfaceRadius,
                    x: 0,
                    y: -AppLayout.elevationSurfaceOffsetY
                )
        )
        .overlay(
            RoundedRectangle(cornerRadius: AppLayout.radiusSurface)
                .strokeBorder(ReferenceAppUXColor.surfaceBorder(theme), lineWidth: 1)
        )
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("diagnostics-sheet")
    }

    private func diagnosticsBlock(
        _ blockTitle: String,
        _ blockRows: [String]
    ) -> some View {
        VStack(alignment: .leading, spacing: AppLayout.stackTight) {
            Text(blockTitle)
                .font(ReferenceAppUXFont.bodyStrong())
                .foregroundStyle(ReferenceAppUXColor.textSecondary(theme))
            ForEach(blockRows, id: \.self) { rowText in
                Text(rowText)
                    .font(ReferenceAppUXFont.numericReadout())
                    .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
                    // Long rows (the adapter-detail honesty line, the
                    // cache path) wrap to two lines before they truncate.
                    .lineLimit(2)
                    .fixedSize(horizontal: false, vertical: true)
                    .truncationMode(.tail)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    /// The renderer block: the negotiated mode, the adapter detail (when
    /// any — the contract does not expose one yet, stated honestly) and
    /// the compiled version probe (§6.1).
    private var rendererRows: [String] {
        [
            "mode \(rendererModeText)",
            ReferenceApplicationConfiguration.rendererAdapterRowText,
            "compiled version \(getSdkCompiledVersion())",
        ]
    }

    private var rendererModeText: String {
        applicationModel.isSurfaceAttached
            ? ReferenceApplicationConfiguration.rendererModeReadout
            : ReferenceApplicationConfiguration.rendererModeUnpreparedReadout
    }

    /// The camera block: the last drawn frame's camera read back through
    /// the attribute getters (§4.1: "no 'no zoom readback'
    /// honesty note is needed on native").
    private var cameraRows: [String] {
        guard let readback = applicationModel.cameraAttributeReadback else {
            return [ReferenceApplicationConfiguration.cameraReadoutUnpreparedText]
        }
        return [
            "center \(readback.centerLatitudeDegrees.decimalReadout(6)), \(readback.centerLongitudeDegrees.decimalReadout(6))",
            "zoom \(readback.zoomLevel.decimalReadout(2))",
            "bearing \(readback.bearingDegrees.decimalOrPlainDegrees)°",
            "pitch \(readback.pitchDegrees.decimalOrPlainDegrees)°",
        ]
    }

    /// The tiles block: the last frame's per-draw statistics (§6.1's
    /// requested/drawn/missing/fragments readout through
    /// `FrameStatistics`).
    private var tileRows: [String] {
        guard let statistics = applicationModel.lastFrameTileStatistics else {
            return [ReferenceApplicationConfiguration.tileStatisticsUnpreparedText]
        }
        return [
            "requested \(statistics.requestedTileCount)",
            "drawn \(statistics.presentTileCount)",
            "parent fallback \(statistics.fallbackTileCount)",
            "deferred \(statistics.deferredTileCount)",
            "unavailable \(statistics.unavailableTileCount)",
            "overlay triangles \(statistics.overlayTriangleCount)",
        ]
    }

    /// The traffic cache block: the serving cache's counters and budget
    /// (the contract readback), so a diagnostics pass can witness that a
    /// repeated pane was answered from memory rather than fetched again.
    private var trafficCacheRows: [String] {
        guard let statistics = applicationModel.mapLayersState.trafficCacheStatistics else {
            return [ReferenceApplicationConfiguration.trafficCacheUnpreparedText]
        }
        return [
            "hits \(statistics.hitCount)",
            "misses \(statistics.missCount)",
            "cells \(statistics.entryCount) of \(statistics.maximumEntryCount)",
            "evictions \(statistics.evictionCount)",
        ]
    }

    /// The Directions block: the selected mode and strategy and the options
    /// the last plan action carried, so an automated pass can witness the
    /// mode a plan used without reading the transient record.
    private var directionsDiagnosticsBlock: some View {
        VStack(alignment: .leading, spacing: AppLayout.stackTight) {
            Text(ReferenceApplicationConfiguration.directionsDiagnosticsBlockTitle)
                .font(ReferenceAppUXFont.bodyStrong())
                .foregroundStyle(ReferenceAppUXColor.textSecondary(theme))
            Text(applicationModel.directionsDiagnosticsText)
                .font(ReferenceAppUXFont.numericReadout())
                .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
                .lineLimit(2)
                .fixedSize(horizontal: false, vertical: true)
                .truncationMode(.tail)
                .accessibilityIdentifier("diagnostics-route-options")
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    /// The demo block: the surface's frame facts and the demo content's
    /// provenance (fixture jargon lives only here, §1).
    private var demoRows: [String] {
        var demoRows: [String] = []
        if let viewportReadout = applicationModel.configuredViewportReadoutText {
            demoRows.append(viewportReadout)
        }
        if let navigationDemo = applicationModel.catalog?.navigationDemo {
            demoRows.append(
                "sample journey: \(navigationDemo.simulatedTrace.name) — \(navigationDemo.simulatedTrace.provenance)"
            )
        }
        if let spokenText = applicationModel.speechEngine.lastSpokenText {
            demoRows.append("voice: spoken \(spokenText.escapedDiagnosticsLine)")
        }
        return demoRows
    }

    /// The events block: the sheet's recent-events readout — the
    /// ring's flushed backlog (oldest first, delivered at open) and the
    /// live subscription's events after it, one scrubbed line each
    /// (`timestamp · severity · subsystem · message`). The block renders
    /// from the buffer's own observing child: the deliveries publish on
    /// the buffer object, which the sheet's `applicationModel` observation
    /// would never see.
    @ViewBuilder
    private var diagnosticsEventsBlock: some View {
        DiagnosticsEventsBlockView(
            applicationModel: applicationModel,
            diagnosticsEventsBuffer: applicationModel.diagnosticsEventsBuffer
        )
    }
}

/// The observing child view of the diagnostics events buffer: the
/// sheet's half-height fold opens on this block, so the events readout
/// leads the §6.1 blocks' order in the sheet's scroll. The pane's
/// one bounded stream starts at Info; the explicit Show debug events switch
/// lowers the subscription to Debug for the session, and Clear empties the
/// display list.
struct DiagnosticsEventsBlockView: View {
    @ObservedObject var applicationModel: ReferenceApplicationModel
    @ObservedObject var diagnosticsEventsBuffer: SdkDiagnosticEventsBuffer

    @Environment(\.appTheme) private var theme

    /// The events the pane shows under the current severity: Info until the
    /// visitor asks for debug detail, Debug afterwards.
    private var visibleEvents: [LogEvent] {
        let minimumSeverity = applicationModel.diagnosticsVisibleMinimumSeverity
        return diagnosticsEventsBuffer.events.filter { diagnosticEvent in
            ReferenceApplicationConfiguration.diagnosticsSeverityMeets(
                diagnosticEvent.severity,
                minimum: minimumSeverity
            )
        }
    }

    var body: some View {
        let bufferedEvents = visibleEvents
        VStack(alignment: .leading, spacing: AppLayout.stackTight) {
            HStack(spacing: AppLayout.stackTight) {
                Text("Events")
                    .font(ReferenceAppUXFont.bodyStrong())
                    .foregroundStyle(ReferenceAppUXColor.textSecondary(theme))
                Spacer(minLength: AppLayout.stackTight)
                ReferenceSubtleButton(
                    label: ReferenceApplicationConfiguration
                        .diagnosticsClearActionText
                ) {
                    applicationModel.clearDiagnosticsEvents()
                }
                .accessibilityIdentifier("diagnostics-clear")
            }
            HStack(spacing: AppLayout.stackTight) {
                Text(
                    applicationModel.diagnosticsShowsDebugEvents
                        ? ReferenceApplicationConfiguration
                            .diagnosticsHideDebugEventsText
                        : ReferenceApplicationConfiguration
                            .diagnosticsShowDebugEventsText
                )
                .font(ReferenceAppUXFont.body())
                .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
                Spacer(minLength: AppLayout.stackTight)
                ReferenceSwitchControl(
                    isOn: Binding(
                        get: {
                            applicationModel.diagnosticsShowsDebugEvents
                        },
                        set: {
                            applicationModel.setDiagnosticsShowsDebugEvents($0)
                        }
                    )
                )
            }
            .accessibilityIdentifier("diagnostics-debug-toggle")
            if applicationModel.diagnosticsShowsDebugEvents {
                ReferenceCaptionText(
                    text: ReferenceApplicationConfiguration
                        .diagnosticsStreamScopeNoteText
                )
                .accessibilityIdentifier("diagnostics-debug-scope-note")
            }
            if bufferedEvents.isEmpty {
                Text(ReferenceApplicationConfiguration.diagnosticsNoEventsText)
                    .font(ReferenceAppUXFont.numericReadout())
                    .foregroundStyle(ReferenceAppUXColor.textSecondary(theme))
                    .accessibilityIdentifier("diagnostics-no-events")
            } else {
                ForEach(bufferedEvents, id: \.self) { diagnosticEvent in
                    Text(diagnosticsEventRowText(diagnosticEvent))
                        .font(ReferenceAppUXFont.numericReadout())
                        .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
                        .lineLimit(2)
                        .fixedSize(horizontal: false, vertical: true)
                        .truncationMode(.tail)
                }
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    /// One event row's text: the local wall-clock time, the severity
    /// label, the named subsystem and the already-scrubbed message line.
    private func diagnosticsEventRowText(_ diagnosticEvent: LogEvent) -> String {
        let eventTimeText = ReferenceApplicationConfiguration
            .diagnosticsEventTimeFormatter
            .string(from: Date(
                timeIntervalSince1970:
                    Double(diagnosticEvent.timestampMillisecondsSinceEpoch) / 1000
            ))
        return eventTimeText
            + " · "
            + ReferenceApplicationConfiguration.diagnosticsSeverityLabel(
                diagnosticEvent.severity
            )
            + " · "
            + diagnosticEvent.subsystemTarget
            + " · "
            + diagnosticEvent.messageLine
    }
}

extension String {
    /// The diagnostics-safe rendering of a payload-provided line: quotes
    /// stay readable, newlines collapse (the echo stays verbatim in
    /// wording).
    var escapedDiagnosticsLine: String {
        "“\(replacingOccurrences(of: "\n", with: " "))”"
    }
}

extension Double {
    /// The degrees' readout helper (the readout's one-decimal degrees).
    var decimalOrPlainDegrees: String {
        decimalReadout(1)
    }
}