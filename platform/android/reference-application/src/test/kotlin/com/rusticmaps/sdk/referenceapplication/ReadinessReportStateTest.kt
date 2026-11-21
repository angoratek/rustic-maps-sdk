package com.rusticmaps.sdk.referenceapplication

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.rustic_maps.SdkException

/**
 * Unit tests of the readiness ladder and the error report
 * (docs/ReferenceAppUX.md §6.1, §6.3): the chip's text for every ladder state,
 * the keyless prompt, the contract variants' verbatim detail mapping, the
 * expected keyless suppression, local reports and the surface-retry reset.
 * Pure Kotlin; no test reaches the network or the SDK.
 */
class ReadinessReportStateTest {
    @Test
    fun `the ladder starts loading and the keyless prompt replaces loading`() {
        var keyless = true
        val state = newState(isProviderKeyless = { keyless })

        assertEquals(ReadinessState.LOADING, state.readinessState)
        assertNull(state.readinessDetail)
        assertNull(state.errorReport)
        assertEquals(
            ReferenceApplicationConstants.KEYLESS_CREDENTIAL_STATUS_TEXT,
            state.readinessChipText,
        )

        keyless = false
        assertEquals(
            ReferenceApplicationConstants.LOADING_STATUS_TEXT +
                ReferenceApplicationConstants.READINESS_LOADING_SUFFIX,
            state.readinessChipText,
        )
    }

    @Test
    fun `a drawn frame makes the ladder ready and the ready chip names the version`() {
        val state = newState()
        state.noteLoading()

        state.noteReady()

        assertEquals(ReadinessState.READY, state.readinessState)
        assertNull(state.readinessDetail)
        assertEquals(
            ReferenceApplicationConstants.READY_STATUS_TEMPLATE.format(SDK_VERSION),
            state.readinessChipText,
        )
    }

    @Test
    fun `degraded and failed chips carry the recorded detail`() {
        val state = newState()

        state.noteFailure(ReadinessState.DEGRADED, "frame texture unavailable")
        assertEquals(
            ReferenceApplicationConstants.DEGRADED_STATUS_TEMPLATE.format(
                "frame texture unavailable",
            ),
            state.readinessChipText,
        )

        state.noteFailure(ReadinessState.FAILED, "surface configuration refused")
        assertEquals(ReadinessState.FAILED, state.readinessState)
        assertEquals("surface configuration refused", state.readinessDetail)
        assertEquals(
            ReferenceApplicationConstants.FAILED_STATUS_TEMPLATE.format(
                "surface configuration refused",
            ),
            state.readinessChipText,
        )
    }

    @Test
    fun `contract failures report the variant label and verbatim detail`() {
        val state = newState()

        state.reportSdkFailure(SdkException.NetworkRequestFailed(detail = "connection reset"))
        assertEquals(
            SdkExceptionReport(
                variantLabel = "NetworkRequestFailed",
                detailText = "connection reset",
            ),
            state.errorReport,
        )

        state.reportSdkFailure(SdkException.ProviderNotConfigured(contractName = "Routing"))
        assertEquals(
            SdkExceptionReport(
                variantLabel = "ProviderNotConfigured",
                detailText = "no 'Routing' provider is configured",
            ),
            state.errorReport,
        )

        state.reportSdkFailure(SdkException.RateLimitExhausted())
        assertEquals(
            SdkExceptionReport(
                variantLabel = "RateLimitExhausted",
                detailText = "rate limit exhausted; retry after the next refill",
            ),
            state.errorReport,
        )
    }

    @Test
    fun `a provider-not-configured failure is suppressed while keyless`() {
        val state = newState(isProviderKeyless = { true })

        assertTrue(
            state.isExpectedKeylessProviderFailure(
                SdkException.ProviderNotConfigured(contractName = "TileSource"),
            ),
        )
        state.reportSdkFailure(SdkException.ProviderNotConfigured(contractName = "TileSource"))
        assertNull(state.errorReport)

        state.reportSdkFailure(SdkException.NetworkRequestFailed(detail = "connection reset"))
        assertEquals("NetworkRequestFailed", state.errorReport?.variantLabel)
    }

    @Test
    fun `a native-internal failure reports its class name and message`() {
        val state = newState()

        state.reportFailure(IllegalStateException("wgpu surface lost"))

        assertEquals("IllegalStateException", state.errorReport?.variantLabel)
        assertEquals("wgpu surface lost", state.errorReport?.detailText)
        assertEquals(
            "wgpu surface lost",
            state.failureDetailTextOf(IllegalStateException("wgpu surface lost")),
        )
    }

    @Test
    fun `a local failure carries its origin label and detail`() {
        val state = newState()

        state.reportLocalFailure("UnresolvedPlanField", "no place matches 'zzz'")

        assertEquals(
            SdkExceptionReport(
                variantLabel = "UnresolvedPlanField",
                detailText = "no place matches 'zzz'",
            ),
            state.errorReport,
        )
        state.clearErrorReport()
        assertNull(state.errorReport)
    }

    @Test
    fun `an expected keyless failure settles the ladder into the neutral prompt`() {
        val state = newState(isProviderKeyless = { true })
        state.reportSdkFailure(SdkException.NetworkRequestFailed(detail = "connection reset"))
        state.noteFailure(ReadinessState.FAILED, "attach refused")

        state.noteExpectedKeylessFailure()

        assertEquals(ReadinessState.LOADING, state.readinessState)
        assertNull(state.readinessDetail)
        assertNull(state.errorReport)
    }

    @Test
    fun `a surface retry resets only a failed or degraded ladder`() {
        val state = newState()

        state.noteFailure(ReadinessState.DEGRADED, "frame draw failed")
        state.reportSdkFailure(SdkException.NetworkRequestFailed(detail = "connection reset"))
        state.prepareSurfaceRetry()
        assertEquals(ReadinessState.LOADING, state.readinessState)
        assertNull(state.readinessDetail)
        assertNull(state.errorReport)

        state.noteReady()
        state.prepareSurfaceRetry()
        assertEquals(
            "a ready ladder is not downgraded by a retry",
            ReadinessState.READY,
            state.readinessState,
        )
    }

    @Test
    fun `the keyless suppression follows the applied-credential read at each call`() {
        var keyless = false
        val state = newState(isProviderKeyless = { keyless })

        assertFalse(
            state.isExpectedKeylessProviderFailure(
                SdkException.ProviderNotConfigured(contractName = "Routing"),
            ),
        )
        keyless = true
        assertTrue(
            state.isExpectedKeylessProviderFailure(
                SdkException.ProviderNotConfigured(contractName = "Routing"),
            ),
        )
    }

    private fun newState(
        isProviderKeyless: () -> Boolean = { false },
    ): ReadinessReportState = ReadinessReportState(
        sdkVersion = SDK_VERSION,
        isProviderKeyless = isProviderKeyless,
    )

    private companion object {
        /** The version the ready chip names in these tests. */
        const val SDK_VERSION: String = "9.9.9"
    }
}
