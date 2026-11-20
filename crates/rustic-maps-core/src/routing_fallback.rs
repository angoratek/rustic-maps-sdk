//! Offline route fallback.
//!
//! [`OfflineFallbackRoutingProvider`] wraps a primary routing provider and
//! retries a failed request over a configured [`OfflineRoutingProvider`],
//! joining the per-pair answers into one [`RouteData`].
//!
//! Register the wrapper in the registry's routing slot like any other routing
//! provider. Applications keep using the ordinary [`RoutingProvider`]
//! contract and need no special handling for offline fallback.

use std::sync::Arc;

use async_trait::async_trait;
use geospatial_traits::{OfflineRoutingProvider, RoutingProvider};
use geospatial_types::route::{RouteCapabilities, RouteData, RouteLeg, RouteOptions};
use geospatial_types::{BoundingBox, GeographicCoordinate, SDKError};

/// Diagnostic target of the fallback's tracing events.
pub const ROUTING_FALLBACK_TRACING_TARGET: &str = "routing_fallback";

/// A point-to-point offline route needs at least a start and an end;
/// requests below this waypoint count are rejected without a fallback
/// retry.
const MINIMUM_ROUTE_WAYPOINT_COUNT: usize = 2;

/// Wraps a primary routing provider with an offline retry.
///
/// When the primary provider fails for a request with at least two
/// waypoints, each consecutive waypoint pair is routed over the loaded
/// offline graph and the answers are joined into one route: one leg per pair,
/// summed distance and duration, and the union bounding box.
///
/// Behavior:
///
/// - A successful primary response is returned unchanged, and the offline
///   provider is not consulted.
/// - A request with fewer than two waypoints has no point-to-point offline
///   equivalent, so it is rejected with
///   [`SDKError::ProviderRequestRejected`] without a fallback attempt.
/// - If the offline retry also fails, that failure is logged and returned. It
///   describes the last failure, matching how a primary failure reaches the
///   caller.
pub struct OfflineFallbackRoutingProvider {
    primary_routing_provider: Arc<dyn RoutingProvider>,
    offline_routing_provider: Arc<dyn OfflineRoutingProvider>,
}

impl OfflineFallbackRoutingProvider {
    /// Builds the fallback wrapper from the two configured providers.
    ///
    /// # Parameters
    /// - `primary_routing_provider` — the online provider tried first.
    /// - `offline_routing_provider` — the provider consulted only after a
    ///   primary failure.
    ///
    /// # Returns
    /// A routing provider ready to register in the provider registry.
    #[must_use]
    pub fn new(
        primary_routing_provider: Arc<dyn RoutingProvider>,
        offline_routing_provider: Arc<dyn OfflineRoutingProvider>,
    ) -> Self {
        Self {
            primary_routing_provider,
            offline_routing_provider,
        }
    }
}

#[async_trait]
impl RoutingProvider for OfflineFallbackRoutingProvider {
    /// The fallback serves the same choices as the primary provider on the
    /// happy path; the offline engine answers only when the primary fails.
    fn route_capabilities(&self) -> RouteCapabilities {
        self.primary_routing_provider.route_capabilities()
    }

    async fn calculate_route(
        &self,
        waypoints: &[GeographicCoordinate],
        route_options: &RouteOptions,
    ) -> Result<RouteData, SDKError> {
        // A successful primary answer is returned untouched; the offline
        // provider is never consulted on the happy path.
        if let Ok(primary_route) = self
            .primary_routing_provider
            .calculate_route(waypoints, route_options)
            .await
        {
            return Ok(primary_route);
        }
        // Below a start-plus-end pair there is no offline point-to-point
        // retry to attempt: the request itself has no routeable segment, so
        // the failure is rejected before any fallback work.
        if waypoints.len() < MINIMUM_ROUTE_WAYPOINT_COUNT {
            return Err(SDKError::ProviderRequestRejected(
                "an offline routing retry needs a request with at least a start and an end point"
                    .to_string(),
            ));
        }
        tracing::warn!(
            target: ROUTING_FALLBACK_TRACING_TARGET,
            waypoint_count = waypoints.len(),
            "primary routing request failed; retrying over the offline routing graph"
        );
        let mut combined_legs = Vec::<RouteLeg>::with_capacity(waypoints.len() - 1);
        let mut combined_bounding_box = None;
        let mut total_distance_meters = 0.0;
        let mut total_duration_seconds = 0.0;
        for waypoint_pair in waypoints.windows(MINIMUM_ROUTE_WAYPOINT_COUNT) {
            let offline_route = match self
                .offline_routing_provider
                .calculate_offline_route(&waypoint_pair[0], &waypoint_pair[1], route_options)
                .await
            {
                Ok(offline_route) => offline_route,
                Err(offline_error) => {
                    tracing::warn!(
                        target: ROUTING_FALLBACK_TRACING_TARGET,
                        ?offline_error,
                        "offline routing fallback also failed"
                    );
                    return Err(offline_error);
                }
            };
            total_distance_meters += offline_route.distance_meters;
            total_duration_seconds += offline_route.duration_seconds;
            combined_bounding_box = Some(match combined_bounding_box {
                Some(previous_box) => {
                    union_bounding_boxes(&previous_box, &offline_route.bounding_box)
                }
                None => offline_route.bounding_box,
            });
            combined_legs.extend(offline_route.legs);
        }
        Ok(RouteData {
            distance_meters: total_distance_meters,
            duration_seconds: total_duration_seconds,
            bounding_box: combined_bounding_box
                .expect("at least one offline route was joined, so a bounding box exists"),
            geometry: Vec::new(),
            legs: combined_legs,
        })
    }
}

/// Merges two bounding boxes into the smallest enclosing one.
fn union_bounding_boxes(first_box: &BoundingBox, second_box: &BoundingBox) -> BoundingBox {
    BoundingBox {
        south_west: GeographicCoordinate {
            latitude: first_box
                .south_west
                .latitude
                .min(second_box.south_west.latitude),
            longitude: first_box
                .south_west
                .longitude
                .min(second_box.south_west.longitude),
        },
        north_east: GeographicCoordinate {
            latitude: first_box
                .north_east
                .latitude
                .max(second_box.north_east.latitude),
            longitude: first_box
                .north_east
                .longitude
                .max(second_box.north_east.longitude),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use geospatial_types::route::{AvoidOptions, RouteStrategy, TransportationMode, UnitSystem};
    use rustic_maps_test_support::validated_coordinate as coordinate;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn successful_primary_answer_skips_the_offline_fallback() {
        let primary_route = route_data_between(
            &coordinate(0.0, 0.0),
            &coordinate(1.0, 0.0),
            PRIMARY_LEG_DISTANCE_METERS,
        );
        let offline_provider = RecordingOfflineProvider::succeeding();
        let fallback_provider = OfflineFallbackRoutingProvider::new(
            Arc::new(SucceedingRoutingProvider {
                route: primary_route.clone(),
            }),
            Arc::new(offline_provider.clone()),
        );
        let calculated_route = fallback_provider
            .calculate_route(&test_waypoints(), &test_route_options())
            .await
            .expect("the primary answer passes through");
        assert_eq!(calculated_route, primary_route);
        assert_eq!(offline_provider.call_count(), 0);
    }

    #[tokio::test]
    async fn failed_primary_retries_over_the_offline_fallback_and_joins_legs() {
        let offline_provider = RecordingOfflineProvider::succeeding();
        let fallback_provider = OfflineFallbackRoutingProvider::new(
            Arc::new(FailingRoutingProvider),
            Arc::new(offline_provider.clone()),
        );
        let calculated_route = fallback_provider
            .calculate_route(&test_waypoints(), &test_route_options())
            .await
            .expect("the offline retry covers the primary failure");
        // One leg per consecutive waypoint pair, with summed totals and the
        // union bounding box across both offline answers.
        assert_eq!(calculated_route.legs.len(), 2);
        assert_eq!(
            calculated_route.distance_meters,
            2.0 * OFFLINE_LEG_DISTANCE_METERS
        );
        assert_eq!(
            calculated_route.duration_seconds,
            2.0 * OFFLINE_LEG_DURATION_SECONDS
        );
        assert_eq!(
            calculated_route.bounding_box,
            BoundingBox {
                south_west: coordinate(0.0, 0.0),
                north_east: coordinate(2.0, 0.0),
            }
        );
        assert_eq!(offline_provider.call_count(), 2);
    }

    #[tokio::test]
    async fn two_waypoint_fallback_keeps_the_single_offline_leg() {
        let offline_provider = RecordingOfflineProvider::succeeding();
        let fallback_provider = OfflineFallbackRoutingProvider::new(
            Arc::new(FailingRoutingProvider),
            Arc::new(offline_provider),
        );
        let calculated_route = fallback_provider
            .calculate_route(
                &[coordinate(0.0, 0.0), coordinate(1.0, 0.0)],
                &test_route_options(),
            )
            .await
            .expect("the single offline leg covers the pair");
        assert_eq!(calculated_route.legs.len(), 1);
        assert_eq!(
            calculated_route.bounding_box,
            BoundingBox {
                south_west: coordinate(0.0, 0.0),
                north_east: coordinate(1.0, 0.0),
            }
        );
    }

    #[tokio::test]
    async fn offline_fallback_failure_is_returned_after_the_primary_failure() {
        let offline_error = SDKError::ProviderRequestRejected(
            "the offline routing graph cannot connect the request points".to_string(),
        );
        let offline_provider = RecordingOfflineProvider::failing(offline_error.clone());
        let fallback_provider = OfflineFallbackRoutingProvider::new(
            Arc::new(FailingRoutingProvider),
            Arc::new(offline_provider),
        );
        let calculated_result = fallback_provider
            .calculate_route(&test_waypoints(), &test_route_options())
            .await;
        assert_eq!(calculated_result, Err(offline_error));
    }

    #[tokio::test]
    async fn single_waypoint_request_is_rejected_without_a_fallback_attempt() {
        let offline_provider = RecordingOfflineProvider::succeeding();
        let fallback_provider = OfflineFallbackRoutingProvider::new(
            Arc::new(FailingRoutingProvider),
            Arc::new(offline_provider.clone()),
        );
        let calculated_result = fallback_provider
            .calculate_route(&[coordinate(0.0, 0.0)], &test_route_options())
            .await;
        assert!(calculated_result.is_err());
        assert_eq!(offline_provider.call_count(), 0);
    }

    // -- test doubles ------------------------------------------------------

    const PRIMARY_LEG_DISTANCE_METERS: f64 = 42.0;
    const OFFLINE_LEG_DISTANCE_METERS: f64 = 500.0;
    const OFFLINE_LEG_DURATION_SECONDS: f64 = 60.0;

    fn test_waypoints() -> Vec<GeographicCoordinate> {
        vec![
            coordinate(0.0, 0.0),
            coordinate(1.0, 0.0),
            coordinate(2.0, 0.0),
        ]
    }

    fn test_route_options() -> RouteOptions {
        RouteOptions {
            transportation_mode: TransportationMode::Automobile,
            route_strategy: RouteStrategy::Fastest,
            unit_system: UnitSystem::Metric,
            avoid_options: AvoidOptions::default(),
            departure_timestamp_seconds: None,
        }
    }

    fn route_data_between(
        start: &GeographicCoordinate,
        end: &GeographicCoordinate,
        distance_meters: f64,
    ) -> RouteData {
        RouteData {
            distance_meters,
            duration_seconds: OFFLINE_LEG_DURATION_SECONDS,
            bounding_box: BoundingBox::from_corners(*start, *end)
                .expect("the test corners are ordered"),
            geometry: Vec::new(),
            legs: vec![RouteLeg {
                distance_meters,
                duration_seconds: OFFLINE_LEG_DURATION_SECONDS,
                steps: Vec::new(),
            }],
        }
    }

    struct SucceedingRoutingProvider {
        route: RouteData,
    }

    #[async_trait::async_trait]
    impl RoutingProvider for SucceedingRoutingProvider {
        async fn calculate_route(
            &self,
            _waypoints: &[GeographicCoordinate],
            _route_options: &RouteOptions,
        ) -> Result<RouteData, SDKError> {
            Ok(self.route.clone())
        }
    }

    struct FailingRoutingProvider;

    #[async_trait::async_trait]
    impl RoutingProvider for FailingRoutingProvider {
        async fn calculate_route(
            &self,
            _waypoints: &[GeographicCoordinate],
            _route_options: &RouteOptions,
        ) -> Result<RouteData, SDKError> {
            Err(SDKError::ProviderRequestRejected(
                "the primary provider is unavailable".to_string(),
            ))
        }
    }

    #[derive(Clone)]
    struct RecordingOfflineProvider {
        call_count: Arc<AtomicUsize>,
        outcome: Option<SDKError>,
    }

    impl RecordingOfflineProvider {
        fn succeeding() -> Self {
            Self {
                call_count: Arc::new(AtomicUsize::new(0)),
                outcome: None,
            }
        }

        fn failing(failure: SDKError) -> Self {
            Self {
                call_count: Arc::new(AtomicUsize::new(0)),
                outcome: Some(failure),
            }
        }

        fn call_count(&self) -> usize {
            self.call_count.load(Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl OfflineRoutingProvider for RecordingOfflineProvider {
        async fn load_offline_graph(
            &self,
            _graph_source: geospatial_traits::OfflineGraphSource,
        ) -> Result<(), SDKError> {
            Ok(())
        }

        async fn calculate_offline_route(
            &self,
            start: &GeographicCoordinate,
            end: &GeographicCoordinate,
            _route_options: &RouteOptions,
        ) -> Result<RouteData, SDKError> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            match &self.outcome {
                Some(failure) => Err(failure.clone()),
                None => Ok(route_data_between(start, end, OFFLINE_LEG_DISTANCE_METERS)),
            }
        }
    }
}
