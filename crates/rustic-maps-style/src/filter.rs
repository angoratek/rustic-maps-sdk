//! Legacy filter arrays and expression filters.
//!
//! Both spellings normalize into the same boolean [`Expression`] used by
//! paint:
//!
//! - modern: `["==", ["get", "class"], "motorway"]`
//! - legacy: `["all", ["==", "class", "motorway"], ["!=", "class", "rail"]]`
//!
//! In the legacy form a bare string in the first operand position names a
//! feature property and is rewritten to `["get", name]`; `$type` is rewritten
//! to `["geometry-type"]`; `in`/`!in` become an `any` of equalities; `none`
//! becomes a negated `any`. Everything then goes through the one expression
//! parser, so a filter and a paint expression share their comparison and
//! coercion semantics.

use serde_json::{Value as JsonValue, json};

use crate::error::FilterError;
use crate::expression::{Expression, is_expression_array, is_known_operator};

/// The legacy and modern filter comparison operators.
const COMPARISON_OPERATORS: &[&str] = &["==", "!=", "<", "<=", ">", ">="];

/// Parses either filter spelling into a boolean expression.
///
/// # Errors
/// A filter that is not an array or a boolean, an operator with the wrong
/// arity, an unknown operator, or an expression that cannot be parsed reports
/// [`FilterError`].
pub fn parse_filter(json_value: &JsonValue) -> Result<Expression, FilterError> {
    let normalized = normalize_filter(json_value)?;
    let expression = Expression::parse(&normalized).map_err(|expression_error| {
        FilterError::new(format!("filter is not an expression: {expression_error}"))
    })?;
    expression.validate_zoom_placement().map_err(|zoom_error| {
        FilterError::new(format!(
            "filter uses the zoom outside a filter context: {zoom_error}"
        ))
    })?;
    Ok(expression)
}

/// Rewrites a legacy filter array into the expression JSON form.
fn normalize_filter(json_value: &JsonValue) -> Result<JsonValue, FilterError> {
    if json_value.is_boolean() {
        return Ok(json_value.clone());
    }
    let Some(array) = json_value.as_array() else {
        return Err(FilterError::new(
            "a filter must be a boolean or an array starting with a filter operator",
        ));
    };
    let Some(operator) = array.first().and_then(JsonValue::as_str) else {
        return Err(FilterError::new(
            "a filter array must start with an operator string",
        ));
    };
    let arguments = &array[1..];
    match operator {
        "all" => {
            let mut normalized = vec![json!("all")];
            for argument in arguments {
                normalized.push(normalize_filter(argument)?);
            }
            Ok(JsonValue::Array(normalized))
        }
        "any" => {
            let mut normalized = vec![json!("any")];
            for argument in arguments {
                normalized.push(normalize_filter(argument)?);
            }
            Ok(JsonValue::Array(normalized))
        }
        "none" => {
            let mut normalized = vec![json!("any")];
            for argument in arguments {
                normalized.push(normalize_filter(argument)?);
            }
            Ok(json!(["!", JsonValue::Array(normalized)]))
        }
        "has" | "!has" => {
            expect_single_argument(operator, arguments)?;
            let key = normalize_has_key(&arguments[0])?;
            let has = match key {
                HasKey::Always => JsonValue::Bool(operator == "has"),
                HasKey::Key(key) => json!(["has", key]),
            };
            Ok(if operator == "has" {
                has
            } else {
                json!(["!", has])
            })
        }
        "in" | "!in" => normalize_in(operator, arguments),
        comparison if COMPARISON_OPERATORS.contains(&comparison) => {
            expect_single_argument_pair(operator, arguments)?;
            Ok(json!([
                comparison,
                normalize_left_operand(&arguments[0])?,
                normalize_value_operand(&arguments[1])
            ]))
        }
        other if is_known_operator(other) => Ok(json_value.clone()),
        other => Err(FilterError::new(format!(
            "{other:?} is not a filter operator"
        ))),
    }
}

enum HasKey {
    /// `$type` always exists; it is not a property key.
    Always,
    /// A property key or key expression.
    Key(JsonValue),
}

fn normalize_has_key(json_value: &JsonValue) -> Result<HasKey, FilterError> {
    if json_value.as_str() == Some("$type") {
        return Ok(HasKey::Always);
    }
    if let Some(name) = json_value.as_str() {
        return Ok(HasKey::Key(json!(name)));
    }
    if is_expression_array(json_value) {
        return Ok(HasKey::Key(json_value.clone()));
    }
    Err(FilterError::new(
        "has takes a property key string or a key expression",
    ))
}

fn normalize_in(operator: &str, arguments: &[JsonValue]) -> Result<JsonValue, FilterError> {
    if arguments.len() < 2 {
        return Err(FilterError::new(format!(
            "{operator} takes a property and at least one candidate value"
        )));
    }
    let property = normalize_left_operand(&arguments[0])?;
    let mut alternatives = vec![json!("any")];
    for candidate in &arguments[1..] {
        alternatives.push(json!([
            "==",
            property.clone(),
            normalize_value_operand(candidate)
        ]));
    }
    let membership = JsonValue::Array(alternatives);
    Ok(if operator == "in" {
        membership
    } else {
        json!(["!", membership])
    })
}

/// Normalizes an operand that may be a legacy property name: a bare string is
/// a `get`, `$type` is `geometry-type`, and everything else keeps its modern
/// meaning.
fn normalize_left_operand(json_value: &JsonValue) -> Result<JsonValue, FilterError> {
    if let Some(name) = json_value.as_str() {
        return Ok(if name == "$type" {
            json!(["geometry-type"])
        } else {
            json!(["get", name])
        });
    }
    Ok(normalize_value_operand(json_value))
}

/// Normalizes an operand that is a literal value unless it is already the
/// expression form.
fn normalize_value_operand(json_value: &JsonValue) -> JsonValue {
    if is_expression_array(json_value) {
        json_value.clone()
    } else if json_value.is_array() || json_value.is_object() {
        json!(["literal", json_value])
    } else {
        json_value.clone()
    }
}

fn expect_single_argument(operator: &str, arguments: &[JsonValue]) -> Result<(), FilterError> {
    if arguments.len() == 1 {
        Ok(())
    } else {
        Err(FilterError::new(format!(
            "{operator} takes exactly one argument"
        )))
    }
}

fn expect_single_argument_pair(operator: &str, arguments: &[JsonValue]) -> Result<(), FilterError> {
    if arguments.len() == 2 {
        Ok(())
    } else {
        Err(FilterError::new(format!(
            "{operator} takes exactly two arguments"
        )))
    }
}
