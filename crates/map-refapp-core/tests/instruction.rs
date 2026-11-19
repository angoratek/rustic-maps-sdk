//! The shared instruction-text composition: the display priority, the speech
//! priority, the honest unavailable line and the roundabout exit suffix. These
//! values are the ones the four reference applications present verbatim.

use map_refapp_core::{
    UNAVAILABLE_INSTRUCTION_TEXT, instruction_banner_text, instruction_display_text,
    instruction_speech_text, roundabout_exit_suffix,
};

const DISTANCE_COMPOSED_DISPLAY_TEXT: &str = "In 300 meters, Turn left onto the north street";
const PROVIDER_DISPLAY_TEXT: &str = "Turn left onto the north street";
const COMPOSED_SPEECH_TEXT: &str = "In 300 meters, turn left onto the north street";
const PROVIDER_SPEECH_TEXT: &str = "turn left onto the north street";

#[test]
fn display_text_prefers_the_composed_channel_over_the_provider_channel() {
    assert_eq!(
        instruction_display_text(
            Some(DISTANCE_COMPOSED_DISPLAY_TEXT),
            Some(PROVIDER_DISPLAY_TEXT)
        ),
        DISTANCE_COMPOSED_DISPLAY_TEXT
    );
}

#[test]
fn display_text_falls_back_to_the_verbatim_provider_channel() {
    assert_eq!(
        instruction_display_text(None, Some(PROVIDER_DISPLAY_TEXT)),
        PROVIDER_DISPLAY_TEXT
    );
}

#[test]
fn display_text_renders_the_honest_unavailable_line_when_no_text_exists() {
    // The contract's `Unavailable` instruction-text kind carries null text
    // fields, so it reaches the same path as a composed kind with no text.
    assert_eq!(
        instruction_display_text(None, None),
        "No instruction text provided"
    );
    assert_eq!(
        instruction_display_text(None, None),
        UNAVAILABLE_INSTRUCTION_TEXT
    );
}

#[test]
fn speech_text_prefers_the_composed_channel_over_the_provider_channel() {
    assert_eq!(
        instruction_speech_text(Some(COMPOSED_SPEECH_TEXT), Some(PROVIDER_SPEECH_TEXT)),
        Some(COMPOSED_SPEECH_TEXT)
    );
}

#[test]
fn speech_text_falls_back_to_the_verbatim_provider_channel() {
    assert_eq!(
        instruction_speech_text(None, Some(PROVIDER_SPEECH_TEXT)),
        Some(PROVIDER_SPEECH_TEXT)
    );
}

#[test]
fn speech_text_is_absent_when_the_provider_authored_none() {
    // Nothing is spoken instead of inventing a line.
    assert_eq!(instruction_speech_text(None, None), None);
}

#[test]
fn roundabout_exit_suffix_carries_the_structured_exit_index() {
    assert_eq!(roundabout_exit_suffix(None), "");
    assert_eq!(roundabout_exit_suffix(Some(2)), " (exit 2)");
    assert_eq!(roundabout_exit_suffix(Some(17)), " (exit 17)");
}

#[test]
fn banner_text_appends_the_exit_suffix_to_the_display_text() {
    assert_eq!(
        instruction_banner_text(
            Some(DISTANCE_COMPOSED_DISPLAY_TEXT),
            Some(PROVIDER_DISPLAY_TEXT),
            Some(2)
        ),
        "In 300 meters, Turn left onto the north street (exit 2)"
    );
    assert_eq!(
        instruction_banner_text(None, Some(PROVIDER_DISPLAY_TEXT), None),
        PROVIDER_DISPLAY_TEXT
    );
    assert_eq!(
        instruction_banner_text(None, None, Some(3)),
        "No instruction text provided (exit 3)"
    );
}
