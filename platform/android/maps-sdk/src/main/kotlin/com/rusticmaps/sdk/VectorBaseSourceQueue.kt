package com.rusticmaps.sdk

import uniffi.rustic_maps.UrlTemplateVectorTileSource

/**
 * The vector base source the wrapper recorded and the one queued install
 * awaiting the next frame.
 *
 * A source selection can land while a frame draw is in flight; the SDK refuses
 * a source change then by design, so the wrapper queues the selection instead
 * of refusing the user's tap. [source] is the value the native view is asked
 * to serve. While a queued install is pending, [rollbackSource] remembers the
 * source the view served before the first queued selection, so a refusal
 * restores the previous base rather than leaving the bookkeeping naming a
 * source the SDK never accepted.
 *
 * [push] is the native seam (install or clear); the unit tests substitute a
 * scripted answer and pin the refusal rollback.
 */
internal class VectorBaseSourceQueue {
    /** The source the wrapper asks the native view to serve; null means clear. */
    var source: UrlTemplateVectorTileSource? = null
        private set

    private var isInstallPending: Boolean = false
    private var rollbackSource: UrlTemplateVectorTileSource? = null
    private var hasRollbackSource: Boolean = false

    /** Records a source without pushing it (the native view does not exist yet). */
    fun record(nextSource: UrlTemplateVectorTileSource?) {
        source = nextSource
    }

    /**
     * Records [nextSource] and pushes it immediately unless a draw is in
     * flight, in which case the install is queued for the next pump tick.
     * A refused push restores the previously recorded source and rethrows
     * [push]'s failure for the caller to report.
     */
    fun select(
        nextSource: UrlTemplateVectorTileSource?,
        isDrawInFlight: Boolean,
        push: (UrlTemplateVectorTileSource?) -> Unit,
    ) {
        val previousSource = source
        source = nextSource
        if (isDrawInFlight) {
            if (!isInstallPending) {
                rollbackSource = previousSource
                hasRollbackSource = true
            }
            isInstallPending = true
            return
        }
        // A draw finished since the last selection, so an immediate change
        // now supersedes any queued one. The source the native view actually
        // serves is the queued rollback target, not the unserved selection
        // that overwrote it.
        val servedSource =
            if (isInstallPending && hasRollbackSource) rollbackSource else previousSource
        isInstallPending = false
        clearRollback()
        try {
            push(nextSource)
        } catch (thrown: Throwable) {
            source = servedSource
            throw thrown
        }
    }

    /**
     * Hands a queued selection to [push] once no draw is in flight.
     *
     * @return true when a queued selection was applied, false when nothing was
     *   queued or a draw still holds the view.
     * @throws Throwable the push's refusal, after restoring the source the view
     *   served before the first queued selection.
     */
    fun applyPending(
        isDrawInFlight: Boolean,
        push: (UrlTemplateVectorTileSource?) -> Unit,
    ): Boolean {
        if (!isInstallPending || isDrawInFlight) {
            return false
        }
        isInstallPending = false
        return try {
            push(source)
            clearRollback()
            true
        } catch (thrown: Throwable) {
            if (hasRollbackSource) {
                source = rollbackSource
            }
            clearRollback()
            throw thrown
        }
    }

    private fun clearRollback() {
        hasRollbackSource = false
        rollbackSource = null
    }
}
