//! Admission follows actual blocking work, not the lifetime of its async waiter.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use rust_engineering_application::job::{CleanupObservation, JobPermit, JobSignal};
use rust_engineering_application::{
    ExecutionCancellation, ExecutionError, InspectionError, OperationControl, ProjectError,
};
use rust_engineering_domain::OperationalErrorCode;
use rust_engineering_domain::job::Milliseconds;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;

tokio::task_local! {
    static NEGOTIATED_TASKS: bool;
    static ADMITTED_JOB_PERMIT: Arc<JobWorkerPermit>;
    static JOB_EXECUTION: bool;
    static REJECT_JOB_ADMISSION: bool;
}

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

pub(super) struct JobWorkerPermit {
    permit: std::sync::Mutex<Option<OwnedSemaphorePermit>>,
    registry_owned: bool,
}

pub(super) struct RegistryJobPermit(Arc<JobWorkerPermit>);

/// Request-independent signal owned by a background job. Queueing a task seed
/// severs the rmcp request cancellation token; only task control, watchdog or
/// shutdown call `request_cancellation` on this signal.
// Kept in production code for M3-02 Tasks activation; M3-01 qualifies its rmcp
// cancellation separation without advertising Tasks capability.
#[allow(dead_code)]
pub(super) struct JobExecutionSignal {
    cancellation: CancellationToken,
    cleanup: std::sync::Mutex<bool>,
    cleanup_ready: std::sync::Condvar,
}

#[allow(dead_code)]
impl JobExecutionSignal {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            cancellation: CancellationToken::new(),
            cleanup: std::sync::Mutex::new(false),
            cleanup_ready: std::sync::Condvar::new(),
        })
    }
    pub(super) fn cancellation(&self) -> CancellationToken {
        self.cancellation.clone()
    }
    pub(super) fn observe_cleanup(&self) {
        if let Ok(mut observed) = self.cleanup.lock() {
            *observed = true;
            self.cleanup_ready.notify_all();
        }
    }
}

impl JobSignal for JobExecutionSignal {
    fn request_cancellation(&self) {
        self.cancellation.cancel();
    }
    fn cancellation_requested(&self) -> bool {
        self.cancellation.is_cancelled()
    }
    fn cleanup_observed(&self) -> bool {
        self.cleanup.lock().is_ok_and(|observed| *observed)
    }
    fn join_cleanup(&self, timeout: Milliseconds) -> CleanupObservation {
        let Ok(observed) = self.cleanup.lock() else {
            return CleanupObservation::Uncertain;
        };
        if *observed {
            return CleanupObservation::Observed;
        }
        let waited = self
            .cleanup_ready
            .wait_timeout(observed, Duration::from_millis(timeout.0));
        match waited {
            Ok((observed, _)) if *observed => CleanupObservation::Observed,
            _ => CleanupObservation::Uncertain,
        }
    }
}

impl JobPermit for JobWorkerPermit {
    fn is_held(&self) -> bool {
        self.permit.lock().is_ok_and(|permit| permit.is_some())
    }

    fn release_after_cleanup(&self) {
        if self.registry_owned {
            return;
        }
        if let Ok(mut permit) = self.permit.lock() {
            permit.take();
        }
    }
}

impl JobPermit for RegistryJobPermit {
    fn is_held(&self) -> bool {
        self.0.is_held()
    }

    fn release_after_cleanup(&self) {
        if let Ok(mut permit) = self.0.permit.lock() {
            permit.take();
        }
    }
}

pub(super) async fn with_negotiated_tasks<T>(declared: bool, future: impl Future<Output = T>) -> T {
    NEGOTIATED_TASKS.scope(declared, future).await
}

pub(super) fn negotiated_tasks(fallback: bool) -> bool {
    NEGOTIATED_TASKS
        .try_with(|declared| *declared)
        .unwrap_or(fallback)
}

pub(super) fn executing_admitted_job() -> bool {
    JOB_EXECUTION.try_with(|value| *value).unwrap_or(false)
}

pub(super) async fn with_admitted_job<T>(
    permit: Arc<JobWorkerPermit>,
    future: impl Future<Output = T>,
) -> T {
    JOB_EXECUTION
        .scope(true, ADMITTED_JOB_PERMIT.scope(permit, future))
        .await
}

pub(super) async fn with_job_execution_selection<T>(future: impl Future<Output = T>) -> T {
    JOB_EXECUTION.scope(true, future).await
}

#[allow(dead_code)]
pub(super) async fn with_rejected_job_admission<T>(future: impl Future<Output = T>) -> T {
    REJECT_JOB_ADMISSION.scope(true, future).await
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

    /// Reserve the same ADR-030 permit for a registry-owned job. The returned
    /// guard is stored by `JobExecutor` and cannot release capacity until that
    /// executor observes joined cleanup.
    #[allow(dead_code)]
    pub(super) fn admit_job(&self) -> Result<Arc<JobWorkerPermit>, WorkerError> {
        if REJECT_JOB_ADMISSION
            .try_with(|rejected| *rejected)
            .unwrap_or(false)
        {
            return Err(WorkerError::Busy);
        }
        if let Ok(permit) = ADMITTED_JOB_PERMIT.try_with(Arc::clone) {
            return Ok(permit);
        }
        Ok(Arc::new(JobWorkerPermit {
            permit: std::sync::Mutex::new(Some(self.admit()?)),
            registry_owned: false,
        }))
    }

    /// Reserve the one ADR-030 slot for an asynchronous registry-owned job.
    /// The ordinary permit handed to the tool cannot release this reservation;
    /// only the companion `JobPermit` stored by `JobExecutor` can do so after
    /// joined cleanup is observed.
    pub(super) fn reserve_job(
        &self,
    ) -> Result<(Arc<JobWorkerPermit>, Arc<dyn JobPermit>), WorkerError> {
        let permit = Arc::new(JobWorkerPermit {
            permit: std::sync::Mutex::new(Some(self.admit()?)),
            registry_owned: true,
        });
        let registry: Arc<dyn JobPermit> = Arc::new(RegistryJobPermit(Arc::clone(&permit)));
        Ok((permit, registry))
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

    /// Execute the job that already owns the sole ADR-030 permit. The permit
    /// remains registry-owned after the joined work returns; only the job
    /// executor may release it after it observes cleanup.
    pub(super) async fn run_joined_with<T, E, F>(
        &self,
        permit: Arc<JobWorkerPermit>,
        request: CancellationToken,
        deadline: Instant,
        work: F,
    ) -> Result<Joined<T, E>, WorkerError>
    where
        T: Send + 'static,
        E: Send + 'static,
        F: FnOnce(&Control) -> Result<T, E> + Send + 'static,
    {
        if self.panicked.load(Ordering::Acquire) || !permit.is_held() {
            return Err(WorkerError::Internal);
        }
        let local = CancellationToken::new();
        let _cancel_on_drop = local.clone().drop_guard();
        let control = Control {
            request,
            session: self.session.clone(),
            local,
            deadline,
        };
        control.check_worker()?;
        let panicked = Arc::clone(&self.panicked);
        tokio::task::spawn_blocking(move || {
            let _permit_owner = permit;
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

/// The one reading of a worker signal every inspection tool shares: capacity is
/// the host refusing the request, a deadline is an operational timeout, and a
/// panicked or poisoned worker is never described to the peer at all.
pub(super) fn worker_error(error: WorkerError) -> InspectionError {
    match error {
        WorkerError::Busy => InspectionError::Execution(ExecutionError::Busy),
        WorkerError::Cancelled => InspectionError::Project(ProjectError::Cancelled),
        WorkerError::TimedOut => {
            InspectionError::Project(ProjectError::Rejected(OperationalErrorCode::CommandTimeout))
        }
        WorkerError::Internal => InspectionError::Internal,
    }
}

/// A body that completed while cleanup was interrupted did not complete for the
/// peer: the interrupting signal wins. A body that reports its own cancellation
/// defers to the signal that caused it, and every other body error is its own.
pub(super) fn joined_result<T>(joined: Joined<T, InspectionError>) -> Result<T, InspectionError> {
    match (joined.result, joined.interrupted) {
        (
            Err(
                InspectionError::Project(ProjectError::Cancelled)
                | InspectionError::Execution(ExecutionError::Cancelled),
            ),
            Some(signal),
        ) => Err(worker_error(signal)),
        (Err(error), _) => Err(error),
        (Ok(_), Some(signal)) => Err(worker_error(signal)),
        (Ok(value), None) => Ok(value),
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
    fn registry_job_guard_is_the_single_worker_permit() -> Result<(), WorkerError> {
        let workers = Workers::new();
        let permit = workers.admit_job()?;
        assert!(permit.is_held());
        assert!(matches!(workers.admit_job(), Err(WorkerError::Busy)));
        permit.release_after_cleanup();
        assert!(!permit.is_held());
        let next = workers.admit_job()?;
        assert!(next.is_held());
        Ok(())
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
    fn admitted_job_executes_with_its_existing_permit_instead_of_busy()
    -> Result<(), Box<dyn std::error::Error>> {
        runtime()?.block_on(async {
            let workers = Workers::new();
            let permit = workers.admit_job().map_err(|_| "job admission failed")?;
            assert_eq!(
                workers
                    .run_joined(
                        CancellationToken::new(),
                        Instant::now() + Duration::from_secs(1),
                        |_| Ok::<_, ()>(()),
                    )
                    .await,
                Err(WorkerError::Busy)
            );
            assert_eq!(
                workers
                    .run_joined_with(
                        Arc::clone(&permit),
                        CancellationToken::new(),
                        Instant::now() + Duration::from_secs(1),
                        |_| Ok::<_, ()>(42),
                    )
                    .await,
                Ok(Joined {
                    result: Ok(42),
                    interrupted: None,
                })
            );
            assert!(permit.is_held());
            permit.release_after_cleanup();
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

    #[test]
    fn registry_owned_permit_runs_its_job_and_only_executor_release_reopens_capacity()
    -> Result<(), Box<dyn std::error::Error>> {
        runtime()?.block_on(async {
            let workers = Workers::new();
            let (permit, registry_permit) = workers
                .reserve_job()
                .map_err(|error| format!("job reservation failed: {error:?}"))?;
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

            let task_workers = workers.clone();
            let scoped = with_admitted_job(Arc::clone(&permit), async move {
                let inherited = task_workers.admit_job()?;
                assert!(Arc::ptr_eq(&permit, &inherited));
                let joined = task_workers
                    .run_joined_with(
                        Arc::clone(&inherited),
                        CancellationToken::new(),
                        Instant::now() + Duration::from_secs(1),
                        |_| Ok::<_, ()>(7),
                    )
                    .await?;
                inherited.release_after_cleanup();
                Ok::<_, WorkerError>(joined.result)
            })
            .await
            .map_err(|error| format!("joined job failed: {error:?}"))?;
            assert_eq!(scoped, Ok(7));
            assert!(matches!(workers.admit_job(), Err(WorkerError::Busy)));

            registry_permit.release_after_cleanup();
            assert!(workers.admit_job().is_ok());
            Ok(())
        })
    }

    #[test]
    fn rejected_task_admission_cannot_fall_through_to_worker_execution()
    -> Result<(), Box<dyn std::error::Error>> {
        runtime()?.block_on(async {
            let workers = Workers::new();
            let rejected = with_rejected_job_admission(async { workers.admit_job() }).await;
            assert!(matches!(rejected, Err(WorkerError::Busy)));
            assert!(workers.admit_job().is_ok());
        });
        Ok(())
    }
}
