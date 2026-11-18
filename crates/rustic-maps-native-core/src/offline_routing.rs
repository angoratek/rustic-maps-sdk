//! The offline routing graph seam of the native contract: archive bytes in,
//! a route out, over the same `rustic-maps-core` engine the wasm surface uses.
//!
//! The contract member exists because the offline routing capability must be
//! reachable from Kotlin and Swift, not only from the browser free function:
//! a compiled `RMOG` archive arrives as a byte buffer (a `ByteArray` on
//! Kotlin, `Data` on Swift), exactly the in-memory shape the browser already
//! carries, because every target shares one implementation. The provider
//! registry is not involved: the archive is self-contained and the route is
//! computed over the bytes alone.

use crate::error::SdkError;
use crate::projection::validated_coordinate;
use crate::records::{GeographicCoordinate, Route};
use geospatial_traits::{OfflineGraphSource, OfflineRoutingProvider as _};
use geospatial_types::route::RouteOptions;

/// Routes between two coordinates over an offline routing graph loaded from
/// in-memory archive bytes: the native counterpart of the wasm
/// `offlineRouteFromArchiveBytes` export, and the contract's
/// `offline_route_from_archive_bytes` free helper.
///
/// Both surfaces delegate to
/// [`rustic_maps_core::offline_routing::ArchiveOfflineRoutingProvider`],
/// so the answer cannot drift between native and browser. The request uses
/// the graph profile's default options (automobile, fastest, metric); the
/// archive is validated in memory before routing. The archive validation and
/// the search run on the crate-owned runtime's blocking pool, so neither
/// occupies a driver thread. Failures are typed:
/// [`SdkError::OfflineRoutingFailure`] for an archive that fails validation,
/// [`SdkError::InvalidCoordinateInput`] for a coordinate beyond the graph's
/// snap tolerance, and [`SdkError::ProviderRequestRejected`] for a pair the
/// graph cannot connect.
pub async fn offline_route_from_archive_bytes(
    archive_bytes: Vec<u8>,
    start: GeographicCoordinate,
    end: GeographicCoordinate,
) -> Result<Route, SdkError> {
    let native_start = validated_coordinate(start)?;
    let native_end = validated_coordinate(end)?;
    let offline_provider = rustic_maps_core::offline_routing::ArchiveOfflineRoutingProvider::new()
        .with_blocking_work_executor(crate::runtime::blocking_work_executor());
    offline_provider
        .load_offline_graph(OfflineGraphSource::Memory(archive_bytes))
        .await
        .map_err(SdkError::from)?;
    let native_route = offline_provider
        .calculate_offline_route(&native_start, &native_end, &RouteOptions::default())
        .await
        .map_err(SdkError::from)?;
    Ok(Route::from_native_route_data(&native_route))
}
