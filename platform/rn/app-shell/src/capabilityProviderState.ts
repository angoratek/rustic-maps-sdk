/**
 * The settings surface's per-capability provider section
 * (docs/design/ProviderSelectionPerCapability.md): one row per capability the SDK
 * reports through `providerRegistrations()`, in the SDK's own order, with the
 * shared registry's descriptors as the only source of the providers a
 * capability may choose from. The section never hardcodes a capability list
 * or a provider name: the readback names the serving provider, and each
 * descriptor's `served_contracts` decides which capabilities it may serve.
 *
 * Raster and vector tiles are read-only here because the map's layer control
 * owns those slots; their rows state the serving provider without offering a
 * second, conflicting selection path.
 *
 * The module stays free of React and native imports so jest pins every
 * decision with a fake gateway and recorded registrations.
 */

import { ProviderContract } from 'maps-sdk-rn/src/generated/rustic_maps.ts';
import type { ProviderRegistration } from 'maps-sdk-rn/MapsSdk';

import type { ProviderCredentialDescriptor } from './providerRegistry';
import { appStrings } from './theme';

/** One provider a capability row may select. */
export interface CapabilityProviderChoice {
  /** The provider's stable registry key. */
  providerKey: string;
  /** The consumer-facing provider name. */
  displayName: string;
  /** The descriptor's credential label; empty for a keyless provider. */
  credentialLabel: string;
  /** Whether the provider declares a credential at all. */
  requiresCredential: boolean;
  /**
   * The credential label the provider still needs, or null when its key is
   * already applied (or the provider needs none).
   */
  requiredCredentialLabel: string | null;
  /**
   * The composed row label, `{display_name} · {state}`: the same composition
   * the serving line uses, so a picker entry never prints a bare name or a
   * bare state (ReferenceApplicationBehavior.md §provider selection).
   */
  rowLabel: string;
  /**
   * A keyless routing provider's public demo deployment address, or null.
   * It is the credential argument the contract reads as the deployment.
   */
  demoBaseUrl: string | null;
  /** The demo address's rate-limit/terms note, or null. */
  demoBaseUrlNote: string | null;
}

/** One capability row: the SDK readback plus the registry's alternatives. */
export interface CapabilityProviderRow {
  /** The capability this row configures. */
  contract: ProviderContract;
  /** The capability in human words, derived from the contract enum. */
  capabilityDisplayName: string;
  /** The serving provider's registry key, or null when none is installed. */
  servingProviderKey: string | null;
  /** The serving provider's display name, or null when none is installed. */
  servingProviderDisplayName: string | null;
  /** The serving line's composed `{name} · {state}` text, or null. */
  servingProviderRowText: string | null;
  /** Whether the SDK reports a provider installed for this capability. */
  isRegistered: boolean;
  /** Whether another surface (the map's layer control) owns the slot. */
  isReadOnly: boolean;
  /** The one-line note a read-only row carries, or null when it has a picker. */
  readOnlyNoteText: string | null;
  /** The providers the registry offers for the capability, in registry order. */
  choices: CapabilityProviderChoice[];
  /** A refusal or required-credential reason shown verbatim in the row. */
  detailText: string | null;
}

/** The SDK operation a selection uses. */
export interface CapabilityProviderGateway {
  /**
   * Chooses the provider that serves one capability. A refused call rejects
   * with the SDK's own text and leaves the previously serving provider.
   */
  configureProvider(
    contract: ProviderContract,
    providerKey: string,
    credential: string,
  ): Promise<void>;
}

/** One selection's honest outcome. */
export interface CapabilityProviderSelectionOutcome {
  /** Whether the SDK accepted the provider. */
  isApplied: boolean;
  /** The verbatim refusal or required-credential reason (empty on success). */
  detailText: string;
  /**
   * The next step rendered under a refused keyless demo selection: retry or
   * enter an own deployment address (empty when no such step applies).
   */
  nextStepText: string;
  /** The provider that was selected. */
  providerKey: string;
}

/**
 * The capability's registry name (`routing`, `distance_matrix`), matching the
 * spelling `platform/providers.json` uses in `served_contracts`. It derives
 * from the contract's own enum name, so a new contract needs no table here.
 */
export function capabilityRegistryName(contract: ProviderContract): string {
  return contractEnumName(contract)
    .replace(/([a-z0-9])([A-Z])/g, '$1_$2')
    .toLowerCase();
}

/** The capability in human words (`Routing`, `Distance matrix`). */
export function capabilityDisplayName(contract: ProviderContract): string {
  const spacedName = contractEnumName(contract).replace(
    /([a-z0-9])([A-Z])/g,
    '$1 $2',
  );
  return spacedName.charAt(0) + spacedName.slice(1).toLowerCase();
}

/**
 * The contract a registry capability name denotes, or null when the name is
 * not one this contract carries. The inverse of
 * [`capabilityRegistryName`], so a descriptor's `served_contracts` and the
 * SDK's registration call share one spelling.
 */
export function capabilityContractForRegistryName(
  registryName: string,
): ProviderContract | null {
  for (const value of Object.values(ProviderContract)) {
    if (
      typeof value === 'number' &&
      capabilityRegistryName(value) === registryName
    ) {
      return value;
    }
  }
  return null;
}

/** Whether the map's layer control owns this capability's provider slot. */
export function isReadOnlyCapability(contract: ProviderContract): boolean {
  return (
    contract === ProviderContract.RasterTiles ||
    contract === ProviderContract.VectorTiles
  );
}

/** The row's serving line: the provider's composed name and state, or the honest unconfigured line. */
export function capabilityServingStatusText(
  row: CapabilityProviderRow,
): string {
  return (
    row.servingProviderRowText ??
    appStrings.settingsProviderStateNotConfiguredText
  );
}

/**
 * One provider's state, in the shared composition vocabulary:
 * `Ready` when the provider needs no credential or the session holds one,
 * `Needs {credential_label}` when it needs one and the session lacks it.
 */
export function providerCredentialStateText(
  requiresCredential: boolean,
  hasAppliedCredential: boolean,
  credentialLabel: string,
): string {
  if (!requiresCredential || hasAppliedCredential) {
    return appStrings.settingsProviderStateReadyText;
  }
  return appStrings.settingsProviderStateNeedsCredentialFormat.replace(
    '{credential}',
    credentialLabel,
  );
}

/**
 * The one provider-row composition every surface renders:
 * `{display_name} · {state}` (never a bare name or a bare state).
 */
export function composeProviderRowText(
  displayName: string,
  stateText: string,
): string {
  return appStrings.settingsProviderRowFormat
    .replace('{provider}', displayName)
    .replace('{state}', stateText);
}

/**
 * Builds one row per SDK readback entry, in the order the SDK returns them.
 *
 * @param registrations The SDK's `providerRegistrations()` answer.
 * @param descriptors The shared registry's descriptors, in artifact order.
 * @param appliedCredential The credential the session holds for a provider
 *   key, or null when it holds none.
 * @param detailTexts Per-capability refusal or required-credential text.
 */
export function buildCapabilityProviderRows(
  registrations: readonly ProviderRegistration[],
  descriptors: readonly ProviderCredentialDescriptor[],
  appliedCredential: (providerKey: string) => string | null,
  detailTexts: ReadonlyMap<ProviderContract, string> = new Map(),
): CapabilityProviderRow[] {
  return registrations.map(registration =>
    buildCapabilityProviderRow(
      registration,
      descriptors,
      appliedCredential,
      detailTexts.get(registration.contract) ?? null,
    ),
  );
}

/**
 * Selects one provider for one capability.
 *
 * A read-only row or a provider the registry does not offer is a no-op. A
 * provider whose credential is not applied reports that reason without
 * calling the SDK. Every other selection calls `configureProvider` with the
 * credential the session already holds for a credentialed provider and the
 * descriptor's `demo_base_url` for a keyless one; a keyless descriptor that
 * publishes no demo address is refused instead of being pointed at a crate
 * default. A refusal answers the SDK's message verbatim, adds the
 * retry/own-deployment next step for a refused keyless demo server, and
 * leaves the previous provider serving.
 */
export async function applyCapabilityProviderSelection(options: {
  gateway: CapabilityProviderGateway;
  row: CapabilityProviderRow;
  providerKey: string;
  appliedCredential: (providerKey: string) => string | null;
}): Promise<CapabilityProviderSelectionOutcome> {
  const { gateway, row, providerKey, appliedCredential } = options;
  if (row.isReadOnly) {
    return { isApplied: false, detailText: '', nextStepText: '', providerKey };
  }
  const choice = row.choices.find(candidate => candidate.providerKey === providerKey);
  if (choice === undefined) {
    return {
      isApplied: false,
      detailText: appStrings.capabilityProviderNotOfferedTemplate.replace(
        '{provider}',
        providerKey,
      ),
      nextStepText: '',
      providerKey,
    };
  }
  const appliedCredentialText = (appliedCredential(providerKey) ?? '').trim();
  if (choice.requiresCredential && appliedCredentialText === '') {
    return {
      isApplied: false,
      detailText: appStrings.capabilityProviderNeedsCredentialTemplate.replace(
        '{credential}',
        choice.credentialLabel,
      ),
      nextStepText: '',
      providerKey,
    };
  }
  let credential = '';
  if (choice.requiresCredential) {
    credential = appliedCredentialText;
  } else if (choice.demoBaseUrl !== null) {
    credential = choice.demoBaseUrl;
  } else {
    // A keyless provider without a declared demo address: no selection and
    // no invented localhost default. The row says why.
    return {
      isApplied: false,
      detailText: appStrings.settingsProviderStateNotConfiguredText +
        ': the provider descriptor declares no demo deployment address',
      nextStepText: '',
      providerKey,
    };
  }
  try {
    await gateway.configureProvider(row.contract, providerKey, credential);
    return { isApplied: true, detailText: '', nextStepText: '', providerKey };
  } catch (selectionError) {
    return {
      isApplied: false,
      detailText: failureText(selectionError),
      nextStepText: choice.requiresCredential
        ? ''
        : appStrings.settingsRoutingUnreachableNextStepText,
      providerKey,
    };
  }
}

/** The contract's enum name (`Routing`, `DistanceMatrix`). */
function contractEnumName(contract: ProviderContract): string {
  return ProviderContract[contract] ?? '';
}

/** The verbatim text of a thrown selection error. */
function failureText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** One row from the SDK's registration and the shared registry. */
function buildCapabilityProviderRow(
  registration: ProviderRegistration,
  descriptors: readonly ProviderCredentialDescriptor[],
  appliedCredential: (providerKey: string) => string | null,
  detailText: string | null,
): CapabilityProviderRow {
  const providerKey = nonEmptyText(registration.providerKey);
  const registryDescriptor =
    providerKey === null
      ? undefined
      : descriptors.find(descriptor => descriptor.provider_key === providerKey);
  const registryDisplayName = nonEmptyText(registryDescriptor?.display_name);
  const servingDisplayName =
    registryDisplayName ?? nonEmptyText(registration.displayName);
  const isReadOnly = isReadOnlyCapability(registration.contract);
  return {
    contract: registration.contract,
    capabilityDisplayName: capabilityDisplayName(registration.contract),
    servingProviderKey: providerKey,
    servingProviderDisplayName: servingDisplayName,
    servingProviderRowText:
      servingDisplayName === null
        ? null
        : composeProviderRowText(
            servingDisplayName,
            providerCredentialStateText(
              (registryDescriptor?.credential_label ?? '').trim() !== '',
              (appliedCredential(providerKey ?? '') ?? '').trim() !== '',
              (registryDescriptor?.credential_label ?? '').trim(),
            ),
          ),
    isRegistered: registration.registered,
    isReadOnly,
    readOnlyNoteText: isReadOnly
      ? appStrings.capabilityProviderReadOnlyNoteText
      : null,
    choices: isReadOnly
      ? []
      : offeredChoices(registration.contract, descriptors, appliedCredential),
    detailText,
  };
}

/** The registry's providers that serve the capability and register on React Native. */
function offeredChoices(
  contract: ProviderContract,
  descriptors: readonly ProviderCredentialDescriptor[],
  appliedCredential: (providerKey: string) => string | null,
): CapabilityProviderChoice[] {
  const capabilityName = capabilityRegistryName(contract);
  return descriptors
    .filter(
      descriptor =>
        descriptor.registration_available?.react_native === true &&
        (descriptor.served_contracts ?? []).includes(capabilityName),
    )
    .map(descriptor => {
      const credentialLabel = (descriptor.credential_label ?? '').trim();
      const requiresCredential = credentialLabel !== '';
      const hasAppliedCredential =
        (appliedCredential(descriptor.provider_key) ?? '').trim() !== '';
      const demoBaseUrl = nonEmptyText(descriptor.demo_base_url);
      return {
        providerKey: descriptor.provider_key,
        displayName: descriptor.display_name,
        credentialLabel,
        requiresCredential,
        requiredCredentialLabel:
          requiresCredential && !hasAppliedCredential
            ? credentialLabel
            : null,
        rowLabel: composeProviderRowText(
          descriptor.display_name,
          providerCredentialStateText(
            requiresCredential,
            hasAppliedCredential,
            credentialLabel,
          ),
        ),
        demoBaseUrl,
        demoBaseUrlNote: nonEmptyText(descriptor.demo_base_url_note),
      };
    });
}

/** A trimmed non-empty string, or null for anything else. */
function nonEmptyText(value: string | null | undefined): string | null {
  if (typeof value !== 'string') {
    return null;
  }
  const trimmedText = value.trim();
  return trimmedText === '' ? null : trimmedText;
}
