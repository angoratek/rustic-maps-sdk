# Downloads

**No versioned release is published yet.** Every package is built from source
with the scripts in this repository. There is no Maven Central, npm registry or
Swift Package Manager registry entry, and this site does not host a versioned
download path. When a release is published, it uses this site's own versioned
path (`/downloads/vX.Y.Z/`), carries a `.sha256` sidecar and a Sigstore bundle,
and never rewrites an existing file.

<!-- downloads-version:begin -->## No versioned release yet<!-- downloads-version:end -->

No versioned artifact is available to download. Run the script for your
platform and install its output through the steps in the [tutorials](first-map.md).

<!-- downloads-table:begin -->
| Platform | Artifact |
|---|---|
| — | No versioned artifact is published yet; build one with the script table below. |
<!-- downloads-table:end -->

## When a version is published

Every version carries a per-file `.sha256` sidecar, an aggregated
`SHA256SUMS.txt` manifest covering all files, and a Sigstore signature bundle
(`.sigstore.json`) per file and for `SHA256SUMS.txt` itself. A published
version is immutable: a problem is fixed by a new version, never by replacing a
file.

## Verifying a published artifact

Verify by checksum first, then signature. The signature bundles bind the file to
the release workflow and the `vX.Y.Z` tag:

```bash
sha256sum --check SHA256SUMS.txt --ignore-missing

cosign verify-blob <artifact-file> \
  --bundle <artifact-file>.sigstore.json \
  --certificate-identity-regexp "<the identity printed by your download page>" \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
```

The version page prints the exact identity string for its own tag.

### Rust

The Rust package is the workspace source itself: no compiled libraries,
generated bindings or credentials. Cargo builds the crates you use. The [Rust
guide](rust.md#install-from-the-source-tree) lists what the source tree contains
and what it leaves out. Point a path dependency at the crates you use:

```toml
[dependencies]
rustic-maps-rendering = { path = "crates/rustic-maps-rendering" }
rustic-maps-navigation = { path = "crates/rustic-maps-navigation" }
```

A crates.io registry entry is not published yet; the source tree is the source
of truth.

## Install a built package

The [tutorials](first-map.md) carry the per-platform install steps against
these exact artifact names: the AAR for Android, the checksummed zip for the
Swift Package Manager binary target, and the npm tarball for WebAssembly, which
also serves a static host that extracts its `package/` directory.

To build the packages from a checkout, each one has a script that produces the
artifact the tutorials install:

| Package | Script | Output |
|---|---|---|
| Android | `scripts/build-android.sh` | the one AAR: bindings, three ABIs and `MapView` |
| iOS | `scripts/build-ios.sh` | the XCFramework and the verified Swift package |
| React Native | `scripts/build-rn.sh` | the generated TurboModule sources and native payload |
| WebAssembly | `scripts/package-web-npm.sh` | the npm tarball, after the wasm build |

The scripts print their prerequisites when one is missing.
