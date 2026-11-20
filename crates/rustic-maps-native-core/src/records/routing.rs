//! Route requests, route results and distance matrices.

use super::{BoundingBox, GeographicCoordinate};
use crate::error::SdkError;

/// Mode of transport used to calculate a route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportationMode {
    /// Private car routing.
    Automobile,
    /// Bicycle routing.
    Bicycle,
    /// Walking routing.
    Pedestrian,
}

impl TransportationMode {
    /// Builds the native transportation mode from the mirror enum.
    pub(crate) fn to_native_transportation_mode(self) -> geospatial_types::TransportationMode {
        match self {
            Self::Automobile => geospatial_types::TransportationMode::Automobile,
            Self::Bicycle => geospatial_types::TransportationMode::Bicycle,
            Self::Pedestrian => geospatial_types::TransportationMode::Pedestrian,
        }
    }

    /// Builds the mirror enum from the native transportation mode — the
    /// foreign provider adapters' direction, handing a native routing request
    /// to an implementation written in application code.
    pub(crate) fn from_native_transportation_mode(
        native_transportation_mode: geospatial_types::TransportationMode,
    ) -> Self {
        match native_transportation_mode {
            geospatial_types::TransportationMode::Automobile => Self::Automobile,
            geospatial_types::TransportationMode::Bicycle => Self::Bicycle,
            geospatial_types::TransportationMode::Pedestrian => Self::Pedestrian,
        }
    }
}

/// Length and speed conventions used when displaying a route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitSystem {
    /// Kilometers and kilometers per hour.
    Metric,
    /// Miles and miles per hour.
    Imperial,
}

impl UnitSystem {
    /// Builds the native unit system from the mirror enum.
    pub(crate) fn to_native_unit_system(self) -> geospatial_types::UnitSystem {
        match self {
            Self::Metric => geospatial_types::UnitSystem::Metric,
            Self::Imperial => geospatial_types::UnitSystem::Imperial,
        }
    }

    /// Builds the mirror enum from the native unit system (the foreign
    /// provider adapters' direction).
    pub(crate) fn from_native_unit_system(
        native_unit_system: geospatial_types::UnitSystem,
    ) -> Self {
        match native_unit_system {
            geospatial_types::UnitSystem::Metric => Self::Metric,
            geospatial_types::UnitSystem::Imperial => Self::Imperial,
        }
    }
}

/// What a routing request optimizes for; the strategy applies to automobile
/// routing only (the current providers have no time-based cost model for the
/// other modes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteStrategy {
    /// Minimize expected travel time.
    Fastest,
    /// Minimize traveled distance.
    Shortest,
}

impl RouteStrategy {
    /// Builds the native route strategy from the mirror enum.
    pub(crate) fn to_native_route_strategy(self) -> geospatial_types::route::RouteStrategy {
        match self {
            Self::Fastest => geospatial_types::route::RouteStrategy::Fastest,
            Self::Shortest => geospatial_types::route::RouteStrategy::Shortest,
        }
    }

    /// Builds the mirror enum from the native route strategy (the foreign
    /// provider adapters' direction).
    pub(crate) fn from_native_route_strategy(
        native_route_strategy: geospatial_types::route::RouteStrategy,
    ) -> Self {
        match native_route_strategy {
            geospatial_types::route::RouteStrategy::Fastest => Self::Fastest,
            geospatial_types::route::RouteStrategy::Shortest => Self::Shortest,
        }
    }
}

/// Road features a routing request can ask the provider to avoid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

impl AvoidOptions {
    /// Builds the native avoid set from the mirror record.
    pub(crate) fn to_native_avoid_options(self) -> geospatial_types::AvoidOptions {
        geospatial_types::AvoidOptions {
            toll_roads: self.toll_roads,
            limited_access_highways: self.limited_access_highways,
            ferries: self.ferries,
            unpaved_roads: self.unpaved_roads,
        }
    }

    /// Builds the mirror record from the native avoid set (the foreign
    /// provider adapters' direction).
    pub(crate) fn from_native_avoid_options(
        native_avoid_options: &geospatial_types::AvoidOptions,
    ) -> Self {
        Self {
            toll_roads: native_avoid_options.toll_roads,
            limited_access_highways: native_avoid_options.limited_access_highways,
            ferries: native_avoid_options.ferries,
            unpaved_roads: native_avoid_options.unpaved_roads,
        }
    }
}

/// Options controlling how a route is calculated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteOptions {
    /// Mode of transport.
    pub transportation_mode: TransportationMode,
    /// Whether routing optimizes travel time or distance.
    pub route_strategy: RouteStrategy,
    /// Display unit convention the provider should annotate.
    pub unit_system: UnitSystem,
    /// Features to avoid.
    pub avoid_options: AvoidOptions,
    /// Departure instant as seconds since the Unix epoch; `None` for
    /// "depart now".
    pub departure_timestamp_seconds: Option<u64>,
}

impl RouteOptions {
    /// Builds the native options from the mirror record, consuming it (the
    /// record is a `Copy` value).
    pub(crate) fn to_native_route_options(self) -> geospatial_types::RouteOptions {
        geospatial_types::RouteOptions {
            transportation_mode: self.transportation_mode.to_native_transportation_mode(),
            route_strategy: self.route_strategy.to_native_route_strategy(),
            unit_system: self.unit_system.to_native_unit_system(),
            avoid_options: self.avoid_options.to_native_avoid_options(),
            departure_timestamp_seconds: self.departure_timestamp_seconds,
        }
    }

    /// Builds the mirror record from the native options — the foreign
    /// provider adapters' direction, handing a native routing request to an
    /// implementation written in application code.
    pub(crate) fn from_native_route_options(
        native_route_options: &geospatial_types::RouteOptions,
    ) -> Self {
        Self {
            transportation_mode: TransportationMode::from_native_transportation_mode(
                native_route_options.transportation_mode,
            ),
            route_strategy: RouteStrategy::from_native_route_strategy(
                native_route_options.route_strategy,
            ),
            unit_system: UnitSystem::from_native_unit_system(native_route_options.unit_system),
            avoid_options: AvoidOptions::from_native_avoid_options(
                &native_route_options.avoid_options,
            ),
            departure_timestamp_seconds: native_route_options.departure_timestamp_seconds,
        }
    }
}

/// Which transport modes and strategies the serving routing provider can
/// honour, as reported by `MapSdk::route_capabilities`.
#[derive(Debug, Clone, PartialEq)]
pub struct RouteCapabilities {
    /// The transport modes the provider can route for.
    pub supported_transportation_modes: Vec<TransportationMode>,
    /// Whether the provider can minimize traveled distance for automobile
    /// routing.
    pub supports_shortest_route_strategy: bool,
}

impl RouteCapabilities {
    /// Builds the mirror record from the native capability answer.
    pub(crate) fn from_native_route_capabilities(
        native_route_capabilities: &geospatial_types::route::RouteCapabilities,
    ) -> Self {
        Self {
            supported_transportation_modes: native_route_capabilities
                .supported_transportation_modes
                .iter()
                .map(|mode| TransportationMode::from_native_transportation_mode(*mode))
                .collect(),
            supports_shortest_route_strategy: native_route_capabilities
                .supports_shortest_route_strategy,
        }
    }
}

/// One maneuver within a route leg. The native maneuver classification
/// crosses as a stable snake-case text label (see
/// [`maneuver_type_text`]) and this record carries no lane guidance.
#[derive(Debug, Clone, PartialEq)]
pub struct RouteStep {
    /// Distance of this step in meters.
    pub distance_meters: f64,
    /// Travel time of this step in seconds.
    pub duration_seconds: f64,
    /// Maneuver classification as a stable snake-case text label; the
    /// turn direction and the one-based roundabout exit index are carried
    /// in the label (`turn-left`, `roundabout-2`).
    pub maneuver_type: String,
    /// Human-readable instruction in the display language.
    pub instruction_display_text: String,
    /// Instruction tuned for text-to-speech engines.
    pub instruction_speech_text: String,
    /// Step geometry from the previous step end to this maneuver point.
    pub shape: Vec<GeographicCoordinate>,
}

/// Stable snake-case text labels of the native maneuver classification;
/// turn directions and the roundabout exit index are embedded in the label,
/// because the native classification carries payloads UDL enums cannot.
///
/// The spelling is owned by [`geospatial_types::ManeuverType::stable_label`]
/// (the same single spelling the serde serialization of route records emits)
/// — this converter delegates so the FFI surface and the serialized route
/// records cannot drift apart.
pub(crate) fn maneuver_type_text(native_maneuver_type: &geospatial_types::ManeuverType) -> String {
    native_maneuver_type.stable_label()
}

/// Builds the native maneuver classification back from one of the stable
/// snake-case labels [`maneuver_type_text`] emitted; the inverse of that
/// labeling, needed because route records cross the FFI as labels. Unknown
/// labels are refused at the caller's boundary (never guessed).
pub(crate) fn native_maneuver_type_from_text(
    maneuver_type_text: &str,
) -> Result<geospatial_types::ManeuverType, SdkError> {
    geospatial_types::ManeuverType::from_stable_label(maneuver_type_text).ok_or_else(|| {
        SdkError::InvalidNavigationInput {
            detail: format!("unrecognized step maneuver label `{maneuver_type_text}`"),
        }
    })
}

impl RouteStep {
    /// Builds the mirror record from a native route step.
    pub(crate) fn from_native_route_step(native_route_step: &geospatial_types::RouteStep) -> Self {
        Self {
            distance_meters: native_route_step.distance_meters,
            duration_seconds: native_route_step.duration_seconds,
            maneuver_type: maneuver_type_text(&native_route_step.maneuver_type),
            instruction_display_text: native_route_step.instruction_display_text.clone(),
            instruction_speech_text: native_route_step.instruction_speech_text.clone(),
            shape: native_route_step
                .shape
                .iter()
                .map(GeographicCoordinate::from_native_coordinate)
                .collect(),
        }
    }

    /// Builds the native route step from the mirror record, refusing a
    /// maneuver label the converter cannot map. Lane guidance is not part of
    /// this surface (see [`Self::from_native_route_step`]), so steps rebuilt
    /// across the FFI arrive without it — mirroring the forward direction.
    pub(crate) fn into_native_route_step(self) -> Result<geospatial_types::RouteStep, SdkError> {
        Ok(geospatial_types::RouteStep {
            distance_meters: self.distance_meters,
            duration_seconds: self.duration_seconds,
            maneuver_type: native_maneuver_type_from_text(&self.maneuver_type)?,
            instruction_display_text: self.instruction_display_text,
            instruction_speech_text: self.instruction_speech_text,
            shape: self
                .shape
                .into_iter()
                .map(|shape_coordinate| shape_coordinate.to_native_coordinate())
                .collect(),
            lane_guidances: Vec::new(),
        })
    }
}

/// Leg between two consecutive waypoints.
#[derive(Debug, Clone, PartialEq)]
pub struct RouteLeg {
    /// Leg distance in meters.
    pub distance_meters: f64,
    /// Leg travel time in seconds.
    pub duration_seconds: f64,
    /// Maneuver steps in traversal order.
    pub steps: Vec<RouteStep>,
}

impl RouteLeg {
    /// Builds the mirror record from a native route leg.
    pub(crate) fn from_native_route_leg(native_route_leg: &geospatial_types::RouteLeg) -> Self {
        Self {
            distance_meters: native_route_leg.distance_meters,
            duration_seconds: native_route_leg.duration_seconds,
            steps: native_route_leg
                .steps
                .iter()
                .map(RouteStep::from_native_route_step)
                .collect(),
        }
    }

    /// Builds the native route leg from the mirror record, refusing a step
    /// with an unrecognized maneuver label.
    pub(crate) fn into_native_route_leg(self) -> Result<geospatial_types::RouteLeg, SdkError> {
        Ok(geospatial_types::RouteLeg {
            distance_meters: self.distance_meters,
            duration_seconds: self.duration_seconds,
            steps: self
                .steps
                .into_iter()
                .map(|mirror_step| mirror_step.into_native_route_step())
                .collect::<Result<Vec<_>, SdkError>>()?,
        })
    }
}

/// A complete calculated route.
#[derive(Debug, Clone, PartialEq)]
pub struct Route {
    /// Total distance across all legs, in meters.
    pub distance_meters: f64,
    /// Total travel time across all legs, in seconds.
    pub duration_seconds: f64,
    /// Geographic bounds of the full route geometry.
    pub bounding_box: BoundingBox,
    /// The full route geometry in traversal order.
    pub geometry: Vec<GeographicCoordinate>,
    /// Legs, one per consecutive waypoint pair.
    pub legs: Vec<RouteLeg>,
}

impl Route {
    /// Builds the mirror record from a native route.
    pub(crate) fn from_native_route_data(native_route_data: &geospatial_types::RouteData) -> Self {
        Self {
            distance_meters: native_route_data.distance_meters,
            duration_seconds: native_route_data.duration_seconds,
            bounding_box: BoundingBox::from_native_bounding_box(&native_route_data.bounding_box),
            geometry: native_route_data
                .geometry
                .iter()
                .map(GeographicCoordinate::from_native_coordinate)
                .collect(),
            legs: native_route_data
                .legs
                .iter()
                .map(RouteLeg::from_native_route_leg)
                .collect(),
        }
    }

    /// Builds the native route from the mirror record, refusing a step with
    /// an unrecognized maneuver label.
    pub(crate) fn into_native_route_data(self) -> Result<geospatial_types::RouteData, SdkError> {
        Ok(geospatial_types::RouteData {
            distance_meters: self.distance_meters,
            duration_seconds: self.duration_seconds,
            bounding_box: self.bounding_box.to_native_bounding_box(),
            geometry: self
                .geometry
                .into_iter()
                .map(GeographicCoordinate::to_native_coordinate)
                .collect(),
            legs: self
                .legs
                .into_iter()
                .map(|mirror_leg| mirror_leg.into_native_route_leg())
                .collect::<Result<Vec<_>, SdkError>>()?,
        })
    }
}

/// One origin-destination cell of a distance matrix.
#[derive(Debug, Clone, Copy, PartialEq)]
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

/// Travel-time matrix for origins x destinations.
#[derive(Debug, Clone, PartialEq)]
pub struct DistanceMatrix {
    /// Entries in row-major origin-major order.
    pub entries: Vec<DistanceMatrixEntry>,
}

impl DistanceMatrix {
    /// Builds the mirror record from a native distance matrix.
    pub(crate) fn from_native_distance_matrix(
        native_distance_matrix: &geospatial_types::DistanceMatrix,
    ) -> Self {
        Self {
            entries: native_distance_matrix
                .entries
                .iter()
                .map(|native_entry| DistanceMatrixEntry {
                    origin_index: native_entry.origin_index,
                    destination_index: native_entry.destination_index,
                    distance_meters: native_entry.distance_meters,
                    duration_seconds: native_entry.duration_seconds,
                })
                .collect(),
        }
    }

    /// Builds the native distance matrix from the mirror record — the foreign
    /// provider adapters' direction. The cell values copy verbatim: a
    /// provider's reported distance and duration are its answer, and the
    /// routing call that requested the matrix is where input validity is
    /// enforced.
    pub(crate) fn into_native_distance_matrix(self) -> geospatial_types::DistanceMatrix {
        geospatial_types::DistanceMatrix {
            entries: self
                .entries
                .into_iter()
                .map(|entry| geospatial_types::DistanceMatrixEntry {
                    origin_index: entry.origin_index,
                    destination_index: entry.destination_index,
                    distance_meters: entry.distance_meters,
                    duration_seconds: entry.duration_seconds,
                })
                .collect(),
        }
    }
}
