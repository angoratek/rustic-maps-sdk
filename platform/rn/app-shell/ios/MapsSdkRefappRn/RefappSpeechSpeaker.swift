//! The app shell's platform voice of the simulated guidance on iOS — the
//! React Native mirror of the Android shell's `RefappSpeechSpeaker.kt` (the
//! mirror of the Compose reference application's `SpeechGuidanceSpeaker`,
//! focus behavior). `AVSpeechSynthesizer` speaks the contract's *speech* text
//! channel **verbatim** — the topic's `utteranceSpeechText`, never
//! paraphrased, prefixed or emptied. One line without speech text is never
//! voiced with invented words (§6.3's verbatim rule applied to spoken
//! guidance). Every utterance is logged.

import AVFoundation
import Foundation
import os

/// The app-owned TTS speaker the React side's speech consumer drives through
/// the bridge module's `speakUtterance`/`stopSpeech` methods (the
/// speech slice's platform half). The *speaker* owns only playback; the
/// contract's dedup, pacing and toggle decisions stay on the
/// React side's speech consumer (the jest-testable decision half): this
/// engine receives one verbatim line per consumer decision and flushes what
/// plays at the speak call (the Compose application's documented pacing —
/// `stopSpeaking(at: .immediate)` drops everything and replaces whatever
/// plays mid-sentence; the guidance never overlaps or queues far into the
/// future).
///
/// Trip speech also activates the audio session (`.playback` with
/// `.duckOthers` — the iOS equivalent of the Android side's transient
/// may-duck focus) at the first utterance of a voice-over, deactivated only
/// after the engine drains (the drain-aware abandon), and an honest stop on
/// session interruption (the trip's guidance is never a zombie voice talking
/// over a call).
final class RefappSpeechSpeaker: NSObject, AVSpeechSynthesizerDelegate {
    private static let speechLogTag = "RefappSpeech"

    private let speechSynthesizer = AVSpeechSynthesizer()
    private let log = Logger(
        subsystem: "com.rusticmaps.sdk.refapprn",
        category: RefappSpeechSpeaker.speechLogTag
    )

    /// The consumer-registered engine-state callback (the drawer's §6.2
    /// disabled-with-reason facts, delivered through the bridge's
    /// `RefappSpeechState` event).
    private let onSpeechEngineState: (Bool, String?) -> Void

    // -------------------------------------------------------------------
    // Engine state (the drawer's §6.2 disabled-with-reason facts)
    // -------------------------------------------------------------------

    /// Whether the platform speech engine is usable (the language voice
    /// resolved).
    private(set) var isSpeechEngineReady = false

    /// The engine's verbatim failure detail when the voice lookup failed (§6.2).
    private(set) var speechEngineUnavailableReason: String?

    /// Whether the speaker currently holds the activated trip-speech session.
    private var isAudioSessionActive = false
    /// The contract's `utterance_id` of the line currently voiced (the
    /// engine callbacks log it; AVSpeechUtterance carries no identifier).
    private var inFlightUtteranceId: String?

    init(onSpeechEngineState: @escaping (Bool, String?) -> Void) {
        self.onSpeechEngineState = onSpeechEngineState
        super.init()
        speechSynthesizer.delegate = self
        // The iOS engine's readiness check: the demo's language voice must
        // exist (§6.2: an unavailable engine is a state, never a silent
        // fallback — the failure's own text is the verbatim detail).
        let languageVoice = AVSpeechSynthesisVoice(language: RefappConstants.speechLanguageTag)
        if let languageVoice = languageVoice {
            isSpeechEngineReady = true
            speechEngineUnavailableReason = nil
            log.info("speech voice ready (\(languageVoice.name, privacy: .public))")
        } else {
            isSpeechEngineReady = false
            speechEngineUnavailableReason =
                "iOS AVSpeechSynthesizer found no voice for \(RefappConstants.speechLanguageTag); spoken guidance stays unavailable"
        }
        log.info("speech engine init: ready=\(self.isSpeechEngineReady)")
        onSpeechEngineState(isSpeechEngineReady, speechEngineUnavailableReason)
    }

    /// The engine's current state, read back by the React side once at mount
    /// (§6.2): the initialization event can fire before the React event
    /// listeners exist, so the honest state must also be readable.
    func speechEngineState() -> (isEngineReady: Bool, unavailableReasonText: String?) {
        (isSpeechEngineReady, speechEngineUnavailableReason)
    }

    /// Speaks the consumer-voiced line verbatim. `speechText` always carries a
    /// line here (the consumer filters empty deliveries); `engineUtteranceId`
    /// identifies the emission (the contract's `utterance_id` verbatim when
    /// the topic delivery carried one, the consumer's own monotonic counter
    /// on the fallback). Pacing (the Compose application's documented fix):
    /// the new line cancels the queued and in-flight speech —
    /// `stopSpeaking(at: .immediate)` drops everything and the new
    /// utterance replaces whatever is left mid-sentence, the guidance never
    /// overlaps or queues far into the future. A refused engine (never
    /// initialized) skips with the log, never a crash.
    func speak(speechText: String, engineUtteranceId: String) {
        if !isSpeechEngineReady {
            log.info("speech skipped: the engine never initialized")
            return
        }
        log.info("speech utterance: \"\(speechText, privacy: .public)\" (id \(engineUtteranceId, privacy: .public))")
        activateAudioSessionForSpeech()
        let utterance = AVSpeechUtterance(string: speechText)
        utterance.voice = AVSpeechSynthesisVoice(language: RefappConstants.speechLanguageTag)
        // AVSpeechUtterance carries no identifier; the in-flight id is kept
        // on the speaker so the engine callbacks log the contract's
        // `utterance_id` verbatim (the drain tracking's same reference).
        inFlightUtteranceId = engineUtteranceId
        speechSynthesizer.stopSpeaking(at: .immediate)
        speechSynthesizer.speak(utterance)
    }

    /// Stops whatever speaks (the drawer's toggle-off path, and interruption).
    func stopSpeech() {
        speechSynthesizer.stopSpeaking(at: .immediate)
        log.info("speech stopped")
    }

    /// Ends the trip's speech lifecycle (the simulated playback's end and the
    /// drawer's exit): the session deactivates when the engine has already
    /// drained, and an in-flight utterance keeps the session until its
    /// completion callback deactivates after drain — the tail of the last
    /// instruction is never cut by the trip ending.
    func releaseJourneySpeech() {
        if speechSynthesizer.isSpeaking {
            log.info("journey speech release deferred: utterance in flight, session deactivates after drain")
            return
        }
        abandonAudioSession("journey speech release")
    }

    // -------------------------------------------------------------------
    // Audio session (gain at trip speech, abandon after drain)
    // -------------------------------------------------------------------

    /// Activates the trip speech's audio session before the guidance's first
    /// (or next) utterance voices: `.playback` with `.duckOthers` — the
    /// guidance ducks other audio while it speaks, and an in-flight
    /// utterance keeps the session. A failed activation is logged honestly;
    /// the guidance still speaks unmanaged rather than silently.
    private func activateAudioSessionForSpeech() {
        if isAudioSessionActive {
            return
        }
        let audioSession = AVAudioSession.sharedInstance()
        do {
            try audioSession.setCategory(
                .playback,
                mode: .spokenAudio,
                options: [.duckOthers]
            )
            try audioSession.setActive(true)
            isAudioSessionActive = true
            log.info("audio session activated (playback duck-others, spoken guidance)")
        } catch {
            isAudioSessionActive = false
            log.info("audio session activation failed; guidance speaks unmanaged (\(error.localizedDescription, privacy: .public))")
        }
    }

    /// Deactivates the trip speech's audio session when held (the trip's
    /// `releaseJourneySpeech` call sites and the drain paths). A session
    /// never activated is a logged no-op.
    private func abandonAudioSession(_ whenReason: String) {
        if !isAudioSessionActive {
            return
        }
        isAudioSessionActive = false
        do {
            try AVAudioSession.sharedInstance().setActive(
                false,
                options: .notifyOthersOnDeactivation
            )
            log.info("audio session abandoned \(whenReason, privacy: .public)")
        } catch {
            log.info("audio session deactivation failed \(whenReason, privacy: .public) (\(error.localizedDescription, privacy: .public))")
        }
    }

    // -------------------------------------------------------------------
    // The synthesizer delegate (the engine's progress callbacks; the drain
    // guard of the session: whatever ends an utterance — finished, canceled,
    // paused/continued never fire here — an idling engine abandons the trip
    // speech's session, never during active speech)
    // -------------------------------------------------------------------

    func speechSynthesizer(_ synthesizer: AVSpeechSynthesizer, didStart utterance: AVSpeechUtterance) {
        log.info("speech engine started utterance: \(self.inFlightUtteranceId ?? "-", privacy: .public)")
    }

    func speechSynthesizer(_ synthesizer: AVSpeechSynthesizer, didFinish utterance: AVSpeechUtterance) {
        abandonAudioSessionWhenEngineIdle("utterance done: \(inFlightUtteranceId ?? "-")")
    }

    func speechSynthesizer(_ synthesizer: AVSpeechSynthesizer, didCancel utterance: AVSpeechUtterance) {
        abandonAudioSessionWhenEngineIdle("utterance canceled: \(inFlightUtteranceId ?? "-")")
    }

    /// The drain-aware abandon: the session deactivates only when the engine
    /// has no queued or in-flight work left (the tail of the last
    /// instruction keeps the session until it finishes).
    private func abandonAudioSessionWhenEngineIdle(_ completionReason: String) {
        if speechSynthesizer.isSpeaking {
            log.info("speech still in flight after \(completionReason, privacy: .public); session holds until drained")
            return
        }
        abandonAudioSession("after drain (\(completionReason))")
    }
}