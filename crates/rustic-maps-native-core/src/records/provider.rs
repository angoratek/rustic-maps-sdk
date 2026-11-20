//! Provider capability contracts and the registration readback.

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
    /// The capability's name in the shared registry's `served_contracts`
    /// list. This match is the one place the registry spelling lives; the
    /// contract's parser and accessor (`crate::provider_contract_names`)
    /// derive from it.
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
