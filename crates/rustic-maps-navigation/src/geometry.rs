//! Polyline snapping mathematics for route progress.
//!
//! The navigation state machine repeatedly needs two measurements against the
//! current step's shape: the closest point on the polyline (the snapped
//! position) and the distance from a raw position fix to the polyline (the
//! same minimum the deviation detector consumes later). Both live here so the
//! projection error is documented and tested in exactly one place.
//!
//! # Projection model
//!
//! Each segment is projected into a local equirectangular plane around its own
//! midpoint: a north/south offset of `y` meters becomes `y / RADIUS` radians
//! of latitude and an east/west offset of `x` meters becomes
//! `x / (RADIUS * cos(reference_latitude))` radians of longitude. This is
//! exact along the reference parallel and along meridians within the
//! small-angle approximation; the residual grows with segment length and
//! distance from the equator. For the road-scale segments a navigation step
//! carries (well under 1 km) at inhabited latitudes the distortion is far
//! below a meter, comfortably inside GNSS accuracy. The minimization over
//! segments uses this planar space; the reported distance is then recomputed
//! exactly with the haversine formula. Antimeridian-crossing shapes are not
//! supported: longitudes are treated as a plain number line.

use geospatial_types::GeographicCoordinate;

/// Mean sphere radius of the Earth in meters, matching the haversine constant
/// used by [`GeographicCoordinate`] and following the IUGG mean-radius
/// convention.
pub(crate) const EARTH_RADIUS_METERS: f64 = 6_371_008.8;

/// Segments shorter than this, in meters, are treated as a single point when
/// projecting: subdividing them further is numerically meaningless and makes
/// the parametric projection degenerate.
const MINIMUM_SEGMENT_LENGTH_METERS: f64 = 0.01;

/// Full angular circle, in degrees; a bearing's normalization modulus.
const FULL_CIRCLE_DEGREES: f64 = 360.0;

/// Outcome of snapping a position fix onto a polyline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PolylineSnap {
    /// Closest point on the polyline.
    pub snapped_coordinate: GeographicCoordinate,
    /// Exact great-circle distance from the input to [`Self::snapped_coordinate`], in meters.
    pub distance_to_polyline_meters: f64,
    /// Direction of travel along the polyline at the snapped position, in
    /// degrees clockwise from true north (0 = north, 90 = east): the bearing
    /// of the segment that hosted the projection. `None` when no direction is
    /// representable — a shape without enough geometry (a single point) or a
    /// winning segment too short to carry a direction (per
    /// [`MINIMUM_SEGMENT_LENGTH_METERS`]); the course filtering consults it
    /// (see [`crate::course_filtering`]).
    pub bearing_degrees: Option<f64>,
}

/// Snaps a position fix onto a polyline by projecting it onto every segment
/// and keeping the closest projection.
///
/// Returns `None` when the shape has no coordinates at all. A single-point
/// shape snaps to that point. Projections are clipped to the segment
/// (a position fix beyond a segment end snaps to the endpoint); see the
/// module documentation for the projection model.
pub fn snap_coordinate_to_polyline(
    coordinate: &GeographicCoordinate,
    shape: &[GeographicCoordinate],
) -> Option<PolylineSnap> {
    if shape.is_empty() {
        return None;
    }
    if shape.len() == 1 {
        return Some(PolylineSnap {
            snapped_coordinate: shape[0],
            distance_to_polyline_meters: coordinate.haversine_distance_meters(&shape[0]),
            bearing_degrees: None,
        });
    }

    let mut best_snap: Option<PolylineSnap> = None;
    for segment in shape.windows(2) {
        let projected_coordinate = project_onto_segment(coordinate, &segment[0], &segment[1]);
        let candidate_distance = coordinate.haversine_distance_meters(&projected_coordinate);
        let is_improvement = best_snap
            .as_ref()
            .is_none_or(|current| candidate_distance < current.distance_to_polyline_meters);
        if is_improvement {
            best_snap = Some(PolylineSnap {
                snapped_coordinate: projected_coordinate,
                distance_to_polyline_meters: candidate_distance,
                bearing_degrees: segment_bearing_degrees(&segment[0], &segment[1]),
            });
        }
    }
    best_snap
}

/// Direction of travel along the segment from `segment_start` to
/// `segment_end`, in degrees clockwise from true north, or `None` when the
/// segment is too short to carry a direction.
///
/// The same equirectangular local plane the projection uses measures the
/// segment's eastward and northward components around the segment midpoint;
/// the resulting north-referenced bearing is then accurate exactly to the
/// same documented projection error (and the segment is exactly the one a
/// projection onto the segment would consult).
pub(crate) fn segment_bearing_degrees(
    segment_start: &GeographicCoordinate,
    segment_end: &GeographicCoordinate,
) -> Option<f64> {
    let reference_latitude = (segment_start.latitude + segment_end.latitude) / 2.0;
    let reference_longitude = (segment_start.longitude + segment_end.longitude) / 2.0;
    let reference = GeographicCoordinate {
        latitude: reference_latitude,
        longitude: reference_longitude,
    };
    let (segment_east_meters, segment_north_meters) = {
        let (start_east_meters, start_north_meters) =
            local_planar_meters(&reference, segment_start);
        let (end_east_meters, end_north_meters) = local_planar_meters(&reference, segment_end);
        (
            end_east_meters - start_east_meters,
            end_north_meters - start_north_meters,
        )
    };
    let segment_length_meters = segment_east_meters.hypot(segment_north_meters);
    if segment_length_meters < MINIMUM_SEGMENT_LENGTH_METERS {
        return None;
    }
    let north_referenced_bearing_degrees =
        segment_east_meters.atan2(segment_north_meters).to_degrees();
    Some((north_referenced_bearing_degrees + FULL_CIRCLE_DEGREES).rem_euclid(FULL_CIRCLE_DEGREES))
}

/// Projects a point onto one segment in the local equirectangular plane around
/// the segment midpoint, clamping to the segment endpoints.
fn project_onto_segment(
    point: &GeographicCoordinate,
    segment_start: &GeographicCoordinate,
    segment_end: &GeographicCoordinate,
) -> GeographicCoordinate {
    let reference_latitude = (segment_start.latitude + segment_end.latitude) / 2.0;
    let reference_longitude = (segment_start.longitude + segment_end.longitude) / 2.0;
    let reference = GeographicCoordinate {
        latitude: reference_latitude,
        longitude: reference_longitude,
    };

    let (start_east_meters, start_north_meters) = local_planar_meters(&reference, segment_start);
    let (end_east_meters, end_north_meters) = local_planar_meters(&reference, segment_end);
    let (point_east_meters, point_north_meters) = local_planar_meters(&reference, point);

    let segment_east_meters = end_east_meters - start_east_meters;
    let segment_north_meters = end_north_meters - start_north_meters;
    let segment_length_squared =
        segment_east_meters * segment_east_meters + segment_north_meters * segment_north_meters;

    let along_fraction =
        if segment_length_squared < MINIMUM_SEGMENT_LENGTH_METERS * MINIMUM_SEGMENT_LENGTH_METERS {
            // Degenerate segment: every projection is the same point.
            0.0
        } else {
            let offset_east_meters = point_east_meters - start_east_meters;
            let offset_north_meters = point_north_meters - start_north_meters;
            let unclamped_fraction = (offset_east_meters * segment_east_meters
                + offset_north_meters * segment_north_meters)
                / segment_length_squared;
            unclamped_fraction.clamp(0.0, 1.0)
        };

    local_planar_coordinate(
        &reference,
        start_east_meters + along_fraction * segment_east_meters,
        start_north_meters + along_fraction * segment_north_meters,
    )
}

/// Converts a coordinate into local east/north meter offsets from `reference`.
fn local_planar_meters(
    reference: &GeographicCoordinate,
    point: &GeographicCoordinate,
) -> (f64, f64) {
    let north_meters = (point.latitude - reference.latitude).to_radians() * EARTH_RADIUS_METERS;
    let east_meters = (point.longitude - reference.longitude).to_radians()
        * EARTH_RADIUS_METERS
        * reference.latitude.to_radians().cos();
    (east_meters, north_meters)
}

/// Rebuilds a coordinate from local east/north meter offsets around `reference`.
fn local_planar_coordinate(
    reference: &GeographicCoordinate,
    east_meters: f64,
    north_meters: f64,
) -> GeographicCoordinate {
    GeographicCoordinate {
        latitude: reference.latitude + (north_meters / EARTH_RADIUS_METERS).to_degrees(),
        longitude: reference.longitude
            + (east_meters / (EARTH_RADIUS_METERS * reference.latitude.to_radians().cos()))
                .to_degrees(),
    }
}

#[cfg(test)]
mod tests {
    use super::snap_coordinate_to_polyline;
    use crate::test_support::coordinate_east_of;
    use proptest::prelude::*;

    /// Perpendicular distance, in meters, used for the 90-degree-offset case.
    const OFFSET_METERS: f64 = 30.0;
    /// Total eastward span, in meters, of the straight test segments.
    const SEGMENT_SPAN_METERS: f64 = 1000.0;
    /// Half of [`SEGMENT_SPAN_METERS`], the segment midpoint.
    const SEGMENT_MIDPOINT_METERS: f64 = SEGMENT_SPAN_METERS / 2.0;
    /// Tolerance, in meters, for reconstructed planar distances at test scale.
    const PLANAR_TOLERANCE_METERS: f64 = 0.1;
    /// Structural tolerance for distance measurements that must be near zero.
    const NEAR_ZERO_TOLERANCE_METERS: f64 = 0.001;
    /// Bearing tolerance, in degrees, for the reconstructed segment directions
    /// (haversine and planar reconstruction rounding at test scale).
    const BEARING_TOLERANCE_DEGREES: f64 = 0.001;

    fn snapped_on(
        shape: &[geospatial_types::GeographicCoordinate],
        point: &geospatial_types::GeographicCoordinate,
    ) -> super::PolylineSnap {
        snap_coordinate_to_polyline(point, shape).expect("test polyline must not be empty")
    }

    #[test]
    fn empty_shape_snaps_to_nothing() {
        let coordinate = coordinate_east_of(0.0);
        assert!(snap_coordinate_to_polyline(&coordinate, &[]).is_none());
    }

    #[test]
    fn single_point_shape_snaps_to_that_point() {
        let shape = [coordinate_east_of(SEGMENT_MIDPOINT_METERS)];
        let snap = snapped_on(&shape, &coordinate_east_of(0.0));
        assert_eq!(snap.snapped_coordinate, shape[0]);
        assert!(snap.distance_to_polyline_meters > 0.0);
    }

    #[test]
    fn point_on_segment_midpoint_snaps_to_itself() {
        let shape = [
            coordinate_east_of(0.0),
            coordinate_east_of(SEGMENT_SPAN_METERS),
        ];
        let midpoint = coordinate_east_of(SEGMENT_MIDPOINT_METERS);
        let snap = snapped_on(&shape, &midpoint);
        assert!(
            midpoint.haversine_distance_meters(&snap.snapped_coordinate)
                < NEAR_ZERO_TOLERANCE_METERS,
            "midpoint must snap to itself"
        );
        assert!(snap.distance_to_polyline_meters < NEAR_ZERO_TOLERANCE_METERS);
    }

    #[test]
    fn perpendicular_offset_returns_the_offset_distance() {
        // 90 degrees off the eastward segment: the perpendicular distance
        // equals the northward offset; the planar conversion error documented
        // in the module is negligible at these sizes.
        let shape = [
            coordinate_east_of(0.0),
            coordinate_east_of(SEGMENT_SPAN_METERS),
        ];
        let offset_point =
            crate::test_support::offset_coordinate(SEGMENT_MIDPOINT_METERS, OFFSET_METERS);
        let snap = snapped_on(&shape, &offset_point);
        assert!(
            (snap.distance_to_polyline_meters - OFFSET_METERS).abs() < PLANAR_TOLERANCE_METERS,
            "expected ~{OFFSET_METERS}, got {}",
            snap.distance_to_polyline_meters
        );
        // The perpendicular foot keeps the along-segment position of the
        // input; degrees of longitude tolerate ~1e-9 here at the test scale.
        assert!(
            (snap.snapped_coordinate.longitude - offset_point.longitude).abs() < 1e-9,
            "projection must keep the along-segment position of the input"
        );
    }

    #[test]
    fn beyond_segment_end_clamps_to_the_endpoint() {
        let shape = [
            coordinate_east_of(0.0),
            coordinate_east_of(SEGMENT_SPAN_METERS),
        ];
        let beyond = coordinate_east_of(SEGMENT_SPAN_METERS * 2.0);
        let snap = snapped_on(&shape, &beyond);
        assert_eq!(snap.snapped_coordinate, shape[1]);
        let expected_distance = beyond.haversine_distance_meters(&shape[1]);
        assert!((snap.distance_to_polyline_meters - expected_distance).abs() < 1e-6);
    }

    #[test]
    fn multi_segment_polyline_picks_the_closest_segment() {
        // An L shape: east 1 km, then north 1 km. A point halfway along the
        // eastward leg must snap onto that leg, not the northward one.
        let east_corner = crate::test_support::offset_coordinate(SEGMENT_SPAN_METERS, 0.0);
        let north_corner =
            crate::test_support::offset_coordinate(SEGMENT_SPAN_METERS, SEGMENT_SPAN_METERS);
        let shape = [coordinate_east_of(0.0), east_corner, north_corner];
        let point_on_east_leg = coordinate_east_of(SEGMENT_MIDPOINT_METERS);
        let snap = snapped_on(&shape, &point_on_east_leg);
        assert!(
            (snap.snapped_coordinate.longitude - point_on_east_leg.longitude).abs() < 1e-9
                && (snap.snapped_coordinate.latitude - point_on_east_leg.latitude).abs() < 1e-9,
            "expected the projection on the eastward segment"
        );
    }

    /// The snap's bearing is the winning segment's direction of travel:
    /// eastward, northward, westward and southward segments carry their
    /// compass bearings, measured clockwise from true north.
    #[test]
    fn snap_bearing_follows_the_winning_segments_direction() {
        // Eastward (90 degrees) — the single-segment shape of the other tests.
        let eastward_shape = [
            coordinate_east_of(0.0),
            coordinate_east_of(SEGMENT_SPAN_METERS),
        ];
        let eastward_snap = snapped_on(
            &eastward_shape,
            &coordinate_east_of(SEGMENT_MIDPOINT_METERS),
        );
        let eastward_bearing = eastward_snap
            .bearing_degrees
            .expect("an eastward test segment carries a direction");
        assert!(
            (eastward_bearing - 90.0).abs() < BEARING_TOLERANCE_DEGREES,
            "eastward travel is 90 degrees clockwise from north, got {eastward_bearing}"
        );

        // Northward (0 degrees), built from planar offsets around the origin.
        let northward_shape = [
            crate::test_support::offset_coordinate(0.0, 0.0),
            crate::test_support::offset_coordinate(0.0, SEGMENT_SPAN_METERS),
        ];
        let northward_point = crate::test_support::offset_coordinate(
            SEGMENT_MIDPOINT_METERS,
            SEGMENT_MIDPOINT_METERS,
        );
        let northward_snap = snapped_on(&northward_shape, &northward_point);
        let northward_bearing = northward_snap
            .bearing_degrees
            .expect("a northward test segment carries a direction");
        assert!(
            (northward_bearing - 0.0).abs() < BEARING_TOLERANCE_DEGREES,
            "northward travel is 0 degrees, got {northward_bearing}"
        );

        // Westward (270 degrees).
        let westward_shape = [
            coordinate_east_of(SEGMENT_SPAN_METERS),
            coordinate_east_of(0.0),
        ];
        let westward_snap = snapped_on(
            &westward_shape,
            &coordinate_east_of(SEGMENT_MIDPOINT_METERS),
        );
        let westward_bearing = westward_snap
            .bearing_degrees
            .expect("a westward test segment carries a direction");
        assert!(
            (westward_bearing - 270.0).abs() < BEARING_TOLERANCE_DEGREES,
            "westward travel is 270 degrees clockwise from north, got {westward_bearing}"
        );

        // Southward (180 degrees).
        let southward_shape = [
            crate::test_support::offset_coordinate(0.0, SEGMENT_SPAN_METERS),
            crate::test_support::offset_coordinate(0.0, 0.0),
        ];
        let southward_snap = snapped_on(
            &southward_shape,
            &crate::test_support::offset_coordinate(
                SEGMENT_MIDPOINT_METERS,
                SEGMENT_MIDPOINT_METERS,
            ),
        );
        let southward_bearing = southward_snap
            .bearing_degrees
            .expect("a southward test segment carries a direction");
        assert!(
            (southward_bearing - 180.0).abs() < BEARING_TOLERANCE_DEGREES,
            "southward travel is 180 degrees clockwise from north, got {southward_bearing}"
        );
    }

    /// Geometry without a representable direction snaps without a bearing: a
    /// single-point shape and a winning segment below the degenerate-segment
    /// threshold both.
    #[test]
    fn snap_without_a_representable_direction_carries_no_bearing() {
        // Single-point shape: snaps to that point, no direction exists.
        let point_shape = [coordinate_east_of(SEGMENT_MIDPOINT_METERS)];
        let point_snap = snapped_on(&point_shape, &coordinate_east_of(0.0));
        assert_eq!(point_snap.bearing_degrees, None);

        // A closed shape built as a line, then collapsed: the winning segment
        // is the collapse (below the degenerate threshold), the bearing is
        // suppressed while the position still snaps.
        let degenerate_shape = [
            coordinate_east_of(SEGMENT_MIDPOINT_METERS),
            coordinate_east_of(SEGMENT_MIDPOINT_METERS),
        ];
        let degenerate_snap = snapped_on(
            &degenerate_shape,
            &coordinate_east_of(SEGMENT_MIDPOINT_METERS),
        );
        assert!(degenerate_snap.distance_to_polyline_meters < NEAR_ZERO_TOLERANCE_METERS);
        assert_eq!(degenerate_snap.bearing_degrees, None);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// Snapping must be idempotent: the snapped point lies on the polyline,
        /// so re-snapping it must return the same point at (near) zero distance.
        #[test]
        fn snapping_is_idempotent(
            east_meters in -500.0f64..1500.0,
            north_offset_meters in -300.0f64..300.0,
        ) {
            let shape = [coordinate_east_of(0.0), coordinate_east_of(crate::test_support::TEST_SEGMENT_SPAN_METERS)];
            let noisy_point = crate::test_support::offset_coordinate(east_meters, north_offset_meters);
            let first = snapped_on(&shape, &noisy_point);
            let second = snap_coordinate_to_polyline(&first.snapped_coordinate, &shape).unwrap();
            assert_eq!(first.snapped_coordinate, second.snapped_coordinate);
            assert!(second.distance_to_polyline_meters < 1e-6);
        }
    }
}
