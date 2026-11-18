//! The plain-data navigation state record produced by the reducer.
//!
//! `TripNavigationState` is the exported half of the navigation model: it holds
//! only serializable data (indices, distances, a snapped coordinate), never
//! the live [`StepAdvanceCondition`](crate::step_advance::StepAdvanceCondition)
//! or [`DeviationDetector`](crate::deviation::DeviationDetector) instance.
//! Both live beside the state in a
//! [`NavigationSession`](crate::reducer::NavigationSession), which keeps this
//! record safe to expose through a language binding.

use geospatial_types::{GeographicCoordinate, RouteData, RouteStep};

/// High-level phase of a trip that has already been started.
///
/// Rerouting is not a phase of its own: a reroute is the caller's answer
/// (a `RerouteProvided` event) and the session is simply rebuilt for the new
/// route, so the machine flows `Navigating ⇄ OffRoute` within a trip and
/// `Navigating → Arrived` at its end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationPhase {
    /// Following the route's step sequence.
    Navigating,
    /// The deviation detector judged the last graded fix off route; progress
    /// is frozen at the last on-route values until a graded fix recovers or a
    /// reroute replaces the route (see [`crate::reducer`]).
    OffRoute,
    /// The final step has been completed; location updates no longer change
    /// the state.
    Arrived,
}

/// Which grade of deviation a recorded deviation carries.
///
/// The two grades distinguish a fix that is off the *current step* yet still
/// on the route line from a fix that is completely off the route. Only the
/// fully-off grade freezes navigation; the wrong-step grade never freezes on
/// its own (see [`crate::reducer`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviationGrade {
    /// The fix projects onto neither the current step nor any future step of
    /// the remaining route line: the freezing grade. The trip's progress is
    /// frozen until a graded fix recovers or a reroute replaces the route.
    FullyOffRoute,
    /// The fix is off the current step but projects onto a future step of the
    /// remaining route line: "on route, wrong step". The trip keeps
    /// navigating, counting down, and is never frozen by this grade alone —
    /// the step-advance condition reconciles the step as the fixes continue.
    OffStepOnRoute,
}

/// Plain-data description of an active deviation from the route.
///
/// The record carries the deviation's
/// [`grade`](DeviationInfo::grade) beside its measurement: a
/// [`DeviationGrade::OffStepOnRoute`] reading is guidance feedback ("you are
/// on the route, on a later step") rather than a frozen off-route trip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeviationInfo {
    /// Which grade of deviation this is (see [`DeviationGrade`]).
    pub grade: DeviationGrade,
    /// Distance, in meters, from the raw fix that triggered (or most recently
    /// confirmed) the deviation to the remaining route line, as measured and
    /// judged by the
    /// [`DeviationDetector`](crate::deviation::DeviationDetector).
    pub deviation_from_route_meters: f64,
}

/// Location of one route step in traversal order.
///
/// A route is organized as legs of steps; navigation walks the steps of all
/// legs in order, so the reducer flattens them up front while keeping both
/// indices so an instruction can still be attributed to its leg.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteStepPosition {
    /// Index of the leg that owns this step in [`RouteData::legs`].
    pub leg_index: usize,
    /// Index of the step within its leg in [`RouteLeg::steps`].
    pub step_index: usize,
}

/// Progress along the remaining trip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TripProgress {
    /// Distance, in meters, from the snapped position to the end of the
    /// current step — the point of the next maneuver.
    pub distance_to_next_maneuver_meters: f64,
    /// Total remaining distance, in meters, including the current step's
    /// remainder: [`Self::distance_to_next_maneuver_meters`] plus the declared
    /// distances of all steps after the current one.
    pub distance_remaining_meters: f64,
    /// Total remaining travel time, in seconds, measured the same way as
    /// [`Self::distance_remaining_meters`].
    pub duration_remaining_seconds: f64,
}

impl Default for TripProgress {
    /// Before any location fix is processed, only the declared initial step
    /// values are known, and the reducer reports its full length as ahead.
    fn default() -> Self {
        Self {
            distance_to_next_maneuver_meters: 0.0,
            distance_remaining_meters: 0.0,
            duration_remaining_seconds: 0.0,
        }
    }
}

/// The full navigable state of an active trip.
///
/// The reducer is the only writer; see [`crate::reducer`] for the event
/// semantics. Steps are held as traversal-ordered indices into the route
/// (never a duplicated step list) so the record stays small and the leg
/// attribution survives.
#[derive(Debug, Clone, PartialEq)]
pub struct TripNavigationState {
    /// Current phase of the trip.
    pub phase: NavigationPhase,
    /// The route being navigated.
    pub route: RouteData,
    /// All steps across every leg, flattened in traversal order.
    pub step_positions: Vec<RouteStepPosition>,
    /// Index into [`Self::step_positions`] of the step currently traversed.
    pub current_step_position: usize,
    /// Progress along the remaining trip.
    pub progress: TripProgress,
    /// Position snapped onto the current step's shape; `None` until the first
    /// location update has been processed.
    pub snapped_coordinate: Option<GeographicCoordinate>,
    /// The trip's effective course, in degrees clockwise from true north, as
    /// stamped by the fix-processing pass and the trip's own
    /// [`crate::course_filtering::CourseFiltering`] policy: the raw fix's
    /// course verbatim under [`crate::course_filtering::CourseFiltering::Raw`],
    /// the route line's bearing at the snapped position under
    /// [`crate::course_filtering::CourseFiltering::SnapToRoute`] — or the
    /// policy's documented fall-back. `None` until the first processed fix
    /// (or the first one carrying / yielding a course).
    pub course_heading_degrees: Option<f64>,
    /// Active deviation, as judged by the
    /// [`DeviationDetector`](crate::deviation::DeviationDetector). The phase
    /// [`NavigationPhase::OffRoute`] always carries a deviation graded
    /// [`DeviationGrade::FullyOffRoute`] here, and such a grade always keeps
    /// the phase `OffRoute`: the fully-off grade is exactly the freezing one.
    /// A deviation graded [`DeviationGrade::OffStepOnRoute`] is recorded while
    /// the phase stays `Navigating` (no freeze; see [`crate::reducer`]); the
    /// deviation is `None` on an ordinary navigating or arrived trip.
    pub deviation: Option<DeviationInfo>,
}

impl TripNavigationState {
    /// Flattens a route's steps into traversal-ordered positions across legs.
    ///
    /// Steps are ordered leg-major: every step of leg zero comes before every
    /// step of leg one, matching how a device actually traverses a route.
    pub fn build_step_positions(route: &RouteData) -> Vec<RouteStepPosition> {
        let mut positions = Vec::new();
        for (leg_index, leg) in route.legs.iter().enumerate() {
            for step_index in 0..leg.steps.len() {
                positions.push(RouteStepPosition {
                    leg_index,
                    step_index,
                });
            }
        }
        positions
    }

    /// The step currently being traversed, or `None` for a route without steps.
    pub fn current_step(&self) -> Option<&RouteStep> {
        self.step_at(self.current_step_position)
    }

    /// Looks up the step recorded at a traversal position.
    pub fn step_at(&self, position: usize) -> Option<&RouteStep> {
        self.step_positions
            .get(position)
            .and_then(|step_position| self.step_for_position(step_position))
    }

    /// Resolves a flattened position into the route's actual step record.
    pub fn step_for_position(&self, position: &RouteStepPosition) -> Option<&RouteStep> {
        self.route
            .legs
            .get(position.leg_index)
            .and_then(|leg| leg.steps.get(position.step_index))
    }

    /// Whether the trip currently sits on the final step of the route.
    pub fn is_on_final_step(&self) -> bool {
        self.current_step_position + 1 >= self.step_positions.len()
    }
}

#[cfg(test)]
mod tests {
    use geospatial_types::{
        BoundingBox, GeographicCoordinate, ManeuverType, RouteData, RouteLeg, RouteStep,
    };

    use super::{NavigationPhase, RouteStepPosition, TripNavigationState, TripProgress};
    use crate::test_support::{
        TEST_ORIGIN_LATITUDE_DEGREES, TEST_ORIGIN_LONGITUDE_DEGREES, offset_coordinate, test_origin,
    };

    /// Length, in meters, of each synthetic step.
    const TEST_STEP_LENGTH_METERS: f64 = 500.0;
    /// Declared duration, in seconds, of each synthetic step.
    const TEST_STEP_DURATION_SECONDS: f64 = 20.0;

    fn step_with_maneuver(maneuver_type: ManeuverType) -> RouteStep {
        RouteStep {
            distance_meters: TEST_STEP_LENGTH_METERS,
            duration_seconds: TEST_STEP_DURATION_SECONDS,
            maneuver_type,
            instruction_display_text: "Turn".to_string(),
            instruction_speech_text: "Turn".to_string(),
            shape: vec![
                test_origin(),
                offset_coordinate(TEST_STEP_LENGTH_METERS, 0.0),
            ],
            lane_guidances: Vec::new(),
        }
    }

    fn two_leg_route() -> RouteData {
        let first_leg = RouteLeg {
            distance_meters: TEST_STEP_LENGTH_METERS * 2.0,
            duration_seconds: TEST_STEP_DURATION_SECONDS * 2.0,
            steps: vec![
                step_with_maneuver(ManeuverType::Departure),
                step_with_maneuver(ManeuverType::Turn(geospatial_types::TurnDirection::Left)),
            ],
        };
        let second_leg = RouteLeg {
            distance_meters: TEST_STEP_LENGTH_METERS,
            duration_seconds: TEST_STEP_DURATION_SECONDS,
            steps: vec![step_with_maneuver(ManeuverType::Arrival)],
        };
        RouteData {
            distance_meters: TEST_STEP_LENGTH_METERS * 3.0,
            duration_seconds: TEST_STEP_DURATION_SECONDS * 3.0,
            bounding_box: BoundingBox {
                south_west: GeographicCoordinate {
                    latitude: TEST_ORIGIN_LATITUDE_DEGREES,
                    longitude: TEST_ORIGIN_LONGITUDE_DEGREES,
                },
                north_east: GeographicCoordinate {
                    latitude: TEST_ORIGIN_LATITUDE_DEGREES,
                    longitude: TEST_ORIGIN_LONGITUDE_DEGREES,
                },
            },
            geometry: Vec::new(),
            legs: vec![first_leg, second_leg],
        }
    }

    #[test]
    fn flattening_preserves_leg_and_step_indices_in_traversal_order() {
        let route = two_leg_route();
        let positions = TripNavigationState::build_step_positions(&route);
        assert_eq!(positions.len(), 3);
        assert_eq!(
            positions[0],
            RouteStepPosition {
                leg_index: 0,
                step_index: 0
            }
        );
        assert_eq!(
            positions[1],
            RouteStepPosition {
                leg_index: 0,
                step_index: 1
            }
        );
        assert_eq!(
            positions[2],
            RouteStepPosition {
                leg_index: 1,
                step_index: 0
            }
        );
    }

    #[test]
    fn step_looking_up_resolves_the_route_records() {
        let route = two_leg_route();
        let step_positions = TripNavigationState::build_step_positions(&route);
        let state = TripNavigationState {
            phase: NavigationPhase::Navigating,
            route,
            step_positions,
            current_step_position: 2,
            progress: TripProgress::default(),
            deviation: None,
            snapped_coordinate: None,
            course_heading_degrees: None,
        };
        assert!(state.is_on_final_step());
        let arrival_step = state.current_step().unwrap();
        assert_eq!(arrival_step.maneuver_type, ManeuverType::Arrival);
    }

    #[test]
    fn empty_route_flattens_to_no_positions() {
        let route = RouteData {
            distance_meters: 0.0,
            duration_seconds: 0.0,
            bounding_box: BoundingBox {
                south_west: GeographicCoordinate {
                    latitude: TEST_ORIGIN_LATITUDE_DEGREES,
                    longitude: TEST_ORIGIN_LONGITUDE_DEGREES,
                },
                north_east: GeographicCoordinate {
                    latitude: TEST_ORIGIN_LATITUDE_DEGREES,
                    longitude: TEST_ORIGIN_LONGITUDE_DEGREES,
                },
            },
            geometry: Vec::new(),
            legs: vec![RouteLeg {
                distance_meters: 0.0,
                duration_seconds: 0.0,
                steps: Vec::new(),
            }],
        };
        assert!(TripNavigationState::build_step_positions(&route).is_empty());
    }
}
