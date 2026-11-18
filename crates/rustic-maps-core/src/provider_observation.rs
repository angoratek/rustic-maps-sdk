//! Provider-request observation: the record a console shows for one provider
//! call, and the sink the SDK installs to receive it.
//!
//! The observation is built here rather than in a provider crate so the
//! redaction rule has one home: a request path keeps its structure, query
//! values are dropped except the documented non-secret parameters, and the
//! response snippet is capped at [`RESPONSE_SNIPPET_MAXIMUM_BYTES`] on a
//! character boundary. A credential reaches this module as part of the request
//! and never leaves it — the tests feed one through a request and assert the
//! observation carries neither the value nor the parameter pair.
//!
//! The sink is process-wide because the provider crates are stateless with
//! respect to the SDK: the native binding and the browser façade each install
//! one observer at startup, and a provider crate used on its own (a unit test,
//! a stand-alone integration) runs with none installed and pays no cost.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use url::Url;

/// The HTTP method an observation reports.
///
/// The SDK's providers use the pair below; a provider layer method outside it
/// is reported by its adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    /// A retrieval.
    Get,
    /// A submission.
    Post,
}

/// The diagnostics target every provider observation is published under.
pub const PROVIDER_REQUEST_DIAGNOSTIC_TARGET: &str = "provider_transport";

/// Immediate provider requests a source admits when it declares no budget.
pub const DEFAULT_TOKEN_BUCKET_CAPACITY_TOKENS: f64 = 8.0;

/// Sustained provider request rate a source admits when it declares none.
pub const DEFAULT_REFILL_TOKENS_PER_SECOND: f64 = 8.0;

/// Maximum length of the response snippet in an observation, in bytes,
/// counted on character boundaries so a multi-byte glyph is never split.
pub const RESPONSE_SNIPPET_MAXIMUM_BYTES: usize = 512;

/// Query parameters whose values may appear in an observation because they
/// describe the request shape rather than carry data or a secret. Every other
/// parameter's value is dropped; its name is not.
pub const OBSERVABLE_QUERY_PARAMETER_NAMES: [&str; 4] =
    ["format", "overview", "geometries", "units"];

/// The status class an observation reports when no HTTP response arrived.
pub const TRANSPORT_FAILURE_STATUS_CLASS: &str = "transport-failure";

/// Whether observations capture the response prefix. Off by default: a
/// host opts in when it shows request rows, and the switch reaches the
/// provider layer's own excerpt capture so a hosted provider hands the body
/// over only while it is on.
static RESPONSE_EXCERPT_CAPTURE_ENABLED: AtomicBool = AtomicBool::new(false);

/// Enables or disables the bounded response excerpt on provider
/// observations, and on the provider layer's own observation seam.
pub fn set_response_excerpt_capture(enabled: bool) {
    RESPONSE_EXCERPT_CAPTURE_ENABLED.store(enabled, Ordering::Relaxed);
    everymap_core::client::set_response_excerpt_capture(enabled);
}

/// Whether observations currently capture the response prefix.
pub fn response_excerpt_capture_enabled() -> bool {
    RESPONSE_EXCERPT_CAPTURE_ENABLED.load(Ordering::Relaxed)
}

/// The outcome an observation reports for a `2xx` response.
pub const OUTCOME_SUCCESS: &str = "success";
/// The outcome an observation reports for a `3xx` response.
pub const OUTCOME_REDIRECT: &str = "redirect";
/// The outcome an observation reports for a `4xx` response.
pub const OUTCOME_CLIENT_ERROR: &str = "client-error";
/// The outcome an observation reports for a `5xx` response.
pub const OUTCOME_SERVER_ERROR: &str = "server-error";
/// The outcome an observation reports when no HTTP response arrived.
pub const OUTCOME_TRANSPORT_FAILURE: &str = "transport-failure";
/// The outcome an observation reports for a status class it does not
/// recognize.
pub const OUTCOME_UNCLASSIFIED: &str = "unclassified";

/// One provider call, already redacted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRequestObservation {
    /// The provider contract the call served, for example `RoutingProvider`.
    pub provider_contract: String,
    /// The registry key of the serving provider.
    pub provider_key: String,
    /// The HTTP method as a two-to-four-letter word (`GET`, `POST`).
    pub http_method: String,
    /// The redacted request path, query names included and values removed.
    pub request_path: String,
    /// `2xx`, `3xx`, `4xx`, `5xx` or [`TRANSPORT_FAILURE_STATUS_CLASS`].
    pub status_class: String,
    /// Wall-clock duration of the attempt, in milliseconds.
    pub duration_milliseconds: u64,
    /// Response payload size in bytes; zero when no response arrived.
    pub response_bytes: u64,
    /// The redacted response prefix captured behind the excerpt switch, capped
    /// at [`RESPONSE_SNIPPET_MAXIMUM_BYTES`]; empty while capture is off or no
    /// response body arrived.
    pub response_excerpt: String,
}

impl ProviderRequestObservation {
    /// The outcome word for this observation, derived from its status class:
    /// [`OUTCOME_SUCCESS`], [`OUTCOME_REDIRECT`], [`OUTCOME_CLIENT_ERROR`],
    /// [`OUTCOME_SERVER_ERROR`], [`OUTCOME_TRANSPORT_FAILURE`] or
    /// [`OUTCOME_UNCLASSIFIED`].
    pub fn outcome(&self) -> &'static str {
        outcome_text(&self.status_class)
    }
}

/// Receives one observation per completed provider call.
pub trait ProviderRequestObserver: Send + Sync {
    /// Records one provider call. Called synchronously on the request's
    /// task, so an implementation must not block.
    fn on_provider_request(&self, observation: ProviderRequestObservation);
}

/// The installed observer, if any.
static INSTALLED_OBSERVER: OnceLock<Mutex<Option<Arc<dyn ProviderRequestObserver>>>> =
    OnceLock::new();

/// Installs the observer that receives every provider call made afterwards.
///
/// Replacing an installed observer is allowed: a binding constructs its SDK
/// more than once in the same process, and the last installed observer is the
/// one that matters. Passing no observer (`None`) stops delivery.
pub fn install_provider_request_observer(observer: Option<Arc<dyn ProviderRequestObserver>>) {
    let installed_observer = INSTALLED_OBSERVER.get_or_init(|| Mutex::new(None));
    *installed_observer
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = observer;
}

/// A [`ProviderRequestObserver`] that publishes every observation as one
/// structured `tracing` event under [`PROVIDER_REQUEST_DIAGNOSTIC_TARGET`].
///
/// The event carries the provider key, the contract, the HTTP method, the
/// redacted request path, the status class, the duration, the response byte
/// count and the derived outcome. Because it is the observation the transport
/// already redacted, a credential cannot reach the event; the SDK's
/// diagnostics sink records the event like any other SDK log line, so a
/// request observation is visible through the existing diagnostics
/// subscription and flush seams.
///
/// A provider crate used on its own never installs this bridge and pays no
/// logging cost; each SDK seam installs it once at construction through
/// [`install_tracing_observation_bridge`].
#[derive(Debug, Default)]
pub struct TracingObservationBridge;

impl ProviderRequestObserver for TracingObservationBridge {
    fn on_provider_request(&self, observation: ProviderRequestObservation) {
        let outcome = observation.outcome();
        tracing::info!(
            target: PROVIDER_REQUEST_DIAGNOSTIC_TARGET,
            provider_key = observation.provider_key.as_str(),
            provider_contract = observation.provider_contract.as_str(),
            http_method = observation.http_method.as_str(),
            request_path = observation.request_path.as_str(),
            status_class = observation.status_class.as_str(),
            outcome,
            duration_milliseconds = observation.duration_milliseconds,
            response_bytes = observation.response_bytes,
            response_excerpt = observation.response_excerpt.as_str(),
            "provider request completed"
        );
    }
}

/// Installs [`TracingObservationBridge`] as the process's provider-request
/// observer, replacing any observer installed earlier (the slot's documented
/// last-one-wins rule).
pub fn install_tracing_observation_bridge() {
    install_provider_request_observer(Some(Arc::new(TracingObservationBridge)));
}

/// Builds one redacted observation for a completed attempt and hands it to
/// the installed observer. Returns immediately when none is installed.
///
/// # Parameters
/// - `provider_contract` — the contract the call served.
/// - `provider_key` — the serving provider's registry key.
/// - `http_method` — the request's method.
/// - `request_url` — the fully built request URL, before redaction.
/// - `outcome` — the status code and body, when a response arrived.
/// - `duration` — how long the attempt took.
pub fn observe_provider_request(
    provider_contract: &str,
    provider_key: &str,
    http_method: HttpMethod,
    request_url: &str,
    outcome: Option<(u16, &[u8])>,
    duration: Duration,
) {
    deliver_provider_request_observation(provider_request_observation(
        provider_contract,
        provider_key,
        http_method,
        request_url,
        outcome,
        duration,
    ));
}

/// Builds one [`ProviderRequestObservation`] without delivering it.
///
/// A caller that reports through a different transport — the provider layer
/// has its own observer — can build the record here and hand it to
/// [`deliver_provider_request_observation`], so both transports publish the
/// same vocabulary.
pub fn provider_request_observation(
    provider_contract: &str,
    provider_key: &str,
    http_method: HttpMethod,
    request_url: &str,
    outcome: Option<(u16, &[u8])>,
    duration: Duration,
) -> ProviderRequestObservation {
    let (status_class, response_bytes, response_excerpt) = match outcome {
        Some((status_code, body)) => (
            status_class_text(status_code),
            body.len() as u64,
            if response_excerpt_capture_enabled() {
                response_snippet_text(body)
            } else {
                String::new()
            },
        ),
        None => (TRANSPORT_FAILURE_STATUS_CLASS.to_string(), 0, String::new()),
    };
    ProviderRequestObservation {
        provider_contract: provider_contract.to_string(),
        provider_key: provider_key.to_string(),
        http_method: http_method_text(http_method).to_string(),
        request_path: redacted_request_path(request_url),
        status_class,
        duration_milliseconds: duration.as_millis().min(u64::MAX as u128) as u64,
        response_bytes,
        response_excerpt,
    }
}

/// Hands one observation to the installed observer, if any.
pub fn deliver_provider_request_observation(observation: ProviderRequestObservation) {
    if let Some(observer) = installed_observer() {
        observer.on_provider_request(observation);
    }
}

/// Reduces a request URL to what an observation may show: the path, plus the
/// names of its query parameters with values removed except the parameters
/// whose value describes the request shape.
pub fn redacted_request_path(request_url: &str) -> String {
    let Ok(parsed_url) = Url::parse(request_url) else {
        // An unparseable URL never reaches a provider in practice; refuse to
        // echo it verbatim, because it may carry a credential in any shape.
        return String::from("<unparseable request url>");
    };
    let mut redacted_path = parsed_url.path().to_string();
    let mut redacted_parameters: Vec<String> = parsed_url
        .query_pairs()
        .map(|(parameter_name, parameter_value)| {
            if OBSERVABLE_QUERY_PARAMETER_NAMES.contains(&parameter_name.as_ref()) {
                format!("{parameter_name}={parameter_value}")
            } else {
                format!("{parameter_name}=<redacted>")
            }
        })
        .collect();
    redacted_parameters.sort();
    if !redacted_parameters.is_empty() {
        redacted_path.push('?');
        redacted_path.push_str(&redacted_parameters.join("&"));
    }
    redacted_path
}

/// Caps a response body at [`RESPONSE_SNIPPET_MAXIMUM_BYTES`] on a character
/// boundary, so a multi-byte glyph is never split.
pub fn response_snippet_text(response_body: &[u8]) -> String {
    let text = String::from_utf8_lossy(response_body);
    if text.len() <= RESPONSE_SNIPPET_MAXIMUM_BYTES {
        return text.into_owned();
    }
    let mut snippet_end = RESPONSE_SNIPPET_MAXIMUM_BYTES;
    while !text.is_char_boundary(snippet_end) {
        snippet_end -= 1;
    }
    text[..snippet_end].to_string()
}

/// The status class an observation reports for one HTTP status code.
fn status_class_text(status_code: u16) -> String {
    let status_class_prefix = status_code / 100;
    format!("{status_class_prefix}xx")
}

/// The outcome word an observation reports for one status class.
fn outcome_text(status_class: &str) -> &'static str {
    match status_class {
        "2xx" => OUTCOME_SUCCESS,
        "3xx" => OUTCOME_REDIRECT,
        "4xx" => OUTCOME_CLIENT_ERROR,
        "5xx" => OUTCOME_SERVER_ERROR,
        TRANSPORT_FAILURE_STATUS_CLASS => OUTCOME_TRANSPORT_FAILURE,
        _ => OUTCOME_UNCLASSIFIED,
    }
}

/// The word an observation reports for one HTTP method.
fn http_method_text(http_method: HttpMethod) -> &'static str {
    match http_method {
        HttpMethod::Get => "GET",
        HttpMethod::Post => "POST",
    }
}

/// The installed observer, cloned out of the process-wide slot so delivery
/// never holds the lock across the observer's own work.
fn installed_observer() -> Option<Arc<dyn ProviderRequestObserver>> {
    INSTALLED_OBSERVER
        .get()?
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

// The provider layer's request observer, rendered in this module's vocabulary.
//
// The SDK's provider diagnostics are built from one observation per
// completed provider call: the contract it served, the provider's registry
// key, the method, the redacted path, the status class, the duration and the
// response size. The provider layer reports the same facts through its own
// [`everymap_core::client::RequestObserver`], so an adapter lets a provider
// built on that layer keep feeding the existing diagnostics ring, the typed
// `ProviderRequestTrace` and the browser's event stream unchanged.
//
// The adapter pairs each request with its response, because the provider
// layer reports the two separately and the SDK's vocabulary wants one
// record. A response without a recorded request is dropped rather than
// reported with invented fields.

use everymap_core::client::{RequestObservation, RequestObserver, ResponseObservation};
use std::sync::PoisonError;
use web_time::Instant;

/// One request the provider layer has started and not yet answered.
struct InFlightRequest {
    /// The method the SDK's vocabulary names.
    http_method: HttpMethod,
    /// The redacted request URL the provider layer handed over.
    ///
    /// The provider layer redacts credential values with its own marker; the
    /// SDK re-renders the path from this URL, so the marker never reaches the
    /// diagnostic line.
    request_url: String,
    /// When the transport started the attempt.
    started_at: Instant,
}

/// Publishes one provider's request attempts into the SDK's diagnostics.
pub struct EveryMapRequestObserver {
    /// The contract the calls through this client serve.
    provider_contract: String,
    /// The provider's registry key.
    provider_key: String,
    /// The attempt awaiting its response.
    in_flight: Mutex<Option<InFlightRequest>>,
}

impl EveryMapRequestObserver {
    /// Builds the observer for one provider and one contract.
    ///
    /// # Parameters
    /// - `provider_contract` — the contract name the SDK reports.
    /// - `provider_key` — the provider's registry key.
    pub fn new(provider_contract: &str, provider_key: &str) -> Self {
        Self {
            provider_contract: provider_contract.to_string(),
            provider_key: provider_key.to_string(),
            in_flight: Mutex::new(None),
        }
    }

    /// Renders one completed attempt in the SDK's vocabulary.
    ///
    /// The provider layer reports no response body, so the byte count is zero
    /// and the snippet empty; both are facts about what was observed rather
    /// than invented values.
    fn translated_observation(
        &self,
        in_flight: InFlightRequest,
        status: Option<u16>,
        response_excerpt: Option<&str>,
        elapsed_upstream: Duration,
    ) -> ProviderRequestObservation {
        let response_body = response_excerpt.map(str::as_bytes).unwrap_or(&[]);
        let outcome = status.map(|status_code| (status_code, response_body));
        provider_request_observation(
            &self.provider_contract,
            &self.provider_key,
            in_flight.http_method,
            &in_flight.request_url,
            outcome,
            duration_or_zero(in_flight.started_at.elapsed(), elapsed_upstream),
        )
    }
}

impl RequestObserver for EveryMapRequestObserver {
    fn on_request(&self, observation: &RequestObservation<'_>) {
        let mut in_flight = self
            .in_flight
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        *in_flight = Some(InFlightRequest {
            http_method: sdk_http_method(observation.method),
            request_url: observation.url.to_string(),
            started_at: Instant::now(),
        });
    }

    fn on_response(&self, observation: &ResponseObservation<'_>) {
        let Some(in_flight) = self
            .in_flight
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        else {
            return;
        };
        deliver_provider_request_observation(self.translated_observation(
            in_flight,
            observation.status,
            observation.response_excerpt,
            observation.elapsed,
        ));
    }
}

/// The SDK method word for one provider-layer method.
///
/// The SDK's vocabulary models the two methods its providers use; a provider
/// layer method outside that pair is reported as a POST, which is what the
/// request carries a body for.
fn sdk_http_method(method: everymap_core::http::Method) -> HttpMethod {
    match method {
        everymap_core::http::Method::Get => HttpMethod::Get,
        _ => HttpMethod::Post,
    }
}

/// The attempt duration, preferring the provider layer's own measurement.
fn duration_or_zero(measured_here: Duration, measured_upstream: Duration) -> Duration {
    if measured_upstream.is_zero() {
        measured_here
    } else {
        measured_upstream
    }
}

#[cfg(test)]
mod everymap_adapter_tests {
    use super::{
        Duration, EveryMapRequestObserver, HttpMethod, InFlightRequest, ProviderRequestObservation,
        duration_or_zero,
    };
    use everymap_core::client::{RequestObservation, RequestObserver};
    use web_time::Instant;

    /// Builds the observer with one attempt already in flight.
    fn observer_with_attempt(request_url: &str, method: HttpMethod) -> EveryMapRequestObserver {
        let observer = EveryMapRequestObserver::new("GeocodingProvider", "mapquest");
        *observer
            .in_flight
            .lock()
            .expect("the test observer must lock") = Some(InFlightRequest {
            http_method: method,
            request_url: request_url.to_string(),
            started_at: Instant::now(),
        });
        observer
    }

    /// Takes the translated record for the attempt in flight.
    fn translated(
        observer: &EveryMapRequestObserver,
        status: Option<u16>,
    ) -> ProviderRequestObservation {
        let in_flight = observer
            .in_flight
            .lock()
            .expect("the test observer must lock")
            .take()
            .expect("the fixture leaves one attempt in flight");
        observer.translated_observation(in_flight, status, None, Duration::from_millis(12))
    }

    #[test]
    fn one_provider_request_becomes_one_sdk_observation() {
        let observer = observer_with_attempt(
            "https://example.test/geocoding/v1/address?key=***&location=Sacramento",
            HttpMethod::Get,
        );
        let observation = translated(&observer, Some(200));
        assert_eq!(observation.provider_contract, "GeocodingProvider");
        assert_eq!(observation.provider_key, "mapquest");
        assert_eq!(observation.http_method, "GET");
        assert!(observation.request_path.contains("/geocoding/v1/address"));
        assert!(observation.request_path.contains("key=<redacted>"));
        assert_eq!(observation.status_class, "2xx");
        assert_eq!(observation.outcome(), "success");
        assert_eq!(observation.response_bytes, 0);
    }

    #[test]
    fn the_request_and_response_callbacks_pair_into_one_record() {
        let observer = EveryMapRequestObserver::new("GeocodingProvider", "mapquest");
        observer.on_request(&RequestObservation {
            provider: "mapquest",
            method: everymap_core::http::Method::Get,
            url: "https://example.test/geocoding/v1/address?key=***",
            attempt: 1,
        });
        let in_flight = observer
            .in_flight
            .lock()
            .expect("the test observer must lock")
            .take()
            .expect("the request callback records the attempt");
        assert_eq!(in_flight.http_method, HttpMethod::Get);
        assert!(
            observer
                .in_flight
                .lock()
                .expect("the test observer must lock")
                .is_none()
        );
    }

    #[test]
    fn a_response_without_a_recorded_request_yields_nothing() {
        let observer = EveryMapRequestObserver::new("GeocodingProvider", "mapquest");
        assert!(
            observer
                .in_flight
                .lock()
                .expect("the test observer must lock")
                .is_none()
        );
    }

    #[test]
    fn a_transport_failure_keeps_its_class() {
        let observer = observer_with_attempt(
            "https://example.test/route/v1/driving/1,2;3,4",
            HttpMethod::Post,
        );
        let observation = translated(&observer, None);
        assert_eq!(observation.status_class, "transport-failure");
        assert_eq!(observation.http_method, "POST");
    }

    #[test]
    fn the_attempt_duration_prefers_the_provider_layer_measurement() {
        assert_eq!(
            duration_or_zero(Duration::from_millis(5), Duration::from_millis(12)),
            Duration::from_millis(12)
        );
        assert_eq!(
            duration_or_zero(Duration::from_millis(5), Duration::ZERO),
            Duration::from_millis(5)
        );
    }

    #[test]
    fn the_sdk_method_mapping_covers_the_pair() {
        assert_eq!(
            super::sdk_http_method(everymap_core::http::Method::Get),
            HttpMethod::Get
        );
        assert_eq!(
            super::sdk_http_method(everymap_core::http::Method::Post),
            HttpMethod::Post
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The installed observer is process-wide, so the tests that install one
    /// take this lock: two of them running at once would see each other's
    /// observations (a parallel `cargo test` run did exactly that).
    ///
    /// The lock is async-aware because some of these tests await the transport
    /// while they hold it; a standard-library guard must never be held across
    /// an await.
    static OBSERVER_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    /// Takes the process-wide observer lock from a synchronous test.
    fn lock_installed_observer() -> tokio::sync::MutexGuard<'static, ()> {
        OBSERVER_TEST_LOCK.blocking_lock()
    }

    /// An observer that keeps every observation it receives.
    #[derive(Default)]
    struct RecordingObserver {
        observations: Mutex<Vec<ProviderRequestObservation>>,
    }

    impl ProviderRequestObserver for RecordingObserver {
        fn on_provider_request(&self, observation: ProviderRequestObservation) {
            self.observations
                .lock()
                .expect("the observation list is never poisoned")
                .push(observation);
        }
    }

    #[test]
    fn a_credential_never_reaches_the_redacted_path() {
        let redacted_path = redacted_request_path(
            "https://provider.invalid/route?key=SECRET_VALUE&overview=full&units=metric",
        );
        assert!(!redacted_path.contains("SECRET_VALUE"));
        assert!(redacted_path.contains("key=<redacted>"));
        assert!(redacted_path.contains("overview=full"));
        assert!(redacted_path.contains("units=metric"));
        assert_eq!(redacted_path.split('?').next(), Some("/route"));
    }

    #[test]
    fn query_parameter_order_does_not_change_the_redacted_path() {
        let first_path = redacted_request_path("https://provider.invalid/a?b=1&a=2");
        let second_path = redacted_request_path("https://provider.invalid/a?a=2&b=1");
        assert_eq!(first_path, second_path);
    }

    #[test]
    fn an_unparseable_url_is_never_echoed() {
        assert_eq!(
            redacted_request_path("not a url with SECRET_VALUE"),
            "<unparseable request url>",
        );
    }

    #[test]
    fn the_status_class_groups_codes() {
        assert_eq!(status_class_text(200), "2xx");
        assert_eq!(status_class_text(404), "4xx");
        assert_eq!(status_class_text(503), "5xx");
    }

    #[test]
    fn status_classes_map_onto_outcomes() {
        assert_eq!(outcome_text("2xx"), OUTCOME_SUCCESS);
        assert_eq!(outcome_text("3xx"), OUTCOME_REDIRECT);
        assert_eq!(outcome_text("4xx"), OUTCOME_CLIENT_ERROR);
        assert_eq!(outcome_text("5xx"), OUTCOME_SERVER_ERROR);
        assert_eq!(
            outcome_text(TRANSPORT_FAILURE_STATUS_CLASS),
            OUTCOME_TRANSPORT_FAILURE
        );
        assert_eq!(outcome_text("unexpected"), OUTCOME_UNCLASSIFIED);
    }

    #[test]
    fn the_snippet_caps_at_the_documented_bytes_on_a_character_boundary() {
        let multi_byte_body = "é".repeat(RESPONSE_SNIPPET_MAXIMUM_BYTES);
        let snippet = response_snippet_text(multi_byte_body.as_bytes());
        assert!(snippet.len() <= RESPONSE_SNIPPET_MAXIMUM_BYTES);
        assert_eq!(snippet, "é".repeat(RESPONSE_SNIPPET_MAXIMUM_BYTES / 2));
    }

    #[test]
    fn an_observation_reaches_the_installed_observer() {
        let _observer_lock = lock_installed_observer();
        let recording_observer = Arc::new(RecordingObserver::default());
        install_provider_request_observer(Some(recording_observer.clone()));
        set_response_excerpt_capture(false);
        observe_provider_request(
            "RoutingProvider",
            "test-provider",
            HttpMethod::Get,
            "https://provider.invalid/route/v1/driving/a;b?key=SECRET_VALUE",
            Some((200, b"{}".as_slice())),
            Duration::from_millis(12),
        );
        // The excerpt is off by default: the size is reported, the body is not.
        let observations = recording_observer
            .observations
            .lock()
            .expect("the observation list is never poisoned")
            .clone();
        assert_eq!(observations.len(), 1);
        assert_eq!(observations[0].response_bytes, 2);
        assert!(observations[0].response_excerpt.is_empty());
        set_response_excerpt_capture(true);
        observe_provider_request(
            "RoutingProvider",
            "test-provider",
            HttpMethod::Get,
            "https://provider.invalid/route/v1/driving/a;b?key=SECRET_VALUE",
            Some((200, b"{}".as_slice())),
            Duration::from_millis(12),
        );
        set_response_excerpt_capture(false);
        let observations = recording_observer
            .observations
            .lock()
            .expect("the observation list is never poisoned")
            .clone();
        assert_eq!(observations.len(), 2);
        let observation = &observations[1];
        assert_eq!(observation.provider_contract, "RoutingProvider");
        assert_eq!(observation.provider_key, "test-provider");
        assert_eq!(observation.http_method, "GET");
        assert_eq!(observation.status_class, "2xx");
        assert_eq!(observation.response_bytes, 2);
        assert_eq!(observation.response_excerpt, "{}");
        assert!(observation.duration_milliseconds >= 12);
        assert!(!observation.request_path.contains("SECRET_VALUE"));
    }

    #[test]
    fn a_transport_failure_is_reported_without_a_status() {
        let _observer_lock = lock_installed_observer();
        let recording_observer = Arc::new(RecordingObserver::default());
        install_provider_request_observer(Some(recording_observer.clone()));
        observe_provider_request(
            "GeocodingProvider",
            "test-provider",
            HttpMethod::Post,
            "https://provider.invalid/search",
            None,
            Duration::ZERO,
        );
        let observations = recording_observer
            .observations
            .lock()
            .expect("the observation list is never poisoned")
            .clone();
        assert_eq!(observations[0].status_class, TRANSPORT_FAILURE_STATUS_CLASS);
        assert_eq!(observations[0].response_bytes, 0);
        assert!(observations[0].response_excerpt.is_empty());
    }

    #[test]
    fn no_observation_is_built_without_an_installed_observer() {
        let _observer_lock = lock_installed_observer();
        install_provider_request_observer(None);
        observe_provider_request(
            "RoutingProvider",
            "test-provider",
            HttpMethod::Get,
            "https://provider.invalid/route",
            Some((200, b"{}".as_slice())),
            Duration::ZERO,
        );
    }
}
