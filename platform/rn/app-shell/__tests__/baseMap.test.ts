/**
 * Jest-level unit tests of the reference application's base-map decisions
 * (M9's native base-map switch): which registry descriptors the settings
 * sheet offers, whether each can serve yet, which one is serving, and what
 * one row's act registers — pure decisions, no native side.
 */

import {
  activeBaseMap,
  baseMapChoices,
  baseMapRegistrationPlan,
  baseMapMinimumZoomLevel,
  hostedVectorBaseInstall,
  keylessVectorBaseChoice,
  vectorBaseAttributionText,
  vectorBaseChoiceKey,
  vectorBaseChoiceLabel,
  vectorBaseMapChoices,
  vectorBaseSource,
} from '../src/baseMap';
import type { ProviderCredentialDescriptor } from '../src/providerRegistry';

/** A credential-less raster source the SDK registers itself. */
const openStreetMapDescriptor: ProviderCredentialDescriptor = {
  provider_key: 'openstreetmap',
  display_name: 'OpenStreetMap',
  credential_label: '',
  registration_available: {
    android: true,
    ios: true,
    react_native: true,
    wasm: false,
  },
  attribution_text: 'Map data © OpenStreetMap contributors',
  terms_note: 'OpenStreetMap tile usage policy applies.',
  settings_hint: '',
  raster_url_template: 'https://tiles.example.org/{z}/{x}/{y}.png',
  maximum_zoom_level: 19,
};

/** A credential provider whose registration installs its raster slot. */
const credentialedDescriptor: ProviderCredentialDescriptor = {
  provider_key: 'credentialed',
  display_name: 'Credentialed provider',
  credential_label: 'API key',
  registration_available: {
    android: true,
    ios: true,
    react_native: true,
    wasm: false,
  },
  attribution_text: 'Map data © Credentialed provider',
  settings_hint: 'Create a key.',
};

/** A descriptor with no React Native registration arm. */
const unavailableDescriptor: ProviderCredentialDescriptor = {
  ...credentialedDescriptor,
  provider_key: 'unavailable',
  display_name: 'Unavailable provider',
  registration_available: {
    android: false,
    ios: false,
    react_native: false,
    wasm: false,
  },
};

test('choices offer credential-less sources and every credential provider', () => {
  const descriptors = [
    openStreetMapDescriptor,
    credentialedDescriptor,
    unavailableDescriptor,
  ];

  const keyless = baseMapChoices(descriptors, new Set());
  expect(
    keyless.map(choice => [choice.descriptor.provider_key, choice.isServable]),
  ).toEqual([
    ['openstreetmap', true],
    // Offered before its key is applied, but not servable yet.
    ['credentialed', false],
  ]);

  const credentialed = baseMapChoices(descriptors, new Set(['credentialed']));
  expect(
    credentialed.map(choice => [choice.descriptor.provider_key, choice.isServable]),
  ).toEqual([
    ['openstreetmap', true],
    ['credentialed', true],
  ]);
});

test('a credential provider without a registration arm is never offered', () => {
  const choices = baseMapChoices(
    [unavailableDescriptor],
    new Set(['unavailable']),
  );
  expect(choices).toEqual([]);
});

test('the active base map falls back only to servable choices', () => {
  const choices = baseMapChoices(
    [credentialedDescriptor, openStreetMapDescriptor],
    new Set(),
  );

  // The credential descriptor is first in registry order but not servable:
  // the fallback skips it.
  expect(activeBaseMap(choices, null)?.provider_key).toBe('openstreetmap');
  // An unservable selection never becomes active either.
  expect(activeBaseMap(choices, 'credentialed')?.provider_key).toBe(
    'openstreetmap',
  );
  expect(activeBaseMap([], null)).toBeNull();

  const servableChoices = baseMapChoices(
    [credentialedDescriptor, openStreetMapDescriptor],
    new Set(['credentialed']),
  );
  // Once its key is applied the credential provider is servable and, being
  // first in registry order, becomes the default.
  expect(activeBaseMap(servableChoices, null)?.provider_key).toBe(
    'credentialed',
  );
  expect(activeBaseMap(servableChoices, 'openstreetmap')?.provider_key).toBe(
    'openstreetmap',
  );
});

test('a credential-less descriptor plans the URL-template record', () => {
  const plan = baseMapRegistrationPlan(
    openStreetMapDescriptor,
    'ReferenceApp/1.2.3',
  );
  expect(plan).toEqual({
    kind: 'urlTemplate',
    source: {
      urlTemplate: 'https://tiles.example.org/{z}/{x}/{y}.png',
      sourceName: 'openstreetmap',
      minimumZoomLevel: baseMapMinimumZoomLevel,
      maximumZoomLevel: 19,
      imageFormat: 0, // generated RasterImageFormat.Png
      tileResolution: 0, // generated RasterTileResolution.Standard
      userAgent: 'ReferenceApp/1.2.3',
    },
  });
});

test('a descriptor without a maximum zoom reads the artifact default of zero', () => {
  const plan = baseMapRegistrationPlan(
    { ...openStreetMapDescriptor, maximum_zoom_level: undefined },
    '',
  );
  expect(plan.kind).toBe('urlTemplate');
  if (plan.kind === 'urlTemplate') {
    expect(plan.source.maximumZoomLevel).toBe(0);
    expect(plan.source.userAgent).toBe('');
  }
});

test('a credentialed descriptor plans a credentialed restore marker', () => {
  expect(baseMapRegistrationPlan(credentialedDescriptor, '')).toEqual({
    kind: 'credentialedRestore',
    providerKey: 'credentialed',
  });
});

/** A registry descriptor publishing a vector tileset with two committed styles. */
const vectorDescriptor: ProviderCredentialDescriptor = {
  provider_key: 'vector-provider',
  display_name: 'Vector provider',
  credential_label: '',
  registration_available: {
    android: false,
    ios: false,
    react_native: false,
    wasm: false,
  },
  attribution_text: 'Map data © OpenStreetMap contributors',
  terms_note: 'Vector provider terms apply.',
  settings_hint: '',
  url_template: 'https://tiles.example.org/{z}/{x}/{y}.pbf',
  maximum_zoom_level: 14,
  vector_style: 'openmaptiles',
  vector_styles: [
    {
      style_id: 'bright-light',
      display_name: 'Bright',
      theme: 'light',
      attribution_text: 'Style Bright, design CC BY 4.0',
    },
    {
      style_id: 'fiord-dark',
      display_name: 'Fiord',
      theme: 'dark',
      attribution_text: '',
    },
  ],
};

test('vector choices offer one row per committed style of a vector descriptor', () => {
  const choices = vectorBaseMapChoices([
    openStreetMapDescriptor,
    vectorDescriptor,
  ]);
  expect(choices.map(vectorBaseChoiceKey)).toEqual([
    'vector-provider/bright-light',
    'vector-provider/fiord-dark',
  ]);
  expect(choices.map(vectorBaseChoiceLabel)).toEqual([
    'Vector provider · Bright',
    'Vector provider · Fiord',
  ]);
});

test('the registry-declared keyless base resolves to its offered style', () => {
  const descriptors = [openStreetMapDescriptor, vectorDescriptor];
  const keylessChoice = keylessVectorBaseChoice(descriptors, {
    provider_key: 'vector-provider',
    style_id: 'fiord-dark',
  });
  expect(keylessChoice).not.toBeNull();
  expect(keylessChoice && vectorBaseChoiceKey(keylessChoice)).toBe(
    'vector-provider/fiord-dark',
  );
  // An explicit Fiord declaration does not drift to the descriptor's first
  // style: the declaration is honoured exactly or not at all.
  expect(
    keylessVectorBaseChoice(descriptors, {
      provider_key: 'vector-provider',
      style_id: 'positron-light',
    }),
  ).toBeNull();
  expect(
    keylessVectorBaseChoice(descriptors, {
      provider_key: 'openstreetmap',
      style_id: 'bright-light',
    }),
  ).toBeNull();
  expect(keylessVectorBaseChoice(descriptors, undefined)).toBeNull();
  expect(
    keylessVectorBaseChoice(descriptors, {
      provider_key: '',
      style_id: 'bright-light',
    }),
  ).toBeNull();
});

test('a descriptor without a vector template or styles offers no vector base', () => {
  expect(vectorBaseMapChoices([openStreetMapDescriptor])).toEqual([]);
  expect(
    vectorBaseMapChoices([{ ...vectorDescriptor, vector_styles: [] }]),
  ).toEqual([]);
});

test('a vector choice carries the style attribution, falling back to the provider line', () => {
  const choices = vectorBaseMapChoices([vectorDescriptor]);
  expect(vectorBaseAttributionText(choices[0])).toBe(
    'Style Bright, design CC BY 4.0',
  );
  expect(vectorBaseAttributionText(choices[1])).toBe(
    'Map data © OpenStreetMap contributors',
  );
});

test('a vector choice plans the contract source record', () => {
  const [brightChoice] = vectorBaseMapChoices([vectorDescriptor]);
  expect(vectorBaseSource(brightChoice)).toEqual({
    urlTemplate: 'https://tiles.example.org/{z}/{x}/{y}.pbf',
    sourceName: 'vector-provider',
    attributionText: 'Style Bright, design CC BY 4.0',
    styleId: 'bright-light',
    minimumZoomLevel: baseMapMinimumZoomLevel,
    maximumZoomLevel: 14,
  });
});

/** A credentialed descriptor whose vector family paints a hosted style. */
const hostedVectorDescriptor: ProviderCredentialDescriptor = {
  provider_key: 'hosted-vector-provider',
  display_name: 'Hosted vector provider',
  credential_label: 'Access token',
  served_contracts: ['vector_tiles'],
  registration_available: {
    android: true,
    ios: true,
    react_native: true,
    wasm: true,
  },
  attribution_text: '© Hosted vector provider',
  settings_hint: 'Create a token.',
  maximum_zoom_level: 22,
  vector_styles: [
    {
      style_id: 'streets-light',
      display_name: 'Streets',
      theme: 'light',
      attribution_text: '© Hosted vector provider',
      document_url: 'https://styles.example.org/v1/streets',
    },
  ],
};

test('a credentialed vector family is listed and states whether it can install', () => {
  // Without the credential the row is listed but cannot install, so the panel
  // states `Needs <credential label>` rather than hiding the family.
  const [unkeyedChoice] = vectorBaseMapChoices([hostedVectorDescriptor]);
  expect(vectorBaseChoiceKey(unkeyedChoice)).toBe(
    'hosted-vector-provider/streets-light',
  );
  expect(unkeyedChoice.isServable).toBe(false);
  const [keyedChoice] = vectorBaseMapChoices(
    [hostedVectorDescriptor],
    new Set(['hosted-vector-provider']),
  );
  expect(keyedChoice.isServable).toBe(true);
  // A keyless template family always installs.
  const [keylessChoice] = vectorBaseMapChoices([vectorDescriptor]);
  expect(keylessChoice.isServable).toBe(true);
});

test('a hosted vector choice plans the provider-document install', () => {
  const [hostedChoice] = vectorBaseMapChoices(
    [hostedVectorDescriptor],
    new Set(['hosted-vector-provider']),
  );
  expect(hostedVectorBaseInstall(hostedChoice)).toEqual({
    source: {
      providerKey: 'hosted-vector-provider',
      styleId: 'streets-light',
    },
    styleDocumentUrl: 'https://styles.example.org/v1/streets',
  });
  // A committed style installs through the synchronous template member.
  const [carriedChoice] = vectorBaseMapChoices([vectorDescriptor]);
  expect(hostedVectorBaseInstall(carriedChoice)).toBeNull();
});
