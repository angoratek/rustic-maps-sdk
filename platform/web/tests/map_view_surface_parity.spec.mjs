// Behavioural surface parity: the wasm package carries two `MapView` faces —
// the hand-written browser canvas class and the contract `MapView` the
// generated bindings expose. The contract face cannot host a drawing surface
// on wasm and refuses every surface-touching member with
// `RenderingSurfaceUnavailable` (pinned by the native-core unit tests). This
// test exercises the real wasm canvas class and pins that the browser path
// reports the same stable error code when a canvas cannot host a GPU surface,
// so the two faces cannot drift in the code a consumer branches on.
import { expect, test, waitForMapTestReady } from './network_guard.mjs';

test('the canvas MapView reports the contract surface-unavailable code', async ({
  page,
}) => {
  await page.goto('/?advanced');
  await waitForMapTestReady(page);

  const failure = await page.evaluate(() => {
    const canvas = document.createElement('canvas');
    // Claiming a 2D context first means the element cannot also host the
    // WebGPU surface the canvas renderer needs.
    canvas.getContext('2d');
    document.body.append(canvas);
    try {
      new window.mapTest.MapView(canvas);
      return null;
    } catch (error) {
      return {
        code: typeof error?.code === 'string' ? error.code : null,
        message: typeof error?.message === 'string' ? error.message : String(error),
      };
    } finally {
      canvas.remove();
    }
  });

  expect(failure, 'the canvas constructor must refuse a claimed canvas').not.toBeNull();
  expect(failure.code).toBe('renderingSurfaceUnavailable');
});
