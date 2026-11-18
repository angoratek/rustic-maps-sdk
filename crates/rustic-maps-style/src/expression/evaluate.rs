//! Expression evaluation.
//!
//! Paint expressions are evaluated at the frame's fractional zoom; layout
//! expressions are evaluated at the integer zoom the specification requires.
//! The caller selects the zoom it passes in [`EvaluationContext::zoom`].
//!
//! A type error, a missing property or an operator this phase parses but does
//! not implement evaluates to [`Value::Null`]. The property resolver then
//! substitutes the property's documented default, so one malformed feature
//! cannot blank a layer.

use std::collections::BTreeMap;

use crate::color::Color;
use crate::error::EvaluationError;
use crate::types::Value;

use super::interpolation::Interpolation;
use super::operators::{
    value_at, value_contains, value_index_of, value_length, value_slice, value_to_color,
    value_to_number, value_to_string, values_equal, values_order,
};
use super::{Expression, ExpressionKind, FormatSection};

/// The geometry kind an expression's `geometry-type` operator reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeometryType {
    /// A point or multipoint feature.
    Point,
    /// A line-string or multi-line-string feature.
    LineString,
    /// A polygon or multi-polygon feature.
    Polygon,
}

impl GeometryType {
    /// The specification's `geometry-type` spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Point => "Point",
            Self::LineString => "LineString",
            Self::Polygon => "Polygon",
        }
    }
}

/// The feature, camera and state values an expression is evaluated against.
#[derive(Debug, Clone, Copy, Default)]
pub struct EvaluationContext<'a> {
    /// The zoom level: fractional for paint, integer for layout.
    pub zoom: f64,
    /// The feature's properties, when one feature is being resolved.
    pub properties: Option<&'a BTreeMap<String, Value>>,
    /// The feature's geometry kind.
    pub geometry_type: Option<GeometryType>,
    /// The feature's stable identifier.
    pub feature_id: Option<&'a Value>,
    /// The style's global state, when a caller supplies it.
    pub global_state: Option<&'a BTreeMap<String, Value>>,
}

impl<'a> EvaluationContext<'a> {
    /// A camera-only context at `zoom`.
    pub fn at_zoom(zoom: f64) -> Self {
        Self {
            zoom,
            ..Self::default()
        }
    }

    /// Attaches one feature's properties and geometry.
    pub fn with_feature(
        mut self,
        properties: &'a BTreeMap<String, Value>,
        geometry_type: GeometryType,
    ) -> Self {
        self.properties = Some(properties);
        self.geometry_type = Some(geometry_type);
        self
    }
}

impl Expression {
    /// Evaluates the expression in a context.
    ///
    /// # Errors
    /// The evaluator reports an error only for an internal inconsistency such
    /// as a stop array it cannot interpret; runtime type errors and missing
    /// properties evaluate to [`Value::Null`].
    pub fn evaluate(&self, context: &EvaluationContext<'_>) -> Result<Value, EvaluationError> {
        evaluate_expression(self, context, &BTreeMap::new())
    }
}

fn evaluate_expression(
    expression: &Expression,
    context: &EvaluationContext<'_>,
    variables: &BTreeMap<String, Value>,
) -> Result<Value, EvaluationError> {
    let value = match &expression.kind {
        ExpressionKind::Literal(literal) => literal.clone(),
        ExpressionKind::Get { key, object } => {
            let key = evaluate_expression(key, context, variables)?;
            let key = value_to_string(&key);
            // The feature's own property map is borrowed for the lookup; only
            // the addressed value is cloned. An explicit object operand is
            // already an owned evaluation result, so its map is borrowed too.
            match object {
                Some(object) => match evaluate_expression(object, context, variables)? {
                    Value::Object(object_properties) => {
                        object_properties.get(&key).cloned().unwrap_or(Value::Null)
                    }
                    _ => Value::Null,
                },
                None => context
                    .properties
                    .and_then(|properties| properties.get(&key).cloned())
                    .unwrap_or(Value::Null),
            }
        }
        ExpressionKind::Has { key } => {
            let key = evaluate_expression(key, context, variables)?;
            let key = value_to_string(&key);
            Value::Boolean(
                context
                    .properties
                    .is_some_and(|properties| properties.contains_key(&key)),
            )
        }
        ExpressionKind::GeometryType => {
            context.geometry_type.map_or(Value::Null, |geometry_type| {
                Value::String(geometry_type.as_str().to_string())
            })
        }
        ExpressionKind::FeatureId => context.feature_id.cloned().unwrap_or(Value::Null),
        ExpressionKind::Properties => context
            .properties
            .map_or(Value::Null, |properties| Value::Object(properties.clone())),
        ExpressionKind::Zoom => Value::Number(context.zoom),
        ExpressionKind::GlobalState { key } => {
            let key = evaluate_expression(key, context, variables)?;
            let key = value_to_string(&key);
            context
                .global_state
                .and_then(|state| state.get(&key).cloned())
                .unwrap_or(Value::Null)
        }
        ExpressionKind::FeatureState { .. } => Value::Null,
        ExpressionKind::Let { bindings, body } => {
            let mut bound_variables = variables.clone();
            for (name, binding) in bindings {
                let bound_value = evaluate_expression(binding, context, &bound_variables)?;
                bound_variables.insert(name.clone(), bound_value);
            }
            evaluate_expression(body, context, &bound_variables)?
        }
        ExpressionKind::Variable(name) => variables.get(name).cloned().unwrap_or(Value::Null),
        ExpressionKind::All(expressions) => {
            let mut all_true = true;
            for child in expressions {
                if !evaluate_expression(child, context, variables)?.truthy() {
                    all_true = false;
                    break;
                }
            }
            Value::Boolean(all_true)
        }
        ExpressionKind::Any(expressions) => {
            let mut any_true = false;
            for child in expressions {
                if evaluate_expression(child, context, variables)?.truthy() {
                    any_true = true;
                    break;
                }
            }
            Value::Boolean(any_true)
        }
        ExpressionKind::Not(child) => {
            Value::Boolean(!evaluate_expression(child, context, variables)?.truthy())
        }
        ExpressionKind::Equal(left, right) => {
            let left = evaluate_expression(left, context, variables)?;
            let right = evaluate_expression(right, context, variables)?;
            Value::Boolean(values_equal(&left, &right))
        }
        ExpressionKind::NotEqual(left, right) => {
            let left = evaluate_expression(left, context, variables)?;
            let right = evaluate_expression(right, context, variables)?;
            Value::Boolean(!values_equal(&left, &right))
        }
        ExpressionKind::Less(left, right) => {
            compare(left, right, context, variables, |ordering| ordering.is_lt())?
        }
        ExpressionKind::LessEqual(left, right) => {
            compare(left, right, context, variables, |ordering| ordering.is_le())?
        }
        ExpressionKind::Greater(left, right) => {
            compare(left, right, context, variables, |ordering| ordering.is_gt())?
        }
        ExpressionKind::GreaterEqual(left, right) => {
            compare(left, right, context, variables, |ordering| ordering.is_ge())?
        }
        ExpressionKind::Case {
            branches,
            otherwise,
        } => {
            let mut selected = None;
            for (condition, output) in branches {
                if evaluate_expression(condition, context, variables)?.truthy() {
                    selected = Some(output);
                    break;
                }
            }
            match selected {
                Some(output) => evaluate_expression(output, context, variables)?,
                None => evaluate_expression(otherwise, context, variables)?,
            }
        }
        ExpressionKind::Match {
            input,
            labels,
            otherwise,
        } => {
            let input = evaluate_expression(input, context, variables)?;
            let mut selected = None;
            for (candidate_labels, output) in labels {
                if candidate_labels
                    .iter()
                    .any(|candidate| values_equal(&input, candidate))
                {
                    selected = Some(output);
                    break;
                }
            }
            match selected {
                Some(output) => evaluate_expression(output, context, variables)?,
                None => evaluate_expression(otherwise, context, variables)?,
            }
        }
        ExpressionKind::Coalesce(expressions) => {
            let mut coalesced = Value::Null;
            for child in expressions {
                coalesced = evaluate_expression(child, context, variables)?;
                if !coalesced.is_null() {
                    break;
                }
            }
            coalesced
        }
        ExpressionKind::Step {
            input,
            first_output,
            stops,
        } => {
            let input = evaluate_expression(input, context, variables)?;
            let Some(input) = input.as_number() else {
                return Ok(Value::Null);
            };
            let mut selected = evaluate_expression(first_output, context, variables)?;
            for (stop_input, output) in stops {
                if input < *stop_input {
                    break;
                }
                selected = evaluate_expression(output, context, variables)?;
            }
            selected
        }
        ExpressionKind::Interpolate {
            interpolation,
            input,
            stops,
        } => evaluate_interpolation(*interpolation, input, stops, context, variables)?,
        ExpressionKind::ToString(child) => Value::String(value_to_string(&evaluate_expression(
            child, context, variables,
        )?)),
        ExpressionKind::ToNumber(child) => {
            value_to_number(&evaluate_expression(child, context, variables)?)
                .map_or(Value::Null, Value::Number)
        }
        ExpressionKind::ToBoolean(child) => {
            Value::Boolean(evaluate_expression(child, context, variables)?.truthy())
        }
        ExpressionKind::ToColor(child) => {
            value_to_color(&evaluate_expression(child, context, variables)?)
                .map_or(Value::Null, Value::Color)
        }
        ExpressionKind::Rgb { red, green, blue } => {
            let channels = [
                evaluate_expression(red, context, variables)?,
                evaluate_expression(green, context, variables)?,
                evaluate_expression(blue, context, variables)?,
            ];
            match channels
                .iter()
                .map(Value::as_number)
                .collect::<Option<Vec<f64>>>()
            {
                Some(channels) => Value::Color(Color::from_rgb_numbers(
                    channels[0],
                    channels[1],
                    channels[2],
                    1.0,
                )),
                None => Value::Null,
            }
        }
        ExpressionKind::Rgba {
            red,
            green,
            blue,
            alpha,
        } => {
            let channels = [
                evaluate_expression(red, context, variables)?,
                evaluate_expression(green, context, variables)?,
                evaluate_expression(blue, context, variables)?,
                evaluate_expression(alpha, context, variables)?,
            ];
            match channels
                .iter()
                .map(Value::as_number)
                .collect::<Option<Vec<f64>>>()
            {
                Some(channels) => Value::Color(Color::from_rgb_numbers(
                    channels[0],
                    channels[1],
                    channels[2],
                    channels[3],
                )),
                None => Value::Null,
            }
        }
        ExpressionKind::Concat(expressions) => {
            let mut concatenated = String::new();
            for child in expressions {
                concatenated.push_str(&value_to_string(&evaluate_expression(
                    child, context, variables,
                )?));
            }
            Value::String(concatenated)
        }
        ExpressionKind::Upcase(child) => Value::String(
            value_to_string(&evaluate_expression(child, context, variables)?).to_uppercase(),
        ),
        ExpressionKind::Downcase(child) => Value::String(
            value_to_string(&evaluate_expression(child, context, variables)?).to_lowercase(),
        ),
        ExpressionKind::Length(child) => {
            value_length(&evaluate_expression(child, context, variables)?)
                .map_or(Value::Null, |length| Value::Number(length as f64))
        }
        ExpressionKind::At { input, index } => {
            let input = evaluate_expression(input, context, variables)?;
            let index = evaluate_expression(index, context, variables)?;
            match index.as_number() {
                Some(index) => value_at(&input, index as i64).unwrap_or(Value::Null),
                None => Value::Null,
            }
        }
        ExpressionKind::In { needle, haystack } => {
            let needle = evaluate_expression(needle, context, variables)?;
            let haystack = evaluate_expression(haystack, context, variables)?;
            value_contains(&haystack, &needle).map_or(Value::Null, Value::Boolean)
        }
        ExpressionKind::IndexOf { haystack, needle } => {
            let haystack = evaluate_expression(haystack, context, variables)?;
            let needle = evaluate_expression(needle, context, variables)?;
            value_index_of(&haystack, &needle)
                .map_or(Value::Null, |index| Value::Number(index as f64))
        }
        ExpressionKind::Slice { input, start, end } => {
            let input = evaluate_expression(input, context, variables)?;
            let start = evaluate_expression(start, context, variables)?
                .as_number()
                .map(|number| number as i64);
            let end = match end {
                Some(end) => evaluate_expression(end, context, variables)?
                    .as_number()
                    .map(|number| number as i64),
                None => None,
            };
            match start {
                Some(start) => value_slice(&input, start, end).unwrap_or(Value::Null),
                None => Value::Null,
            }
        }
        ExpressionKind::Add(expressions) => {
            accumulate_numbers(expressions, context, variables, 0.0, |sum, value| {
                sum + value
            })?
        }
        ExpressionKind::Multiply(expressions) => {
            accumulate_numbers(expressions, context, variables, 1.0, |product, value| {
                product * value
            })?
        }
        ExpressionKind::Subtract {
            minuend,
            subtrahend,
        } => binary_number(minuend, subtrahend, context, variables, |left, right| {
            left - right
        })?,
        ExpressionKind::Divide { dividend, divisor } => {
            binary_number(dividend, divisor, context, variables, |left, right| {
                if right == 0.0 { f64::NAN } else { left / right }
            })?
        }
        ExpressionKind::Modulo { dividend, divisor } => {
            binary_number(dividend, divisor, context, variables, |left, right| {
                if right == 0.0 { f64::NAN } else { left % right }
            })?
        }
        ExpressionKind::Power { base, exponent } => {
            binary_number(base, exponent, context, variables, f64::powf)?
        }
        ExpressionKind::Min(expressions) => {
            accumulate_numbers(expressions, context, variables, f64::INFINITY, f64::min)?
        }
        ExpressionKind::Max(expressions) => {
            accumulate_numbers(expressions, context, variables, f64::NEG_INFINITY, f64::max)?
        }
        ExpressionKind::Round(child) => unary_number(child, context, variables, f64::round)?,
        ExpressionKind::Floor(child) => unary_number(child, context, variables, f64::floor)?,
        ExpressionKind::Ceil(child) => unary_number(child, context, variables, f64::ceil)?,
        ExpressionKind::Abs(child) => unary_number(child, context, variables, f64::abs)?,
        ExpressionKind::Sqrt(child) => unary_number(child, context, variables, |value| {
            if value < 0.0 { f64::NAN } else { value.sqrt() }
        })?,
        ExpressionKind::NaturalLog(child) => unary_number(child, context, variables, |value| {
            if value <= 0.0 { f64::NAN } else { value.ln() }
        })?,
        ExpressionKind::Log10(child) => unary_number(child, context, variables, |value| {
            if value <= 0.0 {
                f64::NAN
            } else {
                value.log10()
            }
        })?,
        ExpressionKind::Log2(child) => unary_number(child, context, variables, |value| {
            if value <= 0.0 { f64::NAN } else { value.log2() }
        })?,
        ExpressionKind::Sin(child) => unary_number(child, context, variables, f64::sin)?,
        ExpressionKind::Cos(child) => unary_number(child, context, variables, f64::cos)?,
        ExpressionKind::Tan(child) => unary_number(child, context, variables, f64::tan)?,
        ExpressionKind::Asin(child) => unary_number(child, context, variables, |value| {
            if (-1.0..=1.0).contains(&value) {
                value.asin()
            } else {
                f64::NAN
            }
        })?,
        ExpressionKind::Acos(child) => unary_number(child, context, variables, |value| {
            if (-1.0..=1.0).contains(&value) {
                value.acos()
            } else {
                f64::NAN
            }
        })?,
        ExpressionKind::Atan(child) => unary_number(child, context, variables, f64::atan)?,
        ExpressionKind::Pi => Value::Number(std::f64::consts::PI),
        ExpressionKind::Euler => Value::Number(std::f64::consts::E),
        ExpressionKind::Ln2 => Value::Number(std::f64::consts::LN_2),
        ExpressionKind::Format(sections) => {
            Value::String(evaluate_format(sections, context, variables)?)
        }
        ExpressionKind::Unsupported { .. } => Value::Null,
    };
    Ok(value)
}

fn compare(
    left: &Expression,
    right: &Expression,
    context: &EvaluationContext<'_>,
    variables: &BTreeMap<String, Value>,
    predicate: impl Fn(std::cmp::Ordering) -> bool,
) -> Result<Value, EvaluationError> {
    let left = evaluate_expression(left, context, variables)?;
    let right = evaluate_expression(right, context, variables)?;
    Ok(Value::Boolean(
        values_order(&left, &right).is_some_and(predicate),
    ))
}

fn evaluate_interpolation(
    interpolation: Interpolation,
    input: &Expression,
    stops: &[(f64, Expression)],
    context: &EvaluationContext<'_>,
    variables: &BTreeMap<String, Value>,
) -> Result<Value, EvaluationError> {
    let input = evaluate_expression(input, context, variables)?;
    let Some(input) = input.as_number() else {
        return Ok(Value::Null);
    };
    let Some(first_stop) = stops.first() else {
        return Ok(Value::Null);
    };
    if input <= first_stop.0 {
        return evaluate_expression(&first_stop.1, context, variables);
    }
    let last_stop = match stops.last() {
        Some(last_stop) => last_stop,
        None => return Ok(Value::Null),
    };
    if input >= last_stop.0 {
        return evaluate_expression(&last_stop.1, context, variables);
    }
    for stop_pair in stops.windows(2) {
        let [(lower_input, lower_output), (upper_input, upper_output)] =
            [(&stop_pair[0]), (&stop_pair[1])];
        if input < *upper_input {
            let fraction = interpolation.fraction(*lower_input, *upper_input, input);
            let lower_value = evaluate_expression(lower_output, context, variables)?;
            let upper_value = evaluate_expression(upper_output, context, variables)?;
            return Ok(interpolate_values(&lower_value, &upper_value, fraction));
        }
    }
    Ok(Value::Null)
}

fn interpolate_values(lower: &Value, upper: &Value, fraction: f64) -> Value {
    match (lower, upper) {
        (Value::Number(lower), Value::Number(upper)) => {
            Value::Number(lower + (upper - lower) * fraction)
        }
        (Value::Color(lower), Value::Color(upper)) => {
            Value::Color(Color::interpolate(*lower, *upper, fraction))
        }
        (Value::Color(lower), Value::String(upper)) => Color::from_css(upper)
            .map_or(Value::Null, |upper| {
                Value::Color(Color::interpolate(*lower, upper, fraction))
            }),
        (Value::String(lower), Value::Color(upper)) => Color::from_css(lower)
            .map_or(Value::Null, |lower| {
                Value::Color(Color::interpolate(lower, *upper, fraction))
            }),
        (Value::String(lower), Value::String(upper)) => {
            match (Color::from_css(lower), Color::from_css(upper)) {
                (Ok(lower), Ok(upper)) => Value::Color(Color::interpolate(lower, upper, fraction)),
                _ => Value::Null,
            }
        }
        _ => Value::Null,
    }
}

fn evaluate_format(
    sections: &[FormatSection],
    context: &EvaluationContext<'_>,
    variables: &BTreeMap<String, Value>,
) -> Result<String, EvaluationError> {
    let mut text = String::new();
    for section in sections {
        let value = evaluate_expression(&section.value, context, variables)?;
        text.push_str(&value_to_string(&value));
    }
    Ok(text)
}

fn unary_number(
    child: &Expression,
    context: &EvaluationContext<'_>,
    variables: &BTreeMap<String, Value>,
    operation: impl Fn(f64) -> f64,
) -> Result<Value, EvaluationError> {
    let value = evaluate_expression(child, context, variables)?;
    Ok(match value.as_number() {
        Some(number) => {
            let result = operation(number);
            if result.is_nan() {
                Value::Null
            } else {
                Value::Number(result)
            }
        }
        None => Value::Null,
    })
}

fn binary_number(
    left: &Expression,
    right: &Expression,
    context: &EvaluationContext<'_>,
    variables: &BTreeMap<String, Value>,
    operation: impl Fn(f64, f64) -> f64,
) -> Result<Value, EvaluationError> {
    let left = evaluate_expression(left, context, variables)?;
    let right = evaluate_expression(right, context, variables)?;
    match (left.as_number(), right.as_number()) {
        (Some(left), Some(right)) => {
            let result = operation(left, right);
            Ok(if result.is_nan() {
                Value::Null
            } else {
                Value::Number(result)
            })
        }
        _ => Ok(Value::Null),
    }
}

fn accumulate_numbers(
    expressions: &[Expression],
    context: &EvaluationContext<'_>,
    variables: &BTreeMap<String, Value>,
    initial: f64,
    operation: impl Fn(f64, f64) -> f64,
) -> Result<Value, EvaluationError> {
    let mut accumulated = initial;
    for child in expressions {
        let value = evaluate_expression(child, context, variables)?;
        let Some(number) = value.as_number() else {
            return Ok(Value::Null);
        };
        accumulated = operation(accumulated, number);
    }
    // The sibling numeric operators map NaN to `Null`; an accumulating
    // operation must do the same for any non-finite result, or an infinity
    // reaches geometry as a degenerate vertex.
    if accumulated.is_finite() {
        Ok(Value::Number(accumulated))
    } else {
        Ok(Value::Null)
    }
}
