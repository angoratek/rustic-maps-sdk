// The browser vector-tile seam (M9 live-tile integration): the module
// `platform/web/vector_layer.mjs` fetches a committed vector tile through the
// shared generic provider router, decodes and styles it in wasm, and returns
// the `setOverlays` descriptors the WebGPU canvas draws. Every provider fact
// (key, fixture route, fixture bytes) is read from the shared registry
// `platform/providers.json`, so this suite names no vendor and touches no
// external network (the dev server serves the committed fixture same-origin).
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
const PROVIDER_FIXTURE_BASE_PATH = providers_registry.fixture_base_path;

/**
 * The registry's URL-template vector-tile fixture route and the descriptor
 * owning it, whose exact path spells a `z/x/y.mvt` tile address.
 */
const vector_tile_fixture = providers_registry.providers
  .filter((descriptor) => typeof descriptor.url_template === 'string')
  .flatMap((descriptor) =>
    descriptor.fixture_routes.map((fixture_route) => ({
      descriptor,
      fixture_route,
    })),
  )
  .find(({ fixture_route }) => fixture_route.purpose === 'vector_tiles');

if (vector_tile_fixture === undefined) {
  throw new Error('the registry declares no "vector_tiles" fixture route');
}

const PROVIDER_KEY = vector_tile_fixture.descriptor.provider_key;
const RECORDED_VECTOR_TILE_BYTES = readFileSync(
  path.join(REPOSITORY_ROOT, vector_tile_fixture.fixture_route.fixture),
);

/**
 * The route's literal example path: the descriptor's `path_pattern` with its
 * anchors removed and its escapes resolved. The committed vector route is an
 * exact path (no captures), which is all this suite needs.
 */
function example_path_from_pattern(path_pattern) {
  return path_pattern
    .replace(/^\^/, '')
    .replace(/\$$/, '')
    .replace(/\\(.)/g, '$1');
}

const SERVED_VECTOR_TILE_PATH = `/${example_path_from_pattern(
  vector_tile_fixture.fixture_route.path_pattern,
)}`;
const SERVED_VECTOR_TILE_URL = `${PROVIDER_FIXTURE_BASE_PATH}/${PROVIDER_KEY}${SERVED_VECTOR_TILE_PATH}`;
/** An address the exact-path route does not serve. */
const UNSERVED_VECTOR_TILE_URL = SERVED_VECTOR_TILE_URL.replace(
  /\.mvt$/,
  '_unrecorded.mvt',
);

/** The tile address the demo route path spells (`z/x/y.mvt`). */
const TILE_ADDRESS_MATCH = /(\d+)\/(\d+)\/(\d+)\.mvt$/.exec(
  SERVED_VECTOR_TILE_PATH,
);
const [TILE_ZOOM_LEVEL, TILE_X, TILE_Y] = TILE_ADDRESS_MATCH.slice(1).map(
  (address_segment) => Number.parseInt(address_segment, 10),
);

/** The demo's raster tile edge, passed to the module's tile-size option. */
const TILE_SIZE_PIXELS = 256;

/**
 * MVT producers buffer geometry beyond the tile extent so features do not
 * clip at the seam, and label layers anchor points well past the edge. The
 * `rustic-maps-rendering` conversion test allows a third of a tile; this suite uses
 * the same tolerance.
 */
const TILE_BUFFER_TOLERANCE_FRACTION = 0.34;

// The coordinate list a descriptor kind carries, in the canvas vocabulary.
function descriptor_coordinates(descriptor) {
  switch (descriptor.kind) {
    case 'filledPolygon':
      return descriptor.ringCoordinates;
    case 'routePolyline':
      return descriptor.coordinates;
    case 'pointMarkers':
      return descriptor.positions;
    default:
      return [];
  }
}

test.beforeEach(async ({ page }) => {
  await page.goto('/?advanced');
  // The Rust-side `initializeSdk` promise gates the wasm module's readiness.
  await waitForMapTestReady(page);
});

test('the fixture route serves the committed vector tile bytes', async ({
  page,
}) => {
  const tile_bytes_report = await page.evaluate(async (vector_tile_url) => {
    const vector_layer = await import('/vector_layer.mjs');
    const tile_bytes = await vector_layer.fetchVectorTileBytes(vector_tile_url);
    return {
      is_uint8_array: tile_bytes instanceof Uint8Array,
      byte_length: tile_bytes.length,
    };
  }, SERVED_VECTOR_TILE_URL);
  expect(tile_bytes_report.is_uint8_array).toBe(true);
  expect(tile_bytes_report.byte_length).toBe(RECORDED_VECTOR_TILE_BYTES.length);
});

test('the wasm seam decodes the fixture into fill, line and point overlays inside the tile', async ({
  page,
}) => {
  const decode_report = await page.evaluate(
    async ([vector_tile_url, zoom_level, tile_x, tile_y, tile_size_pixels]) => {
      const vector_layer = await import('/vector_layer.mjs');
      const tile_bytes = await vector_layer.fetchVectorTileBytes(vector_tile_url);
      const overlays = vector_layer.buildVectorOverlaysFromBytes({
        tileBytes: tile_bytes,
        tileZoomLevel: zoom_level,
        tileX: tile_x,
        tileY: tile_y,
      });
      const bounds = vector_layer.tileGeographicBounds({
        zoomLevel: zoom_level,
        tileX: tile_x,
        tileY: tile_y,
      });
      const world_pixel_quad = vector_layer.tileWorldPixelQuad({
        zoomLevel: zoom_level,
        tileX: tile_x,
        tileY: tile_y,
        tileSizePixels: tile_size_pixels,
      });
      return { overlays, bounds, world_pixel_quad };
    },
    [
      SERVED_VECTOR_TILE_URL,
      TILE_ZOOM_LEVEL,
      TILE_X,
      TILE_Y,
      TILE_SIZE_PIXELS,
    ],
  );

  const overlays = decode_report.overlays;
  expect(Array.isArray(overlays)).toBe(true);
  expect(overlays.length).toBeGreaterThan(0);

  const painted_kinds = new Set(overlays.map((overlay) => overlay.kind));
  expect(painted_kinds.has('filledPolygon')).toBe(true);
  expect(painted_kinds.has('routePolyline')).toBe(true);
  expect(painted_kinds.has('pointMarkers')).toBe(true);

  // The tile's geographic corners, expanded by the producer-buffer tolerance.
  const { northWest, southEast } = decode_report.bounds;
  expect(northWest.latitude).toBeGreaterThan(southEast.latitude);
  expect(northWest.longitude).toBeLessThan(southEast.longitude);
  const latitude_margin =
    (northWest.latitude - southEast.latitude) * TILE_BUFFER_TOLERANCE_FRACTION;
  const longitude_margin =
    (southEast.longitude - northWest.longitude) * TILE_BUFFER_TOLERANCE_FRACTION;

  for (const overlay of overlays) {
    const coordinates = descriptor_coordinates(overlay);
    expect(coordinates.length).toBeGreaterThan(0);
    for (const coordinate of coordinates) {
      expect(Number.isFinite(coordinate.latitude)).toBe(true);
      expect(Number.isFinite(coordinate.longitude)).toBe(true);
      expect(coordinate.latitude).toBeLessThanOrEqual(
        northWest.latitude + latitude_margin,
      );
      expect(coordinate.latitude).toBeGreaterThanOrEqual(
        southEast.latitude - latitude_margin,
      );
      expect(coordinate.longitude).toBeGreaterThanOrEqual(
        northWest.longitude - longitude_margin,
      );
      expect(coordinate.longitude).toBeLessThanOrEqual(
        southEast.longitude + longitude_margin,
      );
    }
    // Every descriptor carries a well-formed color in the canvas vocabulary.
    expect(Number.isFinite(overlay.color.red)).toBe(true);
    expect(Number.isFinite(overlay.color.green)).toBe(true);
    expect(Number.isFinite(overlay.color.blue)).toBe(true);
    expect(Number.isFinite(overlay.color.alpha)).toBe(true);
  }

  // The pure world-pixel helper places this tile in the map square.
  expect(decode_report.world_pixel_quad.leftPixels).toBe(
    TILE_X * TILE_SIZE_PIXELS,
  );
  expect(decode_report.world_pixel_quad.topPixels).toBe(
    TILE_Y * TILE_SIZE_PIXELS,
  );
  expect(decode_report.world_pixel_quad.widthPixels).toBe(TILE_SIZE_PIXELS);
  expect(decode_report.world_pixel_quad.worldPixelSize).toBe(
    TILE_SIZE_PIXELS * 2 ** TILE_ZOOM_LEVEL,
  );
});

test('malformed and empty payloads return a typed refusal, never a wasm panic', async ({
  page,
}) => {
  const refusals = await page.evaluate(
    async ([zoom_level, tile_x, tile_y]) => {
      const vector_layer = await import('/vector_layer.mjs');
      const refusal_messages = [];
      for (const tile_bytes of [
        new Uint8Array(0),
        new Uint8Array([1, 2, 3, 4]),
      ]) {
        try {
          vector_layer.buildVectorOverlaysFromBytes({
            tileBytes: tile_bytes,
            tileZoomLevel: zoom_level,
            tileX: tile_x,
            tileY: tile_y,
          });
          refusal_messages.push(null);
        } catch (error) {
          refusal_messages.push({
            error_type: typeof error,
            error_code: error?.code,
            error_text: String(error),
          });
        }
      }
      return refusal_messages;
    },
    [TILE_ZOOM_LEVEL, TILE_X, TILE_Y],
  );

  for (const refusal of refusals) {
    // wasm-bindgen surfaces the Rust failure as a thrown error object with a
    // stable code, never an opaque wasm trap or a panic. Its string form
    // stays the verbatim detail existing callers compare.
    expect(refusal).not.toBeNull();
    expect(refusal.error_type).toBe('object');
    expect(refusal.error_code).toBe('responseParsingFailure');
    expect(refusal.error_text.length).toBeGreaterThan(0);
  }
  expect(refusals[0].error_text).toContain('vector tile payload is empty');
  expect(refusals[1].error_text).toContain('failed to decode');
});

test('an unknown provider path 404s and the fetch reports it', async ({
  page,
}) => {
  const unknown_provider_url = `${PROVIDER_FIXTURE_BASE_PATH}/unknown-provider${SERVED_VECTOR_TILE_PATH}`;
  const unknown_provider_response = await page.request.get(unknown_provider_url);
  expect(unknown_provider_response.status()).toBe(404);

  const unserved_response = await page.request.get(UNSERVED_VECTOR_TILE_URL);
  expect(unserved_response.status()).toBe(404);

  const fetch_refusal = await page.evaluate(async (vector_tile_url) => {
    const vector_layer = await import('/vector_layer.mjs');
    try {
      await vector_layer.fetchVectorTileBytes(vector_tile_url);
      return null;
    } catch (error) {
      return String(error);
    }
  }, UNSERVED_VECTOR_TILE_URL);
  expect(fetch_refusal).toContain('404');
});
