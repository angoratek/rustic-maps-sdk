//! Shared data records and the error model for the Maps SDK.
//!
//! The crate defines the value types exchanged between provider
//! integrations, the rendering engine, the routing engine and the navigation
//! engine: coordinates, bounding boxes, places, route and traffic records,
//! raster and vector tile data, and the unified [`SDKError`] type.
//!
//! It performs no input or output and holds no state.

pub mod bounding_box;
pub mod coordinate;
pub mod elevation;
pub mod error;
pub mod place;
pub mod raster;
pub mod route;
pub mod traffic;
pub mod vector;

pub use bounding_box::BoundingBox;
pub use coordinate::GeographicCoordinate;
pub use elevation::{ElevationPoint, ElevationProfile};
pub use error::{CacheOperationFailureReason, OfflineGraphInvalidReason, SDKError};
pub use place::{GeocodeQuality, PlaceResult};
pub use raster::{RasterImageFormat, RasterTileData, RasterTileResolution};
pub use route::{
    AvoidOptions, DistanceMatrix, DistanceMatrixEntry, LaneGuidance, ManeuverType, RouteData,
    RouteLeg, RouteOptions, RouteStep, TransportationMode, TurnDirection, UnitSystem,
};
pub use traffic::{
    TrafficFlowCongestionLevel, TrafficFlowData, TrafficIncident, TrafficIncidentSeverity,
    TrafficIncidentType,
};
pub use vector::{VectorTileData, VectorTileEncoding};
