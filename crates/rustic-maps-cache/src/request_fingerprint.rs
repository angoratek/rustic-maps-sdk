//! Order-independent request fingerprinting.
//!
//! A [`RequestDescriptor`] captures everything that defines one cacheable
//! provider request: the [`ResponseCategory`] plus the normalized key-value
//! parameters. Its fingerprint is the SHA-256 of a canonical text form with
//! pairs sorted before hashing, so a request built with its parameters in a
//! different order reproduces the same fingerprint.

use crate::ResponseCategory;
use geospatial_types::GeographicCoordinate;
use sha2::{Digest, Sha256};

/// Field and record separators for the canonical fingerprint text; control
/// characters that cannot appear in parameter values.
const FIELD_SEPARATOR: char = '\u{1e}';
const RECORD_SEPARATOR: char = '\u{1f}';

/// Normalized, cacheable description of one provider request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestDescriptor {
    response_category: ResponseCategory,
    normalized_key_value_pairs: Vec<(String, String)>,
}

impl RequestDescriptor {
    /// Builds a descriptor from unordered pairs; they are sorted so the
    /// canonical form is order-independent.
    pub fn new(
        response_category: ResponseCategory,
        key_value_pairs: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        let mut normalized_key_value_pairs: Vec<(String, String)> = key_value_pairs
            .into_iter()
            .map(|(key, value)| (key.into(), value.into()))
            .collect();
        normalized_key_value_pairs.sort();
        normalized_key_value_pairs.dedup();
        Self {
            response_category,
            normalized_key_value_pairs,
        }
    }

    /// Fingerprint of the canonical text form: `"<category>:<sha256-hex>"`.
    ///
    /// The category prefix lets the SQLite store recover which kind of
    /// response an entry holds without a second column lookup.
    pub fn to_fingerprint(&self) -> String {
        let mut canonical_text = String::new();
        for (key, value) in &self.normalized_key_value_pairs {
            canonical_text.push(RECORD_SEPARATOR);
            canonical_text.push_str(key);
            canonical_text.push(FIELD_SEPARATOR);
            canonical_text.push_str(value);
        }
        let digest = Sha256::digest(canonical_text.as_bytes());
        let digest_hex = digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        format!("{}:{digest_hex}", self.response_category.storage_name())
    }
}

/// Formats a coordinate as a stable fingerprint component at ~1 cm precision.
pub fn coordinate_to_fingerprint_text(coordinate: &GeographicCoordinate) -> String {
    format!("{:.7},{:.7}", coordinate.latitude, coordinate.longitude)
}

#[cfg(test)]
mod tests {
    use super::{RequestDescriptor, coordinate_to_fingerprint_text};
    use crate::ResponseCategory::Geocoding;
    use geospatial_types::GeographicCoordinate;

    #[test]
    fn pair_order_does_not_change_the_fingerprint() {
        let ordered = RequestDescriptor::new(
            Geocoding,
            [("address_text", "Berlin"), ("unit_system", "metric")],
        );
        let reordered = RequestDescriptor::new(
            Geocoding,
            [("unit_system", "metric"), ("address_text", "Berlin")],
        );
        assert_eq!(ordered.to_fingerprint(), reordered.to_fingerprint());
    }

    #[test]
    fn category_and_value_changes_rehash() {
        let base = RequestDescriptor::new(Geocoding, [("address_text", "Berlin")]);
        let other_category = RequestDescriptor::new(
            crate::ResponseCategory::Routing,
            [("address_text", "Berlin")],
        );
        let other_value = RequestDescriptor::new(Geocoding, [("address_text", "Potsdam")]);
        let fingerprint = base.to_fingerprint();
        assert_eq!(fingerprint.len(), "geocoding:".len() + 64);
        assert!(fingerprint.starts_with("geocoding:"));
        assert_ne!(fingerprint, other_category.to_fingerprint());
        assert_ne!(fingerprint, other_value.to_fingerprint());
    }

    #[test]
    fn coordinates_format_stably() {
        let coordinate = GeographicCoordinate::from_degrees(38.5766123, -121.4944123).unwrap();
        assert_eq!(
            coordinate_to_fingerprint_text(&coordinate),
            "38.5766123,-121.4944123"
        );
        let shifted = GeographicCoordinate::from_degrees(38.5766123, -121.49441231).unwrap();
        assert_eq!(
            coordinate_to_fingerprint_text(&coordinate),
            coordinate_to_fingerprint_text(&shifted)
        );
    }
}
