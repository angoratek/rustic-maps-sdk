//! Distance-parameterised traversal of a planned route's own geometry.
//!
//! A simulated trip must follow the polyline the visitor actually planned,
//! not a fixed demo line. [`RouteWalk`] flattens a [`RouteData`]'s legs →
//! steps → shapes into one traversal-order polyline, collapses duplicate and
//! zero-length segments, accumulates great-circle distances, and answers a
//! fix either at a travelled distance or at an elapsed time. Because the SDK
//! carries no pacing policy, the caller supplies a [`SimulatedTripPace`].
//!
//! # Traversal model
//!
//! - Distances between consecutive kept vertices are great-circle haversine
//!   distances (the same mathematics as [`GeographicCoordinate`]).
//! - Vertices closer than [`MINIMUM_SEGMENT_LENGTH_METERS`] to their
//!   predecessor are collapsed, so duplicated step boundaries and repeated
//!   shape points contribute no segment and no undefined bearing.
//! - A position between two vertices is a linear interpolation in latitude
//!   and in antimeridian-unwrapped longitude. This matches how the route
//!   polyline is drawn; over road-scale segments the difference from a
//!   great-circle arc is far below a meter.
//! - Segment bearings are the direction of travel along the active segment,
//!   in degrees clockwise from true north (0 = north, 90 = east). At an exact
//!   interior vertex the outgoing segment's bearing wins; the final vertex
//!   reports the last segment's bearing.
//! - Longitudes are unwrapped across the antimeridian before interpolation
//!   and bearings are computed, then the resulting coordinate is wrapped back
//!   into `[-180, 180]` the way the rest of the SDK normalizes world
//!   longitudes. A route from 179.9° to −179.9° therefore traverses 0.2°
//!   eastward rather than 359.8° westward.
//!
//! Both fix functions clamp their argument into the route's parameter range
//! instead of failing: a negative or NaN distance/time is the trip start,
//! anything past the end is the final fix. [`SimulatedTripFix::is_arrived`]
//! is then true exactly when the fix sits at the route's final coordinate.
//!
//! [`GeographicCoordinate`]: geospatial_types::GeographicCoordinate

use geospatial_types::{GeographicCoordinate, RouteData};

use crate::geometry::segment_bearing_degrees;

/// Segments shorter than this, in meters, are collapsed: subdividing them
/// further is numerically meaningless and leaves a segment without a
/// representable bearing. Shared with [`crate::geometry`]'s projection
/// threshold.
const MINIMUM_SEGMENT_LENGTH_METERS: f64 = 0.01;

/// Half a full turn, in degrees: the antimeridian longitude magnitude and the
/// unwrap/normalize offset.
const HALF_TURN_DEGREES: f64 = 180.0;

/// A full turn, in degrees: the antimeridian unwrap/normalize modulus.
const FULL_TURN_DEGREES: f64 = 360.0;

/// Pace chosen by the caller for a simulated trip.
///
/// The SDK deliberately carries no default pace: every application picks its
/// own numbers. The effective travel speed is
/// [`Self::speed_meters_per_second`] × [`Self::speed_multiplier`]; both
/// values must be finite and strictly positive, and their product must stay
/// finite. [`RouteWalk::new`] enforces that.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SimulatedTripPace {
    /// Baseline travel speed in meters per second.
    pub speed_meters_per_second: f64,
    /// Multiplier applied to the baseline speed (for example 1, 2, 4 or 8).
    pub speed_multiplier: f64,
}

/// One position of a simulated trip along the planned route.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SimulatedTripFix {
    /// Position on the planned route's polyline.
    pub coordinate: GeographicCoordinate,
    /// Direction of travel along the active segment, in degrees clockwise
    /// from true north; `None` when the route has no segment (a single-point
    /// route).
    pub course_heading_degrees: Option<f64>,
    /// Distance travelled from the route start, in meters.
    pub travelled_meters: f64,
    /// Distance left to the route end, in meters.
    pub remaining_meters: f64,
    /// Simulated elapsed time from the trip start, in seconds.
    pub elapsed_seconds: f64,
    /// Whether this fix sits at the route's final coordinate.
    pub is_arrived: bool,
}

/// Why a [`RouteWalk`] could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteWalkError {
    /// The pace cannot drive a trip: a speed or multiplier was non-finite or
    /// not strictly positive, or their product overflowed.
    InvalidPace {
        /// Verbatim description of the refused pace value.
        detail: String,
    },
    /// The route's legs and steps carry no coordinates at all, so there is
    /// nothing to traverse.
    NoUsableGeometry,
}

impl std::fmt::Display for RouteWalkError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPace { detail } => {
                write!(formatter, "invalid simulated-trip pace: {detail}")
            }
            Self::NoUsableGeometry => {
                write!(formatter, "the route has no usable geometry")
            }
        }
    }
}

impl std::error::Error for RouteWalkError {}

/// Validates a pace and returns its effective travel speed in meters per
/// second.
fn validated_effective_speed(pace: SimulatedTripPace) -> Result<f64, RouteWalkError> {
    if !pace.speed_meters_per_second.is_finite() || pace.speed_meters_per_second <= 0.0 {
        return Err(RouteWalkError::InvalidPace {
            detail: format!(
                "speed_meters_per_second must be finite and positive, got {}",
                pace.speed_meters_per_second
            ),
        });
    }
    if !pace.speed_multiplier.is_finite() || pace.speed_multiplier <= 0.0 {
        return Err(RouteWalkError::InvalidPace {
            detail: format!(
                "speed_multiplier must be finite and positive, got {}",
                pace.speed_multiplier
            ),
        });
    }
    let effective_speed = pace.speed_meters_per_second * pace.speed_multiplier;
    if !effective_speed.is_finite() || effective_speed <= 0.0 {
        return Err(RouteWalkError::InvalidPace {
            detail: format!(
                "the effective speed {} m/s is not finite and positive",
                effective_speed
            ),
        });
    }
    Ok(effective_speed)
}

/// Clamps `value` into `[minimum, maximum]`, mapping NaN to `minimum`.
/// Callers guarantee `minimum <= maximum` and both finite.
fn clamped_into_range(value: f64, minimum: f64, maximum: f64) -> f64 {
    if value.is_nan() {
        minimum
    } else {
        value.clamp(minimum, maximum)
    }
}

/// Continues an unwrapped longitude line from `previous_unwrapped` to
/// `longitude`, choosing the representation within half a turn so a crossing
/// of the antimeridian becomes a short step instead of a near-full turn.
fn unwrapped_longitude(previous_unwrapped: f64, longitude: f64) -> f64 {
    let mut unwrapped = longitude;
    while unwrapped - previous_unwrapped > HALF_TURN_DEGREES {
        unwrapped -= FULL_TURN_DEGREES;
    }
    while unwrapped - previous_unwrapped < -HALF_TURN_DEGREES {
        unwrapped += FULL_TURN_DEGREES;
    }
    unwrapped
}

/// Wraps a longitude back into `[-180, 180)`, matching the normalized-world
/// convention the renderer and camera use.
fn wrapped_longitude(longitude: f64) -> f64 {
    if longitude.is_finite() {
        (longitude + HALF_TURN_DEGREES).rem_euclid(FULL_TURN_DEGREES) - HALF_TURN_DEGREES
    } else {
        longitude
    }
}

/// Appends one shape vertex to the traversal polyline unless it duplicates
/// the previous kept vertex within [`MINIMUM_SEGMENT_LENGTH_METERS`].
fn append_vertex(
    vertices: &mut Vec<GeographicCoordinate>,
    unwrapped_vertices: &mut Vec<GeographicCoordinate>,
    coordinate: GeographicCoordinate,
) {
    let continued_longitude = match unwrapped_vertices.last() {
        Some(previous) => unwrapped_longitude(previous.longitude, coordinate.longitude),
        None => coordinate.longitude,
    };
    if let Some(previous) = vertices.last()
        && previous.haversine_distance_meters(&coordinate) < MINIMUM_SEGMENT_LENGTH_METERS
    {
        return;
    }
    vertices.push(coordinate);
    unwrapped_vertices.push(GeographicCoordinate {
        latitude: coordinate.latitude,
        longitude: continued_longitude,
    });
}

/// An immutable distance-parameterised walk over one planned route.
///
/// Construct it once per trip with [`RouteWalk::new`]; every fix is then a
/// cheap read. The walk owns the flattened, deduplicated polyline and the
/// cumulative distances, so a fix never re-flattens the route.
#[derive(Debug, Clone, PartialEq)]
pub struct RouteWalk {
    /// Deduplicated traversal-order vertices, in their original longitudes.
    vertices: Vec<GeographicCoordinate>,
    /// The same vertices with longitudes unwrapped across the antimeridian;
    /// used for interpolation and segment bearings.
    unwrapped_vertices: Vec<GeographicCoordinate>,
    /// Cumulative distance from the first vertex to each vertex, in meters;
    /// `cumulative_meters[0]` is zero and the last entry is the total.
    cumulative_meters: Vec<f64>,
    /// Bearing of each segment, one per consecutive vertex pair.
    segment_bearings_degrees: Vec<Option<f64>>,
    /// Total length of the flattened polyline, in meters.
    total_distance_meters: f64,
    /// The pace's effective travel speed, in meters per second.
    effective_speed_meters_per_second: f64,
}

impl RouteWalk {
    /// Flattens the route's legs, steps and shapes into one polyline and
    /// builds the walk at the given pace.
    ///
    /// A single distinct coordinate is accepted as an already-arrived route:
    /// its total distance and duration are zero and every fix is the final
    /// fix.
    ///
    /// # Parameters
    /// - `route` — the planned route whose geometry is traversed.
    /// - `pace` — the travel speed and multiplier that map elapsed time to
    ///   distance.
    ///
    /// # Errors
    /// Returns [`RouteWalkError::InvalidPace`] when the pace's speed or
    /// multiplier is non-finite or not strictly positive (or their product
    /// overflows), and [`RouteWalkError::NoUsableGeometry`] when the route has
    /// no coordinates.
    pub fn new(route: &RouteData, pace: SimulatedTripPace) -> Result<Self, RouteWalkError> {
        let effective_speed_meters_per_second = validated_effective_speed(pace)?;

        let mut vertices = Vec::new();
        let mut unwrapped_vertices = Vec::new();
        for step in route.legs.iter().flat_map(|leg| leg.steps.iter()) {
            for coordinate in &step.shape {
                append_vertex(&mut vertices, &mut unwrapped_vertices, *coordinate);
            }
        }
        if vertices.is_empty() {
            return Err(RouteWalkError::NoUsableGeometry);
        }

        let mut cumulative_meters = Vec::with_capacity(vertices.len());
        cumulative_meters.push(0.0);
        let mut total_distance_meters = 0.0;
        for segment in unwrapped_vertices.windows(2) {
            total_distance_meters += segment[0].haversine_distance_meters(&segment[1]);
            cumulative_meters.push(total_distance_meters);
        }

        let segment_bearings_degrees = unwrapped_vertices
            .windows(2)
            .map(|segment| segment_bearing_degrees(&segment[0], &segment[1]))
            .collect();

        Ok(Self {
            vertices,
            unwrapped_vertices,
            cumulative_meters,
            segment_bearings_degrees,
            total_distance_meters,
            effective_speed_meters_per_second,
        })
    }

    /// Total length of the flattened route polyline, in meters.
    ///
    /// # Returns
    /// The accumulated great-circle length of every kept segment.
    pub fn total_distance_meters(&self) -> f64 {
        self.total_distance_meters
    }

    /// Total simulated travel time at the configured pace, in seconds.
    ///
    /// # Returns
    /// The total route distance divided by the pace's effective speed.
    pub fn total_duration_seconds(&self) -> f64 {
        self.total_distance_meters / self.effective_speed_meters_per_second
    }

    /// The fix at a travelled distance from the route start.
    ///
    /// # Parameters
    /// - `travelled_meters` — distance along the route, clamped into
    ///   `[0, total_distance_meters()]`.
    ///
    /// # Returns
    /// The interpolated position and its course. The final coordinate is
    /// returned exactly at (and beyond) the total, with
    /// [`SimulatedTripFix::is_arrived`] true.
    pub fn fix_at_travelled_meters(&self, travelled_meters: f64) -> SimulatedTripFix {
        let travelled = clamped_into_range(travelled_meters, 0.0, self.total_distance_meters);
        let is_arrived = travelled >= self.total_distance_meters;
        let elapsed_seconds = travelled / self.effective_speed_meters_per_second;
        self.fix_at(travelled, elapsed_seconds, is_arrived)
    }

    /// The fix at an elapsed time of simulated travel.
    ///
    /// # Parameters
    /// - `elapsed_seconds` — seconds since the trip start, clamped into
    ///   `[0, total_duration_seconds()]`.
    ///
    /// # Returns
    /// The interpolated position and its course. The final coordinate is
    /// returned exactly at (and beyond) the total duration, with
    /// [`SimulatedTripFix::is_arrived`] true.
    pub fn fix_at_elapsed_seconds(&self, elapsed_seconds: f64) -> SimulatedTripFix {
        let total_duration_seconds = self.total_duration_seconds();
        let elapsed = clamped_into_range(elapsed_seconds, 0.0, total_duration_seconds);
        let travelled = (elapsed * self.effective_speed_meters_per_second)
            .clamp(0.0, self.total_distance_meters);
        // The final fix is due at the total duration even if the
        // multiplication rounds a hair short of the total distance.
        let is_arrived =
            elapsed >= total_duration_seconds || travelled >= self.total_distance_meters;
        let travelled = if is_arrived {
            self.total_distance_meters
        } else {
            travelled
        };
        self.fix_at(travelled, elapsed, is_arrived)
    }

    /// Builds the fix for an already-clamped travelled distance.
    fn fix_at(
        &self,
        travelled_meters: f64,
        elapsed_seconds: f64,
        is_arrived: bool,
    ) -> SimulatedTripFix {
        let (coordinate, course_heading_degrees) = self.coordinate_at(travelled_meters, is_arrived);
        SimulatedTripFix {
            coordinate,
            course_heading_degrees,
            travelled_meters,
            remaining_meters: self.total_distance_meters - travelled_meters,
            elapsed_seconds,
            is_arrived,
        }
    }

    /// The coordinate and active segment bearing at an already-clamped
    /// travelled distance.
    fn coordinate_at(
        &self,
        travelled_meters: f64,
        is_arrived: bool,
    ) -> (GeographicCoordinate, Option<f64>) {
        if is_arrived {
            let final_index = self.vertices.len() - 1;
            return (
                self.vertices[final_index],
                self.segment_bearings_degrees.last().copied().flatten(),
            );
        }

        // The last cumulative entry not beyond the position starts the
        // active segment; at an exact interior vertex this is the outgoing
        // segment, as documented.
        let segment_index = self
            .cumulative_meters
            .partition_point(|cumulative| *cumulative <= travelled_meters)
            .saturating_sub(1);
        let segment_start_meters = self.cumulative_meters[segment_index];
        let segment_end_meters = self.cumulative_meters[segment_index + 1];
        let along_segment_fraction = ((travelled_meters - segment_start_meters)
            / (segment_end_meters - segment_start_meters))
            .clamp(0.0, 1.0);
        let coordinate = if along_segment_fraction <= 0.0 {
            self.vertices[segment_index]
        } else if along_segment_fraction >= 1.0 {
            self.vertices[segment_index + 1]
        } else {
            let segment_start = self.unwrapped_vertices[segment_index];
            let segment_end = self.unwrapped_vertices[segment_index + 1];
            GeographicCoordinate {
                latitude: segment_start.latitude
                    + along_segment_fraction * (segment_end.latitude - segment_start.latitude),
                longitude: wrapped_longitude(
                    segment_start.longitude
                        + along_segment_fraction
                            * (segment_end.longitude - segment_start.longitude),
                ),
            }
        };
        (coordinate, self.segment_bearings_degrees[segment_index])
    }
}

#[cfg(test)]
mod tests {
    use super::{
        RouteWalk, RouteWalkError, SimulatedTripFix, SimulatedTripPace, clamped_into_range,
        wrapped_longitude,
    };
    use geospatial_types::{
        BoundingBox, GeographicCoordinate, ManeuverType, RouteData, RouteLeg, RouteStep,
    };

    /// Tolerance, in meters, for distances that must equal a haversine
    /// computation exactly up to floating-point noise.
    const EXACT_DISTANCE_TOLERANCE_METERS: f64 = 1e-6;

    /// Tolerance, in degrees, for bearings (the planar reconstruction
    /// rounding documented by the navigation geometry module).
    const BEARING_TOLERANCE_DEGREES: f64 = 1e-6;

    /// Tolerance, in degrees, for longitude arithmetic on the antimeridian.
    const LONGITUDE_TOLERANCE_DEGREES: f64 = 1e-9;

    /// Tolerance, in meters, for a fraction of a synthetic segment.
    const COORDINATE_TOLERANCE_METERS: f64 = 1e-3;

    /// Relative tolerance comparing a reference scene's walked polyline
    /// total with its declared provider distance. The committed demo routes
    /// differ by up to about 6.8% (the provider rounds each route's distance
    /// and the step shapes are straight segments between vertices), so 10%
    /// keeps the guard meaningful without pinning provider rounding.
    const REFERENCE_SCENE_DISTANCE_RELATIVE_TOLERANCE: f64 = 0.10;

    /// The committed shared demo artifact every reference app consumes.
    const REFERENCE_SCENES_PATH: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../platform/reference-scenes.json"
    );

    fn coordinate(latitude: f64, longitude: f64) -> GeographicCoordinate {
        GeographicCoordinate {
            latitude,
            longitude,
        }
    }

    fn pace(speed_meters_per_second: f64, speed_multiplier: f64) -> SimulatedTripPace {
        SimulatedTripPace {
            speed_meters_per_second,
            speed_multiplier,
        }
    }

    fn step_with_shape(shape: Vec<GeographicCoordinate>) -> RouteStep {
        RouteStep {
            distance_meters: 0.0,
            duration_seconds: 0.0,
            maneuver_type: ManeuverType::Continue,
            instruction_display_text: String::new(),
            instruction_speech_text: String::new(),
            shape,
            lane_guidances: Vec::new(),
        }
    }

    /// Builds a route from legs, each a list of step shapes, in traversal
    /// order.
    fn route_with_leg_shapes(leg_shapes: Vec<Vec<Vec<GeographicCoordinate>>>) -> RouteData {
        let legs = leg_shapes
            .into_iter()
            .map(|step_shapes| RouteLeg {
                distance_meters: 0.0,
                duration_seconds: 0.0,
                steps: step_shapes.into_iter().map(step_with_shape).collect(),
            })
            .collect();
        RouteData {
            distance_meters: 0.0,
            duration_seconds: 0.0,
            bounding_box: BoundingBox {
                south_west: coordinate(0.0, 0.0),
                north_east: coordinate(1.0, 1.0),
            },
            geometry: Vec::new(),
            legs,
        }
    }

    /// Builds a single-step route over one shape.
    fn route_over(shape: Vec<GeographicCoordinate>) -> RouteData {
        route_with_leg_shapes(vec![vec![shape]])
    }

    #[test]
    fn segment_course_follows_the_direction_of_travel() {
        let southbound = RouteWalk::new(
            &route_over(vec![
                coordinate(38.5769, -121.4950),
                coordinate(38.5764, -121.4950),
            ]),
            pace(1.4, 1.0),
        )
        .expect("the southbound route walks");
        let southbound_course = southbound
            .fix_at_travelled_meters(1.0)
            .course_heading_degrees
            .expect("a segment carries a course");
        assert!(
            (southbound_course - 180.0).abs() < 1.0,
            "southbound travel reports about 180 degrees, got {southbound_course}"
        );

        let eastbound = RouteWalk::new(
            &route_over(vec![
                coordinate(38.5769, -121.4950),
                coordinate(38.5769, -121.4945),
            ]),
            pace(1.4, 1.0),
        )
        .expect("the eastbound route walks");
        let eastbound_course = eastbound
            .fix_at_travelled_meters(1.0)
            .course_heading_degrees
            .expect("a segment carries a course");
        assert!(
            (eastbound_course - 90.0).abs() < 1.0,
            "eastbound travel reports about 90 degrees, got {eastbound_course}"
        );
    }

    #[test]
    fn empty_route_geometry_is_refused() {
        let no_legs = route_with_leg_shapes(Vec::new());

        error_route_walk_without_usable_geometry(&no_legs);
        error_route_walk_without_usable_geometry(&route_with_leg_shapes(vec![vec![Vec::new()]]));
    }

    fn error_route_walk_without_usable_geometry(route: &RouteData) {
        let error = RouteWalk::new(route, pace(1.0, 1.0))
            .expect_err("a route without coordinates must be refused");
        assert_eq!(error, RouteWalkError::NoUsableGeometry);
    }

    #[test]
    fn single_point_route_is_immediately_arrived() {
        let only_coordinate = coordinate(47.0, 8.0);
        let walk = RouteWalk::new(&route_over(vec![only_coordinate]), pace(2.0, 1.0))
            .expect("a single-point route is usable");

        assert_eq!(walk.total_distance_meters(), 0.0);
        assert_eq!(walk.total_duration_seconds(), 0.0);

        let fix = walk.fix_at_travelled_meters(0.0);
        assert_eq!(fix.coordinate, only_coordinate);
        assert_eq!(fix.course_heading_degrees, None);
        assert_eq!(fix.travelled_meters, 0.0);
        assert_eq!(fix.remaining_meters, 0.0);
        assert_eq!(fix.elapsed_seconds, 0.0);
        assert!(fix.is_arrived);

        assert!(walk.fix_at_elapsed_seconds(10.0).is_arrived);
    }

    #[test]
    fn duplicate_and_zero_length_vertices_are_collapsed() {
        let start = coordinate(47.0, 8.0);
        let end = coordinate(47.0, 8.001);
        let walk = RouteWalk::new(
            &route_with_leg_shapes(vec![vec![vec![start, start, start]], vec![vec![end, end]]]),
            pace(1.0, 1.0),
        )
        .expect("duplicates still leave two usable vertices");

        let expected_distance = start.haversine_distance_meters(&end);
        assert!(
            (walk.total_distance_meters() - expected_distance).abs()
                < EXACT_DISTANCE_TOLERANCE_METERS
        );
        assert_eq!(walk.fix_at_travelled_meters(0.0).coordinate, start);
        assert_eq!(
            walk.fix_at_travelled_meters(walk.total_distance_meters())
                .coordinate,
            end
        );
    }

    #[test]
    fn multi_leg_route_flattens_in_traversal_order() {
        let first = coordinate(47.0, 8.0);
        let second = coordinate(47.0, 8.002);
        let third = coordinate(47.001, 8.002);
        let fourth = coordinate(47.001, 8.004);
        let walk = RouteWalk::new(
            &route_with_leg_shapes(vec![
                vec![vec![first, second], vec![second, third]],
                vec![vec![third, fourth]],
            ]),
            pace(1.0, 1.0),
        )
        .expect("a multi-leg route is usable");

        let expected_distance = first.haversine_distance_meters(&second)
            + second.haversine_distance_meters(&third)
            + third.haversine_distance_meters(&fourth);
        assert!(
            (walk.total_distance_meters() - expected_distance).abs()
                < EXACT_DISTANCE_TOLERANCE_METERS
        );
        let first_leg_distance = first.haversine_distance_meters(&second);
        let crossing = walk.fix_at_travelled_meters(first_leg_distance);
        assert!(
            crossing.coordinate.haversine_distance_meters(&second) < COORDINATE_TOLERANCE_METERS,
            "an exact leg boundary returns the shared vertex"
        );
    }

    #[test]
    fn segment_distances_are_monotonic_and_sum_to_the_total() {
        let shape = vec![
            coordinate(47.0, 8.0),
            coordinate(47.0005, 8.0007),
            coordinate(47.0011, 8.0019),
            coordinate(47.0016, 8.0021),
        ];
        let walk = RouteWalk::new(&route_over(shape.clone()), pace(1.0, 1.0))
            .expect("a polyline is usable");

        let expected_total: f64 = shape
            .windows(2)
            .map(|segment| segment[0].haversine_distance_meters(&segment[1]))
            .sum();
        assert!(
            (walk.total_distance_meters() - expected_total).abs() < EXACT_DISTANCE_TOLERANCE_METERS
        );

        let sample_count = 20;
        let mut previous = walk.fix_at_travelled_meters(0.0);
        assert_eq!(previous.coordinate, shape[0]);
        for sample_index in 1..=sample_count {
            let travelled =
                walk.total_distance_meters() * (sample_index as f64 / sample_count as f64);
            let fix = walk.fix_at_travelled_meters(travelled);
            assert!(fix.travelled_meters >= previous.travelled_meters);
            assert!(fix.remaining_meters <= previous.remaining_meters);
            assert!(fix.remaining_meters >= 0.0);
            previous = fix;
        }
        assert!(previous.is_arrived);
        assert_eq!(previous.coordinate, shape[shape.len() - 1]);
    }

    #[test]
    fn exact_vertices_return_the_vertex_and_the_outgoing_segment_bearing() {
        // Eastward, then northward: the shared vertex's incoming bearing is
        // 90 degrees, its outgoing bearing 0 degrees.
        let start = coordinate(47.0, 8.0);
        let corner = coordinate(47.0, 8.002);
        let end = coordinate(47.002, 8.002);
        let walk = RouteWalk::new(
            &route_with_leg_shapes(vec![vec![vec![start, corner], vec![corner, end]]]),
            pace(1.0, 1.0),
        )
        .expect("an L-shaped route is usable");

        let corner_distance = start.haversine_distance_meters(&corner);
        let corner_fix = walk.fix_at_travelled_meters(corner_distance);
        assert_eq!(corner_fix.coordinate, corner);
        let corner_bearing = corner_fix
            .course_heading_degrees
            .expect("the outgoing segment carries a bearing");
        assert!(
            (corner_bearing - 0.0).abs() < BEARING_TOLERANCE_DEGREES,
            "the exact interior vertex reports the outgoing northward segment, got {corner_bearing}"
        );

        let start_fix = walk.fix_at_travelled_meters(0.0);
        assert_eq!(start_fix.coordinate, start);
        let start_bearing = start_fix
            .course_heading_degrees
            .expect("the first segment carries a bearing");
        assert!(
            (start_bearing - 90.0).abs() < BEARING_TOLERANCE_DEGREES,
            "the first segment runs eastward, got {start_bearing}"
        );

        let final_fix = walk.fix_at_travelled_meters(walk.total_distance_meters());
        assert_eq!(final_fix.coordinate, end);
        assert!(final_fix.is_arrived);
        let final_bearing = final_fix
            .course_heading_degrees
            .expect("the last segment carries a bearing");
        assert!(
            (final_bearing - 0.0).abs() < BEARING_TOLERANCE_DEGREES,
            "the final vertex reports the last segment, got {final_bearing}"
        );
    }

    #[test]
    fn fix_zero_is_first_and_fix_total_is_last() {
        let start = coordinate(47.0, 8.0);
        let end = coordinate(47.003, 8.004);
        let walk = RouteWalk::new(&route_over(vec![start, end]), pace(1.0, 1.0))
            .expect("a two-point route is usable");

        let first_fix = walk.fix_at_travelled_meters(0.0);
        assert_eq!(first_fix.coordinate, start);
        assert!(!first_fix.is_arrived);
        assert!((first_fix.remaining_meters - walk.total_distance_meters()).abs() < 1e-9);

        let final_fix = walk.fix_at_travelled_meters(walk.total_distance_meters());
        assert_eq!(final_fix.coordinate, end);
        assert!(final_fix.is_arrived);
        assert_eq!(final_fix.remaining_meters, 0.0);
    }

    #[test]
    fn antimeridian_crossings_travel_the_short_way_and_unwrap_for_interpolation() {
        let west_of_antimeridian = coordinate(0.0, -179.9);
        let east_of_antimeridian = coordinate(0.0, 179.9);

        // Eastward across the antimeridian: 179.9 -> -179.9.
        let eastward = RouteWalk::new(
            &route_over(vec![east_of_antimeridian, west_of_antimeridian]),
            pace(1.0, 1.0),
        )
        .expect("an antimeridian crossing is usable");
        let crossing_distance =
            east_of_antimeridian.haversine_distance_meters(&west_of_antimeridian);
        assert!(
            (eastward.total_distance_meters() - crossing_distance).abs()
                < EXACT_DISTANCE_TOLERANCE_METERS,
            "the crossing is the short arc, not the long way around"
        );
        let eastward_bearing = eastward
            .fix_at_travelled_meters(0.0)
            .course_heading_degrees
            .expect("a crossing segment carries a bearing");
        assert!(
            (eastward_bearing - 90.0).abs() < BEARING_TOLERANCE_DEGREES,
            "eastward travel across the antimeridian is 90 degrees, got {eastward_bearing}"
        );
        let midpoint = eastward.fix_at_travelled_meters(eastward.total_distance_meters() / 2.0);
        assert!(
            (midpoint.coordinate.longitude.abs() - HALF_TURN_DEGREES_FOR_TEST).abs()
                < LONGITUDE_TOLERANCE_DEGREES,
            "the midpoint sits on the antimeridian, got {}",
            midpoint.coordinate.longitude
        );

        // Westward across the antimeridian: -179.9 -> 179.9.
        let westward = RouteWalk::new(
            &route_over(vec![west_of_antimeridian, east_of_antimeridian]),
            pace(1.0, 1.0),
        )
        .expect("an antimeridian crossing is usable");
        let westward_bearing = westward
            .fix_at_travelled_meters(0.0)
            .course_heading_degrees
            .expect("a crossing segment carries a bearing");
        assert!(
            (westward_bearing - 270.0).abs() < BEARING_TOLERANCE_DEGREES,
            "westward travel across the antimeridian is 270 degrees, got {westward_bearing}"
        );
    }

    /// Half a full turn, in degrees, restated in the test module so the
    /// assertion reads without reaching into private constants.
    const HALF_TURN_DEGREES_FOR_TEST: f64 = 180.0;

    #[test]
    fn invalid_paces_are_refused() {
        let route = route_over(vec![coordinate(47.0, 8.0), coordinate(47.001, 8.0)]);
        for invalid_speed in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(
                matches!(
                    RouteWalk::new(&route, pace(invalid_speed, 1.0)),
                    Err(RouteWalkError::InvalidPace { .. })
                ),
                "speed {invalid_speed} must be refused"
            );
        }
        for invalid_multiplier in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(
                matches!(
                    RouteWalk::new(&route, pace(1.0, invalid_multiplier)),
                    Err(RouteWalkError::InvalidPace { .. })
                ),
                "multiplier {invalid_multiplier} must be refused"
            );
        }
        assert!(matches!(
            RouteWalk::new(&route, pace(f64::MAX, 2.0)),
            Err(RouteWalkError::InvalidPace { .. })
        ));
    }

    #[test]
    fn fix_parameters_clamp_instead_of_failing() {
        let start = coordinate(47.0, 8.0);
        let end = coordinate(47.001, 8.0);
        let walk = RouteWalk::new(&route_over(vec![start, end]), pace(2.0, 3.0))
            .expect("a two-point route is usable");

        assert_eq!(walk.fix_at_travelled_meters(-100.0).coordinate, start);
        assert_eq!(walk.fix_at_travelled_meters(f64::NAN).coordinate, start);
        assert_eq!(
            walk.fix_at_travelled_meters(-f64::INFINITY).coordinate,
            start
        );
        let beyond = walk.fix_at_travelled_meters(walk.total_distance_meters() + 100.0);
        assert_eq!(beyond.coordinate, end);
        assert!(beyond.is_arrived);
        assert_eq!(beyond.remaining_meters, 0.0);

        assert_eq!(walk.fix_at_elapsed_seconds(-1.0).coordinate, start);
        assert_eq!(walk.fix_at_elapsed_seconds(f64::NAN).coordinate, start);
        let final_elapsed = walk.total_duration_seconds();
        let arrived = walk.fix_at_elapsed_seconds(final_elapsed + walk.total_duration_seconds());
        assert_eq!(arrived.coordinate, end);
        assert!(arrived.is_arrived);
    }

    #[test]
    fn elapsed_and_distance_parameters_describe_the_same_fix() {
        let walk = RouteWalk::new(
            &route_over(vec![
                coordinate(47.0, 8.0),
                coordinate(47.001, 8.001),
                coordinate(47.002, 8.0015),
            ]),
            pace(1.5, 4.0),
        )
        .expect("a polyline is usable");

        let effective_speed = 1.5 * 4.0;
        for elapsed_seconds in [1.0, 10.0, 25.0] {
            let by_elapsed = walk.fix_at_elapsed_seconds(elapsed_seconds);
            let by_distance = walk.fix_at_travelled_meters(elapsed_seconds * effective_speed);
            assert!(
                by_elapsed
                    .coordinate
                    .haversine_distance_meters(&by_distance.coordinate)
                    < COORDINATE_TOLERANCE_METERS
            );
            assert!((by_elapsed.travelled_meters - by_distance.travelled_meters).abs() < 1e-6);
            assert!((by_elapsed.elapsed_seconds - elapsed_seconds).abs() < 1e-9);
        }
        assert!(
            (walk.total_duration_seconds() - walk.total_distance_meters() / effective_speed).abs()
                < 1e-9
        );
    }

    #[test]
    fn clamped_into_range_maps_nan_and_out_of_range_values() {
        assert_eq!(clamped_into_range(f64::NAN, 0.0, 10.0), 0.0);
        assert_eq!(clamped_into_range(-1.0, 0.0, 10.0), 0.0);
        assert_eq!(clamped_into_range(11.0, 0.0, 10.0), 10.0);
        assert_eq!(clamped_into_range(5.0, 0.0, 10.0), 5.0);
    }

    #[test]
    fn wrapped_longitude_normalizes_into_the_canonical_range() {
        assert!((wrapped_longitude(180.0) - -180.0).abs() < f64::EPSILON);
        assert!((wrapped_longitude(190.0) - -170.0).abs() < 1e-9);
        assert!((wrapped_longitude(-190.0) - 170.0).abs() < 1e-9);
        assert!((wrapped_longitude(38.5) - 38.5).abs() < f64::EPSILON);
    }

    #[derive(serde::Deserialize)]
    struct ReferenceScenes {
        demo_routes: Vec<ReferenceSceneRoute>,
    }

    #[derive(serde::Deserialize)]
    struct ReferenceSceneRoute {
        route: RouteData,
    }

    #[test]
    fn every_reference_scene_route_walks_to_its_declared_distance() {
        let scenes_json = std::fs::read_to_string(REFERENCE_SCENES_PATH)
            .expect("the shared reference-scenes artifact is committed");
        let scenes: ReferenceScenes =
            serde_json::from_str(&scenes_json).expect("reference-scenes parses");
        assert!(
            !scenes.demo_routes.is_empty(),
            "the shared artifact carries demo routes"
        );

        for demo_route in &scenes.demo_routes {
            let declared = demo_route.route.distance_meters;
            assert!(declared > 0.0, "a demo route declares a positive distance");
            let walk = RouteWalk::new(&demo_route.route, pace(1.0, 1.0))
                .expect("a committed demo route carries usable geometry");
            let walked = walk.total_distance_meters();
            let relative_difference = (walked - declared).abs() / declared;
            assert!(
                relative_difference <= REFERENCE_SCENE_DISTANCE_RELATIVE_TOLERANCE,
                "walked {walked} m against declared {declared} m differs by \
                 {:.2}%, over the documented {:.0}% tolerance",
                relative_difference * 100.0,
                REFERENCE_SCENE_DISTANCE_RELATIVE_TOLERANCE * 100.0
            );
            let final_fix: SimulatedTripFix = walk.fix_at_travelled_meters(walked);
            assert!(final_fix.is_arrived);
        }
    }
}
