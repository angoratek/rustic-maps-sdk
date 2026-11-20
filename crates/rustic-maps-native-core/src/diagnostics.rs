//! The diagnostics sink: the `SdkDiagnosticEventListener`
//! foreign-callback contract foreign code implements to receive the SDK's
//! structured internal log events, plus the ring-buffer `tracing` subscriber
//! and the sink's privacy scrubbing behind
//! `MapSdk::subscribe_to_diagnostic_events`,
//! `MapSdk::subscribe_to_diagnostic_events_with_subscription` and
//! `MapSdk::flush_recent_diagnostic_events`.
//!
//! # Architecture
//!
//! The SDK crates emit plain `tracing` events under per-subsystem targets
//! (`tile_fetch`, `renderer`, `navigation`, `binding_camera`, `cache`);
//! [`DiagnosticsHandle::shared`] is the only writer into the bounded rings —
//! a [`DiagnosticsSubscriber`] installed as the process's global subscriber
//! at the first `MapSdk` construction — and the FFI seams are two read-only
//! views of it. Delivery is fire-and-forget: an event is copied into the
//! ring and to any live listener and the emitting thread moves on
//! immediately, so a host's slow listener never slows the code whose events
//! it observes.
//!
//! The state is process-wide because a `tracing` global subscriber is: a
//! second `MapSdk` in the same process shares the single rings-and-listeners
//! state — the documented, honest shape of a process-wide logging facade. A
//! host that already installed its own global subscriber keeps it (the host
//! then owns the process's logging policy, and the rings and the live
//! stream stay empty — the flush seam answers an empty list).
//!
//! # The severity gate
//!
//! The rings record at [`DIAGNOSTIC_IDLE_RECORDING_SEVERITY`] or above while
//! no fine-grained subscription is live. Trace- and debug-severity events —
//! the GPU-hot render path's instrumentation — are recorded and dispatched
//! only once a subscription requested them: the subscriber's `enabled`
//! check consults the gate before any argument formatting runs, so an idle
//! foreign listener leaves the frame path at near-zero logging cost. The
//! gate never raises back: once fine-grained events have been subscribed,
//! they keep recording while the process lives (the rings are bounded).
//!
//! # Privacy (scrubbing at the sink)
//!
//! A structured event's `latitude` and `longitude` fields are rounded to
//! [`DIAGNOSTIC_COORDINATE_DECIMALS`] decimals when the line is composed
//! (≈ 110 m at the equator, a coarse privacy granularity), in exactly one
//! place — the subscriber's event visitor — regardless of which crate emitted.
//! Call sites never embed raw coordinates in free-text messages, and the
//! API key never appears in any event.

use std::cell::Cell;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use web_time::{SystemTime, UNIX_EPOCH};

use tracing::{Event, Id, Level, Metadata, Subscriber, field::Field, field::Visit};

use crate::records::{LogEvent, ProviderRequestTrace, SdkLogSeverity};

/// The capacity of each subsystem's diagnostics ring, in events (bounded so
/// a chatty subsystem cannot grow the process's memory unboundedly).
pub const DIAGNOSTIC_RING_CAPACITY: usize = 512;

/// Decimals coordinate values are rounded to at the sink for coarse privacy
/// granularity (≈ 110 m at the equator).
pub const DIAGNOSTIC_COORDINATE_DECIMALS: usize = 3;

/// The severity the rings record down to while no fine-grained subscription
/// is live: only the coarse (informational and above) events pass, keeping
/// the GPU-hot frame path's trace instrumentation off when nothing listens.
pub const DIAGNOSTIC_IDLE_RECORDING_SEVERITY: SdkLogSeverity = SdkLogSeverity::Info;

/// The named severity ranks (higher is more severe; the intuitive — not the
/// `tracing` — direction). Only relative order is meaningful.
pub(crate) const TRACE_SEVERITY_RANK: u8 = 0;
pub(crate) const DEBUG_SEVERITY_RANK: u8 = 1;
pub(crate) const INFO_SEVERITY_RANK: u8 = 2;
pub(crate) const WARN_SEVERITY_RANK: u8 = 3;
pub(crate) const ERROR_SEVERITY_RANK: u8 = 4;

/// The numeric severity rank of an `SdkLogSeverity` (higher is more severe).
pub(crate) const fn severity_rank(severity: SdkLogSeverity) -> u8 {
    match severity {
        SdkLogSeverity::Trace => TRACE_SEVERITY_RANK,
        SdkLogSeverity::Debug => DEBUG_SEVERITY_RANK,
        SdkLogSeverity::Info => INFO_SEVERITY_RANK,
        SdkLogSeverity::Warn => WARN_SEVERITY_RANK,
        SdkLogSeverity::Error => ERROR_SEVERITY_RANK,
    }
}

/// The numeric severity rank of a `tracing` level, in `severity_rank`'s
/// ordering (both severity ladders ascend from trace).
pub(crate) fn severity_rank_for_tracing_level(level: &Level) -> u8 {
    match level.as_str() {
        "TRACE" => TRACE_SEVERITY_RANK,
        "DEBUG" => DEBUG_SEVERITY_RANK,
        "INFO" => INFO_SEVERITY_RANK,
        "WARN" => WARN_SEVERITY_RANK,
        _ => ERROR_SEVERITY_RANK,
    }
}

/// The gate's idle encoding: only events at
/// [`DIAGNOSTIC_IDLE_RECORDING_SEVERITY`] or above (in `severity_rank`'s
/// ordering) are recorded.
const GATE_DEFAULT_RANK: u8 = severity_rank(DIAGNOSTIC_IDLE_RECORDING_SEVERITY);

/// The module-path prefixes of the SDK crates whose events the sink records.
/// Foreign (application) code's events never enter the rings or the live
/// stream — the sink observes the SDK, not its host.
const SDK_MODULE_PATH_PREFIXES: [&str; 4] = [
    "rustic_maps_rendering",
    "rustic_maps_navigation",
    "rustic_maps",
    "rustic_maps_cache",
];

/// The structured event fields carrying coordinates, which the sink's
/// visitor scrubs to [`DIAGNOSTIC_COORDINATE_DECIMALS`] decimals.
const COORDINATE_FIELD_NAMES: [&str; 2] = ["latitude", "longitude"];

/// A placeholder replacing an un-parseable coordinate field value at the
/// sink (the field's raw content never reaches the composed line).
const SCRUBBED_FIELD_PLACEHOLDER: &str = "scrubbed";

/// Whether an event with this module path (or tracing target) originates
/// from a SDK crate — the sink observes the SDK, not its host.
pub(crate) fn is_sdk_module_path(module_path_or_target: &str) -> bool {
    SDK_MODULE_PATH_PREFIXES
        .iter()
        .any(|prefix| module_path_or_target.starts_with(prefix))
}

// Re-entrancy state of the current thread's event recording: a listener
// callback emitting its own `tracing` events must not recurse into the
// sink (the listener's events are the host's, not the SDK's to record).
thread_local! {
    static EVENT_RECORDING_IN_FLIGHT: Cell<bool> = const { Cell::new(false) };
}

/// Marks the current thread as inside a listener dispatch for its lifetime;
/// while marked, the subscriber drops events (which stops the recursion a
/// listener-emitted event would otherwise cause).
struct EventRecordingMarker {
    _no_construction_without_enter: (),
}

impl EventRecordingMarker {
    /// Enters the marked state unless a marker is already live on this
    /// thread (re-entrancy), in which case `None` says "drop this event".
    fn enter() -> Option<Self> {
        EVENT_RECORDING_IN_FLIGHT.with(|in_flight| {
            if in_flight.get() {
                None
            } else {
                in_flight.set(true);
                Some(Self {
                    _no_construction_without_enter: (),
                })
            }
        })
    }
}

impl Drop for EventRecordingMarker {
    fn drop(&mut self) {
        EVENT_RECORDING_IN_FLIGHT.with(|in_flight| in_flight.set(false));
    }
}

/// One ring entry: the sink's already-scrubbed event (the subsystem target
/// is the ring's map key).
struct RecordedEvent {
    timestamp_milliseconds_since_epoch: u64,
    severity: SdkLogSeverity,
    /// Process-wide sequence, ordering events recorded within the same
    /// millisecond.
    sequence: u64,
    message_line: String,
    /// The typed request facts for a completed provider call.
    provider_request_trace: Option<ProviderRequestTrace>,
}

/// One live foreign diagnostics listener registration (the registration a
/// `MapSdk::subscribe_to_diagnostic_events` or
/// `MapSdk::subscribe_to_diagnostic_events_with_subscription` call created).
struct DiagnosticsListenerRegistration {
    listener: Arc<dyn SdkDiagnosticEventListener>,
    minimum_severity: SdkLogSeverity,
    /// The process-wide id of this registration; the handle a
    /// `subscribe_to_diagnostic_events_with_subscription` call answers owns it.
    registration_id: u64,
}

/// The process-wide diagnostics state: the per-subsystem bounded rings, the
/// live subscription list, and the severity gate, behind the single
/// `tracing` subscriber. Constructed through [`DiagnosticsHandle::shared`]
/// (the FFI objects take the leaked handle).
pub struct DiagnosticsHandle {
    rings: Mutex<HashMap<String, VecDeque<RecordedEvent>>>,
    gate: AtomicU8,
    listeners: Mutex<Vec<DiagnosticsListenerRegistration>>,
    ring_sequence: AtomicU64,
    /// The next registration id a subscription handle owns; process-wide, so
    /// a handle can only ever remove the registration it was created for.
    next_registration_id: AtomicU64,
}

impl DiagnosticsHandle {
    /// The process-wide state, installing the global subscriber on first
    /// use. A host-installed global subscriber keeps precedence (documented
    /// in the module head): the rings and the live stream then stay empty
    /// and the flush seam answers an empty list.
    pub(crate) fn shared() -> &'static DiagnosticsHandle {
        static SHARED_HANDLE: OnceLock<&'static DiagnosticsHandle> = OnceLock::new();
        SHARED_HANDLE.get_or_init(|| {
            let handle: &'static DiagnosticsHandle = Box::leak(Box::new(DiagnosticsHandle {
                rings: Mutex::new(HashMap::new()),
                gate: AtomicU8::new(GATE_DEFAULT_RANK),
                listeners: Mutex::new(Vec::new()),
                ring_sequence: AtomicU64::new(1),
                next_registration_id: AtomicU64::new(1),
            }));
            let _ = tracing::subscriber::set_global_default(DiagnosticsSubscriber {
                handle,
                next_span_id: AtomicU64::new(1),
            });
            handle
        })
    }

    /// Subscribes a listener to the live event stream and answers the new
    /// registration's process-wide id; also lowers the recording gate to the
    /// subscription's minimum severity (the gate never raises back, so a later
    /// coarser subscription keeps recording the finer-grained events).
    pub(crate) fn subscribe(
        &'static self,
        listener: Arc<dyn SdkDiagnosticEventListener>,
        minimum_severity: SdkLogSeverity,
    ) -> u64 {
        let registration_id = self.next_registration_id.fetch_add(1, Ordering::Relaxed);
        crate::panic_boundary::state_lock_guard(
            &self.listeners,
            "the diagnostics listener registry",
        )
        .push(DiagnosticsListenerRegistration {
            listener,
            minimum_severity,
            registration_id,
        });
        self.lower_gate_to(severity_rank(minimum_severity));
        registration_id
    }

    /// Removes the one registration a subscription handle owns, answering
    /// whether it was still live. An unknown or already closed id is a no-op
    /// (`false`), so closing a subscription twice is safe. The severity gate
    /// is left where the subscription lowered it; the gate never raises back.
    pub(crate) fn close_subscription(&self, registration_id: u64) -> bool {
        let mut listeners = crate::panic_boundary::state_lock_guard(
            &self.listeners,
            "the diagnostics listener registry",
        );
        let registrations_before = listeners.len();
        listeners.retain(|registration| registration.registration_id != registration_id);
        listeners.len() != registrations_before
    }

    /// Lowers the gate to `rank` if it is finer than the current gate (the
    /// gate never raises back once fine-grained events have been requested).
    fn lower_gate_to(&self, rank: u8) {
        if rank < self.gate.load(Ordering::Relaxed) {
            self.gate.store(rank, Ordering::Relaxed);
        }
    }

    /// Whether the sink records an event of `severity_rank_value` emitted
    /// from `module_path_or_target`: the event must come from an SDK crate's
    /// module path (not the host's) and pass the severity gate. The
    /// subscriber's `enabled` consults exactly this, so no argument
    /// formatting happens for a rejected event.
    pub(crate) fn should_record(
        &self,
        module_path_or_target: &str,
        severity_rank_value: u8,
    ) -> bool {
        is_sdk_module_path(module_path_or_target)
            && severity_rank_value >= self.gate.load(Ordering::Relaxed)
    }

    /// Reads the most recent events across every subsystem's ring, oldest
    /// first, filtered by `minimum_severity` and capped at `maximum_count`
    /// (the flush seam; the rings are never emptied, so a later flush can
    /// redeliver).
    pub(crate) fn recent_events(
        &self,
        minimum_severity: SdkLogSeverity,
        maximum_count: u32,
    ) -> Vec<LogEvent> {
        let minimum_rank = severity_rank(minimum_severity);
        let mut matched_events: Vec<(
            String,
            u64,
            SdkLogSeverity,
            u64,
            String,
            Option<ProviderRequestTrace>,
        )> = {
            let rings =
                crate::panic_boundary::state_lock_guard(&self.rings, "the diagnostics event rings");
            rings
                .iter()
                .flat_map(|(subsystem_target, ring)| {
                    ring.iter()
                        .filter(|event| severity_rank(event.severity) >= minimum_rank)
                        .map(|event| {
                            (
                                subsystem_target.clone(),
                                event.timestamp_milliseconds_since_epoch,
                                event.severity,
                                event.sequence,
                                event.message_line.clone(),
                                event.provider_request_trace.clone(),
                            )
                        })
                })
                .collect()
        };
        // Oldest first across subsystems, the process-wide sequence breaking
        // same-millisecond ties; the newest `maximum_count` events are the
        // requested prefix.
        matched_events
            .sort_unstable_by_key(|(_, timestamp, _, sequence, _, _)| (*timestamp, *sequence));
        matched_events
            .into_iter()
            .rev()
            .take(maximum_count as usize)
            .rev()
            .map(
                |(
                    subsystem_target,
                    timestamp_milliseconds_since_epoch,
                    severity,
                    _,
                    message_line,
                    provider_request_trace,
                )| {
                    LogEvent {
                        timestamp_milliseconds_since_epoch,
                        severity,
                        subsystem_target,
                        message_line,
                        provider_request_trace,
                    }
                },
            )
            .collect()
    }

    /// Records one scrubbed event into the named subsystem's ring (ring
    /// retention: at [`DIAGNOSTIC_RING_CAPACITY`] the oldest event drops).
    fn record(&self, subsystem_target: &str, event: RecordedEvent) {
        let mut rings =
            crate::panic_boundary::state_lock_guard(&self.rings, "the diagnostics event rings");
        let ring = rings
            .entry(subsystem_target.to_string())
            .or_insert_with(|| VecDeque::with_capacity(DIAGNOSTIC_RING_CAPACITY));
        if ring.len() >= DIAGNOSTIC_RING_CAPACITY {
            ring.pop_front();
        }
        ring.push_back(event);
    }

    /// Dispatches a scrubbed event to every live listener whose minimum
    /// severity it meets: listener handles are cloned under the listeners
    /// lock and the callbacks run fire-and-forget, with no lock held across
    /// a callback and the thread's re-entrancy flag dropping listener-emitted
    /// events.
    fn dispatch_live(&self, event: &LogEvent) {
        let Some(_recording_marker) = EventRecordingMarker::enter() else {
            return;
        };
        let minimum_rank = severity_rank(event.severity);
        let matched_listeners: Vec<Arc<dyn SdkDiagnosticEventListener>> = {
            let listeners = crate::panic_boundary::state_lock_guard(
                &self.listeners,
                "the diagnostics listener registry",
            );
            listeners
                .iter()
                .filter(|registration| severity_rank(registration.minimum_severity) <= minimum_rank)
                .map(|registration| Arc::clone(&registration.listener))
                .collect()
        };
        // No lock is held across the callbacks; the callbacks are
        // fire-and-forget (a callback's failure cannot fail the caller).
        for listener in matched_listeners {
            listener.on_diagnostic_event(event.clone());
        }
    }
}

/// The `tracing` subscriber that is the diagnostics sink: SDK-crate events
/// passing the severity gate are scrubbed, ring-recorded and fire-and-forget
/// dispatched. Spans are not supported (the sink records flat events); a
/// span's `new_span` hands out a unique id and span bookkeeping is ignored,
/// so events carry all of their structure themselves.
struct DiagnosticsSubscriber {
    handle: &'static DiagnosticsHandle,
    next_span_id: AtomicU64,
}

impl Subscriber for DiagnosticsSubscriber {
    /// The gate check — called by `tracing` before any event argument is
    /// formatted, which is what keeps the GPU-hot render path at near-zero
    /// logging cost while the gate is closed to a coarse severity.
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        let severity_rank_value = severity_rank_for_tracing_level(metadata.level());
        self.handle.should_record(
            metadata.module_path().unwrap_or(metadata.target()),
            severity_rank_value,
        )
    }

    fn new_span(&self, _attributes: &tracing::span::Attributes<'_>) -> Id {
        // The sink supports no span structure; ids exist to satisfy the
        // subscriber contract and are never dereferenced.
        Id::from_u64(self.next_span_id.fetch_add(1, Ordering::Relaxed))
    }

    fn record(&self, _span: &Id, _values: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, event: &Event<'_>) {
        if !self.enabled(event.metadata()) {
            return;
        }
        let mut visitor = EventLineVisitor::default();
        event.record(&mut visitor);
        let EventLineVisitorCollected {
            message,
            latitude,
            longitude,
            structured_field_pairs,
        } = visitor.into_collected();
        let subsystem_target = event.metadata().target().to_string();
        let provider_request_trace =
            extract_provider_request_trace(&subsystem_target, &structured_field_pairs);
        // Scrubbing at the sink: the composed line rounds the coordinate
        // fields to the sink's decimals.
        let mut message_line = message.unwrap_or_default();
        if let Some(latitude) = latitude {
            message_line.push_str(" latitude=");
            message_line.push_str(&EventLineVisitor::scrub_coordinate(latitude));
        }
        if let Some(longitude) = longitude {
            message_line.push_str(" longitude=");
            message_line.push_str(&EventLineVisitor::scrub_coordinate(longitude));
        }
        for (name, value) in structured_field_pairs {
            message_line.push_str(&format!(" {name}={value}"));
        }
        let severity = tracing_level_severity(event.metadata().level());
        let timestamp_milliseconds_since_epoch = unix_epoch_milliseconds();
        self.handle.record(
            &subsystem_target,
            RecordedEvent {
                timestamp_milliseconds_since_epoch,
                severity,
                sequence: self.handle.ring_sequence.fetch_add(1, Ordering::Relaxed),
                message_line: message_line.clone(),
                provider_request_trace: provider_request_trace.clone(),
            },
        );
        self.handle.dispatch_live(&LogEvent {
            timestamp_milliseconds_since_epoch,
            severity,
            subsystem_target,
            message_line,
            provider_request_trace,
        });
    }

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}

    /// No span structure: the sink has no current span (events are flat).
    fn current_span(&self) -> tracing_core::span::Current {
        tracing_core::span::Current::none()
    }
}

/// Builds the typed request facts when an event is a completed provider-call
/// observation under the shared transport's diagnostic target.
///
/// The observation's tracing fields are already redacted by the transport, so
/// the trace copies them without touching the request a second time. An event
/// that is not a provider observation, or one missing a required field, keeps
/// `None` rather than fabricating facts.
fn extract_provider_request_trace(
    subsystem_target: &str,
    structured_field_pairs: &[(String, String)],
) -> Option<ProviderRequestTrace> {
    if subsystem_target
        != rustic_maps_core::provider_observation::PROVIDER_REQUEST_DIAGNOSTIC_TARGET
    {
        return None;
    }
    let field = |name: &str| {
        structured_field_pairs
            .iter()
            .find(|(field_name, _)| field_name == name)
            .map(|(_, value)| value.clone())
    };
    Some(ProviderRequestTrace {
        provider_key: field("provider_key")?,
        provider_contract: field("provider_contract")?,
        http_method: field("http_method")?,
        request_path: field("request_path")?,
        status_class: field("status_class")?,
        outcome: field("outcome")?,
        duration_milliseconds: field("duration_milliseconds")?.parse().ok()?,
        response_bytes: field("response_bytes")?.parse().ok()?,
        response_excerpt: field("response_excerpt").filter(|excerpt| !excerpt.is_empty()),
    })
}

/// Maps a `tracing` level onto the contract's `SdkLogSeverity` (both
/// ladders ascend from trace).
fn tracing_level_severity(level: &Level) -> SdkLogSeverity {
    match level.as_str() {
        "TRACE" => SdkLogSeverity::Trace,
        "DEBUG" => SdkLogSeverity::Debug,
        "INFO" => SdkLogSeverity::Info,
        "WARN" => SdkLogSeverity::Warn,
        _ => SdkLogSeverity::Error,
    }
}

/// Wall-clock milliseconds since the Unix epoch (the `tracing` sink rings'
/// timestamp and the provider-failure diagnostic seam's timestamp).
pub(crate) fn unix_epoch_milliseconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

/// The sink's event visitor collecting a `tracing` event's fields. The
/// privacy scrub — coordinate fields rounded to
/// [`DIAGNOSTIC_COORDINATE_DECIMALS`] decimals — happens at the sink's
/// composing step, whatever crate emitted the event.
#[derive(Default)]
struct EventLineVisitor {
    message: Option<String>,
    latitude: Option<f64>,
    longitude: Option<f64>,
    structured_field_pairs: Vec<(String, String)>,
}

struct EventLineVisitorCollected {
    message: Option<String>,
    latitude: Option<f64>,
    longitude: Option<f64>,
    structured_field_pairs: Vec<(String, String)>,
}

impl EventLineVisitor {
    /// Rounds a coordinate value to the sink's decimals.
    fn scrub_coordinate(value: f64) -> String {
        format!("{:.*}", DIAGNOSTIC_COORDINATE_DECIMALS, value)
    }

    /// Hands the collected fields to the sink's composing step.
    fn into_collected(self) -> EventLineVisitorCollected {
        EventLineVisitorCollected {
            message: self.message,
            latitude: self.latitude,
            longitude: self.longitude,
            structured_field_pairs: self.structured_field_pairs,
        }
    }

    /// Scrubs a textual coordinate value: parsed as an f64 it rounds to the
    /// sink's decimals; un-parseable it is replaced with the placeholder
    /// (the raw text never reaches the composed line).
    fn scrub_coordinate_text(text: &str) -> String {
        match text.parse::<f64>() {
            Ok(value) => Self::scrub_coordinate(value),
            Err(_) => SCRUBBED_FIELD_PLACEHOLDER.to_string(),
        }
    }
}

impl Visit for EventLineVisitor {
    fn record_f64(&mut self, field: &Field, value: f64) {
        if COORDINATE_FIELD_NAMES.contains(&field.name()) {
            match field.name() {
                "latitude" => self.latitude = Some(value),
                _ => self.longitude = Some(value),
            }
            return;
        }
        self.structured_field_pairs
            .push((field.name().to_string(), format!("{value}")));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
            return;
        }
        if COORDINATE_FIELD_NAMES.contains(&field.name()) {
            self.structured_field_pairs
                .push((field.name().to_string(), Self::scrub_coordinate_text(value)));
            return;
        }
        self.structured_field_pairs
            .push((field.name().to_string(), value.to_string()));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = Some(format!("{value:?}"));
            return;
        }
        if COORDINATE_FIELD_NAMES.contains(&field.name()) {
            self.structured_field_pairs.push((
                field.name().to_string(),
                Self::scrub_coordinate_text(&format!("{value:?}")),
            ));
            return;
        }
        self.structured_field_pairs
            .push((field.name().to_string(), format!("{value:?}")));
    }
}

/// The exported [`SdkDiagnosticEventListener`] contract: foreign code
/// implements it to receive the SDK's structured internal log events —
/// either streamed live (through
/// `MapSdk::subscribe_to_diagnostic_events`, which also lowers the sink's
/// severity gate) or replayed from the bounded recent-events rings (through
/// `MapSdk::flush_recent_diagnostic_events`). Callbacks are
/// fire-and-forget: the sink cannot fail on a listener's failure, and a
/// listener emitting its own events is not recursed into.
///
/// Coordinates in event messages are rounded at the sink to
/// [`DIAGNOSTIC_COORDINATE_DECIMALS`] decimals; the API key never appears
/// in a message.
pub trait SdkDiagnosticEventListener: Send + Sync {
    /// Receives one structured log event of the SDK's internals.
    fn on_diagnostic_event(&self, event: LogEvent);

    /// Internally required by the uniffi foreign-handle machinery (the
    /// foreign-handle probe of the uniffi 0.31 trait-object lowering);
    /// Rust-side implementations keep the default, which reports a
    /// Rust-implemented object.
    #[doc(hidden)]
    fn uniffi_foreign_handle(&self) -> Option<::uniffi::Handle> {
        Option::None
    }
}

/// The exported [`DiagnosticSubscription`] contract: the handle one
/// `MapSdk::subscribe_to_diagnostic_events_with_subscription` registration
/// answers. The handle owns the registration's process-wide id and closes
/// exactly that registration; closing an already closed or unknown
/// subscription is a no-op. Dropping the handle does not remove the
/// registration.
pub struct DiagnosticSubscription {
    /// The process-wide id of the registration this handle owns.
    registration_id: u64,
    /// The process-wide sink the registration was made on.
    handle: &'static DiagnosticsHandle,
}

impl DiagnosticSubscription {
    /// Wraps one registration id on `handle`.
    pub(crate) fn new(registration_id: u64, handle: &'static DiagnosticsHandle) -> Self {
        Self {
            registration_id,
            handle,
        }
    }

    /// Removes the registration this handle owns, so its listener stops
    /// receiving live events from the next event on. Cancelling an already
    /// cancelled subscription changes nothing. The member is named `cancel`
    /// rather than `close` because UniFFI already generates a `close` for an
    /// object handle, and a same-named member collides in Kotlin.
    pub fn cancel(&self) {
        self.handle.close_subscription(self.registration_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A standalone diagnostics state for a test (the process-shared
    /// singleton is off-limits: other tests in this binary run concurrently
    /// and would pollute each other's rings and gate).
    fn fresh_handle() -> DiagnosticsHandle {
        DiagnosticsHandle {
            rings: Mutex::new(HashMap::new()),
            gate: AtomicU8::new(GATE_DEFAULT_RANK),
            listeners: Mutex::new(Vec::new()),
            ring_sequence: AtomicU64::new(1),
            next_registration_id: AtomicU64::new(1),
        }
    }

    /// The wall clock reads from the platform's real time source on every
    /// target: a plausible Unix-epoch millisecond value that never decreases
    /// between two reads. The bounds keep the check meaningful for both the
    /// native and the browser clock without pinning one exact instant.
    #[test]
    fn unix_epoch_milliseconds_reads_the_platform_clock() {
        const EARLIEST_PLAUSIBLE_UNIX_MILLISECONDS: u64 = 1_000_000_000_000;
        const LATEST_PLAUSIBLE_UNIX_MILLISECONDS: u64 = 10_000_000_000_000;

        let first_read = unix_epoch_milliseconds();
        let second_read = unix_epoch_milliseconds();

        assert!(
            (EARLIEST_PLAUSIBLE_UNIX_MILLISECONDS..LATEST_PLAUSIBLE_UNIX_MILLISECONDS)
                .contains(&first_read),
            "the wall clock reads a plausible Unix-epoch millisecond value: {first_read}"
        );
        assert!(
            second_read >= first_read,
            "the wall clock never runs backwards: {first_read} then {second_read}"
        );
    }

    /// The flush seam's ordering: events across subsystems interleave in
    /// recording order (timestamp, then the process-wide sequence) and the
    /// idle gate's severity filter holds.
    #[test]
    fn recent_events_is_oldest_first_and_severity_filtered() {
        let handle = fresh_handle();
        handle.record(
            "renderer",
            RecordedEvent {
                timestamp_milliseconds_since_epoch: 100,
                severity: SdkLogSeverity::Info,
                sequence: 10,
                message_line: "first".to_string(),
                provider_request_trace: None,
            },
        );
        handle.record(
            "tile_fetch",
            RecordedEvent {
                timestamp_milliseconds_since_epoch: 100,
                severity: SdkLogSeverity::Warn,
                sequence: 11,
                message_line: "second".to_string(),
                provider_request_trace: None,
            },
        );
        handle.record(
            "renderer",
            RecordedEvent {
                timestamp_milliseconds_since_epoch: 101,
                severity: SdkLogSeverity::Info,
                sequence: 12,
                message_line: "third".to_string(),
                provider_request_trace: None,
            },
        );
        let flushed = handle.recent_events(SdkLogSeverity::Info, u32::MAX);
        let flushed_lines: Vec<&str> = flushed
            .iter()
            .map(|event| event.message_line.as_str())
            .collect();
        assert_eq!(flushed_lines, ["first", "second", "third"]);
        // The same-millisecond tie resolves on the process-wide sequence.
        assert_eq!(
            flushed[0].subsystem_target, "renderer",
            "sequence 10 precedes sequence 11"
        );
    }

    /// The gate's direction: only an SDK crate's module path is recorded,
    /// and an event coarser than the gate's severity passes.
    #[test]
    fn should_record_filters_module_path_and_gate() {
        let handle = fresh_handle();
        assert!(handle.should_record(
            "rustic_maps_rendering::surface",
            severity_rank(SdkLogSeverity::Info)
        ));
        assert!(!handle.should_record(
            "rustic_maps_rendering::surface",
            severity_rank(SdkLogSeverity::Trace)
        ));
        assert!(
            !handle.should_record("some_app::controller", severity_rank(SdkLogSeverity::Error)),
            "host events never enter the sink"
        );
    }

    /// The severity gate never raises: after a finer subscription lowered
    /// it, a coarser one leaves the finer gate in place.
    #[test]
    fn gate_lowers_and_never_raises() {
        let handle = fresh_handle();
        handle.lower_gate_to(severity_rank(SdkLogSeverity::Trace));
        assert!(handle.should_record(
            "rustic_maps_navigation::reducer",
            severity_rank(SdkLogSeverity::Trace)
        ));
        handle.lower_gate_to(severity_rank(SdkLogSeverity::Info));
        assert!(handle.should_record(
            "rustic_maps_navigation::reducer",
            severity_rank(SdkLogSeverity::Trace)
        ));
    }

    /// The flush's cap: at most `maximum_count` of the newest matching
    /// events, oldest first; the flush is non-destructive (a second flush
    /// redelivers the same events).
    #[test]
    fn flush_is_capped_and_non_destructive() {
        let handle = fresh_handle();
        for index in 0..16_u32 {
            handle.record(
                "renderer",
                RecordedEvent {
                    timestamp_milliseconds_since_epoch: 1_000 + u64::from(index),
                    severity: SdkLogSeverity::Info,
                    sequence: u64::from(index),
                    message_line: format!("frame {index}"),
                    provider_request_trace: None,
                },
            );
        }
        let capped = handle.recent_events(SdkLogSeverity::Info, 8);
        assert_eq!(capped.len(), 8);
        assert_eq!(
            capped.first().expect("capped non-empty").message_line,
            "frame 8"
        );
        assert_eq!(
            capped.last().expect("capped non-empty").message_line,
            "frame 15"
        );
        let redelivered = handle.recent_events(SdkLogSeverity::Info, 8);
        assert_eq!(redelivered.len(), capped.len());
        assert_eq!(
            redelivered.first().expect("capped non-empty").message_line,
            "frame 8"
        );
    }

    /// The level-to-severity mapping ascends consistently with
    /// `severity_rank` (the gate's comparisons rely on the same ladder).
    #[test]
    fn tracing_levels_map_onto_the_severity_ladder() {
        assert_eq!(
            severity_rank_for_tracing_level(&Level::TRACE),
            severity_rank(SdkLogSeverity::Trace)
        );
        assert_eq!(
            severity_rank_for_tracing_level(&Level::DEBUG),
            severity_rank(SdkLogSeverity::Debug)
        );
        assert_eq!(
            severity_rank_for_tracing_level(&Level::WARN),
            severity_rank(SdkLogSeverity::Warn)
        );
        assert_eq!(
            severity_rank_for_tracing_level(&Level::ERROR),
            severity_rank(SdkLogSeverity::Error)
        );
    }

    /// The sink's privacy scrub: coordinate fields are rounded at the sink
    /// to [`DIAGNOSTIC_COORDINATE_DECIMALS`] decimals, in both the numeric
    /// and the textual field forms, and the message text passes through
    /// verbatim. Emitted as a real `tracing` event through the subscriber,
    /// so the whole sink path (field visit, compose, ring record) is
    /// exercised.
    #[test]
    fn the_visitor_scrubs_coordinates_at_the_sink() {
        let handle: &'static DiagnosticsHandle = Box::leak(Box::new(fresh_handle()));
        let subscriber = DiagnosticsSubscriber {
            handle,
            next_span_id: AtomicU64::new(1),
        };
        tracing::subscriber::with_default(Arc::new(subscriber), || {
            tracing::info!(
                target: "renderer",
                latitude = 38.576_612_345_678_f64,
                tile_x = 8_724_u32,
                "frame drawn"
            );
        });
        let recorded = handle.recent_events(SdkLogSeverity::Info, 16);
        let recorded_line = recorded
            .last()
            .expect("the info event was recorded at the gate's default severity")
            .message_line
            .clone();
        assert!(
            recorded_line.contains("latitude=38.577"),
            "scrubbed latitude: {recorded_line}"
        );
        assert!(recorded_line.contains("tile_x=8724"));
        assert!(recorded_line.contains("frame drawn"));
        assert!(
            !recorded_line.contains("38.576612345678"),
            "full precision must not survive the sink: {recorded_line}"
        );
    }

    /// An un-parseable textual coordinate is replaced with the scrub
    /// placeholder, never passed through raw.
    #[test]
    fn unparseable_coordinates_are_replaced_with_the_placeholder() {
        assert_eq!(
            EventLineVisitor::scrub_coordinate_text("38.576612345678 deg N"),
            SCRUBBED_FIELD_PLACEHOLDER
        );
    }

    /// A diagnostics listener keeping every event it receives.
    #[derive(Default)]
    struct RecordingBridgeListener {
        events: Mutex<Vec<LogEvent>>,
    }

    impl SdkDiagnosticEventListener for RecordingBridgeListener {
        fn on_diagnostic_event(&self, event: LogEvent) {
            self.events
                .lock()
                .expect("the bridge test's event list is never poisoned")
                .push(event);
        }
    }

    /// One Info event on the sink's test target.
    fn diagnostic_event(message_line: &str) -> LogEvent {
        LogEvent {
            timestamp_milliseconds_since_epoch: 0,
            severity: SdkLogSeverity::Info,
            subsystem_target: "rustic_maps::tests".to_string(),
            message_line: message_line.to_string(),
            provider_request_trace: None,
        }
    }

    /// The count of events the bridge listener received.
    fn delivered_event_count(listener: &RecordingBridgeListener) -> usize {
        listener
            .events
            .lock()
            .expect("the bridge test's event list is never poisoned")
            .len()
    }

    impl RecordingBridgeListener {
        /// The provider-request observations the listener received for one
        /// probe provider.
        fn observations_for(&self, provider_key: &str) -> Vec<LogEvent> {
            let provider_key_field = format!("provider_key={provider_key}");
            self.events
                .lock()
                .expect("the bridge test's event list is never poisoned")
                .iter()
                .filter(|event| {
                    event.subsystem_target == rustic_maps_core::provider_observation::PROVIDER_REQUEST_DIAGNOSTIC_TARGET
                        && event.message_line.contains(&provider_key_field)
                })
                .cloned()
                .collect()
        }
    }

    /// One provider request reaches both diagnostics seams: the observation
    /// the transports emit becomes one Info event under the transport's
    /// target, streamed to a live subscription and replayable from the
    /// bounded ring. The redacted path is the observation's, so the
    /// credential never appears.
    #[tokio::test]
    async fn a_provider_request_observation_reaches_the_subscription_and_the_ring() {
        const PROBE_PROVIDER_KEY: &str = "diagnostic-probe-provider";
        const PROBE_CONTRACT: &str = "RoutingProvider";
        const PROBE_CREDENTIAL: &str = "SECRET_PROBE_VALUE";

        // The SDK construction installs the diagnostics subscriber and the
        // transport observation bridge, so this exercises the real seam.
        let map_sdk = crate::provider::MapSdk::new();
        let subscription_listener = Arc::new(RecordingBridgeListener::default());
        map_sdk.subscribe_to_diagnostic_events(
            Arc::clone(&subscription_listener) as Arc<dyn SdkDiagnosticEventListener>,
            SdkLogSeverity::Info,
        );

        // The excerpt is behind an explicit switch; this test turns it on for
        // its duration and asserts the redacted prefix reaches both seams.
        rustic_maps_core::provider_observation::set_response_excerpt_capture(true);

        // The provider layer reports one attempt; the SDK's observation
        // vocabulary turns it into the diagnostics event this test follows.
        rustic_maps_core::provider_observation::observe_provider_request(
            PROBE_CONTRACT,
            PROBE_PROVIDER_KEY,
            rustic_maps_core::provider_observation::HttpMethod::Get,
            &format!("https://provider.invalid/route/v1/driving/a;b?key={PROBE_CREDENTIAL}"),
            Some((200, b"{}".as_slice())),
            std::time::Duration::from_millis(1),
        );

        let streamed_observations = subscription_listener.observations_for(PROBE_PROVIDER_KEY);
        assert_eq!(
            streamed_observations.len(),
            1,
            "one observation is streamed per request"
        );
        let observation_line = &streamed_observations[0];
        assert_eq!(observation_line.severity, SdkLogSeverity::Info);
        for expected_fragment in [
            "provider request completed",
            "provider_contract=RoutingProvider",
            "http_method=GET",
            "request_path=/route/v1/driving/a;b?key=<redacted>",
            "status_class=2xx",
            "outcome=success",
            "response_bytes=2",
            "response_excerpt={}",
        ] {
            assert!(
                observation_line.message_line.contains(expected_fragment),
                "the observation line carries {expected_fragment:?}: {}",
                observation_line.message_line
            );
        }
        assert!(
            observation_line
                .message_line
                .contains("duration_milliseconds="),
            "the observation reports its duration: {}",
            observation_line.message_line
        );
        assert!(
            !observation_line.message_line.contains(PROBE_CREDENTIAL),
            "the credential never reaches the diagnostics line: {}",
            observation_line.message_line
        );

        // The same facts are available through the typed record, so a shell
        // reads fields instead of parsing the line.
        let trace = observation_line
            .provider_request_trace
            .as_ref()
            .expect("a provider observation carries typed request facts");
        assert_eq!(trace.provider_key, PROBE_PROVIDER_KEY);
        assert_eq!(trace.provider_contract, PROBE_CONTRACT);
        assert_eq!(trace.http_method, "GET");
        assert_eq!(trace.request_path, "/route/v1/driving/a;b?key=<redacted>");
        assert_eq!(trace.status_class, "2xx");
        assert_eq!(trace.outcome, "success");
        assert_eq!(trace.response_bytes, 2);
        assert_eq!(trace.response_excerpt.as_deref(), Some("{}"));
        assert!(
            !trace.request_path.contains(PROBE_CREDENTIAL),
            "the typed path is redacted too"
        );

        // The bounded ring holds the same event for a later flush.
        let flush_listener = Arc::new(RecordingBridgeListener::default());
        let flushed_count = map_sdk.flush_recent_diagnostic_events(
            Arc::clone(&flush_listener) as Arc<dyn SdkDiagnosticEventListener>,
            SdkLogSeverity::Info,
            u32::MAX,
        );
        assert!(
            flushed_count >= 1,
            "the flush delivered the recorded events"
        );
        let flushed_observations = flush_listener.observations_for(PROBE_PROVIDER_KEY);
        assert_eq!(
            flushed_observations.len(),
            1,
            "the ring replays the request observation"
        );
        assert_eq!(
            flushed_observations[0]
                .provider_request_trace
                .as_ref()
                .map(|trace| trace.provider_key.as_str()),
            Some(PROBE_PROVIDER_KEY),
            "the ring keeps the typed request facts"
        );
        rustic_maps_core::provider_observation::set_response_excerpt_capture(false);
    }

    /// The live dispatch delivers only to listeners that meet the event's
    /// severity and fire-and-forgets; a listener re-emitting its own
    /// `tracing` event while being called is not recursed into (the marker
    /// drops the re-emitted event).
    #[test]
    fn dispatch_live_matches_severity_and_survives_re_emit() {
        use std::sync::Mutex as TestMutex;
        struct RecordingTestListener {
            received: TestMutex<Vec<String>>,
        }
        impl SdkDiagnosticEventListener for RecordingTestListener {
            fn on_diagnostic_event(&self, event: LogEvent) {
                if let Ok(mut received) = self.received.lock() {
                    received.push(event.message_line);
                }
                // Re-emit through the subscriber while being called: the
                // marking drops the re-emitted event (no recursion).
                tracing::info!(target: "rustic_maps::tests", "listener re-emits");
            }
        }

        let listener = Arc::new(RecordingTestListener {
            received: TestMutex::new(Vec::new()),
        });
        let handle: &'static DiagnosticsHandle = Box::leak(Box::new(fresh_handle()));
        handle.subscribe(
            Arc::clone(&listener) as Arc<dyn SdkDiagnosticEventListener>,
            SdkLogSeverity::Info,
        );
        handle.dispatch_live(&LogEvent {
            timestamp_milliseconds_since_epoch: 0,
            severity: SdkLogSeverity::Debug,
            subsystem_target: "rustic_maps::tests".to_string(),
            message_line: "below the subscription".to_string(),
            provider_request_trace: None,
        });
        handle.dispatch_live(&LogEvent {
            timestamp_milliseconds_since_epoch: 1,
            severity: SdkLogSeverity::Info,
            subsystem_target: "rustic_maps::tests".to_string(),
            message_line: "delivered".to_string(),
            provider_request_trace: None,
        });
        let received = listener
            .received
            .lock()
            .expect("the test listener's mutex is only held by this test");
        assert_eq!(
            received.as_slice(),
            ["delivered"],
            "only severity-meeting events are dispatched"
        );
    }

    /// Closing a subscription removes its registration, so no later event
    /// reaches its listener; closing it again is a no-op.
    #[test]
    fn closing_a_subscription_stops_its_live_delivery() {
        let handle: &'static DiagnosticsHandle = Box::leak(Box::new(fresh_handle()));
        let listener = Arc::new(RecordingBridgeListener::default());
        let listener_as_contract = Arc::clone(&listener) as Arc<dyn SdkDiagnosticEventListener>;
        let registration_id = handle.subscribe(listener_as_contract, SdkLogSeverity::Info);
        let subscription = DiagnosticSubscription::new(registration_id, handle);
        handle.dispatch_live(&diagnostic_event("before close"));
        assert_eq!(
            delivered_event_count(&listener),
            1,
            "the subscription delivers"
        );

        subscription.cancel();
        handle.dispatch_live(&diagnostic_event("after close"));
        assert_eq!(
            delivered_event_count(&listener),
            1,
            "a closed subscription stops delivering"
        );
        subscription.cancel();
        handle.dispatch_live(&diagnostic_event("after a second close"));
        assert_eq!(
            delivered_event_count(&listener),
            1,
            "closing twice is a no-op"
        );
    }

    /// Two subscriptions deliver independently: closing one leaves the other
    /// delivering, so a close removes exactly the registration it owns.
    #[test]
    fn closing_one_subscription_leaves_the_other_delivering() {
        let handle: &'static DiagnosticsHandle = Box::leak(Box::new(fresh_handle()));
        let first = Arc::new(RecordingBridgeListener::default());
        let first_as_contract = Arc::clone(&first) as Arc<dyn SdkDiagnosticEventListener>;
        let second = Arc::new(RecordingBridgeListener::default());
        let second_as_contract = Arc::clone(&second) as Arc<dyn SdkDiagnosticEventListener>;
        let first_subscription = DiagnosticSubscription::new(
            handle.subscribe(first_as_contract, SdkLogSeverity::Info),
            handle,
        );
        let _second_subscription = DiagnosticSubscription::new(
            handle.subscribe(second_as_contract, SdkLogSeverity::Info),
            handle,
        );

        first_subscription.cancel();
        handle.dispatch_live(&diagnostic_event("after closing the first"));
        assert_eq!(
            delivered_event_count(&first),
            0,
            "the closed subscription does not deliver"
        );
        assert_eq!(
            delivered_event_count(&second),
            1,
            "the other subscription still delivers"
        );
    }

    /// Closing a subscription whose id is unknown changes nothing, so an
    /// already closed or never created registration cannot remove another's.
    #[test]
    fn closing_an_unknown_subscription_is_a_no_op() {
        let handle: &'static DiagnosticsHandle = Box::leak(Box::new(fresh_handle()));
        let listener = Arc::new(RecordingBridgeListener::default());
        let listener_as_contract = Arc::clone(&listener) as Arc<dyn SdkDiagnosticEventListener>;
        handle.subscribe(listener_as_contract, SdkLogSeverity::Info);

        DiagnosticSubscription::new(u64::MAX, handle).cancel();
        handle.dispatch_live(&diagnostic_event("after closing an unknown subscription"));
        assert_eq!(
            delivered_event_count(&listener),
            1,
            "the live registration is untouched"
        );
    }

    /// The exported `MapSdk` member answers a handle that closes its
    /// registration; closing it twice is safe. This is the registration path
    /// the generated bindings call, over the process-wide sink.
    #[test]
    fn the_map_sdk_member_answers_a_closable_subscription() {
        let map_sdk = crate::provider::MapSdk::new();
        let listener = Arc::new(RecordingBridgeListener::default());
        let subscription = map_sdk.subscribe_to_diagnostic_events_with_subscription(
            Arc::clone(&listener) as Arc<dyn SdkDiagnosticEventListener>,
            SdkLogSeverity::Info,
        );
        subscription.cancel();
        subscription.cancel();
    }
}
