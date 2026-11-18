# Screenshots

Every capture below comes from the reference application for that platform, at
phone width, on the same recorded Capitol Park ⇔ Tower Bridge trip. Each run
injects one MapQuest credential into the session through the platform's
runtime seam; the credential is never stored in the build, the page, the image
or this page. The screenshot set is six captures per platform from one
revision: the four flow moments on this page and the layers-control pair on
the [Map layers and traffic](map-layers-and-traffic.md) page.

**These images come from an earlier build.** They were recorded before the
developer console, the map motion changes and the current provider rows. They
still show the reference scenario's flow, drawn map and vocabulary, but they
do not picture the build you can run today. The layers-control pair on the map
layers page comes from the same build.

Each platform shows the same four moments of the same flow: the plan sheet
with both endpoints chosen, the provider section with every row's display name
and credential state, the simulated trip at its first instruction with 1×
playback speed, and the styled vector base with its place labels.

## The plan sheet

The labelled Origin and Destination fields with their A/B markers, each
field's own Search action, a My location chip, and the swap action. The route
card states distance, duration, mode and strategy. The readiness chip and the
applied-key line show that the session's provider credential is serving.

### Android — Kotlin and Jetpack Compose

![The Android plan sheet: A Origin holding Capitol Park and B Destination holding Tower Bridge, each with a Search action and a My location chip, the Swap origin and destination action, Done, the Directions controls, the Plan route action and the MapQuest key applied line, over the Bright vector base](img/android-plan-sheet.png)

### iOS — Swift and SwiftUI

![The iOS plan sheet: the same A and B endpoint fields with their Search actions and My location chips, the swap action, Done, the assignment hint, the Directions controls and Plan route, under the Ready chip over the Bright vector base](img/ios-plan-sheet.png)

### React Native — one TypeScript surface, two native shells

![The React Native plan sheet: the A and B endpoint fields holding Capitol Park and Tower Bridge, each with its Search action and the live type-ahead's result rows below it, the My location chips, the swap action and Done, over the Bright vector base](img/react-native-plan-sheet.png)

### WebAssembly — the browser build

![The browser plan sheet: the A and B endpoint fields with Search actions, My location chips, the swap action, the applied-key line, the demo-place list and the route card, over the Bright vector base](img/wasm-plan-sheet.png)

## Providers and credentials

Each capability row names the serving provider and its credential state in
one line, such as `MapQuest · Ready` or `OSRM · Ready`. A keyless routing
provider states its public demo server and the terms that server carries. The
applied provider's own row states that its key is applied.

### Android

![The Android settings sheet at Capabilities: Geocoding, Routing, Distance Matrix and Place Search each reading MapQuest · Ready with MapQuest's terms note, scrolled below the applied key's attribution, over the Bright vector base](img/android-provider-section.png)

### iOS

![The iOS settings sheet: the Routing, Distance matrix, Place search, Traffic incidents and Traffic flow capability rows each reading MapQuest · Ready with MapQuest's terms note, over the Bright vector base](img/ios-provider-section.png)

### React Native

![The React Native settings sheet: Geocoding, Routing, Distance matrix and Place search each reading MapQuest · Ready on their serving line and picker, over the Bright vector base](img/react-native-provider-section.png)

### WebAssembly

![The browser settings dialog: the MapQuest API key row marked applied, the live connection check, and Providers by capability with Geocoding, Routing, Distance matrix, Place search, Traffic incidents, Traffic flow and Raster tiles all reading MapQuest · Ready](img/wasm-provider-section.png)

## Simulated navigation

The reference scenario at its first instruction: the turn-by-turn banner with
the next maneuver and the then line, the planned route polyline with its
endpoints, the ETA-distance line, and the playback-speed controls with **1×**
selected, beside voice guidance, pause and exit.

### Android

![The Android simulated trip at its first instruction: the 60 m turn banner with the then line, the route polyline, the compass, the scale bar, the ETA-distance line, the speed chips with 1× selected, voice guidance, Pause and Exit, over the Bright vector base](img/android-navigation.png)

### iOS

![The iOS simulated trip at its first instruction: the turn banner and then line, the route polyline, the compass, the scale bar, the ETA-distance line, the speed chips with 1× selected, voice guidance, Pause and Exit, over the Bright vector base](img/ios-navigation.png)

### React Native

![The React Native simulated trip at its first instruction: the turn banner and then line, the route polyline, the compass, the scale bar, the ETA-distance line, the speed chips with 1× selected, voice guidance, Pause and Exit, over the Bright vector base](img/react-native-navigation.png)

### WebAssembly

![The browser simulated trip at its first instruction: the 100 m turn banner and then line, the route polyline, the compass, the scale bar, and the trip drawer with the ETA-distance line and the speed chips with 1× selected, over the Bright vector base](img/wasm-navigation.png)

## Vector base

The OpenFreeMap Bright styled vector base with the embedded glyph atlas's
street and place labels. Streets are named along their own lines, rotated to
the road's bearing and skipped when the text would not fit; place names sit
above them, and a collision pass keeps one from covering another. They arrive
with the vector base; a raster base carries its imagery's own labels instead.
Every surface draws them, and the labels are renderer output rather than page
markup.

### Android

![The Android map over the Bright vector base: streets named along their own lines and place names above them, Downtown, Midtown, Old Sacramento, Alkali Flat, Mansion Flats and Richmond Grove, with the peek plan sheet below](img/android-vector-base.png)

### iOS

![The iOS map over the Bright vector base: streets named along their own lines and place names above them, Sacramento, Downtown, Old Sacramento, Alkali Flat, Mansion Flats and Richmond Grove, with the peek plan sheet below](img/ios-vector-base.png)

### React Native

![The React Native map over the Bright vector base: streets named along their own lines and place names above them, Sacramento, Downtown, Old Sacramento, Alkali Flat, Mansion Flats and Richmond Grove, with the peek plan sheet below](img/react-native-vector-base.png)

### WebAssembly

![The browser map over the Bright vector base: streets named along their own lines and place names above them, Sacramento, Downtown, Alkali Flat, Mansion Flats and Richmond Grove, with the map controls and scale bar](img/wasm-vector-base.png)

## Reading the screenshots

The six captures per platform share one design language: the four moments on
this page and the layers-control pair on the map layers page show the same
flow. They also share one vocabulary: *plan sheet*,
*Origin* and *Destination*, *Search*, *My location*, *swap*, *route card*,
*provider section*, *maneuver banner*, *then line*, *route polyline*,
*map control cluster*, *compass*, *scale bar*, *ETA-distance line*,
*speed chips*, the browser's *trip drawer*, *vector base*, and *place labels*.
Differences are the platform's own chrome: Compose's Material surface on
Android, SwiftUI's safe-area handling on iOS, the React Native host view, and
the browser's WebGPU canvas. The map content, the guidance, the control
cluster, the scale ladder and the drawn labels are the same.
