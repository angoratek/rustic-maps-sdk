# Roadmap

This roadmap is directional. It states the outcomes the maintainers are working
toward, not a schedule or a promise about a specific release. Issues and
Discussions are the source of priorities; a theme moves forward when someone
opens a focused issue and a maintainer accepts it.

## Rendering and map quality

- One GPU renderer that draws raster and vector bases on the same surface and
  keeps a stable frame budget while the camera moves.
- Styled vector bases with placed labels, route shields, and readable
  transitions across zoom changes.
- A location indicator and navigation presentation that look the same on every
  platform.

## Provider breadth and fidelity

- A vendor-neutral provider layer where a new provider is a provider crate, a
  registry descriptor, and a registration arm.
- Fidelity to each vendor's style, attribution, and terms on every target.
- Honest fallback behavior when a provider is absent or a key is rejected, with
  the applied provider visible to the developer.

## Platform parity and developer experience

- The same `MapView` tasks with the same names, states, and errors on Android,
  iOS, React Native, and WebAssembly.
- Reference applications that demonstrate one consistent flow across platforms.
- A developer console and diagnostics surface that shows which provider served
  a request, what came back, and why a request failed.

## Offline and performance

- Tile packs and an offline routing graph compiled from OpenStreetMap that a
  trip can use with no network.
- Responsive tile throughput and smooth zoom under load on mobile and in the
  browser.
- Measured frame budgets that guide the renderer work instead of guesses.

## Documentation and releases

- Guides that take a developer from installation to a working map, search,
  routing, and navigation on every target.
- Generated API references that match the shipped contract.
- Signed releases with checksums, third-party notices, and a documented
  install path for each platform.

## How priorities are set

Priorities come from reported defects, contributor proposals, and the
maintainers' review of the themes above. To propose work, open an issue that
describes the problem and the outcome you want. Governance and the decision
model are in [`GOVERNANCE.md`](GOVERNANCE.md).
