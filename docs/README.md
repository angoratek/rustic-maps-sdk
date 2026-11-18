# Repository documentation

This folder holds the engineering documents behind the SDK: the frozen API
shape, the provider and credential rules, how each subsystem works, the current
designs and dated records of past investigations. Published documentation lives
in `site/book-src/` and is not indexed here. Everything in `history/` is a
frozen record from the date in its title; it describes the tree as it was then,
not today. Superseded documents were removed from the tree and remain in git
history.

## Root

| File | Question it answers |
|---|---|
| `PublicApiV2.md` | What public API shape did the SDK adopt, and what does each vendor call the same concept? |
| `ReferenceAppUX.md` | How must every reference app look and behave? |
| `PackagingAndRelease.md` | How is a release produced, signed and published? |
| `ProviderCredentialsAndBrowserProviders.md` | How do provider credentials work on native and in the browser, and why? |
| `ProviderGenericity.md` | How is provider genericity enforced at every layer? |
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
| `reference/BrowserLoop.md` | How do I build the wasm `pkg`, serve it, test it and use the browser harness? |
| `reference/BrowserRenderPerformance.md` | How is the browser frame measured, and what are the current numbers? |
| `reference/BrowserRasterFallback.md` | When may a page's live raster source serve tiles, and how does a configured provider stay authoritative? |
| `reference/TrafficLayerCaching.md` | How are traffic-layer answers cached, bounded and released? |
| `reference/OfflineGraphCompilation.md` | How is the RMOG offline routing graph compiled from OpenStreetMap? |
| `reference/MobileDemoOperations.md` | How is the hosted Appetize mobile demo operated? |
| `reference/DocumentationSnippets.md` | How are tutorial code blocks generated from compiled source and checked? |

## design — current designs

| File | Question it answers |
|---|---|
| `design/Autocomplete.md` | What rule do the search fields follow for suggestions while typing, and which provider answers? |
| `design/WebFacadeGeneration.md` | Can the browser API be generated from the frozen contract, and what must the one-module shape fix before the page adopts it? |
| `design/BindingSurfaceConsolidation.md` | Where do the Android, iOS, React Native and WebAssembly surfaces hand-maintain what the UDL generation or the Rust core could own, and which consolidations pay first? |
| `design/DirectionsModesAndStrategy.md` | How do applications choose transport mode and route strategy, and what may a provider honour? |
| `design/DeveloperConsole.md` | What does the in-app developer and provider console show, and which half is still open? |
| `design/EverymapAdoption.md` | How does the provider layer sit on `angoratek/everymap-rs`, and how do the remaining duplicate abstractions go? |
| `design/LocationInterpolation.md` | How does the location puck move between fixes, so it glides instead of hopping, on every platform? |
| `design/MapControlsAndTrafficLayer.md` | How does the on-map layer, traffic and device-location control work, per target? |
| `design/PlanFlowAudit.md` | How does the React Native plan flow compare with the iOS reference pattern, and what evidence backs each target? |
| `design/RouteShields.md` | How does a road or transit route reference become the shaped badge a map reads at a glance? |
| `design/PositioningAndRouteUx.md` | How do positioning feedback and the plan flow align across all four targets? |
| `design/ProviderBackedTileSources.md` | How do provider raster and vector tile sources replace the URL-template paths, and what does each provider serve? |
| `design/ProviderConsole.md` | Merged into `DeveloperConsole.md`. |
| `design/ProviderCoverage.md` | Which provider serves which capability on which target, how is one selected per capability, and what closes each remaining gap? |
| `design/ProviderLayerConsolidation.md` | Merged into `EverymapAdoption.md`. |
| `design/ProviderSelectionPerCapability.md` | Merged into `ProviderCoverage.md`. |
| `design/ProviderVectorSourcesAndCaches.md` | How do every vector-capable provider and its style family share one registration path, and how does a cache keep one provider's tiles out of another's? |
| `design/ResponsiveTilesAndZoom.md` | Why do tiles fill one at a time, and how do zoom changes cross-fade instead of swapping the tile set? |
| `design/ReferenceApplicationBehavior.md` | What must every reference application do, step by step, and how is that one behavior model enforced across Android, iOS, React Native and the web demo? |
| `design/ReferenceAppSettingsAndPersistence.md` | What do the reference apps remember across relaunches, and how is the settings surface sized? |
| `design/TilePackManager.md` | How do offline raster regions download, resume, report progress, persist and evict on every target? |
| `design/VectorStyleSpecification.md` | How does the renderer adopt the MapLibre source and layer model, and how are the committed road hierarchy, labels and vector pipeline tuned? |

## history — dated records

| File | Question it answers |
|---|---|
| `history/FourPlatformParityAudit.md` | What was the four-target capability and UX parity state on 2026-10-31? |
| `history/PositioningResearch.md` | What did the device-positioning platforms offer, and what split was chosen? |
| `history/research/browser-render-performance.md` | What was the 2026-10-30 browser performance evidence and plan? |
