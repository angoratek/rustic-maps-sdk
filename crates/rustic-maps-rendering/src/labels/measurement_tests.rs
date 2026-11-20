use super::*;

#[test]
fn a_name_with_an_unservable_character_keeps_its_servable_text() {
    let font = default_label_font();
    let ascii_only = font
        .measure_text("Sibenik", 14.0)
        .expect("the default range serves Latin letters");
    // A character outside the served range contributes no advance and
    // paints nothing; the rest of the name still measures (and draws)
    // instead of the whole label disappearing.
    let with_unservable = font
        .measure_text("Sibenik\u{4e2d}", 14.0)
        .expect("one unservable character must not drop the label");
    assert_eq!(
        with_unservable.width_pixels, ascii_only.width_pixels,
        "an unservable character contributes no advance"
    );
}

#[test]
fn text_the_font_cannot_serve_at_all_is_not_placed() {
    let font = default_label_font();
    assert!(
        font.measure_text("\u{4e2d}\u{6587}", 14.0).is_none(),
        "a label with no servable glyph paints nothing and is skipped"
    );
}
