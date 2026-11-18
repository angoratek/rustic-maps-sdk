# Repository documentation

This folder holds the engineering documents behind the SDK: the frozen API
shape, the provider and credential rules, how each subsystem works and the
current designs. Published documentation lives in `site/book-src/` and is not
indexed here. Superseded documents were removed from the tree and remain in git
history.

## Root

| File | Question it answers |
|---|---|
| `PublicApiV2.md` | What public API shape did the SDK adopt, and what does each vendor call the same concept? |
| `ReferenceAppUX.md` | How must every reference app look and behave? |
| `PackagingAndRelease.md` | How is a release produced, signed and published? |
| `ProviderCredentialsAndBrowserProviders.md` | How do provider credentials work on native and in the browser, and why? |
| `DocsStyleGuide.md` | What standard must published pages, screenshots and public documentation comments meet? |
| `ScreenshotCapture.md` | How is the published screenshot set captured on each platform, and which revision produced it? |
| `OpenSourceReadiness.md` | What must be true before the project is opened, and in what order does that work land? |
| `THIRD_PARTY_NOTICES.md` | Which third-party licenses does the distribution carry? |
| `THIRD_PARTY_LICENSES/` | The full license texts the notices cite. |
| `doc-snippets.json` | Which reference-application roots the snippet generator scans and which chapters it rewrites. |
| `browser-map.png`, `playground.png`, `rendered-map.png` | The README's screenshots. |

## reference — how subsystems work today

| File | Question it answers |
|---|---|
| `reference/BrowserLoop.md` | How do I build the generated wasm package, serve it, test it and use the browser harness? |
| `reference/BrowserRenderPerformance.md` | How is the browser frame measured, and what are the current numbers? |
| `reference/BrowserRasterFallback.md` | When may a page's live raster source serve tiles, and how does a configured provider stay authoritative? |
| `reference/TrafficLayerCaching.md` | How are traffic-layer answers cached, bounded and released? |
| `reference/OfflineGraphCompilation.md` | How is the RMOG offline routing graph compiled from OpenStreetMap? |
| `reference/MobileDemoOperations.md` | How is the hosted Appetize mobile demo operated? |
| `reference/DocumentationSnippets.md` | How are tutorial code blocks generated from compiled source and checked? |

## design — current designs

| File | Question it answers |
|---|---|
| `design/WebFacadeGeneration.md` | Can the browser API be generated from the frozen contract, and what must the one-module shape fix before the page adopts it? |
| `design/WebFacadeRetirement.md` | What of the hand-written browser façade can retire now that the page runs on the generated surface, and what must stay canvas-only? |
| `design/DeveloperConsole.md` | What does the in-app developer and provider console show, and which half is still open? |
| `design/EverymapAdoption.md` | How does the provider layer sit on `angoratek/everymap-rs`, and how do the remaining duplicate abstractions go? |
| `design/ProviderBackedTileSources.md` | How do provider raster and vector tile sources replace the URL-template paths, and what does each provider serve? |
| `design/ProviderCoverage.md` | Which provider serves which capability on which target, how is one selected per capability, and what closes each remaining gap? |
| `design/ProviderVectorSourcesAndCaches.md` | How do every vector-capable provider and its style family share one registration path, and how does a cache keep one provider's tiles out of another's? |
| `design/ResponsiveTilesAndZoom.md` | Why do tiles fill one at a time, and how do zoom changes cross-fade instead of swapping the tile set? |
| `design/VectorStyleSpecification.md` | How does the renderer adopt the MapLibre source and layer model, and how are the committed road hierarchy, labels and vector pipeline tuned? |
