#!/usr/bin/env bash
# Generate the install-snippet README that ships as a release asset
# (docs/Release.md (The release workflow)):
# `rustic-maps-sdk-v<version>-README.md`.
#
# The README names the tagged version and every release asset, and carries the
# copy-paste install snippet per platform (Rust source archive, Android AAR,
# iOS Swift Package Manager, React Native npm tarball, WebAssembly npm tarball)
# plus the checksum and Sigstore verification commands. It is assembled from
# the version and repository only — never from a timestamp, host or environment
# value — so the same inputs always produce the same bytes.
#
# Usage:
#   scripts/generate-release-readme.sh --version 0.1.0 \
#     --repository angoratek/rustic-maps-sdk \
#     --output dist/rustic-maps-sdk-v0.1.0-README.md
#
# `--output` defaults to stdout.
set -euo pipefail

readonly DEFAULT_REPOSITORY="angoratek/rustic-maps-sdk"
readonly VERSION_PATTERN='^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z][0-9A-Za-z.-]*)?$'
readonly MAVEN_GROUP="com.angoratek"
readonly MAVEN_GROUP_JNA="net.java.dev.jna"
readonly JNA_VERSION="5.18.1"
readonly COROUTINES_GROUP="org.jetbrains.kotlinx"
readonly COROUTINES_VERSION="1.10.2"

version=""
repository="${DEFAULT_REPOSITORY}"
output_path=""

usage() {
    cat >&2 <<'USAGE'
usage: generate-release-readme.sh --version <X.Y.Z> [--repository <owner/repo>]
                                  [--output <path>]

  --version     release version without the leading `v`
  --repository  GitHub repository, for the package URL and verify command
                (default: angoratek/maps-sdk-rs)
  --output      output path (default: stdout)
USAGE
}

fail() {
    printf 'error: %s\n' "$1" >&2
    exit 1
}

parse_arguments() {
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --version)
                [ "$#" -ge 2 ] || { usage; exit 2; }
                version="${2#v}"
                shift 2
                ;;
            --repository)
                [ "$#" -ge 2 ] || { usage; exit 2; }
                repository="$2"
                shift 2
                ;;
            --output)
                [ "$#" -ge 2 ] || { usage; exit 2; }
                output_path="$2"
                shift 2
                ;;
            -h|--help)
                usage
                exit 0
                ;;
            *)
                printf 'error: unknown argument: %s\n' "$1" >&2
                usage
                exit 2
                ;;
        esac
    done
}

validate_arguments() {
    [[ "${version}" =~ ${VERSION_PATTERN} ]] \
        || fail "--version must be a semantic version (X.Y.Z, optional -pre-release)"
    case "${repository}" in
        */*) ;;
        *) fail "--repository must be an owner/repo pair" ;;
    esac
}

emit_readme() {
    cat <<EOF
# Rustic Maps SDK ${version}

Installation, artifact and verification notes for the ${version} GitHub
Release. Every artifact below is attached to this release; nothing is
published to Maven Central, the npm registry or the Swift Package registry.

## Release assets

| Platform | Asset |
|---|---|
| Rust | \`rustic-maps-sdk-rust-v${version}.tar.gz\` (the workspace crates' sources at the tag, for path-dependency consumption) |
| Android | \`rustic-maps-sdk-android-v${version}.aar\` (bindings, native libraries and \`MapView\`) |
| iOS | \`rustic-maps-sdk-ios-v${version}.xcframework.zip\` |
| React Native | \`rustic-maps-sdk-rn-v${version}.tgz\` |
| WebAssembly | \`rustic-maps-sdk-web-v${version}.tgz\` (npm-installable; extract and serve \`package/\` without npm) |
| Notices and licenses | \`rustic-maps-sdk-notices-v${version}.md\`, \`LICENSE-MIT\`, \`LICENSE-APACHE\` |

## Checksums

\`SHA256SUMS.txt\` lists the SHA-256 of every asset, and each asset also has a
\`<asset>.sha256\` sidecar. Check a downloaded file with
\`sha256sum --check <asset>.sha256\` (or compare it against the matching line
in \`SHA256SUMS.txt\`) before installing it. Every asset has a keyless
Sigstore \`.sigstore.json\` bundle (see *Verifying the download* below).

## Rust

The tarball is the workspace source at this tag, for building the crates
yourself. Unpack it:

\`\`\`bash
tar -xzf rustic-maps-sdk-rust-v${version}.tar.gz
\`\`\`

From the unpacked tree, point a path dependency at the crates you use:

\`\`\`toml
[dependencies]
rustic-maps-rendering = { path = "crates/rustic-maps-rendering" }
rustic-maps-navigation = { path = "crates/rustic-maps-navigation" }
\`\`\`

The archive carries \`Cargo.lock\`, \`rust-toolchain.toml\`, the UDL contracts,
the provider registry and both license files. Rust 1.89 or newer is required.
Consumers who install a compiled package do not need it.

## Android

The AAR is a plain local file dependency; add it to the application's module:

\`\`\`kotlin
repositories { flatDir { dirs("libs") } }
dependencies {
    implementation(files("libs/rustic-maps-sdk-android-v${version}.aar"))
    implementation("${MAVEN_GROUP_JNA}:jna:${JNA_VERSION}@aar")   // UniFFI runtime
    implementation("${COROUTINES_GROUP}:kotlinx-coroutines-core:${COROUTINES_VERSION}")
}
\`\`\`

The AAR carries the Kotlin bindings, the Rust library for every released ABI,
and the dual license files plus the third-party notices under
\`assets/licenses/\` (published as Maven \`${MAVEN_GROUP}:rustic-maps-sdk\`
metadata).

## iOS (Swift Package Manager)

Add the generated package to an Xcode project or a \`Package.swift\`:

\`\`\`swift
.package(url: "https://github.com/${repository}.git", branch: "swift-package")
\`\`\`

The \`swift-package\` branch carries the generated \`Package.swift\` plus the
Swift binding sources. Its \`binaryTarget\` points at
\`rustic-maps-sdk-ios-v${version}.xcframework.zip\` on this release and pins the
archive with its SHA-256 checksum. The dual license files and the third-party
notices are inside the XCFramework directory.

## React Native

\`\`\`bash
npm install ./rustic-maps-sdk-rn-v${version}.tgz
\`\`\`

The published React Native tarball is the generated TurboModule package
(TypeScript, JSI C++, Android Gradle and iOS podspec sources, the dual license
files and the third-party notices) with the compiled Rust libraries it links:
\`android/src/main/jniLibs/<abi>/librustic_maps.a\` for arm64-v8a, armeabi-v7a
and x86_64, and \`AngoratekRusticMapsSdkFramework.xcframework\` for arm64 device
and simulator. Installing it needs no Rust toolchain.

## WebAssembly

\`\`\`bash
npm install ./rustic-maps-sdk-web-v${version}.tgz
\`\`\`

Or without npm, extract the tarball and serve its \`package/\` directory as a
static path root: it is the generated \`wasm2\` module
(\`index.js\`, \`rustic_maps.js\`, \`rustic_maps-ffi.js\`, \`rustic_maps_bg.js\`,
\`rustic_maps.wasm\`, the type declarations) with the licenses and notices.
The module imports its player runtime (\`@ubjs/core\`, \`@ubjs/wasm\`) as bare
specifiers; resolve them with an import map. WebGPU is required; feature-detect
with \`navigator.gpu\` and serve the page from a secure context (HTTPS or
localhost).

## Verifying the download

Every asset is signed with Sigstore keyless signing; each signature bundle is
bound to this repository's release workflow at this tag:

\`\`\`bash
cosign verify-blob --bundle <asset>.sigstore.json \\
  --certificate-identity-regexp "https://github.com/${repository}/.github/workflows/release.yml@refs/tags/v${version}" \\
  --certificate-oidc-issuer "https://token.actions.githubusercontent.com" \\
  <asset>
\`\`\`

Check the archive against \`SHA256SUMS.txt\` (or the asset's \`.sha256\`
sidecar) before installing it.

## Licenses

The SDK is dual-licensed under MIT or Apache-2.0; both texts ship as
\`LICENSE-MIT\` and \`LICENSE-APACHE\`. The third-party notices
(\`rustic-maps-sdk-notices-v${version}.md\` and the copies inside each package)
list every dependency's license and the map-data attribution.
EOF
}

main() {
    parse_arguments "$@"
    validate_arguments

    if [ -n "${output_path}" ]; then
        mkdir -p "$(dirname "${output_path}")"
        emit_readme > "${output_path}"
        printf 'ok: %s\n' "${output_path}" >&2
    else
        emit_readme
    fi
}

main "$@"
