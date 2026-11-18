// The demo's raster ⇄ vector base-map switch (M9 live-tile integration): the
// page hides the raster layer, decodes the provider-declared vector tiles and
// paints them through the overlay layer, then switches back. Every provider
// fact comes from the shared registry and every tile payload is the committed
// fixture, intercepted by the test, so no external network is touched.
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, test, waitForMapTestReady } from './network_guard.mjs';

/** Closes the layers panel (Escape, the panel's own dismiss). */
async function close_map_layers_panel(page) {
  const layers_panel = page.locator('#map-layers-panel');
  if (await layers_panel.isVisible()) {
    await page.keyboard.press('Escape');
  }
  await expect(layers_panel).toBeHidden();
}

/** Opens the layers panel, the base-map flow's single home on every target. */
async function open_map_layers_panel(page) {
  const layers_panel = page.locator('#map-layers-panel');
  if (!(await layers_panel.isVisible())) {
    await page.locator('#map-layers-control').click();
  }
  await expect(layers_panel).toBeVisible();
}

/** The first row for one base mode, in the panel's registry order. */
function base_row(page, tile_mode) {
  return page.locator(
    `#map-layer-base-rows button[data-tile-mode="${tile_mode}"]`).first();
}

const TEST_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const REPOSITORY_ROOT = path.resolve(TEST_DIRECTORY, '..', '..', '..');

/** The shared provider registry the page and the assertions read. */
const providers_registry = JSON.parse(
  readFileSync(path.join(REPOSITORY_ROOT, 'platform', 'providers.json'), 'utf8'),
);

/** The first URL-template descriptor that declares a vector-tile fixture route. */
const vector_provider_descriptor = providers_registry.providers.find(
  (provider_descriptor) => typeof provider_descriptor.url_template === 'string'
    && provider_descriptor.fixture_routes.some(
      (fixture_route) => fixture_route.purpose === 'vector_tiles'),
);

/** The recorded vector tile every intercepted request answers with. */
const recorded_vector_tile_bytes = readFileSync(path.join(
  REPOSITORY_ROOT,
  vector_provider_descriptor.fixture_routes.find(
    (fixture_route) => fixture_route.purpose === 'vector_tiles',
  ).fixture,
));

/** Every provider that can back a vector base layer. */
const vector_provider_descriptors = providers_registry.providers.filter(
  (provider_descriptor) => typeof provider_descriptor.url_template === 'string',
);

/** The second vector provider and its recorded tile (the style proof). */
const second_vector_provider_descriptor = vector_provider_descriptors.find(
  (provider_descriptor) => provider_descriptor !== vector_provider_descriptor,
);
const second_vector_fixture_route = second_vector_provider_descriptor.fixture_routes.find(
  (fixture_route) => fixture_route.purpose === 'vector_tiles',
);
const recorded_second_vector_tile_bytes = readFileSync(path.join(
  REPOSITORY_ROOT, second_vector_fixture_route.fixture,
));

test('the base-map switch hides the raster layer and paints the decoded vector base', async ({
  page,
}) => {
  // Intercept every vector-tile request and answer with the committed
  // fixture: the page's own URL template is exercised, the network is not.
  await page.route('**/*.mvt', (route) => route.fulfill({
    status: 200,
    contentType: 'application/vnd.mapbox-vector-tile',
    body: recorded_vector_tile_bytes,
  }));
  await page.goto('/?advanced&useProviderFixtures');
  await waitForMapTestReady(page);

  await open_map_layers_panel(page);
  const raster_row = base_row(page, 'raster');
  const first_vector_row = base_row(page, 'vector');
  // One raster row plus one row per committed vector style; the registry
  // decides how many styles exist.
  await expect(raster_row).toContainText('Raster');
  await expect(first_vector_row).toContainText(
    vector_provider_descriptor.display_name);
  await expect(raster_row).toHaveAttribute('aria-pressed', 'true');

  // Let the raster layer settle at least one drawn tile, then switch.
  await expect
    .poll(async () => page.evaluate(
      () => window.mapLastFrameStats()?.drawnTiles ?? 0), { timeout: 15000 })
    .toBeGreaterThan(0);

  await first_vector_row.click();
  await expect(first_vector_row).toHaveAttribute('aria-pressed', 'true');
  expect(await page.evaluate(() => window.mapTileMode())).toBe('vector');

  // The vector base paints decoded overlays while the raster layer reports
  // no drawn tiles; the overlay triangles prove the decoded geometry reached
  // the GPU frame.
  await expect
    .poll(async () => page.evaluate(() => window.mapVectorBaseStatus()),
      { timeout: 20000 })
    .toContain('tiles drawn');
  await expect
    .poll(async () => page.evaluate(
      () => (window.mapLastFrameStats()?.overlayTriangles ?? 0) > 0),
    { timeout: 20000 })
    .toBe(true);
  expect(await page.evaluate(
    () => window.mapLastFrameStats()?.drawnTiles ?? -1)).toBe(0);

  // Switching back restores the raster layer and the scene overlays.
  await raster_row.click();
  await expect(raster_row).toHaveAttribute('aria-pressed', 'true');
  await expect
    .poll(async () => page.evaluate(
      () => window.mapLastFrameStats()?.drawnTiles ?? 0), { timeout: 15000 })
    .toBeGreaterThan(0);
  expect(await page.evaluate(() => window.mapTileMode())).toBe('raster');
  await close_map_layers_panel(page);
});

test('a second vector provider paints with its own committed style', async ({
  page,
}) => {
  // The second provider's tiles are served by interception with its committed
  // fixture, so the browser never touches the external tile service.
  await page.route('**/*.pbf', (route) => route.fulfill({
    status: 200,
    contentType: 'application/vnd.mapbox-vector-tile',
    body: recorded_second_vector_tile_bytes,
  }));
  await page.goto('/?advanced');
  await waitForMapTestReady(page);

  await open_map_layers_panel(page);
  const second_provider_button = page
    .locator('#map-layer-base-rows button[data-tile-mode="vector"]')
    .filter({ hasText: second_vector_provider_descriptor.display_name })
    .first();
  await expect(second_provider_button).toHaveCount(1);
  await second_provider_button.click();
  await expect(second_provider_button).toHaveAttribute('aria-pressed', 'true');
  expect(await page.evaluate(() => window.mapTileMode())).toBe('vector');
  expect(await page.evaluate(() => window.mapVectorProviderKey())).toBe(
    second_vector_provider_descriptor.provider_key,
  );

  // Its own schema's style paints filled, lined and pointed features.
  await expect
    .poll(() => page.evaluate(() => window.mapVectorBaseStatus()), { timeout: 20000 })
    .toContain('tiles drawn');
  await expect
    .poll(() => page.evaluate(() => window.mapVectorBaseStatus()), { timeout: 20000 })
    .toContain(second_vector_provider_descriptor.display_name);
  await expect
    .poll(() => page.evaluate(
      () => (window.mapLastFrameStats()?.overlayTriangles ?? 0) > 0),
    { timeout: 20000 })
    .toBe(true);

  // Switching back to the first provider repaints that provider's schema.
  const first_provider_button = page
    .locator('#map-layer-base-rows button[data-tile-mode="vector"]')
    .filter({ hasText: vector_provider_descriptor.display_name })
    .first();
  await first_provider_button.click();
  await expect(first_provider_button).toHaveAttribute('aria-pressed', 'true');
  expect(await page.evaluate(() => window.mapVectorProviderKey())).toBe(
    vector_provider_descriptor.provider_key,
  );
});

test('the layers panel lists every committed style and a pick repaints the base', async ({
  page,
}) => {
  // The base map, its styles and its scene overlays live only in the floating
  // layers control; the panel offers one row per committed vector style.
  // The descriptor offering the named Bright/Fiord/Positron family (a second
  // vector descriptor now offers its own Carto styles).
  const styled_descriptor = vector_provider_descriptors.find(
    (provider_descriptor) => Array.isArray(provider_descriptor.vector_styles)
      && provider_descriptor.vector_styles.some(
        (style_choice) => style_choice.style_id === 'fiord-dark',
      ),
  );
  const styled_fixture_route = styled_descriptor.fixture_routes.find(
    (fixture_route) => fixture_route.purpose === 'vector_tiles',
  );
  await page.route('**/*.pbf', (route) => route.fulfill({
    status: 200,
    contentType: 'application/vnd.mapbox-vector-tile',
    body: readFileSync(path.join(REPOSITORY_ROOT, styled_fixture_route.fixture)),
  }));
  await page.goto('/?advanced');
  await waitForMapTestReady(page);
  await open_map_layers_panel(page);

  // Every committed style has its own row, labeled with provider and style.
  for (const style_choice of styled_descriptor.vector_styles) {
    await expect(
      page.locator(`#map-layer-style-${style_choice.style_id}`),
    ).toContainText(`${styled_descriptor.display_name} · ${style_choice.display_name}`);
  }

  // A pick repaints the base in that style.
  await page.locator('#map-layer-style-bright-light').click();
  await expect
    .poll(() => page.evaluate(() => window.mapVectorBaseStatus()), { timeout: 20000 })
    .toContain('bright-light');
  await expect
    .poll(() => page.evaluate(
      () => (window.mapLastFrameStats()?.overlayTriangles ?? 0) > 0),
    { timeout: 20000 })
    .toBe(true);

  // A second style of the same descriptor repaints it again.
  await page.locator('#map-layer-style-fiord-dark').click();
  await expect
    .poll(() => page.evaluate(() => window.mapVectorBaseStatus()), { timeout: 20000 })
    .toContain('fiord-dark');
});

test('a vector style change drops the previous style before the new one draws', async ({
  page,
}) => {
  // The M49 vector → vector case: a style change on the same provider must
  // not leave the previous style's geometry on the surface. The committed
  // fixture answers every tile request; the assertions read the canvas' own
  // source status in the same turn as the change and the first frame drawn
  // afterwards.
  // The descriptor offering the named Bright/Fiord/Positron family: two of its
  // styles paint measurably different geometry from the same fixture.
  const styled_descriptor = vector_provider_descriptors.find(
    (provider_descriptor) => Array.isArray(provider_descriptor.vector_styles)
      && provider_descriptor.vector_styles.some(
        (style_choice) => style_choice.style_id === 'fiord-dark',
      ),
  );
  const styled_fixture_route = styled_descriptor.fixture_routes.find(
    (fixture_route) => fixture_route.purpose === 'vector_tiles',
  );
  const [previous_style_id, next_style_id] = styled_descriptor.vector_styles
    .map((style_choice) => style_choice.style_id);
  await page.route('**/*.pbf', (route) => route.fulfill({
    status: 200,
    contentType: 'application/vnd.mapbox-vector-tile',
    body: readFileSync(path.join(REPOSITORY_ROOT, styled_fixture_route.fixture)),
  }));
  await page.goto('/?advanced');
  await waitForMapTestReady(page);
  // The vector base needs the GPU renderer; wait for the ladder to reach it
  // before opening the sheet so the base rows are built enabled.
  await expect
    .poll(() => page.evaluate(() => window.mapRendererMode()), { timeout: 25000 })
    .toBe('gpu');
  await open_map_layers_panel(page);
  const previous_style_row = page.locator(
    `#map-layer-style-${previous_style_id}`);
  await expect(previous_style_row).toBeEnabled({ timeout: 60000 });

  // Settle the first style: its own tiles decoded and drawn.
  await previous_style_row.click();
  await expect
    .poll(() => page.evaluate((style_id) => {
      const status = window.mapGpuCanvas().vectorTileSourceStatus();
      return status.styleId === style_id && status.drawnTileCount > 0;
    }, previous_style_id), { timeout: 20000 })
    .toBe(true);
  const previous_frame_triangles = await page.evaluate(
    () => window.mapLastFrameStats()?.overlayTriangles ?? 0);
  expect(previous_frame_triangles).toBeGreaterThan(0);

  // Change the style and read the canvas' source status in the same turn: the
  // previous style's decoded frame is already gone (no drawn tiles), even
  // though the last presented frame's statistics still describe it.
  const observation = await page.evaluate((style_id) => {
    const frame_marker = window.mapFramesDrawn();
    document.getElementById(`map-layer-style-${style_id}`).click();
    const source_status = window.mapGpuCanvas().vectorTileSourceStatus();
    return {
      frameMarker: frame_marker,
      styleId: source_status.styleId,
      drawnTileCount: source_status.drawnTileCount,
    };
  }, next_style_id);
  expect(observation.styleId).toBe(next_style_id);
  expect(observation.drawnTileCount).toBe(0);

  // The first frame drawn after the change does not carry the previous
  // style's geometry; the new style then settles with its own triangle count.
  await expect
    .poll(() => page.evaluate(
      (frame_marker) => window.mapFramesDrawn() > frame_marker,
      observation.frameMarker), { timeout: 20000 })
    .toBe(true);
  expect(await page.evaluate(
    () => window.mapLastFrameStats()?.overlayTriangles ?? 0))
    .not.toBe(previous_frame_triangles);

  await expect
    .poll(() => page.evaluate((style_id) => {
      const status = window.mapGpuCanvas().vectorTileSourceStatus();
      return status.styleId === style_id && status.drawnTileCount > 0;
    }, next_style_id), { timeout: 20000 })
    .toBe(true);
  const next_frame_triangles = await page.evaluate(
    () => window.mapLastFrameStats()?.overlayTriangles ?? 0);
  expect(next_frame_triangles).toBeGreaterThan(0);
  expect(next_frame_triangles).not.toBe(previous_frame_triangles);
});

test('scene overlays stay available and draw over the vector base', async ({
  page,
}) => {
  // The round-36 field report: choosing a vector base used to disable the
  // scene toggles and clear the overlay list. The base is composed with the
  // page's own overlays now, so both draw in one frame.
  await page.route('**/*.mvt', (route) => route.fulfill({
    status: 200,
    contentType: 'application/vnd.mapbox-vector-tile',
    body: recorded_vector_tile_bytes,
  }));
  await page.goto('/?advanced&useProviderFixtures');
  await waitForMapTestReady(page);

  await open_map_layers_panel(page);
  await base_row(page, 'vector').click();
  expect(await page.evaluate(() => window.mapTileMode())).toBe('vector');

  // The vector base paints while the panel's scene switches stay usable (not
  // the disabled-with-reason state the old clear-and-pause behavior produced).
  await expect
    .poll(async () => page.evaluate(
      () => window.mapLastFrameStats()?.overlayTriangles ?? 0), { timeout: 20_000 })
    .toBeGreaterThan(0);
  const first_scene_switch = page
    .locator('#map-layer-scene-rows [role="switch"]')
    .first();
  await expect(first_scene_switch).toBeEnabled();
  await expect(page.locator('#map-layers-scenes-reason')).toBeHidden();

  // Enabling a scene composes its geometry over the base: the frame's overlay
  // triangle count grows by the scene's own triangles.
  const base_triangle_count = await page.evaluate(
    () => window.mapLastFrameStats()?.overlayTriangles ?? 0);
  await first_scene_switch.click();
  await expect(first_scene_switch).toHaveAttribute('aria-checked', 'true');
  await expect
    .poll(async () => page.evaluate(
      () => window.mapLastFrameStats()?.overlayTriangles ?? 0), { timeout: 15_000 })
    .toBeGreaterThan(base_triangle_count);

  // ...and switching it off returns to the base alone (the base is remembered,
  // not re-fetched into an empty list).
  await first_scene_switch.click();
  await expect(first_scene_switch).toHaveAttribute('aria-checked', 'false');
  await expect
    .poll(async () => page.evaluate(
      () => window.mapLastFrameStats()?.overlayTriangles ?? 0), { timeout: 15_000 })
    .toBe(base_triangle_count);
});

test('switching back to raster while a vector load is in flight paints no vector geometry', async ({
  page,
}) => {
  // The field-reported superposition: a vector-base load starts, the visitor
  // switches back to raster before the first tile answers, and the completed
  // load paints its geometry over the restored imagery. The tile answer is
  // held here until after the switch, so the race is deterministic.
  let release_tile_response;
  const tile_response_gate = new Promise((resolve) => {
    release_tile_response = resolve;
  });
  await page.route('**/*.mvt', async (route) => {
    await tile_response_gate;
    await route.fulfill({
      status: 200,
      contentType: 'application/vnd.mapbox-vector-tile',
      body: recorded_vector_tile_bytes,
    });
  });

  await page.goto('/?advanced&useProviderFixtures');
  await waitForMapTestReady(page);
  await expect
    .poll(async () => page.evaluate(
      () => window.mapLastFrameStats()?.drawnTiles ?? 0), { timeout: 15_000 })
    .toBeGreaterThan(0);

  await open_map_layers_panel(page);
  const raster_row = base_row(page, 'raster');
  await base_row(page, 'vector').click();
  await expect
    .poll(() => page.evaluate(() => window.mapVectorBaseStatus()), { timeout: 15_000 })
    .toContain('Loading');

  // Back to raster while the first tile is still in flight.
  await raster_row.click();
  release_tile_response();

  // Let the released load finish and a few frames settle: no vector overlays
  // and no labels may appear over the raster base.
  await expect
    .poll(async () => page.evaluate(
      () => window.mapLastFrameStats()?.drawnTiles ?? 0), { timeout: 15_000 })
    .toBeGreaterThan(0);
  await page.waitForTimeout(1_000);
  const settled_frame = await page.evaluate(() => window.mapLastFrameStats());
  expect(settled_frame.overlayTriangles).toBe(0);
  expect(settled_frame.labelCount).toBe(0);
  expect(await page.evaluate(() => window.mapTileMode())).toBe('raster');
});
