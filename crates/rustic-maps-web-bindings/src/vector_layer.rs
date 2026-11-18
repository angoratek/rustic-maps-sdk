//! Browser vector-tile seam: one fetched vector tile becomes the plain
//! overlay descriptors the WebGPU canvas'
//! [`crate::canvas::BrowserMapView::set_overlays`] draws.
//!
//! The decoder ([`rustic_maps_vector_tiles`]), the data-only style
//! ([`rustic_maps_rendering::vector_style`]) and the tile-local-to-geographic
//! conversion all live below this seam; the wasm exports only unwrap the wire
//! envelope, validate the tile address, and serialize the styled
//! [`rustic_maps_rendering::MapOverlay`]s back into the JavaScript descriptor
//! vocabulary (`kind`, `coordinates`/`ringCoordinates`/`positions`, `color`,
//! `widthPixels`, `radiusPixels`). The field names and `kind` values come from
//! [`crate::canvas`], the same constants `set_overlays` parses, so the two
//! sides of the seam cannot drift.
//!
//! The descriptors carry geographic coordinates, exactly like the overlay
//! scenes the page already supplies, so the canvas projects them through the
//! same camera pose as the raster tiles. A page switches between raster and
//! vector tiles by changing where the overlay list comes from.
//!
//! Every failure is a coded error `JsValue`; nothing on this path panics,
//! because the tile bytes arrive from an untrusted network fetch.
//!
//! Known limit: a filled polygon's interior rings (holes) are computed by
//! `rustic_maps_rendering`'s style conversion, but `set_overlays`' `filledPolygon`
//! vocabulary carries only the exterior ring, so the seam serializes the
//! exterior and drops interior rings until the descriptor gains a hole field.

use crate::canvas::{
    OVERLAY_FIELD_COLOR, OVERLAY_FIELD_POSITIONS, OVERLAY_FIELD_RADIUS_PIXELS,
    OVERLAY_KIND_FILLED_POLYGON, OVERLAY_KIND_POINT_MARKERS, OVERLAY_KIND_POINT_PINS,
    OVERLAY_KIND_ROUTE_POLYLINE,
};
use crate::exports::{
    ERROR_CODE_INVALID_COORDINATE_INPUT, ERROR_CODE_RESPONSE_PARSING_FAILURE,
    ERROR_CODE_UNSUPPORTED_CAPABILITY, failure, failure_with_code, geographic_coordinate_object,
    object_from_fields,
};
use crate::generated_contract_field_names::*;
use geospatial_types::{GeographicCoordinate, VectorTileData, VectorTileEncoding};
use js_sys::{Array, Object};
use rustic_maps_rendering::MapOverlay;
use rustic_maps_rendering::TileKey;
use rustic_maps_rendering::vector_style::{
    VectorStyleArtifactError, VectorTileAddress, default_vector_tile_style,
    tile_local_point_to_coordinate, vector_tile_overlays, vector_tile_style_from_id,
};
use rustic_maps_style::CompiledStyle;
use rustic_maps_vector_tiles::{
    DEFAULT_TILE_EXTENT, MAXIMUM_DECOMPRESSED_VECTOR_TILE_BYTES, VectorTileGeometryPoint,
    decode_vector_tile,
};
use std::io::Read;
use wasm_bindgen::prelude::*;

/// Name of the tile-bounds object field carrying its north-west corner.
const TILE_BOUNDS_FIELD_NORTH_WEST: &str = "northWest";
/// Name of the tile-bounds object field carrying its south-east corner.
const TILE_BOUNDS_FIELD_SOUTH_EAST: &str = "southEast";

/// Extent coordinate of a tile's north-west corner (the layer grid's origin);
/// the south-east corner sits at [`DEFAULT_TILE_EXTENT`].
const TILE_NORTH_WEST_EXTENT_POINT: i64 = 0;

/// Decodes and styles one vector tile into the overlay descriptor list the
/// WebGPU canvas consumes.
///
/// `tile_bytes` is the wire body a provider served — plain MVT protobuf or a
/// gzip-wrapped body, which is sniffed and unwrapped here — and the tile
/// address is validated against the zoom grid before decoding. The result is
/// a plain JavaScript array of `set_overlays` descriptors in style-layer
/// order (later style layers paint over earlier ones). A malformed payload,
/// an out-of-grid address, an empty body or an undecompressable gzip envelope
/// rejects with a coded error; nothing throws across the wasm boundary as a
/// panic.
#[wasm_bindgen(js_name = vectorTileOverlayDescriptors)]
pub fn vector_tile_overlay_descriptors(
    tile_bytes: &[u8],
    tile_zoom_level: u8,
    tile_x: u32,
    tile_y: u32,
) -> Result<Array, JsValue> {
    overlay_descriptors_for_style(
        tile_bytes,
        tile_zoom_level,
        tile_x,
        tile_y,
        &default_vector_tile_style(),
    )
}

/// The same decode-and-style conversion, painted with the committed style a
/// style id names (`carto-light`, `carto-dark`, `openmaptiles-light`,
/// `openmaptiles-dark`). The page joins a registry descriptor's style schema
/// with the active theme, so one provider can serve a light and a dark base
/// map without a page-side style table. An unknown style id rejects with the
/// documented list of known ids.
#[wasm_bindgen(js_name = vectorTileOverlayDescriptorsForStyle)]
pub fn vector_tile_overlay_descriptors_for_style(
    tile_bytes: &[u8],
    tile_zoom_level: u8,
    tile_x: u32,
    tile_y: u32,
    style_id: &str,
) -> Result<Array, JsValue> {
    let style =
        vector_tile_style_from_id(style_id).map_err(|style_error: VectorStyleArtifactError| {
            failure_with_code(ERROR_CODE_UNSUPPORTED_CAPABILITY, &style_error.to_string())
        })?;
    overlay_descriptors_for_style(tile_bytes, tile_zoom_level, tile_x, tile_y, &style)
}

/// The shared decode-and-style body of both descriptor exports.
fn overlay_descriptors_for_style(
    tile_bytes: &[u8],
    tile_zoom_level: u8,
    tile_x: u32,
    tile_y: u32,
    style: &CompiledStyle,
) -> Result<Array, JsValue> {
    let tile_address =
        validated_tile_address(tile_zoom_level, tile_x, tile_y).map_err(|failure_message| {
            failure_with_code(ERROR_CODE_INVALID_COORDINATE_INPUT, &failure_message)
        })?;
    let unwrapped_tile_bytes =
        unwrapped_vector_tile_bytes(tile_bytes, tile_address).map_err(|failure_message| {
            failure_with_code(ERROR_CODE_RESPONSE_PARSING_FAILURE, &failure_message)
        })?;
    let decoded_tile = decode_vector_tile(&unwrapped_tile_bytes).map_err(|decode_error| {
        failure_with_code(
            ERROR_CODE_RESPONSE_PARSING_FAILURE,
            &format!("the vector tile payload failed to decode: {decode_error}"),
        )
    })?;
    let overlay_descriptors = Array::new();
    for overlay in vector_tile_overlays(&decoded_tile, tile_address, style) {
        overlay_descriptors.push(&overlay_descriptor(&overlay)?);
    }
    Ok(overlay_descriptors)
}

/// The geographic bounds of one web-mercator tile, as
/// `{northWest: {latitude, longitude}, southEast: {latitude, longitude}}`.
///
/// A page uses this to focus its camera on a fetched tile, and tests use it
/// to check decoded overlay coordinates against the tile they came from. The
/// conversion is the same
/// [`tile_local_point_to_coordinate`] the overlay seam applies, so bounds and
/// geometry share one source of truth. An out-of-grid address rejects with a
/// coded error.
#[wasm_bindgen(js_name = vectorTileGeographicBounds)]
pub fn vector_tile_geographic_bounds(
    tile_zoom_level: u8,
    tile_x: u32,
    tile_y: u32,
) -> Result<Object, JsValue> {
    let tile_address =
        validated_tile_address(tile_zoom_level, tile_x, tile_y).map_err(|failure_message| {
            failure_with_code(ERROR_CODE_INVALID_COORDINATE_INPUT, &failure_message)
        })?;
    let north_west = tile_corner_coordinate(
        tile_address,
        TILE_NORTH_WEST_EXTENT_POINT,
        TILE_NORTH_WEST_EXTENT_POINT,
    );
    let south_east_extent_point = DEFAULT_TILE_EXTENT as i64;
    let south_east = tile_corner_coordinate(
        tile_address,
        south_east_extent_point,
        south_east_extent_point,
    );
    object_from_fields(&[
        (
            TILE_BOUNDS_FIELD_NORTH_WEST,
            geographic_coordinate_object(&north_west)?.into(),
        ),
        (
            TILE_BOUNDS_FIELD_SOUTH_EAST,
            geographic_coordinate_object(&south_east)?.into(),
        ),
    ])
}

/// Validates the tile address against the zoom grid (the shared
/// [`TileKey::new`] rule) and returns the style conversion's address.
fn validated_tile_address(
    tile_zoom_level: u8,
    tile_x: u32,
    tile_y: u32,
) -> Result<VectorTileAddress, String> {
    let tile_key = TileKey::new(tile_x, tile_y, tile_zoom_level)
        .map_err(|sdk_error| format!("invalid vector tile address: {sdk_error}"))?;
    Ok(VectorTileAddress {
        zoom_level: tile_key.zoom_level,
        tile_x: tile_key.tile_x,
        tile_y: tile_key.tile_y,
    })
}

/// Unwraps the wire envelope: the shared [`VectorTileData`] sniff classifies
/// the payload by its gzip magic number, and a gzip-wrapped body is
/// decompressed here so the decoder only ever sees plain MVT protobuf. An
/// empty payload is the shared record's own typed parsing failure.
fn unwrapped_vector_tile_bytes(
    wire_bytes: &[u8],
    tile_address: VectorTileAddress,
) -> Result<Vec<u8>, String> {
    let tile_data = VectorTileData::from_wire_bytes(
        tile_address.tile_x,
        tile_address.tile_y,
        tile_address.zoom_level,
        wire_bytes.to_vec(),
    )
    .map_err(|sdk_error| sdk_error.to_string())?;
    match tile_data.tile_encoding {
        VectorTileEncoding::MapboxVectorTile => Ok(tile_data.tile_bytes),
        VectorTileEncoding::GzippedMapboxVectorTile => {
            let gzip_decoder = flate2::read::GzDecoder::new(tile_data.tile_bytes.as_slice());
            let mut unwrapped_bytes = Vec::new();
            let mut limited_decoder =
                gzip_decoder.take(MAXIMUM_DECOMPRESSED_VECTOR_TILE_BYTES as u64 + 1);
            limited_decoder
                .read_to_end(&mut unwrapped_bytes)
                .map_err(|decompress_error| {
                    format!(
                        "the gzip-wrapped vector tile payload failed to decompress: \
                         {decompress_error}"
                    )
                })?;
            if unwrapped_bytes.len() > MAXIMUM_DECOMPRESSED_VECTOR_TILE_BYTES {
                return Err(format!(
                    "the gzip-wrapped vector tile payload decompressed beyond the {MAXIMUM_DECOMPRESSED_VECTOR_TILE_BYTES}-byte limit"
                ));
            }
            Ok(unwrapped_bytes)
        }
    }
}

/// Converts one extent-grid corner of the tile into its geographic corner.
fn tile_corner_coordinate(
    tile_address: VectorTileAddress,
    extent_x_coordinate: i64,
    extent_y_coordinate: i64,
) -> GeographicCoordinate {
    tile_local_point_to_coordinate(
        tile_address,
        DEFAULT_TILE_EXTENT,
        &VectorTileGeometryPoint {
            x_coordinate: extent_x_coordinate,
            y_coordinate: extent_y_coordinate,
        },
    )
}

/// Serializes one styled overlay into the `set_overlays` descriptor it came
/// from. Filled polygons cross as their exterior ring (the documented hole
/// limit at the module level); a traffic-flow overlay never comes from a
/// vector style and rejects rather than inventing a shape.
fn overlay_descriptor(overlay: &MapOverlay) -> Result<JsValue, JsValue> {
    match overlay {
        MapOverlay::RoutePolyline(route_overlay) => Ok(object_from_fields(&[
            (
                OVERLAY_FIELD_KIND,
                JsValue::from_str(OVERLAY_KIND_ROUTE_POLYLINE),
            ),
            (
                OVERLAY_FIELD_COORDINATES,
                coordinate_descriptor_array(&route_overlay.coordinates)?.into(),
            ),
            (
                OVERLAY_FIELD_COLOR,
                color_descriptor_object(route_overlay.rgba_color)?.into(),
            ),
            (
                OVERLAY_FIELD_WIDTH_PIXELS,
                JsValue::from(route_overlay.width_pixels),
            ),
        ])?
        .into()),
        MapOverlay::FilledPolygon(polygon_overlay) => Ok(object_from_fields(&[
            (
                OVERLAY_FIELD_KIND,
                JsValue::from_str(OVERLAY_KIND_FILLED_POLYGON),
            ),
            (
                OVERLAY_FIELD_RING_COORDINATES,
                coordinate_descriptor_array(&polygon_overlay.ring_coordinates)?.into(),
            ),
            (
                OVERLAY_FIELD_COLOR,
                color_descriptor_object(polygon_overlay.rgba_color)?.into(),
            ),
        ])?
        .into()),
        MapOverlay::PointMarkers(markers_overlay) => Ok(object_from_fields(&[
            (
                OVERLAY_FIELD_KIND,
                JsValue::from_str(OVERLAY_KIND_POINT_MARKERS),
            ),
            (
                OVERLAY_FIELD_POSITIONS,
                coordinate_descriptor_array(&markers_overlay.positions)?.into(),
            ),
            (
                OVERLAY_FIELD_COLOR,
                color_descriptor_object(markers_overlay.rgba_color)?.into(),
            ),
            (
                OVERLAY_FIELD_RADIUS_PIXELS,
                JsValue::from(markers_overlay.radius_pixels),
            ),
        ])?
        .into()),
        MapOverlay::PointPins(pins_overlay) => Ok(object_from_fields(&[
            (
                OVERLAY_FIELD_KIND,
                JsValue::from_str(OVERLAY_KIND_POINT_PINS),
            ),
            (
                OVERLAY_FIELD_POSITIONS,
                coordinate_descriptor_array(&pins_overlay.positions)?.into(),
            ),
            (
                OVERLAY_FIELD_COLOR,
                color_descriptor_object(pins_overlay.rgba_color)?.into(),
            ),
            (
                OVERLAY_FIELD_RADIUS_PIXELS,
                JsValue::from(pins_overlay.radius_pixels),
            ),
        ])?
        .into()),
        MapOverlay::TrafficFlow(_) => Err(failure(
            "a vector tile style never paints traffic-flow overlays",
        )),
    }
}

/// Builds a JavaScript array of `{latitude, longitude}` descriptor objects.
fn coordinate_descriptor_array(coordinates: &[GeographicCoordinate]) -> Result<Array, JsValue> {
    let coordinate_array = Array::new();
    for coordinate in coordinates {
        coordinate_array.push(&geographic_coordinate_object(coordinate)?.into());
    }
    Ok(coordinate_array)
}

/// Builds the `{red, green, blue, alpha}` color descriptor (channels in
/// `0.0..=1.0`, the `set_overlays` vocabulary).
fn color_descriptor_object(rgba_color: [f32; 4]) -> Result<Object, JsValue> {
    object_from_fields(&[
        (COLOR_FIELD_RED, JsValue::from(rgba_color[0])),
        (COLOR_FIELD_GREEN, JsValue::from(rgba_color[1])),
        (COLOR_FIELD_BLUE, JsValue::from(rgba_color[2])),
        (COLOR_FIELD_ALPHA, JsValue::from(rgba_color[3])),
    ])
}
