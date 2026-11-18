# `@angoratek/rustic-maps-sdk-web`

WebAssembly bindings for the Rustic Maps SDK, produced by `wasm-bindgen`
from the frozen UniFFI contract. The package is distributed as a release
asset (`rustic-maps-sdk-web-vX.Y.Z.tgz`) and installed from the downloaded
file, not from the npm registry:

```bash
npm install ./rustic-maps-sdk-web-v0.1.0.tgz
```

The module is the wasm-bindgen `web` target: serve `rustic_maps_web_bindings.js` and
`rustic_maps_web_bindings_bg.wasm` from the same origin and import the shim with
`await init()`:

```js
import init, { getSdkCompiledVersion } from '@angoratek/rustic-maps-sdk-web';

await init();
console.log(getSdkCompiledVersion());
```

## Typed records

The generated declarations type record-shaped answers generically (`object`,
`Array<any>`). The package also ships `rustic_maps.d.ts`, a generated mirror of
the frozen contract's records, enums and error unions, which
`scripts/generate-web-facade.mjs` writes from the UDL and the repository's
contract-sync test drift-checks:

```ts
import type { PlaceRow, Route } from '@angoratek/rustic-maps-sdk-web/rustic_maps';
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
