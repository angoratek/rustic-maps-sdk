//! Response categories and their default freshness policies.
//!
//! Every cache entry is tagged with one [`ResponseCategory`]; the category
//! drives the default time-to-live so that volatile traffic data expires
//! minutes after capture while immutable tile imagery lives for weeks.
//! Individual calls may pass a shorter or longer TTL — the cached provider
//! wrappers in [`crate::cached_providers`] accept a per-instance override.

use geospatial_types::SDKError;
use serde::{Deserialize, Serialize};

const SECONDS_PER_MINUTE: u64 = 60;
const SECONDS_PER_HOUR: u64 = 60 * SECONDS_PER_MINUTE;
const SECONDS_PER_DAY: u64 = 24 * SECONDS_PER_HOUR;

/// Geocoding results change slowly enough to keep for a day.
const GEOCODING_TIME_TO_LIVE_SECONDS: u64 = 24 * SECONDS_PER_HOUR;
/// Routes and matrices follow live traffic and road closures.
const ROUTING_TIME_TO_LIVE_SECONDS: u64 = 10 * SECONDS_PER_MINUTE;
const DISTANCE_MATRIX_TIME_TO_LIVE_SECONDS: u64 = 10 * SECONDS_PER_MINUTE;
/// Predictions follow the places a provider currently knows.
const SEARCH_PREDICTION_TIME_TO_LIVE_SECONDS: u64 = 5 * SECONDS_PER_MINUTE;
/// Incident reports are the most volatile provider response.
const TRAFFIC_INCIDENTS_TIME_TO_LIVE_SECONDS: u64 = SECONDS_PER_MINUTE;
/// Congestion polylines move with every probe update.
const TRAFFIC_FLOW_TIME_TO_LIVE_SECONDS: u64 = 30;
/// The common ceiling under imagery-provider terms.
const TILE_IMAGERY_TIME_TO_LIVE_SECONDS: u64 = 30 * SECONDS_PER_DAY;

/// Kind of provider response a cache entry holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseCategory {
    /// Forward and reverse geocoding results.
    Geocoding,
    /// Calculated routes and their maneuvers.
    Routing,
    /// Origin-destination travel-time matrices.
    DistanceMatrix,
    /// Place search predictions (autocomplete and full search).
    SearchPrediction,
    /// Traffic disruption reports.
    TrafficIncidents,
    /// Traffic flow congestion polylines.
    TrafficFlow,
    /// Fetched raster XYZ tiles.
    RasterTiles,
    /// Fetched xyz vector tiles (MVT protobuf).
    VectorTiles,
    /// The style document, glyph ranges and sprite sheets a provider
    /// publishes for its vector tiles.
    StyleResources,
}

impl ResponseCategory {
    /// Default time-to-live in seconds for this category. Values reflect
    /// how quickly the underlying data changes; anything more aggressive
    /// than these defaults is wasteful, anything longer risks serving
    /// plainly stale live data.
    ///
    /// Tiles follow their own policy: tile imagery caches heavily by default
    /// — **30 calendar days**, the common maximum under imagery-provider
    /// terms — and an integrator tuning it shorter or longer does so
    /// knowingly per provider (each provider contract declares its own
    /// caching terms). The cached provider wrappers accept the per-instance
    /// override.
    ///
    /// Vector tiles inherit the same tile-imagery policy: 30 calendar days
    /// here, clamped against whichever provider ceiling the installed vector
    /// source declares. The cache layer carries no provider value itself.
    pub fn default_time_to_live_seconds(self) -> u64 {
        match self {
            ResponseCategory::Geocoding => GEOCODING_TIME_TO_LIVE_SECONDS,
            ResponseCategory::Routing => ROUTING_TIME_TO_LIVE_SECONDS,
            ResponseCategory::DistanceMatrix => DISTANCE_MATRIX_TIME_TO_LIVE_SECONDS,
            ResponseCategory::SearchPrediction => SEARCH_PREDICTION_TIME_TO_LIVE_SECONDS,
            ResponseCategory::TrafficIncidents => TRAFFIC_INCIDENTS_TIME_TO_LIVE_SECONDS,
            ResponseCategory::TrafficFlow => TRAFFIC_FLOW_TIME_TO_LIVE_SECONDS,
            ResponseCategory::RasterTiles => TILE_IMAGERY_TIME_TO_LIVE_SECONDS,
            ResponseCategory::VectorTiles => TILE_IMAGERY_TIME_TO_LIVE_SECONDS,
            ResponseCategory::StyleResources => TILE_IMAGERY_TIME_TO_LIVE_SECONDS,
        }
    }

    /// Stable name persisted in the SQLite `category` column.
    pub fn storage_name(self) -> &'static str {
        match self {
            ResponseCategory::Geocoding => "geocoding",
            ResponseCategory::Routing => "routing",
            ResponseCategory::DistanceMatrix => "distance_matrix",
            ResponseCategory::SearchPrediction => "search_prediction",
            ResponseCategory::TrafficIncidents => "traffic_incidents",
            ResponseCategory::TrafficFlow => "traffic_flow",
            ResponseCategory::RasterTiles => "raster_tiles",
            ResponseCategory::VectorTiles => "vector_tiles",
            ResponseCategory::StyleResources => "style_resources",
        }
    }

    /// Reverses [`ResponseCategory::storage_name`].
    pub fn from_storage_name(storage_name: &str) -> Result<Self, SDKError> {
        match storage_name {
            "geocoding" => Ok(ResponseCategory::Geocoding),
            "routing" => Ok(ResponseCategory::Routing),
            "distance_matrix" => Ok(ResponseCategory::DistanceMatrix),
            "search_prediction" => Ok(ResponseCategory::SearchPrediction),
            "traffic_incidents" => Ok(ResponseCategory::TrafficIncidents),
            "traffic_flow" => Ok(ResponseCategory::TrafficFlow),
            "raster_tiles" => Ok(ResponseCategory::RasterTiles),
            "vector_tiles" => Ok(ResponseCategory::VectorTiles),
            "style_resources" => Ok(ResponseCategory::StyleResources),
            other => {
                let _ = other;
                Err(SDKError::CacheOperationFailure(
                    geospatial_types::CacheOperationFailureReason::RecordCorrupted,
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ResponseCategory, SDKError};

    #[test]
    fn expiry_ages_follow_data_volatility() {
        assert!(ResponseCategory::TrafficFlow.default_time_to_live_seconds() < 60);
        assert!(
            ResponseCategory::TrafficIncidents.default_time_to_live_seconds()
                < ResponseCategory::Routing.default_time_to_live_seconds()
        );
        assert_eq!(
            ResponseCategory::Geocoding.default_time_to_live_seconds(),
            86_400
        );
        assert_eq!(
            ResponseCategory::RasterTiles.default_time_to_live_seconds(),
            30 * 24 * 60 * 60
        );
        assert_eq!(
            ResponseCategory::StyleResources.default_time_to_live_seconds(),
            30 * 24 * 60 * 60,
            "a published style follows the same thirty-day window as tile imagery"
        );
    }

    #[test]
    fn storage_names_round_trip() {
        for category in [
            ResponseCategory::Geocoding,
            ResponseCategory::Routing,
            ResponseCategory::DistanceMatrix,
            ResponseCategory::SearchPrediction,
            ResponseCategory::TrafficIncidents,
            ResponseCategory::TrafficFlow,
            ResponseCategory::RasterTiles,
            ResponseCategory::VectorTiles,
            ResponseCategory::StyleResources,
        ] {
            assert_eq!(
                ResponseCategory::from_storage_name(category.storage_name()).unwrap(),
                category
            );
        }
        assert!(matches!(
            ResponseCategory::from_storage_name("unknown"),
            Err(SDKError::CacheOperationFailure(
                geospatial_types::CacheOperationFailureReason::RecordCorrupted
            ))
        ));
    }
}
