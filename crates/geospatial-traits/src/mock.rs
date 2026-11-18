//! Deterministic mock provider implementations for tests.
//!
//! Each mock returns canned records, so a test can exercise code that depends
//! on a provider contract without an HTTP stack or any network access. All
//! mocks are stateless and cheap to construct.

use async_trait::async_trait;
use geospatial_types::raster::{RasterImageFormat, RasterTileData, RasterTileResolution};
use geospatial_types::route::{DistanceMatrix, DistanceMatrixEntry};
use geospatial_types::{
    BoundingBox, ElevationPoint, GeocodeQuality, GeographicCoordinate, PlaceResult, RouteData,
    RouteLeg, RouteOptions, RouteStep, SDKError, TrafficFlowData, TrafficIncident, VectorTileData,
    VectorTileEncoding,
};

use crate::{
    DistanceMatrixProvider, ElevationProvider, GeocodingProvider, OfflineGraphSource,
    OfflineRoutingProvider, RasterTileProvider, RoutingProvider, SearchPlaceProvider,
    TrafficFlowProvider, TrafficIncidentProvider, VectorTileProvider, VectorTileSourceDescriptor,
};

/// Geocoding mock returning one canned city result.
#[derive(Debug, Default)]
pub struct MockGeocodingProvider;

#[async_trait]
impl GeocodingProvider for MockGeocodingProvider {
    async fn forward_geocode(&self, address_text: &str) -> Result<Vec<PlaceResult>, SDKError> {
        Ok(vec![PlaceResult {
            structured_address_text: address_text.to_string(),
            coordinate: GeographicCoordinate::from_degrees(38.5766, -121.4944).unwrap(),
            geocode_quality: GeocodeQuality::City,
        }])
    }

    async fn reverse_geocode(
        &self,
        coordinate: &GeographicCoordinate,
    ) -> Result<Vec<PlaceResult>, SDKError> {
        Ok(vec![PlaceResult {
            structured_address_text: format!(
                "Reverse result at {:.4}, {:.4}",
                coordinate.latitude, coordinate.longitude
            ),
            coordinate: *coordinate,
            geocode_quality: GeocodeQuality::Street,
        }])
    }
}

/// Routing mock returning a two-step straight line between the waypoints.
#[derive(Debug, Default)]
pub struct MockRoutingProvider;

#[async_trait]
impl RoutingProvider for MockRoutingProvider {
    async fn calculate_route(
        &self,
        waypoints: &[GeographicCoordinate],
        _route_options: &RouteOptions,
    ) -> Result<RouteData, SDKError> {
        debug_assert!(!waypoints.is_empty());
        Ok(RouteData {
            distance_meters: 1000.0,
            duration_seconds: 90.0,
            bounding_box: BoundingBox::from_corners(waypoints[0], waypoints[waypoints.len() - 1])
                .unwrap(),
            geometry: Vec::new(),
            legs: vec![RouteLeg {
                distance_meters: 1000.0,
                duration_seconds: 90.0,
                steps: vec![RouteStep {
                    distance_meters: 1000.0,
                    duration_seconds: 90.0,
                    maneuver_type: geospatial_types::route::ManeuverType::Departure,
                    instruction_display_text: "Head out".to_string(),
                    instruction_speech_text: "Head out".to_string(),
                    shape: waypoints.to_vec(),
                    lane_guidances: vec![],
                }],
            }],
        })
    }
}

/// Distance-matrix mock filling a uniform diagonal-free matrix.
#[derive(Debug, Default)]
pub struct MockDistanceMatrixProvider;

#[async_trait]
impl DistanceMatrixProvider for MockDistanceMatrixProvider {
    async fn calculate_distance_matrix(
        &self,
        origins: &[GeographicCoordinate],
        destinations: &[GeographicCoordinate],
        _transportation_mode: geospatial_types::route::TransportationMode,
    ) -> Result<DistanceMatrix, SDKError> {
        let entries = origins
            .iter()
            .enumerate()
            .flat_map(|(origin_index, _)| {
                destinations
                    .iter()
                    .enumerate()
                    .map(move |(destination_index, _)| DistanceMatrixEntry {
                        origin_index: origin_index as u32,
                        destination_index: destination_index as u32,
                        distance_meters: 500.0,
                        duration_seconds: 45.0,
                    })
            })
            .collect();
        Ok(DistanceMatrix { entries })
    }
}

/// Search mock echoing the query back as one completion candidate.
#[derive(Debug, Default)]
pub struct MockSearchPlaceProvider;

#[async_trait]
impl SearchPlaceProvider for MockSearchPlaceProvider {
    async fn search_autocomplete(
        &self,
        query_text: &str,
        _search_boundary: Option<BoundingBox>,
    ) -> Result<Vec<PlaceResult>, SDKError> {
        Ok(vec![PlaceResult {
            structured_address_text: query_text.to_string(),
            coordinate: GeographicCoordinate::from_degrees(0.0, 0.0).unwrap(),
            geocode_quality: GeocodeQuality::City,
        }])
    }

    async fn search_places(
        &self,
        query_text: &str,
        search_boundary: Option<BoundingBox>,
    ) -> Result<Vec<PlaceResult>, SDKError> {
        self.search_autocomplete(query_text, search_boundary).await
    }
}

/// Incident mock emitting one canned accident.
#[derive(Debug, Default)]
pub struct MockTrafficIncidentProvider;

#[async_trait]
impl TrafficIncidentProvider for MockTrafficIncidentProvider {
    async fn fetch_traffic_incidents(
        &self,
        bounding_box: &BoundingBox,
    ) -> Result<Vec<TrafficIncident>, SDKError> {
        Ok(vec![TrafficIncident {
            identifier: "mock-incident-1".to_string(),
            incident_type: geospatial_types::TrafficIncidentType::Accident,
            severity: geospatial_types::TrafficIncidentSeverity::Medium,
            short_description_text: "Mock accident".to_string(),
            start_coordinate: bounding_box.south_west,
            end_coordinate: None,
        }])
    }
}

/// Flow mock emitting one free-flow observation.
#[derive(Debug, Default)]
pub struct MockTrafficFlowProvider;

#[async_trait]
impl TrafficFlowProvider for MockTrafficFlowProvider {
    async fn fetch_traffic_flow(
        &self,
        bounding_box: &BoundingBox,
    ) -> Result<Vec<TrafficFlowData>, SDKError> {
        Ok(vec![TrafficFlowData::from_speeds(
            bounding_box.south_west,
            60.0,
            62.5,
        )])
    }
}

/// Elevation mock reporting constant 500 m everywhere.
#[derive(Debug, Default)]
pub struct MockElevationProvider;

#[async_trait]
impl ElevationProvider for MockElevationProvider {
    async fn fetch_elevation_points(
        &self,
        points: &[GeographicCoordinate],
    ) -> Result<Vec<ElevationPoint>, SDKError> {
        Ok(points
            .iter()
            .map(|coordinate| ElevationPoint {
                coordinate: *coordinate,
                elevation_meters: 500.0,
            })
            .collect())
    }
}

/// Raster-tile mock producing a deterministic synthetic payload per XYZ key
/// and resolution.
///
/// Standard tiles carry a 256x256 texel payload and retina tiles a 512x512
/// one, so a caller can verify that a requested resolution reaches the
/// provider.
#[derive(Debug, Default)]
pub struct MockRasterTileProvider;

/// Edge of the synthetic 256x256 standard-resolution tile payload.
const MOCK_STANDARD_TILE_EDGE_PIXELS: u32 = 256;
/// Edge of the synthetic 512x512 retina-resolution tile payload.
const MOCK_RETINA_TILE_EDGE_PIXELS: u32 = 512;

#[async_trait]
impl RasterTileProvider for MockRasterTileProvider {
    async fn fetch_raster_tile(
        &self,
        tile_x: u32,
        tile_y: u32,
        zoom_level: u8,
        image_format: RasterImageFormat,
        tile_resolution: RasterTileResolution,
    ) -> Result<RasterTileData, SDKError> {
        let edge_pixels = match tile_resolution {
            RasterTileResolution::Standard => MOCK_STANDARD_TILE_EDGE_PIXELS,
            RasterTileResolution::Retina => MOCK_RETINA_TILE_EDGE_PIXELS,
        };
        Ok(RasterTileData {
            tile_x,
            tile_y,
            zoom_level,
            image_format,
            tile_resolution,
            image_bytes: vec![
                tile_y as u8;
                usize::try_from(edge_pixels * edge_pixels)
                    .expect("the synthetic tile payload edge product fits")
            ],
        })
    }
}

/// Offline-routing mock that accepts any non-empty source and returns a
/// canned route.
#[derive(Debug, Default)]
pub struct MockOfflineRoutingProvider;

#[async_trait]
impl OfflineRoutingProvider for MockOfflineRoutingProvider {
    async fn load_offline_graph(&self, graph_source: OfflineGraphSource) -> Result<(), SDKError> {
        if matches!(graph_source, OfflineGraphSource::Memory(ref bytes) if bytes.is_empty()) {
            return Err(SDKError::OfflineRoutingFailure(
                geospatial_types::OfflineGraphInvalidReason::ArchiveMalformed,
            ));
        }
        Ok(())
    }

    async fn calculate_offline_route(
        &self,
        start: &GeographicCoordinate,
        end: &GeographicCoordinate,
        _route_options: &RouteOptions,
    ) -> Result<RouteData, SDKError> {
        Ok(RouteData {
            distance_meters: 2500.0,
            duration_seconds: 180.0,
            bounding_box: BoundingBox::from_corners(*start, *end).unwrap(),
            geometry: Vec::new(),
            legs: vec![],
        })
    }
}

/// Vector-tile mock returning a minimal deterministic MVT-shaped payload.
///
/// The payload starts with an MVT protobuf `layers` field key but carries no
/// real decoding semantics; it is enough for callers that check tile identity
/// and encoding, not geometry.
#[derive(Debug, Default)]
pub struct MockVectorTileProvider;

/// Source name the vector mock reports through its source descriptor.
const MOCK_VECTOR_TILE_SOURCE_NAME: &str = "mock-vector-tiles";
/// Attribution line the vector mock reports.
const MOCK_VECTOR_TILE_ATTRIBUTION_TEXT: &str = "Mock vector attribution";
/// Highest zoom level the vector mock accepts.
const MOCK_VECTOR_TILE_MAXIMUM_ZOOM_LEVEL: u8 = 30;
/// Protobuf key for MVT's repeated `layers` field (field 3, wire type 2).
const MOCK_MVT_LAYER_FIELD_PROTOBUF_KEY: u8 = 0x1a;

#[async_trait]
impl VectorTileProvider for MockVectorTileProvider {
    fn source_descriptor(&self) -> VectorTileSourceDescriptor {
        VectorTileSourceDescriptor {
            source_name: MOCK_VECTOR_TILE_SOURCE_NAME.to_string(),
            attribution_text: MOCK_VECTOR_TILE_ATTRIBUTION_TEXT.to_string(),
            minimum_zoom_level: 0,
            maximum_zoom_level: MOCK_VECTOR_TILE_MAXIMUM_ZOOM_LEVEL,
            style_hints: Vec::new(),
        }
    }

    async fn fetch_vector_tile(
        &self,
        tile_x: u32,
        tile_y: u32,
        zoom_level: u8,
    ) -> Result<VectorTileData, SDKError> {
        Ok(VectorTileData {
            tile_x,
            tile_y,
            zoom_level,
            tile_encoding: VectorTileEncoding::MapboxVectorTile,
            tile_bytes: vec![MOCK_MVT_LAYER_FIELD_PROTOBUF_KEY, 0],
        })
    }
}
