package com.rusticmaps.sdk.referenceapplication

import uniffi.rustic_maps.Place

/**
 * The plan fields' typed-text resolution (docs/ReferenceAppUX.md,
 * "Search, route card and navigation UX"): one field's text resolves in the
 * same order on every target.
 *
 * 1. the place the field already holds (an assigned result or a my-location
 *    fill), while the text still names it;
 * 2. an exact demo-place name from the shared artifact;
 * 3. the provider's first geocoding result.
 *
 * The order is pure and provider-neutral, so the state holder owns only the
 * I/O around step three. A field that resolves to nothing is reported by the
 * caller; Plan route never silently stays disabled while the reader types.
 */
object PlanFieldResolver {
    /**
     * Resolves [typedText] for one plan field.
     *
     * @param typedText The field's current text; blank never resolves.
     * @param assignedPlace The place the field currently holds, or null.
     * @param demoPlaceNamed Looks up the shared artifact's exact demo-place
     *   name, or returns null when the artifact records none.
     * @param providerFirstResult Asks the serving provider for the first
     *   geocoding result, or returns null when the session has no geocoding
     *   provider. The caller reports a provider failure instead of swallowing
     *   it.
     * @return The resolved place, or null when the field resolves to nothing.
     */
    suspend fun resolve(
        typedText: String,
        assignedPlace: Place?,
        demoPlaceNamed: (String) -> Place?,
        providerFirstResult: suspend (String) -> Place?,
    ): Place? {
        val trimmedText = typedText.trim()
        if (trimmedText.isEmpty()) {
            return null
        }
        val heldPlace = assignedPlace?.takeIf { place ->
            place.structuredAddressText.trim().equals(trimmedText, ignoreCase = true)
        }
        if (heldPlace != null) {
            return heldPlace
        }
        return demoPlaceNamed(trimmedText)
            ?: providerFirstResult(trimmedText)
    }
}
