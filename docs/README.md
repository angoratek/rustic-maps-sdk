# Repository documentation

This folder is the contributor record: how the SDK is built, why it is shaped
this way, and how it ships. The published guides live in `site/book-src/` and
are not indexed here.

| File | Question it answers |
|---|---|
| `Architecture.md` | What is the SDK made of, what does the contract promise, how does the `MapView` work, and how do the provider layer and offline routing fit in? |
| `Browser.md` | How does the browser target work: generation, the loop, fallbacks, tile throughput, the console and measured performance? |
| `Providers.md` | How does the provider registry work, how do credentials flow on each target, which provider serves which contract, and how are tiles and styles cached? |
| `StyleSupport.md` | Which MapLibre style properties ship, how do they evaluate, and what does the renderer draw? |
| `ReferenceAppUX.md` | How must every reference application look and behave? |
| `Release.md` | How is a release produced, signed and published, how are the docs, screenshots and demo operated, and what does opening the repository require? |
| `DocsStyleGuide.md` | What standard must published pages, screenshots and public documentation comments meet? |
| `THIRD_PARTY_NOTICES.md` | Which third-party licenses does the distribution carry? |
| `THIRD_PARTY_LICENSES/` | The full license texts the notices cite. |
| `doc-snippets.json` | Which reference-application roots the snippet generator scans and which chapters it rewrites. |
| `browser-map.png`, `playground.png`, `rendered-map.png` | The README's screenshots. |

The project record lives at the repository root: `PRD.md` holds the locked
product decisions, `PLAN.md` the milestones and open work, `CHANGELOG.md` the
dated changes, and `CLAUDE.md` the working conventions.
