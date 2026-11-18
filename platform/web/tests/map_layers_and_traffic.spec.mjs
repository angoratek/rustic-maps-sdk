// The on-map layer control and the device-location affordances
// (docs/ReferenceAppUX.md §4.2): the panel's base-map rows, the
// traffic layer fetched from the committed flow fixture, the demo-scene
// switches, the keyboard contract, the remembered choices and the route
// fields' "My location" chips. Every provider fact comes from the shared
// registry and every payload is a committed fixture served same-origin, so no
// test touches the network.
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, test, waitForMapTestReady } from './network_guard.mjs';
import { HOST, PORT } from '../serve.mjs';

/** The origin the harness serves and the browser permission grants name. */
const HARNESS_ORIGIN = `http://${HOST}:${PORT}`;

import {
  DEVICE_LOCATION_STATUS_IDENTIFIER,
  MAP_LAYER_BASE_ROW_IDENTIFIER_PREFIX,
  MAP_LAYER_SCENE_ROW_IDENTIFIER_PREFIX,
  MAP_LAYER_STYLE_ROW_IDENTIFIER_PREFIX,
  MAP_LAYER_TRAFFIC_IDENTIFIER,
  RASTER_RETRY_STATUS_TEXT,
  TRAFFIC_REFRESH_DEBOUNCE_MILLISECONDS,
} from '../map_layers.mjs';

const TEST_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const REPOSITORY_ROOT = path.resolve(TEST_DIRECTORY, '..', '..', '..');
const PREFERENCES_STORAGE_KEY = 'rustic-maps-demo-preferences';

/** The settle budget for a renderer-bound assertion. */
const RENDERER_READINESS_TIMEOUT_MILLISECONDS = 20_000;
/** The traffic debounce's own budget plus margin. */
const TRAFFIC_SETTLE_TIMEOUT_MILLISECONDS =
  TRAFFIC_REFRESH_DEBOUNCE_MILLISECONDS + 10_000;
/** The stubbed device position, and its accuracy in meters. */
const DEVICE_LATITUDE_DEGREES = 38.5766;
const DEVICE_LONGITUDE_DEGREES = -121.4944;
const DEVICE_ACCURACY_METERS = 5;

const providers_registry = JSON.parse(
  readFileSync(path.join(REPOSITORY_ROOT, 'platform', 'providers.json'), 'utf8'),
);
const reference_scenes = JSON.parse(
  readFileSync(path.join(REPOSITORY_ROOT, 'platform', 'reference-scenes.json'), 'utf8'),
);

/** The first raster descriptor a base row is built from. */
const raster_base_descriptor = providers_registry.providers.find(
  (provider_descriptor) => (provider_descriptor.fixture_routes ?? []).some(
    (fixture_route) => fixture_route.purpose === 'raster_tiles'));

/** A vector descriptor offering committed styles and a vector-tile fixture. */
const vector_descriptor = providers_registry.providers.find(
  (provider_descriptor) =>
    typeof provider_descriptor.url_template === 'string'
    && (provider_descriptor.vector_styles ?? []).length > 0
    && (provider_descriptor.fixture_routes ?? []).some(
      (fixture_route) => fixture_route.purpose === 'vector_tiles'));
const VECTOR_STYLE = vector_descriptor.vector_styles[0];
/**
 * Every credentialed vector family that publishes its style document, with
 * the committed tile fixture its own route serves. Each one gets the same
 * browser case, so a newly declared family is covered without a test edit.
 */
const hosted_vector_cases = providers_registry.providers.flatMap(
  (provider_descriptor) => {
    const style = (provider_descriptor.vector_styles ?? []).find(
      (style_choice) => typeof style_choice.document_url === 'string');
    const tile_route = (provider_descriptor.fixture_routes ?? []).find(
      (fixture_route) => fixture_route.purpose === 'vector_tiles');
    return style === undefined || tile_route === undefined
      ? []
      : [{ provider_descriptor, style, tile_route }];
  },
);
const VECTOR_TILE_FIXTURE = readFileSync(path.join(
  REPOSITORY_ROOT,
  vector_descriptor.fixture_routes.find(
    (fixture_route) => fixture_route.purpose === 'vector_tiles').fixture,
));

/** The descriptor that declares the traffic-flow capability. */
const traffic_descriptor = providers_registry.providers.find(
  (provider_descriptor) =>
    (provider_descriptor.served_contracts ?? []).includes('traffic_flow'));
const TRAFFIC_REQUEST_PATH =
  `${providers_registry.fixture_base_path}/${traffic_descriptor.provider_key}`;

/** The first demo scene key and its label (the switch's row). */
const [SCENE_KEY, SCENE] = Object.entries(reference_scenes.overlay_scenes)[0];
const SCENE_LABEL = SCENE.label;

/** Answers vector-tile requests from the committed fixture. */
async function serve_vector_tiles(page) {
  const serves_protobuf = vector_descriptor.url_template.endsWith('.pbf');
  const content_type = serves_protobuf
    ? 'application/x-protobuf'
    : 'application/vnd.mapbox-vector-tile';
  await page.route(serves_protobuf ? '**/*.pbf' : '**/*.mvt', (route) =>
    route.fulfill({
      status: 200,
      contentType: content_type,
      body: VECTOR_TILE_FIXTURE,
    }));
}

/** Applies the traffic provider's credential through the page's own seam, so
 * the wasm registry serves the capability from the same-origin fixtures. */
async function apply_traffic_credential(page) {
  await page.locator('#settings-control').click();
  const credential_row = page.locator(
    `[data-provider-key="${traffic_descriptor.provider_key}"]`);
  await credential_row.locator('[data-provider-credential-input]').fill('test-key');
  await credential_row.locator('[data-provider-credential-apply]').click();
  await expect(
    credential_row.locator('.provider-applied-chip'),
  ).toBeVisible();
  await page.keyboard.press('Escape');
}

/** Waits until the renderer's mode is known (`gpu` or the 2D fallback). */
async function wait_for_renderer(page) {
  await expect
    .poll(() => page.evaluate(() => (typeof window.mapRendererMode === 'function' ? window.mapRendererMode() : null)), {
      timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS,
    })
    .not.toBeNull();
  return page.evaluate(() => (typeof window.mapRendererMode === 'function' ? window.mapRendererMode() : null));
}

/** Opens the map layers panel. */
async function open_map_layers_panel(page) {
  await page.locator('#map-layers-control').click();
  await expect(page.locator('#map-layers-panel')).toBeVisible();
}

/** The stored demo-preferences record. */
async function stored_preferences(page) {
  return page.evaluate((storage_key) => {
    const stored_text = window.localStorage.getItem(storage_key);
    return stored_text === null ? null : JSON.parse(stored_text);
  }, PREFERENCES_STORAGE_KEY);
}

test('the panel lists the sections in order and switches raster to a vector style', async ({
  page,
}) => {
  test.setTimeout(120_000);
  await serve_vector_tiles(page);
  await page.goto('/?advanced&useProviderFixtures');
  await waitForMapTestReady(page);
  const renderer_mode = await wait_for_renderer(page);
  if (renderer_mode !== 'gpu') {
    test.skip(true, 'headless WebGPU unavailable; the vector base needs the GPU renderer');
    return;
  }

  await open_map_layers_panel(page);
  const section_titles = await page
    .locator('#map-layers-panel > .map-layers-section > h3')
    .allTextContents();
  expect(section_titles.slice(0, 3)).toEqual(['Base map', 'Traffic', 'Overlays']);
  // The attribution notice is the live region a layer change is announced in.
  await expect(page.locator('#attribution-notice')).toHaveAttribute(
    'aria-live', 'polite');

  // The default raster row is the one serving.
  const raster_row = page.locator(
    `#${MAP_LAYER_BASE_ROW_IDENTIFIER_PREFIX}${raster_base_descriptor.provider_key}`);
  await expect(raster_row).toHaveAttribute('aria-pressed', 'true');
  await expect(raster_row).toContainText('Serving');

  // Select one committed vector style: the same switch the settings picker
  // performs, with the style applied.
  const style_row = page.locator(
    `#${MAP_LAYER_STYLE_ROW_IDENTIFIER_PREFIX}${VECTOR_STYLE.style_id}`);
  // The row names the provider and the style, the composition every provider
  // row uses; a bare style name would not say where the base comes from.
  await expect(style_row).toHaveText(
    `${vector_descriptor.display_name} · ${VECTOR_STYLE.display_name}`,
  );
  await style_row.click();
  await expect
    .poll(() => page.evaluate(() => window.mapTileMode()), {
      timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS,
    })
    .toBe('vector');
  expect(await page.evaluate(() => window.mapVectorProviderKey()))
    .toBe(vector_descriptor.provider_key);
  await expect(style_row).toHaveAttribute('aria-pressed', 'true');
  await expect(style_row).toContainText('Serving');
  await expect(raster_row).toHaveAttribute('aria-pressed', 'false');

  // The choice survives a reload.
  await page.reload();
  await waitForMapTestReady(page);
  await expect
    .poll(() => page.evaluate(() => window.mapTileMode()), {
      timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS,
    })
    .toBe('vector');
  expect(await page.evaluate(() => window.mapVectorProviderKey()))
    .toBe(vector_descriptor.provider_key);
  await open_map_layers_panel(page);
  await expect(page.locator(
    `#${MAP_LAYER_STYLE_ROW_IDENTIFIER_PREFIX}${VECTOR_STYLE.style_id}`,
  )).toHaveAttribute('aria-pressed', 'true');
});

test('the console shows the vector base payload cache counters', async ({
  page,
}) => {
  test.setTimeout(120_000);
  await serve_vector_tiles(page);
  await page.goto('/?advanced&useProviderFixtures');
  await waitForMapTestReady(page);
  const renderer_mode = await wait_for_renderer(page);
  if (renderer_mode !== 'gpu') {
    test.skip(true, 'headless WebGPU unavailable; the vector base needs the GPU renderer');
    return;
  }

  // Install the keyless vector base; its tiles come from the committed
  // fixture the route above serves same-origin.
  await open_map_layers_panel(page);
  const style_row = page.locator(
    `#${MAP_LAYER_STYLE_ROW_IDENTIFIER_PREFIX}${VECTOR_STYLE.style_id}`);
  const raster_row = page.locator(
    `#${MAP_LAYER_BASE_ROW_IDENTIFIER_PREFIX}${raster_base_descriptor.provider_key}`);
  await expect(style_row).toBeEnabled();
  await style_row.click();
  await expect
    .poll(() => page.evaluate(() => {
      const status = window.mapGpuCanvas().vectorTileSourceStatus();
      return status.isConfigured && status.drawnTileCount > 0;
    }), { timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS })
    .toBe(true);

  // Return to raster and install the same style again. The new scheduler
  // starts with an empty decoded-tile cache but consults the canvas' payload
  // cache, which survives the switch, so the visible tiles resolve as hits.
  await raster_row.click();
  await expect
    .poll(() => page.evaluate(() => window.mapTileMode()), {
      timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS,
    })
    .toBe('raster');
  await expect(style_row).toBeEnabled();
  await style_row.click();
  await expect
    .poll(() => page.evaluate(() => {
      const status = window.mapGpuCanvas().vectorTileSourceStatus();
      return status.isConfigured
        && status.drawnTileCount > 0
        && (status.payloadCacheHitCount ?? 0) > 0;
    }), { timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS })
    .toBe(true);

  // Closing the layers panel leaves the provider console's control reachable.
  await page.keyboard.press('Escape');
  await expect(page.locator('#map-layers-panel')).toBeHidden();
  await page.locator('#provider-console-control').click();
  const tile_cache_counters =
    page.locator('#provider-console-tile-cache-counters');
  await expect(tile_cache_counters).toContainText(/payload cache hits \d+/);
  await expect(tile_cache_counters).toContainText(/payload cache misses \d+/);

  // The line renders the source status' own counters, so a read from the
  // wrong object would omit the line and fail above.
  const tile_cache_text = await tile_cache_counters.textContent();
  const panel_hit_count =
    Number(/payload cache hits (\d+)/.exec(tile_cache_text)?.[1]);
  const panel_miss_count =
    Number(/payload cache misses (\d+)/.exec(tile_cache_text)?.[1]);
  const payload_cache = await page.evaluate(() => {
    const status = window.mapGpuCanvas().vectorTileSourceStatus();
    return {
      hitCount: status.payloadCacheHitCount,
      missCount: status.payloadCacheMissCount,
    };
  });
  expect(panel_hit_count).toBeGreaterThan(0);
  expect(panel_miss_count).toBe(payload_cache.missCount);
  // A later same-pose frame can only add hits, never misses.
  expect(payload_cache.hitCount).toBeGreaterThanOrEqual(panel_hit_count);
});

test('switching a vector base back to raster clears its styled geometry', async ({
  page,
}) => {
  test.setTimeout(120_000);
  await serve_vector_tiles(page);
  await page.goto('/?advanced&useProviderFixtures');
  await waitForMapTestReady(page);
  const renderer_mode = await wait_for_renderer(page);
  if (renderer_mode !== 'gpu') {
    test.skip(true, 'headless WebGPU unavailable; the vector base needs the GPU renderer');
    return;
  }

  // The frame's overlay triangle count is what the renderer actually draws
  // over the base: the page's own overlays plus, while a vector base serves,
  // the style's fills and lines.
  const frame_overlay_triangles = () =>
    page.evaluate(() => window.mapLastFrameStats()?.overlayTriangles ?? 0);
  const raster_triangles = await frame_overlay_triangles();

  await open_map_layers_panel(page);
  const style_row = page.locator(
    `#${MAP_LAYER_STYLE_ROW_IDENTIFIER_PREFIX}${VECTOR_STYLE.style_id}`);
  await style_row.click();
  await expect
    .poll(frame_overlay_triangles, {
      timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS,
    })
    .toBeGreaterThan(raster_triangles);

  // Back to the raster base: the vector style's geometry must be gone from
  // the very next frames, not left painting over the tiles.
  const raster_row = page.locator(
    `#${MAP_LAYER_BASE_ROW_IDENTIFIER_PREFIX}${raster_base_descriptor.provider_key}`);
  await raster_row.click();
  await expect
    .poll(frame_overlay_triangles, {
      timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS,
    })
    .toBe(raster_triangles);
  // And it stays gone: a later frame carries the same geometry.
  await page.evaluate(() => new Promise((resolve) => {
    window.requestAnimationFrame(() => window.requestAnimationFrame(resolve));
  }));
  expect(await frame_overlay_triangles()).toBe(raster_triangles);
});

for (const hosted_vector_case of hosted_vector_cases) {
  const { provider_descriptor, style, tile_route } = hosted_vector_case;
  test(`a credentialed vector family waits for its key and paints its published style (${provider_descriptor.display_name})`, async ({
    page,
  }) => {
    test.setTimeout(120_000);
    // The provider's own recorded tile route addresses one tile; the demo
    // camera asks for its neighbours too, so every tile address under the
    // provider's fixture root answers with the family's committed bytes (the
    // document route itself keeps the fixture server's own answer). The match
    // is the registry's fixture root, never a vendor path shape.
    const hosted_vector_tile_fixture = readFileSync(
      path.join(REPOSITORY_ROOT, tile_route.fixture));
    const hosted_fixture_root =
      `${providers_registry.fixture_base_path}/${provider_descriptor.provider_key}`;
    const document_fixture_path =
      `${hosted_fixture_root}${new URL(style.document_url).pathname}`;
    await page.route(
      (candidate) => candidate.pathname.startsWith(`${hosted_fixture_root}/`)
        && candidate.pathname !== document_fixture_path,
      (route) => route.fulfill({
        status: 200,
        contentType: 'application/vnd.mapbox-vector-tile',
        body: hosted_vector_tile_fixture,
      }),
    );
    await page.goto('/?advanced&useProviderFixtures');
    await waitForMapTestReady(page);
    const renderer_mode = await wait_for_renderer(page);
    if (renderer_mode !== 'gpu') {
      test.skip(true, 'headless WebGPU unavailable; the vector base needs the GPU renderer');
      return;
    }

    // Without the provider's credential the family's row is present but
    // refused, exactly like the raster rows of a credentialed provider.
    await open_map_layers_panel(page);
    const style_row = page.locator(
      `#${MAP_LAYER_STYLE_ROW_IDENTIFIER_PREFIX}${style.style_id}`);
    await expect(style_row).toContainText(
      `${provider_descriptor.display_name} · ${style.display_name}`);
    await expect(style_row).toBeDisabled();
    await expect(style_row).toContainText(
      `Needs ${provider_descriptor.credential_label}`);

    // Applying the key through the settings surface registers the provider and
    // enables the row; the page rewrites the document and tile addresses to
    // the same-origin fixture routes in this mode. The layers panel closes
    // first, because it overlays the settings control.
    await page.keyboard.press('Escape');
    await expect(page.locator('#map-layers-panel')).toBeHidden();
    await page.locator('#settings-control').click();
    const credential_row = page.locator(
      `[data-provider-key="${provider_descriptor.provider_key}"]`);
    await credential_row.locator('[data-provider-credential-input]').fill('test-key');
    await credential_row.locator('[data-provider-credential-apply]').click();
    await expect(credential_row.locator('.provider-applied-chip')).toBeVisible();
    await page.keyboard.press('Escape');
    await open_map_layers_panel(page);
    await expect(style_row).toBeEnabled();

    // The install goes through the provider's authenticated client: the style
    // document comes from the committed synthetic fixture, and the base paints
    // its layers over the provider's tiles.
    await style_row.click();
    await expect
      .poll(() => page.evaluate(() => window.mapVectorBaseStatus()), {
        timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS,
      })
      .toContain('tiles drawn');
    const status_text = await page.evaluate(() => window.mapVectorBaseStatus());
    expect(status_text).toContain(provider_descriptor.display_name);
    expect(status_text).toContain(style.style_id);
    await expect(style_row).toHaveAttribute('aria-pressed', 'true');
  });
}

test('the traffic switch installs one overlay per settle and clears when off', async ({
  page,
}) => {
  test.setTimeout(120_000);
  await page.goto('/?advanced&useProviderFixtures');
  await waitForMapTestReady(page);
  await apply_traffic_credential(page);

  // Every traffic request must be the same-origin fixture route.
  const traffic_request_urls = [];
  page.on('request', (request) => {
    if (request.url().includes(TRAFFIC_REQUEST_PATH)) {
      traffic_request_urls.push(request.url());
    }
  });

  await open_map_layers_panel(page);
  const traffic_switch = page.locator(`#${MAP_LAYER_TRAFFIC_IDENTIFIER}`);
  await expect(traffic_switch).toHaveAttribute('role', 'switch');
  await expect(traffic_switch).toHaveAttribute('aria-checked', 'false');
  await traffic_switch.click();
  await expect(traffic_switch).toHaveAttribute('aria-checked', 'true');

  // One settle yields exactly one overlay install from the fixture.
  await expect
    .poll(
      () => page.evaluate(() => window.mapTrafficLayerState().overlayInstallCount),
      { timeout: TRAFFIC_SETTLE_TIMEOUT_MILLISECONDS },
    )
    .toBe(1);
  const first_state = await page.evaluate(() => window.mapTrafficLayerState());
  expect(first_state.segmentCount).toBeGreaterThan(0);
  expect(traffic_request_urls.length).toBe(1);
  expect(traffic_request_urls[0].startsWith(HARNESS_ORIGIN)).toBe(true);
  expect(traffic_request_urls[0]).toContain(`${TRAFFIC_REQUEST_PATH}/`);
  // While the layer is on, its serving descriptor's attribution is present.
  await expect(page.locator('#attribution-notice')).toContainText(
    traffic_descriptor.attribution_text);

  // The traffic cache is a real readback, not a page guess: the first settle
  // recorded one miss. Both the frame statistics and the provider console
  // show the same counters, so a reader can check one place against the other.
  const first_traffic_cache = await page.evaluate(
    () => window.mapLastFrameStats()?.trafficFlowCache ?? null,
  );
  expect(first_traffic_cache).toMatchObject({ hitCount: 0, missCount: 1 });
  expect(first_traffic_cache.entryCount).toBeGreaterThan(0);
  expect(first_traffic_cache.maximumEntryCount).toBeGreaterThan(0);
  expect(first_traffic_cache.timeToLiveSeconds).toBeGreaterThan(0);
  expect(first_traffic_cache.gridZoomLevel).toBeGreaterThan(0);

  // Closing the layers panel leaves the provider console's control reachable.
  await page.keyboard.press('Escape');
  await expect(page.locator('#map-layers-panel')).toBeHidden();
  await page.locator('#provider-console-control').click();
  const traffic_cache_counters =
    page.locator('#provider-console-traffic-cache-counters');
  await expect(traffic_cache_counters).toContainText(
    traffic_descriptor.display_name);
  await expect(traffic_cache_counters).toContainText('hits 0');
  await expect(traffic_cache_counters).toContainText('misses 1');
  // The tile pipeline's counters sit beside them in the same panel.
  await expect(page.locator('#provider-console-tile-cache-counters'))
    .toContainText('drawn ');

  // A small camera pan causes one more refresh after the debounce. It stays
  // inside the traffic cache's grid cell, so the page asks again but the
  // provider is not called again: the answer comes from memory.
  await page.locator('#map-overlay').focus();
  await page.keyboard.press('ArrowRight');
  await expect
    .poll(
      () => page.evaluate(() => window.mapTrafficLayerState().overlayInstallCount),
      { timeout: TRAFFIC_SETTLE_TIMEOUT_MILLISECONDS },
    )
    .toBe(2);
  expect(await page.evaluate(() => window.mapTrafficLayerState().fetchCount)).toBe(2);
  expect(traffic_request_urls.length).toBe(1);
  const cache_statistics = await page.evaluate(
    () => window.mapTest.trafficFlowCacheStatistics(),
  );
  expect(cache_statistics.hitCount).toBe(1);
  expect(cache_statistics.missCount).toBe(1);

  // The repeat request reads as one hit in the frame statistics and in the
  // panel, so both surfaces follow the same readback.
  const repeat_traffic_cache = await page.evaluate(
    () => window.mapLastFrameStats()?.trafficFlowCache ?? null,
  );
  expect(repeat_traffic_cache).toMatchObject({ hitCount: 1, missCount: 1 });
  expect(repeat_traffic_cache.entryCount).toBeGreaterThan(0);
  await expect(traffic_cache_counters).toContainText('hits 1');
  await expect(traffic_cache_counters).toContainText('misses 1');

  // Turning the layer off through the panel that owns the switch clears the
  // overlay; the console closes first so the layers control is reachable.
  await page.locator('#provider-console-close').click();
  await expect(page.locator('#provider-console-panel')).toBeHidden();
  await open_map_layers_panel(page);
  await traffic_switch.click();
  await expect(traffic_switch).toHaveAttribute('aria-checked', 'false');
  await expect
    .poll(() => page.evaluate(() => window.mapTrafficLayerState()))
    .toMatchObject({ enabled: false, segmentCount: 0 });
});

test('a session without a traffic provider reports the cache as unavailable', async ({
  page,
}) => {
  test.setTimeout(120_000);
  await page.goto('/?advanced&useProviderFixtures');
  await waitForMapTestReady(page);
  // The frame statistics only exist once a GPU frame settled.
  await expect
    .poll(
      () => page.evaluate(() => window.mapLastFrameStats() !== null),
      { timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS },
    )
    .toBe(true);

  // The frame statistics carry an explicit null rather than fabricated zeros.
  const frame_statistics = await page.evaluate(() => window.mapLastFrameStats());
  expect(frame_statistics.trafficFlowCache).toBeNull();

  // The console states the same absence in words, with no counter line.
  await page.locator('#provider-console-control').click();
  const traffic_cache_counters =
    page.locator('#provider-console-traffic-cache-counters');
  await expect(traffic_cache_counters).toContainText('Unavailable');
  await expect(traffic_cache_counters).not.toContainText('hits ');
});

test('a remembered traffic choice restores only while a provider serves it', async ({
  page,
}) => {
  test.setTimeout(120_000);
  await page.goto('/?advanced&useProviderFixtures');
  await waitForMapTestReady(page);

  // Remember the key so the reload's registration path can restore the layer.
  await page.locator('#settings-control').click();
  await page.locator('#remember-provider-credential').check();
  const credential_row = page.locator(
    `[data-provider-key="${traffic_descriptor.provider_key}"]`);
  await credential_row.locator('[data-provider-credential-input]').fill('test-key');
  await credential_row.locator('[data-provider-credential-apply]').click();
  await expect(credential_row.locator('.provider-applied-chip')).toBeVisible();
  await page.keyboard.press('Escape');

  await open_map_layers_panel(page);
  await page.locator(`#${MAP_LAYER_TRAFFIC_IDENTIFIER}`).click();
  await expect
    .poll(
      () => page.evaluate(() => window.mapTrafficLayerState().enabled),
      { timeout: TRAFFIC_SETTLE_TIMEOUT_MILLISECONDS },
    )
    .toBe(true);
  expect((await stored_preferences(page)).trafficLayerEnabled).toBe(true);

  // The reload re-applies the remembered key and then the layer.
  await page.reload();
  await waitForMapTestReady(page);
  await expect
    .poll(
      () => page.evaluate(() => window.mapTrafficLayerState().enabled),
      { timeout: TRAFFIC_SETTLE_TIMEOUT_MILLISECONDS },
    )
    .toBe(true);
  await expect
    .poll(
      () => page.evaluate(() => window.mapTrafficLayerState().overlayInstallCount),
      { timeout: TRAFFIC_SETTLE_TIMEOUT_MILLISECONDS },
    )
    .toBeGreaterThan(0);

  // Clearing the remembered key removes the provider: the next visit starts
  // with traffic off instead of pretending the layer can be served.
  await page.locator('#settings-control').click();
  await credential_row.locator('[data-provider-credential-clear]').click();
  await page.keyboard.press('Escape');
  expect(await page.evaluate(() => window.mapTrafficLayerState().enabled)).toBe(false);
  await page.reload();
  await waitForMapTestReady(page);
  await open_map_layers_panel(page);
  await expect
    .poll(() => page.evaluate(() => window.mapTrafficLayerState().enabled))
    .toBe(false);
  await expect(page.locator(`#${MAP_LAYER_TRAFFIC_IDENTIFIER}`)).toContainText(
    `Needs ${traffic_descriptor.credential_label}`);
});

test('the traffic switch reads Needs <credential> while unconfigured', async ({
  page,
}) => {
  await page.goto('/?advanced&useProviderFixtures');
  await waitForMapTestReady(page);
  await open_map_layers_panel(page);
  const traffic_switch = page.locator(`#${MAP_LAYER_TRAFFIC_IDENTIFIER}`);
  if (traffic_descriptor.credential_label.trim() === '') {
    test.skip(true, 'the registry declares a keyless traffic provider');
    return;
  }
  await expect(traffic_switch).toBeDisabled();
  await expect(traffic_switch).toContainText(
    `Needs ${traffic_descriptor.credential_label}`);
});

test('the traffic switch reads Not configured when no descriptor declares it', async ({
  page,
}) => {
  // A registry without the capability is served same-origin by this route, so
  // the page must not pretend a layer exists.
  const registry_without_traffic = {
    ...providers_registry,
    providers: providers_registry.providers.map((provider_descriptor) => ({
      ...provider_descriptor,
      served_contracts: (provider_descriptor.served_contracts ?? [])
        .filter((capability_name) => capability_name !== 'traffic_flow'),
    })),
  };
  await page.route('**/providers.json', (route) =>
    route.fulfill({ json: registry_without_traffic }));
  await page.goto('/?advanced&useProviderFixtures');
  await waitForMapTestReady(page);
  await open_map_layers_panel(page);
  const traffic_switch = page.locator(`#${MAP_LAYER_TRAFFIC_IDENTIFIER}`);
  await expect(traffic_switch).toBeDisabled();
  await expect(traffic_switch).toContainText('Not configured');
});

test('a scene switch records its remembered choice', async ({
  page,
}) => {
  await page.goto('/?advanced&useProviderFixtures');
  await waitForMapTestReady(page);
  await open_map_layers_panel(page);

  const scene_switch = page.locator(
    `#${MAP_LAYER_SCENE_ROW_IDENTIFIER_PREFIX}${SCENE_KEY}`);
  await expect(scene_switch).toContainText(SCENE_LABEL);
  await scene_switch.click();
  await expect(scene_switch).toHaveAttribute('aria-checked', 'true');
  expect((await stored_preferences(page)).overlaySceneKeys).toContain(SCENE_KEY);

  // The switch is the scene's single control.
  await scene_switch.click();
  await expect(scene_switch).toHaveAttribute('aria-checked', 'false');
  expect((await stored_preferences(page)).overlaySceneKeys).not.toContain(SCENE_KEY);
});

test('the clear action unchecks every scene and records the empty selection', async ({
  page,
}) => {
  await page.goto('/?advanced&useProviderFixtures');
  await waitForMapTestReady(page);
  await open_map_layers_panel(page);

  const scene_switch = page.locator(
    `#${MAP_LAYER_SCENE_ROW_IDENTIFIER_PREFIX}${SCENE_KEY}`);
  await scene_switch.click();
  await expect(scene_switch).toHaveAttribute('aria-checked', 'true');

  // The shared layers-panel clear action: every scene switch returns to
  // unchecked and the remembered selection becomes empty, exactly as
  // untoggling each scene by hand would.
  await page.locator('#map-layers-clear-scenes').click();
  await expect(scene_switch).toHaveAttribute('aria-checked', 'false');
  expect((await stored_preferences(page)).overlaySceneKeys).toEqual([]);
});

test('the panel is keyboard operable and Escape returns focus to the button', async ({
  page,
}) => {
  await page.goto('/?advanced&useProviderFixtures');
  await waitForMapTestReady(page);
  // The vector rows become focusable once the GPU renderer answers; before
  // that only the raster row is enabled and one Tab would leave the panel.
  await expect
    .poll(() => page.evaluate(() => window.mapRendererMode()), { timeout: 25_000 })
    .toBe('gpu');

  const layers_control = page.locator('#map-layers-control');
  await layers_control.focus();
  await page.keyboard.press('Enter');
  await expect(layers_control).toHaveAttribute('aria-expanded', 'true');
  await expect(page.locator('#map-layers-panel')).toBeVisible();

  // Tab moves through the panel's rows and switches, all real buttons.
  await page.keyboard.press('Tab');
  expect(await page.evaluate(
    () => document.getElementById('map-layers-panel').contains(document.activeElement),
  )).toBe(true);

  // Escape closes the panel and returns focus to the collapsed control.
  await page.keyboard.press('Escape');
  await expect(layers_control).toHaveAttribute('aria-expanded', 'false');
  await expect(page.locator('#map-layers-panel')).toBeHidden();
  await expect(layers_control).toBeFocused();

  // A row is activatable from the keyboard.
  await page.keyboard.press('Enter');
  const raster_row = page.locator(
    `#${MAP_LAYER_BASE_ROW_IDENTIFIER_PREFIX}${raster_base_descriptor.provider_key}`);
  await raster_row.focus();
  await page.keyboard.press('Enter');
  await expect(raster_row).toHaveAttribute('aria-pressed', 'true');
});

test('each My location chip fills only its own route field', async ({
  page,
  context,
}) => {
  await context.grantPermissions(['geolocation'], {
    origin: HARNESS_ORIGIN,
  });
  await context.setGeolocation({
    latitude: DEVICE_LATITUDE_DEGREES,
    longitude: DEVICE_LONGITUDE_DEGREES,
    accuracy: DEVICE_ACCURACY_METERS,
  });
  await page.goto('/?advanced&useProviderFixtures');
  await waitForMapTestReady(page);

  const origin_chip = page.locator('#origin-my-location');
  const destination_chip = page.locator('#destination-my-location');
  await expect(origin_chip).toBeEnabled();
  await expect(origin_chip).toHaveAttribute(
    'aria-label', 'Use my location as the origin');
  await expect(destination_chip).toHaveAttribute(
    'aria-label', 'Use my location as the destination');

  // The origin chip starts the source and fills only the origin.
  await origin_chip.click();
  await expect(page.locator('#trip-origin-input')).toHaveValue(
    /38\.57660, -121\.49440/, { timeout: 20_000 });
  await expect(page.locator('#trip-destination-input')).toHaveValue('');

  // The destination chip reuses the running source's latest fix.
  await destination_chip.click();
  await expect(page.locator('#trip-destination-input')).toHaveValue(
    /38\.57660, -121\.49440/);
  await expect(page.locator('#trip-origin-input')).toHaveValue(
    /38\.57660, -121\.49440/);

  // The panel's Device location row reports the running source.
  await open_map_layers_panel(page);
  await expect(page.locator(`#${DEVICE_LOCATION_STATUS_IDENTIFIER}`)).toHaveText(
    'Active');
});

test('a raster frame with requested tiles and none drawn reports retry on paper', async ({
  page,
}) => {
  test.setTimeout(120_000);
  await page.goto('/?advanced&useProviderFixtures');
  await waitForMapTestReady(page);
  const renderer_mode = await wait_for_renderer(page);
  if (renderer_mode !== 'gpu') {
    test.skip(true, 'headless WebGPU unavailable; the retry state reads frame statistics');
    return;
  }
  // Warm the pool at the recorded camera: the demo's committed fixtures draw.
  await expect
    .poll(() => page.evaluate(() => window.mapLastFrameStats()?.drawnTiles ?? 0), {
      timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS,
    })
    .toBeGreaterThan(0);

  // Deny every recorded tile and leave the fixture coverage: the next settled
  // frame requests tiles and draws none, without any thrown render frame.
  await page.route('**/tiles/**', (route) => route.fulfill({
    status: 404,
    contentType: 'text/plain',
    body: '',
  }));
  await page.evaluate(() => {
    const zoom_slider = document.getElementById('zoom-control');
    zoom_slider.value = '3';
    zoom_slider.dispatchEvent(new Event('input', { bubbles: true }));
  });
  await expect
    .poll(() => page.evaluate(() => {
      const statistics = window.mapLastFrameStats();
      return (statistics?.requestedTiles ?? 0) > 0
        && (statistics?.drawnTiles ?? 0) === 0;
    }), { timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS })
    .toBe(true);
  // The base-map line surfaces the retry state instead of staying silent.
  await expect(page.locator('#map-layers-base-status')).toHaveText(
    RASTER_RETRY_STATUS_TEXT);

  // The retrying frame paints neutral paper, not the raster base's black.
  const frames_before_snapshot = await page.evaluate(() => window.mapFramesDrawn());
  await page.evaluate(() => window.mapRequestRenderForTest());
  await expect
    .poll(() => page.evaluate(() => window.mapFramesDrawn()), {
      timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS,
    })
    .toBeGreaterThan(frames_before_snapshot);
  // Let the queued frame finish so the snapshot replays its paper clear.
  await page.waitForTimeout(250);
  const corner_pixel = await page.evaluate(async () => {
    const pixels = await window.mapGpuCanvas().snapshot();
    return Array.from(pixels.slice(0, 4));
  });
  const [red, green, blue] = corner_pixel;
  expect(red, 'the retry background must read as paper, not black').toBeGreaterThan(128);
  expect(green).toBeGreaterThan(128);
  expect(blue).toBeGreaterThan(128);
});
