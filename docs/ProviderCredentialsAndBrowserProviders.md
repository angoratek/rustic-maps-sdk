# Provider credentials at runtime and live browser providers

This document records the owner's 2026-10-29 direction and the design for it:
reference applications must stop embedding provider credentials at build time,
let users enter credentials at runtime from a settings surface, fail honestly
when no credential is present, and grow cleanly to future providers. The
browser reference application goes further: it gains a live provider path, so
the wasm demo can take a key instead of relying only on recorded fixtures.

## 1. Problem and threat model

The reference applications originally received the MapQuest key at build time
(`BuildConfig.MAPQUEST_API_KEY` on Android, an `Info.plist` build setting on
iOS, and the same two seams in the React Native shells) before the
runtime-credential change below removed them. Anyone who can obtain
a refapp binary can recover a key baked into it — keys that may be internal or
shared with a distribution channel. A demo application should not be the place
where a production credential lives.

The threat model is therefore:

- **No provider credential may be embedded in a refapp artifact.** Removing
  the build-time seam is the primary mitigation, not encrypting a baked key.
- **A credential entered at runtime is user-controlled.** It may be held for
  the session only, or persisted on the device behind an explicit opt-in and a
  platform secure store. It is never written to logs, diagnostics, error
  text, screenshots, or the shared demo artifact.
- **The browser is different.** A key entered into a browser page is visible
  to the page and in the network panel by construction; the goal is to avoid
  shipping one by default, not to promise secrecy from the page's own user.

## 2. Recorded decisions (owner, 2026-10-29)

1. **Persistence: both, opt-in.** Session-only by default; a "Remember on this
   device" toggle persists the credential in the platform secure store
   (iOS Keychain, Android Keystore AES/GCM). Off means the value
   is gone when the app process ends.
2. **Build-time keys: removed.** Delete the `MAPQUEST_API_KEY` build field,
   `Info.plist` entry, `local.properties` wiring and `Local.xcconfig` key
   handling from every refapp. CI stays keyless; local developers enter a key
   at runtime like everyone else.
3. **Wasm: live providers.** The browser refapp gains a provider path, not a
   disabled-with-reason panel. The ubrn web binding route is evaluated as a
   binding-alignment option (see §8).
4. **Extensible provider slots.** The settings surface is driven by a
   provider-credential descriptor list, not a hard-coded MapQuest field.
   Every vendor crate the provider layer publishes has a descriptor here, a
   catalog row and a dispatch row; adding another is those three changes,
   with no rework of the view.

### Live provider smoke

`scripts/smoke-providers.mjs` exercises every provider the shared registry advertises against its served
contracts in a real browser: geocoding, place search and type-ahead, routing, the distance matrix, traffic and
raster tiles. It reads the provider keys from `~/.everymap/config.toml` on the machine it runs on, never prints or
writes them, and never runs in CI or the test suites because the requests are live. `--local` points it at the
development server, `--base-url` at any deployment, and `--strict` exits nonzero on a failure. Each row names the
provider, the contract and a redacted result, so a failure can be reported without a credential.

## 3. Settings surface

Every refapp exposes a settings button (a gear control near the readiness
chip) that opens a settings view:

| Area | Contents |
|---|---|
| Provider credentials | One row per provider descriptor: display name, credential label, masked secure entry, a status line, Apply and Clear |
| Persistence | "Remember on this device" toggle (default off); the secure-store availability is reported honestly when it fails |
| Map SDK settings | The settings that are meaningful per platform today (e.g. the theme toggle stays in the sheet; the browser adds backend/canvas facts); the section is a list so later SDK knobs join it |
| Attribution and terms | The provider's attribution/terms line, carried by the descriptor where known |

Behavior rules:

- **Masked entry.** The credential field is a secure text field, never echoed
  back in full. A short confirmation may show only the last four characters.
- **Apply is explicit.** Applying registers the providers through the SDK's
  existing registration calls; success flips the readiness/credential status
  and retries initialization. A refusal is shown verbatim.
- **Clear is honest.** Clearing removes the stored credential and the UI
  state. The SDK's registry has no unregister operation, so the app states
  that a restart is required for the running session to become keyless again.
- **No key is a first-class state.** With no credential, the app starts
  without providers, renders cached/recorded imagery where available, reports
  "Set a provider key to load live map data" (not a crash or a scary failure),
  and offers the settings action. Operations that need a provider surface the
  typed `ProviderNotConfigured` error verbatim.
- **Extensible rows.** The view renders the shared provider registry's
  descriptors (`platform/providers.json`); a provider the SDK does not
  register yet renders disabled-with-reason rather than pretending to work.

## 4. Shared descriptor schema

`platform/providers.json` is the single descriptor source every refapp, the
dev server and the playground consume (M15; `platform/providers.json`).
Each entry carries the settings row's fields:

```json
"providers": [
  {
    "provider_key": "mapquest",
    "display_name": "MapQuest",
    "credential_label": "API key",
    "registration_available": { "android": true, "ios": true, "react_native": true, "wasm": true },
    "attribution_text": "Map data © OpenStreetMap contributors (ODbL) — tile imagery © MapQuest",
    "terms_note": "Provider-specific rate limits and caching terms.",
    "settings_hint": "Create a key in the provider's developer console."
  }
]
```

Future entries add a row and a registration dispatch arm; no view rewrite.
The registry stays credential-free (descriptors only, never values). The demo
artifact (`platform/reference-scenes.json`) carries no provider descriptors.

The registry's top level may also declare the keyless start's styled base:

```json
"keyless_base_map": { "provider_key": "openfreemap", "style_id": "bright-light" }
```

A keyless refapp still registers the credential-less raster descriptor so the
shipped surface can attach, then installs this vector style over it once the
map view exists (owner ruling 2026-10-31, `PRD.md` §6 item 18). A registry that
omits the field keeps the round-39 raster start; an explicit base-map choice
wins over it. The browser demo is not affected: it starts raster and offers
the vector bases one tap away.

## 5. Per-platform implementation

### Android (Compose)

- `MapsSdkFacade` becomes runtime-credentialed: it constructs the SDK without
  registering providers, takes the shared registry's provider descriptors,
  exposes `applyProviderCredential(providerKey, credential)` (registering
  every capability the descriptor declares through the generic
  `configureProvider` call, and repeating the descriptor's provider key and
  credential through `setProviderCredentialWithTileCache` for the tile-cache
  registration) and a keyless `ensureStarted()` path.
- `ReferenceApplicationState` owns a `ProviderCredentialState`
  (descriptors, entered value, applied/persisted/status) and the settings
  visibility; the readiness ladder gains the neutral keyless state.
- The settings view is a Compose dialog/sheet opened by a gear control next
  to the readiness chip; it renders the shared registry's descriptor rows.
- Persistence: Keystore AES/GCM private preferences behind the opt-in toggle; the
  availability failure path reports honestly and falls back to session-only.
- Delete `buildConfigField MAPQUEST_API_KEY` and the key guidance string that
  points at `local.properties`.

### iOS (SwiftUI)

- `ReferenceApplicationModel` starts keyless; the Info.plist key lookup and
  `missingApiKeyFailureDetail` failure path are removed in favor of a neutral
  keyless readiness state and `applyProviderCredential(_:_:)`.
- A SwiftUI settings sheet with a `SecureField`, the remember toggle, and the
  descriptor-driven rows; Keychain (`kSecClassGenericPassword`, service
  `com.angoratek.rustic-maps.refapp.provider-credentials`) backs the opt-in.
- Delete the `MAPQUEST_API_KEY` Info.plist entry and the `Local.xcconfig`
  key path from the refapp.

### React Native

- Both native controllers start keyless; a new
  `setProviderCredential(providerKey, credential)` `@ReactMethod` dispatches
  to the same registration path and re-emits readiness.
- A JS settings sheet (masked `TextInput`, remember toggle, descriptor rows
  from the Metro asset) calls the bridge; native secure storage methods
  (`getStoredCredential`/`setStoredCredential`/`clearStoredCredential`) use
  Keychain on iOS and Android Keystore AES/GCM private preferences on Android.
- Delete the `BuildConfig`/`Info.plist` key plumbing from both shells.

### WebAssembly

- The wasm SDK gains provider registration backed by a browser `fetch`
  transport (§7); the playground's settings panel takes the key and applies
  it, with the same descriptor rows and honest keyless state.
- Fixtures remain the default and the offline path; a key only adds live
  provider data.

### 5.5 Automation injection (runtime, never build-time)

Local and CI UI automation must be able to run keyed without reintroducing a
build-time key. Each refapp therefore supports a documented **runtime
injection seam**:

- **Android**: a launch-intent extra `providerApiKey` (`adb shell am start -e
  providerApiKey …`, or an `ActivityScenario` intent in the instrumentation
  suite) is applied through the same registration path as the settings
  surface; the optional `providerKey` extra names its provider, defaulting to
  MapQuest, the documented full-service provider. A build with several
  credentialed providers requires the explicit key.
- **iOS**: the `PROVIDER_API_KEY` launch-environment variable is applied at
  application start, optionally paired with `PROVIDER_KEY`; the XCUITest suite
  forwards both from the test process. Absent the key name, MapQuest is the
  target; a build with several credentialed providers requires the explicit
  key.
- **React Native**: the Android `providerApiKey`/`providerKey` launch-intent
  extras and the iOS `PROVIDER_API_KEY`/`PROVIDER_KEY` launch environment are
  read by the native controller at SDK start and applied via
  `setProviderCredential`.

The injected value is never persisted by the injection itself, never logged,
and is not compiled into any artifact. A run without the variable stays on the
neutral keyless path; tests that need live/surface data skip rather than fail.
CI supplies the variable only to the UI-test job when a key is configured.

### 5.6 Running keyed UI suites

```bash
# Android (the intent extra is forwarded from the host environment; the
# optional providerKey runner argument names the provider)
PROVIDER_API_KEY=… ./gradlew :reference-application:connectedDebugAndroidTest \
  -Pandroid.testInstrumentationRunnerArguments.providerApiKey="$PROVIDER_API_KEY"

# iOS (the XCUITest reads PROVIDER_API_KEY from its own process and forwards
# it to the app's launch environment; the app applies it at start.
# PROVIDER_KEY is optional.) The TEST_RUNNER_ prefix is required: xcodebuild
# does not forward the shell environment to the UI-test runner, so a bare
# PROVIDER_API_KEY makes every keyed case skip without an error.
TEST_RUNNER_PROVIDER_API_KEY=… \
TEST_RUNNER_PROVIDER_KEY=mapquest \
xcodebuild test -project ReferenceApplication.xcodeproj \
  -scheme ReferenceApplication -sdk iphonesimulator \
  -destination 'platform=iOS Simulator,name=iPhone 17 Pro'
```

Keyless runs exercise the neutral prompt and skip provider-dependent cases;
keyed runs execute the raster, search, routing and navigation checks. The
Android suite also carries a key-gated live search → route flow. Verified
2026-10-31 on both standing devices: the bare-variable form skipped all five
keyed iOS cases, the `TEST_RUNNER_` form ran them.

Keyed screenshot captures use the same seams for one session and publish no
credential. `docs/ScreenshotCapture.md` records the per-platform commands and
the revision of the published set.

## 6. Security rules (enforced by review and tests)

1. No credential in build inputs, source, fixtures, docs, screenshots or the
   shared artifact. **One recorded exception (owner ruling 2026-10-30, M24):**
   the build made *for the hosted demo* carries a dedicated, separately
   issued credential, injected on the build machine from a repository secret
   and never present in the repository. See §6.1.
2. No credential in logs, diagnostics lines, error detail text or crash
   reports; redaction is tested by a grep-style gate where practical.
3. Masked fields only; clipboard paste allowed, no "show key" affordance.
4. Persistence only through the platform secure store, only when the user
   opts in; the toggle's default is off. **One recorded browser exception
   (owner ruling 2026-11-17):** the browser has no platform secure store, so
   the demo may persist a key in this origin's `localStorage` only behind the
   explicit opt-in and a visible warning that the value is stored in readable
   form and a shared computer should not use the option. Native targets keep
   the secure store and the off default. Automation injection is session-only
   and never persisted.
5. Clear removes both memory and secure-store copies; the running-session
   limitation is stated, not hidden.
6. CI never receives a provider key; live provider testing stays local and
   fixture-backed in automation.

### 6.1 The hosted demo's own credential (owner ruling 2026-10-30)

The Appetize-hosted demo is the one artifact that may carry a credential,
because a visitor comparing SDKs should see a real map rather than a mock
scene, and because a build living inside Appetize's service is not a
downloadable distribution. The exception is bounded:

- **Dedicated**: the key is issued for the demo alone, rotated on a schedule,
  and never the key the fixture tools or the live-smoke example use. Its abuse
  is therefore cheap to detect and revoke.
- **Injected, never stored**: `scripts/inject-hosted-demo-credential.sh`
  writes `assets/hosted-demo-credential.json` into the Android app on the
  build machine from `HOSTED_DEMO_PROVIDER_API_KEY`; the iOS app reads the
  same value from `Info.plist`, expanded from the build setting of that name.
  Both paths are gitignored and produced only inside
  `.github/workflows/hosted-demo.yml`; `--clean` removes the Android file so a
  local build cannot silently stay keyed.
- **Runtime semantics unchanged**: the app applies it through the same
  registration path as the settings surface and the automation seam — never
  persisted to the Keystore/Keychain, never logged, gone when the process
  ends. A build without the injected value is keyless and still works.
- **Permissions stay off**: the hosted apps keep `appPermissions.adbConnect`,
  `networkProxy` and `debugLog` disabled, so a visitor cannot attach to the
  running build, and no API path serves the binary.
- **Still forbidden**: any credential in a *distributed* artifact (GitHub
  Release, npm/Maven/SwiftPM package, docs download), in the repository, in
  fixtures, or in a screenshot. Those stay keyless and runtime-entered.

## 7. Live browser providers

### Feasibility (measured 2026-10-29)

- MapQuest geocoding and static tiles return `access-control-allow-origin: *`
  on a preflight-style request (HTTP 401 with the header present), so browser
  fetches are allowed.
- `tiles.openstreetmap.us` returns `access-control-allow-origin: http://localhost:8642`
  for the committed vector fixture route, so the OSM US vector source is
  fetchable from the demo origin.
- The blocker was the Rust HTTP client, not CORS: `reqwest` and `tokio` did
  not build for `wasm32-unknown-unknown`. The provider layer now selects its
  transport per target, so a browser build sends the same request through
  `window.fetch`; key injection, bounded retries and status mapping live in
  that shared client rather than in a vendor crate. The SDK's
  provider layer's client carries its own pacing, and the SDK's observation
  module (`crates/rustic-maps-core/src/provider_observation.rs`) is the only
  place the diagnostics vocabulary lives.

### Style resources in the browser (2026-11-13)

A provider-published style document names its own glyph ranges and sprite
sheet. The credential only travels to the document's origin and the hosts the
provider declares, so the browser canvas applies the same rule the native
surface does: an address on any other host is refused before a request, with
the host named. The demo's fixture mode serves the same code path from the
harness origin, which is what the suite exercises.

Whether each vendor's asset host sends `access-control-allow-origin` for a
page origin is not yet measured. The live path needs one maintainer check with
a key entered at runtime; until then the fixture path is the verified one.

### Transport abstraction

Introduce a small transport seam so one provider implementation serves native
and browser targets:

```rust
pub trait HttpTransport: Send + Sync {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, TransportError>;
}
```

- `HttpRequest`/`HttpResponse` are plain byte/header records (method, URL,
  headers, body, status, bytes).
- Native: a `reqwest` transport preserves today's connect/read timeouts.
- Browser: a `web-sys`/`gloo-net` fetch transport implements the same trait;
  `HttpResponse` carries the body bytes.
- The rate limiter and retry backoff move to a portable async mutex and a
  portable sleep (`web-time` + a wasm timer on the browser, `tokio::time` on
  native), behind a small `ProviderClock` seam so tests stay deterministic.
- The provider layer selects its transport per target: `reqwest` and `tokio`
  are native dependencies, and the browser build pulls the fetch transport
  instead. `rustic-maps-web-bindings` exports a registration function that
  installs the provider implementations into the wasm registry.

### Testing

- Native behaviour stays wiremock-tested.
- Browser: a same-origin fixture endpoint in `serve.mjs` (already the pattern
  for tiles) stands in for the provider; a Playwright spec enters a key,
  applies it, and asserts the provider path used `fetch`, parsed the response,
  and rendered/filled the expected state. No network in automation.
- CORS and key exposure in the browser are documented as inherent, not
  hidden.

### The keyless raster fallback

A page may register one live `{z}/{x}/{y}` raster source with
`registerUrlTemplateRasterTileSource`. It is a **fallback**: the canvas
resolves the configured `RasterTiles` provider first and the fallback second,
so a credential-configured provider always wins, whatever order the page's
asynchronous registrations run in. `clearProviderRegistrations` leaves the
fallback in place; `clearUrlTemplateRasterTileSource` removes it.
`providerRegistrations()` reports the raster contract as configured only when
a provider actually serves it.

### Forbidden request headers

The browser transport drops the Fetch specification's forbidden request
header names (`user-agent`, `host`, `content-length`, `connection`, `cookie`,
`origin`, `referer`, `accept-encoding`, and the `proxy-*`/`sec-*` families)
before building a request. Setting one makes a cross-origin fetch advertise
it in the CORS preflight, and a host whose policy does not allow it refuses
every request. The native transport still sends the provider's declared user
agent. Design and evidence: `docs/reference/BrowserRasterFallback.md`.

### Deferred but linked

IndexedDB response caching for wasm remains the separate backlog item; live
providers do not require it. A browser-side tile cache is a later slice.

## 8. Future providers

Each provider adds:

1. A descriptor row in `platform/providers.json` (`provider_key`, labels,
   availability per platform, attribution, terms note, hint, fixture routes).
2. A registration dispatch arm in each app's credential applier and, when the
   provider needs its own transport policy, its provider crate.
3. A per-provider terms note (rate limits, attribution, caching ceiling) in
   the descriptor or the provider crate docs.

The view never needs a provider-specific branch; the applier is the only
switch. HERE, Google Maps, TomTom, Mapbox and Radar are descriptors and
dispatch rows in this build, and their provider crates are published in the
provider layer.

## 9. Risks

| Risk | Mitigation |
|---|---|
| Provider HTTP code grows a second implementation | One transport trait; provider request/parse/mapping logic stays single-source |
| wasm networking drifts from native | Shared wire records and wiremock/fixture parity; browser tests use the same response shapes |
| Secure-store APIs differ or fail | Availability reported honestly; session-only fallback; opt-in default off |
| Keys leak through diagnostics or logs | Redaction rules and review; never echo; masked fields |
| Registration cannot be undone at runtime | State the restart requirement; clear the stored value and UI immediately |
| ubrn web churn | Evaluate after the transport seam; keep the current harness until a generated surface passes `contract_sync` |
