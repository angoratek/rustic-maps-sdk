# Generated documentation snippets

Two tutorial chapters carry snippet markers and their code blocks are
**generated from compiled source** in the reference applications, not
hand-written: `site/book-src/first-map.md` and
`site/book-src/search-and-routing.md`. Every other chapter's code block is
hand-authored and still compile-checked. Two harnesses keep that true:

- `scripts/check-doc-snippets.mjs` (M18) compiles every block against the
  generated platform surfaces, so a block cannot name an API that does not
  exist.
- `scripts/generate-doc-snippets.mjs` (this document, M25) regenerates every
  block from marked regions of the reference applications, so a block cannot
  drift from the code that actually ships.

## The worked example

Both marked chapters walk **one** example: create the SDK, show a map, search two
places, plan a route between them, draw the route, start turn-by-turn
navigation, and make an offline region available. Each platform tab shows the
same steps in the same order with the same identifiers, so a reader can move
between tabs without relearning the program.

The identifiers are fixed across Android (Kotlin), iOS (Swift), React Native
(TypeScript) and WebAssembly (TypeScript):

| Identifier | What it holds |
|---|---|
| `mapSdk` | the SDK object, created once per process |
| `apiKey` | the map-platform credential, read at runtime |
| `mapView` | the platform map view that owns the surface and the frame pump |
| `mapCamera` | the camera pose the map opens on |
| `searchQuery` | the text a user typed into the search field |
| `originPlace` | the place the search resolved for the origin |
| `destinationPlace` | the place the search resolved for the destination |
| `plannedRoute` | the route the SDK's routing provider returned |
| `routeOverlay` | the overlay the route draws on the map |
| `navigationController` | the turn-by-turn session over `plannedRoute` |
| `listener` | the listener the session reports progress to |

## Marker format in source

A marked region in a reference-application source file begins and ends with a
comment carrying the snippet identifier:

```kotlin
// docs-snippet: begin search-and-routing/plan-route
val plannedRoute = mapSdk.planRoute(originPlace, destinationPlace)
// docs-snippet: end search-and-routing/plan-route
```

The identifier is `<chapter>/<step>`. Markers are ordinary comments in the
platform's syntax (`//` for Kotlin, Swift, TypeScript and JavaScript). A marked
region is a contiguous run of complete statements at one indentation depth; the
generator strips the region's common leading whitespace, so the block reads at
column zero in the chapter and compiles inside the chapter's ambient context.

Regions may be nested only if the outer region is itself a snippet; prefer
separate regions.

## Marker format in a chapter

A block in a chapter names its identifier **and** its platform:

```markdown
<!-- docs-snippet: begin search-and-routing/plan-route android -->
```kotlin
val plannedRoute = mapSdk.planRoute(originPlace, destinationPlace)
```
<!-- docs-snippet: end search-and-routing/plan-route android -->
```

The generator rewrites everything between the two comments with the region it
found, fenced with the language that matches the source file. Prose around a
block is authored and reviewed; only the fenced content is generated.

## The generator

```sh
node scripts/generate-doc-snippets.mjs           # rewrite chapters in place
node scripts/generate-doc-snippets.mjs --check   # fail on any drift
```

`--check` exits non-zero and names every drifted, missing or orphaned region.
It runs in `docs-snippets.yml` next to the compile check, so a reference
application that changes a marked region without regenerating the chapters
fails CI, and a chapter that keeps a block no source produces fails too.

Sources are discovered from `docs/doc-snippets.json`, which names the platform
roots to scan; no chapter hard-codes a file path.

## What belongs in a marked region

A region is the code a reader would type, and it must be code the application
really runs:

- Prefer SDK-facing statements — calls on `mapSdk`, `mapView` and the records
  they exchange — over view plumbing.
- Keep a region to one teachable step. A step that needs a platform container
  (a Compose `AndroidView`, a SwiftUI `UIViewRepresentable`) marks only the
  closure the container calls into, not the container.
- Never mark a region whose only purpose is the tutorial. If the reference
  application does not need the statement, the guidance does not belong in a
  generated block.
