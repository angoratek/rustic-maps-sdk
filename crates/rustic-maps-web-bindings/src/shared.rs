//! Cross-platform plumbing shared by the wasm exports and the host tests:
//! error-to-string mapping for the core constructors the surface calls, and
//! the recorded-tile addressing used by the browser GPU canvas.

use geospatial_types::{GeographicCoordinate, RasterTileResolution};
use rustic_maps_rendering::{TileKey, ViewportPixels};

/// Highest zoom level of the recorded raster tile fixtures the local server
/// exposes at the same-origin `/tiles` route: the zoom-13 Sacramento parent
/// tile, the zoom-14 neighborhood block plus the wider footprint recorded
/// around it, and the zoom 15–17 corridor along the demo route.
pub(crate) const MAXIMUM_RECORDED_TILE_ZOOM_LEVEL: u8 = 17;

/// Same-origin URL prefix under which the local static server exposes the
/// recorded PNG tile fixtures.
pub(crate) const TILE_FIXTURE_URL_PREFIX: &str = "tiles/";

/// Filename marker distinguishing a recorded retina-resolution tile fixture
/// (the Tiles API's `@2x` form) from its standard-resolution sibling.
pub(crate) const RETINA_TILE_FILE_MARKER: &str = "@2x";

/// The same-origin URL a tile's recorded PNG payload is served from, at the
/// given pixel resolution.
pub(crate) fn tile_fixture_url(
    tile_key: &TileKey,
    tile_resolution: RasterTileResolution,
) -> String {
    let resolution_file_marker = match tile_resolution {
        RasterTileResolution::Standard => "",
        RasterTileResolution::Retina => RETINA_TILE_FILE_MARKER,
    };
    format!(
        "{TILE_FIXTURE_URL_PREFIX}{}_{}_{}{resolution_file_marker}.png",
        tile_key.zoom_level, tile_key.tile_x, tile_key.tile_y
    )
}

/// Clamps a camera's tile zoom level to the highest level the active tile
/// source can serve: the recorded fixtures' ceiling, or the ceiling a live
/// raster source declares. The canvas fetches at the returned level, so a deep
/// camera over unrecorded ground still asks for tiles the source has.
pub(crate) fn tile_fetch_zoom_level(
    camera_tile_zoom_level: u8,
    tile_source_maximum_zoom_level: u8,
) -> u8 {
    camera_tile_zoom_level.min(tile_source_maximum_zoom_level)
}

/// Builds a coordinate from decimal degrees, mapping the SDK error to the
/// detail text the JavaScript surface reports.
pub(crate) fn validated_coordinate(
    latitude: f64,
    longitude: f64,
) -> Result<GeographicCoordinate, String> {
    GeographicCoordinate::from_degrees(latitude, longitude)
        .map_err(|sdk_error| sdk_error.to_string())
}

/// Builds a bounding box from its corner degrees, mapping the SDK error to
/// the detail text the JavaScript surface reports.
pub(crate) fn validated_bounding_box(
    south_west_latitude: f64,
    south_west_longitude: f64,
    north_east_latitude: f64,
    north_east_longitude: f64,
) -> Result<geospatial_types::BoundingBox, String> {
    let south_west = validated_coordinate(south_west_latitude, south_west_longitude)?;
    let north_east = validated_coordinate(north_east_latitude, north_east_longitude)?;
    geospatial_types::BoundingBox::from_corners(south_west, north_east)
        .map_err(|sdk_error| sdk_error.to_string())
}

/// The extent a frame can actually present: the canvas' pixel size clamped to
/// the adapter's maximum two-dimensional texture dimension. A backing store
/// larger than the limit (an 8K display at a high device-pixel ratio) is
/// presented scaled, because configuring a surface above the limit would fail
/// every frame while the clamped surface keeps the map usable.
pub(crate) fn clamped_surface_extent(
    canvas_extent: (u32, u32),
    maximum_texture_dimension: u32,
) -> (u32, u32) {
    (
        canvas_extent.0.min(maximum_texture_dimension),
        canvas_extent.1.min(maximum_texture_dimension),
    )
}

/// Builds a viewport from pixel dimensions, mapping the SDK error to the
/// detail text the JavaScript surface reports.
pub(crate) fn validated_viewport_pixels(
    width_pixels: f64,
    height_pixels: f64,
) -> Result<ViewportPixels, String> {
    ViewportPixels::new(width_pixels, height_pixels).map_err(|sdk_error| sdk_error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{MAXIMUM_RECORDED_TILE_ZOOM_LEVEL, RasterTileResolution, TileKey};

    #[test]
    fn the_fetch_zoom_never_rises_above_the_tile_sources_ceiling() {
        // The camera's rounding passes through unchanged within the source's
        // range; deeper cameras clamp so the page never fetches past it. The
        // recorded fixtures stop at their own ceiling; a live source declares
        // a higher one and the same clamp applies.
        assert_eq!(
            super::tile_fetch_zoom_level(0, MAXIMUM_RECORDED_TILE_ZOOM_LEVEL),
            0
        );
        assert_eq!(
            super::tile_fetch_zoom_level(13, MAXIMUM_RECORDED_TILE_ZOOM_LEVEL),
            13
        );
        assert_eq!(
            super::tile_fetch_zoom_level(
                MAXIMUM_RECORDED_TILE_ZOOM_LEVEL,
                MAXIMUM_RECORDED_TILE_ZOOM_LEVEL
            ),
            MAXIMUM_RECORDED_TILE_ZOOM_LEVEL
        );
        assert_eq!(
            super::tile_fetch_zoom_level(24, MAXIMUM_RECORDED_TILE_ZOOM_LEVEL),
            MAXIMUM_RECORDED_TILE_ZOOM_LEVEL
        );
        assert_eq!(super::tile_fetch_zoom_level(14, 19), 14);
        assert_eq!(super::tile_fetch_zoom_level(19, 19), 19);
        assert_eq!(super::tile_fetch_zoom_level(22, 19), 19);
    }

    #[test]
    fn a_surface_extent_is_clamped_to_the_adapter_texture_limit() {
        assert_eq!(super::clamped_surface_extent((640, 480), 8192), (640, 480));
        assert_eq!(
            super::clamped_surface_extent((8192, 8192), 8192),
            (8192, 8192)
        );
        assert_eq!(
            super::clamped_surface_extent((10_240, 600), 8192),
            (8192, 600)
        );
        assert_eq!(
            super::clamped_surface_extent((600, 10_240), 8192),
            (600, 8192)
        );
        assert_eq!(super::clamped_surface_extent((0, 0), 8192), (0, 0));
    }

    #[test]
    fn a_recorded_tile_url_addresses_the_server_route() {
        let tile_key = TileKey::new(2662, 6286, 14).expect("a valid Sacramento tile address");
        assert_eq!(
            super::tile_fixture_url(&tile_key, RasterTileResolution::Standard),
            "tiles/14_2662_6286.png"
        );
        assert_eq!(
            super::tile_fixture_url(&tile_key, RasterTileResolution::Retina),
            "tiles/14_2662_6286@2x.png"
        );
        let root_tile = TileKey::new(0, 0, 0).expect("the world root tile address");
        assert_eq!(
            super::tile_fixture_url(&root_tile, RasterTileResolution::Standard),
            "tiles/0_0_0.png"
        );
    }
}
