//! The Directions control (PRD.md §6 item 36): the mode and
//! strategy every plan carries, beside the plan action. Car and Fastest are
//! the defaults; the strategy row shows only for automobile plans, and a
//! choice the serving routing provider reports it cannot honour is disabled
//! with its reason. Both choices join the versioned remembered record.

import SwiftUI
import MapNativeCoreUI

/// The Directions section beside the plan action.
struct DirectionsControlView: View {
    @ObservedObject var applicationModel: ReferenceApplicationModel
    @ObservedObject var directionsState: DirectionsChoiceState

    @Environment(\.appTheme) private var theme

    var body: some View {
        ReferenceControlGroup(
            groupLabel: ReferenceApplicationConfiguration.directionsGroupLabel,
            disabledReason: nil
        ) {
            VStack(alignment: .leading, spacing: AppLayout.stackTight) {
                choiceRow(
                    rowLabel: ReferenceApplicationConfiguration
                        .directionsModeRowLabel,
                    rowIdentifier: "directions-mode-row"
                ) {
                    ForEach(DirectionsMode.allCases) { offeredMode in
                        modeChoiceButton(offeredMode)
                    }
                }
                if directionsState.isStrategyRowVisible {
                    choiceRow(
                        rowLabel: ReferenceApplicationConfiguration
                            .directionsStrategyRowLabel,
                        rowIdentifier: "directions-strategy-row"
                    ) {
                        ForEach(DirectionsStrategy.allCases) { offeredStrategy in
                            strategyChoiceButton(offeredStrategy)
                        }
                    }
                }
                capabilityReasons
            }
            .padding(AppLayout.stackTight)
        }
        .onAppear {
            // The plan sheet appearing re-reads what the serving routing
            // provider can honour; a provider or credential change does the
            // same through the model.
            applicationModel.refreshDirectionsCapabilities()
        }
    }

    /// One labelled choice row (the mode row, then the strategy row).
    private func choiceRow<Content: View>(
        rowLabel: String,
        rowIdentifier: String,
        @ViewBuilder content: () -> Content
    ) -> some View {
        VStack(alignment: .leading, spacing: AppLayout.stackTight) {
            Text(rowLabel)
                .font(ReferenceAppUXFont.caption())
                .foregroundStyle(ReferenceAppUXColor.textSecondary(theme))
                // The label carries the row's identifier; an identifier on
                // the row container would clobber the choices' own handles.
                .accessibilityIdentifier(rowIdentifier)
            HStack(spacing: AppLayout.stackTight) {
                content()
            }
        }
    }

    /// One mode choice: selected styling, selected accessibility trait, and
    /// the disabled-with-reason state.
    private func modeChoiceButton(_ offeredMode: DirectionsMode) -> some View {
        let disabledReason = directionsState.disabledReason(for: offeredMode)
        let isSelected = directionsState.mode == offeredMode
        return ReferenceSubtleButton(
            label: offeredMode.displayName,
            isSelected: isSelected
        ) {
            directionsState.selectMode(offeredMode)
        }
        .disabled(disabledReason != nil)
        .opacity(disabledReason == nil ? 1 : 0.45)
        .accessibilityIdentifier(modeIdentifier(offeredMode))
        .accessibilityAddTraits(isSelected ? [.isSelected] : [])
    }

    /// One strategy choice: selected styling, selected accessibility trait,
    /// and the disabled-with-reason state.
    private func strategyChoiceButton(
        _ offeredStrategy: DirectionsStrategy
    ) -> some View {
        let isShortest = offeredStrategy == .shortest
        let disabledReason = isShortest
            ? directionsState.shortestDisabledReason
            : nil
        let isSelected = directionsState.strategy == offeredStrategy
        return ReferenceSubtleButton(
            label: offeredStrategy.displayName,
            isSelected: isSelected
        ) {
            directionsState.selectStrategy(offeredStrategy)
        }
        .disabled(disabledReason != nil)
        .opacity(disabledReason == nil ? 1 : 0.45)
        .accessibilityIdentifier(strategyIdentifier(offeredStrategy))
        .accessibilityAddTraits(isSelected ? [.isSelected] : [])
    }

    /// The disabled choices' reasons, and the capability read's own reason
    /// when it throws (the keyless `ProviderNotConfigured` state).
    @ViewBuilder
    private var capabilityReasons: some View {
        if let capabilityReasonText = directionsState.capabilityReasonText {
            ReferenceCaptionText(text: capabilityReasonText)
                .accessibilityIdentifier("directions-capability-reason")
        }
        ForEach(DirectionsMode.allCases) { offeredMode in
            if let reasonText = directionsState.disabledReason(for: offeredMode) {
                ReferenceCaptionText(text: reasonText)
                    .accessibilityIdentifier(
                        "directions-mode-reason-\(offeredMode.rawValue)"
                    )
            }
        }
        if let shortestDisabledReason = directionsState.shortestDisabledReason {
            ReferenceCaptionText(text: shortestDisabledReason)
                .accessibilityIdentifier("directions-strategy-reason-shortest")
        }
    }

    /// The specification's shared test identifier for a mode.
    private func modeIdentifier(_ offeredMode: DirectionsMode) -> String {
        switch offeredMode {
        case .car: return "directions-mode-car"
        case .walk: return "directions-mode-walk"
        }
    }

    /// The specification's shared test identifier for a strategy.
    private func strategyIdentifier(
        _ offeredStrategy: DirectionsStrategy
    ) -> String {
        switch offeredStrategy {
        case .fastest: return "directions-strategy-fastest"
        case .shortest: return "directions-strategy-shortest"
        }
    }
}
