//! The provider-failure diagnostic seam: the bounded per-`MapSdk` ring
//! [`ProviderFailureDiagnosticsRing`] recording every failed
//! foreign-callback provider call, read back through
//! `MapSdk::latest_provider_failure_diagnostics`.
//!
//! The write sites are the [`crate::foreign_providers`] adapters — the only
//! boundary that sees both what the foreign implementation threw and the
//! contract and method it occurred on. When the failure came from the
//! unclassified-callback conversion, the adapter classifies it (through the
//! crate's `SdkError::unclassified_foreign_callback_reason`) and records the
//! callback machinery's verbatim reason; otherwise the failure was a typed
//! `SdkError` round trip and the entry carries its verbatim display form.
//! Nothing is parsed or invented beyond that text; the UDL documentation
//! states the limitation.
//!
//! The ring is per `MapSdk` (one registration passes the owning object's
//! ring into the adapters, mirroring how the adapters wrap exactly the
//! callbacks registered on it), bounded so a systematically failing foreign
//! provider cannot grow the process's memory unboundedly, and read-only from
//! the foreign side: the readback neither records nor clears.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::records::{ProviderFailureClassification, ProviderFailureDiagnostic};

/// Capacity of a [`ProviderFailureDiagnosticsRing`], in entries (bounded so
/// a systematically failing foreign provider cannot grow the process's
/// memory unboundedly; the newest failures are what integrators classify).
pub(crate) const PROVIDER_FAILURE_DIAGNOSTIC_RING_CAPACITY: usize = 32;

/// The bounded ring of provider-failure diagnostics of one `MapSdk`: every
/// failed foreign-callback provider call registered on the object lands
/// here, and `MapSdk::latest_provider_failure_diagnostics` reads it back.
#[derive(Debug)]
pub struct ProviderFailureDiagnosticsRing {
    ring: Mutex<VecDeque<ProviderFailureDiagnostic>>,
}

impl ProviderFailureDiagnosticsRing {
    /// Builds the empty ring.
    pub(crate) fn new() -> Self {
        Self {
            ring: Mutex::new(VecDeque::with_capacity(
                PROVIDER_FAILURE_DIAGNOSTIC_RING_CAPACITY,
            )),
        }
    }

    /// Records one failed foreign-callback provider call (ring retention: at
    /// [`PROVIDER_FAILURE_DIAGNOSTIC_RING_CAPACITY`] the oldest entry
    /// drops).
    pub(crate) fn record(
        &self,
        provider_contract_name: &str,
        provider_method_name: &str,
        failure_classification: ProviderFailureClassification,
        failure_detail: String,
    ) {
        let diagnostic = ProviderFailureDiagnostic {
            timestamp_milliseconds_since_epoch: crate::diagnostics::unix_epoch_milliseconds(),
            failure_classification,
            provider_contract_name: provider_contract_name.to_string(),
            provider_method_name: provider_method_name.to_string(),
            failure_detail,
        };
        let mut ring = crate::panic_boundary::state_lock_guard(
            &self.ring,
            "the provider-failure diagnostics ring",
        );
        if ring.len() >= PROVIDER_FAILURE_DIAGNOSTIC_RING_CAPACITY {
            ring.pop_front();
        }
        ring.push_back(diagnostic);
    }

    /// Reads the retained diagnostics, oldest first; the readback is
    /// non-destructive (a later read returns the same entries, plus any
    /// recorded since).
    pub(crate) fn latest(&self) -> Vec<ProviderFailureDiagnostic> {
        let ring = crate::panic_boundary::state_lock_guard(
            &self.ring,
            "the provider-failure diagnostics ring",
        );
        ring.iter().cloned().collect()
    }
}

impl Default for ProviderFailureDiagnosticsRing {
    /// Mirrors the inline `ProviderFailureDiagnosticsRing::new` constructor
    /// (the clippy `new_without_default` convention).
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience alias for the shared-ring handle the registration paths pass
/// into the foreign-callback adapters.
pub(crate) type SharedProviderFailureDiagnostics = Arc<ProviderFailureDiagnosticsRing>;

#[cfg(test)]
mod tests {
    use super::*;

    /// A failure detail the recording tests use.
    const TEST_FAILURE_DETAIL: &str = "the foreign geocoder rejected the request";

    /// Records `count` entries of the given classification into `ring`,
    /// each with its index as the detail suffix.
    fn record_test_entries(
        ring: &ProviderFailureDiagnosticsRing,
        count: usize,
        classification: ProviderFailureClassification,
    ) {
        for index in 0..count {
            ring.record(
                "GeocodingProvider",
                "forward_geocode",
                classification,
                format!("{TEST_FAILURE_DETAIL} {index}"),
            );
        }
    }

    /// The ring reads back oldest first and is non-destructive.
    #[test]
    fn ring_reads_back_oldest_first_and_is_non_destructive() {
        let ring = ProviderFailureDiagnosticsRing::new();
        record_test_entries(&ring, 3, ProviderFailureClassification::MappedContractError);
        let first_read = ring.latest();
        let read_details: Vec<&str> = first_read
            .iter()
            .map(|diagnostic| diagnostic.failure_detail.as_str())
            .collect();
        assert_eq!(
            read_details,
            [
                "the foreign geocoder rejected the request 0",
                "the foreign geocoder rejected the request 1",
                "the foreign geocoder rejected the request 2",
            ],
            "entries read back in recording order"
        );
        let second_read = ring.latest();
        assert_eq!(
            second_read, first_read,
            "the readback never clears the ring"
        );
    }

    /// The ring rotates at its named capacity: at capacity the oldest entry
    /// drops, so exactly the newest [`PROVIDER_FAILURE_DIAGNOSTIC_RING_CAPACITY`]
    /// entries are retained.
    #[test]
    fn ring_rotates_at_capacity_keeping_the_newest_entries() {
        let ring = ProviderFailureDiagnosticsRing::new();
        let over_capacity_count = PROVIDER_FAILURE_DIAGNOSTIC_RING_CAPACITY + 1;
        record_test_entries(
            &ring,
            over_capacity_count,
            ProviderFailureClassification::UnclassifiedCallbackFailure,
        );
        let retained = ring.latest();
        assert_eq!(retained.len(), PROVIDER_FAILURE_DIAGNOSTIC_RING_CAPACITY);
        assert_eq!(
            retained[0].failure_detail,
            format!("{TEST_FAILURE_DETAIL} 1"),
            "the oldest entry (index 0) dropped at capacity"
        );
        assert_eq!(
            retained
                .last()
                .expect("the ring holds capacity entries")
                .failure_detail,
            format!(
                "{TEST_FAILURE_DETAIL} {over_capacity_count_minus_one}",
                over_capacity_count_minus_one = over_capacity_count - 1
            ),
            "the newest entry is retained"
        );
        for diagnostic in &retained {
            assert_eq!(
                diagnostic.failure_classification,
                ProviderFailureClassification::UnclassifiedCallbackFailure,
            );
        }
    }

    /// The recorded fields carry the contract and method names verbatim
    /// alongside the classification.
    #[test]
    fn recorded_entries_carry_contract_and_method_names() {
        let ring = ProviderFailureDiagnosticsRing::new();
        ring.record(
            "RasterTileProvider",
            "fetch_raster_tile",
            ProviderFailureClassification::MappedContractError,
            TEST_FAILURE_DETAIL.to_string(),
        );
        let diagnostic = ring.latest().pop().expect("one recorded entry");
        assert_eq!(diagnostic.provider_contract_name, "RasterTileProvider");
        assert_eq!(diagnostic.provider_method_name, "fetch_raster_tile");
        assert_eq!(diagnostic.failure_detail, TEST_FAILURE_DETAIL);
    }
}
