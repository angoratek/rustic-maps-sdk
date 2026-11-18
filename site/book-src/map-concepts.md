# Map concepts

This page explains the ideas the SDK's calls are built on: positions, the world
square, tiles, zoom, the camera, styles, and overlays. The guides use these ideas
without defining them. Read this page once, then continue with [Your first
map](first-map.md).

## Positions and regions

A position is a `GeographicCoordinate`: latitude and longitude in decimal
degrees on the WGS 84 ellipsoid. Latitude runs from -90 to 90; longitude runs
from -180 to 180.

A region is a `BoundingBox` between a south-west and a north-east corner, each
one a coordinate. A box never spans the antimeridian. If a region crosses it,
encode the region as two adjacent boxes.

The examples use Kotlin field spelling. Each binding uses its platform's spelling
for the same records.

```kotlin
import uniffi.rustic_maps.BoundingBox
import uniffi.rustic_maps.GeographicCoordinate

val capitolPark = GeographicCoordinate(38.5766, -121.4944)
val downtownSacramento = BoundingBox(
    southWest = GeographicCoordinate(38.5700, -121.5030),
    northEast = GeographicCoordinate(38.5850, -121.4850),
)
```

## The world square

The SDK does not draw in latitude and longitude. Before anything is placed on
screen, a coordinate is projected onto the Web Mercator world: a square where
every position becomes two numbers in `[0, 1]`. The horizontal number is zero on
the antimeridian and grows eastward. The vertical number is zero at the north
pole and grows southward.

Only part of the ellipsoid fits in that square. The projection stops at about
85.05° north and south, and latitudes beyond the cutoff clamp to the edge. The
top and bottom of the world square are therefore not the poles, and a coordinate
near a pole sits on the boundary rather than beyond it.

Mercator also changes scale across the map: it stretches the ground farther from
the equator. At a given zoom, the ground distance covered by one screen pixel
shrinks with the cosine of the center latitude. The camera answers that distance
for a scale bar through `groundMetersPerScreenPixel`.

## Tiles

A tile is one payload at one zoom level. Its address is a `TileKey`: a column, a
row counted from the north, and the zoom level. This is the web-mercator XYZ
scheme. At zoom 0 the whole world is one tile, and each level doubles the columns
and rows, so zoom level `z` has `2^z` columns and `2^z` rows. A coordinate's
address is the tile cell that contains its position in the world square.

A raster tile is a pre-rendered image. The contract carries one as
`RasterTileData`: the tile address, an image format (`Png` or `Jpeg`), a tile
resolution, and the encoded bytes. `Standard` is the plain form; `Retina` is the
`@2x` form with twice the texels per edge for the same logical tile span, which
is what a high-density screen wants.

A vector tile carries geometry and style rather than pixels. Its vertices are
stored in the tile's own space, with x to the right and y down inside the tile.
The SDK converts each vertex through the tile's web-mercator address into a
`GeographicCoordinate`, so vector geometry and your own overlays share one
coordinate space. The renderer paints a vector base map with a named style.
Bright, Fiord, and Positron style OpenMapTiles-schema sources; Carto Light and
Carto Dark style the Sourdough source.

Either kind of tile is addressed, fetched, and reused by the same key. When the
view moves, the SDK asks for the tiles the view now covers and keeps the ones it
already holds.

## Zoom is continuous

A camera's zoom is a number, not a whole tile level, and each unit doubles the
on-screen scale. The camera accepts zoom 0 through 24.

The tile grid changes only at whole levels, so a smooth pinch gesture passes
through fractional zoom while the SDK picks the nearest whole level for a tile
fetch: `tileZoomLevel`. To ask which tiles the view covers, use
`visibleTileKeys`, which lists every tile at a chosen level whose area intersects
the visible ground, sorted row-major from the north. A rotated or tilted view can
cover tiles you would not guess from the center alone, so read the list instead
of computing a rectangle.

## The camera

A `MapCamera` is an anchored pose made of four values.

| Value | What it means |
|---|---|
| `center` | The ground point rendered at the viewport center |
| `zoom_level` | Scale; continuous, and each unit doubles the on-screen scale |
| `bearing_degrees` | The compass direction the top of the viewport points to, clockwise from north, wrapped into `[0, 360)` |
| `pitch_degrees` | Tilt from straight down; zero is top-down, and the maximum is 75 degrees so the view keeps ground in it |

The camera carries no setters. A new view is a new camera; the getters read the
pose back as it was built.

```kotlin
import uniffi.rustic_maps.GeographicCoordinate
import uniffi.rustic_maps.MapCamera

val camera = MapCamera(
    center = GeographicCoordinate(38.5766, -121.4944),
    zoomLevel = 12.0,
    bearingDegrees = 0.0,
    pitchDegrees = 0.0,
)
```

A camera is also the projection and the tile selector, not only a description.
`projectToScreen` maps a coordinate to viewport pixels, and
`screenToGroundCoordinate` maps a pixel back to the ground, returning nothing
when the pixel's ray misses the ground. Both use the camera's pose, so your own
UI and the renderer place things on the same pixel without duplicating the math.

The second constructor, `fitBounds`, builds a camera that frames a bounding box:
top-down and north-up, zoomed to the largest level that keeps the whole box
inside the inset region you pass. Insets are the pixels your own UI keeps clear,
such as a route card, a sheet panel, or an attribution label, so the box never
hides beneath them.

## Overlays are geography

Everything drawn on top of the tiles is an overlay: a `MapOverlay` record with a
kind and the payload that kind reads.

| Kind | Payload |
|---|---|
| `RoutePolyline` | `coordinates`, a polyline in traversal order |
| `FilledPolygon` | `ringCoordinates`, one outer ring in traversal order |
| `PointMarkers` and `PointPins` | `pointMarkerPositions`, one coordinate per marker |
| `TrafficFlow` | `trafficFlowSegments`, traffic records whose `shapeCoordinates` are coordinates |

The payload is the point: a list of `GeographicCoordinate`. An overlay is not a
picture and not a set of screen pixels. It is geography plus a style (a color, a
line width, a marker radius), and the SDK projects it through the same camera as
the tiles in every frame.

That shared space is why an overlay needs no conversion work from your
application. A calculated route carries its geometry as coordinates on its
steps; flatten those shapes into one list and a `RoutePolyline` draws over the
roads. A place returned by search carries a coordinate; put it in
`pointMarkerPositions` and the marker sits on that place. Panning, zooming,
rotating, and tilting keep the two layers aligned because every frame
re-projects the same geographic inputs.

## A frame in order

A frame is one pass through the same few steps:

1. You choose a `MapCamera` pose.
2. The camera lists the tiles the view covers and maps every overlay coordinate
   into the viewport.
3. The SDK fetches the missing tiles, draws them, then draws the overlays
   through the same camera.
4. A new pose repeats the pass; the coordinate space never changes.

Once those steps are clear, the guides are applications of them. Start with
[Your first map](first-map.md), then continue to [Search and
routing](search-and-routing.md), [Turn-by-turn
navigation](navigation-simulation.md), and [Offline maps](offline-maps.md).
