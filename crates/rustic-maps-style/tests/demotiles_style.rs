//! The acceptance target: the MapLibre demotiles style parses with zero
//! unsupported properties, and the legacy functions and `match` expression
//! its layers carry evaluate to the values the style was designed around.

use std::collections::BTreeMap;

use rustic_maps_style::{
    Color, EvaluationContext, GeometryType, LayerType, MapStyleDocument, Projection, SourceSpec,
    TextTransform, Value,
};

const DEMOTILES_STYLE_JSON: &str = include_str!("../fixtures/styles/demotiles-style.json");

fn demotiles_document() -> MapStyleDocument {
    MapStyleDocument::parse_json(DEMOTILES_STYLE_JSON).expect("the demotiles style parses")
}

fn properties(entries: &[(&str, &str)]) -> BTreeMap<String, Value> {
    entries
        .iter()
        .map(|(name, value)| ((*name).to_string(), Value::String((*value).to_string())))
        .collect()
}

#[test]
fn the_document_is_version_eight_and_names_itself() {
    let document = demotiles_document();
    assert_eq!(document.version, 8);
    assert_eq!(document.name.as_deref(), Some("MapLibre"));
    assert_eq!(document.projection, Projection::Mercator);
    assert_eq!(document.layers.len(), 8);
    assert_eq!(
        document
            .layers
            .iter()
            .map(|layer| layer.id.as_str())
            .collect::<Vec<_>>(),
        [
            "background",
            "coastline",
            "countries-fill",
            "countries-boundary",
            "geolines",
            "geolines-label",
            "countries-label",
            "crimea-fill",
        ]
    );
}

#[test]
fn every_demotiles_layer_parses_with_zero_unsupported_properties() {
    let document = demotiles_document();
    for layer in &document.layers {
        let unsupported = document.unsupported_properties_on_layer(&layer.id);
        assert!(
            unsupported.is_empty(),
            "layer {:?} reported unsupported properties: {unsupported:?}",
            layer.id
        );
    }
    assert!(
        document.unsupported.is_empty(),
        "the demotiles document reported unsupported properties: {:?}",
        document.unsupported
    );
}

#[test]
fn the_demotiles_sources_are_a_vector_source_and_a_geojson_source() {
    let document = demotiles_document();
    let vector = document.sources.get("maplibre").expect("the vector source");
    assert_eq!(vector.source_type(), "vector");
    let SourceSpec::Vector(vector) = vector else {
        panic!("maplibre is a vector source");
    };
    assert_eq!(
        vector.url.as_deref(),
        Some("https://demotiles.maplibre.org/tiles/tiles.json")
    );

    let geojson = document.sources.get("crimea").expect("the GeoJSON source");
    assert_eq!(geojson.source_type(), "geojson");
    let SourceSpec::GeoJson(geojson) = geojson else {
        panic!("crimea is a GeoJSON source");
    };
    assert!(geojson.data.is_some());
}

#[test]
fn the_layer_types_are_the_eight_the_style_renders() {
    let document = demotiles_document();
    let layer_types: Vec<LayerType> = document
        .layers
        .iter()
        .map(|layer| layer.layer_type.clone())
        .collect();
    assert_eq!(
        layer_types,
        [
            LayerType::Background,
            LayerType::Line,
            LayerType::Fill,
            LayerType::Line,
            LayerType::Line,
            LayerType::Symbol,
            LayerType::Symbol,
            LayerType::Fill,
        ]
    );
}

#[test]
fn legacy_line_width_stops_interpolate_at_the_frame_zoom() {
    let document = demotiles_document();
    let coastline = document.layer("coastline").expect("the coastline layer");
    let line_width = coastline.paint.line_width.as_ref().expect("line-width");
    let width_at = |zoom: f64| {
        line_width
            .evaluate(&EvaluationContext::at_zoom(zoom))
            .expect("line-width evaluates")
    };
    assert_eq!(width_at(0.0), 2.0);
    assert!((width_at(3.0) - 4.0).abs() < 1e-12);
    assert_eq!(width_at(22.0), 18.0);
}

#[test]
fn legacy_line_opacity_stops_interpolate_between_their_stops() {
    let document = demotiles_document();
    let boundary = document
        .layer("countries-boundary")
        .expect("the boundary layer");
    let line_opacity = boundary.paint.line_opacity.as_ref().expect("line-opacity");
    let opacity_at = |zoom: f64| {
        line_opacity
            .evaluate(&EvaluationContext::at_zoom(zoom))
            .expect("line-opacity evaluates")
    };
    assert_eq!(opacity_at(3.0), 0.5);
    assert!((opacity_at(4.5) - 0.75).abs() < 1e-12);
    assert_eq!(opacity_at(6.0), 1.0);
}

#[test]
fn the_country_fill_match_expression_selects_the_designed_palette() {
    let document = demotiles_document();
    let fill = document.layer("countries-fill").expect("the fill layer");
    let fill_color = fill.paint.fill_color.as_ref().expect("fill-color");
    let color_for = |administrative_code: &str| {
        let properties = properties(&[("ADM0_A3", administrative_code)]);
        let context =
            EvaluationContext::at_zoom(2.0).with_feature(&properties, GeometryType::Polygon);
        fill_color.evaluate(&context).expect("fill-color evaluates")
    };
    assert_eq!(
        color_for("USA"),
        Color::from_css("#C1E599").expect("the palette colour parses")
    );
    assert_eq!(
        color_for("ATA"),
        Color::from_css("#FFFFFF").expect("the palette colour parses")
    );
    assert_eq!(
        color_for("ABW"),
        Color::from_css("#B1BBF9").expect("the palette colour parses")
    );
    assert_eq!(
        color_for("AIA"),
        Color::from_css("#83D5F4").expect("the palette colour parses")
    );
    assert_eq!(
        color_for("FRA"),
        Color::from_css("#EAB38F").expect("the fallback colour parses")
    );
}

#[test]
fn the_geolines_filter_excludes_the_international_date_line() {
    let document = demotiles_document();
    let geolines = document.layer("geolines").expect("the geolines layer");
    let filter = geolines.filter.as_ref().expect("the geolines filter");
    let matches = |name: Option<&str>| {
        let properties = name
            .map(|name| properties(&[("name", name)]))
            .unwrap_or_default();
        let context =
            EvaluationContext::at_zoom(3.0).with_feature(&properties, GeometryType::LineString);
        filter
            .evaluate(&context)
            .expect("the filter evaluates")
            .truthy()
    };
    assert!(matches(Some("Equator")));
    assert!(!matches(Some("International Date Line")));
}

#[test]
fn legacy_text_size_and_halo_stops_evaluate() {
    let document = demotiles_document();
    let geolines_label = document
        .layer("geolines-label")
        .expect("the geolines label layer");
    let text_size = geolines_label.layout.text_size.as_ref().expect("text-size");
    assert_eq!(
        text_size
            .evaluate(&EvaluationContext::at_zoom(4.0))
            .expect("text-size evaluates"),
        14.0
    );

    let countries_label = document
        .layer("countries-label")
        .expect("the country label layer");
    let halo_blur = countries_label
        .paint
        .text_halo_blur
        .as_ref()
        .expect("text-halo-blur");
    assert!(
        (halo_blur
            .evaluate(&EvaluationContext::at_zoom(4.0))
            .expect("halo blur evaluates")
            - 0.1)
            .abs()
            < 1e-12
    );
    let halo_width = countries_label
        .paint
        .text_halo_width
        .as_ref()
        .expect("text-halo-width");
    assert!(
        (halo_width
            .evaluate(&EvaluationContext::at_zoom(4.0))
            .expect("halo width evaluates")
            - 1.3)
            .abs()
            < 1e-12
    );
}

#[test]
fn legacy_text_field_and_transform_stops_are_discrete() {
    let document = demotiles_document();
    let countries_label = document
        .layer("countries-label")
        .expect("the country label layer");
    let text_field = countries_label
        .layout
        .text_field
        .as_ref()
        .expect("text-field");
    assert_eq!(
        text_field
            .evaluate(&EvaluationContext::at_zoom(2.0))
            .expect("text-field evaluates"),
        "{ABBREV}"
    );
    assert_eq!(
        text_field
            .evaluate(&EvaluationContext::at_zoom(5.0))
            .expect("text-field evaluates"),
        "{NAME}"
    );

    let text_transform = countries_label
        .layout
        .text_transform
        .as_ref()
        .expect("text-transform");
    assert_eq!(
        text_transform
            .evaluate(&EvaluationContext::at_zoom(0.0))
            .expect("text-transform evaluates"),
        TextTransform::Uppercase
    );
    assert_eq!(
        text_transform
            .evaluate(&EvaluationContext::at_zoom(2.0))
            .expect("text-transform evaluates"),
        TextTransform::None
    );
}

#[test]
fn constant_paint_and_layout_values_parse() {
    let document = demotiles_document();
    let background = document.layer("background").expect("the background layer");
    assert_eq!(
        background
            .paint
            .background_color
            .as_ref()
            .expect("background-color")
            .evaluate(&EvaluationContext::at_zoom(0.0))
            .expect("background-color evaluates"),
        Color::from_css("#D8F2FF").expect("the colour parses")
    );

    let coastline = document.layer("coastline").expect("the coastline layer");
    assert!(
        coastline
            .layout
            .line_cap
            .as_ref()
            .expect("line-cap")
            .is_constant()
    );
    assert!(
        coastline
            .layout
            .line_join
            .as_ref()
            .expect("line-join")
            .is_constant()
    );

    let geolines = document.layer("geolines").expect("the geolines layer");
    assert_eq!(
        geolines
            .paint
            .line_dasharray
            .as_ref()
            .expect("line-dasharray")
            .evaluate(&EvaluationContext::at_zoom(3.0))
            .expect("line-dasharray evaluates"),
        vec![3.0, 3.0]
    );
}

#[test]
fn the_layer_zoom_spans_match_the_document() {
    let document = demotiles_document();
    let label = document
        .layer("countries-label")
        .expect("the country label layer");
    assert_eq!(label.minimum_zoom, 2.0);
    assert_eq!(label.maximum_zoom, 24.0);
    assert!(!label.is_visible_at_zoom(1.9));
    assert!(label.is_visible_at_zoom(2.0));
    assert!(label.is_visible_at_zoom(23.9));
    assert!(!label.is_visible_at_zoom(24.0));
}
