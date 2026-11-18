//! The shared provider registry names committed vector styles; this test keeps
//! every descriptor honest against the renderer that has to paint it.
//!
//! The registry (`platform/providers.json`) is the single source of a
//! provider's identity, templates and styles (`platform/providers.json`).
//! The reference applications build their base-map menus directly from it, so
//! a typo here is a menu row that installs nothing:
//!
//! - a `vector_styles` entry names either a style artifact this crate can
//!   load or a style document the provider publishes, and in that case the
//!   address is a credential-free `https` URL with no query; an entry must
//!   carry a display name and declare a real theme;
//! - every tile template must be a `{z}/{x}/{y}` template (the URL-template
//!   providers substitute exactly those placeholders);
//! - every descriptor that serves the `vector_tiles` contract offers at least
//!   one named style, because the native menus offer only `offersVectorBase`
//!   descriptors (a vector source plus a non-empty style list) — a schema-only
//!   vector descriptor would silently disappear from Android, iOS and React
//!   Native while still working on web.

use std::path::{Path, PathBuf};

use rustic_maps_rendering::vector_style::vector_tile_style_from_id;
use serde_json::Value;

/// The repository's shared provider registry.
fn registry_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("platform")
        .join("providers.json")
}

/// The parsed registry document.
fn registry_document() -> Value {
    let registry_path = registry_path();
    let registry_text = std::fs::read_to_string(&registry_path).unwrap_or_else(|read_error| {
        panic!(
            "the shared provider registry {} could not be read: {read_error}",
            registry_path.display()
        )
    });
    serde_json::from_str(&registry_text)
        .unwrap_or_else(|parse_error| panic!("the registry is not valid JSON: {parse_error}"))
}

/// The registry's provider descriptors.
fn registry_providers() -> Vec<Value> {
    registry_document()
        .get("providers")
        .and_then(Value::as_array)
        .cloned()
        .expect("the registry carries a providers array")
}

/// One provider's string field, trimmed, or the empty string.
fn string_field(provider: &Value, field_name: &str) -> String {
    provider
        .get(field_name)
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

/// Asserts one tile template substitutes the three slippy-map placeholders.
fn assert_tile_template(provider: &Value, field_name: &str) {
    let template = string_field(provider, field_name);
    if template.is_empty() {
        return;
    }
    for placeholder in ["{z}", "{x}", "{y}"] {
        assert!(
            template.contains(placeholder),
            "{} declares a {field_name} without the {placeholder} placeholder: {template}",
            string_field(provider, "provider_key"),
        );
    }
}

#[test]
fn every_vector_style_names_a_committed_style_or_a_provider_document() {
    for provider in registry_providers() {
        let provider_key = string_field(&provider, "provider_key");
        let Some(styles) = provider.get("vector_styles").and_then(Value::as_array) else {
            continue;
        };
        for style in styles {
            let style_id = style.get("style_id").and_then(Value::as_str).unwrap_or("");
            assert!(
                !style_id.is_empty(),
                "{provider_key} declares a vector style without a style_id",
            );
            let document_url = style
                .get("document_url")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            if document_url.is_empty() {
                vector_tile_style_from_id(style_id).unwrap_or_else(|style_error| {
                    panic!(
                        "{provider_key} names {style_id:?} with no document_url, and this \
                         renderer cannot load that committed style: {style_error}"
                    )
                });
            } else {
                assert!(
                    document_url.starts_with("https://"),
                    "{provider_key}'s style {style_id} names document_url {document_url:?}; \
                     hosted style documents are fetched over https",
                );
                assert!(
                    !document_url.contains('?'),
                    "{provider_key}'s style {style_id} names document_url {document_url:?} \
                     with a query; the provider client appends the credential, so a query \
                     here could carry one into the registry",
                );
            }
            let display_name = style
                .get("display_name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            assert!(
                !display_name.is_empty(),
                "{provider_key}'s style {style_id} has no consumer-facing display name",
            );
            let theme = style.get("theme").and_then(Value::as_str).unwrap_or("");
            assert!(
                theme == "light" || theme == "dark",
                "{provider_key}'s style {style_id} declares theme {theme:?}; expected light or dark",
            );
            let attribution = style
                .get("attribution_text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            assert!(
                !attribution.is_empty(),
                "{provider_key}'s style {style_id} carries no attribution line",
            );
        }
    }
}

#[test]
fn every_vector_descriptor_offers_a_named_style() {
    for provider in registry_providers() {
        let provider_key = string_field(&provider, "provider_key");
        let serves_vector_tiles = provider
            .get("served_contracts")
            .and_then(Value::as_array)
            .is_some_and(|contracts| {
                contracts
                    .iter()
                    .any(|contract| contract.as_str() == Some("vector_tiles"))
            });
        if string_field(&provider, "url_template").is_empty() && !serves_vector_tiles {
            continue;
        }
        let offered_styles = provider
            .get("vector_styles")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        assert!(
            offered_styles > 0,
            "{provider_key} serves vector tiles but declares no vector_styles, so \
             the native base-map menus would not offer it",
        );
    }
}

#[test]
fn the_keyless_base_map_names_an_offered_vector_style() {
    let registry = registry_document();
    let declared = registry
        .get("keyless_base_map")
        .expect("the registry declares the keyless start's styled base (keyless_base_map)");
    let provider_key = declared
        .get("provider_key")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let style_id = declared
        .get("style_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    assert!(
        !provider_key.is_empty(),
        "the keyless base map declaration names no provider_key",
    );
    assert!(
        !style_id.is_empty(),
        "the keyless base map declaration names no style_id",
    );
    let provider = registry_providers()
        .into_iter()
        .find(|provider| string_field(provider, "provider_key") == provider_key)
        .unwrap_or_else(|| {
            panic!(
                "the keyless base map names provider {provider_key:?}, which the registry does not declare"
            )
        });
    assert!(
        !string_field(&provider, "url_template").is_empty(),
        "{provider_key} is the keyless base map provider but publishes no vector tile template",
    );
    let offers_style = provider
        .get("vector_styles")
        .and_then(Value::as_array)
        .is_some_and(|styles| {
            styles
                .iter()
                .any(|style| style.get("style_id").and_then(Value::as_str) == Some(style_id))
        });
    assert!(
        offers_style,
        "{provider_key} is the keyless base map provider but does not offer the declared style {style_id:?}",
    );
    vector_tile_style_from_id(style_id).unwrap_or_else(|style_error| {
        panic!(
            "the keyless base map names {style_id:?}, which this renderer cannot load: {style_error}"
        )
    });
}

#[test]
fn every_tile_template_is_a_slippy_map_template() {
    for provider in registry_providers() {
        assert_tile_template(&provider, "url_template");
        assert_tile_template(&provider, "raster_url_template");
    }
}

#[test]
fn every_provider_declares_identity_and_attribution() {
    for provider in registry_providers() {
        let provider_key = string_field(&provider, "provider_key");
        assert!(!provider_key.is_empty(), "a provider has no provider_key");
        assert!(
            !string_field(&provider, "display_name").is_empty(),
            "{provider_key} has no display name",
        );
        assert!(
            !string_field(&provider, "attribution_text").is_empty(),
            "{provider_key} has no attribution line",
        );
        let serves_something = !string_field(&provider, "url_template").is_empty()
            || !string_field(&provider, "raster_url_template").is_empty()
            || provider
                .get("registration_available")
                .and_then(Value::as_object)
                .is_some_and(|platforms| {
                    platforms
                        .values()
                        .any(|availability| availability.as_bool() == Some(true))
                });
        assert!(
            serves_something,
            "{provider_key} declares neither a tile template nor a credential registration",
        );
    }
}
