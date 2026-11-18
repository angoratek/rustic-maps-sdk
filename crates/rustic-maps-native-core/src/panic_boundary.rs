//! # Panic containment at the FFI edges
//!
//! The task of this module is one decision, written down once instead of
//! forty times: what happens when a Rust panic reaches an FFI edge of this
//! crate — and where the crate takes deliberate panics instead of hidden
//! `unwrap`/`expect` calls.
//!
//! # What UniFFI's own scaffolding already contains (verified against the
//! pinned `uniffi 0.31.2`)
//!
//! Every frame the foreign code enters goes through code UniFFI generated,
//! and UniFFI generated it with a catch boundary already installed:
//!
//! - **Exported synchronous frames** (free functions, methods, constructors,
//!   object handle clone/free): the generated `extern "C"` wrapper bodies
//!   are `::uniffi::rust_call(call_status, || ...)` (the proc-macro
//!   scaffolding of the `export`/`export_for_udl` paths, `uniffi_macros`
//!   `export/scaffolding.rs` and `object.rs`), and `uniffi_core`'s
//!   `ffi::rustcalls::rust_call_with_out_status` wraps that closure in
//!   `panic::catch_unwind`. A caught panic becomes the call's
//!   `RustCallStatusCode::UnexpectedError` (an "unexpected error" status the
//!   foreign bindings report as an unspecified failure, not a typed
//!   `SdkError`), with the panic message lowered into the status' error
//!   buffer; the message lowering itself runs inside a second,
//!   deliberately defensive `catch_unwind`.
//! - **Exported `[Async]` frames**: the exported FFI function does not hand
//!   the future to the foreign executor; `rust_future_new` wraps it in
//!   `uniffi_core`'s `WrappedFuture`, whose every poll runs inside
//!   `try_rust_call` (with `AssertUnwindSafe`, because a panicked future is
//!   never polled again). A poll panic completes the future with the same
//!   unexpected-error status, so the foreign await *resolves* — the handback
//!   never tears — instead of the panic tearing through the executor.
//! - **The crate-owned runtime handback** ([`crate::runtime::drive`]): a
//!   panic in the spawned work is deliberately resurfaced on the poller,
//!   which is exactly the frame the per-poll boundary above catches ([`crate::runtime::drive`]
//!   documents this pairing). A panicking fire-and-forget task
//!   ([`crate::runtime::spawn_detached`]) never crosses any FFI edge at
//!   all: it dies on the crate-owned runtime's worker thread.
//! - **Foreign callbacks invoked from Rust** (the `[Trait, WithForeign]`
//!   contracts): a foreign implementation raising an exception (rather than
//!   throwing the contract's `SdkError`) is not a Rust panic at all — the
//!   foreign vtable reports an unexpected status and `uniffi_core`'s
//!   `LiftReturn` machinery turns it into `UnexpectedUniFFICallbackError`,
//!   which panics the proxy call unless the contract's error type converts
//!   it ([`crate::error::SdkError`] implements that `From`; see there). That
//!   panic, like every other panic inside an exported frame, is caught by
//!   the same scaffolding boundary. Callback contracts whose methods have no
//!   error channel (`NavigationListener`,
//!   `SdkDiagnosticEventListener`) can only take the panic route; that is
//!   fine for the same reason.
//!
//! So: **no Rust panic can unwind into foreign code over UniFFI's edges.**
//! Adding our own `catch_unwind` wrappers at the exported entry points would
//! be redundant containment; mapping a caught internal panic onto a typed
//! `SdkError` variant would be dishonest — every variant names a *cause*
//! (`invalid render input`, `network request failed`, ...) and none of them
//! truthfully names "an internal invariant broke". The scaffolding's
//! `UnexpectedError` status is the contract's honest report for that. The
//! one honest typing addition is
//! [`crate::error::SdkError`]'s conversion of a foreign provider
//! implementation's unexpected failure, which today (without it) degrades
//! through a panic into the same untyped unexpected error.
//!
//! # The remaining deliberate panics: unreachable internal invariants
//!
//! A small class of failure modes in this crate cannot produce an `SdkError`
//! without lying, and cannot happen without a bug in this crate's own code:
//! a mutex that is only ever held over documented panic-free regions is
//! poisoned by an internal invariant breach, and an invariant whose value is
//! provably established by validation a few lines earlier. For those, the
//! helpers below take the panic out of the `unwrap`/`expect` shorthand and
//! into a documented, named choice — a panic that the scaffolding boundary
//! contains, so an internal bug degrades an *exported call* to an unexpected
//! error instead of aborting the process or hiding behind a default value.
//!
//! The workspace clippy config denies `clippy::unwrap_used` and
//! `clippy::expect_used` for the crate's non-test build (see
//! `lib.rs`); the helpers here hold the crate's only justifications.

use std::sync::{Mutex, MutexGuard};

/// Locks a state mutex whose guard region is documented, module by module,
/// to hold only panic-free code: short clone-or-write state snapshots, never
/// a foreign callback, never a `?`-fallible user path. A poisoned lock
/// therefore signals an internal invariant breach, not an expected failure —
/// the panic is deliberate and contained by the FFI scaffolding's panic
/// boundary (see the module documentation).
///
/// `state_lock_name` names the mutex in the panic report (e.g. "the
/// surface view's slot lock"), keeping the contained failure diagnosable
/// without hiding the choice behind a bare `unwrap`.
pub(crate) fn state_lock_guard<'guard, State>(
    state_lock: &'guard Mutex<State>,
    state_lock_name: &'static str,
) -> MutexGuard<'guard, State> {
    match state_lock.lock() {
        Ok(state_guard) => state_guard,
        // Poisoned: the guard region is documented panic-free, so this can
        // only mean an internal invariant breach. Deliberate panic, contained
        // at the exported frame (the module documentation).
        Err(_poisoned_state_lock) => {
            panic!("internal invariant broken: {state_lock_name} is poisoned")
        }
    }
}

/// Takes an established-invariant `Option`'s value, or panics with the
/// invariant's description: for invariants whose establishment is provable
/// by construction (a value validated a few lines earlier, or an infallible
/// reducer event documented by the reducer's own contract). The panic is
/// deliberate and contained by the FFI scaffolding's panic boundary (see the
/// module documentation).
#[track_caller]
pub(crate) fn required_invariant<InvariantValue>(
    invariant_value: Option<InvariantValue>,
    invariant_description: &'static str,
) -> InvariantValue {
    match invariant_value {
        Some(invariant) => invariant,
        None => panic!("internal invariant broken: {invariant_description}"),
    }
}

/// Takes an established-invariant `Result`'s value, or panics with the
/// invariant's description and the failure's verbatim message: for
/// fallible constructions no exported contract call can honestly refuse
/// (a fixed build-the-process-runtime configuration failing means the
/// process itself is unusable). The panic is deliberate and contained by
/// the FFI scaffolding's panic boundary (see the module documentation).
#[cfg(not(target_arch = "wasm32"))]
#[track_caller]
pub(crate) fn required_invariant_result<InvariantValue, InvariantError>(
    invariant_result: Result<InvariantValue, InvariantError>,
    invariant_description: &'static str,
) -> InvariantValue
where
    InvariantError: std::fmt::Display,
{
    match invariant_result {
        Ok(invariant) => invariant,
        // The established-invariant construction failed: report the failure
        // verbatim, deliberately, contained at the exported frame.
        Err(invariant_failure) => {
            panic!("internal invariant broken: {invariant_description}: {invariant_failure}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{required_invariant, required_invariant_result, state_lock_guard};
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::Mutex;

    /// The panic messages the helpers report broken invariants with; the
    /// contained failure's diagnosability is the helpers' whole business.
    const STATE_LOCK_NAME: &str = "the test state lock";
    const INVARIANT_DESCRIPTION: &str = "the test invariant holds";
    const INVARIANT_FAILURE_MESSAGE: &str = "the failure message";

    #[test]
    fn state_lock_guard_returns_the_guard_for_an_healthy_lock() {
        let state_lock = Mutex::new(7u32);
        let state_guard = state_lock_guard(&state_lock, STATE_LOCK_NAME);
        assert_eq!(*state_guard, 7);
    }

    #[test]
    fn state_lock_guard_panics_named_on_a_poisoned_lock() {
        let state_lock = Mutex::new(0u32);
        let _ = catch_unwind(AssertUnwindSafe(|| {
            let _poisoning_guard = state_lock.lock().expect("poisons the test lock");
            panic!("the deliberate poison");
        }));
        let resumed_message = catch_unwind(AssertUnwindSafe(|| {
            let _guard_read = *state_lock_guard(&state_lock, STATE_LOCK_NAME);
        }))
        .expect_err("the poisoned lock panics deliberately")
        .downcast_ref::<String>()
        .expect("the panic payload is a string")
        .clone();
        assert!(
            resumed_message.contains(STATE_LOCK_NAME),
            "the panic names the lock: {resumed_message}"
        );
    }

    #[test]
    fn required_invariant_takes_the_invariant_value() {
        assert_eq!(
            required_invariant(Some("value"), INVARIANT_DESCRIPTION),
            "value"
        );
    }

    #[test]
    fn required_invariant_panics_with_the_description_on_none() {
        let panic_message = catch_unwind(AssertUnwindSafe(|| {
            required_invariant::<bool>(None, INVARIANT_DESCRIPTION);
        }))
        .expect_err("a broken invariant panics deliberately")
        .downcast_ref::<String>()
        .expect("the panic payload is a string")
        .clone();
        assert!(
            panic_message.contains(INVARIANT_DESCRIPTION),
            "the panic names the invariant: {panic_message}"
        );
    }

    #[test]
    fn required_invariant_result_takes_the_invariant_value() {
        assert_eq!(
            required_invariant_result::<i32, String>(Ok(4), INVARIANT_DESCRIPTION),
            4
        );
    }

    #[test]
    fn required_invariant_result_panics_with_the_description_and_the_failure() {
        let panic_message = catch_unwind(AssertUnwindSafe(|| {
            required_invariant_result::<bool, String>(
                Err(INVARIANT_FAILURE_MESSAGE.to_string()),
                INVARIANT_DESCRIPTION,
            );
        }))
        .expect_err("a broken invariant panics deliberately")
        .downcast_ref::<String>()
        .expect("the panic payload is a string")
        .clone();
        assert!(
            panic_message.contains(INVARIANT_DESCRIPTION)
                && panic_message.contains(INVARIANT_FAILURE_MESSAGE),
            "the panic names the invariant and the failure: {panic_message}"
        );
    }
}
