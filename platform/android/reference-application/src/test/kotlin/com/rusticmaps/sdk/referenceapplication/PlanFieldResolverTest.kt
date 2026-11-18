package com.rusticmaps.sdk.referenceapplication

import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import uniffi.rustic_maps.GeocodeQuality
import uniffi.rustic_maps.GeographicCoordinate
import uniffi.rustic_maps.Place

/**
 * Unit tests of the plan field's typed-text resolution: the assigned place
 * wins while the text still names it, an exact demo-place name resolves
 * without a provider, and the provider's first result is the last resort.
 * The order is the documented one every target follows
 * (docs/design/ReferenceApplicationBehavior.md).
 */
class PlanFieldResolverTest {
    @Test
    fun `the assigned place resolves while the text still names it`() {
        val assignedPlace = place("Capitol Park")
        var demoLookups = 0
        var providerLookups = 0
        val resolved = runBlocking {
            PlanFieldResolver.resolve(
                typedText = "Capitol Park",
                assignedPlace = assignedPlace,
                demoPlaceNamed = { demoLookups += 1; null },
                providerFirstResult = { providerLookups += 1; null },
            )
        }
        assertEquals(assignedPlace, resolved)
        assertEquals(0, demoLookups)
        assertEquals(0, providerLookups)
    }

    @Test
    fun `an edited text no longer resolves to the stale assigned place`() {
        val assignedPlace = place("Capitol Park")
        val demoPlace = place("Tower Bridge")
        var providerLookups = 0
        val resolved = runBlocking {
            PlanFieldResolver.resolve(
                typedText = "Tower Bridge",
                assignedPlace = assignedPlace,
                demoPlaceNamed = { name -> demoPlace.takeIf { name == "Tower Bridge" } },
                providerFirstResult = { providerLookups += 1; null },
            )
        }
        assertEquals(demoPlace, resolved)
        assertEquals(0, providerLookups)
    }

    @Test
    fun `an exact demo place resolves before any provider result`() {
        val demoPlace = place("Tower Bridge")
        var providerLookups = 0
        val resolved = runBlocking {
            PlanFieldResolver.resolve(
                typedText = "tower bridge",
                assignedPlace = null,
                demoPlaceNamed = { name -> demoPlace.takeIf { name.equals("Tower Bridge", true) } },
                providerFirstResult = { providerLookups += 1; null },
            )
        }
        assertEquals(demoPlace, resolved)
        assertEquals(0, providerLookups)
    }

    @Test
    fun `the provider's first result resolves free-form text`() {
        val providerPlace = place("1000 J Street")
        val resolved = runBlocking {
            PlanFieldResolver.resolve(
                typedText = "1000 J Street",
                assignedPlace = null,
                demoPlaceNamed = { null },
                providerFirstResult = { providerPlace },
            )
        }
        assertEquals(providerPlace, resolved)
    }

    @Test
    fun `a field that resolves to nothing returns null`() {
        assertNull(
            runBlocking {
                PlanFieldResolver.resolve(
                    typedText = "Nowhere at all",
                    assignedPlace = null,
                    demoPlaceNamed = { null },
                    providerFirstResult = { null },
                )
            },
        )
        assertNull(
            runBlocking {
                PlanFieldResolver.resolve(
                    typedText = "   ",
                    assignedPlace = place("Capitol Park"),
                    demoPlaceNamed = { null },
                    providerFirstResult = { null },
                )
            },
        )
    }

    private fun place(addressText: String): Place = Place(
        structuredAddressText = addressText,
        coordinate = GeographicCoordinate(latitude = 38.5767, longitude = -121.4934),
        geocodeQuality = GeocodeQuality.POINT,
    )
}
