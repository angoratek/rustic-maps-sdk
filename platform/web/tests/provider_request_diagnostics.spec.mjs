// The provider-request diagnostics bridge in the browser: every observed
// provider request becomes one diagnostics event under the transport's
// target, carrying the provider key, contract, method, redacted URL, status,
// duration and outcome. The credential never appears. One case per
// credentialed geocoding provider, so a module-path regression in the
// diagnostics filter fails for every provider rather than for one example.
// Every provider fact comes from the shared registry, and the dev server
// serves the recorded fixture same-origin, so no test touches the network.
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, test, waitForMapTestReady } from './network_guard.mjs';

const TEST_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const REPOSITORY_ROOT = path.resolve(TEST_DIRECTORY, '..', '..', '..');
const PROVIDERS_REGISTRY = JSON.parse(
  readFileSync(
    path.join(REPOSITORY_ROOT, 'platform', 'providers.json'),
    'utf8',
  ),
);

/** The capability contract the settings surface configures. */
const GEOCODING_CONTRACT = 'geocoding';
/** The contract name the observation reports. */
const GEOCODING_PROVIDER_CONTRACT = 'GeocodingProvider';
/** The transport's diagnostics target, the event's subsystem key. */
const PROVIDER_REQUEST_DIAGNOSTIC_TARGET = 'provider_transport';
/** A unique credential, so the redaction assertion cannot pass by accident. */
const DIAGNOSTIC_CREDENTIAL = 'diagnostic-probe-secret-value';
/** The flush's requested cap: larger than the ring's own bound. */
const DIAGNOSTIC_FLUSH_LIMIT = 4096;

/**
 * The literal request path a fixture route pattern names, or `null` when the
 * pattern has no literal form to assert against.
 */
function literal_request_path(path_pattern) {
  const body = path_pattern.replace(/^\^|\$$/g, '').replaceAll('\\.', '.');
  return /^[A-Za-z0-9\-._~/;,]+$/.test(body) ? `/${body}` : null;
}

/**
 * One case per credentialed provider the browser can register that serves
 * geocoding from a recorded payload.
 */
const GEOCODING_DIAGNOSTIC_CASES = PROVIDERS_REGISTRY.providers
  .filter(
    (descriptor) =>
      descriptor.registration_available?.wasm === true
      && descriptor.credential_label !== ''
      && descriptor.served_contracts.includes(GEOCODING_CONTRACT)
      && (descriptor.fixture_routes ?? []).some(
        (fixture_route) => fixture_route.purpose === 'geocoding',
      ),
  )
  .map((descriptor) => ({
    descriptor,
    requestPath: literal_request_path(
      descriptor.fixture_routes.find(
        (fixture_route) => fixture_route.purpose === 'geocoding',
      ).path_pattern,
    ),
  }));

if (GEOCODING_DIAGNOSTIC_CASES.length < 2) {
  throw new Error(
    'the registry declares fewer than two credentialed geocoding providers '
      + 'with a fixture route, so observations are proven for one example only',
  );
}

test.beforeEach(async ({ page }) => {
  await page.goto('/?advanced');
  await waitForMapTestReady(page);
});

for (const diagnostic_case of GEOCODING_DIAGNOSTIC_CASES) {
  const { descriptor, requestPath } = diagnostic_case;
  test(`${descriptor.provider_key} request is recorded as one redacted observation`, async ({
    page,
  }) => {
    await page.evaluate(
      ([contract, provider_key, fixture_base_path, credential]) => {
        window.mapTest.setProviderBaseUrl(
          provider_key,
          `${window.location.origin}${fixture_base_path}/${provider_key}`,
        );
        window.mapTest.configureProvider(contract, provider_key, credential);
      },
      [
        GEOCODING_CONTRACT,
        descriptor.provider_key,
        PROVIDERS_REGISTRY.fixture_base_path,
        DIAGNOSTIC_CREDENTIAL,
      ],
    );

    const rows = await page.evaluate(() =>
      window.mapTest.forwardGeocode('Sacramento,California'),
    );
    expect(rows.length).toBeGreaterThan(0);

    const observation_lines = await page.evaluate(
      ([target, provider_key, flush_limit]) => {
        const events = window.mapTest.flushRecentDiagnosticEvents(
          window.mapTest.SdkLogSeverity.Info,
          flush_limit,
        );
        const provider_key_field = `provider_key=${provider_key}`;
        return events
          .filter(
            (event) =>
              event.subsystemTarget === target
              && event.messageLine.includes(provider_key_field),
          )
          .map((event) => event.messageLine);
      },
      [
        PROVIDER_REQUEST_DIAGNOSTIC_TARGET,
        descriptor.provider_key,
        DIAGNOSTIC_FLUSH_LIMIT,
      ],
    );

    expect(observation_lines).toHaveLength(1);
    const observation_line = observation_lines[0];
    const expected_fragments = [
      'provider request completed',
      `provider_key=${descriptor.provider_key}`,
      `provider_contract=${GEOCODING_PROVIDER_CONTRACT}`,
      'http_method=GET',
      'status_class=2xx',
      'outcome=success',
      'duration_milliseconds=',
      'response_bytes=',
    ];
    if (requestPath !== null) {
      expected_fragments.push(requestPath);
    }
    for (const expected_fragment of expected_fragments) {
      expect(observation_line).toContain(expected_fragment);
    }
    expect(observation_line).not.toContain(DIAGNOSTIC_CREDENTIAL);

    // The same event carries the facts as a typed object, so a console reads
    // fields instead of parsing the composed line.
    const provider_request = await page.evaluate(
      ([target, provider_key, flush_limit]) => {
        const events = window.mapTest.flushRecentDiagnosticEvents(
          window.mapTest.SdkLogSeverity.Info,
          flush_limit,
        );
        const event = events.find(
          (candidate) =>
            candidate.subsystemTarget === target
            && candidate.providerRequest?.providerKey === provider_key,
        );
        return event?.providerRequest ?? null;
      },
      [
        PROVIDER_REQUEST_DIAGNOSTIC_TARGET,
        descriptor.provider_key,
        DIAGNOSTIC_FLUSH_LIMIT,
      ],
    );
    expect(provider_request).not.toBeNull();
    expect(provider_request.providerKey).toBe(descriptor.provider_key);
    expect(provider_request.providerContract).toBe(GEOCODING_PROVIDER_CONTRACT);
    expect(provider_request.httpMethod).toBe('GET');
    if (requestPath !== null) {
      expect(provider_request.requestPath).toContain(requestPath);
    }
    // Whether the credential travels in a query parameter or a header, the
    // redacted path never carries its value.
    expect(provider_request.requestPath).not.toContain(DIAGNOSTIC_CREDENTIAL);
    expect(provider_request.statusClass).toBe('2xx');
    expect(provider_request.outcome).toBe('success');
    expect(provider_request.durationMilliseconds).toBeGreaterThanOrEqual(0);
    // The observer reports the byte count the provider layer measured; a layer
    // that does not report a body length answers zero.
    expect(provider_request.responseBytes).toBeGreaterThanOrEqual(0);
    expect(JSON.stringify(provider_request)).not.toContain(
      DIAGNOSTIC_CREDENTIAL,
    );
  });
}
