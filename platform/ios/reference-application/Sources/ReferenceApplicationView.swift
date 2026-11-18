//! The root layout — the v2 archetype (docs/ReferenceAppUX.md §1): the map
//! fills the viewport and is the screen's background layer; above it, the
//! readiness chip docks top-start, the §7 banner stack (turn banner, then
//! line, deviation alert) at the map's top-center during a simulated trip,
//! the attribution notice and the error strip above the sheet's grabber,
//! and the bottom dock: the home sheet (peek/half/full) — replaced in
//! place by the navigation drawer during a simulated trip. Below the
//! phone-width breakpoint the sheet is full-bleed, the banner spans the
//! full width, and nothing scrolls horizontally (the sheet scrolls inside
//! itself).

import SwiftUI
import RusticMapsSDK
import MapNativeCoreUI

/// The map control row's measured height, so the bottom dock can keep the
/// map controls, the attribution and the sheet on-screen together.
private struct MapControlRowHeightPreferenceKey: PreferenceKey {
    static var defaultValue: CGFloat = 0

    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) {
        value = max(value, nextValue())
    }
}

/// The reference application's root view.
struct ReferenceApplicationView: View {
    @ObservedObject var applicationModel: ReferenceApplicationModel

    /// The measured map control row height.
    @State private var mapControlRowHeight: CGFloat = 0

    /// The operating system's color scheme; while the theme choice is Auto
    /// the resolved theme follows it live (§3).
    @Environment(\.colorScheme) private var systemColorScheme

    /// The theme the token slots render against: the explicit choice for Light
    /// and Dark, the operating system's scheme while Auto (§3).
    private var resolvedTheme: AppTheme {
        applicationModel.themeChoice.resolvedTheme(systemColorScheme: systemColorScheme)
    }

    var body: some View {
        GeometryReader { viewportProxy in
            ZStack {
                mapCanvas
                referenceLayout(viewportSize: viewportProxy.size)
                diagnosticsOverlay(viewportSize: viewportProxy.size)
            }
            .environment(\.appTheme, resolvedTheme)
            // The native controls (text fields) style themselves against
            // the system color scheme; the design language's theme choice
            // owns that decision (§3, §8) — applied here so the subtree
            // under this level matches the application root.
            .environment(\.colorScheme, resolvedTheme.colorScheme)
            .sheet(isPresented: $applicationModel.isProviderSettingsSheetOpen) {
                ProviderSettingsView(applicationModel: applicationModel)
                    .environment(\.appTheme, resolvedTheme)
                    .environment(
                        \.colorScheme, resolvedTheme.colorScheme
                    )
            }
            .sheet(isPresented: $applicationModel.isMapLayersPanelOpen) {
                MapLayersPanelView(
                    applicationModel: applicationModel,
                    layersState: applicationModel.mapLayersState
                )
                .environment(\.appTheme, resolvedTheme)
                .environment(\.colorScheme, resolvedTheme.colorScheme)
            }
            .sheet(isPresented: $applicationModel.isProviderConsoleOpen) {
                ProviderConsoleSheet(
                    applicationModel: applicationModel,
                    providerConsoleFeed: applicationModel.providerConsoleFeed
                )
                .environment(\.appTheme, resolvedTheme)
                .environment(\.colorScheme, resolvedTheme.colorScheme)
            }
            .task {
                await applicationModel.startApplication()
            }
        }
        .background(Color.black)
    }

    /// The map canvas: the shipped `MapView` fills the viewport (and
    /// ignores the safe area, so the gutter floats are design, not
    /// safe-area accidents). The view owns the surface, the frame pump,
    /// gestures and the camera store; the model supplies the SDK and
    /// receives the delegate callbacks.
    private var mapCanvas: some View {
        SwiftUIMapView(
            sdk: applicationModel.surfaceSdk,
            delegate: applicationModel,
            onViewCreated: { mapView in
                applicationModel.onMapViewReady(mapView)
            }
        )
        .ignoresSafeArea()
    }

    /// The v2 layout: the chip row at the top, the banner stack under it
    /// (the map's top-center dock, §7.2), the map's middle left open, and
    /// the bottom dock (attribution + error strip + the sheet or drawer).
    private func referenceLayout(viewportSize: CGSize) -> some View {
        let isPhoneWidth = viewportSize.width < AppLayout.phoneWidthBreakpointPixels
        return VStack(spacing: 0) {
            HStack(spacing: AppLayout.stackTight) {
                ReadinessChipView(applicationModel: applicationModel)
                ProviderSettingsGearControl(applicationModel: applicationModel)
                Spacer()
                MapCompassControl(applicationModel: applicationModel)
            }
            .padding(.horizontal, AppLayout.gutterInline)
            bannerOverlay(viewportSize: viewportSize, isPhoneWidth: isPhoneWidth)
            Spacer(minLength: 0)
            mapControlRow()
            bottomDock(
                viewportSize: viewportSize,
                isPhoneWidth: isPhoneWidth,
                maximumDockHeight: maximumDockHeight(viewportSize: viewportSize)
            )
        }
        .padding(.top, AppLayout.gutterBlock)
        .onPreferenceChange(MapControlRowHeightPreferenceKey.self) { measuredHeight in
            mapControlRowHeight = measuredHeight
        }
    }

    /// The height the bottom dock may take without pushing itself below the
    /// screen. The top row and the map control row are measured or reserved;
    /// the sheet's own maximum-height token remains an upper bound inside it.
    private func maximumDockHeight(viewportSize: CGSize) -> CGFloat {
        let topRowReservation = MapControlClusterMetrics.controlButtonSize
        let reservedHeight = topRowReservation
            + mapControlRowHeight
            + AppLayout.gutterBlock
        return max(0, viewportSize.height - reservedHeight)
    }

    /// The §4.2 map control layer's bottom row: the scale bar docks
    /// bottom-start, the control cluster (now zoom +/−) and the compact
    /// attribution info control dock bottom-end — all above the bottom dock,
    /// so the sheet and the drawer never cover them. The inset is `gutter-inline`
    /// on every width (the full-bleed phone sheet keeps the cluster clear
    /// of the viewport edge by the map's gutter token).
    private func mapControlRow() -> some View {
        HStack(alignment: .bottom) {
            MapScaleBarView(applicationModel: applicationModel)
            Spacer()
            VStack(alignment: .trailing, spacing: AppLayout.stackTight) {
                MapControlClusterView(applicationModel: applicationModel)
                // The provider console's floating control sits directly
                // above the layer control; its unread dot reports the
                // Warning/Error events that arrived while it was closed.
                ProviderConsoleControl(
                    applicationModel: applicationModel,
                    providerConsoleFeed: applicationModel.providerConsoleFeed
                )
                // The layer control sits directly above the attribution
                // notice, as the specification places it on every target.
                MapLayersCollapsedControl(applicationModel: applicationModel)
                AttributionInfoControlView(applicationModel: applicationModel)
            }
        }
        .padding(.horizontal, AppLayout.gutterInline)
        .background(
            GeometryReader { controlRowProxy in
                Color.clear.preference(
                    key: MapControlRowHeightPreferenceKey.self,
                    value: controlRowProxy.size.height
                )
            }
        )
    }

    /// The bottom dock: the error strip (§6.3, directly above the sheet's
    /// or drawer's grabber) and the present dock — the navigation drawer
    /// during a simulated trip, the home sheet otherwise (§1). The
    /// attribution rides the bottom-start map-control row as a compact info
    /// control now.
    @ViewBuilder
    private func bottomDock(
        viewportSize: CGSize,
        isPhoneWidth: Bool,
        maximumDockHeight: CGFloat
    ) -> some View {
        let dockHorizontalInset: CGFloat = isPhoneWidth ? 0 : AppLayout.gutterInline
        VStack(spacing: AppLayout.stackTight) {
            if let errorReport = applicationModel.errorReport {
                ErrorReportView(errorReport: errorReport)
                    .padding(.horizontal, max(dockHorizontalInset, AppLayout.stackTight))
            }
            if applicationModel.isJourneyActive {
                NavigationDrawerView(applicationModel: applicationModel)
            } else {
                HomeSheetView(
                    applicationModel: applicationModel,
                    viewportSize: viewportSize,
                    maximumDockHeight: maximumDockHeight
                )
            }
        }
        .padding(.horizontal, dockHorizontalInset)
    }

    /// The §7 banner stack (v2 placement, §7.2): turn banner, then line,
    /// deviation alert — top-center under the chip row, full-width at
    /// phone width, centered above the map's middle across the viewport's
    /// central region otherwise.
    @ViewBuilder
    private func bannerOverlay(viewportSize: CGSize, isPhoneWidth: Bool) -> some View {
        if navigationBannerStackIsLive {
            let showArrivalState = applicationModel.navigationCardStore.isTripArrived
                || applicationModel.hasSimulatedJourneyArrived
            VStack(spacing: AppLayout.stackTight) {
                NavigationTurnBannerView(
                    cardStore: applicationModel.navigationCardStore,
                    showArrivalState: showArrivalState
                )
                NavigationThenLineView(
                    cardStore: applicationModel.navigationCardStore,
                    showArrivalState: showArrivalState
                )
                NavigationDeviationAlertView(cardStore: applicationModel.navigationCardStore)
            }
            .frame(
                maxWidth: isPhoneWidth ? .infinity : viewportSize.width * bannerMaximumWidthFraction
            )
            .frame(maxWidth: .infinity, alignment: .top)
            .padding(.horizontal, AppLayout.gutterInline)
            .padding(.top, AppLayout.stackGroup)
            .animation(.easeOut(duration: bannerStackAnimationSeconds), value: showArrivalState)
        }
    }

    /// Whether the banner stack has §7 content to render (the cards the
    /// payload topics and the seeded queries filled).
    private var navigationBannerStackIsLive: Bool {
        let cardStore = applicationModel.navigationCardStore
        return applicationModel.isJourneyActive
            || cardStore.instruction != nil
            || cardStore.isTripArrived
    }

    /// The banner stack's arrival-swap animation duration (platform glue,
    /// kept short).
    private let bannerStackAnimationSeconds: TimeInterval = 0.12

    /// The banner spans the viewport's central region at wide widths,
    /// leaving the map's edges unobstructed (§1: the banner is the first
    /// thing the driver must read; full width at phone width).
    private let bannerMaximumWidthFraction: CGFloat = 0.6

    /// The diagnostics sheet's modal overlay (§4.1): a dimmed scrim + the
    /// half-height sheet docked at the bottom; the scrim's tap dismisses
    /// alongside the close control.
    @ViewBuilder
    private func diagnosticsOverlay(viewportSize: CGSize) -> some View {
        if applicationModel.isDiagnosticsSheetOpen {
            ZStack(alignment: .bottom) {
                Color.black.opacity(diagnosticsScrimOpacity)
                    .ignoresSafeArea()
                    .contentShape(Rectangle())
                    .onTapGesture {
                        applicationModel.isDiagnosticsSheetOpen = false
                    }
                VStack(spacing: 0) {
                    DiagnosticsSheetView(
                        applicationModel: applicationModel,
                        viewportSize: viewportSize
                    )
                    .padding(.horizontal, AppLayout.gutterInline)
                    .padding(.bottom, AppLayout.gutterBlock)
                }
            }
            .transition(.opacity)
            .animation(.easeOut(duration: diagnosticsSheetAnimationSeconds), value: applicationModel.isDiagnosticsSheetOpen)
        }
    }

    /// The diagnostics scrim's dimming opacity (platform glue).
    private let diagnosticsScrimOpacity: Double = 0.3

    /// The diagnostics sheet's open/close animation duration (platform
    /// glue).
    private let diagnosticsSheetAnimationSeconds: TimeInterval = 0.16
}