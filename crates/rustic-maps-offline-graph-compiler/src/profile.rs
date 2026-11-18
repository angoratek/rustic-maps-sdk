//! Minimal OpenStreetMap routing profile:
//! way filtering (drivable highway classes), per-class default speeds,
//! optional `maxspeed` overrides, oneway direction and access blocking.
//!
//! The profile is deliberately minimal, growing incrementally as routing needs
//! arise; a data-driven configuration with penalty groups (the OsmAnd
//! `routing.xml` pattern) would only extend this table.

use crate::archive::{
    EdgeProperties, RestrictionKind, RoadClass, SPEED_CELL_LIMIT, SPEED_QUANTUM_KILOMETERS_PER_HOUR,
};

/// Highest speed the packed `encoded_value` column can store exactly.
const MAX_STORABLE_SPEED_KILOMETERS_PER_HOUR: u16 =
    SPEED_CELL_LIMIT * SPEED_QUANTUM_KILOMETERS_PER_HOUR;

/// Default free-flow speed per drivable class, km/h. All values are
/// multiples of [`crate::archive::SPEED_QUANTUM_KILOMETERS_PER_HOUR`] so the
/// packed speed column stores them exactly.
const MOTORWAY_DEFAULT_SPEED_KILOMETERS_PER_HOUR: u16 = 120;
const TRUNK_DEFAULT_SPEED_KILOMETERS_PER_HOUR: u16 = 100;
const PRIMARY_DEFAULT_SPEED_KILOMETERS_PER_HOUR: u16 = 80;
const SECONDARY_DEFAULT_SPEED_KILOMETERS_PER_HOUR: u16 = 60;
const TERTIARY_DEFAULT_SPEED_KILOMETERS_PER_HOUR: u16 = 45;
const UNCLASSIFIED_DEFAULT_SPEED_KILOMETERS_PER_HOUR: u16 = 40;
const RESIDENTIAL_DEFAULT_SPEED_KILOMETERS_PER_HOUR: u16 = 30;
const SERVICE_DEFAULT_SPEED_KILOMETERS_PER_HOUR: u16 = 15;

/// `maxspeed` values below this floor are treated as unparseable (e.g. the
/// "0" placeholder value) and fall back to the class default.
const POSTED_SPEED_MINIMUM_KILOMETERS_PER_HOUR: u16 = 5;

/// Which directions exist along a way's node order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TravelDirection {
    /// Only along the node order (e.g. `oneway=yes`).
    ForwardOnly,
    /// Only against the node order (e.g. `oneway=-1`).
    BackwardOnly,
    /// Both directions.
    Both,
}

/// Maps an OSM `highway` value to its road class and default speed, or
/// `None` when the way is not drivable under this profile.
///
/// `living_street` borrows the residential class at residential speed, and
/// all `*_link` ways inherit their parent class at parent-class speed —
/// two documented simplifications until link-road penalties land.
pub(crate) fn road_class_and_default_speed_for_highway(
    highway_value: &str,
) -> Option<(RoadClass, u16)> {
    match highway_value {
        "motorway" | "motorway_link" => Some((
            RoadClass::Motorway,
            MOTORWAY_DEFAULT_SPEED_KILOMETERS_PER_HOUR,
        )),
        "trunk" | "trunk_link" => Some((RoadClass::Trunk, TRUNK_DEFAULT_SPEED_KILOMETERS_PER_HOUR)),
        "primary" | "primary_link" => Some((
            RoadClass::Primary,
            PRIMARY_DEFAULT_SPEED_KILOMETERS_PER_HOUR,
        )),
        "secondary" | "secondary_link" => Some((
            RoadClass::Secondary,
            SECONDARY_DEFAULT_SPEED_KILOMETERS_PER_HOUR,
        )),
        "tertiary" | "tertiary_link" => Some((
            RoadClass::Tertiary,
            TERTIARY_DEFAULT_SPEED_KILOMETERS_PER_HOUR,
        )),
        "unclassified" => Some((
            RoadClass::Unclassified,
            UNCLASSIFIED_DEFAULT_SPEED_KILOMETERS_PER_HOUR,
        )),
        "residential" | "living_street" => Some((
            RoadClass::Residential,
            RESIDENTIAL_DEFAULT_SPEED_KILOMETERS_PER_HOUR,
        )),
        "service" => Some((
            RoadClass::Service,
            SERVICE_DEFAULT_SPEED_KILOMETERS_PER_HOUR,
        )),
        _ => None,
    }
}

/// Whether an `access` tag blocks routing on the way entirely
/// (private/`no`); other access restrictions are not enforced.
pub(crate) fn is_blocked_by_access(access_value: Option<&str>) -> bool {
    matches!(access_value, Some("no") | Some("private"))
}

/// Maps an `oneway` tag value to the travel directions that exist, defaulting
/// to both directions when the tag is absent or empty. Roundabout and other
/// implied-oneway rules are not applied.
pub(crate) fn travel_direction_for_oneway_tag(oneway_value: Option<&str>) -> TravelDirection {
    match oneway_value {
        Some("yes") | Some("1") | Some("true") => TravelDirection::ForwardOnly,
        Some("-1") | Some("reverse") => TravelDirection::BackwardOnly,
        _ => TravelDirection::Both,
    }
}

/// Parses a plain-integer km/h `maxspeed` override; anything syntactically
/// richer (locale values, zones, "none") falls back to the class default.
pub(crate) fn posted_speed_override_kilometers_per_hour(
    maxspeed_value: Option<&str>,
) -> Option<u16> {
    let parsed = maxspeed_value?.parse::<u16>().ok()?;
    if !(POSTED_SPEED_MINIMUM_KILOMETERS_PER_HOUR..=MAX_STORABLE_SPEED_KILOMETERS_PER_HOUR)
        .contains(&parsed)
    {
        return None;
    }
    Some(parsed)
}

/// Maps an OSM `restriction` relation value to its kind; unknown values
/// (turn:restriction expressions, conditional restrictions) return `None`
/// and are skipped by the compiler.
pub(crate) fn restriction_kind_for_openstreetmap_value(
    restriction_value: &str,
) -> Option<RestrictionKind> {
    match restriction_value {
        "no_left_turn" => Some(RestrictionKind::NoLeftTurn),
        "no_right_turn" => Some(RestrictionKind::NoRightTurn),
        "no_u_turn" => Some(RestrictionKind::NoUTurn),
        "no_straight_on" => Some(RestrictionKind::NoStraightOn),
        "only_straight_on" => Some(RestrictionKind::OnlyStraightOn),
        _ => None,
    }
}

/// Precomputed instruction text per restriction kind (the compiler emits one
/// phrase per recorded restriction).
pub(crate) fn instruction_phrase_for_restriction_kind(
    restriction_kind: RestrictionKind,
) -> &'static str {
    match restriction_kind {
        RestrictionKind::NoLeftTurn => "No left turn",
        RestrictionKind::NoRightTurn => "No right turn",
        RestrictionKind::NoUTurn => "No U-turn",
        RestrictionKind::NoStraightOn => "No straight on",
        RestrictionKind::OnlyStraightOn => "Only straight on",
    }
}

/// Builds the edge properties for one emitted directed edge: its access
/// bits record which of the way's two node-order directions this record
/// covers (every emitted record is a traversable direction, so exactly one
/// bit is set).
pub(crate) fn edge_properties_for_direction(
    road_class: RoadClass,
    speed_kilometers_per_hour: u16,
    emits_forward_direction: bool,
) -> EdgeProperties {
    EdgeProperties {
        road_class,
        speed_kilometers_per_hour,
        forward_allowed: emits_forward_direction,
        backward_allowed: !emits_forward_direction,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drivable_highway_values_map_to_named_classes() {
        assert_eq!(
            road_class_and_default_speed_for_highway("motorway"),
            Some((RoadClass::Motorway, 120))
        );
        assert_eq!(
            road_class_and_default_speed_for_highway("residential"),
            Some((RoadClass::Residential, 30))
        );
        assert_eq!(
            road_class_and_default_speed_for_highway("living_street"),
            Some((RoadClass::Residential, 30))
        );
        assert_eq!(road_class_and_default_speed_for_highway("footway"), None);
    }

    #[test]
    fn oneway_values_resolve_travel_direction() {
        assert_eq!(
            travel_direction_for_oneway_tag(Some("yes")),
            TravelDirection::ForwardOnly
        );
        assert_eq!(
            travel_direction_for_oneway_tag(Some("-1")),
            TravelDirection::BackwardOnly
        );
        assert_eq!(
            travel_direction_for_oneway_tag(Some("no")),
            TravelDirection::Both
        );
        assert_eq!(travel_direction_for_oneway_tag(None), TravelDirection::Both);
    }

    #[test]
    fn maxspeed_parsing_accepts_plain_integers_only() {
        assert_eq!(
            posted_speed_override_kilometers_per_hour(Some("50")),
            Some(50)
        );
        assert_eq!(
            posted_speed_override_kilometers_per_hour(Some("RU:urban")),
            None
        );
        assert_eq!(
            posted_speed_override_kilometers_per_hour(Some("none")),
            None
        );
        assert_eq!(posted_speed_override_kilometers_per_hour(Some("3")), None);
        assert_eq!(posted_speed_override_kilometers_per_hour(None), None);
    }

    #[test]
    fn direction_access_bits_record_the_covered_direction() {
        let forward_edge = edge_properties_for_direction(RoadClass::Residential, 30, true);
        let backward_edge = edge_properties_for_direction(RoadClass::Residential, 30, false);
        assert!(forward_edge.forward_allowed);
        assert!(!forward_edge.backward_allowed);
        assert!(!backward_edge.forward_allowed);
        assert!(backward_edge.backward_allowed);
    }
}
