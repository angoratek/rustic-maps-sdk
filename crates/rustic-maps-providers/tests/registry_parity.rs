//! Parity between the shared provider table and `platform/providers.json`.
//!
//! [`PROVIDER_ENTRIES`] is the one place a constructed provider's key, display
//! name, credential requirement and served contracts are declared; the native
//! catalog and the browser dispatch derive their rows from it. The registry
//! descriptors in `platform/providers.json` are what the reference
//! applications, the fixture server and the documentation read. This test
//! keeps the two descriptions of the same providers from drifting: a provider
//! added to one and not the other fails here.
//!
//! A registry descriptor that carries a `url_template` or a
//! `raster_url_template` names a tile address the layers control installs. It
//! is not a provider the shared table constructs, so it is compared only for
//! the absence of a registration alias. Every other descriptor must have a
//! table row, and every table row must have a descriptor.
//!
//! The registry is read from the repository root, two levels above this
//! crate. When that file is absent (an extracted copy of the crate), the test
//! prints one message and skips instead of failing, so the crate still tests
//! standalone.

use rustic_maps_providers::{PROVIDER_ENTRIES, entry_for_key};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// The registry's path relative to the crate root.
const REGISTRY_RELATIVE_PATH: &str = "../../platform/providers.json";

/// The registry's array of provider descriptors.
const PROVIDERS_FIELD: &str = "providers";
/// The field naming a descriptor's stable provider key.
const PROVIDER_KEY_FIELD: &str = "provider_key";
/// The field naming the display name a settings surface shows.
const DISPLAY_NAME_FIELD: &str = "display_name";
/// The field naming the credential a settings surface asks for.
const CREDENTIAL_LABEL_FIELD: &str = "credential_label";
/// The field listing the capabilities a descriptor serves.
const SERVED_CONTRACTS_FIELD: &str = "served_contracts";
/// The field naming the browser registration alias.
const WASM_REGISTRATION_FIELD: &str = "wasm_registration";

/// The descriptor fields that name a tile address rather than a constructed
/// provider.
const URL_TEMPLATE_FIELDS: [&str; 2] = ["url_template", "raster_url_template"];

/// Reads the registry's descriptors, or answers `None` when the registry is
/// absent and prints the one skip message.
fn registry_descriptors() -> Option<Vec<Value>> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(REGISTRY_RELATIVE_PATH);
    let registry_text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            println!(
                "skipped registry/catalog parity: {} is absent, so this is an extracted copy of the crate",
                path.display()
            );
            return None;
        }
        Err(error) => panic!("{} is unreadable: {error}", path.display()),
    };
    let registry: Value = serde_json::from_str(&registry_text)
        .unwrap_or_else(|error| panic!("{} is not valid JSON: {error}", path.display()));
    let descriptors = registry
        .get(PROVIDERS_FIELD)
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("{} carries no `{PROVIDERS_FIELD}` array", path.display()));
    Some(descriptors.clone())
}

/// The provider key a descriptor declares.
fn provider_key_of(descriptor: &Value) -> &str {
    descriptor
        .get(PROVIDER_KEY_FIELD)
        .and_then(Value::as_str)
        .unwrap_or_else(|| {
            panic!("every registry descriptor declares a string `{PROVIDER_KEY_FIELD}`")
        })
}

/// A descriptor's string field, named by its provider in the failure message.
fn string_field<'a>(descriptor: &'a Value, field: &str, provider_key: &str) -> &'a str {
    descriptor
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("provider `{provider_key}` declares no string `{field}`"))
}

/// The descriptors indexed by provider key, refusing a duplicated key.
fn descriptors_by_key(descriptors: &[Value]) -> BTreeMap<&str, &Value> {
    let mut by_key = BTreeMap::new();
    for descriptor in descriptors {
        let provider_key = provider_key_of(descriptor);
        assert!(
            by_key.insert(provider_key, descriptor).is_none(),
            "the registry declares `{provider_key}` more than once"
        );
    }
    by_key
}

/// The tile address a descriptor declares, when it declares one.
fn url_template(descriptor: &Value) -> Option<&str> {
    URL_TEMPLATE_FIELDS.iter().find_map(|field| {
        descriptor
            .get(*field)
            .and_then(Value::as_str)
            .filter(|template| !template.trim().is_empty())
    })
}

/// The descriptor of a table row, failing when the registry omits it.
fn descriptor_for<'a>(by_key: &'a BTreeMap<&str, &'a Value>, provider_key: &str) -> &'a Value {
    by_key.get(provider_key).copied().unwrap_or_else(|| {
        panic!(
            "provider `{provider_key}` is in the shared table but the registry has no descriptor"
        )
    })
}

#[test]
fn the_table_and_the_registry_declare_the_same_constructed_providers() {
    let Some(descriptors) = registry_descriptors() else {
        return;
    };
    let by_key = descriptors_by_key(&descriptors);
    let table_keys: BTreeSet<&str> = PROVIDER_ENTRIES
        .iter()
        .map(|entry| entry.provider_key)
        .collect();
    let constructed_registry_keys: BTreeSet<&str> = by_key
        .iter()
        .filter(|(_, descriptor)| url_template(descriptor).is_none())
        .map(|(provider_key, _)| *provider_key)
        .collect();
    let only_in_table: Vec<&str> = table_keys
        .difference(&constructed_registry_keys)
        .copied()
        .collect();
    let only_in_registry: Vec<&str> = constructed_registry_keys
        .difference(&table_keys)
        .copied()
        .collect();
    assert!(
        only_in_table.is_empty(),
        "the shared table constructs providers the registry does not describe: {only_in_table:?}"
    );
    assert!(
        only_in_registry.is_empty(),
        "the registry describes constructed providers the shared table does not carry: \
         {only_in_registry:?}"
    );
}

#[test]
fn every_descriptor_outside_the_table_is_a_url_template_source() {
    let Some(descriptors) = registry_descriptors() else {
        return;
    };
    let by_key = descriptors_by_key(&descriptors);
    for (provider_key, descriptor) in &by_key {
        if entry_for_key(provider_key).is_some() {
            continue;
        }
        assert!(
            url_template(descriptor).is_some(),
            "provider `{provider_key}` is in the registry but not the shared table and \
             declares no URL template; catalogue it in `PROVIDER_ENTRIES` or describe it \
             as a URL-template base map"
        );
    }
}

#[test]
fn every_descriptor_display_name_matches_its_table_row() {
    let Some(descriptors) = registry_descriptors() else {
        return;
    };
    let by_key = descriptors_by_key(&descriptors);
    for entry in PROVIDER_ENTRIES {
        let descriptor = descriptor_for(&by_key, entry.provider_key);
        let display_name = string_field(descriptor, DISPLAY_NAME_FIELD, entry.provider_key);
        assert_eq!(
            display_name, entry.display_name,
            "provider `{}`: the registry display name differs from the table's",
            entry.provider_key
        );
    }
}

#[test]
fn every_descriptor_serves_the_contracts_its_table_row_declares() {
    let Some(descriptors) = registry_descriptors() else {
        return;
    };
    let by_key = descriptors_by_key(&descriptors);
    for entry in PROVIDER_ENTRIES {
        let descriptor = descriptor_for(&by_key, entry.provider_key);
        let declared_contracts = descriptor
            .get(SERVED_CONTRACTS_FIELD)
            .and_then(Value::as_array)
            .unwrap_or_else(|| {
                panic!(
                    "provider `{}` declares no `{SERVED_CONTRACTS_FIELD}` array",
                    entry.provider_key
                )
            });
        let declared: Vec<&str> = declared_contracts
            .iter()
            .map(|contract| {
                contract.as_str().unwrap_or_else(|| {
                    panic!(
                        "provider `{}` lists a non-string served contract: {contract}",
                        entry.provider_key
                    )
                })
            })
            .collect();
        let expected: Vec<&str> = entry
            .served_contracts
            .iter()
            .map(|contract| contract.registry_name())
            .collect();
        let declared_set: BTreeSet<&str> = declared.iter().copied().collect();
        let expected_set: BTreeSet<&str> = expected.iter().copied().collect();
        if declared != expected && declared_set == expected_set {
            println!(
                "provider `{}`: `{SERVED_CONTRACTS_FIELD}` order differs (table {expected:?}, \
                 registry {declared:?}); the sets agree",
                entry.provider_key
            );
        }
        assert_eq!(
            declared_set, expected_set,
            "provider `{}`: the registry serves {declared:?} but the table declares {expected:?}",
            entry.provider_key
        );
    }
}

#[test]
fn a_descriptor_requires_a_credential_exactly_when_its_label_is_non_empty() {
    let Some(descriptors) = registry_descriptors() else {
        return;
    };
    let by_key = descriptors_by_key(&descriptors);
    for entry in PROVIDER_ENTRIES {
        let descriptor = descriptor_for(&by_key, entry.provider_key);
        let credential_label = string_field(descriptor, CREDENTIAL_LABEL_FIELD, entry.provider_key);
        assert_eq!(
            entry.credential_required,
            !credential_label.trim().is_empty(),
            "provider `{}`: the table declares credential_required={} but the registry \
             credential label is {credential_label:?}",
            entry.provider_key,
            entry.credential_required
        );
    }
}

#[test]
fn a_registration_key_is_present_exactly_for_constructed_providers() {
    let Some(descriptors) = registry_descriptors() else {
        return;
    };
    let by_key = descriptors_by_key(&descriptors);
    // The table carries no registrable flag: every row in it is constructible
    // and therefore registrable, so table membership is the registrable set.
    for entry in PROVIDER_ENTRIES {
        let descriptor = descriptor_for(&by_key, entry.provider_key);
        let registration_key = descriptor
            .get(WASM_REGISTRATION_FIELD)
            .and_then(Value::as_str)
            .unwrap_or_else(|| {
                panic!(
                    "provider `{}` is in the shared table but the registry declares no string \
                     `{WASM_REGISTRATION_FIELD}`",
                    entry.provider_key
                )
            });
        assert!(
            !registration_key.trim().is_empty(),
            "provider `{}`: the registry's `{WASM_REGISTRATION_FIELD}` is blank",
            entry.provider_key
        );
        assert_eq!(
            registration_key, entry.provider_key,
            "provider `{}`: the registration key must resolve the catalog row of the same name",
            entry.provider_key
        );
    }
    for (provider_key, descriptor) in &by_key {
        if url_template(descriptor).is_none() {
            continue;
        }
        let registration = descriptor.get(WASM_REGISTRATION_FIELD);
        assert!(
            registration.is_none_or(Value::is_null),
            "provider `{provider_key}` is a URL-template base map outside the shared table \
             and must not declare a `{WASM_REGISTRATION_FIELD}` alias (found {registration:?})"
        );
    }
}
