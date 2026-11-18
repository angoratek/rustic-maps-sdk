//! Compile-time contract checks: every mock implements its trait via
//! `dyn`-dispatchable handles, proving the traits stay object-safe.

use geospatial_traits::mock::{
    MockDistanceMatrixProvider, MockElevationProvider, MockGeocodingProvider,
    MockOfflineRoutingProvider, MockRasterTileProvider, MockRoutingProvider,
    MockSearchPlaceProvider, MockTrafficFlowProvider, MockTrafficIncidentProvider,
};
use geospatial_traits::{
    DistanceMatrixProvider, ElevationProvider, GeocodingProvider, OfflineGraphSource,
    OfflineRoutingProvider, RasterTileProvider, RoutingProvider, SearchPlaceProvider,
    TrafficFlowProvider, TrafficIncidentProvider,
};
use geospatial_types::raster::{RasterImageFormat, RasterTileResolution};
use geospatial_types::route::RouteOptions;
use geospatial_types::{BoundingBox, GeographicCoordinate};
use std::sync::Arc;

fn coordinate(latitude: f64, longitude: f64) -> GeographicCoordinate {
    GeographicCoordinate::from_degrees(latitude, longitude).unwrap()
}

#[tokio::test]
async fn every_mock_dispatches_through_a_trait_object() {
    let geocoder: Arc<dyn GeocodingProvider> = Arc::new(MockGeocodingProvider);
    let router: Arc<dyn RoutingProvider> = Arc::new(MockRoutingProvider);
    let matrix: Arc<dyn DistanceMatrixProvider> = Arc::new(MockDistanceMatrixProvider);
    let search: Arc<dyn SearchPlaceProvider> = Arc::new(MockSearchPlaceProvider);
    let incidents: Arc<dyn TrafficIncidentProvider> = Arc::new(MockTrafficIncidentProvider);
    let flow: Arc<dyn TrafficFlowProvider> = Arc::new(MockTrafficFlowProvider);
    let elevation: Arc<dyn ElevationProvider> = Arc::new(MockElevationProvider);
    let raster: Arc<dyn RasterTileProvider> = Arc::new(MockRasterTileProvider);
    let offline: Arc<dyn OfflineRoutingProvider> = Arc::new(MockOfflineRoutingProvider);

    assert!(geocoder.forward_geocode("Sacramento").await.is_ok());
    assert!(
        router
            .calculate_route(
                &[coordinate(0.0, 0.0), coordinate(1.0, 1.0)],
                &RouteOptions::default()
            )
            .await
            .is_ok()
    );
    assert!(
        matrix
            .calculate_distance_matrix(
                &[coordinate(0.0, 0.0)],
                &[coordinate(1.0, 1.0)],
                geospatial_types::route::TransportationMode::Automobile
            )
            .await
            .is_ok()
    );
    assert!(search.search_autocomplete("Sacramento", None).await.is_ok());
    let bounding_box =
        BoundingBox::from_corners(coordinate(0.0, 0.0), coordinate(2.0, 2.0)).unwrap();
    assert!(
        incidents
            .fetch_traffic_incidents(&bounding_box)
            .await
            .is_ok()
    );
    assert!(flow.fetch_traffic_flow(&bounding_box).await.is_ok());
    assert!(
        elevation
            .fetch_elevation_points(&[coordinate(0.0, 0.0)])
            .await
            .is_ok()
    );
    assert!(
        raster
            .fetch_raster_tile(
                0,
                0,
                0,
                RasterImageFormat::Png,
                RasterTileResolution::Standard
            )
            .await
            .is_ok()
    );
    assert!(
        offline
            .load_offline_graph(OfflineGraphSource::Memory(vec![1, 2, 3]))
            .await
            .is_ok()
    );
    assert!(
        offline
            .calculate_offline_route(
                &coordinate(0.0, 0.0),
                &coordinate(1.0, 1.0),
                &RouteOptions::default()
            )
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn offline_mock_rejects_empty_memory_archives() {
    let offline = MockOfflineRoutingProvider;
    assert!(
        offline
            .load_offline_graph(OfflineGraphSource::Memory(vec![]))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn the_mock_serves_distinct_payloads_per_resolution() {
    // The mock's synthetic payload edge tracks the resolution (256x256
    // standard, 512x512 retina), so a retina fetch never answers with the
    // standard payload and dependent tests can observe the parameter flow.
    let raster = MockRasterTileProvider;
    let standard_tile = raster
        .fetch_raster_tile(
            3,
            7,
            5,
            RasterImageFormat::Png,
            RasterTileResolution::Standard,
        )
        .await
        .unwrap();
    let retina_tile = raster
        .fetch_raster_tile(
            3,
            7,
            5,
            RasterImageFormat::Png,
            RasterTileResolution::Retina,
        )
        .await
        .unwrap();
    assert_eq!(
        standard_tile.tile_resolution,
        RasterTileResolution::Standard
    );
    assert_eq!(retina_tile.tile_resolution, RasterTileResolution::Retina);
    assert_ne!(standard_tile.image_bytes, retina_tile.image_bytes);
    assert_eq!(standard_tile.image_bytes.len(), 256 * 256);
    assert_eq!(retina_tile.image_bytes.len(), 512 * 512);
}
