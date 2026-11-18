//! The executor seam for blocking work.
//!
//! Some SDK work is synchronous by nature: the response store that backs the
//! cache-then-network policy reads and writes SQLite, and the offline routing
//! engine runs a CPU-bound graph search. Running either on the async reactor
//! monopolizes the thread that drives every other future.
//!
//! [`BlockingWorkExecutor`] is the seam that moves such a job to where
//! blocking is acceptable. A target with worker threads (the native bindings)
//! implements it over a blocking thread pool; a target without threads (the
//! browser) uses [`InlineBlockingWorkExecutor`], which runs the job where it is
//! awaited. The cache and routing layers depend only on the trait, so neither
//! names an executor implementation.

use std::any::Any;
use std::future::Future;
use std::pin::Pin;

/// Runs one owned blocking job away from the async reactor.
///
/// Implementations must not run `work` before the returned future is polled;
/// they decide where it ultimately executes.
pub trait BlockingWorkExecutor: Send + Sync {
    /// Runs `work` and returns a future that resolves to its output.
    ///
    /// The closure and its output are owned (`'static`) because a threaded
    /// implementation hands them to a worker that outlives the caller's
    /// borrow. The erased output is downcast back to its concrete type by
    /// [`run_blocking`], which only this module calls.
    fn run_blocking(
        &self,
        work: Box<dyn FnOnce() -> Box<dyn Any + Send> + Send + 'static>,
    ) -> Pin<Box<dyn Future<Output = Box<dyn Any + Send>> + Send + 'static>>;
}

/// Runs `work` through `executor` and returns its typed output.
///
/// The type erasure the trait needs for object safety stays confined here: the
/// closure and the executor agree on the output type, so the downcast cannot
/// fail.
///
/// # Parameters
/// - `executor` — where blocking is acceptable for this job.
/// - `work` — the owned blocking job.
///
/// # Returns
/// The job's output.
pub async fn run_blocking<WorkOutput>(
    executor: &dyn BlockingWorkExecutor,
    work: impl FnOnce() -> WorkOutput + Send + 'static,
) -> WorkOutput
where
    WorkOutput: Send + 'static,
{
    let boxed_output = executor
        .run_blocking(Box::new(move || Box::new(work()) as Box<dyn Any + Send>))
        .await;
    *boxed_output.downcast::<WorkOutput>().unwrap_or_else(|_| {
        unreachable!("the blocking executor returns the work closure's own output")
    })
}

/// Runs blocking work inline, on the thread that polls the future.
///
/// This is the browser posture: WebAssembly has no worker threads, so the
/// synchronous store calls and the offline search run on the single browser
/// task queue, exactly where they ran before the seam existed.
#[derive(Debug, Default, Clone, Copy)]
pub struct InlineBlockingWorkExecutor;

impl BlockingWorkExecutor for InlineBlockingWorkExecutor {
    fn run_blocking(
        &self,
        work: Box<dyn FnOnce() -> Box<dyn Any + Send> + Send + 'static>,
    ) -> Pin<Box<dyn Future<Output = Box<dyn Any + Send>> + Send + 'static>> {
        Box::pin(async move { work() })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::{BlockingWorkExecutor, InlineBlockingWorkExecutor, run_blocking};

    /// Executor proving the seam routes the job through the implementation
    /// rather than running it at the call site.
    #[derive(Default)]
    struct CountingExecutor {
        run_count: AtomicUsize,
    }

    impl BlockingWorkExecutor for CountingExecutor {
        fn run_blocking(
            &self,
            work: Box<dyn FnOnce() -> Box<dyn std::any::Any + Send> + Send + 'static>,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Box<dyn std::any::Any + Send>> + Send + 'static>,
        > {
            self.run_count.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move { work() })
        }
    }

    #[tokio::test]
    async fn run_blocking_returns_the_typed_output_through_the_executor() {
        let executor = CountingExecutor::default();
        let output = run_blocking(&executor, || 21usize * 2).await;
        assert_eq!(output, 42);
        assert_eq!(executor.run_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn the_inline_executor_runs_the_job_on_the_polling_thread() {
        let inline_executor = InlineBlockingWorkExecutor;
        let polling_thread_id = std::thread::current().id();
        let job_thread_id = run_blocking(&inline_executor, || std::thread::current().id()).await;
        assert_eq!(job_thread_id, polling_thread_id);
    }
}
