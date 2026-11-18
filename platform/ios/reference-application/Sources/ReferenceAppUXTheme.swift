//! The SwiftUI presentation of the design tokens in
//! `docs/ReferenceAppUX.md` §2 for the Swift reference applications (§8):
//! every token of the locked table appears here under its exact token name
//! (`ReferenceAppUXColor` for the §2.1 semantic colors, `ReferenceAppUXFont`
//! for the §2.3 type scale). A token exists if and only if it is in the
//! document's table; adding a color here first means adding it to that
//! table.

import SwiftUI

/// The two presentation themes of the design language (§3). This is the
/// *resolved* theme the token slots render against; the reference
/// application's three-way `ThemeChoice` (Light / Dark / Auto) resolves to one
/// of these two, and Auto follows the operating system's scheme live. The
/// canvas is opaque black in both themes.
public enum AppTheme: String, CaseIterable, Sendable {
    /// The default theme: dark translucent panels over the dark canvas.
    case dark
    /// Light panels ("paper on glass") over the same dark canvas.
    case light

    /// The SwiftUI `ColorScheme` that drives the environment's contrast
    /// direction while the explicit theme state owns the decision (§8:
    /// `@Environment(\.colorScheme)` is driven by the refapp's explicit
    /// theme state, not the system).
    public var colorScheme: ColorScheme {
        switch self {
        case .dark: return .dark
        case .light: return .light
        }
    }
}

/// Environment key carrying the reference application's explicit theme (§3).
private struct AppThemeEnvironmentKey: EnvironmentKey {
    static let defaultValue: AppTheme = .dark
}

public extension EnvironmentValues {
    /// The presentation theme the panels resolve their tokens against.
    var appTheme: AppTheme {
        get { self[AppThemeEnvironmentKey.self] }
        set { self[AppThemeEnvironmentKey.self] = newValue }
    }
}

/// One eight-bit-quantified RGBA color value a token row carries. The
/// design table quantifies its dark and light values to 8-bit channels
/// (0–255) plus an alpha in the unit interval, mirrored exactly here.
struct TokenColorComponents: Sendable {
    /// Red channel, 0–255 per the design table's hex or rgba notation.
    let redComponent: Double
    /// Green channel, 0–255 per the design table's notation.
    let greenComponent: Double
    /// Blue channel, 0–255 per the design table's notation.
    let blueComponent: Double
    /// Alpha channel in `[0, 1]` per the design table's rgba notation
    /// (hex rows carry the full-alpha value below).
    let alphaComponent: Double

    /// The sRGB `Color` of these components.
    var color: Color {
        Color(.sRGB, red: redComponent, green: greenComponent, blue: blueComponent, opacity: alphaComponent)
    }

    /// The fully opaque variant the table's hex rows denote: the hex rows
    /// carry 0–255 channels, and the `Color` constructor consumes 0–1
    /// components — the channels divide by the table's channel maximum the
    /// same way the `rgba` rows' channels do (an un-divided channel clamps
    /// at 1.0 and the color collapses to white).
    static func opaque(red: Double, green: Double, blue: Double) -> Self {
        TokenColorComponents(
            redComponent: red / hexRowChannelMaximum,
            greenComponent: green / hexRowChannelMaximum,
            blueComponent: blue / hexRowChannelMaximum,
            alphaComponent: opaqueHexTokenAlphaComponent
        )
    }

    /// The channel maximum the table's hex-row channels (0–255) divide by
    /// to become `Color` components; shared by every hex-derived row.
    private static let hexRowChannelMaximum: Double = 255.0
}

/// The alpha the table's hex rows denote: fully opaque.
private let opaqueHexTokenAlphaComponent: Double = 1.0

/// The rgba channel maximum the table's `rgba(r, g, b, a)` rows divide their
/// 0–255 channels by (`20` in `rgba(20, 24, 30, 0.86)`).
private let rgbaChannelMaximum: Double = 255.0

/// One semantic token row: a named pair of dark- and light-theme color
/// values from the design table.
private struct SemanticColorToken {
    let darkThemeValue: TokenColorComponents
    let lightThemeValue: TokenColorComponents

    /// The row's value for the given theme.
    func color(for theme: AppTheme) -> Color {
        switch theme {
        case .dark: return darkThemeValue.color
        case .light: return lightThemeValue.color
        }
    }

    private static func rgba(
        _ red: Double, _ green: Double, _ blue: Double, alpha: Double
    ) -> TokenColorComponents {
        TokenColorComponents(
            redComponent: red / rgbaChannelMaximum,
            greenComponent: green / rgbaChannelMaximum,
            blueComponent: blue / rgbaChannelMaximum,
            alphaComponent: alpha
        )
    }

    /// The full token table of `docs/ReferenceAppUX.md` §2.1, verbatim.
    static let surface = SemanticColorToken(
        darkThemeValue: rgba(20, 24, 30, alpha: 0.86),
        lightThemeValue: rgba(255, 255, 255, alpha: 0.92)
    )
    static let surfaceBorder = SemanticColorToken(
        darkThemeValue: rgba(255, 255, 255, alpha: 0.14),
        lightThemeValue: rgba(0, 0, 0, alpha: 0.12)
    )
    static let textPrimary = SemanticColorToken(
        darkThemeValue: .opaque(red: 0xf2, green: 0xf4, blue: 0xf8),
        lightThemeValue: .opaque(red: 0x1c, green: 0x1e, blue: 0x21)
    )
    static let textSecondary = SemanticColorToken(
        darkThemeValue: .opaque(red: 0x9a, green: 0xa4, blue: 0xb2),
        lightThemeValue: .opaque(red: 0x56, green: 0x5c, blue: 0x66)
    )
    static let accent = SemanticColorToken(
        darkThemeValue: .opaque(red: 0x4c, green: 0x8d, blue: 0xff),
        lightThemeValue: .opaque(red: 0x25, green: 0x63, blue: 0xeb)
    )
    static let positive = SemanticColorToken(
        darkThemeValue: .opaque(red: 0x34, green: 0xc0, blue: 0x7c),
        lightThemeValue: .opaque(red: 0x1d, green: 0x8a, blue: 0x4e)
    )
    static let warning = SemanticColorToken(
        darkThemeValue: .opaque(red: 0xe0, green: 0xa0, blue: 0x3a),
        lightThemeValue: .opaque(red: 0xa0, green: 0x70, blue: 0x16)
    )
    static let danger = SemanticColorToken(
        darkThemeValue: .opaque(red: 0xe0, green: 0x56, blue: 0x56),
        lightThemeValue: .opaque(red: 0xc2, green: 0x2f, blue: 0x2f)
    )
}

/// The §2.1 semantic color tokens, resolved against the explicit `AppTheme`
/// state. Token names mirror the design document's rows verbatim.
public enum ReferenceAppUXColor {
    /// `surface`: the panel background ("glass on glass" in the dark theme,
    /// "paper on glass" in the light theme).
    public static func surface(_ theme: AppTheme) -> Color {
        SemanticColorToken.surface.color(for: theme)
    }

    /// `surface-border`: panel and divider borders.
    public static func surfaceBorder(_ theme: AppTheme) -> Color {
        SemanticColorToken.surfaceBorder.color(for: theme)
    }

    /// `text-primary`: primary panel text.
    public static func textPrimary(_ theme: AppTheme) -> Color {
        SemanticColorToken.textPrimary.color(for: theme)
    }

    /// `text-secondary`: labels, hints, value lines.
    public static func textSecondary(_ theme: AppTheme) -> Color {
        SemanticColorToken.textSecondary.color(for: theme)
    }

    /// `accent`: interactive colors — sliders, links, focus, route imagery.
    public static func accent(_ theme: AppTheme) -> Color {
        SemanticColorToken.accent.color(for: theme)
    }

    /// `positive`: readiness "ready", arrival.
    public static func positive(_ theme: AppTheme) -> Color {
        SemanticColorToken.positive.color(for: theme)
    }

    /// `warning`: readiness "degraded", the pending-reason strips.
    public static func warning(_ theme: AppTheme) -> Color {
        SemanticColorToken.warning.color(for: theme)
    }

    /// `danger`: readiness "failed", errors, the off-route alert.
    public static func danger(_ theme: AppTheme) -> Color {
        SemanticColorToken.danger.color(for: theme)
    }
}

/// The §2.3 type scale, resolved as SwiftUI fonts under the exact token
/// names. One font stack per platform is intentionally not shared; the
/// sizes and weights of the scale are.
public enum ReferenceAppUXFont {
    /// The pixel sizes of the §2.3 table, mirrored as named constants.
    private enum TypeScale {
        /// `type-caption`: 12px.
        static let captionPixels: CGFloat = 12
        /// `type-body`: 13px.
        static let bodyPixels: CGFloat = 13
        /// `type-title`: 14px.
        static let titlePixels: CGFloat = 14
        /// `type-maneuver`: 22px, shared with `type-maneuver-distance`.
        static let maneuverPixels: CGFloat = 22
        /// The regular weight of the 400-weight rows.
        static let regularWeight: Font.Weight = .regular
        /// The semibold weight of the 600-weight rows.
        static let semiboldWeight: Font.Weight = .semibold
    }

    /// `type-caption`: status line, attribution, reasons.
    public static func caption() -> Font {
        .system(size: TypeScale.captionPixels, weight: TypeScale.regularWeight)
    }

    /// `type-body`: panel body, control labels.
    public static func body() -> Font {
        .system(size: TypeScale.bodyPixels, weight: TypeScale.regularWeight)
    }

    /// `type-body-strong`: group labels, toggle labels.
    public static func bodyStrong() -> Font {
        .system(size: TypeScale.bodyPixels, weight: TypeScale.semiboldWeight)
    }

    /// `type-title`: panel titles.
    public static func title() -> Font {
        .system(size: TypeScale.titlePixels, weight: TypeScale.semiboldWeight)
    }

    /// `type-maneuver`: the turn banner's instruction text.
    public static func maneuver() -> Font {
        .system(size: TypeScale.maneuverPixels, weight: TypeScale.semiboldWeight)
    }

    /// `type-maneuver-distance`: the turn banner's distance phrase —
    /// intentionally lighter weight than the instruction text at the same
    /// size: distance is context, text is command.
    public static func maneuverDistance() -> Font {
        .system(size: TypeScale.maneuverPixels, weight: TypeScale.regularWeight)
    }

    /// The numeral readout stack (the design language's monospaced numeric
    /// stack): the platform default monospace stack; nothing else is
    /// monospace.
    public static func numericReadout() -> Font {
        .system(size: TypeScale.bodyPixels, weight: TypeScale.regularWeight, design: .monospaced)
    }
}