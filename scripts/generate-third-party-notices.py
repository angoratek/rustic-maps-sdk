#!/usr/bin/env python3
"""Generate docs/THIRD_PARTY_NOTICES.md from the workspace dependency graph.

Reads `cargo metadata` (offline — no network access) for the workspace reachable
from crates/*.toml and Cargo.lock, resolves every third-party package's license
(SPDX expression), homepage/repository, and — where the vendored crate carries
license files in the local cargo cache — its copyright notice lines.

Deterministic by construction: entries are sorted (license expression, package
name, version), nothing time-dependent is emitted, so re-running on an
unchanged dependency graph yields a byte-identical file.

Usage (from anywhere):

    scripts/generate-third-party-notices

Regenerate whenever any `Cargo.toml` / `Cargo.lock` file changes, and commit
the refreshed `docs/THIRD_PARTY_NOTICES.md` alongside the dependency change.
"""

import json
import re
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
NOTICES_OUTPUT_PATH = REPO_ROOT / "docs" / "THIRD_PARTY_NOTICES.md"
REGISTRY_SOURCE_ROOT = Path.home() / ".cargo" / "registry" / "src"
LICENSE_FILE_NAME_PATTERN = re.compile(r"^(LICENSE|COPYING|COPYRIGHT)", re.IGNORECASE)
# A copyright line: "Copyright (c) 2025 The gfx-rs developers", "© 2019 ACME".
COPYRIGHT_LINE_PATTERN = re.compile(
    r"[Cc]opyright(?:\s+\(c\)|\s+\(©\))?[,]?\s+\d{4}|©\s*\d{4}"
)
# License-file boilerplate mentioning "copyright" is not an attribution line.
COPYRIGHT_LINE_EXCLUSION_PATTERN = re.compile(
    r"the above copyright|copyright holders|copyright notice"
    r"|copyright owner|copyright that is included|shall mean"
    r"|owners or entity authorized",
    re.IGNORECASE,
)
MAXIMUM_NOTICE_LINES_PER_CRATE = 3
# SPDX identifier prefixes that carry redistribution obligations beyond the
# plain permissive set; each gets a full-text pointer in its license group.
WEAK_COPYLEFT_IDENTIFIER_PATTERN = re.compile(
    r"^(MPL|LGPL|GPL|AGPL|CDDL|CPL|EPL|EUPL|OSL|RSCPL)[.\-]"
)
SPDX_LICENSE_TEXT_URL_TEMPLATE = "https://spdx.org/licenses/{}.txt"
DEVELOPMENT_ONLY_MARK = "†"

# Notes for license identities found (or plausibly found) in this workspace,
# keyed by the exact SPDX identifier the note describes.
LICENSE_IDENTITY_NOTES = {
    "MPL-2.0": (
        "The Mozilla Public License 2.0 applies only to the source files of the "
        "covered crates (file-level copyleft — not viral). This SDK consumes "
        "those crates as libraries without modifying their sources. Full text: "
        "{text_url}. Note: the published `uniffi` crate family ships no license "
        "text file inside its package; the full text lives in each crate's "
        "repository."
    ),
    "LGPL-2.1-or-later": (
        "These packages offer LGPL-2.1-or-later only as one alternative inside "
        "a permissive SPDX expression; this SDK relies on the permissive "
        "alternatives. Full text: {text_url}."
    ),
    "CDLA-Permissive-2.0": (
        "This is a data license, not a software license: it covers the root-"
        "certificate bundle published by `webpki-roots`, not code. It permits "
        "use, sharing and commercial use with attribution. Full text: "
        "{text_url}."
    ),
}


def load_workspace_metadata():
    """Run `cargo metadata` offline and return the parsed JSON document."""
    try:
        metadata_process = subprocess.run(
            ["cargo", "metadata", "--offline", "--format-version", "1"],
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
            check=True,
        )
    except subprocess.CalledProcessError as metadata_failure:
        print(
            "error: `cargo metadata --offline` failed; is Cargo.lock in sync "
            "with the workspace manifests? Generate or update the lockfile, "
            "then retry.\n" + metadata_failure.stderr,
            file=sys.stderr,
        )
        raise SystemExit(1) from metadata_failure
    return json.loads(metadata_process.stdout)


def collect_runtime_package_ids(metadata):
    """Return the package ids reachable through normal (non-dev) dependencies.

    Breadth-first walk over the resolved dependency graph, starting at the
    workspace members, following only edges with a normal (None-kind) edge
    kind. Target-gated normal dependencies (platform-specific code paths)
    count as runtime by design.
    """
    dependency_targets_by_id = {}
    for resolved_node in metadata["resolve"]["nodes"]:
        dependency_targets_by_id[resolved_node["id"]] = [
            resolved_dependency["pkg"]
            for resolved_dependency in resolved_node["deps"]
            if any(
                dependency_kind["kind"] is None
                for dependency_kind in resolved_dependency["dep_kinds"]
            )
        ]

    runtime_package_ids = set()
    frontier = list(metadata["workspace_members"])
    while frontier:
        current_package_id = frontier.pop()
        if current_package_id in runtime_package_ids:
            continue
        runtime_package_ids.add(current_package_id)
        frontier.extend(
            dependency_id
            for dependency_id in dependency_targets_by_id.get(current_package_id, [])
            if dependency_id not in runtime_package_ids
        )
    return runtime_package_ids


def find_vendored_crates_directory(package_name, package_version):
    """Locate the vendored checkout of a registry crate in the local cargo
    cache, without touching the network. Returns None when absent."""
    if not REGISTRY_SOURCE_ROOT.is_dir():
        return None
    for registry_index_directory in sorted(REGISTRY_SOURCE_ROOT.iterdir()):
        candidate = registry_index_directory / f"{package_name}-{package_version}"
        if candidate.is_dir():
            return candidate
    return None


def extract_copyright_notice_lines(vendored_crates_directory):
    """Collect human copyright lines from the vendored crate's license files.

    Keeps only short first attribution lines (license boilerplate is filtered
    out), capped so the document stays readable. Empty when the crate ships no
    extractable attribution (e.g. the uniffi family).
    """
    notice_lines = []
    for license_path in sorted(vendored_crates_directory.iterdir()):
        if not license_path.is_file() or not LICENSE_FILE_NAME_PATTERN.match(
            license_path.name
        ):
            continue
        try:
            license_text = license_path.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        for candidate_line in license_text.splitlines():
            stripped_line = candidate_line.strip().lstrip("*").strip()
            if not stripped_line or not COPYRIGHT_LINE_PATTERN.search(stripped_line):
                continue
            if COPYRIGHT_LINE_EXCLUSION_PATTERN.search(stripped_line):
                continue
            if stripped_line in notice_lines:
                continue
            notice_lines.append(stripped_line)
            if len(notice_lines) >= MAXIMUM_NOTICE_LINES_PER_CRATE:
                return notice_lines
    return notice_lines


def collect_third_party_entries(metadata):
    """Build the sorted list of third-party package entries for the report."""
    workspace_member_ids = set(metadata["workspace_members"])
    runtime_package_ids = collect_runtime_package_ids(metadata)

    entries = []
    for metadata_package in metadata["packages"]:
        if metadata_package["id"] in workspace_member_ids:
            # First-party workspace crates are covered by the SDK's own
            # license (MIT OR Apache-2.0); they are not third-party.
            continue
        homepage_url = metadata_package.get("homepage") or metadata_package.get(
            "repository"
        )
        vendored_directory = find_vendored_crates_directory(
            metadata_package["name"], metadata_package["version"]
        )
        entries.append(
            {
                "name": metadata_package["name"],
                "version": metadata_package["version"],
                "license": metadata_package["license"] or "SPDX unknown — see crate",
                "homepage": homepage_url,
                "is_runtime": metadata_package["id"] in runtime_package_ids,
                "copyright_notices": (
                    extract_copyright_notice_lines(vendored_directory)
                    if vendored_directory
                    else []
                ),
            }
        )
    entries.sort(key=lambda entry: (entry["license"], entry["name"], entry["version"]))
    return entries


def group_entries_by_license(entries):
    """Group entries by their raw SPDX license expression, preserving the
    entry order (already sorted) inside each group."""
    groups = {}
    for entry in entries:
        groups.setdefault(entry["license"], []).append(entry)
    return groups


def license_identifier_tokenizer(license_expression):
    """Split an SPDX expression into identifiers and logical operators."""
    return re.findall(r"[A-Za-z0-9.\-]+|WITH|OR|AND", license_expression)


def collect_license_group_notes(license_expression):
    """Render notes owed by a license expression (copyleft pointers etc.)."""
    note_text_lines = []
    for license_identifier in license_identifier_tokenizer(license_expression):
        if license_identifier in LICENSE_IDENTITY_NOTES:
            note_text_lines.append(
                LICENSE_IDENTITY_NOTES[license_identifier].format(
                    text_url=SPDX_LICENSE_TEXT_URL_TEMPLATE.format(license_identifier)
                )
            )
        elif WEAK_COPYLEFT_IDENTIFIER_PATTERN.match(license_identifier):
            note_text_lines.append(
                "Non-permissive license identity detected; review before "
                "redistribution. Full text: "
                + SPDX_LICENSE_TEXT_URL_TEMPLATE.format(license_identifier)
            )
    return note_text_lines


def render_license_group(section_text, license_expression, entries):
    """Append one license-group section (table, footnotes, notes)."""
    section_text.append(f"### `{license_expression}` ({len(entries)} package(s))")
    section_text.append("")
    section_text.append("| Package | Version | Homepage |")
    section_text.append("|---|---|---|")
    for entry in entries:
        homepage_cell = f'<{entry["homepage"]}>' if entry["homepage"] else "—"
        package_cell = entry["name"]
        if not entry["is_runtime"]:
            package_cell += DEVELOPMENT_ONLY_MARK
        section_text.append(
            f"| `{package_cell}` | {entry['version']} | {homepage_cell} |"
        )
    section_text.append("")
    if any(not entry["is_runtime"] for entry in entries):
        section_text.append(
            f"{DEVELOPMENT_ONLY_MARK} development/test-only dependency: compiled "
            "inside this repository's own test harnesses, but not linked into a "
            "published SDK artifact."
        )
        section_text.append("")
    for license_note in collect_license_group_notes(license_expression):
        section_text.append(license_note)
        section_text.append("")


def render_document(entries):
    """Render the full THIRD_PARTY_NOTICES.md content."""
    groups = group_entries_by_license(entries)
    runtime_count = sum(1 for entry in entries if entry["is_runtime"])
    development_only_count = len(entries) - runtime_count

    section_text = []
    section_text.append("# Third-Party Notices")
    section_text.append("")
    section_text.append(
        "Generated by `scripts/generate-third-party-notices` from the workspace "
        "dependency graph (`Cargo.toml` manifests + `Cargo.lock`, via `cargo "
        "metadata --offline` — no network access). License identities are "
        "additionally gated by `cargo-deny` (`deny.toml`). **Regenerate and "
        "commit this file whenever any dependency changes.** Output is "
        "deterministic, so a re-run on an unchanged graph is a byte-identical "
        "no-op."
    )
    section_text.append("")
    section_text.append(
        f"This file lists the third-party *software* the workspace depends on: "
        f"{len(entries)} registry packages ({runtime_count} reachable through "
        f"normal dependency edges, {development_only_count} test/dev-only). The "
        "first-party workspace crates are not listed here; they carry the "
        "SDK's own license (MIT OR Apache-2.0, see `Cargo.toml`)."
    )
    section_text.append("")
    section_text.append("## Map-data attribution")
    section_text.append("")
    section_text.append(
        "Separate from the software above: map imagery © MapQuest; map data © "
        "OpenStreetMap contributors, licensed under the Open Database License "
        "(ODbL). Every reference app renders this attribution in-app from the "
        "shared provider registry descriptors (`platform/providers.json`; the "
        "web playground reads the same file). "
        "Integrators must keep this attribution visible in their own apps and "
        "reproduce this notices file with their distribution."
    )
    section_text.append("")
    section_text.append("## License summary")
    section_text.append("")
    section_text.append("| SPDX license expression | Packages |")
    section_text.append("|---|---|")
    for license_expression, group_entries in groups.items():
        section_text.append(
            f"| `{license_expression}` | {len(group_entries)} |"
        )
    section_text.append(f"| **Total** | **{len(entries)}** |")
    section_text.append("")
    section_text.append("## Packages by license")
    section_text.append("")
    for license_expression, group_entries in groups.items():
        render_license_group(section_text, license_expression, group_entries)
    section_text.append("## Copyright notice lines (extracted from vendored crates)")
    section_text.append("")
    section_text.append(
        "First attribution lines extracted from the license files vendored in "
        "the local cargo cache; each crate's source carries the complete text. "
        "Crates without an extractable line (or absent from the local cache) "
        "simply appear only in the license tables above, which remain "
        "authoritative."
    )
    section_text.append("")
    for entry in entries:
        if not entry["copyright_notices"]:
            continue
        section_text.append(
            f"- `{entry['name']}` {entry['version']} — "
            + " / ".join(entry["copyright_notices"])
        )
    section_text.append("")
    section_text.append("## Prior art & inspiration")
    section_text.append("")
    section_text.append(
        "Credits for projects this SDK learns from. Where only a design was "
        "learned from, no code was copied; where an entry says logic was "
        "ported, the derived work is credited and its license text is "
        "retained under `docs/THIRD_PARTY_LICENSES/`."
    )
    section_text.append("")
    section_text.append(
        "- [MapLibre](https://maplibre.org/) — its open map renderers and how "
        "they frame tile scheduling, frustum listing and tile budgeting are "
        "the mature prior art this SDK's renderer seams knowingly "
        "re-implement from first principles."
    )
    section_text.append(
        "- [MapLibre style specification and demotiles](https://github.com/maplibre/demotiles) "
        "— the version-8 document model, typed expressions, filters and CSS "
        "colour parsing in `rustic-maps-style` are a from-scratch Rust "
        "implementation of the published MapLibre style specification; no "
        "specification text, schema or C++ source is copied. "
        "`crates/rustic-maps-style/fixtures/styles/demotiles-style.json` is a "
        "verbatim copy of <https://demotiles.maplibre.org/style.json> (BSD "
        "3-Clause, Copyright (c) 2021 MapLibre), retained as test data only. "
        "Its licence text is committed at "
        "`docs/THIRD_PARTY_LICENSES/maplibre-demotiles-LICENSE.txt`. The "
        "style's metadata carries its own attribution: \"This style was "
        "generated on MapTiler Cloud. Usage is governed by the license terms "
        "in https://github.com/maplibre/demotiles/blob/gh-pages/LICENSE\"."
    )
    section_text.append(
        "- [Noto Sans](https://github.com/notofonts/latin-greek-cyrillic) — the "
        "committed glyph ranges "
        "`fixtures/glyphs/noto_sans_regular_0-255.pbf` and "
        "`fixtures/glyphs/noto_sans_regular_256-511.pbf` are Noto Sans font "
        "material served in the Mapbox glyph protocol, "
        "retained as test data for the label pass. The Noto Sans Regular stack "
        "is licensed under the SIL Open Font License 1.1 (Copyright 2022 The "
        "Noto Project Authors), committed at "
        "`docs/THIRD_PARTY_LICENSES/noto-sans-OFL-1.1.txt`; no font file ships "
        "in the SDK packages."
    )
    section_text.append(
        "- [OpenStreetMap US Tile Service](https://tile.openstreetmap.us/) and "
        "[Natural Earth](https://www.naturalearthdata.com/) — the demotiles "
        "vector tiles are © OpenStreetMap contributors, licensed under the "
        "Open Database License (ODbL), and its country data derives from "
        "Natural Earth; that attribution accompanies the committed fixture "
        "and is reproduced in "
        "`crates/rustic-maps-style/fixtures/styles/README.md`."
    )
    section_text.append(
        "- [OpenStreetMap](https://www.openstreetmap.org/) and its ODbL "
        "database — the map-data backbone this SDK renders; the ODbL "
        "attribution obligations above are real and are honored in every "
        "reference app."
    )
    section_text.append(
        "- [UniFFI](https://github.com/mozilla/uniffi-rs) — whose UDL-first, "
        "contract-as-single-source-of-truth binding approach this SDK adopts "
        "for its Kotlin/Swift/React Native FFI surface."
    )
    section_text.append(
        "- [wgpu](https://wgpu.rs/) — the portable WebGPU-in-Rust stack that "
        "lets one GPU rendering path run natively, on mobile, and in the "
        "browser."
    )
    section_text.append(
        "- [Navara](https://github.com/maplibre/navara) — MapLibre-affiliated "
        "web 3D-globe / GIS visualization engine (Rust/WASM headless GIS "
        "core); credited as surveyed prior art for headless-engine "
        "architecture, not reused and not a reference for this SDK's "
        "navigation capabilities."
    )
    section_text.append(
        "- [Ferrostar](https://github.com/stadiamaps/ferrostar) — Stadia "
        "Maps' Rust navigation core. Its trip-state / step-advance-condition "
        "design is the reference this SDK's `rustic-maps-navigation` crate follows "
        "closely: the step-advance conditions and their recommended defaults "
        "are modeled on Ferrostar's (including its rapid-double-advance "
        "hazard, which this SDK's ports avoid), and its deviation-detection "
        "and spoken-instruction models informed this SDK's seams. Because "
        "this SDK's navigation logic is derived from Ferrostar's design, its "
        "license text is retained in "
        "`docs/THIRD_PARTY_LICENSES/ferrostar-LICENSE.txt` (BSD 3-Clause, "
        "Copyright (c) 2023, Stadia Maps, Inc.); no file in this workspace is "
        "a verbatim copy, and this SDK's own code remains MIT OR Apache-2.0."
    )
    section_text.append("")
    return "\n".join(section_text) + "\n"


def main():
    metadata = load_workspace_metadata()
    entries = collect_third_party_entries(metadata)
    document_text = render_document(entries)
    NOTICES_OUTPUT_PATH.write_text(document_text, encoding="utf-8")
    print(
        "generated: " + str(NOTICES_OUTPUT_PATH.relative_to(REPO_ROOT))
        + f" ({len(entries)} third-party packages)"
    )


if __name__ == "__main__":
    main()