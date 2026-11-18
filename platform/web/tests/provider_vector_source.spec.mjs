// The wasm canvas' provider-backed vector base (the UDL
// `MapView.set_provider_vector_tile_source` member): it resolves the provider
// configured for the vector-tiles capability, takes the tileset name,
// attribution and zoom span from that provider, and refuses a source naming a
// different provider, a missing provider or an unknown style. The provider
// facts come from the shared registry, so the suite names no vendor; no
// request is made because every case fails or settles before a tile is
// fetched.
//
// The sibling member (`set_provider_vector_tile_source_from_document`) loads
// the style document the provider publishes: the case below fetches each
// registered provider's synthetic document through the provider's own
// authenticated client, from the shared fixture server, and asserts an
// unserved address leaves no base installed.
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

/** Every registry row the browser can attempt to register. */
const REGISTRABLE_DESCRIPTORS = PROVIDERS_REGISTRY.providers
  .filter((descriptor) => descriptor.registration_available?.wasm === true)
  .map((descriptor) => descriptor.provider_key);

/** A placeholder credential the provider constructor accepts; no test uses it. */
const PLACEHOLDER_CREDENTIAL = 'test-placeholder-vector-key';

/** The renderer-readiness budget (a slow first WebGPU init). */
const RENDERER_READINESS_TIMEOUT_MILLISECONDS = 30_000;

test.beforeEach(async ({ page }) => {
  await page.goto('/');
  await waitForMapTestReady(page);
  await expect
    .poll(
      () =>
        page.evaluate(() =>
          typeof window.mapRendererMode === 'function' ? window.mapRendererMode() : null,
        ),
      { timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS },
    )
    .toBe('gpu');
});

/** Builds an off-screen canvas bound to the page's SDK root. */
async function create_probe_view(page) {
  return page.evaluate(async () => {
    const canvas = document.createElement('canvas');
    canvas.width = 256;
    canvas.height = 256;
    canvas.style.position = 'fixed';
    canvas.style.left = '-10000px';
    document.body.appendChild(canvas);
    const map_view = new window.mapTest.MapView(canvas, window.mapTest.maps);
    await map_view.initialize();
    window.providerVectorSourceProbeView = map_view;
    return true;
  });
}

test('a provider-backed base refuses before any vector provider is configured', async ({
  page,
}) => {
  await create_probe_view(page);
  const outcome = await page.evaluate(() => {
    try {
      window.providerVectorSourceProbeView.setProviderVectorTileSource({
        providerKey: 'any-provider',
        styleId: 'bright-light',
      });
      return { accepted: true };
    } catch (install_failure) {
      return { accepted: false, detail: String(install_failure) };
    }
  });
  expect(outcome.accepted).toBe(false);
  expect(outcome.detail).toContain('no vector-tile provider is configured');
  const is_configured = await page.evaluate(
    () => window.providerVectorSourceProbeView.vectorTileSourceStatus().isConfigured,
  );
  expect(is_configured).toBe(false);
});

test('a provider-backed base takes its descriptor from the configured provider', async ({
  page,
}) => {
  await create_probe_view(page);
  const outcome = await page.evaluate(
    ({ providerKeys, credential }) => {
      window.mapTest.clearProviderRegistrations();
      const serving_provider_key = providerKeys.find((provider_key) => {
        try {
          window.mapTest.configureProvider('VectorTiles', provider_key, credential);
          return true;
        } catch (registration_failure) {
          return false;
        }
      });
      if (serving_provider_key === undefined) {
        return { servingProviderKey: null };
      }
      const map_view = window.providerVectorSourceProbeView;
      const mismatch = (() => {
        try {
          map_view.setProviderVectorTileSource({
            providerKey: 'another-provider',
            styleId: 'bright-light',
          });
          return null;
        } catch (install_failure) {
          return String(install_failure);
        }
      })();
      const unknown_style = (() => {
        try {
          map_view.setProviderVectorTileSource({
            providerKey: serving_provider_key,
            styleId: 'no-such-committed-style',
          });
          return null;
        } catch (install_failure) {
          return String(install_failure);
        }
      })();
      map_view.setProviderVectorTileSource({
        providerKey: serving_provider_key,
        styleId: 'bright-light',
      });
      const status = map_view.vectorTileSourceStatus();
      window.mapTest.clearProviderRegistrations();
      map_view.clearVectorTileSource();
      return {
        servingProviderKey: serving_provider_key,
        mismatch,
        unknownStyle: unknown_style,
        configured: status.isConfigured,
        styleId: status.styleId,
        sourceName: status.sourceName,
        attributionText: status.attributionText,
        maximumZoomLevel: status.maximumZoomLevel,
      };
    },
    { providerKeys: REGISTRABLE_DESCRIPTORS, credential: PLACEHOLDER_CREDENTIAL },
  );

  expect(outcome.servingProviderKey).not.toBeNull();
  expect(outcome.mismatch).toContain(
    `the installed vector-tile provider is ${outcome.servingProviderKey}`,
  );
  expect(outcome.unknownStyle).toContain('names a style the SDK does not carry');
  expect(outcome.configured).toBe(true);
  expect(outcome.styleId).toBe('bright-light');
  expect(outcome.sourceName.length).toBeGreaterThan(0);
  expect(outcome.attributionText.length).toBeGreaterThan(0);
  expect(outcome.maximumZoomLevel).toBeGreaterThan(0);
});

/** The route's literal example path: the registry's `path_pattern` with its
 * anchors removed and its escapes resolved. The committed style routes are
 * exact paths (no captures). */
function example_path_from_pattern(path_pattern) {
  return path_pattern
    .replace(/^\^/, '')
    .replace(/\$$/, '')
    .replace(/\\(.)/g, '$1');
}

test("a provider-backed base loads the provider's published style document", async ({
  page,
}) => {
  const style_document_cases = PROVIDERS_REGISTRY.providers
    .filter((descriptor) => descriptor.registration_available?.wasm === true)
    .flatMap((descriptor) => {
      const style_route = descriptor.fixture_routes.find(
        (fixture_route) => fixture_route.purpose === 'style_document',
      );
      if (style_route === undefined) {
        return [];
      }
      const provider_fixture_base = `${PROVIDERS_REGISTRY.fixture_base_path}/${descriptor.provider_key}`;
      return [
        {
          providerKey: descriptor.provider_key,
          styleDocumentUrl: `${provider_fixture_base}/${example_path_from_pattern(
            style_route.path_pattern,
          )}`,
          unservedStyleDocumentUrl: `${provider_fixture_base}/styles/not-served.json`,
        },
      ];
    });
  expect(style_document_cases.length).toBeGreaterThan(0);
  await create_probe_view(page);
  const recorded_request_urls = [];
  page.on('request', (provider_request) => {
    recorded_request_urls.push(provider_request.url());
  });
  const outcome = await page.evaluate(
    async ({ cases, credential }) => {
      const map_view = window.providerVectorSourceProbeView;
      window.mapTest.clearProviderRegistrations();
      for (const style_case of cases) {
        try {
          window.mapTest.configureProvider(
            'VectorTiles',
            style_case.providerKey,
            credential,
          );
        } catch (registration_failure) {
          continue;
        }
        let refusal = null;
        try {
          await map_view.setProviderVectorTileSourceFromDocument(
            { providerKey: style_case.providerKey, styleId: 'fixture-style' },
            style_case.unservedStyleDocumentUrl,
          );
        } catch (install_failure) {
          refusal = String(install_failure);
        }
        const configured_before_served =
          map_view.vectorTileSourceStatus().isConfigured;
        await map_view.setProviderVectorTileSourceFromDocument(
          { providerKey: style_case.providerKey, styleId: 'fixture-style' },
          style_case.styleDocumentUrl,
        );
        const status = map_view.vectorTileSourceStatus();
        window.mapTest.clearProviderRegistrations();
        map_view.clearVectorTileSource();
        return {
          providerKey: style_case.providerKey,
          refusal,
          configuredBeforeServed: configured_before_served,
          configured: status.isConfigured,
          styleId: status.styleId,
          sourceName: status.sourceName,
          unsupportedPropertyCount: status.unsupportedPropertyCount,
          unsupportedPropertyNames: status.unsupportedPropertyNames,
          degradedAssetNames: status.degradedAssetNames,
          sourceLayerCounts: status.sourceLayerCounts,
        };
      }
      return { providerKey: null };
    },
    { cases: style_document_cases, credential: PLACEHOLDER_CREDENTIAL },
  );

  expect(outcome.providerKey).not.toBeNull();
  expect(outcome.refusal).not.toBeNull();
  expect(outcome.configuredBeforeServed).toBe(false);
  expect(outcome.configured).toBe(true);
  expect(outcome.styleId).toBe('fixture-style');
  expect(outcome.sourceName.length).toBeGreaterThan(0);
  // The status reports the installed document's fidelity in the contract
  // shape: a numeric unsupported-property count, both bounded name lists, and
  // the per-source layer accounting (empty until the first frame resolves).
  expect(typeof outcome.unsupportedPropertyCount).toBe('number');
  expect(Array.isArray(outcome.unsupportedPropertyNames)).toBe(true);
  expect(outcome.unsupportedPropertyNames.length).toBeLessThanOrEqual(32);
  expect(Array.isArray(outcome.degradedAssetNames)).toBe(true);
  expect(Array.isArray(outcome.sourceLayerCounts)).toBe(true);

  // The install resolves the style document's own glyph endpoint and sprite
  // base, relative to the address the document was served from, so both
  // reach the fixture server through the provider's client.
  const installed_descriptor = PROVIDERS_REGISTRY.providers.find(
    (descriptor) => descriptor.provider_key === outcome.providerKey,
  );
  const resource_routes = installed_descriptor.fixture_routes.filter(
    (fixture_route) =>
      fixture_route.purpose === 'glyphs' ||
      fixture_route.purpose === 'sprite_sheet',
  );
  expect(resource_routes.length).toBeGreaterThan(0);
  const provider_fixture_prefix = `${PROVIDERS_REGISTRY.fixture_base_path}/${outcome.providerKey}/`;
  const requested_relative_paths = () =>
    recorded_request_urls
      .map((request_url) => new URL(request_url).pathname)
      .filter((request_path) => request_path.startsWith(provider_fixture_prefix))
      .map((request_path) => request_path.slice(provider_fixture_prefix.length));
  const requested_purposes = () => {
    const relative_paths = requested_relative_paths();
    return [
      ...new Set(
        resource_routes
          .filter((fixture_route) =>
            relative_paths.some((relative_path) =>
              new RegExp(fixture_route.path_pattern).test(relative_path),
            ),
          )
          .map((fixture_route) => fixture_route.purpose),
      ),
    ].sort();
  };
  await expect.poll(requested_purposes).toEqual(['glyphs', 'sprite_sheet']);
});
