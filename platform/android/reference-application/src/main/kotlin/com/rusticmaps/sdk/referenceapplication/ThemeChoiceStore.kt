package com.rusticmaps.sdk.referenceapplication

import android.content.Context

/**
 * The device-local store of the reference application's theme choice
 * (docs/ReferenceAppUX.md §3): an explicit Light or Dark choice — and Auto
 * chosen explicitly after one of them — is remembered in this application's
 * private preferences. Nothing about the theme leaves the device, and a
 * device that never chose reads back as Auto.
 */
class ThemeChoiceStore(context: Context) {
    private val preferences = context.applicationContext.getSharedPreferences(
        PREFERENCES_NAME,
        Context.MODE_PRIVATE,
    )

    /** The remembered choice, or Auto when this device never chose. */
    fun load(): ThemeChoice =
        ThemeChoice.fromStorageValue(preferences.getString(CHOICE_PREFERENCE_KEY, null))

    /** Remembers one choice on this device. */
    fun store(choice: ThemeChoice) {
        preferences.edit()
            .putString(CHOICE_PREFERENCE_KEY, choice.storageValue)
            .apply()
    }

    /** Clears the remembered choice (the record reset's theme half). */
    fun clear() {
        preferences.edit()
            .remove(CHOICE_PREFERENCE_KEY)
            .apply()
    }

    companion object {
        /** The preferences file the theme choice lives in. */
        const val PREFERENCES_NAME: String = "rusticMapsSdk.themeChoice"

        /** The key inside that file. */
        const val CHOICE_PREFERENCE_KEY: String = "themeChoice"
    }
}
