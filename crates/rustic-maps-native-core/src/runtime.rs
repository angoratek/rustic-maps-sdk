//! The crate-owned async runtime: the executor seam of the exported
//! `[Async]` surface.
//!
//! Native targets drive every exported future through a crate-owned
//! multi-thread tokio runtime (the sections below). The wasm build has no
//! tokio runtime and no `reqwest`: its provider work runs on browser
//! `fetch`, whose futures are single-threaded `wasm-bindgen` futures, so
//! [`drive`] there simply awaits the work on the browser's own task queue.
//!
//! # The driving problem
//!
//! UniFFI's foreign-driven async model lets the foreign executor (Swift's
//! main actor, Kotlin's main dispatcher) poll the exported future directly.
//! That model only holds for futures that never touch a process-wide
//! executor's internals: the MapQuest provider futures run through
//! `reqwest` and `tokio::time` retries, whose first poll requires a live
//! tokio IO and time reactor ("there is no reactor running, must be called
//! from the context of a Tokio 1.x runtime"). A foreign executor has no such
//! reactor, so the first geocode, route or `MapView::draw` call fails there
//! unless the work is handed to a runtime that owns one.
//!
//! # The handback
//!
//! Every exported async method therefore hands its runtime-dependent work
//! to [`drive`]: the work runs on a crate-owned multi-thread tokio runtime
//! (initialised lazily on the first exported async call, with the IO and
//! time drivers enabled), and the returned future — the one the foreign
//! executor polls — only awaits a completion channel, which is
//! runtime-independent by construction. A panicking task resurfaces as the
//! same panic on the poller (the FFI scaffolding catches it into the
//! honest failure report), so the panic semantics of the contract are
//! preserved end to end.

use std::any::Any;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use std::sync::OnceLock;
#[cfg(not(target_arch = "wasm32"))]
use tokio::runtime::{Builder, Runtime};

#[cfg(not(target_arch = "wasm32"))]
use crate::panic_boundary;

/// The worker-thread name the crate-owned runtime's drivers carry, naming
/// the SDK's runtime threads in thread dumps and profilers.
#[cfg(not(target_arch = "wasm32"))]
const RUNTIME_THREAD_NAME: &str = "rustic-maps-native-core-driver";

/// The lazily initialised crate-owned runtime backing every exported async
/// call.
#[cfg(not(target_arch = "wasm32"))]
static RUNTIME: OnceLock<Runtime> = OnceLock::new();

/// Builds the crate-owned runtime: the multi-thread scheduler with every
/// driver enabled, because the provider and draw work relies on the tokio
/// IO and time reactors (`reqwest` connections, retry back-offs).
///
/// The fixed builder configuration is process-usable by construction; its
/// build can only fail on process-wide resource exhaustion, which no
/// exported call can honestly refuse (the panic boundary's
/// internal-invariant case).
#[cfg(not(target_arch = "wasm32"))]
fn runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        panic_boundary::required_invariant_result(
            Builder::new_multi_thread()
                .enable_all()
                .thread_name(RUNTIME_THREAD_NAME)
                .build(),
            "the crate-owned multi-thread tokio runtime is buildable",
        )
    })
}

/// Hands a work future to the crate-owned runtime and returns the
/// runtime-independent future the exported method awaits instead: whatever
/// executor polls the returned future never touches the tokio context the
/// work internally needs.
///
/// The work must be fully owned (`'static`) and `Send`, which the exported
/// methods satisfy by moving cloned provider handles, records and session
/// state into the closure rather than borrowing the exported object.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn drive<WorkOutput>(
    work: impl Future<Output = WorkOutput> + Send + 'static,
) -> impl Future<Output = WorkOutput> + Send
where
    WorkOutput: Send + 'static,
{
    let completion = runtime().spawn(work);
    async move {
        match completion.await {
            Ok(work_output) => work_output,
            // The worker task panicked (aborted tasks do not happen here:
            // nothing aborts the spawned work). Resurface the panic on the
            // poller, where the FFI scaffolding's panic boundary converts
            // it into the same honest failure the contract reports.
            Err(panic_join_error) => {
                if panic_join_error.is_panic() {
                    std::panic::resume_unwind(panic_join_error.into_panic());
                } else {
                    unreachable!("the spawned driver task is never aborted");
                }
            }
        }
    }
}

/// Awaits a work future directly on the wasm build: there is no tokio
/// runtime and no reactor to hand it to. The browser `fetch` futures the
/// provider path produces are single-threaded, so the single-threaded UniFFI
/// runtime path (the `wasm-unstable-single-threaded` feature) polls them on
/// the browser's own task queue.
#[cfg(target_arch = "wasm32")]
pub(crate) async fn drive<WorkOutput>(work: impl Future<Output = WorkOutput>) -> WorkOutput {
    work.await
}

/// The crate-owned executor for blocking work.
///
/// A target with worker threads hands a job (a SQLite lookup or an offline
/// A* search) to the runtime's blocking pool, so the job never occupies a
/// driver thread. A panicking job resurfaces on the poller exactly as
/// [`drive`] resurfacing a panicking task does.
#[cfg(not(target_arch = "wasm32"))]
struct RuntimeBlockingWorkExecutor;

#[cfg(not(target_arch = "wasm32"))]
impl rustic_maps_core::BlockingWorkExecutor for RuntimeBlockingWorkExecutor {
    fn run_blocking(
        &self,
        work: Box<dyn FnOnce() -> Box<dyn Any + Send> + Send + 'static>,
    ) -> Pin<Box<dyn Future<Output = Box<dyn Any + Send>> + Send + 'static>> {
        let completion = runtime().spawn_blocking(work);
        Box::pin(async move {
            match completion.await {
                Ok(output) => output,
                Err(panic_join_error) => {
                    if panic_join_error.is_panic() {
                        std::panic::resume_unwind(panic_join_error.into_panic());
                    } else {
                        unreachable!("the spawned blocking task is never aborted");
                    }
                }
            }
        })
    }
}

/// The browser has no worker threads: the executor runs the job where the
/// returned future is polled, on the single browser task queue.
#[cfg(target_arch = "wasm32")]
struct RuntimeBlockingWorkExecutor;

#[cfg(target_arch = "wasm32")]
impl rustic_maps_core::BlockingWorkExecutor for RuntimeBlockingWorkExecutor {
    fn run_blocking(
        &self,
        work: Box<dyn FnOnce() -> Box<dyn Any + Send> + Send + 'static>,
    ) -> Pin<Box<dyn Future<Output = Box<dyn Any + Send>> + Send + 'static>> {
        Box::pin(async move { work() })
    }
}

/// The blocking-work executor the cache and offline-routing seams share.
#[must_use]
pub(crate) fn blocking_work_executor() -> Arc<dyn rustic_maps_core::BlockingWorkExecutor> {
    Arc::new(RuntimeBlockingWorkExecutor)
}

/// Runs a work future on the crate-owned runtime fire-and-forget: the frame
/// path's detached tile-warm tasks run without any exported
/// future awaiting them, so the `JoinHandle` is dropped immediately. A
/// panicking work task therefore never surfaces to the caller — best-effort
/// semantics by design (the warm fetches retry on a later pass), which is the
/// difference from [`drive`], whose panics resurface on the poller. The panic
/// is not contained: it still reaches the process's panic hook, and it aborts
/// the process under a `panic = "abort"` profile.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn spawn_detached<WorkOutput>(work: impl Future<Output = WorkOutput> + Send + 'static)
where
    WorkOutput: Send + 'static,
{
    drop(runtime().spawn(work));
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::drive;
    use std::time::Duration;
    use tokio::runtime::Builder;

    /// A short sleep proving the time driver; long enough to require a
    /// real reactor, too short to matter.
    const HANDBACK_PROBE_SLEEP: Duration = Duration::from_millis(1);

    #[test]
    fn runtime_dependent_work_completes_when_driven_outside_a_tokio_context() {
        // A current-thread runtime built *without* `enable_*`: it has no
        // reactor, exactly like a foreign executor's polling thread.
        let bare_executor = Builder::new_current_thread().build().unwrap();
        let driven_result = bare_executor.block_on(async {
            drive(async {
                tokio::time::sleep(HANDBACK_PROBE_SLEEP).await;
                42usize
            })
            .await
        });
        assert_eq!(driven_result, 42);
    }

    #[test]
    fn a_panicking_task_surfaces_the_panic_on_the_poller() {
        // A foreign executor cannot catch a Rust panic through its own
        // machinery, so the panic surfaces on the poller's thread
        // (uniffi's panic boundary converts it there); the bare
        // current-thread runtime mimics that polling context.
        let joined = std::thread::spawn(|| {
            let bare_executor = Builder::new_current_thread().build().unwrap();
            bare_executor.block_on(drive(async { panic!("driver work panicked") }))
        })
        .join();
        let payload = joined.expect_err("the surfacing panic must reach the poller");
        let panic_message = *payload.downcast::<&str>().expect("the str panic payload");
        assert_eq!(panic_message, "driver work panicked");
    }

    #[test]
    fn blocking_work_runs_on_a_runtime_blocking_thread() {
        let calling_thread_id = std::thread::current().id();
        let executor = super::blocking_work_executor();
        let bare_executor = Builder::new_current_thread().build().unwrap();
        let (job_thread_id, job_thread_name) =
            bare_executor.block_on(rustic_maps_core::run_blocking(executor.as_ref(), || {
                (
                    std::thread::current().id(),
                    std::thread::current().name().map(str::to_owned),
                )
            }));
        assert_ne!(
            job_thread_id, calling_thread_id,
            "blocking work must leave the polling thread"
        );
        assert!(
            job_thread_name
                .as_deref()
                .unwrap_or_default()
                .starts_with(super::RUNTIME_THREAD_NAME),
            "blocking work runs on the crate-owned runtime's named thread, got {job_thread_name:?}"
        );
    }
}
