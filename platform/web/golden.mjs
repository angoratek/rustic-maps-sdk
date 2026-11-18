// Dedicated fixture page for the browser canvas-vs-golden compare
// (tests/golden_compare.spec.mjs): renders the two committed golden scenes
// and the four committed Sacramento overlay scenes through the wasm GPU canvas
// onto a fixed 512x512 surface, decodes the committed golden PNGs
// same-origin, and diffs rendered bytes against golden bytes fully in-page,
// exposing only per-sample error metrics to the test.
//
// Camera and viewport mirror `crates/rustic-maps-rendering/tests/golden_image.rs`
// exactly (both scenes, field of view, 512-pixel edge), so the only
// differences the compare sees are the GPU sampler differences the mirrored
// tolerance constants absorb. The overlay scenes mirror
// `crates/rustic-maps-rendering/tests/overlay_golden.rs` through the shared scene
// data in `overlay_scenes.mjs` (the same Sacramento scenes as the tile goldens,
// plus the overlay vocabulary's one shape per scene).
//
// The committed antimeridian overlay fixture
// (`world_zoom0_route_antimeridian.png`) has no browser counterpart by
// design: its base frame is drawn from synthetic gradient tiles the native
// harness generates in-memory (`overlay_golden.rs`), while the wasm canvas
// only ever draws recorded same-origin `/tiles` fixtures — no recorded
// zoom-0 imagery exists at the antimeridian, so the wrapped world-copy
// rendering stays verified by the native overlay goldens alone.
import { uniffiInitAsync } from './generated/index.js';
import {
  TileResolution,
  BrowserMapView,
  MapCamera,
} from './generated/rustic_maps_bg.js';
import { SACRAMENTO_OVERLAY_SCENES } from './overlay_scenes.mjs';

// Frame edge in pixels of both golden scenes; the canvas is sized to it at
// authoring time in `golden.html`, and the surface configure follows it.
const GOLDEN_VIEWPORT_EDGE_PIXELS = 512;
// `rustic-maps-rendering`'s `DEFAULT_FIELD_OF_VIEW_DEGREES`, which the golden
// harness rendered with.
const GOLDEN_FIELD_OF_VIEW_DEGREES = 60;
// Sacramento (California State Capitol) center, the fixed camera anchor of
// the golden scenes.
const GOLDEN_CENTER_LATITUDE_DEGREES = 38.5766;
const GOLDEN_CENTER_LONGITUDE_DEGREES = -121.4944;
// Tile zoom at which the golden scenes scheduled and rendered.
const GOLDEN_SCENE_TILE_ZOOM_LEVEL = 14;
// Bearing of both golden cameras: north up.
const GOLDEN_BEARING_DEGREES = 0;
// Tile resolution matching the recorded fixtures the native golden harness
// sampled (standard, one texel per logical tile pixel) — fixed, never
// derived from the device-pixel ratio, so the compare cannot drift if a
// retina context ever opened the page.
const GOLDEN_TILE_RESOLUTION = TileResolution.Standard;
// Same-origin route to the golden fixtures (allow-listed in `serve.mjs`).
const GOLDEN_FIXTURE_URL_PREFIX = '/golden/';
// The golden fixture files, named exactly as `serve.mjs` allow-lists them.
const TOP_DOWN_GOLDEN_FILE = 'sacramento_zoom14_top_down.png';
const PITCHED_GOLDEN_FILE = 'sacramento_zoom14_pitched45.png';
// One browser golden per named vector style (M21 residual): the same camera
// and the one committed golden vector tile, painted through the canvas-owned
// vector source. A style-artifact edit that changes what a style id paints
// fails these compares.
const GOLDEN_VECTOR_STYLE_GOLDEN_FILES = new Map([
  ['bright-light', 'sacramento_zoom14_bright_light.png'],
  ['fiord-dark', 'sacramento_zoom14_fiord_dark.png'],
  ['positron-light', 'sacramento_zoom14_positron_light.png'],
]);
// The golden vector tile's address and the same-origin template the style
// scenes install (serve.mjs answers only this address).
const GOLDEN_VECTOR_TILE_TEMPLATE_PATH = '/golden-vector/{z}/{x}/{y}.pbf';
const GOLDEN_VECTOR_TILE_MAXIMUM_ZOOM_LEVEL = 14;
const GOLDEN_VECTOR_TILE_SETTLE_ATTEMPTS = 30;
// The committed golden vector tile's center (z14/2661/6286), the style
// scenes' fixed camera so exactly that tile is visible.
const GOLDEN_VECTOR_TILE_CENTER_LATITUDE_DEGREES = 38.57393751557591;
const GOLDEN_VECTOR_TILE_CENTER_LONGITUDE_DEGREES = -121.519775390625;
// The regional scene: the recorded z9 regional tile over the demo's California
// region, painted by the Carto light style at the camera the native regional
// golden uses (`crates/rustic-maps-rendering/tests/vector_base_golden.rs`).
// The browser golden is captured from this same 512x512 frame, so the two
// goldens differ only in what the browser and native renderers make of one
// scene.
const REGIONAL_CARTO_GOLDEN_FILE = 'sacramento_region_zoom9_carto_light_browser.png';
const NATIVE_REGIONAL_CARTO_GOLDEN_FILE = 'sacramento_region_zoom9_carto_light.png';
const REGIONAL_TILE_ZOOM_LEVEL = 9;
// The recorded vector provider's declared maximum zoom level, the ceiling the
// installed source accepts; the scene itself only requests the z9 tile.
const REGIONAL_TILE_MAXIMUM_ZOOM_LEVEL = 15;
const REGIONAL_STYLE_ID = 'carto-light';
const REGIONAL_CENTER_LATITUDE_DEGREES = 38.5548;
const REGIONAL_CENTER_LONGITUDE_DEGREES = -121.2890625;
// Animation frames each probe samples across after a fresh presentation,
// keeping the minimum-error sample (a stale pre-presentation read is a
// clear-color frame and maximizes the error rather than matching). Same
// count as the WebGPU smoke test's probe loop.
const PROBE_SAMPLE_ATTEMPTS = 12;
// Channels in one RGBA sample of `getImageData` data.
const PIXELS_PER_RGBA_SAMPLE = 4;

// Renderer mode reporting, read by the test: 'gpu' once the WebGPU canvas
// initialized, 'overlay' after the documented degradation path, null until
// readiness resolved.
let renderer_mode = null;
let renderer_detail = '';
// Last frame statistics, read by the test before probing.
let last_frame_statistics = null;
// The GPU canvas construction promise, awaited by every frame and probe
// once readiness resolved.
let gpu_map_canvas_promise = null;
// The 2D canvases behind the in-page pixel loop: one probes the GPU
// surface, the other holds the decoded golden imagery (each 512x512, so
// drawImage never rescales).
const gpu_canvas = document.getElementById('golden-canvas');
const probe_canvas = document.createElement('canvas');
probe_canvas.width = GOLDEN_VIEWPORT_EDGE_PIXELS;
probe_canvas.height = GOLDEN_VIEWPORT_EDGE_PIXELS;
const probe_context = probe_canvas.getContext('2d', {
  willReadFrequently: true,
});
const golden_canvas = document.createElement('canvas');
golden_canvas.width = GOLDEN_VIEWPORT_EDGE_PIXELS;
golden_canvas.height = GOLDEN_VIEWPORT_EDGE_PIXELS;
const golden_context = golden_canvas.getContext('2d', {
  willReadFrequently: true,
});
const golden_pixels_by_bytes = new Map();

// Builds the golden scene camera at the given pitch (an immutable value
// type; the render consumes it, so every frame builds its own).
function build_vector_style_camera() {
  return new MapCamera(
    GOLDEN_VECTOR_TILE_CENTER_LATITUDE_DEGREES,
    GOLDEN_VECTOR_TILE_CENTER_LONGITUDE_DEGREES,
    GOLDEN_SCENE_TILE_ZOOM_LEVEL,
    GOLDEN_BEARING_DEGREES,
    0,
  );
}

// The regional scene's camera: top-down, north up at zoom 9, exactly the
// native regional golden's pose.
function build_regional_camera() {
  return new MapCamera(
    REGIONAL_CENTER_LATITUDE_DEGREES,
    REGIONAL_CENTER_LONGITUDE_DEGREES,
    REGIONAL_TILE_ZOOM_LEVEL,
    GOLDEN_BEARING_DEGREES,
    0,
  );
}

function build_camera(pitch_degrees) {
  return new MapCamera(
    GOLDEN_CENTER_LATITUDE_DEGREES,
    GOLDEN_CENTER_LONGITUDE_DEGREES,
    GOLDEN_SCENE_TILE_ZOOM_LEVEL,
    GOLDEN_BEARING_DEGREES,
    pitch_degrees,
  );
}

// Renders one golden scene frame, awaiting it (the wasm binding borrows its
// draw state across the future, so frames never overlap).
async function rendered_frame_statistics(pitch_degrees) {
  if (renderer_mode !== 'gpu') {
    throw new Error(`the GPU renderer degraded: ${renderer_detail}`);
  }
  const gpu_map_canvas = await gpu_map_canvas_promise;
  last_frame_statistics = await gpu_map_canvas.renderFrame(
    build_camera(pitch_degrees),
    GOLDEN_FIELD_OF_VIEW_DEGREES,
  );
  return last_frame_statistics;
}

// Reproduces the native golden harness's deliberate parent-fallback
// (`fill_pool_and_prepare` in `tests/golden_image.rs`): the last visible
// scene tile is withdrawn from the pool and its coarser parent (`x / 2`,
// `y / 2`, one zoom coarser — the native `TileKey::parent_tile_key` rule) is
// pooled through `stageParentTile`, so the frame samples the parent
// texture over that tile's span exactly like the committed goldens do.
//
// Both committed golden scenes list the same final tile (14/2663/6286), but
// the derivation is per-camera so it tracks the scene lists rather than
// duplicating addresses by hand.
async function stage_last_tile_parent_fallback(pitch_degrees) {
  if (renderer_mode !== 'gpu') {
    throw new Error(`the GPU renderer degraded: ${renderer_detail}`);
  }
  const gpu_map_canvas = await gpu_map_canvas_promise;
  const scene_tile_keys = build_camera(pitch_degrees).visibleTileKeys(
    GOLDEN_VIEWPORT_EDGE_PIXELS,
    GOLDEN_VIEWPORT_EDGE_PIXELS,
    GOLDEN_FIELD_OF_VIEW_DEGREES,
    GOLDEN_SCENE_TILE_ZOOM_LEVEL,
  );
  const fallback_tile_key = scene_tile_keys[scene_tile_keys.length - 1];
  if (!fallback_tile_key || fallback_tile_key.zoomLevel === 0) {
    throw new Error('the golden scene requests no tile below the world root');
  }
  const parent_tile_key = {
    tileX: Math.floor(fallback_tile_key.tileX / 2),
    tileY: Math.floor(fallback_tile_key.tileY / 2),
    zoomLevel: fallback_tile_key.zoomLevel - 1,
  };
  await gpu_map_canvas.stageParentTile(
    fallback_tile_key.tileX,
    fallback_tile_key.tileY,
    fallback_tile_key.zoomLevel,
    parent_tile_key.tileX,
    parent_tile_key.tileY,
    parent_tile_key.zoomLevel,
  );
  return { fallbackTileKey: fallback_tile_key, parentTileKey: parent_tile_key };
}

// Fetches and decodes a committed golden fixture, keeping its RGBA bytes
// for the pixel loop. The decoded edge is reported so the test can assert
// the 512x512 precondition.
async function golden_pixels(file_name) {
  const cached_pixels = golden_pixels_by_bytes.get(file_name);
  if (cached_pixels) {
    return cached_pixels;
  }
  const encoded_bytes = await (
    await fetch(`${GOLDEN_FIXTURE_URL_PREFIX}${file_name}`)
  ).arrayBuffer();
  const decoded_bitmap = await createImageBitmap(new Blob([encoded_bytes]));
  const pixels = {
    width_pixels: decoded_bitmap.width,
    height_pixels: decoded_bitmap.height,
    bytes: null,
  };
  golden_context.drawImage(decoded_bitmap, 0, 0);
  decoded_bitmap.close();
  pixels.bytes = golden_context.getImageData(
    0,
    0,
    GOLDEN_VIEWPORT_EDGE_PIXELS,
    GOLDEN_VIEWPORT_EDGE_PIXELS,
  ).data;
  golden_pixels_by_bytes.set(file_name, pixels);
  return pixels;
}

// The pixel loop itself, a faithful mirror of `compare_or_create_golden` in
// `golden_image.rs`: per channel an absolute error summed for the mean, per
// pixel a maximum channel error counted against the outlier fraction.
function compared_metrics(rendered_bytes, golden_bytes, outlier_channel_error) {
  const pixel_count = GOLDEN_VIEWPORT_EDGE_PIXELS * GOLDEN_VIEWPORT_EDGE_PIXELS;
  let absolute_error_sum = 0;
  let outlier_pixel_count = 0;
  for (
    let byte_index = 0;
    byte_index < rendered_bytes.length;
    byte_index += PIXELS_PER_RGBA_SAMPLE
  ) {
    let pixel_maximum_error = 0;
    for (let channel_index = 0; channel_index < PIXELS_PER_RGBA_SAMPLE; ++channel_index) {
      const channel_error = Math.abs(
        rendered_bytes[byte_index + channel_index] -
          golden_bytes[byte_index + channel_index],
      );
      absolute_error_sum += channel_error;
      pixel_maximum_error = Math.max(pixel_maximum_error, channel_error);
    }
    if (pixel_maximum_error > outlier_channel_error) {
      ++outlier_pixel_count;
    }
  }
  return {
    meanAbsoluteError: absolute_error_sum / (pixel_count * PIXELS_PER_RGBA_SAMPLE),
    outlierPixelFraction: outlier_pixel_count / pixel_count,
  };
}

// Probes one golden scene: re-presents the frame with an identical camera
// and samples the fresh surface across the following animation frames,
// keeping the minimum-error sample. Returns only the best metrics (plus the
// sampling evidence), never the 512x512x4 payload itself.
async function probe(pitch_degrees, golden_file_name, outlier_channel_error) {
  const golden = await golden_pixels(golden_file_name);
  let best_metrics = null;
  let best_sample_attempt = 0;
  for (let attempt = 0; attempt < PROBE_SAMPLE_ATTEMPTS; ++attempt) {
    await rendered_frame_statistics(pitch_degrees);
    // The freshly presented frame is readable right after the compositor
    // picks it up; sample the next animation frame's snapshot.
    await new Promise((resolve) => requestAnimationFrame(resolve));
    probe_context.drawImage(gpu_canvas, 0, 0);
    const metrics = compared_metrics(
      probe_context.getImageData(
        0,
        0,
        GOLDEN_VIEWPORT_EDGE_PIXELS,
        GOLDEN_VIEWPORT_EDGE_PIXELS,
      ).data,
      golden.bytes,
      outlier_channel_error,
    );
    if (best_metrics === null || metrics.meanAbsoluteError < best_metrics.meanAbsoluteError) {
      best_metrics = metrics;
      best_sample_attempt = attempt;
    }
  }
  return {
    ...best_metrics,
    sampleAttempts: PROBE_SAMPLE_ATTEMPTS,
    bestSampleAttempt: best_sample_attempt,
  };
}

// Page readiness: wasm module, SDK start, golden fixtures decoded, GPU
// canvas initialized (or the documented degradation applied). The test
// awaits this before every assertion.
window.mapGoldenReady = (async () => {
  await uniffiInitAsync(new URL('./generated/rustic_maps.wasm', import.meta.url));
  // The golden harness pins its tile resolution instead of following the
  // device ratio: the captured golden frames were rendered at that scale.
  const golden_sizes = {};
  for (const golden_file_name of [TOP_DOWN_GOLDEN_FILE, PITCHED_GOLDEN_FILE]) {
    const golden = await golden_pixels(golden_file_name);
    golden_sizes[golden_file_name] = {
      width_pixels: golden.width_pixels,
      height_pixels: golden.height_pixels,
    };
  }
  try {
    gpu_map_canvas_promise = (async () => {
      const canvas_renderer = new BrowserMapView(
        gpu_canvas,
        GOLDEN_TILE_RESOLUTION,
      );
      renderer_detail = `GPU adapter ${await canvas_renderer.initialize()}`;
      renderer_mode = 'gpu';
      return canvas_renderer;
    })();
    await gpu_map_canvas_promise;
  } catch (gpu_failure) {
    // Documented degradation path: no browser WebGPU (or a surface failure)
    // leaves the page inert; the test asserts the degradation and skips.
    gpu_map_canvas_promise = null;
    renderer_mode = 'overlay';
    renderer_detail = `WebGPU unavailable: ${String(
      gpu_failure?.message ?? gpu_failure,
    )}`;
  }
  return {
    rendererMode: renderer_mode,
    rendererDetail: renderer_detail,
    goldenSizes: golden_sizes,
    viewportEdgePixels: GOLDEN_VIEWPORT_EDGE_PIXELS,
  };
})();

// The window API the test drives: renderer mode + status detail, last frame
// statistics, one awaited frame, the pixel-loop probe, and the overlay
// scene surface (one committed Sacramento overlay scene applied at a time).
window.mapGoldenStatus = () => ({ rendererMode: renderer_mode, rendererDetail: renderer_detail });
window.mapGoldenFrameStatistics = () => last_frame_statistics;
window.mapGoldenRenderFrame = rendered_frame_statistics;
window.mapGoldenStageTileParentFallback = stage_last_tile_parent_fallback;
window.mapGoldenProbe = probe;

// Replaces the wasm canvas' overlay list with one committed Sacramento overlay
// scene's descriptor list (the `overlay_golden.rs` scenes; the scene list
// stays in force across the probe's re-renders), or clears it. Every
// wasm-side rejection is a string exception; the page surfaces it untouched
// so the test fails on an invalid scene.
window.mapGoldenSetOverlayScene = async (scene_key) => {
  if (renderer_mode !== 'gpu') {
    throw new Error(`the GPU renderer degraded: ${renderer_detail}`);
  }
  const overlay_scene = SACRAMENTO_OVERLAY_SCENES[scene_key];
  if (!overlay_scene) {
    throw new Error(`unknown overlay scene ${scene_key}`);
  }
  const gpu_map_canvas = await gpu_map_canvas_promise;
  gpu_map_canvas.setOverlays([overlay_scene]);
};

// Renders one named vector style over the committed golden vector tile and
// answers the source's status, so the caller knows the base actually drew.
async function render_vector_style_frame(style_id) {
  if (renderer_mode !== 'gpu') {
    throw new Error(`the GPU renderer degraded: ${renderer_detail}`);
  }
  const golden_file_name = GOLDEN_VECTOR_STYLE_GOLDEN_FILES.get(style_id);
  if (!golden_file_name) {
    throw new Error(`unknown vector style ${style_id}`);
  }
  const gpu_map_canvas = await gpu_map_canvas_promise;
  gpu_map_canvas.clearOverlays();
  gpu_map_canvas.setVectorTileSource({
    urlTemplate: `${window.location.origin}${GOLDEN_VECTOR_TILE_TEMPLATE_PATH}`,
    sourceName: 'golden-vector',
    attributionText: 'Committed golden vector tile',
    styleId: style_id,
    minimumZoomLevel: 0,
    maximumZoomLevel: GOLDEN_VECTOR_TILE_MAXIMUM_ZOOM_LEVEL,
  });
  let source_status = gpu_map_canvas.vectorTileSourceStatus();
  for (
    let attempt = 0;
    attempt < GOLDEN_VECTOR_TILE_SETTLE_ATTEMPTS
      && source_status.drawnTileCount === 0;
    ++attempt
  ) {
    last_frame_statistics = await gpu_map_canvas.renderFrame(
      build_vector_style_camera(),
      GOLDEN_FIELD_OF_VIEW_DEGREES,
    );
    source_status = gpu_map_canvas.vectorTileSourceStatus();
    if (source_status.drawnTileCount === 0) {
      await new Promise((resolve) => requestAnimationFrame(resolve));
    }
  }
  if (source_status.drawnTileCount === 0) {
    throw new Error(
      `the ${style_id} golden vector base drew no tile: ` +
        `${source_status.lastFailureDetail ?? 'no failure detail'}`,
    );
  }
  return source_status;
}

// Probes one named vector style against its committed golden.
window.mapGoldenProbeVectorStyle = async (
  style_id,
  outlier_channel_error,
) => {
  const golden_file_name = GOLDEN_VECTOR_STYLE_GOLDEN_FILES.get(style_id);
  if (!golden_file_name) {
    throw new Error(`unknown vector style ${style_id}`);
  }
  const source_status = await render_vector_style_frame(style_id);
  const golden = await golden_pixels(golden_file_name);
  let best_metrics = null;
  let best_sample_attempt = 0;
  for (let attempt = 0; attempt < PROBE_SAMPLE_ATTEMPTS; ++attempt) {
    const gpu_map_canvas = await gpu_map_canvas_promise;
    last_frame_statistics = await gpu_map_canvas.renderFrame(
      build_vector_style_camera(),
      GOLDEN_FIELD_OF_VIEW_DEGREES,
    );
    await new Promise((resolve) => requestAnimationFrame(resolve));
    probe_context.drawImage(gpu_canvas, 0, 0);
    const metrics = compared_metrics(
      probe_context.getImageData(
        0,
        0,
        GOLDEN_VIEWPORT_EDGE_PIXELS,
        GOLDEN_VIEWPORT_EDGE_PIXELS,
      ).data,
      golden.bytes,
      outlier_channel_error,
    );
    if (best_metrics === null || metrics.meanAbsoluteError < best_metrics.meanAbsoluteError) {
      best_metrics = metrics;
      best_sample_attempt = attempt;
    }
  }
  return {
    ...best_metrics,
    sampleAttempts: PROBE_SAMPLE_ATTEMPTS,
    bestSampleAttempt: best_sample_attempt,
    drawnTileCount: source_status.drawnTileCount,
    failedTileCount: source_status.failedTileCount,
  };
};

// Returns the freshly rendered style frame as a PNG data URL, used once to
// author a new committed golden when a style artifact intentionally changes.
window.mapGoldenVectorStyleImage = async (style_id) => {
  await render_vector_style_frame(style_id);
  const gpu_map_canvas = await gpu_map_canvas_promise;
  await gpu_map_canvas.renderFrame(build_vector_style_camera(), GOLDEN_FIELD_OF_VIEW_DEGREES);
  await new Promise((resolve) => requestAnimationFrame(resolve));
  return gpu_canvas.toDataURL('image/png');
};

// Renders the regional Carto scene over the recorded z9 tile and answers the
// source's status, so the caller knows the base actually drew.
async function render_regional_frame() {
  if (renderer_mode !== 'gpu') {
    throw new Error(`the GPU renderer degraded: ${renderer_detail}`);
  }
  const gpu_map_canvas = await gpu_map_canvas_promise;
  gpu_map_canvas.clearOverlays();
  gpu_map_canvas.setVectorTileSource({
    urlTemplate: `${window.location.origin}${GOLDEN_VECTOR_TILE_TEMPLATE_PATH}`,
    sourceName: 'golden-regional-vector',
    attributionText: 'Committed golden vector tile',
    styleId: REGIONAL_STYLE_ID,
    minimumZoomLevel: 0,
    maximumZoomLevel: REGIONAL_TILE_MAXIMUM_ZOOM_LEVEL,
  });
  let source_status = gpu_map_canvas.vectorTileSourceStatus();
  for (
    let attempt = 0;
    attempt < GOLDEN_VECTOR_TILE_SETTLE_ATTEMPTS
      && source_status.drawnTileCount === 0;
    ++attempt
  ) {
    last_frame_statistics = await gpu_map_canvas.renderFrame(
      build_regional_camera(),
      GOLDEN_FIELD_OF_VIEW_DEGREES,
    );
    source_status = gpu_map_canvas.vectorTileSourceStatus();
    if (source_status.drawnTileCount === 0) {
      await new Promise((resolve) => requestAnimationFrame(resolve));
    }
  }
  if (source_status.drawnTileCount === 0) {
    throw new Error(
      'the regional Carto golden vector base drew no tile: ' +
        `${source_status.lastFailureDetail ?? 'no failure detail'}`,
    );
  }
  return source_status;
}

// Probes the live regional frame against one committed golden, keeping the
// minimum-error sample across fresh presentations. The same pixel loop the
// native harness uses, so the browser golden (determinism) and the native
// golden (the cross-renderer measure) are compared on equal terms.
async function probe_regional_frame(golden_file_name, outlier_channel_error) {
  const source_status = await render_regional_frame();
  const golden = await golden_pixels(golden_file_name);
  let best_metrics = null;
  let best_sample_attempt = 0;
  for (let attempt = 0; attempt < PROBE_SAMPLE_ATTEMPTS; ++attempt) {
    const gpu_map_canvas = await gpu_map_canvas_promise;
    last_frame_statistics = await gpu_map_canvas.renderFrame(
      build_regional_camera(),
      GOLDEN_FIELD_OF_VIEW_DEGREES,
    );
    await new Promise((resolve) => requestAnimationFrame(resolve));
    probe_context.drawImage(gpu_canvas, 0, 0);
    const metrics = compared_metrics(
      probe_context.getImageData(
        0,
        0,
        GOLDEN_VIEWPORT_EDGE_PIXELS,
        GOLDEN_VIEWPORT_EDGE_PIXELS,
      ).data,
      golden.bytes,
      outlier_channel_error,
    );
    if (best_metrics === null || metrics.meanAbsoluteError < best_metrics.meanAbsoluteError) {
      best_metrics = metrics;
      best_sample_attempt = attempt;
    }
  }
  return {
    ...best_metrics,
    sampleAttempts: PROBE_SAMPLE_ATTEMPTS,
    bestSampleAttempt: best_sample_attempt,
    drawnTileCount: source_status.drawnTileCount,
    failedTileCount: source_status.failedTileCount,
  };
}

window.mapGoldenProbeRegionalGolden = (outlier_channel_error) =>
  probe_regional_frame(REGIONAL_CARTO_GOLDEN_FILE, outlier_channel_error);

// Compares the two committed regional goldens file-to-file: the deterministic
// cross-renderer measure, independent of the live canvas.
window.mapGoldenCompareRegionalGoldens = async (outlier_channel_error) => {
  const browser = await golden_pixels(REGIONAL_CARTO_GOLDEN_FILE);
  const native = await golden_pixels(NATIVE_REGIONAL_CARTO_GOLDEN_FILE);
  return {
    ...compared_metrics(browser.bytes, native.bytes, outlier_channel_error),
    widthPixels: browser.width_pixels,
    heightPixels: browser.height_pixels,
  };
};

// Returns the freshly rendered regional frame as a PNG data URL, used once to
// author the committed browser golden from the same scene the native golden
// pins.
window.mapGoldenRegionalImage = async () => {
  await render_regional_frame();
  const gpu_map_canvas = await gpu_map_canvas_promise;
  await gpu_map_canvas.renderFrame(build_regional_camera(), GOLDEN_FIELD_OF_VIEW_DEGREES);
  await new Promise((resolve) => requestAnimationFrame(resolve));
  return gpu_canvas.toDataURL('image/png');
};

window.mapGoldenClearOverlays = async () => {
  if (renderer_mode !== 'gpu') {
    throw new Error(`the GPU renderer degraded: ${renderer_detail}`);
  }
  (await gpu_map_canvas_promise).clearOverlays();
};

// ---------------------------------------------------------------------------
// Motion grading (M38): the settled frame after a pan/zoom gesture must be
// pixel-identical to the frame the same camera draws directly.
// ---------------------------------------------------------------------------

// The gesture starts a short pan and zoom away from the golden vector style
// camera, staying inside the one committed z14 tile's address so the scene
// keeps drawing the fixture. The camera zoom stays in [14, 15) so the served
// tile zoom remains 14 (the scheduler floors the camera zoom).
const MOTION_START_LATITUDE_OFFSET_DEGREES = 0.0008;
const MOTION_START_LONGITUDE_OFFSET_DEGREES = -0.0008;
const MOTION_START_ZOOM_LEVEL = GOLDEN_SCENE_TILE_ZOOM_LEVEL;
// The graded middle of the gesture: a zoom in and back out while the center
// pans toward the final pose.
const MOTION_INTERMEDIATE_ZOOMS = [14.2, 14.6, 14.3];

function build_motion_start_camera() {
  return new MapCamera(
    GOLDEN_VECTOR_TILE_CENTER_LATITUDE_DEGREES + MOTION_START_LATITUDE_OFFSET_DEGREES,
    GOLDEN_VECTOR_TILE_CENTER_LONGITUDE_DEGREES + MOTION_START_LONGITUDE_OFFSET_DEGREES,
    MOTION_START_ZOOM_LEVEL,
    GOLDEN_BEARING_DEGREES,
    0,
  );
}

function build_motion_intermediate_cameras() {
  return MOTION_INTERMEDIATE_ZOOMS.map((zoom_level, index) => {
    const progress = (index + 1) / (MOTION_INTERMEDIATE_ZOOMS.length + 1);
    return new MapCamera(
      GOLDEN_VECTOR_TILE_CENTER_LATITUDE_DEGREES
        + MOTION_START_LATITUDE_OFFSET_DEGREES * (1 - progress),
      GOLDEN_VECTOR_TILE_CENTER_LONGITUDE_DEGREES
        + MOTION_START_LONGITUDE_OFFSET_DEGREES * (1 - progress),
      zoom_level,
      GOLDEN_BEARING_DEGREES,
      0,
    );
  });
}

// Renders one frame, waits for the compositor to present it, and answers the
// frame statistics (the probe reads the same fresh frame right afterwards).
async function render_motion_frame(camera) {
  if (renderer_mode !== 'gpu') {
    throw new Error(`the GPU renderer degraded: ${renderer_detail}`);
  }
  const gpu_map_canvas = await gpu_map_canvas_promise;
  last_frame_statistics = await gpu_map_canvas.renderFrame(
    camera,
    GOLDEN_FIELD_OF_VIEW_DEGREES,
  );
  await new Promise((resolve) => requestAnimationFrame(resolve));
  return last_frame_statistics;
}

// Installs the Bright golden vector source and settles the start camera: a
// source install clears the scheduler, the drawn frame and the label-placement
// history, so both measurement runs begin from the same state.
async function settle_vector_style_source_at_start_camera(style_id) {
  // The existing style scene installs the source, awaits the fixture tile
  // through the canvas' own loop, and answers the source status; a second
  // frame at the same camera then carries the settled labels both measurement
  // runs start from.
  await render_vector_style_frame(style_id);
  const statistics = await render_motion_frame(build_motion_start_camera());
  if ((statistics.labelCount ?? 0) === 0) {
    throw new Error(
      `the ${style_id} motion scene placed no label at the start camera: ` +
        `${JSON.stringify(statistics)}`,
    );
  }
  return statistics;
}

// Reads the freshly presented GPU canvas into a byte array, exactly as the
// golden probe does.
function capture_gpu_canvas_bytes() {
  probe_context.drawImage(gpu_canvas, 0, 0);
  return new Uint8Array(
    probe_context.getImageData(
      0,
      0,
      GOLDEN_VIEWPORT_EDGE_PIXELS,
      GOLDEN_VIEWPORT_EDGE_PIXELS,
    ).data,
  );
}

// The frames a comparison reports, stripped to the fields the test asserts.
function motion_frame_facts(statistics) {
  return {
    motionGraded: statistics.motionGraded ?? false,
    labelCount: statistics.labelCount ?? 0,
    overlayTriangles: statistics.overlayTriangles ?? 0,
  };
}

window.mapGoldenCompareMotionSettledFrame = async () => {
  if (renderer_mode !== 'gpu') {
    throw new Error(`the GPU renderer degraded: ${renderer_detail}`);
  }
  // Direct: settle at the start camera, then draw the final camera once with
  // no gesture in progress. That frame is the ungraded reference.
  const direct_start_statistics = await settle_vector_style_source_at_start_camera(
    'bright-light',
  );
  const direct_statistics = await render_motion_frame(build_vector_style_camera());
  const direct_bytes = capture_gpu_canvas_bytes();

  // Gesture: the same state, then pan/zoom while the surface grades, ending
  // at the final camera. The last graded frame still owes the settle.
  const gesture_start_statistics = await settle_vector_style_source_at_start_camera(
    'bright-light',
  );
  const gpu_map_canvas = await gpu_map_canvas_promise;
  gpu_map_canvas.setCameraGestureActive(true);
  const graded_frame_facts = [];
  for (const camera of build_motion_intermediate_cameras()) {
    graded_frame_facts.push(motion_frame_facts(await render_motion_frame(camera)));
  }
  graded_frame_facts.push(motion_frame_facts(await render_motion_frame(
    build_vector_style_camera(),
  )));
  gpu_map_canvas.setCameraGestureActive(false);
  const settle_statistics = await render_motion_frame(build_vector_style_camera());
  const settle_bytes = capture_gpu_canvas_bytes();

  let differing_pixel_count = 0;
  let maximum_channel_difference = 0;
  for (
    let byte_index = 0;
    byte_index < direct_bytes.length;
    byte_index += PIXELS_PER_RGBA_SAMPLE
  ) {
    let pixel_maximum_difference = 0;
    for (let channel_index = 0; channel_index < PIXELS_PER_RGBA_SAMPLE; ++channel_index) {
      pixel_maximum_difference = Math.max(
        pixel_maximum_difference,
        Math.abs(direct_bytes[byte_index + channel_index] - settle_bytes[byte_index + channel_index]),
      );
    }
    maximum_channel_difference = Math.max(
      maximum_channel_difference,
      pixel_maximum_difference,
    );
    if (pixel_maximum_difference > 0) {
      differing_pixel_count += 1;
    }
  }
  return {
    directStartFrame: motion_frame_facts(direct_start_statistics),
    directFrame: motion_frame_facts(direct_statistics),
    gestureStartFrame: motion_frame_facts(gesture_start_statistics),
    gradedFrames: graded_frame_facts,
    settleFrame: motion_frame_facts(settle_statistics),
    differingPixelCount: differing_pixel_count,
    maximumChannelDifference: maximum_channel_difference,
  };
};

// Captures the canvas' own `snapshot()` twice, once after each of two
// renders at the same camera pose, and answers the byte-level agreement
// between the captures together with the first capture's golden metrics.
//
// This is the capture-free path: `snapshot()` replays the presented frame's
// draw list into an offscreen texture and copies it to a mapped buffer, so
// no compositor re-present (`drawImage`) is involved. A settled frame is
// drawn first so label placement history is established; the measured pair
// then differs only if the frame itself is nondeterministic.
window.mapGoldenSnapshotDeterminism = async (style_id, outlier_channel_error) => {
  const golden_file_name = GOLDEN_VECTOR_STYLE_GOLDEN_FILES.get(style_id);
  if (!golden_file_name) {
    throw new Error(`unknown vector style ${style_id}`);
  }
  await render_vector_style_frame(style_id);
  const gpu_map_canvas = await gpu_map_canvas_promise;
  // The first frame after the install establishes the label-placement
  // history the measured pair reuses. Every frame builds its own camera: the
  // binding is consumed by value.
  await gpu_map_canvas.renderFrame(
    build_vector_style_camera(),
    GOLDEN_FIELD_OF_VIEW_DEGREES,
  );
  await new Promise((resolve) => requestAnimationFrame(resolve));
  last_frame_statistics = await gpu_map_canvas.renderFrame(
    build_vector_style_camera(),
    GOLDEN_FIELD_OF_VIEW_DEGREES,
  );
  const first_bytes = await gpu_map_canvas.snapshot();
  last_frame_statistics = await gpu_map_canvas.renderFrame(
    build_vector_style_camera(),
    GOLDEN_FIELD_OF_VIEW_DEGREES,
  );
  const second_bytes = await gpu_map_canvas.snapshot();

  let differing_byte_count = 0;
  for (
    let byte_index = 0;
    byte_index < Math.min(first_bytes.length, second_bytes.length);
    ++byte_index
  ) {
    if (first_bytes[byte_index] !== second_bytes[byte_index]) {
      ++differing_byte_count;
    }
  }
  differing_byte_count += Math.abs(first_bytes.length - second_bytes.length);
  const golden = await golden_pixels(golden_file_name);
  const metrics = compared_metrics(
    first_bytes,
    golden.bytes,
    outlier_channel_error,
  );
  return {
    byteLength: first_bytes.length,
    secondByteLength: second_bytes.length,
    expectedByteLength:
      GOLDEN_VIEWPORT_EDGE_PIXELS * GOLDEN_VIEWPORT_EDGE_PIXELS * PIXELS_PER_RGBA_SAMPLE,
    differingByteCount: differing_byte_count,
    meanAbsoluteError: metrics.meanAbsoluteError,
    outlierPixelFraction: metrics.outlierPixelFraction,
  };
};
