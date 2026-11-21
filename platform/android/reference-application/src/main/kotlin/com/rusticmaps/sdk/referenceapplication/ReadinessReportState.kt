package com.rusticmaps.sdk.referenceapplication

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import uniffi.rustic_maps.SdkException

/**
 * The readiness ladder's states (docs/ReferenceAppUX.md §6.1): loading while
 * the SDK resolves, ready once a frame draws, degraded when a documented
 * capability substitute fires, and failed when the SDK cannot start. A
 * credential-less session is the neutral keyless prompt of §6.5, not a failure.
 */
enum class ReadinessState {
    /** SDK module/provider registration still resolving. */
    LOADING,

    /** The GPU surface renderer is active. */
    READY,

    /** A documented capability substitute fired. */
    DEGRADED,

    /** The SDK failed to start; the detail names the failure verbatim. */
    FAILED,
}

/**
 * One reported SDK failure (§6.3): the provider-echoing label line (the
 * `SdkException` variant name when the failure crossed the contract, the
 * failure origin otherwise) and the verbatim detail text.
 */
data class SdkExceptionReport(
    /** The label line: the contract variant's own name or the failure origin. */
    val variantLabel: String,
    /** The failure detail echoed verbatim — never paraphrased or swallowed. */
    val detailText: String,
)

/**
 * The readiness ladder and the provider-echoing error report as one state
 * type (docs/ReferenceAppUX.md §6.1, §6.3).
 *
 * The type owns the ladder's current state, the diagnostic detail behind a
 * degraded or failed state, and the error report the screen renders until the
 * next successful action clears it. [sdkVersion] fills the ready chip's
 * version slot; [isProviderKeyless] gates the keyless prompt and suppresses a
 * provider-not-configured failure the visitor has not caused yet (§6.5). No
 * method here reaches the network or the SDK: callers hand in the failure they
 * already observed.
 *
 * @param sdkVersion the compiled SDK version the ready chip reports.
 * @param isProviderKeyless whether no provider credential is applied in this
 *   session, read at each transition.
 */
class ReadinessReportState(
    private val sdkVersion: String,
    private val isProviderKeyless: () -> Boolean,
) {
    /** The ladder's current state. */
    var readinessState: ReadinessState by mutableStateOf(ReadinessState.LOADING)
        private set

    /** Verbatim failure detail backing a degraded or failed state, or null. */
    var readinessDetail: String? by mutableStateOf(null)
        private set

    /** The provider-echoing error report, or null after a successful action. */
    var errorReport: SdkExceptionReport? by mutableStateOf(null)
        private set

    /** The readiness chip's short ladder line (§6.1's four states, text form). */
    val readinessChipText: String
        get() = if (isProviderKeyless() && readinessState != ReadinessState.READY) {
            // The neutral keyless prompt, never an endless "Loading…" or a
            // provider failure the user has not caused yet (§6.5).
            ReferenceApplicationConstants.KEYLESS_CREDENTIAL_STATUS_TEXT
        } else {
            when (readinessState) {
                ReadinessState.LOADING ->
                    ReferenceApplicationConstants.LOADING_STATUS_TEXT +
                        ReferenceApplicationConstants.READINESS_LOADING_SUFFIX
                ReadinessState.READY ->
                    ReferenceApplicationConstants.READY_STATUS_TEMPLATE.format(sdkVersion)
                ReadinessState.DEGRADED ->
                    ReferenceApplicationConstants.DEGRADED_STATUS_TEMPLATE.format(
                        readinessDetail ?: NO_FRAME_DRAW_DETAIL_TEXT,
                    )
                ReadinessState.FAILED ->
                    ReferenceApplicationConstants.FAILED_STATUS_TEMPLATE.format(
                        readinessDetail ?: UNKNOWN_FAILURE_DETAIL_TEXT,
                    )
            }
        }

    /** Marks the ladder ready without touching the error report. */
    fun noteReady() {
        readinessState = ReadinessState.READY
        readinessDetail = null
    }

    /** Returns the ladder to loading without touching the error report. */
    fun noteLoading() {
        readinessState = ReadinessState.LOADING
        readinessDetail = null
    }

    /**
     * Records a degraded or failed transition with its verbatim detail. The
     * caller reports the failure through [reportFailure].
     */
    fun noteFailure(state: ReadinessState, detailText: String) {
        readinessState = state
        readinessDetail = detailText
    }

    /**
     * Settles a provider-not-configured failure while keyless into the neutral
     * prompt: the ladder returns to loading and the report clears, because the
     * expected startup failure is not the visitor's (§6.5).
     */
    fun noteExpectedKeylessFailure() {
        noteLoading()
        clearErrorReport()
    }

    /** Echoes a contract failure into the error report, suppressing keyless noise. */
    fun reportSdkFailure(exception: SdkException) {
        if (isExpectedKeylessProviderFailure(exception)) {
            clearErrorReport()
            return
        }
        publish(
            SdkExceptionReport(
                variantLabel = variantLabelOf(exception),
                detailText = failureDetailText(exception),
            ),
        )
    }

    /**
     * Echoes any observed failure into the error report. A native-internal
     * failure (uniffi raises wgpu and other Rust-side errors as
     * `InternalException`, which is no `SdkException` variant) reports its
     * class name and message verbatim (§6.3).
     */
    fun reportFailure(failure: Throwable) {
        if (isExpectedKeylessProviderFailure(failure)) {
            // The keyless prompt already tells the user what to do; a red
            // provider-not-configured strip would be noise (§6.5).
            clearErrorReport()
            return
        }
        publish(
            SdkExceptionReport(
                variantLabel = failure::class.simpleName ?: UNKNOWN_VARIANT_LABEL,
                detailText = failureDetailTextOf(failure),
            ),
        )
    }

    /** Echoes a local (non-contract) failure naming its origin. */
    fun reportLocalFailure(variantLabel: String, detailText: String) {
        publish(SdkExceptionReport(variantLabel = variantLabel, detailText = detailText))
    }

    /** Clears the error report on the next successful action (§6.3). */
    fun clearErrorReport() {
        errorReport = null
    }

    /**
     * Resets a failed or degraded ladder before the surface attach is retried,
     * so the retry renders as loading rather than a stale failure, and clears
     * the report.
     */
    fun prepareSurfaceRetry() {
        if (readinessState == ReadinessState.FAILED || readinessState == ReadinessState.DEGRADED) {
            noteLoading()
        }
        clearErrorReport()
    }

    /**
     * The verbatim detail text of any failure: a declared `SdkException`
     * reports its mapped fields; anything else reports its message, class name
     * or [UNKNOWN_FAILURE_DETAIL_TEXT] in that order.
     */
    fun failureDetailTextOf(failure: Throwable): String = when (failure) {
        is SdkException -> failureDetailText(failure)
        else -> failure.message ?: failure::class.simpleName ?: UNKNOWN_FAILURE_DETAIL_TEXT
    }

    /** The contract error variant's own name, the §6.3 label line. */
    private fun variantLabelOf(exception: SdkException): String =
        exception::class.simpleName ?: SDK_EXCEPTION_VARIANT_LABEL

    /**
     * The verbatim detail text of a contract error. The two sentences the
     * contract does not carry as a detail string are the shared
     * reference-tokens artifact's `strings.rate_limit_exhausted_detail` and
     * `strings.provider_not_configured_detail_template`
     * (platform/reference-tokens.json); scripts/check_ux_parity.mjs gates the
     * text against that artifact.
     */
    private fun failureDetailText(exception: SdkException): String = when (exception) {
        is SdkException.InvalidCoordinateInput -> exception.detail
        is SdkException.NetworkRequestFailed -> exception.detail
        is SdkException.ResponseParsingFailure -> exception.detail
        is SdkException.ProviderRequestRejected -> exception.detail
        // platform/reference-tokens.json strings.provider_not_configured_detail_template
        is SdkException.ProviderNotConfigured ->
            "no '${exception.contractName}' provider is configured"
        is SdkException.InvalidRenderInput -> exception.detail
        is SdkException.RenderingSurfaceUnavailable -> exception.detail
        is SdkException.InvalidNavigationInput -> exception.detail
        // The cache and offline variants carry their own verbatim reason; the
        // screen states it rather than inventing a sentence for it.
        is SdkException.CacheOperationFailure -> exception.detail
        is SdkException.OfflineRoutingFailure -> exception.detail
        // platform/reference-tokens.json strings.rate_limit_exhausted_detail
        is SdkException.RateLimitExhausted ->
            "rate limit exhausted; retry after the next refill"
    }

    /**
     * A provider-not-configured failure that is expected while keyless: the
     * surface attach and tile fetches cannot work before a credential is
     * applied, so they must not paint a failed readiness state or a red error
     * strip (§6.5's neutral keyless prompt). Any other failure reports
     * verbatim.
     */
    fun isExpectedKeylessProviderFailure(failure: Throwable): Boolean =
        isProviderKeyless() && failure is SdkException.ProviderNotConfigured

    /** Publishes a report; a repeated identical failure does not retrigger recomposition. */
    private fun publish(report: SdkExceptionReport) {
        if (errorReport != report) {
            errorReport = report
        }
    }

    private companion object {
        /** The degraded chip's detail when no failure text was recorded. */
        const val NO_FRAME_DRAW_DETAIL_TEXT: String = "a frame draw failed"

        /** The failed chip's detail when no failure text was recorded. */
        const val UNKNOWN_FAILURE_DETAIL_TEXT: String = "unknown failure"

        /** The contract failure label when the variant class has no simple name. */
        const val SDK_EXCEPTION_VARIANT_LABEL: String = "SdkException"

        /** The generic failure label when the class has no simple name. */
        const val UNKNOWN_VARIANT_LABEL: String = "UnknownException"
    }
}
