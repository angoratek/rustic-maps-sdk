//! Traffic incidents, flow observations and their classifications.

use super::GeographicCoordinate;
use crate::error::SdkError;

/// Class of disruption reported in a traffic incident.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrafficIncidentType {
    /// Road construction.
    Construction,
    /// Traffic-impacting event.
    Event,
    /// Collision.
    Accident,
    /// Weather-related disruption.
    Weather,
    /// Lane or road closure.
    Closure,
}

impl TrafficIncidentType {
    /// Builds the mirror enum from the native incident type.
    pub(crate) fn from_native_traffic_incident_type(
        native_type: geospatial_types::TrafficIncidentType,
    ) -> Self {
        match native_type {
            geospatial_types::TrafficIncidentType::Construction => Self::Construction,
            geospatial_types::TrafficIncidentType::Event => Self::Event,
            geospatial_types::TrafficIncidentType::Accident => Self::Accident,
            geospatial_types::TrafficIncidentType::Weather => Self::Weather,
            geospatial_types::TrafficIncidentType::Closure => Self::Closure,
        }
    }

    /// Builds the native incident type from the mirror enum (the foreign
    /// provider adapters' direction).
    pub(crate) fn to_native_traffic_incident_type(self) -> geospatial_types::TrafficIncidentType {
        match self {
            Self::Construction => geospatial_types::TrafficIncidentType::Construction,
            Self::Event => geospatial_types::TrafficIncidentType::Event,
            Self::Accident => geospatial_types::TrafficIncidentType::Accident,
            Self::Weather => geospatial_types::TrafficIncidentType::Weather,
            Self::Closure => geospatial_types::TrafficIncidentType::Closure,
        }
    }
}

/// Severity ranking of a traffic incident.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrafficIncidentSeverity {
    /// Minimal delay expected.
    Low,
    /// Noticeable delay expected.
    Medium,
    /// Significant delay expected.
    High,
}

impl TrafficIncidentSeverity {
    /// Builds the mirror enum from the native incident severity.
    pub(crate) fn from_native_traffic_incident_severity(
        native_severity: geospatial_types::TrafficIncidentSeverity,
    ) -> Self {
        match native_severity {
            geospatial_types::TrafficIncidentSeverity::Low => Self::Low,
            geospatial_types::TrafficIncidentSeverity::Medium => Self::Medium,
            geospatial_types::TrafficIncidentSeverity::High => Self::High,
        }
    }

    /// Builds the native incident severity from the mirror enum (the foreign
    /// provider adapters' direction).
    pub(crate) fn to_native_traffic_incident_severity(
        self,
    ) -> geospatial_types::TrafficIncidentSeverity {
        match self {
            Self::Low => geospatial_types::TrafficIncidentSeverity::Low,
            Self::Medium => geospatial_types::TrafficIncidentSeverity::Medium,
            Self::High => geospatial_types::TrafficIncidentSeverity::High,
        }
    }
}

/// Congestion classification of a road segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrafficFlowCongestionLevel {
    /// At or above free-flow speed.
    FreeFlow,
    /// Moderately below free-flow speed.
    Slow,
    /// Stop-and-go traffic.
    Congested,
    /// Roadway effectively closed.
    Blocked,
}

impl TrafficFlowCongestionLevel {
    /// Builds the mirror enum from the native congestion level.
    pub(crate) fn from_native_congestion_level(
        native_congestion_level: geospatial_types::TrafficFlowCongestionLevel,
    ) -> Self {
        match native_congestion_level {
            geospatial_types::TrafficFlowCongestionLevel::FreeFlow => Self::FreeFlow,
            geospatial_types::TrafficFlowCongestionLevel::Slow => Self::Slow,
            geospatial_types::TrafficFlowCongestionLevel::Congested => Self::Congested,
            geospatial_types::TrafficFlowCongestionLevel::Blocked => Self::Blocked,
        }
    }
}

/// One reported disruption inside a queried bounding box.
#[derive(Debug, Clone, PartialEq)]
pub struct TrafficIncident {
    /// Provider-stable incident identifier.
    pub identifier: String,
    /// Class of disruption.
    pub incident_type: TrafficIncidentType,
    /// Severity ranking.
    pub severity: TrafficIncidentSeverity,
    /// Provider display text for this incident.
    pub short_description_text: String,
    /// Position where the incident starts.
    pub start_coordinate: GeographicCoordinate,
    /// Position where the incident ends, if reported.
    pub end_coordinate: Option<GeographicCoordinate>,
}

impl TrafficIncident {
    /// Builds the mirror record from a native traffic incident.
    pub(crate) fn from_native_traffic_incident(
        native_traffic_incident: &geospatial_types::TrafficIncident,
    ) -> Self {
        Self {
            identifier: native_traffic_incident.identifier.clone(),
            incident_type: TrafficIncidentType::from_native_traffic_incident_type(
                native_traffic_incident.incident_type,
            ),
            severity: TrafficIncidentSeverity::from_native_traffic_incident_severity(
                native_traffic_incident.severity,
            ),
            short_description_text: native_traffic_incident.short_description_text.clone(),
            start_coordinate: GeographicCoordinate::from_native_coordinate(
                &native_traffic_incident.start_coordinate,
            ),
            end_coordinate: native_traffic_incident
                .end_coordinate
                .map(|native_end_coordinate| {
                    GeographicCoordinate::from_native_coordinate(&native_end_coordinate)
                }),
        }
    }

    /// Builds the native traffic incident from the mirror record — the
    /// foreign provider adapters' direction. The coordinates validate like
    /// every other geometry a provider returns, so an out-of-range incident
    /// position refuses the call rather than entering the map pipeline.
    pub(crate) fn into_native_traffic_incident(
        self,
    ) -> Result<geospatial_types::TrafficIncident, SdkError> {
        Ok(geospatial_types::TrafficIncident {
            identifier: self.identifier,
            incident_type: self.incident_type.to_native_traffic_incident_type(),
            severity: self.severity.to_native_traffic_incident_severity(),
            short_description_text: self.short_description_text,
            start_coordinate: crate::projection::validated_coordinate(self.start_coordinate)?,
            end_coordinate: self
                .end_coordinate
                .map(crate::projection::validated_coordinate)
                .transpose()?,
        })
    }
}

/// Flow condition for one road segment (or polyline of road segments) inside
/// the queried box, in kilometers per hour when the provider reports speeds
/// and as a congestion classification otherwise.
#[derive(Debug, Clone, PartialEq)]
pub struct TrafficFlowData {
    /// Segment geometry, ordered along the road.
    pub shape_coordinates: Vec<GeographicCoordinate>,
    /// Currently observed speed in kilometers per hour, when reported.
    pub current_speed_kilometers_per_hour: Option<f64>,
    /// Unconstrained (free-flow) speed in kilometers per hour, when reported.
    pub free_flow_speed_kilometers_per_hour: Option<f64>,
    /// Congestion classification from the speed ratio or provider buckets.
    pub congestion_level: TrafficFlowCongestionLevel,
}

impl TrafficFlowData {
    /// Builds the mirror record from a native traffic flow observation.
    pub(crate) fn from_native_traffic_flow_data(
        native_traffic_flow_data: &geospatial_types::TrafficFlowData,
    ) -> Self {
        Self {
            shape_coordinates: native_traffic_flow_data
                .shape_coordinates
                .iter()
                .map(GeographicCoordinate::from_native_coordinate)
                .collect(),
            current_speed_kilometers_per_hour: native_traffic_flow_data
                .current_speed_kilometers_per_hour,
            free_flow_speed_kilometers_per_hour: native_traffic_flow_data
                .free_flow_speed_kilometers_per_hour,
            congestion_level: TrafficFlowCongestionLevel::from_native_congestion_level(
                native_traffic_flow_data.congestion_level,
            ),
        }
    }

    /// Builds the native traffic flow observation from the mirror record —
    /// the surface-view converter's and the foreign provider adapters'
    /// direction, feeding the overlay assembly. The shape coordinates
    /// validate like every other overlay payload; the speeds and the
    /// congestion classification copy verbatim (the congestion color lookup
    /// is total over the classification).
    pub(crate) fn into_native_traffic_flow_data(
        self,
    ) -> Result<geospatial_types::TrafficFlowData, SdkError> {
        Ok(geospatial_types::TrafficFlowData {
            shape_coordinates: self
                .shape_coordinates
                .into_iter()
                .map(crate::projection::validated_coordinate)
                .collect::<Result<Vec<_>, SdkError>>()?,
            current_speed_kilometers_per_hour: self.current_speed_kilometers_per_hour,
            free_flow_speed_kilometers_per_hour: self.free_flow_speed_kilometers_per_hour,
            congestion_level: self.congestion_level.to_native_congestion_level(),
        })
    }
}

impl TrafficFlowCongestionLevel {
    /// Builds the native congestion level from the mirror enum.
    pub(crate) fn to_native_congestion_level(self) -> geospatial_types::TrafficFlowCongestionLevel {
        match self {
            Self::FreeFlow => geospatial_types::TrafficFlowCongestionLevel::FreeFlow,
            Self::Slow => geospatial_types::TrafficFlowCongestionLevel::Slow,
            Self::Congested => geospatial_types::TrafficFlowCongestionLevel::Congested,
            Self::Blocked => geospatial_types::TrafficFlowCongestionLevel::Blocked,
        }
    }
}
