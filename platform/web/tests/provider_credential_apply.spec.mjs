// The two-phase credential apply (issue #67): a descriptor's whole declared
// capability set is validated before the wasm seam is touched, so an
// unmappable contract registers nothing; and a failure partway through the
// registration loop leaves the provider unapplied while the row names the
// capabilities that did register. Every provider fact comes from the shared
// registry `platform/providers.json`, so this suite names no vendor, and the
// network guard aborts anything that leaves the harness origin.
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, test, waitForMapTestReady } from './network_guard.mjs';

const TEST_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const REPOSITORY_ROOT = path.resolve(TEST_DIRECTORY, '..', '..', '..');

/** The shared provider registry the dev server also serves to the page. */
const providers_registry = JSON.parse(
  readFileSync(path.join(REPOSITORY_ROOT, 'platform', 'providers.json'), 'utf8'),
);

/**
 * The credentialed wasm provider that serves two mapped capabilities, so the
 * second registration can be made to fail after the first succeeds. The
 * provider itself comes from the registry; only the capability keys are
 * named, never a vendor.
 */
const credentialed_provider_descriptor = providers_registry.providers.find(
  (provider_descriptor) =>
    provider_descriptor.registration_available?.wasm === true
    && typeof provider_descriptor.credential_label === 'string'
    && provider_descriptor.credential_label.trim() !== ''
    && Array.isArray(provider_descriptor.served_contracts)
    && provider_descriptor.served_contracts.includes('geocoding')
    && provider_descriptor.served_contracts.includes('routing'),
);
if (credentialed_provider_descriptor === undefined) {
  throw new Error(
    'the registry declares no credentialed provider serving geocoding and routing');
}
const PROVIDER_KEY = credentialed_provider_descriptor.provider_key;
const PROVIDER_DISPLAY_NAME = credentialed_provider_descriptor.display_name;
const PROVIDER_CREDENTIAL = 'test-key';

/** The contract key the registry does not declare and this build cannot map. */
const UNMAPPABLE_CONTRACT_KEY = 'teleportation';

/** The index of the registration call the simulated failure interrupts. */
const FAILING_REGISTRATION_INDEX = 1;

/** The provider row the registry descriptor renders. */
function provider_row(page) {
  return page.locator(
    `#provider-credential-rows [data-provider-key="${PROVIDER_KEY}"]`);
}

/** Opens the settings sheet, fills the key and clicks Apply. */
async function apply_credential(page) {
  await page.locator('#settings-control').click();
  await expect(page.locator('#settings-dialog')).toBeVisible();
  await provider_row(page)
    .locator('[data-provider-credential-input]')
    .fill(PROVIDER_CREDENTIAL);
  await provider_row(page)
    .locator('[data-provider-credential-apply]')
    .click();
}

/** Counts the registration calls the page makes through the wasm seam. */
async function record_configure_provider_calls(page) {
  await page.evaluate(() => {
    window.__configureProviderCalls = [];
    const original_configure_provider = window.mapTest.configureProvider;
    window.mapTest.configureProvider = (...configuration_arguments) => {
      window.__configureProviderCalls.push(configuration_arguments[0]);
      return original_configure_provider(...configuration_arguments);
    };
  });
}

test('an unmappable declared contract registers nothing and names the contract', async ({
  page,
}) => {
  // Serve the same-origin registry with one extra contract the build cannot
  // map; the descriptor still declares mappable contracts beside it.
  const registry_with_unmappable_contract = {
    ...providers_registry,
    providers: providers_registry.providers.map((provider_descriptor) =>
      provider_descriptor.provider_key === PROVIDER_KEY
        ? {
          ...provider_descriptor,
          served_contracts: [
            ...provider_descriptor.served_contracts,
            UNMAPPABLE_CONTRACT_KEY,
          ],
        }
        : provider_descriptor),
  };
  await page.route('**/providers.json', (route) =>
    route.fulfill({ json: registry_with_unmappable_contract }));

  await page.goto('/?useProviderFixtures');
  await waitForMapTestReady(page);
  await record_configure_provider_calls(page);

  await apply_credential(page);

  // The whole set failed validation, so the seam was never called.
  await expect(page.locator('#provider-credential-status')).toContainText('Refused:');
  await expect(page.locator('#provider-credential-status')).toContainText(
    PROVIDER_DISPLAY_NAME);
  await expect(page.locator('#provider-credential-status')).toContainText(
    UNMAPPABLE_CONTRACT_KEY);
  const configure_provider_calls = await page.evaluate(
    () => window.__configureProviderCalls);
  expect(configure_provider_calls).toEqual([]);

  // The provider's own row carries the same refusal, and the provider is not
  // claimed applied.
  const row = provider_row(page);
  await expect(row.locator('[data-provider-apply-failure]')).toBeVisible();
  await expect(row.locator('[data-provider-apply-failure]')).toContainText(
    PROVIDER_DISPLAY_NAME);
  await expect(row.locator('[data-provider-apply-failure]')).toContainText(
    UNMAPPABLE_CONTRACT_KEY);
  await expect(row.locator('.provider-applied-chip')).toBeHidden();
  const registrations = await page.evaluate(
    () => window.mapTest.providerRegistrations());
  expect(registrations.filter(
    (registration) => registration.providerKey === PROVIDER_KEY
      && registration.registered === true)).toEqual([]);
});

test('a mid-apply failure leaves the provider unapplied and names what registered', async ({
  page,
}) => {
  await page.goto('/?useProviderFixtures');
  await waitForMapTestReady(page);

  // Let the first registration through, then fail the second: the seam
  // registers one capability before the apply stops.
  await page.evaluate((failing_registration_index) => {
    window.__configureProviderCalls = [];
    const original_configure_provider = window.mapTest.configureProvider;
    window.mapTest.configureProvider = (...configuration_arguments) => {
      window.__configureProviderCalls.push(configuration_arguments[0]);
      if (window.__configureProviderCalls.length === failing_registration_index + 1) {
        throw new Error('simulated registration failure');
      }
      return original_configure_provider(...configuration_arguments);
    };
  }, FAILING_REGISTRATION_INDEX);

  await apply_credential(page);

  await expect(page.locator('#provider-credential-status')).toContainText('Refused:');
  const configure_provider_calls = await page.evaluate(
    () => window.__configureProviderCalls);
  expect(configure_provider_calls.length).toBe(FAILING_REGISTRATION_INDEX + 1);
  const registered_contract_name = configure_provider_calls[0];
  const failed_contract_name = configure_provider_calls[1];

  // The capability that registered is the registration readback's one truth.
  const registrations = await page.evaluate(
    () => window.mapTest.providerRegistrations());
  expect(registrations.filter(
    (registration) => registration.providerKey === PROVIDER_KEY
      && registration.registered === true).map(
    (registration) => registration.contract)).toEqual([registered_contract_name]);

  // The row records the partial outcome: the provider is not fully applied,
  // the registered capability is named, and the failure that stopped it is
  // carried verbatim. The provider is not claimed applied anywhere.
  const row = provider_row(page);
  const row_failure = row.locator('[data-provider-apply-failure]');
  await expect(row_failure).toBeVisible();
  await expect(row_failure).toContainText(PROVIDER_DISPLAY_NAME);
  await expect(row_failure).toContainText('is not fully applied');
  await expect(row_failure).toContainText(registered_contract_name);
  await expect(row_failure).toContainText('registered');
  await expect(row_failure).toContainText(failed_contract_name);
  await expect(row_failure).toContainText('simulated registration failure');
  await expect(row.locator('.provider-applied-chip')).toBeHidden();
  await expect(page.locator('#trip-credential-action')).toHaveAttribute(
    'data-state', 'keyless');
});
