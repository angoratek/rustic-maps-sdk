//! The one mapping between the shared provider registry's `served_contracts`
//! names and the contract's capability enum.
//!
//! The shared registry spells every capability in snake_case (`geocoding`,
//! `distance_matrix`, …) while the frozen contract names the same capabilities
//! as [`ProviderContract`] variants. The mapping lives here, in the contract,
//! and the platform surfaces resolve names through these two functions: the
//! Kotlin registration plan and descriptor accessors, and the Swift registry
//! catalog and registration path, whose generated bindings carry both calls.
//! Adding a capability to the registry therefore never requires a per-surface
//! table.
//!
//! [`ProviderContract::registry_name`] is the single table; both functions
//! read it, and the module's tests prove that every variant has exactly one
//! name and that parsing round-trips.

use crate::records::ProviderContract;

/// The capability contract a registry `served_contracts` entry denotes.
///
/// # Parameters
/// - `registry_name` — one entry of a provider descriptor's `served_contracts`
///   list, spelled as the shared registry spells it (`geocoding`,
///   `distance_matrix`, …).
///
/// # Returns
/// The matching [`ProviderContract`], or `None` when this build's contract
/// does not define the name. A registry artifact newer than the SDK may
/// declare a capability this build cannot serve; callers skip it rather than
/// refuse the whole descriptor.
///
/// # Errors
/// This call cannot fail.
pub fn provider_contract_from_registry_name(registry_name: String) -> Option<ProviderContract> {
    ProviderContract::ALL
        .into_iter()
        .find(|contract| contract.registry_name() == registry_name.as_str())
}

/// The shared registry's `served_contracts` name for a capability contract.
///
/// # Parameters
/// - `contract` — the capability to name.
///
/// # Returns
/// The registry's snake_case storage name (`geocoding`, `distance_matrix`, …),
/// matching the spelling the shared registry uses.
///
/// # Errors
/// This call cannot fail: every [`ProviderContract`] variant carries exactly
/// one registry name.
pub fn registry_name_for_provider_contract(contract: ProviderContract) -> String {
    contract.registry_name().to_owned()
}

#[cfg(test)]
mod tests {
    use super::{provider_contract_from_registry_name, registry_name_for_provider_contract};
    use crate::records::ProviderContract;

    /// The registry spellings the shared artifact declares, in the contract's
    /// stable order. The test compares the contract-owned names against this
    /// list, so a renamed variant or a typo in either direction fails here.
    const EXPECTED_REGISTRY_NAMES: [&str; ProviderContract::ALL.len()] = [
        "geocoding",
        "routing",
        "distance_matrix",
        "place_search",
        "traffic_incidents",
        "traffic_flow",
        "elevation",
        "raster_tiles",
        "vector_tiles",
        "offline_routing",
    ];

    #[test]
    fn every_contract_has_exactly_one_registry_name() {
        for (contract, expected_name) in ProviderContract::ALL
            .into_iter()
            .zip(EXPECTED_REGISTRY_NAMES)
        {
            let registry_name = registry_name_for_provider_contract(contract);
            assert_eq!(
                registry_name, expected_name,
                "{contract:?} must use the registry's spelling"
            );
            assert_eq!(
                EXPECTED_REGISTRY_NAMES
                    .iter()
                    .filter(|candidate| **candidate == expected_name)
                    .count(),
                1,
                "{expected_name:?} must name exactly one contract"
            );
        }
    }

    #[test]
    fn registry_names_round_trip_through_every_contract() {
        for contract in ProviderContract::ALL {
            let registry_name = registry_name_for_provider_contract(contract);
            assert_eq!(
                provider_contract_from_registry_name(registry_name.clone()),
                Some(contract),
                "{registry_name:?} must parse back to {contract:?}"
            );
        }
    }

    #[test]
    fn a_registry_name_the_contract_does_not_define_has_no_contract() {
        assert_eq!(
            provider_contract_from_registry_name("a-future-capability".to_owned()),
            None
        );
        assert_eq!(provider_contract_from_registry_name(String::new()), None);
        assert_eq!(
            provider_contract_from_registry_name("Geocoding".to_owned()),
            None
        );
    }
}
