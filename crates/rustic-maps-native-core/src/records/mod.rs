//! The binding-surface records of the UDL contract
//! (`src/rustic_maps.udl`).
//!
//! Records are plain value shapes on the FFI, so the mirror structs carry
//! exactly the documented fields; the projection core's own types
//! (`rustic-maps-rendering` `TileKey`/two-component arrays, `geospatial-types`
//! coordinates) convert through the explicit constructors here, because
//! FFI traits cannot be implemented for foreign-crate types. The async
//! provider surface mirrors the `geospatial-types` provider enums and
//! records under the same names.

mod diagnostics;
mod geometry;
mod navigation;
mod overlays;
mod places;
mod positioning;
mod provider;
mod routing;
mod terrain;
mod tiles;
mod traffic;

pub use diagnostics::{
    FrameStatistics, LogEvent, ProviderFailureClassification, ProviderFailureDiagnostic,
    ProviderRequestTrace, SdkLogSeverity, TileCacheStatistics, TileCacheTerms,
    TrafficFlowCacheStatistics,
};
pub use geometry::{
    BoundingBox, EdgeInsets, GeographicCoordinate, NormalizedWorldCoordinate, ScreenPoint, TileKey,
    ViewportSize,
};
pub use navigation::{
    ActiveManeuverInstruction, DeviationGradeKind, DeviationInfo, DeviationPolicy,
    DeviationPolicyKind, DeviationStatusChange, DeviationStatusChangeKind, DistancePhrase,
    DistancePhraseKind, DistanceUnit, ManeuverInstructionText, ManeuverInstructionTextKind,
    NavigationInstruction, NavigationLocationUpdate, NavigationPhase, NavigationPhaseChange,
    ReroutePolicy, RouteStepPosition, SimulatedTripFix, SimulatedTripPace, SpeechInstruction,
    SpeechInstructionKind, StepAdvancePolicy, StepAdvancePolicyKind, TripNavigationState,
    TripProgress, UpcomingManeuverInstruction,
};
pub use overlays::{
    LocationIndicatorState, LocationIndicatorStyle, LocationIndicatorVisualKind, MapOverlay,
    MapOverlayKind, OverlayColor, SurfaceHandle,
};
pub use places::{GeocodeQuality, Place};
pub use positioning::{PositionFix, PositionSourceError};
pub use provider::{ProviderContract, ProviderRegistration};
pub use routing::{
    AvoidOptions, DistanceMatrix, DistanceMatrixEntry, Route, RouteCapabilities, RouteLeg,
    RouteOptions, RouteStep, RouteStrategy, TransportationMode, UnitSystem,
};
pub use terrain::ElevationPoint;
pub use tiles::{
    ProviderVectorTileSource, RasterImageFormat, RasterTileData, RasterTileResolution,
    RegionDefinition, RegionDownloadPhase, RegionDownloadProgress, RegionRecord,
    RegionRemovalReport, SourceHeader, UrlTemplateRasterTileSource, UrlTemplateVectorTileSource,
    VectorTileData, VectorTileEncoding, VectorTileSourceDescriptor, VectorTileSourceLayerCount,
    VectorTileSourceStatus,
};
pub use traffic::{
    TrafficFlowCongestionLevel, TrafficFlowData, TrafficIncident, TrafficIncidentSeverity,
    TrafficIncidentType,
};
