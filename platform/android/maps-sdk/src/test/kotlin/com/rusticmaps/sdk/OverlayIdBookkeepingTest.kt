package com.rusticmaps.sdk

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.rustic_maps.GeographicCoordinate
import uniffi.rustic_maps.MapOverlay
import uniffi.rustic_maps.MapOverlayKind

/**
 * Unit cover for the overlay identifier bookkeeping's refusal rollback: a push
 * the SDK refuses must leave the tracked overlays exactly as they were, so a
 * later call never believes an overlay it never pushed and a refused removal
 * never forgets an overlay the SDK still draws.
 */
class OverlayIdBookkeepingTest {
    @Test
    fun aRefusedAddRestoresThePreviousBookkeeping() {
        val bookkeeping = OverlayIdBookkeeping()
        val firstOverlay = markerOverlay(0.0)

        val accepted = bookkeeping.add(FIRST_OVERLAY_ID, firstOverlay) { false }

        assertFalse("the refusal must be reported", accepted)
        assertFalse(
            "a refused add must not leave the overlay tracked",
            bookkeeping.contains(FIRST_OVERLAY_ID),
        )
        assertTrue(bookkeeping.descriptors.isEmpty())
    }

    @Test
    fun aRefusedRemoveKeepsTheOverlayTracked() {
        val bookkeeping = OverlayIdBookkeeping()
        val firstOverlay = markerOverlay(0.0)
        assertTrue(bookkeeping.add(FIRST_OVERLAY_ID, firstOverlay) { true })

        val accepted = bookkeeping.remove(FIRST_OVERLAY_ID) { false }

        assertFalse("the refusal must be reported", accepted)
        assertTrue(
            "a refused removal must keep the overlay the SDK still draws",
            bookkeeping.contains(FIRST_OVERLAY_ID),
        )
        assertEquals(listOf(firstOverlay), bookkeeping.descriptors)
    }

    @Test
    fun anAcceptedMutationAdvancesTheBookkeepingAndPushesTheCandidateList() {
        val bookkeeping = OverlayIdBookkeeping()
        val firstOverlay = markerOverlay(0.0)
        val replacementOverlay = markerOverlay(1.0)
        val pushedLists = mutableListOf<List<MapOverlay>>()

        assertTrue(
            bookkeeping.add(FIRST_OVERLAY_ID, firstOverlay) { candidates ->
                pushedLists.add(candidates)
                true
            },
        )
        assertTrue(
            bookkeeping.update(FIRST_OVERLAY_ID, replacementOverlay) { candidates ->
                pushedLists.add(candidates)
                true
            },
        )
        assertTrue(
            bookkeeping.remove(FIRST_OVERLAY_ID) { candidates ->
                pushedLists.add(candidates)
                true
            },
        )

        assertEquals(
            listOf(
                listOf(firstOverlay),
                listOf(replacementOverlay),
                emptyList<MapOverlay>(),
            ),
            pushedLists,
        )
        assertFalse(bookkeeping.contains(FIRST_OVERLAY_ID))
    }

    private fun markerOverlay(longitude: Double): MapOverlay = MapOverlay(
        kind = MapOverlayKind.POINT_MARKERS,
        coordinates = null,
        rgbaColor = null,
        widthPixels = null,
        ringCoordinates = null,
        pointMarkerPositions = listOf(
            GeographicCoordinate(latitude = 0.0, longitude = longitude),
        ),
        pointMarkerRadiusPixels = null,
        trafficFlowSegments = null,
    )

    private companion object {
        const val FIRST_OVERLAY_ID: String = "route-line"
    }
}
