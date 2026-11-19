//! The instruction-text composition the reference applications present.
//!
//! The turn banner, the then-line and the speech channel read the same
//! provider-neutral payload from the SDK contract, and all three apply one
//! rule: the composed text first, the provider-authored text second, and — on
//! the display channel — the honest unavailable line when the provider
//! authored no text at all. The banner appends the structured roundabout exit
//! index when the maneuver carries one.
//!
//! Provider text is carried verbatim: the rule chooses between channels and
//! appends the exit suffix; it never rewrites, shortens or invents text.

/// The honest display line rendered for a maneuver whose provider authored no
/// instruction text.
pub const UNAVAILABLE_INSTRUCTION_TEXT: &str = "No instruction text provided";

/// The roundabout exit suffix's fixed text before the exit index.
const ROUNDABOUT_EXIT_SUFFIX_PREFIX: &str = " (exit ";

/// The roundabout exit suffix's fixed text after the exit index.
const ROUNDABOUT_EXIT_SUFFIX_TERMINATOR: &str = ")";

/// Returns the display text of one maneuver instruction.
///
/// The composed display text wins; when it is absent the provider-authored
/// display text renders verbatim; when neither exists the honest
/// [`UNAVAILABLE_INSTRUCTION_TEXT`] line renders. The contract's `Unavailable`
/// instruction-text kind carries null text fields, so it follows the same
/// path and renders the honest line rather than an invented instruction.
///
/// # Parameters
/// - `composed_display_text` — the banner variant the contract composed (the
///   live distance prefix plus the provider display text), when present.
/// - `provider_display_text` — the provider-authored display text, verbatim,
///   when present.
///
/// # Returns
/// The display line for the banner and then-line, never empty for absent
/// provider text.
pub fn instruction_display_text(
    composed_display_text: Option<&str>,
    provider_display_text: Option<&str>,
) -> String {
    composed_display_text
        .or(provider_display_text)
        .unwrap_or(UNAVAILABLE_INSTRUCTION_TEXT)
        .to_string()
}

/// Returns the speech text of one maneuver instruction.
///
/// The composed speech text wins; when it is absent the provider-authored
/// speech text is returned verbatim. When neither exists the maneuver is
/// never spoken: speaking an invented line is not an option, so the answer is
/// `None`.
///
/// # Parameters
/// - `composed_speech_text` — the text-to-speech variant the contract
///   composed (the live distance prefix plus the provider speech text), when
///   present.
/// - `provider_speech_text` — the provider-authored speech text, verbatim,
///   when present.
///
/// # Returns
/// The line to speak verbatim, or `None` when the provider authored none.
pub fn instruction_speech_text<'text>(
    composed_speech_text: Option<&'text str>,
    provider_speech_text: Option<&'text str>,
) -> Option<&'text str> {
    composed_speech_text.or(provider_speech_text)
}

/// Returns the banner's roundabout exit suffix.
///
/// # Parameters
/// - `roundabout_exit_index` — the structured exit index of a roundabout
///   maneuver, starting at one; every other maneuver has `None`.
///
/// # Returns
/// `" (exit <index>)"` for a roundabout maneuver, or the empty string for
/// every other maneuver.
pub fn roundabout_exit_suffix(roundabout_exit_index: Option<u8>) -> String {
    match roundabout_exit_index {
        Some(exit_index) => format!(
            "{ROUNDABOUT_EXIT_SUFFIX_PREFIX}{exit_index}{ROUNDABOUT_EXIT_SUFFIX_TERMINATOR}"
        ),
        None => String::new(),
    }
}

/// Returns the turn banner's full instruction line.
///
/// The line is the display text ([`instruction_display_text`]) followed by the
/// roundabout exit suffix ([`roundabout_exit_suffix`]). The then-line renders
/// the display text alone; the suffix belongs to the banner only.
///
/// # Parameters
/// - `composed_display_text` — the banner variant the contract composed, when
///   present.
/// - `provider_display_text` — the provider-authored display text, verbatim,
///   when present.
/// - `roundabout_exit_index` — the structured exit index of a roundabout
///   maneuver, when the contract carries one.
///
/// # Returns
/// The banner line, including the honest unavailable line when the provider
/// authored no display text.
pub fn instruction_banner_text(
    composed_display_text: Option<&str>,
    provider_display_text: Option<&str>,
    roundabout_exit_index: Option<u8>,
) -> String {
    let mut banner_text = instruction_display_text(composed_display_text, provider_display_text);
    banner_text.push_str(&roundabout_exit_suffix(roundabout_exit_index));
    banner_text
}
