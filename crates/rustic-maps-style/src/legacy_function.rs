//! Normalization of the deprecated `{stops, type, base, property, default}`
//! function form into the typed expression tree.
//!
//! The demotiles target still carries legacy functions for `line-width`,
//! `line-opacity`, `text-size`, `text-field` and `text-transform`, so the
//! parser cannot accept modern expressions alone. Normalization happens once,
//! at parse time: every consumer downstream sees one tree and one evaluator.
//!
//! The rules follow the specification's deprecation page:
//!
//! - `interval` becomes `step`;
//! - `categorical` becomes `match`;
//! - `identity` becomes `get`;
//! - `exponential` (the default) becomes `interpolate`, except where the
//!   output type cannot be interpolated (strings, booleans, collections),
//!   where the specification's behavior is discrete and the function becomes
//!   `step`.

use serde_json::{Value as JsonValue, json};

use crate::color::Color;
use crate::defaults::DEFAULT_LEGACY_EXPONENTIAL_BASE;
use crate::error::ExpressionError;

/// The legacy function types the specification defines.
const FUNCTION_TYPE_INTERVAL: &str = "interval";
const FUNCTION_TYPE_CATEGORICAL: &str = "categorical";
const FUNCTION_TYPE_IDENTITY: &str = "identity";
const FUNCTION_TYPE_EXPONENTIAL: &str = "exponential";
const FUNCTION_TYPE_LINEAR: &str = "linear";

/// An exponential base of one is a linear curve; the specification renders it
/// with the `linear` interpolation so the two spellings evaluate identically.
const LINEAR_EXPONENTIAL_BASE: f64 = 1.0;

/// Normalizes a legacy function object into an expression JSON value.
///
/// # Errors
/// A missing or malformed `stops` array, a non-numeric stop input where a
/// numeric one is required, a non-positive base, or a function form this
/// phase cannot interpret reports [`ExpressionError`].
pub(crate) fn normalize_legacy_function(
    function: &JsonValue,
) -> Result<JsonValue, ExpressionError> {
    let object = function
        .as_object()
        .ok_or_else(|| ExpressionError::new("a legacy function must be a JSON object"))?;
    let function_type = object
        .get("type")
        .and_then(JsonValue::as_str)
        .unwrap_or(FUNCTION_TYPE_EXPONENTIAL);
    let property = object.get("property").and_then(JsonValue::as_str);
    let default = object.get("default").map(literalize);

    if function_type == FUNCTION_TYPE_IDENTITY {
        let Some(property) = property else {
            return Err(ExpressionError::new(
                "an identity legacy function needs a property",
            ));
        };
        let identity = json!(["get", property]);
        return Ok(match default {
            Some(default) => json!(["coalesce", identity, default]),
            None => identity,
        });
    }

    let stops_value = object
        .get("stops")
        .ok_or_else(|| ExpressionError::new("a legacy function needs a stops array"))?;
    let stops_array = stops_value
        .as_array()
        .ok_or_else(|| ExpressionError::new("a legacy function's stops must be an array"))?;
    let mut stops: Vec<(JsonValue, JsonValue)> = Vec::with_capacity(stops_array.len());
    for stop in stops_array {
        let pair = stop.as_array().ok_or_else(|| {
            ExpressionError::new("each legacy stop must be an [input, output] pair")
        })?;
        if pair.len() != 2 {
            return Err(ExpressionError::new(
                "each legacy stop must be an [input, output] pair",
            ));
        }
        stops.push((pair[0].clone(), literalize(&pair[1])));
    }
    if stops.is_empty() {
        return Err(ExpressionError::new(
            "a legacy function needs at least one stop",
        ));
    }

    let base = match object.get("base").and_then(JsonValue::as_f64) {
        Some(base) if base > 0.0 => base,
        Some(_) => {
            return Err(ExpressionError::new(
                "a legacy exponential base must be positive",
            ));
        }
        None => DEFAULT_LEGACY_EXPONENTIAL_BASE,
    };
    // A `linear` function ignores a stray `base`; the two spellings must
    // evaluate identically.
    let curve_base = if function_type == FUNCTION_TYPE_LINEAR {
        LINEAR_EXPONENTIAL_BASE
    } else {
        base
    };

    let input = match property {
        Some(property) => json!(["get", property]),
        None => json!(["zoom"]),
    };

    let output_is_numeric = stops.iter().all(|(_, output)| output.as_f64().is_some());
    let output_is_color = stops.iter().all(|(_, output)| {
        output
            .as_str()
            .is_some_and(|text| Color::from_css(text).is_ok())
    });
    let stop_inputs_are_numeric = stops
        .iter()
        .all(|(stop_input, _)| stop_input.as_f64().is_some());

    let expression = match function_type {
        FUNCTION_TYPE_CATEGORICAL => build_match(&input, &stops, default),
        FUNCTION_TYPE_INTERVAL => {
            if !stop_inputs_are_numeric {
                return Err(ExpressionError::new(
                    "an interval legacy function needs numeric stop inputs",
                ));
            }
            let step = build_step(&input, &stops);
            wrap_with_default(step, property, default)
        }
        FUNCTION_TYPE_EXPONENTIAL | FUNCTION_TYPE_LINEAR => {
            if !stop_inputs_are_numeric {
                // String stop labels are the categorical spelling in the
                // wild even when the function omits `type`.
                build_match(&input, &stops, default)
            } else if (output_is_numeric || output_is_color) && stops.len() >= 2 {
                let interpolation = build_interpolate(&input, &stops, curve_base);
                wrap_with_default(interpolation, property, default)
            } else {
                let step = build_step(&input, &stops);
                wrap_with_default(step, property, default)
            }
        }
        other => {
            return Err(ExpressionError::new(format!(
                "legacy function type {other:?} is not recognized"
            )));
        }
    };
    Ok(expression)
}

/// Wraps a property-driven function so a missing property resolves to the
/// function's `default` rather than to `null`.
fn wrap_with_default(
    expression: JsonValue,
    property: Option<&str>,
    default: Option<JsonValue>,
) -> JsonValue {
    match (property, default) {
        (Some(_), Some(default)) => json!(["coalesce", expression, default]),
        _ => expression,
    }
}

fn build_step(input: &JsonValue, stops: &[(JsonValue, JsonValue)]) -> JsonValue {
    let mut expression = vec![json!("step"), input.clone(), stops[0].1.clone()];
    for (stop_input, output) in stops.iter().skip(1) {
        expression.push(stop_input.clone());
        expression.push(output.clone());
    }
    JsonValue::Array(expression)
}

fn build_interpolate(input: &JsonValue, stops: &[(JsonValue, JsonValue)], base: f64) -> JsonValue {
    let interpolation = if (base - LINEAR_EXPONENTIAL_BASE).abs() <= f64::EPSILON {
        json!(["linear"])
    } else {
        json!(["exponential", base])
    };
    let mut expression = vec![json!("interpolate"), interpolation, input.clone()];
    for (stop_input, output) in stops {
        expression.push(stop_input.clone());
        expression.push(output.clone());
    }
    JsonValue::Array(expression)
}

fn build_match(
    input: &JsonValue,
    stops: &[(JsonValue, JsonValue)],
    default: Option<JsonValue>,
) -> JsonValue {
    let mut expression = vec![json!("match"), input.clone()];
    for (label, output) in stops {
        expression.push(label.clone());
        expression.push(output.clone());
    }
    expression.push(default.unwrap_or(JsonValue::Null));
    JsonValue::Array(expression)
}

/// Treats a non-expression collection output as a literal, so a legacy stop
/// that produces an array (a dash pattern, for example) survives parsing.
fn literalize(output: &JsonValue) -> JsonValue {
    if (output.is_array() || output.is_object()) && !crate::expression::is_expression_array(output)
    {
        json!(["literal", output])
    } else {
        output.clone()
    }
}
