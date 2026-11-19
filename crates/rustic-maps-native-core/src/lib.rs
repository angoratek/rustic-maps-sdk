//! The UniFFI binding crate of the SDK: objects, records, callback
//! interfaces and the UDL contract consumed by Kotlin, Swift and
//! `uniffi-bindgen-react-native`. The WebAssembly target uses a
//! hand-maintained TypeScript surface checked against the same UDL.
//!
//! The crate exposes the anchored [`camera::MapCamera`] and its free
//! [`projection`] helpers, the async provider surface over
//! [`provider::MapSdk`], the foreign-callback provider contracts
//! ([`foreign_providers`]), the navigation controller ([`navigation`]), the
//! map surface view and its platform pointer lowering ([`surface_view`],
//! [`platform_surface`]), the simulated-trip seam ([`simulation`]) and the
//! device-positioning controller ([`positioning`]). The diagnostics surface
//! ([`diagnostics`], [`provider_diagnostics`]) reports SDK log events and
//! failed foreign provider calls.
//!
//! The UDL dictionaries are mirrored by the value structs in [`records`]:
//! FFI traits cannot be implemented for foreign-crate types, so binding
//! records convert to and from the `geospatial-types` and `rustic-maps-rendering`
//! types through explicit constructors and the `projection` validation
//! helpers. [`error::SdkError`] mirrors the `geospatial-types` `SDKError`
//! subset these calls produce.
//!
//! Every exported async method hands reactor-dependent work (the `reqwest`
//! provider futures, the surface draw's tile fetches) to a crate-owned tokio
//! runtime through [`runtime::drive`], so a foreign executor polls only a
//! runtime-independent handback future.
//!
//! The engine facades ([`engines`]: `SearchEngine`, `RoutingEngine`,
//! `OfflineManager`, `Diagnostics`) construct typed capability objects over
//! an existing `MapSdk` and delegate every member to it. The offline routing
//! seam ([`offline_routing`]) exposes the contract's
//! `offline_route_from_archive_bytes` function: archive bytes in, a route
//! out, over the same `rustic-maps-core` implementation the WebAssembly export uses.
//!
//! Every exported entry point of this crate (and every callback invocation
//! inside an exported frame) is panic-contained by the UniFFI 0.31.2
//! scaffolding itself; [`crate::panic_boundary`] documents the containment
//! map, which is why this crate adds no redundant `catch_unwind` layer of
//! its own, and holds the crate's `unwrap`/`expect` policy for FFI-reachable
//! code.

// The scaffolding generated from the UDL builds its namespace metadata in
// large const arrays (uniffi 0.31's codegen pattern); the attribute cannot
// scope into the machine-generated macro expansion, so the lint is allowed
// crate-wide.
#![allow(clippy::large_const_arrays)]
// No `unwrap`/`expect` in FFI-reachable code: the deny is lifted for the
// test builds (the crate's own unit-test modules and the integration-test
// crates, which rustc also compiles with `cfg(test)`), where the
// conventional assertion shorthands are honest. The production denials are
// upheld by the invariants documented in [`crate::panic_boundary`], whose
// helpers hold the crate's only justifications for the remaining,
// deliberately panic-expressing sites.
#![cfg_attr(not(test), deny(clippy::expect_used, clippy::unwrap_used))]

// The wasm2 player's runtime helpers (`__ubrn_alloc`/`__ubrn_free` and the
// panic-hook installation). Nothing in this crate calls into them, so the
// dependency must be referenced for its side effects: without this line
// rustc drops the unused rlib and the player fails at init with
// `required export "__ubrn_alloc" not found in wasm module`.
#[cfg(target_arch = "wasm32")]
extern crate uniffi_runtime_wasm as _;

/// The hand-written browser canvas surface ([`rustic_maps_web_bindings`]),
/// folded into this crate's wasm module: one generated package then carries
/// both the generated contract surface and the canvas `wasm_bindgen` classes,
/// with one initialization and one provider registry. The re-export is what
/// keeps the dependency linked (and therefore its exports in the module);
/// native builds never compile this branch.
#[cfg(target_arch = "wasm32")]
pub use rustic_maps_web_bindings as browser_canvas;

/// The Android JNI seam of the shipped `MapView` (the surface-pointer shim),
/// compiled only for the Android target.
#[cfg(target_os = "android")]
pub mod android_jni;

pub mod camera;
pub mod diagnostics;
pub mod engines;
pub mod error;
pub mod foreign_providers;
pub mod navigation;
pub mod offline_routing;
pub mod panic_boundary;
/// The platform window-handle lowering onto `wgpu`'s unsafe surface target:
/// native-only, like the surface renderer it feeds.
#[cfg(not(target_arch = "wasm32"))]
pub mod platform_surface;
pub mod positioning;
pub mod projection;
pub mod provider;
pub mod provider_catalog;
pub mod provider_contract_names;
pub mod provider_diagnostics;
pub mod records;
pub mod runtime;
pub mod simulation;
/// The native surface view over `rustic-maps-rendering`'s surface renderer.
#[cfg(not(target_arch = "wasm32"))]
pub mod surface_view;

pub use crate::camera::MapCamera;
pub use crate::diagnostics::SdkDiagnosticEventListener;
pub use crate::engines::{
    Diagnostics, OfflineManager, RegionDownloadProgressListener, RoutingEngine, SearchEngine,
};
pub use crate::error::SdkError;
pub use crate::foreign_providers::{
    DistanceMatrixProvider, ElevationProvider, GeocodingProvider, RasterTileProvider,
    RoutingProvider, SearchPlaceProvider, TrafficFlowProvider, TrafficIncidentProvider,
    VectorTileProvider,
};
pub use crate::navigation::{
    NavigationListener, Navigator, Subscription, default_deviation_policy, default_reroute_policy,
    default_step_advance_policy,
};
pub use crate::offline_routing::offline_route_from_archive_bytes;
pub use crate::positioning::PositionController;
pub use crate::projection::{
    coordinate_to_normalized_world, tile_key_for_coordinate, tiles_covering,
};
pub use crate::provider::MapSdk;
pub use crate::provider_contract_names::{
    provider_contract_from_registry_name, registry_name_for_provider_contract,
};
pub use crate::records::{
    ActiveManeuverInstruction, AvoidOptions, BoundingBox, DeviationGradeKind, DeviationInfo,
    DeviationPolicy, DeviationPolicyKind, DeviationStatusChange, DeviationStatusChangeKind,
    DistanceMatrix, DistanceMatrixEntry, DistancePhrase, DistancePhraseKind, DistanceUnit,
    EdgeInsets, ElevationPoint, FrameStatistics, GeocodeQuality, GeographicCoordinate,
    LocationIndicatorState, LocationIndicatorStyle, LocationIndicatorVisualKind, LogEvent,
    ManeuverInstructionText, ManeuverInstructionTextKind, MapOverlay, MapOverlayKind,
    NavigationInstruction, NavigationLocationUpdate, NavigationPhase, NavigationPhaseChange,
    NormalizedWorldCoordinate, OverlayColor, Place, PositionFix, PositionSourceError,
    ProviderContract, ProviderFailureClassification, ProviderFailureDiagnostic,
    ProviderRegistration, ProviderRequestTrace, ProviderVectorTileSource, RasterImageFormat,
    RasterTileData, RasterTileResolution, RegionDefinition, RegionDownloadPhase,
    RegionDownloadProgress, RegionRecord, RegionRemovalReport, ReroutePolicy, Route,
    RouteCapabilities, RouteLeg, RouteOptions, RouteStep, RouteStepPosition, RouteStrategy,
    ScreenPoint, SdkLogSeverity, SimulatedTripFix, SimulatedTripPace, SourceHeader,
    SpeechInstruction, SpeechInstructionKind, StepAdvancePolicy, StepAdvancePolicyKind,
    SurfaceHandle, TileCacheStatistics, TileCacheTerms, TileKey, TrafficFlowCacheStatistics,
    TrafficFlowCongestionLevel, TrafficFlowData, TrafficIncident, TrafficIncidentSeverity,
    TrafficIncidentType, TransportationMode, TripNavigationState, TripProgress, UnitSystem,
    UpcomingManeuverInstruction, UrlTemplateRasterTileSource, UrlTemplateVectorTileSource,
    VectorTileData, VectorTileEncoding, VectorTileSourceDescriptor, VectorTileSourceLayerCount,
    VectorTileSourceStatus, ViewportSize,
};
pub use crate::simulation::RouteSimulator;
#[cfg(not(target_arch = "wasm32"))]
pub use crate::surface_view::MapView;

/// Version of the compiled SDK, equal to the workspace package version.
pub const SDK_COMPILED_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Version of the compiled SDK, surfaced for diagnostics, mirroring
/// `rustic-maps-web-bindings`' same-named export.
pub fn get_sdk_compiled_version() -> String {
    SDK_COMPILED_VERSION.to_string()
}

// Scaffolding generated at build time from the committed UDL. The contract is
// compiled on every target: natively from the frozen contract, and on wasm
// from `rustic_maps_wasm.udl`, which is the same contract without the native
// surface `MapView` interface (the browser owns the surface in the
// hand-written canvas class).
#[cfg(target_arch = "wasm32")]
uniffi::include_scaffolding!("rustic_maps_wasm");

#[cfg(not(target_arch = "wasm32"))]
uniffi::include_scaffolding!("rustic_maps");
