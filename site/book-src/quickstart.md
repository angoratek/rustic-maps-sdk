# Developer quickstart

Build a map, resolve places, plan a route, and follow a guided trip. The same
calls exist on Android (Kotlin), iOS (Swift), React Native (TypeScript), and
WebAssembly (TypeScript) because all four bind one UniFFI contract. This page
gives the order to do it in and the concepts the calls use. Each step links to
the guide that shows it in full.

## What you need

- The SDK artifact for your platform, built or installed by [Your first
  map](first-map.md).
- A provider credential for search, routing, and traffic. A raster or vector
  base map from a URL-template source needs no credential.
- The minimum platform version for your target: Android API 24, iOS 17, React
  Native 0.88, or a browser with WebGPU. [Requirements and
  distributions](requirements.md) lists the exact toolchains and artifact
  sizes.

## What the SDK gives you

| Call | What it gives you |
|---|---|
| `MapView` | The map surface. The shipped widget owns the platform surface, frame pump, gestures, and lifecycle. On WebAssembly you supply a canvas and render a frame on request. |
| `setCamera` | Center, zoom, bearing, and pitch. Zoom is continuous; pitch tops out at 75°. |
| `setOverlays` | Route polylines, filled rings, point markers, A/B pins, and traffic flow, all in geographic coordinates. |
| `forwardGeocode`, `suggest`, `search` | Provider geocoding and place search. Results carry a coordinate. |
| `calculateRoute` | A route with legs, steps, shapes, and instructions. Flatten the step shapes and you have the polyline. |
| `startNavigation` and location updates | A reducer that advances steps, grades deviation, and reports progress. |
| `setVectorTileSource` | A styled vector base map from any `{z}/{x}/{y}` vector source. |
| `setProviderCredentialWithTileCache`, `offlineRouteFromArchiveBytes` | A disk tile cache with per-category time-to-live, and routes over an offline graph compiled from OpenStreetMap. |

## Set up in order

1. **Install the artifact.** Build the package for your platform or download a
   release, then add it to your project. [Your first
   map](first-map.md) has the exact steps for Android, iOS, React Native, and
   WebAssembly.
2. **Create the SDK and register a provider.** `MapSdk` starts with no
   providers. Apply a credential with `setProviderCredential`, or register a
   URL-template base map. Calls into an unregistered contract throw
   `ProviderNotConfigured`.
3. **Open the map and set the camera.** Give the map view an SDK instance and a
   camera pose. The shared demo camera is Capitol Park, Sacramento at zoom 12,
   north-up and top-down.
4. **Add what you want to show.** Overlays are geographic coordinates plus
   style. A calculated route's step shapes flattened into one list make a route
   polyline.
5. **Plan and follow.** Call `calculateRoute`, then deliver location updates to
   the `Navigator`. The reducer reports the active step, the distance
   remaining, and when a fix is off route.

## Positions and the world square

A position is a `GeographicCoordinate`: latitude and longitude in decimal
degrees on the WGS 84 ellipsoid. A region is a `BoundingBox` between a
south-west and a north-east corner and never spans the antimeridian. Every
coordinate is projected into the Web Mercator world square, where each position
becomes two numbers in `[0, 1]`; the projection stops at about 85.05° north and
south.

## Tiles and zoom

A tile is one payload at one zoom level, addressed by a `TileKey`: a column, a
row counted from the north, and the zoom level. At zoom 0 the world is one tile;
each level doubles the columns and rows. A camera's zoom is continuous from 0
through 24 and each unit doubles the on-screen scale. `tileZoomLevel` gives the
nearest whole level for a tile fetch, and `visibleTileKeys` lists every tile at
a chosen level that intersects the visible ground.

## Cameras

A `MapCamera` is an immutable pose of center, zoom, bearing, and pitch. A new
pose is a new camera. Pitch measures tilt from straight down, from 0° to a
maximum of 75°, and bearing wraps into `[0, 360)`. The camera is also the
projection and tile selector: `projectToScreen` maps a coordinate to viewport
pixels, `screenToGroundCoordinate` maps a pixel back to the ground, and
`groundMetersPerScreenPixel` sizes a scale bar. The `fitBounds` constructor
frames a bounding box inside an inset region you pass.

## Styles and overlays

A raster tile is a pre-rendered image. A vector tile carries geometry and style
separately and is painted on the device. Vector base maps use named styles:
Bright, Fiord, and Positron for OpenMapTiles-schema sources, and Carto Light and
Carto Dark for the Sourdough source. Place names and road names are drawn from a
glyph atlas. A style that declares a shield block on its route-reference layer
draws each route reference as a shaped badge; Bright, Fiord, and Positron carry
no such layer, so those bases draw no shields.

An overlay is geography plus a style, not a picture or a set of screen pixels.
`RoutePolyline`, `FilledPolygon`, `PointMarkers`, `PointPins`, and `TrafficFlow`
all carry coordinates, and the SDK projects them through the same camera as the
tiles in every frame.

## Providers and credentials

Provider-backed capabilities reach you through a registry on `MapSdk`. Register
an implementation, then call the contract methods: geocoding, routing, the
distance matrix, place search, traffic incidents and flow, and raster tiles. The
provider layer carries MapQuest, HERE, Google Maps, TomTom, Mapbox and Radar for
keyed geocoding and routing, Photon for keyless search, OSRM and Valhalla for
keyless routing, and URL-template providers for any raster or vector base map.

Provider keys are runtime inputs, not build inputs. Enter a key through your own
settings surface, secure storage, or a launch value injected for one test
session. There is no `local.properties`, `Info.plist`, or `.xcconfig` field for a
key, and no release artifact carries one. A key you apply is held for the
session and optionally remembered in the platform's secure store.

## Where to go next

| You want | Read |
|---|---|
| A rendered map in an app | [Your first map](first-map.md) |
| Search results and a planned route | [Search and routing](search-and-routing.md) |
| A moving trip with turn-by-turn guidance | [Turn-by-turn navigation](navigation-simulation.md) |
| Routing with no network | [Offline maps](offline-maps.md) |
| Base maps, traffic and device location from the map | [Map layers and traffic](map-layers-and-traffic.md) |
| A different provider for each capability | [Mix providers per capability](mix-providers-per-capability.md) |
| The model behind these calls | [Map concepts](map-concepts.md) |
| The exact surface, member by member | [The FFI contract](udl-contract.md) |
| Frame costs and how to measure them | [Performance and profiling](performance.md) |
