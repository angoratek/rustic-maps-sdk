// The developer console: the floating control beside the layers control, the
// warning/error dot, the scrubbed event stream at Error/Warning/Info, the
// typed detail view with live cache readbacks, the credential-free export and
// the honest empty state. Provider facts come from the shared registry and
// the dev server serves every recorded fixture same-origin, so no test
// touches the network.
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

/** The credentialed geocoding provider whose fixture route this suite uses. */
const GEOCODING_DESCRIPTOR = PROVIDERS_REGISTRY.providers.find(
  (descriptor) =>
    descriptor.credential_label !== ''
    && descriptor.served_contracts.includes('geocoding')
    && descriptor.fixture_routes.some(
      (fixture_route) => fixture_route.purpose === 'geocoding',
    ),
);
if (GEOCODING_DESCRIPTOR === undefined) {
  throw new Error(
    'the registry declares no credentialed geocoding provider with a fixture route',
  );
}

/** The credentialed provider that serves traffic flow from a fixture. */
const TRAFFIC_FLOW_DESCRIPTOR = PROVIDERS_REGISTRY.providers.find(
  (descriptor) =>
    descriptor.credential_label !== ''
    && descriptor.served_contracts.includes('traffic_flow')
    && descriptor.fixture_routes.some(
      (fixture_route) => fixture_route.purpose === 'traffic_flow',
    ),
);
if (TRAFFIC_FLOW_DESCRIPTOR === undefined) {
  throw new Error(
    'the registry declares no credentialed traffic-flow provider with a fixture route',
  );
}

/** A unique credential, so the redaction assertion cannot pass by accident. */
const CONSOLE_CREDENTIAL = 'developer-console-probe-secret-value';
/** The provider console's control and panel identifiers. */
const CONSOLE_CONTROL_SELECTOR = '#provider-console-control';
const CONSOLE_PANEL_SELECTOR = '#provider-console-panel';
const CONSOLE_ROWS_SELECTOR = '#provider-console-rows li';
const CONSOLE_DETAIL_SELECTOR = '#provider-console-detail';
const CONSOLE_DOT_SELECTOR = '#provider-console-alert-dot';
/** The subsystem an injected test event names, which no real SDK target uses. */
const INJECTED_EVENT_TARGET = 'developer_console_test';

/** A recorded event in the shape the wasm ring flushes. */
function recorded_event({
  severity,
  message_line,
  timestamp_milliseconds,
  provider_request = null,
}) {
  return {
    timestampMillisecondsSinceEpoch: timestamp_milliseconds,
    severity,
    subsystemTarget: INJECTED_EVENT_TARGET,
    messageLine: message_line,
    providerRequest: provider_request,
  };
}

/** Replaces the ring read with a crafted stream for this page only. */
async function inject_recorded_events(page, events) {
  await page.evaluate((crafted_events) => {
    window.mapTest.flushRecentDiagnosticEvents = () => crafted_events;
  }, events);
}

/** Opens the console and waits for its first render. */
async function open_console(page) {
  await page.locator(CONSOLE_CONTROL_SELECTOR).click();
  await expect(page.locator(CONSOLE_PANEL_SELECTOR)).toBeVisible();
}

/** Configures the geocoding fixture route and issues one forward geocode. */
async function issue_geocoding_request(page) {
  await page.evaluate(
    ([contract, provider_key, fixture_base_path, credential]) => {
      window.mapTest.setProviderBaseUrl(
        provider_key,
        `${window.location.origin}${fixture_base_path}/${provider_key}`,
      );
      window.mapTest.configureProvider(contract, provider_key, credential);
    },
    [
      'geocoding',
      GEOCODING_DESCRIPTOR.provider_key,
      PROVIDERS_REGISTRY.fixture_base_path,
      CONSOLE_CREDENTIAL,
    ],
  );
  await page.evaluate(() => window.mapTest.forwardGeocode('Sacramento,California'));
}

/** Applies a synthetic credential through the settings surface, which is the
 * path that records the applied value the export scrub must remove. */
async function apply_synthetic_credential(page, credential) {
  const settings_dialog = page.locator('#settings-dialog');
  if (!(await settings_dialog.evaluate((element) => element.open))) {
    await page.locator('#settings-control').click();
  }
  await expect(settings_dialog).toBeVisible();
  const descriptor_row = page.locator(
    `#provider-credential-rows [data-provider-key="${GEOCODING_DESCRIPTOR.provider_key}"]`,
  );
  await descriptor_row.locator('[data-provider-credential-input]').fill(credential);
  await descriptor_row.locator('[data-provider-credential-apply]').click();
  await expect(page.locator('#settings-credentials-status'))
    .toContainText('Applied');
  await page.keyboard.press('Escape');
  await expect(settings_dialog).toBeHidden();
}

/** Replaces the page's clipboard writer with a capture, so the copy action's
 * text can be asserted without a browser permission. */
async function capture_clipboard_text(page) {
  await page.evaluate(() => {
    window.__providerConsoleCopiedText = null;
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: {
        writeText: async (text) => {
          window.__providerConsoleCopiedText = text;
        },
      },
    });
  });
}

test('the control beside the layers control opens and closes the console', async ({
  page,
}) => {
  await page.goto('/');
  await waitForMapTestReady(page);

  const control = page.locator(CONSOLE_CONTROL_SELECTOR);
  const panel = page.locator(CONSOLE_PANEL_SELECTOR);
  await expect(control).toBeVisible();
  await expect(panel).toBeHidden();
  await expect(control).toHaveAttribute('aria-expanded', 'false');

  // The control sits in the same row as the layers control, not in the header.
  const placement = await page.evaluate(() => {
    const row = document.getElementById('map-floating-control-row');
    return {
      control_in_row: row?.contains(
        document.getElementById('provider-console-control')) === true,
      layers_in_row: row?.contains(
        document.getElementById('map-layers-control')) === true,
    };
  });
  expect(placement).toEqual({ control_in_row: true, layers_in_row: true });
  const control_box = await control.boundingBox();
  const layers_box = await page.locator('#map-layers-control').boundingBox();
  expect(Math.abs(control_box.y - layers_box.y)).toBeLessThanOrEqual(4);

  await control.click();
  await expect(panel).toBeVisible();
  await expect(control).toHaveAttribute('aria-expanded', 'true');

  await page.locator('#provider-console-close').click();
  await expect(panel).toBeHidden();
  await expect(control).toHaveAttribute('aria-expanded', 'false');

  await control.click();
  await expect(panel).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(panel).toBeHidden();
});

test('a warning or error lights the dot while the console is closed', async ({
  page,
}) => {
  await page.goto('/');
  await waitForMapTestReady(page);

  const dot = page.locator(CONSOLE_DOT_SELECTOR);
  await expect(dot).toBeHidden();

  await inject_recorded_events(page, [
    recorded_event({
      severity: 'Warn',
      message_line: 'a provider answer was retried',
      timestamp_milliseconds: Date.now() + 1000,
    }),
  ]);
  await expect(dot).toBeVisible({ timeout: 5000 });
  await expect(dot).toHaveAttribute('data-severity', 'Warn');

  // Opening the panel reads the stream and clears the dot.
  await open_console(page);
  await expect(dot).toBeHidden();
  await page.locator('#provider-console-close').click();
  await expect(page.locator(CONSOLE_PANEL_SELECTOR)).toBeHidden();

  await inject_recorded_events(page, [
    recorded_event({
      severity: 'Error',
      message_line: 'the renderer reported a surface failure',
      timestamp_milliseconds: Date.now() + 2000,
    }),
  ]);
  await expect(dot).toBeVisible({ timeout: 5000 });
  await expect(dot).toHaveAttribute('data-severity', 'Error');
});

test('the level filter narrows the stream to Error, Warning or Info', async ({
  page,
}) => {
  await page.goto('/');
  await waitForMapTestReady(page);

  const base_timestamp = Date.now();
  await inject_recorded_events(page, [
    recorded_event({
      severity: 'Info',
      message_line: 'info-probe-event',
      timestamp_milliseconds: base_timestamp + 1,
    }),
    recorded_event({
      severity: 'Warn',
      message_line: 'warning-probe-event',
      timestamp_milliseconds: base_timestamp + 2,
    }),
    recorded_event({
      severity: 'Error',
      message_line: 'error-probe-event',
      timestamp_milliseconds: base_timestamp + 3,
    }),
  ]);

  await open_console(page);
  const rows = page.locator(CONSOLE_ROWS_SELECTOR);
  await expect(rows).toHaveCount(3);
  // Newest first: the error arrived last.
  await expect(rows.first()).toContainText('error-probe-event');

  const level_filter = page.locator('#provider-console-level-filter');
  await level_filter.selectOption('error');
  await expect(rows).toHaveCount(1);
  await expect(rows.first()).toContainText('error-probe-event');

  await level_filter.selectOption('warning');
  await expect(rows).toHaveCount(2);
  await expect(rows.first()).toContainText('error-probe-event');
  await expect(rows.nth(1)).toContainText('warning-probe-event');

  await level_filter.selectOption('info');
  await expect(rows).toHaveCount(3);
});

test('a provider request detail shows the typed trace fields', async ({
  page,
}) => {
  await page.goto('/');
  await waitForMapTestReady(page);
  await open_console(page);
  await issue_geocoding_request(page);

  const provider_rows = page
    .locator(CONSOLE_ROWS_SELECTOR)
    .filter({ hasText: GEOCODING_DESCRIPTOR.display_name });
  await expect(provider_rows.first()).toBeVisible();
  await provider_rows.first().locator('button').click();
  await expect(page.locator(CONSOLE_DETAIL_SELECTOR)).toBeVisible();

  // The typed trace is the source of truth, not the composed line.
  const typed_trace = await page.evaluate(
    ([provider_key]) => {
      const events = window.mapTest.flushRecentDiagnosticEvents(
        window.mapTest.SdkLogSeverity.Info,
        1000,
      );
      const event = Array.from(events).find(
        (candidate) => candidate.providerRequest?.providerKey === provider_key,
      );
      return event.providerRequest;
    },
    [GEOCODING_DESCRIPTOR.provider_key],
  );

  await expect(page.locator('#provider-console-detail-provider'))
    .toHaveText(GEOCODING_DESCRIPTOR.display_name);
  await expect(page.locator('#provider-console-detail-provider-key'))
    .toHaveText(typed_trace.providerKey);
  await expect(page.locator('#provider-console-detail-contract'))
    .toHaveText(typed_trace.providerContract);
  await expect(page.locator('#provider-console-detail-method'))
    .toHaveText(typed_trace.httpMethod);
  await expect(page.locator('#provider-console-detail-path'))
    .toHaveText(typed_trace.requestPath);
  await expect(page.locator('#provider-console-detail-status'))
    .toHaveText(typed_trace.statusClass);
  await expect(page.locator('#provider-console-detail-duration'))
    .toHaveText(`${Math.round(typed_trace.durationMilliseconds)} ms`);
  await expect(page.locator('#provider-console-detail-outcome'))
    .toHaveText(typed_trace.outcome);
  await expect(page.locator('#provider-console-detail-bytes'))
    .toHaveText(`${typed_trace.responseBytes}`);

  await page.locator('#provider-console-detail-close').click();
  await expect(page.locator(CONSOLE_DETAIL_SELECTOR)).toBeHidden();
  await expect(page.locator(CONSOLE_ROWS_SELECTOR).first()).toBeVisible();
});

test('the detail reads the typed trace when the message line disagrees', async ({
  page,
}) => {
  await page.goto('/');
  await waitForMapTestReady(page);

  // The message line names different facts. A panel that parses it instead of
  // reading `providerRequest` shows MessageLineContract, POST, 5xx and 9999.
  await inject_recorded_events(page, [
    recorded_event({
      severity: 'Info',
      message_line: 'provider request completed'
        + ' provider_key=message-line-provider'
        + ' provider_contract=MessageLineContract'
        + ' http_method=POST'
        + ' request_path=/parsed/from/message'
        + ' status_class=5xx'
        + ' outcome=failed'
        + ' duration_milliseconds=9999'
        + ' response_bytes=9999',
      timestamp_milliseconds: Date.now() + 3000,
      provider_request: {
        providerKey: GEOCODING_DESCRIPTOR.provider_key,
        providerContract: 'GeocodingProvider',
        httpMethod: 'GET',
        requestPath: '/typed/trace/path',
        statusClass: '2xx',
        outcome: 'success',
        durationMilliseconds: 42,
        responseBytes: 7,
        responseExcerpt: null,
      },
    }),
  ]);

  await open_console(page);
  const only_row = page.locator(CONSOLE_ROWS_SELECTOR).first();
  await expect(only_row).toContainText(GEOCODING_DESCRIPTOR.display_name);
  await expect(only_row).toContainText('GeocodingProvider');
  await expect(only_row).not.toContainText('MessageLineContract');
  await only_row.locator('button').click();
  await expect(page.locator(CONSOLE_DETAIL_SELECTOR)).toBeVisible();

  await expect(page.locator('#provider-console-detail-provider-key'))
    .toHaveText(GEOCODING_DESCRIPTOR.provider_key);
  await expect(page.locator('#provider-console-detail-contract'))
    .toHaveText('GeocodingProvider');
  await expect(page.locator('#provider-console-detail-method')).toHaveText('GET');
  await expect(page.locator('#provider-console-detail-path'))
    .toHaveText('/typed/trace/path');
  await expect(page.locator('#provider-console-detail-status')).toHaveText('2xx');
  await expect(page.locator('#provider-console-detail-duration')).toHaveText('42 ms');
  await expect(page.locator('#provider-console-detail-outcome')).toHaveText('success');
  await expect(page.locator('#provider-console-detail-bytes')).toHaveText('7');
  for (const misleading_value of [
    'message-line-provider',
    'MessageLineContract',
    'POST',
    '/parsed/from/message',
    '5xx',
    '9999',
  ]) {
    await expect(page.locator('#provider-console-detail-provider-fields'))
      .not.toContainText(misleading_value);
  }
});

test('the detail cache rows match the live statistics and status readbacks', async ({
  page,
}) => {
  await page.goto('/');
  await waitForMapTestReady(page);
  await open_console(page);

  await page.evaluate(
    ([contract, provider_key, fixture_base_path, credential]) => {
      window.mapTest.setProviderBaseUrl(
        provider_key,
        `${window.location.origin}${fixture_base_path}/${provider_key}`,
      );
      window.mapTest.configureProvider(contract, provider_key, credential);
    },
    [
      'traffic_flow',
      TRAFFIC_FLOW_DESCRIPTOR.provider_key,
      PROVIDERS_REGISTRY.fixture_base_path,
      CONSOLE_CREDENTIAL,
    ],
  );
  await page.evaluate(() =>
    window.mapTest.fetchTrafficFlow(40.7, -74.05, 40.8, -73.95));

  const provider_rows = page
    .locator(CONSOLE_ROWS_SELECTOR)
    .filter({ hasText: TRAFFIC_FLOW_DESCRIPTOR.display_name });
  await expect(provider_rows.first()).toBeVisible();
  await provider_rows.first().locator('button').click();
  await expect(page.locator(CONSOLE_DETAIL_SELECTOR)).toBeVisible();

  // The detail refreshes on the console's cadence; wait until both cache rows
  // agree with the same live reads the test makes.
  await expect
    .poll(
      () => page.evaluate(() => {
        const tile_text = document.getElementById(
          'provider-console-detail-tile-cache').textContent;
        const traffic_text = document.getElementById(
          'provider-console-detail-traffic-cache').textContent;
        const traffic = window.mapTest.trafficFlowCacheStatistics?.() ?? null;
        const vector_status =
          window.mapGpuCanvas()?.vectorTileSourceStatus?.() ?? null;
        const frame = window.mapLastFrameStats();
        if (traffic === null) {
          return false;
        }
        const traffic_matches = traffic_text.includes(`hits ${traffic.hitCount}`)
          && traffic_text.includes(`misses ${traffic.missCount}`);
        if (vector_status?.isConfigured === true
          && Number.isFinite(vector_status.payloadCacheHitCount)
          && Number.isFinite(vector_status.payloadCacheMissCount)) {
          return traffic_matches
            && tile_text.includes(`hits ${vector_status.payloadCacheHitCount}`)
            && tile_text.includes(`misses ${vector_status.payloadCacheMissCount}`);
        }
        if (frame !== null) {
          return traffic_matches
            && tile_text.includes(`requested ${frame.requestedTiles}`)
            && tile_text.includes(`drawn ${frame.drawnTiles}`)
            && tile_text.includes(`missing ${frame.missingTiles}`);
        }
        return traffic_matches && tile_text.includes('No GPU frame settled yet.');
      }),
      { timeout: 10_000 },
    )
    .toBe(true);

  const live_readbacks = await page.evaluate(() => {
    const vector_status = window.mapGpuCanvas()?.vectorTileSourceStatus?.() ?? null;
    return {
      traffic: window.mapTest.trafficFlowCacheStatistics?.() ?? null,
      frame: window.mapLastFrameStats(),
      vector_configured: vector_status?.isConfigured === true,
      vector_hit_count: vector_status?.payloadCacheHitCount ?? null,
      vector_miss_count: vector_status?.payloadCacheMissCount ?? null,
    };
  });
  expect(live_readbacks.traffic).not.toBeNull();
  const traffic_detail = page.locator('#provider-console-detail-traffic-cache');
  await expect(traffic_detail)
    .toContainText(`hits ${live_readbacks.traffic.hitCount}`);
  await expect(traffic_detail)
    .toContainText(`misses ${live_readbacks.traffic.missCount}`);
  await expect(traffic_detail)
    .toContainText(TRAFFIC_FLOW_DESCRIPTOR.display_name);
  const tile_detail = page.locator('#provider-console-detail-tile-cache');
  await expect(tile_detail).not.toHaveText('');
});

test('the export carries the filtered stream without a credential or token', async ({
  page,
}) => {
  await page.goto('/');
  await waitForMapTestReady(page);
  await open_console(page);
  await issue_geocoding_request(page);
  await expect(
    page.locator(CONSOLE_ROWS_SELECTOR)
      .filter({ hasText: GEOCODING_DESCRIPTOR.display_name })
      .first(),
  ).toBeVisible();

  const [download] = await Promise.all([
    page.waitForEvent('download'),
    page.locator('#provider-console-export').click(),
  ]);
  const exported_stream = JSON.parse(
    readFileSync(await download.path(), 'utf8'));
  expect(exported_stream.length).toBeGreaterThan(0);

  const provider_record = exported_stream.find(
    (record) => record.providerRequest?.providerKey
      === GEOCODING_DESCRIPTOR.provider_key,
  );
  expect(provider_record).toBeDefined();
  expect(provider_record.providerKey).toBe(GEOCODING_DESCRIPTOR.provider_key);
  expect(provider_record.providerContract).toBe('GeocodingProvider');
  expect(provider_record.severity).toBe('Info');

  const serialized_stream = JSON.stringify(exported_stream);
  expect(serialized_stream).not.toContain(CONSOLE_CREDENTIAL);
  // The stream is already redacted; an assignment still carrying a value is a
  // leak the export must never add.
  expect(serialized_stream).not.toMatch(
    /(?:api[_-]?key|apikey|access_token|token|signature|session)=(?!<redacted>|\*{3})[^&\s"<>]+/i,
  );
  expect(serialized_stream).not.toMatch(/Bearer\s+[A-Za-z0-9._-]{8,}/);
});

test('the export and clipboard scrub the applied credential and query values', async ({
  page,
}) => {
  await page.goto('/?useProviderFixtures');
  await waitForMapTestReady(page);
  // A synthetic value, applied through the settings surface, so the export
  // scrub's own applied-value pass is what removes it.
  const synthetic_credential = 'test-session-key';
  await apply_synthetic_credential(page, synthetic_credential);

  // A crafted event deliberately carries the value in its message, request
  // path and excerpt, in the credential-shaped forms the native consoles
  // scrub. The ring's own redaction is bypassed by this injection.
  const crafted_event = recorded_event({
    severity: 'Info',
    message_line: `GET /route?key=${synthetic_credential}`
      + `&apiKey=${synthetic_credential}`
      + ` access_token=${synthetic_credential}`,
    timestamp_milliseconds: Date.now() + 5000,
    provider_request: {
      providerKey: GEOCODING_DESCRIPTOR.provider_key,
      providerContract: 'GeocodingProvider',
      httpMethod: 'GET',
      requestPath: `/geocoding/v1/address?key=${synthetic_credential}`,
      statusClass: 'Success',
      outcome: 'Success',
      durationMilliseconds: 7,
      responseExcerpt: `{"apiKey": "${synthetic_credential}"}`,
    },
  });
  await inject_recorded_events(page, [crafted_event]);
  await open_console(page);
  await expect(page.locator(CONSOLE_ROWS_SELECTOR)).toHaveCount(1);

  const [download] = await Promise.all([
    page.waitForEvent('download'),
    page.locator('#provider-console-export').click(),
  ]);
  const exported_text = readFileSync(await download.path(), 'utf8');
  expect(exported_text).not.toContain(synthetic_credential);
  expect(exported_text).toContain('<redacted>');

  await capture_clipboard_text(page);
  await page.locator('#provider-console-copy').click();
  const copied_text = await page.evaluate(
    () => window.__providerConsoleCopiedText);
  expect(copied_text).not.toBeNull();
  expect(copied_text).not.toContain(synthetic_credential);
  expect(copied_text).toContain('<redacted>');
});

test('the empty console names the missing registration honestly', async ({
  page,
}) => {
  await page.goto('/');
  await waitForMapTestReady(page);
  await open_console(page);

  await expect(page.locator(CONSOLE_ROWS_SELECTOR)).toHaveCount(0);
  const empty_state = page.locator('#provider-console-empty');
  await expect(empty_state).toBeVisible();
  await expect(empty_state).toContainText('No provider is registered');
  await expect(page.locator('#provider-console-registration-note')).toBeVisible();
  await expect(page.locator('#provider-console-level-filter')).toHaveValue('info');
});
