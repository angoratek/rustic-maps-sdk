/**
 * The SDK's failure text in consumer language: a thrown error's own words,
 * the contract's missing-provider variant, and the shared sentence every
 * surface renders for a capability no provider serves. The screen and the
 * capability provider module both read these mappings from here, so the
 * verbatim-text rule has one implementation.
 *
 * The module stays free of React and native imports so jest pins every
 * mapping directly.
 */

import { appStrings } from './theme';

/** A thrown error's own text, without an `Error:` prefix (the SDK's verbatim words). */
export function verbatimErrorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * Whether a thrown error is the contract's missing-provider variant. The
 * generated binding's message carries the variant name, so a surface can turn
 * its control off with the capability's own reason.
 */
export function isProviderNotConfiguredFailure(error: unknown): boolean {
  return /ProviderNotConfigured/.test(verbatimErrorText(error));
}

/**
 * The consumer sentence for a capability read that failed because no
 * provider serves it. The generated binding's message names the contract
 * (`ProviderNotConfigured(contractName=RoutingProvider)`); every surface
 * renders the shared `no '{provider}' provider is configured` sentence
 * instead of the binding's representation. Any other failure keeps its own
 * verbatim text.
 */
export function capabilityUnavailableText(error: unknown): string {
  const message = verbatimErrorText(error);
  // The binding's message for this variant can be either the full
  // `ProviderNotConfigured(contractName=…)` representation or just the
  // `contractName=…` field list, so the field is the stable signal.
  const contractName = /contractName=([^,)]+)/.exec(message)?.[1]?.trim();
  if (contractName === undefined) {
    return message;
  }
  return appStrings.providerNotConfiguredDetailTemplateText.replace(
    '{provider}',
    contractName,
  );
}
