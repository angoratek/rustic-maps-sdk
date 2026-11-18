//! Deterministic GeoJSON tiling at the source boundary.
//!
//! A GeoJSON source is consumed through this module so downstream buckets,
//! overlays and symbol placement see the same [`VectorTile`] feature model a
//! vector source produces. The tiler projects each GeoJSON position through
//! web mercator into tile-local extent coordinates, clips it to the tile
//! rectangle, optionally simplifies it and quantizes it to the integer extent.
//!
//! # Tiling policy
//!
//! - **Projection**: longitude and latitude convert to normalized world
//!   coordinates, then to the tile's local grid of [`DEFAULT_TILE_EXTENT`]
//!   units.
//! - **Clipping**: points outside the tile (plus the configured buffer) are
//!   dropped; line segments are clipped to the rectangle; polygon rings are
//!   clipped with the Sutherland–Hodgman algorithm and re-oriented so an
//!   exterior ring winds positively and a hole negatively in the tile's
//!   `(x right, y down)` grid.
//! - **Simplification**: the Douglas–Peucker algorithm runs on the tile-local
//!   float geometry with the configured tolerance. The default tolerance is
//!   zero, so quantization to the integer extent is the only loss.
//! - **Quantization**: every surviving coordinate rounds to the nearest
//!   integer extent unit, consecutive duplicates collapse, rings with fewer
//!   than three distinct vertices drop and line pieces with fewer than two
//!   drop.
//! - **Determinism**: features keep their input order, attributes sort by key,
//!   polygon rings keep their source order with each exterior followed by its
//!   holes, and a generated feature id is a fixed FNV-1a hash of the feature's
//!   canonical JSON. The same collection and the same tile address therefore
//!   always produce a byte-identical tile.
//!
//! The module is pure CPU and JSON: it never fetches, reads a clock or uses a
//! hash map whose iteration order could leak into the output.

use crate::tile_key::TileKey;
use rustic_maps_vector_tiles::{
    DEFAULT_TILE_EXTENT, VectorTile, VectorTileAttributeValue, VectorTileFeature,
    VectorTileGeometry, VectorTileGeometryPoint, VectorTileGeometryType, VectorTileLayer,
    VectorTileLinearRing,
};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;
use std::f64::consts::PI;

/// The name of the implicit tile layer a GeoJSON source produces. A GeoJSON
/// style layer reads it without a `source-layer`.
pub const GEOJSON_TILE_LAYER_NAME: &str = "geojson";

/// The default tile edge, in extent units.
pub const DEFAULT_GEOJSON_TILING_EXTENT: u64 = DEFAULT_TILE_EXTENT;

/// The default clipping buffer, in extent units. A style's source `buffer`
/// overrides it; zero keeps a feature in exactly one tile and makes the tile
/// set a partition of the input.
pub const DEFAULT_GEOJSON_BUFFER_PIXELS: f64 = 0.0;

/// The default Douglas–Peucker tolerance, in extent units. Zero disables
/// simplification.
pub const DEFAULT_GEOJSON_SIMPLIFICATION_TOLERANCE: f64 = 0.0;

/// The latitude beyond which web mercator is not finite.
pub const GEOJSON_MAXIMUM_MERCATOR_LATITUDE_DEGREES: f64 = 85.051_128_779_806_59;

/// The FNV-1a 64-bit offset basis a generated feature id starts from.
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// The FNV-1a 64-bit prime.
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// How one GeoJSON source is tiled.
#[derive(Debug, Clone, PartialEq)]
pub struct GeoJsonTilingOptions {
    /// The tile edge in extent units.
    pub extent: u64,
    /// The clipping buffer beyond the tile edge, in extent units.
    pub buffer_pixels: f64,
    /// The Douglas–Peucker tolerance in extent units; zero disables it.
    pub simplification_tolerance: f64,
    /// Whether a feature without an `id` gets a stable generated identifier.
    pub generate_identifier: bool,
    /// The name of the tile layer the tiler emits.
    pub layer_name: String,
}

impl Default for GeoJsonTilingOptions {
    fn default() -> Self {
        Self {
            extent: DEFAULT_GEOJSON_TILING_EXTENT,
            buffer_pixels: DEFAULT_GEOJSON_BUFFER_PIXELS,
            simplification_tolerance: DEFAULT_GEOJSON_SIMPLIFICATION_TOLERANCE,
            generate_identifier: false,
            layer_name: GEOJSON_TILE_LAYER_NAME.to_string(),
        }
    }
}

/// Why a GeoJSON source could not be tiled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeoJsonTilingError {
    /// The malformed member or geometry.
    pub detail: String,
}

impl std::fmt::Display for GeoJsonTilingError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "GeoJSON tiling failed: {}", self.detail)
    }
}

impl std::error::Error for GeoJsonTilingError {}

impl GeoJsonTilingError {
    fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}

/// Tiles one GeoJSON value for one tile address.
///
/// # Parameters
/// - `json_value` — a `FeatureCollection`, a single `Feature` or a bare
///   geometry.
/// - `tile_key` — the web-mercator address the geometry is clipped to.
/// - `options` — extent, buffer, simplification and layer naming.
///
/// # Errors
/// A value that is not a GeoJSON object, a missing or unsupported geometry
/// type, or a malformed coordinate reports [`GeoJsonTilingError`]. A feature
/// with no geometry is skipped, not an error.
pub fn tile_geojson_for_tile(
    json_value: &JsonValue,
    tile_key: TileKey,
    options: &GeoJsonTilingOptions,
) -> Result<VectorTile, GeoJsonTilingError> {
    let extent = options.extent.max(1);
    let projection = TileProjection::new(tile_key, extent);
    let clip_rectangle = ClipRectangle::new(extent, options.buffer_pixels.max(0.0));
    let features = collect_features(json_value)?;
    let mut tile_features = Vec::new();
    for (feature_index, feature) in features.into_iter().enumerate() {
        if let Some(tile_feature) = tile_feature(
            feature,
            feature_index,
            &projection,
            &clip_rectangle,
            options,
            extent,
        )? {
            tile_features.push(tile_feature);
        }
    }
    Ok(VectorTile {
        layers: vec![VectorTileLayer {
            name: options.layer_name.clone(),
            extent,
            version: 2,
            features: tile_features,
        }],
    })
}

/// Tiles one GeoJSON value for every requested tile address.
///
/// The map keeps the requested addresses' order and every tile is produced by
/// [`tile_geojson_for_tile`], so the same collection and viewport always
/// produce the same map.
///
/// # Errors
/// The first tile that cannot be produced reports [`GeoJsonTilingError`].
pub fn tile_geojson_for_tiles(
    json_value: &JsonValue,
    tile_keys: &[TileKey],
    options: &GeoJsonTilingOptions,
) -> Result<BTreeMap<TileKey, VectorTile>, GeoJsonTilingError> {
    let mut tiles = BTreeMap::new();
    for tile_key in tile_keys {
        tiles.insert(
            *tile_key,
            tile_geojson_for_tile(json_value, *tile_key, options)?,
        );
    }
    Ok(tiles)
}

/// One GeoJSON feature after collection, before projection.
struct CollectedFeature<'a> {
    identifier: Option<u64>,
    properties: Vec<(String, VectorTileAttributeValue)>,
    geometry_type: Option<&'a str>,
    geometry: Option<&'a JsonValue>,
    canonical_json: String,
}

/// Collects the features of a `FeatureCollection`, `Feature` or bare geometry.
fn collect_features(
    json_value: &JsonValue,
) -> Result<Vec<CollectedFeature<'_>>, GeoJsonTilingError> {
    let object = json_value
        .as_object()
        .ok_or_else(|| GeoJsonTilingError::new("the value must be a GeoJSON object"))?;
    match object.get("type").and_then(JsonValue::as_str) {
        Some("FeatureCollection") => {
            let features = object
                .get("features")
                .and_then(JsonValue::as_array)
                .ok_or_else(|| {
                    GeoJsonTilingError::new("a FeatureCollection must carry a features array")
                })?;
            features.iter().map(collect_feature).collect()
        }
        Some("Feature") => Ok(vec![collect_feature(json_value)?]),
        Some(geometry_type) => Ok(vec![CollectedFeature {
            identifier: None,
            properties: Vec::new(),
            geometry_type: Some(geometry_type),
            geometry: Some(json_value),
            canonical_json: canonical_json(json_value),
        }]),
        None => Err(GeoJsonTilingError::new(
            "a GeoJSON object must declare its type",
        )),
    }
}

/// Collects one `Feature` object.
fn collect_feature(json_value: &JsonValue) -> Result<CollectedFeature<'_>, GeoJsonTilingError> {
    let object = json_value
        .as_object()
        .ok_or_else(|| GeoJsonTilingError::new("a Feature must be an object"))?;
    if object.get("type").and_then(JsonValue::as_str) != Some("Feature") {
        return Err(GeoJsonTilingError::new(
            "a FeatureCollection may contain only Feature objects",
        ));
    }
    let properties = object
        .get("properties")
        .and_then(JsonValue::as_object)
        .map(|property_object| {
            let mut attributes: Vec<(String, VectorTileAttributeValue)> = property_object
                .iter()
                .map(|(name, value)| (name.clone(), attribute_value(value)))
                .collect();
            attributes.sort_by(|first, second| first.0.cmp(&second.0));
            attributes
        })
        .unwrap_or_default();
    let geometry = object
        .get("geometry")
        .filter(|geometry| !geometry.is_null());
    let geometry_type = geometry
        .and_then(|geometry| geometry.get("type"))
        .and_then(JsonValue::as_str);
    Ok(CollectedFeature {
        identifier: object.get("id").and_then(JsonValue::as_u64),
        properties,
        geometry_type,
        geometry,
        canonical_json: canonical_json(json_value),
    })
}

/// One JSON attribute as a vector-tile attribute. Objects and arrays keep
/// their canonical JSON text, so their order is stable.
fn attribute_value(json_value: &JsonValue) -> VectorTileAttributeValue {
    match json_value {
        JsonValue::String(text) => VectorTileAttributeValue::Text(text.clone()),
        JsonValue::Bool(boolean) => VectorTileAttributeValue::Boolean(*boolean),
        JsonValue::Number(number) => match number.as_i64() {
            Some(integer) => VectorTileAttributeValue::Signed(integer),
            None => VectorTileAttributeValue::Double(number.as_f64().unwrap_or_default()),
        },
        JsonValue::Null | JsonValue::Array(_) | JsonValue::Object(_) => {
            VectorTileAttributeValue::Text(canonical_json(json_value))
        }
    }
}

/// Canonical JSON text: `serde_json`'s object map is key-sorted, so equal
/// values always produce equal text.
fn canonical_json(json_value: &JsonValue) -> String {
    serde_json::to_string(json_value).unwrap_or_default()
}

/// The projected, clipped and quantized form of one feature.
fn tile_feature(
    feature: CollectedFeature<'_>,
    feature_index: usize,
    projection: &TileProjection,
    clip_rectangle: &ClipRectangle,
    options: &GeoJsonTilingOptions,
    extent: u64,
) -> Result<Option<VectorTileFeature>, GeoJsonTilingError> {
    let Some(geometry) = feature.geometry else {
        return Ok(None);
    };
    let geometry_type = feature.geometry_type.ok_or_else(|| {
        GeoJsonTilingError::new(format!(
            "feature {feature_index} carries a geometry without a type"
        ))
    })?;
    let coordinates = geometry.get("coordinates").ok_or_else(|| {
        GeoJsonTilingError::new(format!(
            "feature {feature_index} carries a {geometry_type} without coordinates"
        ))
    })?;
    let identifier = feature.identifier.or_else(|| {
        options
            .generate_identifier
            .then(|| stable_identifier(&feature.canonical_json))
    });
    let (geometry_type, geometry) = match geometry_type {
        "Point" => {
            let point = project_point(coordinates, projection)?;
            let points = point
                .filter(|point| clip_rectangle.contains(*point))
                .map(|point| vec![quantized_point(point, extent)])
                .unwrap_or_default();
            (
                VectorTileGeometryType::Point,
                VectorTileGeometry::Points(points),
            )
        }
        "MultiPoint" => {
            let coordinates = coordinates
                .as_array()
                .ok_or_else(|| GeoJsonTilingError::new("a MultiPoint needs an array"))?;
            let mut points = Vec::new();
            for coordinate in coordinates {
                if let Some(point) = project_point(coordinate, projection)?
                    && clip_rectangle.contains(point)
                {
                    points.push(quantized_point(point, extent));
                }
            }
            (
                VectorTileGeometryType::Point,
                VectorTileGeometry::Points(points),
            )
        }
        "LineString" => {
            let line = project_line_string(coordinates, projection)?;
            let line_strings = clip_rectangle
                .clip_line_string(&line, options.simplification_tolerance)
                .into_iter()
                .map(|line_string| quantized_line_string(&line_string, extent))
                .filter(|line_string| line_string.len() >= 2)
                .collect();
            (
                VectorTileGeometryType::LineString,
                VectorTileGeometry::LineStrings(line_strings),
            )
        }
        "MultiLineString" => {
            let line_strings_value = coordinates
                .as_array()
                .ok_or_else(|| GeoJsonTilingError::new("a MultiLineString needs an array"))?;
            let mut line_strings = Vec::new();
            for line_string_value in line_strings_value {
                let line = project_line_string(line_string_value, projection)?;
                for clipped_line in
                    clip_rectangle.clip_line_string(&line, options.simplification_tolerance)
                {
                    let quantized = quantized_line_string(&clipped_line, extent);
                    if quantized.len() >= 2 {
                        line_strings.push(quantized);
                    }
                }
            }
            (
                VectorTileGeometryType::LineString,
                VectorTileGeometry::LineStrings(line_strings),
            )
        }
        "Polygon" => {
            let rings = project_polygon(coordinates, projection)?;
            let polygons = clip_rectangle
                .clip_polygon(&rings, options.simplification_tolerance)
                .map(|polygon| quantized_polygon(&polygon, extent))
                .unwrap_or_default();
            (
                VectorTileGeometryType::Polygon,
                VectorTileGeometry::Polygons(polygons),
            )
        }
        "MultiPolygon" => {
            let polygons_value = coordinates
                .as_array()
                .ok_or_else(|| GeoJsonTilingError::new("a MultiPolygon needs an array"))?;
            let mut rings = Vec::new();
            for polygon_value in polygons_value {
                let polygon_rings = project_polygon(polygon_value, projection)?;
                if let Some(clipped) =
                    clip_rectangle.clip_polygon(&polygon_rings, options.simplification_tolerance)
                {
                    rings.extend(quantized_polygon(&clipped, extent));
                }
            }
            (
                VectorTileGeometryType::Polygon,
                VectorTileGeometry::Polygons(rings),
            )
        }
        other => {
            return Err(GeoJsonTilingError::new(format!(
                "unsupported GeoJSON geometry type {other:?}"
            )));
        }
    };
    if geometry_is_empty(&geometry) {
        // A feature clipped entirely out of this tile contributes nothing; it
        // must not surface as an empty geometry the renderer would walk.
        return Ok(None);
    }
    Ok(Some(VectorTileFeature {
        identifier,
        geometry_type,
        geometry,
        attributes: feature.properties,
    }))
}

/// Whether a clipped geometry carries no drawable primitive.
fn geometry_is_empty(geometry: &VectorTileGeometry) -> bool {
    match geometry {
        VectorTileGeometry::Points(points) => points.is_empty(),
        VectorTileGeometry::LineStrings(line_strings) => line_strings.is_empty(),
        VectorTileGeometry::Polygons(rings) => rings.is_empty(),
    }
}

/// The web-mercator projection of one tile address into local extent units.
struct TileProjection {
    /// The reciprocal of the world span covered by one tile.
    tiles_per_world: f64,
    /// The tile's column.
    tile_x: u32,
    /// The tile's row.
    tile_y: u32,
    /// The tile edge in extent units.
    extent: f64,
}

impl TileProjection {
    fn new(tile_key: TileKey, extent: u64) -> Self {
        Self {
            tiles_per_world: 1u64
                .checked_shl(u32::from(tile_key.zoom_level))
                .unwrap_or(u64::MAX) as f64,
            tile_x: tile_key.tile_x,
            tile_y: tile_key.tile_y,
            extent: extent as f64,
        }
    }

    /// Projects a GeoJSON position to tile-local extent units.
    fn project(&self, position: &JsonValue) -> Result<Option<Point>, GeoJsonTilingError> {
        let coordinates = position
            .as_array()
            .ok_or_else(|| GeoJsonTilingError::new("a position must be an array"))?;
        let longitude = coordinates
            .first()
            .and_then(JsonValue::as_f64)
            .ok_or_else(|| GeoJsonTilingError::new("a position needs a numeric longitude"))?;
        let latitude = coordinates
            .get(1)
            .and_then(JsonValue::as_f64)
            .ok_or_else(|| GeoJsonTilingError::new("a position needs a numeric latitude"))?;
        if !longitude.is_finite() || !latitude.is_finite() {
            return Ok(None);
        }
        let world_x = (longitude + 180.0) / 360.0;
        let clamped_latitude = latitude.clamp(
            -GEOJSON_MAXIMUM_MERCATOR_LATITUDE_DEGREES,
            GEOJSON_MAXIMUM_MERCATOR_LATITUDE_DEGREES,
        );
        let latitude_radians = clamped_latitude.to_radians();
        let world_y =
            (1.0 - (latitude_radians.tan() + 1.0 / latitude_radians.cos()).ln() / PI) / 2.0;
        Ok(Some(Point {
            x: (world_x * self.tiles_per_world - f64::from(self.tile_x)) * self.extent,
            y: (world_y * self.tiles_per_world - f64::from(self.tile_y)) * self.extent,
        }))
    }
}

/// Projects one position; a non-array reports an error.
fn project_point(
    position: &JsonValue,
    projection: &TileProjection,
) -> Result<Option<Point>, GeoJsonTilingError> {
    projection.project(position)
}

/// Projects a LineString's coordinates, dropping non-finite positions.
fn project_line_string(
    coordinates: &JsonValue,
    projection: &TileProjection,
) -> Result<Vec<Point>, GeoJsonTilingError> {
    let positions = coordinates
        .as_array()
        .ok_or_else(|| GeoJsonTilingError::new("a LineString needs an array of positions"))?;
    let mut line = Vec::with_capacity(positions.len());
    for position in positions {
        if let Some(point) = projection.project(position)? {
            line.push(point);
        }
    }
    Ok(line)
}

/// Projects a Polygon's rings; the first ring is exterior and the rest are
/// holes. Positions that cannot project are dropped.
fn project_polygon(
    coordinates: &JsonValue,
    projection: &TileProjection,
) -> Result<Vec<Vec<Point>>, GeoJsonTilingError> {
    let rings = coordinates
        .as_array()
        .ok_or_else(|| GeoJsonTilingError::new("a Polygon needs an array of rings"))?;
    let mut projected_rings = Vec::with_capacity(rings.len());
    for ring in rings {
        let mut projected_ring = project_line_string(ring, projection)?;
        remove_closing_vertex(&mut projected_ring);
        projected_rings.push(projected_ring);
    }
    Ok(projected_rings)
}

/// A tile-local point in float extent units.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Point {
    x: f64,
    y: f64,
}

/// The tile rectangle, optionally grown by a buffer.
struct ClipRectangle {
    minimum_x: f64,
    minimum_y: f64,
    maximum_x: f64,
    maximum_y: f64,
}

impl ClipRectangle {
    fn new(extent: u64, buffer_pixels: f64) -> Self {
        let extent = extent as f64;
        Self {
            minimum_x: -buffer_pixels,
            minimum_y: -buffer_pixels,
            maximum_x: extent + buffer_pixels,
            maximum_y: extent + buffer_pixels,
        }
    }

    fn contains(&self, point: Point) -> bool {
        point.x >= self.minimum_x
            && point.x <= self.maximum_x
            && point.y >= self.minimum_y
            && point.y <= self.maximum_y
    }

    /// Clips one open polyline into the pieces that lie inside the rectangle.
    ///
    /// Each inside run becomes one piece; each segment crossing the boundary
    /// gains its two exact intersection points, so adjacent tiles agree on the
    /// boundary coordinate. A piece with fewer than two points is dropped.
    fn clip_line_string(&self, line: &[Point], simplification_tolerance: f64) -> Vec<Vec<Point>> {
        let simplified = simplify_open_line(line, simplification_tolerance);
        let mut pieces = Vec::new();
        let mut current_piece: Vec<Point> = Vec::new();
        for window in simplified.windows(2) {
            let start = window[0];
            let end = window[1];
            let Some((clipped_start, clipped_end)) = self.clip_segment(start, end) else {
                if current_piece.len() >= 2 {
                    pieces.push(std::mem::take(&mut current_piece));
                } else {
                    current_piece.clear();
                }
                continue;
            };
            let continues_current_piece = current_piece
                .last()
                .is_some_and(|last| last.x == clipped_start.x && last.y == clipped_start.y);
            if !continues_current_piece {
                if current_piece.len() >= 2 {
                    pieces.push(std::mem::take(&mut current_piece));
                } else {
                    current_piece.clear();
                }
                current_piece.push(clipped_start);
            }
            current_piece.push(clipped_end);
            let segment_leaves_rectangle = clipped_end.x != end.x || clipped_end.y != end.y;
            if segment_leaves_rectangle {
                if current_piece.len() >= 2 {
                    pieces.push(std::mem::take(&mut current_piece));
                } else {
                    current_piece.clear();
                }
            }
        }
        if current_piece.len() >= 2 {
            pieces.push(current_piece);
        }
        pieces
    }

    /// Clips a polygon (exterior first, then holes) to the rectangle.
    ///
    /// A clipped exterior with fewer than three distinct vertices drops the
    /// whole polygon. A hole is dropped when it no longer intersects the
    /// rectangle.
    fn clip_polygon(
        &self,
        rings: &[Vec<Point>],
        simplification_tolerance: f64,
    ) -> Option<Vec<Vec<Point>>> {
        let exterior = rings.first()?;
        let simplified_exterior = simplify_closed_ring(exterior, simplification_tolerance);
        let clipped_exterior = self.clip_ring_against_rectangle(&simplified_exterior);
        if distinct_vertex_count(&clipped_exterior) < 3 {
            return None;
        }
        let mut clipped_rings = vec![clipped_exterior];
        for hole in rings.iter().skip(1) {
            let simplified_hole = simplify_closed_ring(hole, simplification_tolerance);
            let clipped_hole = self.clip_ring_against_rectangle(&simplified_hole);
            if distinct_vertex_count(&clipped_hole) >= 3 {
                clipped_rings.push(clipped_hole);
            }
        }
        Some(clipped_rings)
    }

    /// Sutherland–Hodgman against the four rectangle edges.
    fn clip_ring_against_rectangle(&self, ring: &[Point]) -> Vec<Point> {
        if ring.is_empty() {
            return Vec::new();
        }
        let edges: [(Point, Point); 4] = [
            (
                Point {
                    x: self.minimum_x,
                    y: self.minimum_y,
                },
                Point {
                    x: self.maximum_x,
                    y: self.minimum_y,
                },
            ),
            (
                Point {
                    x: self.maximum_x,
                    y: self.minimum_y,
                },
                Point {
                    x: self.maximum_x,
                    y: self.maximum_y,
                },
            ),
            (
                Point {
                    x: self.maximum_x,
                    y: self.maximum_y,
                },
                Point {
                    x: self.minimum_x,
                    y: self.maximum_y,
                },
            ),
            (
                Point {
                    x: self.minimum_x,
                    y: self.maximum_y,
                },
                Point {
                    x: self.minimum_x,
                    y: self.minimum_y,
                },
            ),
        ];
        let mut clipped = ring.to_vec();
        for edge in edges {
            if clipped.is_empty() {
                break;
            }
            clipped = clip_ring_against_edge(&clipped, edge.0, edge.1);
        }
        clipped
    }

    /// Liang–Barsky clipping of one segment; `None` when it misses the
    /// rectangle.
    fn clip_segment(&self, start: Point, end: Point) -> Option<(Point, Point)> {
        let delta_x = end.x - start.x;
        let delta_y = end.y - start.y;
        let boundaries = [
            (-delta_x, start.x - self.minimum_x),
            (delta_x, self.maximum_x - start.x),
            (-delta_y, start.y - self.minimum_y),
            (delta_y, self.maximum_y - start.y),
        ];
        let mut entering = 0.0_f64;
        let mut leaving = 1.0_f64;
        for (boundary_direction, boundary_distance) in boundaries {
            if boundary_direction == 0.0 {
                if boundary_distance < 0.0 {
                    return None;
                }
                continue;
            }
            let ratio = boundary_distance / boundary_direction;
            if boundary_direction < 0.0 {
                entering = entering.max(ratio);
            } else {
                leaving = leaving.min(ratio);
            }
            if entering > leaving {
                return None;
            }
        }
        Some((
            Point {
                x: start.x + entering * delta_x,
                y: start.y + entering * delta_y,
            },
            Point {
                x: start.x + leaving * delta_x,
                y: start.y + leaving * delta_y,
            },
        ))
    }
}

/// Clips a ring against one directed edge, keeping the points to its left.
fn clip_ring_against_edge(ring: &[Point], edge_start: Point, edge_end: Point) -> Vec<Point> {
    let mut output = Vec::with_capacity(ring.len() + 2);
    for (index, current) in ring.iter().enumerate() {
        let previous = ring[(index + ring.len() - 1) % ring.len()];
        let current_inside = is_inside_edge(*current, edge_start, edge_end);
        let previous_inside = is_inside_edge(previous, edge_start, edge_end);
        if current_inside {
            if !previous_inside
                && let Some(intersection) =
                    segment_intersection(previous, *current, edge_start, edge_end)
            {
                output.push(intersection);
            }
            output.push(*current);
        } else if previous_inside
            && let Some(intersection) =
                segment_intersection(previous, *current, edge_start, edge_end)
        {
            output.push(intersection);
        }
    }
    output
}

/// Whether a point lies on the left side (or on) the directed edge.
fn is_inside_edge(point: Point, edge_start: Point, edge_end: Point) -> bool {
    (edge_end.x - edge_start.x) * (point.y - edge_start.y)
        - (edge_end.y - edge_start.y) * (point.x - edge_start.x)
        >= 0.0
}

/// The intersection of two segments, or `None` when they are parallel.
fn segment_intersection(
    first_start: Point,
    first_end: Point,
    second_start: Point,
    second_end: Point,
) -> Option<Point> {
    let first_delta = Point {
        x: first_end.x - first_start.x,
        y: first_end.y - first_start.y,
    };
    let second_delta = Point {
        x: second_end.x - second_start.x,
        y: second_end.y - second_start.y,
    };
    let denominator = first_delta.x * second_delta.y - first_delta.y * second_delta.x;
    if denominator.abs() <= f64::EPSILON {
        return None;
    }
    let difference = Point {
        x: second_start.x - first_start.x,
        y: second_start.y - first_start.y,
    };
    let ratio = (difference.x * second_delta.y - difference.y * second_delta.x) / denominator;
    Some(Point {
        x: first_start.x + ratio * first_delta.x,
        y: first_start.y + ratio * first_delta.y,
    })
}

/// Removes a repeated closing vertex from a projected ring.
fn remove_closing_vertex(ring: &mut Vec<Point>) {
    if ring.len() >= 2 {
        let first = ring[0];
        let last = ring[ring.len() - 1];
        if first.x == last.x && first.y == last.y {
            ring.pop();
        }
    }
}

/// The number of distinct vertices a ring carries.
fn distinct_vertex_count(ring: &[Point]) -> usize {
    let mut distinct = Vec::new();
    for point in ring {
        if !distinct
            .iter()
            .any(|existing: &Point| existing.x == point.x && existing.y == point.y)
        {
            distinct.push(*point);
        }
    }
    distinct.len()
}

/// Simplifies an open polyline with Douglas–Peucker. A non-positive tolerance
/// returns the input unchanged; an output shorter than two points returns the
/// input, so a degenerate tolerance can never erase a line.
fn simplify_open_line(line: &[Point], tolerance: f64) -> Vec<Point> {
    if line.len() <= 2 || tolerance <= 0.0 {
        return line.to_vec();
    }
    let mut keep = vec![false; line.len()];
    keep[0] = true;
    keep[line.len() - 1] = true;
    let mut ranges = vec![(0, line.len() - 1)];
    while let Some((start, end)) = ranges.pop() {
        let mut farthest_distance = 0.0;
        let mut farthest_index = start;
        for index in (start + 1)..end {
            let distance = perpendicular_distance(line[index], line[start], line[end]);
            if distance > farthest_distance {
                farthest_distance = distance;
                farthest_index = index;
            }
        }
        if farthest_distance > tolerance {
            keep[farthest_index] = true;
            ranges.push((start, farthest_index));
            ranges.push((farthest_index, end));
        }
    }
    let simplified: Vec<Point> = line
        .iter()
        .zip(keep)
        .filter_map(|(point, keep_point)| keep_point.then_some(*point))
        .collect();
    if simplified.len() >= 2 {
        simplified
    } else {
        line.to_vec()
    }
}

/// Simplifies a closed ring, keeping the first vertex and the closing edge.
fn simplify_closed_ring(ring: &[Point], tolerance: f64) -> Vec<Point> {
    if ring.len() <= 3 || tolerance <= 0.0 {
        return ring.to_vec();
    }
    let mut closed = ring.to_vec();
    closed.push(ring[0]);
    let mut simplified = simplify_open_line(&closed, tolerance);
    simplified.pop();
    if simplified.len() >= 3 {
        simplified
    } else {
        ring.to_vec()
    }
}

/// The distance from a point to the infinite line through two points.
fn perpendicular_distance(point: Point, start: Point, end: Point) -> f64 {
    let delta_x = end.x - start.x;
    let delta_y = end.y - start.y;
    let length = (delta_x * delta_x + delta_y * delta_y).sqrt();
    if length == 0.0 {
        return ((point.x - start.x).powi(2) + (point.y - start.y).powi(2)).sqrt();
    }
    ((delta_x * (start.y - point.y) - (start.x - point.x) * delta_y) / length).abs()
}

/// Quantizes one point to the integer extent grid.
fn quantized_point(point: Point, extent: u64) -> VectorTileGeometryPoint {
    let maximum = i64::try_from(extent).unwrap_or(i64::MAX);
    VectorTileGeometryPoint {
        x_coordinate: quantize(point.x, maximum),
        y_coordinate: quantize(point.y, maximum),
    }
}

/// Rounds one coordinate to the nearest integer, dropping consecutive
/// duplicates.
fn quantized_line_string(line: &[Point], extent: u64) -> Vec<VectorTileGeometryPoint> {
    let mut quantized: Vec<VectorTileGeometryPoint> = Vec::with_capacity(line.len());
    for point in line {
        let next = quantized_point(*point, extent);
        if quantized.last() != Some(&next) {
            quantized.push(next);
        }
    }
    quantized
}

/// Quantizes one polygon's rings, orienting the exterior positively and every
/// hole negatively in the tile's `(x right, y down)` grid. A ring that
/// degenerates below three distinct vertices is dropped; if the exterior
/// drops, the whole polygon drops.
fn quantized_polygon(polygon: &[Vec<Point>], extent: u64) -> Vec<VectorTileLinearRing> {
    let Some(exterior) = polygon.first() else {
        return Vec::new();
    };
    let Some(exterior_ring) = oriented_ring(exterior, extent, true) else {
        return Vec::new();
    };
    let mut rings = vec![exterior_ring];
    for hole in polygon.iter().skip(1) {
        if let Some(hole_ring) = oriented_ring(hole, extent, false) {
            rings.push(hole_ring);
        }
    }
    rings
}

/// Quantizes one ring and orients it for its role.
fn oriented_ring(ring: &[Point], extent: u64, exterior: bool) -> Option<VectorTileLinearRing> {
    let mut vertices = quantized_line_string(ring, extent);
    if vertices.len() >= 2 && vertices.first() == vertices.last() {
        vertices.pop();
    }
    if distinct_quantized_vertex_count(&vertices) < 3 {
        return None;
    }
    let is_positive = ring_signed_area(&vertices) > 0.0;
    if is_positive != exterior {
        vertices.reverse();
    }
    Some(VectorTileLinearRing { vertices })
}

/// The number of distinct integer vertices.
fn distinct_quantized_vertex_count(vertices: &[VectorTileGeometryPoint]) -> usize {
    let mut distinct = Vec::new();
    for vertex in vertices {
        if !distinct.contains(vertex) {
            distinct.push(*vertex);
        }
    }
    distinct.len()
}

/// The ring's doubled signed area in the tile's y-down grid.
fn ring_signed_area(vertices: &[VectorTileGeometryPoint]) -> f64 {
    let mut doubled_area = 0.0;
    for index in 0..vertices.len() {
        let current = vertices[index];
        let next = vertices[(index + 1) % vertices.len()];
        doubled_area += current.x_coordinate as f64 * next.y_coordinate as f64
            - next.x_coordinate as f64 * current.y_coordinate as f64;
    }
    doubled_area
}

/// Rounds to the nearest integer and clamps into `[-maximum, maximum]`.
fn quantize(coordinate: f64, maximum: i64) -> i64 {
    if coordinate.is_nan() {
        return 0;
    }
    coordinate.round().clamp(-(maximum as f64), maximum as f64) as i64
}

/// A stable FNV-1a identifier for a feature without an `id`.
fn stable_identifier(canonical_json: &str) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    for byte in canonical_json.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The two-feature fixture: one point in the western z1 tile and one in
    /// the eastern z1 tile, enough to prove projection, clipping,
    /// quantization and determinism.
    fn two_feature_collection() -> JsonValue {
        json!({
            "type": "FeatureCollection",
            "features": [
                {
                    "type": "Feature",
                    "id": 7,
                    "properties": { "name": "West", "rank": 2 },
                    "geometry": { "type": "Point", "coordinates": [-90.0, 45.0] }
                },
                {
                    "type": "Feature",
                    "properties": { "name": "East", "class": "primary" },
                    "geometry": { "type": "Point", "coordinates": [90.0, 45.0] }
                }
            ]
        })
    }

    #[test]
    fn two_features_tile_to_the_expected_tiles_for_a_fixed_viewport() {
        let options = GeoJsonTilingOptions::default();
        let west_tile = TileKey::new(0, 0, 1).expect("the tile address is valid");
        let east_tile = TileKey::new(1, 0, 1).expect("the tile address is valid");
        let tiles =
            tile_geojson_for_tiles(&two_feature_collection(), &[west_tile, east_tile], &options)
                .expect("the collection tiles");
        assert_eq!(tiles.len(), 2);
        for (tile_key, expected_name) in [(west_tile, "West"), (east_tile, "East")] {
            let tile = &tiles[&tile_key];
            assert_eq!(tile.layers.len(), 1);
            let layer = &tile.layers[0];
            assert_eq!(layer.name, GEOJSON_TILE_LAYER_NAME);
            assert_eq!(layer.extent, DEFAULT_GEOJSON_TILING_EXTENT);
            assert_eq!(layer.features.len(), 1);
            let feature = &layer.features[0];
            assert_eq!(feature.geometry_type, VectorTileGeometryType::Point);
            assert_eq!(
                feature.geometry,
                VectorTileGeometry::Points(vec![VectorTileGeometryPoint {
                    x_coordinate: 2048,
                    y_coordinate: 2947,
                }])
            );
            let name_attribute = feature
                .attributes
                .iter()
                .find(|(attribute_name, _)| attribute_name == "name")
                .expect("the feature carries a name");
            assert_eq!(
                name_attribute.1,
                VectorTileAttributeValue::Text(expected_name.to_string())
            );
        }
    }

    #[test]
    fn the_same_collection_and_viewport_produce_identical_tiles() {
        let options = GeoJsonTilingOptions::default();
        let tile_keys = [
            TileKey::new(0, 0, 1).expect("valid"),
            TileKey::new(1, 0, 1).expect("valid"),
        ];
        let first = tile_geojson_for_tiles(&two_feature_collection(), &tile_keys, &options)
            .expect("the collection tiles");
        let second = tile_geojson_for_tiles(&two_feature_collection(), &tile_keys, &options)
            .expect("the collection tiles");
        assert_eq!(first, second);
        assert_eq!(
            format!("{first:?}"),
            format!("{second:?}"),
            "the tile output must be byte-identical for the same input"
        );
    }

    #[test]
    fn a_polygon_exterior_winds_positively_and_its_hole_negatively() {
        let collection = json!({
            "type": "Feature",
            "properties": {},
            "geometry": {
                "type": "Polygon",
                "coordinates": [
                    [[-100.0, 55.0], [-80.0, 55.0], [-80.0, 35.0], [-100.0, 35.0], [-100.0, 55.0]],
                    [[-95.0, 50.0], [-85.0, 50.0], [-85.0, 40.0], [-95.0, 40.0], [-95.0, 50.0]]
                ]
            }
        });
        let tile_key = TileKey::new(0, 0, 1).expect("the tile address is valid");
        let tile = tile_geojson_for_tile(&collection, tile_key, &GeoJsonTilingOptions::default())
            .expect("the polygon tiles");
        let VectorTileGeometry::Polygons(rings) = &tile.layers[0].features[0].geometry else {
            panic!("the feature is a polygon");
        };
        assert_eq!(rings.len(), 2);
        assert!(ring_signed_area(&rings[0].vertices) > 0.0);
        assert!(ring_signed_area(&rings[1].vertices) < 0.0);
    }

    #[test]
    fn a_generated_identifier_is_stable_for_the_same_feature() {
        let options = GeoJsonTilingOptions {
            generate_identifier: true,
            ..GeoJsonTilingOptions::default()
        };
        let tile_key = TileKey::new(1, 0, 1).expect("the tile address is valid");
        let first = tile_geojson_for_tile(&two_feature_collection(), tile_key, &options)
            .expect("the collection tiles");
        let second = tile_geojson_for_tile(&two_feature_collection(), tile_key, &options)
            .expect("the collection tiles");
        assert_eq!(
            first.layers[0].features[0].identifier,
            second.layers[0].features[0].identifier
        );
        assert!(first.layers[0].features[0].identifier.is_some());
    }
}
