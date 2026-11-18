//! Optional declaration fields of a browser URL-template vector source.
//!
//! The canvas reads the source object's lower-camel fields, normalizes each
//! value into [`SourceFieldValue`] and validates the optional terms,
//! subdomains, headers and style hints here. Keeping the validation and the
//! resulting provider declaration in one host-testable place means the
//! browser seam and its tests share one source of truth.
//!
//! A field that is absent, `undefined` or `null` declares nothing; a present
//! value of the wrong shape is refused rather than partly dropped. Unlike the
//! native surface, the browser path does not consult the provider layer's
//! named keyless declarations, so an omitted field stays empty even when the
//! template matches one of them.

use everymap_core::domains::tiling::{VectorTileEncoding, VectorTileSourceDescriptor};
use everymap_providers_url_template_vector::{TileSourcePacing, UrlTemplateVectorTileSource};

use crate::generated_contract_field_names::{
    SOURCE_HEADER_FIELD_NAME, SOURCE_HEADER_FIELD_VALUE, VECTOR_SOURCE_FIELD_HEADERS,
    VECTOR_SOURCE_FIELD_STYLE_HINTS, VECTOR_SOURCE_FIELD_SUBDOMAINS,
    VECTOR_SOURCE_FIELD_TERMS_TEXT,
};

/// The absent-field value the lookups answer for a name the object lacks, so
/// no caller allocates a temporary.
const ABSENT_SOURCE_FIELD_VALUE: SourceFieldValue = SourceFieldValue::Absent;

/// One raw JavaScript field value, normalized for host-testable validation.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SourceFieldValue {
    /// The field is absent (`undefined` or `null`).
    Absent,
    /// A JavaScript string.
    Text(String),
    /// A JavaScript array, in order.
    List(Vec<SourceFieldValue>),
    /// A plain JavaScript object's named fields.
    Object(Vec<(String, SourceFieldValue)>),
    /// Any other JavaScript value, which the contract has no field for.
    Other,
}

impl SourceFieldValue {
    /// Reads one named field of an object value; a non-object or a missing
    /// name answers [`SourceFieldValue::Absent`].
    fn field(&self, field_name: &str) -> &SourceFieldValue {
        match self {
            SourceFieldValue::Object(fields) => fields
                .iter()
                .find(|(name, _)| name == field_name)
                .map(|(_, value)| value)
                .unwrap_or(&ABSENT_SOURCE_FIELD_VALUE),
            _ => &ABSENT_SOURCE_FIELD_VALUE,
        }
    }
}

/// The optional extras one URL-template vector source declares. `None` means
/// the field was absent; the provider declaration turns it into an empty
/// value.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct VectorSourceExtras {
    /// Terms the tileset's usage policy requires alongside attribution.
    pub(crate) terms_text: Option<String>,
    /// Values substituted for the template's optional `{s}` placeholder.
    pub(crate) subdomains: Option<Vec<String>>,
    /// Headers every tile request carries, as name-value pairs.
    pub(crate) headers: Option<Vec<(String, String)>>,
    /// Style names or families the tileset is designed to render with.
    pub(crate) style_hints: Option<Vec<String>>,
}

/// The values one URL-template vector source declaration is built from.
pub(crate) struct VectorSourceDeclarationFields {
    /// The `{z}/{x}/{y}` template the tileset publishes.
    pub(crate) url_template: String,
    /// The tileset's cache key name.
    pub(crate) source_name: String,
    /// The attribution line the tileset's terms require.
    pub(crate) attribution_text: String,
    /// Lowest zoom level the tileset serves.
    pub(crate) minimum_zoom_level: u8,
    /// Highest zoom level the tileset serves.
    pub(crate) maximum_zoom_level: u8,
    /// The optional declaration extras, validated from the source object.
    pub(crate) extras: VectorSourceExtras,
    /// The request budget the source declares.
    pub(crate) pacing: Option<TileSourcePacing>,
}

/// Validates the optional fields of one source object. Every malformed entry
/// names its field and position, so the browser seam can refuse with the
/// invalid-argument code instead of dropping the value.
pub(crate) fn vector_source_extras(
    source_fields: &SourceFieldValue,
) -> Result<VectorSourceExtras, String> {
    Ok(VectorSourceExtras {
        terms_text: optional_string(
            source_fields.field(VECTOR_SOURCE_FIELD_TERMS_TEXT),
            VECTOR_SOURCE_FIELD_TERMS_TEXT,
        )?,
        subdomains: optional_string_list(
            source_fields.field(VECTOR_SOURCE_FIELD_SUBDOMAINS),
            VECTOR_SOURCE_FIELD_SUBDOMAINS,
        )?,
        headers: optional_header_list(
            source_fields.field(VECTOR_SOURCE_FIELD_HEADERS),
            VECTOR_SOURCE_FIELD_HEADERS,
        )?,
        style_hints: optional_string_list(
            source_fields.field(VECTOR_SOURCE_FIELD_STYLE_HINTS),
            VECTOR_SOURCE_FIELD_STYLE_HINTS,
        )?,
    })
}

/// Builds the provider layer's declaration from the validated source fields.
pub(crate) fn url_template_vector_source(
    fields: VectorSourceDeclarationFields,
) -> UrlTemplateVectorTileSource {
    UrlTemplateVectorTileSource {
        url_template: fields.url_template,
        source_descriptor: VectorTileSourceDescriptor {
            source_name: fields.source_name,
            attribution_text: fields.attribution_text,
            minimum_zoom_level: fields.minimum_zoom_level,
            maximum_zoom_level: fields.maximum_zoom_level,
            // A URL template does not declare its wire encoding; the response
            // sniffs it, so the descriptor names plain MVT and the fetched
            // tile reports what actually arrived.
            tile_encoding: VectorTileEncoding::MapboxVectorTile,
            style_hints: fields.extras.style_hints.unwrap_or_default(),
        },
        terms_text: fields.extras.terms_text.unwrap_or_default(),
        subdomains: fields.extras.subdomains.unwrap_or_default(),
        headers: fields.extras.headers.unwrap_or_default(),
        pacing: fields.pacing,
    }
}

/// Reads one optional string field; an absent field is `None`.
fn optional_string(
    field_value: &SourceFieldValue,
    field_name: &str,
) -> Result<Option<String>, String> {
    match field_value {
        SourceFieldValue::Absent => Ok(None),
        SourceFieldValue::Text(text) => Ok(Some(text.clone())),
        _ => Err(format!("the field {field_name} must be a string")),
    }
}

/// Reads one optional array of strings; an absent field is `None`. Every entry
/// must be a string, so a malformed list is refused rather than partly kept.
fn optional_string_list(
    field_value: &SourceFieldValue,
    field_name: &str,
) -> Result<Option<Vec<String>>, String> {
    let SourceFieldValue::List(entries) = field_value else {
        return match field_value {
            SourceFieldValue::Absent => Ok(None),
            _ => Err(format!(
                "the field {field_name} must be an array of strings"
            )),
        };
    };
    let mut values = Vec::with_capacity(entries.len());
    for (entry_index, entry_value) in entries.iter().enumerate() {
        values.push(match entry_value {
            SourceFieldValue::Text(value) => value.clone(),
            _ => {
                return Err(format!(
                    "entry {entry_index} of {field_name} must be a string"
                ));
            }
        });
    }
    Ok(Some(values))
}

/// Reads one optional array of `{name, value}` header objects; an absent field
/// is `None`. Both members must be strings, so a malformed entry is refused
/// rather than dropped.
fn optional_header_list(
    field_value: &SourceFieldValue,
    field_name: &str,
) -> Result<Option<Vec<(String, String)>>, String> {
    let SourceFieldValue::List(entries) = field_value else {
        return match field_value {
            SourceFieldValue::Absent => Ok(None),
            _ => Err(format!(
                "the field {field_name} must be an array of header objects"
            )),
        };
    };
    let mut headers = Vec::with_capacity(entries.len());
    for (entry_index, entry_value) in entries.iter().enumerate() {
        let SourceFieldValue::Object(_) = entry_value else {
            return Err(format!(
                "entry {entry_index} of {field_name} must be a header object"
            ));
        };
        let header_name = required_string(
            entry_value.field(SOURCE_HEADER_FIELD_NAME),
            SOURCE_HEADER_FIELD_NAME,
        )
        .map_err(|failure_message| {
            format!("entry {entry_index} of {field_name}: {failure_message}")
        })?;
        let header_value = required_string(
            entry_value.field(SOURCE_HEADER_FIELD_VALUE),
            SOURCE_HEADER_FIELD_VALUE,
        )
        .map_err(|failure_message| {
            format!("entry {entry_index} of {field_name}: {failure_message}")
        })?;
        headers.push((header_name, header_value));
    }
    Ok(Some(headers))
}

/// Reads one required string member of a header object.
fn required_string(field_value: &SourceFieldValue, field_name: &str) -> Result<String, String> {
    match field_value {
        SourceFieldValue::Text(text) => Ok(text.clone()),
        _ => Err(format!("the field {field_name} must be a string")),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        SourceFieldValue, VectorSourceDeclarationFields, url_template_vector_source,
        vector_source_extras,
    };
    use crate::generated_contract_field_names::{
        SOURCE_HEADER_FIELD_NAME, SOURCE_HEADER_FIELD_VALUE, VECTOR_SOURCE_FIELD_HEADERS,
        VECTOR_SOURCE_FIELD_STYLE_HINTS, VECTOR_SOURCE_FIELD_SUBDOMAINS,
        VECTOR_SOURCE_FIELD_TERMS_TEXT,
    };

    /// A neutral, valid template the declaration builder keeps verbatim.
    const TEST_VECTOR_URL_TEMPLATE: &str = "https://tiles.example.com/{z}/{x}/{y}.pbf";
    /// The tileset name the test declaration carries.
    const TEST_VECTOR_SOURCE_NAME: &str = "test/source";
    /// The attribution line the test declaration carries.
    const TEST_VECTOR_ATTRIBUTION_TEXT: &str = "Test vector tiles";

    /// Builds one source object value from its named optional fields.
    fn source_fields(entries: &[(&str, SourceFieldValue)]) -> SourceFieldValue {
        SourceFieldValue::Object(
            entries
                .iter()
                .map(|(name, value)| ((*name).to_string(), value.clone()))
                .collect(),
        )
    }

    /// Builds the provider declaration from validated extras.
    fn declaration_with_extras(
        url_template: &str,
        extras: super::VectorSourceExtras,
    ) -> everymap_providers_url_template_vector::UrlTemplateVectorTileSource {
        url_template_vector_source(VectorSourceDeclarationFields {
            url_template: url_template.to_string(),
            source_name: TEST_VECTOR_SOURCE_NAME.to_string(),
            attribution_text: TEST_VECTOR_ATTRIBUTION_TEXT.to_string(),
            minimum_zoom_level: 0,
            maximum_zoom_level: 14,
            extras,
            pacing: None,
        })
    }

    #[test]
    fn explicit_source_fields_reach_the_provider_declaration() {
        let extras = vector_source_extras(&source_fields(&[
            (
                VECTOR_SOURCE_FIELD_TERMS_TEXT,
                SourceFieldValue::Text("terms".to_string()),
            ),
            (
                VECTOR_SOURCE_FIELD_SUBDOMAINS,
                SourceFieldValue::List(vec![
                    SourceFieldValue::Text("a".to_string()),
                    SourceFieldValue::Text("b".to_string()),
                ]),
            ),
            (
                VECTOR_SOURCE_FIELD_HEADERS,
                SourceFieldValue::List(vec![SourceFieldValue::Object(vec![
                    (
                        SOURCE_HEADER_FIELD_NAME.to_string(),
                        SourceFieldValue::Text("User-Agent".to_string()),
                    ),
                    (
                        SOURCE_HEADER_FIELD_VALUE.to_string(),
                        SourceFieldValue::Text("test-agent".to_string()),
                    ),
                ])]),
            ),
            (
                VECTOR_SOURCE_FIELD_STYLE_HINTS,
                SourceFieldValue::List(vec![SourceFieldValue::Text("openmaptiles".to_string())]),
            ),
        ]))
        .expect("the explicit fields are well formed");
        let declaration = declaration_with_extras(TEST_VECTOR_URL_TEMPLATE, extras);
        assert_eq!(declaration.terms_text, "terms");
        assert_eq!(
            declaration.subdomains,
            vec!["a".to_string(), "b".to_string()]
        );
        assert_eq!(
            declaration.headers,
            vec![("User-Agent".to_string(), "test-agent".to_string())]
        );
        assert_eq!(
            declaration.source_descriptor.style_hints,
            vec!["openmaptiles".to_string()]
        );
    }

    #[test]
    fn absent_or_null_source_fields_declare_nothing() {
        for source_fields_value in [
            SourceFieldValue::Object(Vec::new()),
            source_fields(&[
                (VECTOR_SOURCE_FIELD_TERMS_TEXT, SourceFieldValue::Absent),
                (VECTOR_SOURCE_FIELD_SUBDOMAINS, SourceFieldValue::Absent),
                (VECTOR_SOURCE_FIELD_HEADERS, SourceFieldValue::Absent),
                (VECTOR_SOURCE_FIELD_STYLE_HINTS, SourceFieldValue::Absent),
            ]),
        ] {
            let extras = vector_source_extras(&source_fields_value)
                .expect("an absent field declares nothing");
            let declaration = declaration_with_extras(TEST_VECTOR_URL_TEMPLATE, extras);
            assert!(declaration.terms_text.is_empty());
            assert!(declaration.subdomains.is_empty());
            assert!(declaration.headers.is_empty());
            assert!(declaration.source_descriptor.style_hints.is_empty());
        }
    }

    #[test]
    fn a_browser_source_inherits_nothing_from_a_named_keyless_declaration() {
        // The registry is read at test time so this test follows the shared
        // provider declaration rather than naming a provider URL itself.
        let registry_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../platform/providers.json");
        let registry_text =
            std::fs::read_to_string(registry_path).expect("the provider registry is readable");
        let registry: serde_json::Value =
            serde_json::from_str(&registry_text).expect("the provider registry is valid JSON");
        let keyless_provider_key = registry["keyless_base_map"]["provider_key"]
            .as_str()
            .expect("the registry names a keyless base provider");
        let keyless_url_template = registry["providers"]
            .as_array()
            .expect("the registry lists providers")
            .iter()
            .find(|provider| provider["provider_key"] == keyless_provider_key)
            .expect("the registry lists the keyless base provider")["url_template"]
            .as_str()
            .expect("the keyless declaration carries a URL template")
            .to_string();
        // The native inheritance rule matches this template against the
        // provider layer's named declarations and inherits their extras; the
        // native records tests pin that match, so the browser assertion below
        // is not vacuous.

        // The browser builder never consults the provider layer's named
        // declarations: an empty source object declares nothing.
        let extras = vector_source_extras(&SourceFieldValue::Object(Vec::new()))
            .expect("an empty source object declares nothing");
        let browser_declaration = declaration_with_extras(&keyless_url_template, extras);
        assert!(
            browser_declaration.terms_text.is_empty(),
            "the browser does not inherit the named declaration's terms"
        );
        assert!(
            browser_declaration.subdomains.is_empty(),
            "the browser does not inherit the named declaration's subdomains"
        );
        assert!(
            browser_declaration.headers.is_empty(),
            "the browser does not inherit the named declaration's headers"
        );
        assert!(
            browser_declaration.source_descriptor.style_hints.is_empty(),
            "the browser does not inherit the named declaration's style hints"
        );

        // A header the caller declares explicitly is carried into the builder.
        let explicit_extras = vector_source_extras(&source_fields(&[(
            VECTOR_SOURCE_FIELD_HEADERS,
            SourceFieldValue::List(vec![SourceFieldValue::Object(vec![
                (
                    SOURCE_HEADER_FIELD_NAME.to_string(),
                    SourceFieldValue::Text("X-Test-Header".to_string()),
                ),
                (
                    SOURCE_HEADER_FIELD_VALUE.to_string(),
                    SourceFieldValue::Text("test-value".to_string()),
                ),
            ])]),
        )]))
        .expect("the explicit header is well formed");
        let explicit_declaration = declaration_with_extras(&keyless_url_template, explicit_extras);
        assert_eq!(
            explicit_declaration.headers,
            vec![("X-Test-Header".to_string(), "test-value".to_string())],
            "a caller-declared header reaches the browser provider builder"
        );
    }

    #[test]
    fn malformed_source_fields_are_refused_with_a_named_message() {
        let non_string_entry = vector_source_extras(&source_fields(&[(
            VECTOR_SOURCE_FIELD_SUBDOMAINS,
            SourceFieldValue::List(vec![
                SourceFieldValue::Text("a".to_string()),
                SourceFieldValue::Other,
            ]),
        )]))
        .expect_err("a non-string list entry must refuse");
        assert!(non_string_entry.contains("entry 1 of subdomains must be a string"));

        let non_list = vector_source_extras(&source_fields(&[(
            VECTOR_SOURCE_FIELD_SUBDOMAINS,
            SourceFieldValue::Text("not-a-list".to_string()),
        )]))
        .expect_err("a non-array field must refuse");
        assert!(non_list.contains("must be an array of strings"));

        let missing_header_name = vector_source_extras(&source_fields(&[(
            VECTOR_SOURCE_FIELD_HEADERS,
            SourceFieldValue::List(vec![SourceFieldValue::Object(vec![(
                SOURCE_HEADER_FIELD_VALUE.to_string(),
                SourceFieldValue::Text("test-agent".to_string()),
            )])]),
        )]))
        .expect_err("a header without a name must refuse");
        assert!(missing_header_name.contains("name must be a string"));

        let non_string_header_value = vector_source_extras(&source_fields(&[(
            VECTOR_SOURCE_FIELD_HEADERS,
            SourceFieldValue::List(vec![SourceFieldValue::Object(vec![
                (
                    SOURCE_HEADER_FIELD_NAME.to_string(),
                    SourceFieldValue::Text("User-Agent".to_string()),
                ),
                (
                    SOURCE_HEADER_FIELD_VALUE.to_string(),
                    SourceFieldValue::Other,
                ),
            ])]),
        )]))
        .expect_err("a non-string header value must refuse");
        assert!(non_string_header_value.contains("value must be a string"));

        let non_object_header = vector_source_extras(&source_fields(&[(
            VECTOR_SOURCE_FIELD_HEADERS,
            SourceFieldValue::List(vec![SourceFieldValue::Text("not-an-object".to_string())]),
        )]))
        .expect_err("a non-object header entry must refuse");
        assert!(non_object_header.contains("must be a header object"));
    }
}
