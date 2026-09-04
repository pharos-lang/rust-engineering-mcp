//! Admission follows actual blocking work, not the lifetime of its async waiter.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use rust_engineering_application::ExecutionCancellation;
use rust_engineering_application::{OperationControl, ProjectError};
use rust_engineering_domain::OperationalErrorCode;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub(super) struct Workers {
    slots: Arc<Semaphore>,
    session: CancellationToken,
    panicked: Arc<AtomicBool>,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum WorkerError {
    Busy,
    Cancelled,
    TimedOut,
    Internal,
}

/// The caller retains the gateway result even when interruption was observed.
/// Cleanup errors must take precedence over mapping `interrupted` to a status.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Joined<T, E> {
    pub(super) result: Result<T, E>,
    /// Snapshot after work returns, before the async waiter's drop guard fires.
    /// Only Cancelled/TimedOut occur here; this does not claim which signal won
    /// a race earlier in the operation. Control retains its cancellation-first
    /// precedence when cancellation and deadline are both observable.
    pub(super) interrupted: Option<WorkerError>,
}

/// The SDK/runtime may discard a detached JoinHandle's panic result. Publish the
/// failure while unwinding, before the worker's admission permit is released.
struct PanicLatch(Arc<AtomicBool>);
impl Drop for PanicLatch {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.0.store(true, Ordering::Release);
        }
    }
}

pub(super) struct Control {
    request: CancellationToken,
    session: CancellationToken,
    local: CancellationToken,
    deadline: Instant,
}

impl Control {
    fn check_worker(&self) -> Result<(), WorkerError> {
        if self.request.is_cancelled() || self.session.is_cancelled() || self.local.is_cancelled() {
            Err(WorkerError::Cancelled)
        } else if Instant::now() >= self.deadline {
            Err(WorkerError::TimedOut)
        } else {
            Ok(())
        }
    }
}

impl OperationControl for Control {
    fn check(&self) -> Result<(), ProjectError> {
        self.check_worker().map_err(|error| match error {
            WorkerError::TimedOut => ProjectError::Rejected(OperationalErrorCode::CommandTimeout),
            _ => ProjectError::Cancelled,
        })
    }
}

impl ExecutionCancellation for Control {
    fn is_cancelled(&self) -> bool {
        self.check_worker().is_err()
    }
}

impl Workers {
    pub(super) fn cancellation(&self) -> CancellationToken {
        self.session.clone()
    }

    pub(super) fn new() -> Self {
        Self {
            slots: Arc::new(Semaphore::new(1)),
            session: CancellationToken::new(),
            panicked: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(super) async fn run<T, E, F>(
        &self,
        request: CancellationToken,
        deadline: Instant,
        work: F,
    ) -> Result<Result<T, E>, WorkerError>
    where
        T: Send + 'static,
        E: Send + 'static,
        F: FnOnce(&Control) -> Result<T, E> + Send + 'static,
    {
        let local = CancellationToken::new();
        let _cancel_on_drop = local.clone().drop_guard();
        let control = Control {
            request: request.clone(),
            session: self.session.clone(),
            local,
            deadline,
        };
        control.check_worker()?;
        let permit = self.admit()?;
        let panicked = Arc::clone(&self.panicked);
        let job = tokio::task::spawn_blocking(move || {
            // A cancelled/aborted async request must not release this slot while
            // kernel I/O or gateway cleanup continues on the blocking thread.
            let _permit = permit;
            // Reverse drop order publishes a panic before returning capacity.
            let _panic = PanicLatch(panicked);
            control.check_worker()?;
            Ok(work(&control))
        });
        tokio::select! {
            biased;
            _ = request.cancelled() => Err(WorkerError::Cancelled),
            _ = self.session.cancelled() => Err(WorkerError::Cancelled),
            _ = tokio::time::sleep_until(deadline.into()) => Err(WorkerError::TimedOut),
            result = job => result.map_err(|_| WorkerError::Internal)?,
        }
    }

    /// Wait for actual completion, including synchronous gateway cleanup.
    /// Dropping this future still signals local cancellation without aborting
    /// the blocking task or releasing its admission permit.
    pub(super) async fn run_joined<T, E, F>(
        &self,
        request: CancellationToken,
        deadline: Instant,
        work: F,
    ) -> Result<Joined<T, E>, WorkerError>
    where
        T: Send + 'static,
        E: Send + 'static,
        F: FnOnce(&Control) -> Result<T, E> + Send + 'static,
    {
        let local = CancellationToken::new();
        let _cancel_on_drop = local.clone().drop_guard();
        let control = Control {
            request,
            session: self.session.clone(),
            local,
            deadline,
        };
        control.check_worker()?;
        let permit = self.admit()?;
        let panicked = Arc::clone(&self.panicked);
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let _panic = PanicLatch(panicked);
            control.check_worker()?;
            let result = work(&control);
            let interrupted = control.check_worker().err();
            Ok(Joined {
                result,
                interrupted,
            })
        })
        .await
        .map_err(|_| WorkerError::Internal)?
    }

    fn admit(&self) -> Result<OwnedSemaphorePermit, WorkerError> {
        if self.panicked.load(Ordering::Acquire) {
            return Err(WorkerError::Internal);
        }
        let permit = Arc::clone(&self.slots)
            .try_acquire_owned()
            .map_err(|_| WorkerError::Busy)?;
        // The prior worker may have panicked between the first check and our
        // acquisition. Its latch is published before its permit is released.
        if self.panicked.load(Ordering::Acquire) {
            return Err(WorkerError::Internal);
        }
        Ok(permit)
    }

    /// Cancellation is a request; only reacquiring the slot witnesses completion.
    pub(super) async fn shutdown(&self, grace: Duration) -> bool {
        self.session.cancel();
        let drained = matches!(
            tokio::time::timeout(grace, Arc::clone(&self.slots).acquire_owned()).await,
            Ok(Ok(_))
        );
        drained && !self.panicked.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime() -> std::io::Result<tokio::runtime::Runtime> {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
    }

    #[test]
    fn cancelled_waiter_retains_capacity_until_real_completion()
    -> Result<(), Box<dyn std::error::Error>> {
        runtime()?.block_on(async {
            let workers = Workers::new();
            let request = CancellationToken::new();
            let (started_tx, started_rx) = tokio::sync::oneshot::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            let task_workers = workers.clone();
            let task_request = request.clone();
            let task = tokio::spawn(async move {
                task_workers
                    .run(
                        task_request,
                        Instant::now() + Duration::from_secs(5),
                        move |control| {
                            let _ = started_tx.send(());
                            release_rx
                                .recv_timeout(Duration::from_secs(5))
                                .map_err(|_| ())?;
                            assert!(ExecutionCancellation::is_cancelled(control));
                            Ok::<_, ()>(())
                        },
                    )
                    .await
            });
            started_rx.await?;
            request.cancel();
            assert_eq!(task.await?, Err(WorkerError::Cancelled));
            assert_eq!(
                workers
                    .run(
                        CancellationToken::new(),
                        Instant::now() + Duration::from_secs(1),
                        |_| Ok::<_, ()>(())
                    )
                    .await,
                Err(WorkerError::Busy)
            );
            assert!(!workers.shutdown(Duration::from_millis(5)).await);
            release_tx.send(())?;
            assert!(workers.shutdown(Duration::from_secs(1)).await);
            Ok(())
        })
    }

    #[test]
    fn aborted_waiter_and_session_shutdown_signal_the_worker()
    -> Result<(), Box<dyn std::error::Error>> {
        runtime()?.block_on(async {
            for abort in [false, true] {
                let workers = Workers::new();
                let task_workers = workers.clone();
                let (started_tx, started_rx) = tokio::sync::oneshot::channel();
                let (stopped_tx, stopped_rx) = tokio::sync::oneshot::channel();
                let task = tokio::spawn(async move {
                    task_workers
                        .run(
                            CancellationToken::new(),
                            Instant::now() + Duration::from_secs(5),
                            move |control| {
                                let _ = started_tx.send(());
                                while !ExecutionCancellation::is_cancelled(control) {
                                    std::thread::yield_now();
                                }
                                let _ = stopped_tx.send(());
                                Ok::<_, ()>(())
                            },
                        )
                        .await
                });
                started_rx.await?;
                if abort {
                    task.abort();
                    tokio::time::timeout(Duration::from_secs(1), stopped_rx).await??;
                    assert!(workers.shutdown(Duration::from_secs(1)).await);
                } else {
                    assert!(workers.shutdown(Duration::from_secs(1)).await);
                    tokio::time::timeout(Duration::from_secs(1), stopped_rx).await??;
                }
                let result = task.await;
                if abort {
                    assert!(result.is_err());
                } else {
                    assert_eq!(result?, Err(WorkerError::Cancelled));
                }
            }
            Ok(())
        })
    }

    #[test]
    fn expired_cancelled_and_closed_admission_never_invoke_work()
    -> Result<(), Box<dyn std::error::Error>> {
        runtime()?.block_on(async {
            let workers = Workers::new();
            let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            for case in 0..3 {
                let request = CancellationToken::new();
                if case == 1 {
                    request.cancel();
                }
                if case == 2 {
                    assert!(workers.shutdown(Duration::from_secs(1)).await);
                }
                let deadline = if case == 0 {
                    Instant::now()
                } else {
                    Instant::now() + Duration::from_secs(1)
                };
                let counter = Arc::clone(&calls);
                let expected = if case == 0 {
                    WorkerError::TimedOut
                } else {
                    WorkerError::Cancelled
                };
                assert_eq!(
                    workers
                        .run(request, deadline, move |_| {
                            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                            Ok::<_, ()>(())
                        })
                        .await,
                    Err(expected)
                );
            }
            assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
            Ok(())
        })
    }

    #[test]
    fn deadline_does_not_release_a_still_running_worker() -> Result<(), Box<dyn std::error::Error>>
    {
        runtime()?.block_on(async {
            let workers = Workers::new();
            let task_workers = workers.clone();
            let (started_tx, started_rx) = tokio::sync::oneshot::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            let task = tokio::spawn(async move {
                task_workers
                    .run(
                        CancellationToken::new(),
                        Instant::now() + Duration::from_secs(1),
                        move |control| {
                            let _ = started_tx.send(());
                            release_rx
                                .recv_timeout(Duration::from_secs(5))
                                .map_err(|_| ())?;
                            assert!(ExecutionCancellation::is_cancelled(control));
                            Ok::<_, ()>(())
                        },
                    )
                    .await
            });
            started_rx.await?;
            assert_eq!(task.await?, Err(WorkerError::TimedOut));
            assert_eq!(
                workers
                    .run(
                        CancellationToken::new(),
                        Instant::now() + Duration::from_secs(1),
                        |_| Ok::<_, ()>(())
                    )
                    .await,
                Err(WorkerError::Busy)
            );
            release_tx.send(())?;
            assert!(workers.shutdown(Duration::from_secs(1)).await);
            Ok(())
        })
    }

    #[derive(Debug, PartialEq, Eq)]
    enum GatewayFailure {
        CleanupUncertain,
    }

    #[test]
    fn joined_preserves_cleanup_error_and_waits_after_cancel_or_deadline()
    -> Result<(), Box<dyn std::error::Error>> {
        runtime()?.block_on(async {
            for expire in [false, true] {
                let workers = Workers::new();
                let request = CancellationToken::new();
                let task_workers = workers.clone();
                let task_request = request.clone();
                let (started_tx, started_rx) = tokio::sync::oneshot::channel();
                let (cleanup_tx, cleanup_rx) = tokio::sync::oneshot::channel();
                let (release_tx, release_rx) = std::sync::mpsc::channel();
                let task = tokio::spawn(async move {
                    task_workers
                        .run_joined(
                            task_request,
                            Instant::now() + Duration::from_secs(if expire { 1 } else { 5 }),
                            move |control| {
                                let _ = started_tx.send(());
                                while !ExecutionCancellation::is_cancelled(control) {
                                    std::thread::yield_now();
                                }
                                let _ = cleanup_tx.send(());
                                let _ = release_rx.recv_timeout(Duration::from_secs(5));
                                Err::<(), _>(GatewayFailure::CleanupUncertain)
                            },
                        )
                        .await
                });
                started_rx.await?;
                if !expire {
                    request.cancel();
                }
                tokio::time::timeout(Duration::from_secs(2), cleanup_rx).await??;
                assert!(!task.is_finished(), "joined waiter returned before cleanup");
                assert_eq!(
                    workers
                        .run(
                            CancellationToken::new(),
                            Instant::now() + Duration::from_secs(1),
                            |_| Ok::<_, ()>(()),
                        )
                        .await,
                    Err(WorkerError::Busy)
                );
                release_tx.send(())?;
                assert_eq!(
                    task.await?,
                    Ok(Joined {
                        result: Err(GatewayFailure::CleanupUncertain),
                        interrupted: Some(if expire {
                            WorkerError::TimedOut
                        } else {
                            WorkerError::Cancelled
                        }),
                    })
                );
                assert!(workers.shutdown(Duration::from_secs(1)).await);
            }
            Ok(())
        })
    }

    #[test]
    fn joined_success_does_not_observe_its_own_waiter_drop_as_cancellation()
    -> Result<(), Box<dyn std::error::Error>> {
        runtime()?.block_on(async {
            let workers = Workers::new();
            assert_eq!(
                workers
                    .run_joined(
                        CancellationToken::new(),
                        Instant::now() + Duration::from_secs(1),
                        |_| Ok::<_, ()>(42),
                    )
                    .await,
                Ok(Joined {
                    result: Ok(42),
                    interrupted: None
                })
            );
            assert!(workers.shutdown(Duration::from_secs(1)).await);
            Ok(())
        })
    }

    #[test]
    fn abandoning_joined_waiter_signals_cancellation_but_shutdown_waits_for_cleanup()
    -> Result<(), Box<dyn std::error::Error>> {
        runtime()?.block_on(async {
            let workers = Workers::new();
            let task_workers = workers.clone();
            let (started_tx, started_rx) = tokio::sync::oneshot::channel();
            let (cleanup_tx, cleanup_rx) = tokio::sync::oneshot::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            let task = tokio::spawn(async move {
                task_workers
                    .run_joined(
                        CancellationToken::new(),
                        Instant::now() + Duration::from_secs(5),
                        move |control| {
                            let _ = started_tx.send(());
                            while !ExecutionCancellation::is_cancelled(control) {
                                std::thread::yield_now();
                            }
                            let _ = cleanup_tx.send(());
                            release_rx
                                .recv_timeout(Duration::from_secs(5))
                                .map_err(|_| ())?;
                            Ok::<_, ()>(())
                        },
                    )
                    .await
            });
            started_rx.await?;
            task.abort();
            assert!(task.await.is_err());
            tokio::time::timeout(Duration::from_secs(1), cleanup_rx).await??;
            assert!(!workers.shutdown(Duration::from_millis(5)).await);
            release_tx.send(())?;
            assert!(workers.shutdown(Duration::from_secs(1)).await);
            Ok(())
        })
    }

    #[test]
    fn detached_worker_panic_latches_failure_for_both_wait_modes()
    -> Result<(), Box<dyn std::error::Error>> {
        runtime()?.block_on(async {
            for joined in [false, true] {
                let workers = Workers::new();
                let task_workers = workers.clone();
                let (started_tx, started_rx) = tokio::sync::oneshot::channel();
                let (panic_tx, panic_rx) = std::sync::mpsc::channel();
                let task = tokio::spawn(async move {
                    let work = move |_: &Control| -> Result<(), ()> {
                        let _ = started_tx.send(());
                        let _ = panic_rx.recv_timeout(Duration::from_secs(5));
                        // Deliberate unwind fixture, caught only by Tokio's own
                        // blocking task boundary. Production does not normalize it.
                        std::panic::resume_unwind(Box::new("synthetic worker failure"));
                    };
                    if joined {
                        task_workers
                            .run_joined(
                                CancellationToken::new(),
                                Instant::now() + Duration::from_secs(5),
                                work,
                            )
                            .await
                            .map(|result| result.result)
                    } else {
                        task_workers
                            .run(
                                CancellationToken::new(),
                                Instant::now() + Duration::from_secs(5),
                                work,
                            )
                            .await
                    }
                });
                started_rx.await?;
                task.abort();
                assert!(task.await.is_err());
                panic_tx.send(())?;
                // Observe actual worker release independently of shutdown's
                // boolean, proving failure is the latch rather than a timeout.
                let permit = tokio::time::timeout(
                    Duration::from_secs(1),
                    Arc::clone(&workers.slots).acquire_owned(),
                )
                .await??;
                assert!(workers.panicked.load(Ordering::Acquire));
                drop(permit);
                let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
                let counter = calls.clone();
                assert_eq!(
                    workers
                        .run(
                            CancellationToken::new(),
                            Instant::now() + Duration::from_secs(1),
                            move |_| {
                                counter.fetch_add(1, Ordering::SeqCst);
                                Ok::<_, ()>(())
                            },
                        )
                        .await,
                    Err(WorkerError::Internal)
                );
                let counter = calls.clone();
                assert_eq!(
                    workers
                        .run_joined(
                            CancellationToken::new(),
                            Instant::now() + Duration::from_secs(1),
                            move |_| {
                                counter.fetch_add(1, Ordering::SeqCst);
                                Ok::<_, ()>(())
                            },
                        )
                        .await,
                    Err(WorkerError::Internal)
                );
                assert_eq!(calls.load(Ordering::SeqCst), 0);
                assert!(!workers.shutdown(Duration::from_secs(1)).await);
                assert!(!workers.shutdown(Duration::from_secs(1)).await);
            }
            Ok(())
        })
    }
}
