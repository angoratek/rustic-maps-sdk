//! The MapLibre style-specification version-8 document model.
//!
//! The crate parses a style document into a renderer-neutral model: the
//! sources, the ordered layers, each layer's layout and paint properties, its
//! filter, and the typed expression tree those properties evaluate. It is
//! pure CPU and JSON; nothing here touches a GPU, a provider, the network or a
//! clock, so it builds on native targets and on `wasm32-unknown-unknown`
//! alike.
//!
//! # What a caller gets
//!
//! - [`document::MapStyleDocument::parse_json`] parses a whole version-8
//!   style. Unknown and unimplemented properties are not silently dropped:
//!   [`document::MapStyleDocument::unsupported`] names each one with the layer
//!   or source that carries it.
//! - [`expression::Expression`] is the one expression tree. Modern array
//!   expressions and legacy `{stops, type, base, property, default}`
//!   functions normalize into it, and [`expression::Expression::evaluate`]
//!   resolves it against a [`expression::EvaluationContext`].
//! - [`filter::parse_filter`] accepts both the legacy filter array and the
//!   expression form and returns the same boolean expression tree.
//! - [`color::Color::from_css`] parses the CSS colour forms a style carries.
//! - [`compiled::compile_style_document`] turns a parsed document into a
//!   [`compiled::CompiledStyle`] of render plans: document order, resolved zoom
//!   spans, kind-specific paint and layout properties and the filter tree,
//!   ready to evaluate at a frame zoom and a feature without re-parsing.
//!
//! # Evaluation split
//!
//! Paint properties are evaluated at the frame's fractional zoom; layout
//! properties are evaluated at the integer zoom the specification requires.
//! The caller supplies the zoom it wants through the evaluation context. A
//! property that fails at runtime resolves to the documented default in
//! [`defaults`], never to a panic.

#![cfg_attr(not(test), deny(clippy::expect_used, clippy::unwrap_used))]

pub mod color;
pub mod compiled;
pub mod defaults;
pub mod document;
pub mod error;
pub mod expression;
pub mod filter;

mod legacy_function;
pub mod types;

pub use color::Color;
pub use compiled::{
    CompiledBackground, CompiledCirclePaint, CompiledFillPaint, CompiledGeoJsonSource,
    CompiledLayer, CompiledLayerKind, CompiledLayout, CompiledLinePaint, CompiledOpaqueSource,
    CompiledPaint, CompiledRasterSource, CompiledShield, CompiledSource, CompiledSourceKind,
    CompiledSprite, CompiledStyle, CompiledSymbolPaint, CompiledVectorSource, ResolvedCirclePaint,
    ResolvedFillPaint, ResolvedLinePaint, ResolvedPaint, ResolvedSymbolLayout, ResolvedSymbolPaint,
    TileScheme, compile_style_document,
};
pub use document::{
    GeoJsonSourceSpec, LayerLayout, LayerPaint, LayerType, MapStyleDocument, OpaqueSourceSpec,
    Projection, RasterSourceSpec, SourceSpec, SpriteSpec, StyleLayer, VectorLayerSpec,
    VectorSourceSpec,
};
pub use error::{
    ColorParseError, EvaluationError, ExpressionError, FilterError, StyleError,
    UnsupportedProperty, UnsupportedScope,
};
pub use expression::{
    EvaluationContext, Expression, ExpressionAnalysis, ExpressionKind, GeometryType, Interpolation,
};
pub use filter::parse_filter;
pub use types::{
    CirclePitchAlignment, CirclePitchScale, IconTextFit, LineCap, LineJoin, Property,
    PropertyValue, ShieldShape, ShieldSpec, SymbolPlacement, TextAnchor, TextJustify,
    TextPitchAlignment, TextRotationAlignment, TextTransform, TransitionSpec, TranslateAnchor,
    Value, Visibility,
};
