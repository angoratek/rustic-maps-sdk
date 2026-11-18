package com.rusticmaps.sdk.ux

import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp

/**
 * The design tokens of `docs/ReferenceAppUX.md` §2.1 in Kotlin colors. Every
 * value is copied verbatim from the token table (dark and light); a token
 * that is not in the table does not exist here.
 *
 * The channel values match the CSS table exactly. The dark panel background
 * `rgba(20, 24, 30, 0.86)` becomes `Color(20f/255f, 24f/255f, 30f/255f, 0.86f)`
 * and so on; alpha carries the translucency of the floating panels over the
 * live map surface ("glass on glass" / "paper on glass", §2.1).
 */
data class ReferenceAppUXTokenColors(
    /** Panel background (`surface`). */
    val surface: Color,
    /** Panel and divider borders (`surface-border`). */
    val surfaceBorder: Color,
    /** Primary panel text (`text-primary`). */
    val textPrimary: Color,
    /** Labels, hints, value lines (`text-secondary`). */
    val textSecondary: Color,
    /** Interactive accent: sliders, links, route imagery (`accent`). */
    val accent: Color,
    /** Readiness "ready", arrival (`positive`). */
    val positive: Color,
    /** Readiness "degraded", pending-reason strips (`warning`). */
    val warning: Color,
    /** Readiness "failed", errors, off-route alert (`danger`). */
    val danger: Color,
)

/** Dark-theme token values of `docs/ReferenceAppUX.md` §2.1 (locked hex values). */
val ReferenceAppUXTokenColorsDark: ReferenceAppUXTokenColors = ReferenceAppUXTokenColors(
    surface = Color(red = 20f / 255f, green = 24f / 255f, blue = 30f / 255f, alpha = 0.86f),
    surfaceBorder = Color(red = 1f, green = 1f, blue = 1f, alpha = 0.14f),
    textPrimary = Color(0xFFF2F4F8),
    textSecondary = Color(0xFF9AA4B2),
    accent = Color(0xFF4C8DFF),
    positive = Color(0xFF34C07C),
    warning = Color(0xFFE0A03A),
    danger = Color(0xFFE05656),
)

/** Light-theme token values of `docs/ReferenceAppUX.md` §2.1 (locked hex values). */
val ReferenceAppUXTokenColorsLight: ReferenceAppUXTokenColors = ReferenceAppUXTokenColors(
    surface = Color(red = 1f, green = 1f, blue = 1f, alpha = 0.92f),
    surfaceBorder = Color(red = 0f, green = 0f, blue = 0f, alpha = 0.12f),
    textPrimary = Color(0xFF1C1E21),
    textSecondary = Color(0xFF565C66),
    accent = Color(0xFF2563EB),
    positive = Color(0xFF1D8A4E),
    warning = Color(0xFFA07016),
    danger = Color(0xFFC22F2F),
)

/**
 * Token colors the Material color scheme has no slot for. The theme provides
 * them through [LocalReferenceAppUXExtendedColors] so no component ever falls
 * back to a Material default color the table does not name
 * (`docs/ReferenceAppUX.md` §8).
 */
data class ReferenceAppUXExtendedColors(
    val textSecondary: Color,
    val positive: Color,
    val warning: Color,
    val danger: Color,
    val surfaceBorder: Color,
)

/** Composition-local carrying the extended token colors inside the theme. */
val LocalReferenceAppUXExtendedColors = staticCompositionLocalOf<ReferenceAppUXExtendedColors> {
    error(message = "ReferenceAppUXTheme is not composed; wrap the UI in it first")
}

/** Companion color resolved inside the theme: the panel background against accent and danger fills. */
private fun companionColorOn(theme: ReferenceAppUXTokenColors): Color = theme.surface

/**
 * Maps the token table into ONE `MaterialTheme(colorScheme = …, typography = …)`
 * per `docs/ReferenceAppUX.md` §8: `surface`/`onSurface` take `surface` and
 * `text-primary`, `outlineVariant` takes `surface-border`, `primary` takes
 * `accent`, `error` takes `danger`; `secondary`/`tertiary` absorb the
 * warning and positive tokens so error-adjacent components resolve themed
 * colors before any Material default. Every other color-scheme slot keeps the
 * panel-background companion so nothing renders an unthemed color.
 *
 * Only the slots the design language names are themed; a slot the table does
 * not name is not a design decision and the companion mapping above is the
 * documented resolution inside this one place.
 */
fun mapTokenColorsIntoColorScheme(theme: ReferenceAppUXTokenColors): ColorScheme {
    val companionSurface = companionColorOn(theme)
    return ColorScheme(
        primary = theme.accent,
        onPrimary = companionSurface,
        primaryContainer = theme.accent,
        onPrimaryContainer = companionSurface,
        inversePrimary = theme.accent,
        secondary = theme.warning,
        onSecondary = companionSurface,
        secondaryContainer = theme.warning,
        onSecondaryContainer = companionSurface,
        tertiary = theme.positive,
        onTertiary = companionSurface,
        tertiaryContainer = theme.positive,
        onTertiaryContainer = companionSurface,
        background = companionSurface,
        onBackground = theme.textPrimary,
        surface = theme.surface,
        onSurface = theme.textPrimary,
        surfaceVariant = theme.surface,
        onSurfaceVariant = theme.textPrimary,
        surfaceTint = theme.surface,
        inverseSurface = companionSurface,
        inverseOnSurface = theme.textPrimary,
        error = theme.danger,
        onError = companionSurface,
        errorContainer = theme.danger,
        onErrorContainer = companionSurface,
        outline = theme.surfaceBorder,
        outlineVariant = theme.surfaceBorder,
        scrim = companionSurface,
        surfaceBright = theme.surface,
        surfaceDim = theme.surface,
        surfaceContainer = theme.surface,
        surfaceContainerHigh = theme.surface,
        surfaceContainerHighest = theme.surface,
        surfaceContainerLow = theme.surface,
        surfaceContainerLowest = theme.surface,
    )
}

/**
 * The typography scale of `docs/ReferenceAppUX.md` §2.3 mapped onto Material
 * slots: `type-caption` onto `labelSmall`, `type-body` onto `bodySmall`,
 * `type-body-strong` onto `labelLarge` (the label slot Material uses for
 * labeled controls), `type-title` onto `titleSmall`, and both maneuver-scale
 * slots onto `headlineSmall`/`headlineMedium` (§8: `type-maneuver*` map onto
 * the headline slot). Numerals in the readout use the platform monospace
 * stack through the dedicated readout style below; nothing else is
 * monospace.
 */
fun mapTokenTypographyIntoTypography(): Typography = Typography(
    labelSmall = TextStyle(
        fontSize = 12.sp,
        fontWeight = FontWeight(400),
    ),
    bodySmall = TextStyle(
        fontSize = 13.sp,
        fontWeight = FontWeight(400),
    ),
    labelLarge = TextStyle(
        fontSize = 13.sp,
        fontWeight = FontWeight(600),
    ),
    titleSmall = TextStyle(
        fontSize = 14.sp,
        fontWeight = FontWeight(600),
    ),
    headlineSmall = TextStyle(
        fontSize = 22.sp,
        fontWeight = FontWeight(600),
    ),
    headlineMedium = TextStyle(
        fontSize = 22.sp,
        fontWeight = FontWeight(400),
    ),
)

/** The monospace readout style of §2.3 (`--font-numeric-readout`, body scale). */
val numericReadoutStyle: TextStyle = TextStyle(
    fontSize = 13.sp,
    fontWeight = FontWeight(400),
    fontFamily = FontFamily.Monospace,
)

/**
 * The one uniform Compose theme of the design language: theme state is
 * explicit (§3 — never resolved from the OS preference at runtime), dark is
 * the default, and every panel component resolves tokens from
 * [MaterialTheme] or [LocalReferenceAppUXExtendedColors] only.
 */
@Composable
fun ReferenceAppUXTheme(
    isDarkTheme: Boolean,
    content: @Composable () -> Unit,
) {
    val tokens = if (isDarkTheme) ReferenceAppUXTokenColorsDark else ReferenceAppUXTokenColorsLight
    val extendedColors = ReferenceAppUXExtendedColors(
        textSecondary = tokens.textSecondary,
        positive = tokens.positive,
        warning = tokens.warning,
        danger = tokens.danger,
        surfaceBorder = tokens.surfaceBorder,
    )
    MaterialTheme(
        colorScheme = mapTokenColorsIntoColorScheme(theme = tokens),
        typography = mapTokenTypographyIntoTypography(),
    ) {
        CompositionLocalProvider(LocalReferenceAppUXExtendedColors provides extendedColors) {
            content()
        }
    }
}