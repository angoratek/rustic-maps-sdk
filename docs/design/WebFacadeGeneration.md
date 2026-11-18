# Web façade generation from the frozen contract

**Status:** design investigation, 2026-11-17.
**Question it answers:** can `uniffi-bindgen-react-native`'s `wasm2` target generate the browser API from the UDL, and what must the one-module shape fix before the page can adopt it?

## Outcome

Adopt **one `wasm2` module owned by `rustic-maps-native-core`**, carrying the generated contract surface and the hand-written canvas, and migrate the page behind a `window.mapTest` compatibility façade. The module shape works; two blockers must be fixed first.

1. **Generation reads the wrong contract.** `ubrn` parses `src/rustic_maps.udl` from disk, but the wasm scaffolding is built from a copy with `MapView` stripped. The generated TypeScript therefore calls 42 wasm exports and 20 checksums the module does not have, and `uniffiInitAsync` rejects at `registerSync`. Generating from the stripped contract fixes it exactly.
2. **The real tree cannot generate at all.** `ubrn build wasm2` fails with `UDL file "rustic_maps" not found for crate "rustic_maps_native_core"`: the package name is `rustic-maps-native-core`, the library target is `rustic_maps`, and `ubrn`'s path supplier keys crates by the library target name.

Do not choose two modules: objects cannot cross the generated/wasm-bindgen boundary, and two modules would recreate two provider registries and two credential states.

## What `wasm2` generates here

Command, after building the crate for `wasm32-unknown-unknown`:

```sh
ubrn build wasm2 --config ubrn.config.yaml --release
```

The command compiles the lib, reads `UNIFFI_META_*` out of cargo's output, writes TypeScript, runs `wasm-bindgen` over the copy, and stages the module beside the bindings. Release output measured for this contract:

| Artifact | Size / lines |
|---|---|
| `rustic_maps.ts` (the API) | 679,923 bytes, 16,426 lines |
| `rustic_maps-ffi.ts` (signature table) | 139,437 bytes, 2,843 lines |
| `index.ts` (entrypoint) | 2,177 bytes, 55 lines |
| `rustic_maps.wasm` (staged) | 6,575,205 bytes; 5,887,591 after `wasm-opt -Oz` |
| `rustic_maps_bg.js` (wasm-bindgen glue) | 242,699 bytes |
| debug module, for comparison | 38,373,193 bytes |

The entrypoint exports `uniffiInitAsync(source)`, which opens the module once, registers the signature table, calls the namespace `initialize()` (checksums), and is idempotent. When the module imports `__wbindgen_placeholder__`, staging runs `wasm-bindgen`, keeps `<stem>_bg.js`, and the generated entrypoint imports it and hands it to the player as `resolveModule`; the player calls `__wbg_set_wasm` and `__wbindgen_start`. That is the canvas's carrier.

## Feature coverage

`wasm2` covers every construct this contract uses. The one gap is the stripped `MapView` interface, and it is a generation-input problem, not a bindgen limitation.

| UDL construct | Generated TypeScript | Verified |
|---|---|---|
| 61 dictionaries | `export type` + factory; optional fields `?:` | yes |
| 27 enums (unit variants) | numeric `export enum` | yes |
| `[Error] interface SdkError`, 11 data variants | tagged class union; detail in `.inner.detail` | yes |
| 22 interfaces (minus `MapView`) | handle-backed classes | yes |
| async members (42) | `Promise<T>`, optional `{ signal: AbortSignal }` | yes |
| `[Throws=SdkError]` | typed rejection, one class per variant | yes |
| `[Trait, WithForeign]` (12) | `XImpl` classes, vtable registration exports | yes |
| `sequence<T>` | `Array<T>` | yes |
| `bytes` | `ArrayBuffer` | yes |
| `u64` | `bigint`, 71 fields or arguments | yes |
| constructor `Name=fit_bounds` | static `fitBounds` | yes |
| single-threaded wasm | `uniffi_core` feature checked by `ubrn`; wasm `drive` awaits browser futures | yes |
| contract `MapView` (stripped on wasm) | generated anyway from the on-disk UDL; no matching Rust exports | **blocker** |

Runtime proof in Node against the built module: with bindings generated from the frozen UDL, `uniffiInitAsync` rejects with `register: wasm export "uniffi_rustic_maps_fn_clone_mapview" not found`. With bindings generated from the same module but the stripped contract, init succeeds; `getSdkCompiledVersion`, projection, `new MapSdk()`, `providerRegistrations()`, a JS-async `GeocodingProvider` invoked from Rust, a diagnostics listener, and a canvas-side wasm-bindgen class all work in the same instance.

## Canvas boundary

**One module can serve both, and wasm-bindgen exports survive.** The built module carries 5,351 exports: 306 UniFFI exports and metadata globals, plus wasm-bindgen classes (`mapview_new`, `mapview_renderFrame`, `mapssdk_create`, …). The staged module imports `./rustic_maps_bg.js` 640 times; the player wires the glue, and a canvas-side class answers calls after `uniffiInitAsync`. The `rustic-maps-native-core` wasm build already re-exports the canvas crate for exactly this reason.

**Objects do not cross the two ABIs.** A generated `MapSdk` instance is a JavaScript proxy over an integer UniFFI handle. The canvas glue's `MapView` constructor passes `maps.__wbg_ptr` into `wasm.mapview_new`; a generated object has no `__wbg_ptr`, and wasm-bindgen cannot accept a UniFFI handle. Only primitives and plain values cross. The current canvas root only carries the compiled version, so that crossing can be removed; the registry is already shared by mirroring `MapSdk::register_*` into the canvas slots on wasm.

## Recommended shape

- **Owner:** `rustic-maps-native-core` compiles the scaffolding and re-exports `rustic-maps-web-bindings` for `wasm32` only. The canvas crate keeps its wasm-bindgen classes; the contract crate keeps the ABI.
- **Generation input:** keep a committed stripped contract, `src/rustic_maps_wasm.udl`, generate the wasm scaffolding from it, and generate bindings from it too. A test asserts it equals the frozen UDL minus the native surface interface, the way the current split test does. This removes the build-script stripping and makes `ubrn`'s on-disk lookup correct.
- **Canvas naming split:** the generated contract `MapView` must not reach the page. Name the canvas class distinctly (`BrowserMapView`) or keep them in separate import namespaces and document it.
- **Page façade:** keep `window.mapTestReady` (now awaiting `uniffiInitAsync`) and `window.mapTest`. Map the 37 used members onto generated methods; adapt typed errors back to the existing `{ code, message }` shape and convert `bigint` to `number` where the page expects numbers.
- **Packaging:** build the module from `rustic-maps-native-core`, stage with `ubrn`, ship `_bg.js`, add `@ubjs/wasm` and `@ubjs/core` at the aligned version, and run `wasm-opt` in the packaging step (`ubrn` does not).

## What the best shape removes

Retired as generated: `exports.rs` (1,105 lines, contract mirror), `navigation.rs` (1,187), `positioning.rs` (187), `camera_interaction.rs` (83), `diagnostics.rs` (673), the provider-call half of `live_providers.rs` (roughly 1,000–1,500 of 2,007), `shared.rs` (166), `generated_contract_field_names.rs` (270), `platform/web/types/rustic_maps.d.ts` (1,104), `rustic_maps_surface.d.ts` (183), `generate-web-facade.mjs` (1,194) and its test (209). `contract_sync.mjs` (1,014) shrinks to the contract-split assertion.

Remains hand-written: `canvas.rs` (5,929), `vector_layer.rs` (357), `vector_source_fields.rs` (468), `refapp.rs` (600), `refapp_math.rs` (213), `diagnostics_filter.rs` (75), the canvas shared slots and browser transport in `live_providers.rs`, and a small declaration file for the untyped `_bg.js`.

## Effort and risk

| Slice | Size |
|---|---|
| Stripped-contract generation input; init and checksums green | S |
| Packaging switch; `uniffiInitAsync` in the page bootstrap | M |
| `window.mapTest` compatibility façade, error-tag and `bigint` adaptation | M–L |
| Retire hand-written mirrors, re-scope `contract_sync`, retire the generator | M |
| Canvas naming split and glue declarations | S |
| Generated WebAssembly API reference and release artifacts | S–M |

Roughly six slices, two to three rounds. Risks:

- `ubrn`'s own documentation still says the project "is still in early development, and should not yet be used in production"; `wasm2` exists only since `0.31.0-3`.
- The same `ubrn` version generates the React Native JSI bindings, so a bump must be proved on both paths.
- The optimized module grows from about 4.2 MB to 5.89 MB, under the 8 MB budget but about 40% larger.
- Async init is mandatory: nothing may touch the surface before `uniffiInitAsync` resolves. The page's current module-scope rule already matches this.
- wasm is single-threaded and per-instance; workers are the only isolation, and long Rust calls block the main thread.
- Error detail moves from `Error.message` to `SdkError.inner.detail`; `String(error)` becomes `SdkError.Variant`. Ten spec assertions read `.code` today and need the façade adapter or migration.
- `u64` becomes `bigint` in 71 places; page and test arithmetic must convert deliberately.
- `setProviderBaseUrl` and `clearProviderRegistrations` have no contract member and remain wasm-only seams.

## PLAN follow-up items

- [ ] M35 wasm2 generation input: commit the stripped wasm contract, generate scaffolding and bindings from it, and assert the split; `ubrn build wasm2` must initialize with every signature-table entry and checksum present.
- [ ] M35 wasm2 packaging: build the browser module from the binding crate, stage the wasm-bindgen glue, add the player runtime packages at the aligned version, and run `wasm-opt` in `build-wasm.sh`.
- [ ] M35 browser page migration: bootstrap with `uniffiInitAsync`, keep `mapTestReady`, and map every used `window.mapTest` member onto the generated surface with the error and `bigint` adaptations.
- [ ] M35 retire the hand-written contract mirror: remove the generated-instead files, re-scope `contract_sync.mjs` to the contract split, and retire `generate-web-facade.mjs` and the generated type mirror.
- [ ] M35 canvas naming split: keep the generated `MapView` off the page, name the canvas class distinctly, and ship declarations for the wasm-bindgen glue.
- [ ] M35 generated WebAssembly API reference: build the reference from the generated TypeScript and update the browser guides.

## Evidence

- Real tree: `ubrn build wasm2` exits 1 at generation with the UDL lookup error; the wasm build itself succeeds in about 45 s.
- Scratch copy with package and library names aligned: debug and release builds stage `rustic_maps.ts`, `rustic_maps-ffi.ts`, `index.ts`, `rustic_maps.wasm` and `rustic_maps_bg.js`.
- Stripped contract generated against the same module: 349 signature-table entries and 122 checksums, zero missing.
- Node harnesses: frozen-contract bindings fail init; stripped bindings initialize and serve sync calls, an async foreign provider callback, and a wasm-bindgen canvas class.
