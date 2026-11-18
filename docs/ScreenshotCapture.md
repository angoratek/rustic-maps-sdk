# Screenshot capture

This document is how the published screenshot set is produced and how a
reviewer confirms it. It is the working companion to
`docs/DocsStyleGuide.md` §5.

## What the set contains

The set is the six captures per platform that `docs/DocsStyleGuide.md` §5
defines: four flow frames and a layers-control pair, all from one revision.
The four flow frames, always published together:

1. **Plan sheet**: the reference scenario's Origin and Destination fields
   holding Capitol Park and Tower Bridge, with each field's Search action, the
   My location chips, the swap action and the Plan route action.
2. **Provider section**: the provider settings surface with every visible
   capability row reading its composed `{display name} · {credential state}`
   line, the applied credential's row, and the serving provider selected on
   each picker.
3. **Navigation**: the recorded Capitol Park ⇔ Tower Bridge trip at its
   **first instruction**, with the turn banner, the then line, the route
   polyline, the ETA-distance line and the playback-speed controls set to
   **1×**.
4. **Vector base**: the styled OpenFreeMap Bright vector base with its drawn
   street and place labels. Each run asserts the styled base is serving before
   the frame: the layer panel's Serving mark on iOS and React Native, the
   pressed Bright row and vector tile mode on the web, and the SDK's vector
   source status (style id, provider, drawn tiles) on Android. A raster base
   therefore cannot be published in its place.

The navigation marker is the same moment on every platform: the first
instruction shown and the 1× speed chip selected. Captures are phone width
only; tablets are out of scope.

The layers control pair adds the remaining two captures per platform, both
from one keyed session and the same revision:

5. **Layers control closed**: the map with the credential provider's raster
   base serving, the round layers control and the attribution control on the
   map's end edge, and no panel open.
6. **Layers control open**: the same session, camera pose and provider state
   with the layers panel open. The panel scrolls on a phone, so the frame
   holds its lower rows: the last base-map rows, the traffic switch, the
   overlay switches with **Clear overlays**, and the device-location row.

## What you need

| Requirement | Value |
| --- | --- |
| Android device | the standing Pixel_9 emulator, booted |
| Android SDK and NDK | `ANDROID_HOME` set; the NDK version `scripts/build-android.sh` pins |
| iOS simulator | the standing iPhone 17 Pro, booted |
| Xcode and xcodegen | the pinned Xcode; `xcodegen` on `PATH` |
| React Native | Metro on port 8081 from `platform/rn/app-shell`, and the debug app installed |
| Web | `platform/web` dependencies installed and a WebGPU-capable Chromium |
| Credential | a MapQuest key in the repository root `.env` as `MAPQUEST_CONSUMER_KEY` |

No capture run compiles the credential into a build. Every run injects it
into the session through the runtime seam the reference applications document
in `docs/ProviderCredentialsAndBrowserProviders.md` §5.5.

## Credential handling

Load the key into the capture shell without printing it:

```bash
set -a
. /path/to/repository/.env
set +a
```

Verify it once with a live tile request before capturing. The command uses the
environment variable, never a literal key, and prints only the response status:

```bash
curl -sS -o /tmp/mapquest-probe.png \
  -w '%{http_code} %{content_type}\n' \
  "https://www.mapquestapi.com/tiles/v1/render/latest/vivid/13/1873/3374/256/png?key=${MAPQUEST_CONSUMER_KEY}"
file /tmp/mapquest-probe.png
```

Expected: `200 image/png` and a `PNG image data` file. Stop and report if it
is anything else; do not capture with a refused key.

After capturing, compare every published image and every generated text file
against the key in-process and fail on a hit (the check is at the end of this
document).

## Android

The capture is the opt-in instrumentation case in
`platform/android/reference-application/src/androidTest/.../ReferenceApplicationScreenshotCaptureTest.kt`.
It applies the credential from the launch-intent extra, selects the styled
base through the layers row's own act, and drives the same user-path methods
the behavior suite uses.

```bash
# A fresh checkout has no generated bindings, refapp bindings or AAR;
# produce them once.
./scripts/build-android.sh
./scripts/build-refapp-core.sh android

cd platform/android
./gradlew :reference-application:assembleDebugAndroidTest

adb install -r reference-application/build/outputs/apk/debug/reference-application-debug.apk
adb install -r reference-application/build/outputs/apk/androidTest/debug/reference-application-debug-androidTest.apk

adb shell am instrument -w \
  -e class 'com.rusticmaps.sdk.referenceapplication.ReferenceApplicationScreenshotCaptureTest#capturesTheKeyedPlanSheetProviderSectionNavigationAndVectorFrames' \
  -e captureReferenceScreenshots true \
  -e providerApiKey "$MAPQUEST_CONSUMER_KEY" \
  -e providerKey mapquest \
  com.rusticmaps.sdk.referenceapplication.test/androidx.test.runner.AndroidJUnitRunner

adb pull /sdcard/Android/data/com.rusticmaps.sdk.referenceapplication/files/reference-screenshots/. \
  tmp/reference-screenshots/android/
```

The layers control pair is a second opt-in method in the same class. It
launches one keyed session, selects the provider's raster row, takes the
closed frame, then opens the panel and scrolls it to its lower rows.

```bash
adb shell am instrument -w \
  -e class 'com.rusticmaps.sdk.referenceapplication.ReferenceApplicationScreenshotCaptureTest#capturesTheKeyedLayersPanelClosedAndOpenFrames' \
  -e captureReferenceScreenshots true \
  -e providerApiKey "$MAPQUEST_CONSUMER_KEY" \
  -e providerKey mapquest \
  com.rusticmaps.sdk.referenceapplication.test/androidx.test.runner.AndroidJUnitRunner

adb pull /sdcard/Android/data/com.rusticmaps.sdk.referenceapplication/files/reference-screenshots/. \
  tmp/reference-screenshots/android/
```

Run the test through `adb shell am instrument` rather than the Gradle
connected task: Gradle uninstalls the application after the test and with it
the external capture directory, before the host can pull it.

## iOS

The opt-in XCUITest is
`platform/ios/reference-application/Tests/ReferenceApplicationUITests/ReferenceApplicationScreenshotCaptureTest.swift`.
It launches the installed application four times with the credential in its
launch environment, uses the capture launch flags for the plan sheet and the
recorded sample journey, and attaches the four screenshots to the result
bundle. The class also runs the layers method, so a class-level run captures
both sets and the result bundle carries six attachments.

```bash
# A fresh checkout has no generated bindings or XCFramework; produce them once.
./scripts/build-ios.sh
./scripts/build-refapp-core.sh ios

cd platform/ios/reference-application
xcodegen generate
REPOSITORY_ROOT="$(git rev-parse --show-toplevel)"

TEST_RUNNER_CAPTURE_REFERENCE_SCREENSHOTS=1 \
TEST_RUNNER_PROVIDER_API_KEY="$MAPQUEST_CONSUMER_KEY" \
TEST_RUNNER_PROVIDER_KEY=mapquest \
xcodebuild test -project ReferenceApplication.xcodeproj \
  -scheme ReferenceApplication -sdk iphonesimulator \
  -destination 'platform=iOS Simulator,name=iPhone 17 Pro' \
  -only-testing:ReferenceApplicationUITests/ReferenceApplicationScreenshotCaptureTest \
  -resultBundlePath \
  "$REPOSITORY_ROOT/tmp/reference-screenshots/ios/ReferenceApplicationScreenshots.xcresult"

xcrun xcresulttool export attachments \
  --path "$REPOSITORY_ROOT/tmp/reference-screenshots/ios/ReferenceApplicationScreenshots.xcresult" \
  --output-path "$REPOSITORY_ROOT/tmp/reference-screenshots/ios/attachments"
```

Rename the exported attachments from `manifest.json` to the published names
(`ios-plan-sheet.png`, `ios-provider-section.png`, `ios-navigation.png`,
`ios-vector-base.png`, `ios-layers-closed.png`, `ios-layers-open.png`) with
the mapping the manifest records. The `TEST_RUNNER_` prefix is required:
`xcodebuild` does not forward the host environment to the UI-test runner.

The layers control pair has its own method. It launches one keyed session,
selects the provider's raster row, takes the closed frame, then opens the
panel, scrolls it to its lower rows, and takes the open frame. It runs as
part of the class-level command above; the command below recaptures only the
pair.

```bash
REPOSITORY_ROOT="$(git rev-parse --show-toplevel)"
TEST_RUNNER_CAPTURE_REFERENCE_SCREENSHOTS=1 \
TEST_RUNNER_PROVIDER_API_KEY="$MAPQUEST_CONSUMER_KEY" \
TEST_RUNNER_PROVIDER_KEY=mapquest \
xcodebuild test -project ReferenceApplication.xcodeproj \
  -scheme ReferenceApplication -sdk iphonesimulator \
  -destination 'platform=iOS Simulator,name=iPhone 17 Pro' \
  -only-testing:ReferenceApplicationUITests/ReferenceApplicationScreenshotCaptureTest/testCapturesTheLayersPanelSet \
  -resultBundlePath \
  "$REPOSITORY_ROOT/tmp/reference-screenshots/ios/LayersPanelScreenshots.xcresult"

xcrun xcresulttool export attachments \
  --path "$REPOSITORY_ROOT/tmp/reference-screenshots/ios/LayersPanelScreenshots.xcresult" \
  --output-path "$REPOSITORY_ROOT/tmp/reference-screenshots/ios/attachments"
```

Rename the two attachments to `ios-layers-closed.png` and
`ios-layers-open.png` with the mapping the manifest records.

## React Native

The runner is
`platform/rn/app-shell/scripts/capture_reference_screenshots.mjs`. It launches
the Android shell with the `providerApiKey` intent extra and drives the same
controls through `uiautomator dump` and `adb shell input`.

Preconditions: `scripts/build-android.sh` produced the SDK AAR,
`scripts/generate-rn-bindings.sh` produced the JSI and TurboModule sources and
the Rust static archives the TurboModule links, `:app:assembleDebug` built the
app shell against them, Metro serves `platform/rn/app-shell` on port 8081, and
`adb reverse tcp:8081 tcp:8081` points the debug app at Metro. The runner
clears the app's data before each launch, so a remembered field or key cannot
leak into the frames.

```bash
adb reverse tcp:8081 tcp:8081
PROVIDER_API_KEY="$MAPQUEST_CONSUMER_KEY" PROVIDER_KEY=mapquest \
  node platform/rn/app-shell/scripts/capture_reference_screenshots.mjs
```

The runner writes the four PNGs and the layers control pair under
`tmp/reference-screenshots/react-native/`. It captures the pair on the
credential provider's raster base before it selects a styled base, then
scrolls the open panel to its lower rows.

## WebAssembly

The opt-in Playwright spec is
`platform/web/tests/capture_reference_screenshots.spec.mjs`. It applies the
credential through the page's own settings surface, keeps provider traffic
same-origin with `?useProviderFixtures`, and writes the four PNGs.

```bash
cd platform/web
CAPTURE_REFERENCE_SCREENSHOTS=1 PROVIDER_API_KEY="$MAPQUEST_CONSUMER_KEY" \
  npx playwright test tests/capture_reference_screenshots.spec.mjs --workers=1
```

The output lands under `tmp/reference-screenshots/web/`. The provider frame
uses a tall phone-width viewport so the whole settings sheet fits in one
frame; the other three frames use the phone viewport.

The layers control pair is a second opt-in test in the same spec. The plan
sheet steps aside so the map is the subject, and both frames keep the same
camera pose and the same serving base.

```bash
cd platform/web
CAPTURE_REFERENCE_SCREENSHOTS=1 PROVIDER_API_KEY="$MAPQUEST_CONSUMER_KEY" \
  npx playwright test tests/capture_reference_screenshots.spec.mjs \
  --workers=1 -g "layers panel"
```

The pair lands beside the other frames as `layers-closed.png` and
`layers-open.png`.

## Publish the reviewed set

Copy only the reviewed files from one revision into `site/book-src/img/`:
the four flow frames and the two layers frames for each platform, 24 PNGs in
all (`android-*`, `ios-*`, `react-native-*` and `wasm-*` for
`plan-sheet`, `provider-section`, `navigation`, `vector-base`,
`layers-closed` and `layers-open`). Then run the documentation
gates:

```bash
node scripts/check-docs-references.mjs
node scripts/check-doc-snippets.mjs
node scripts/check-reference-flows.mjs
bash scripts/build-docs-site.sh
```

## Capture log

The published set comes from one revision. The four-frame set and the layers
control pair were captured on every platform from the revision below, so no
frame on the page comes from a different tree.

| Field | Value |
| --- | --- |
| Revision | `6f180a4adef60856210d8fe88158e546cbd818b0` |
| Date | 2026-11-04 |
| Navigation marker | first instruction shown, 1× playback speed |
| Four-frame base | the registry's keyless styled base, `openfreemap` `bright-light` |
| Layers-pair base | the applied credential provider's raster base, `mapquest` |
| Android evidence | `tmp/reference-screenshots/android/` |
| iOS evidence | `tmp/reference-screenshots/ios/` |
| React Native evidence | `tmp/reference-screenshots/react-native/` |
| Web evidence | `tmp/reference-screenshots/web/` |

**The published set is stale.** It was recorded on the revision above
(2026-11-04). The rendered surface has changed since: route-reference shield
badges, the provider console's traffic-cache counters, the developer console,
the map motion changes and the current provider rows all postdate it. The
frames on the site are not a picture of the current build. Recapture the whole
six-capture set on every platform from one revision; never update one frame
alone.

The layers pair is a second run on Android and the web and a second test method
on iOS; the React Native runner captures it in the same launch. Each platform
takes the pair from one keyed session, and both frames keep the same camera
pose and serving base.

**Pending recapture.** The settings frames in the published set were captured
before the five vendor providers joined the registry, so they show one
credential row. A recapture must come from a revision that carries the
nine-key catalog and its twelve descriptors; until then the provider section
is described in text rather than shown from the older frame.

Per-target notes:

- Android: the standing Pixel_9 emulator at 1080×2200. The four-frame method
  asserts the styled base's style id, provider key and drawn tile count before
  it takes the vector frame; the layers method asserts the credential
  provider's raster row is serving.
- iOS: the standing iPhone 17 Pro simulator. The class runs both test methods;
  `xcresulttool export attachments` writes six attachments named `ios-*`.
- React Native: the Android shell on the same Pixel_9 emulator, with Metro on
  port 8081. The runner clears app data before launch, then captures the
  layers pair, vector base, provider section, plan sheet and navigation frame
  from that one keyed session.
- WebAssembly: the Playwright Chromium channel with WebGPU. Every frame is
  430 px wide; the provider sheet uses a 430×1600 viewport so its whole body
  fits, and the other frames use 430×932. Provider traffic stays same-origin
  through `?useProviderFixtures`.

## Credential check

Run this after the set is copied and before publishing. It reads the key from
the environment and never prints it; it exits non-zero on a hit.

```bash
python3 - <<'PY'
import os
from pathlib import Path

needle = os.environ["PROVIDER_API_KEY"].encode()
roots = [
    Path("site/book-src"),
    Path("tmp/reference-screenshots"),
    Path("platform/web/test-results"),
]
text_suffixes = {".md", ".json", ".mjs", ".js", ".txt", ".log", ".xml", ".html"}
hits = []
for root in roots:
    if not root.exists():
        continue
    for path in root.rglob("*"):
        if not path.is_file():
            continue
        if path.suffix == ".png":
            data = path.read_bytes()
        elif path.suffix in text_suffixes:
            data = path.read_bytes()
        else:
            continue
        if needle in data:
            hits.append(str(path))
if hits:
    raise SystemExit("credential found in: " + ", ".join(hits))
print("credential check: clean")
PY
```
