# Reference-app settings, persistence and sheet sizing

Date: 2026-11-01. Status: landed (round 125) on all four targets. The owner
reported that the web
demo forgets its settings across reloads, that choices should persist on every
reference app (including the last search inputs), that the Android settings
surface is too tall for the screen, and that the web demo should link to the
mobile demos. This document records the shared record and what shipped per
target.

## What the apps persist today

| Preference | Web | Android | iOS | React Native |
|---|---|---|---|---|
| Theme (Light/Dark/Auto) | versioned `localStorage` record | `RememberedChoicesStore` over SharedPreferences | `RememberedChoicesStore` over UserDefaults | `loadPreferences`/`storePreferences` on the curated module |
| Provider credential | localStorage, only when "remember" is chosen | `ProviderCredentialStore` (opt-in) | keychain/UserDefaults (opt-in) | curated SDK storage |
| Base map (raster source or vector style) | remembered | remembered | remembered | remembered |
| Demo plan / scene selection | remembered | remembered | remembered | remembered |
| Camera preset | remembered | remembered | remembered | remembered |
| Speed multiplier | remembered | remembered | remembered | remembered |
| Offline card selections | not remembered | not remembered | not remembered | not remembered |
| Last origin / destination text and search query | remembered | remembered | remembered | remembered |
| Reset row | clears the record, leaves the credential store | same | same | `clearPreferences` |

All four use the same versioned JSON shape; the reset row never touches the
credential store.

## The shared persistence contract

One small, versioned preferences record per app, written on every explicit user
choice and read at startup after the registry is ready:

- **Remembered choices**: theme, base map (provider key plus raster source or
  style id), selected demo plan and scene, camera preset, speed multiplier, the
  last origin and destination texts, the last search query text, the traffic
  layer's on/off state, and whether the device location source is on (its first
  fix re-centers the map on the next launch when the permission still holds).
- **Never remembered**: anything transient (current camera pose, open dialog,
  current route result), the provider credential unless the user turned on the
  existing "remember on this device" switch, and any cache-like state.
- **Shape**: a single JSON object namespaced by app version, for example
  `{"version":1,"theme":"auto","baseMap":{"providerKey":"openfreemap","styleId":"bright-light"},"plans":{...},"search":{"origin":"Capitol Park","destination":"Tower Bridge"}}`. Unknown keys are ignored and a version bump drops incompatible records instead of failing.
- **Reset**: a "Reset remembered choices" row clears the record; the credential
  store stays a separate, explicitly opt-in store.
- **Platform stores**: web `localStorage` (wrapped in try/catch so private
  windows keep working); Android `SharedPreferences` beside the existing stores;
  iOS `UserDefaults`; React Native through a small native preference call on the
  existing curated module (AsyncStorage is not currently a dependency).
- **Startup order**: preferences apply before the first frame or plan: the theme
  and base map restore silently; a remembered plan pair does not auto-plan a
  route (that would spend a provider call), it fills the fields so the user can
  press Plan route — the expedite the owner asked for.

## Sheet sizing and scrolling

- One rule for every platform: the settings surface is a scrollable content
  column inside a container capped at the panel maximum height fraction (0.72
  at phone width, the existing `phonePanelMaximumHeightFraction` token), with
  its title and Close action pinned. Content that overflows scrolls; the page
  never grows past the viewport and nothing is cut off under the system bars.
- Web: the dialog caps at the panel maximum with an internal scroll and a
  pinned header.
- Android: `heightIn(max = viewport × 0.72)` with a weighted scroll column and
  a pinned Close action.
- iOS: sheet detents at 0.72 and 0.8, every section reachable, the Close/Done
  action visible.
- React Native: the panel caps at `sheetFullHeight` with `settings-scroll`.
- Sizing cases assert the panel stays inside the viewport, the Close action is
  hittable and the last row is reachable (browser at 360×640; Pixel_9 and
  iPhone 17 Pro on device).

## Navigation between demos

The browser demo's header links to the documentation and to the hosted mobile
demo (`/demo/mobile/` on the published site, `../mobile-demo/` locally), so a
visitor can move between the browser demo and the device builds without
hunting through the book.

## Tests

- Web: a Playwright case changes theme, base map, demo plan and both search
  fields, reloads, and asserts each value is restored; a second case asserts the
  reset row clears them and that an un-remembered key is still not persisted.
- Android/iOS: one UI case each that changes the theme and the base map,
  relaunches the app and asserts the restored state; the search fields restore
  with the remembered pair.
- React Native: a jest case hydrating the preferences and one asserting the
  native write on change.
- Sizing: a small-viewport case per platform (web viewport and each native
  device) asserts the settings surface stays inside the viewport and its last
  section is reachable.
