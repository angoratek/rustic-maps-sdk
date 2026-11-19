// Browser launch flags shared by Playwright and its setup preflight. WebGPU is
// behind Chromium's own switch in headless mode: the GPU sandbox switch keeps
// the headless GPU process from crashing the adapter request. Dawn on Linux
// runners wants its Vulkan backend: without the feature flag the adapter
// initializes (`renderer_mode` reports `gpu`) but the frame loop then stalls —
// golden probes and pixel checks never settle, and the CI wasm-e2e job times
// out the WebGPU tests (2026-10-28 diagnosis). The Vulkan-on-SwiftShader
// pairing unblocks the shader path on a no-GPU Linux runner; on macOS hosts
// the flag is inert (the MoltenVK path is default). `--use-webgpu-adapter`
// names the software adapter explicitly: without it Chromium asks Dawn for a
// hardware adapter and `requestAdapter()` resolves to null on a runner with
// no GPU, which fails the suite's preflight before any test runs.
/** Environment value that selects the software-rendering WebGPU stack. */
export const SOFTWARE_WEBGPU_ENVIRONMENT_NAME = 'RUSTIC_MAPS_WEBGPU_SOFTWARE';

// On a GPU-less Linux runner ANGLE must render on Vulkan with SwiftShader's
// ICD, and Dawn must be told to take the software adapter; `--disable-vulkan-surface`
// keeps the headless GPU process from waiting for a window surface. A host
// with a real GPU keeps the previous ANGLE default.
const software_webgpu_args =
  process.env[SOFTWARE_WEBGPU_ENVIRONMENT_NAME] === '1'
    ? [
        '--use-angle=vulkan',
        '--use-webgpu-adapter=swiftshader',
        '--disable-vulkan-surface',
      ]
    : ['--use-angle=swiftshader'];

export const BROWSER_LAUNCH_OPTIONS = {
  args: [
    '--enable-unsafe-webgpu',
    '--enable-gpu',
    '--enable-features=Vulkan',
    '--use-vulkan=swiftshader',
    ...software_webgpu_args,
    '--no-sandbox',
  ],
};
