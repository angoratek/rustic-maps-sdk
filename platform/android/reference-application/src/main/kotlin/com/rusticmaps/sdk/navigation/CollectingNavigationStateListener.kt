package com.rusticmaps.sdk.navigation

import androidx.compose.runtime.mutableStateOf
import uniffi.rustic_maps.DeviationStatusChange
import uniffi.rustic_maps.NavigationInstruction
import uniffi.rustic_maps.NavigationPhaseChange
import uniffi.rustic_maps.NavigationListener
import uniffi.rustic_maps.SpeechInstruction

/**
 * Main-thread-safe listener dispatch glue (M5 Kotlin slice): the SDK's
 * listener callbacks arrive synchronously on the thread that drives the
 * `Navigator` call (the platform seam), so this collector holds
 * the delivered payloads in Compose snapshot state — snapshot writes are
 * thread-safe from any thread, and recomposition reads the latest value on
 * the UI thread with no manual thread hop.
 *
 * The reference applications drive the UI from these listener topics
 * (`docs/ReferenceAppUX.md` §7.2) and use the controller's read-only queries
 * only to seed an initial state. Listeners never re-enter the controller.
 *
 * The collector implements [NavigationListener] once; the same
 * instance is subscribed to the controller's topics (the round-10 speech
 * topic included).
 */
class CollectingNavigationStateListener : NavigationListener {
    /**
     * Latest delivered instruction payload (the instructions topic); Compose
     * state, so readers observe the newest payload without a dispatcher.
     */
    val instructionState = mutableStateOf<NavigationInstruction?>(value = null)

    /** Latest delivered deviation-status update (the deviation topic). */
    val deviationStatusChangeState = mutableStateOf<DeviationStatusChange?>(value = null)

    /** Latest delivered phase transition (the phase topic). */
    val phaseChangeState = mutableStateOf<NavigationPhaseChange?>(value = null)

    /**
     * Latest delivered speech-instruction trigger (the round-10 speech
     * topic, OQ5): the SDK's distance-band policy decided the fix was
     * newsworthy; the platform speaks only these deliveries, verbatim.
     */
    val speechInstructionState = mutableStateOf<SpeechInstruction?>(value = null)

    override fun onNavigationInstruction(instruction: NavigationInstruction) {
        instructionState.value = instruction
    }

    override fun onDeviationStatusChange(deviationStatusChange: DeviationStatusChange) {
        deviationStatusChangeState.value = deviationStatusChange
    }

    override fun onNavigationPhaseChange(navigationPhaseChange: NavigationPhaseChange) {
        phaseChangeState.value = navigationPhaseChange
    }

    override fun onSpeechInstruction(speechInstruction: SpeechInstruction) {
        speechInstructionState.value = speechInstruction
    }

    /**
     * Clears every delivered-topic state so a fresh trip begins with no
     * residual payload: the controller's topics replay their latest event
     * again on re-subscription, so the reference applications must reset the
     * collector when a trip stops — otherwise a new trip's playback reads the
     * previous trip's `ARRIVED` phase and never drives a fix.
     */
    fun reset() {
        instructionState.value = null
        deviationStatusChangeState.value = null
        phaseChangeState.value = null
        speechInstructionState.value = null
    }
}