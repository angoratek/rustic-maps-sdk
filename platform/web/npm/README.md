# `@angoratek/rustic-maps-sdk-web`

WebAssembly bindings for the Rustic Maps SDK, generated from the frozen
UniFFI contract by `uniffi-bindgen-react-native`'s `wasm2` target. The package
is distributed as a release asset (`rustic-maps-sdk-web-vX.Y.Z.tgz`) and
installed from the downloaded file, not from the npm registry:

```bash
npm install ./rustic-maps-sdk-web-v0.1.0.tgz
```

The package is a set of ES modules. Open the module and register the contract
namespace with `uniffiInitAsync`, naming the module file:

```js
import { uniffiInitAsync, getSdkCompiledVersion, MapSdk } from '@angoratek/rustic-maps-sdk-web';

await uniffiInitAsync(new URL(
  '@angoratek/rustic-maps-sdk-web/rustic_maps.wasm',
  import.meta.url,
));
console.log(getSdkCompiledVersion());
const mapSdk = new MapSdk();
```

The module loads its player runtime through the bare specifiers `@ubjs/wasm`
and `@ubjs/core`; both are runtime dependencies of this package. A page
without a bundler resolves them with an import map.

## Canvas

The contract cannot express a GPU canvas, so the package also ships the
wasm-bindgen glue `rustic_maps_bg.js` with hand-written declarations in
`rustic_maps_bg.d.ts`. Import the canvas class and camera from it:

```js
import { BrowserMapView, MapCamera, TileResolution } from '@angoratek/rustic-maps-sdk-web/rustic_maps_bg.js';

const view = new BrowserMapView(canvas, TileResolution.Standard);
await view.initialize();
```

## Typed records

The generated declarations (`index.d.ts`, `rustic_maps.d.ts`) type the
contract's records, enums, errors, and the `MapSdk`, `Navigator` and
`PositionController` classes:

```ts
import type { Place, Route } from '@angoratek/rustic-maps-sdk-web/rustic_maps';
```

Use `import type`: the subpath carries declarations, not runtime code.

## Browser support

WebGPU is required. Chromium-based browsers have supported it flag-free
since 113 (2023); Safari ships it from version 26; Firefox ships it since
141, with desktop first. It is a secure-context API, so the page must be
served over HTTPS (or localhost) and `navigator.gpu` is the feature test to
use — no user-agent sniffing.

## License

Dual-licensed under MIT OR Apache-2.0; see `LICENSE-MIT` and
`LICENSE-APACHE`. Third-party notices are in `THIRD_PARTY_NOTICES.md`.
