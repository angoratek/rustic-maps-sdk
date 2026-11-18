//! The diagnostics readout's wasm seam: a bounded recent-events ring behind a
//! `tracing` subscriber, mirroring the native targets' diagnostic sink in the
//! reduced shape the browser surface needs.
//!
//! The native sink's ring record dispatches to live foreign listeners; the
//! wasm surface has no live-listener seam, so this module keeps the ring and
//! the severity gate and drops the live dispatch: the page reads the rings
//! through [`flush_recent_diagnostic_events`]. The exports mirror the
//! contract's `MapSdk` method names minus its listener parameter:
//! [`subscribe_to_diagnostic_events`] lowers the recording gate to the
//! requested severity, and [`flush_recent_diagnostic_events`] reads the most
//! recent events matching a severity, oldest first.
//!
//! The same contract rules apply: the privacy scrub rounds structured
//! `latitude`/`longitude` fields to the sink's decimals at the composing
//! step in exactly one place; the gate never raises back once fine-grained
//! events have been requested; a host that already installed its own global
//! subscriber keeps precedence (the ring then stays empty and the flush seam
//! answers an empty list); spans are unsupported (events are flat; every
//! event's `tracing` target names its subsystem).

use std::collections::{HashMap, VecDeque};
use std::str::FromStr;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use tracing::{Event, Id, Level, Metadata, Subscriber, field::Field, field::Visit};
use tracing_core::{Interest, LevelFilter};
use wasm_bindgen::prelude::*;

use rustic_maps_core::provider_observation::PROVIDER_REQUEST_DIAGNOSTIC_TARGET;

/// The capacity of each subsystem's diagnostics ring, in events — the
/// native sink's own bound (bounded so a chatty subsystem cannot grow the
/// page's memory unboundedly).
pub(crate) const DIAGNOSTIC_RING_CAPACITY: usize = 512;

/// Decimals coordinate values are rounded to at the sink — the native sink's
/// privacy granularity (≈ 110 m at the equator).
const DIAGNOSTIC_COORDINATE_DECIMALS: usize = 3;

/// The named severity ranks (higher is more severe; only relative order is
/// meaningful), the native sink's own ladder.
const TRACE_SEVERITY_RANK: u8 = 0;
const DEBUG_SEVERITY_RANK: u8 = 1;
const INFO_SEVERITY_RANK: u8 = 2;
const WARN_SEVERITY_RANK: u8 = 3;
const ERROR_SEVERITY_RANK: u8 = 4;
/// The rank an unclassifiable `tracing` level folds to (most severe).
const UNCLASSIFIED_SEVERITY_RANK: u8 = ERROR_SEVERITY_RANK;

/// The idle recording gate: events at informational severity or above
/// record while nothing has requested finer granularity — the native sink's
/// idle severity, so the GPU-hot frame path's trace and debug instrumentation
/// stays unformatted until the readout asks for it.
const GATE_DEFAULT_RANK: u8 = INFO_SEVERITY_RANK;

/// The module-path (and `tracing` target) prefixes of the SDK crates whose
/// events the ring records; the page's own code's events never enter it.
/// The structured event fields carrying coordinates, which the visitor
/// scrubs at the sink.
const COORDINATE_FIELD_NAMES: [&str; 2] = ["latitude", "longitude"];

/// A placeholder replacing an un-parseable coordinate field value at the
/// sink (the field's raw content never reaches the composed line).
const SCRUBBED_FIELD_PLACEHOLDER: &str = "scrubbed";

/// Severity classification of a diagnostics event, mirroring the contract's
/// `SdkLogSeverity` at the JavaScript seam (the shared record type cannot
/// itself carry `wasm-bindgen` attributes; the same pattern as
/// [`crate::canvas::CanvasTileResolution`] and friends).
#[wasm_bindgen(js_name = SdkLogSeverity)]
pub enum CanvasSdkLogSeverity {
    /// The finest-grained events (the GPU-hot render path's instrumentation).
    Trace,
    /// Verbose internal events.
    Debug,
    /// Coarse informational events (the idle gate records down to here).
    Info,
    /// Warnings.
    Warn,
    /// Errors.
    Error,
}

/// The numeric severity rank of a [`CanvasSdkLogSeverity`] (higher is more
/// severe).
const fn severity_rank_for_severity(severity: CanvasSdkLogSeverity) -> u8 {
    match severity {
        CanvasSdkLogSeverity::Trace => TRACE_SEVERITY_RANK,
        CanvasSdkLogSeverity::Debug => DEBUG_SEVERITY_RANK,
        CanvasSdkLogSeverity::Info => INFO_SEVERITY_RANK,
        CanvasSdkLogSeverity::Warn => WARN_SEVERITY_RANK,
        CanvasSdkLogSeverity::Error => ERROR_SEVERITY_RANK,
    }
}

/// The named severity an entry reports: the contract's `SdkLogSeverity` enum
/// spellings, which the consumer readouts mirror verbatim.
fn severity_name_for_rank(severity_rank: u8) -> &'static str {
    match severity_rank {
        TRACE_SEVERITY_RANK => "Trace",
        DEBUG_SEVERITY_RANK => "Debug",
        INFO_SEVERITY_RANK => "Info",
        WARN_SEVERITY_RANK => "Warn",
        _ => "Error",
    }
}

/// The numeric severity rank of a `tracing` level, in the severity ranks'
/// ordering (both severity ladders ascend from trace).
fn severity_rank_for_tracing_level(level: &Level) -> u8 {
    match level.as_str() {
        "TRACE" => TRACE_SEVERITY_RANK,
        "DEBUG" => DEBUG_SEVERITY_RANK,
        "INFO" => INFO_SEVERITY_RANK,
        "WARN" => WARN_SEVERITY_RANK,
        _ => UNCLASSIFIED_SEVERITY_RANK,
    }
}

/// One ring entry: the sink's already-scrubbed composed line and its
/// classification.
struct RecordedEvent {
    timestamp_milliseconds_since_epoch: u64,
    severity_rank: u8,
    /// Process-wide sequence, ordering events recorded within the same
    /// millisecond.
    sequence: u64,
    message_line: String,
    /// The structured provider-request facts, when the event is one the
    /// provider observation bridge published. The console reads these fields
    /// instead of parsing the message line.
    provider_request: Option<RecordedProviderRequest>,
}

/// The provider-request facts one recorded event carries.
#[derive(Debug, Clone)]
struct RecordedProviderRequest {
    provider_key: String,
    provider_contract: String,
    http_method: String,
    request_path: String,
    status_class: String,
    outcome: String,
    duration_milliseconds: u64,
    response_bytes: u64,
    /// The redacted response prefix, present only while the excerpt switch is
    /// on and the response carried a body.
    response_excerpt: Option<String>,
}

/// The provider-request fields one event's visitor collected.
#[derive(Default)]
struct ProviderRequestFields {
    provider_key: Option<String>,
    provider_contract: Option<String>,
    http_method: Option<String>,
    request_path: Option<String>,
    status_class: Option<String>,
    outcome: Option<String>,
    duration_milliseconds: Option<u64>,
    response_bytes: Option<u64>,
    response_excerpt: Option<String>,
}

impl ProviderRequestFields {
    /// The complete record, or `None` when an event is not a provider
    /// request (any field missing means it is not one).
    fn into_record(self) -> Option<RecordedProviderRequest> {
        Some(RecordedProviderRequest {
            provider_key: self.provider_key?,
            provider_contract: self.provider_contract?,
            http_method: self.http_method?,
            request_path: self.request_path?,
            status_class: self.status_class?,
            outcome: self.outcome?,
            duration_milliseconds: self.duration_milliseconds?,
            response_bytes: self.response_bytes?,
            response_excerpt: self.response_excerpt.filter(|excerpt| !excerpt.is_empty()),
        })
    }
}

/// The wasm diagnostics state: the per-subsystem bounded rings and the
/// severity gate, shared through [`shared_state`] behind the global
/// subscriber. No live listeners exist on the wasm seam (see the module
/// head): the rings are the only record.
struct DiagnosticsRingState {
    rings: Mutex<HashMap<String, VecDeque<RecordedEvent>>>,
    gate_rank: AtomicU8,
    ring_sequence: AtomicU64,
}

impl DiagnosticsRingState {
    /// Lowers the gate to `rank` if it is finer than the current gate (the
    /// gate never raises back once fine-grained events have been requested).
    fn lower_gate_to(&self, rank: u8) {
        if rank < self.gate_rank.load(Ordering::Relaxed) {
            self.gate_rank.store(rank, Ordering::Relaxed);
        }
    }

    /// Whether an event with this rank from this module path records: the
    /// module path must come from an SDK crate (not the page's code) and
    /// pass the severity gate (called before any argument formatting).
    fn should_record(&self, module_path_or_target: &str, severity_rank_value: u8) -> bool {
        crate::diagnostics_filter::records_sdk_event(
            module_path_or_target,
            severity_rank_value,
            self.gate_rank.load(Ordering::Relaxed),
        )
    }

    /// Records one scrubbed event into the named subsystem's ring (ring
    /// retention: at the capacity the oldest event drops).
    fn record(&self, subsystem_target: &str, recorded_event: RecordedEvent) {
        let mut rings = self
            .rings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let ring = rings
            .entry(subsystem_target.to_string())
            .or_insert_with(|| VecDeque::with_capacity(DIAGNOSTIC_RING_CAPACITY));
        if ring.len() >= DIAGNOSTIC_RING_CAPACITY {
            ring.pop_front();
        }
        ring.push_back(recorded_event);
    }

    /// Reads the most recent events across every subsystem's ring, oldest
    /// first, filtered by the minimum severity rank and capped at
    /// `maximum_count` (the flush seam; the rings are never emptied, so a
    /// later flush can redeliver).
    fn recent_events(&self, minimum_severity_rank: u8, maximum_count: u32) -> Vec<EventLine> {
        let mut matched_events: Vec<(String, RecordedEvent)> = {
            let rings = self
                .rings
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            rings
                .iter()
                .flat_map(|(subsystem_target, ring)| {
                    ring.iter()
                        .filter(|recorded_event| {
                            recorded_event.severity_rank >= minimum_severity_rank
                        })
                        .map(|recorded_event| {
                            (
                                subsystem_target.clone(),
                                RecordedEvent {
                                    timestamp_milliseconds_since_epoch: recorded_event
                                        .timestamp_milliseconds_since_epoch,
                                    severity_rank: recorded_event.severity_rank,
                                    sequence: recorded_event.sequence,
                                    message_line: recorded_event.message_line.clone(),
                                    provider_request: recorded_event.provider_request.clone(),
                                },
                            )
                        })
                })
                .collect()
        };
        // Oldest first across subsystems, the process-wide sequence breaking
        // same-millisecond ties; the newest `maximum_count` events are the
        // requested prefix.
        matched_events.sort_unstable_by_key(|(_, recorded_event)| {
            (
                recorded_event.timestamp_milliseconds_since_epoch,
                recorded_event.sequence,
            )
        });
        matched_events
            .into_iter()
            .rev()
            .take(maximum_count as usize)
            .rev()
            .map(|(subsystem_target, recorded_event)| EventLine {
                subsystem_target,
                recorded_event,
            })
            .collect()
    }
}

/// One flushed diagnostics line, the plain-JS shape the page's readout
/// consumes (the native `LogEvent`'s fields, severity already
/// spelled with the contract's enum name).
struct EventLine {
    subsystem_target: String,
    recorded_event: RecordedEvent,
}

/// The process-wide state, installing the ring subscriber's handle on first
/// use; the shared handle reference the exports read through.
fn shared_state() -> &'static DiagnosticsRingState {
    static SHARED_STATE: OnceLock<&'static DiagnosticsRingState> = OnceLock::new();
    SHARED_STATE.get_or_init(|| {
        let ring_state: &'static DiagnosticsRingState = Box::leak(Box::new(DiagnosticsRingState {
            rings: Mutex::new(HashMap::new()),
            gate_rank: AtomicU8::new(GATE_DEFAULT_RANK),
            ring_sequence: AtomicU64::new(1),
        }));
        // An already-installed global subscriber (the page's host code) keeps
        // precedence; the ring then stays empty and the flush answers an
        // empty list — the documented, honest shape of a process-wide
        // logging facade.
        let _ = tracing::subscriber::set_global_default(DiagnosticsSubscriber {
            ring_state,
            next_span_id: AtomicU64::new(1),
        });
        ring_state
    })
}

/// Installs the ring subscriber if no global subscriber is live yet. Called
/// from `start_sdk` (once, at module start); later calls are no-ops whatever
/// the caller, because the OnceLock is already held.
pub(crate) fn ensure_ring_subscriber() {
    let _ = shared_state();
}

/// Lowers the recording gate to the requested severity: from that moment on,
/// events at the severity or above record into the bounded rings (the gate
/// never raises back — the same rule the native sink documents). Installs
/// the ring subscriber on the first call; a no-op gate-wise when the page's
/// host already installed its own global subscriber (the ring then stays
/// empty and the flush seam answers an empty list — the subscriber's own
/// `enabled` is never consulted).
#[wasm_bindgen(js_name = subscribeToDiagnosticEvents)]
pub fn subscribe_to_diagnostic_events(minimum_severity: CanvasSdkLogSeverity) {
    shared_state().lower_gate_to(severity_rank_for_severity(minimum_severity));
}

/// Enables or disables the bounded, redacted response prefix carried by
/// provider request observations.
///
/// The switch is off by default; while it is on, each flushed provider
/// request carries `responseExcerpt` (the first 512 bytes of the response
/// body, redacted and truncated on a character boundary). The switch is
/// process-wide and reaches the provider layer's own excerpt capture.
#[wasm_bindgen(js_name = setResponseExcerptCapture)]
pub fn set_response_excerpt_capture(enabled: bool) {
    rustic_maps_core::provider_observation::set_response_excerpt_capture(enabled);
}

/// Reads the most recent events the rings recorded at the requested
/// severity or above, oldest first, capped at `maximum_count`, as an array
/// of `{timestampMillisecondsSinceEpoch, severity, subsystemTarget,
/// messageLine, providerRequest}` objects. `providerRequest` is `null` unless
/// the event is a provider request observation, in which case it carries
/// `{providerKey, providerContract, httpMethod, requestPath, statusClass,
/// outcome, durationMilliseconds, responseBytes, responseExcerpt}`. Coordinates
/// in the messages are already rounded to the sink's decimals (the sink's
/// privacy scrub, the native contract rule).
#[wasm_bindgen(js_name = flushRecentDiagnosticEvents)]
pub fn flush_recent_diagnostic_events(
    minimum_severity: CanvasSdkLogSeverity,
    maximum_count: u32,
) -> js_sys::Array {
    let ring_state = shared_state();
    let event_lines =
        ring_state.recent_events(severity_rank_for_severity(minimum_severity), maximum_count);
    let flushed_events = js_sys::Array::new();
    for event_line in event_lines {
        let flushed_event = js_sys::Object::new();
        let _ = js_sys::Reflect::set(
            &flushed_event,
            &"timestampMillisecondsSinceEpoch".into(),
            &JsValue::from(event_line.recorded_event.timestamp_milliseconds_since_epoch),
        );
        let _ = js_sys::Reflect::set(
            &flushed_event,
            &"severity".into(),
            &JsValue::from_str(severity_name_for_rank(
                event_line.recorded_event.severity_rank,
            )),
        );
        let _ = js_sys::Reflect::set(
            &flushed_event,
            &"subsystemTarget".into(),
            &JsValue::from_str(&event_line.subsystem_target),
        );
        let _ = js_sys::Reflect::set(
            &flushed_event,
            &"messageLine".into(),
            &JsValue::from_str(&event_line.recorded_event.message_line),
        );
        let provider_request_value = match &event_line.recorded_event.provider_request {
            Some(provider_request) => {
                let request_object = js_sys::Object::new();
                for (field_name, field_value) in [
                    ("providerKey", provider_request.provider_key.as_str()),
                    (
                        "providerContract",
                        provider_request.provider_contract.as_str(),
                    ),
                    ("httpMethod", provider_request.http_method.as_str()),
                    ("requestPath", provider_request.request_path.as_str()),
                    ("statusClass", provider_request.status_class.as_str()),
                    ("outcome", provider_request.outcome.as_str()),
                ] {
                    let _ = js_sys::Reflect::set(
                        &request_object,
                        &JsValue::from_str(field_name),
                        &JsValue::from_str(field_value),
                    );
                }
                for (field_name, field_value) in [
                    (
                        "durationMilliseconds",
                        provider_request.duration_milliseconds,
                    ),
                    ("responseBytes", provider_request.response_bytes),
                ] {
                    let _ = js_sys::Reflect::set(
                        &request_object,
                        &JsValue::from_str(field_name),
                        &JsValue::from_f64(field_value as f64),
                    );
                }
                let response_excerpt_value = match &provider_request.response_excerpt {
                    Some(response_excerpt) => JsValue::from_str(response_excerpt),
                    None => JsValue::NULL,
                };
                let _ = js_sys::Reflect::set(
                    &request_object,
                    &"responseExcerpt".into(),
                    &response_excerpt_value,
                );
                request_object.into()
            }
            None => JsValue::NULL,
        };
        let _ = js_sys::Reflect::set(
            &flushed_event,
            &"providerRequest".into(),
            &provider_request_value,
        );
        flushed_events.push(&flushed_event.into());
    }
    flushed_events
}

/// Wall-clock milliseconds since the Unix epoch, read from the browser clock
/// (`Date.now()` on wasm — `std::time` has no source to query here).
fn unix_epoch_milliseconds() -> u64 {
    let now_milliseconds = js_sys::Date::now();
    if now_milliseconds.is_finite() && now_milliseconds >= 0.0 {
        now_milliseconds as u64
    } else {
        0
    }
}

/// The `tracing` subscriber behind the wasm diagnostics readout: SDK-crate
/// events passing the severity gate are scrubbed and ring-recorded. Spans
/// are not supported (events are flat; a span's `new_span` hands out a
/// unique id, and span bookkeeping is ignored), mirroring the native sink.
struct DiagnosticsSubscriber {
    ring_state: &'static DiagnosticsRingState,
    next_span_id: AtomicU64,
}

impl Subscriber for DiagnosticsSubscriber {
    /// The gate check — called by `tracing` before any event argument is
    /// formatted, which is what keeps the GPU-hot render path at near-zero
    /// logging cost while the gate sits at informational.
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        self.ring_state.should_record(
            metadata.module_path().unwrap_or(metadata.target()),
            severity_rank_for_tracing_level(metadata.level()),
        )
    }

    /// Keeps every callsite interested for the process's lifetime. `tracing`
    /// caches the interest of a callsite the first time it is hit, so a
    /// callsite that fires while the gate sits at Info would be marked
    /// `never` and stay invisible to a later Debug opt-in. The gate check
    /// still happens per event in [`Self::enabled`], so the render path pays
    /// no formatting cost; the interest only decides whether the callsite is
    /// cheap to check.
    fn register_callsite(&self, _metadata: &'static Metadata<'static>) -> Interest {
        Interest::always()
    }

    /// Advertises the highest level the ring can record, so a later Debug
    /// opt-in sees callsites that were first hit at Info.
    fn max_level_hint(&self) -> Option<LevelFilter> {
        Some(LevelFilter::TRACE)
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
        // Scrubbing at the sink: the composed line rounds the coordinate
        // fields to the sink's decimals, then appends the remaining
        // structured fields as `name=value` pairs.
        let mut message_line = visitor.message.unwrap_or_default();
        if let Some(latitude) = visitor.latitude {
            message_line.push_str(" latitude=");
            message_line.push_str(&EventLineVisitor::scrub_coordinate(latitude));
        }
        if let Some(longitude) = visitor.longitude {
            message_line.push_str(" longitude=");
            message_line.push_str(&EventLineVisitor::scrub_coordinate(longitude));
        }
        for (field_name, field_value) in visitor.structured_field_pairs {
            message_line.push_str(&format!(" {field_name}={field_value}"));
        }
        let provider_request = if event.metadata().target() == PROVIDER_REQUEST_DIAGNOSTIC_TARGET {
            visitor.provider_request_fields.into_record()
        } else {
            None
        };
        self.ring_state.record(
            event.metadata().target(),
            RecordedEvent {
                timestamp_milliseconds_since_epoch: unix_epoch_milliseconds(),
                severity_rank: severity_rank_for_tracing_level(event.metadata().level()),
                sequence: self
                    .ring_state
                    .ring_sequence
                    .fetch_add(1, Ordering::Relaxed),
                message_line,
                provider_request,
            },
        );
    }

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}

    /// No span structure: the sink has no current span (events are flat).
    fn current_span(&self) -> tracing_core::span::Current {
        tracing_core::span::Current::none()
    }
}

/// The sink's event visitor collecting a `tracing` event's fields. The
/// privacy scrub — coordinate fields rounded to
/// [`DIAGNOSTIC_COORDINATE_DECIMALS`] decimals at the composing step —
/// happens exactly here, whatever crate emitted the event.
#[derive(Default)]
struct EventLineVisitor {
    message: Option<String>,
    latitude: Option<f64>,
    longitude: Option<f64>,
    structured_field_pairs: Vec<(String, String)>,
    provider_request_fields: ProviderRequestFields,
}

impl EventLineVisitor {
    /// Rounds a coordinate value to the sink's decimals.
    fn scrub_coordinate(coordinate_value: f64) -> String {
        format!("{coordinate_value:.DIAGNOSTIC_COORDINATE_DECIMALS$}")
    }

    /// Scrubs a textual coordinate value: parsed as an `f64` it rounds to
    /// the sink's decimals; un-parseable it is replaced with the placeholder
    /// (the raw text never reaches the composed line).
    fn scrub_coordinate_text(coordinate_text: &str) -> String {
        match f64::from_str(coordinate_text) {
            Ok(coordinate_value) => Self::scrub_coordinate(coordinate_value),
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
        match field.name() {
            "provider_key" => self.provider_request_fields.provider_key = Some(value.to_string()),
            "provider_contract" => {
                self.provider_request_fields.provider_contract = Some(value.to_string())
            }
            "http_method" => self.provider_request_fields.http_method = Some(value.to_string()),
            "request_path" => self.provider_request_fields.request_path = Some(value.to_string()),
            "status_class" => self.provider_request_fields.status_class = Some(value.to_string()),
            "outcome" => self.provider_request_fields.outcome = Some(value.to_string()),
            "response_excerpt" => {
                self.provider_request_fields.response_excerpt = Some(value.to_string())
            }
            _ => {}
        }
        self.structured_field_pairs
            .push((field.name().to_string(), value.to_string()));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        match field.name() {
            "duration_milliseconds" => {
                self.provider_request_fields.duration_milliseconds = Some(value)
            }
            "response_bytes" => self.provider_request_fields.response_bytes = Some(value),
            _ => {}
        }
        self.structured_field_pairs
            .push((field.name().to_string(), value.to_string()));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
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
        let debug_text = format!("{value:?}");
        // `tracing` 0.1 records integer fields through `record_debug` for the
        // macros' default path, so the numeric provider facts are read here
        // as well as in `record_u64`.
        match field.name() {
            "duration_milliseconds" => {
                self.provider_request_fields.duration_milliseconds = debug_text.parse().ok()
            }
            "response_excerpt" => {
                self.provider_request_fields.response_excerpt = Some(debug_text.to_string())
            }
            "response_bytes" => {
                self.provider_request_fields.response_bytes = debug_text.parse().ok()
            }
            _ => {}
        }
        self.structured_field_pairs
            .push((field.name().to_string(), debug_text));
    }
}
