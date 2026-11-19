//! The instruction-text composition the contract exposes to every reference
//! application.
//!
//! The rule itself lives in [`map_refapp_core`]: the composed text first, the
//! provider-authored text second, the honest unavailable line on the display
//! channel and silence on the speech channel, plus the roundabout exit
//! suffix. This module only lowers those answers across the UniFFI boundary;
//! no platform keeps its own copy of the rule.

/// The display text of one maneuver instruction.
///
/// The composed display text wins; when it is absent the provider-authored
/// display text renders verbatim; when neither exists the honest `No
/// instruction text provided` line renders.
///
/// # Parameters
/// - `composed_display_text` — the banner variant composed by the contract,
///   when present.
/// - `provider_display_text` — the provider-authored display text, verbatim,
///   when present.
///
/// # Returns
/// The display line, never empty for absent provider text.
pub fn instruction_display_text(
    composed_display_text: Option<String>,
    provider_display_text: Option<String>,
) -> String {
    map_refapp_core::instruction_display_text(
        composed_display_text.as_deref(),
        provider_display_text.as_deref(),
    )
}

/// The speech text of one maneuver instruction.
///
/// The composed speech text wins; when it is absent the provider-authored
/// speech text is returned verbatim. When neither exists the maneuver is
/// never spoken, so the result is `None`.
///
/// # Parameters
/// - `composed_speech_text` — the text-to-speech variant composed by the
///   contract, when present.
/// - `provider_speech_text` — the provider-authored speech text, verbatim,
///   when present.
///
/// # Returns
/// The line to speak verbatim, or `None` when the provider authored none.
pub fn instruction_speech_text(
    composed_speech_text: Option<String>,
    provider_speech_text: Option<String>,
) -> Option<String> {
    map_refapp_core::instruction_speech_text(
        composed_speech_text.as_deref(),
        provider_speech_text.as_deref(),
    )
    .map(str::to_string)
}

/// The turn banner's full instruction line.
///
/// The line is the display text followed by the roundabout exit suffix: ` (exit
/// <index>)` for a roundabout maneuver and nothing for every other maneuver.
///
/// # Parameters
/// - `composed_display_text` — the banner variant composed by the contract,
///   when present.
/// - `provider_display_text` — the provider-authored display text, verbatim,
///   when present.
/// - `roundabout_exit_index` — the structured exit index of a roundabout
///   maneuver, when the contract carries one.
///
/// # Returns
/// The banner line, including the honest unavailable line when the provider
/// authored no display text.
pub fn instruction_banner_text(
    composed_display_text: Option<String>,
    provider_display_text: Option<String>,
    roundabout_exit_index: Option<u8>,
) -> String {
    map_refapp_core::instruction_banner_text(
        composed_display_text.as_deref(),
        provider_display_text.as_deref(),
        roundabout_exit_index,
    )
}

#[cfg(test)]
mod tests {
    use super::{instruction_banner_text, instruction_display_text, instruction_speech_text};

    fn text(value: &str) -> Option<String> {
        Some(value.to_string())
    }

    #[test]
    fn display_text_priority_and_honest_unavailable_line() {
        assert_eq!(
            instruction_display_text(text("In 300 meters, Turn left"), text("Turn left")),
            "In 300 meters, Turn left"
        );
        assert_eq!(
            instruction_display_text(None, text("Turn left")),
            "Turn left"
        );
        assert_eq!(
            instruction_display_text(None, None),
            "No instruction text provided"
        );
    }

    #[test]
    fn speech_text_priority_and_silence_without_provider_text() {
        assert_eq!(
            instruction_speech_text(text("In 300 meters, turn left"), text("turn left")),
            Some("In 300 meters, turn left".to_string())
        );
        assert_eq!(
            instruction_speech_text(None, text("turn left")),
            Some("turn left".to_string())
        );
        assert_eq!(instruction_speech_text(None, None), None);
    }

    #[test]
    fn banner_text_appends_the_roundabout_exit_suffix() {
        assert_eq!(
            instruction_banner_text(text("Take the roundabout"), None, Some(2)),
            "Take the roundabout (exit 2)"
        );
        assert_eq!(
            instruction_banner_text(None, text("Take the roundabout"), None),
            "Take the roundabout"
        );
    }
}
