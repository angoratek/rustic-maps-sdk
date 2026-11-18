//! The provider-credential settings surface (docs/ReferenceAppUX.md §6.5):
//! the gear control beside the readiness chip opens a sheet whose rows come
//! from the shared provider registry's descriptors — one masked secure field,
//! an explicit Apply and Clear, the row's status, the opt-in "Remember on
//! this device" toggle (default off), and the provider's attribution and
//! hint. No credential is ever echoed in full, logged, or given a show-key
//! affordance.

import SwiftUI
import MapNativeCoreUI

/// The settings surface's platform-glue metrics (the design language owns
/// every token the surface consumes).
private enum ProviderSettingsMetrics {
    /// The gear control's SF Symbol glyph.
    static let gearGlyph = "gearshape"
    /// The gear glyph's point size.
    static let gearGlyphPixels: CGFloat = 14
    /// The gear button's square edge in points (the readiness chip's
    /// height class).
    static let gearButtonSize: CGFloat = 30
    /// The disabled controls' opacity (the group still reads as present,
    /// not as a rendering artifact).
    static let disabledControlOpacity: Double = 0.45
}

/// The gear control beside the readiness chip: it opens the settings sheet.
struct ProviderSettingsGearControl: View {
    @ObservedObject var applicationModel: ReferenceApplicationModel

    @Environment(\.appTheme) private var theme

    var body: some View {
        Button {
            applicationModel.isProviderSettingsSheetOpen = true
        } label: {
            Image(systemName: ProviderSettingsMetrics.gearGlyph)
                .font(.system(size: ProviderSettingsMetrics.gearGlyphPixels))
                .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
                .frame(
                    width: ProviderSettingsMetrics.gearButtonSize,
                    height: ProviderSettingsMetrics.gearButtonSize
                )
                .background(
                    RoundedRectangle(cornerRadius: AppLayout.radiusInline)
                        .fill(ReferenceAppUXColor.surface(theme))
                )
                .overlay(
                    RoundedRectangle(cornerRadius: AppLayout.radiusInline)
                        .strokeBorder(
                            ReferenceAppUXColor.surfaceBorder(theme),
                            lineWidth: 1
                        )
                )
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(ReferenceApplicationConfiguration.settingsControlLabel)
        .accessibilityIdentifier("provider-settings-gear")
    }
}

/// The settings sheet: the provider-credential rows and the persistence
/// opt-in. Every row comes from the shared provider registry, so a new
/// provider is a descriptor row plus a registration dispatch arm, never a
/// view rewrite.
struct ProviderSettingsView: View {
    @ObservedObject var applicationModel: ReferenceApplicationModel

    @Environment(\.appTheme) private var theme
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass

    /// The documented maximum height fraction for this width (§1): the
    /// tightened phone cap on a compact width, the panel cap otherwise. The
    /// sheet never grows past it; its content column scrolls inside.
    private var settingsMaximumHeightFraction: CGFloat {
        horizontalSizeClass == .regular
            ? AppLayout.panelMaximumHeightFraction
            : AppLayout.phonePanelMaximumHeightFraction
    }

    var body: some View {
        VStack(alignment: .leading, spacing: AppLayout.stackGroup) {
            headerRow
            if !applicationModel.hasAppliedProviderCredential {
                Text(ReferenceApplicationConfiguration.keylessReadinessText)
                    .font(ReferenceAppUXFont.body())
                    .foregroundStyle(ReferenceAppUXColor.textSecondary(theme))
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("provider-settings-keyless-line")
            }
            ScrollView {
                VStack(alignment: .leading, spacing: AppLayout.stackGroup) {
                    CapabilityProvidersSectionView(
                        state: applicationModel.capabilityProviderState
                    )
                    providerCredentialsGroup
                    resetRememberedChoicesRow
                    ReferenceCaptionText(
                        text: ReferenceApplicationConfiguration
                            .credentialStoreExplanation
                    )
                    if let persistenceNote =
                        applicationModel.providerCredentialPersistenceNote
                    {
                        Text(persistenceNote)
                            .font(ReferenceAppUXFont.caption())
                            .foregroundStyle(ReferenceAppUXColor.warning(theme))
                            .fixedSize(horizontal: false, vertical: true)
                            .accessibilityIdentifier(
                                "provider-settings-persistence-note"
                            )
                    }
                }
            }
        }
        .padding(AppLayout.panelPadding)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
        .background(
            ReferenceAppUXColor.surface(theme)
                .ignoresSafeArea()
        )
        .presentationDetents([.fraction(settingsMaximumHeightFraction)])
        .presentationDragIndicator(.visible)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier(
            ReferenceApplicationModel.providerSettingsSheetIdentifier
        )
    }

    /// The sheet's title row and its close control (the diagnostics sheet's
    /// own header pattern).
    private var headerRow: some View {
        HStack {
            Text(ReferenceApplicationConfiguration.settingsTitleText)
                .font(ReferenceAppUXFont.title())
                .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
            Spacer()
            Button(ReferenceApplicationConfiguration.settingsCloseLabel) {
                applicationModel.isProviderSettingsSheetOpen = false
            }
            .font(ReferenceAppUXFont.bodyStrong())
            .foregroundStyle(ReferenceAppUXColor.accent(theme))
            .accessibilityIdentifier("provider-settings-close")
        }
    }

    /// The reset act for the shared remembered-choices record. The separate,
    /// opt-in credential store is deliberately untouched.
    private var resetRememberedChoicesRow: some View {
        ReferenceSubtleButton(
            label: ReferenceApplicationConfiguration.resetRememberedChoicesLabel
        ) {
            applicationModel.resetRememberedChoices()
        }
        .accessibilityIdentifier("provider-settings-reset-remembered-choices")
    }

    /// The provider-credential group: one row per descriptor and the
    /// persistence toggle under them.
    private var providerCredentialsGroup: some View {
        ReferenceControlGroup(
            groupLabel: ReferenceApplicationConfiguration
                .providerCredentialsGroupLabel,
            disabledReason: nil
        ) {
            VStack(alignment: .leading, spacing: AppLayout.stackGroup) {
                ForEach(applicationModel.providerCredentialDescriptors) {
                    descriptor in
                    ProviderCredentialRowView(
                        applicationModel: applicationModel,
                        descriptor: descriptor
                    )
                }
                rememberCredentialRow
            }
            .padding(AppLayout.stackTight)
        }
    }

    /// The opt-in persistence row (§6.5: default off; the Keychain backs it
    /// only while on).
    private var rememberCredentialRow: some View {
        VStack(alignment: .leading, spacing: AppLayout.stackTight) {
            HStack(spacing: AppLayout.stackTight) {
                Text(ReferenceApplicationConfiguration.rememberCredentialLabel)
                    .font(ReferenceAppUXFont.body())
                    .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
                Spacer(minLength: AppLayout.stackTight)
                ReferenceSwitchControl(
                    isOn: Binding(
                        get: {
                            applicationModel.remembersProviderCredential
                        },
                        set: {
                            applicationModel.setRememberProviderCredential($0)
                        }
                    )
                )
            }
            ReferenceCaptionText(
                text: ReferenceApplicationConfiguration
                    .rememberCredentialExplanation
            )
        }
        .accessibilityIdentifier("provider-settings-remember-toggle")
    }
}

/// One descriptor's settings row: the provider's name and credential label,
/// the masked entry, Apply/Clear, the status, and the attribution and hint.
struct ProviderCredentialRowView: View {
    @ObservedObject var applicationModel: ReferenceApplicationModel
    let descriptor: ProviderDescriptor

    @Environment(\.appTheme) private var theme

    /// The row's masked entry text (the draft, never an applied value).
    private var credentialDraftBinding: Binding<String> {
        Binding(
            get: {
                applicationModel.providerCredentialDrafts[
                    descriptor.providerKey, default: ""
                ]
            },
            set: {
                applicationModel.providerCredentialDrafts[descriptor.providerKey] = $0
            }
        )
    }

    private var status: ProviderCredentialStatus {
        applicationModel.providerCredentialStatus(
            forProviderKey: descriptor.providerKey
        )
    }

    private var isRegistrationAvailable: Bool {
        descriptor.registrationAvailable.ios
    }

    /// Apply is explicit and needs a non-empty entry on a provider this
    /// application registers.
    private var canApply: Bool {
        isRegistrationAvailable
            && !credentialDraftBinding.wrappedValue
                .trimmingCharacters(in: .whitespacesAndNewlines)
                .isEmpty
    }

    /// Clear is available once a credential was applied (or a failed apply
    /// left a stored copy to remove).
    private var canClear: Bool {
        switch status {
        case .applied, .failed:
            return true
        case .notApplied, .needsCredential, .clearedRestartRequired:
            return false
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: AppLayout.stackTight) {
            Text(descriptor.displayName)
                .font(ReferenceAppUXFont.bodyStrong())
                .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
            Text(descriptor.credentialLabel)
                .font(ReferenceAppUXFont.caption())
                .foregroundStyle(ReferenceAppUXColor.textSecondary(theme))
            secureCredentialField
            actionRow
            statusLine
            if !isRegistrationAvailable {
                Text(
                    ReferenceApplicationConfiguration.providerNotRegisteredReason(
                        displayName: descriptor.displayName
                    )
                )
                .font(ReferenceAppUXFont.caption())
                .foregroundStyle(ReferenceAppUXColor.warning(theme))
                .fixedSize(horizontal: false, vertical: true)
            }
            ReferenceCaptionText(text: descriptor.attributionText)
            ReferenceCaptionText(text: descriptor.settingsHint)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("provider-credential-row-\(descriptor.providerKey)")
    }

    /// The masked entry: a `SecureField` only — no show-key affordance
    /// (§6.5's security rule), clipboard paste allowed by the platform.
    private var secureCredentialField: some View {
        SecureField(descriptor.credentialLabel, text: credentialDraftBinding)
            .font(ReferenceAppUXFont.body())
            .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
            .textInputAutocapitalization(.never)
            .autocorrectionDisabled()
            .padding(.horizontal, AppLayout.stackGroup)
            .padding(.vertical, AppLayout.stackTight)
            .background(
                RoundedRectangle(cornerRadius: AppLayout.radiusControl)
                    .fill(ReferenceAppUXColor.surface(theme))
            )
            .overlay(
                RoundedRectangle(cornerRadius: AppLayout.radiusControl)
                    .strokeBorder(
                        ReferenceAppUXColor.surfaceBorder(theme),
                        lineWidth: 1
                    )
            )
            .disabled(!isRegistrationAvailable)
            .accessibilityIdentifier(
                "provider-credential-field-\(descriptor.providerKey)"
            )
    }

    /// The explicit Apply and Clear actions.
    private var actionRow: some View {
        HStack(spacing: AppLayout.stackTight) {
            ReferencePrimaryButton(
                label: ReferenceApplicationConfiguration.applyCredentialLabel
            ) {
                applicationModel.applyProviderCredential(
                    providerKey: descriptor.providerKey,
                    credential: credentialDraftBinding.wrappedValue
                )
            }
            .disabled(!canApply)
            .opacity(
                canApply ? 1 : ProviderSettingsMetrics.disabledControlOpacity
            )
            .accessibilityIdentifier(
                "provider-credential-apply-\(descriptor.providerKey)"
            )
            ReferenceSubtleButton(
                label: ReferenceApplicationConfiguration.clearCredentialLabel
            ) {
                applicationModel.clearProviderCredential(
                    providerKey: descriptor.providerKey
                )
            }
            .disabled(!canClear)
            .opacity(
                canClear ? 1 : ProviderSettingsMetrics.disabledControlOpacity
            )
            .accessibilityIdentifier(
                "provider-credential-clear-\(descriptor.providerKey)"
            )
        }
    }

    /// The row's status line: applied, the verbatim failure, or the
    /// restart requirement Clear states.
    @ViewBuilder
    private var statusLine: some View {
        switch status {
        case .notApplied:
            statusText(
                String(
                    format: ReferenceApplicationConfiguration.credentialNotAppliedFormat,
                    descriptor.displayName
                ),
                color: ReferenceAppUXColor.textSecondary(theme)
            )
        case .applied:
            statusText(
                String(
                    format: applicationModel.remembersProviderCredential
                        ? ReferenceApplicationConfiguration
                            .credentialAppliedAndRememberedFormat
                        : ReferenceApplicationConfiguration.credentialAppliedFormat,
                    descriptor.displayName
                ),
                color: ReferenceAppUXColor.positive(theme)
            )
        case let .failed(detailText):
            statusText(
                detailText,
                color: ReferenceAppUXColor.danger(theme)
            )
        case let .needsCredential(detailText):
            statusText(
                detailText,
                color: ReferenceAppUXColor.warning(theme)
            )
        case .clearedRestartRequired:
            statusText(
                ReferenceApplicationConfiguration
                    .credentialClearedRestartRequiredText,
                color: ReferenceAppUXColor.warning(theme)
            )
        }
    }

    private func statusText(_ text: String, color: Color) -> some View {
        Text(text)
            .font(ReferenceAppUXFont.caption())
            .foregroundStyle(color)
            .fixedSize(horizontal: false, vertical: true)
            .accessibilityIdentifier(
                "provider-credential-status-\(descriptor.providerKey)"
            )
    }
}

/// The per-capability provider section: one row per capability the SDK
/// reports through `providerRegistrations`, in the SDK's own order. The
/// rows read the SDK's registration readback; the picker options come from
/// the shared registry's `served_contracts`, so a further provider is a
/// descriptor entry, never a view rewrite.
struct CapabilityProvidersSectionView: View {
    @ObservedObject var state: CapabilityProviderState

    var body: some View {
        if !state.rows.isEmpty {
            ReferenceControlGroup(
                groupLabel: ReferenceApplicationConfiguration
                    .capabilityProvidersGroupLabel,
                disabledReason: nil
            ) {
                VStack(alignment: .leading, spacing: AppLayout.stackGroup) {
                    ForEach(state.rows) { row in
                        CapabilityProviderRowView(state: state, row: row)
                    }
                }
                .padding(AppLayout.stackTight)
            }
        }
    }
}

/// One capability's settings row: the capability's human wording, the
/// serving provider's name (or the honest unconfigured line), the picker of
/// the registry's eligible providers, and the row's message after an act.
/// Raster and vector tiles are read-only: the map's layer control owns
/// those providers, so the row states the serving provider without a second
/// selection path.
struct CapabilityProviderRowView: View {
    @ObservedObject var state: CapabilityProviderState
    let row: CapabilityProviderRow

    @Environment(\.appTheme) private var theme

    var body: some View {
        VStack(alignment: .leading, spacing: AppLayout.stackTight) {
            Text(row.title)
                .font(ReferenceAppUXFont.bodyStrong())
                .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
            if row.isReadOnly {
                readOnlyServingLine
            } else {
                providerPicker
            }
            // The serving descriptor's public demo address and terms travel
            // with the row, so a keyless routing provider states where it
            // sends requests and under what terms.
            if let demoEndpointText = state.servingDemoEndpointText(
                for: row.contract
            ) {
                Text(demoEndpointText)
                    .font(ReferenceAppUXFont.caption())
                    .foregroundStyle(ReferenceAppUXColor.textSecondary(theme))
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier(
                        "capability-provider-endpoint-\(row.id)"
                    )
            }
            if let termsNote = state.servingTermsNote(for: row.contract) {
                ReferenceCaptionText(text: termsNote)
                    .accessibilityIdentifier(
                        "capability-provider-terms-\(row.id)"
                    )
            }
            if let message = state.rowMessage(for: row.contract) {
                Text(message.text)
                    .font(ReferenceAppUXFont.caption())
                    .foregroundStyle(messageColor(message.severity))
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier(
                        "capability-provider-message-\(row.id)"
                    )
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("capability-provider-row-\(row.id)")
    }

    /// The read-only rows' serving line and the layer-control note.
    private var readOnlyServingLine: some View {
        VStack(alignment: .leading, spacing: AppLayout.stackTight) {
            servingText
            Text(ReferenceApplicationConfiguration.capabilityLayerControlNoteText)
                .font(ReferenceAppUXFont.caption())
                .foregroundStyle(ReferenceAppUXColor.textSecondary(theme))
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    /// The picker: a menu of the registry's eligible providers for this
    /// capability. Selecting one calls the SDK through the state type.
    private var providerPicker: some View {
        Menu {
            ForEach(state.options(for: row.contract)) { option in
                Button(option.pickerLabel) {
                    state.select(
                        contract: row.contract,
                        providerKey: option.providerKey
                    )
                }
            }
        } label: {
            HStack(spacing: AppLayout.stackTight) {
                Text(row.servingText)
                    .font(ReferenceAppUXFont.bodyStrong())
                    .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
                Spacer(minLength: AppLayout.stackTight)
                Text(ReferenceApplicationConfiguration.capabilityChangeActionText)
                    .font(ReferenceAppUXFont.bodyStrong())
                    .foregroundStyle(ReferenceAppUXColor.accent(theme))
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
                        ReferenceAppUXColor.surfaceBorder(theme),
                        lineWidth: 1
                    )
            )
            .contentShape(Rectangle())
        }
        .accessibilityIdentifier("capability-provider-picker-\(row.id)")
    }

    /// The serving provider's name, or the honest unconfigured line.
    private var servingText: some View {
        Text(row.servingText)
            .font(ReferenceAppUXFont.bodyStrong())
            .foregroundStyle(ReferenceAppUXColor.textPrimary(theme))
            .accessibilityIdentifier("capability-provider-serving-\(row.id)")
    }

    /// A warning states a next step (the credential still needed); a
    /// refusal is the SDK's own text and reads as an error.
    private func messageColor(
        _ severity: CapabilityProviderRowMessage.Severity
    ) -> Color {
        switch severity {
        case .warning:
            return ReferenceAppUXColor.warning(theme)
        case .refusal:
            return ReferenceAppUXColor.danger(theme)
        }
    }
}
