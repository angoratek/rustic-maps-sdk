# Fixture provenance

Every committed test payload has a recorded provenance: its source, the date
it was recorded or lifted, and the SHA-256 of its bytes. The machine-checked
record is [`provenance.json`](provenance.json); the fixture directories'
READMEs carry the human-readable account.

`platform/web/tests/fixture_provenance.mjs` reads the manifest and fails when:

- a recorded file's SHA-256 (or byte count) differs;
- a payload exists under a scanned fixture root without an entry;
- an entry names a file that no longer exists; or
- the provider layer's pinned version changes. The gate reads the
  `everymap-*` requirement from `Cargo.toml` and the resolved version from
  `Cargo.lock`, and compares both with `providerLayer` in the manifest. A
  dependency bump fails until the provenance is reviewed and re-recorded.

Run it directly (no server or browser involved):

```sh
node --test platform/web/tests/fixture_provenance.mjs
```

## Manifest schema

```json
{
  "schema": "fixture-provenance/v1",
  "providerLayer": {
    "crate": "everymap-core",
    "requirement": "0.4",
    "resolvedVersion": "0.4.7",
    "reviewedAt": "2026-11-19"
  },
  "groups": [
    {
      "directory": "fixtures/example",
      "source": { "kind": "url", "url": "https://example.test/tile.pbf" },
      "recorded": "2026-11-17",
      "files": [
        {
          "name": "tile.pbf",
          "bytes": 1234,
          "sha256": "…64 lowercase hexadecimal characters…"
        }
      ]
    }
  ]
}
```

A group's `source` and `recorded` are the defaults; a file may override either
when the group's files were not captured together. The source kinds are:

| Kind | Required fields | Meaning |
| --- | --- | --- |
| `url` | `url` | Fetched from an http or https address. |
| `provider-crate` | `crate`, `version`, `test` | Lifted from an `everymap-providers-*` crate, naming the version and the test or fixture it came from. |
| `recorded` | `recorder`, `note` | Recorded live by an in-repository recorder program. |
| `generated` | `generator` | Produced deterministically in this repository. |
| `authored` | `note` | Hand-authored test data with no upstream capture. |
| `copied` | `from` | A byte copy of another committed fixture. |
| `unrecorded` | `note` | The origin is honestly unknown; the note says what is known. |

## Provider payloads

The credentialed vendors' JSON payloads under `fixtures/<vendor>/` were
hand-exported from the inline payloads of the provider layer's own contract
tests (or copied verbatim from an `everymap-providers-*` crate's committed
fixture). They are not live recordings. Each file's manifest entry names the
provider crate, its pinned version and the test it came from; the crate
fixture and inline-payload tests are the authority for the shape. A bump of
the `everymap-*` pin therefore fails the gate until every entry is reviewed.

Vendor style documents and the HERE and Mapbox vector tiles are synthetic
documents and tiles authored in this repository, because both vendors'
terms forbid redistributing their own style assets.

## Credentials

No fixture, README or manifest carries a provider credential. The recorded
raster and vector tiles are payload bytes; the recorder examples read the
credential from the environment at capture time and never write it into a
file.
