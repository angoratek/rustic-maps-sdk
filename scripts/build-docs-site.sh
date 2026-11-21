#!/usr/bin/env bash
# Build the public documentation site (GitHub Pages content) into a staged,
# scrubbed tree. Round-11 docs slice; see tmp/round11-research/docs-site-spec.md
# (owner-ruled) for the architecture it implements.
#
# What it does, in order:
#   1. Stamp provenance (source commit + dirty flag) for a fail-closed publish.
#   2. Regenerate docs/THIRD_PARTY_NOTICES.md (deterministic no-op when the
#      dependency graph is unchanged) so the published copy tracks Cargo.lock.
#   3. Compose the mdBook source tree from the curated `site/book-src/` inputs
#      plus repo documents, and build it with mdbook. The five platform API
#      reference pages are generated from the contract, the generated binding
#      surfaces and the Rust sources, then stamped with the source surface
#      revision they rendered from and verified (fail-closed: a missing,
#      unmarked or stale page refuses the build, publish or not).
#   4. Stage the live wasm demo at /demo (the browser harness copied 1:1,
#      its tile/registry/graph fixtures beside it), then render the
#      UniFFI contract verbatim and stage the rustdoc tree. Byte-equality assertions then refuse any drift between the
#      staged UDL copies (the plain-text copy, the embedded chapter block)
#      and the source file, and refuse a missing rustdoc index.
#   5. Run the pre-publish scrub (fail-closed): the build REFUSES on any hit.
#   6. Leave the finished site in target/docs-site-stage/site/ unless --publish
#      is given, in which case it force-replaces the gh-pages branch of the
#      public rustic-maps-sdk-docs checkout named by $PAGES_REPO_DIR (requires
#      --gates-green=<commit>: the publisher asserts the CI gate set passed
#      for that commit, which must equal HEAD — the script cannot observe CI,
#      so the assertion is explicitly named and pinned to the published tree).
#
# The demo / downloads sections are later slices (owner-ruled gating: publish
# a section only when its producing pipeline is green); this script stages the
# docs-only section first.
set -euo pipefail

readonly REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly STAGE_ROOT="${REPO_ROOT}/target/docs-site-stage"
readonly SITE_SOURCES_DIR="${REPO_ROOT}/site"
readonly UDL_CONTRACT_PATH="${REPO_ROOT}/crates/rustic-maps-native-core/src/rustic_maps.udl"
readonly NOTICES_GENERATOR="$(cd "${REPO_ROOT}/scripts" && pwd)/generate-third-party-notices"
readonly API_REFERENCE_GENERATOR="$(cd "${REPO_ROOT}/scripts" && pwd)/generate-api-reference.mjs"
readonly GENERATED_API_REFERENCE_GATE="$(cd "${REPO_ROOT}/scripts" && pwd)/check-generated-api-reference.mjs"
# Set by `mark_and_verify_generated_api_references`; recorded in the staged
# provenance file so a published site states the source surface it rendered.
GENERATED_API_REFERENCE_REVISION=""
# GitHub refuses any single file larger than 100 MB in a repository. The
# published site lives on the gh-pages branch, so an artifact above that limit
# (today the React Native tarball) is hosted by the Release and linked there.
readonly GITHUB_MAXIMUM_FILE_BYTES_IN_REPOSITORY=$((100 * 1024 * 1024))
readonly PUBLIC_PAGES_REPOSITORY_NAME="rustic-maps-sdk-docs"
readonly WEB_HARNESS_DIRECTORY="${REPO_ROOT}/platform/web"
# The committed Latin glyph range the demo's label pass draws with. The page
# requests it at `/glyphs/{fontStack}/{range}.pbf`, so the staged tree must
# carry exactly that path or every label silently disappears in production
# (the dev server serves the same fixture; this is the static-server twin).
readonly GLYPH_FIXTURES_DIRECTORY="${REPO_ROOT}/fixtures/glyphs"
readonly GLYPH_FIXTURE_FILE_NAME="noto_sans_regular_0-255.pbf"
readonly GLYPH_SERVED_FONT_STACK="Noto Sans Regular"
readonly GLYPH_SERVED_RANGE="0-255"
readonly OFFLINE_GRAPH_FIXTURE_PATH="${REPO_ROOT}/crates/rustic-maps-offline-graph-compiler/fixtures/rmog/routing_grid.rmog"
# The shared demo-plan artifact and the recorded navigation traces its
# `demo_plans` entries name (M31): the live demo's Demo plans section fetches
# `/navigation-traces/{scenario}.json`, so the staged tree must carry one copy
# per declared plan.
readonly REFERENCE_SCENES_ARTIFACT_PATH="${REPO_ROOT}/platform/reference-scenes.json"
readonly LIVE_DEMO_MAXIMUM_BYTES=$((25 * 1024 * 1024))
# The recorded walkthrough is built by this crate from its contract; the
# published directory holds the player, its manifest and eighteen normalised
# stills (about 19 MB today), with transition clips to follow.
readonly WALKTHROUGH_CRATE_NAME="rustic-maps-walkthrough"
readonly WALKTHROUGH_CONTRACT_RELATIVE_PATH="platform/reference-walkthrough.json"
readonly WALKTHROUGH_MAXIMUM_BYTES=$((64 * 1024 * 1024))


log_step() {
    printf '\n[%s] %s\n' "$(date +%H:%M:%S)" "$1"
}

fail() {
    printf 'error: %s\n' "$1" >&2
    exit 1
}

need_tool() {
    command -v "$1" >/dev/null 2>&1 || fail "required tool '$1' was not found on PATH"
}

check_source_tree() {
    git -C "$REPO_ROOT" rev-parse --verify HEAD >/dev/null 2>&1 \
        || fail "$REPO_ROOT is not a git repository"
}

stamp_provenance() {
    local provenance_file="$1"
    local source_commit
    local dirty_tree_flag
    source_commit="$(git -C "$REPO_ROOT" rev-parse HEAD)"
    if [ -n "$(git -C "$REPO_ROOT" status --porcelain)" ]; then
        dirty_tree_flag="yes (publishing from this tree is refused; commit or stash first)"
    else
        dirty_tree_flag="no"
    fi
    cat > "$provenance_file" <<PROVENANCE_EOF
source_commit: ${source_commit}
dirty_tree: ${dirty_tree_flag}
generator: scripts/build-docs-site.sh (rustic-maps-sdk)
content_license: MIT OR Apache-2.0
map_attribution: map imagery © MapQuest; map data © OpenStreetMap contributors (ODbL)
PROVENANCE_EOF
    printf '%s\n' "${source_commit}"
}

check_provider_documentation() {
    log_step "checking the provider pages against the shared registry"
    node "${REPO_ROOT}/scripts/check-provider-docs.mjs"
}

regenerate_third_party_notices() {
    log_step "regenerating third-party notices (deterministic when unchanged)"
    "$NOTICES_GENERATOR" >/dev/null
    git -C "$REPO_ROOT" diff --quiet -- docs/THIRD_PARTY_NOTICES.md \
        || printf 'note: docs/THIRD_PARTY_NOTICES.md changed; commit the refreshed file with the dependency change\n'
}

# Renders the downloads page's version heading and artifact table from
# `downloads.json` in the assembled release directory. Without a manifest the
# authored markers stay in place, so the page keeps its explicit
# not-published-yet state.
render_downloads_page() {
    local book_source_dir="$1"
    local workspace_version
    workspace_version="$(sed -n 's/^version = "\(.*\)"/\1/p' "${REPO_ROOT}/Cargo.toml" | head -n 1)"
    local artifacts_dir="${RELEASE_ARTIFACTS_DIR:-${REPO_ROOT}/target/release-artifacts/v${workspace_version}}"
    local manifest_path="${artifacts_dir}/downloads.json"
    [ -f "$manifest_path" ] || return 0
    python3 - "$book_source_dir/downloads.md" "$manifest_path" <<'PYTHON'
import json
import sys

page_path, manifest_path = sys.argv[1], sys.argv[2]
manifest = json.load(open(manifest_path, encoding="utf-8"))
version = manifest["version"]

def human_bytes(byte_count):
    if byte_count >= 1024 * 1024:
        return f"{byte_count / (1024 * 1024):.1f} MB"
    if byte_count >= 1024:
        return f"{byte_count / 1024:.0f} kB"
    return f"{byte_count} B"

def describe(name):
    if name.startswith("rustic-maps-sdk-android"):
        return ("Android",
                "one package: the Kotlin bindings, the Rust library for arm64-v8a, "
                "armeabi-v7a and x86_64, and the `MapView` with its Compose wrapper")
    if name.startswith("rustic-maps-sdk-ios"):
        return ("iOS",
                "the XCFramework (arm64 device and simulator slices) for a Swift Package Manager binary target")
    if name.startswith("rustic-maps-sdk-rust") and name.endswith(".tar.gz"):
        return ("Rust",
                "the workspace crates' source archive at this version; unpack it and "
                "use the crates by path dependency")
    if name.startswith("rustic-maps-sdk-web") and name.endswith(".tgz"):
        return ("WebAssembly",
                "one package: the wasm module, its JavaScript shim, both type "
                "files, the manifest, licenses and notices; install it with npm "
                "or extract it and serve the `package/` directory")
    if name.startswith("rustic-maps-sdk-rn"):
        return ("React Native",
                "the TurboModule package (`@angoratek/rustic-maps-sdk`) with the "
                "compiled Android libraries and iOS framework")
    if "notices" in name:
        return ("All platforms",
                "the third-party dependency notices that accompany your distribution")
    return ("Artifact", "the release artifact")

# GitHub refuses a file larger than 100 MB in a repository, so an artifact
# above that limit is not staged on the Pages branch (the build's staging step
# skips it). The published site may not name the source repository, so the row
# describes the artifact and says it accompanies the release rather than
# linking a repository URL.
GITHUB_MAXIMUM_FILE_BYTES_IN_REPOSITORY = 100 * 1024 * 1024

rows = []
for artifact in manifest["files"]:
    name = artifact["name"]
    platform, description = describe(name)
    if artifact["bytes"] > GITHUB_MAXIMUM_FILE_BYTES_IN_REPOSITORY:
        # The file travels with the release, not with this site.
        rows.append(
            f"| {platform} | `{name}` — {description} "
            f"· {human_bytes(artifact['bytes'])} · "
            f"[`sha256`](downloads/v{version}/{name}.sha256) "
            f"· ships with the release, not hosted here |"
        )
        continue
    rows.append(
        f"| {platform} | [`{name}`](downloads/v{version}/{name}) — {description} "
        f"· {human_bytes(artifact['bytes'])} · [`sha256`](downloads/v{version}/{name}.sha256) |"
    )
table = "| Platform | Artifact |\n|---|---|\n" + "\n".join(rows) + "\n"

page = open(page_path, encoding="utf-8").read()
# Replace the version heading between the marker pair.
version_start = page.index("<!-- downloads-version:begin -->")
version_end = page.index("<!-- downloads-version:end -->") + len("<!-- downloads-version:end -->")
page = page[:version_start] + f"<!-- downloads-version:begin -->## Version {version}<!-- downloads-version:end -->" + page[version_end:]
table_start = page.index("<!-- downloads-table:begin -->") + len("<!-- downloads-table:begin -->")
table_end = page.index("<!-- downloads-table:end -->")
page = page[:table_start] + "\n" + table + page[table_end:]
page = page.replace(
    f"downloads/v0.1.0/SHA256SUMS.txt",
    f"downloads/v{version}/SHA256SUMS.txt",
)
open(page_path, "w", encoding="utf-8").write(page)
PYTHON
}

compose_book_sources() {
    local book_source_dir="$1"
    mkdir -p "$book_source_dir"
    cp "${SITE_SOURCES_DIR}/book-src/SUMMARY.md" "${book_source_dir}/SUMMARY.md"
    cp "${SITE_SOURCES_DIR}/book-src/landing.md" "${book_source_dir}/index.md"
    # Chapters carried over verbatim from the workspace documents.
    cp "${REPO_ROOT}/docs/THIRD_PARTY_NOTICES.md" "${book_source_dir}/third-party-notices.md"

    # Curated public changelog (site/book-src/changelog-public.md), kept in
    # the site sources: no commit hashes, no internal paths, public tone.
    cp "${SITE_SOURCES_DIR}/book-src/changelog-public.md" "${book_source_dir}/changelog.md"

    # The product documentation chapters: hand-authored site sources
    # (overview, per-platform requirements, performance guidance, the live
    # demo placeholder, the four tutorials, the downloads skeleton, the
    # FAQ, and the compressed dev-loop page), carried into the book by
    # SUMMARY.md's listed name. They are curated, not generated, so they
    # live in the site sources; the scrub below stays their fail-closed
    # gate. The img/ directory accepts the screenshots a later slice
    # places into the tutorials.
    for authored_chapter in requirements.md performance.md live-demo.md \
        walkthrough.md how-it-works.md quickstart.md map-concepts.md first-map.md \
        search-and-routing.md navigation-simulation.md offline-maps.md positioning.md \
        map-layers-and-traffic.md vector-base.md versions-and-support.md faq.md downloads.md screenshots.md \
        rust.md api-reference.md udl-contract.md \
        provider-capabilities-android.md provider-capabilities-ios.md \
        provider-capabilities-react-native.md provider-capabilities-wasm.md \
        provider-capabilities-rust.md adding-a-provider.md \
        mix-providers-per-capability.md provider-console.md; do
        cp "${SITE_SOURCES_DIR}/book-src/${authored_chapter}" \
            "${book_source_dir}/${authored_chapter}"
    done
    cp -R "${SITE_SOURCES_DIR}/book-src/img" "${book_source_dir}/img"

    # The downloads table and version come from the release manifest, when an
    # artifact set has been assembled, so a new version never edits prose.
    render_downloads_page "$book_source_dir"

    # The five platform API reference pages are generated from the contract,
    # the generated bindings and the wasm/Rust sources. The landing page
    # (`api-reference.md`) stays authored and is copied with the chapters
    # above; these five are artifacts of the source tree, never edited by
    # hand.
    log_step "generating the five platform API reference pages"
    node "$API_REFERENCE_GENERATOR" --output-dir "$book_source_dir"
    assert_generated_api_references "$book_source_dir"
    mark_and_verify_generated_api_references "$book_source_dir"
}

# Stamps every generated page with the source surface revision it was rendered
# from (HEAD plus a fingerprint of the contract, the generated binding
# surfaces, the wasm export sources, the indexed Rust sources and the
# generator), then verifies the staged set against that revision. The
# verification runs on a non-publish build too, so a local build cannot pass
# while a stale page is staged. `--mark` refuses a page that is already marked:
# stamping a page the build did not just render would hide its staleness.
mark_and_verify_generated_api_references() {
    local book_source_dir="$1"
    log_step "stamping and verifying the generated API reference revision"
    GENERATED_API_REFERENCE_REVISION="$(node "$GENERATED_API_REFERENCE_GATE" --print-revision)"
    [ -n "$GENERATED_API_REFERENCE_REVISION" ] \
        || fail "the generated API reference gate answered no source surface revision; refusing to stage unstamped pages"
    node "$GENERATED_API_REFERENCE_GATE" --mark "$book_source_dir" \
        --revision "$GENERATED_API_REFERENCE_REVISION"
    node "$GENERATED_API_REFERENCE_GATE" --verify "$book_source_dir" \
        --revision "$GENERATED_API_REFERENCE_REVISION"
    printf 'generated API reference source revision: %s\n' "$GENERATED_API_REFERENCE_REVISION"
}

# The five reference pages are build artifacts of the contract, the generated
# binding surfaces and the Rust sources: a page that the generator did not
# write, or wrote empty, must fail the build rather than publish a missing
# document. The surfaces themselves are build artifacts too, so a publish
# (the only path that reaches readers) requires all four: a page derived from
# the contract alone would silently drop the per-platform call shapes.
readonly GENERATED_API_REFERENCE_PAGE_NAMES=(
    "api-reference-rust.md"
    "api-reference-android.md"
    "api-reference-ios.md"
    "api-reference-react-native.md"
    "api-reference-wasm.md"
)
readonly GENERATED_BINDING_SURFACE_PATHS=(
    "platform/android/maps-sdk-native/src/main/kotlin/uniffi/rustic_maps/rustic_maps.kt"
    "platform/ios/Sources/RusticMapsSDK/rustic_maps.swift"
    "platform/rn/src/generated/rustic_maps.ts"
    "platform/web/generated/index.js"
)

assert_generated_api_references() {
    local book_source_dir="$1"
    local generated_page
    for generated_page in "${GENERATED_API_REFERENCE_PAGE_NAMES[@]}"; do
        [ -s "${book_source_dir}/${generated_page}" ] \
            || fail "the generated API reference page ${generated_page} is missing or empty; the site would publish an incomplete reference"
    done
    if [ "${publish_requested}" = "yes" ]; then
        local binding_surface
        for binding_surface in "${GENERATED_BINDING_SURFACE_PATHS[@]}"; do
            [ -s "${REPO_ROOT}/${binding_surface}" ] \
                || fail "publishing needs the generated binding surface ${binding_surface}; run scripts/build-android.sh, scripts/build-ios.sh, scripts/build-rn.sh and scripts/build-wasm2-package.sh first"
        done
    fi
}

build_book() {
    local stage_root="$1"
    local stage_book_source_dir="${stage_root}/book-src"
    log_step "composing and building the mdBook"
    compose_book_sources "$stage_book_source_dir"
    cp "${SITE_SOURCES_DIR}/book.toml" "${stage_root}/book.toml"
    # The additional css/js the book.toml references, fingerprinted into the
    # rendered book by mdBook.
    cp -R "${SITE_SOURCES_DIR}/additional-css" "${stage_root}/additional-css"
    cp -R "${SITE_SOURCES_DIR}/additional-js" "${stage_root}/additional-js"
    (cd "$stage_root" && mdbook build --dest-dir site .)
}

# The SUMMARY is the book's contract with its sources: every entry must
# render. The `create-missing = false` setting makes mdBook itself refuse
# a missing chapter (verified: the build exits nonzero naming the missing
# file); this gate adds the rendered-tree check on top, so a missing
# chapter is a staging failure even if a future mdBook changes that
# behavior.
verify_summary_coverage() {
    local stage_root="$1"
    log_step "verifying every SUMMARY entry renders a chapter"
    python3 -I - "$stage_root" <<'SUMMARY_COVERAGE_EOF'
import re
import sys
from pathlib import Path

stage_root = Path(sys.argv[1])
summary_text = (stage_root / "book-src" / "SUMMARY.md").read_text(encoding="utf-8")
missing = []
for link in re.findall(r"\[[^\]]+\]\(([^)#\s]+)\)", summary_text):
    if link.startswith(("http:", "https:")):
        continue
    rendered = stage_root / "site" / Path(link).with_suffix(".html")
    if not rendered.is_file():
        missing.append(f"{link} -> {rendered.relative_to(stage_root)}")
if missing:
    print("\n".join(f"unrendered SUMMARY entry: {entry}" for entry in missing), file=sys.stderr)
    print(f"summary coverage: {len(missing)} unrendered entry(ies)", file=sys.stderr)
    sys.exit(1)
print("summary coverage: every SUMMARY entry rendered")
SUMMARY_COVERAGE_EOF
}

stage_reference_material() {
    local stage_root="$1"
    log_step "staging the rustdoc tree and the UDL contract"
    # Build from an empty output directory: a previous revision's crate that
    # the tree no longer carries would otherwise stay in target/doc and be
    # published beside the current tree.
    rm -rf "${REPO_ROOT}/target/doc"
    # Rustdoc links every item to its source page and names the source path in
    # the page header. The workspace, dependency registry and toolchain roots
    # are remapped to relative names so neither the source tree nor the build
    # machine's home directory is published.
    readable_source_paths="--remap-path-prefix=${REPO_ROOT}=source"
    if [ -n "${HOME:-}" ]; then
        readable_source_paths="${readable_source_paths} --remap-path-prefix=${HOME}=home"
    fi
    # The walkthrough recorder is internal tooling: its sources name a local
    # harness address and its player is published separately at
    # /demo/walkthrough, so it stays out of the SDK's published rustdoc tree.
    RUSTDOCFLAGS="${RUSTDOCFLAGS:-} ${readable_source_paths}" \
        cargo doc --workspace --exclude "$WALKTHROUGH_CRATE_NAME" --lib --no-deps --quiet
    rm -rf "${stage_root}/site/api"
    cp -R "${REPO_ROOT}/target/doc" "${stage_root}/site/api"
    rewrite_build_machine_paths "${stage_root}/site"
    mkdir -p "${stage_root}/site/udl"
    cp "$UDL_CONTRACT_PATH" "${stage_root}/site/udl/rustic_maps.udl"
}

# Removes every trace of the build machine's absolute paths from the staged
# tree. Rustdoc renders the source path of generated code (the UniFFI
# scaffolding compiled from OUT_DIR) as a directory tree and links to it from
# the item pages; the compiler-level remap does not cover that source-link
# layout, so the staged copy is rewritten and the source trees are dropped.
rewrite_build_machine_paths() {
    local site_dir="$1"
    local absolute_source_root
    for absolute_source_root in Users home root; do
        find "${site_dir}/api" -type d -path "*/src/*/${absolute_source_root}" -prune -exec rm -rf {} +
    done
    python3 -I - "$site_dir" "${REPO_ROOT}" "${HOME:-}" <<'REWRITE_PATHS_EOF'
import sys
from pathlib import Path

site_dir = Path(sys.argv[1])
replacements = [(sys.argv[2], 'source')]
if len(sys.argv) > 3 and sys.argv[3]:
    replacements.append((sys.argv[3], 'home'))
text_suffixes = {'.html', '.js', '.mjs', '.css', '.json', '.txt', '.md', '.xml'}
for path in site_dir.rglob('*'):
    if not path.is_file() or path.suffix.lower() not in text_suffixes:
        continue
    original = path.read_text(errors='replace')
    updated = original
    for old, new in replacements:
        updated = updated.replace(old, new)
    if updated != original:
        path.write_text(updated)
REWRITE_PATHS_EOF
}

# Emits one `scenario<TAB>fixture_path` row per shared demo plan, read from
# the artifact itself so no scenario, trace path or vendor is hardcoded here.
list_demo_plan_traces() {
    local trace_listing="$1"
    python3 -I - "$REFERENCE_SCENES_ARTIFACT_PATH" > "$trace_listing" <<'DEMO_PLANS_EOF'
import json
import sys

with open(sys.argv[1], encoding='utf-8') as artifact_file:
    artifact = json.load(artifact_file)
for demo_plan in artifact.get('demo_plans', []):
    print(f"{demo_plan['scenario']}\t{demo_plan['fixture_path']}")
DEMO_PLANS_EOF
}

# Stages the recorded navigation traces the live demo's Demo plans section
# replays, one file per artifact-declared plan at the same-origin address the
# dev server answers (`/navigation-traces/{scenario}.json`). A missing trace
# fails the build: the demo would otherwise offer a plan that cannot replay.
stage_navigation_traces() {
    local demo_directory="$1"
    local navigation_traces_directory="${demo_directory}/navigation-traces"
    local trace_listing="${STAGE_ROOT}/demo-plan-traces.txt"
    mkdir -p "${navigation_traces_directory}"
    list_demo_plan_traces "$trace_listing"
    local scenario
    local fixture_path
    while IFS=$'\t' read -r scenario fixture_path; do
        [ -n "$scenario" ] || continue
        [ -f "${REPO_ROOT}/${fixture_path}" ] \
            || fail "the demo plan trace ${fixture_path} is missing; the live demo's '${scenario}' plan could not replay"
        cp "${REPO_ROOT}/${fixture_path}" "${navigation_traces_directory}/${scenario}.json"
    done < "$trace_listing"
    rm -f "$trace_listing"
}

# The live wasm demo (`docs/Release.md` (The documentation site)): the browser
# harness served statically at `/demo/`, its fixture routes answered by the
# copy plan rather than a Node server. The harness files are copied 1:1
# (asserted below); `overlay_scenes.mjs` imports `../reference-scenes.json`,
# so the shared artifact is staged beside the demo directory, and the page's
# relative registry URL (`providers.json`) resolves inside it. The recorded
# raster tile fixtures stay in the repository for the dev server and the test
# harness and are never staged: the provider's terms cap how long its imagery
# may be stored.
stage_live_demo() {
    local stage_root="$1"
    log_step "staging the live wasm demo at /demo"
    local demo_directory="${stage_root}/site/demo"
    for demo_input in index.html playground.mjs styles/playground.css main.mjs \
        playground_preferences.mjs playground_provider_settings.mjs \
        playground_base_map.mjs playground_overlays.mjs playground_navigation.mjs \
        playground_diagnostics.mjs playground_map_surface.mjs \
        map_controls.mjs map_layers.mjs overlay_scenes.mjs vector_layer.mjs tile_switch.mjs \
        tile_cache.mjs ui_strings.mjs generated/index.js generated/rustic_maps.js \
        generated/rustic_maps-ffi.js generated/rustic_maps_bg.js \
        generated/rustic_maps.wasm; do
        [ -f "${WEB_HARNESS_DIRECTORY}/${demo_input}" ] \
            || fail "the live demo input platform/web/${demo_input} is missing; build the generated wasm package (scripts/build-wasm2-package.sh) before staging the docs site"
    done
    [ -f "${REPO_ROOT}/platform/providers.json" ] \
        || fail "the provider registry platform/providers.json is missing"
    [ -f "${GLYPH_FIXTURES_DIRECTORY}/${GLYPH_FIXTURE_FILE_NAME}" ] \
        || fail "the committed glyph range ${GLYPH_FIXTURES_DIRECTORY}/${GLYPH_FIXTURE_FILE_NAME} is missing; the live demo's labels would silently disappear"
    mkdir -p "${demo_directory}/generated" "${demo_directory}/generated-runtime" \
        "${demo_directory}/offline-graph" "${demo_directory}/styles" \
        "${demo_directory}/glyphs/${GLYPH_SERVED_FONT_STACK}"
    cp "${WEB_HARNESS_DIRECTORY}/index.html" "${WEB_HARNESS_DIRECTORY}/playground.mjs" \
        "${WEB_HARNESS_DIRECTORY}/playground_preferences.mjs" \
        "${WEB_HARNESS_DIRECTORY}/playground_provider_settings.mjs" \
        "${WEB_HARNESS_DIRECTORY}/playground_base_map.mjs" \
        "${WEB_HARNESS_DIRECTORY}/playground_overlays.mjs" \
        "${WEB_HARNESS_DIRECTORY}/playground_navigation.mjs" \
        "${WEB_HARNESS_DIRECTORY}/playground_diagnostics.mjs" \
        "${WEB_HARNESS_DIRECTORY}/playground_map_surface.mjs" \
        "${WEB_HARNESS_DIRECTORY}/main.mjs" \
        "${WEB_HARNESS_DIRECTORY}/map_controls.mjs" "${WEB_HARNESS_DIRECTORY}/map_layers.mjs" \
        "${WEB_HARNESS_DIRECTORY}/overlay_scenes.mjs" \
        "${WEB_HARNESS_DIRECTORY}/vector_layer.mjs" "${WEB_HARNESS_DIRECTORY}/tile_switch.mjs" \
        "${WEB_HARNESS_DIRECTORY}/tile_cache.mjs" \
        "${WEB_HARNESS_DIRECTORY}/ui_strings.mjs" \
        "${demo_directory}/"
    cp "${WEB_HARNESS_DIRECTORY}/styles/playground.css" "${demo_directory}/styles/"
    cp "${REPO_ROOT}/platform/reference-scenes.json" "${stage_root}/site/reference-scenes.json"
    cp "${REPO_ROOT}/platform/reference-flows.json" "${stage_root}/site/reference-flows.json"
    cp "${REPO_ROOT}/platform/providers.json" "${demo_directory}/providers.json"
    cp "${WEB_HARNESS_DIRECTORY}/generated/"* "${demo_directory}/generated/"
    # The generated package's player runtime ships beside the page so the
    # relative import map resolves inside the staged demo.
    cp -R "${WEB_HARNESS_DIRECTORY}/node_modules/@ubjs" "${demo_directory}/generated-runtime/"
    cp "${GLYPH_FIXTURES_DIRECTORY}/${GLYPH_FIXTURE_FILE_NAME}" \
        "${demo_directory}/glyphs/${GLYPH_SERVED_FONT_STACK}/${GLYPH_SERVED_RANGE}.pbf"
    cp "$OFFLINE_GRAPH_FIXTURE_PATH" "${demo_directory}/offline-graph/routing_grid.rmog"
    stage_navigation_traces "${demo_directory}"
    verify_staged_demo_imports "${demo_directory}"
    stamp_provenance "${demo_directory}/provenance.txt" >/dev/null
}

# Fails when a staged browser module imports a file the staging did not copy.
# The demo is staged from an explicit list, so a new module (the shared
# consumer-copy source, for instance) is easy to add to the page and forget
# here — and the browser then refuses the 404's HTML with a MIME error, as it
# did once in production. Every relative import in a staged script must
# resolve inside the staged tree.
verify_staged_demo_imports() {
    local demo_directory="$1"
    local script_path
    local import_specifier
    local import_target
    local missing_imports=""
    for script_path in "${demo_directory}"/*.mjs; do
        [ -f "${script_path}" ] || continue
        # Every relative specifier a module can name: `from './x'`,
        # `import './x'` and the dynamic `import('./x')` form.
        while IFS= read -r import_specifier; do
            [ -n "${import_specifier}" ] || continue
            import_target="${demo_directory}/${import_specifier#./}"
            [ -f "${import_target}" ] \
                || missing_imports="${missing_imports}\n  ${script_path#"${demo_directory}/"} imports ${import_specifier}"
        done < <(perl -ne "while (/(?:from|import)\s*\(?\s*'(\.[^']*)'/g) { print \"\$1\n\" }" "${script_path}")
    done
    if [ -n "${missing_imports}" ]; then
        fail "the staged demo is missing files its modules import:${missing_imports}"
    fi
}

# The recorded walkthrough (`docs/Release.md` (The recorded walkthrough)): the
# player, its embedded data and the normalised stills the contract names, built
# by the walkthrough crate into /demo/walkthrough/. The crate validates the
# contract before it writes anything, so an invalid contract fails the build.
stage_walkthrough() {
    local stage_root="$1"
    local walkthrough_directory="${stage_root}/site/demo/walkthrough"
    log_step "building the recorded walkthrough at /demo/walkthrough"
    (cd "$REPO_ROOT" && cargo run --quiet -p "$WALKTHROUGH_CRATE_NAME" -- build \
        --output "$walkthrough_directory" \
        --contract "$WALKTHROUGH_CONTRACT_RELATIVE_PATH") \
        || fail "the recorded walkthrough failed to build; the site would publish no player"
    verify_staged_walkthrough "$walkthrough_directory"
}

# The walkthrough is a build artifact: the stylesheet and script must be
# byte-equal to the crate's committed player, and every copied still byte-equal
# to its normalised source. The document must carry the generated player data.
# This is the walkthrough twin of the demo copy's byte-equality gate.
verify_staged_walkthrough() {
    local walkthrough_directory="$1"
    local player_source_root="${REPO_ROOT}/crates/${WALKTHROUGH_CRATE_NAME}/player"
    local asset_source_root="${REPO_ROOT}/crates/${WALKTHROUGH_CRATE_NAME}/assets/normalised"
    local player_file
    for player_file in index.html player.css player.js manifest.json; do
        [ -s "${walkthrough_directory}/${player_file}" ] \
            || fail "the staged walkthrough is missing ${player_file}"
    done
    cmp -s "${walkthrough_directory}/player.css" "${player_source_root}/player.css" \
        || fail "the staged walkthrough's player.css drifted from the crate's player"
    cmp -s "${walkthrough_directory}/player.js" "${player_source_root}/player.js" \
        || fail "the staged walkthrough's player.js drifted from the crate's player"
    grep -q '"platform_order"' "${walkthrough_directory}/index.html" \
        || fail "the staged walkthrough's index.html carries no generated player data"
    local staged_asset
    local source_asset
    while IFS= read -r staged_asset; do
        source_asset="${asset_source_root}/$(basename "$staged_asset")"
        [ -f "$source_asset" ] \
            || fail "the staged walkthrough asset $(basename "$staged_asset") has no normalised source"
        cmp -s "$staged_asset" "$source_asset" \
            || fail "the staged walkthrough asset $(basename "$staged_asset") drifted from its normalised source"
    done < <(find "${walkthrough_directory}/assets" -type f -name '*.png' | sort)
    python3 -I - "$walkthrough_directory" "$WALKTHROUGH_MAXIMUM_BYTES" <<'WALKTHROUGH_SIZE_EOF'
import sys
from pathlib import Path

walkthrough_directory = Path(sys.argv[1])
maximum_bytes = int(sys.argv[2])
total_bytes = sum(
    file.stat().st_size for file in walkthrough_directory.rglob('*') if file.is_file()
)
print(f"recorded walkthrough size: {total_bytes} bytes (limit {maximum_bytes})")
if total_bytes > maximum_bytes:
    print(
        f"the recorded walkthrough exceeds the publish size limit: {total_bytes} > {maximum_bytes}",
        file=sys.stderr,
    )
    sys.exit(1)
WALKTHROUGH_SIZE_EOF
}

# The demo copy is a byte-for-byte copy of the harness inputs (an anti-drift
# gate beside the UDL render gates), ships no test-only `/golden` route, and
# stays inside the publish size budget.
verify_staged_demo() {
    local stage_root="$1"
    log_step "verifying the staged live demo (byte equality, no golden, size)"
    local demo_directory="${stage_root}/site/demo"
    local copied_file
    for copied_file in index.html playground.mjs styles/playground.css main.mjs \
        playground_preferences.mjs playground_provider_settings.mjs \
        playground_base_map.mjs playground_overlays.mjs playground_navigation.mjs \
        playground_diagnostics.mjs playground_map_surface.mjs \
        map_controls.mjs map_layers.mjs overlay_scenes.mjs vector_layer.mjs tile_switch.mjs \
        tile_cache.mjs ui_strings.mjs; do
        cmp -s "${demo_directory}/${copied_file}" "${WEB_HARNESS_DIRECTORY}/${copied_file}" \
            || fail "the staged demo's ${copied_file} drifted from platform/web/${copied_file}"
    done
    local harness_package_file
    for harness_package_file in "${WEB_HARNESS_DIRECTORY}/generated/"*; do
        cmp -s "${demo_directory}/generated/$(basename "$harness_package_file")" "$harness_package_file" \
            || fail "the staged demo's generated/$(basename "$harness_package_file") drifted from platform/web/generated"
    done
    cmp -s "${demo_directory}/glyphs/${GLYPH_SERVED_FONT_STACK}/${GLYPH_SERVED_RANGE}.pbf" \
        "${GLYPH_FIXTURES_DIRECTORY}/${GLYPH_FIXTURE_FILE_NAME}" \
        || fail "the staged demo's glyph range drifted from the committed fixture"
    cmp -s "${stage_root}/site/reference-scenes.json" "${REPO_ROOT}/platform/reference-scenes.json" \
        || fail "the staged demo's reference-scenes.json drifted from platform/reference-scenes.json"
    cmp -s "${stage_root}/site/reference-flows.json" "${REPO_ROOT}/platform/reference-flows.json" \
        || fail "the staged demo's reference-flows.json drifted from platform/reference-flows.json"
    # Every artifact-declared demo plan must have its recorded trace staged
    # byte-identically; a plan without a trace would fail only when a visitor
    # selected it in production.
    local trace_listing="${STAGE_ROOT}/demo-plan-traces.txt"
    list_demo_plan_traces "$trace_listing"
    local scenario
    local fixture_path
    while IFS=$'\t' read -r scenario fixture_path; do
        [ -n "$scenario" ] || continue
        cmp -s "${demo_directory}/navigation-traces/${scenario}.json" \
            "${REPO_ROOT}/${fixture_path}" \
            || fail "the staged demo's navigation-traces/${scenario}.json is missing or drifted from ${fixture_path}"
    done < "$trace_listing"
    rm -f "$trace_listing"
    [ ! -e "${demo_directory}/golden" ] \
        || fail "the live demo must not ship the test-only /golden route"
    # The recorded walkthrough shares /demo/ but has its own budget; the live
    # demo's limit covers only the wasm harness copied beside it.
    python3 -I - "$demo_directory" "$LIVE_DEMO_MAXIMUM_BYTES" <<'DEMO_SIZE_EOF'
import sys
from pathlib import Path

demo_directory = Path(sys.argv[1])
maximum_bytes = int(sys.argv[2])
walkthrough_directory = demo_directory / 'walkthrough'
total_bytes = sum(
    file.stat().st_size
    for file in demo_directory.rglob('*')
    if file.is_file() and walkthrough_directory not in file.parents
)
print(f"live demo size: {total_bytes} bytes (limit {maximum_bytes})")
if total_bytes > maximum_bytes:
    print(
        f"live demo exceeds the publish size limit: {total_bytes} > {maximum_bytes}",
        file=sys.stderr,
    )
    sys.exit(1)
DEMO_SIZE_EOF
}

# The recorded raster tile fixtures are test data. The dev server answers them
# from the repository, but the published site must carry no directory the demo
# could serve as tile imagery: the provider's terms cap how long its imagery
# may be stored, so a published tile copy is refused. The rustdoc tree is
# pruned because a crate module may legitimately be named `tiles`.
verify_no_served_tile_directory() {
    local stage_root="$1"
    log_step "verifying the staged site carries no recorded tile directory"
    local served_tile_directories
    served_tile_directories="$(find "${stage_root}/site" \
        -path "${stage_root}/site/api" -prune \
        -o -type d -name tiles -print)"
    [ -z "$served_tile_directories" ] \
        || fail "the staged site must not serve a recorded tile directory: ${served_tile_directories}"
}

# The UDL contract reaches the site through two generated copies: the
# plain-text copy at `site/udl/rustic_maps.udl` and the fenced block
# at the published plain-text path. It must match the source file byte for
# byte on every publish; a mismatch means someone edited a copy (or staged a
# stale one) and the site would drift from the source contract. The authored
# contract chapter explains that surface for outside developers; it does not
# republish the raw file. The build refuses on any drift. The rustdoc tree is generated
# fresh by `stage_reference_material`; its presence is asserted here so a
# silent staging failure can never publish a dangling `api/` link.
# Copies the locally built release artifacts into the site's versioned
# downloads path when they exist, so the published page can link real files.
# The release workflow produces the same tree; a site build without artifacts
# simply stages no downloads directory and the page keeps its build-from-source
# guidance.
stage_release_downloads() {
    local stage_root="$1"
    local workspace_version
    workspace_version="$(sed -n 's/^version = "\(.*\)"/\1/p' "${REPO_ROOT}/Cargo.toml" | head -n 1)"
    local artifacts_dir="${RELEASE_ARTIFACTS_DIR:-${REPO_ROOT}/target/release-artifacts/v${workspace_version}}"
    if [ ! -d "$artifacts_dir" ]; then
        printf 'note: no release artifacts at %s; the downloads page keeps its build-from-source guidance\n' "$artifacts_dir"
        return
    fi
    log_step "staging release downloads from ${artifacts_dir}"
    mkdir -p "${stage_root}/site/downloads/v${workspace_version}"
    # GitHub refuses any single file above 100 MB in a repository, and the
    # Pages branch is a repository: an artifact that large stays on the
    # Release and the downloads table links it there. The sidecars and
    # SHA256SUMS.txt always travel.
    local artifact_path
    for artifact_path in "${artifacts_dir}"/*; do
        [ -f "$artifact_path" ] || continue
        if [ "$(wc -c < "$artifact_path" | tr -d ' ')" -gt "${GITHUB_MAXIMUM_FILE_BYTES_IN_REPOSITORY}" ]; then
            printf 'note: %s exceeds the 100 MB file limit a GitHub repository allows; the release hosts it and the page links the release asset\n' "$(basename "$artifact_path")"
            continue
        fi
        cp "$artifact_path" "${stage_root}/site/downloads/v${workspace_version}/"
    done
}

verify_staged_render_fidelity() {
    local stage_root="$1"
    log_step "verifying staged contract copies and the rustdoc tree (byte equality)"
    local staged_plain_text="${stage_root}/site/udl/rustic_maps.udl"
    local rendered_chapter="${stage_root}/book-src/udl-contract.md"

    cmp -s "$staged_plain_text" "$UDL_CONTRACT_PATH" \
        || fail "site/udl/rustic_maps.udl drifted from ${UDL_CONTRACT_PATH}; the staged copy must be byte-identical"

    [ -f "$rendered_chapter" ] || fail "the rendered contract chapter is missing from the book sources"

    # The api-reference chapter links into the rustdoc tree; assert the
    # tree was staged and the entry link's target exists, so a silent
    # staging failure can never publish a dangling link.
    [ -d "${stage_root}/site/api" ] \
        || fail "the rustdoc tree was not staged; the api-reference chapter would link nothing"
    [ -f "${stage_root}/site/api/geospatial_types/index.html" ] \
        || fail "the staged rustdoc tree lacks geospatial_types, the api-reference chapter's entry link"
}

run_prepublish_scrub() {
    local site_dir="$1"
    log_step "running the fail-closed pre-publish scrub"
    # The scrub runs its own self-check first (owner ruling, packaging
    # identities, 2026-10-29): the synthetic cases assert that the
    # package-coordinate allowance is narrow, that a bare `com.angoratek`
    # stays denied, and that a denied path fails regardless of its
    # contents. Both staging and publishing runs execute the same checked
    # code, and a misbehaving case fails the build loudly.
    python3 -I - "$site_dir" --self-check <<'SCRUB_EOF'
import re
import sys
import tempfile
from pathlib import Path

# Filesystem names and path fragments that must never appear in the staged
# tree (owner rule: nothing private, nothing personal, no secrets). This
# check runs before any content check, so a denied path fails no matter
# what its contents say.
denied_path_pattern = re.compile(
    r"(^|/)(PLAN\.md|PRD\.md|CLAUDE\.md|\.env|local\.properties|Local\.xcconfig)$"
    r"|(^|/)(research|history|contracts-draft)"
    # A build machine's absolute source path (rustdoc renders the OUT_DIR of
    # generated files as a directory tree) never belongs on the public site.
    r"|(^|/)(Users|home)/[^/]+/"
)
# Case-insensitive content denylist: the private development host. (The
# `com.angoratek` identity is checked per occurrence below, with its
# package-coordinate allowance: the blanket string denial of the pre-amend
# scrub would refuse the published Maven group the installation pages
# name.)
denied_content_pattern = re.compile(
    r"dev\.mapssdk|127\.0\.0\.1:8642"
    # The development repository's own URL never appears on the public site
    # (owner direction 2026-10-29); the docs repository's `-docs` suffix is a
    # different name and stays available for ordinary links. Both the web
    # URL and the SSH clone form are refused; the bare product name stays
    # available because every artifact name and npm scope contains it.
    r"|github\.com/angoratek/rustic-maps-sdk(?![A-Za-z0-9._-])"
    r"|git@github\.com:angoratek/rustic-maps-sdk(?:\.git)?(?![A-Za-z0-9._-])"
    # A build machine's home directory never appears in published content.
    r"|/Users/[^/\s\"'<>]+|/home/[^/\s\"'<>]+",
    re.IGNORECASE,
)
# The published Maven group. Owner amendment of 2026-10-29: the string
# stays denied as a bare/standalone form (bundle-id usage) and is allowed
# only in the two narrow structural shapes of a published package
# coordinate. This is a position-anchored allowance on the exact string
# `com.angoratek`, never a blanket whitelist of the group name.
maven_group_pattern = re.compile(r"com\.angoratek", re.IGNORECASE)
# 1. The Maven dependency coordinate `group:artifact[:version]`: the colon
#    is the coordinate separator the Maven and Gradle notations require,
#    and the artifact id must be a Maven artifact id (`rustic-maps-sdk`).
allowed_maven_coordinate_pattern = re.compile(
    r"com\.angoratek:[a-z0-9][a-z0-9._-]*(?::[0-9][0-9a-zA-Z._+-]*)?",
    re.IGNORECASE,
)
# 2. The released artifact's dotted form (`com.angoratek.rustic-maps-sdk`).
#    The shipped Kotlin package is `com.rusticmaps.sdk`, outside the denied
#    group, so it needs no allowance. A different dotted continuation — a
#    private bundle id's next segment — does not match and stays denied.
allowed_artifact_package_pattern = re.compile(
    r"com\.angoratek\.rustic-maps-sdk[a-z0-9._-]*",
    re.IGNORECASE,
)
# Key material: a `key`-named assignment carrying a token-looking value. A
# bare environment-variable *name* (e.g. PROVIDER_API_KEY in usage text) is
# permitted, and so is an all-lowercase snake_case identifier on the right
# side (variables like `api_key: mapquest_api_key` are code, not secrets);
# anything else of sixteen-plus mixed token characters is refused.
key_assignment_pattern = re.compile(
    r"[A-Za-z_]*(?:mapquest|api)[A-Za-z_]*key[A-Za-z_]*\s*[:=]\s*[\"']?"
    r"([A-Za-z0-9_-]{16,})"
)
lowercase_snake_case_identifier_pattern = re.compile(r"[a-z][a-z0-9]*(?:_[a-z0-9]+)+")
# Internal-development vocabulary that never reads correctly to an external
# developer (owner direction 2026-10-29). Matched as whole phrases, only in
# the hand-authored chapters at the site root: the generated rustdoc tree and
# the demo's own sources keep their internal vocabulary.
internal_vocabulary_patterns = [
    re.compile(r"frozen (?:ffi |uniffi )?contract", re.IGNORECASE),
    re.compile(r"\bfixtures?\b", re.IGNORECASE),
    re.compile(r"\bharness(?:es)?\b", re.IGNORECASE),
    re.compile(r"\bplaygrounds?\b", re.IGNORECASE),
    re.compile(r"\bmilestones?\b", re.IGNORECASE),
    # A numbered development round ("round 26", "round-10"); "round trip" in
    # the routing prose is the network sense and deliberately not matched.
    re.compile(r"\bround[- ]?[0-9]+\b", re.IGNORECASE),
    # "slice" is legitimate architecture prose (an arm64 slice, a binary
    # slice, a slice of the per-frame counts), so only the development-
    # process senses are refused rather than the bare word.
    re.compile(
        r"\b(?:later|this|next|previous|earlier|current)\s+slice\b"
        r"|\b(?:contract|surface|docs?|documentation|release|web)\s+slice\b",
        re.IGNORECASE,
    ),
]


# The chapter pages whose prose this project authors (the file names of the
# composed book). Generated aggregates (`print.html`), the rendered contract,
# the notices and the rustdoc tree keep their own wording.
hand_authored_chapter_pages = {
    "index.html", "requirements.html", "performance.html", "live-demo.html",
    "provider-console.html",
    "walkthrough.html", "how-it-works.html", "quickstart.html",
    "map-concepts.html", "first-map.html", "search-and-routing.html",
    "navigation-simulation.html", "offline-maps.html",
    "map-layers-and-traffic.html",
    "versions-and-support.html", "faq.html", "downloads.html",
    "screenshots.html", "changelog.html", "api-reference.html",
    # The five platform API reference pages are generated build artifacts
    # (scripts/generate-api-reference.mjs), so they keep the contract's and
    # the source comments' wording like the rustdoc tree; the authored
    # landing page stays in this list and is scanned.
    "udl-contract.html",
    "provider-capabilities-android.html", "provider-capabilities-ios.html",
    "provider-capabilities-react-native.html", "provider-capabilities-wasm.html",
    "provider-capabilities-rust.html", "adding-a-provider.html",
}


def is_hand_authored_chapter(staged_file: Path, tree_dir: Path) -> bool:
    """True for a chapter page this project wrote (never generated trees)."""
    relative_path = staged_file.relative_to(tree_dir)
    return (
        len(relative_path.parts) == 1
        and relative_path.name in hand_authored_chapter_pages
    )


def scrub_tree(tree_dir: Path) -> list:
    """Returns the violations the fail-closed scrub finds under `tree_dir`."""
    violations = []
    for staged_file in sorted(tree_dir.rglob("*")):
        if not staged_file.is_file():
            continue
        if denied_path_pattern.search(staged_file.name) or denied_path_pattern.search(
            str(staged_file.relative_to(tree_dir))
        ):
            violations.append(f"path: {staged_file.relative_to(tree_dir)}")
            continue
        try:
            staged_text = staged_file.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        if denied_content_pattern.search(staged_text):
            violations.append(f"content: {staged_file.relative_to(tree_dir)}")
            continue
        if any(
            allowed_maven_coordinate_pattern.match(staged_text, group_match.start()) is None
            and allowed_artifact_package_pattern.match(staged_text, group_match.start()) is None
            for group_match in maven_group_pattern.finditer(staged_text)
        ):
            violations.append(
                f"content: {staged_file.relative_to(tree_dir)} (bare com.angoratek)"
            )
        for match in key_assignment_pattern.finditer(staged_text):
            candidate_value = match.group(1)
            if lowercase_snake_case_identifier_pattern.fullmatch(candidate_value):
                continue
            violations.append(f"key material: {staged_file.relative_to(tree_dir)}")
        if is_hand_authored_chapter(staged_file, tree_dir):
            # Match visible prose only: HTML tags and attributes carry class
            # names (for example mdBook's code-block playground controls) that
            # are not the page's vocabulary.
            visible_text = re.sub(r"<[^>]*>", " ", staged_text)
            for vocabulary_pattern in internal_vocabulary_patterns:
                vocabulary_match = vocabulary_pattern.search(visible_text)
                if vocabulary_match is not None:
                    violations.append(
                        f"internal vocabulary: {staged_file.relative_to(tree_dir)} "
                        f"({vocabulary_match.group(0)})"
                    )
                    break
    return violations


def run_self_check() -> None:
    """Asserts the scrub's fail-closed behavior with synthetic cases."""
    allowed_coordinate_text = (
        "install the artifacts: the Maven coordinate"
        " com.angoratek:rustic-maps-sdk:0.1.0, the artifact's package form"
        " com.angoratek.rustic-maps-sdk, and the npm scope"
        " @angoratek/rustic-maps-sdk-web\n"
    )
    denied_bundle_id_text = "bundle id com.angoratek.refapp stays denied\n"
    private_repository_url_text = (
        "clone from https://github.com/angoratek/rustic-maps-sdk for the sources\n"
    )
    internal_vocabulary_text = "This surface joins the contract in a later slice.\n"
    architecture_slice_text = "The artifact ships one arm64 slice and one x86_64 slice.\n"
    check_cases = [
        (
            "allowed package coordinate passes",
            "case.txt",
            allowed_coordinate_text,
            [],
        ),
        (
            "bare com.angoratek is refused",
            "case.txt",
            denied_bundle_id_text,
            ["content: case.txt (bare com.angoratek)"],
        ),
        (
            "the private repository URL is refused",
            "case.txt",
            private_repository_url_text,
            ["content: case.txt"],
        ),
        (
            "internal vocabulary is refused in an authored chapter",
            "first-map.html",
            internal_vocabulary_text,
            ["internal vocabulary: first-map.html (later slice)"],
        ),
        (
            "an architecture slice passes in an authored chapter",
            "first-map.html",
            architecture_slice_text,
            [],
        ),
    ]
    for check_name, case_name, case_text, expected_violations in check_cases:
        with tempfile.TemporaryDirectory() as scratch:
            case_file = Path(scratch) / case_name
            case_file.write_text(case_text, encoding="utf-8")
            observed_violations = scrub_tree(case_file.parent)
        if observed_violations != expected_violations:
            print(
                f"scrub self-check FAILED: case '{check_name}' expected"
                f" {expected_violations!r}, observed {observed_violations!r}",
                file=sys.stderr,
            )
            sys.exit(1)
    # A denied path stays a failure regardless of its contents: the
    # allowlisted coordinate inside a file named after a denied
    # configuration path must never rescue it.
    with tempfile.TemporaryDirectory() as scratch:
        case_file = Path(scratch) / "local.properties"
        case_file.write_text(allowed_coordinate_text, encoding="utf-8")
        observed_violations = scrub_tree(case_file.parent)
        expected_violations = ["path: local.properties"]
        if observed_violations != expected_violations:
            print(
                "scrub self-check FAILED: the denied-path case observed"
                f" {observed_violations!r}, expected {expected_violations!r}",
                file=sys.stderr,
            )
            sys.exit(1)
    print("scrub self-check: 6/6 cases behave as ruled")


def main() -> None:
    run_self_check()
    violations = scrub_tree(Path(sys.argv[1]))
    if violations:
        print("\n".join(f"scrub violation: {entry}" for entry in violations), file=sys.stderr)
        print(f"scrub: {len(violations)} violation(s) — refusing to publish", file=sys.stderr)
        sys.exit(1)
    print("scrub: clean")


main()
SCRUB_EOF
}

publish_site() {
    local site_dir="$1"
    local source_commit="$2"
    local gates_green_commit="$3"
    local pages_repo_dir="${PAGES_REPO_DIR:-}"
    [ -n "$pages_repo_dir" ] || fail "--publish requires the PAGES_REPO_DIR environment variable pointing at a checkout of ${PUBLIC_PAGES_REPOSITORY_NAME}"
    [ -d "$pages_repo_dir/.git" ] || fail "${pages_repo_dir} is not a git checkout"
    [ -z "$(git -C "$REPO_ROOT" status --porcelain)" ] || fail "the source tree is dirty; refusing to publish"
    pages_repo_dir="$(cd "$pages_repo_dir" && pwd)"

    cd "$pages_repo_dir"
    # Render commits carry the source repository's configured identity, not
    # whatever the clone environment guessed (a fresh clone with no git
    # identity would otherwise publish a personal name/email address).
    local pages_git_author
    local pages_git_email
    pages_git_author="$(git -C "$REPO_ROOT" config user.name)"
    pages_git_email="$(git -C "$REPO_ROOT" config user.email)"
    [ -n "$pages_git_author" ] && [ -n "$pages_git_email" ] \
        || fail "the source repository has no configured git identity for render commits"
    git -C . config user.name "$pages_git_author"
    git -C . config user.email "$pages_git_email"
    git remote get-url origin | grep -q "${PUBLIC_PAGES_REPOSITORY_NAME}" \
        || fail "${pages_repo_dir} is not a checkout of ${PUBLIC_PAGES_REPOSITORY_NAME}"
    if git show-ref --verify --quiet refs/heads/gh-pages; then
        git checkout gh-pages
    else
        git checkout --orphan gh-pages
        git rm -rf --quiet . 2>/dev/null || true
    fi
    # The mirror prints every path it deletes and keeps an existing
    # `downloads/` tree unless the staged tree carries a complete replacement
    # set, so a source checkout that never built the release artifacts cannot
    # silently delete the published downloads.
    node "${REPO_ROOT}/scripts/mirror-docs-site-tree.mjs" "$site_dir" "$pages_repo_dir"
    printf 'gates-green-asserted-by-publisher: %s (recorded %s)\n' \
        "$gates_green_commit" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" >> provenance.txt
    git add -A
    git commit -m "docs: render site from ${source_commit}"
    git push origin gh-pages
}

main() {
    local publish_requested="no"
    local gates_green_commit=""
    for argument in "$@"; do
        case "$argument" in
            --publish) publish_requested="yes" ;;
            # The publisher names the commit the CI gate set passed for. The
            # script cannot observe CI, so it pins the assertion to the exact
            # tree being published instead of accepting a bare honor flag.
            --gates-green=*) gates_green_commit="${argument#--gates-green=}" ;;
            --gates-green) fail "--gates-green now takes the asserted commit: --gates-green=<commit>" ;;
            *) fail "unknown argument: ${argument}" ;;
        esac
    done
    [ "$publish_requested" = "no" ] || [ -n "$gates_green_commit" ] \
        || fail "--publish requires --gates-green=<commit> (the commit the CI gate set passed for)"

    need_tool cargo
    need_tool mdbook
    need_tool node
    check_source_tree
    if [ -n "$gates_green_commit" ]; then
        local head_commit
        head_commit="$(git -C "$REPO_ROOT" rev-parse HEAD)"
        [ "$gates_green_commit" = "$head_commit" ] \
            || fail "--gates-green names ${gates_green_commit}, but HEAD is ${head_commit}; the assertion must name the tree being published"
    fi

    rm -rf "$STAGE_ROOT"
    mkdir -p "$STAGE_ROOT"

    regenerate_third_party_notices
    check_provider_documentation
    build_book "$STAGE_ROOT"
    verify_summary_coverage "$STAGE_ROOT"
    stage_reference_material "$STAGE_ROOT"
    stage_live_demo "$STAGE_ROOT"
    stage_walkthrough "$STAGE_ROOT"
    stage_release_downloads "$STAGE_ROOT"
    verify_staged_render_fidelity "$STAGE_ROOT"
    verify_staged_demo "$STAGE_ROOT"
    verify_no_served_tile_directory "$STAGE_ROOT"
    run_prepublish_scrub "$STAGE_ROOT/site"
    # mdBook clears its destination, so the provenance stamp lands after the
    # build; the publish step then appends the publisher's gate assertion.
    local source_commit
    source_commit="$(stamp_provenance "$STAGE_ROOT/site/provenance.txt")"
    # The generated pages carry their own markers; this records the revision
    # the published tree was rendered from beside the source commit.
    printf 'generated_api_reference_revision: %s\n' "$GENERATED_API_REFERENCE_REVISION" \
        >> "$STAGE_ROOT/site/provenance.txt"

    if [ "$publish_requested" = "yes" ]; then
        publish_site "$STAGE_ROOT/site" "$source_commit" "$gates_green_commit"
    else
        printf '\nstaged site ready (not published): %s/site\n' "$STAGE_ROOT"
        printf 'publish with: scripts/build-docs-site.sh --publish --gates-green=%s  (PAGES_REPO_DIR=<checkout of %s>)\n' \
            "$source_commit" "$PUBLIC_PAGES_REPOSITORY_NAME"
    fi
}

main "$@"