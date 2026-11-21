//! Recorded navigation-trace fixtures powering the end-to-end navigation
//! replay tests ([`crate::trace_replay_tests`]): the loader, the trace
//! document format, the recorded route fixtures and the deterministic fix
//! generator behind the committed files.
//!
//! # Provenance and honesty
//!
//! A trace is **recorded route geometry plus synthetic fixes**, never a live
//! GPS capture:
//!
//! - The `route` section carries geography recorded from the routing provider.
//!   Each scenario's start route is a committed
//!   [`RecordedRouteFixtureDocument`] under `fixtures/navigation/routes/`,
//!   recorded from the shared `platform/reference-scenes.json` `demo_routes`
//!   artifact — the same pedestrian routes the reference applications plan
//!   with. The generator never synthesizes a street: the fixture is the
//!   recorded step shapes stitched at their shared vertices, with the
//!   declared distances recomputed from the resulting polyline.
//! - The `samples` are **synthetic**. [`generate_trace_document`] places them
//!   along the recorded route with a seeded SplitMix64 pseudorandom walk and
//!   radially clamped Gaussian fix noise. The seed is recorded in each trace's
//!   metadata and honored by the regeneration test.
//! - The reroute scenario's `reroute_route` is derived from the same recorded
//!   geometry: the recorded route's remaining polyline from the reducer's
//!   frozen snapped coordinate. Its first vertex is exactly that snapped
//!   coordinate and its final vertex is the recorded route's own destination,
//!   so the anchoring and destination contracts hold by construction without
//!   inventing a street.
//!
//! # Trace file format
//!
//! One JSON document per scenario in `fixtures/navigation/traces/`, named
//! `<scenario_name>.json` (see the README next to the files):
//!
//! ```json
//! {
//!   "metadata": {
//!     "scenario_name": "sacramento_clean_walk",
//!     "provenance": "…",
//!     "generator_seed": 2026102701,
//!     "deviation_threshold_meters": 25.0,
//!     "horizontal_accuracy_meters": 8.0,
//!     "sample_interval_seconds": 5.0
//!   },
//!   "route": { "distance_meters": …, … serialized `RouteData` … },
//!   "reroute_route": { … serialized `RouteData` … },
//!   "samples": [
//!     {
//!       "latitude": …,
//!       "longitude": …,
//!       "horizontal_accuracy_meters": …,
//!       "timestamp_offset_seconds": …
//!     }
//!   ]
//! }
//! ```
//!
//! - `route` is the recorded route the replayed trip starts on.
//! - `reroute_route` is present exactly on the reroute scenario: the
//!   replacement route the replay hands to
//!   [`NavigationEvent::RerouteProvided`] the moment the trip is first judged
//!   off route. It starts at the exact snapped coordinate the reducer holds at
//!   that moment and ends at the recorded route's destination.
//! - Every sample carries a strictly increasing timestamp offset; the replay
//!   feeds each fix at that generated offset rather than inventing timing.
//!   Samples are generated at a fixed cadence (every
//!   [`SAMPLE_SPACING_METERS`] of travel, [`SAMPLE_INTERVAL_SECONDS`] apart),
//!   which keeps the committed files to tens of kilobytes while giving
//!   several fixes per step to the step-advance and instruction assertions.
//! - All fixes carry the same metadata-identical accuracy report; the noise
//!   model (clamped radially to [`MAXIMUM_FIX_NOISE_METERS`]) is deliberately
//!   inside that envelope and far inside the deviation threshold, so the
//!   scenarios deviate (or not) by design rather than by luck.
//!
//! # Drift gates
//!
//! Two always-on tests keep the committed data honest:
//!
//! - `committed_route_fixtures_match_the_recorded_demo_routes` compares every
//!   committed route fixture's vertices, in order, against the corresponding
//!   `platform/reference-scenes.json` `demo_routes` entry. Re-recording that
//!   artifact without re-recording the fixtures fails the test instead of
//!   silently diverging.
//! - `regenerated_trace_bytes_match_the_committed_fixtures` proves each trace
//!   is exactly what the generator produces from its recorded seed and route
//!   fixture.
//!
//! # Determinism and regeneration
//!
//! After changing the generator (its seeds, constants or excursion profile),
//! rewrite the committed traces with:
//!
//! ```sh
//! cargo test -p rustic-maps-navigation regenerate_trace_fixtures -- --ignored
//! ```
//!
//! After re-recording `demo_routes`, rewrite the committed route fixtures (and
//! then the traces) with:
//!
//! ```sh
//! cargo test -p rustic-maps-navigation regenerate_recorded_route_fixtures -- --ignored
//! cargo test -p rustic-maps-navigation regenerate_trace_fixtures -- --ignored
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use geospatial_types::{BoundingBox, GeographicCoordinate, RouteData, RouteLeg, RouteStep};

use crate::course_filtering::CourseFiltering;
use crate::deviation::{DEFAULT_DEVIATION_DISTANCE_THRESHOLD_METERS, DistanceDeviationDetector};
use crate::geometry::{EARTH_RADIUS_METERS, snap_coordinate_to_polyline};
use crate::location::NavigationLocationUpdate;
use crate::reducer::{NavigationEvent, reduce_navigation_event};
use crate::state::NavigationPhase;
use crate::step_advance::DistanceToEndOfStepCondition;

/// Scenario with a clean walk along the whole route: no deviation, the step
/// sequence advances in order, and the replay ends arrived.
pub(crate) const CLEAN_TRACE_SCENARIO: &str = "sacramento_clean_walk";

/// Scenario with an excursion off route that returns to the route line: the
/// trip freezes `OffRoute`, then a graded fix recovers it and re-anchors the
/// projection mid-step.
pub(crate) const RECOVERY_TRACE_SCENARIO: &str = "sacramento_deviation_recovery";

/// Scenario with an excursion that is answered by a reroute: the replay
/// issues [`NavigationEvent::RerouteProvided`] at the first off-route fix and
/// the remaining recorded samples drive the replacement route.
pub(crate) const REROUTE_TRACE_SCENARIO: &str = "sacramento_deviation_reroute";

/// Every committed trace scenario, in the order the shared tests visit them.
pub(crate) const ALL_TRACE_SCENARIOS: [&str; 3] = [
    CLEAN_TRACE_SCENARIO,
    RECOVERY_TRACE_SCENARIO,
    REROUTE_TRACE_SCENARIO,
];

/// Walking speed, in meters per second, that maps the recorded fix spacing to
/// a recording interval. The route fixtures keep the provider's own declared
/// durations; this speed only sets the synthetic fix cadence.
const SAMPLE_WALKING_SPEED_METERS_PER_SECOND: f64 = 2.0;

/// Travel spacing, in meters, between two consecutive recorded fixes.
///
/// This cadence (with [`SAMPLE_INTERVAL_SECONDS`]) keeps a replayed trace at
/// a few hundred samples at most while still giving the eager step-advance
/// condition and the instruction bands several fixes per step.
pub(crate) const SAMPLE_SPACING_METERS: f64 = 10.0;

/// Recording interval, in seconds, between two consecutive fixes (= the
/// spacing divided by the assumed walking speed).
pub(crate) const SAMPLE_INTERVAL_SECONDS: f64 =
    SAMPLE_SPACING_METERS / SAMPLE_WALKING_SPEED_METERS_PER_SECOND;

/// Standard deviation, in meters, of the seeded Gaussian fix noise per planar
/// component (along and across the route line).
const FIX_NOISE_SIGMA_METERS: f64 = 2.0;

/// Radial clamp, in meters, of the generated fix noise (three standard
/// deviations of [`FIX_NOISE_SIGMA_METERS`]): every fix stays within this
/// radius of its true walk position in every direction.
pub(crate) const MAXIMUM_FIX_NOISE_METERS: f64 = 3.0 * FIX_NOISE_SIGMA_METERS;

/// Horizontal accuracy, in meters, recorded for every synthetic fix: wider
/// than the clamped noise bound (so the fix can never leave its own accuracy
/// envelope), inside the step-advance conditions' 15 m accuracy gate, and far
/// inside the 25 m deviation threshold plus this accuracy — so walks advance
/// and stay on route (or deviate) by design, never by noise luck.
const TRACE_FIX_ACCURACY_METERS: f64 = 8.0;

/// Lateral excursion, in meters, of the deviation scenarios: the walker moves
/// this far to the left of the route line, comfortably beyond the default
/// deviation allowance (threshold plus accuracy).
pub(crate) const EXCURSION_METERS: f64 = 60.0;

/// Along-route position, in meters, where the recovery scenario's excursion
/// starts.
const EXCURSION_PLATEAU_START_ALONG_METERS: f64 = 60.0;

/// Along-route position, in meters, where the recovery scenario's excursion
/// plateau ends and the return ramp begins.
const EXCURSION_PLATEAU_END_ALONG_METERS: f64 = 120.0;

/// Along-route position, in meters, where the recovery scenario's excursion
/// has fully rejoined the route line.
const EXCURSION_REJOIN_ALONG_METERS: f64 = 170.0;

/// Along-route position, in meters, of the reroute scenario's single
/// deviating fix.
const REROUTE_DEVIATION_ALONG_METERS: f64 = 280.0;

/// Seed of the clean-walk scenario's noise stream (an arbitrary fixed value;
/// recorded in the trace's metadata and honored by the generator).
const CLEAN_TRACE_SEED: u64 = 2_026_102_701;

/// Seed of the deviation-and-recovery scenario's noise stream.
const RECOVERY_TRACE_SEED: u64 = 2_026_102_702;

/// Seed of the deviation-and-reroute scenario's noise stream.
const REROUTE_TRACE_SEED: u64 = 2_026_102_703;

/// Committed recorded-route fixture the clean-walk scenario walks: the
/// provider's recorded pedestrian route from Capitol Park to Tower Bridge in
/// downtown Sacramento (from the shared artifact's `demo_routes` member).
pub(crate) const CLEAN_ROUTE_FIXTURE_FILE_NAME: &str = "capitol_park_to_tower_bridge.json";

/// Committed recorded-route fixture the deviation-recovery scenario walks: the
/// provider's recorded pedestrian route from Golden 1 Center to Capitol Park,
/// whose long first step holds the whole excursion (the recovery must rejoin
/// before the first maneuver, or the eager advance can no longer fire).
pub(crate) const RECOVERY_ROUTE_FIXTURE_FILE_NAME: &str = "golden_1_center_to_capitol_park.json";

/// Committed recorded-route fixture the deviation-and-reroute scenario starts
/// on: the provider's recorded pedestrian route from Tower Bridge to Capitol
/// Park (the reverse walk, so the reroute demo visits different streets).
pub(crate) const REROUTE_ROUTE_FIXTURE_FILE_NAME: &str = "tower_bridge_to_capitol_park.json";

/// One recorded route fixture source: the committed file and the ordered demo
/// place pair whose recorded route (in `platform/reference-scenes.json`'s
/// `demo_routes`) it derives from.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RecordedRouteFixtureSource {
    /// File name under `fixtures/navigation/routes/`.
    pub(crate) file_name: &'static str,
    /// Recorded origin place name, as spelled in the artifact.
    pub(crate) origin_name: &'static str,
    /// Recorded destination place name, as spelled in the artifact.
    pub(crate) destination_name: &'static str,
}

/// Every committed recorded-route fixture, in regeneration order.
pub(crate) const ALL_RECORDED_ROUTE_FIXTURE_SOURCES: [RecordedRouteFixtureSource; 3] = [
    RecordedRouteFixtureSource {
        file_name: CLEAN_ROUTE_FIXTURE_FILE_NAME,
        origin_name: "Capitol Park",
        destination_name: "Tower Bridge",
    },
    RecordedRouteFixtureSource {
        file_name: RECOVERY_ROUTE_FIXTURE_FILE_NAME,
        origin_name: "Golden 1 Center",
        destination_name: "Capitol Park",
    },
    RecordedRouteFixtureSource {
        file_name: REROUTE_ROUTE_FIXTURE_FILE_NAME,
        origin_name: "Tower Bridge",
        destination_name: "Capitol Park",
    },
];

/// Provenance statement recorded in a trace's metadata: it names the recorded
/// route pair, the synthetic-fix generator and the regeneration command, so a
/// reader of the JSON alone learns exactly which part is recorded and which is
/// synthesized.
fn trace_provenance_statement(origin_name: &str, destination_name: &str) -> String {
    format!(
        "recorded route geometry with deterministic synthetic fixes: the route is the \
provider-recorded pedestrian route {origin_name} to {destination_name} from \
platform/reference-scenes.json's demo_routes member (recorded MapQuest geometry, stitched \
at its shared step vertices, declared distances recomputed); the fixes are generated along \
it from the seed in this metadata by rustic-maps-navigation's trace_fixture generator (SplitMix64 \
PRNG, radially clamped Gaussian fix noise); no live GPS capture. Regenerate with: \
cargo test -p rustic-maps-navigation regenerate_trace_fixtures -- --ignored"
    )
}

/// Tolerance, in meters, when validating a fixture's declared route geometry
/// against its own polyline lengths (the generator declares distances from
/// the same haversine arithmetic, so equality holds well inside this bound).
const FIXTURE_GEOMETRY_TOLERANCE_METERS: f64 = 1e-6;

/// Tight tolerance, in meters, of the recorded-route drift gate: a committed
/// route fixture may differ from the shared artifact's recorded vertices by at
/// most this much (the regeneration seam writes the artifact's own doubles, so
/// real drift is orders of magnitude larger).
const ROUTE_FIXTURE_COORDINATE_TOLERANCE_METERS: f64 = 0.01;

/// Tolerance for validating plain float fields that pass through the JSON
/// round-trip unchanged (serde_json's shortest-roundtrip formatting is exact).
const FIXTURE_FLOAT_TOLERANCE: f64 = 1e-9;

/// Segments shorter than this, in meters, are treated as points when walking
/// a planar polyline: a recorded route never produces them, but the guard
/// keeps the parametric position finite if one ever appears.
const MINIMUM_WALK_SEGMENT_LENGTH_METERS: f64 = 0.01;

/// A committed recorded-route fixture: the ordered demo place pair whose
/// provider-recorded route the file carries, plus that route.
///
/// One file per distinct recorded pair lives under
/// `fixtures/navigation/routes/`. The route is the provider's recorded step
/// geometry stitched at the steps' shared vertices, with the declared distances
/// and durations made self-consistent with that stitched polyline (see the
/// regeneration test); no coordinate is invented.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct RecordedRouteFixtureDocument {
    /// Recorded origin place name, as spelled in the shared artifact.
    pub(crate) origin_name: String,
    /// Recorded destination place name, as spelled in the shared artifact.
    pub(crate) destination_name: String,
    /// The recorded route between them.
    pub(crate) route: RouteData,
}

/// The serialized trace document exactly as it is committed on disk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct NavigationTraceDocument {
    /// Scenario metadata (seed, thresholds, provenance).
    pub(crate) metadata: TraceMetadataDocument,
    /// The route the trip starts on.
    pub(crate) route: RouteData,
    /// The replacement route, present exactly on the reroute scenario.
    pub(crate) reroute_route: Option<RouteData>,
    /// The recorded fixes in replay order.
    pub(crate) samples: Vec<TraceSampleDocument>,
}

/// The `metadata` object of a trace document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct TraceMetadataDocument {
    /// The scenario this trace implements; also the file name's stem.
    pub(crate) scenario_name: String,
    /// The provenance statement (see [`trace_provenance_statement`]): recorded
    /// route geometry, synthetic fixes, no live capture.
    pub(crate) provenance: String,
    /// Seed the noise stream was generated from (non-zero).
    pub(crate) generator_seed: u64,
    /// Deviation threshold, in meters, the trace was generated against; the
    /// loader requires it to match the shipped detector's default.
    pub(crate) deviation_threshold_meters: f64,
    /// Horizontal accuracy, in meters, recorded on every sample.
    pub(crate) horizontal_accuracy_meters: f64,
    /// Recording interval, in seconds, between consecutive fixes.
    pub(crate) sample_interval_seconds: f64,
}

/// One recorded fix of a trace document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct TraceSampleDocument {
    /// Raw fix latitude, in degrees.
    pub(crate) latitude: f64,
    /// Raw fix longitude, in degrees.
    pub(crate) longitude: f64,
    /// Horizontal accuracy radius, in meters, of the fix.
    pub(crate) horizontal_accuracy_meters: f64,
    /// Offset, in seconds, from the replay's start instant.
    pub(crate) timestamp_offset_seconds: f64,
}

/// A validated trace: the loaded fixture the replay tests consume.
///
/// Built only through [`load_trace_fixture`] (or `parse` for malformed-input
/// tests); the loader validates the document hard before accepting it.
#[derive(Debug, Clone)]
pub(crate) struct NavigationTraceFixture {
    scenario_name: String,
    generator_seed: u64,
    deviation_threshold_meters: f64,
    sample_accuracy_meters: f64,
    sample_interval_seconds: f64,
    route: RouteData,
    reroute_route: Option<RouteData>,
    samples: Vec<TraceSample>,
}

/// One validated recorded fix.
#[derive(Debug, Clone)]
pub(crate) struct TraceSample {
    /// Raw fix position.
    pub(crate) coordinate: GeographicCoordinate,
    /// Horizontal accuracy radius, in meters, of the fix.
    pub(crate) horizontal_accuracy_meters: f64,
    /// Offset, in seconds, from the replay's start instant.
    pub(crate) timestamp_offset_seconds: f64,
}

impl NavigationTraceFixture {
    /// Parses and validates a trace document from its JSON source.
    ///
    /// Errors carry the source name (the file path in the committed case) and
    /// the exact expectation that failed, so a malformed fixture is
    /// immediately actionable.
    pub(crate) fn parse(document_json: &str, source_name: &str) -> Result<Self, String> {
        let document: NavigationTraceDocument =
            serde_json::from_str(document_json).map_err(|parse_error| {
                format!("{source_name}: not a valid navigation trace document: {parse_error}")
            })?;
        Self::from_document(document, source_name)
    }

    /// Validates an already-parsed trace document.
    pub(crate) fn from_document(
        document: NavigationTraceDocument,
        source_name: &str,
    ) -> Result<Self, String> {
        validate_trace_metadata(&document.metadata, source_name)?;
        validate_route(&document.route, "route", source_name)?;
        if let Some(reroute_route) = &document.reroute_route {
            validate_route(reroute_route, "reroute_route", source_name)?;
        }
        validate_samples(&document.samples, source_name)?;

        let samples = document
            .samples
            .iter()
            .map(|sample_document| TraceSample {
                coordinate: GeographicCoordinate {
                    latitude: sample_document.latitude,
                    longitude: sample_document.longitude,
                },
                horizontal_accuracy_meters: sample_document.horizontal_accuracy_meters,
                timestamp_offset_seconds: sample_document.timestamp_offset_seconds,
            })
            .collect();
        Ok(Self {
            scenario_name: document.metadata.scenario_name,
            generator_seed: document.metadata.generator_seed,
            deviation_threshold_meters: document.metadata.deviation_threshold_meters,
            sample_accuracy_meters: document.metadata.horizontal_accuracy_meters,
            sample_interval_seconds: document.metadata.sample_interval_seconds,
            route: document.route,
            reroute_route: document.reroute_route,
            samples,
        })
    }

    /// The scenario this fixture implements, as recorded in its metadata.
    pub(crate) fn scenario_name(&self) -> &str {
        &self.scenario_name
    }

    /// The seed recorded in the metadata (the loader does not generate; the
    /// determinism test compares regeneration output against the file).
    pub(crate) fn generator_seed(&self) -> u64 {
        self.generator_seed
    }

    /// The deviation threshold, in meters, the trace was generated against.
    pub(crate) fn deviation_threshold_meters(&self) -> f64 {
        self.deviation_threshold_meters
    }

    /// The horizontal accuracy, in meters, recorded on every sample.
    pub(crate) fn sample_accuracy_meters(&self) -> f64 {
        self.sample_accuracy_meters
    }

    /// The recording interval, in seconds, between consecutive fixes.
    pub(crate) fn sample_interval_seconds(&self) -> f64 {
        self.sample_interval_seconds
    }

    /// The route the trip starts on.
    pub(crate) fn route(&self) -> &RouteData {
        &self.route
    }

    /// The replacement route, when the scenario answers a deviation with a
    /// reroute.
    pub(crate) fn reroute_route(&self) -> Option<&RouteData> {
        self.reroute_route.as_ref()
    }

    /// The recorded fixes in replay order.
    pub(crate) fn samples(&self) -> &[TraceSample] {
        &self.samples
    }

    /// The recorded fixes as reducer location updates, replayed timing
    /// included (the offset becomes the fix's timestamp).
    pub(crate) fn iter_location_updates(
        &self,
    ) -> impl Iterator<Item = NavigationLocationUpdate> + '_ {
        self.samples.iter().map(|sample| {
            location_update_from_parts(
                sample.coordinate.latitude,
                sample.coordinate.longitude,
                sample.horizontal_accuracy_meters,
                sample.timestamp_offset_seconds,
            )
        })
    }
}

/// The committed fixture file's path for one scenario.
pub(crate) fn fixture_file_path(scenario_name: &str) -> PathBuf {
    fixture_directory_path().join(format!("{scenario_name}.json"))
}

/// The committed fixture directory, next to this crate's sources.
pub(crate) fn fixture_directory_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join("navigation")
        .join("traces")
}

/// The committed recorded-route fixture directory, beside the traces.
pub(crate) fn route_fixture_directory_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join("navigation")
        .join("routes")
}

/// The committed recorded-route fixture file's path for one file name.
pub(crate) fn route_fixture_file_path(file_name: &str) -> PathBuf {
    route_fixture_directory_path().join(file_name)
}

/// Loads and validates one committed recorded-route fixture, failing the test
/// run hard (with an actionable message) on any malformed file.
pub(crate) fn load_recorded_route_fixture(file_name: &str) -> RecordedRouteFixtureDocument {
    let fixture_path = route_fixture_file_path(file_name);
    let fixture_path_display = fixture_path.to_string_lossy();
    let document_json = std::fs::read_to_string(&fixture_path).unwrap_or_else(|read_error| {
        panic!(
            "recorded route fixture {} is unreadable: {read_error} \
             (regenerate with `cargo test -p rustic-maps-navigation \
             regenerate_recorded_route_fixtures -- --ignored`)",
            fixture_path.display()
        )
    });
    let document: RecordedRouteFixtureDocument = serde_json::from_str(&document_json)
        .unwrap_or_else(|parse_error| {
            panic!(
                "{}: not a valid recorded-route fixture document: {parse_error}",
                fixture_path.display()
            )
        });
    validate_route(&document.route, "route", &fixture_path_display)
        .unwrap_or_else(|validation_error| panic!("{validation_error}"));
    document
}

/// Loads and validates the committed trace fixture of one scenario, failing
/// the test run hard (with an actionable message) on any malformed fixture.
pub(crate) fn load_trace_fixture(scenario_name: &str) -> NavigationTraceFixture {
    let fixture_path = fixture_file_path(scenario_name);
    let fixture_path_display = fixture_path.to_string_lossy();
    let document_json = std::fs::read_to_string(&fixture_path).unwrap_or_else(|read_error| {
        panic!(
            "trace fixture {} is unreadable: {read_error} \
                 (regenerate with `cargo test -p rustic-maps-navigation \
                 regenerate_trace_fixtures -- --ignored`)",
            fixture_path.display()
        )
    });
    NavigationTraceFixture::parse(&document_json, &fixture_path_display)
        .unwrap_or_else(|validation_error| panic!("{validation_error}"))
}

/// Validates a trace document's metadata section.
fn validate_trace_metadata(
    metadata: &TraceMetadataDocument,
    source_name: &str,
) -> Result<(), String> {
    if metadata.scenario_name.is_empty() {
        return Err(format!(
            "{source_name}: metadata.scenario_name must not be empty"
        ));
    }
    if metadata.generator_seed == 0 {
        return Err(format!(
            "{source_name}: metadata.generator_seed must be non-zero so the \
             trace stays regenerable from its recorded seed"
        ));
    }
    if !metadata.deviation_threshold_meters.is_finite()
        || (metadata.deviation_threshold_meters - DEFAULT_DEVIATION_DISTANCE_THRESHOLD_METERS).abs()
            > FIXTURE_FLOAT_TOLERANCE
    {
        return Err(format!(
            "{source_name}: metadata.deviation_threshold_meters must equal the \
             shipped default ({DEFAULT_DEVIATION_DISTANCE_THRESHOLD_METERS}); \
             update the metadata or replay with a custom detector"
        ));
    }
    if !metadata.horizontal_accuracy_meters.is_finite()
        || metadata.horizontal_accuracy_meters <= 0.0
    {
        return Err(format!(
            "{source_name}: metadata.horizontal_accuracy_meters must be a \
             finite positive number"
        ));
    }
    if !metadata.sample_interval_seconds.is_finite() || metadata.sample_interval_seconds <= 0.0 {
        return Err(format!(
            "{source_name}: metadata.sample_interval_seconds must be a finite \
             positive number"
        ));
    }
    Ok(())
}

/// Validates a trace document's route-like section (the start route or the
/// replacement route): traversable legs, geometrically self-consistent
/// declared distances, and a bounding box that contains the shape.
fn validate_route(route: &RouteData, field_prefix: &str, source_name: &str) -> Result<(), String> {
    let reject = |detail: String| -> Result<(), String> {
        Err(format!("{source_name}: {field_prefix}: {detail}"))
    };
    if route.legs.is_empty() {
        return reject("the route must have at least one leg".to_string());
    }
    for (leg_index, leg) in route.legs.iter().enumerate() {
        if leg.steps.is_empty() {
            return reject(format!("legs[{leg_index}] must have at least one step"));
        }
        for (step_index, step) in leg.steps.iter().enumerate() {
            if step.shape.is_empty() {
                return reject(format!(
                    "legs[{leg_index}].steps[{step_index}] must have a shape"
                ));
            }
            if !step.distance_meters.is_finite() || step.distance_meters <= 0.0 {
                return reject(format!(
                    "legs[{leg_index}].steps[{step_index}].distance_meters must \
                     be a finite positive number"
                ));
            }
            let distance_mismatch = step_distance_from_shape(step) - step.distance_meters;
            if distance_mismatch.abs() > FIXTURE_GEOMETRY_TOLERANCE_METERS {
                return reject(format!(
                    "legs[{leg_index}].steps[{step_index}].distance_meters must \
                     equal the shape's polyline length ({}), not {}",
                    step_distance_from_shape(step),
                    step.distance_meters
                ));
            }
            if !step.duration_seconds.is_finite() || step.duration_seconds < 0.0 {
                return reject(format!(
                    "legs[{leg_index}].steps[{step_index}].duration_seconds \
                     must be a finite non-negative number"
                ));
            }
        }
    }
    validate_route_totals(route, source_name, field_prefix)?;
    validate_route_bounding_box(route, source_name, field_prefix)?;
    Ok(())
}

/// The exact polyline length, in meters, of one step's shape.
fn step_distance_from_shape(step: &RouteStep) -> f64 {
    shape_length_meters(&step.shape)
}

/// Validates the route-level and leg-level declared totals.
fn validate_route_totals(
    route: &RouteData,
    source_name: &str,
    field_prefix: &str,
) -> Result<(), String> {
    let leg_distance_sum_meters: f64 = route.legs.iter().map(|leg| leg.distance_meters).sum();
    let leg_duration_sum_seconds: f64 = route.legs.iter().map(|leg| leg.duration_seconds).sum();
    if (route.distance_meters - leg_distance_sum_meters).abs() > FIXTURE_GEOMETRY_TOLERANCE_METERS {
        return Err(format!(
            "{source_name}: {field_prefix}.distance_meters must equal the sum \
             of the leg distances"
        ));
    }
    if (route.duration_seconds - leg_duration_sum_seconds).abs() > FIXTURE_GEOMETRY_TOLERANCE_METERS
    {
        return Err(format!(
            "{source_name}: {field_prefix}.duration_seconds must equal the sum \
             of the leg durations"
        ));
    }
    for (leg_index, leg) in route.legs.iter().enumerate() {
        let step_distance_sum_meters: f64 = leg.steps.iter().map(|step| step.distance_meters).sum();
        if (leg.distance_meters - step_distance_sum_meters).abs()
            > FIXTURE_GEOMETRY_TOLERANCE_METERS
        {
            return Err(format!(
                "{source_name}: {field_prefix}.legs[{leg_index}].distance_meters \
                 must equal the sum of its step distances"
            ));
        }
    }
    Ok(())
}

/// Validates that the route's bounding box contains every shape coordinate.
fn validate_route_bounding_box(
    route: &RouteData,
    source_name: &str,
    field_prefix: &str,
) -> Result<(), String> {
    let mut minimum_latitude = f64::INFINITY;
    let mut maximum_latitude = f64::NEG_INFINITY;
    let mut minimum_longitude = f64::INFINITY;
    let mut maximum_longitude = f64::NEG_INFINITY;
    for step in route
        .legs
        .iter()
        .flat_map(|leg| leg.steps.iter())
        .flat_map(|step| step.shape.iter())
    {
        minimum_latitude = minimum_latitude.min(step.latitude);
        maximum_latitude = maximum_latitude.max(step.latitude);
        minimum_longitude = minimum_longitude.min(step.longitude);
        maximum_longitude = maximum_longitude.max(step.longitude);
    }
    let bounding_box = &route.bounding_box;
    let contains = [
        bounding_box.south_west.latitude <= minimum_latitude + FIXTURE_GEOMETRY_TOLERANCE_METERS,
        bounding_box.north_east.latitude >= maximum_latitude - FIXTURE_GEOMETRY_TOLERANCE_METERS,
        bounding_box.south_west.longitude <= minimum_longitude + FIXTURE_GEOMETRY_TOLERANCE_METERS,
        bounding_box.north_east.longitude >= maximum_longitude - FIXTURE_GEOMETRY_TOLERANCE_METERS,
    ];
    if !contains.iter().all(|holds| *holds) {
        return Err(format!(
            "{source_name}: {field_prefix}.bounding_box must contain every \
             shape coordinate"
        ));
    }
    Ok(())
}

/// Validates a trace document's samples: graded, in range, and replayed in a
/// strictly increasing order of recorded instants.
fn validate_samples(samples: &[TraceSampleDocument], source_name: &str) -> Result<(), String> {
    if samples.is_empty() {
        return Err(format!("{source_name}: samples must not be empty"));
    }
    let mut previous_timestamp_offset_seconds = None;
    for (sample_index, sample) in samples.iter().enumerate() {
        if let Err(coordinate_error) =
            GeographicCoordinate::from_degrees(sample.latitude, sample.longitude)
        {
            return Err(format!(
                "{source_name}: samples[{sample_index}]: {coordinate_error}"
            ));
        }
        if !sample.horizontal_accuracy_meters.is_finite()
            || sample.horizontal_accuracy_meters <= 0.0
        {
            return Err(format!(
                "{source_name}: samples[{sample_index}].horizontal_accuracy_meters \
                 must be a finite positive number (an ungraded fix is never \
                 judged for deviation, so a trace of ungraded fixes would be \
                 inert)"
            ));
        }
        if !sample.timestamp_offset_seconds.is_finite() || sample.timestamp_offset_seconds < 0.0 {
            return Err(format!(
                "{source_name}: samples[{sample_index}].timestamp_offset_seconds \
                 must be a finite non-negative number"
            ));
        }
        if let Some(previous) = previous_timestamp_offset_seconds
            && sample.timestamp_offset_seconds <= previous
        {
            return Err(format!(
                "{source_name}: samples[{sample_index}].timestamp_offset_seconds \
             must be strictly increasing (samples replay in recorded \
             order)"
            ));
        }
        previous_timestamp_offset_seconds = Some(sample.timestamp_offset_seconds);
    }
    Ok(())
}

/// Builds the reducer-facing location update of one validated sample.
///
/// The course is never reported by the synthetic fix pipeline, exactly as a
/// source without a course sensor would.
fn location_update_from_parts(
    latitude: f64,
    longitude: f64,
    horizontal_accuracy_meters: f64,
    timestamp_offset_seconds: f64,
) -> NavigationLocationUpdate {
    NavigationLocationUpdate {
        coordinate: GeographicCoordinate {
            latitude,
            longitude,
        },
        horizontal_accuracy_meters: Some(horizontal_accuracy_meters),
        course_heading_degrees: None,
        timestamp_seconds: Some(timestamp_offset_seconds),
    }
}

/// Plans a walk along a planar polyline of maneuver corners: the geometry the
/// generator records fixes against.
pub(crate) struct PlanarWalk {
    corner_planars: Vec<(f64, f64)>,
    cumulative_segment_lengths: Vec<f64>,
    total_length_meters: f64,
}

impl PlanarWalk {
    /// Builds the walk over corner offsets in planar meters.
    pub(crate) fn from_corner_planars(corner_planars: Vec<(f64, f64)>) -> Self {
        assert!(
            corner_planars.len() >= 2,
            "a walked polyline needs at least a start and an end corner"
        );
        let segment_lengths = corner_planars
            .windows(2)
            .map(|segment| {
                let (start_east, start_north) = segment[0];
                let (end_east, end_north) = segment[1];
                let segment_east_meters = end_east - start_east;
                let segment_north_meters = end_north - start_north;
                (segment_east_meters * segment_east_meters
                    + segment_north_meters * segment_north_meters)
                    .sqrt()
            })
            .collect::<Vec<f64>>();
        assert!(
            segment_lengths
                .iter()
                .all(|length| *length >= MINIMUM_WALK_SEGMENT_LENGTH_METERS),
            "the walked polyline's segments must be non-degenerate"
        );
        let mut cumulative_segment_lengths = Vec::with_capacity(segment_lengths.len());
        let mut running_total_meters = 0.0;
        for segment_length_meters in &segment_lengths {
            running_total_meters += segment_length_meters;
            cumulative_segment_lengths.push(running_total_meters);
        }
        Self {
            corner_planars,
            cumulative_segment_lengths,
            total_length_meters: running_total_meters,
        }
    }

    /// The walk's total planar length, in meters.
    pub(crate) fn total_length_meters(&self) -> f64 {
        self.total_length_meters
    }

    /// The planar position, unit tangent and unit normal (left of travel) at
    /// `along_meters` of the walk; the position is clamped to the walk's ends.
    pub(crate) fn position_at_along(
        &self,
        along_meters: f64,
    ) -> ((f64, f64), (f64, f64), (f64, f64)) {
        let clamped_along_meters = along_meters.clamp(0.0, self.total_length_meters);
        let mut segment_index = 0;
        for (index, segment_end_along_meters) in self.cumulative_segment_lengths.iter().enumerate()
        {
            if clamped_along_meters <= *segment_end_along_meters {
                segment_index = index;
                break;
            }
            segment_index = index;
        }
        let segment_start_along_meters = if segment_index == 0 {
            0.0
        } else {
            self.cumulative_segment_lengths[segment_index - 1]
        };
        let segment_length_meters =
            self.cumulative_segment_lengths[segment_index] - segment_start_along_meters;
        let (start_east_meters, start_north_meters) = self.corner_planars[segment_index];
        let (end_east_meters, end_north_meters) = self.corner_planars[segment_index + 1];
        let (segment_east_meters, segment_north_meters) = (
            end_east_meters - start_east_meters,
            end_north_meters - start_north_meters,
        );
        let along_fraction = if segment_length_meters < MINIMUM_WALK_SEGMENT_LENGTH_METERS {
            0.0
        } else {
            (clamped_along_meters - segment_start_along_meters) / segment_length_meters
        };
        let position = (
            start_east_meters + along_fraction * segment_east_meters,
            start_north_meters + along_fraction * segment_north_meters,
        );
        let tangent = (
            segment_east_meters / segment_length_meters,
            segment_north_meters / segment_length_meters,
        );
        // Left of travel: a positive lateral offset displaces the walker to
        // the left of the walked direction (north on an eastward street).
        let normal = (-tangent.1, tangent.0);
        (position, tangent, normal)
    }
}

/// Converts a planar east/north offset around `base_coordinate` into a
/// geographic coordinate (the same small-angle planarity the navigation
/// geometry module documents).
pub(crate) fn planar_offset_coordinate(
    base_coordinate: &GeographicCoordinate,
    east_meters: f64,
    north_meters: f64,
) -> GeographicCoordinate {
    GeographicCoordinate {
        latitude: base_coordinate.latitude + (north_meters / EARTH_RADIUS_METERS).to_degrees(),
        longitude: base_coordinate.longitude
            + (east_meters / (EARTH_RADIUS_METERS * base_coordinate.latitude.to_radians().cos()))
                .to_degrees(),
    }
}

/// Converts a geographic coordinate into its planar east/north offset around
/// `base_coordinate`: the inverse of [`planar_offset_coordinate`] under the
/// same small-angle planarity.
pub(crate) fn planar_offset_from_coordinate(
    base_coordinate: &GeographicCoordinate,
    coordinate: &GeographicCoordinate,
) -> (f64, f64) {
    let north_meters =
        (coordinate.latitude - base_coordinate.latitude).to_radians() * EARTH_RADIUS_METERS;
    let east_meters = (coordinate.longitude - base_coordinate.longitude).to_radians()
        * EARTH_RADIUS_METERS
        * base_coordinate.latitude.to_radians().cos();
    (east_meters, north_meters)
}

/// A recorded route's flattened geometry as a planar walk: the coordinate the
/// fixes are recorded around (the route's first vertex) and the walk over the
/// route's deduplicated shape vertices.
///
/// The generator records fixes against this walk, so every fix follows the
/// recorded route line rather than a synthesized street grid.
pub(crate) struct RecordedRouteWalk {
    /// The route's first recorded vertex: the planar projection's origin.
    pub(crate) base_coordinate: GeographicCoordinate,
    /// The planar walk over the recorded route's flattened shape.
    pub(crate) walk: PlanarWalk,
}

impl RecordedRouteWalk {
    /// Builds the walk from a route's flattened step shapes (consecutive
    /// duplicate vertices collapsed).
    pub(crate) fn from_route(route: &RouteData) -> Self {
        let mut flattened_coordinates = Vec::new();
        for coordinate in route
            .legs
            .iter()
            .flat_map(|leg| leg.steps.iter())
            .flat_map(|step| step.shape.iter())
        {
            append_coordinate_if_new(&mut flattened_coordinates, *coordinate);
        }
        let base_coordinate = *flattened_coordinates
            .first()
            .expect("a recorded route has at least one shape vertex");
        let corner_planars = flattened_coordinates
            .iter()
            .map(|coordinate| planar_offset_from_coordinate(&base_coordinate, coordinate))
            .collect();
        Self {
            base_coordinate,
            walk: PlanarWalk::from_corner_planars(corner_planars),
        }
    }
}

/// Appends `coordinate` to `shape` unless it duplicates the shape's current
/// last vertex exactly.
///
/// Exact duplicates are the only vertices dropped: every distinct recorded
/// coordinate survives, so the drift gate can match the committed geometry
/// against the recorded payload vertex for vertex.
fn append_coordinate_if_new(
    shape: &mut Vec<GeographicCoordinate>,
    coordinate: GeographicCoordinate,
) {
    if shape.last() != Some(&coordinate) {
        shape.push(coordinate);
    }
}

/// SplitMix64: the seeded pseudorandom number generator of the fixture noise
/// streams.
///
/// A small, fully specified generator (Steele, Lea & Flood's splitmix64) so
/// the committed traces stay regenerable bit-for-bit across platforms.
#[derive(Debug)]
struct SplitMix64 {
    state: u64,
}

/// SplitMix64's golden-ratio increment.
const SPLIT_MIX64_GOLDEN_GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;

/// SplitMix64's first mixer multiplier.
const SPLIT_MIX64_MIXER_MULTIPLIER_ONE: u64 = 0xBF58_476D_1CE4_E5B9;

/// SplitMix64's second mixer multiplier.
const SPLIT_MIX64_MIXER_MULTIPLIER_TWO: u64 = 0x94D0_49BB_1331_11EB;

/// Bits shifted out of the 64-bit word to keep 53 mantissa bits of a uniform.
const UNIFORM_MANTISSA_DISCARD_BITS: u32 = 11;

impl SplitMix64 {
    /// Seeds a stream.
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Advances the stream and returns the next 64-bit word.
    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(SPLIT_MIX64_GOLDEN_GAMMA);
        let mut mixed = self.state;
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(SPLIT_MIX64_MIXER_MULTIPLIER_ONE);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(SPLIT_MIX64_MIXER_MULTIPLIER_TWO);
        mixed ^ (mixed >> 31)
    }

    /// A uniform in (0, 1]: the 53-mantissa-bit word over 2^53 covers [0, 1),
    /// and one minus it covers (0, 1] — zero excluded, so the Box–Muller
    /// logarithm below stays finite.
    fn next_uniform_open_at_zero(&mut self) -> f64 {
        let mantissa_integer = (self.next_u64() >> UNIFORM_MANTISSA_DISCARD_BITS) as f64;
        let mantissa_denominator = (1u64 << (u64::BITS - UNIFORM_MANTISSA_DISCARD_BITS)) as f64;
        1.0 - mantissa_integer / mantissa_denominator
    }

    /// A standard normal variate via the Box–Muller transform.
    fn next_standard_normal(&mut self) -> f64 {
        let first_uniform_open_at_zero = self.next_uniform_open_at_zero();
        let second_uniform_open_at_zero = self.next_uniform_open_at_zero();
        (-2.0 * first_uniform_open_at_zero.ln()).sqrt()
            * (2.0 * core::f64::consts::PI * second_uniform_open_at_zero).cos()
    }
}

/// Clamps an (along, cross) Gaussian noise pair radially: a fix may never sit
/// farther than [`MAXIMUM_FIX_NOISE_METERS`] from its true walk position, in any
/// direction (per-axis clamping would allow corners of the noise square
/// outside the envelope).
fn clamp_radial_noise(along_noise_meters: f64, cross_noise_meters: f64) -> (f64, f64) {
    let radial_noise_meters =
        (along_noise_meters * along_noise_meters + cross_noise_meters * cross_noise_meters).sqrt();
    if radial_noise_meters <= MAXIMUM_FIX_NOISE_METERS {
        return (along_noise_meters, cross_noise_meters);
    }
    let clamp_scale = MAXIMUM_FIX_NOISE_METERS / radial_noise_meters;
    (
        along_noise_meters * clamp_scale,
        cross_noise_meters * clamp_scale,
    )
}

/// Records one noisy fix at `along_meters` of the recorded route, laterally
/// displaced `lateral_offset_meters` left of the route line, drawing its seeded
/// noise from `prng`.
fn record_walk_sample(
    route_walk: &RecordedRouteWalk,
    along_meters: f64,
    lateral_offset_meters: f64,
    timestamp_offset_seconds: f64,
    prng: &mut SplitMix64,
) -> TraceSampleDocument {
    let along_noise_meters = prng.next_standard_normal() * FIX_NOISE_SIGMA_METERS;
    let cross_noise_meters = prng.next_standard_normal() * FIX_NOISE_SIGMA_METERS;
    let (clamped_along_noise_meters, clamped_cross_noise_meters) =
        clamp_radial_noise(along_noise_meters, cross_noise_meters);
    let (position, tangent, normal) = route_walk.walk.position_at_along(along_meters);
    let displaced_east_meters = position.0
        + tangent.0 * clamped_along_noise_meters
        + normal.0 * (lateral_offset_meters + clamped_cross_noise_meters);
    let displaced_north_meters = position.1
        + tangent.1 * clamped_along_noise_meters
        + normal.1 * (lateral_offset_meters + clamped_cross_noise_meters);
    let displaced_coordinate = planar_offset_coordinate(
        &route_walk.base_coordinate,
        displaced_east_meters,
        displaced_north_meters,
    );
    TraceSampleDocument {
        latitude: displaced_coordinate.latitude,
        longitude: displaced_coordinate.longitude,
        horizontal_accuracy_meters: TRACE_FIX_ACCURACY_METERS,
        timestamp_offset_seconds,
    }
}

/// The recorded timestamp offset of the fix at `sample_index` (zero-based).
fn walk_sample_timestamp_offset_seconds(sample_index: usize) -> f64 {
    sample_index as f64 * SAMPLE_INTERVAL_SECONDS
}

/// The recovery scenario's lateral excursion profile: zero on route, the full
/// excursion across the plateau, and a linear ramp back to the route line.
fn recovery_lateral_offset_at_along(along_meters: f64) -> f64 {
    if along_meters <= EXCURSION_PLATEAU_START_ALONG_METERS {
        0.0
    } else if along_meters <= EXCURSION_PLATEAU_END_ALONG_METERS {
        EXCURSION_METERS
    } else if along_meters <= EXCURSION_REJOIN_ALONG_METERS {
        let ramp_span_meters = EXCURSION_REJOIN_ALONG_METERS - EXCURSION_PLATEAU_END_ALONG_METERS;
        let remaining_ramp_meters = EXCURSION_REJOIN_ALONG_METERS - along_meters;
        EXCURSION_METERS * (remaining_ramp_meters / ramp_span_meters)
    } else {
        0.0
    }
}

/// The exact polyline length, in meters, of a shape.
fn shape_length_meters(shape: &[GeographicCoordinate]) -> f64 {
    shape
        .windows(2)
        .map(|segment| segment[0].haversine_distance_meters(&segment[1]))
        .sum()
}

/// Stitches a provider-recorded route into a trace-ready route whose step
/// shapes form one continuous polyline.
///
/// The recorded artifact's per-step shapes are decimated slices with gaps at
/// the maneuver points, and its declared distances are rounded to the
/// provider's reporting granularity. Stitching appends each step's recorded
/// start vertex to the previous step's shape (the connector), folds a
/// single-vertex maneuver step into the preceding step (such a step carries no
/// traversable geometry), and recomputes every declared distance from the
/// resulting polyline. Every distinct recorded coordinate survives in order;
/// no vertex is invented, and the provider's durations are kept unchanged (a
/// folded single-vertex step's duration is added).
fn stitch_recorded_route(recorded_route: &RouteData) -> RouteData {
    let mut stitched_legs = Vec::with_capacity(recorded_route.legs.len());
    for recorded_leg in &recorded_route.legs {
        let mut stitched_steps: Vec<RouteStep> = Vec::new();
        let mut leading_start_coordinate: Option<GeographicCoordinate> = None;
        for recorded_step in &recorded_leg.steps {
            if recorded_step.shape.is_empty() {
                continue;
            }
            if recorded_step.shape.len() == 1 {
                let single_coordinate = recorded_step.shape[0];
                match stitched_steps.last_mut() {
                    Some(previous_step) => {
                        append_coordinate_if_new(&mut previous_step.shape, single_coordinate);
                        previous_step.duration_seconds += recorded_step.duration_seconds;
                    }
                    None => leading_start_coordinate = Some(single_coordinate),
                }
                continue;
            }
            if let Some(previous_step) = stitched_steps.last_mut() {
                append_coordinate_if_new(&mut previous_step.shape, recorded_step.shape[0]);
            }
            let mut shape = Vec::with_capacity(recorded_step.shape.len() + 1);
            if let Some(leading_start_coordinate) = leading_start_coordinate.take() {
                append_coordinate_if_new(&mut shape, leading_start_coordinate);
            }
            for coordinate in &recorded_step.shape {
                append_coordinate_if_new(&mut shape, *coordinate);
            }
            stitched_steps.push(RouteStep {
                // Recomputed from the finished shapes below.
                distance_meters: 0.0,
                duration_seconds: recorded_step.duration_seconds,
                maneuver_type: recorded_step.maneuver_type,
                instruction_display_text: recorded_step.instruction_display_text.clone(),
                instruction_speech_text: recorded_step.instruction_speech_text.clone(),
                shape,
                lane_guidances: recorded_step.lane_guidances.clone(),
            });
        }
        assert!(
            !stitched_steps.is_empty(),
            "a recorded route leg must contribute at least one traversable step"
        );
        for step in &mut stitched_steps {
            step.distance_meters = shape_length_meters(&step.shape);
            assert!(
                step.distance_meters > 0.0,
                "a stitched step must retain non-degenerate geometry"
            );
        }
        stitched_legs.push(RouteLeg {
            distance_meters: stitched_steps.iter().map(|step| step.distance_meters).sum(),
            duration_seconds: stitched_steps
                .iter()
                .map(|step| step.duration_seconds)
                .sum(),
            steps: stitched_steps,
        });
    }
    route_from_legs(stitched_legs)
}

/// Assembles a route from its legs, deriving the totals and the bounding box
/// from the step geometry.
fn route_from_legs(legs: Vec<RouteLeg>) -> RouteData {
    RouteData {
        distance_meters: legs.iter().map(|leg| leg.distance_meters).sum(),
        duration_seconds: legs.iter().map(|leg| leg.duration_seconds).sum(),
        bounding_box: bounding_box_of_legs(&legs),
        geometry: Vec::new(),
        legs,
    }
}

/// The bounding box containing every coordinate of every step of `legs`.
fn bounding_box_of_legs(legs: &[RouteLeg]) -> BoundingBox {
    let mut minimum_latitude = f64::INFINITY;
    let mut maximum_latitude = f64::NEG_INFINITY;
    let mut minimum_longitude = f64::INFINITY;
    let mut maximum_longitude = f64::NEG_INFINITY;
    for coordinate in legs
        .iter()
        .flat_map(|leg| leg.steps.iter())
        .flat_map(|step| step.shape.iter())
    {
        minimum_latitude = minimum_latitude.min(coordinate.latitude);
        maximum_latitude = maximum_latitude.max(coordinate.latitude);
        minimum_longitude = minimum_longitude.min(coordinate.longitude);
        maximum_longitude = maximum_longitude.max(coordinate.longitude);
    }
    BoundingBox {
        south_west: GeographicCoordinate {
            latitude: minimum_latitude,
            longitude: minimum_longitude,
        },
        north_east: GeographicCoordinate {
            latitude: maximum_latitude,
            longitude: maximum_longitude,
        },
    }
}

/// The shape of `shape` from the projection of `anchor` onward: the anchor
/// followed by every recorded vertex after the hosting segment.
fn remaining_shape_from_projection(
    shape: &[GeographicCoordinate],
    anchor: &GeographicCoordinate,
) -> Vec<GeographicCoordinate> {
    let mut closest_segment_index = 0;
    let mut closest_distance_meters = f64::INFINITY;
    for segment_index in 0..shape.len().saturating_sub(1) {
        if let Some(snap) =
            snap_coordinate_to_polyline(anchor, &shape[segment_index..=segment_index + 1])
            && snap.distance_to_polyline_meters <= closest_distance_meters
        {
            closest_distance_meters = snap.distance_to_polyline_meters;
            closest_segment_index = segment_index;
        }
    }
    let mut remaining_shape = vec![*anchor];
    for coordinate in &shape[closest_segment_index + 1..] {
        append_coordinate_if_new(&mut remaining_shape, *coordinate);
    }
    remaining_shape
}

/// Regenerates the trace document of one scenario.
///
/// Panics on an unknown scenario name; the named scenarios are the three
/// committed ones in [`ALL_TRACE_SCENARIOS`].
pub(crate) fn generate_trace_document(scenario_name: &str) -> NavigationTraceDocument {
    match scenario_name {
        CLEAN_TRACE_SCENARIO => generate_on_route_trace(
            scenario_name,
            CLEAN_ROUTE_FIXTURE_FILE_NAME,
            CLEAN_TRACE_SEED,
            |_| 0.0,
        ),
        RECOVERY_TRACE_SCENARIO => generate_on_route_trace(
            scenario_name,
            RECOVERY_ROUTE_FIXTURE_FILE_NAME,
            RECOVERY_TRACE_SEED,
            recovery_lateral_offset_at_along,
        ),
        REROUTE_TRACE_SCENARIO => generate_reroute_trace(),
        other => panic!("unknown trace scenario '{other}'"),
    }
}

/// Generates a trace that walks the whole of one recorded route fixture, with
/// the given lateral excursion profile for the fixes.
fn generate_on_route_trace(
    scenario_name: &str,
    route_fixture_file_name: &str,
    generator_seed: u64,
    lateral_offset_at_along: impl Fn(f64) -> f64,
) -> NavigationTraceDocument {
    let route_fixture = load_recorded_route_fixture(route_fixture_file_name);
    let route_walk = RecordedRouteWalk::from_route(&route_fixture.route);
    let mut noise_stream = SplitMix64::new(generator_seed);
    let samples = record_walk_samples(&route_walk, &mut noise_stream, lateral_offset_at_along);
    trace_document(
        scenario_name,
        generator_seed,
        route_fixture.route.clone(),
        None,
        samples,
        &route_fixture,
    )
}

/// Assembles a trace document from its generated parts.
fn trace_document(
    scenario_name: &str,
    generator_seed: u64,
    route: RouteData,
    reroute_route: Option<RouteData>,
    samples: Vec<TraceSampleDocument>,
    route_fixture: &RecordedRouteFixtureDocument,
) -> NavigationTraceDocument {
    NavigationTraceDocument {
        metadata: TraceMetadataDocument {
            scenario_name: scenario_name.to_string(),
            provenance: trace_provenance_statement(
                &route_fixture.origin_name,
                &route_fixture.destination_name,
            ),
            generator_seed,
            deviation_threshold_meters: DEFAULT_DEVIATION_DISTANCE_THRESHOLD_METERS,
            horizontal_accuracy_meters: TRACE_FIX_ACCURACY_METERS,
            sample_interval_seconds: SAMPLE_INTERVAL_SECONDS,
        },
        route,
        reroute_route,
        samples,
    }
}

/// Records the on-route sample sequence of one recorded route: every
/// [`SAMPLE_SPACING_METERS`] of travel up to the last whole spacing (so the
/// final fix lies inside the eager-advance window of the route's end and the
/// trip always arrives), with the given lateral excursion profile.
fn record_walk_samples(
    route_walk: &RecordedRouteWalk,
    noise_stream: &mut SplitMix64,
    lateral_offset_at_along: impl Fn(f64) -> f64,
) -> Vec<TraceSampleDocument> {
    let total_length_meters = route_walk.walk.total_length_meters();
    assert!(
        total_length_meters > 2.0 * SAMPLE_SPACING_METERS,
        "a recorded trace route must be long enough to record on-route fixes and arrive"
    );
    let sample_count = (total_length_meters / SAMPLE_SPACING_METERS) as usize;
    (1..=sample_count)
        .map(|sample_number| {
            let along_meters = sample_number as f64 * SAMPLE_SPACING_METERS;
            record_walk_sample(
                route_walk,
                along_meters,
                lateral_offset_at_along(along_meters),
                walk_sample_timestamp_offset_seconds(sample_number - 1),
                noise_stream,
            )
        })
        .collect()
}

/// Generates the reroute scenario: on-route fixes along the recorded route,
/// the single deviating fix, the recorded route's remaining geometry anchored
/// at the reducer's exact snapped coordinate at that moment, and the
/// post-reroute fixes driving that replacement route.
fn generate_reroute_trace() -> NavigationTraceDocument {
    let route_fixture = load_recorded_route_fixture(REROUTE_ROUTE_FIXTURE_FILE_NAME);
    let main_route = route_fixture.route.clone();
    let main_walk = RecordedRouteWalk::from_route(&main_route);
    assert!(
        REROUTE_DEVIATION_ALONG_METERS < main_walk.walk.total_length_meters(),
        "the recorded reroute deviation point must fall inside the recorded route"
    );
    let mut noise_stream = SplitMix64::new(REROUTE_TRACE_SEED);
    let mut samples = Vec::new();
    let last_on_route_along_meters = REROUTE_DEVIATION_ALONG_METERS - SAMPLE_SPACING_METERS;
    let pre_deviation_sample_count = (last_on_route_along_meters / SAMPLE_SPACING_METERS) as usize;
    for sample_number in 1..=pre_deviation_sample_count {
        let along_meters = sample_number as f64 * SAMPLE_SPACING_METERS;
        samples.push(record_walk_sample(
            &main_walk,
            along_meters,
            0.0,
            walk_sample_timestamp_offset_seconds(samples.len()),
            &mut noise_stream,
        ));
    }
    samples.push(record_walk_sample(
        &main_walk,
        REROUTE_DEVIATION_ALONG_METERS,
        EXCURSION_METERS,
        walk_sample_timestamp_offset_seconds(samples.len()),
        &mut noise_stream,
    ));
    let (anchor_coordinate, hosting_step_position) = deviation_freeze_anchor(&samples, &main_route);
    let replacement_route =
        replacement_route_from_anchor(&main_route, &anchor_coordinate, hosting_step_position);
    let replacement_walk = RecordedRouteWalk::from_route(&replacement_route);
    let replacement_sample_count =
        (replacement_walk.walk.total_length_meters() / SAMPLE_SPACING_METERS) as usize;
    for replacement_sample_number in 1..=replacement_sample_count {
        let along_meters = replacement_sample_number as f64 * SAMPLE_SPACING_METERS;
        samples.push(record_walk_sample(
            &replacement_walk,
            along_meters,
            0.0,
            walk_sample_timestamp_offset_seconds(samples.len()),
            &mut noise_stream,
        ));
    }
    trace_document(
        REROUTE_TRACE_SCENARIO,
        REROUTE_TRACE_SEED,
        main_route,
        Some(replacement_route),
        samples,
        &route_fixture,
    )
}

/// Builds the reroute scenario's replacement route from the recorded route's
/// own remaining geometry: the hosting step's shape from `anchor` onward, then
/// the recorded steps that follow it.
///
/// The first vertex is exactly `anchor` (the reducer's frozen snapped
/// coordinate) and the last is the recorded route's own destination, so the
/// anchoring and destination contracts hold without inventing a street. The
/// hosting step's declared duration is scaled to its remaining fraction; every
/// following step keeps its recorded distance, duration and text.
fn replacement_route_from_anchor(
    main_route: &RouteData,
    anchor: &GeographicCoordinate,
    hosting_step_position: usize,
) -> RouteData {
    assert_eq!(
        main_route.legs.len(),
        1,
        "a recorded demo route is single-leg; the reroute tail is built for that fixture shape"
    );
    let main_leg = &main_route.legs[0];
    let hosting_step = &main_leg.steps[hosting_step_position];
    let remaining_shape = remaining_shape_from_projection(&hosting_step.shape, anchor);

    let mut replacement_steps = Vec::new();
    let mut following_step_index = hosting_step_position + 1;
    if remaining_shape.len() >= 2 {
        let remaining_distance_meters = shape_length_meters(&remaining_shape);
        let remaining_fraction =
            (remaining_distance_meters / hosting_step.distance_meters).clamp(0.0, 1.0);
        replacement_steps.push(RouteStep {
            distance_meters: remaining_distance_meters,
            duration_seconds: hosting_step.duration_seconds * remaining_fraction,
            maneuver_type: hosting_step.maneuver_type,
            instruction_display_text: hosting_step.instruction_display_text.clone(),
            instruction_speech_text: hosting_step.instruction_speech_text.clone(),
            shape: remaining_shape,
            lane_guidances: hosting_step.lane_guidances.clone(),
        });
    } else {
        // The anchor sits at (or within floating-point noise of) the hosting
        // step's end. Start on the next recorded step, prepending the exact
        // anchor so the anchoring contract still holds.
        let following_step = main_leg
            .steps
            .get(hosting_step_position + 1)
            .unwrap_or_else(|| {
                panic!("the reroute anchor must fall inside the recorded route, not at its end")
            });
        let mut shape = vec![*anchor];
        for coordinate in &following_step.shape {
            append_coordinate_if_new(&mut shape, *coordinate);
        }
        replacement_steps.push(RouteStep {
            distance_meters: shape_length_meters(&shape),
            duration_seconds: following_step.duration_seconds,
            maneuver_type: following_step.maneuver_type,
            instruction_display_text: following_step.instruction_display_text.clone(),
            instruction_speech_text: following_step.instruction_speech_text.clone(),
            shape,
            lane_guidances: following_step.lane_guidances.clone(),
        });
        following_step_index = hosting_step_position + 2;
    }
    for step in &main_leg.steps[following_step_index..] {
        replacement_steps.push(step.clone());
    }
    route_from_legs(vec![RouteLeg {
        distance_meters: replacement_steps
            .iter()
            .map(|step| step.distance_meters)
            .sum(),
        duration_seconds: replacement_steps
            .iter()
            .map(|step| step.duration_seconds)
            .sum(),
        steps: replacement_steps,
    }])
}

/// Replays the generated samples through the real reducer (the same eager
/// step advance and default deviation detector the replay tests drive) and
/// returns the trip's frozen state at the moment the deviation was judged: the
/// snapped coordinate the replacement route must begin at (the reroute
/// anchoring contract) and the flat step position the reducer was on then.
fn deviation_freeze_anchor(
    samples: &[TraceSampleDocument],
    main_route: &RouteData,
) -> (GeographicCoordinate, usize) {
    let mut session = reduce_navigation_event(
        None,
        NavigationEvent::StartTrip {
            route: main_route.clone(),
            arrival_step_advance_condition: None,
            course_filtering: CourseFiltering::Raw,
            step_advance_condition: Arc::new(
                DistanceToEndOfStepCondition::with_ferrostar_defaults(),
            ),
            deviation_detector: Arc::new(DistanceDeviationDetector::with_default_threshold()),
        },
    )
    .expect("the reroute generator's replay must start a trip");
    for sample_document in samples {
        let update = location_update_from_parts(
            sample_document.latitude,
            sample_document.longitude,
            sample_document.horizontal_accuracy_meters,
            sample_document.timestamp_offset_seconds,
        );
        session = reduce_navigation_event(Some(session), NavigationEvent::LocationUpdated(update))
            .expect("the reroute generator's replay must keep the trip alive");
    }
    assert_eq!(
        session.state.phase,
        NavigationPhase::OffRoute,
        "the reroute generator's replay must freeze at the recorded deviating fix"
    );
    let anchor_coordinate = session
        .state
        .snapped_coordinate
        .expect("a deviating session keeps its last on-route snapped coordinate");
    (anchor_coordinate, session.state.current_step_position)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_fixtures_load_and_validate() {
        for scenario_name in ALL_TRACE_SCENARIOS {
            let fixture = load_trace_fixture(scenario_name);
            assert_eq!(fixture.scenario_name(), scenario_name);
            assert_eq!(
                fixture.deviation_threshold_meters(),
                DEFAULT_DEVIATION_DISTANCE_THRESHOLD_METERS
            );
            assert_eq!(
                fixture.sample_interval_seconds(),
                SAMPLE_INTERVAL_SECONDS,
                "the recorded replay cadence matches the generator's cadence"
            );
            assert_eq!(
                fixture.generator_seed(),
                generate_trace_document(scenario_name)
                    .metadata
                    .generator_seed,
                "the committed fixture's seed must be the generator's seed for \
                 {scenario_name}"
            );
            assert!(
                fixture.generator_seed() != 0,
                "scenario {scenario_name} must record its regeneration seed"
            );
            assert!(
                !fixture.samples().is_empty(),
                "scenario {scenario_name} must record samples"
            );
            assert!(!fixture.route().legs.is_empty());
        }
        let clean_fixture = load_trace_fixture(CLEAN_TRACE_SCENARIO);
        assert!(clean_fixture.reroute_route().is_none());
        let reroute_fixture = load_trace_fixture(REROUTE_TRACE_SCENARIO);
        assert!(
            reroute_fixture.reroute_route().is_some(),
            "the reroute scenario must carry its replacement route"
        );
        let recovery_fixture = load_trace_fixture(RECOVERY_TRACE_SCENARIO);
        assert!(recovery_fixture.reroute_route().is_none());
    }

    #[test]
    fn malformed_trace_json_fails_with_an_actionable_message() {
        let error = NavigationTraceFixture::parse("{ not json", "fixtures/x.json")
            .expect_err("malformed JSON must be rejected");
        assert!(
            error.contains("fixtures/x.json") && error.contains("not a valid navigation trace"),
            "the error must name the source and the expectation: {error}"
        );
    }

    #[test]
    fn malformed_trace_documents_fail_validation_with_actionable_messages() {
        let valid_document = generate_trace_document(CLEAN_TRACE_SCENARIO);
        let mut without_samples = valid_document.clone();
        without_samples.samples.clear();
        let error = NavigationTraceFixture::from_document(without_samples, "trace.json")
            .expect_err("a trace without samples must be rejected");
        assert!(error.contains("samples must not be empty"), "{error}");

        let mut out_of_range_latitude = valid_document.clone();
        out_of_range_latitude.samples[0].latitude = 95.0;
        let error = NavigationTraceFixture::from_document(out_of_range_latitude, "trace.json")
            .expect_err("an out-of-range fix latitude must be rejected");
        assert!(error.contains("samples[0]"), "{error}");

        let mut non_increasing_timestamps = valid_document.clone();
        non_increasing_timestamps.samples[1].timestamp_offset_seconds =
            non_increasing_timestamps.samples[0].timestamp_offset_seconds;
        let error = NavigationTraceFixture::from_document(non_increasing_timestamps, "trace.json")
            .expect_err("a non-replayable sample order must be rejected");
        assert!(error.contains("strictly increasing"), "{error}");

        let mut noise_free_accuracy = valid_document.clone();
        noise_free_accuracy.samples[0].horizontal_accuracy_meters = 0.0;
        let error = NavigationTraceFixture::from_document(noise_free_accuracy, "trace.json")
            .expect_err("an ungraded fix recording must be rejected");
        assert!(error.contains("horizontal_accuracy_meters"), "{error}");

        let mut threshold_drifted_metadata = valid_document.clone();
        threshold_drifted_metadata
            .metadata
            .deviation_threshold_meters = 30.0;
        let error = NavigationTraceFixture::from_document(threshold_drifted_metadata, "trace.json")
            .expect_err("metadata recorded against another threshold must be rejected");
        assert!(error.contains("deviation_threshold_meters"), "{error}");

        let mut zero_seed_metadata = valid_document.clone();
        zero_seed_metadata.metadata.generator_seed = 0;
        let error = NavigationTraceFixture::from_document(zero_seed_metadata, "trace.json")
            .expect_err("an unregenerable trace must be rejected");
        assert!(error.contains("generator_seed"), "{error}");

        let mut empty_shape_route = valid_document.clone();
        empty_shape_route.route.legs[0].steps[0].shape.clear();
        let error = NavigationTraceFixture::from_document(empty_shape_route, "trace.json")
            .expect_err("a step without a shape must be rejected");
        assert!(error.contains("must have a shape"), "{error}");

        let mut drifted_step_distance = valid_document.clone();
        drifted_step_distance.route.legs[0].steps[0].distance_meters += 1.0;
        let error = NavigationTraceFixture::from_document(drifted_step_distance, "trace.json")
            .expect_err("a declared step distance off the geometry must be rejected");
        assert!(
            error.contains("distance_meters must equal the shape's polyline length"),
            "{error}"
        );
    }

    #[test]
    fn generator_noise_constants_stay_inside_the_fixture_envelopes() {
        // Both envelopes are compile-time facts about the generator's
        // constants, so the test compiles the checks as constant assertions.
        const _: () = {
            assert!(
                MAXIMUM_FIX_NOISE_METERS < TRACE_FIX_ACCURACY_METERS,
                "the noise clamp must sit inside the recorded accuracy envelope, \
                 so a snapped fix stays within its own accuracy"
            );
            assert!(
                EXCURSION_METERS
                    > DEFAULT_DEVIATION_DISTANCE_THRESHOLD_METERS + TRACE_FIX_ACCURACY_METERS,
                "the excursion must be beyond the effective deviation allowance"
            );
        };
    }

    #[test]
    fn regenerated_trace_bytes_match_the_committed_fixtures() {
        for scenario_name in ALL_TRACE_SCENARIOS {
            let committed_document_json = std::fs::read_to_string(fixture_file_path(scenario_name))
                .unwrap_or_else(|read_error| {
                    panic!(
                        "the committed fixture for '{scenario_name}' is unreadable: \
                         {read_error} (regenerate with `cargo test -p rustic-maps-navigation \
                         regenerate_trace_fixtures -- --ignored`)"
                    )
                });
            let generated_document_json =
                serde_json::to_string_pretty(&generate_trace_document(scenario_name))
                    .expect("a generated trace must serialize");
            assert_eq!(
                committed_document_json.trim(),
                generated_document_json,
                "the committed fixture for '{scenario_name}' must byte-match the \
                 generator's output for the seed recorded in its metadata; \
                 regenerate with `cargo test -p rustic-maps-navigation \
                 regenerate_trace_fixtures -- --ignored`"
            );
        }
    }

    #[test]
    #[ignore = "regenerates the committed fixture files; run with --ignored"]
    fn regenerate_trace_fixtures() {
        let fixture_directory = fixture_directory_path();
        std::fs::create_dir_all(&fixture_directory)
            .expect("the fixture directory must be creatable");
        for scenario_name in ALL_TRACE_SCENARIOS {
            let document = generate_trace_document(scenario_name);
            let document_json =
                serde_json::to_string_pretty(&document).expect("a trace must serialize");
            std::fs::write(
                fixture_file_path(scenario_name),
                format!("{document_json}\n"),
            )
            .expect("the fixture file must be writable");
        }
    }

    /// The shared demo artifact read from the repository root.
    fn reference_scenes_artifact() -> serde_json::Value {
        let artifact_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("platform")
            .join("reference-scenes.json");
        let artifact_json = std::fs::read_to_string(&artifact_path).unwrap_or_else(|read_error| {
            panic!(
                "the shared demo artifact {} is unreadable: {read_error}",
                artifact_path.display()
            )
        });
        serde_json::from_str(&artifact_json).expect("the shared demo artifact must be valid JSON")
    }

    /// The artifact's `demo_routes` entry for one ordered demo place pair.
    fn demo_route_entry(
        artifact: &serde_json::Value,
        origin_name: &str,
        destination_name: &str,
    ) -> serde_json::Value {
        artifact["demo_routes"]
            .as_array()
            .expect("the shared artifact carries demo_routes")
            .iter()
            .find(|entry| {
                entry["origin_name"].as_str() == Some(origin_name)
                    && entry["destination_name"].as_str() == Some(destination_name)
            })
            .unwrap_or_else(|| {
                panic!(
                    "demo_routes must carry the recorded {origin_name} -> {destination_name} pair"
                )
            })
            .clone()
    }

    /// A route's flattened shape coordinates, consecutive duplicates collapsed.
    fn flattened_route_coordinates(route: &RouteData) -> Vec<GeographicCoordinate> {
        let mut coordinates = Vec::new();
        for coordinate in route
            .legs
            .iter()
            .flat_map(|leg| leg.steps.iter())
            .flat_map(|step| step.shape.iter())
        {
            append_coordinate_if_new(&mut coordinates, *coordinate);
        }
        coordinates
    }

    /// The drift gate over the recorded route fixtures: every committed vertex
    /// must still be the recorded provider payload's vertex, in order, and the
    /// fixture must still start and end at the recorded place coordinates.
    #[test]
    fn committed_route_fixtures_match_the_recorded_demo_routes() {
        let artifact = reference_scenes_artifact();
        for source in ALL_RECORDED_ROUTE_FIXTURE_SOURCES {
            let document = load_recorded_route_fixture(source.file_name);
            assert_eq!(document.origin_name, source.origin_name);
            assert_eq!(document.destination_name, source.destination_name);

            let recorded_entry =
                demo_route_entry(&artifact, &document.origin_name, &document.destination_name);
            let recorded_route: RouteData = serde_json::from_value(recorded_entry["route"].clone())
                .expect("the recorded demo route must deserialize as RouteData");
            let recorded_coordinates = flattened_route_coordinates(&recorded_route);
            let fixture_coordinates = flattened_route_coordinates(&document.route);
            assert_eq!(
                fixture_coordinates.len(),
                recorded_coordinates.len(),
                "{}: the committed route must carry exactly the recorded vertices",
                source.file_name
            );
            for (vertex_index, (fixture_coordinate, recorded_coordinate)) in fixture_coordinates
                .iter()
                .zip(recorded_coordinates.iter())
                .enumerate()
            {
                let separation_meters =
                    fixture_coordinate.haversine_distance_meters(recorded_coordinate);
                assert!(
                    separation_meters <= ROUTE_FIXTURE_COORDINATE_TOLERANCE_METERS,
                    "{}: vertex {vertex_index} drifted {separation_meters} m from the recorded \
                     demo_routes payload (tolerance {ROUTE_FIXTURE_COORDINATE_TOLERANCE_METERS} m); \
                     re-record the route fixtures with `cargo test -p rustic-maps-navigation \
                     regenerate_recorded_route_fixtures -- --ignored`",
                    source.file_name
                );
            }
        }
    }

    /// Each committed trace embeds exactly its scenario's committed recorded
    /// route fixture (not a hand-edited copy).
    #[test]
    fn committed_trace_routes_are_the_recorded_route_fixtures() {
        assert_eq!(
            load_trace_fixture(CLEAN_TRACE_SCENARIO).route(),
            &load_recorded_route_fixture(CLEAN_ROUTE_FIXTURE_FILE_NAME).route
        );
        assert_eq!(
            load_trace_fixture(RECOVERY_TRACE_SCENARIO).route(),
            &load_recorded_route_fixture(RECOVERY_ROUTE_FIXTURE_FILE_NAME).route
        );
        assert_eq!(
            load_trace_fixture(REROUTE_TRACE_SCENARIO).route(),
            &load_recorded_route_fixture(REROUTE_ROUTE_FIXTURE_FILE_NAME).route
        );
    }

    #[test]
    #[ignore = "regenerates the committed recorded-route fixtures; run with --ignored"]
    fn regenerate_recorded_route_fixtures() {
        let artifact = reference_scenes_artifact();
        let fixture_directory = route_fixture_directory_path();
        std::fs::create_dir_all(&fixture_directory)
            .expect("the recorded-route fixture directory must be creatable");
        for source in ALL_RECORDED_ROUTE_FIXTURE_SOURCES {
            let recorded_entry =
                demo_route_entry(&artifact, source.origin_name, source.destination_name);
            let recorded_route: RouteData = serde_json::from_value(recorded_entry["route"].clone())
                .expect("the recorded demo route must deserialize as RouteData");
            let stitched_route = stitch_recorded_route(&recorded_route);
            validate_route(&stitched_route, "route", source.file_name)
                .unwrap_or_else(|validation_error| panic!("{validation_error}"));
            let document = RecordedRouteFixtureDocument {
                origin_name: source.origin_name.to_string(),
                destination_name: source.destination_name.to_string(),
                route: stitched_route,
            };
            let document_json =
                serde_json::to_string_pretty(&document).expect("a route fixture must serialize");
            std::fs::write(
                route_fixture_file_path(source.file_name),
                format!("{document_json}\n"),
            )
            .expect("the recorded-route fixture file must be writable");
        }
    }
}
