//! Parse, validation and evaluation diagnostics for style documents.
//!
//! Every error is a value, never a panic: a style author receives the
//! operator or property name, the failing text and a sentence explaining the
//! rule that was broken. Evaluation failures are separated from parse
//! failures because the renderer treats a property that fails at runtime as
//! "use the property's documented default" rather than as an unloadable
//! document.

use thiserror::Error;

/// A CSS colour that cannot be parsed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("invalid colour {text:?}: {detail}")]
pub struct ColorParseError {
    /// The text that was offered as a colour.
    pub text: String,
    /// The rule the text violated.
    pub detail: String,
}

impl ColorParseError {
    /// Builds a colour diagnostic.
    pub fn new(text: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            detail: detail.into(),
        }
    }
}

/// An expression that cannot be parsed or violates the specification's
/// static rules.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{detail}")]
pub struct ExpressionError {
    /// The rule the expression violated.
    pub detail: String,
}

impl ExpressionError {
    /// Builds an expression diagnostic.
    pub fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}

/// A filter that cannot be normalised into a boolean expression.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{detail}")]
pub struct FilterError {
    /// The rule the filter violated.
    pub detail: String,
}

impl FilterError {
    /// Builds a filter diagnostic.
    pub fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}

/// An expression that cannot be evaluated in a given context.
///
/// The renderer catches this and resolves the property's documented default,
/// so an evaluation error never stops a frame.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{detail}")]
pub struct EvaluationError {
    /// The reason the expression produced no value.
    pub detail: String,
}

impl EvaluationError {
    /// Builds an evaluation diagnostic.
    pub fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}

/// A whole style document that cannot be parsed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum StyleError {
    /// The document text is not valid JSON.
    #[error("style document is not valid JSON: {detail}")]
    Json {
        /// The underlying JSON parser message.
        detail: String,
    },
    /// The document root is not a JSON object.
    #[error("style document must be a JSON object")]
    DocumentNotObject,
    /// The document declares a version other than 8.
    #[error("style document version {version} is not supported; version 8 is required")]
    UnsupportedVersion {
        /// The version the document declares.
        version: u64,
    },
    /// A required structural member is absent or has the wrong type.
    #[error("style document member {member:?} is invalid: {detail}")]
    Member {
        /// The member name.
        member: String,
        /// The rule the member violated.
        detail: String,
    },
    /// A layer has no `id` or no `type`.
    #[error("layer at index {layer_index} is invalid: {detail}")]
    Layer {
        /// The layer's document position.
        layer_index: usize,
        /// The rule the layer violated.
        detail: String,
    },
    /// A source declaration is invalid.
    #[error("source {source_id:?} is invalid: {detail}")]
    Source {
        /// The source id the document uses.
        source_id: String,
        /// The rule the source violated.
        detail: String,
    },
    /// An expression failed to parse.
    #[error("expression invalid: {0}")]
    Expression(#[from] ExpressionError),
    /// A filter failed to normalise.
    #[error("filter invalid: {0}")]
    Filter(#[from] FilterError),
    /// A colour failed to parse.
    #[error("colour invalid: {0}")]
    Color(#[from] ColorParseError),
}

/// The part of a document an unsupported property belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnsupportedScope {
    /// A root property.
    Document,
    /// A layer property, named by the layer's `id`.
    Layer {
        /// The layer id.
        layer_id: String,
    },
    /// A source property, named by the source id.
    Source {
        /// The source id.
        source_id: String,
    },
}

impl UnsupportedScope {
    /// A short human-readable description of the scope.
    pub fn description(&self) -> String {
        match self {
            Self::Document => "document".to_string(),
            Self::Layer { layer_id } => format!("layer {layer_id:?}"),
            Self::Source { source_id } => format!("source {source_id:?}"),
        }
    }
}

/// A property the parser recognizes but this phase cannot apply.
///
/// Unknown and unimplemented properties are reported rather than silently
/// dropped, so a caller can tell the difference between "the style paints
/// this" and "the style asked for something the SDK does not implement yet".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupportedProperty {
    /// The document part that carries the property.
    pub scope: UnsupportedScope,
    /// The property's JSON member name.
    pub property_name: String,
    /// Why the property is not applied.
    pub detail: String,
}

impl UnsupportedProperty {
    /// Builds a diagnostic for one property.
    pub fn new(
        scope: UnsupportedScope,
        property_name: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            scope,
            property_name: property_name.into(),
            detail: detail.into(),
        }
    }
}
