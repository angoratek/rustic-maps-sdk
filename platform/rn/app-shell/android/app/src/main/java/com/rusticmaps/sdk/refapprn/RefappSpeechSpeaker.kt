package com.rusticmaps.sdk.refapprn

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.os.Build
import android.speech.tts.TextToSpeech
import android.speech.tts.UtteranceProgressListener
import android.util.Log

/**
 * The app shell's platform voice of the simulated guidance — the React Native
 * mirror of the Compose reference application's `SpeechGuidanceSpeaker`
 * (focus behavior): Android's [TextToSpeech] speaks the contract's *speech* text
 * channel **verbatim** — the topic's `utterance_speech_text`, never
 * paraphrased, prefixed or emptied. One line without speech text is never
 * voiced with invented words (§6.3's verbatim rule applied to spoken
 * guidance). Every utterance is logged.
 *
 * The *speaker* owns only playback; the contract's dedup, pacing
 * and toggle decisions stay on the React side's speech consumer (the jest-
 * testable decision half): this engine receives one verbatim line per
 * consumer decision and flushes what plays at the speak call (the Compose
 * application's documented pacing — `stop()` drops everything pending,
 * `speak(QUEUE_FLUSH)` replaces whatever plays mid-sentence — the guidance
 * never overlaps or queues far into the future).
 *
 * Engine-initialization failure is an honest disabled-with-reason state
 * (§6.2): the state reports the engine's own failure text through
 * [onSpeechEngineState] and the drawer renders the reason, never silence.
 *
 * Trip speech also holds audio focus ([AudioManager]/[AudioFocusRequest],
 * `AUDIOFOCUS_GAIN_TRANSIENT_MAY_DUCK` — the Android equivalent of the iOS
 * side's duck-able session) acquired at the first utterance of a voice-over,
 * abandoned only after the engine drains, and an honest stop on loss (the
 * trip's guidance is never a zombie voice talking over a call).
 */
class RefappSpeechSpeaker(
    context: Context,
    private val onSpeechEngineState: (isEngineReady: Boolean, unavailableReasonText: String?) -> Unit,
) {

    private val speechEngine: TextToSpeech

    // -------------------------------------------------------------------
    // Engine state (the drawer's §6.2 disabled-with-reason facts)
    // -------------------------------------------------------------------

    /** Whether the platform speech engine initialized successfully. */
    private var isSpeechEngineReady: Boolean = false

    /** The engine's verbatim failure detail when initialization failed (§6.2). */
    private var speechEngineUnavailableReason: String? = null

    /**
     * The engine's current initialization state, read back by the React side
     * once at mount (§6.2): the initialization event can fire before the
     * React event listeners exist, so the honest state must also be readable.
     */
    fun speechEngineState(): Pair<Boolean, String?> =
        Pair(isSpeechEngineReady, speechEngineUnavailableReason)

    // -------------------------------------------------------------------
    // Audio focus (the trip-speech focus)
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
                    // flush pacing carries nothing queued, so the guidance
                    // stops and the next instruction re-speaks — never a
                    // zombie voice over the call.
                    speechEngine.stop()
                    Log.i(
                        SPEECH_LOG_TAG,
                        "audio focus lost ($focusChange): guidance speech stopped",
                    )
                }
                AudioManager.AUDIOFOCUS_LOSS_TRANSIENT_CAN_DUCK -> {
                    // Ducked is the guidance's own granted condition; the
                    // system lowers the volume and nothing stops.
                    Log.i(SPEECH_LOG_TAG, "audio focus ducked: guidance speech continues")
                }
                else -> {
                    Log.i(SPEECH_LOG_TAG, "audio focus change: $focusChange")
                }
            }
        }

    /** Whether the speaker currently holds the trip speech's audio focus. */
    private var isAudioFocusHeld: Boolean = false

    /** The focus request built once (API 26+; the legacy path stands below). */
    private var audioFocusRequest: AudioFocusRequest? = null

    init {
        speechEngine = TextToSpeech(context.applicationContext) { initializationStatus ->
            val succeeded = initializationStatus == TextToSpeech.SUCCESS
            isSpeechEngineReady = succeeded
            if (succeeded) {
                val languageStatus = speechEngine.setLanguage(
                    java.util.Locale.forLanguageTag(RefappConstants.SPEECH_LANGUAGE_TAG),
                )
                Log.i(SPEECH_LOG_TAG, "speech language request: $languageStatus")
            } else {
                // §6.2: an unavailable engine is a state, never a silent
                // fallback. The status code's own value is the verbatim detail.
                speechEngineUnavailableReason =
                    "Android TextToSpeech engine failed to initialize " +
                        "(status $initializationStatus); spoken guidance stays unavailable"
            }
            onSpeechEngineState(isSpeechEngineReady, speechEngineUnavailableReason)
            Log.i(
                SPEECH_LOG_TAG,
                "speech engine init: ready=$succeeded status=$initializationStatus",
            )
        }
        audioManager =
            context.applicationContext.getSystemService(Context.AUDIO_SERVICE) as AudioManager
        speechAudioAttributes = AudioAttributes.Builder()
            .setUsage(AudioAttributes.USAGE_ASSISTANCE_NAVIGATION_GUIDANCE)
            .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
            .build()
        // The drain guard of the focus equivalent: whatever ends an utterance
        // (done, error, flushed, stopped), an idling engine abandons the trip
        // speech's focus — release happens after the queue drains, never
        // during active speech.
        speechEngine.setOnUtteranceProgressListener(
            object : UtteranceProgressListener() {
                override fun onStart(engineUtteranceId: String?) {
                    Log.i(SPEECH_LOG_TAG, "speech engine started utterance: $engineUtteranceId")
                }

                override fun onDone(engineUtteranceId: String?) {
                    abandonAudioFocusWhenEngineIdle("utterance done: $engineUtteranceId")
                }

                override fun onError(engineUtteranceId: String?, errorCode: Int) {
                    abandonAudioFocusWhenEngineIdle("utterance error: $engineUtteranceId ($errorCode)")
                }

                @Deprecated(
                    "Deprecated in Java (the engine dispatches the error-code overload on API 23+)",
                )
                override fun onError(engineUtteranceId: String?) {
                    abandonAudioFocusWhenEngineIdle("utterance error: $engineUtteranceId")
                }

                override fun onStop(engineUtteranceId: String?, interrupted: Boolean) {
                    abandonAudioFocusWhenEngineIdle("utterance stopped: $engineUtteranceId")
                }
            },
        )
    }

    /**
     * Speaks the consumer-voiced line verbatim. `speechText` always carries a
     * line here (the consumer filters empty deliveries); `engineUtteranceId`
     * identifies the emission — the contract's `utterance_id` verbatim when
     * the topic delivery carried one, the consumer's own monotonic counter on
     * the fallback — so the progress callbacks identify the emission they
     * belong to. Pacing (the Compose application's documented fix): the new
     * line cancels the queued and in-flight speech — `stop()` drops
     * everything pending, `speak(QUEUE_FLUSH)` replaces whatever is left
     * mid-sentence, the guidance never overlaps or queues far into the
     * future. A refused engine (never initialized) skips with the log, never
     * a crash.
     */
    fun speak(speechText: String, engineUtteranceId: String) {
        if (!isSpeechEngineReady) {
            Log.i(SPEECH_LOG_TAG, "speech skipped: the engine never initialized")
            return
        }
        Log.i(SPEECH_LOG_TAG, "speech utterance: \"$speechText\" (id $engineUtteranceId)")
        speechEngine.stop()
        requestAudioFocusForSpeech()
        speechEngine.speak(speechText, TextToSpeech.QUEUE_FLUSH, null, engineUtteranceId)
    }

    /** Stops whatever speaks (the drawer's toggle-off path, and focus loss). */
    fun stopSpeech() {
        speechEngine.stop()
    }

    /**
     * Ends the trip's speech lifecycle (the simulated playback's end and the
     * drawer's exit): the focus equivalent abandons when the engine has
     * already drained, and an in-flight utterance keeps its focus until its
     * completion callback abandons after drain — the tail of the last
     * instruction is never cut by the trip ending.
     */
    fun releaseJourneySpeech() {
        if (speechEngine.isSpeaking) {
            Log.i(
                SPEECH_LOG_TAG,
                "journey speech release deferred: utterance in flight, focus abandons after drain",
            )
            return
        }
        abandonAudioFocus("journey speech release")
    }

    // -------------------------------------------------------------------
    // Audio focus (gain at trip speech, abandon after drain)
    // -------------------------------------------------------------------

    /**
     * Acquires the trip speech's audio focus before the guidance's first (or
     * next) utterance voices: a transient, may-duck request built over the
     * navigation-guidance speech attributes — the guidance ducks other audio
     * while it speaks, and an in-flight utterance keeps its focus. A denied
     * request is logged honestly; the guidance still speaks unmanaged rather
     * than silently.
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
        Log.i(
            SPEECH_LOG_TAG,
            "audio focus request: granted=$isAudioFocusHeld (transient may-duck, navigation guidance)",
        )
    }

    /**
     * Abandons the trip speech's audio focus when the engine has drained:
     * fired from every utterance-completion path (done, error, stopped) and
     * never while the engine still speaks — the tail of the last instruction
     * keeps its focus until it finishes.
     */
    private fun abandonAudioFocusWhenEngineIdle(completionReason: String) {
        if (speechEngine.isSpeaking) {
            Log.i(SPEECH_LOG_TAG, "speech still in flight after $completionReason; focus holds until drained")
            return
        }
        abandonAudioFocus("after drain ($completionReason)")
    }

    /**
     * Abandons the trip speech's audio focus when held: the trip's
     * [releaseJourneySpeech] call sites (stop, exit) and the drain paths. A
     * focus never held is a logged no-op.
     */
    private fun abandonAudioFocus(whenReason: String) {
        if (!isAudioFocusHeld) {
            return
        }
        val abandoned = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val request = audioFocusRequest
            if (request == null) {
                Log.i(SPEECH_LOG_TAG, "audio focus abandon skipped: no request exists")
                false
            } else {
                audioManager.abandonAudioFocusRequest(request)
            }
        } else {
            @Suppress("DEPRECATION")
            audioManager.abandonAudioFocus(audioFocusChangeListener)
        }
        isAudioFocusHeld = false
        Log.i(
            SPEECH_LOG_TAG,
            "audio focus abandoned $whenReason (result $abandoned)",
        )
    }

    private companion object {
        /** Logcat tag every utterance log carries (the emulator mandate's evidence line). */
        const val SPEECH_LOG_TAG = "RefappSpeech"
    }
}