//! Conformance cases for legacy and expression filters.

use std::collections::BTreeMap;

use rustic_maps_style::{EvaluationContext, GeometryType, Value, parse_filter};
use serde_json::Value as JsonValue;

fn filter(filter_json: &str) -> rustic_maps_style::Expression {
    let json_value: JsonValue = serde_json::from_str(filter_json).expect("the filter is JSON");
    parse_filter(&json_value).expect("the filter normalizes")
}

fn feature(properties: &[(&str, Value)], _geometry_type: GeometryType) -> BTreeMap<String, Value> {
    properties
        .iter()
        .map(|(name, value)| ((*name).to_string(), value.clone()))
        .collect()
}

fn matches(filter_json: &str, properties: &[(&str, Value)], geometry_type: GeometryType) -> bool {
    let properties = feature(properties, geometry_type);
    let context = EvaluationContext::at_zoom(12.0).with_feature(&properties, geometry_type);
    filter(filter_json)
        .evaluate(&context)
        .expect("the filter evaluates")
        .truthy()
}

#[test]
fn legacy_equality_rewrites_a_bare_string_to_a_property_lookup() {
    assert!(matches(
        r#"["==", "class", "motorway"]"#,
        &[("class", Value::String("motorway".into()))],
        GeometryType::LineString
    ));
    assert!(!matches(
        r#"["==", "class", "motorway"]"#,
        &[("class", Value::String("primary".into()))],
        GeometryType::LineString
    ));
}

#[test]
fn modern_equality_keeps_the_explicit_get() {
    assert!(matches(
        r#"["==", ["get", "class"], "motorway"]"#,
        &[("class", Value::String("motorway".into()))],
        GeometryType::LineString
    ));
}

#[test]
fn legacy_in_and_not_in_match_any_listed_value() {
    let filter_json = r#"["in", "class", "motorway", "trunk", "primary"]"#;
    assert!(matches(
        filter_json,
        &[("class", Value::String("trunk".into()))],
        GeometryType::LineString
    ));
    assert!(!matches(
        filter_json,
        &[("class", Value::String("service".into()))],
        GeometryType::LineString
    ));

    let negated = r#"["!in", "class", "motorway", "trunk"]"#;
    assert!(matches(
        negated,
        &[("class", Value::String("service".into()))],
        GeometryType::LineString
    ));
    assert!(!matches(
        negated,
        &[("class", Value::String("trunk".into()))],
        GeometryType::LineString
    ));
}

#[test]
fn has_and_not_has_test_property_existence() {
    assert!(matches(
        r#"["has", "name"]"#,
        &[("name", Value::String("Sacramento".into()))],
        GeometryType::Point
    ));
    assert!(!matches(r#"["has", "name"]"#, &[], GeometryType::Point));
    assert!(matches(r#"["!has", "name"]"#, &[], GeometryType::Point));
}

#[test]
fn all_any_and_none_combine_filters() {
    let filter_json = r#"["all", ["==", "class", "motorway"], ["!=", "class", "rail"]]"#;
    assert!(matches(
        filter_json,
        &[("class", Value::String("motorway".into()))],
        GeometryType::LineString
    ));
    assert!(!matches(
        filter_json,
        &[("class", Value::String("primary".into()))],
        GeometryType::LineString
    ));

    let any_filter = r#"["any", ["==", "class", "motorway"], ["==", "class", "trunk"]]"#;
    assert!(matches(
        any_filter,
        &[("class", Value::String("trunk".into()))],
        GeometryType::LineString
    ));

    let none_filter = r#"["none", ["==", "class", "motorway"], ["==", "class", "trunk"]]"#;
    assert!(matches(
        none_filter,
        &[("class", Value::String("service".into()))],
        GeometryType::LineString
    ));
    assert!(!matches(
        none_filter,
        &[("class", Value::String("motorway".into()))],
        GeometryType::LineString
    ));

    // An empty `all` matches every feature, as the specification requires.
    assert!(matches(r#"["all"]"#, &[], GeometryType::Point));
}

#[test]
fn type_operand_compares_the_geometry_kind() {
    assert!(matches(
        r#"["==", "$type", "Polygon"]"#,
        &[],
        GeometryType::Polygon
    ));
    assert!(!matches(
        r#"["==", "$type", "Polygon"]"#,
        &[],
        GeometryType::LineString
    ));
    assert!(matches(
        r#"["in", "$type", "Point", "LineString"]"#,
        &[],
        GeometryType::LineString
    ));
}

#[test]
fn numeric_comparisons_are_numeric_not_lexical() {
    assert!(matches(
        r#"["<", "rank", 10]"#,
        &[("rank", Value::Number(2.0))],
        GeometryType::Point
    ));
    assert!(!matches(
        r#"["<", "rank", 10]"#,
        &[("rank", Value::Number(20.0))],
        GeometryType::Point
    ));
    assert!(matches(
        r#"["<=", "rank", 2]"#,
        &[("rank", Value::Number(2.0))],
        GeometryType::Point
    ));
}

#[test]
fn mixed_number_and_string_comparisons_are_false() {
    assert!(!matches(
        r#"["==", "rank", "2"]"#,
        &[("rank", Value::Number(2.0))],
        GeometryType::Point
    ));
}

#[test]
fn nested_modern_filters_parse() {
    assert!(matches(
        r#"["all", ["==", ["get", "class"], "motorway"], ["!", ["has", "ref"]]]"#,
        &[("class", Value::String("motorway".into()))],
        GeometryType::LineString
    ));
}
