//! Conformance cases for the typed expression engine: literals, property
//! access, zoom functions, interpolation curves, legacy stops normalization,
//! comparisons, boolean logic, arithmetic, strings and colours.

use std::collections::BTreeMap;

use rustic_maps_style::{Color, EvaluationContext, Expression, GeometryType, Value};

fn parse(expression_json: &str) -> Expression {
    Expression::parse_str(expression_json).expect("the expression parses")
}

fn evaluate(expression_json: &str, context: &EvaluationContext<'_>) -> Value {
    parse(expression_json)
        .evaluate(context)
        .expect("the expression evaluates")
}

fn feature(properties: &[(&str, Value)], _geometry_type: GeometryType) -> BTreeMap<String, Value> {
    properties
        .iter()
        .map(|(name, value)| ((*name).to_string(), value.clone()))
        .collect()
}

#[test]
fn scalar_and_literal_expressions() {
    let context = EvaluationContext::at_zoom(0.0);
    assert_eq!(evaluate("42", &context), Value::Number(42.0));
    assert_eq!(evaluate("\"map\"", &context), Value::String("map".into()));
    assert_eq!(evaluate("true", &context), Value::Boolean(true));
    assert_eq!(
        evaluate(r#"["literal", [1, 2]]"#, &context),
        Value::Array(vec![Value::Number(1.0), Value::Number(2.0)])
    );
}

#[test]
fn property_access_and_existence() {
    let properties = feature(
        &[
            ("class", Value::String("motorway".into())),
            ("rank", Value::Number(2.0)),
        ],
        GeometryType::LineString,
    );
    let context =
        EvaluationContext::at_zoom(10.0).with_feature(&properties, GeometryType::LineString);
    assert_eq!(
        evaluate(r#"["get", "class"]"#, &context),
        Value::String("motorway".into())
    );
    assert_eq!(evaluate(r#"["get", "missing"]"#, &context), Value::Null);
    assert_eq!(
        evaluate(r#"["has", "rank"]"#, &context),
        Value::Boolean(true)
    );
    assert_eq!(
        evaluate(r#"["has", "missing"]"#, &context),
        Value::Boolean(false)
    );
    assert_eq!(
        evaluate(r#"["geometry-type"]"#, &context),
        Value::String("LineString".into())
    );
    assert_eq!(evaluate(r#"["zoom"]"#, &context), Value::Number(10.0));
}

#[test]
fn step_selects_the_greatest_stop_at_or_below_the_input() {
    let expression = r#"["step", ["zoom"], 1, 5, 2, 10, 3]"#;
    assert_eq!(
        evaluate(expression, &EvaluationContext::at_zoom(0.0)),
        Value::Number(1.0)
    );
    assert_eq!(
        evaluate(expression, &EvaluationContext::at_zoom(5.0)),
        Value::Number(2.0)
    );
    assert_eq!(
        evaluate(expression, &EvaluationContext::at_zoom(9.9)),
        Value::Number(2.0)
    );
    assert_eq!(
        evaluate(expression, &EvaluationContext::at_zoom(10.0)),
        Value::Number(3.0)
    );
    assert_eq!(
        evaluate(expression, &EvaluationContext::at_zoom(20.0)),
        Value::Number(3.0)
    );
}

#[test]
fn linear_interpolation_clamps_and_blends() {
    let expression = r#"["interpolate", ["linear"], ["zoom"], 0, 2, 6, 6, 14, 9]"#;
    assert_eq!(
        evaluate(expression, &EvaluationContext::at_zoom(-1.0)),
        Value::Number(2.0)
    );
    assert_eq!(
        evaluate(expression, &EvaluationContext::at_zoom(3.0)),
        Value::Number(4.0)
    );
    assert_eq!(
        evaluate(expression, &EvaluationContext::at_zoom(6.0)),
        Value::Number(6.0)
    );
    assert_eq!(
        evaluate(expression, &EvaluationContext::at_zoom(20.0)),
        Value::Number(9.0)
    );
}

#[test]
fn exponential_interpolation_uses_the_specification_formula() {
    let expression = r#"["interpolate", ["exponential", 2], ["zoom"], 0, 0, 10, 1023]"#;
    let Value::Number(result) = evaluate(expression, &EvaluationContext::at_zoom(2.0)) else {
        panic!("interpolation produces a number");
    };
    let expected = (2.0_f64.powf(2.0) - 1.0) / (2.0_f64.powf(10.0) - 1.0) * 1023.0;
    assert!((result - expected).abs() < 1e-9);
}

#[test]
fn colour_interpolation_is_channel_wise_srgb() {
    let expression = r##"["interpolate", ["linear"], ["zoom"], 0, ["literal", "#000000"], 10, ["literal", "#ffffff"]]"##;
    let Value::Color(color) = evaluate(expression, &EvaluationContext::at_zoom(5.0)) else {
        panic!("interpolation produces a colour");
    };
    assert!((color.red - 0.5).abs() < 1e-9);
    assert!((color.green - 0.5).abs() < 1e-9);
    assert!((color.blue - 0.5).abs() < 1e-9);
}

#[test]
fn legacy_exponential_stops_interpolate_by_zoom() {
    let expression = parse(r#"{"stops": [[0, 2], [6, 6]]}"#);
    assert_eq!(
        expression
            .evaluate(&EvaluationContext::at_zoom(3.0))
            .expect("evaluates"),
        Value::Number(4.0)
    );
}

#[test]
fn legacy_interval_stops_step_by_zoom() {
    let expression = parse(r#"{"type": "interval", "stops": [[0, "a"], [2, "b"]]}"#);
    assert_eq!(
        expression
            .evaluate(&EvaluationContext::at_zoom(1.0))
            .expect("evaluates"),
        Value::String("a".into())
    );
    assert_eq!(
        expression
            .evaluate(&EvaluationContext::at_zoom(2.0))
            .expect("evaluates"),
        Value::String("b".into())
    );
}

#[test]
fn legacy_string_outputs_step_rather_than_interpolate() {
    let expression = parse(r#"{"stops": [[2, "{ABBREV}"], [4, "{NAME}"]]}"#);
    assert_eq!(
        expression
            .evaluate(&EvaluationContext::at_zoom(2.0))
            .expect("evaluates"),
        Value::String("{ABBREV}".into())
    );
    assert_eq!(
        expression
            .evaluate(&EvaluationContext::at_zoom(5.0))
            .expect("evaluates"),
        Value::String("{NAME}".into())
    );
}

#[test]
fn legacy_categorical_stops_match_a_property() {
    let expression = parse(
        r#"{"property": "class", "type": "categorical", "stops": [["a", 1], ["b", 2]], "default": 0}"#,
    );
    let properties = feature(&[("class", Value::String("b".into()))], GeometryType::Point);
    let context = EvaluationContext::at_zoom(10.0).with_feature(&properties, GeometryType::Point);
    assert_eq!(
        expression.evaluate(&context).expect("evaluates"),
        Value::Number(2.0)
    );

    let unknown = feature(&[("class", Value::String("z".into()))], GeometryType::Point);
    let unknown_context =
        EvaluationContext::at_zoom(10.0).with_feature(&unknown, GeometryType::Point);
    assert_eq!(
        expression.evaluate(&unknown_context).expect("evaluates"),
        Value::Number(0.0)
    );
}

#[test]
fn comparisons_follow_the_specification_coercion_rules() {
    let properties = feature(
        &[
            ("rank", Value::Number(1.0)),
            ("class", Value::String("primary".into())),
        ],
        GeometryType::Point,
    );
    let context = EvaluationContext::at_zoom(10.0).with_feature(&properties, GeometryType::Point);
    assert_eq!(
        evaluate(r#"["==", ["get", "rank"], 1]"#, &context),
        Value::Boolean(true)
    );
    assert_eq!(
        evaluate(r#"["==", ["get", "rank"], "1"]"#, &context),
        Value::Boolean(false)
    );
    assert_eq!(
        evaluate(r#"["<", ["get", "rank"], 2]"#, &context),
        Value::Boolean(true)
    );
    assert_eq!(
        evaluate(r#"["<=", ["get", "rank"], 1]"#, &context),
        Value::Boolean(true)
    );
    assert_eq!(
        evaluate(r#"["!=", ["get", "class"], "motorway"]"#, &context),
        Value::Boolean(true)
    );
}

#[test]
fn boolean_logic_case_match_and_coalesce() {
    let properties = feature(&[("rank", Value::Number(7.0))], GeometryType::Point);
    let context = EvaluationContext::at_zoom(10.0).with_feature(&properties, GeometryType::Point);
    assert_eq!(
        evaluate(r#"["all", true, [">", ["get", "rank"], 5]]"#, &context),
        Value::Boolean(true)
    );
    assert_eq!(
        evaluate(r#"["any", false, ["==", ["get", "rank"], 7]]"#, &context),
        Value::Boolean(true)
    );
    assert_eq!(evaluate(r#"["!", false]"#, &context), Value::Boolean(true));
    assert_eq!(
        evaluate(
            r#"["case", [">", ["get", "rank"], 5], "big", "small"]"#,
            &context
        ),
        Value::String("big".into())
    );
    assert_eq!(
        evaluate(
            r#"["match", ["get", "rank"], 7, "seven", "other"]"#,
            &context
        ),
        Value::String("seven".into())
    );
    assert_eq!(
        evaluate(r#"["coalesce", ["get", "missing"], "fallback"]"#, &context),
        Value::String("fallback".into())
    );
}

#[test]
fn arithmetic_and_math_operators() {
    let context = EvaluationContext::at_zoom(0.0);
    assert_eq!(
        evaluate(r#"["+", 1, 2, ["*", 3, 4]]"#, &context),
        Value::Number(15.0)
    );
    assert_eq!(evaluate(r#"["-", 10, 4]"#, &context), Value::Number(6.0));
    assert_eq!(evaluate(r#"["/", 10, 4]"#, &context), Value::Number(2.5));
    assert_eq!(evaluate(r#"["%", 10, 4]"#, &context), Value::Number(2.0));
    assert_eq!(evaluate(r#"["^", 2, 3]"#, &context), Value::Number(8.0));
    assert_eq!(
        evaluate(r#"["min", 3, 1, 2]"#, &context),
        Value::Number(1.0)
    );
    assert_eq!(
        evaluate(r#"["max", 3, 1, 2]"#, &context),
        Value::Number(3.0)
    );
    assert_eq!(evaluate(r#"["round", 1.6]"#, &context), Value::Number(2.0));
    assert_eq!(evaluate(r#"["floor", 1.6]"#, &context), Value::Number(1.0));
    assert_eq!(evaluate(r#"["ceil", 1.2]"#, &context), Value::Number(2.0));
    assert_eq!(evaluate(r#"["abs", -3]"#, &context), Value::Number(3.0));
    assert_eq!(evaluate(r#"["sqrt", 9]"#, &context), Value::Number(3.0));
    let Value::Number(pi) = evaluate(r#"["pi"]"#, &context) else {
        panic!("pi is a number");
    };
    assert!((pi - std::f64::consts::PI).abs() < 1e-12);
}

#[test]
fn string_operators() {
    let context = EvaluationContext::at_zoom(0.0);
    assert_eq!(
        evaluate(r#"["concat", "a", ["to-string", 1], "b"]"#, &context),
        Value::String("a1b".into())
    );
    assert_eq!(
        evaluate(r#"["upcase", "abc"]"#, &context),
        Value::String("ABC".into())
    );
    assert_eq!(
        evaluate(r#"["downcase", "ABC"]"#, &context),
        Value::String("abc".into())
    );
    assert_eq!(
        evaluate(r#"["length", "map"]"#, &context),
        Value::Number(3.0)
    );
    assert_eq!(
        evaluate(r#"["at", "map", -1]"#, &context),
        Value::String("p".into())
    );
    assert_eq!(
        evaluate(r#"["in", "a", "map"]"#, &context),
        Value::Boolean(true)
    );
    assert_eq!(
        evaluate(r#"["index-of", "map", "a"]"#, &context),
        Value::Number(1.0)
    );
    assert_eq!(
        evaluate(r#"["slice", "map", 1]"#, &context),
        Value::String("ap".into())
    );
}

#[test]
fn colour_expressions_and_css_parsing() {
    let context = EvaluationContext::at_zoom(0.0);
    assert_eq!(
        evaluate(r#"["rgb", 255, 0, 0]"#, &context),
        Value::Color(Color::from_rgb_numbers(255.0, 0.0, 0.0, 1.0))
    );
    let Value::Color(translucent) = evaluate(r#"["rgba", 0, 0, 255, 0.5]"#, &context) else {
        panic!("rgba produces a colour");
    };
    assert!((translucent.alpha - 0.5).abs() < 1e-12);
    assert_eq!(
        evaluate(r##"["to-color", "#ff0000"]"##, &context),
        Value::Color(Color::from_css("#ff0000").expect("red parses"))
    );
    assert_eq!(
        evaluate(r#"["to-color", "not-a-colour"]"#, &context),
        Value::Null
    );

    assert_eq!(
        Color::from_css("rgba(255, 255, 255, 1)").expect("rgba parses"),
        Color::WHITE
    );
    assert_eq!(
        Color::from_css("#1a2b3c").expect("hex parses").to_rgba8(),
        [0x1a, 0x2b, 0x3c, 0xff]
    );
    assert_eq!(
        Color::from_css("hsl(120, 100%, 50%)")
            .expect("hsl parses")
            .to_rgba8(),
        [0, 255, 0, 255]
    );
    assert_eq!(
        Color::from_css("transparent")
            .expect("named colour parses")
            .alpha,
        0.0
    );
}

#[test]
fn let_bindings_shadow_outer_names() {
    let context = EvaluationContext::at_zoom(0.0);
    assert_eq!(
        evaluate(r#"["let", "x", 2, ["+", ["var", "x"], 3]]"#, &context),
        Value::Number(5.0)
    );
}

#[test]
fn zoom_must_be_the_direct_input_of_a_top_level_step_or_interpolate() {
    let valid = parse(r#"["step", ["zoom"], 0, 1, 10]"#);
    assert!(valid.validate_zoom_placement().is_ok());
    let valid_interpolation = parse(r#"["interpolate", ["linear"], ["zoom"], 0, 0, 1, 10]"#);
    assert!(valid_interpolation.validate_zoom_placement().is_ok());
    let invalid = parse(r#"["+", ["zoom"], 1]"#);
    assert!(invalid.validate_zoom_placement().is_err());
}

#[test]
fn interpolation_stops_must_be_numeric_and_strictly_ascending() {
    assert!(Expression::parse_str(r#"["interpolate", ["linear"], ["zoom"], 0, 0, 0, 1]"#).is_err());
    assert!(
        Expression::parse_str(r#"["interpolate", ["linear"], ["zoom"], 0, 0, "two", 1]"#).is_err()
    );
    assert!(Expression::parse_str(r#"["step", ["zoom"], 0, 1, 1, 1, 2]"#).is_err());
}

#[test]
fn expression_analysis_classifies_inputs() {
    use rustic_maps_style::ExpressionAnalysis;
    assert_eq!(parse("42").analysis(), ExpressionAnalysis::Constant);
    assert_eq!(parse(r#"["zoom"]"#).analysis(), ExpressionAnalysis::Camera);
    assert_eq!(
        parse(r#"["get", "class"]"#).analysis(),
        ExpressionAnalysis::DataDriven
    );
    assert_eq!(
        parse(r#"["+", ["zoom"], ["get", "rank"]]"#).analysis(),
        ExpressionAnalysis::CameraAndData
    );
}

#[test]
fn accumulating_operators_never_yield_a_non_finite_number() {
    // The sibling numeric operators map NaN to `Null`; an accumulating
    // operator must map any non-finite result there too, or an infinity
    // reaches geometry as a degenerate vertex.
    let context = EvaluationContext::at_zoom(0.0);
    assert_eq!(evaluate(r#"["*", 1e308, 1e308]"#, &context), Value::Null);
    assert_eq!(evaluate(r#"["+", 1e308, 1e308]"#, &context), Value::Null);
    assert_eq!(
        evaluate(r#"["max", 1e308, ["*", 1e308, 10]]"#, &context),
        Value::Null
    );
    assert_eq!(
        evaluate(r#"["min", -1e308, ["*", 1e308, -10]]"#, &context),
        Value::Null
    );
}
