//! The rule deciding which `tracing` events the web diagnostics ring records.
//!
//! The ring exists to expose the SDK's own events; the page's code logs through
//! the same global subscriber, and a page-local event must never enter the
//! ring. The rule is a pure predicate so it is testable on every target, while
//! the ring itself (`diagnostics`) is built on browser-only APIs.

/// Module-path prefixes of the SDK crates whose events the ring records.
pub(crate) const SDK_MODULE_PATH_PREFIXES: [&str; 5] = [
    "rustic_maps_core",
    "rustic_maps_rendering",
    "rustic_maps_navigation",
    "rustic_maps_cache",
    "rustic_maps_web_bindings",
];

/// Whether an event from `module_path_or_target` at `severity_rank` passes the
/// ring's SDK-crate filter and severity gate.
///
/// The check is a prefix match on the module path, so the page's own modules
/// (`playground`, a host application) and any module that merely contains an
/// SDK crate name deeper in its path are excluded.
pub(crate) fn records_sdk_event(
    module_path_or_target: &str,
    severity_rank: u8,
    gate_rank: u8,
) -> bool {
    SDK_MODULE_PATH_PREFIXES
        .iter()
        .any(|prefix| module_path_or_target.starts_with(prefix))
        && severity_rank >= gate_rank
}

#[cfg(test)]
mod tests {
    use super::{SDK_MODULE_PATH_PREFIXES, records_sdk_event};

    /// The rank [`records_sdk_event`] receives when the gate admits Info.
    const INFO_GATE_RANK: u8 = 3;

    #[test]
    fn only_sdk_crate_events_enter_the_ring() {
        for sdk_module_path in [
            "rustic_maps_core::provider_observation",
            "rustic_maps_rendering::tile_scheduler",
            "rustic_maps_web_bindings::canvas",
        ] {
            assert!(
                records_sdk_event(sdk_module_path, INFO_GATE_RANK, INFO_GATE_RANK),
                "{sdk_module_path} is an SDK event"
            );
        }
        for page_module_path in [
            "playground",
            "playground::provider_console",
            "my_app::rustic_maps_core",
            "",
        ] {
            assert!(
                !records_sdk_event(page_module_path, INFO_GATE_RANK, INFO_GATE_RANK),
                "{page_module_path:?} is not an SDK event"
            );
        }
    }

    #[test]
    fn the_severity_gate_still_admits_only_what_it_is_lowered_to() {
        assert!(!records_sdk_event(
            "rustic_maps_core::provider_observation",
            INFO_GATE_RANK - 1,
            INFO_GATE_RANK
        ));
        assert!(SDK_MODULE_PATH_PREFIXES.contains(&"rustic_maps_core"));
    }
}
