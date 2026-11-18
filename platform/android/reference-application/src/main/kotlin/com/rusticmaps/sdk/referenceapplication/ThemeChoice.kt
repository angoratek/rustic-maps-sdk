package com.rusticmaps.sdk.referenceapplication

/**
 * The reference application's theme choice (docs/ReferenceAppUX.md §3, the
 * owner ruling of 2026-10-30 and PRD §6 item 9): three explicit choices —
 * Light, Dark and Auto — with Auto the default on a device that never chose.
 * Auto follows the operating system's color scheme live; an explicit Light
 * or Dark choice is remembered on the device and wins from then on. The
 * choice itself never leaves the device.
 *
 * The stored value is the lowercase name the iOS application and the web
 * page also use, so all four surfaces speak one theme vocabulary.
 */
enum class ThemeChoice(val storageValue: String) {
    /** The light presentation: light panels over the dark canvas. */
    LIGHT("light"),

    /** The dark presentation: dark translucent panels over the dark canvas. */
    DARK("dark"),

    /** The operating system's choice, followed live; the default state. */
    AUTO("auto");

    /** The choice the toggle cycles to (light → dark → auto → light). */
    val nextChoice: ThemeChoice
        get() = when (this) {
            LIGHT -> DARK
            DARK -> AUTO
            AUTO -> LIGHT
        }

    /** The toggle's label, always naming the choice it switches to next (§3). */
    val switchControlLabel: String
        get() = "Switch to ${nextChoice.storageValue} theme"

    /**
     * The resolved presentation: the choice itself for Light and Dark, the
     * operating system's color scheme while Auto.
     */
    fun resolvesToDarkTheme(systemInDarkTheme: Boolean): Boolean = when (this) {
        LIGHT -> false
        DARK -> true
        AUTO -> systemInDarkTheme
    }

    companion object {
        /** The choice a stored value names, or Auto when it names none. */
        fun fromStorageValue(storedValue: String?): ThemeChoice =
            entries.firstOrNull { choice -> choice.storageValue == storedValue }
                ?: AUTO
    }
}
