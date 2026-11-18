//! The value model the expression engine evaluates and the property
//! abstraction a style layer stores.
//!
//! [`Value`] mirrors the JSON value types an expression can produce, plus a
//! resolved [`Color`]. [`Property`] is either a constant of the property's
//! type or an expression that produces one; a constant never enters the
//! evaluator, so a style layer's paint constants stay as cheap to read as the
//! renderer's existing paint fields.

use std::collections::BTreeMap;

use serde_json::{Map, Value as JsonValue};

use crate::color::Color;
use crate::error::{EvaluationError, ExpressionError};
use crate::expression::evaluate::EvaluationContext;
use crate::expression::{Expression, is_expression_array};

/// A value an expression evaluates to.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// JSON `null`.
    Null,
    /// A boolean.
    Boolean(bool),
    /// A number; JSON integers and floating-point values share this variant.
    Number(f64),
    /// A string.
    String(String),
    /// A resolved colour, produced by `rgb`, `rgba`, `to-color` and colour
    /// interpolation.
    Color(Color),
    /// A JSON array.
    Array(Vec<Value>),
    /// A JSON object with deterministic key order.
    Object(BTreeMap<String, Value>),
}

impl Value {
    /// The value as a number, when it is one.
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Self::Number(number) => Some(*number),
            _ => None,
        }
    }

    /// The value as a string slice, when it is a string.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(string) => Some(string),
            _ => None,
        }
    }

    /// The value as a boolean under the specification's truthiness rules:
    /// `null`, `false`, `0`, an empty string and an empty collection are
    /// false; every other value is true.
    pub fn truthy(&self) -> bool {
        match self {
            Self::Null => false,
            Self::Boolean(boolean) => *boolean,
            Self::Number(number) => *number != 0.0 && !number.is_nan(),
            Self::String(string) => !string.is_empty(),
            Self::Color(color) => color.alpha > 0.0,
            Self::Array(array) => !array.is_empty(),
            Self::Object(object) => !object.is_empty(),
        }
    }

    /// Whether the value is JSON `null`.
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// Converts the value into a `serde_json` value.
    pub fn to_json(&self) -> JsonValue {
        match self {
            Self::Null => JsonValue::Null,
            Self::Boolean(boolean) => JsonValue::Bool(*boolean),
            Self::Number(number) => {
                serde_json::Number::from_f64(*number).map_or(JsonValue::Null, JsonValue::Number)
            }
            Self::String(string) => JsonValue::String(string.clone()),
            Self::Color(color) => JsonValue::String(color.to_css_string()),
            Self::Array(array) => JsonValue::Array(array.iter().map(Value::to_json).collect()),
            Self::Object(object) => {
                let mut json_object = Map::new();
                for (key, value) in object {
                    json_object.insert(key.clone(), value.to_json());
                }
                JsonValue::Object(json_object)
            }
        }
    }
}

impl From<&JsonValue> for Value {
    fn from(json_value: &JsonValue) -> Self {
        match json_value {
            JsonValue::Null => Self::Null,
            JsonValue::Bool(boolean) => Self::Boolean(*boolean),
            JsonValue::Number(number) => number.as_f64().map_or(Self::Null, Self::Number),
            JsonValue::String(string) => Self::String(string.clone()),
            JsonValue::Array(array) => Self::Array(array.iter().map(Value::from).collect()),
            JsonValue::Object(object) => {
                let mut values = BTreeMap::new();
                for (key, value) in object {
                    values.insert(key.clone(), Value::from(value));
                }
                Self::Object(values)
            }
        }
    }
}

impl From<&Value> for JsonValue {
    fn from(value: &Value) -> Self {
        value.to_json()
    }
}

impl From<f64> for Value {
    fn from(number: f64) -> Self {
        Self::Number(number)
    }
}

impl From<bool> for Value {
    fn from(boolean: bool) -> Self {
        Self::Boolean(boolean)
    }
}

impl From<&str> for Value {
    fn from(string: &str) -> Self {
        Self::String(string.to_string())
    }
}

impl From<String> for Value {
    fn from(string: String) -> Self {
        Self::String(string)
    }
}

/// A type a property can be resolved to.
pub trait PropertyValue: Clone {
    /// Builds a constant from the JSON text a style document carries.
    fn from_json_constant(json_value: &JsonValue) -> Option<Self>;
    /// Converts an evaluated expression value to this type.
    fn from_value(value: &Value) -> Option<Self>;
}

impl PropertyValue for f64 {
    fn from_json_constant(json_value: &JsonValue) -> Option<Self> {
        json_value.as_f64()
    }

    fn from_value(value: &Value) -> Option<Self> {
        value.as_number()
    }
}

impl PropertyValue for bool {
    fn from_json_constant(json_value: &JsonValue) -> Option<Self> {
        json_value.as_bool()
    }

    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Boolean(boolean) => Some(*boolean),
            _ => None,
        }
    }
}

impl PropertyValue for String {
    fn from_json_constant(json_value: &JsonValue) -> Option<Self> {
        json_value.as_str().map(str::to_string)
    }

    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::String(string) => Some(string.clone()),
            Value::Number(number) => Some(crate::expression::operators::number_to_string(*number)),
            Value::Boolean(boolean) => Some(boolean.to_string()),
            _ => None,
        }
    }
}

impl PropertyValue for Color {
    fn from_json_constant(json_value: &JsonValue) -> Option<Self> {
        match json_value {
            JsonValue::String(string) => Color::from_css(string).ok(),
            JsonValue::Array(array) => {
                let channels: Option<Vec<f64>> = array.iter().map(JsonValue::as_f64).collect();
                let channels = channels?;
                match channels.as_slice() {
                    [red, green, blue] => Some(Color::from_rgb_numbers(*red, *green, *blue, 1.0)),
                    [red, green, blue, alpha] => {
                        Some(Color::from_rgb_numbers(*red, *green, *blue, *alpha))
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Color(color) => Some(*color),
            Value::String(string) => Color::from_css(string).ok(),
            Value::Array(array) => {
                let channels: Option<Vec<f64>> = array.iter().map(Value::as_number).collect();
                let channels = channels?;
                match channels.as_slice() {
                    [red, green, blue] => Some(Color::from_rgb_numbers(*red, *green, *blue, 1.0)),
                    [red, green, blue, alpha] => {
                        Some(Color::from_rgb_numbers(*red, *green, *blue, *alpha))
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }
}

impl PropertyValue for Vec<f64> {
    fn from_json_constant(json_value: &JsonValue) -> Option<Self> {
        json_value
            .as_array()?
            .iter()
            .map(JsonValue::as_f64)
            .collect()
    }

    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Array(array) => array.iter().map(Value::as_number).collect(),
            _ => None,
        }
    }
}

impl PropertyValue for Vec<String> {
    fn from_json_constant(json_value: &JsonValue) -> Option<Self> {
        json_value
            .as_array()?
            .iter()
            .map(|entry| entry.as_str().map(str::to_string))
            .collect()
    }

    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Array(array) => array
                .iter()
                .map(|entry| entry.as_str().map(str::to_string))
                .collect(),
            _ => None,
        }
    }
}

/// A property value that is either a constant or an expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Property<T> {
    /// The value every feature and every zoom level resolves to.
    Constant(T),
    /// An expression evaluated per feature at the frame zoom.
    Expression(Expression),
}

impl<T: PropertyValue> Property<T> {
    /// Parses one property value from the style document.
    ///
    /// An array whose first member is a known expression operator is parsed
    /// as an expression; a legacy `{stops, …}` object is normalised into an
    /// expression; every other JSON value is a constant of the property's
    /// type.
    ///
    /// # Errors
    /// A malformed expression or a value that is not a constant of the
    /// property's type reports [`ExpressionError`].
    pub fn parse(json_value: &JsonValue) -> Result<Self, ExpressionError> {
        let is_legacy_function = json_value
            .as_object()
            .is_some_and(|object| object.contains_key("stops"));
        if is_legacy_function || is_expression_array(json_value) {
            return Ok(Self::Expression(Expression::parse(json_value)?));
        }
        if let Some(constant) = T::from_json_constant(json_value) {
            return Ok(Self::Constant(constant));
        }
        Err(ExpressionError::new(format!(
            "value {json_value} is neither a constant of the property's type nor a known expression"
        )))
    }

    /// Resolves the property in an evaluation context.
    ///
    /// # Errors
    /// An expression that fails or produces a value of the wrong type reports
    /// [`EvaluationError`]; the caller substitutes the property's documented
    /// default.
    pub fn evaluate(&self, context: &EvaluationContext<'_>) -> Result<T, EvaluationError> {
        match self {
            Self::Constant(constant) => Ok(constant.clone()),
            Self::Expression(expression) => {
                let value = expression.evaluate(context)?;
                T::from_value(&value).ok_or_else(|| {
                    EvaluationError::new(format!(
                        "expression produced {value:?}, which is not the property's type"
                    ))
                })
            }
        }
    }

    /// Whether the property is a constant, independent of feature and zoom.
    pub fn is_constant(&self) -> bool {
        matches!(self, Self::Constant(_))
    }
}

/// The timing a style declares for one paint-property transition.
///
/// A transition blends the property's previous value into its new value over
/// `duration_milliseconds`, starting `delay_milliseconds` after the style
/// change. The specification declares per-property blocks such as
/// `fill-opacity-transition`; the root `transition` block supplies the
/// defaults for properties that do not declare their own.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransitionSpec {
    /// How long the blend runs, in milliseconds.
    pub duration_milliseconds: f64,
    /// How long the previous value holds before the blend starts, in
    /// milliseconds.
    pub delay_milliseconds: f64,
}

impl TransitionSpec {
    /// The interpolation fraction at `elapsed_seconds` after the change:
    /// zero during the delay, then linear to one at the end of the duration.
    /// A non-positive duration, a non-finite clock and a clock before the
    /// change all resolve to one (the new value) except a clock inside the
    /// delay window, which resolves to zero (the previous value).
    pub fn progress_at(&self, elapsed_seconds: f64) -> f64 {
        if !elapsed_seconds.is_finite() {
            return 1.0;
        }
        if self.duration_milliseconds <= 0.0 {
            return 1.0;
        }
        if elapsed_seconds <= 0.0 {
            return 0.0;
        }
        let delay_seconds = (self.delay_milliseconds / 1000.0).max(0.0);
        if elapsed_seconds <= delay_seconds {
            return 0.0;
        }
        let duration_seconds = self.duration_milliseconds / 1000.0;
        ((elapsed_seconds - delay_seconds) / duration_seconds).clamp(0.0, 1.0)
    }

    /// How long, in seconds, a change under this timing still has work to do:
    /// the delay plus the duration, or zero when the duration is zero.
    pub fn lifetime_seconds(&self) -> f64 {
        if self.duration_milliseconds <= 0.0 {
            return 0.0;
        }
        ((self.delay_milliseconds.max(0.0) + self.duration_milliseconds) / 1000.0).max(0.0)
    }
}

/// Whether a layer is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    /// The layer is drawn.
    Visible,
    /// The layer is skipped.
    None,
}

impl Visibility {
    /// The specification's spelling of the visibility.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Visible => "visible",
            Self::None => "none",
        }
    }
}

impl PropertyValue for Visibility {
    fn from_json_constant(json_value: &JsonValue) -> Option<Self> {
        match json_value.as_str()? {
            "visible" => Some(Self::Visible),
            "none" => Some(Self::None),
            _ => None,
        }
    }

    fn from_value(value: &Value) -> Option<Self> {
        match value.as_str()? {
            "visible" => Some(Self::Visible),
            "none" => Some(Self::None),
            _ => None,
        }
    }
}

/// How a line's ends are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineCap {
    /// Ends exactly at the line's endpoint.
    Butt,
    /// Rounded ends.
    Round,
    /// Square ends extending half the line width past the endpoint.
    Square,
}

impl LineCap {
    /// The specification's spelling of the cap.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Butt => "butt",
            Self::Round => "round",
            Self::Square => "square",
        }
    }
}

impl PropertyValue for LineCap {
    fn from_json_constant(json_value: &JsonValue) -> Option<Self> {
        match json_value.as_str()? {
            "butt" => Some(Self::Butt),
            "round" => Some(Self::Round),
            "square" => Some(Self::Square),
            _ => None,
        }
    }

    fn from_value(value: &Value) -> Option<Self> {
        Self::from_json_constant(&value.to_json())
    }
}

/// How two line segments meet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineJoin {
    /// A pointed join.
    Miter,
    /// A flattened join.
    Bevel,
    /// A rounded join.
    Round,
}

impl LineJoin {
    /// The specification's spelling of the join.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Miter => "miter",
            Self::Bevel => "bevel",
            Self::Round => "round",
        }
    }
}

impl PropertyValue for LineJoin {
    fn from_json_constant(json_value: &JsonValue) -> Option<Self> {
        match json_value.as_str()? {
            "miter" => Some(Self::Miter),
            "bevel" => Some(Self::Bevel),
            "round" => Some(Self::Round),
            _ => None,
        }
    }

    fn from_value(value: &Value) -> Option<Self> {
        Self::from_json_constant(&value.to_json())
    }
}

/// How a circle's radius behaves when the map is pitched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CirclePitchScale {
    /// The circle lies on the ground, so it shrinks with perspective as the
    /// map is pitched.
    Map,
    /// The circle keeps its radius in screen pixels at every pitch.
    Viewport,
}

impl CirclePitchScale {
    /// The specification's spelling of the scale behaviour.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Map => "map",
            Self::Viewport => "viewport",
        }
    }
}

impl PropertyValue for CirclePitchScale {
    fn from_json_constant(json_value: &JsonValue) -> Option<Self> {
        match json_value.as_str()? {
            "map" => Some(Self::Map),
            "viewport" => Some(Self::Viewport),
            _ => None,
        }
    }

    fn from_value(value: &Value) -> Option<Self> {
        Self::from_json_constant(&value.to_json())
    }
}

/// What a circle is aligned to when the map is pitched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CirclePitchAlignment {
    /// The circle lies flat on the ground, so a pitched camera sees an ellipse.
    Map,
    /// The circle always faces the screen.
    Viewport,
}

impl CirclePitchAlignment {
    /// The specification's spelling of the alignment.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Map => "map",
            Self::Viewport => "viewport",
        }
    }
}

impl PropertyValue for CirclePitchAlignment {
    fn from_json_constant(json_value: &JsonValue) -> Option<Self> {
        match json_value.as_str()? {
            "map" => Some(Self::Map),
            "viewport" => Some(Self::Viewport),
            _ => None,
        }
    }

    fn from_value(value: &Value) -> Option<Self> {
        Self::from_json_constant(&value.to_json())
    }
}

/// The frame of reference a `*-translate` property is measured against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranslateAnchor {
    /// The displacement is relative to the map, so it rotates with the
    /// camera's bearing.
    Map,
    /// The displacement is relative to the viewport, in screen pixels.
    Viewport,
}

impl TranslateAnchor {
    /// The specification's spelling of the anchor.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Map => "map",
            Self::Viewport => "viewport",
        }
    }

    /// Reads the specification's spelling. An unrecognized value is `None`,
    /// so the caller can report it rather than guessing a frame of reference.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "map" => Some(Self::Map),
            "viewport" => Some(Self::Viewport),
            _ => None,
        }
    }
}

impl PropertyValue for TranslateAnchor {
    fn from_json_constant(json_value: &JsonValue) -> Option<Self> {
        Self::from_name(json_value.as_str()?)
    }

    fn from_value(value: &Value) -> Option<Self> {
        Self::from_json_constant(&value.to_json())
    }
}

/// Where a symbol layer places its text block relative to the symbol anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAnchor {
    /// Centered on the anchor.
    Center,
    /// The block's left edge at the anchor.
    Left,
    /// The block's right edge at the anchor.
    Right,
    /// The block's top edge at the anchor.
    Top,
    /// The block's bottom edge at the anchor.
    Bottom,
    /// The block's top-left corner at the anchor.
    TopLeft,
    /// The block's top-right corner at the anchor.
    TopRight,
    /// The block's bottom-left corner at the anchor.
    BottomLeft,
    /// The block's bottom-right corner at the anchor.
    BottomRight,
}

impl TextAnchor {
    /// The specification's spelling of the anchor.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Center => "center",
            Self::Left => "left",
            Self::Right => "right",
            Self::Top => "top",
            Self::Bottom => "bottom",
            Self::TopLeft => "top-left",
            Self::TopRight => "top-right",
            Self::BottomLeft => "bottom-left",
            Self::BottomRight => "bottom-right",
        }
    }
}

impl PropertyValue for TextAnchor {
    fn from_json_constant(json_value: &JsonValue) -> Option<Self> {
        match json_value.as_str()? {
            "center" => Some(Self::Center),
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            "top" => Some(Self::Top),
            "bottom" => Some(Self::Bottom),
            "top-left" => Some(Self::TopLeft),
            "top-right" => Some(Self::TopRight),
            "bottom-left" => Some(Self::BottomLeft),
            "bottom-right" => Some(Self::BottomRight),
            _ => None,
        }
    }

    fn from_value(value: &Value) -> Option<Self> {
        Self::from_json_constant(&value.to_json())
    }
}

impl PropertyValue for Vec<TextAnchor> {
    fn from_json_constant(json_value: &JsonValue) -> Option<Self> {
        json_value
            .as_array()?
            .iter()
            .map(TextAnchor::from_json_constant)
            .collect()
    }

    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Array(array) => array
                .iter()
                .map(|entry| TextAnchor::from_json_constant(&entry.to_json()))
                .collect(),
            _ => None,
        }
    }
}

/// What a point-placed label's rotation is aligned to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextRotationAlignment {
    /// The text rotates with the map's bearing.
    Map,
    /// The text always reads horizontally on screen.
    Viewport,
    /// Map for a line-placed label, viewport for a point-placed one.
    Auto,
}

impl TextRotationAlignment {
    /// The specification's spelling of the alignment.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Map => "map",
            Self::Viewport => "viewport",
            Self::Auto => "auto",
        }
    }
}

impl PropertyValue for TextRotationAlignment {
    fn from_json_constant(json_value: &JsonValue) -> Option<Self> {
        match json_value.as_str()? {
            "map" => Some(Self::Map),
            "viewport" => Some(Self::Viewport),
            "auto" => Some(Self::Auto),
            _ => None,
        }
    }

    fn from_value(value: &Value) -> Option<Self> {
        Self::from_json_constant(&value.to_json())
    }
}

/// How a symbol layer aligns its wrapped lines within the text block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextJustify {
    /// Centered for point placement, matching the anchor's own alignment.
    Auto,
    /// Aligned to the block's left edge.
    Left,
    /// Centered within the block.
    Center,
    /// Aligned to the block's right edge.
    Right,
}

impl TextJustify {
    /// The specification's spelling of the justification.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Left => "left",
            Self::Center => "center",
            Self::Right => "right",
        }
    }
}

impl PropertyValue for TextJustify {
    fn from_json_constant(json_value: &JsonValue) -> Option<Self> {
        match json_value.as_str()? {
            "auto" => Some(Self::Auto),
            "left" => Some(Self::Left),
            "center" => Some(Self::Center),
            "right" => Some(Self::Right),
            _ => None,
        }
    }

    fn from_value(value: &Value) -> Option<Self> {
        Self::from_json_constant(&value.to_json())
    }
}

/// Where a symbol layer places its text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolPlacement {
    /// At the feature's point.
    Point,
    /// Along a line feature.
    Line,
    /// Once at the line's center.
    LineCenter,
}

impl SymbolPlacement {
    /// The specification's spelling of the placement.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Point => "point",
            Self::Line => "line",
            Self::LineCenter => "line-center",
        }
    }
}

impl PropertyValue for SymbolPlacement {
    fn from_json_constant(json_value: &JsonValue) -> Option<Self> {
        match json_value.as_str()? {
            "point" => Some(Self::Point),
            "line" => Some(Self::Line),
            "line-center" => Some(Self::LineCenter),
            _ => None,
        }
    }

    fn from_value(value: &Value) -> Option<Self> {
        Self::from_json_constant(&value.to_json())
    }
}

/// The case transformation a symbol layer applies to its text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextTransform {
    /// Leave the text unchanged.
    None,
    /// Upper-case the text.
    Uppercase,
    /// Lower-case the text.
    Lowercase,
}

impl TextTransform {
    /// The specification's spelling of the transform.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Uppercase => "uppercase",
            Self::Lowercase => "lowercase",
        }
    }
}

impl PropertyValue for TextTransform {
    fn from_json_constant(json_value: &JsonValue) -> Option<Self> {
        match json_value.as_str()? {
            "none" => Some(Self::None),
            "uppercase" => Some(Self::Uppercase),
            "lowercase" => Some(Self::Lowercase),
            _ => None,
        }
    }

    fn from_value(value: &Value) -> Option<Self> {
        Self::from_json_constant(&value.to_json())
    }
}

/// What a symbol layer's text is aligned to when the map is pitched: the
/// ground plane or the screen plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextPitchAlignment {
    /// The text lies on the ground plane, foreshortening as the map is
    /// pitched.
    Map,
    /// The text always faces the screen.
    Viewport,
    /// The placement decides: a point-placed label faces the screen and a
    /// line-placed label lies on the ground, as the specification defines
    /// `auto`.
    Auto,
}

impl TextPitchAlignment {
    /// The specification's spelling of the alignment.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Map => "map",
            Self::Viewport => "viewport",
            Self::Auto => "auto",
        }
    }

    /// The alignment an `auto` declaration resolves to for one placement.
    pub fn resolved_for_placement(self, placement: SymbolPlacement) -> TextPitchAlignment {
        match (self, placement) {
            (Self::Auto, SymbolPlacement::Point) => Self::Viewport,
            (Self::Auto, _) => Self::Map,
            (declared, _) => declared,
        }
    }
}

impl PropertyValue for TextPitchAlignment {
    fn from_json_constant(json_value: &JsonValue) -> Option<Self> {
        match json_value.as_str()? {
            "map" => Some(Self::Map),
            "viewport" => Some(Self::Viewport),
            "auto" => Some(Self::Auto),
            _ => None,
        }
    }

    fn from_value(value: &Value) -> Option<Self> {
        Self::from_json_constant(&value.to_json())
    }
}

/// How a symbol layer stretches its icon to fit the text block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconTextFit {
    /// Draw the icon at its own aspect ratio.
    None,
    /// Stretch the icon across the text block's width.
    Width,
    /// Stretch the icon across the text block's height.
    Height,
    /// Stretch the icon across both the text block's width and height.
    Both,
}

impl IconTextFit {
    /// The specification's spelling of the fit.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Width => "width",
            Self::Height => "height",
            Self::Both => "both",
        }
    }
}

impl PropertyValue for IconTextFit {
    fn from_json_constant(json_value: &JsonValue) -> Option<Self> {
        match json_value.as_str()? {
            "none" => Some(Self::None),
            "width" => Some(Self::Width),
            "height" => Some(Self::Height),
            "both" => Some(Self::Both),
            _ => None,
        }
    }

    fn from_value(value: &Value) -> Option<Self> {
        Self::from_json_constant(&value.to_json())
    }
}

/// The outline a route-reference badge draws around its reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShieldShape {
    /// The two-lobed highway badge: rounded shoulders tapering to a point.
    Shield,
    /// A rectangle with softened corners.
    RoundedRectangle,
}

impl ShieldShape {
    /// The specification's spelling of the shape.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shield => "shield",
            Self::RoundedRectangle => "rounded_rectangle",
        }
    }

    /// Reads the specification's spelling. An unrecognized value is `None`,
    /// so the caller reports it instead of guessing an outline.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "shield" => Some(Self::Shield),
            "rounded_rectangle" => Some(Self::RoundedRectangle),
            _ => None,
        }
    }
}

impl PropertyValue for ShieldShape {
    fn from_json_constant(json_value: &JsonValue) -> Option<Self> {
        Self::from_name(json_value.as_str()?)
    }

    fn from_value(value: &Value) -> Option<Self> {
        Self::from_json_constant(&value.to_json())
    }
}

/// A symbol layer's route-reference badge declaration.
///
/// The badge encloses the reference's measured glyphs at
/// [`ShieldSpec::text_size_pixels`] plus [`ShieldSpec::padding_pixels`], growing
/// past [`ShieldSpec::minimum_size_pixels`] when the reference is longer. It
/// draws behind its own text, in the badge's fill and optional outline, so a
/// route reference reads as a badge rather than as a road name.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShieldSpec {
    /// The badge outline.
    pub shape: ShieldShape,
    /// The badge fill colour.
    pub fill_color: Color,
    /// The outline colour; absent draws no outline.
    pub border_color: Option<Color>,
    /// The outline width in screen pixels; zero draws no outline even when a
    /// colour is declared.
    pub border_width_pixels: f64,
    /// The reference's own text colour inside the badge.
    pub text_color: Color,
    /// The reference's own text size in screen pixels.
    pub text_size_pixels: f64,
    /// The inset between the reference's glyphs and the badge edge, in screen
    /// pixels.
    pub padding_pixels: f64,
    /// The smallest badge width and height in screen pixels, so a short
    /// reference still reads as a badge.
    pub minimum_size_pixels: f64,
}
