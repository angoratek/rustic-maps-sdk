//! Geocoded places and their precision classification.

use super::GeographicCoordinate;

/// Structural precision a geocoder reached for one resolved location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

impl GeocodeQuality {
    /// Builds the mirror enum from the native geocode quality.
    pub(crate) fn from_native_geocode_quality(
        native_geocode_quality: geospatial_types::GeocodeQuality,
    ) -> Self {
        match native_geocode_quality {
            geospatial_types::GeocodeQuality::Point => Self::Point,
            geospatial_types::GeocodeQuality::StreetAddress => Self::StreetAddress,
            geospatial_types::GeocodeQuality::Intersection => Self::Intersection,
            geospatial_types::GeocodeQuality::Street => Self::Street,
            geospatial_types::GeocodeQuality::PostalCode => Self::PostalCode,
            geospatial_types::GeocodeQuality::City => Self::City,
            geospatial_types::GeocodeQuality::County => Self::County,
            geospatial_types::GeocodeQuality::State => Self::State,
            geospatial_types::GeocodeQuality::Country => Self::Country,
        }
    }

    /// Builds the native geocode quality from the mirror enum.
    pub(crate) fn to_native_geocode_quality(self) -> geospatial_types::GeocodeQuality {
        match self {
            Self::Point => geospatial_types::GeocodeQuality::Point,
            Self::StreetAddress => geospatial_types::GeocodeQuality::StreetAddress,
            Self::Intersection => geospatial_types::GeocodeQuality::Intersection,
            Self::Street => geospatial_types::GeocodeQuality::Street,
            Self::PostalCode => geospatial_types::GeocodeQuality::PostalCode,
            Self::City => geospatial_types::GeocodeQuality::City,
            Self::County => geospatial_types::GeocodeQuality::County,
            Self::State => geospatial_types::GeocodeQuality::State,
            Self::Country => geospatial_types::GeocodeQuality::Country,
        }
    }
}

/// One resolved location returned by forward or reverse geocoding.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    /// Display string the provider produced for this place.
    pub structured_address_text: String,
    /// Resolved position.
    pub coordinate: GeographicCoordinate,
    /// Structural precision reached for this resolution.
    pub geocode_quality: GeocodeQuality,
}

impl Place {
    /// Builds the mirror record from a native place result.
    pub(crate) fn from_native_place_result(
        native_place_result: &geospatial_types::PlaceResult,
    ) -> Self {
        Self {
            structured_address_text: native_place_result.structured_address_text.clone(),
            coordinate: GeographicCoordinate::from_native_coordinate(
                &native_place_result.coordinate,
            ),
            geocode_quality: GeocodeQuality::from_native_geocode_quality(
                native_place_result.geocode_quality,
            ),
        }
    }

    /// Builds the native place result from the mirror record, consuming it
    /// (the fields move without a copy).
    pub(crate) fn into_native_place_result(self) -> geospatial_types::PlaceResult {
        geospatial_types::PlaceResult {
            structured_address_text: self.structured_address_text,
            coordinate: self.coordinate.to_native_coordinate(),
            geocode_quality: self.geocode_quality.to_native_geocode_quality(),
        }
    }
}
