//! The simulated-trip contract seam: the immutable [`RouteSimulator`] over
//! one planned route.
//!
//! The traversal mathematics lives in `rustic-maps-navigation`'s
//! [`rustic_maps_navigation::RouteWalk`] so the WebAssembly surface and every native
//! shell share one implementation; this module only lowers the binding
//! records onto it and maps its refusals onto the contract's
//! [`SdkError::InvalidNavigationInput`]. The SDK carries no pacing policy:
//! the caller constructs the [`SimulatedTripPace`] it wants.

use rustic_maps_navigation::RouteWalk;

use crate::error::SdkError;
use crate::records::{Route, SimulatedTripFix, SimulatedTripPace};

/// An immutable simulated trip along the planned route's own geometry.
///
/// Construct one per trip with [`RouteSimulator::new`]; the two fix methods
/// then answer distance- or time-parameterised positions. All methods are
/// synchronous and cheap (the polyline is flattened once at construction),
/// and both fix parameters clamp instead of failing.
pub struct RouteSimulator {
    route_walk: RouteWalk,
}

impl RouteSimulator {
    /// Builds the simulator over `route` at `pace`.
    ///
    /// Refuses a route record whose step maneuver label the conversion cannot
    /// map, a pace the walker cannot use (non-finite or non-positive speed or
    /// multiplier, or an overflowing product), and a route whose legs and
    /// steps carry no geometry.
    pub fn new(route: Route, pace: SimulatedTripPace) -> Result<Self, SdkError> {
        let native_route = route.into_native_route_data()?;
        let route_walk =
            RouteWalk::new(&native_route, pace.to_native_pace()).map_err(|route_walk_error| {
                SdkError::InvalidNavigationInput {
                    detail: route_walk_error.to_string(),
                }
            })?;
        Ok(Self { route_walk })
    }

    /// Total length of the route's flattened polyline, in meters.
    pub fn total_distance_meters(&self) -> f64 {
        self.route_walk.total_distance_meters()
    }

    /// Total simulated travel time at the chosen pace, in seconds.
    pub fn total_duration_seconds(&self) -> f64 {
        self.route_walk.total_duration_seconds()
    }

    /// The fix at `travelled_meters` from the route start, clamped into
    /// `[0, total_distance_meters()]`.
    pub fn fix_at_travelled_meters(&self, travelled_meters: f64) -> SimulatedTripFix {
        SimulatedTripFix::from_native_fix(
            &self.route_walk.fix_at_travelled_meters(travelled_meters),
        )
    }

    /// The fix at `elapsed_seconds` of simulated travel, clamped into
    /// `[0, total_duration_seconds()]`.
    pub fn fix_at_elapsed_seconds(&self, elapsed_seconds: f64) -> SimulatedTripFix {
        SimulatedTripFix::from_native_fix(&self.route_walk.fix_at_elapsed_seconds(elapsed_seconds))
    }
}

#[cfg(test)]
mod tests {
    use super::RouteSimulator;
    use crate::error::SdkError;
    use crate::records::{
        BoundingBox, GeographicCoordinate, Route, RouteLeg, RouteStep, SimulatedTripPace,
    };

    /// Latitude of the synthetic fixture origin, a neutral mid-latitude.
    const TEST_ORIGIN_LATITUDE_DEGREES: f64 = 47.0;
    /// Longitude of the synthetic fixture origin.
    const TEST_ORIGIN_LONGITUDE_DEGREES: f64 = 8.0;
    /// Earth's mean radius, matching the haversine constant.
    const EARTH_RADIUS_METERS: f64 = 6_371_008.8;
    /// Eastward length of each synthetic step, in meters.
    const TEST_STEP_LENGTH_METERS: f64 = 100.0;
    /// Pace used by the seam tests, in meters per second.
    const TEST_SPEED_METERS_PER_SECOND: f64 = 2.0;
    /// Pace multiplier used by the seam tests.
    const TEST_SPEED_MULTIPLIER: f64 = 4.0;

    fn coordinate_east_of(east_meters: f64) -> GeographicCoordinate {
        GeographicCoordinate {
            latitude: TEST_ORIGIN_LATITUDE_DEGREES,
            longitude: TEST_ORIGIN_LONGITUDE_DEGREES
                + (east_meters
                    / (EARTH_RADIUS_METERS * TEST_ORIGIN_LATITUDE_DEGREES.to_radians().cos()))
                .to_degrees(),
        }
    }

    fn route_step(start_east_meters: f64) -> RouteStep {
        RouteStep {
            distance_meters: TEST_STEP_LENGTH_METERS,
            duration_seconds: 10.0,
            maneuver_type: "continue".to_string(),
            instruction_display_text: "Continue".to_string(),
            instruction_speech_text: "Continue".to_string(),
            shape: vec![
                coordinate_east_of(start_east_meters),
                coordinate_east_of(start_east_meters + TEST_STEP_LENGTH_METERS),
            ],
        }
    }

    fn straight_route() -> Route {
        let steps = [0.0, TEST_STEP_LENGTH_METERS]
            .into_iter()
            .map(route_step)
            .collect();
        Route {
            distance_meters: TEST_STEP_LENGTH_METERS * 2.0,
            duration_seconds: 20.0,
            bounding_box: BoundingBox {
                south_west: coordinate_east_of(0.0),
                north_east: coordinate_east_of(TEST_STEP_LENGTH_METERS * 2.0),
            },
            geometry: Vec::new(),
            legs: vec![RouteLeg {
                distance_meters: TEST_STEP_LENGTH_METERS * 2.0,
                duration_seconds: 20.0,
                steps,
            }],
        }
    }

    fn test_pace() -> SimulatedTripPace {
        SimulatedTripPace {
            speed_meters_per_second: TEST_SPEED_METERS_PER_SECOND,
            speed_multiplier: TEST_SPEED_MULTIPLIER,
        }
    }

    #[test]
    fn simulator_answers_totals_and_both_fix_parameters() {
        let simulator = RouteSimulator::new(straight_route(), test_pace())
            .expect("the synthetic route and pace are usable");

        let expected_distance = TEST_STEP_LENGTH_METERS * 2.0;
        let effective_speed = TEST_SPEED_METERS_PER_SECOND * TEST_SPEED_MULTIPLIER;
        assert!((simulator.total_distance_meters() - expected_distance).abs() < 1e-6);
        assert!(
            (simulator.total_duration_seconds() - expected_distance / effective_speed).abs() < 1e-9
        );

        let start_fix = simulator.fix_at_travelled_meters(0.0);
        assert_eq!(start_fix.travelled_meters, 0.0);
        assert!(!start_fix.is_arrived);
        let heading = start_fix
            .course_heading_degrees
            .expect("an eastward segment carries a bearing");
        assert!((heading - 90.0).abs() < 1e-6);

        let final_fix = simulator.fix_at_travelled_meters(expected_distance);
        assert!(final_fix.is_arrived);
        assert_eq!(final_fix.remaining_meters, 0.0);
        assert_eq!(final_fix.coordinate, coordinate_east_of(expected_distance));

        let by_elapsed = simulator.fix_at_elapsed_seconds(expected_distance / effective_speed);
        assert!(by_elapsed.is_arrived);
        assert_eq!(by_elapsed.coordinate, final_fix.coordinate);
    }

    #[test]
    fn simulator_maps_walker_refusals_onto_the_navigation_error() {
        let invalid_pace = SimulatedTripPace {
            speed_meters_per_second: 0.0,
            speed_multiplier: 1.0,
        };
        assert!(matches!(
            RouteSimulator::new(straight_route(), invalid_pace),
            Err(SdkError::InvalidNavigationInput { .. })
        ));

        let empty_route = Route {
            distance_meters: 0.0,
            duration_seconds: 0.0,
            bounding_box: BoundingBox {
                south_west: coordinate_east_of(0.0),
                north_east: coordinate_east_of(0.0),
            },
            geometry: Vec::new(),
            legs: Vec::new(),
        };
        assert!(matches!(
            RouteSimulator::new(empty_route, test_pace()),
            Err(SdkError::InvalidNavigationInput { .. })
        ));
    }
}
