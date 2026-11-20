/**
 * The shared SDK failure-text mappings: a thrown error's verbatim words, the
 * contract's missing-provider variant, and the consumer sentence every
 * surface renders for a capability no provider serves.
 */

import { appStrings } from '../src/theme';
import {
  capabilityUnavailableText,
  isProviderNotConfiguredFailure,
  verbatimErrorText,
} from '../src/sdkErrorText';

test("verbatimErrorText keeps an error's own words and stringifies other values", () => {
  expect(verbatimErrorText(new Error('the SDK refused'))).toBe(
    'the SDK refused',
  );
  expect(verbatimErrorText('plain refusal')).toBe('plain refusal');
});

test('isProviderNotConfiguredFailure recognises the contract variant only', () => {
  expect(
    isProviderNotConfiguredFailure(
      new Error('ProviderNotConfigured(contractName=RoutingProvider)'),
    ),
  ).toBe(true);
  expect(isProviderNotConfiguredFailure(new Error('unreachable'))).toBe(false);
});

test('capabilityUnavailableText renders the shared sentence for the contract variant', () => {
  expect(
    capabilityUnavailableText(
      new Error('ProviderNotConfigured(contractName=RoutingProvider)'),
    ),
  ).toBe(
    appStrings.providerNotConfiguredDetailTemplateText.replace(
      '{provider}',
      'RoutingProvider',
    ),
  );

  // The binding sometimes reports only the field list, not the full variant
  // representation. The contract field is the stable signal either way.
  expect(
    capabilityUnavailableText(
      new Error('ProviderNotConfigured: contractName=VectorTiles, other=1'),
    ),
  ).toBe(
    appStrings.providerNotConfiguredDetailTemplateText.replace(
      '{provider}',
      'VectorTiles',
    ),
  );
});

test('capabilityUnavailableText keeps any other failure verbatim', () => {
  expect(capabilityUnavailableText(new Error('the demo server refused'))).toBe(
    'the demo server refused',
  );
});
