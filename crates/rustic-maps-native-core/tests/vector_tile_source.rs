//! End-to-end tests for the vector base-map surface (M22):
//! `MapView::set_vector_tile_source`, `clear_vector_tile_source` and
//! `vector_tile_source_status`.
//!
//! They prove what the contract promises without a GPU surface: a source is
//! validated when it is installed (the URL template's placeholders and scheme,
//! the zoom range, the style id), a refused install keeps the previously
//! installed base in force, and the status readback reports the configured
//! style, source name, attribution and zoom ceiling. None of it needs an
//! attached surface, because — like the overlay list — the vector base source
//! outlives attach/detach cycles; the rendering half is covered by the
//! scheduler's own tests in `rustic-maps-rendering`.

use rustic_maps::{MapSdk, MapView, UrlTemplateVectorTileSource, VectorTileSourceStatus};
use std::sync::Arc;

/// The committed style the installed test sources paint with.
const TEST_STYLE_ID: &str = "bright-light";

/// The attribution line the installed test sources carry.
const TEST_ATTRIBUTION_TEXT: &str = "Test vector tiles";

/// Builds a source declaration pointing at a neutral test tileset.
fn test_vector_source(source_name: &str) -> UrlTemplateVectorTileSource {
    UrlTemplateVectorTileSource {
        url_template: "https://tiles.example.com/{z}/{x}/{y}.pbf".to_string(),
        source_name: source_name.to_string(),
        attribution_text: TEST_ATTRIBUTION_TEXT.to_string(),
        style_id: TEST_STYLE_ID.to_string(),
        minimum_zoom_level: 0,
        maximum_zoom_level: 14,
        token_bucket_capacity_tokens: None,
        refill_tokens_per_second: None,
        terms_text: None,
        subdomains: None,
        headers: None,
        style_hints: None,
    }
}

/// A view over an empty SDK; no provider or surface is needed to configure a
/// vector base.
fn fresh_map_view() -> MapView {
    MapView::new(Arc::new(MapSdk::new()))
}

/// Whether the status reports no vector base source.
fn reports_no_source(status: &VectorTileSourceStatus) -> bool {
    !status.is_configured
        && status.style_id.is_empty()
        && status.source_name.is_empty()
        && status.attribution_text.is_empty()
        && status.maximum_zoom_level == 0
        && status.drawn_tile_count == 0
        && status.failed_tile_count == 0
        && status.last_failure_detail.is_none()
}

#[test]
fn a_new_view_reports_no_vector_base_source() {
    let map_view = fresh_map_view();
    let status = map_view.vector_tile_source_status();
    assert!(reports_no_source(&status));
}

#[test]
fn installing_a_vector_base_source_reports_it_in_the_status() {
    let map_view = fresh_map_view();
    map_view
        .set_vector_tile_source(test_vector_source("test-tileset"))
        .expect("a well-formed source installs");

    let status = map_view.vector_tile_source_status();
    assert!(status.is_configured);
    assert_eq!(status.style_id, TEST_STYLE_ID);
    assert_eq!(status.source_name, "test-tileset");
    assert_eq!(status.attribution_text, TEST_ATTRIBUTION_TEXT);
    assert_eq!(status.maximum_zoom_level, 14);
    // No frame has resolved yet, so no tile accounting exists to report.
    assert_eq!(status.drawn_tile_count, 0);
    assert_eq!(status.failed_tile_count, 0);
    assert!(status.last_failure_detail.is_none());
}

#[test]
fn an_unknown_style_id_is_refused_and_keeps_the_installed_base() {
    let map_view = fresh_map_view();
    map_view
        .set_vector_tile_source(test_vector_source("installed-tileset"))
        .expect("a well-formed source installs");

    let mut unknown_style_source = test_vector_source("refused-tileset");
    unknown_style_source.style_id = "no-such-style".to_string();
    assert!(
        map_view
            .set_vector_tile_source(unknown_style_source)
            .is_err()
    );

    let status = map_view.vector_tile_source_status();
    assert!(status.is_configured);
    assert_eq!(status.source_name, "installed-tileset");
    assert_eq!(status.style_id, TEST_STYLE_ID);
}

#[test]
fn a_malformed_url_template_is_refused_and_keeps_the_installed_base() {
    let map_view = fresh_map_view();
    map_view
        .set_vector_tile_source(test_vector_source("installed-tileset"))
        .expect("a well-formed source installs");

    let mut malformed_template_source = test_vector_source("refused-tileset");
    malformed_template_source.url_template = "not-a-url".to_string();
    assert!(
        map_view
            .set_vector_tile_source(malformed_template_source)
            .is_err()
    );

    let status = map_view.vector_tile_source_status();
    assert_eq!(status.source_name, "installed-tileset");
}

#[test]
fn an_inverted_zoom_range_is_refused_and_keeps_the_installed_base() {
    let map_view = fresh_map_view();
    map_view
        .set_vector_tile_source(test_vector_source("installed-tileset"))
        .expect("a well-formed source installs");

    let mut inverted_zoom_range_source = test_vector_source("refused-tileset");
    inverted_zoom_range_source.minimum_zoom_level = 14;
    inverted_zoom_range_source.maximum_zoom_level = 6;
    assert!(
        map_view
            .set_vector_tile_source(inverted_zoom_range_source)
            .is_err()
    );

    let status = map_view.vector_tile_source_status();
    assert_eq!(status.source_name, "installed-tileset");
}

#[test]
fn clearing_the_vector_base_source_reports_no_source() {
    let map_view = fresh_map_view();
    map_view
        .set_vector_tile_source(test_vector_source("test-tileset"))
        .expect("a well-formed source installs");
    map_view
        .clear_vector_tile_source()
        .expect("clearing an installed source succeeds");
    assert!(reports_no_source(&map_view.vector_tile_source_status()));

    // Clearing again is not an error: the view simply carries no base.
    map_view
        .clear_vector_tile_source()
        .expect("clearing a view with no source succeeds");
}

#[test]
fn installing_a_second_source_replaces_the_first() {
    let map_view = fresh_map_view();
    map_view
        .set_vector_tile_source(test_vector_source("first-tileset"))
        .expect("a well-formed source installs");
    map_view
        .set_vector_tile_source(test_vector_source("second-tileset"))
        .expect("a second well-formed source replaces the first");

    let status = map_view.vector_tile_source_status();
    assert_eq!(status.source_name, "second-tileset");
    assert_eq!(status.style_id, TEST_STYLE_ID);
}
