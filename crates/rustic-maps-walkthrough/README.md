# rustic-maps-walkthrough

Records the Android, iOS and React Native reference walkthroughs, cuts stills and short transition clips from the recordings, validates the flow contract, and builds a static player.

`MapView` is the product on every platform. This crate records what it looks like and plays the result on the documentation site without a server, an account or a credential.

## What you get

- One recorded walkthrough per platform, cut from the same flow at the same logical points.
- One shared frame ratio, `9/19.5`, so the three recordings play in step.
- A contract that names the flow step behind every frame and the gesture that moves to the next one.
- A static player with an embedded manifest of SHA-256 hashes and the source commit.

## Prerequisites

| Tool | Used for | Install |
| --- | --- | --- |
| `ffmpeg` | Cutting stills, normalising frames and encoding transition clips | `brew install ffmpeg` on macOS, `apt-get install ffmpeg` on Debian or Ubuntu |
| `xcrun` | Recording the iOS simulator | `xcode-select --install` |
| `adb` | Recording the Android emulator | Android platform-tools, for example `brew install --cask android-platform-tools` |
| Node.js and Playwright | Recording the browser harness | `npm install` in `platform/web` |
| Android platform-tools and an emulator | Android capture | Android Studio or the SDK command-line tools |
| Xcode and a booted simulator | iOS capture | the App Store or `xcode-select --install` |

`ffmpeg` is never bundled and never downloaded by this crate. When it is absent, a command exits with a message naming the prerequisite and the install command.

## Keyless providers

Every recording runs without a credential on every platform, through the same providers:

- OpenFreeMap for the vector base.
- Photon for geocoding and place search.
- OSRM for routing. Valhalla is the documented fallback.

`capture` refuses to start when any known credential variable is present in the environment: `PROVIDER_API_KEY`, `TEST_RUNNER_PROVIDER_API_KEY`, `HOSTED_DEMO_PROVIDER_API_KEY` or `MAPQUEST_CONSUMER_KEY`. The refusal names the variable, so a key can never slip into a recording, a manifest or a published frame.

## The two-step capture

### Step 1: record

Start the target first: boot the Android emulator, boot the iOS simulator, or run `node platform/web/serve.mjs` for the browser harness.

```sh
# All three platforms, into tmp/walkthrough-recordings/.
cargo run -p rustic-maps-walkthrough -- capture

# One platform.
cargo run -p rustic-maps-walkthrough -- capture --platform ios

# Print every command without running it.
cargo run -p rustic-maps-walkthrough -- capture --platform all --dry-run
```

The recipes are:

- **iOS** clears the simulator status bar with `xcrun simctl status_bar booted override` and records with `xcrun simctl io booted recordVideo --codec=h264 --force <file>`. Stop the recording with `Ctrl-C`.
- **Android** records with `adb shell screenrecord --bit-rate 16000000 /sdcard/rustic-maps-walkthrough.mp4` and copies the file with `adb pull`. Stop the recording with `Ctrl-C`.
- **Browser** runs a documented Playwright `recordVideo` invocation from `platform/web`, at the shared ratio, and saves the result as `react-native.webm`.

Recordings land in `tmp/walkthrough-recordings/` by default. Choose another directory with `--recording <directory>`.

### Step 2: slice

Cut one still at an exact timestamp:

```sh
cargo run -p rustic-maps-walkthrough -- slice \
  --input tmp/walkthrough-recordings/ios.mp4 \
  --timestamp 12.5 \
  --output tmp/walkthrough/ios-plan-sheet.png
```

Cut one transition clip and prove that it ends on the next still:

```sh
cargo run -p rustic-maps-walkthrough -- slice \
  --input tmp/walkthrough-recordings/ios.mp4 \
  --start 12.5 --end 15.0 \
  --next-still crates/rustic-maps-walkthrough/assets/normalised/ios-provider-section.png \
  --output tmp/walkthrough/ios-plan-to-provider.mp4
```

The clip command fails with both paths and the measured channel difference when the clip's last decoded frame does not match the declared still. That check is what keeps a hotspot from swapping to a frame the clip never reaches.

## Normalisation

The three devices record different frame shapes. The walkthrough uses one shared ratio, `19.5 / 9`, which is height divided by width. A published still that contradicts it is normalised before the contract cites it:

```sh
cargo run -p rustic-maps-walkthrough -- normalise \
  --input site/book-src/img/android-plan-sheet.png \
  --output crates/rustic-maps-walkthrough/assets/normalised/android-plan-sheet.png
```

The command keeps the source width and adjusts the height. `--normalisation-mode auto` pads when the shared frame is taller and crops when it is shorter; `pad` and `crop` force one. `--aspect-ratio` changes the shared ratio. A padded frame takes `--pad-color`, black by default.

The contract records the exact transform next to the platform:

| Platform | Published frame | Walkthrough frame | Transform |
| --- | --- | --- | --- |
| Android | 1080×2200 | 1080×2340 | pad 70 pixels above and below |
| iOS | 1206×2622 | 1206×2613 | crop 4 pixels above and 5 below |
| React Native | 1080×2200 | 1080×2340 | pad 70 pixels above and below |

The build copies the transform into the generated manifest, so every published build states how each platform reached the shared ratio.

The published screenshots under `site/book-src/img/` are never edited. The normalised copies are walkthrough assets under `crates/rustic-maps-walkthrough/assets/normalised/`.

## Why `-ss` follows `-i`

`ffmpeg` treats a seek before the input as an input option. It jumps to the nearest keyframe, which is fast but lands on the wrong frame. A seek after the input is an output option: `ffmpeg` decodes from the start of the stream and discards frames until the requested timestamp. The result is the exact frame, which is what a still and a frame-accurate cut both need.

Every still and transition command in this crate places `-ss` after `-i`. Transition clips also use `-to`, `-an`, the `libx264` codec, CRF 20, the `slow` preset, `-pix_fmt yuv420p` and `-movflags +faststart`.

## Validate and build

```sh
cargo run -p rustic-maps-walkthrough -- validate
cargo run -p rustic-maps-walkthrough -- build --output tmp/walkthrough-site
cargo run -p rustic-maps-walkthrough -- serve --directory tmp/walkthrough-site
```

`validate` checks the contract against `platform/reference-flows.json`, every image's measured shape, every recorded normalisation, every hotspot target, and that all three platforms declare the same ratio. `build` writes the player, the assets and `manifest.json`. `serve` adds the hotspot authoring overlay for local work.

## Tests

```sh
cargo test -p rustic-maps-walkthrough
```

The capture and slice tests need no encoder and no device. They put a stub `ffmpeg` first on `PATH`; the stub records each argument vector and writes deterministic bytes. That is how the tests assert:

- `-ss` follows `-i`.
- Transition clips carry `-an`, `libx264`, the CRF, the preset, `yuv420p` and `+faststart`.
- The handoff comparison rejects a clip whose last frame differs.
- A credential variable refuses a capture.
- A missing `ffmpeg` reports the install command.

One test normalises a published still with a real `ffmpeg`. When the binary is absent it prints an explicit skip message instead of asserting.
