// The raster ⇄ vector tile-layer switch (M9 live-tile integration): the wasm
// canvas' `setBaseMapVisible` seam and the page-side `tile_switch.mjs`
// module. Every provider fact (the vector tile URL template) is read from the
// shared registry `platform/providers.json`, so this suite names no vendor.
// The render tests drive a test-owned wasm canvas over the committed
// same-origin raster fixtures; the URL, address-listing and coalescing tests
// never touch the network.
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, test, waitForMapTestReady } from './network_guard.mjs';

const TEST_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const REPOSITORY_ROOT = path.resolve(TEST_DIRECTORY, '..', '..', '..');

/** The shared provider registry whose descriptor owns the URL template. */
const PROVIDERS_REGISTRY = JSON.parse(
  readFileSync(path.join(REPOSITORY_ROOT, 'platform', 'providers.json'), 'utf8'),
);
/** The first registry descriptor declaring a vector tile URL template. */
const VECTOR_TILE_DESCRIPTOR = PROVIDERS_REGISTRY.providers.find(
  (descriptor) => typeof descriptor.url_template === 'string',
);
if (VECTOR_TILE_DESCRIPTOR === undefined) {
  throw new Error('the registry declares no provider with a url_template');
}

/** Edge of the test canvas, in pixels. */
const TEST_CANVAS_EDGE_PIXELS = 256;
/** The playground's field of view (`FIELD_OF_VIEW_DEGREES` in index.html). */
const FIELD_OF_VIEW_DEGREES = 60;
/** The demo's launch camera: over the recorded Sacramento raster fixtures. */
const SACRAMENTO_CAMERA = {
  latitude: 38.5766,
  longitude: -121.4944,
  zoom: 14,
  bearing: 0,
  pitch: 0,
};
/** The Sacramento camera's integer tile address at zoom 14. */
const SACRAMENTO_TILE = { zoomLevel: 14, tileX: 2662, tileY: 6286 };
/** Tile edge the address listing validates (it does not affect addresses). */
const TILE_SIZE_PIXELS = 256;
/** Camera near the north-east grid corner at zoom 2, for the clamp case. */
const GRID_EDGE_CAMERA = { latitude: 84.9, longitude: 179.9, zoom: 2 };
/** The grid-edge camera's integer zoom level. */
const GRID_EDGE_ZOOM_LEVEL = 2;
/** Highest valid tile index at [`GRID_EDGE_ZOOM_LEVEL`]. */
const GRID_EDGE_MAXIMUM_INDEX = 2 ** GRID_EDGE_ZOOM_LEVEL - 1;
/** Ring counts the (2r+1)² listing must produce. */
const ADDRESS_COUNTS_BY_RADIUS = new Map([
  [0, 1],
  [1, 9],
  [2, 25],
]);
/** The radius used for the exact-count and clamp assertions. */
const CLAMP_TILE_RADIUS = 2;
/** The center address's index inside a radius-1 row-major listing. */
const RADIUS_ONE_CENTER_INDEX = 4;
/** Frames a settled fixture camera may need (the per-frame fetch budget). */
// The settle loop's frame limit: the loop yields between frames (see
// `settle_frame_delay_milliseconds` inside the page setup), so it needs more
// turns than the old blocking frame path did.
const SETTLE_FRAME_LIMIT = 20;
/** GPU initialization and first-frame budget, as in the golden suite. */
const RENDERER_READINESS_TIMEOUT_MILLISECONDS = 30_000;
/** The page's own render loop must publish a frame while the test runs. */
const PAGE_FRAME_TIMEOUT_MILLISECONDS = 20_000;
/** A short visible route at the demo camera, so overlays draw in hidden frames. */
const VISIBLE_ROUTE_OVERLAY = {
  kind: 'routePolyline',
  coordinates: [
    { latitude: 38.5766, longitude: -121.4944 },
    { latitude: 38.5776, longitude: -121.4934 },
  ],
  color: { red: 1, green: 0, blue: 0, alpha: 1 },
  widthPixels: 8,
};
/** Fake provider templates for the coalescing test (never fetched). */
const FIRST_LOAD_URL_TEMPLATE = 'https://first.invalid/{z}/{x}/{y}.mvt';
const SECOND_LOAD_URL_TEMPLATE = 'https://second.invalid/{z}/{x}/{y}.mvt';

test.beforeEach(async ({ page }) => {
  await page.goto('/?advanced');
  // The Rust-side `initializeSdk` promise gates the wasm module's readiness.
  await waitForMapTestReady(page);
});

/**
 * Builds a test-owned wasm canvas and its frame helper on `window`. Skips the
 * test when the browser cannot initialize WebGPU (the documented degradation
 * path the other GPU suites assert).
 */
async function require_test_canvas(page) {
  const initialization = await page.evaluate(
    async ({ canvas_edge_pixels }) => {
      const canvas_element = document.createElement('canvas');
      canvas_element.width = canvas_edge_pixels;
      canvas_element.height = canvas_edge_pixels;
      canvas_element.id = 'tile-switch-test-canvas';
      document.body.append(canvas_element);
      try {
        // A frame starts its tile fetches and returns (a presenting frame
        // never blocks on the network, PLAN.md M23), so the settle loop below
        // yields between frames — the page's settled pump's same discipline.
        const settle_frame_delay_milliseconds = 30;
        const canvas = new window.mapTest.MapView(
          canvas_element,
          window.mapTest.TileResolution.Standard,
        );
        const adapter = await canvas.initialize();
        window.tileSwitchTest = {
          canvas,
          canvasElement: canvas_element,
          // `renderFrame` consumes the camera by value, so the frame builds
          // its own and never frees it (a free after the call would
          // double-free the moved binding).
          renderFrame: async (camera_pose, field_of_view_degrees) => {
            const camera = new window.mapTest.MapCamera(
              camera_pose.latitude,
              camera_pose.longitude,
              camera_pose.zoom,
              camera_pose.bearing,
              camera_pose.pitch,
            );
            return canvas.renderFrame(camera, field_of_view_degrees);
          },
          renderSettledFrame: async (
            camera_pose,
            field_of_view_degrees,
            maximum_frames,
          ) => {
            let frame_statistics = null;
            for (
              let settle_frame = 0;
              settle_frame < maximum_frames;
              settle_frame += 1
            ) {
              const camera = new window.mapTest.MapCamera(
                camera_pose.latitude,
                camera_pose.longitude,
                camera_pose.zoom,
                camera_pose.bearing,
                camera_pose.pitch,
              );
              frame_statistics = await canvas.renderFrame(
                camera,
                field_of_view_degrees,
              );
              await new Promise((resolve) =>
                setTimeout(resolve, settle_frame_delay_milliseconds));
              if (
                frame_statistics.drawnTiles > 0 &&
                frame_statistics.missingTiles === 0
              ) {
                break;
              }
            }
            return frame_statistics;
          },
        };
        return { ready: true, adapter };
      } catch (initialization_failure) {
        return {
          ready: false,
          error: String(initialization_failure?.message ?? initialization_failure),
        };
      }
    },
    { canvas_edge_pixels: TEST_CANVAS_EDGE_PIXELS },
  );
  if (!initialization.ready) {
    test.skip(
      true,
      `headless WebGPU unavailable in this browser; degradation asserted: ${initialization.error}`,
    );
    return false;
  }
  return true;
}

test('hiding the raster tile layer keeps presenting overlays and reports zero tiles', async ({
  page,
}) => {
  if (!(await require_test_canvas(page))) {
    return;
  }
  const frame_report = await page.evaluate(
    async ({
      camera_pose,
      field_of_view_degrees,
      settle_frame_limit,
      overlay,
    }) => {
      const tile_switch = await import('/tile_switch.mjs');
      window.tileSwitchTest.canvas.setOverlays([overlay]);
      const visible_statistics = await window.tileSwitchTest.renderSettledFrame(
        camera_pose,
        field_of_view_degrees,
        settle_frame_limit,
      );
      tile_switch.applyTileLayerMode({
        canvas: window.tileSwitchTest.canvas,
        mode: tile_switch.TILE_LAYER_MODES.vector,
      });
      const hidden_statistics = await window.tileSwitchTest.renderFrame(
        camera_pose,
        field_of_view_degrees,
      );
      return { visible_statistics, hidden_statistics };
    },
    {
      camera_pose: SACRAMENTO_CAMERA,
      field_of_view_degrees: FIELD_OF_VIEW_DEGREES,
      settle_frame_limit: SETTLE_FRAME_LIMIT,
      overlay: VISIBLE_ROUTE_OVERLAY,
    },
  );

  // The fixture camera settles onto drawn tiles before the switch.
  expect(frame_report.visible_statistics.drawnTiles).toBeGreaterThan(0);
  expect(frame_report.visible_statistics.overlayTriangles).toBeGreaterThan(0);

  const hidden_statistics = frame_report.hidden_statistics;
  expect(hidden_statistics.requestedTiles).toBe(0);
  expect(hidden_statistics.drawnTiles).toBe(0);
  expect(hidden_statistics.missingTiles).toBe(0);
  expect(hidden_statistics.drawnFragments).toBe(0);
  // The overlay layer still draws over the cleared surface…
  expect(hidden_statistics.overlayTriangles).toBe(
    frame_report.visible_statistics.overlayTriangles,
  );
  // …and the frame still reports its zoom and resolution.
  expect(hidden_statistics.tileZoomLevel).toBe(
    frame_report.visible_statistics.tileZoomLevel,
  );
  expect(hidden_statistics.tileResolution).toBe(
    frame_report.visible_statistics.tileResolution,
  );

  // The page's own render loop keeps presenting frames while the raster base
  // layer is hidden.
  await expect
    .poll(() => page.evaluate(() => window.mapLastFrameStats() !== null), {
      timeout: PAGE_FRAME_TIMEOUT_MILLISECONDS,
    })
    .toBe(true);
});

test('showing the raster tile layer again draws tiles on the settled camera', async ({
  page,
}) => {
  if (!(await require_test_canvas(page))) {
    return;
  }
  const frame_report = await page.evaluate(
    async ({ camera_pose, field_of_view_degrees, settle_frame_limit }) => {
      const tile_switch = await import('/tile_switch.mjs');
      tile_switch.applyTileLayerMode({
        canvas: window.tileSwitchTest.canvas,
        mode: tile_switch.TILE_LAYER_MODES.vector,
      });
      const hidden_statistics = await window.tileSwitchTest.renderFrame(
        camera_pose,
        field_of_view_degrees,
      );
      tile_switch.applyTileLayerMode({
        canvas: window.tileSwitchTest.canvas,
        mode: tile_switch.TILE_LAYER_MODES.raster,
      });
      const shown_statistics = await window.tileSwitchTest.renderSettledFrame(
        camera_pose,
        field_of_view_degrees,
        settle_frame_limit,
      );
      return { hidden_statistics, shown_statistics };
    },
    {
      camera_pose: SACRAMENTO_CAMERA,
      field_of_view_degrees: FIELD_OF_VIEW_DEGREES,
      settle_frame_limit: SETTLE_FRAME_LIMIT,
    },
  );

  expect(frame_report.hidden_statistics.drawnTiles).toBe(0);
  expect(frame_report.shown_statistics.requestedTiles).toBeGreaterThan(0);
  expect(frame_report.shown_statistics.drawnTiles).toBeGreaterThan(0);
  expect(frame_report.shown_statistics.missingTiles).toBe(0);
});

test('vector tile URLs come from the registry template and a missing template is refused', async ({
  page,
}) => {
  const url_report = await page.evaluate(
    async ({ descriptor, zoom_level, tile_x, tile_y }) => {
      const tile_switch = await import('/tile_switch.mjs');
      const substituted_url = tile_switch.vectorTileUrlForDescriptor({
        descriptor,
        zoomLevel: zoom_level,
        tileX: tile_x,
        tileY: tile_y,
      });
      let refusal_text = null;
      try {
        tile_switch.vectorTileUrlForDescriptor({
          descriptor: { provider_key: 'no-template' },
          zoomLevel: zoom_level,
          tileX: tile_x,
          tileY: tile_y,
        });
      } catch (template_refusal) {
        refusal_text = String(template_refusal);
      }
      return { substituted_url, refusal_text };
    },
    {
      descriptor: VECTOR_TILE_DESCRIPTOR,
      zoom_level: SACRAMENTO_TILE.zoomLevel,
      tile_x: SACRAMENTO_TILE.tileX,
      tile_y: SACRAMENTO_TILE.tileY,
    },
  );

  const expected_url = VECTOR_TILE_DESCRIPTOR.url_template
    .replaceAll('{z}', String(SACRAMENTO_TILE.zoomLevel))
    .replaceAll('{x}', String(SACRAMENTO_TILE.tileX))
    .replaceAll('{y}', String(SACRAMENTO_TILE.tileY));
  expect(url_report.substituted_url).toBe(expected_url);
  // The placeholders are really substituted, and the substitution is not a
  // copy of the template by accident.
  expect(url_report.substituted_url).not.toContain('{z}');
  expect(url_report.substituted_url).not.toContain('{x}');
  expect(url_report.substituted_url).not.toContain('{y}');

  expect(url_report.refusal_text).not.toBeNull();
  expect(url_report.refusal_text).toContain('url_template');
});

test('the address listing returns 1/9/25 tiles and clamps onto the grid edges', async ({
  page,
}) => {
  const address_report = await page.evaluate(
    async ({ camera_pose, edge_pose, tile_size_pixels, tile_radius }) => {
      const tile_switch = await import('/tile_switch.mjs');
      const listing_for_radius = (camera_state, radius) =>
        tile_switch.vectorTileAddressesAroundCenter({
          cameraState: camera_state,
          tileSizePixels: tile_size_pixels,
          tileRadius: radius,
        });
      return {
        one: listing_for_radius(camera_pose, 0),
        nine: listing_for_radius(camera_pose, 1),
        twenty_five: listing_for_radius(camera_pose, 2),
        edge: listing_for_radius(edge_pose, tile_radius),
      };
    },
    {
      camera_pose: SACRAMENTO_CAMERA,
      edge_pose: GRID_EDGE_CAMERA,
      tile_size_pixels: TILE_SIZE_PIXELS,
      tile_radius: CLAMP_TILE_RADIUS,
    },
  );

  expect(address_report.one).toHaveLength(ADDRESS_COUNTS_BY_RADIUS.get(0));
  expect(address_report.nine).toHaveLength(ADDRESS_COUNTS_BY_RADIUS.get(1));
  expect(address_report.twenty_five).toHaveLength(
    ADDRESS_COUNTS_BY_RADIUS.get(2),
  );
  // The center tile of the Sacramento camera, at its rounded integer zoom.
  expect(address_report.one[0]).toEqual(SACRAMENTO_TILE);
  // Row-major order keeps the center tile at the middle of a radius-1 listing.
  expect(address_report.nine[RADIUS_ONE_CENTER_INDEX]).toEqual(SACRAMENTO_TILE);

  // A camera at the north-east corner clamps every address onto the zoom
  // grid instead of leaving it, and repeats the edge indices.
  expect(address_report.edge).toHaveLength(
    ADDRESS_COUNTS_BY_RADIUS.get(CLAMP_TILE_RADIUS),
  );
  for (const tile_address of address_report.edge) {
    expect(tile_address.zoomLevel).toBe(GRID_EDGE_ZOOM_LEVEL);
    expect(Number.isInteger(tile_address.tileX)).toBe(true);
    expect(Number.isInteger(tile_address.tileY)).toBe(true);
    expect(tile_address.tileX).toBeGreaterThanOrEqual(0);
    expect(tile_address.tileX).toBeLessThanOrEqual(GRID_EDGE_MAXIMUM_INDEX);
    expect(tile_address.tileY).toBeGreaterThanOrEqual(0);
    expect(tile_address.tileY).toBeLessThanOrEqual(GRID_EDGE_MAXIMUM_INDEX);
  }
  expect(
    address_report.edge.filter(
      (tile_address) => tile_address.tileX === GRID_EDGE_MAXIMUM_INDEX,
    ).length,
  ).toBeGreaterThan(1);
  expect(
    address_report.edge.filter((tile_address) => tile_address.tileY === 0)
      .length,
  ).toBeGreaterThan(1);
});

test('the address listing never asks above the declared maximum zoom', async ({
  page,
}) => {
  // A camera past the provider's ceiling keeps listing its highest served
  // zoom, so no request can 404 an address the service does not have.
  const zoom_report = await page.evaluate(
    async ({ maximum_zoom_level, past_ceiling_zoom }) => {
      const tile_switch = await import('/tile_switch.mjs');
      const addresses = tile_switch.vectorTileAddressesAroundCenter({
        cameraState: {
          latitude: 38.5766,
          longitude: -121.4944,
          zoom: past_ceiling_zoom,
        },
        maximumZoomLevel: maximum_zoom_level,
      });
      return [...new Set(addresses.map((address) => address.zoomLevel))];
    },
    {
      maximum_zoom_level: VECTOR_TILE_DESCRIPTOR.maximum_zoom_level,
      past_ceiling_zoom: VECTOR_TILE_DESCRIPTOR.maximum_zoom_level + 3,
    },
  );
  expect(zoom_report).toEqual([VECTOR_TILE_DESCRIPTOR.maximum_zoom_level]);
});

test('a newer vector-base load supersedes the in-flight one', async ({
  page,
}) => {
  const coalescing_report = await page.evaluate(
    async ({ camera_pose, first_url_template, second_url_template }) => {
      const tile_switch = await import('/tile_switch.mjs');
      let release_first_tile;
      const first_tile_gate = new Promise((resolve) => {
        release_first_tile = resolve;
      });
      const applied_overlay_lists = [];
      const fake_canvas = {
        setOverlays: (overlays) => applied_overlay_lists.push(overlays),
      };
      const decoder = async ({ url }) => {
        if (url.startsWith('https://first.invalid')) {
          await first_tile_gate;
          return [{ marker: 'first' }];
        }
        return [{ marker: 'second' }];
      };
      const load_for_descriptor = (descriptor) =>
        tile_switch.loadVectorBaseOverlays({
          canvas: fake_canvas,
          descriptors: [descriptor],
          cameraState: camera_pose,
          tileSizePixels: 256,
          tileRadius: 0,
          fetchAndBuildVectorOverlays: decoder,
        });
      const first_load = load_for_descriptor({
        provider_key: 'first',
        url_template: first_url_template,
      });
      // The second call starts while the first tile is still in flight.
      const second_load = load_for_descriptor({
        provider_key: 'second',
        url_template: second_url_template,
      });
      const second_result = await second_load;
      release_first_tile();
      const first_result = await first_load;
      return {
        first_result,
        second_result,
        applied_overlay_lists,
      };
    },
    {
      camera_pose: SACRAMENTO_CAMERA,
      first_url_template: FIRST_LOAD_URL_TEMPLATE,
      second_url_template: SECOND_LOAD_URL_TEMPLATE,
    },
  );

  // The superseded call applies nothing; only the latest list reaches the
  // canvas.
  expect(coalescing_report.first_result.overlayCount).toBe(0);
  expect(coalescing_report.first_result.superseded).toBe(true);
  expect(coalescing_report.second_result.overlayCount).toBe(1);
  expect(coalescing_report.second_result.superseded).toBe(false);
  expect(coalescing_report.applied_overlay_lists).toHaveLength(1);
  expect(coalescing_report.applied_overlay_lists[0][0].marker).toBe('second');
});

test('leaving the vector base cancels an in-flight load before it paints', async ({
  page,
}) => {
  // The superposition this covers: a vector-base load started, the visitor
  // switched back to raster while its first tile was still fetching, and the
  // completed load painted its overlays over the freshly restored imagery.
  // `cancelVectorBaseLoad` is what the switch calls before clearing.
  const cancellation_report = await page.evaluate(
    async ({ camera_pose, url_template }) => {
      const tile_switch = await import('/tile_switch.mjs');
      let release_first_tile;
      const first_tile_gate = new Promise((resolve) => {
        release_first_tile = resolve;
      });
      const applied_overlay_lists = [];
      const fake_canvas = {
        setOverlays: (overlays) => applied_overlay_lists.push(overlays),
      };
      const decoder = async () => {
        await first_tile_gate;
        return { overlays: [{ marker: 'vector' }] };
      };
      const load = tile_switch.loadVectorBaseOverlays({
        canvas: fake_canvas,
        descriptors: [{ provider_key: 'vector', url_template }],
        cameraState: camera_pose,
        tileSizePixels: 256,
        tileRadius: 1,
        fetchAndBuildVectorOverlays: decoder,
      });
      // The switch back to raster happens while the tiles are in flight.
      tile_switch.cancelVectorBaseLoad();
      release_first_tile();
      const result = await load;
      return { result, applied_overlay_lists };
    },
    {
      camera_pose: SACRAMENTO_CAMERA,
      url_template: FIRST_LOAD_URL_TEMPLATE,
    },
  );

  expect(cancellation_report.result.superseded).toBe(true);
  expect(cancellation_report.result.overlayCount).toBe(0);
  expect(cancellation_report.applied_overlay_lists).toEqual([]);
});
