//! Route, leg, step, maneuver, lane guidance and distance-matrix records.

use crate::bounding_box::BoundingBox;
use crate::coordinate::GeographicCoordinate;
use serde::{Deserialize, Serialize};

/// Mode of transport used to calculate a route.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportationMode {
    /// Private car routing.
    Automobile,
    /// Bicycle routing.
    Bicycle,
    /// Walking routing.
    Pedestrian,
}

/// Length and speed conventions used when displaying a route.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitSystem {
    /// Kilometers and kilometers per hour.
    Metric,
    /// Miles and miles per hour.
    Imperial,
}

/// Road features a routing request can ask the provider to avoid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct AvoidOptions {
    /// Avoid toll roads.
    pub toll_roads: bool,
    /// Avoid limited-access highways and motorways.
    pub limited_access_highways: bool,
    /// Avoid ferry crossings.
    pub ferries: bool,
    /// Avoid unpaved roads.
    pub unpaved_roads: bool,
}

/// What a routing request optimizes for.
///
/// Providers rank candidate roads by a cost function. The strategy selects
/// between travel time and physical length. Non-automobile modes have no
/// time-based cost model, so the strategy applies only to automobile routing,
/// and a rerouting request must reuse the original request's strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteStrategy {
    /// Minimize expected travel time.
    #[default]
    Fastest,
    /// Minimize traveled distance.
    Shortest,
}

/// What a routing provider can honour of the mode and strategy choices a
/// request may carry.
///
/// The SDK can express three transport modes and two strategies, but a
/// provider may not serve all of them: OSRM's route service has no
/// shortest-path switch, so it documents `supports_shortest_route_strategy`
/// as false rather than silently ignoring the choice. A user interface reads
/// this answer so it can disable a choice with a reason instead of letting a
/// route come back from a path the reader did not ask for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteCapabilities {
    /// The transport modes the provider can route for.
    pub supported_transportation_modes: Vec<TransportationMode>,
    /// Whether the provider can minimize traveled distance instead of travel
    /// time for automobile routing.
    pub supports_shortest_route_strategy: bool,
}

impl RouteCapabilities {
    /// Every mode and both strategies: the answer of a provider that honours
    /// every choice the SDK can express.
    pub fn all_supported() -> Self {
        Self {
            supported_transportation_modes: vec![
                TransportationMode::Automobile,
                TransportationMode::Bicycle,
                TransportationMode::Pedestrian,
            ],
            supports_shortest_route_strategy: true,
        }
    }

    /// Whether the provider can route for one transport mode.
    pub fn supports_transportation_mode(&self, transportation_mode: TransportationMode) -> bool {
        self.supported_transportation_modes
            .contains(&transportation_mode)
    }
}

/// Options controlling how a route is calculated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteOptions {
    /// Mode of transport.
    pub transportation_mode: TransportationMode,
    /// Whether routing optimizes travel time or distance.
    pub route_strategy: RouteStrategy,
    /// Display unit convention the provider should annotate.
    pub unit_system: UnitSystem,
    /// Features to avoid; all disabled by default.
    pub avoid_options: AvoidOptions,
    /// Departure instant as seconds since the Unix epoch, or `None` for
    /// "depart now".
    pub departure_timestamp_seconds: Option<u64>,
}

impl Default for RouteOptions {
    fn default() -> Self {
        Self {
            transportation_mode: TransportationMode::Automobile,
            route_strategy: RouteStrategy::Fastest,
            unit_system: UnitSystem::Metric,
            avoid_options: AvoidOptions::default(),
            departure_timestamp_seconds: None,
        }
    }
}

/// Turn direction reported by a maneuver or a lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnDirection {
    /// Continue straight ahead.
    Straight,
    /// Sharp left turn.
    SharpLeft,
    /// Standard left turn.
    Left,
    /// Gentle left turn.
    SlightLeft,
    /// Gentle right turn.
    SlightRight,
    /// Standard right turn.
    Right,
    /// Sharp right turn.
    SharpRight,
    /// Full reversal of direction.
    UTurn,
    /// Leave the current road.
    LeavingRoad,
}

/// High-level maneuver classification carried by a route step.
///
/// The classification serializes as its [`Self::stable_label`], the SDK's
/// single maneuver spelling. A route serialized by this SDK and deserialized
/// again therefore round-trips without spelling drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ManeuverType {
    /// Route start.
    Departure,
    /// Route end.
    Arrival,
    /// Road-name or road-class change without a turn.
    Continue,
    /// Left/right/slight/sharp turn, see [`TurnDirection`].
    Turn(TurnDirection),
    /// Merge onto another road.
    Merge,
    /// Motorway on-ramp.
    OnRamp,
    /// Motorway off-ramp.
    OffRamp,
    /// Fork in the road.
    Fork,
    /// Roundabout traversal; carries the exit index starting at one.
    Roundabout(u8),
    /// Transition to a ferry.
    Ferry,
}

/// The label prefix of turn maneuvers in the stable spelling.
const TURN_STABLE_LABEL_PREFIX: &str = "turn";
/// The label prefix of roundabout maneuvers in the stable spelling.
const ROUNDABOUT_STABLE_LABEL_PREFIX: &str = "roundabout";
/// The hyphen joining a maneuver label to its embedded parameter.
const STABLE_LABEL_PARAMETER_SEPARATOR: char = '-';
/// The lowest one-based exit index a roundabout label may carry.
const STABLE_LABEL_ROUNDABOUT_MINIMUM_EXIT_INDEX: u8 = 1;
/// The serde error expectation naming the stable spelling for a decoder.
const STABLE_LABEL_SERDE_EXPECTATION: &str = "a stable maneuver label such as \
    \"departure\", \"turn-left\" or \"roundabout-2\"";

/// The kebab-case parameter segment a turn direction contributes to its
/// maneuver label (`left`, `sharp-left`, `u-turn`, …).
fn turn_direction_label_segment(turn_direction: TurnDirection) -> &'static str {
    match turn_direction {
        TurnDirection::Straight => "straight",
        TurnDirection::SharpLeft => "sharp-left",
        TurnDirection::Left => "left",
        TurnDirection::SlightLeft => "slight-left",
        TurnDirection::SlightRight => "slight-right",
        TurnDirection::Right => "right",
        TurnDirection::SharpRight => "sharp-right",
        TurnDirection::UTurn => "u-turn",
        TurnDirection::LeavingRoad => "leaving-road",
    }
}

/// Builds the turn direction a kebab-case parameter segment names; the
/// inverse of [`turn_direction_label_segment`].
fn turn_direction_from_label_segment(label_segment: &str) -> Option<TurnDirection> {
    Some(match label_segment {
        "straight" => TurnDirection::Straight,
        "sharp-left" => TurnDirection::SharpLeft,
        "left" => TurnDirection::Left,
        "slight-left" => TurnDirection::SlightLeft,
        "slight-right" => TurnDirection::SlightRight,
        "right" => TurnDirection::Right,
        "sharp-right" => TurnDirection::SharpRight,
        "u-turn" => TurnDirection::UTurn,
        "leaving-road" => TurnDirection::LeavingRoad,
        _ => return None,
    })
}

impl ManeuverType {
    /// The stable kebab-case label of this maneuver: `departure`,
    /// `turn-left`, `roundabout-2`, and so on.
    ///
    /// # Returns
    /// The SDK's single maneuver spelling. Serialization emits this label,
    /// and [`Self::from_stable_label`] accepts it back.
    pub fn stable_label(&self) -> String {
        match self {
            Self::Departure => "departure".to_string(),
            Self::Arrival => "arrival".to_string(),
            Self::Continue => "continue".to_string(),
            Self::Turn(turn_direction) => format!(
                "{TURN_STABLE_LABEL_PREFIX}{STABLE_LABEL_PARAMETER_SEPARATOR}{}",
                turn_direction_label_segment(*turn_direction)
            ),
            Self::Merge => "merge".to_string(),
            Self::OnRamp => "on-ramp".to_string(),
            Self::OffRamp => "off-ramp".to_string(),
            Self::Fork => "fork".to_string(),
            Self::Roundabout(exit_index) => format!(
                "{ROUNDABOUT_STABLE_LABEL_PREFIX}{STABLE_LABEL_PARAMETER_SEPARATOR}{exit_index}"
            ),
            Self::Ferry => "ferry".to_string(),
        }
    }

    /// Builds the maneuver a stable label (see [`Self::stable_label`]) names.
    ///
    /// # Parameters
    /// - `stable_label` — a label such as `departure`, `turn-left` or
    ///   `roundabout-2`.
    ///
    /// # Returns
    /// The matching maneuver, or `None` for an unknown or malformed label. No
    /// label is guessed.
    pub fn from_stable_label(stable_label: &str) -> Option<Self> {
        match stable_label {
            "departure" => Some(Self::Departure),
            "arrival" => Some(Self::Arrival),
            "continue" => Some(Self::Continue),
            "merge" => Some(Self::Merge),
            "on-ramp" => Some(Self::OnRamp),
            "off-ramp" => Some(Self::OffRamp),
            "fork" => Some(Self::Fork),
            "ferry" => Some(Self::Ferry),
            _ => parameterized_from_stable_label(stable_label),
        }
    }
}

/// Builds the parameterized maneuvers (`Turn(…)`, `Roundabout(…)`) a stable
/// label names; `None` for anything that is not a well-formed parameterized
/// label of those kinds.
fn parameterized_from_stable_label(stable_label: &str) -> Option<ManeuverType> {
    let label_parameter = |label_prefix: &str| {
        stable_label
            .strip_prefix(label_prefix)
            .and_then(|rest| rest.strip_prefix(STABLE_LABEL_PARAMETER_SEPARATOR))
    };
    if let Some(turn_direction) =
        label_parameter(TURN_STABLE_LABEL_PREFIX).and_then(turn_direction_from_label_segment)
    {
        return Some(ManeuverType::Turn(turn_direction));
    }
    label_parameter(ROUNDABOUT_STABLE_LABEL_PREFIX)
        .and_then(|exit_index_text| exit_index_text.parse::<u8>().ok())
        .filter(|exit_index| *exit_index >= STABLE_LABEL_ROUNDABOUT_MINIMUM_EXIT_INDEX)
        .map(ManeuverType::Roundabout)
}

/// Maneuvers serialize as their stable label.
impl Serialize for ManeuverType {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.stable_label())
    }
}

/// Maneuvers deserialize from their stable label. Any other form is a
/// decoding error, not a guess.
impl<'de> Deserialize<'de> for ManeuverType {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct StableLabelVisitor;

        impl serde::de::Visitor<'_> for StableLabelVisitor {
            type Value = ManeuverType;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(STABLE_LABEL_SERDE_EXPECTATION)
            }

            fn visit_str<E: serde::de::Error>(self, label: &str) -> Result<Self::Value, E> {
                ManeuverType::from_stable_label(label).ok_or_else(|| {
                    E::invalid_value(
                        serde::de::Unexpected::Str(label),
                        &STABLE_LABEL_SERDE_EXPECTATION,
                    )
                })
            }
        }

        deserializer.deserialize_str(StableLabelVisitor)
    }
}

/// Recommended lane(s) for an upcoming maneuver.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LaneGuidance {
    /// Zero-based lane index across the road cross-section.
    pub lane_index: u8,
    /// Directions this lane can legally lead to.
    pub permitted_directions: Vec<TurnDirection>,
    /// Whether this lane is among the recommended ones.
    pub is_recommended: bool,
}

/// One maneuver within a route leg.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteStep {
    /// Distance of this step in meters.
    pub distance_meters: f64,
    /// Travel time of this step in seconds.
    pub duration_seconds: f64,
    /// Maneuver classification.
    pub maneuver_type: ManeuverType,
    /// Human-readable instruction in the display language.
    pub instruction_display_text: String,
    /// Instruction tuned for text-to-speech engines.
    pub instruction_speech_text: String,
    /// Step geometry from the previous step end to this maneuver point.
    pub shape: Vec<GeographicCoordinate>,
    /// Lane guidance available at the maneuver, oldest lane first.
    pub lane_guidances: Vec<LaneGuidance>,
}

/// Leg between two consecutive waypoints.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteLeg {
    /// Leg distance in meters.
    pub distance_meters: f64,
    /// Leg travel time in seconds.
    pub duration_seconds: f64,
    /// Maneuver steps in traversal order.
    pub steps: Vec<RouteStep>,
}

/// A complete calculated route.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteData {
    /// Total distance across all legs, in meters.
    pub distance_meters: f64,
    /// Total travel time across all legs, in seconds.
    pub duration_seconds: f64,
    /// Geographic bounds of the full route geometry.
    pub bounding_box: BoundingBox,
    /// The full route geometry in traversal order, one point per shape
    /// vertex the provider returned. A provider that reports only per-step
    /// shapes leaves this empty and the steps carry the geometry instead;
    /// a recorded document that predates this field carries no list and
    /// decodes to the same empty geometry.
    #[serde(default)]
    pub geometry: Vec<GeographicCoordinate>,
    /// Legs, one per consecutive waypoint pair.
    pub legs: Vec<RouteLeg>,
}

/// One origin-destination cell of a distance matrix.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DistanceMatrixEntry {
    /// Origin index into the request's origins.
    pub origin_index: u32,
    /// Destination index into the request's destinations.
    pub destination_index: u32,
    /// Distance for this cell in meters.
    pub distance_meters: f64,
    /// Travel time for this cell in seconds.
    pub duration_seconds: f64,
}

/// Travel-time matrix for origins × destinations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DistanceMatrix {
    /// Entries in row-major origin-major order.
    pub entries: Vec<DistanceMatrixEntry>,
}
