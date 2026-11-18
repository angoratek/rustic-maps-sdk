//! The simulated navigation's platform voice (docs/ReferenceAppUX.md §4.1,
//! §4.2): the iOS `AVSpeechSynthesizer` speaking the contract's speech
//! text channel. What is spoken comes verbatim from the payload — the
//! speech text is never paraphrased, translated, prefixed or rephrased
//! here (§6.3's verbatim-echo rule applied to spoken guidance). The voice,
//! its queue/interrupt behavior and the speaking state are platform-owned.

import AVFoundation
import Combine
import Foundation
import RusticMapsSDK
import UIKit
import os

/// The platform voice of the simulated navigation.
///
/// Queue/interrupt behavior (the platform-owned policy of §4.1): the
/// synthesizer never carries a
/// queue — every new line first flushes whatever waits or speaks
/// (`stop(at: .immediate)`, no stale utterances far ahead). A *new
/// maneuver* (the stable unprefixed speech text changed) interrupts a
/// line in flight — the driver must always hear the most current
/// maneuver. A *distance-only refresh* (same maneuver, only the live
/// distance prefix moved) is never spoken over the line in flight: while
/// an utterance speaks it is dropped (no queueing — the next fix's
/// payload is fresher anyway), and only an idle queue gets the refresh.
/// An unchanged line is never repeated (the payloads re-broadcast on
/// every fix while the driver sits inside one step, and repeating the
/// same line per fix would chatter). While the guidance is frozen off
/// route, the payload marks it and the replay loop never hands the
/// frozen projection's speech text over (the caller's document, not this
/// engine's).
@MainActor
final class NavigationSpeechEngine: NSObject, ObservableObject {
    /// The synthesizer backing the voice.
    private let synthesizer = AVSpeechSynthesizer()

    /// Whether the voice is enabled (the drawer's speech toggle); turning
    /// it off mid-utterance stops the voice immediately, turning it on
    /// takes effect at the next instruction payload.
    var isVoiceEnabled = true

    // MARK: Audio session

    /// Whether the engine's AVAudioSession is configured and activated
    /// (the focus-equivalent for the trip's spoken guidance; activated at
    /// the first utterance of a guidance run, released only after drain).
    private var isAudioSessionActivated = false

    /// The pending post-drain session-release polling task; canceled and
    /// replaced whenever the synthesizer starts another utterance before
    /// the poll's window elapses.
    private var audioSessionReleaseTask: Task<Void, Never>?

    /// The seconds the release poll waits before it decides the queue has
    /// drained: the ~500 ms poll pattern — short enough
    /// that the session releases promptly after the last line, long enough
    /// that the tail of a finishing utterance is never cut by a premature
    /// deactivation.
    private static let audioSessionDrainPollSeconds: TimeInterval = 0.5

    /// The session category the trip's spoken guidance runs under.
    private static let speechAudioSessionCategory = AVAudioSession.Category.playback

    /// The session mode for spoken navigation guidance (a spoken-audio
    /// mode lets the system treat the guidance as speech, the audio-focus
    /// equivalent of the contract's duck-able transient request).
    private static let speechAudioSessionMode = AVAudioSession.Mode.spokenAudio

    /// The category option that ducks other audio for the guidance's
    /// duration (the iOS side's `AUDIOFOCUS_GAIN_TRANSIENT_MAY_DUCK`
    /// counterpart; category options, not activation options — ducking on
    /// iOS is a property of the category the guidance activates under).
    private static let speechAudioSessionCategoryOptions: AVAudioSession.CategoryOptions = [.duckOthers]

    /// The deactivation option that notifies the other audio the guidance
    /// ducked so it can resume un-ducked.
    private static let speechAudioSessionDeactivationOptions: AVAudioSession.SetActiveOptions = [.notifyOthersOnDeactivation]

    /// The utterance identity (the contract's `utterance_id`) of
    /// the line last handed to the synthesizer — the delivery dedup key:
    /// the contract's fire-and-forget speech topic must never re-voice an
    /// emission it already delivered (same id twice is a re-delivery, the
    /// seam's documented robustness case).
    private var lastSpokenUtteranceId: UInt64?

    /// The speech text currently (or last) spoken — the log/echo line the
    /// evidence pass reads (the voice cannot be verified audibly on a
    /// simulator: the honest check is the synthesizer's own state).
    @Published private(set) var lastSpokenText: String?

    /// The stable (unprefixed) speech text of the maneuver last spoken —
    /// the interrupt key: a change means a new maneuver (flush + speak),
    /// sameness means a distance-only refresh (speak only when idle).
    private var lastSpokenStepSpeechText: String?

    /// Whether the synthesizer reports an utterance in flight, captured at
    /// the moment utterances start (the honest verification the simulator
    /// evidence pass asserts).
    @Published private(set) var isSynthesizerSpeaking = false

    /// The number of speech requests the engine has handed to the
    /// synthesizer this session (the evidence pass's exercise counter).
    @Published private(set) var spokenUtteranceCount = 0

    /// The logger the spoken text goes to (the console the evidence pass
    /// reads).
    private let speechLog = Logger(subsystem: "com.rusticmaps.sdk.referenceapplication",
                                   category: "navigation-speech")

    override init() {
        super.init()
        synthesizer.delegate = self
    }

    /// The speech id of the emission last delivered (the contract's
    /// `utterance_id`): a
    /// re-delivery of the same id is skipped, an identical text re-emitted
    /// is a NEW id and follows the platform's own pacing policy below.
    /// `stableStepSpeechText` is the payload's unprefixed speech variant
    /// (the maneuver's identity): a change
    /// flushes the queue and interrupts a line in flight (a newer
    /// maneuver outranks the lagging utterance); with the same maneuver,
    /// only a distance-prefix refresh arrived, which never queues behind
    /// — it is dropped while the synthesizer speaks and spoken once the
    /// queue is idle. Never repeats the line that was just spoken.
    func speakGuidance(
        speechTextVerbatim: String,
        stableStepSpeechText: String?,
        utteranceId: UInt64
    ) {
        guard isVoiceEnabled else { return }
        guard utteranceId != lastSpokenUtteranceId else {
            // B4's platform dedup: the same emission delivered twice (the
            // speech topic is fire-and-forget; the platform speaker owns
            // the flush/dedup decision) is voiced once, never twice.
            speechLog.info(
                "skipped re-delivered utterance id \(utteranceId): \"\(speechTextVerbatim, privacy: .public)\""
            )
            return
        }
        guard speechTextVerbatim != lastSpokenText else {
            // The same line re-arrives per fix while the driver sits
            // inside one step; the policy repeats nothing.
            return
        }
        if stableStepSpeechText == lastSpokenStepSpeechText,
           synthesizer.isSpeaking || synthesizer.isPaused {
            // Distance-only refresh over a line in flight: dropped, not
            // queued (the engine never carries a stale queue, §4.1's
            // pacing policy).
            speechLog.info(
                "dropped distance-only refresh while speaking: \"\(speechTextVerbatim, privacy: .public)\""
            )
            return
        }
        stopVoice()
        activateAudioSessionForSpeech()
        let guidanceUtterance = AVSpeechUtterance(string: speechTextVerbatim)
        guidanceUtterance.voice = navigationEnglishVoice()
        synthesizer.speak(guidanceUtterance)
        lastSpokenText = speechTextVerbatim
        lastSpokenStepSpeechText = stableStepSpeechText
        lastSpokenUtteranceId = utteranceId
        spokenUtteranceCount += 1
        speechLog.info(
            "spoken verbatim (utterance id \(utteranceId)): \"\(speechTextVerbatim, privacy: .public)\""
        )
    }

    // MARK: Audio session focus equivalent

    /// Configures and activates the guidance's AVAudioSession once (the
    /// focus-equivalent of the trip's spoken guidance): playback under the
    /// spoken-audio mode, ducking other audio for the guidance's duration.
    /// A failed activation is logged honestly and never voiced as success
    /// — the synthesizer still speaks through whatever session the system
    /// left, the platform policy releases only the state it owns.
    private func activateAudioSessionForSpeech() {
        // Any release poll parked before this line is stale: the guidance
        // took the session back.
        audioSessionReleaseTask?.cancel()
        audioSessionReleaseTask = nil
        guard !isAudioSessionActivated else { return }
        let audioSession = AVAudioSession.sharedInstance()
        do {
            try audioSession.setCategory(
                Self.speechAudioSessionCategory,
                mode: Self.speechAudioSessionMode,
                options: Self.speechAudioSessionCategoryOptions
            )
            try audioSession.setActive(true)
            isAudioSessionActivated = true
            speechLog.info("audio session activated (playback, spoken audio, duck others)")
        } catch {
            speechLog.error(
                "audio session activation failed: \(String(describing: error), privacy: .public)"
            )
        }
    }

    /// Releases the activated audio session only after the queue drains
    /// (the B4 plan's poll pattern, never during active I/O): the poll
    /// fires `audioSessionDrainPollSeconds` after the last utterance
    /// ended, re-checks the synthesizer (a line started within the window
    /// keeps the session and re-schedules at its own finish), and only an
    /// idle synthesizer deactivates — so the tail of the last instruction
    /// is never cut before it finishes.
    func releaseAudioSessionAfterDrain() {
        guard isAudioSessionActivated else { return }
        audioSessionReleaseTask?.cancel()
        audioSessionReleaseTask = Task { @MainActor [weak self] in
            try? await Task.sleep(for: .seconds(Self.audioSessionDrainPollSeconds))
            guard let self, !(Task.isCancelled) else { return }
            guard !(self.synthesizer.isSpeaking || self.synthesizer.isPaused) else {
                // A line is in flight again; the poll never deactivates
                // during active I/O — its finish re-schedules the release.
                return
            }
            self.deactivateAudioSession()
        }
    }

    /// Deactivates the session (notifying the ducked other audio) and
    /// clears the activation state.
    private func deactivateAudioSession() {
        audioSessionReleaseTask = nil
        guard isAudioSessionActivated else { return }
        do {
            try AVAudioSession.sharedInstance().setActive(
                false,
                options: Self.speechAudioSessionDeactivationOptions
            )
            speechLog.info("audio session released after drain")
        } catch {
            speechLog.error(
                "audio session release failed: \(String(describing: error), privacy: .public)"
            )
        }
        isAudioSessionActivated = false
    }

    /// The voice the guidance speaks with: an English voice the demo's
    /// provider-authored speech text is written in, chosen by the
    /// deterministic search order (never a hardcoded language code guess).
    private func navigationEnglishVoice() -> AVSpeechSynthesisVoice? {
        AVSpeechSynthesisVoice.speechVoices()
            .first(where: { $0.language == englishSpeechLanguage })
    }

    /// The speech language tag the demo's guidance text is authored in.
    private let englishSpeechLanguage = "en-US"

    /// Stops the voice immediately (the speech toggle turned off, or a
    /// newer line interrupting). When nothing stays in flight after the
    /// stop, the audio session's release schedules after drain (the
    /// toggle-off path releases the ducking like the journey's end does;
    /// a newer line re-cancels the release and keeps the session).
    func stopVoice() {
        if synthesizer.isSpeaking || synthesizer.isPaused {
            synthesizer.stopSpeaking(at: .immediate)
        }
        isSynthesizerSpeaking = synthesizer.isSpeaking
        if !(synthesizer.isSpeaking || synthesizer.isPaused) {
            releaseAudioSessionAfterDrain()
        }
    }
}

extension NavigationSpeechEngine: AVSpeechSynthesizerDelegate {
    /// The synthesizer's state, mirrored into the published flag the
    /// evidence pass asserts (no audio assertion is possible on a
    /// simulator).
    nonisolated func speechSynthesizer(
        _ synthesizer: AVSpeechSynthesizer,
        didStart utterance: AVSpeechUtterance
    ) {
        Task { @MainActor in
            self.isSynthesizerSpeaking = synthesizer.isSpeaking
            // The evidence pass's honest assertion: the engine's state
            // flag mirrors the synthesizer actually in flight (audio
            // itself cannot be asserted on a simulator).
            speechLog.info(
                "synthesizer didStart speaking (isSpeaking \(synthesizer.isSpeaking))"
            )
        }
    }

    nonisolated func speechSynthesizer(
        _ synthesizer: AVSpeechSynthesizer,
        didFinish utterance: AVSpeechUtterance
    ) {
        Task { @MainActor in
            self.isSynthesizerSpeaking = synthesizer.isSpeaking
            // The line finished: the audio session's release waits for
            // the drain window (a newer line started within it keeps
            // the session).
            self.releaseAudioSessionAfterDrain()
        }
    }

    nonisolated func speechSynthesizer(
        _ synthesizer: AVSpeechSynthesizer,
        didCancel utterance: AVSpeechUtterance
    ) {
        Task { @MainActor in
            self.isSynthesizerSpeaking = synthesizer.isSpeaking
            self.releaseAudioSessionAfterDrain()
        }
    }
}