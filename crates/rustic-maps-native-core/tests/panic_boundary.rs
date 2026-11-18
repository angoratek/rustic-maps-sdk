//! The panic-boundary tests (the `unwrap`-hardening slice): the uniffi
//! 0.31.2 scaffolding itself contains every Rust panic that can cross an
//! exported FFI frame — this file pins that containment at the real
//! generated symbols instead of re-implementing a `catch_unwind` layer the
//! scaffolding already provides (see [`rustic_maps::panic_boundary`]).
//!
//! Two pins against the raw scaffolding: a Rust callback panicking inside a
//! **sync** exported frame resolves the call as uniffi's
//! `UnexpectedError` status carrying the panic message (a control call
//! pins the happy path's `Success`), and the crate's addition-only
//! `From<uniffi::UnexpectedUniFFICallbackError> for SdkError` conversion
//! keeps future foreign-callback failures typed rather than untyped. A
//! third pin covers the async seam through [`rustic_maps::runtime`]'s
//! documented handback pairing: a panicking worker task resurfaces its
//! panic on the poller, where the scaffolding's per-poll boundary catches
//! it.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use async_trait::async_trait;
use uniffi::RustCallStatusCode;

use rustic_maps::{
    GeocodingProvider, MapSdk, NavigationListener, NavigationLocationUpdate, Navigator, Place,
    Route, RouteLeg, RouteStep, SdkError, UnitSystem, default_deviation_policy,
    default_step_advance_policy,
};
use rustic_maps::{
    UniFfiTag, uniffi_rustic_maps_fn_method_navigator_update_location as update_location_ffi,
};

/// Latitude, in degrees, of the synthetic single-step fixture (the same
/// neutral mid-latitude convention the navigation fixtures use).
const TEST_ORIGIN_LATITUDE_DEGREES: f64 = 47.0;
/// Longitude, in degrees, of the synthetic fixture origin.
const TEST_ORIGIN_LONGITUDE_DEGREES: f64 = 8.0;
/// Earth's mean radius, in meters, provenance shared with the navigation
/// geometry module's own constant.
const EARTH_RADIUS_METERS: f64 = 6_371_008.8;
/// Length, in meters, of the single fixture step.
const TEST_STEP_LENGTH_METERS: f64 = 100.0;
/// Declared duration, in seconds, of the fixture step.
const TEST_STEP_DURATION_SECONDS: f64 = 10.0;
/// Horizontal accuracy, in meters, of the fixture fix: far better than the
/// default accuracy gate.
const TEST_ACCURACY_METERS: f64 = 2.0;
/// East offset, in meters, of the fixture fix on the route line (mid-step,
/// so the instruction dispatch carries the first step with a distance).
const FIX_EAST_METERS: f64 = TEST_STEP_LENGTH_METERS / 2.0;
/// The panic message the listener raises; the caught-and-lowered call
/// status must carry it verbatim.
const LISTENER_PANIC_MESSAGE: &str = "the listener panicked inside the exported frame";
/// The panic message the panicking async provider callback raises.
const ASYNC_PROVIDER_PANIC_MESSAGE: &str = "the provider callback panicked";
/// The reason text of the unexpected foreign callback failure the mapping
/// pin converts.
const UNEXPECTED_CALLBACK_REASON: &str = "the foreign geocoder failed outside its contract";

/// A fixture coordinate displaced by east/north meter offsets around the
/// fixture origin, the same equirectangular conversion the navigation
/// fixtures use.
fn east_north_coordinate(east_meters: f64, north_meters: f64) -> rustic_maps::GeographicCoordinate {
    rustic_maps::GeographicCoordinate {
        latitude: TEST_ORIGIN_LATITUDE_DEGREES + (north_meters / EARTH_RADIUS_METERS).to_degrees(),
        longitude: TEST_ORIGIN_LONGITUDE_DEGREES
            + (east_meters
                / (EARTH_RADIUS_METERS * TEST_ORIGIN_LATITUDE_DEGREES.to_radians().cos()))
            .to_degrees(),
    }
}

/// Builds the minimal single-step eastward route a trip can start on.
fn single_step_route() -> Route {
    Route {
        distance_meters: TEST_STEP_LENGTH_METERS,
        duration_seconds: TEST_STEP_DURATION_SECONDS,
        bounding_box: rustic_maps::BoundingBox {
            south_west: east_north_coordinate(0.0, 0.0),
            north_east: east_north_coordinate(TEST_STEP_LENGTH_METERS, 0.0),
        },
        geometry: Vec::new(),
        legs: vec![RouteLeg {
            distance_meters: TEST_STEP_LENGTH_METERS,
            duration_seconds: TEST_STEP_DURATION_SECONDS,
            steps: vec![RouteStep {
                distance_meters: TEST_STEP_LENGTH_METERS,
                duration_seconds: TEST_STEP_DURATION_SECONDS,
                maneuver_type: "departure".to_string(),
                instruction_display_text: "Instruction".to_string(),
                instruction_speech_text: "Instruction".to_string(),
                shape: vec![
                    east_north_coordinate(0.0, 0.0),
                    east_north_coordinate(TEST_STEP_LENGTH_METERS, 0.0),
                ],
            }],
        }],
    }
}

/// A graded fix on the route line mid-step, whose update dispatches the
/// topic instruction.
fn route_fix() -> NavigationLocationUpdate {
    NavigationLocationUpdate {
        coordinate: east_north_coordinate(FIX_EAST_METERS, 0.0),
        horizontal_accuracy_meters: Some(TEST_ACCURACY_METERS),
        course_heading_degrees: None,
        timestamp_seconds: None,
    }
}

/// A listener whose instruction callback panics: the stand-in for the
/// foreign implementation failing inside a dispatched topic.
struct PanickingInstructionListener;

impl NavigationListener for PanickingInstructionListener {
    fn on_navigation_instruction(&self, _instruction: rustic_maps::NavigationInstruction) {
        panic!("{LISTENER_PANIC_MESSAGE}");
    }

    fn on_deviation_status_change(
        &self,
        _deviation_status_change: rustic_maps::DeviationStatusChange,
    ) {
    }

    fn on_navigation_phase_change(
        &self,
        _navigation_phase_change: rustic_maps::NavigationPhaseChange,
    ) {
    }

    fn on_speech_instruction(&self, _speech_instruction: rustic_maps::SpeechInstruction) {}
}

/// A geocoding provider callback whose async bodies panic: the stand-in for
/// the foreign implementation failing outside its typed error contract.
struct PanickingGeocodingCallback;

#[async_trait]
impl GeocodingProvider for PanickingGeocodingCallback {
    async fn forward_geocode(&self, _address_text: String) -> Result<Vec<Place>, SdkError> {
        panic!("{ASYNC_PROVIDER_PANIC_MESSAGE}");
    }

    async fn reverse_geocode(
        &self,
        _coordinate: rustic_maps::GeographicCoordinate,
    ) -> Result<Vec<Place>, SdkError> {
        panic!("{ASYNC_PROVIDER_PANIC_MESSAGE}");
    }
}

/// Starts the fixture trip before any listener subscribes, so the trip
/// start's dispatch happens off the test's panicking listener.
fn controller_with_started_trip() -> Arc<Navigator> {
    let controller = Arc::new(Navigator::new());
    controller
        .start_navigation(
            single_step_route(),
            UnitSystem::Metric,
            default_step_advance_policy(),
            default_deviation_policy(),
        )
        .expect("the fixture route and policies must start");
    controller
}

#[test]
fn a_panicking_rust_callback_inside_a_sync_exported_frame_resolves_the_call_status() {
    let controller = controller_with_started_trip();
    let controller_arc = Arc::clone(&controller);
    controller.subscribe_to_navigation_instructions(
        Arc::new(PanickingInstructionListener) as Arc<dyn NavigationListener>
    );

    // Lower the object and the fix record with the scaffolding's own
    // converters, as the foreign bindings do, and call the generated fn
    // symbol directly. The listener's panic must be caught by the
    // scaffolding's `rust_call` boundary: no unwind, an `UnexpectedError`
    // status (2) and the panic message lowered in the status's error
    // buffer.
    // The handle transfers one owned `Arc` across the FFI (its lift
    // consumes it, object.rs's documented ownership contract), so each call
    // takes a fresh clone's handle.
    let controller_handle = uniffi::Handle::from_arc(Arc::clone(&controller_arc));
    let lowered_fix = <NavigationLocationUpdate as uniffi::Lower<UniFfiTag>>::lower(route_fix());
    let mut call_outcome = catch_unwind(AssertUnwindSafe(|| {
        let mut call_status = uniffi::RustCallStatus::default();
        update_location_ffi(controller_handle, lowered_fix, &mut call_status);
        call_status
    }))
    .expect("the scaffolding must catch the callback's panic, not unwind the exported frame");
    assert_eq!(
        call_outcome.code,
        RustCallStatusCode::UnexpectedError,
        "a panicking callback must resolve as the untyped unexpected-error status"
    );
    // SAFETY: `ManuallyDrop::take` moves the call status's error buffer
    // out exactly once; the lifted `String` drains it (the buffers the
    // scaffolding lowers are single-owning handles).
    let lowered_message = <String as uniffi::Lift<UniFfiTag>>::try_lift(unsafe {
        std::mem::ManuallyDrop::take(&mut call_outcome.error_buf)
    })
    .expect("the panic message must be liftable from the call status");
    assert!(
        lowered_message.contains(LISTENER_PANIC_MESSAGE),
        "the resolved status must carry the panic message verbatim: {lowered_message}"
    );

    // The caught panic must not have torn the controller: the same FFI
    // symbol serves a fresh call on the intact object.
    let resumed_handle = uniffi::Handle::from_arc(Arc::clone(&controller_arc));
    let lowered_retry = <NavigationLocationUpdate as uniffi::Lower<UniFfiTag>>::lower(route_fix());
    let mut resumed_call_status = uniffi::RustCallStatus::default();
    update_location_ffi(resumed_handle, lowered_retry, &mut resumed_call_status);
    assert_eq!(
        resumed_call_status.code,
        RustCallStatusCode::UnexpectedError,
        "the still-subscribed panicking listener panics every dispatching call again"
    );
}

#[test]
fn a_calming_rust_callback_serves_the_same_exported_frame_with_success() {
    // The control pin: the same generated symbol, the same lowering path,
    // a listener that does not panic — the call resolves as `Success` (0)
    // with an untouched error buffer. The boundary machinery itself stays
    // out of the way of healthy dispatches.
    struct QuietInstructionListener;

    impl NavigationListener for QuietInstructionListener {
        fn on_navigation_instruction(&self, _instruction: rustic_maps::NavigationInstruction) {}

        fn on_deviation_status_change(
            &self,
            _deviation_status_change: rustic_maps::DeviationStatusChange,
        ) {
        }

        fn on_navigation_phase_change(
            &self,
            _navigation_phase_change: rustic_maps::NavigationPhaseChange,
        ) {
        }

        fn on_speech_instruction(&self, _speech_instruction: rustic_maps::SpeechInstruction) {}
    }

    let controller = controller_with_started_trip();
    let controller_arc = Arc::clone(&controller);
    controller.subscribe_to_navigation_instructions(
        Arc::new(QuietInstructionListener) as Arc<dyn NavigationListener>
    );
    let controller_handle = uniffi::Handle::from_arc(controller_arc);
    let lowered_fix = <NavigationLocationUpdate as uniffi::Lower<UniFfiTag>>::lower(route_fix());
    let mut call_status = uniffi::RustCallStatus::default();
    update_location_ffi(controller_handle, lowered_fix, &mut call_status);
    assert_eq!(call_status.code, RustCallStatusCode::Success);
    assert!(
        call_status.error_buf.is_empty(),
        "a successful call must leave the error buffer empty"
    );
}

#[test]
fn a_panicking_async_provider_callback_resurfaces_the_panic_on_the_poller() {
    // The async seam's pairing ([`rustic_maps::runtime`]): the panicking
    // callback runs on the crate-owned runtime's worker; `drive`'s handback
    // resurfaces the panic on the poller — here the test's executor, and on
    // a real device the foreign poller — where the scaffolding's per-poll
    // boundary catches it into the same unexpected-error report. The test
    // pins the resurfacing (the runtime's own unit tests pin the pairing's
    // channel mechanics).
    let map_sdk = MapSdk::new();
    map_sdk.register_geocoding_provider_from_foreign(Arc::new(PanickingGeocodingCallback));
    // A bare current-thread executor mimics the foreign poller: the handback
    // future itself is runtime-independent (see `runtime`), so the exported
    // future awaits it fine outside any tokio context. The await runs on a
    // dedicated thread because catching a panic's payload through `join`
    // keeps the resurfaced payload readable (the same pattern the `runtime`
    // unit test uses).
    let poll_outcome = std::thread::spawn(move || {
        let bare_executor = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("the probe executor builds");
        catch_unwind(AssertUnwindSafe(|| {
            bare_executor
                .block_on(map_sdk.forward_geocode("any address the callback panics on".to_string()))
        }))
    })
    .join()
    .expect("the probe thread itself must not fail")
    .expect_err("the panicking callback must surface its panic on the poller");
    let panic_message = match poll_outcome.downcast_ref::<String>() {
        Some(owned_message) => owned_message.clone(),
        None => match poll_outcome.downcast_ref::<&'static str>() {
            Some(borrowed_message) => (*borrowed_message).to_string(),
            None => panic!("the panic payload must be a string: {poll_outcome:?}"),
        },
    };
    assert!(
        panic_message.contains(ASYNC_PROVIDER_PANIC_MESSAGE),
        "the resurfaced panic must carry the callback's message verbatim: {panic_message}"
    );
}

#[test]
fn an_unclassified_foreign_callback_failure_maps_onto_the_typed_variant() {
    // The addition-only conversion the hardening added: a foreign provider
    // implementation failing outside its typed `SdkError` contract is
    // reported by the scaffolding as an unexpected callback failure; with
    // the conversion the provider contracts' exported futures resolve
    // typed (the closest existing variant names the provider) instead of
    // the scaffolding's honest-but-untyped panic route.
    let mapped: SdkError =
        uniffi::UnexpectedUniFFICallbackError::new(UNEXPECTED_CALLBACK_REASON).into();
    match mapped {
        SdkError::ProviderRequestRejected { detail } => {
            assert!(
                detail.contains(UNEXPECTED_CALLBACK_REASON),
                "the typed failure must carry the foreign reason verbatim: {detail}"
            );
            assert!(
                detail.contains("the provider implementation failed without a mapped error"),
                "the typed failure must state its own seam: {detail}"
            );
        }
        other_failure => {
            panic!("the mapping must land on the provider variant, got {other_failure:?}")
        }
    }

    // And the typed contract round trips are untouched by the seam (the
    // mapping only engages for the unclassified foreign failures).
    let typed_failure = SdkError::RateLimitExhausted;
    let core_failure: geospatial_types::SDKError = typed_failure.into();
    let restored: SdkError = core_failure.into();
    assert_eq!(
        restored,
        SdkError::RateLimitExhausted,
        "typed failures must keep their exact variant"
    );
}
