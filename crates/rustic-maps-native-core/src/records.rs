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

use rustic_maps_rendering::TileKey as RenderingTileKey;
// The per-frame accounting of the native surface renderer: the wasm build
// has no surface renderer (the canvas owns the browser surface), so the
// converter from it exists natively only.
#[cfg(not(target_arch = "wasm32"))]
use rustic_maps_rendering::SurfaceFrameStatistics;

use crate::error::SdkError;

/// A position on the WGS 84 ellipsoid in decimal degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeographicCoordinate {
    /// Latitude in decimal degrees, valid within `[-90, 90]`.
    pub latitude: f64,
    /// Longitude in decimal degrees, valid within `[-180, 180]`.
    pub longitude: f64,
}

impl GeographicCoordinate {
    /// Builds the mirror record from a validated native coordinate.
    pub(crate) fn from_native_coordinate(
        native_coordinate: &geospatial_types::GeographicCoordinate,
    ) -> Self {
        Self {
            latitude: native_coordinate.latitude,
            longitude: native_coordinate.longitude,
        }
    }

    /// Builds the native coordinate from the mirror record. The mirror never
    /// carries canonical-degree guarantees (foreign implementations may hand
    /// any value across), so this is a plain field copy without validation;
    /// the call paths that need canonical degrees validate on their own
    /// boundary.
    pub(crate) fn to_native_coordinate(self) -> geospatial_types::GeographicCoordinate {
        geospatial_types::GeographicCoordinate {
            latitude: self.latitude,
            longitude: self.longitude,
        }
    }
}

/// Rectangular area between a south-west and a north-east corner; boxes
/// never span the antimeridian (encoded as two adjacent boxes instead).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundingBox {
    /// South-west corner (minimum latitude and longitude).
    pub south_west: GeographicCoordinate,
    /// North-east corner (maximum latitude and longitude).
    pub north_east: GeographicCoordinate,
}

impl BoundingBox {
    /// Builds the mirror record from a native bounding box.
    pub(crate) fn from_native_bounding_box(
        native_bounding_box: &geospatial_types::BoundingBox,
    ) -> Self {
        Self {
            south_west: GeographicCoordinate::from_native_coordinate(
                &native_bounding_box.south_west,
            ),
            north_east: GeographicCoordinate::from_native_coordinate(
                &native_bounding_box.north_east,
            ),
        }
    }

    /// Builds the native bounding box from the mirror record. As with the
    /// coordinate converters this is a plain field copy without box
    /// validation; the call paths needing canonical boxes validate on their
    /// own boundary.
    pub(crate) fn to_native_bounding_box(self) -> geospatial_types::BoundingBox {
        geospatial_types::BoundingBox {
            south_west: self.south_west.to_native_coordinate(),
            north_east: self.north_east.to_native_coordinate(),
        }
    }
}

/// Viewport dimensions in physical pixels.
#[derive(Debug, Clone, Copy)]
pub struct ViewportSize {
    /// Horizontal extent in pixels.
    pub width_pixels: f64,
    /// Vertical extent in pixels.
    pub height_pixels: f64,
}

/// Screen-space insets kept clear of map content: distances from each
/// viewport edge in pixels, so UI overlays — route cards, attribution, sheet
/// panels — never cover the content the camera guarantees visible.
#[derive(Debug, Clone, Copy)]
pub struct EdgeInsets {
    /// Pixels kept clear from the left viewport edge.
    pub left: f64,
    /// Pixels kept clear from the top viewport edge.
    pub top: f64,
    /// Pixels kept clear from the right viewport edge.
    pub right: f64,
    /// Pixels kept clear from the bottom viewport edge.
    pub bottom: f64,
}

impl EdgeInsets {
    /// Builds the native inset set from the mirror record, for the native
    /// `fit_bounds` constructor.
    pub(crate) fn to_native_edge_insets(self) -> rustic_maps_rendering::EdgeInsetsPixels {
        rustic_maps_rendering::EdgeInsetsPixels {
            left: self.left,
            top: self.top,
            right: self.right,
            bottom: self.bottom,
        }
    }
}

/// Position on the viewport in physical pixels (origin top-left, y down).
#[derive(Debug, Clone, Copy)]
pub struct ScreenPoint {
    /// Horizontal position in pixels.
    pub x: f64,
    /// Vertical position in pixels.
    pub y: f64,
}

impl ScreenPoint {
    /// Builds a screen point from the native projection's two-component
    /// result in viewport pixels.
    pub(crate) fn from_screen_pixels(screen_pixels: [f64; 2]) -> Self {
        Self {
            x: screen_pixels[0],
            y: screen_pixels[1],
        }
    }
}

/// A world position in normalized world coordinates, both axes in `[0, 1]`
/// (x eastward from the antimeridian, y southward from the north pole).
#[derive(Debug, Clone, Copy)]
pub struct NormalizedWorldCoordinate {
    /// Eastward position from the antimeridian.
    pub x: f64,
    /// Southward position from the north pole.
    pub y: f64,
}

impl NormalizedWorldCoordinate {
    /// Builds a normalized world coordinate from the native projection's
    /// two-component result.
    pub(crate) fn from_normalized_world_components(normalized_world_components: [f64; 2]) -> Self {
        Self {
            x: normalized_world_components[0],
            y: normalized_world_components[1],
        }
    }
}

/// One web-mercator XYZ tile address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileKey {
    /// Tile column, in `0..2^zoom_level`.
    pub tile_x: u32,
    /// Tile row, counted from the north, in `0..2^zoom_level`.
    pub tile_y: u32,
    /// Zoom level this tile belongs to.
    pub zoom_level: u8,
}

impl TileKey {
    /// Builds a tile key record from the native `TileKey`.
    pub(crate) fn from_native_tile_key(tile_key: RenderingTileKey) -> Self {
        Self {
            tile_x: tile_key.tile_x,
            tile_y: tile_key.tile_y,
            zoom_level: tile_key.zoom_level,
        }
    }
}

// ---------------------------------------------------------------------------
// Async provider surface records and enums (M4 slice 3): field-for-field
// mirrors of the `geospatial-types` provider records, converted through the
// explicit constructors below because FFI types cannot alias foreign-crate
// types.
// ---------------------------------------------------------------------------

/// Structural precision a geocoder reached for one resolved location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeocodeQuality {
    /// Rooftop or feature point.
    Point,
    /// Street address matched to the building level.
    StreetAddress,
    /// Street intersection.
    Intersection,
    /// Street level without house number.
    Street,
    /// Postal code area.
    PostalCode,
    /// Settlement level (village, town, city).
    City,
    /// County or administrative district.
    County,
    /// First-level administrative division.
    State,
    /// Sovereign country level.
    Country,
}

impl GeocodeQuality {
    /// Builds the mirror enum from the native geocode quality.
    pub(crate) fn from_native_geocode_quality(
        native_geocode_quality: geospatial_types::GeocodeQuality,
    ) -> Self {
        match native_geocode_quality {
            geospatial_types::GeocodeQuality::Point => Self::Point,
            geospatial_types::GeocodeQuality::StreetAddress => Self::StreetAddress,
            geospatial_types::GeocodeQuality::Intersection => Self::Intersection,
            geospatial_types::GeocodeQuality::Street => Self::Street,
            geospatial_types::GeocodeQuality::PostalCode => Self::PostalCode,
            geospatial_types::GeocodeQuality::City => Self::City,
            geospatial_types::GeocodeQuality::County => Self::County,
            geospatial_types::GeocodeQuality::State => Self::State,
            geospatial_types::GeocodeQuality::Country => Self::Country,
        }
    }

    /// Builds the native geocode quality from the mirror enum.
    pub(crate) fn to_native_geocode_quality(self) -> geospatial_types::GeocodeQuality {
        match self {
            Self::Point => geospatial_types::GeocodeQuality::Point,
            Self::StreetAddress => geospatial_types::GeocodeQuality::StreetAddress,
            Self::Intersection => geospatial_types::GeocodeQuality::Intersection,
            Self::Street => geospatial_types::GeocodeQuality::Street,
            Self::PostalCode => geospatial_types::GeocodeQuality::PostalCode,
            Self::City => geospatial_types::GeocodeQuality::City,
            Self::County => geospatial_types::GeocodeQuality::County,
            Self::State => geospatial_types::GeocodeQuality::State,
            Self::Country => geospatial_types::GeocodeQuality::Country,
        }
    }
}

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

/// Encoded image formats a raster tile provider may deliver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RasterImageFormat {
    /// PNG imagery.
    Png,
    /// JPEG imagery.
    Jpeg,
}

impl RasterImageFormat {
    /// Builds the mirror enum from the native image format.
    pub(crate) fn from_native_image_format(
        native_image_format: geospatial_types::RasterImageFormat,
    ) -> Self {
        match native_image_format {
            geospatial_types::RasterImageFormat::Png => Self::Png,
            geospatial_types::RasterImageFormat::Jpeg => Self::Jpeg,
        }
    }

    /// Builds the native image format from the mirror enum.
    pub(crate) fn to_native_image_format(self) -> geospatial_types::RasterImageFormat {
        match self {
            Self::Png => geospatial_types::RasterImageFormat::Png,
            Self::Jpeg => geospatial_types::RasterImageFormat::Jpeg,
        }
    }
}

/// Pixel resolution a raster tile is rendered at; `Standard` is the plain
/// tile form, `Retina` the `@2x` form with twice the texels per edge for the
/// same logical tile span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RasterTileResolution {
    /// One texel per logical tile pixel.
    Standard,
    /// Twice the texels per logical tile pixel (the `@2x` request form).
    Retina,
}

impl RasterTileResolution {
    /// Builds the mirror enum from the native tile resolution.
    pub(crate) fn from_native_tile_resolution(
        native_tile_resolution: geospatial_types::RasterTileResolution,
    ) -> Self {
        match native_tile_resolution {
            geospatial_types::RasterTileResolution::Standard => Self::Standard,
            geospatial_types::RasterTileResolution::Retina => Self::Retina,
        }
    }

    /// Builds the native tile resolution from the mirror enum.
    pub(crate) fn to_native_tile_resolution(self) -> geospatial_types::RasterTileResolution {
        match self {
            Self::Standard => geospatial_types::RasterTileResolution::Standard,
            Self::Retina => geospatial_types::RasterTileResolution::Retina,
        }
    }
}

/// One capability whose provider an application chooses independently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProviderContract {
    /// Forward and reverse address lookup.
    Geocoding,
    /// Route calculation through ordered waypoints.
    Routing,
    /// Travel-time and distance matrix calculation.
    DistanceMatrix,
    /// Place prediction and lookup by free text.
    PlaceSearch,
    /// Traffic incident reports.
    TrafficIncidents,
    /// Traffic flow along road segments.
    TrafficFlow,
    /// Terrain elevation lookup.
    Elevation,
    /// Raster map imagery.
    RasterTiles,
    /// Vector map tiles.
    VectorTiles,
    /// Route calculation on a compiled offline graph.
    OfflineRouting,
}

impl ProviderContract {
    /// The capability's name in the shared registry's `served_contracts` list.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn registry_name(self) -> &'static str {
        match self {
            Self::Geocoding => "geocoding",
            Self::Routing => "routing",
            Self::DistanceMatrix => "distance_matrix",
            Self::PlaceSearch => "place_search",
            Self::TrafficIncidents => "traffic_incidents",
            Self::TrafficFlow => "traffic_flow",
            Self::Elevation => "elevation",
            Self::RasterTiles => "raster_tiles",
            Self::VectorTiles => "vector_tiles",
            Self::OfflineRouting => "offline_routing",
        }
    }

    /// Every capability, in the order a settings surface lists them.
    pub const ALL: [Self; 10] = [
        Self::Geocoding,
        Self::Routing,
        Self::DistanceMatrix,
        Self::PlaceSearch,
        Self::TrafficIncidents,
        Self::TrafficFlow,
        Self::Elevation,
        Self::RasterTiles,
        Self::VectorTiles,
        Self::OfflineRouting,
    ];

    /// Builds the mirror enum from the core registry's contract.
    pub(crate) fn from_core_contract(core_contract: rustic_maps_core::ProviderContract) -> Self {
        match core_contract {
            rustic_maps_core::ProviderContract::Geocoding => Self::Geocoding,
            rustic_maps_core::ProviderContract::Routing => Self::Routing,
            rustic_maps_core::ProviderContract::DistanceMatrix => Self::DistanceMatrix,
            rustic_maps_core::ProviderContract::PlaceSearch => Self::PlaceSearch,
            rustic_maps_core::ProviderContract::TrafficIncidents => Self::TrafficIncidents,
            rustic_maps_core::ProviderContract::TrafficFlow => Self::TrafficFlow,
            rustic_maps_core::ProviderContract::Elevation => Self::Elevation,
            rustic_maps_core::ProviderContract::RasterTiles => Self::RasterTiles,
            rustic_maps_core::ProviderContract::VectorTiles => Self::VectorTiles,
            rustic_maps_core::ProviderContract::OfflineRouting => Self::OfflineRouting,
        }
    }

    /// Builds the core registry's contract from the mirror enum.
    pub(crate) fn to_core_contract(self) -> rustic_maps_core::ProviderContract {
        match self {
            Self::Geocoding => rustic_maps_core::ProviderContract::Geocoding,
            Self::Routing => rustic_maps_core::ProviderContract::Routing,
            Self::DistanceMatrix => rustic_maps_core::ProviderContract::DistanceMatrix,
            Self::PlaceSearch => rustic_maps_core::ProviderContract::PlaceSearch,
            Self::TrafficIncidents => rustic_maps_core::ProviderContract::TrafficIncidents,
            Self::TrafficFlow => rustic_maps_core::ProviderContract::TrafficFlow,
            Self::Elevation => rustic_maps_core::ProviderContract::Elevation,
            Self::RasterTiles => rustic_maps_core::ProviderContract::RasterTiles,
            Self::VectorTiles => rustic_maps_core::ProviderContract::VectorTiles,
            Self::OfflineRouting => rustic_maps_core::ProviderContract::OfflineRouting,
        }
    }
}

/// Which provider serves one capability.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderRegistration {
    /// The capability this entry describes.
    pub contract: ProviderContract,
    /// The provider's stable registry key, or an empty string when no
    /// provider is installed.
    pub provider_key: String,
    /// The provider's display name, or an empty string when no provider is
    /// installed.
    pub display_name: String,
    /// Whether a provider is installed for the capability.
    pub registered: bool,
}

/// One resolved location returned by forward or reverse geocoding.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    /// Display string the provider produced for this place.
    pub structured_address_text: String,
    /// Resolved position.
    pub coordinate: GeographicCoordinate,
    /// Structural precision reached for this resolution.
    pub geocode_quality: GeocodeQuality,
}

impl Place {
    /// Builds the mirror record from a native place result.
    pub(crate) fn from_native_place_result(
        native_place_result: &geospatial_types::PlaceResult,
    ) -> Self {
        Self {
            structured_address_text: native_place_result.structured_address_text.clone(),
            coordinate: GeographicCoordinate::from_native_coordinate(
                &native_place_result.coordinate,
            ),
            geocode_quality: GeocodeQuality::from_native_geocode_quality(
                native_place_result.geocode_quality,
            ),
        }
    }

    /// Builds the native place result from the mirror record, consuming it
    /// (the fields move without a copy).
    pub(crate) fn into_native_place_result(self) -> geospatial_types::PlaceResult {
        geospatial_types::PlaceResult {
            structured_address_text: self.structured_address_text,
            coordinate: self.coordinate.to_native_coordinate(),
            geocode_quality: self.geocode_quality.to_native_geocode_quality(),
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

/// Pace chosen by the application for a simulated trip. The SDK carries no
/// default pace; validation happens in `rustic-maps-navigation`'s route walker.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SimulatedTripPace {
    /// Baseline travel speed in meters per second.
    pub speed_meters_per_second: f64,
    /// Multiplier applied to the baseline speed.
    pub speed_multiplier: f64,
}

impl SimulatedTripPace {
    /// Builds the native pace from the mirror record. Values cross unvalidated
    /// (foreign code may hand anything across); the walker refuses a pace that
    /// cannot drive a trip.
    pub(crate) fn to_native_pace(self) -> rustic_maps_navigation::SimulatedTripPace {
        rustic_maps_navigation::SimulatedTripPace {
            speed_meters_per_second: self.speed_meters_per_second,
            speed_multiplier: self.speed_multiplier,
        }
    }
}

/// One position of a simulated trip along the planned route.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SimulatedTripFix {
    /// Position on the planned route's geometry.
    pub coordinate: GeographicCoordinate,
    /// Direction of travel along the active segment, in degrees clockwise
    /// from true north, or `None` when the route has no segment.
    pub course_heading_degrees: Option<f64>,
    /// Distance travelled from the route start, in meters.
    pub travelled_meters: f64,
    /// Distance left to the route end, in meters.
    pub remaining_meters: f64,
    /// Simulated elapsed time from the trip start, in seconds.
    pub elapsed_seconds: f64,
    /// Whether the fix sits at the route's final coordinate.
    pub is_arrived: bool,
}

impl SimulatedTripFix {
    /// Builds the mirror record from a native simulated-trip fix.
    pub(crate) fn from_native_fix(native_fix: &rustic_maps_navigation::SimulatedTripFix) -> Self {
        Self {
            coordinate: GeographicCoordinate::from_native_coordinate(&native_fix.coordinate),
            course_heading_degrees: native_fix.course_heading_degrees,
            travelled_meters: native_fix.travelled_meters,
            remaining_meters: native_fix.remaining_meters,
            elapsed_seconds: native_fix.elapsed_seconds,
            is_arrived: native_fix.is_arrived,
        }
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

/// One terrain elevation sample at a coordinate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ElevationPoint {
    /// Position the sample belongs to.
    pub coordinate: GeographicCoordinate,
    /// Elevation above mean sea level in meters.
    pub elevation_meters: f64,
}

impl ElevationPoint {
    /// Builds the mirror record from a native elevation sample.
    pub(crate) fn from_native_elevation_point(
        native_elevation_point: &geospatial_types::ElevationPoint,
    ) -> Self {
        Self {
            coordinate: GeographicCoordinate::from_native_coordinate(
                &native_elevation_point.coordinate,
            ),
            elevation_meters: native_elevation_point.elevation_meters,
        }
    }

    /// Builds the native elevation sample from the mirror record — the
    /// foreign provider adapters' direction. The coordinate validates like
    /// every other geometry a provider returns; the elevation copies
    /// verbatim, because a provider may legitimately report a value below
    /// mean sea level.
    pub(crate) fn into_native_elevation_point(
        self,
    ) -> Result<geospatial_types::ElevationPoint, SdkError> {
        Ok(geospatial_types::ElevationPoint {
            coordinate: crate::projection::validated_coordinate(self.coordinate)?,
            elevation_meters: self.elevation_meters,
        })
    }
}

/// One fetched web-mercator XYZ raster tile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RasterTileData {
    /// Tile column coordinate at the given zoom level.
    pub tile_x: u32,
    /// Tile row coordinate at the given zoom level.
    pub tile_y: u32,
    /// Zoom level this tile belongs to.
    pub zoom_level: u8,
    /// Encoding of the byte payload.
    pub image_format: RasterImageFormat,
    /// Pixel resolution the payload was rendered at.
    pub tile_resolution: RasterTileResolution,
    /// Encoded image bytes.
    pub image_bytes: Vec<u8>,
}

impl RasterTileData {
    /// Builds the mirror record from a native raster tile.
    pub(crate) fn from_native_raster_tile_data(
        native_raster_tile_data: &geospatial_types::RasterTileData,
    ) -> Self {
        Self {
            tile_x: native_raster_tile_data.tile_x,
            tile_y: native_raster_tile_data.tile_y,
            zoom_level: native_raster_tile_data.zoom_level,
            image_format: RasterImageFormat::from_native_image_format(
                native_raster_tile_data.image_format,
            ),
            tile_resolution: RasterTileResolution::from_native_tile_resolution(
                native_raster_tile_data.tile_resolution,
            ),
            image_bytes: native_raster_tile_data.image_bytes.clone(),
        }
    }

    /// Builds the native raster tile from the mirror record, consuming it
    /// (the tile payload moves without a copy).
    pub(crate) fn into_native_raster_tile_data(self) -> geospatial_types::RasterTileData {
        geospatial_types::RasterTileData {
            tile_x: self.tile_x,
            tile_y: self.tile_y,
            zoom_level: self.zoom_level,
            image_format: self.image_format.to_native_image_format(),
            tile_resolution: self.tile_resolution.to_native_tile_resolution(),
            image_bytes: self.image_bytes,
        }
    }
}

/// Wire encoding of a vector tile's byte payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VectorTileEncoding {
    /// Plain MVT protobuf bytes.
    MapboxVectorTile,
    /// MVT protobuf bytes wrapped in gzip (the CDN's `Content-Encoding`).
    GzippedMapboxVectorTile,
}

impl VectorTileEncoding {
    /// Builds the native encoding from the mirror variant — the foreign
    /// provider adapters' direction.
    pub(crate) fn to_native_vector_tile_encoding(self) -> geospatial_types::VectorTileEncoding {
        match self {
            VectorTileEncoding::MapboxVectorTile => {
                geospatial_types::VectorTileEncoding::MapboxVectorTile
            }
            VectorTileEncoding::GzippedMapboxVectorTile => {
                geospatial_types::VectorTileEncoding::GzippedMapboxVectorTile
            }
        }
    }
}

/// One HTTP header a vector-tile source's requests carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceHeader {
    /// Header name.
    pub name: String,
    /// Header value.
    pub value: String,
}

/// Description of one vector-tileset source a foreign vector-tile provider
/// serves (see `rustic_maps.udl`'s `VectorTileSourceDescriptor` for the field
/// contract).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorTileSourceDescriptor {
    /// Stable unique name of the tileset, used to namespace its cached vector
    /// payloads so two tilesets serving one address never collide.
    pub source_name: String,
    /// Human-readable attribution line the provider's terms require.
    pub attribution_text: String,
    /// Lowest zoom level the provider serves.
    pub minimum_zoom_level: u8,
    /// Highest zoom level the provider serves.
    pub maximum_zoom_level: u8,
    /// Wire encoding every tile from this source arrives in.
    pub tile_encoding: VectorTileEncoding,
    /// Style names or families the tileset is designed to be rendered with.
    pub style_hints: Vec<String>,
}

impl VectorTileSourceDescriptor {
    /// Builds the native descriptor from the mirror record, consuming it.
    ///
    /// The native trait's descriptor carries the cache namespace, attribution
    /// and zoom span plus the provider's declared style hints. The mirror's
    /// `tile_encoding` has no native field: every fetched [`VectorTileData`]
    /// reports the encoding its own payload arrived in, and the decoder
    /// unwraps by that per-tile value. The hints are the provider's own
    /// declaration of the tileset's schema or style family; the installed
    /// style id alone decides which style document paints the tiles, while a
    /// consumer verifying a document's declared source uses the hints as
    /// additional served-tileset identities.
    pub(crate) fn into_native_vector_tile_source_descriptor(
        self,
    ) -> geospatial_traits::VectorTileSourceDescriptor {
        geospatial_traits::VectorTileSourceDescriptor {
            source_name: self.source_name,
            attribution_text: self.attribution_text,
            minimum_zoom_level: self.minimum_zoom_level,
            maximum_zoom_level: self.maximum_zoom_level,
            style_hints: self.style_hints,
        }
    }
}

/// One fetched web-mercator XYZ vector tile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorTileData {
    /// Tile column coordinate at the given zoom level.
    pub tile_x: u32,
    /// Tile row coordinate at the given zoom level.
    pub tile_y: u32,
    /// Zoom level this tile belongs to.
    pub zoom_level: u8,
    /// Wire encoding of the byte payload.
    pub tile_encoding: VectorTileEncoding,
    /// Encoded tile bytes: MVT protobuf, optionally gzip-wrapped.
    pub tile_bytes: Vec<u8>,
}

impl VectorTileData {
    /// Builds the native vector tile from the mirror record, consuming it (the
    /// payload moves without a copy).
    pub(crate) fn into_native_vector_tile_data(self) -> geospatial_types::VectorTileData {
        geospatial_types::VectorTileData {
            tile_x: self.tile_x,
            tile_y: self.tile_y,
            zoom_level: self.zoom_level,
            tile_encoding: self.tile_encoding.to_native_vector_tile_encoding(),
            tile_bytes: self.tile_bytes,
        }
    }
}

// ---------------------------------------------------------------------------
// Navigation state records and policies (M6): field-for-field mirrors of the
// `rustic-maps-navigation` plain-data types, converted through the explicit
// constructors below. The state side of a trip only flows outbound (native
// state -> mirror record), so those records carry `from_native_*`
// constructors only, while the controller inputs (location fixes, routes)
// flow inbound and carry the reverse converters.
// ---------------------------------------------------------------------------

/// High-level phase of a trip that has already been started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationPhase {
    /// Following the route's step sequence.
    Navigating,
    /// The deviation detector judged the last graded fix off route; progress
    /// is frozen at the last on-route values until a graded fix recovers or a
    /// reroute replaces the route.
    OffRoute,
    /// The final step has been completed; location updates no longer change
    /// the state.
    Arrived,
}

impl NavigationPhase {
    /// Builds the mirror enum from the native navigation phase.
    pub(crate) fn from_native_navigation_phase(
        native_navigation_phase: rustic_maps_navigation::NavigationPhase,
    ) -> Self {
        match native_navigation_phase {
            rustic_maps_navigation::NavigationPhase::Navigating => Self::Navigating,
            rustic_maps_navigation::NavigationPhase::OffRoute => Self::OffRoute,
            rustic_maps_navigation::NavigationPhase::Arrived => Self::Arrived,
        }
    }
}

/// Which grade of deviation a recorded deviation carries.
///
/// The two grades of the two-grade deviation distinction: off the *current
/// step* yet still on the route line versus completely off the route. Only
/// the fully-off grade freezes navigation; the wrong-step grade never freezes
/// on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviationGradeKind {
    /// The fix projects onto neither the current step nor any future step of
    /// the remaining route line: the freezing grade. The trip's progress is
    /// frozen until a graded fix recovers or a reroute replaces the route.
    FullyOffRoute,
    /// The fix is off the current step but projects onto a future step of the
    /// remaining route line: "on route, wrong step". The trip keeps
    /// navigating and counting down, and is never frozen by this grade alone.
    OffStepOnRoute,
}

impl DeviationGradeKind {
    /// Builds the mirror enum from the native deviation grade.
    pub(crate) fn from_native_deviation_grade(
        native_deviation_grade: rustic_maps_navigation::DeviationGrade,
    ) -> Self {
        match native_deviation_grade {
            rustic_maps_navigation::DeviationGrade::FullyOffRoute => Self::FullyOffRoute,
            rustic_maps_navigation::DeviationGrade::OffStepOnRoute => Self::OffStepOnRoute,
        }
    }
}

/// Plain-data description of an active deviation from the route.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeviationInfo {
    /// Which grade of deviation this is (see [`DeviationGradeKind`]).
    pub grade_kind: DeviationGradeKind,
    /// Distance, in meters, from the raw fix that triggered (or most recently
    /// confirmed) the deviation to the remaining route line, as measured and
    /// judged by the deviation detector.
    pub deviation_from_route_meters: f64,
}

impl DeviationInfo {
    /// Builds the mirror record from the native deviation description.
    pub(crate) fn from_native_deviation_info(
        native_deviation_info: &rustic_maps_navigation::DeviationInfo,
    ) -> Self {
        Self {
            grade_kind: DeviationGradeKind::from_native_deviation_grade(
                native_deviation_info.grade,
            ),
            deviation_from_route_meters: native_deviation_info.deviation_from_route_meters,
        }
    }
}

/// Location of one route step in traversal order across a route's legs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteStepPosition {
    /// Index of the leg that owns this step in the route's legs.
    pub leg_index: u64,
    /// Index of the step within its leg in the leg's steps.
    pub step_index: u64,
}

impl RouteStepPosition {
    /// Builds the mirror record from the native step position.
    pub(crate) fn from_native_route_step_position(
        native_route_step_position: &rustic_maps_navigation::RouteStepPosition,
    ) -> Self {
        Self {
            leg_index: native_route_step_position.leg_index as u64,
            step_index: native_route_step_position.step_index as u64,
        }
    }
}

/// Progress along the remaining trip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TripProgress {
    /// Distance, in meters, from the snapped position to the end of the
    /// current step — the point of the next maneuver.
    pub distance_to_next_maneuver_meters: f64,
    /// Total remaining distance, in meters, including the current step's
    /// remainder.
    pub distance_remaining_meters: f64,
    /// Total remaining travel time, in seconds, measured the same way as
    /// `distance_remaining_meters`.
    pub duration_remaining_seconds: f64,
}

impl TripProgress {
    /// Builds the mirror record from the native progress record.
    pub(crate) fn from_native_trip_progress(
        native_trip_progress: &rustic_maps_navigation::TripProgress,
    ) -> Self {
        Self {
            distance_to_next_maneuver_meters: native_trip_progress.distance_to_next_maneuver_meters,
            distance_remaining_meters: native_trip_progress.distance_remaining_meters,
            duration_remaining_seconds: native_trip_progress.duration_remaining_seconds,
        }
    }
}

/// The full navigable state of an active trip, read-only for foreign code.
///
/// The steps of [`Self::route`] are carried as traversal-ordered indices
/// (`step_positions`), never duplicated; the route's steps themselves arrive
/// through the [`super::Route`] record, which carries no lane guidance. The
/// live step-advance condition and deviation detector are not records and
/// cannot cross the FFI; they are configured through the navigation policies
/// on the controller, matching the native session's pairing.
#[derive(Debug, Clone, PartialEq)]
pub struct TripNavigationState {
    /// Current phase of the trip.
    pub phase: NavigationPhase,
    /// The route being navigated.
    pub route: Route,
    /// All steps across every leg, flattened in traversal order.
    pub step_positions: Vec<RouteStepPosition>,
    /// Index into `step_positions` of the step currently traversed.
    pub current_step_position: u64,
    /// Progress along the remaining trip.
    pub progress: TripProgress,
    /// Position snapped onto the current step's shape; `None` until the first
    /// location update has been processed.
    pub snapped_coordinate: Option<GeographicCoordinate>,
    /// Active deviation, as judged by the deviation detector. Set exactly
    /// while the phase is `OffRoute`.
    pub deviation: Option<DeviationInfo>,
    /// Whether a reroute recalculation is currently in flight (the reroute
    /// gate's observable readback). This is a controller wall-clock
    /// fact, not reducer state, so the pure conversion below cannot know it;
    /// it always builds `false` here and the only caller — the exported
    /// controller's trip-state readback — overlays its gate's live value.
    pub is_calculating_new_route: bool,
}

impl TripNavigationState {
    /// Builds the mirror record from the native trip state.
    pub(crate) fn from_native_trip_navigation_state(
        native_trip_navigation_state: &rustic_maps_navigation::TripNavigationState,
    ) -> Self {
        Self {
            phase: NavigationPhase::from_native_navigation_phase(
                native_trip_navigation_state.phase,
            ),
            route: Route::from_native_route_data(&native_trip_navigation_state.route),
            step_positions: native_trip_navigation_state
                .step_positions
                .iter()
                .map(RouteStepPosition::from_native_route_step_position)
                .collect(),
            current_step_position: native_trip_navigation_state.current_step_position as u64,
            progress: TripProgress::from_native_trip_progress(
                &native_trip_navigation_state.progress,
            ),
            snapped_coordinate: native_trip_navigation_state
                .snapped_coordinate
                .as_ref()
                .map(GeographicCoordinate::from_native_coordinate),
            deviation: native_trip_navigation_state
                .deviation
                .as_ref()
                .map(DeviationInfo::from_native_deviation_info),
            // A pure conversion cannot know the controller's wall-clock gate
            // state; the controller's trip-state readback overlays the live
            // value.
            is_calculating_new_route: false,
        }
    }
}

/// Length unit a distance-phrase magnitude is expressed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistanceUnit {
    /// Meters, used by the metric near and moderate bands.
    Meters,
    /// Kilometers, used by the metric far band.
    Kilometers,
    /// Feet, used by the imperial near and moderate bands.
    Feet,
    /// Miles, used by the imperial far band.
    Miles,
}

impl DistanceUnit {
    /// Builds the mirror enum from the native distance unit.
    pub(crate) fn from_native_distance_unit(
        native_distance_unit: rustic_maps_navigation::DistanceUnit,
    ) -> Self {
        match native_distance_unit {
            rustic_maps_navigation::DistanceUnit::Meters => Self::Meters,
            rustic_maps_navigation::DistanceUnit::Kilometers => Self::Kilometers,
            rustic_maps_navigation::DistanceUnit::Feet => Self::Feet,
            rustic_maps_navigation::DistanceUnit::Miles => Self::Miles,
        }
    }
}

/// Which form a distance phrase takes. UDL enums carry no payloads, so the
/// phrase mirrors its three forms under this kind plus the nullable fields of
/// the [`DistancePhrase`] record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistancePhraseKind {
    /// The position is closer than half a near-band increment to the maneuver
    /// point; no distance is phrased and composed text drops the prefix. The
    /// record's magnitude is `None` on this kind.
    Immediate,
    /// A rounded distance magnitude in a specific unit; the record's
    /// magnitude and unit are set on this kind.
    Quantity,
    /// No distance is phrased because the freezing (fully-off-route) deviation
    /// grade holds: the projection is frozen, so any countdown composed off it
    /// would be a phantom measurement. The record's magnitude is `None` on
    /// this kind, and the composed text channels carry no distance prefix.
    Suppressed,
}

/// The unit-system-aware phrasing of the distance remaining to a maneuver,
/// rounded into the native distance phrasing bands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DistancePhrase {
    /// Which form the phrase takes (see [`DistancePhraseKind`]).
    pub phrase_kind: DistancePhraseKind,
    /// Rounded magnitude of the phrase (half-up, per the native bands);
    /// `None` for the immediate and suppressed forms.
    pub magnitude: Option<f64>,
    /// Unit the magnitude is expressed in; `None` for the immediate and
    /// suppressed forms.
    pub magnitude_unit: Option<DistanceUnit>,
}

impl DistancePhrase {
    /// Builds the mirror record from the native distance phrase.
    pub(crate) fn from_native_distance_phrase(
        native_distance_phrase: &rustic_maps_navigation::DistancePhrase,
    ) -> Self {
        match *native_distance_phrase {
            rustic_maps_navigation::DistancePhrase::Immediate => Self {
                phrase_kind: DistancePhraseKind::Immediate,
                magnitude: None,
                magnitude_unit: None,
            },
            rustic_maps_navigation::DistancePhrase::Quantity { magnitude, unit } => Self {
                phrase_kind: DistancePhraseKind::Quantity,
                magnitude: Some(magnitude),
                magnitude_unit: Some(DistanceUnit::from_native_distance_unit(unit)),
            },
            rustic_maps_navigation::DistancePhrase::Suppressed => Self {
                phrase_kind: DistancePhraseKind::Suppressed,
                magnitude: None,
                magnitude_unit: None,
            },
        }
    }
}

/// Which form an instruction text takes. UDL enums carry no payloads, so the
/// text mirrors its two forms under this kind plus the nullable fields of the
/// [`ManeuverInstructionText`] record; all four text fields are `None` on the
/// unavailable kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManeuverInstructionTextKind {
    /// The provider authored instruction text for this maneuver.
    Composed,
    /// The provider authored no instruction text for this maneuver; nothing
    /// is invented to fill it.
    Unavailable,
}

/// The composed text channels of one maneuver: provider-authored text is
/// carried verbatim beside the composed strings (the composed display variant
/// is the live distance prefix plus the provider display text, the composed
/// speech variant the prefix plus the provider speech text, and an empty
/// provider variant falls back to the other, still provider-authored one).
#[derive(Debug, Clone, PartialEq)]
pub struct ManeuverInstructionText {
    /// Which form the text takes (see [`ManeuverInstructionTextKind`]).
    pub text_kind: ManeuverInstructionTextKind,
    /// Provider-authored display text, verbatim and never rephrased.
    pub provider_display_text: Option<String>,
    /// Provider-authored speech text, verbatim and never rephrased.
    pub provider_speech_text: Option<String>,
    /// Banner variant: the live distance prefix plus the display variant.
    pub composed_display_text: Option<String>,
    /// Text-to-speech variant: the live distance prefix plus the speech
    /// variant.
    pub composed_speech_text: Option<String>,
}

impl ManeuverInstructionText {
    /// Builds the mirror record from the native text enum.
    pub(crate) fn from_native_maneuver_instruction_text(
        native_maneuver_instruction_text: &rustic_maps_navigation::ManeuverInstructionText,
    ) -> Self {
        match native_maneuver_instruction_text {
            rustic_maps_navigation::ManeuverInstructionText::Composed {
                provider_display_text,
                provider_speech_text,
                display_text,
                speech_text,
            } => Self {
                text_kind: ManeuverInstructionTextKind::Composed,
                provider_display_text: Some(provider_display_text.clone()),
                provider_speech_text: Some(provider_speech_text.clone()),
                composed_display_text: Some(display_text.clone()),
                composed_speech_text: Some(speech_text.clone()),
            },
            rustic_maps_navigation::ManeuverInstructionText::Unavailable => Self {
                text_kind: ManeuverInstructionTextKind::Unavailable,
                provider_display_text: None,
                provider_speech_text: None,
                composed_display_text: None,
                composed_speech_text: None,
            },
        }
    }
}

/// Instruction payload of the maneuver currently being traversed (the banner
/// maneuver: the step the snapped position sits inside). Mirrors the native
/// active maneuver except for its lane-guidance list, which stays native
/// until the route records carry lane guidance (see [`RouteStep`]).
#[derive(Debug, Clone, PartialEq)]
pub struct ActiveManeuverInstruction {
    /// Composed display and speech text of the active step.
    pub instruction: ManeuverInstructionText,
    /// Unit-system-aware distance phrasing of the distance to the next
    /// maneuver.
    pub distance_phrase: DistancePhrase,
    /// Structured exit index of a roundabout active step, starting at one
    /// (`None` for every other maneuver).
    pub roundabout_exit_index: Option<u8>,
}

impl ActiveManeuverInstruction {
    /// Builds the mirror record from the native active maneuver instruction.
    pub(crate) fn from_native_active_maneuver_instruction(
        native_active_maneuver_instruction: &rustic_maps_navigation::ActiveManeuverInstruction,
    ) -> Self {
        Self {
            instruction: ManeuverInstructionText::from_native_maneuver_instruction_text(
                &native_active_maneuver_instruction.instruction,
            ),
            distance_phrase: DistancePhrase::from_native_distance_phrase(
                &native_active_maneuver_instruction.distance_phrase,
            ),
            roundabout_exit_index: native_active_maneuver_instruction.roundabout_exit_index,
        }
    }
}

/// Instruction payload of the maneuver after the active one (the "then"
/// instruction), text-carrying only.
#[derive(Debug, Clone, PartialEq)]
pub struct UpcomingManeuverInstruction {
    /// Composed display and speech text of the following step.
    pub instruction: ManeuverInstructionText,
    /// Structured exit index when the following step is a roundabout.
    pub roundabout_exit_index: Option<u8>,
}

impl UpcomingManeuverInstruction {
    /// Builds the mirror record from the native upcoming maneuver instruction.
    pub(crate) fn from_native_upcoming_maneuver_instruction(
        native_upcoming_maneuver_instruction: &rustic_maps_navigation::UpcomingManeuverInstruction,
    ) -> Self {
        Self {
            instruction: ManeuverInstructionText::from_native_maneuver_instruction_text(
                &native_upcoming_maneuver_instruction.instruction,
            ),
            roundabout_exit_index: native_upcoming_maneuver_instruction.roundabout_exit_index,
        }
    }
}

/// The emitted turn-by-turn instruction payload for one trip state, mirroring
/// the native instruction record one-to-one (except the native active
/// maneuver's lane-guidance list, which stays native until the route records
/// carry lane guidance). Emission is pure in the native core, so the
/// same state yields the same payload on every read.
#[derive(Debug, Clone, PartialEq)]
pub struct NavigationInstruction {
    /// The banner maneuver currently being traversed.
    pub active_maneuver: ActiveManeuverInstruction,
    /// The maneuver after the active one, where the route has a following
    /// step (always `None` on the route's final step and after arrival).
    pub upcoming_maneuver: Option<UpcomingManeuverInstruction>,
    /// `true` exactly while the trip has arrived and the remaining progress
    /// is zero.
    pub is_arrival: bool,
    /// `true` exactly while the trip is off route: the payload beneath the
    /// marker is the frozen last-on-route projection and must not be
    /// presented as current guidance.
    pub is_rerouting_required: bool,
}

impl NavigationInstruction {
    /// Builds the mirror record from the native instruction payload.
    pub(crate) fn from_native_navigation_instruction(
        native_navigation_instruction: &rustic_maps_navigation::NavigationInstruction,
    ) -> Self {
        Self {
            active_maneuver: ActiveManeuverInstruction::from_native_active_maneuver_instruction(
                &native_navigation_instruction.active_maneuver,
            ),
            upcoming_maneuver: native_navigation_instruction
                .upcoming_maneuver
                .as_ref()
                .map(UpcomingManeuverInstruction::from_native_upcoming_maneuver_instruction),
            is_arrival: native_navigation_instruction.is_arrival,
            is_rerouting_required: native_navigation_instruction.is_rerouting_required,
        }
    }
}

/// Which form a deviation-status update takes. UDL enums carry no payloads,
/// so the update mirrors its two forms under this kind plus the nullable
/// field of the [`DeviationStatusChange`] record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviationStatusChangeKind {
    /// The trip is judged off route; the record's deviation description is
    /// set on this kind.
    OffRoute,
    /// A previously recorded deviation cleared; the trip is judged on route.
    OnRoute,
}

/// A deviation-topic update: the detector's judgment changed between the
/// previous and the current trip state — entry, measurement drift while the
/// deviation persists, and clearing.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviationStatusChange {
    /// Which form the update takes (see [`DeviationStatusChangeKind`]).
    pub status_change_kind: DeviationStatusChangeKind,
    /// The recorded measurement of the deviation, set exactly on the
    /// off-route kind.
    pub deviation_info: Option<DeviationInfo>,
}

impl DeviationStatusChange {
    /// Builds the mirror record from the native deviation-status update.
    pub(crate) fn from_native_deviation_status_change(
        native_deviation_status_change: &rustic_maps_navigation::DeviationStatusChange,
    ) -> Self {
        match native_deviation_status_change {
            rustic_maps_navigation::DeviationStatusChange::OffRoute(deviation_info) => Self {
                status_change_kind: DeviationStatusChangeKind::OffRoute,
                deviation_info: Some(DeviationInfo::from_native_deviation_info(deviation_info)),
            },
            rustic_maps_navigation::DeviationStatusChange::OnRoute => Self {
                status_change_kind: DeviationStatusChangeKind::OnRoute,
                deviation_info: None,
            },
        }
    }
}

/// A phase-transition event delivered on the phase topic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationPhaseChange {
    /// The trip entered the off-route phase from the navigating phase.
    OffRouteEntered,
    /// A graded fix back on the route line lifted the deviation and returned
    /// the trip to the navigating phase.
    OffRouteCleared,
    /// A reroute event replaced the route (delivered only when the event
    /// actually applied — an ignored reroute produces no transition).
    RerouteApplied,
    /// The final step completed; the trip reached the arrived phase.
    Arrived,
}

impl NavigationPhaseChange {
    /// Builds the mirror enum from the native phase transition.
    pub(crate) fn from_native_navigation_phase_change(
        native_navigation_phase_change: &rustic_maps_navigation::NavigationPhaseChange,
    ) -> Self {
        match *native_navigation_phase_change {
            rustic_maps_navigation::NavigationPhaseChange::OffRouteEntered => Self::OffRouteEntered,
            rustic_maps_navigation::NavigationPhaseChange::OffRouteCleared => Self::OffRouteCleared,
            rustic_maps_navigation::NavigationPhaseChange::RerouteApplied => Self::RerouteApplied,
            rustic_maps_navigation::NavigationPhaseChange::Arrived => Self::Arrived,
        }
    }
}

/// One raw position fix delivered to the navigation controller; mirrors the
/// native fix record field-for-field.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NavigationLocationUpdate {
    /// Raw position on the WGS 84 ellipsoid, before route snapping.
    pub coordinate: GeographicCoordinate,
    /// Radius, in meters, of the 68% horizontal-accuracy envelope of this fix
    /// (`None` when the source does not report accuracy). Fixes without an
    /// accuracy report never trigger a step advance and are never judged by
    /// the deviation detector.
    pub horizontal_accuracy_meters: Option<f64>,
    /// Direction of travel in degrees clockwise from true north (`None` when
    /// the source does not report a course).
    pub course_heading_degrees: Option<f64>,
    /// Fix instant in seconds since the Unix epoch (`None` for untimed
    /// sources).
    pub timestamp_seconds: Option<f64>,
}

impl NavigationLocationUpdate {
    /// Builds the native fix record from the mirror record.
    pub(crate) fn into_native_location_update(
        self,
    ) -> rustic_maps_navigation::NavigationLocationUpdate {
        rustic_maps_navigation::NavigationLocationUpdate {
            coordinate: self.coordinate.to_native_coordinate(),
            horizontal_accuracy_meters: self.horizontal_accuracy_meters,
            course_heading_degrees: self.course_heading_degrees,
            timestamp_seconds: self.timestamp_seconds,
        }
    }
}

/// Which step-advance strategy a navigation policy configures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepAdvancePolicyKind {
    /// Steps change only when driven programmatically; this binding surface
    /// exposes no programmatic step advance, so the trip tracks progress
    /// without changing steps.
    Manual,
    /// Advances when the position fix comes within the threshold of the end
    /// of the current step, eager-advance semantics
    /// (`DistanceToEndOfStepCondition`). Thresholds from
    /// [crate::navigation::default_step_advance_policy] are the documented
    /// defaults.
    DistanceToEndOfStep,
    /// Advances when the position fix moves farther from the current step's
    /// polyline than the threshold, delayed-advance semantics
    /// (`DistanceFromStepCondition`).
    DistanceFromStep,
}

/// The step-advance policy of a trip: mirrors the native decision objects
/// that cannot cross the FFI as one tagged record. The `Manual` kind ignores
/// the threshold fields; both distance kinds require finite, non-negative
/// threshold values (refused otherwise at the controller boundary).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StepAdvancePolicy {
    /// Which step-advance strategy the policy configures.
    pub policy_kind: StepAdvancePolicyKind,
    /// Distance, in meters, of the advance threshold (the distance to the
    /// step's end, or from the step's polyline, depending on the kind).
    pub advance_threshold_meters: f64,
    /// Fixes with a reported horizontal accuracy worse than this, in meters,
    /// never advance.
    pub minimum_horizontal_accuracy_meters: f64,
}

/// Which deviation-detection strategy a deviation policy configures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviationPolicyKind {
    /// Flags a deviation when the raw fix is farther from the remaining route
    /// line than the configured threshold, widened by the fix's reported
    /// accuracy (`DistanceDeviationDetector`).
    DistanceThreshold,
}

/// The deviation-detection policy of a trip: mirrors the native decision
/// object that cannot cross the FFI. The threshold must be finite and
/// non-negative (refused otherwise at the controller boundary).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeviationPolicy {
    /// Which deviation-detection strategy the policy configures.
    pub policy_kind: DeviationPolicyKind,
    /// Distance, in meters, from the remaining route line beyond which a fix
    /// is judged off route (before any accuracy widening, strictly greater).
    pub deviation_threshold_meters: f64,
}

/// The reroute-recalculation hysteresis policy of a controller: mirrors the
/// native decision values of
/// `rustic_maps_navigation::ReroutePolicy`. Both thresholds must be finite and
/// non-negative (refused otherwise at the controller boundary).
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

// ---------------------------------------------------------------------------
// Surface-view records (round 8, slice 0): the overlay descriptors of the
// `MapView` `set_overlays` list (mirroring the wasm canvas'
// plain-object overlay names via the kind enum plus nullable fields, the
// round-7 navigation payload pattern) and the platform surface handle of
// `attach`. These mirror the `rustic-maps-rendering` overlay structs one-to-one by
// field, so the same fields the wasm seam passes (`coordinates`,
// `ringCoordinates`, `positions`, `segments`, `color`, `widthPixels`,
// `radiusPixels`) stay name-compatible. Validation of the style ranges and
// the kind/payload coherence happens in the converter, so a descriptor is
// either fully native or rejected at the boundary.
// ---------------------------------------------------------------------------

/// Lowest valid overlay color channel (a channel is also refused when NaN,
/// outside the closed range by construction). Native-only with the surface
/// view's overlay converter.
#[cfg(not(target_arch = "wasm32"))]
const OVERLAY_MINIMUM_COLOR_CHANNEL: f64 = 0.0;
/// Highest valid overlay color channel. Native-only with the surface view's
/// overlay converter.
#[cfg(not(target_arch = "wasm32"))]
const OVERLAY_MAXIMUM_COLOR_CHANNEL: f64 = 1.0;

/// Solid RGBA overlay style, every channel in `[0, 1]`; the rustic-maps-rendering
/// pipelines consume `f32` channels, converted in the constructor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OverlayColor {
    /// Red channel in `[0, 1]`.
    pub red: f64,
    /// Green channel in `[0, 1]`.
    pub green: f64,
    /// Blue channel in `[0, 1]`.
    pub blue: f64,
    /// Alpha channel in `[0, 1]` (the overlay pipeline alpha-blends).
    pub alpha: f64,
}

impl OverlayColor {
    /// Builds the native `f32` RGBA style array from the mirror record,
    /// refusing any channel outside the documented range. Native-only with
    /// the surface view's overlay converter.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn to_native_rgba(self, style_description: &str) -> Result<[f32; 4], SdkError> {
        for (channel_name, channel_value) in [
            ("red", self.red),
            ("green", self.green),
            ("blue", self.blue),
            ("alpha", self.alpha),
        ] {
            if !((OVERLAY_MINIMUM_COLOR_CHANNEL..=OVERLAY_MAXIMUM_COLOR_CHANNEL)
                .contains(&channel_value))
            {
                return Err(SdkError::InvalidRenderInput {
                    detail: format!(
                        "{style_description} color channel {channel_name} \
                         {channel_value} outside [0, 1]"
                    ),
                });
            }
        }
        Ok([
            self.red as f32,
            self.green as f32,
            self.blue as f32,
            self.alpha as f32,
        ])
    }
}

/// Which overlay shape one descriptor configures — the kind tag of the
/// wasm canvas' plain-object overlay names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapOverlayKind {
    /// A routed path drawn as a thick line.
    RoutePolyline,
    /// A filled polygon: one outer ring, ear-clip triangulated.
    FilledPolygon,
    /// Screen-facing point markers (octagon fans).
    PointMarkers,
    /// Congestion-colored traffic-flow lines.
    TrafficFlow,
    /// Screen-facing teardrop pins anchored at their tip.
    PointPins,
}

/// One overlay shape drawn over the map surface's tiles in every frame,
/// selected by `kind`. Each kind reads exactly its own payload fields and
/// ignores the others (the wasm canvas carries the same tolerance); the
/// kind-required payload must be present.
#[derive(Debug, Clone, PartialEq)]
pub struct MapOverlay {
    /// Which overlay shape this descriptor configures and which payload
    /// fields carry it.
    pub kind: MapOverlayKind,
    /// Route polyline: the traversal-order polyline. Required for the kind.
    pub coordinates: Option<Vec<GeographicCoordinate>>,
    /// Route polyline, filled polygon and point markers: solid color
    /// override, or `None` for the kind's documented default.
    pub rgba_color: Option<OverlayColor>,
    /// Route polyline and traffic flow: line width in screen pixels
    /// (positive and finite), or `None` for the documented default.
    pub width_pixels: Option<f64>,
    /// Filled polygon: the outer ring without a repeated closing vertex.
    /// Required for the kind.
    pub ring_coordinates: Option<Vec<GeographicCoordinate>>,
    /// Point markers and point pins: the geographic anchor of every marker
    /// (a pin's anchor is its tip). Required for both kinds.
    pub point_marker_positions: Option<Vec<GeographicCoordinate>>,
    /// Point markers and point pins: marker or head radius in screen pixels
    /// (positive and finite), or `None` for the documented default.
    pub point_marker_radius_pixels: Option<f64>,
    /// Traffic flow: the flow observations to paint. Required for the kind.
    pub traffic_flow_segments: Option<Vec<TrafficFlowData>>,
}

impl MapOverlay {
    /// Validates a screen-pixel size field (a line width or marker radius):
    /// it must be positive and finite, like the wasm canvas' validation of
    /// the same fields. Native-only with the surface view's overlay
    /// converter.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn validated_size_pixels(
        size_pixels: f64,
        size_description: &str,
    ) -> Result<f64, SdkError> {
        if !size_pixels.is_finite() || size_pixels.is_sign_negative() || size_pixels == 0.0 {
            return Err(SdkError::InvalidRenderInput {
                detail: format!("{size_description} must be positive and finite"),
            });
        }
        Ok(size_pixels)
    }

    /// Builds the native overlay from the descriptor, refusing an
    /// incompatible kind/payload pair or an unusable style field. A `None`
    /// style field keeps the documented rustic-maps-rendering default of the kind.
    /// Native-only with the surface view that draws it.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn into_native_overlay(self) -> Result<rustic_maps_rendering::MapOverlay, SdkError> {
        match self.kind {
            MapOverlayKind::RoutePolyline => {
                let mut route_polyline = rustic_maps_rendering::RoutePolylineOverlay::new(
                    validated_coordinates(self.coordinates, "route polyline")?,
                );
                if let Some(rgba_color) = self.rgba_color {
                    route_polyline.rgba_color = rgba_color.to_native_rgba("route polyline")?;
                }
                if let Some(width_pixels) = self.width_pixels {
                    route_polyline.width_pixels =
                        Self::validated_size_pixels(width_pixels, "route polyline width")?;
                }
                Ok(rustic_maps_rendering::MapOverlay::RoutePolyline(
                    route_polyline,
                ))
            }
            MapOverlayKind::FilledPolygon => {
                let mut filled_polygon = rustic_maps_rendering::FilledPolygonOverlay::new(
                    validated_coordinates(self.ring_coordinates, "filled polygon ring")?,
                );
                if let Some(rgba_color) = self.rgba_color {
                    filled_polygon.rgba_color = rgba_color.to_native_rgba("filled polygon")?;
                }
                Ok(rustic_maps_rendering::MapOverlay::FilledPolygon(
                    filled_polygon,
                ))
            }
            MapOverlayKind::PointMarkers => {
                let mut point_markers = rustic_maps_rendering::PointMarkersOverlay::new(
                    validated_coordinates(self.point_marker_positions, "point markers")?,
                );
                if let Some(rgba_color) = self.rgba_color {
                    point_markers.rgba_color = rgba_color.to_native_rgba("point markers")?;
                }
                if let Some(radius_pixels) = self.point_marker_radius_pixels {
                    point_markers.radius_pixels =
                        Self::validated_size_pixels(radius_pixels, "point marker radius")?;
                }
                Ok(rustic_maps_rendering::MapOverlay::PointMarkers(
                    point_markers,
                ))
            }
            MapOverlayKind::PointPins => {
                let mut point_pins = rustic_maps_rendering::PointPinsOverlay::new(
                    validated_coordinates(self.point_marker_positions, "point pins")?,
                );
                if let Some(rgba_color) = self.rgba_color {
                    point_pins.rgba_color = rgba_color.to_native_rgba("point pins")?;
                }
                if let Some(radius_pixels) = self.point_marker_radius_pixels {
                    point_pins.radius_pixels =
                        Self::validated_size_pixels(radius_pixels, "point pin radius")?;
                }
                Ok(rustic_maps_rendering::MapOverlay::PointPins(point_pins))
            }
            MapOverlayKind::TrafficFlow => {
                let mut traffic_flow = rustic_maps_rendering::TrafficFlowOverlay::new(
                    self.traffic_flow_segments
                        .ok_or(SdkError::InvalidRenderInput {
                            detail: "traffic flow descriptor needs traffic_flow_segments"
                                .to_string(),
                        })?
                        .into_iter()
                        .map(|traffic_flow_segment| {
                            traffic_flow_segment.into_native_traffic_flow_data()
                        })
                        .collect::<Result<Vec<_>, SdkError>>()?,
                );
                if let Some(width_pixels) = self.width_pixels {
                    traffic_flow.width_pixels =
                        Self::validated_size_pixels(width_pixels, "traffic flow width")?;
                }
                Ok(rustic_maps_rendering::MapOverlay::TrafficFlow(traffic_flow))
            }
        }
    }
}

/// Converts the kind-required coordinate payload: missing payload is
/// refused, and present payload validates every coordinate the overlay
/// seam validates — the same canonical-degree boundary the projection
/// surface applies (`SdkError::InvalidCoordinateInput` for an out-of-range
/// latitude or longitude, matching the wasm canvas' overlay validation).
/// Native-only with the surface view's overlay converter.
#[cfg(not(target_arch = "wasm32"))]
fn validated_coordinates(
    coordinates: Option<Vec<GeographicCoordinate>>,
    payload_description: &str,
) -> Result<Vec<geospatial_types::GeographicCoordinate>, SdkError> {
    let mirror_coordinates = coordinates.ok_or(SdkError::InvalidRenderInput {
        detail: format!("{payload_description} descriptor needs its coordinates"),
    })?;
    mirror_coordinates
        .into_iter()
        .map(crate::projection::validated_coordinate)
        .collect()
}

/// The platform window surface handle of `MapView::attach`: exactly
/// one platform pointer is set. Pointers lower as `u64` (Kotlin `ULong`,
/// Swift `UInt64`), where a plain `u64 0` stands in for the foreign
/// languages' null pointer without ever dereferencing one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SurfaceHandle {
    /// macOS: pointer to the `NSView` the surface renders into.
    pub appkit_view_pointer: Option<u64>,
    /// iOS: pointer to the `UIView` the surface renders into.
    pub ios_view_pointer: Option<u64>,
    /// Android: pointer to the `ANativeWindow` obtained from the platform
    /// `Surface`.
    pub android_native_window_pointer: Option<u64>,
}

/// One presented frame's tile and overlay accounting (`MapView::draw`'s
/// return), mirroring `rustic-maps-rendering`'s per-frame statistics: every requested
/// tile lands in exactly one readiness count, and drawn pixels come from the
/// present and fallback tiles while the deferred and unavailable ones are
/// where later frames may still change pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameStatistics {
    /// Tile requests the frame resolved (per request; duplicates in a
    /// request listing cost one fetch but report per request).
    pub requested_tile_count: u32,
    /// Tiles drawn at their own zoom level.
    pub present_tile_count: u32,
    /// Tiles drawn from their nearest pooled ancestor.
    pub fallback_tile_count: u32,
    /// Tiles whose fetch was deferred by the per-frame budget.
    pub deferred_tile_count: u32,
    /// Tiles the provider rejected this frame.
    pub unavailable_tile_count: u32,
    /// Clip-space triangles the overlay layer contributed to the frame.
    pub overlay_triangle_count: u32,
}

impl FrameStatistics {
    /// Builds the mirror record from the native frame statistics.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn from_native_frame_statistics(
        native_frame_statistics: &SurfaceFrameStatistics,
    ) -> Self {
        Self {
            requested_tile_count: native_frame_statistics.requested_tile_count,
            present_tile_count: native_frame_statistics.present_tile_count,
            fallback_tile_count: native_frame_statistics.fallback_tile_count,
            deferred_tile_count: native_frame_statistics.deferred_tile_count,
            unavailable_tile_count: native_frame_statistics.unavailable_tile_count,
            overlay_triangle_count: native_frame_statistics.overlay_triangle_count,
        }
    }
}

/// The serving traffic-flow cache's counters beside the budget they obey
/// (`MapSdk::traffic_flow_cache_statistics`'s return; see
/// `rustic_maps.udl`'s `TrafficFlowCacheStatistics` for the field contract).
/// The hits and misses prove a repeated pane was answered without asking the
/// provider, and `maximum_entry_count` bounds how many cells the cache may
/// hold at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrafficFlowCacheStatistics {
    /// Requests answered entirely from fresh cells.
    pub hit_count: u64,
    /// Requests that had to ask the provider for at least one cell.
    pub miss_count: u64,
    /// Cells currently held.
    pub entry_count: u64,
    /// Cells dropped to stay inside the entry budget.
    pub eviction_count: u64,
    /// Flow segments currently held across every cell.
    pub segment_count: u64,
    /// Zoom level the cells are keyed at.
    pub grid_zoom_level: u32,
    /// Cells the cache may hold before the least recently used is evicted.
    pub maximum_entry_count: u64,
    /// Seconds a stored cell stays fresh.
    pub time_to_live_seconds: u64,
}

impl From<rustic_maps_core::TrafficFlowCacheReadback> for TrafficFlowCacheStatistics {
    fn from(readback: rustic_maps_core::TrafficFlowCacheReadback) -> Self {
        Self {
            hit_count: readback.statistics.hit_count,
            miss_count: readback.statistics.miss_count,
            entry_count: u64::try_from(readback.statistics.entry_count).unwrap_or(u64::MAX),
            eviction_count: readback.statistics.eviction_count,
            segment_count: u64::try_from(readback.statistics.segment_count).unwrap_or(u64::MAX),
            grid_zoom_level: u32::from(readback.configuration.grid_zoom_level),
            maximum_entry_count: u64::try_from(readback.configuration.maximum_entry_count)
                .unwrap_or(u64::MAX),
            time_to_live_seconds: readback.configuration.time_to_live_seconds,
        }
    }
}

/// The installed tile payload cache's exact counters and occupancy
/// (`MapSdk::tile_cache_statistics`'s return; see `rustic_maps.udl`'s
/// `TileCacheStatistics` for the field contract).
///
/// Both the raster base and the vector base fetch through the same installed
/// store, so one counter pair covers every payload lookup: a hit was answered
/// without a provider request, and a miss went to the provider (or degraded to
/// one after a store read failure).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TileCacheStatistics {
    /// Payload lookups the store answered without a provider request.
    pub hit_count: u64,
    /// Payload lookups the store could not answer.
    pub miss_count: u64,
    /// Bytes currently stored, or `None` when the store's occupancy could not
    /// be read.
    pub stored_byte_count: Option<u64>,
}

#[cfg(not(target_arch = "wasm32"))]
impl From<rustic_maps_cache::TileCacheStatistics> for TileCacheStatistics {
    fn from(statistics: rustic_maps_cache::TileCacheStatistics) -> Self {
        Self {
            hit_count: statistics.hit_count,
            miss_count: statistics.miss_count,
            stored_byte_count: None,
        }
    }
}

/// The effective raster-tile caching terms of the installed cache
/// (`MapSdk::raster_tile_cache_terms`'s return), shaped so a developer can
/// display the caching posture: the category default, the provider's terms
/// ceiling and the value currently in force (see
/// `rustic_maps.udl`'s `TileCacheTerms` for the field contract).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileCacheTerms {
    /// Time-to-live currently in force on the installed cache, in seconds:
    /// the category default, or the last accepted override (clamped to the
    /// ceiling when the override exceeded it).
    pub effective_time_to_live_seconds: u64,
    /// The installed provider's declared caching-terms ceiling in seconds,
    /// or `None` when the provider declares no ceiling — the requested
    /// time-to-live then applies verbatim. For the MapQuest raster tiles
    /// this is the documented terms ceiling (thirty calendar days).
    pub provider_terms_ceiling_seconds: Option<u64>,
    /// The cache category's default time-to-live for raster tiles, in
    /// seconds (thirty days, the imagery provider's declared ceiling); the
    /// value a freshly registered cache starts at.
    pub default_time_to_live_seconds: u64,
}

/// One offline region selection: the identity the region is stored and
/// managed under, the area it covers, the zoom range and tile form the pack
/// holds, and the retention the application requests (see
/// `rustic_maps.udl`'s `RegionDefinition` for the field contract).
#[derive(Debug, Clone, PartialEq)]
pub struct RegionDefinition {
    /// Caller-chosen stable identifier; re-downloading the same identifier is
    /// idempotent and a redefined shape replaces the stored region.
    pub region_id: String,
    /// Human-readable name for region-listing surfaces.
    pub display_name: String,
    /// The region's bounding box; boxes never span the antimeridian.
    pub bounding_box: BoundingBox,
    /// Lowest zoom level downloaded; ranges download coarse-first.
    pub minimum_zoom_level: u8,
    /// Highest zoom level downloaded.
    pub maximum_zoom_level: u8,
    /// Encoded format the stored tile payloads carry.
    pub image_format: RasterImageFormat,
    /// Pixel resolution the stored tile payloads carry.
    pub tile_resolution: RasterTileResolution,
    /// Requested retention in seconds; null takes the raster-tiles category
    /// default, clamped to the provider's declared ceiling. Zero is refused.
    pub time_to_live_seconds: Option<u64>,
}

/// One offline region as it stands on disk: the stored shape, the effective
/// retention and the live tiles among its members (see `rustic_maps.udl`'s
/// `RegionRecord` for the field contract).
#[derive(Debug, Clone, PartialEq)]
pub struct RegionRecord {
    /// The region's identifier.
    pub region_id: String,
    /// The region's display name.
    pub display_name: String,
    /// The recorded area the region covers.
    pub bounding_box: BoundingBox,
    /// The recorded lowest zoom level.
    pub minimum_zoom_level: u8,
    /// The recorded highest zoom level.
    pub maximum_zoom_level: u8,
    /// The recorded encoded format.
    pub image_format: RasterImageFormat,
    /// The recorded pixel resolution.
    pub tile_resolution: RasterTileResolution,
    /// The retention the region's entries carry (requested, ceiling-clamped).
    pub effective_time_to_live_seconds: u64,
    /// Tile fingerprints the region claims membership for.
    pub member_tile_count: u64,
    /// Members whose entries are live on disk right now.
    pub present_tile_count: u64,
    /// Payload bytes of the live members.
    pub present_payload_bytes: u64,
}

/// The phase of one region download.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionDownloadPhase {
    /// The download is running; progress reports carry the counts so far.
    Downloading,
    /// The download finished with every member tile present.
    Completed,
    /// The application cancelled the download; the partial region is kept and
    /// resumable.
    Cancelled,
    /// The download stopped on a failure after keeping the partial region.
    Failed,
}

/// One progress report of a region download, delivered to a
/// [`RegionDownloadProgressListener`](crate::RegionDownloadProgressListener):
/// one at zero completions, one per completed tile and exactly one terminal
/// phase (see `rustic_maps.udl`'s `RegionDownloadProgress` for the field
/// contract).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionDownloadProgress {
    /// The region this report belongs to.
    pub region_id: String,
    /// Tiles the region enumerates.
    pub total_tile_count: u64,
    /// Tiles downloaded and refreshed so far.
    pub completed_tile_count: u64,
    /// Completed tiles the provider had to fetch.
    pub network_fetched_tile_count: u64,
    /// Completed tiles the store already held.
    pub disk_served_tile_count: u64,
    /// The phase this report describes.
    pub phase: RegionDownloadPhase,
}

/// What a region's explicit deletion removed (see `rustic_maps.udl`'s
/// `RegionRemovalReport` for the field contract).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegionRemovalReport {
    /// Tile entries deleted (never a tile another live region still claims).
    pub removed_entry_count: u64,
    /// Payload bytes those entries carried.
    pub removed_payload_bytes: u64,
}

/// One provider-neutral base-map tile source
/// (`MapSdk::register_url_template_raster_tile_source`'s parameter), shaped
/// so an application hands the SDK exactly what its shared provider registry
/// declares: a tileset's URL template, name, zoom range, served image format
/// and resolution, and the User-Agent its requests identify themselves with
/// (see `rustic_maps.udl`'s `UrlTemplateRasterTileSource` for the field
/// contract).
#[derive(Debug, Clone, PartialEq)]
pub struct UrlTemplateRasterTileSource {
    /// The `{z}/{x}/{y}` URL template the tileset publishes.
    pub url_template: String,
    /// The tileset's name, used verbatim in every diagnostic it raises.
    pub source_name: String,
    /// Lowest zoom level the tileset serves.
    pub minimum_zoom_level: u8,
    /// Highest zoom level the tileset serves.
    pub maximum_zoom_level: u8,
    /// The image format the tileset serves.
    pub image_format: RasterImageFormat,
    /// The pixel resolution the tileset serves.
    pub tile_resolution: RasterTileResolution,
    /// The `User-Agent` header every tile request carries; empty sends none.
    pub user_agent: String,
    /// Immediate requests the source admits before pacing starts; `None`
    /// keeps the documented default.
    pub token_bucket_capacity_tokens: Option<f64>,
    /// Sustained request rate in tokens per second; `None` keeps the
    /// documented default.
    pub refill_tokens_per_second: Option<f64>,
}

impl UrlTemplateRasterTileSource {
    /// Converts the binding record into the provider layer's source
    /// declaration, pacing and user agent included. The provider layer owns
    /// the tile semantics and the request budget; the binding crate owns only
    /// the contract mirror.
    pub(crate) fn into_provider_source(
        self,
    ) -> Result<everymap_providers_url_template_raster::UrlTemplateRasterTileSource, SdkError> {
        let pacing = everymap_tile_source_pacing(
            &self.source_name,
            self.token_bucket_capacity_tokens,
            self.refill_tokens_per_second,
        )?;
        let headers = everymap_request_headers(self.user_agent);
        Ok(
            everymap_providers_url_template_raster::UrlTemplateRasterTileSource {
                url_template: self.url_template,
                source_name: self.source_name,
                attribution_text: String::new(),
                terms_text: String::new(),
                minimum_zoom_level: self.minimum_zoom_level,
                maximum_zoom_level: self.maximum_zoom_level,
                image_format: everymap_image_format(self.image_format),
                tile_resolution: everymap_tile_resolution(self.tile_resolution),
                subdomains: Vec::new(),
                headers,
                pacing: Some(pacing),
            },
        )
    }
}

/// Builds the provider layer's request budget from the contract's pacing
/// fields, with the documented defaults for absent values.
///
/// The provider layer admits a whole number of requests, so a fractional
/// capacity is refused rather than rounded: rounding would silently change
/// the budget the application declared.
fn everymap_tile_source_pacing(
    source_name: &str,
    capacity_tokens: Option<f64>,
    refill_tokens_per_second: Option<f64>,
) -> Result<everymap_providers_url_template_raster::TileSourcePacing, SdkError> {
    const DEFAULT_CAPACITY_TOKENS: f64 = 8.0;
    const DEFAULT_REFILL_TOKENS_PER_SECOND: f64 = 8.0;

    let (capacity_tokens, refill_tokens_per_second) =
        rustic_maps_core::everymap_bridge::tile_source_budget_rates(
            source_name,
            capacity_tokens.unwrap_or(DEFAULT_CAPACITY_TOKENS),
            refill_tokens_per_second.unwrap_or(DEFAULT_REFILL_TOKENS_PER_SECOND),
        )
        .map_err(SdkError::from)?;
    let pacing = everymap_providers_url_template_raster::TileSourcePacing::new(
        capacity_tokens,
        refill_tokens_per_second,
    )
    .with_maximum_wait_seconds(rustic_maps_core::everymap_bridge::TILE_SOURCE_MAXIMUM_WAIT_SECONDS);
    pacing.validate(source_name).map_err(|error| {
        SdkError::from(rustic_maps_core::everymap_bridge::map_provider_error(error))
    })?;
    Ok(pacing)
}

/// The headers every tile request carries: the declared user agent, when one
/// is set. Any credential a public template needs belongs in the template.
fn everymap_request_headers(user_agent: String) -> Vec<(String, String)> {
    if user_agent.is_empty() {
        return Vec::new();
    }
    vec![("User-Agent".to_string(), user_agent)]
}

fn everymap_image_format(
    image_format: RasterImageFormat,
) -> everymap_core::domains::tiling::RasterImageFormat {
    match image_format {
        RasterImageFormat::Png => everymap_core::domains::tiling::RasterImageFormat::Png,
        RasterImageFormat::Jpeg => everymap_core::domains::tiling::RasterImageFormat::Jpeg,
    }
}

fn everymap_tile_resolution(
    tile_resolution: RasterTileResolution,
) -> everymap_core::domains::tiling::RasterTileResolution {
    match tile_resolution {
        RasterTileResolution::Standard => {
            everymap_core::domains::tiling::RasterTileResolution::Standard
        }
        RasterTileResolution::Retina => {
            everymap_core::domains::tiling::RasterTileResolution::Retina
        }
    }
}

/// A vector base map served by a provider configured through
/// `MapSdk::configure_provider_set` (`MapView::set_provider_vector_tile_source`'s
/// parameter). The provider supplies its own tileset name, attribution and
/// zoom span; the application chooses only the provider and the committed
/// style the base paints with (see `rustic_maps.udl`'s
/// `ProviderVectorTileSource` for the field contract).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderVectorTileSource {
    /// The provider key `configure_provider_set` registered for vector tiles.
    pub provider_key: String,
    /// The committed style id the base paints with.
    pub style_id: String,
}

/// One provider-neutral vector base-map source
/// (`MapView::set_vector_tile_source`'s parameter), shaped so an application
/// hands the SDK exactly what its
/// shared provider registry declares: a tileset's URL template, name, the
/// attribution line its terms require, the committed style id the base paints
/// with, and its zoom range (see `rustic_maps.udl`'s
/// `UrlTemplateVectorTileSource` for the field contract).
#[derive(Debug, Clone, PartialEq)]
pub struct UrlTemplateVectorTileSource {
    /// The `{z}/{x}/{y}` URL template the tileset publishes.
    pub url_template: String,
    /// The tileset's name, used verbatim in every diagnostic it raises.
    pub source_name: String,
    /// The attribution line the tileset's terms require wherever it is shown.
    pub attribution_text: String,
    /// The committed style id the base paints with.
    pub style_id: String,
    /// Lowest zoom level the tileset serves.
    pub minimum_zoom_level: u8,
    /// Highest zoom level the tileset serves.
    pub maximum_zoom_level: u8,
    /// Immediate requests the source admits before pacing starts; `None`
    /// keeps the documented default.
    pub token_bucket_capacity_tokens: Option<f64>,
    /// Sustained request rate in tokens per second; `None` keeps the
    /// documented default.
    pub refill_tokens_per_second: Option<f64>,
    /// Terms the tileset's usage policy requires alongside attribution;
    /// `None` declares none.
    pub terms_text: Option<String>,
    /// Values substituted for the template's optional `{s}` placeholder, in
    /// the source's preference order; `None` when the template has no `{s}`.
    pub subdomains: Option<Vec<String>>,
    /// Headers every tile request carries, in the order given; `None` sends
    /// none.
    pub headers: Option<Vec<SourceHeader>>,
    /// Style names or families the tileset is designed to be rendered with;
    /// `None` when it declares none.
    pub style_hints: Option<Vec<String>>,
}

impl UrlTemplateVectorTileSource {
    /// Converts the binding record into the provider layer's source
    /// declaration and the style id the base paints with, so the binding
    /// crate owns only the contract mirror. Native-only with the surface view
    /// that installs it.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn into_provider_source(
        self,
    ) -> Result<
        (
            everymap_providers_url_template_vector::UrlTemplateVectorTileSource,
            String,
        ),
        SdkError,
    > {
        let pacing = everymap_tile_source_pacing(
            &self.source_name,
            self.token_bucket_capacity_tokens,
            self.refill_tokens_per_second,
        )?;
        // A keyless tileset's named declaration carries the terms, subdomains,
        // headers and style hints its operator publishes. A caller that
        // leaves one of them out inherits the recorded value rather than
        // dropping it.
        let recorded_source = recorded_url_template_vector_source(&self.url_template);
        let style_hints = self
            .style_hints
            .or_else(|| recorded_source.as_ref().map(recorded_source_style_hints))
            .unwrap_or_default();
        let terms_text = self
            .terms_text
            .or_else(|| recorded_source.as_ref().map(recorded_source_terms_text))
            .unwrap_or_default();
        let subdomains = self
            .subdomains
            .or_else(|| recorded_source.as_ref().map(recorded_source_subdomains))
            .unwrap_or_default();
        let headers = self
            .headers
            .map(source_header_pairs)
            .or_else(|| recorded_source.as_ref().map(recorded_source_headers))
            .unwrap_or_default();
        let source = everymap_providers_url_template_vector::UrlTemplateVectorTileSource {
            url_template: self.url_template,
            source_descriptor: everymap_core::domains::tiling::VectorTileSourceDescriptor {
                source_name: self.source_name,
                attribution_text: self.attribution_text,
                minimum_zoom_level: self.minimum_zoom_level,
                maximum_zoom_level: self.maximum_zoom_level,
                // A URL template does not declare its wire encoding; the
                // response sniffs it, so the descriptor names plain MVT and
                // the fetched tile reports what actually arrived.
                tile_encoding: everymap_core::domains::tiling::VectorTileEncoding::MapboxVectorTile,
                style_hints,
            },
            terms_text,
            subdomains,
            headers,
            pacing: Some(pacing),
        };
        Ok((source, self.style_id))
    }
}

/// The named keyless declaration the provider layer publishes for a URL
/// template, when one matches. The provider crate owns the declarations, so
/// the SDK names no tileset itself; a match lets the caller inherit the
/// terms, subdomains, headers and style hints the operator recorded.
///
/// The provider crate exposes each declaration as its own constructor but no
/// lookup by template, so this list is the SDK-side coupling to its current
/// pair: a third declaration would need an edit here. An upstream accessor
/// such as `vector_source_for_template(url_template: &str)` (returning the
/// named declaration that publishes a template, if any) would remove it.
#[cfg(not(target_arch = "wasm32"))]
fn recorded_url_template_vector_source(
    url_template: &str,
) -> Option<everymap_providers_url_template_vector::UrlTemplateVectorTileSource> {
    [
        everymap_providers_url_template_vector::openstreetmap_us_vector_source(),
        everymap_providers_url_template_vector::openfreemap_vector_source(),
    ]
    .into_iter()
    .find(|declaration| declaration.url_template == url_template)
}

/// The recorded terms text of a named declaration.
#[cfg(not(target_arch = "wasm32"))]
fn recorded_source_terms_text(
    declaration: &everymap_providers_url_template_vector::UrlTemplateVectorTileSource,
) -> String {
    declaration.terms_text.clone()
}

/// The recorded `{s}` subdomains of a named declaration.
#[cfg(not(target_arch = "wasm32"))]
fn recorded_source_subdomains(
    declaration: &everymap_providers_url_template_vector::UrlTemplateVectorTileSource,
) -> Vec<String> {
    declaration.subdomains.clone()
}

/// The recorded request headers of a named declaration.
#[cfg(not(target_arch = "wasm32"))]
fn recorded_source_headers(
    declaration: &everymap_providers_url_template_vector::UrlTemplateVectorTileSource,
) -> Vec<(String, String)> {
    declaration.headers.clone()
}

/// The recorded style hints of a named declaration.
#[cfg(not(target_arch = "wasm32"))]
fn recorded_source_style_hints(
    declaration: &everymap_providers_url_template_vector::UrlTemplateVectorTileSource,
) -> Vec<String> {
    declaration.source_descriptor.style_hints.clone()
}

/// Converts the contract's `SourceHeader` records into the provider layer's
/// name-value pairs.
#[cfg(not(target_arch = "wasm32"))]
fn source_header_pairs(headers: Vec<SourceHeader>) -> Vec<(String, String)> {
    headers
        .into_iter()
        .map(|header| (header.name, header.value))
        .collect()
}

/// One vector source's layer accounting for the last resolved frame
/// (`VectorTileSourceStatus::source_layer_counts`): how many of the source's
/// style layers the painted tileset matched and how many painted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorTileSourceLayerCount {
    /// The style source id the counts belong to.
    pub source_id: String,
    /// The source's layers whose source layer the painted tileset carries.
    pub matched_layer_count: u32,
    /// The source's layers that painted in the frame.
    pub drawn_layer_count: u32,
}

/// The vector base source's status readback
/// (`MapView::vector_tile_source_status`): what the view draws a styled base
/// with and what its last resolved
/// frame achieved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorTileSourceStatus {
    /// Whether the view carries a vector base source at all.
    pub is_configured: bool,
    /// The active style id; empty when no source is configured.
    pub style_id: String,
    /// The tileset's name; empty when no source is configured.
    pub source_name: String,
    /// The attribution line the tileset's terms require; empty when no source
    /// is configured.
    pub attribution_text: String,
    /// Highest zoom level the tileset serves; zero when no source is
    /// configured.
    pub maximum_zoom_level: u8,
    /// Tile addresses the last resolved frame drew.
    pub drawn_tile_count: u32,
    /// Tile addresses the last resolved frame skipped after a failed fetch or
    /// decode.
    pub failed_tile_count: u32,
    /// The last resolved frame's last tile failure detail, if any.
    pub last_failure_detail: Option<String>,
    /// Every unsupported-property diagnostic the installed style produced.
    pub unsupported_property_count: u32,
    /// The distinct unsupported properties, scope-qualified and sorted, capped
    /// at the renderer's documented bound.
    pub unsupported_property_names: Vec<String>,
    /// The assets that fell back to a degraded draw, sorted and capped at the
    /// renderer's documented bound.
    pub degraded_asset_names: Vec<String>,
    /// Each referenced source's layer accounting for the last resolved frame,
    /// sorted by source id.
    pub source_layer_counts: Vec<VectorTileSourceLayerCount>,
}

impl VectorTileSourceStatus {
    /// The honest not-configured status (no base installed). Only the native
    /// surface view asks for it; the browser canvas reports its own status
    /// through the hand-written facade.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn not_configured() -> Self {
        Self {
            is_configured: false,
            style_id: String::new(),
            source_name: String::new(),
            attribution_text: String::new(),
            maximum_zoom_level: 0,
            drawn_tile_count: 0,
            failed_tile_count: 0,
            last_failure_detail: None,
            unsupported_property_count: 0,
            unsupported_property_names: Vec::new(),
            degraded_asset_names: Vec::new(),
            source_layer_counts: Vec::new(),
        }
    }
}

/// What triggered a speech-instruction line (the speech-instruction topic).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeechInstructionKind {
    /// A trigger band was crossed this fix (the far, moderate or near band).
    ManeuverApproach,
    /// The immediate band: the maneuver is imminent.
    ImmediateManeuver,
    /// Guidance just recovered from off-route; guidance resumed.
    OffRouteRecovery,
    /// The trip arrived.
    Arrival,
}

/// One speech-trigger line the navigation controller's pacing policy judged
/// newsworthy; the speech text is the composed payload's verbatim channel.
#[derive(Debug, Clone, PartialEq)]
pub struct SpeechInstruction {
    /// The verbatim composed speech text (never rewritten by the trigger
    /// policy).
    pub utterance_speech_text: String,
    /// The live distance to the maneuver, in meters, at emission time.
    pub trigger_distance_meters: f64,
    /// The utterance identity (a controller-ordered integer, monotonic across
    /// the controller's lifetime): a stable monotonic
    /// id composed at emission — strictly increasing over every line the
    /// composing planner's owning controller has emitted since construction,
    /// never reused, and stable for the single delivery of one line.
    /// Identical text re-emitted is a new id; dedup and flush decisions stay
    /// at the platform speaker.
    pub utterance_id: u64,
    /// What triggered this line.
    pub instruction_kind: SpeechInstructionKind,
}

impl SpeechInstruction {
    /// Builds the mirror record from the native utterance.
    pub(crate) fn from_native_speech_utterance(
        native_utterance: &rustic_maps_navigation::SpeechUtterance,
    ) -> Self {
        Self {
            utterance_speech_text: native_utterance.utterance_speech_text.clone(),
            trigger_distance_meters: native_utterance.trigger_distance_meters,
            utterance_id: native_utterance.utterance_id,
            instruction_kind: match native_utterance.kind {
                rustic_maps_navigation::SpeechUtteranceKind::ManeuverApproach => {
                    SpeechInstructionKind::ManeuverApproach
                }
                rustic_maps_navigation::SpeechUtteranceKind::ImmediateManeuver => {
                    SpeechInstructionKind::ImmediateManeuver
                }
                rustic_maps_navigation::SpeechUtteranceKind::OffRouteRecovery => {
                    SpeechInstructionKind::OffRouteRecovery
                }
                rustic_maps_navigation::SpeechUtteranceKind::Arrival => {
                    SpeechInstructionKind::Arrival
                }
            },
        }
    }
}

/// Which visual form the renderer-owned location layer draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocationIndicatorVisualKind {
    /// The soft tracking dot (no course known or no route): accent core,
    /// white ring and dark halo.
    TrackingDot,
    /// The course-directed navigation dart inside the same ring and halo
    /// (navigation follow mode); degrades to the tracking dot without a
    /// course.
    GuidedCourseArrow,
}

/// Style of the location layer; `None` fields keep the documented
/// `rustic-maps-rendering` defaults (the overlay-style pattern).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocationIndicatorStyle {
    /// Which visual form this layer draws.
    pub visual_kind: LocationIndicatorVisualKind,
    /// Accent-core radius in screen pixels; `None` keeps the default.
    pub radius_pixels: Option<f64>,
    /// Accent fill color of the core disc and navigation dart; `None` keeps
    /// the default.
    pub rgba_color: Option<OverlayColor>,
    /// Ring color: the ring around the puck and the navigation dart's
    /// outline; `None` keeps the default white.
    pub ring_rgba_color: Option<OverlayColor>,
    /// Halo color drawn under the puck; `None` keeps the default.
    pub halo_rgba_color: Option<OverlayColor>,
    /// Accuracy ring drawn around the puck; `None` keeps the default.
    pub show_accuracy_ring: Option<bool>,
}

/// One frame's location-layer state (the presentable subset of a position
/// fix): position, optional course over ground, optional horizontal accuracy.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocationIndicatorState {
    /// Fix position.
    pub coordinate: GeographicCoordinate,
    /// Course over ground in degrees clockwise from true north; `None` when
    /// the source does not report one.
    pub course_heading_degrees: Option<f64>,
    /// Horizontal accuracy radius in meters; the accuracy ring renders when
    /// set and the style shows rings.
    pub horizontal_accuracy_meters: Option<f64>,
}

impl LocationIndicatorVisualKind {
    /// Builds the native visual kind tag from the mirror record.
    /// Native-only with the surface view that draws it.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn to_native_visual_kind(
        self,
    ) -> rustic_maps_rendering::LocationIndicatorVisualKind {
        match self {
            Self::TrackingDot => rustic_maps_rendering::LocationIndicatorVisualKind::TrackingDot,
            Self::GuidedCourseArrow => {
                rustic_maps_rendering::LocationIndicatorVisualKind::GuidedCourseArrow
            }
        }
    }
}

impl LocationIndicatorState {
    /// Validates the fix and builds the native location-layer state: the
    /// coordinate must be canonical degrees (the same boundary the overlay
    /// seam applies), while the optional scalar fields pass through
    /// verbatim (the renderer's clamps own their ranges). Native-only with
    /// the surface view that draws it.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn into_native_location_indicator_state(
        self,
    ) -> Result<rustic_maps_rendering::LocationIndicatorState, SdkError> {
        Ok(rustic_maps_rendering::LocationIndicatorState {
            coordinate: crate::projection::validated_coordinate(self.coordinate)?,
            course_heading_degrees: self.course_heading_degrees,
            horizontal_accuracy_meters: self.horizontal_accuracy_meters,
        })
    }
}

impl LocationIndicatorStyle {
    /// Validates the numeric style fields and builds the native style; the
    /// `None` fields keep the documented rustic-maps-rendering defaults (mirroring
    /// the overlay descriptor's style validation: a radius must be positive
    /// and finite, a color channel inside `[0, 1]`). Native-only with the
    /// surface view that draws it.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn into_native_location_indicator_style(
        self,
    ) -> Result<rustic_maps_rendering::LocationIndicatorStyle, SdkError> {
        if let Some(radius_pixels) = self.radius_pixels {
            MapOverlay::validated_size_pixels(radius_pixels, "location indicator disc radius")?;
        }
        let rgba_color = match self.rgba_color {
            Some(rgba_color) => Some(rgba_color.to_native_rgba("location indicator")?),
            None => None,
        };
        let ring_rgba_color = match self.ring_rgba_color {
            Some(ring_rgba_color) => {
                Some(ring_rgba_color.to_native_rgba("location indicator ring")?)
            }
            None => None,
        };
        let halo_rgba_color = match self.halo_rgba_color {
            Some(halo_rgba_color) => {
                Some(halo_rgba_color.to_native_rgba("location indicator halo")?)
            }
            None => None,
        };
        Ok(rustic_maps_rendering::LocationIndicatorStyle {
            visual_kind: self.visual_kind.to_native_visual_kind(),
            radius_pixels: self.radius_pixels,
            rgba_color,
            ring_rgba_color,
            halo_rgba_color,
            show_accuracy_ring: self.show_accuracy_ring,
        })
    }
}

/// Severity of an SDK diagnostic event, mirroring the `tracing` levels the
/// crates log under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SdkLogSeverity {
    /// Very fine-grained frame-path instrumentation.
    Trace,
    /// Fine-grained internal state transitions.
    Debug,
    /// Coarse-grained progress.
    Info,
    /// A problem this event explains.
    Warn,
    /// A failure this event explains.
    Error,
}

/// One completed provider request, as a diagnostics event reports it.
///
/// The record is already redacted: the request path keeps its structure with
/// query values removed, and no credential or response body is carried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRequestTrace {
    /// The registry key of the provider that served the call.
    pub provider_key: String,
    /// The contract the call served, for example `RoutingProvider`.
    pub provider_contract: String,
    /// The HTTP method as an uppercase word (`GET`, `POST`).
    pub http_method: String,
    /// The redacted request path, with query names and no values.
    pub request_path: String,
    /// `2xx`, `3xx`, `4xx`, `5xx` or the transport-failure marker.
    pub status_class: String,
    /// The derived outcome word (`success`, `client-error`, …).
    pub outcome: String,
    /// Wall-clock duration of the attempt in milliseconds.
    pub duration_milliseconds: u64,
    /// Response payload size in bytes; zero when no response arrived.
    pub response_bytes: u64,
    /// The redacted response prefix captured behind the excerpt switch,
    /// capped at 512 bytes.
    pub response_excerpt: Option<String>,
}

/// One structured SDK internal log event, as delivered to a foreign
/// diagnostics listener or read back by a flush. Coordinates in event messages
/// are rounded at the sink; the API key never appears in a message.
#[derive(Debug, Clone, PartialEq)]
pub struct LogEvent {
    /// Wall-clock milliseconds since the Unix epoch, as the ring recorded
    /// the event.
    pub timestamp_milliseconds_since_epoch: u64,
    /// The event's severity.
    pub severity: SdkLogSeverity,
    /// The named per-subsystem target that logged it.
    pub subsystem_target: String,
    /// The one-line already-scrubbed message.
    pub message_line: String,
    /// The typed request facts when this event reports a completed provider
    /// call; `None` for every other event.
    pub provider_request_trace: Option<ProviderRequestTrace>,
}

/// Which shape of foreign provider failure a diagnostic captured. The two
/// shapes follow the two ways a foreign implementation of a
/// `[Trait, WithForeign]` provider contract can fail: typed (it threw one of
/// the contract's `SdkError` variants) or unclassified (it failed without a
/// mapped error, the round-11 conversion's subject).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderFailureClassification {
    /// The foreign implementation failed without a mapped error (an
    /// exception the contract does not spell): the failure text is the
    /// uniffi callback machinery's verbatim reason for it.
    UnclassifiedCallbackFailure,
    /// The foreign implementation threw one of the contract's `SdkError`
    /// variants, which round-trips typed through the seam: the failure text
    /// is the thrown binding error's verbatim display form.
    MappedContractError,
}

/// One failed foreign-callback provider call, as retained in
/// `MapSdk::latest_provider_failure_diagnostics`'s bounded per-object ring
/// (the provider-failure diagnostic seam). Every
/// field is knowable at the adapter boundary; no data is invented beyond it
/// (see the UDL documentation for the classification limitation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderFailureDiagnostic {
    /// Wall-clock milliseconds since the Unix epoch, as the seam recorded
    /// the failure.
    pub timestamp_milliseconds_since_epoch: u64,
    /// Which failure shape this entry captured.
    pub failure_classification: ProviderFailureClassification,
    /// The provider contract the foreign code implements, e.g.
    /// `GeocodingProvider`.
    pub provider_contract_name: String,
    /// The contract method whose call failed, e.g. `forward_geocode`.
    pub provider_method_name: String,
    /// The verbatim failure text: the uniffi callback machinery's reason for
    /// the unclassified shape, or the thrown binding error's display form
    /// for the mapped shape.
    pub failure_detail: String,
}

// ---------------------------------------------------------------------------
// Device positioning records (M33): the contract-side mirrors of the
// `rustic-maps-positioning` controller's fix and source-failure types.
// ---------------------------------------------------------------------------

/// One device position report: a coordinate with the source's optional
/// accuracy, course and timestamp.
#[derive(Debug, Clone, PartialEq)]
pub struct PositionFix {
    /// The reported coordinate on the WGS 84 ellipsoid.
    pub coordinate: GeographicCoordinate,
    /// The radius in meters of the 68% horizontal-accuracy envelope, when the
    /// source reports one.
    pub horizontal_accuracy_meters: Option<f64>,
    /// Direction of travel in degrees clockwise from true north, when the
    /// source reports one.
    pub course_heading_degrees: Option<f64>,
    /// The fix's instant in seconds since the Unix epoch, when the source
    /// reports one.
    pub timestamp_seconds: Option<f64>,
}

impl PositionFix {
    /// Converts the contract record into the controller's fix.
    pub(crate) fn to_native_position_fix(
        &self,
    ) -> Result<rustic_maps_positioning::PositionFix, crate::error::SdkError> {
        Ok(rustic_maps_positioning::PositionFix {
            coordinate: self.coordinate.to_native_coordinate(),
            horizontal_accuracy_meters: self.horizontal_accuracy_meters,
            course_heading_degrees: self.course_heading_degrees,
            timestamp_seconds: self.timestamp_seconds,
        })
    }

    /// Converts the controller's fix into the contract record.
    pub(crate) fn from_native_position_fix(
        native_fix: &rustic_maps_positioning::PositionFix,
    ) -> Self {
        Self {
            coordinate: GeographicCoordinate::from_native_coordinate(&native_fix.coordinate),
            horizontal_accuracy_meters: native_fix.horizontal_accuracy_meters,
            course_heading_degrees: native_fix.course_heading_degrees,
            timestamp_seconds: native_fix.timestamp_seconds,
        }
    }
}

/// Why a positioning source could not produce fixes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionSourceError {
    /// The user denied the permission the source needs.
    PermissionDenied,
    /// Location services are disabled on the device or by policy.
    ServiceDisabled,
    /// No provider can answer right now.
    Unavailable,
    /// The source did not answer inside its timeout.
    Timeout,
}

impl PositionSourceError {
    /// Converts the contract enum into the controller's source status.
    pub(crate) fn to_native_source_status(self) -> rustic_maps_positioning::PositionSourceStatus {
        match self {
            Self::PermissionDenied => {
                rustic_maps_positioning::PositionSourceStatus::PermissionDenied
            }
            Self::ServiceDisabled => rustic_maps_positioning::PositionSourceStatus::ServiceDisabled,
            Self::Unavailable => rustic_maps_positioning::PositionSourceStatus::Unavailable,
            Self::Timeout => rustic_maps_positioning::PositionSourceStatus::Timeout,
        }
    }

    /// Converts the controller's source status into the contract enum.
    pub(crate) fn from_native_source_status(
        status: rustic_maps_positioning::PositionSourceStatus,
    ) -> Self {
        match status {
            rustic_maps_positioning::PositionSourceStatus::PermissionDenied => {
                Self::PermissionDenied
            }
            rustic_maps_positioning::PositionSourceStatus::ServiceDisabled => Self::ServiceDisabled,
            rustic_maps_positioning::PositionSourceStatus::Unavailable => Self::Unavailable,
            rustic_maps_positioning::PositionSourceStatus::Timeout => Self::Timeout,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SourceHeader, UrlTemplateVectorTileSource, everymap_tile_source_pacing};
    use rustic_maps_core::everymap_bridge::TILE_SOURCE_MAXIMUM_WAIT_SECONDS;

    #[test]
    fn the_foreign_vector_tile_source_mirror_keeps_encoding_and_style_hints() {
        let mirror = super::VectorTileSourceDescriptor {
            source_name: "application/declared".to_string(),
            attribution_text: "Declared tiles".to_string(),
            minimum_zoom_level: 0,
            maximum_zoom_level: 14,
            tile_encoding: super::VectorTileEncoding::GzippedMapboxVectorTile,
            style_hints: vec!["openmaptiles".to_string()],
        };
        // The contract mirror is where a foreign provider declares its wire
        // encoding and style hints; removing either field fails to compile
        // here rather than silently narrowing the foreign declaration.
        assert_eq!(
            mirror.tile_encoding,
            super::VectorTileEncoding::GzippedMapboxVectorTile
        );
        assert_eq!(mirror.style_hints, vec!["openmaptiles".to_string()]);
        let native = mirror.into_native_vector_tile_source_descriptor();
        assert_eq!(native.source_name, "application/declared");
        assert_eq!(native.attribution_text, "Declared tiles");
        assert_eq!(native.maximum_zoom_level, 14);
    }

    #[test]
    fn a_foreign_tile_keeps_the_wire_encoding_it_declares() {
        let tile = super::VectorTileData {
            tile_x: 3,
            tile_y: 4,
            zoom_level: 5,
            tile_encoding: super::VectorTileEncoding::GzippedMapboxVectorTile,
            tile_bytes: vec![0x1f, 0x8b, 0x00],
        };
        let native = tile.into_native_vector_tile_data();
        assert_eq!(
            native.tile_encoding,
            geospatial_types::VectorTileEncoding::GzippedMapboxVectorTile,
            "the decoder reads the per-tile encoding, so the mirror must preserve it"
        );
    }

    #[test]
    fn a_declared_tile_source_budget_carries_the_documented_ceiling() {
        let pacing = everymap_tile_source_pacing("test/source", None, None)
            .expect("the documented defaults are usable");
        assert_eq!(
            pacing.maximum_wait_seconds,
            Some(TILE_SOURCE_MAXIMUM_WAIT_SECONDS),
            "a tile request is refused after the documented ceiling instead of stalling"
        );
        assert_eq!(TILE_SOURCE_MAXIMUM_WAIT_SECONDS, 10.0);
    }

    #[test]
    fn a_url_template_vector_source_keeps_the_declaration_extras() {
        let (provider_source, style_id) = UrlTemplateVectorTileSource {
            url_template: "https://tiles.example.com/{z}/{x}/{y}.pbf".to_string(),
            source_name: "openfreemap/planet".to_string(),
            attribution_text: "Map data © OpenStreetMap contributors".to_string(),
            style_id: "bright-light".to_string(),
            minimum_zoom_level: 0,
            maximum_zoom_level: 14,
            token_bucket_capacity_tokens: None,
            refill_tokens_per_second: None,
            terms_text: Some("Tileset terms apply".to_string()),
            subdomains: Some(vec!["a".to_string(), "b".to_string()]),
            headers: Some(vec![SourceHeader {
                name: "User-Agent".to_string(),
                value: "test-agent".to_string(),
            }]),
            style_hints: Some(vec!["openmaptiles".to_string()]),
        }
        .into_provider_source()
        .expect("the source declaration builds");
        assert_eq!(style_id, "bright-light");
        assert_eq!(provider_source.terms_text, "Tileset terms apply");
        assert_eq!(
            provider_source.subdomains,
            vec!["a".to_string(), "b".to_string()]
        );
        assert_eq!(
            provider_source.headers,
            vec![("User-Agent".to_string(), "test-agent".to_string())]
        );
        assert_eq!(
            provider_source.source_descriptor.style_hints,
            vec!["openmaptiles".to_string()]
        );
    }

    #[test]
    fn empty_url_template_vector_source_extras_stay_empty() {
        let (provider_source, _style_id) = UrlTemplateVectorTileSource {
            url_template: "https://tiles.example.com/{z}/{x}/{y}.pbf".to_string(),
            source_name: "test/source".to_string(),
            attribution_text: "Test vector tiles".to_string(),
            style_id: "bright-light".to_string(),
            minimum_zoom_level: 0,
            maximum_zoom_level: 14,
            token_bucket_capacity_tokens: None,
            refill_tokens_per_second: None,
            terms_text: None,
            subdomains: None,
            headers: None,
            style_hints: None,
        }
        .into_provider_source()
        .expect("the source declaration builds");
        assert!(provider_source.terms_text.is_empty());
        assert!(provider_source.subdomains.is_empty());
        assert!(provider_source.headers.is_empty());
        assert!(provider_source.source_descriptor.style_hints.is_empty());
    }

    #[test]
    fn a_keyless_named_source_inherits_its_recorded_terms_and_headers() {
        let openfreemap_declaration =
            everymap_providers_url_template_vector::openfreemap_vector_source();
        let (openfreemap_source, _style_id) = UrlTemplateVectorTileSource {
            url_template: openfreemap_declaration.url_template.clone(),
            source_name: openfreemap_declaration
                .source_descriptor
                .source_name
                .clone(),
            attribution_text: openfreemap_declaration
                .source_descriptor
                .attribution_text
                .clone(),
            style_id: "bright-light".to_string(),
            minimum_zoom_level: 0,
            maximum_zoom_level: 14,
            token_bucket_capacity_tokens: None,
            refill_tokens_per_second: None,
            terms_text: None,
            subdomains: None,
            headers: None,
            style_hints: None,
        }
        .into_provider_source()
        .expect("the source declaration builds");
        assert_eq!(
            openfreemap_source.terms_text, openfreemap_declaration.terms_text,
            "the named declaration's terms survive an empty caller field"
        );
        assert_eq!(
            openfreemap_source.source_descriptor.style_hints,
            openfreemap_declaration.source_descriptor.style_hints,
            "the named declaration's style hints survive an empty caller field"
        );

        let openstreetmap_us_declaration =
            everymap_providers_url_template_vector::openstreetmap_us_vector_source();
        let (openstreetmap_us_source, _style_id) = UrlTemplateVectorTileSource {
            url_template: openstreetmap_us_declaration.url_template.clone(),
            source_name: openstreetmap_us_declaration
                .source_descriptor
                .source_name
                .clone(),
            attribution_text: openstreetmap_us_declaration
                .source_descriptor
                .attribution_text
                .clone(),
            style_id: "carto-light".to_string(),
            minimum_zoom_level: 0,
            maximum_zoom_level: 15,
            token_bucket_capacity_tokens: None,
            refill_tokens_per_second: None,
            terms_text: None,
            subdomains: None,
            headers: None,
            style_hints: None,
        }
        .into_provider_source()
        .expect("the source declaration builds");
        assert_eq!(
            openstreetmap_us_source.headers, openstreetmap_us_declaration.headers,
            "the identifying User-Agent the tileservice requires is inherited"
        );
    }
}
