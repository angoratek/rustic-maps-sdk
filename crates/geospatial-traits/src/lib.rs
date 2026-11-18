//! Vendor-neutral provider contracts for the Maps SDK.
//!
//! The core talks to providers only through the traits in this crate. A
//! vendor integration, an offline engine or a URL-template source implements
//! the relevant traits; no core crate depends on a concrete provider.
//!
//! Every trait is asynchronous, `Send + Sync` and object-safe, so an
//! implementation can be shared behind an `Arc` and registered once at
//! startup. Every method reports failure through [`SDKError`].
//!
//! Deterministic mock implementations live in [`mock`] for use in tests.

pub mod mock;

use async_trait::async_trait;
use geospatial_types::route::{
    DistanceMatrix, RouteCapabilities, RouteData, RouteOptions, TransportationMode,
};
use geospatial_types::{
    BoundingBox, ElevationPoint, GeographicCoordinate, PlaceResult, RasterImageFormat,
    RasterTileData, RasterTileResolution, SDKError, TrafficFlowData, TrafficIncident,
    VectorTileData,
};

/// Forward and reverse geocoding.
///
/// Implement this trait to resolve address text into candidate places and
/// coordinates into descriptions of the position.
#[async_trait]
pub trait GeocodingProvider: Send + Sync {
    /// Resolves free-form address text into candidate places.
    ///
    /// # Parameters
    /// - `address_text` — the address to resolve, as entered by the user.
    ///
    /// # Returns
    /// Candidate places in the provider's relevance order. An empty vector
    /// means the provider found nothing.
    ///
    /// # Errors
    /// Returns [`SDKError::NetworkRequestFailed`] when the request cannot
    /// reach the provider and [`SDKError::ResponseParsingFailure`] when the
    /// response cannot be decoded.
    async fn forward_geocode(&self, address_text: &str) -> Result<Vec<PlaceResult>, SDKError>;

    /// Resolves a coordinate into candidate places describing that position.
    ///
    /// # Parameters
    /// - `coordinate` — the position to describe.
    ///
    /// # Returns
    /// Candidate places in the provider's relevance order. An empty vector
    /// means the provider found nothing.
    ///
    /// # Errors
    /// Returns [`SDKError::NetworkRequestFailed`] when the request cannot
    /// reach the provider and [`SDKError::ResponseParsingFailure`] when the
    /// response cannot be decoded.
    async fn reverse_geocode(
        &self,
        coordinate: &GeographicCoordinate,
    ) -> Result<Vec<PlaceResult>, SDKError>;
}

/// Point-to-point route calculation.
#[async_trait]
pub trait RoutingProvider: Send + Sync {
    /// Calculates a route through ordered waypoints with the given options.
    ///
    /// # Parameters
    /// - `waypoints` — the ordered stops, starting at the origin.
    /// - `route_options` — transport mode, optimization strategy, units and
    ///   features to avoid.
    ///
    /// # Returns
    /// A route with one leg per consecutive waypoint pair, including distance,
    /// duration, geometry and turn-by-turn steps.
    ///
    /// # Errors
    /// Returns [`SDKError::ProviderRequestRejected`] when the provider cannot
    /// route the request, [`SDKError::NetworkRequestFailed`] when it cannot be
    /// reached, and [`SDKError::ResponseParsingFailure`] when the response
    /// cannot be decoded.
    async fn calculate_route(
        &self,
        waypoints: &[GeographicCoordinate],
        route_options: &RouteOptions,
    ) -> Result<RouteData, SDKError>;

    /// Reports which transport modes and which route strategy this provider
    /// can honour.
    ///
    /// A surface reads the answer to disable a choice it cannot serve with a
    /// reason, instead of sending the request and pretending the answer
    /// honoured it. The default reports every mode and both strategies, so a
    /// provider that supports everything — and every existing implementation
    /// — keeps compiling unchanged; a provider that cannot serve a choice
    /// overrides this.
    fn route_capabilities(&self) -> RouteCapabilities {
        RouteCapabilities::all_supported()
    }
}

/// Travel-time and distance matrix calculation.
#[async_trait]
pub trait DistanceMatrixProvider: Send + Sync {
    /// Calculates durations and distances for every origin-destination pair.
    ///
    /// # Parameters
    /// - `origins` — the origin coordinates.
    /// - `destinations` — the destination coordinates.
    /// - `transportation_mode` — the mode to route for.
    ///
    /// # Returns
    /// One entry per origin-destination pair, in row-major origin-major
    /// order.
    ///
    /// # Errors
    /// Returns [`SDKError::ProviderRequestRejected`] when the provider cannot
    /// serve the request, [`SDKError::NetworkRequestFailed`] when it cannot be
    /// reached, and [`SDKError::ResponseParsingFailure`] when the response
    /// cannot be decoded.
    async fn calculate_distance_matrix(
        &self,
        origins: &[GeographicCoordinate],
        destinations: &[GeographicCoordinate],
        transportation_mode: TransportationMode,
    ) -> Result<DistanceMatrix, SDKError>;
}

/// Address and place search.
#[async_trait]
pub trait SearchPlaceProvider: Send + Sync {
    /// Returns fast completion candidates while the user types.
    ///
    /// # Parameters
    /// - `query_text` — the partial query entered so far.
    /// - `search_boundary` — an optional area to prefer or restrict results
    ///   to.
    ///
    /// # Returns
    /// Completion candidates in the provider's relevance order.
    ///
    /// # Errors
    /// Returns [`SDKError::NetworkRequestFailed`] when the request cannot
    /// reach the provider and [`SDKError::ResponseParsingFailure`] when the
    /// response cannot be decoded.
    async fn search_autocomplete(
        &self,
        query_text: &str,
        search_boundary: Option<BoundingBox>,
    ) -> Result<Vec<PlaceResult>, SDKError>;

    /// Returns full search results for a settled query.
    ///
    /// # Parameters
    /// - `query_text` — the complete query.
    /// - `search_boundary` — an optional area to prefer or restrict results
    ///   to.
    ///
    /// # Returns
    /// Matching places in the provider's relevance order.
    ///
    /// # Errors
    /// Returns [`SDKError::NetworkRequestFailed`] when the request cannot
    /// reach the provider and [`SDKError::ResponseParsingFailure`] when the
    /// response cannot be decoded.
    async fn search_places(
        &self,
        query_text: &str,
        search_boundary: Option<BoundingBox>,
    ) -> Result<Vec<PlaceResult>, SDKError>;
}

/// Traffic disruption reports.
#[async_trait]
pub trait TrafficIncidentProvider: Send + Sync {
    /// Fetches incidents intersecting a bounding box.
    ///
    /// # Parameters
    /// - `bounding_box` — the area to report incidents for.
    ///
    /// # Returns
    /// Every incident the provider reports inside the area.
    ///
    /// # Errors
    /// Returns [`SDKError::NetworkRequestFailed`] when the request cannot
    /// reach the provider and [`SDKError::ResponseParsingFailure`] when the
    /// response cannot be decoded.
    async fn fetch_traffic_incidents(
        &self,
        bounding_box: &BoundingBox,
    ) -> Result<Vec<TrafficIncident>, SDKError>;
}

/// Observed traffic speeds.
#[async_trait]
pub trait TrafficFlowProvider: Send + Sync {
    /// Fetches per-segment flow observations inside a bounding box.
    ///
    /// # Parameters
    /// - `bounding_box` — the area to report flow for.
    ///
    /// # Returns
    /// One observation per segment the provider reports in the area.
    ///
    /// # Errors
    /// Returns [`SDKError::NetworkRequestFailed`] when the request cannot
    /// reach the provider and [`SDKError::ResponseParsingFailure`] when the
    /// response cannot be decoded.
    async fn fetch_traffic_flow(
        &self,
        bounding_box: &BoundingBox,
    ) -> Result<Vec<TrafficFlowData>, SDKError>;
}

/// Elevation sampling.
#[async_trait]
pub trait ElevationProvider: Send + Sync {
    /// Fetches elevation samples for ordered points.
    ///
    /// # Parameters
    /// - `points` — the coordinates to sample, in order.
    ///
    /// # Returns
    /// One elevation sample per requested point, in the same order.
    ///
    /// # Errors
    /// Returns [`SDKError::NetworkRequestFailed`] when the request cannot
    /// reach the provider and [`SDKError::ResponseParsingFailure`] when the
    /// response cannot be decoded.
    async fn fetch_elevation_points(
        &self,
        points: &[GeographicCoordinate],
    ) -> Result<Vec<ElevationPoint>, SDKError>;
}

/// Raster map imagery served as encoded image tiles.
#[async_trait]
pub trait RasterTileProvider: Send + Sync {
    /// Fetches one web-mercator XYZ raster tile at the requested resolution.
    ///
    /// # Parameters
    /// - `tile_x`, `tile_y` — the tile's column and row at `zoom_level`.
    /// - `zoom_level` — the web-mercator zoom level of the tile.
    /// - `image_format` — the image encoding the caller needs.
    /// - `tile_resolution` — standard or retina texel density.
    ///
    /// # Returns
    /// The encoded tile bytes together with the arguments they were fetched
    /// for.
    ///
    /// # Errors
    /// Returns [`SDKError::NetworkRequestFailed`] when the request cannot
    /// reach the provider, [`SDKError::ResponseParsingFailure`] when the
    /// response cannot be decoded, and [`SDKError::ProviderRequestRejected`]
    /// when the provider does not serve the requested format or resolution.
    async fn fetch_raster_tile(
        &self,
        tile_x: u32,
        tile_y: u32,
        zoom_level: u8,
        image_format: RasterImageFormat,
        tile_resolution: RasterTileResolution,
    ) -> Result<RasterTileData, SDKError>;
}

/// Source of the offline routing graph an [`OfflineRoutingProvider`] consumes.
pub enum OfflineGraphSource {
    /// Memory-mapped path on the native filesystem.
    FilePath(String),
    /// In-memory archive bytes, the source used on WebAssembly targets, where
    /// the browser has no memory mapping.
    Memory(Vec<u8>),
}

/// Offline route calculation over an embedded routing graph.
///
/// A provider holds at most one loaded graph. A successful load replaces the
/// previous graph; a failed load leaves the previous graph usable.
#[async_trait]
pub trait OfflineRoutingProvider: Send + Sync {
    /// Loads and validates the offline graph from the given source.
    ///
    /// # Parameters
    /// - `graph_source` — a filesystem path or in-memory archive bytes.
    ///
    /// # Returns
    /// `Ok(())` once the graph is validated and ready for routing.
    ///
    /// # Errors
    /// Returns [`SDKError::OfflineRoutingFailure`] when the archive is
    /// malformed, truncated or of an unsupported version, and
    /// [`SDKError::ProviderRequestRejected`] when the source cannot be used
    /// on the current target.
    async fn load_offline_graph(&self, graph_source: OfflineGraphSource) -> Result<(), SDKError>;

    /// Calculates a route over the loaded graph.
    ///
    /// # Parameters
    /// - `start`, `end` — the route endpoints.
    /// - `route_options` — transport mode, optimization strategy and units.
    ///
    /// # Returns
    /// A route between the two points, including geometry and steps.
    ///
    /// # Errors
    /// Returns [`SDKError::ProviderRequestRejected`] when no graph is loaded,
    /// when the mode is unsupported or when the graph cannot connect the
    /// points, and [`SDKError::InvalidCoordinateInput`] when an endpoint is
    /// too far from the nearest graph node.
    async fn calculate_offline_route(
        &self,
        start: &GeographicCoordinate,
        end: &GeographicCoordinate,
        route_options: &RouteOptions,
    ) -> Result<RouteData, SDKError>;
}

/// Description of one vector-tileset source a [`VectorTileProvider`] serves.
///
/// The source name namespaces every cached response for the tileset, so two
/// providers, or one provider serving two tilesets, never share cache entries
/// even for identical zoom/x/y arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorTileSourceDescriptor {
    /// Stable unique name of the tileset (for example
    /// `"provider-name/tileset-name"`), used as the cache namespace key.
    pub source_name: String,
    /// Human-readable attribution line the provider's usage terms require
    /// to be displayed wherever the imagery is shown.
    pub attribution_text: String,
    /// Lowest zoom level the provider serves.
    pub minimum_zoom_level: u8,
    /// Highest zoom level the provider serves.
    pub maximum_zoom_level: u8,
    /// Style names or families the tileset is designed to be rendered with,
    /// in the provider's preference order; empty when it declares none.
    ///
    /// The hints are the provider's own declaration of the tileset's schema
    /// or style family (for example `omv` and `base` for HERE's vector
    /// tileset), not a guess. Style selection treats them as advisory; a
    /// consumer verifying that a style document's declared source names this
    /// tileset uses them as additional served-tileset identities.
    pub style_hints: Vec<String>,
}

/// Vector map imagery served as Mapbox Vector Tile protobuf.
///
/// Each implementation serves one web-mercator XYZ tileset and reports its
/// cache namespace, attribution and zoom span through
/// [`Self::source_descriptor`].
#[async_trait]
pub trait VectorTileProvider: Send + Sync {
    /// Describes the served tileset: cache namespace, attribution and zoom
    /// span.
    fn source_descriptor(&self) -> VectorTileSourceDescriptor;

    /// Fetches one web-mercator XYZ vector tile in its wire encoding: MVT
    /// protobuf, optionally gzip-wrapped.
    ///
    /// # Parameters
    /// - `tile_x`, `tile_y` — the tile's column and row at `zoom_level`.
    /// - `zoom_level` — the web-mercator zoom level of the tile.
    ///
    /// # Returns
    /// The tile bytes together with the encoding they arrived in.
    ///
    /// # Errors
    /// Returns [`SDKError::NetworkRequestFailed`] when the request cannot
    /// reach the provider, [`SDKError::ResponseParsingFailure`] when the
    /// payload is empty or undecodable, and
    /// [`SDKError::ProviderRequestRejected`] when the tile is outside the
    /// served zoom span.
    async fn fetch_vector_tile(
        &self,
        tile_x: u32,
        tile_y: u32,
        zoom_level: u8,
    ) -> Result<VectorTileData, SDKError>;
}
