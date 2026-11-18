//! Place and geocoding result records.

use crate::coordinate::GeographicCoordinate;
use serde::{Deserialize, Serialize};

/// Structural precision a geocoder reached for one resolved location.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeocodeQuality {
    /// Rooftop or feature point.
    Point,
    /// Street address matched to the building level.
    StreetAddress,
    /// Street intersection.
    Intersection,
    /// Street level without house number.
    Street,
    /// Postal code area.
    PostalCode,
    /// Settlement level (village, town, city).
    City,
    /// County or administrative district.
    County,
    /// First-level administrative division.
    State,
    /// Sovereign country level.
    Country,
}

/// One resolved location returned by forward or reverse geocoding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaceResult {
    /// Display string the provider produced for this place.
    pub structured_address_text: String,
    /// Resolved position.
    pub coordinate: GeographicCoordinate,
    /// Structural precision reached for this resolution.
    pub geocode_quality: GeocodeQuality,
}
