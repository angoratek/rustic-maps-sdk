# Live demo

<div class="demo-launch">

<a href="demo/">Open the live wasm demo</a>

</div>

Try the SDK in your browser: pan and zoom a rendered map, plan a recorded
route, follow a simulated trip with turn-by-turn guidance, and route over a
compiled archive. The demo runs the same WebAssembly module the package
ships, draws on a WebGPU canvas, and works without a provider key.

## What the demo needs in a browser

- **WebGPU.** Chromium-based browsers support it flag-free since 113. Safari
  ships it from version 26, and Firefox since 141 with desktop first. MDN's
  WebGPU page carries the live compatibility table.
- **A secure context.** The browser enables WebGPU only over HTTPS or
  localhost.
- **A runtime key for live calls.** Keyless, the demo renders recorded tiles,
  plans recorded routes, drives the simulated trip, and serves offline
  routes. Open **Settings** from the gear in the panel header to apply a
  MapQuest key for live MapQuest calls. The settings surface also lists the
  keyless Photon, OSRM and Valhalla providers, which need no key. Any key
  stays in the page's memory and is never persisted unless you opt in.

## Run the same loop locally

Every guide in this book has a WebAssembly tab that uses the same surface.
Install the wasm package and follow [Your first map](first-map.md) for the
whole loop.
