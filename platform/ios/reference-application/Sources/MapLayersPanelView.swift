//! The on-map layer control (docs/ReferenceAppUX.md §4.2): the
//! collapsed round control docked above the attribution notice, and the
//! panel it opens — Base map, Traffic, Overlays and Device location, in that
//! order. The panel renders the shared registry's descriptors; it names no
//! provider and carries no credential.

import SwiftUI
import MapNativeCoreUI
import RusticMapsSDK

/// The layer control's platform-glue metrics (visual decisions no token
/// governs; all named).
enum MapLayersControlMetrics {
    /// The collapsed control's square edge in points (the shared 40-point
    /// control diameter).
    static let controlButtonSize: CGFloat = AppLayout.mapControlDiameter
    /// The collapsed control's SF Symbol glyph.
    static let layersGlyph = "square.3.layers.3d"
    /// The glyph's point size.
    static let glyphBodyPixels: CGFloat = 18
    /// The non-default-layer dot's diameter in points.
    static let activeDotDiameter: CGFloat = 8
    /// The dot's inset from the control's top-trailing edge, in points.
    static let activeDotInset: CGFloat = 6
    /// The panel's height cap as a fraction of the viewport height (the
    /// specification's 70%).
    static let panelMaximumHeightFraction: CGFloat = 0.7
    /// The scroll anchor of the Device location section.
    static let deviceLocationSectionAnchor = "map-layers-device-location"
    /// The dot's accessible value while a non-default layer is active.
    static let activeDotAccessibilityValue = "Non-default layer active"
}

/// The collapsed round control: it opens the layer panel and carries a dot
/// while a non-default layer is active.
struct MapLayersCollapsedControl: View {
    @ObservedObject var applicationModel: ReferenceApplicationModel

    @Environment(\.appTheme) private var theme

    var body: some View {
        Button {
            applicationModel.openMapLayersPanel()
        } label: {
            ZStack(alignment: .topTrailing) {
                Image(systemName: MapLayersControlMetrics.layersGlyph)
                    .font(.system(size: MapLayersControlMetrics.glyphBodyPixels))
                    .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
                    .frame(
                        width: MapLayersControlMetrics.controlButtonSize,
                        height: MapLayersControlMetrics.controlButtonSize
                    )
                    .background(Circle().fill(ReferenceAppUXColor.surface(theme)))
                    .overlay(
                        Circle()
                            .strokeBorder(
                                ReferenceAppUXColor.surfaceBorder(theme),
                                lineWidth: 1
                            )
                    )
                    .contentShape(Circle())
                if isNonDefaultLayerActive {
                    Circle()
                        .fill(ReferenceAppUXColor.accent(theme))
                        .frame(
                            width: MapLayersControlMetrics.activeDotDiameter,
                            height: MapLayersControlMetrics.activeDotDiameter
                        )
                        .padding(MapLayersControlMetrics.activeDotInset)
                        .accessibilityHidden(true)
                }
            }
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier("map-layers-control")
        .accessibilityLabel("Map layers")
        .accessibilityValue(
            isNonDefaultLayerActive
                ? MapLayersControlMetrics.activeDotAccessibilityValue
                : ""
        )
    }

    /// Whether the dot shows: a base other than the default, traffic on, or
    /// any demo scene enabled.
    private var isNonDefaultLayerActive: Bool {
        let layersState = applicationModel.mapLayersState
        if layersState.isTrafficEnabled || !applicationModel.enabledSceneKeys.isEmpty {
            return true
        }
        if let keylessChoice = applicationModel.keylessVectorBaseChoice {
            return applicationModel.activeVectorBase?.choiceKey != keylessChoice.choiceKey
        }
        if applicationModel.activeVectorBase != nil {
            return true
        }
        let defaultRasterProviderKey = applicationModel.baseMapChoices
            .first(where: { choice in choice.isServable })?.providerKey
        return applicationModel.activeBaseMap?.providerKey != defaultRasterProviderKey
    }
}

/// The layer panel: Base map, Traffic, Overlays, Device location.
struct MapLayersPanelView: View {
    @ObservedObject var applicationModel: ReferenceApplicationModel
    @ObservedObject var layersState: MapLayersPanelState

    @Environment(\.appTheme) private var theme

    var body: some View {
        VStack(alignment: .leading, spacing: AppLayout.stackGroup) {
            headerRow
            ScrollViewReader { scrollProxy in
                ScrollView {
                    VStack(alignment: .leading, spacing: AppLayout.stackGroup) {
                        baseMapSection
                        trafficSection
                        overlaysSection
                        deviceLocationSection
                            .id(MapLayersControlMetrics.deviceLocationSectionAnchor)
                        statusLine
                    }
                }
                .onAppear {
                    guard applicationModel.shouldFocusMapLayersDeviceLocation else {
                        return
                    }
                    applicationModel.shouldFocusMapLayersDeviceLocation = false
                    scrollProxy.scrollTo(
                        MapLayersControlMetrics.deviceLocationSectionAnchor,
                        anchor: .bottom
                    )
                }
            }
        }
        .padding(AppLayout.panelPadding)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
        .background(ReferenceAppUXColor.surface(theme).ignoresSafeArea())
        .presentationDetents([
            .fraction(MapLayersControlMetrics.panelMaximumHeightFraction),
        ])
        .presentationDragIndicator(.visible)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("map-layers-panel")
    }

    /// The panel's title and its close control.
    private var headerRow: some View {
        HStack {
            Text("Map layers")
                .font(ReferenceAppUXFont.title())
                .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
            Spacer()
            Button("Close") {
                applicationModel.isMapLayersPanelOpen = false
            }
            .font(ReferenceAppUXFont.bodyStrong())
            .foregroundStyle(ReferenceAppUXColor.accent(theme))
            .accessibilityIdentifier("map-layers-close")
        }
    }

    /// The base-map rows: raster descriptors first, then every committed
    /// vector style. Selecting a row runs the provider-neutral base switch.
    private var baseMapSection: some View {
        ReferenceControlGroup(
            groupLabel: ReferenceApplicationConfiguration.baseMapGroupLabel,
            disabledReason: nil
        ) {
            VStack(alignment: .leading, spacing: AppLayout.stackTight) {
                ForEach(layersState.baseRows) { row in
                    baseMapRow(row)
                }
                // The stated limit travels with the base that has it, so a
                // user never reads the absence of labels as a failure.
                if let labelsNote = applicationModel.vectorBaseLabelsNote {
                    ReferenceCaptionText(text: labelsNote)
                        .foregroundStyle(ReferenceAppUXColor.textSecondary(theme))
                }
                if applicationModel.activeVectorBase != nil,
                   let failedTileCount = applicationModel
                       .vectorBaseStatus?.failedTileCount,
                   failedTileCount > 0 {
                    ReferenceCaptionText(
                        text: ReferenceApplicationConfiguration
                            .vectorBaseFailedStatusText(
                                failedTileCount: failedTileCount
                            )
                    )
                    .foregroundStyle(ReferenceAppUXColor.warning(theme))
                }
            }
            .padding(AppLayout.stackTight)
        }
    }

    /// One base row: the name, its serving mark or missing-credential reason,
    /// and the selection it performs.
    private func baseMapRow(_ row: MapLayerBaseRow) -> some View {
        Button {
            switch row.selection {
            case let .raster(providerKey):
                applicationModel.selectBaseMap(providerKey: providerKey)
            case let .vector(choiceKey):
                applicationModel.selectVectorBase(choiceKey: choiceKey)
            }
        } label: {
            HStack(spacing: AppLayout.stackTight) {
                Text(row.displayName)
                    .font(ReferenceAppUXFont.body())
                    .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
                    .multilineTextAlignment(.leading)
                Spacer(minLength: AppLayout.stackTight)
                Text(baseRowStatusText(row))
                    .font(ReferenceAppUXFont.caption())
                    .foregroundStyle(
                        row.isServing
                            ? ReferenceAppUXColor.accent(theme)
                            : ReferenceAppUXColor.textSecondary(theme)
                    )
            }
            .padding(.horizontal, AppLayout.stackGroup)
            .padding(.vertical, AppLayout.stackTight)
            .background(
                RoundedRectangle(cornerRadius: AppLayout.radiusControl)
                    .fill(ReferenceAppUXColor.surface(theme))
            )
            .overlay(
                RoundedRectangle(cornerRadius: AppLayout.radiusControl)
                    .strokeBorder(
                        row.isServing
                            ? ReferenceAppUXColor.accent(theme)
                            : ReferenceAppUXColor.surfaceBorder(theme),
                        lineWidth: 1
                    )
            )
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .disabled(row.disabledReasonText != nil)
        .opacity(row.disabledReasonText == nil ? 1 : 0.45)
        .accessibilityIdentifier(row.identifier)
        .accessibilityLabel(row.buttonLabel)
    }

    /// The row's trailing status: serving, selectable, or the reason it
    /// cannot serve yet.
    private func baseRowStatusText(_ row: MapLayerBaseRow) -> String {
        if let disabledReasonText = row.disabledReasonText {
            return disabledReasonText
        }
        return row.isServing
            ? ReferenceApplicationConfiguration.baseMapServingStatusText
            : ReferenceApplicationConfiguration.baseMapSelectStatusText
    }

    /// The traffic switch and its availability reason.
    private var trafficSection: some View {
        ReferenceControlGroup(groupLabel: "Traffic", disabledReason: nil) {
            VStack(alignment: .leading, spacing: AppLayout.stackTight) {
                Toggle(
                    isOn: Binding(
                        get: { layersState.isTrafficEnabled },
                        set: { applicationModel.setTrafficLayerEnabled($0) }
                    )
                ) {
                    Text("Traffic")
                        .font(ReferenceAppUXFont.body())
                        .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
                }
                .toggleStyle(.switch)
                .disabled(!layersState.trafficAvailability.isAvailable)
                .accessibilityIdentifier("map-layer-traffic")
                .accessibilityLabel("Traffic")
                .accessibilityValue(layersState.isTrafficEnabled ? "On" : "Off")
                if let reasonText = layersState.trafficAvailability.reasonText {
                    ReferenceCaptionText(text: reasonText)
                        .accessibilityIdentifier("map-layer-traffic-reason")
                }
            }
            .padding(AppLayout.stackTight)
        }
    }

    /// One switch per demo scene key, writing the same remembered choice the
    /// home sheet used to carry; this panel is the one scene home on the
    /// native refapps.
    private var overlaysSection: some View {
        ReferenceControlGroup(groupLabel: "Overlays", disabledReason: nil) {
            VStack(alignment: .leading, spacing: AppLayout.stackTight) {
                if let catalog = applicationModel.catalog {
                    ForEach(catalog.overlayScenes.keys.sorted(), id: \.self) { sceneKey in
                        Toggle(
                            isOn: Binding(
                                get: {
                                    applicationModel.enabledSceneKeys.contains(sceneKey)
                                },
                                set: { isEnabled in
                                    applicationModel.toggleScene(
                                        sceneKey: sceneKey,
                                        isEnabled: isEnabled
                                    )
                                }
                            )
                        ) {
                            Text(catalog.overlayScenes[sceneKey]?.label ?? sceneKey)
                                .font(ReferenceAppUXFont.body())
                                .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
                        }
                        .toggleStyle(.switch)
                        .accessibilityIdentifier("map-layer-scene-\(sceneKey)")
                        .accessibilityValue(
                            applicationModel.enabledSceneKeys.contains(sceneKey)
                                ? "On"
                                : "Off"
                        )
                    }
                    ReferenceSubtleButton(
                        label: ReferenceApplicationConfiguration.clearOverlaysActionText
                    ) {
                        applicationModel.clearOverlays()
                    }
                    .accessibilityIdentifier("map-layer-clear-overlays")
                } else {
                    ReferenceCaptionText(text: "The shared demo content is still loading…")
                }
            }
            .padding(AppLayout.stackTight)
        }
    }

    /// The device-location row: its state, the Stop action while the source
    /// runs, and the reason when it cannot start.
    private var deviceLocationSection: some View {
        ReferenceControlGroup(groupLabel: "Device location", disabledReason: nil) {
            VStack(alignment: .leading, spacing: AppLayout.stackTight) {
                HStack(spacing: AppLayout.stackTight) {
                    Text(layersState.locationStatus.displayText)
                        .font(ReferenceAppUXFont.body())
                        .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
                        .accessibilityIdentifier("device-location-status")
                    Spacer(minLength: AppLayout.stackTight)
                    if layersState.locationStatus.isSourceRunning {
                        ReferenceSubtleButton(label: "Stop") {
                            applicationModel.stopDeviceLocation()
                        }
                        .accessibilityIdentifier("device-location-stop")
                    }
                }
                if let reasonText = layersState.locationStatus.cannotStartReasonText {
                    ReferenceCaptionText(text: reasonText)
                }
            }
            .padding(AppLayout.stackTight)
        }
    }

    /// The panel's status line (a failed traffic refresh, a refusal, a
    /// location reason).
    @ViewBuilder
    private var statusLine: some View {
        if let statusLineText = layersState.statusLineText {
            Text(statusLineText)
                .font(ReferenceAppUXFont.caption())
                .foregroundStyle(ReferenceAppUXColor.warning(theme))
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityIdentifier("map-layers-status")
        }
    }
}
