//! Traffic incident and traffic flow records.

use crate::coordinate::GeographicCoordinate;
use serde::{Deserialize, Serialize};

/// Class of disruption reported in a traffic incident.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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

/// Severity ranking of a traffic incident.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrafficIncidentSeverity {
    /// Minimal delay expected.
    Low,
    /// Noticeable delay expected.
    Medium,
    /// Significant delay expected.
    High,
}

/// One reported disruption inside a queried bounding box.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

/// Congestion classification of a road segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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

/// Flow condition for one road segment (or polyline of road segments)
/// inside the queried box, in kilometers per hour when the provider
/// reports speeds and as a congestion classification otherwise.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// Builds one single-point flow observation.
    ///
    /// # Parameters
    /// - `midpoint_coordinate` — position of the observation.
    /// - `current_speed_kilometers_per_hour` — the speed observed now.
    /// - `free_flow_speed_kilometers_per_hour` — the unconstrained speed of
    ///   the road.
    ///
    /// # Returns
    /// A one-point observation carrying both speeds. The congestion level is
    /// derived from their ratio: at least 0.9 is
    /// [`TrafficFlowCongestionLevel::FreeFlow`], at least 0.5 is
    /// [`TrafficFlowCongestionLevel::Slow`], and anything lower is
    /// [`TrafficFlowCongestionLevel::Congested`]. A free-flow speed of zero
    /// or less reports [`TrafficFlowCongestionLevel::Blocked`].
    pub fn from_speeds(
        midpoint_coordinate: GeographicCoordinate,
        current_speed_kilometers_per_hour: f64,
        free_flow_speed_kilometers_per_hour: f64,
    ) -> Self {
        let congestion_level = Self::classify_congestion_level(
            current_speed_kilometers_per_hour,
            free_flow_speed_kilometers_per_hour,
        );
        Self {
            shape_coordinates: vec![midpoint_coordinate],
            current_speed_kilometers_per_hour: Some(current_speed_kilometers_per_hour),
            free_flow_speed_kilometers_per_hour: Some(free_flow_speed_kilometers_per_hour),
            congestion_level,
        }
    }

    /// Builds one polyline flow observation from a provider that reports only
    /// congestion levels and no speeds.
    ///
    /// # Parameters
    /// - `shape_coordinates` — the segment geometry, ordered along the road.
    /// - `congestion_level` — the provider's congestion classification.
    ///
    /// # Returns
    /// An observation whose speed fields are `None`, because no speed was
    /// reported.
    pub fn from_congestion_level(
        shape_coordinates: Vec<GeographicCoordinate>,
        congestion_level: TrafficFlowCongestionLevel,
    ) -> Self {
        Self {
            shape_coordinates,
            current_speed_kilometers_per_hour: None,
            free_flow_speed_kilometers_per_hour: None,
            congestion_level,
        }
    }

    /// Classifies congestion from the current-to-free-flow speed ratio.
    fn classify_congestion_level(
        current_speed_kilometers_per_hour: f64,
        free_flow_speed_kilometers_per_hour: f64,
    ) -> TrafficFlowCongestionLevel {
        if free_flow_speed_kilometers_per_hour <= 0.0 {
            return TrafficFlowCongestionLevel::Blocked;
        }
        let speed_ratio = current_speed_kilometers_per_hour / free_flow_speed_kilometers_per_hour;
        if speed_ratio >= 0.9 {
            TrafficFlowCongestionLevel::FreeFlow
        } else if speed_ratio >= 0.5 {
            TrafficFlowCongestionLevel::Slow
        } else {
            TrafficFlowCongestionLevel::Congested
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TrafficFlowCongestionLevel;
    use super::TrafficFlowData;
    use crate::coordinate::GeographicCoordinate;

    #[test]
    fn classifies_congestion_from_speed_ratio() {
        let coordinate = GeographicCoordinate::from_degrees(0.0, 0.0).unwrap();
        let from_speeds = TrafficFlowData::from_speeds(coordinate, 55.0, 100.0);
        assert_eq!(from_speeds.shape_coordinates, vec![coordinate]);
        assert_eq!(from_speeds.current_speed_kilometers_per_hour, Some(55.0));
        assert_eq!(
            from_speeds.congestion_level,
            TrafficFlowCongestionLevel::Slow
        );
    }

    #[test]
    fn carries_provider_congestion_buckets_without_speeds() {
        let shape = vec![
            GeographicCoordinate::from_degrees(40.73, -73.99).unwrap(),
            GeographicCoordinate::from_degrees(40.72, -74.01).unwrap(),
        ];
        let polyline =
            TrafficFlowData::from_congestion_level(shape, TrafficFlowCongestionLevel::Blocked);
        assert_eq!(polyline.shape_coordinates.len(), 2);
        assert_eq!(polyline.current_speed_kilometers_per_hour, None);
        assert_eq!(polyline.free_flow_speed_kilometers_per_hour, None);
        assert_eq!(
            polyline.congestion_level,
            TrafficFlowCongestionLevel::Blocked
        );
    }
}
