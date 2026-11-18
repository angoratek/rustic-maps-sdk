// The WebAssembly façade's camera contract (docs/Architecture.md, "WebAssembly
// camera members"): the canvas view holds one camera, `setCamera` adopts a
// pose that `camera()` reads back, `MapCamera.interpolate` answers exact
// endpoints and an eased midpoint, a preset click eases rather than jumping,
// and `cameraGeographicBounds` answers the stored pose's box. Every fact comes
// from the page's own seam or the bound wasm surface; the suite touches no
// network.
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, test, waitForMapTestReady } from './network_guard.mjs';

const TEST_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const REPOSITORY_ROOT = path.resolve(TEST_DIRECTORY, '..', '..', '..');

/** The shared camera presets the buttons are built from. */
const camera_presets = JSON.parse(
  readFileSync(path.join(REPOSITORY_ROOT, 'platform', 'reference-scenes.json'), 'utf8'),
).camera_presets;
/** The preset under test (the demo camera, then the route preview). */
const TARGET_PRESET = camera_presets[1];

/** How long the GPU renderer may take to answer readiness. */
const RENDERER_SETTLE_TIMEOUT_MILLISECONDS = 30_000;
/** The settled pose's tolerance in degrees (the eased float path). */
const SETTLED_POSE_TOLERANCE_DEGREES = 1e-6;
/** The page's vertical field of view. */
const FIELD_OF_VIEW_DEGREES = 60;
/** The viewport used for the bounds readback. */
const BOUNDS_VIEWPORT = { width: 800, height: 600 };

/** Awaits the GPU renderer the canvas view belongs to. */
async function waitForGpuRenderer(page) {
  await expect
    .poll(() => page.evaluate(() => window.mapCameraState?.().rendererMode), {
      timeout: RENDERER_SETTLE_TIMEOUT_MILLISECONDS,
    })
    .toBe('gpu');
}

test.beforeEach(async ({ page }) => {
  await page.goto('/?advanced');
  await waitForMapTestReady(page);
  await waitForGpuRenderer(page);
});

test('the view adopts a camera through setCamera and reads it back', async ({
  page,
}) => {
  const readback = await page.evaluate(() => {
    const read_camera_pose = (camera) => ({
      latitude: camera.center().latitude,
      longitude: camera.center().longitude,
      zoom: camera.zoomLevel(),
      bearing: camera.bearingDegrees(),
      pitch: camera.pitchDegrees(),
    });
    const view = window.mapGpuCanvas();
    const adopted = new window.mapTest.MapCamera(40.5, -120.25, 12.5, 45, 30);
    view.setCamera(adopted);
    // The pose is a validated immutable value: the readback is the adopted
    // values exactly, and the caller's instance stays usable.
    return {
      readback: read_camera_pose(view.camera()),
      adopted: read_camera_pose(adopted),
    };
  });
  expect(readback.readback).toEqual(readback.adopted);
  expect(readback.readback.latitude).toBe(40.5);
  expect(readback.readback.longitude).toBe(-120.25);
  expect(readback.readback.zoom).toBe(12.5);
  expect(readback.readback.bearing).toBe(45);
  expect(readback.readback.pitch).toBe(30);
});

test('MapCamera.interpolate answers exact endpoints and an eased midpoint', async ({
  page,
}) => {
  const interpolated = await page.evaluate(() => {
    const read_camera_pose = (camera) => ({
      latitude: camera.center().latitude,
      longitude: camera.center().longitude,
      zoom: camera.zoomLevel(),
      bearing: camera.bearingDegrees(),
      pitch: camera.pitchDegrees(),
    });
    const MapCamera = window.mapTest.MapCamera;
    const start = new MapCamera(38, -121, 10, 10, 20);
    const target = new MapCamera(39, -120, 14, 30, 40);
    return {
      start: read_camera_pose(start),
      target: read_camera_pose(target),
      at_zero: read_camera_pose(MapCamera.interpolate(start, target, 0)),
      at_one: read_camera_pose(MapCamera.interpolate(start, target, 1)),
      midpoint: read_camera_pose(MapCamera.interpolate(start, target, 0.5)),
    };
  });
  // The endpoints are the input poses verbatim, so an animation settles
  // exactly on its target.
  expect(interpolated.at_zero).toEqual(interpolated.start);
  expect(interpolated.at_one).toEqual(interpolated.target);
  // Smoothstep(0.5) is one half: zoom, bearing and pitch interpolate
  // linearly, and the longitude travels the shortest path.
  expect(interpolated.midpoint.zoom).toBe(12);
  expect(interpolated.midpoint.bearing).toBe(20);
  expect(interpolated.midpoint.pitch).toBe(30);
  expect(interpolated.midpoint.longitude).toBeCloseTo(-120.5, 9);
  expect(interpolated.midpoint.latitude).toBeGreaterThan(38);
  expect(interpolated.midpoint.latitude).toBeLessThan(39);
});

test('a preset click eases and the view stores the settled preset', async ({
  page,
}) => {
  const preset_button = page.locator(
    `[data-camera-preset-id="${TARGET_PRESET.id}"]`,
  );
  await expect(preset_button).toBeVisible();
  const sample = await page.evaluate((preset_id) => {
    const before_click = window.mapCameraState();
    document
      .querySelector(`[data-camera-preset-id="${preset_id}"]`)
      .click();
    return { before_click, immediately_after_click: window.mapCameraState() };
  }, TARGET_PRESET.id);
  // A jump would assign the pose inside the click handler; the eased move
  // starts from the pump, so the live pose is unchanged when the click
  // returns.
  expect(sample.immediately_after_click.latitude).toBe(sample.before_click.latitude);
  expect(sample.immediately_after_click.longitude).toBe(sample.before_click.longitude);
  expect(sample.immediately_after_click.zoom).toBe(sample.before_click.zoom);
  // The page settles on the preset, and the view's stored camera reports the
  // same pose.
  await expect
    .poll(async () => page.evaluate((preset) => {
      const state = window.mapCameraState();
      return Math.abs(state.latitude - preset.latitude_degrees)
          < 1e-6
        && Math.abs(state.longitude - preset.longitude_degrees)
          < 1e-6
        && state.zoom === preset.zoom_level
        && state.bearing === preset.bearing_degrees
        && state.pitch === preset.pitch_degrees;
    }, TARGET_PRESET), { timeout: 5_000 })
    .toBe(true);
  const settled_readback = await page.evaluate(() => {
    const stored = window.mapGpuCanvas().camera();
    return {
      latitude: stored.center().latitude,
      longitude: stored.center().longitude,
      zoom: stored.zoomLevel(),
      bearing: stored.bearingDegrees(),
      pitch: stored.pitchDegrees(),
    };
  });
  expect(
    Math.abs(settled_readback.latitude - TARGET_PRESET.latitude_degrees),
  ).toBeLessThan(SETTLED_POSE_TOLERANCE_DEGREES);
  expect(
    Math.abs(settled_readback.longitude - TARGET_PRESET.longitude_degrees),
  ).toBeLessThan(SETTLED_POSE_TOLERANCE_DEGREES);
  expect(settled_readback.zoom).toBe(TARGET_PRESET.zoom_level);
  expect(settled_readback.bearing).toBe(TARGET_PRESET.bearing_degrees);
  expect(settled_readback.pitch).toBe(TARGET_PRESET.pitch_degrees);
});

test('cameraGeographicBounds answers a sane box for the stored pose', async ({
  page,
}) => {
  const bounds = await page.evaluate(([
    viewport,
    field_of_view_degrees,
  ]) => {
    const view = window.mapGpuCanvas();
    view.setCamera(new window.mapTest.MapCamera(38.5766, -121.4944, 14, 0, 0));
    return view.cameraGeographicBounds(
      viewport.width,
      viewport.height,
      field_of_view_degrees,
    );
  }, [BOUNDS_VIEWPORT, FIELD_OF_VIEW_DEGREES]);
  expect(bounds).not.toBeNull();
  expect(bounds.southWestLatitude).toBeLessThan(38.5766);
  expect(bounds.northEastLatitude).toBeGreaterThan(38.5766);
  expect(bounds.southWestLongitude).toBeLessThan(-121.4944);
  expect(bounds.northEastLongitude).toBeGreaterThan(-121.4944);
  expect(bounds.northEastLatitude).toBeGreaterThan(bounds.southWestLatitude);
  expect(bounds.northEastLongitude).toBeGreaterThan(bounds.southWestLongitude);
});
