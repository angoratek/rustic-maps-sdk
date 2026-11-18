//! A self-contained walk over a synthetic route, demonstrating the controller
//! and the simulated source without any platform API.
//!
//! Run with `cargo run -p rustic-maps-positioning --example simulated_walk`.

use geospatial_types::{
    BoundingBox, GeographicCoordinate, ManeuverType, RouteData, RouteLeg, RouteStep,
};
use rustic_maps_navigation::SimulatedTripPace;
use rustic_maps_positioning::{PositionController, SimulatedPositionSource};

fn coordinate(latitude: f64, longitude: f64) -> GeographicCoordinate {
    GeographicCoordinate {
        latitude,
        longitude,
    }
}

fn route() -> RouteData {
    let shape = vec![
        coordinate(38.5769, -121.4950),
        coordinate(38.5765, -121.4947),
        coordinate(38.5761, -121.4944),
    ];
    RouteData {
        distance_meters: 0.0,
        duration_seconds: 0.0,
        bounding_box: BoundingBox {
            south_west: coordinate(38.5761, -121.4950),
            north_east: coordinate(38.5769, -121.4944),
        },
        geometry: Vec::new(),
        legs: vec![RouteLeg {
            distance_meters: 0.0,
            duration_seconds: 0.0,
            steps: vec![RouteStep {
                distance_meters: 0.0,
                duration_seconds: 0.0,
                maneuver_type: ManeuverType::Continue,
                instruction_display_text: String::new(),
                instruction_speech_text: String::new(),
                shape,
                lane_guidances: Vec::new(),
            }],
        }],
    }
}

fn main() {
    let source = SimulatedPositionSource::new(
        &route(),
        SimulatedTripPace {
            speed_meters_per_second: 1.4,
            speed_multiplier: 1.0,
        },
        5.0,
    )
    .expect("the example route walks");
    let mut controller = PositionController::new();
    let total_seconds = source.total_duration_seconds();
    let mut elapsed_seconds = 0.0;
    while elapsed_seconds <= total_seconds {
        let fix = source.fix_at_elapsed_seconds(elapsed_seconds);
        controller
            .feed_fix(fix)
            .expect("the simulated fixes are valid");
        elapsed_seconds += 1.0;
    }
    let latest_fix = controller.latest_fix().expect("the walk delivered fixes");
    println!(
        "walked {} s; last fix {:.5}, {:.5}; smoothed course {:?}",
        total_seconds,
        latest_fix.coordinate.latitude,
        latest_fix.coordinate.longitude,
        controller.smoothed_course_heading_degrees()
    );
}
