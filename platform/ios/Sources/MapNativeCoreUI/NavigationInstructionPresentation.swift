//! Composing the design document's §7.1 card text from the contract's
//! instruction payloads, exactly per the banner rules: the distance phrase
//! per `DistancePhraseKind`, the display-text priority
//! `composed_display_text` → `provider_display_text` (provider text is
//! never rewritten, only prefixed with the distance phrase), the honest
//! unavailable-text line, and the roundabout exit suffix.

import Foundation
import RusticMapsSDK

/// The distance-phrase formatting the banners use (§7.1). The demo unit
/// system is metric; imperial units format through the same table.
public enum DistancePhraseFormatting {
    /// Whole-unit formatting for the short metric unit (meters): no decimal
    /// places ("250 m").
    private static let wholeUnitDecimalPlaces: Int = 0
    /// Single-decimal formatting for the scale units (kilometers, miles).
    private static let scaledUnitDecimalPlaces: Int = 1

    /// The unit suffix strings, keyed per contract unit.
    private static func unitSuffix(_ unit: DistanceUnit) -> String {
        switch unit {
        case .meters: return "m"
        case .kilometers: return "km"
        case .feet: return "ft"
        case .miles: return "mi"
        }
    }

    /// The decimal places a unit's magnitude renders with: the short units
    /// (meters, feet) never carry decimals, the scale units carry one.
    private static func decimalPlaces(for unit: DistanceUnit) -> Int {
        switch unit {
        case .meters, .feet: return wholeUnitDecimalPlaces
        case .kilometers, .miles: return scaledUnitDecimalPlaces
        }
    }

    /// The distance phrase text of an active maneuver's phrase value
    /// (`type-maneuver-distance` slot): `Immediate` renders no number, a
    /// `Quantity` renders its magnitude with its unit suffix, and the
    /// `Suppressed` kind (the frozen fully-off-route projection) renders
    /// nothing for the same reason `Immediate` does — the contract composes no
    /// distance prefix on it, so none is fabricated here. A quantity missing
    /// its magnitude or unit renders nothing honest rather than a fabricated
    /// number.
    public static func text(ofPhrase phrase: DistancePhrase) -> String? {
        switch phrase.phraseKind {
        case .immediate, .suppressed:
            return nil
        case .quantity:
            guard let phraseMagnitude = phrase.magnitude, let phraseUnit = phrase.magnitudeUnit else {
                return nil
            }
            return String(
                format: "%.\(decimalPlaces(for: phraseUnit))f \(unitSuffix(phraseUnit))",
                phraseMagnitude
            )
        }
    }
}

/// The display-text priority and suffix rules of the banner and then-line
/// (§7.1), shared by every native card consumer.
public enum ManeuverInstructionPresentation {
    /// The honest line an `Unavailable` instruction text kind renders.
    public static let unavailableText: String = "No instruction text provided"

    /// The display text of one `ManeuverInstructionText` value, per the §7.1
    /// priority: the composed display text first, the provider display text
    /// second, and the honest unavailable line for the `Unavailable` kind.
    public static func text(ofInstructionText instructionText: ManeuverInstructionText) -> String {
        switch instructionText.textKind {
        case .composed:
            return instructionText.composedDisplayText
                ?? instructionText.providerDisplayText
                ?? unavailableText
        case .unavailable:
            return unavailableText
        }
    }

    /// The active maneuver's distance phrase, when the phrase kind renders
    /// one (the `Immediate` kind renders no number).
    public static func distancePhraseText(
        ofActiveManeuver activeManeuver: ActiveManeuverInstruction
    ) -> String? {
        DistancePhraseFormatting.text(ofPhrase: activeManeuver.distancePhrase)
    }

    /// The turn banner's full composed instruction line: the maneuver text,
    /// with the roundabout exit suffix appended when the contract carries
    /// an exit index.
    public static func bannerText(
        ofActiveManeuver activeManeuver: ActiveManeuverInstruction
    ) -> String {
        let maneuverText = text(ofInstructionText: activeManeuver.instruction)
        guard let roundaboutExitIndex = activeManeuver.roundaboutExitIndex else {
            return maneuverText
        }
        return "\(maneuverText) (exit \(roundaboutExitIndex))"
    }

    /// The then-line's text for an upcoming maneuver (the display channel,
    /// same priority rules), or nil while the contract's upcoming maneuver
    /// is null (the then-line is hidden then, §7.1).
    public static func thenLineText(
        ofUpcomingManeuver upcomingManeuver: UpcomingManeuverInstruction
    ) -> String? {
        text(ofInstructionText: upcomingManeuver.instruction)
    }

    // MARK: The speech channel (§4.2's speech toggle, §6.3's verbatim rule)

    /// The payload's speech text of one instruction-text value, per the
    /// contract's own channel priority: the TTS-composed variant (the live
    /// distance prefix plus the speech variant) first, the provider-authored
    /// speech text second — both carried verbatim, never paraphrased,
    /// translated or rephrased before they reach the platform voice. Nil for
    /// the `Unavailable` kind (the provider authored no text; nothing is
    /// spoken instead of inventing a line).
    public static func speechText(ofInstructionText instructionText: ManeuverInstructionText) -> String? {
        switch instructionText.textKind {
        case .composed:
            return instructionText.composedSpeechText
                ?? instructionText.providerSpeechText
        case .unavailable:
            return nil
        }
    }

    /// The active maneuver's speech text (the instruction payload the
    /// platform voice speaks), or nil when the payload carries no speech
    /// text (§4.2: "spoken text verbatim from the payload's speech
    /// channel").
    public static func speechText(
        ofActiveManeuver activeManeuver: ActiveManeuverInstruction
    ) -> String? {
        speechText(ofInstructionText: activeManeuver.instruction)
    }
}