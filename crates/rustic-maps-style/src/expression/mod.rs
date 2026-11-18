//! The typed expression tree, its parser and its static analysis.
//!
//! The parser accepts the specification's array form and, through the
//! legacy-function normaliser, the deprecated `{stops, type, base, property,
//! default}` object form. Both normalize into the same tree, so evaluation has
//! one path.
//!
//! Parsing validates the rules that do not depend on runtime data: operator
//! arity, numeric and strictly ascending interpolation stops, and the rule
//! that `["zoom"]` may only be the direct input of a top-level `step` or
//! `interpolate`. [`Expression::analysis`] classifies an expression as
//! constant, camera-driven, data-driven or both, so a caller can reject a
//! data-driven property where the specification forbids one before it is ever
//! evaluated.

pub mod evaluate;
pub mod interpolation;
pub mod operators;

use serde_json::Value as JsonValue;

use crate::error::ExpressionError;
use crate::legacy_function;
use crate::types::Value;

pub use evaluate::{EvaluationContext, GeometryType};
pub use interpolation::Interpolation;

/// One section of a `format` expression: a value and the text styling that
/// applies to it.
#[derive(Debug, Clone, PartialEq)]
pub struct FormatSection {
    /// The text value.
    pub value: Expression,
    /// Optional font size override.
    pub scale: Option<Expression>,
    /// Optional font stack override.
    pub font: Option<Expression>,
    /// Optional text colour override.
    pub text_color: Option<Expression>,
}

/// A parsed expression.
#[derive(Debug, Clone, PartialEq)]
pub struct Expression {
    /// The expression's operator and children.
    pub kind: ExpressionKind,
}

/// The expression operators this crate models.
#[derive(Debug, Clone, PartialEq)]
pub enum ExpressionKind {
    /// A constant JSON value, produced by a scalar or by `literal`.
    Literal(Value),
    /// A property lookup; `object` optionally overrides the feature's
    /// properties, as the legacy `get` form allows.
    Get {
        /// The property key.
        key: Box<Expression>,
        /// The object to read from instead of the feature's properties.
        object: Option<Box<Expression>>,
    },
    /// Whether a property exists.
    Has {
        /// The property key.
        key: Box<Expression>,
    },
    /// The feature's geometry kind.
    GeometryType,
    /// The feature's identifier.
    FeatureId,
    /// The feature's properties as an object.
    Properties,
    /// The current zoom level.
    Zoom,
    /// A value from the style's global state.
    GlobalState {
        /// The state key.
        key: Box<Expression>,
    },
    /// A value from the feature's state (parsed; not implemented in this
    /// phase).
    FeatureState {
        /// The state key.
        key: Box<Expression>,
        /// The value used when the state has no entry.
        default: Option<Box<Expression>>,
    },
    /// A lexical binding followed by a body.
    Let {
        /// The named bindings, evaluated in order.
        bindings: Vec<(String, Expression)>,
        /// The expression that reads them.
        body: Box<Expression>,
    },
    /// A reference to a `let` binding.
    Variable(String),
    /// Whether every child is true.
    All(Vec<Expression>),
    /// Whether any child is true.
    Any(Vec<Expression>),
    /// Logical negation.
    Not(Box<Expression>),
    /// Equality following the specification's coercion rules.
    Equal(Box<Expression>, Box<Expression>),
    /// Inequality.
    NotEqual(Box<Expression>, Box<Expression>),
    /// Numeric or lexical less than.
    Less(Box<Expression>, Box<Expression>),
    /// Less than or equal.
    LessEqual(Box<Expression>, Box<Expression>),
    /// Greater than.
    Greater(Box<Expression>, Box<Expression>),
    /// Greater than or equal.
    GreaterEqual(Box<Expression>, Box<Expression>),
    /// A conditional chain with a fallback.
    Case {
        /// Condition/output pairs, tried in order.
        branches: Vec<(Expression, Expression)>,
        /// The value when no condition is true.
        otherwise: Box<Expression>,
    },
    /// A value match with a fallback.
    Match {
        /// The value to compare.
        input: Box<Expression>,
        /// Label groups and their outputs, tried in order.
        labels: Vec<(Vec<Value>, Expression)>,
        /// The value when no label group matches.
        otherwise: Box<Expression>,
    },
    /// The first non-null child.
    Coalesce(Vec<Expression>),
    /// A stepped function.
    Step {
        /// The input, usually `["zoom"]` or `["get", …]`.
        input: Box<Expression>,
        /// The output below the first stop.
        first_output: Box<Expression>,
        /// Strictly ascending `(input, output)` stops.
        stops: Vec<(f64, Expression)>,
    },
    /// An interpolated function.
    Interpolate {
        /// The curve between stops.
        interpolation: Interpolation,
        /// The input, usually `["zoom"]` or `["get", …]`.
        input: Box<Expression>,
        /// Strictly ascending `(input, output)` stops.
        stops: Vec<(f64, Expression)>,
    },
    /// Converts a value to a string.
    ToString(Box<Expression>),
    /// Converts a value to a number.
    ToNumber(Box<Expression>),
    /// Converts a value to a boolean.
    ToBoolean(Box<Expression>),
    /// Converts a value to a colour.
    ToColor(Box<Expression>),
    /// An `rgb()` colour from three `0..=255` channels.
    Rgb {
        /// Red channel.
        red: Box<Expression>,
        /// Green channel.
        green: Box<Expression>,
        /// Blue channel.
        blue: Box<Expression>,
    },
    /// An `rgba()` colour.
    Rgba {
        /// Red channel.
        red: Box<Expression>,
        /// Green channel.
        green: Box<Expression>,
        /// Blue channel.
        blue: Box<Expression>,
        /// Alpha channel in `0.0..=1.0`.
        alpha: Box<Expression>,
    },
    /// String concatenation.
    Concat(Vec<Expression>),
    /// Upper-cases a string.
    Upcase(Box<Expression>),
    /// Lower-cases a string.
    Downcase(Box<Expression>),
    /// The number of characters in a string or entries in an array.
    Length(Box<Expression>),
    /// The entry at an index in a string or array.
    At {
        /// The string or array.
        input: Box<Expression>,
        /// The index; negative counts from the end.
        index: Box<Expression>,
    },
    /// Whether a string contains a substring or an array contains a value.
    In {
        /// The value to find.
        needle: Box<Expression>,
        /// The string or array to search.
        haystack: Box<Expression>,
    },
    /// The first position of a value in a string or array.
    IndexOf {
        /// The string or array to search.
        haystack: Box<Expression>,
        /// The value to find.
        needle: Box<Expression>,
    },
    /// A slice of a string or array.
    Slice {
        /// The string or array.
        input: Box<Expression>,
        /// The inclusive start index.
        start: Box<Expression>,
        /// The exclusive end index.
        end: Option<Box<Expression>>,
    },
    /// Variadic addition.
    Add(Vec<Expression>),
    /// Subtraction.
    Subtract {
        /// The value subtracted from.
        minuend: Box<Expression>,
        /// The value subtracted.
        subtrahend: Box<Expression>,
    },
    /// Variadic multiplication.
    Multiply(Vec<Expression>),
    /// Division.
    Divide {
        /// The dividend.
        dividend: Box<Expression>,
        /// The divisor.
        divisor: Box<Expression>,
    },
    /// Remainder.
    Modulo {
        /// The dividend.
        dividend: Box<Expression>,
        /// The divisor.
        divisor: Box<Expression>,
    },
    /// Exponentiation.
    Power {
        /// The base.
        base: Box<Expression>,
        /// The exponent.
        exponent: Box<Expression>,
    },
    /// The smallest child.
    Min(Vec<Expression>),
    /// The largest child.
    Max(Vec<Expression>),
    /// Rounds to the nearest integer.
    Round(Box<Expression>),
    /// Rounds down.
    Floor(Box<Expression>),
    /// Rounds up.
    Ceil(Box<Expression>),
    /// Absolute value.
    Abs(Box<Expression>),
    /// Square root.
    Sqrt(Box<Expression>),
    /// Natural logarithm.
    NaturalLog(Box<Expression>),
    /// Base-10 logarithm.
    Log10(Box<Expression>),
    /// Base-2 logarithm.
    Log2(Box<Expression>),
    /// Sine.
    Sin(Box<Expression>),
    /// Cosine.
    Cos(Box<Expression>),
    /// Tangent.
    Tan(Box<Expression>),
    /// Arcsine.
    Asin(Box<Expression>),
    /// Arccosine.
    Acos(Box<Expression>),
    /// Arctangent.
    Atan(Box<Expression>),
    /// π.
    Pi,
    /// Euler's number.
    Euler,
    /// The natural logarithm of two.
    Ln2,
    /// A formatted text expression.
    Format(Vec<FormatSection>),
    /// An operator this phase parses for diagnostics but does not evaluate.
    Unsupported {
        /// The operator's name.
        operator: String,
        /// Its parsed arguments, so a later phase can implement it without
        /// changing the document model.
        arguments: Vec<Expression>,
    },
}

/// How an expression depends on its inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpressionAnalysis {
    /// The expression has the same value for every feature and zoom.
    Constant,
    /// The expression depends on the zoom level only.
    Camera,
    /// The expression depends on the feature only.
    DataDriven,
    /// The expression depends on both the feature and the zoom level.
    CameraAndData,
}

impl Expression {
    /// Parses one expression from a JSON value.
    ///
    /// # Errors
    /// An unknown operator, an argument of the wrong arity or type, a stop
    /// array that is not strictly ascending, or a JSON object that is neither
    /// an expression nor a legacy function reports [`ExpressionError`].
    pub fn parse(json_value: &JsonValue) -> Result<Self, ExpressionError> {
        match json_value {
            JsonValue::Array(array) => parse_array(array),
            JsonValue::Object(object) => {
                if object.contains_key("stops")
                    || object.contains_key("property")
                    || object.contains_key("type")
                {
                    let normalized = legacy_function::normalize_legacy_function(json_value)?;
                    Self::parse(&normalized)
                } else {
                    Err(ExpressionError::new(
                        "a JSON object is not an expression unless it is a legacy function",
                    ))
                }
            }
            scalar => Ok(Self {
                kind: ExpressionKind::Literal(Value::from(scalar)),
            }),
        }
    }

    /// Parses one expression from JSON text.
    ///
    /// # Errors
    /// Invalid JSON and any [`Expression::parse`] error are reported.
    pub fn parse_str(json_text: &str) -> Result<Self, ExpressionError> {
        let json_value: JsonValue = serde_json::from_str(json_text).map_err(|json_error| {
            ExpressionError::new(format!("expression is not valid JSON: {json_error}"))
        })?;
        Self::parse(&json_value)
    }

    /// Classifies the expression by its inputs.
    pub fn analysis(&self) -> ExpressionAnalysis {
        analyze(self)
    }

    /// Validates the specification's zoom restriction: `["zoom"]` may only be
    /// the direct input of a top-level `step` or `interpolate`, optionally
    /// wrapped in `let`.
    ///
    /// # Errors
    /// Any other placement of `["zoom"]` reports [`ExpressionError`].
    pub fn validate_zoom_placement(&self) -> Result<(), ExpressionError> {
        validate_zoom(self, false)
    }

    /// Every direct child expression, in evaluation order.
    pub fn children(&self) -> Vec<&Expression> {
        let mut children = Vec::new();
        match &self.kind {
            ExpressionKind::Literal(_)
            | ExpressionKind::GeometryType
            | ExpressionKind::FeatureId
            | ExpressionKind::Properties
            | ExpressionKind::Zoom
            | ExpressionKind::Variable(_)
            | ExpressionKind::Pi
            | ExpressionKind::Euler
            | ExpressionKind::Ln2 => {}
            ExpressionKind::Get { key, object } => {
                children.push(key.as_ref());
                if let Some(object) = object {
                    children.push(object.as_ref());
                }
            }
            ExpressionKind::Has { key }
            | ExpressionKind::GlobalState { key }
            | ExpressionKind::ToString(key)
            | ExpressionKind::ToNumber(key)
            | ExpressionKind::ToBoolean(key)
            | ExpressionKind::ToColor(key)
            | ExpressionKind::Upcase(key)
            | ExpressionKind::Downcase(key)
            | ExpressionKind::Length(key)
            | ExpressionKind::Round(key)
            | ExpressionKind::Floor(key)
            | ExpressionKind::Ceil(key)
            | ExpressionKind::Abs(key)
            | ExpressionKind::Sqrt(key)
            | ExpressionKind::NaturalLog(key)
            | ExpressionKind::Log10(key)
            | ExpressionKind::Log2(key)
            | ExpressionKind::Sin(key)
            | ExpressionKind::Cos(key)
            | ExpressionKind::Tan(key)
            | ExpressionKind::Asin(key)
            | ExpressionKind::Acos(key)
            | ExpressionKind::Atan(key) => children.push(key.as_ref()),
            ExpressionKind::FeatureState { key, default } => {
                children.push(key.as_ref());
                if let Some(default) = default {
                    children.push(default.as_ref());
                }
            }
            ExpressionKind::Let { bindings, body } => {
                for (_, binding) in bindings {
                    children.push(binding);
                }
                children.push(body.as_ref());
            }
            ExpressionKind::All(expressions)
            | ExpressionKind::Any(expressions)
            | ExpressionKind::Coalesce(expressions)
            | ExpressionKind::Concat(expressions)
            | ExpressionKind::Add(expressions)
            | ExpressionKind::Multiply(expressions)
            | ExpressionKind::Min(expressions)
            | ExpressionKind::Max(expressions) => {
                children.extend(expressions.iter());
            }
            ExpressionKind::Not(child)
            | ExpressionKind::Equal(child, _)
            | ExpressionKind::NotEqual(child, _)
            | ExpressionKind::Less(child, _)
            | ExpressionKind::LessEqual(child, _)
            | ExpressionKind::Greater(child, _)
            | ExpressionKind::GreaterEqual(child, _) => {
                children.push(child.as_ref());
                match &self.kind {
                    ExpressionKind::Equal(_, right)
                    | ExpressionKind::NotEqual(_, right)
                    | ExpressionKind::Less(_, right)
                    | ExpressionKind::LessEqual(_, right)
                    | ExpressionKind::Greater(_, right)
                    | ExpressionKind::GreaterEqual(_, right) => children.push(right.as_ref()),
                    _ => {}
                }
            }
            ExpressionKind::Case {
                branches,
                otherwise,
            } => {
                for (condition, output) in branches {
                    children.push(condition);
                    children.push(output);
                }
                children.push(otherwise.as_ref());
            }
            ExpressionKind::Match {
                input,
                labels,
                otherwise,
            } => {
                children.push(input.as_ref());
                for (_, output) in labels {
                    children.push(output);
                }
                children.push(otherwise.as_ref());
            }
            ExpressionKind::Step {
                input,
                first_output,
                stops,
            } => {
                children.push(input.as_ref());
                children.push(first_output.as_ref());
                for (_, output) in stops {
                    children.push(output);
                }
            }
            ExpressionKind::Interpolate { input, stops, .. } => {
                children.push(input.as_ref());
                for (_, output) in stops {
                    children.push(output);
                }
            }
            ExpressionKind::Rgb { red, green, blue } => {
                children.push(red.as_ref());
                children.push(green.as_ref());
                children.push(blue.as_ref());
            }
            ExpressionKind::Rgba {
                red,
                green,
                blue,
                alpha,
            } => {
                children.push(red.as_ref());
                children.push(green.as_ref());
                children.push(blue.as_ref());
                children.push(alpha.as_ref());
            }
            ExpressionKind::At { input, index } => {
                children.push(input.as_ref());
                children.push(index.as_ref());
            }
            ExpressionKind::In { needle, haystack } => {
                children.push(needle.as_ref());
                children.push(haystack.as_ref());
            }
            ExpressionKind::IndexOf { haystack, needle } => {
                children.push(haystack.as_ref());
                children.push(needle.as_ref());
            }
            ExpressionKind::Slice { input, start, end } => {
                children.push(input.as_ref());
                children.push(start.as_ref());
                if let Some(end) = end {
                    children.push(end.as_ref());
                }
            }
            ExpressionKind::Subtract {
                minuend,
                subtrahend,
            } => {
                children.push(minuend.as_ref());
                children.push(subtrahend.as_ref());
            }
            ExpressionKind::Divide { dividend, divisor } => {
                children.push(dividend.as_ref());
                children.push(divisor.as_ref());
            }
            ExpressionKind::Modulo { dividend, divisor } => {
                children.push(dividend.as_ref());
                children.push(divisor.as_ref());
            }
            ExpressionKind::Power { base, exponent } => {
                children.push(base.as_ref());
                children.push(exponent.as_ref());
            }
            ExpressionKind::Format(sections) => {
                for section in sections {
                    children.push(&section.value);
                }
            }
            ExpressionKind::Unsupported { arguments, .. } => {
                children.extend(arguments.iter());
            }
        }
        children
    }
}

/// Whether a JSON value is an expression array: an array whose first member
/// is a known operator name.
///
/// A bare array such as `["Open Sans Regular", "Noto Sans"]` is a constant
/// list, not an expression, which is why the operator name is checked rather
/// than merely the first member's type.
pub fn is_expression_array(json_value: &JsonValue) -> bool {
    json_value
        .as_array()
        .and_then(|array| array.first())
        .and_then(JsonValue::as_str)
        .is_some_and(is_known_operator)
}

/// Whether a string names an expression operator.
pub fn is_known_operator(operator: &str) -> bool {
    matches!(
        operator,
        "literal"
            | "get"
            | "has"
            | "geometry-type"
            | "id"
            | "properties"
            | "zoom"
            | "global-state"
            | "feature-state"
            | "let"
            | "var"
            | "all"
            | "any"
            | "!"
            | "=="
            | "!="
            | "<"
            | "<="
            | ">"
            | ">="
            | "case"
            | "match"
            | "coalesce"
            | "step"
            | "interpolate"
            | "to-string"
            | "to-number"
            | "to-boolean"
            | "to-color"
            | "rgb"
            | "rgba"
            | "concat"
            | "upcase"
            | "downcase"
            | "length"
            | "at"
            | "in"
            | "index-of"
            | "slice"
            | "+"
            | "-"
            | "*"
            | "/"
            | "%"
            | "^"
            | "min"
            | "max"
            | "round"
            | "floor"
            | "ceil"
            | "abs"
            | "sqrt"
            | "ln"
            | "log10"
            | "log2"
            | "sin"
            | "cos"
            | "tan"
            | "asin"
            | "acos"
            | "atan"
            | "pi"
            | "e"
            | "ln2"
            | "format"
            | "heatmap-density"
            | "line-progress"
            | "accumulated"
            | "within"
            | "distance"
            | "is-supported-script"
            | "number-format"
            | "interpolate-hcl"
            | "interpolate-lab"
            | "collator"
            | "resolved-locale"
    )
}

fn parse_array(array: &[JsonValue]) -> Result<Expression, ExpressionError> {
    let Some(operator) = array.first().and_then(JsonValue::as_str) else {
        return Err(ExpressionError::new(
            "an expression array must start with an operator string",
        ));
    };
    let arguments = &array[1..];
    let kind = match operator {
        "literal" => {
            expect_exact_arity(operator, arguments, 1)?;
            ExpressionKind::Literal(Value::from(&arguments[0]))
        }
        "get" => {
            expect_arity(operator, arguments, 1, 2)?;
            ExpressionKind::Get {
                key: Box::new(parse_argument(arguments, 0)?),
                object: match arguments.get(1) {
                    Some(object) => Some(Box::new(Expression::parse(object)?)),
                    None => None,
                },
            }
        }
        "has" => {
            expect_exact_arity(operator, arguments, 1)?;
            ExpressionKind::Has {
                key: Box::new(parse_argument(arguments, 0)?),
            }
        }
        "geometry-type" => {
            expect_exact_arity(operator, arguments, 0)?;
            ExpressionKind::GeometryType
        }
        "id" => {
            expect_exact_arity(operator, arguments, 0)?;
            ExpressionKind::FeatureId
        }
        "properties" => {
            expect_exact_arity(operator, arguments, 0)?;
            ExpressionKind::Properties
        }
        "zoom" => {
            expect_exact_arity(operator, arguments, 0)?;
            ExpressionKind::Zoom
        }
        "global-state" => {
            expect_exact_arity(operator, arguments, 1)?;
            ExpressionKind::GlobalState {
                key: Box::new(parse_argument(arguments, 0)?),
            }
        }
        "feature-state" => {
            expect_arity(operator, arguments, 1, 2)?;
            ExpressionKind::FeatureState {
                key: Box::new(parse_argument(arguments, 0)?),
                default: match arguments.get(1) {
                    Some(default) => Some(Box::new(Expression::parse(default)?)),
                    None => None,
                },
            }
        }
        "let" => parse_let(arguments)?,
        "var" => {
            expect_exact_arity(operator, arguments, 1)?;
            ExpressionKind::Variable(required_string(arguments, 0, operator)?)
        }
        "all" => ExpressionKind::All(parse_arguments(arguments)?),
        "any" => ExpressionKind::Any(parse_arguments(arguments)?),
        "!" => {
            expect_exact_arity(operator, arguments, 1)?;
            ExpressionKind::Not(Box::new(parse_argument(arguments, 0)?))
        }
        "==" => binary(operator, arguments, ExpressionKind::Equal)?,
        "!=" => binary(operator, arguments, ExpressionKind::NotEqual)?,
        "<" => binary(operator, arguments, ExpressionKind::Less)?,
        "<=" => binary(operator, arguments, ExpressionKind::LessEqual)?,
        ">" => binary(operator, arguments, ExpressionKind::Greater)?,
        ">=" => binary(operator, arguments, ExpressionKind::GreaterEqual)?,
        "case" => parse_case(arguments)?,
        "match" => parse_match(arguments)?,
        "coalesce" => {
            if arguments.is_empty() {
                return Err(ExpressionError::new("coalesce needs at least one argument"));
            }
            ExpressionKind::Coalesce(parse_arguments(arguments)?)
        }
        "step" => parse_step(arguments)?,
        "interpolate" => parse_interpolate(arguments)?,
        "to-string" => unary(operator, arguments, ExpressionKind::ToString)?,
        "to-number" => unary(operator, arguments, ExpressionKind::ToNumber)?,
        "to-boolean" => unary(operator, arguments, ExpressionKind::ToBoolean)?,
        "to-color" => unary(operator, arguments, ExpressionKind::ToColor)?,
        "rgb" => {
            expect_exact_arity(operator, arguments, 3)?;
            ExpressionKind::Rgb {
                red: Box::new(parse_argument(arguments, 0)?),
                green: Box::new(parse_argument(arguments, 1)?),
                blue: Box::new(parse_argument(arguments, 2)?),
            }
        }
        "rgba" => {
            expect_exact_arity(operator, arguments, 4)?;
            ExpressionKind::Rgba {
                red: Box::new(parse_argument(arguments, 0)?),
                green: Box::new(parse_argument(arguments, 1)?),
                blue: Box::new(parse_argument(arguments, 2)?),
                alpha: Box::new(parse_argument(arguments, 3)?),
            }
        }
        "concat" => {
            if arguments.is_empty() {
                return Err(ExpressionError::new("concat needs at least one argument"));
            }
            ExpressionKind::Concat(parse_arguments(arguments)?)
        }
        "upcase" => unary(operator, arguments, ExpressionKind::Upcase)?,
        "downcase" => unary(operator, arguments, ExpressionKind::Downcase)?,
        "length" => unary(operator, arguments, ExpressionKind::Length)?,
        "at" => {
            expect_exact_arity(operator, arguments, 2)?;
            ExpressionKind::At {
                input: Box::new(parse_argument(arguments, 0)?),
                index: Box::new(parse_argument(arguments, 1)?),
            }
        }
        "in" => {
            expect_exact_arity(operator, arguments, 2)?;
            ExpressionKind::In {
                needle: Box::new(parse_argument(arguments, 0)?),
                haystack: Box::new(parse_argument(arguments, 1)?),
            }
        }
        "index-of" => {
            expect_exact_arity(operator, arguments, 2)?;
            ExpressionKind::IndexOf {
                haystack: Box::new(parse_argument(arguments, 0)?),
                needle: Box::new(parse_argument(arguments, 1)?),
            }
        }
        "slice" => {
            expect_arity(operator, arguments, 2, 3)?;
            ExpressionKind::Slice {
                input: Box::new(parse_argument(arguments, 0)?),
                start: Box::new(parse_argument(arguments, 1)?),
                end: match arguments.get(2) {
                    Some(end) => Some(Box::new(Expression::parse(end)?)),
                    None => None,
                },
            }
        }
        "+" => {
            require_at_least(operator, arguments, 1)?;
            ExpressionKind::Add(parse_arguments(arguments)?)
        }
        "-" => {
            expect_exact_arity(operator, arguments, 2)?;
            ExpressionKind::Subtract {
                minuend: Box::new(parse_argument(arguments, 0)?),
                subtrahend: Box::new(parse_argument(arguments, 1)?),
            }
        }
        "*" => {
            require_at_least(operator, arguments, 1)?;
            ExpressionKind::Multiply(parse_arguments(arguments)?)
        }
        "/" => {
            expect_exact_arity(operator, arguments, 2)?;
            ExpressionKind::Divide {
                dividend: Box::new(parse_argument(arguments, 0)?),
                divisor: Box::new(parse_argument(arguments, 1)?),
            }
        }
        "%" => {
            expect_exact_arity(operator, arguments, 2)?;
            ExpressionKind::Modulo {
                dividend: Box::new(parse_argument(arguments, 0)?),
                divisor: Box::new(parse_argument(arguments, 1)?),
            }
        }
        "^" => {
            expect_exact_arity(operator, arguments, 2)?;
            ExpressionKind::Power {
                base: Box::new(parse_argument(arguments, 0)?),
                exponent: Box::new(parse_argument(arguments, 1)?),
            }
        }
        "min" => {
            require_at_least(operator, arguments, 1)?;
            ExpressionKind::Min(parse_arguments(arguments)?)
        }
        "max" => {
            require_at_least(operator, arguments, 1)?;
            ExpressionKind::Max(parse_arguments(arguments)?)
        }
        "round" => unary(operator, arguments, ExpressionKind::Round)?,
        "floor" => unary(operator, arguments, ExpressionKind::Floor)?,
        "ceil" => unary(operator, arguments, ExpressionKind::Ceil)?,
        "abs" => unary(operator, arguments, ExpressionKind::Abs)?,
        "sqrt" => unary(operator, arguments, ExpressionKind::Sqrt)?,
        "ln" => unary(operator, arguments, ExpressionKind::NaturalLog)?,
        "log10" => unary(operator, arguments, ExpressionKind::Log10)?,
        "log2" => unary(operator, arguments, ExpressionKind::Log2)?,
        "sin" => unary(operator, arguments, ExpressionKind::Sin)?,
        "cos" => unary(operator, arguments, ExpressionKind::Cos)?,
        "tan" => unary(operator, arguments, ExpressionKind::Tan)?,
        "asin" => unary(operator, arguments, ExpressionKind::Asin)?,
        "acos" => unary(operator, arguments, ExpressionKind::Acos)?,
        "atan" => unary(operator, arguments, ExpressionKind::Atan)?,
        "pi" => {
            expect_exact_arity(operator, arguments, 0)?;
            ExpressionKind::Pi
        }
        "e" => {
            expect_exact_arity(operator, arguments, 0)?;
            ExpressionKind::Euler
        }
        "ln2" => {
            expect_exact_arity(operator, arguments, 0)?;
            ExpressionKind::Ln2
        }
        "format" => ExpressionKind::Format(parse_format(arguments)?),
        unsupported => ExpressionKind::Unsupported {
            operator: unsupported.to_string(),
            arguments: parse_arguments(arguments)?,
        },
    };
    Ok(Expression { kind })
}

fn parse_argument(arguments: &[JsonValue], index: usize) -> Result<Expression, ExpressionError> {
    let argument = arguments.get(index).ok_or_else(|| {
        ExpressionError::new(format!("expression argument {} is missing", index + 1))
    })?;
    Expression::parse(argument)
}

fn parse_arguments(arguments: &[JsonValue]) -> Result<Vec<Expression>, ExpressionError> {
    arguments.iter().map(Expression::parse).collect()
}

fn required_string(
    arguments: &[JsonValue],
    index: usize,
    operator: &str,
) -> Result<String, ExpressionError> {
    arguments
        .get(index)
        .and_then(JsonValue::as_str)
        .map(str::to_string)
        .ok_or_else(|| {
            ExpressionError::new(format!(
                "{operator} argument {} must be a string",
                index + 1
            ))
        })
}

fn expect_exact_arity(
    operator: &str,
    arguments: &[JsonValue],
    expected: usize,
) -> Result<(), ExpressionError> {
    if arguments.len() == expected {
        Ok(())
    } else {
        Err(ExpressionError::new(format!(
            "{operator} takes {expected} argument(s), not {}",
            arguments.len()
        )))
    }
}

fn expect_arity(
    operator: &str,
    arguments: &[JsonValue],
    minimum: usize,
    maximum: usize,
) -> Result<(), ExpressionError> {
    if (minimum..=maximum).contains(&arguments.len()) {
        Ok(())
    } else {
        Err(ExpressionError::new(format!(
            "{operator} takes {minimum} to {maximum} arguments, not {}",
            arguments.len()
        )))
    }
}

fn require_at_least(
    operator: &str,
    arguments: &[JsonValue],
    minimum: usize,
) -> Result<(), ExpressionError> {
    if arguments.len() >= minimum {
        Ok(())
    } else {
        Err(ExpressionError::new(format!(
            "{operator} takes at least {minimum} argument(s)"
        )))
    }
}

fn unary(
    operator: &str,
    arguments: &[JsonValue],
    constructor: impl FnOnce(Box<Expression>) -> ExpressionKind,
) -> Result<ExpressionKind, ExpressionError> {
    expect_exact_arity(operator, arguments, 1)?;
    Ok(constructor(Box::new(parse_argument(arguments, 0)?)))
}

fn binary(
    operator: &str,
    arguments: &[JsonValue],
    constructor: impl FnOnce(Box<Expression>, Box<Expression>) -> ExpressionKind,
) -> Result<ExpressionKind, ExpressionError> {
    expect_exact_arity(operator, arguments, 2)?;
    Ok(constructor(
        Box::new(parse_argument(arguments, 0)?),
        Box::new(parse_argument(arguments, 1)?),
    ))
}

fn parse_let(arguments: &[JsonValue]) -> Result<ExpressionKind, ExpressionError> {
    if arguments.len() < 3 || arguments.len().is_multiple_of(2) {
        return Err(ExpressionError::new(
            "let takes name/expression pairs followed by a body expression",
        ));
    }
    let mut bindings = Vec::new();
    let binding_count = (arguments.len() - 1) / 2;
    for binding_index in 0..binding_count {
        let name = required_string(arguments, binding_index * 2, "let")?;
        let value = parse_argument(arguments, binding_index * 2 + 1)?;
        bindings.push((name, value));
    }
    let body = parse_argument(arguments, arguments.len() - 1)?;
    Ok(ExpressionKind::Let {
        bindings,
        body: Box::new(body),
    })
}

fn parse_case(arguments: &[JsonValue]) -> Result<ExpressionKind, ExpressionError> {
    if arguments.len() < 3 || arguments.len().is_multiple_of(2) {
        return Err(ExpressionError::new(
            "case takes condition/output pairs followed by a fallback expression",
        ));
    }
    let mut branches = Vec::new();
    let branch_count = (arguments.len() - 1) / 2;
    for branch_index in 0..branch_count {
        branches.push((
            parse_argument(arguments, branch_index * 2)?,
            parse_argument(arguments, branch_index * 2 + 1)?,
        ));
    }
    Ok(ExpressionKind::Case {
        branches,
        otherwise: Box::new(parse_argument(arguments, arguments.len() - 1)?),
    })
}

fn parse_match(arguments: &[JsonValue]) -> Result<ExpressionKind, ExpressionError> {
    if arguments.len() < 4 || (arguments.len() - 1).is_multiple_of(2) {
        return Err(ExpressionError::new(
            "match takes an input, label/output pairs and a fallback expression",
        ));
    }
    let input = parse_argument(arguments, 0)?;
    let mut labels = Vec::new();
    let pair_count = (arguments.len() - 2) / 2;
    for pair_index in 0..pair_count {
        let label_value = arguments
            .get(1 + pair_index * 2)
            .ok_or_else(|| ExpressionError::new("match label is missing"))?;
        let label_group = match label_value {
            JsonValue::Array(array) => array.iter().map(Value::from).collect(),
            scalar => vec![Value::from(scalar)],
        };
        let output = parse_argument(arguments, 2 + pair_index * 2)?;
        labels.push((label_group, output));
    }
    Ok(ExpressionKind::Match {
        input: Box::new(input),
        labels,
        otherwise: Box::new(parse_argument(arguments, arguments.len() - 1)?),
    })
}

fn parse_step(arguments: &[JsonValue]) -> Result<ExpressionKind, ExpressionError> {
    if arguments.len() < 2 || !(arguments.len() - 2).is_multiple_of(2) {
        return Err(ExpressionError::new(
            "step takes an input, the value below the first stop, and input/output stop pairs",
        ));
    }
    let input = parse_argument(arguments, 0)?;
    let first_output = parse_argument(arguments, 1)?;
    let stops = parse_stop_pairs(arguments, "step")?;
    Ok(ExpressionKind::Step {
        input: Box::new(input),
        first_output: Box::new(first_output),
        stops,
    })
}

fn parse_interpolate(arguments: &[JsonValue]) -> Result<ExpressionKind, ExpressionError> {
    if arguments.len() < 4 || !(arguments.len() - 2).is_multiple_of(2) {
        return Err(ExpressionError::new(
            "interpolate takes an interpolation, an input, and input/output stop pairs",
        ));
    }
    let interpolation = parse_interpolation(&arguments[0])?;
    let input = parse_argument(arguments, 1)?;
    let stops = parse_stop_pairs(arguments, "interpolate")?;
    if stops.len() < 2 {
        return Err(ExpressionError::new("interpolate needs at least two stops"));
    }
    Ok(ExpressionKind::Interpolate {
        interpolation,
        input: Box::new(input),
        stops,
    })
}

fn parse_stop_pairs(
    arguments: &[JsonValue],
    operator: &str,
) -> Result<Vec<(f64, Expression)>, ExpressionError> {
    let mut stops = Vec::new();
    let mut previous_input: Option<f64> = None;
    let mut index = 2;
    while index + 1 < arguments.len() {
        let stop_input = arguments[index].as_f64().ok_or_else(|| {
            ExpressionError::new(format!(
                "{operator} stop inputs must be numeric literals (found {})",
                arguments[index]
            ))
        })?;
        if previous_input.is_some_and(|previous| stop_input <= previous) {
            return Err(ExpressionError::new(format!(
                "{operator} stop inputs must be strictly ascending"
            )));
        }
        previous_input = Some(stop_input);
        let output = Expression::parse(&arguments[index + 1])?;
        stops.push((stop_input, output));
        index += 2;
    }
    Ok(stops)
}

fn parse_interpolation(json_value: &JsonValue) -> Result<Interpolation, ExpressionError> {
    if let Some(name) = json_value.as_str() {
        return match name {
            "linear" => Ok(Interpolation::Linear),
            other => Err(ExpressionError::new(format!(
                "interpolation {other:?} is not one this phase implements"
            ))),
        };
    }
    let array = json_value
        .as_array()
        .ok_or_else(|| ExpressionError::new("interpolation must be an array or a string"))?;
    let Some(name) = array.first().and_then(JsonValue::as_str) else {
        return Err(ExpressionError::new(
            "interpolation must start with its name",
        ));
    };
    let arguments = &array[1..];
    match name {
        "linear" => {
            expect_exact_arity("linear", arguments, 0)?;
            Ok(Interpolation::Linear)
        }
        "exponential" => {
            expect_exact_arity("exponential", arguments, 1)?;
            let base = arguments[0].as_f64().ok_or_else(|| {
                ExpressionError::new("exponential interpolation needs a numeric base")
            })?;
            if base <= 0.0 {
                return Err(ExpressionError::new(
                    "exponential interpolation base must be positive",
                ));
            }
            Ok(Interpolation::Exponential { base })
        }
        "cubic-bezier" => {
            expect_exact_arity("cubic-bezier", arguments, 4)?;
            let coordinates: Option<Vec<f64>> = arguments.iter().map(JsonValue::as_f64).collect();
            let coordinates = coordinates
                .ok_or_else(|| ExpressionError::new("cubic-bezier needs four numbers"))?;
            Ok(Interpolation::CubicBezier {
                x1: coordinates[0],
                y1: coordinates[1],
                x2: coordinates[2],
                y2: coordinates[3],
            })
        }
        other => Err(ExpressionError::new(format!(
            "interpolation {other:?} is not one this phase implements"
        ))),
    }
}

fn parse_format(arguments: &[JsonValue]) -> Result<Vec<FormatSection>, ExpressionError> {
    let mut sections: Vec<FormatSection> = Vec::new();
    for argument in arguments {
        match argument {
            JsonValue::Object(options) => {
                let Some(section) = sections.last_mut() else {
                    return Err(ExpressionError::new(
                        "a format section's options must follow its text",
                    ));
                };
                if let Some(scale) = options.get("scale") {
                    section.scale = Some(Expression::parse(scale)?);
                }
                if let Some(font) = options.get("font") {
                    section.font = Some(Expression::parse(font)?);
                }
                if let Some(text_color) = options.get("text-color") {
                    section.text_color = Some(Expression::parse(text_color)?);
                }
            }
            scalar => sections.push(FormatSection {
                value: Expression::parse(scalar)?,
                scale: None,
                font: None,
                text_color: None,
            }),
        }
    }
    if sections.is_empty() {
        return Err(ExpressionError::new("format needs at least one section"));
    }
    Ok(sections)
}

fn analyze(expression: &Expression) -> ExpressionAnalysis {
    match &expression.kind {
        ExpressionKind::Literal(_)
        | ExpressionKind::GeometryType
        | ExpressionKind::FeatureId
        | ExpressionKind::Properties
        | ExpressionKind::Variable(_)
        | ExpressionKind::Pi
        | ExpressionKind::Euler
        | ExpressionKind::Ln2 => ExpressionAnalysis::Constant,
        ExpressionKind::Zoom => ExpressionAnalysis::Camera,
        ExpressionKind::Get { key, object } => {
            let mut analysis = analyze(key);
            if let Some(object) = object {
                analysis = combine_analysis(analysis, analyze(object));
            }
            combine_analysis(ExpressionAnalysis::DataDriven, analysis)
        }
        ExpressionKind::Has { key } => {
            combine_analysis(ExpressionAnalysis::DataDriven, analyze(key))
        }
        ExpressionKind::GlobalState { key } | ExpressionKind::FeatureState { key, .. } => {
            combine_analysis(ExpressionAnalysis::DataDriven, analyze(key))
        }
        ExpressionKind::Unsupported { .. } => ExpressionAnalysis::DataDriven,
        _ => {
            let mut analysis = ExpressionAnalysis::Constant;
            for child in expression.children() {
                analysis = combine_analysis(analysis, analyze(child));
            }
            analysis
        }
    }
}

fn combine_analysis(first: ExpressionAnalysis, second: ExpressionAnalysis) -> ExpressionAnalysis {
    use ExpressionAnalysis::{Camera, CameraAndData, Constant, DataDriven};
    match (first, second) {
        (Constant, other) | (other, Constant) => other,
        (Camera, Camera) | (DataDriven, DataDriven) => first,
        (Camera, DataDriven) | (DataDriven, Camera) => CameraAndData,
        (CameraAndData, _) | (_, CameraAndData) => CameraAndData,
    }
}

fn validate_zoom(
    expression: &Expression,
    zoom_is_direct_input: bool,
) -> Result<(), ExpressionError> {
    match &expression.kind {
        ExpressionKind::Zoom => {
            if zoom_is_direct_input {
                Ok(())
            } else {
                Err(ExpressionError::new(
                    "[\"zoom\"] may only be the direct input of a top-level step or interpolate",
                ))
            }
        }
        ExpressionKind::Step {
            input,
            first_output,
            stops,
        } => {
            validate_zoom(input, true)?;
            validate_zoom(first_output, false)?;
            for (_, output) in stops {
                validate_zoom(output, false)?;
            }
            Ok(())
        }
        ExpressionKind::Interpolate { input, stops, .. } => {
            validate_zoom(input, true)?;
            for (_, output) in stops {
                validate_zoom(output, false)?;
            }
            Ok(())
        }
        _ => {
            for child in expression.children() {
                validate_zoom(child, false)?;
            }
            Ok(())
        }
    }
}
