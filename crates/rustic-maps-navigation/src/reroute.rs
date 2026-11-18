//! Reroute-recalculation hysteresis: the gates a reroute request must pass
//! before it may begin.
//!
//! A fully off-route excursion asks the integrator for a replacement route,
//! but the request itself is an expensive asynchronous calculation that must
//! not be spammed by jittery fixes. Three hysteresis gates stand between the
//! signal and the request, plus their observable in-flight readback:
//!
//! - **Request-in-flight gate.** While one recalculation is still in flight,
//!   a new one is refused; the integrator settles the request (applies or
//!   abandons it) before the gates open again.
//! - **Cooldown.** A new calculation waits out
//!   [`ReroutePolicy::recalculation_cooldown_seconds`] of wall clock since
//!   the last one *began* (a fix jittering in and out of the route within the
//!   cooldown window cannot fire repeated requests).
//! - **Movement threshold.** A new calculation also waits until the raw
//!   position has moved at least
//!   [`ReroutePolicy::recalculation_movement_meters`] from the fix current at
//!   the last calculation's beginning (`Raw` fixes, not the snapped position:
//!   a fully off-route trip freezes its snap, so a movement gate read off the
//!   snap could never be satisfied while it is most needed).
//!
//! The gate is stateful and wall-clock driven, deliberately outside the
//! reducer: the navigation reducer stays a pure, clock-free event function,
//! and the controller holds the gate beside the session and evaluates it with
//! the fix instant, as it does for
//! [`crate::speech::SpeechInstructionPlanner`]. The caller supplies `now` to
//! [`RerouteRecalculationGate::begin_calculation`], which also makes the gates
//! deterministic in tests.

use web_time::Instant;

use geospatial_types::GeographicCoordinate;

/// The default minimum wall-clock seconds between the beginnings of two
/// reroute calculations.
pub const DEFAULT_REROUTE_RECALCULATION_COOLDOWN_SECONDS: f64 = 5.0;

/// The default minimum displacement, in meters, between the raw fix current
/// at a calculation's beginning and the one current at the previous
/// calculation's beginning.
pub const DEFAULT_REROUTE_RECALCULATION_MOVEMENT_METERS: f64 = 50.0;

/// The reroute-recalculation hysteresis policy values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReroutePolicy {
    /// Minimum wall-clock seconds between the beginnings of two reroute
    /// calculations.
    pub recalculation_cooldown_seconds: f64,
    /// Minimum displacement, in meters, between the raw position fix current
    /// at a calculation's beginning and the one current at the previous
    /// calculation's beginning.
    pub recalculation_movement_meters: f64,
}

impl Default for ReroutePolicy {
    /// The SDK's defaults
    /// ([`DEFAULT_REROUTE_RECALCULATION_COOLDOWN_SECONDS`],
    /// [`DEFAULT_REROUTE_RECALCULATION_MOVEMENT_METERS`]).
    fn default() -> Self {
        Self {
            recalculation_cooldown_seconds: DEFAULT_REROUTE_RECALCULATION_COOLDOWN_SECONDS,
            recalculation_movement_meters: DEFAULT_REROUTE_RECALCULATION_MOVEMENT_METERS,
        }
    }
}

/// Stateful per-trip reroute-recalculation gate: consumed by the exported
/// navigation controller on each reroute-request signal and reset on trip
/// start; the navigation reducer stays untouched (the gates are wall-clock
/// controller facts, never reducer state).
#[derive(Debug)]
pub struct RerouteRecalculationGate {
    /// The hysteresis policy in force (configurable by the controller; the
    /// default is [`ReroutePolicy::default`]).
    policy: ReroutePolicy,
    /// The request-in-flight flag, read back as the trip state's
    /// `is_calculating_new_route`.
    is_calculating_new_route: bool,
    /// Wall-clock instant the last recalculation began at (the cooldown's
    /// anchor).
    last_calculation_begun_at: Option<Instant>,
    /// The raw fix coordinate current at the last calculation's beginning
    /// (the movement gate's reference).
    fix_coordinate_at_last_calculation: Option<GeographicCoordinate>,
    /// The raw fix coordinate current at the most recent consumed fix (the
    /// movement gate measures the displacement to this).
    latest_fix_coordinate: Option<GeographicCoordinate>,
}

impl Default for RerouteRecalculationGate {
    /// Builds the gate with nothing in flight, no memory and the default
    /// policy, exactly like [`RerouteRecalculationGate::new`].
    fn default() -> Self {
        Self::new()
    }
}

impl RerouteRecalculationGate {
    /// Builds the gate backed by the default [`ReroutePolicy`].
    pub fn new() -> Self {
        Self::with_policy(ReroutePolicy::default())
    }

    /// Builds the gate backed by the given policy.
    pub fn with_policy(policy: ReroutePolicy) -> Self {
        Self {
            policy,
            is_calculating_new_route: false,
            last_calculation_begun_at: None,
            fix_coordinate_at_last_calculation: None,
            latest_fix_coordinate: None,
        }
    }

    /// Swaps the hysteresis policy in force, keeping the gate's memory as it
    /// stands (reconfiguring mid-trip never silently resets an in-flight
    /// window or a cooldown the caller may still read on).
    pub fn set_policy(&mut self, policy: ReroutePolicy) {
        self.policy = policy;
    }

    /// Forgets all gate memory (trip start): nothing is in flight, no
    /// cooldown runs, no fix reference is held. The policy in force is kept —
    /// it is a controller configuration, not a trip memory.
    pub fn reset(&mut self) {
        self.is_calculating_new_route = false;
        self.last_calculation_begun_at = None;
        self.fix_coordinate_at_last_calculation = None;
        self.latest_fix_coordinate = None;
    }

    /// Records the raw fix coordinate of one consumed position fix (the
    /// movement gate's current side). Call it for every fix the controller
    /// consumes, whether or not a calculation is in flight.
    ///
    /// # Parameters
    /// - `fix_coordinate` — the raw fix, before route snapping.
    pub fn record_fix(&mut self, fix_coordinate: GeographicCoordinate) {
        self.latest_fix_coordinate = Some(fix_coordinate);
    }

    /// Whether a recalculation is currently in flight (the trip state's
    /// `is_calculating_new_route` readback).
    ///
    /// # Returns
    /// `true` between a successful [`Self::begin_calculation`] and its
    /// [`Self::settle_calculation`].
    pub fn is_calculating_new_route(&self) -> bool {
        self.is_calculating_new_route
    }

    /// Applies the gates to one reroute-request signal.
    ///
    /// # Parameters
    /// - `now` — the instant of the fix that raised the request.
    ///
    /// # Returns
    /// `true` when every gate passed and a calculation is now tracked in
    /// flight: the caller owes a settle (the reroute event or a cancellation).
    /// `false` when the request was refused by the first gate that failed (in
    /// flight, then cooldown, then movement); the gate's memory is untouched.
    pub fn begin_calculation(&mut self, now: Instant) -> bool {
        // The request-in-flight gate.
        if self.is_calculating_new_route {
            return false;
        }
        // The cooldown gate: a calculation begun strictly inside the
        // cooldown window refuses (the window's edge passes).
        if let Some(last_calculation_begun_at) = self.last_calculation_begun_at {
            let elapsed_seconds = now
                .saturating_duration_since(last_calculation_begun_at)
                .as_secs_f64();
            if elapsed_seconds < self.policy.recalculation_cooldown_seconds {
                return false;
            }
        }
        // The movement gate: the raw fix must have moved at least the
        // threshold from the last calculation's reference fix. Without a
        // reference (no fix had been consumed when the last calculation began,
        // or this is the trip's first attempt) the gate passes — there is no
        // movement yet that a gate could demand; the trip has still not had
        // its first request.
        if let (Some(fix_coordinate_at_last_calculation), Some(latest_fix_coordinate)) = (
            self.fix_coordinate_at_last_calculation,
            self.latest_fix_coordinate,
        ) {
            let displacement_meters = latest_fix_coordinate
                .haversine_distance_meters(&fix_coordinate_at_last_calculation);
            if displacement_meters < self.policy.recalculation_movement_meters {
                return false;
            }
        }
        // All gates passed: a calculation is in flight, anchored at the fix
        // instant and the fix current now.
        self.is_calculating_new_route = true;
        self.last_calculation_begun_at = Some(now);
        self.fix_coordinate_at_last_calculation = self.latest_fix_coordinate;
        true
    }

    /// Settles the in-flight calculation: the caller applied the replacement
    /// route or abandoned the request, and the request-in-flight gate reopens.
    ///
    /// The cooldown keeps running from the calculation's beginning and the
    /// movement reference stays where it was, so the next request still waits
    /// out both; settling never reads as movement.
    pub fn settle_calculation(&mut self) {
        self.is_calculating_new_route = false;
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use geospatial_types::GeographicCoordinate;

    use super::{
        DEFAULT_REROUTE_RECALCULATION_COOLDOWN_SECONDS,
        DEFAULT_REROUTE_RECALCULATION_MOVEMENT_METERS, ReroutePolicy, RerouteRecalculationGate,
    };

    /// Latitude, in degrees, of the synthetic gate fixtures (the easterly
    /// offsets below ride the parallel, so the haversine displacement of a
    /// 60-meter easterly offset stays clear of every boundary comparison).
    const TEST_LATITUDE_DEGREES: f64 = 47.0;
    /// Longitude, in degrees, of the synthetic gate fixtures' origin.
    const TEST_LONGITUDE_DEGREES: f64 = 8.0;
    /// Earth's mean radius, in meters (equirectangular fixture offsets).
    const EARTH_RADIUS_METERS: f64 = 6_371_008.8;
    /// A policy with both hysteresis gates configured away (isolating the
    /// remaining gates in the tests).
    const UNHYSTERETIC_COOLDOWN_SECONDS: f64 = 0.0;
    const UNHYSTERETIC_MOVEMENT_METERS: f64 = 0.0;
    /// Displacement, in meters, inside the default movement threshold.
    const SMALL_MOVEMENT_METERS: f64 = 40.0;
    /// Displacement, in meters, clear beyond the default movement threshold.
    const LARGE_MOVEMENT_METERS: f64 = 60.0;

    fn gate_coordinate(east_meters: f64) -> GeographicCoordinate {
        GeographicCoordinate {
            latitude: TEST_LATITUDE_DEGREES,
            longitude: TEST_LONGITUDE_DEGREES
                + (east_meters / (EARTH_RADIUS_METERS * TEST_LATITUDE_DEGREES.to_radians().cos()))
                    .to_degrees(),
        }
    }

    /// A gate with both non-in-flight gates configured away (the in-flight
    /// gate is always on: it is behavior, not a tunable).
    fn unhysteretic_gate() -> RerouteRecalculationGate {
        RerouteRecalculationGate::with_policy(ReroutePolicy {
            recalculation_cooldown_seconds: UNHYSTERETIC_COOLDOWN_SECONDS,
            recalculation_movement_meters: UNHYSTERETIC_MOVEMENT_METERS,
        })
    }

    #[test]
    fn first_attempt_passes_with_no_memory() {
        let mut gate = unhysteretic_gate();
        let now = Instant::now();
        assert!(
            gate.begin_calculation(now),
            "the trip's first attempt has no cooldown and no movement to satisfy"
        );
        assert!(gate.is_calculating_new_route());
    }

    #[test]
    fn in_flight_gate_refuses_until_the_calculation_is_settled() {
        let mut gate = unhysteretic_gate();
        let base = Instant::now();
        assert!(gate.begin_calculation(base));
        // Still in flight: refused, with the memory untouched.
        assert!(
            !gate.begin_calculation(base + Duration::from_secs(60)),
            "a request may not begin while one is in flight"
        );
        gate.settle_calculation();
        assert!(!gate.is_calculating_new_route());
        // (Both hysteresis gates are configured away here, so the very next
        // begin passes.)
        assert!(gate.begin_calculation(base + Duration::from_secs(60)));
    }

    #[test]
    fn cooldown_gate_refuses_inside_the_window_and_passes_at_the_edge() {
        let gate_policy = ReroutePolicy {
            recalculation_cooldown_seconds: DEFAULT_REROUTE_RECALCULATION_COOLDOWN_SECONDS,
            // Movement gate away: this test pins the cooldown alone.
            recalculation_movement_meters: UNHYSTERETIC_MOVEMENT_METERS,
        };
        let mut gate = RerouteRecalculationGate::with_policy(gate_policy);
        let base = Instant::now();
        assert!(gate.begin_calculation(base));
        gate.settle_calculation();

        // Strictly inside the window: refused.
        assert!(
            !gate.begin_calculation(
                base + Duration::from_secs_f64(
                    DEFAULT_REROUTE_RECALCULATION_COOLDOWN_SECONDS - 1.0
                )
            ),
            "the request inside the cooldown window must be refused"
        );
        // At the window's edge: the gate passes (strictly-less-than refuses;
        // the full elapsed cooldown does not).
        assert!(
            gate.begin_calculation(
                base + Duration::from_secs_f64(DEFAULT_REROUTE_RECALCULATION_COOLDOWN_SECONDS)
            ),
            "the request at the cooldown edge must pass"
        );
    }

    #[test]
    fn movement_gate_refuses_inside_the_threshold_and_passes_at_the_edge() {
        let gate_policy = ReroutePolicy {
            // Cooldown away: this test pins the movement alone.
            recalculation_cooldown_seconds: UNHYSTERETIC_COOLDOWN_SECONDS,
            recalculation_movement_meters: DEFAULT_REROUTE_RECALCULATION_MOVEMENT_METERS,
        };
        let mut gate = RerouteRecalculationGate::with_policy(gate_policy);
        let base = Instant::now();
        gate.record_fix(gate_coordinate(0.0));
        assert!(gate.begin_calculation(base));
        gate.settle_calculation();

        // Moved 40 meters since the last calculation's reference: refused.
        gate.record_fix(gate_coordinate(SMALL_MOVEMENT_METERS));
        assert!(
            !gate.begin_calculation(base + Duration::from_secs(60)),
            "a fix that moved less than the threshold must be refused"
        );

        // Moved clear beyond the threshold from the *same reference*: the
        // next calculation passes (displacements accumulate only from the
        // last begun calculation's reference).
        gate.record_fix(gate_coordinate(LARGE_MOVEMENT_METERS));
        assert!(gate.begin_calculation(base + Duration::from_secs(60)));
    }

    #[test]
    fn policy_defaults_are_the_named_ferrostar_calibrations() {
        let default_policy = ReroutePolicy::default();
        assert_eq!(
            default_policy.recalculation_cooldown_seconds,
            DEFAULT_REROUTE_RECALCULATION_COOLDOWN_SECONDS
        );
        assert_eq!(
            default_policy.recalculation_movement_meters,
            DEFAULT_REROUTE_RECALCULATION_MOVEMENT_METERS
        );
        assert_eq!(DEFAULT_REROUTE_RECALCULATION_COOLDOWN_SECONDS, 5.0);
        assert_eq!(DEFAULT_REROUTE_RECALCULATION_MOVEMENT_METERS, 50.0);
    }

    #[test]
    fn reset_forgets_the_memory_but_keeps_the_policy() {
        let mut gate = RerouteRecalculationGate::with_policy(ReroutePolicy {
            recalculation_cooldown_seconds: UNHYSTERETIC_COOLDOWN_SECONDS,
            recalculation_movement_meters: DEFAULT_REROUTE_RECALCULATION_MOVEMENT_METERS,
        });
        let base = Instant::now();
        gate.record_fix(gate_coordinate(0.0));
        assert!(gate.begin_calculation(base));
        gate.settle_calculation();

        // With the same policy, an unmoved fix is refused by the movement
        // gate...
        gate.record_fix(gate_coordinate(0.0));
        assert!(
            !gate.begin_calculation(base + Duration::from_secs(60)),
            "an unmoved fix must be refused by the movement gate"
        );

        // ...but a reset (a new trip) forgets the movement reference, and the
        // next attempt passes even unmoved.
        gate.reset();
        assert!(gate.begin_calculation(base + Duration::from_secs(60)));
    }

    #[test]
    fn set_policy_keeps_the_memory() {
        let mut gate = unhysteretic_gate();
        let base = Instant::now();
        gate.record_fix(gate_coordinate(0.0));
        assert!(gate.begin_calculation(base));
        gate.settle_calculation();
        // Reconfiguring mid-trip with hysteresis keeps the memory: the
        // unmoved fix is now refused by the swapped-in movement gate.
        gate.set_policy(ReroutePolicy {
            recalculation_cooldown_seconds: UNHYSTERETIC_COOLDOWN_SECONDS,
            recalculation_movement_meters: DEFAULT_REROUTE_RECALCULATION_MOVEMENT_METERS,
        });
        assert!(
            !gate.begin_calculation(base + Duration::from_secs(60)),
            "reconfiguring must not clear the movement reference"
        );
    }
}
