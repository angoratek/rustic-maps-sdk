// Minimal static file server for the Playwright harness: the Node standard
// library only, so the harness carries no frameworks beyond Playwright.
import { createServer } from 'node:http';
import { readFileSync } from 'node:fs';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const HOST = '127.0.0.1';
// The default port; a second checkout can run its own harness beside this one
// by setting PORT, which the Playwright configuration reads through this same
// export so the server URL and the base URL cannot disagree.
const PORT = Number(process.env.PORT ?? 8642);
const ROOT_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const SERVER_UNAVAILABLE_DELAY_MILLISECONDS = 500;

// The recorded raster tile fixtures the browser canvas renders (the browser
// never fetches an external tile network). They are recorded into map-
// rendering's fixture directory by
// `cargo run -p rustic-maps-rendering --example record_golden_tiles` and
// `cargo run -p rustic-maps-rendering --example record_playground_tiles` and served
// at the same-origin `/tiles/{zoom}_{tileX}_{tileY}[@2x].png` route (the
// `@2x` marker doubles a fixture's texel edge, the Tiles API's retina form).
const TILE_FIXTURES_DIRECTORY = path.resolve(
  ROOT_DIRECTORY,
  '../../crates/rustic-maps-rendering/fixtures/render/tiles',
);
const TILE_FIXTURE_URL_PATTERN = /^\/tiles\/(\d+)_(\d+)_(\d+)(@2x)?\.png$/;
const MAXIMUM_TILE_ZOOM_LEVEL = 30;

// The committed golden fixtures the browser canvas-vs-golden compare
// (tests/golden_compare.spec.mjs) decodes and diffs in-page: the two tile
// scenes and the four Sacramento overlay scenes. The route allow-lists exactly
// these file names, so the address can name nothing else on disk (the same
// traversal safety the tiles route gets from its zoom and tile-index
// bounds).
const GOLDEN_FIXTURE_URL_PATTERN = /^\/golden\/([a-z0-9_]+)\.png$/;
const GOLDEN_FIXTURE_FILE_NAMES = new Set([
  'sacramento_zoom14_top_down',
  'sacramento_zoom14_pitched45',
  'sacramento_zoom14_route_line',
  'sacramento_zoom14_polygon_fill',
  'sacramento_zoom14_markers',
  'sacramento_zoom14_traffic_flow',
  'sacramento_zoom14_bright_light',
  'sacramento_zoom14_fiord_dark',
  'sacramento_zoom14_positron_light',
  // The native Carto regional golden and the browser regional golden captured
  // from the same scene, so the suite can compare the browser frame against
  // its own golden and measure it against the native reference.
  'sacramento_region_zoom9_carto_light',
  'sacramento_region_zoom9_carto_light_browser',
]);
// The committed vector tile fixtures the golden scenes render through the
// canvas-owned vector source: the z14 OpenMapTiles tile behind the per-style
// goldens and the z9 regional tile behind the regional Carto golden. The
// address is resolved through the registry (below), so the route can serve
// nothing a descriptor has not recorded; every other address answers 404.
const GOLDEN_VECTOR_TILE_URL_PATTERN = /^\/golden-vector\/(\d+)\/(\d+)\/(\d+)\.pbf$/;
// Resolves a golden vector tile address to its committed fixture without
// naming a provider or a URL shape: the registry's own `url_template` fills in
// the tile address, the resulting provider-relative path is matched against
// each declared `vector_tiles` fixture route, and that route's fixture is
// served. A descriptor declares no vector template, or no route matches the
// address, so no fixture is served.
function golden_vector_tile_fixture_path(tile_x, tile_y, zoom_level) {
  for (const provider_descriptor of PROVIDERS_REGISTRY.providers) {
    const url_template = provider_descriptor.url_template;
    if (
      typeof url_template !== 'string'
      || !url_template.includes('{z}')
      || !url_template.includes('{x}')
      || !url_template.includes('{y}')
    ) {
      continue;
    }
    const provider_relative_path = new URL(
      url_template
        .replace('{z}', String(zoom_level))
        .replace('{x}', String(tile_x))
        .replace('{y}', String(tile_y)),
    ).pathname.replace(/^\//, '');
    for (const provider_route of PROVIDER_FIXTURE_ROUTES_BY_KEY.get(
      provider_descriptor.provider_key,
    ) ?? []) {
      if (
        provider_route.fixture_route.purpose === 'vector_tiles'
        && provider_route.pattern.test(provider_relative_path)
      ) {
        return repository_path_of_relative_path(provider_route.fixture_route.fixture);
      }
    }
  }
  return MISSING_FIXTURE_PATH;
}

// Directory the golden fixtures live in, relative to the web root.
const GOLDEN_FIXTURES_DIRECTORY = path.resolve(
  ROOT_DIRECTORY,
  '../../crates/rustic-maps-rendering/fixtures/render/golden',
);

// The shared reference-scene artifact the platform refapps load for their
// demo content (docs/ReferenceAppUX.md): it lives one directory above the
// web root, so exactly this address is mapped onto it — anything else stays
// inside the static web root.
const REFERENCE_SCENES_REQUEST_URL = '/reference-scenes.json';
const REFERENCE_SCENES_FILE = path.resolve(
  ROOT_DIRECTORY,
  '../reference-scenes.json',
);
// The shared reference-flow artifact (docs/ReferenceAppUX.md
// §The flow schema): the page's `runReferenceFlow` runner fetches it, and the
// Playwright suite iterates it. It is served when present; before the artifact
// lands the page falls back to its built-in four-flow script and the address
// answers 404 exactly like any other missing static file.
const REFERENCE_FLOWS_REQUEST_URL = '/reference-flows.json';
const REFERENCE_FLOWS_FILE = path.resolve(
  ROOT_DIRECTORY,
  '../reference-flows.json',
);
const REFERENCE_SCENE_ARTIFACT = JSON.parse(
  readFileSync(REFERENCE_SCENES_FILE, 'utf8'),
);

// The recorded navigation traces the artifact's `demo_plans` entries name
// (M31): the browser Demo plans section replays one trace's samples through
// the wasm `Navigator`. The allowlist is built from the artifact's own
// `scenario` and `fixture_path` fields, so a new plan needs no server edit and
// the address can only name a trace the artifact declares (the same
// allow-listing rule the golden and offline-graph routes follow).
const NAVIGATION_TRACE_URL_PATTERN = /^\/navigation-traces\/([a-z0-9_]+)\.json$/;
const NAVIGATION_TRACE_FIXTURE_PATHS_BY_SCENARIO = new Map(
  (REFERENCE_SCENE_ARTIFACT.demo_plans ?? []).map((demo_plan) => [
    demo_plan.scenario,
    demo_plan.fixture_path,
  ]),
);

// The committed compiled offline routing graph archive the wasm seam
// (milestone M7: the wasm target loads the graph from in-memory bytes, no
// mmap) fetches same-origin: it lives beside its producer in the compiler
// crate's fixture directory (mirroring the recorded tile fixtures), so
// exactly this allow-listed address is mapped onto it.
const OFFLINE_GRAPH_FIXTURE_URL_PATTERN = /^\/offline-graph\/([a-z0-9_]+)\.rmog$/;
const OFFLINE_GRAPH_FIXTURE_FILE_NAMES = new Set(['routing_grid']);
const OFFLINE_GRAPH_FIXTURES_DIRECTORY = path.resolve(
  ROOT_DIRECTORY,
  '../../crates/rustic-maps-offline-graph-compiler/fixtures/rmog',
);

// The generated WebAssembly package's runtime dependencies (`@ubjs/core` and
// `@ubjs/wasm`) are ES modules with their own relative imports. The package's
// import map points those bare specifiers here, and this route serves the
// installed package files as-is: the browser loads exactly what `npm ci`
// installed, with no copy to fall out of sync with the lockfile.
const GENERATED_RUNTIME_REQUEST_PREFIX = '/generated-runtime/@ubjs/';
const GENERATED_RUNTIME_PACKAGE_DIRECTORY = path.resolve(
  ROOT_DIRECTORY,
  'node_modules',
  '@ubjs',
);

// The shared provider registry (M15): the single
// data source for provider descriptors, attribution, credential labels and
// fixture routes. The dev server never names a provider; it resolves every
// `/providers/{provider_key}/{provider-relative path}` request through this
// artifact. Descriptors carry no credential values.
const PROVIDERS_REGISTRY_REQUEST_URL = '/providers.json';
const PROVIDERS_REGISTRY_FILE = path.resolve(ROOT_DIRECTORY, '../providers.json');
const REPOSITORY_ROOT = path.resolve(ROOT_DIRECTORY, '../..');
const PROVIDERS_REGISTRY = JSON.parse(
  readFileSync(PROVIDERS_REGISTRY_FILE, 'utf8'),
);
const PROVIDERS_BY_KEY = new Map(
  PROVIDERS_REGISTRY.providers.map((descriptor) => [
    descriptor.provider_key,
    descriptor,
  ]),
);
// The registry owns the fixture route's base path ('/providers' in the
// committed artifact); the server only reads it.
const PROVIDER_FIXTURE_BASE_PATH = PROVIDERS_REGISTRY.fixture_base_path;
// One compiled entry per descriptor route, so a request never re-parses a
// provider's pattern.
const PROVIDER_FIXTURE_ROUTES_BY_KEY = new Map(
  PROVIDERS_REGISTRY.providers.map((descriptor) => [
    descriptor.provider_key,
    descriptor.fixture_routes.map((fixture_route) => ({
      method: fixture_route.method ?? 'GET',
      pattern: new RegExp(fixture_route.path_pattern),
      fixture_route,
    })),
  ]),
);
// The transition alias: a request under `/{provider_key}/…` (the pre-M15
// shape) resolves through the same descriptor routes. The alias is generic —
// it only fires for a key the registry actually declares.
const PROVIDER_ALIAS_REQUEST_PATTERN = /^\/([a-z0-9_-]+)\/(.*)$/;
// A path no route can serve, so the static handler answers 404.
const MISSING_FIXTURE_PATH = path.join(ROOT_DIRECTORY, '.nonexistent');

const CONTENT_TYPES_BY_EXTENSION = {
  '.html': 'text/html; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.wasm': 'application/wasm',
  '.json': 'application/json; charset=utf-8',
  '.webp': 'image/webp',
  '.png': 'image/png',
  '.mvt': 'application/vnd.mapbox-vector-tile',
  '.pbf': 'application/x-protobuf',
};

const server = createServer(async (request, response) => {
  try {
    const request_path = new URL(request.url, `http://${HOST}:${PORT}`).pathname;
    // A directory request serves its index page, the way the static site
    // does; every other path is a file request.
    const static_request_path = request_path.endsWith('/')
      ? `${request_path}index.html`
      : request_path;
    const filesystem_path =
      request_path === '/'
        ? path.join(ROOT_DIRECTORY, 'index.html')
        : filesystem_path_of_request(static_request_path, request.method);
    const body = await readFile(filesystem_path);
    const content_type =
      CONTENT_TYPES_BY_EXTENSION[path.extname(filesystem_path).toLowerCase()] ??
      'application/octet-stream';
    response.writeHead(200, { 'Content-Type': content_type });
    response.end(body);
  } catch {
    response.writeHead(404, { 'Content-Type': 'text/plain' });
    response.end('not found');
  }
});

// Resolves a repository-relative path inside the checkout, refusing anything
// that would escape it (descriptor fixture paths are trusted data, but the
// guard keeps a malformed descriptor from naming an arbitrary file).
function repository_path_of_relative_path(relative_path) {
  const resolved_path = path.resolve(REPOSITORY_ROOT, relative_path);
  return resolved_path.startsWith(`${REPOSITORY_ROOT}${path.sep}`)
    ? resolved_path
    : MISSING_FIXTURE_PATH;
}

// Resolves a tile-directory fixture route onto one committed recorded tile,
// or the nonexistent sentinel when the address names anything the fixture set
// cannot serve (an unsupported format or size segment, a zoom above the
// harness ceiling, or a tile index outside the zoom's valid range — the same
// traversal guard the `/tiles` route applies).
function tile_directory_fixture_path(fixture_route, fixture_match) {
  const captured_segments = fixture_match.groups ?? {};
  const zoom_level = Number.parseInt(captured_segments.tile_zoom_level ?? '', 10);
  const tile_x = Number.parseInt(captured_segments.tile_x ?? '', 10);
  const tile_y = Number.parseInt(captured_segments.tile_y ?? '', 10);
  const size_segment = captured_segments.size_segment;
  const format_segment = captured_segments.format;
  const route_is_complete =
    typeof fixture_route.fixture_directory === 'string' &&
    typeof fixture_route.image_format === 'string' &&
    typeof fixture_route.standard_size_segment === 'string' &&
    typeof fixture_route.retina_size_segment === 'string';
  const tile_count_at_zoom = 2 ** zoom_level;
  const resolution_marker_text =
    size_segment === fixture_route.retina_size_segment ? '@2x' : '';
  const address_is_servable =
    route_is_complete &&
    Number.isInteger(zoom_level) &&
    Number.isInteger(tile_x) &&
    Number.isInteger(tile_y) &&
    format_segment === fixture_route.image_format &&
    (size_segment === fixture_route.standard_size_segment ||
      size_segment === fixture_route.retina_size_segment) &&
    zoom_level >= 0 &&
    zoom_level <= MAXIMUM_TILE_ZOOM_LEVEL &&
    tile_x >= 0 &&
    tile_y >= 0 &&
    tile_x < tile_count_at_zoom &&
    tile_y < tile_count_at_zoom;
  if (!address_is_servable) {
    return MISSING_FIXTURE_PATH;
  }
  return path.join(
    repository_path_of_relative_path(fixture_route.fixture_directory),
    `${zoom_level}_${tile_x}_${tile_y}${resolution_marker_text}.${fixture_route.image_format}`,
  );
}

// Resolves one provider-relative request path through that provider's
// descriptor routes. A route either names a fixture file or a tile directory
// (whose named captures pick the tile); anything else 404s.
function provider_fixture_path(provider_key, relative_path, request_method) {
  const provider_routes = PROVIDER_FIXTURE_ROUTES_BY_KEY.get(provider_key);
  if (provider_routes === undefined) {
    return MISSING_FIXTURE_PATH;
  }
  for (const provider_route of provider_routes) {
    if (provider_route.method !== request_method) {
      continue;
    }
    const fixture_match = provider_route.pattern.exec(relative_path);
    if (fixture_match === null) {
      continue;
    }
    const fixture_route = provider_route.fixture_route;
    if (fixture_route.fixture_kind === 'tile_directory') {
      return tile_directory_fixture_path(fixture_route, fixture_match);
    }
    if (typeof fixture_route.fixture === 'string') {
      return repository_path_of_relative_path(fixture_route.fixture);
    }
  }
  return MISSING_FIXTURE_PATH;
}

// Splits a registry-keyed provider request into its provider key and
// provider-relative path. The canonical shape is
// `{fixture_base_path}/{provider_key}/{relative path}`; the transition alias
// `/{provider_key}/{relative path}` fires only for a declared provider key.
function provider_relative_request(request_path) {
  const canonical_prefix = `${PROVIDER_FIXTURE_BASE_PATH}/`;
  if (request_path.startsWith(canonical_prefix)) {
    const remainder = request_path.slice(canonical_prefix.length);
    const separator_index = remainder.indexOf('/');
    return separator_index < 0
      ? { provider_key: remainder, relative_path: '' }
      : {
          provider_key: remainder.slice(0, separator_index),
          relative_path: remainder.slice(separator_index + 1),
        };
  }
  const alias_match = PROVIDER_ALIAS_REQUEST_PATTERN.exec(request_path);
  if (alias_match !== null && PROVIDERS_BY_KEY.has(alias_match[1])) {
    return { provider_key: alias_match[1], relative_path: alias_match[2] };
  }
  return null;
}

// Resolves a generated-package runtime request onto the installed `@ubjs`
// package file it names, refusing any path outside that directory.
function generated_runtime_path(request_path) {
  const relative_path = request_path.slice(
    GENERATED_RUNTIME_REQUEST_PREFIX.length,
  );
  if (relative_path.length === 0 || relative_path.split('/').includes('..')) {
    return MISSING_FIXTURE_PATH;
  }
  const resolved_path = path.resolve(
    GENERATED_RUNTIME_PACKAGE_DIRECTORY,
    relative_path,
  );
  return resolved_path.startsWith(`${GENERATED_RUNTIME_PACKAGE_DIRECTORY}${path.sep}`)
    ? resolved_path
    : MISSING_FIXTURE_PATH;
}

// Maps a request path to the file it serves: a registry provider request
// resolves through the provider's descriptor routes, a `/tiles/{z}_{x}_{y}
// [@2x].png` address into the recorded fixture directory (guarding against a
// fixture address outside the zoom range, so the request can never name a
// file outside it), anything else into the static web root.
function filesystem_path_of_request(request_path, request_method) {
  if (request_path === REFERENCE_SCENES_REQUEST_URL) {
    return REFERENCE_SCENES_FILE;
  }
  if (request_path === REFERENCE_FLOWS_REQUEST_URL) {
    return REFERENCE_FLOWS_FILE;
  }
  if (request_path === PROVIDERS_REGISTRY_REQUEST_URL) {
    return PROVIDERS_REGISTRY_FILE;
  }
  if (request_path.startsWith(GENERATED_RUNTIME_REQUEST_PREFIX)) {
    return generated_runtime_path(request_path);
  }
  const provider_request = provider_relative_request(request_path);
  if (provider_request !== null) {
    return provider_fixture_path(
      provider_request.provider_key,
      provider_request.relative_path,
      request_method,
    );
  }
  const navigation_trace_match = NAVIGATION_TRACE_URL_PATTERN.exec(request_path);
  if (navigation_trace_match !== null) {
    const [, demo_plan_scenario] = navigation_trace_match;
    const trace_fixture_path =
      NAVIGATION_TRACE_FIXTURE_PATHS_BY_SCENARIO.get(demo_plan_scenario);
    return trace_fixture_path === undefined
      ? MISSING_FIXTURE_PATH
      : repository_path_of_relative_path(trace_fixture_path);
  }
  const offline_graph_fixture_match = OFFLINE_GRAPH_FIXTURE_URL_PATTERN.exec(request_path);
  if (offline_graph_fixture_match !== null) {
    const [, offline_graph_file_name] = offline_graph_fixture_match;
    return OFFLINE_GRAPH_FIXTURE_FILE_NAMES.has(offline_graph_file_name)
      ? path.join(OFFLINE_GRAPH_FIXTURES_DIRECTORY, `${offline_graph_file_name}.rmog`)
      : path.join(OFFLINE_GRAPH_FIXTURES_DIRECTORY, '.nonexistent');
  }
  const golden_vector_tile_match = GOLDEN_VECTOR_TILE_URL_PATTERN.exec(request_path);
  if (golden_vector_tile_match !== null) {
    const [, zoom_level_text, tile_x_text, tile_y_text] = golden_vector_tile_match;
    return golden_vector_tile_fixture_path(
      Number(tile_x_text),
      Number(tile_y_text),
      Number(zoom_level_text),
    );
  }
  const golden_fixture_match = GOLDEN_FIXTURE_URL_PATTERN.exec(request_path);
  if (golden_fixture_match !== null) {
    const [, golden_file_name] = golden_fixture_match;
    return GOLDEN_FIXTURE_FILE_NAMES.has(golden_file_name)
      ? path.join(GOLDEN_FIXTURES_DIRECTORY, `${golden_file_name}.png`)
      : path.join(GOLDEN_FIXTURES_DIRECTORY, '.nonexistent');
  }
  const tile_fixture_match = TILE_FIXTURE_URL_PATTERN.exec(request_path);
  if (tile_fixture_match !== null) {
    const [, zoom_level_text, tile_x_text, tile_y_text, resolution_marker_text] =
      tile_fixture_match;
    const zoom_level = parseInt(zoom_level_text, 10);
    const tile_x = parseInt(tile_x_text, 10);
    const tile_y = parseInt(tile_y_text, 10);
    const tile_count_at_zoom = 2 ** zoom_level;
    if (
      zoom_level > MAXIMUM_TILE_ZOOM_LEVEL ||
      tile_x >= tile_count_at_zoom ||
      tile_y >= tile_count_at_zoom
    ) {
      return path.join(TILE_FIXTURES_DIRECTORY, '.nonexistent');
    }
    return path.join(
      TILE_FIXTURES_DIRECTORY,
      `${zoom_level}_${tile_x}_${tile_y}${resolution_marker_text ?? ''}.png`,
    );
  }
  return path.join(ROOT_DIRECTORY, request_path);
}

export async function startStaticServer() {
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(PORT, HOST, resolve);
  });
  return `http://${HOST}:${PORT}`;
}

/** Stops the server a caller started, so a one-shot harness can exit. */
export async function stopStaticServer() {
  if (!server.listening) return;
  await new Promise((resolve) => server.close(resolve));
}

export { HOST, PORT };

const invoked_directly =
  process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href;
if (invoked_directly) {
  startStaticServer().then(() => {
    console.log(`serving ${ROOT_DIRECTORY} at http://${HOST}:${PORT}/`);
  });
  // Keep the process alive for the harness; Playwright shuts it down with
  // the run (SIGTERM), so no graceful shutdown listener is needed.
  process.on('SIGTERM', () => {
    setTimeout(() => process.exit(0), SERVER_UNAVAILABLE_DELAY_MILLISECONDS);
    server.close(() => process.exit(0));
  });
}