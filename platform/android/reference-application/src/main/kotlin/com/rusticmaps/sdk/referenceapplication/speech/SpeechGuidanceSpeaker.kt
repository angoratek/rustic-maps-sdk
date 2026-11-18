package com.rusticmaps.sdk.referenceapplication.speech

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.os.Build
import android.speech.tts.TextToSpeech
import android.speech.tts.UtteranceProgressListener
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import com.rusticmaps.sdk.referenceapplication.ReferenceApplicationConstants

/**
 * Platform voice of the simulated guidance (docs/ReferenceAppUX.md §4.1/
 * §6.3): Android's [TextToSpeech] engine speaks the contract's *speech*
 * text channel from the instruction payloads verbatim — the payload's
 * `provider_speech_text` when the provider authored one, never paraphrased,
 * prefixed or emptied; an instruction payload without any speech text is
 * skipped, never voiced with invented words. The speaker writes no console
 * output: diagnostics detail reaches the visitor only through the
 * diagnostics pane's opt-in stream.
 *
 * Default state on; the navigation drawer's speech toggle flips it. An
 * engine-initialization failure is an honest disabled-with-reason state
 * (§6.2): the toggle renders disabled with the engine's own error text,
 * never silent.
 *
 * Round-11 (Ferrostar B4) platform half: the topic-delivered utterance
 * carries the contract's monotonic `utterance_id` — a re-delivery of the
 * same id is skipped (the speech topic is fire-and-forget; dedup and flush
 * stay platform decisions), while an identical text re-emitted is a new id
 * and follows the pacing policy below. The engine's own `utteranceId` bundle
 * value passed to [TextToSpeech.speak] is that contract id (the platform
 * counter stands in on the payload fallback, which carries no id), so a
 * progress callback identifies the contract emission it belongs to.
 * Trip speech also holds audio focus ([AudioManager]/
 * [AudioFocusRequest], `AUDIOFOCUS_GAIN_TRANSIENT_MAY_DUCK` — the Android
 * equivalent of the iOS side's duck-able session) acquired at the first
 * utterance of a voice-over, abandoned only after the engine drains, and
 * an honest pause on loss (the trip's guidance is never a zombie voice
 * talking over a call).
 */
class SpeechGuidanceSpeaker(context: Context) {
    /** Whether spoken guidance is wanted (default on, per §4.1). */
    var speechEnabled: Boolean by mutableStateOf(true)
        private set

    /** Whether the platform speech engine initialized successfully. */
    var speechEngineReady: Boolean by mutableStateOf(false)
        private set

    /** The engine's verbatim failure detail when initialization failed (§6.2). */
    var speechEngineUnavailableReason: String? by mutableStateOf(null)
        private set

    /** The last text voiced (or skipped), so a repeated line is not re-spoken. */
    var lastUtteranceText: String? by mutableStateOf(null)
        private set

    private val speechEngine: TextToSpeech

    // -------------------------------------------------------------------
    // Audio focus (round-11 B4: the trip speech's focus equivalent)
    // -------------------------------------------------------------------

    /** The system audio manager the focus requests go through. */
    private val audioManager: AudioManager

    /** The focus attributes: navigation guidance speech on the media stream. */
    private val speechAudioAttributes: AudioAttributes

    /** The focus change listener: loss stops the guidance honestly (§6.2). */
    private val audioFocusChangeListener: AudioManager.OnAudioFocusChangeListener =
        AudioManager.OnAudioFocusChangeListener { focusChange ->
            when (focusChange) {
                AudioManager.AUDIOFOCUS_LOSS,
                AudioManager.AUDIOFOCUS_LOSS_TRANSIENT,
                -> {
                    // The honest focus-loss pause: another consumer claimed
                    // transient or full focus (a call, another guidance); the
                    // queue-flush pacing carries nothing queued, so the
                    // guidance stops and the next instruction re-speaks —
                    // never a zombie voice over the call.
                    speechEngine.stop()
                }
                AudioManager.AUDIOFOCUS_LOSS_TRANSIENT_CAN_DUCK -> {
                    // Ducked is the guidance's own granted condition; the
                    // system lowers the volume and nothing stops.
                }
                else -> {
                }
            }
        }

    /** Whether the speaker currently holds the trip speech's audio focus. */
    private var isAudioFocusHeld: Boolean = false

    /** The focus request built once (API 26+; See the null legacy path below). */
    private var audioFocusRequest: AudioFocusRequest? = null

    /** The contract utterance id (round-11 B4) of the line last spoken. */
    private var lastSpokenInstructionId: ULong? = null

    init {
        var engineInitListener: TextToSpeech.OnInitListener? = null
        engineInitListener = TextToSpeech.OnInitListener { initializationStatus ->
            val succeeded = initializationStatus == TextToSpeech.SUCCESS
            speechEngineReady = succeeded
            if (succeeded) {
                speechEngine.setLanguage(
                    java.util.Locale.forLanguageTag(ReferenceApplicationConstants.SPEECH_LANGUAGE_TAG),
                )
            } else {
                // §6.2: an unavailable engine is a state, never a silent
                // fallback. The status code's own name is the verbatim text.
                speechEngineUnavailableReason = "Android TextToSpeech engine failed to initialize " +
                    "(status $initializationStatus); spoken guidance stays unavailable"
            }
        }
        speechEngine = TextToSpeech(context.applicationContext, engineInitListener)
        audioManager = context.applicationContext.getSystemService(Context.AUDIO_SERVICE) as AudioManager
        speechAudioAttributes = AudioAttributes.Builder()
            .setUsage(AudioAttributes.USAGE_ASSISTANCE_NAVIGATION_GUIDANCE)
            .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
            .build()
        // The drain guard of the focus equivalent: whatever ends a
        // utterance (done, error, flushed, stopped), an idling engine
        // abandons the trip speech's focus — release happens after the
        // queue drains, never during active speech.
        speechEngine.setOnUtteranceProgressListener(
            object : UtteranceProgressListener() {
                override fun onStart(utteranceId: String?) {
                }

                override fun onDone(utteranceId: String?) {
                    abandonAudioFocusWhenEngineIdle("utterance done: $utteranceId")
                }

                override fun onError(utteranceId: String?, errorCode: Int) {
                    abandonAudioFocusWhenEngineIdle("utterance error: $utteranceId ($errorCode)")
                }

                @Deprecated(
                    "Deprecated in Java (the engine dispatches the error-code overload on API 23+)",
                )
                override fun onError(utteranceId: String?) {
                    abandonAudioFocusWhenEngineIdle("utterance error: $utteranceId")
                }

                override fun onStop(utteranceId: String?, interrupted: Boolean) {
                    abandonAudioFocusWhenEngineIdle("utterance stopped: $utteranceId")
                }
            },
        )
    }

    /** Toggles spoken guidance; switching on announces itself on the next instruction. */
    fun requestSpeechEnabled(enabled: Boolean) {
        if (!speechEngineReady) {
            return
        }
        speechEnabled = enabled
        if (!enabled) {
            speechEngine.stop()
        }
    }

    /**
     * Speaks the instruction payload's speech channel verbatim (the provider
     * speech text; the composed display text is the display channel and
     * never stands in for speech). `speechInstructionId` is the round-11
     * contract's `utterance_id` when the speech topic delivered the line
     * (null on the payload fallback, which carries no id): a re-delivery of
     * the same id is skipped, and the id becomes the engine's `utteranceId`
     * bundle value so the progress callbacks identify the emission. A
     * repeated text is not voiced twice in a row — one maneuver spoken once
     * per its duration.
     */
    fun speakInstruction(
        speechText: String?,
        speechInstructionId: ULong? = null,
    ) {
        if (speechText == null) {
            return
        }
        if (!speechEngineReady) {
            return
        }
        if (!speechEnabled) {
            return
        }
        if (speechInstructionId != null && speechInstructionId == lastSpokenInstructionId) {
            return
        }
        if (speechText == lastUtteranceText) {
            return
        }
        lastUtteranceText = speechText
        // Pacing: the new utterance cancels the queued and in-flight speech
        // (stop() drops everything pending, speak() with QUEUE_FLUSH replaces
        // whatever is left mid-sentence) — the guidance never overlaps or
        // queues far into the future; at demo playback speed instructions
        // arrive seconds apart.
        speechEngine.stop()
        requestAudioFocusForSpeech()
        speechEngine.speak(
            speechText,
            TextToSpeech.QUEUE_FLUSH,
            null,
            engineUtteranceIdentifierOf(speechInstructionId),
        )
        if (speechInstructionId != null) {
            lastSpokenInstructionId = speechInstructionId
        }
    }

    /**
     * The engine's `utteranceId` bundle value of one utterance: the
     * contract's `utterance_id` verbatim when a topic delivery carries it
     * (the progress callbacks then identify the contract emission), the
     * platform's own monotonic counter on the payload fallback (no contract
     * id exists there).
     */
    private fun engineUtteranceIdentifierOf(speechInstructionId: ULong?): String =
        speechInstructionId?.toString() ?: platformUtteranceIdentifier().toString()

    private var platformUtteranceIdentifierNextValue: Int = 0

    private fun platformUtteranceIdentifier(): Int {
        platformUtteranceIdentifierNextValue += 1
        return platformUtteranceIdentifierNextValue
    }

    // -------------------------------------------------------------------
    // Audio focus (round-11 B4: gain at trip speech, abandon after drain)
    // -------------------------------------------------------------------

    /**
     * Acquires the trip speech's audio focus before the guidance's first
     * (or next) utterance voices: a transient, may-duck request built over
     * the navigation-guidance speech attributes — the guidance ducks other
     * audio while it speaks, and an in-flight utterance keeps its focus.
     * A denied request is recorded honestly; the guidance still speaks
     * unmanaged rather than silently.
     */
    private fun requestAudioFocusForSpeech() {
        if (isAudioFocusHeld) {
            return
        }
        val granted = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val request = AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN_TRANSIENT_MAY_DUCK)
                .setAudioAttributes(speechAudioAttributes)
                .setOnAudioFocusChangeListener(audioFocusChangeListener)
                .build()
            audioFocusRequest = request
            audioManager.requestAudioFocus(request)
        } else {
            @Suppress("DEPRECATION")
            audioManager.requestAudioFocus(
                audioFocusChangeListener,
                AudioManager.STREAM_MUSIC,
                AudioManager.AUDIOFOCUS_GAIN_TRANSIENT_MAY_DUCK,
            )
        }
        isAudioFocusHeld = granted == AudioManager.AUDIOFOCUS_REQUEST_GRANTED
    }

    /**
     * Abandons the trip speech's audio focus when the engine has drained:
     * fired from every utterance-completion path (done, error, stopped)
     * and never while the engine still speaks — the tail of the last
     * instruction keeps its focus until it finishes.
     */
    private fun abandonAudioFocusWhenEngineIdle(completionReason: String) {
        if (speechEngine.isSpeaking) {
            return
        }
        abandonAudioFocus("after drain ($completionReason)")
    }

    /**
     * Abandons the trip speech's audio focus when held: the trip's
     * [releaseJourneySpeech] call sites (stop, exit) and the drain paths.
     * A focus never held is a no-op.
     */
    private fun abandonAudioFocus(whenReason: String) {
        if (!isAudioFocusHeld) {
            return
        }
        val abandoned = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val request = audioFocusRequest
            if (request == null) {
                false
            } else {
                audioManager.abandonAudioFocusRequest(request)
            }
        } else {
            @Suppress("DEPRECATION")
            audioManager.abandonAudioFocus(audioFocusChangeListener)
        }
        isAudioFocusHeld = false
    }

    /**
     * Ends the trip's speech lifecycle (the simulated playback's end and
     * the drawer's exit): the focus equivalent abandons when the engine
     * has already drained, and an in-flight utterance keeps its focus
     * until its completion callback abandons after drain — the tail of
     * the last instruction is never cut by the trip ending.
     */
    fun releaseJourneySpeech() {
        if (speechEngine.isSpeaking) {
            return
        }
        abandonAudioFocus("journey speech release")
    }

    /**
     * Records the arrival state's speech behavior honestly: the arrival
     * payload carries no speech text, so arrival is recorded as a skipped
     * utterance, never voiced with invented words (§6.3's verbatim rule).
     */
    fun noteArrivalWithoutSpeech() {
        lastUtteranceText = null
    }

    /**
     * Ends the speaker's platform resources when the hosting activity is
     * destroyed: stops any in-flight utterance, releases the engine and
     * abandons the trip speech's audio focus. A platform refusal during
     * teardown is ignored — the process is releasing the engine anyway.
     */
    fun shutdown() {
        speechEnabled = false
        runCatching { speechEngine.stop() }
        runCatching { speechEngine.shutdown() }
        abandonAudioFocus("speech engine shutdown")
    }

}