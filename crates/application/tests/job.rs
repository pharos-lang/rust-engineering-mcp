use rust_engineering_application::{
    job::{
        CleanupObservation, DeliveryToken, DeliveryTracker, InMemoryDeliveryTracker,
        InMemoryJobRegistry, JobAuthority, JobClock, JobError, JobEvent, JobEvents, JobExecutor,
        JobIds, JobPermit, JobRecord, JobRegistry, JobResult, JobSignal, JobSubmission,
    },
    nextest::{
        ArtifactStreams, NextestCompleteness, NextestCounts, NextestObservation, NextestOptions,
        NextestSelection, NextestTaskResult,
    },
};
use rust_engineering_domain::{
    ExecutionFingerprint, ExecutionTermination, ProjectRef, RuntimeIdentity,
    job::{
        JobBudget, JobCompletion, JobId, JobKind, JobOwnerBinding, JobPhase, JobState,
        Milliseconds, RetentionQuotas,
    },
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

#[derive(Default)]
struct Clock(AtomicU64);
impl Clock {
    fn advance(&self, millis: u64) {
        self.0.fetch_add(millis, Ordering::AcqRel);
    }
}
impl JobClock for Clock {
    fn monotonic_millis(&self) -> Milliseconds {
        Milliseconds(self.0.load(Ordering::Acquire))
    }
    fn utc_now(&self) -> Result<String, JobError> {
        Ok("2026-09-05T12:00:00Z".to_owned())
    }
}

#[derive(Default)]
struct Ids(AtomicU64);
impl JobIds for Ids {
    fn random_128(&self) -> Result<[u8; 16], JobError> {
        let value = self.0.fetch_add(1, Ordering::AcqRel) + 1;
        let mut bytes = [0_u8; 16];
        bytes[8..].copy_from_slice(&value.to_be_bytes());
        Ok(bytes)
    }
}

#[derive(Default)]
struct Signal {
    cancelled: AtomicBool,
    cleanup: AtomicBool,
}
impl JobSignal for Signal {
    fn request_cancellation(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
    fn cancellation_requested(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
    fn cleanup_observed(&self) -> bool {
        self.cleanup.load(Ordering::Acquire)
    }
    fn join_cleanup(&self, _: Milliseconds) -> CleanupObservation {
        if self.cleanup_observed() {
            CleanupObservation::Observed
        } else {
            CleanupObservation::Uncertain
        }
    }
}

struct Permit(AtomicBool);
impl Permit {
    fn held() -> Self {
        Self(AtomicBool::new(true))
    }
}
impl JobPermit for Permit {
    fn is_held(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
    fn release_after_cleanup(&self) {
        self.0.store(false, Ordering::Release);
    }
}

struct Authority(AtomicBool);
impl JobAuthority for Authority {
    fn revalidate(&self, owner: &JobOwnerBinding, _: &ProjectRef, generation: u64) -> bool {
        self.0.load(Ordering::Acquire) && owner.digest()[0] != 0 && generation == 11
    }
}

#[derive(Default)]
struct Events(Mutex<Vec<JobEvent>>);
impl JobEvents for Events {
    fn record(&self, event: JobEvent) {
        if let Ok(mut events) = self.0.lock() {
            events.push(event);
        }
    }
}

struct Fixture {
    executor: JobExecutor,
    clock: Arc<Clock>,
    authority: Arc<Authority>,
    delivery: Arc<InMemoryDeliveryTracker>,
    events: Arc<Events>,
}

type SubmissionFixture = (JobSubmission, Arc<Signal>, Arc<Permit>);

fn job_error<T>(result: Result<T, JobError>) -> Result<JobError, Box<dyn std::error::Error>> {
    match result {
        Ok(_) => Err("expected job error".into()),
        Err(error) => Ok(error),
    }
}

fn new_fixture() -> Fixture {
    let clock = Arc::new(Clock::default());
    let authority = Arc::new(Authority(AtomicBool::new(true)));
    let delivery = Arc::new(InMemoryDeliveryTracker::default());
    let events = Arc::new(Events::default());
    let executor = JobExecutor::new(
        Arc::new(InMemoryJobRegistry::default()),
        clock.clone(),
        Arc::new(Ids::default()),
        authority.clone(),
        delivery.clone(),
        events.clone(),
    );
    Fixture {
        executor,
        clock,
        authority,
        delivery,
        events,
    }
}

struct AdvancingIds {
    clock: Arc<Clock>,
    advance_ms: u64,
}

#[derive(Default)]
struct FailFirstRemove {
    inner: InMemoryJobRegistry,
    failed: AtomicBool,
}
impl JobRegistry for FailFirstRemove {
    fn reserve_and_insert(
        &self,
        record: JobRecord,
        quotas: RetentionQuotas,
    ) -> Result<(), JobError> {
        self.inner.reserve_and_insert(record, quotas)
    }
    fn get(&self, id: &JobId) -> Result<Option<JobRecord>, JobError> {
        self.inner.get(id)
    }
    fn mutate(
        &self,
        id: &JobId,
        update: &mut dyn FnMut(&mut JobRecord) -> Result<(), JobError>,
    ) -> Result<bool, JobError> {
        self.inner.mutate(id, update)
    }
    fn remove(&self, id: &JobId) -> Result<bool, JobError> {
        if !self.failed.swap(true, Ordering::AcqRel) {
            Err(JobError::Internal)
        } else {
            self.inner.remove(id)
        }
    }
    fn snapshot(&self) -> Result<Vec<JobRecord>, JobError> {
        self.inner.snapshot()
    }
}

impl JobIds for AdvancingIds {
    fn random_128(&self) -> Result<[u8; 16], JobError> {
        self.clock.advance(self.advance_ms);
        Ok([9; 16])
    }
}

fn new_submission(
    owner_byte: u8,
    token: u64,
    bytes: u64,
) -> Result<SubmissionFixture, Box<dyn std::error::Error>> {
    let signal = Arc::new(Signal::default());
    let permit = Arc::new(Permit::held());
    Ok((
        JobSubmission {
            kind: JobKind::TestNextest,
            owner: JobOwnerBinding::new([owner_byte; 32]),
            project_ref: "prj_00000000000000000000000000000001".parse()?,
            policy_generation: 11,
            budget: JobBudget::asynchronous_default()?,
            delivery_token: DeliveryToken::new(token).ok_or("zero token")?,
            reserved_result_bytes: bytes,
            signal: signal.clone(),
            permit: permit.clone(),
        },
        signal,
        permit,
    ))
}

fn observation() -> Result<NextestObservation, Box<dyn std::error::Error>> {
    Ok(NextestObservation {
        options: NextestOptions::try_from(NextestSelection::default())?,
        validation_complete: true,
        completeness: NextestCompleteness::Complete,
        counts: NextestCounts {
            selected: 1,
            passed: 1,
            ..Default::default()
        },
        tests: Vec::new(),
        tests_omitted: 0,
        doctests_run: false,
        termination: ExecutionTermination::Exited,
        exit_code: Some(0),
        runtime: RuntimeIdentity {
            platform: "linux-aarch64".to_owned(),
            image_id: format!("sha256:{}", "1".repeat(64)),
            configuration_fingerprint: format!("sha256:{}", "2".repeat(64)).parse()?,
            execution_fingerprint: format!("sha256:{}", "3".repeat(64)).parse()?,
            rust_version: "rustc 1.98.1".to_owned(),
            cargo_version: "cargo 1.98.1".to_owned(),
            declared_toolchain: None,
        },
        execution_fingerprint: format!("sha256:{}", "3".repeat(64))
            .parse::<ExecutionFingerprint>()?,
        artifacts: ArtifactStreams::default(),
    })
}

#[test]
fn masking_is_identical_for_unknown_foreign_and_expired_ids()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = new_fixture();
    let unknown = "job_ffffffffffffffffffffffffffffffff".parse()?;
    assert_eq!(
        job_error(fixture.executor.status(&unknown))?,
        JobError::Unavailable
    );

    let (submission, signal, _) = new_submission(7, 1, 1)?;
    let id = fixture.executor.submit(submission)?.id;
    fixture.authority.0.store(false, Ordering::Release);
    assert_eq!(
        job_error(fixture.executor.status(&id))?,
        JobError::Unavailable
    );
    assert!(signal.cancellation_requested());

    fixture.authority.0.store(true, Ordering::Release);
    signal.cleanup.store(true, Ordering::Release);
    fixture.executor.observe_cleanup(&id)?;
    fixture.clock.advance(7_200_000);
    fixture.executor.watchdog()?;
    assert_eq!(
        job_error(fixture.executor.status(&id))?,
        JobError::Unavailable
    );
    Ok(())
}

#[test]
fn cancelled_is_visible_only_after_observed_joined_cleanup()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = new_fixture();
    let (submission, signal, permit) = new_submission(7, 1, 1024)?;
    let id = fixture.executor.submit(submission)?.id;
    fixture.executor.start(&id)?;
    fixture.executor.cancel(&id)?;
    assert_eq!(fixture.executor.status(&id)?.state, JobState::Running);
    assert!(permit.is_held());
    signal.cleanup.store(true, Ordering::Release);
    fixture.executor.watchdog()?;
    assert_eq!(fixture.executor.status(&id)?.state, JobState::Cancelled);
    assert!(!permit.is_held());
    Ok(())
}

#[test]
fn delivery_uses_registry_token_independently_of_request_lifetime()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = new_fixture();
    let (submission, signal, _) = new_submission(7, 9, 1)?;
    let id = fixture.executor.submit(submission)?.id;
    fixture.executor.start(&id)?;
    fixture
        .delivery
        .mark_delivered(DeliveryToken::new(9).ok_or("zero token")?);
    fixture.clock.advance(30_000);
    fixture.executor.watchdog()?;
    assert!(!signal.cancellation_requested());
    assert_eq!(fixture.executor.status(&id)?.state, JobState::Running);

    let other = new_fixture();
    let (submission, orphan, _) = new_submission(7, 10, 1)?;
    let orphan_id = other.executor.submit(submission)?.id;
    other.executor.start(&orphan_id)?;
    orphan.cleanup.store(true, Ordering::Release);
    other.clock.advance(30_000);
    other.executor.watchdog()?;
    assert!(orphan.cancellation_requested());
    assert_eq!(
        other.executor.status(&orphan_id)?.state,
        JobState::Cancelled
    );
    Ok(())
}

#[test]
fn cancellation_before_seed_commit_rolls_back_record_and_permit()
-> Result<(), Box<dyn std::error::Error>> {
    let clock = Arc::new(Clock::default());
    let registry = Arc::new(InMemoryJobRegistry::default());
    let events = Arc::new(Events::default());
    let executor = JobExecutor::new(
        registry.clone(),
        clock,
        Arc::new(Ids::default()),
        Arc::new(Authority(AtomicBool::new(true))),
        Arc::new(InMemoryDeliveryTracker::default()),
        events.clone(),
    );
    let (submission, signal, permit) = new_submission(7, 91, 1)?;
    assert_eq!(
        job_error(executor.submit_guarded(submission, || false))?,
        JobError::Unavailable
    );
    assert!(!permit.is_held());
    assert!(!signal.cancellation_requested());
    assert!(registry.snapshot()?.is_empty());
    let events = events.0.lock().map_err(|_| "event lock")?;
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].event,
        rust_engineering_application::job::JobEventKind::AdmissionRejected
    );
    assert_eq!(
        events[0].reason,
        rust_engineering_application::job::JobEventReason::ClientCancellation
    );
    Ok(())
}

#[test]
fn cancellation_after_seed_commit_prevents_job_start() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = new_fixture();
    let (submission, signal, permit) = new_submission(7, 1, 1024)?;
    let id = fixture.executor.submit(submission)?.id;
    fixture.executor.cancel(&id)?;
    assert_eq!(
        fixture.executor.start(&id),
        Err(JobError::InvalidConfiguration)
    );
    assert!(signal.cancellation_requested());
    assert!(permit.is_held());
    signal.cleanup.store(true, Ordering::Release);
    fixture.executor.observe_cleanup(&id)?;
    assert_eq!(fixture.executor.status(&id)?.state, JobState::Cancelled);
    assert!(!permit.is_held());
    Ok(())
}

#[test]
fn completed_work_is_not_rewritten_when_work_deadline_passes_during_cleanup()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = new_fixture();
    let (submission, signal, permit) = new_submission(7, 1, 1024)?;
    let id = fixture.executor.submit(submission)?.id;
    fixture.executor.start(&id)?;
    assert_eq!(
        fixture.executor.finish(
            &id,
            JobCompletion::ToolResult {
                result: JobResult::TestNextest(NextestTaskResult::new(
                    observation()?,
                    Vec::new(),
                    1,
                )?),
                is_error: false,
            },
            1,
            CleanupObservation::Uncertain,
        ),
        Err(JobError::CleanupUncertain)
    );
    assert_eq!(fixture.executor.status(&id)?.phase, JobPhase::Cleanup);
    assert_eq!(fixture.executor.status(&id)?.state, JobState::Running);
    assert!(permit.is_held());
    fixture.clock.advance(300_000);
    signal.cleanup.store(true, Ordering::Release);
    fixture.executor.watchdog()?;
    let status = fixture.executor.status(&id)?;
    assert_eq!(status.phase, JobPhase::Terminal);
    assert_eq!(status.state, JobState::Completed);
    assert!(!signal.cancellation_requested());
    assert!(!permit.is_held());
    Ok(())
}

#[test]
fn client_cancellation_is_never_rewritten_as_work_budget_failure()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = new_fixture();
    let (submission, signal, _) = new_submission(7, 1, 1024)?;
    let id = fixture.executor.submit(submission)?.id;
    fixture.executor.start(&id)?;
    fixture.executor.cancel(&id)?;
    fixture.clock.advance(300_000);
    signal.cleanup.store(true, Ordering::Release);
    fixture.executor.watchdog()?;
    assert_eq!(fixture.executor.status(&id)?.state, JobState::Cancelled);
    Ok(())
}

#[test]
fn cancellation_during_publication_and_cleanup_waits_for_observed_join()
-> Result<(), Box<dyn std::error::Error>> {
    let publishing = new_fixture();
    let (submission, signal, permit) = new_submission(7, 1, 1024)?;
    let id = publishing.executor.submit(submission)?.id;
    publishing.executor.start(&id)?;
    publishing.executor.set_phase(&id, JobPhase::Prepare)?;
    publishing.executor.set_phase(&id, JobPhase::Execute)?;
    publishing.executor.set_phase(&id, JobPhase::Collect)?;
    publishing.executor.set_phase(&id, JobPhase::Publish)?;
    publishing.executor.cancel(&id)?;
    assert_eq!(publishing.executor.status(&id)?.state, JobState::Running);
    assert!(permit.is_held());
    signal.cleanup.store(true, Ordering::Release);
    publishing.executor.watchdog()?;
    assert_eq!(publishing.executor.status(&id)?.state, JobState::Cancelled);
    assert!(!permit.is_held());

    let cleaning = new_fixture();
    let (submission, signal, permit) = new_submission(7, 2, 1024)?;
    let id = cleaning.executor.submit(submission)?.id;
    cleaning.executor.start(&id)?;
    assert_eq!(
        cleaning.executor.finish(
            &id,
            JobCompletion::ToolResult {
                result: JobResult::TestNextest(NextestTaskResult::new(
                    observation()?,
                    Vec::new(),
                    1,
                )?),
                is_error: false,
            },
            1,
            CleanupObservation::Uncertain,
        ),
        Err(JobError::CleanupUncertain)
    );
    cleaning.executor.cancel(&id)?;
    assert_eq!(cleaning.executor.status(&id)?.state, JobState::Running);
    assert!(permit.is_held());
    signal.cleanup.store(true, Ordering::Release);
    cleaning.executor.watchdog()?;
    assert_eq!(cleaning.executor.status(&id)?.state, JobState::Completed);
    assert!(!permit.is_held());
    Ok(())
}

#[test]
fn uncertain_finish_holds_permit_and_does_not_publish_terminal_state()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = new_fixture();
    let (submission, _, permit) = new_submission(7, 1, 1024)?;
    let id = fixture.executor.submit(submission)?.id;
    fixture.executor.start(&id)?;
    assert_eq!(
        fixture.executor.finish(
            &id,
            JobCompletion::ToolResult {
                result: JobResult::TestNextest(NextestTaskResult::new(
                    observation()?,
                    Vec::new(),
                    1,
                )?),
                is_error: false,
            },
            1,
            CleanupObservation::Uncertain,
        ),
        Err(JobError::CleanupUncertain)
    );
    let status = fixture.executor.status(&id)?;
    assert_eq!(status.state, JobState::Running);
    assert_eq!(status.phase, JobPhase::Cleanup);
    assert!(permit.is_held());
    Ok(())
}

#[test]
fn cleanup_deadline_quarantines_permit_and_reports_cleanup_failed()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = new_fixture();
    let (submission, _, permit) = new_submission(7, 1, 1024)?;
    let id = fixture.executor.submit(submission)?.id;
    fixture.executor.start(&id)?;
    fixture.executor.cancel(&id)?;
    fixture.clock.advance(60_000);
    assert_eq!(fixture.executor.watchdog(), Err(JobError::CleanupUncertain));
    let status = fixture.executor.status(&id)?;
    assert_eq!(status.state, JobState::Failed);
    assert_eq!(status.phase, JobPhase::Terminal);
    assert!(matches!(
        status.completion,
        Some(JobCompletion::InfrastructureFailure(
            rust_engineering_domain::job::JobInfrastructureFailure::CleanupFailed
        ))
    ));
    assert!(permit.is_held());
    Ok(())
}

#[test]
fn serialized_completion_budget_accepts_exact_limit_and_omits_one_byte_over()
-> Result<(), Box<dyn std::error::Error>> {
    let exact = new_fixture();
    let (submission, signal, _) = new_submission(7, 1, 512 * 1024)?;
    let id = exact.executor.submit(submission)?.id;
    signal.cleanup.store(true, Ordering::Release);
    exact.executor.finish(
        &id,
        JobCompletion::ToolResult {
            result: JobResult::TestNextest(NextestTaskResult::new(observation()?, Vec::new(), 1)?),
            is_error: false,
        },
        512 * 1024,
        CleanupObservation::Observed,
    )?;
    assert_eq!(exact.executor.status(&id)?.state, JobState::Completed);

    let over = new_fixture();
    let (submission, signal, _) = new_submission(7, 2, 512 * 1024)?;
    let id = over.executor.submit(submission)?.id;
    signal.cleanup.store(true, Ordering::Release);
    over.executor.finish(
        &id,
        JobCompletion::ToolResult {
            result: JobResult::TestNextest(NextestTaskResult::new(observation()?, Vec::new(), 1)?),
            is_error: false,
        },
        512 * 1024 + 1,
        CleanupObservation::Observed,
    )?;
    let status = over.executor.status(&id)?;
    assert_eq!(status.state, JobState::Failed);
    assert!(matches!(
        status.completion,
        Some(JobCompletion::InfrastructureFailure(
            rust_engineering_domain::job::JobInfrastructureFailure::ResultUnavailable
        ))
    ));
    Ok(())
}

#[test]
fn polling_does_not_extend_retention_ttl() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = new_fixture();
    let (submission, signal, _) = new_submission(7, 1, 1)?;
    let id = fixture.executor.submit(submission)?.id;
    signal.cleanup.store(true, Ordering::Release);
    fixture.executor.finish(
        &id,
        JobCompletion::InfrastructureFailure(
            rust_engineering_domain::job::JobInfrastructureFailure::Internal,
        ),
        1,
        CleanupObservation::Observed,
    )?;
    for _ in 0..7 {
        fixture.clock.advance(1_000_000);
        assert!(fixture.executor.status(&id).is_ok());
    }
    fixture.clock.advance(199_999);
    assert!(fixture.executor.status(&id).is_ok());
    fixture.clock.advance(1);
    fixture.executor.watchdog()?;
    assert_eq!(
        job_error(fixture.executor.status(&id))?,
        JobError::Unavailable
    );
    Ok(())
}

#[test]
fn watchdog_accumulates_an_error_and_continues_expiring_later_records()
-> Result<(), Box<dyn std::error::Error>> {
    let clock = Arc::new(Clock::default());
    let registry = Arc::new(FailFirstRemove::default());
    let executor = JobExecutor::new(
        registry.clone(),
        clock.clone(),
        Arc::new(Ids::default()),
        Arc::new(Authority(AtomicBool::new(true))),
        Arc::new(InMemoryDeliveryTracker::default()),
        Arc::new(Events::default()),
    );
    for token in 1..=2 {
        let (submission, signal, _) = new_submission(7, token, 1)?;
        let id = executor.submit(submission)?.id;
        signal.cleanup.store(true, Ordering::Release);
        executor.finish(
            &id,
            JobCompletion::InfrastructureFailure(
                rust_engineering_domain::job::JobInfrastructureFailure::Internal,
            ),
            1,
            CleanupObservation::Observed,
        )?;
    }
    clock.advance(7_200_000);
    assert_eq!(executor.watchdog(), Err(JobError::Internal));
    assert_eq!(registry.snapshot()?.len(), 1);
    Ok(())
}

#[test]
fn seed_commit_accepts_just_under_five_seconds_and_rejects_the_deadline()
-> Result<(), Box<dyn std::error::Error>> {
    for (advance_ms, expected) in [(4_999, Ok(())), (5_000, Err(JobError::DeadlineExceeded))] {
        let clock = Arc::new(Clock::default());
        let executor = JobExecutor::new(
            Arc::new(InMemoryJobRegistry::default()),
            clock.clone(),
            Arc::new(AdvancingIds { clock, advance_ms }),
            Arc::new(Authority(AtomicBool::new(true))),
            Arc::new(InMemoryDeliveryTracker::default()),
            Arc::new(Events::default()),
        );
        let (submission, _, permit) = new_submission(7, 1, 1)?;
        let result = executor.submit(submission).map(|_| ());
        assert_eq!(result, expected);
        assert_eq!(permit.is_held(), expected.is_ok());
    }
    Ok(())
}

#[test]
fn one_active_job_blocks_submit_but_not_status_cancel_or_update()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = new_fixture();
    let (submission, _, _) = new_submission(7, 1, 1)?;
    let id = fixture.executor.submit(submission)?.id;
    let (second, _, _) = new_submission(7, 2, 1)?;
    assert_eq!(job_error(fixture.executor.submit(second))?, JobError::Busy);
    let before = fixture.executor.status(&id)?;
    assert_eq!(before.state, JobState::Admitted);
    assert_eq!(fixture.executor.update(&id), Err(JobError::InputRejected));
    let after = fixture.executor.status(&id)?;
    assert_eq!(after.project_ref, before.project_ref);
    assert_eq!(after.kind, before.kind);
    assert_eq!(after.state, before.state);
    assert_eq!(after.phase, before.phase);
    assert_eq!(after.ttl_ms, before.ttl_ms);
    fixture.executor.cancel(&id)?;
    Ok(())
}

#[test]
fn retention_limits_reject_before_a_new_record_is_created() -> Result<(), Box<dyn std::error::Error>>
{
    let fixture = new_fixture();
    for token in 1..=64 {
        let (submission, signal, _) = new_submission(7, token, 512 * 1024)?;
        let id = fixture.executor.submit(submission)?.id;
        signal.cleanup.store(true, Ordering::Release);
        fixture.executor.finish(
            &id,
            JobCompletion::InfrastructureFailure(
                rust_engineering_domain::job::JobInfrastructureFailure::Internal,
            ),
            1,
            CleanupObservation::Observed,
        )?;
    }
    let (over_entries, _, permit) = new_submission(7, 65, 1)?;
    assert_eq!(
        job_error(fixture.executor.submit(over_entries))?,
        JobError::QuotaExceeded
    );
    assert!(!permit.is_held());

    let (over_owner_bytes, _, permit) = new_submission(7, 66, 32 * 1024 * 1024 + 1)?;
    assert_eq!(
        job_error(fixture.executor.submit(over_owner_bytes))?,
        JobError::QuotaExceeded
    );
    assert!(!permit.is_held());

    let bytes_fixture = new_fixture();
    let mut token = 1;
    for owner in [7_u8, 8, 9, 10] {
        let (submission, signal, _) = new_submission(owner, token, 32 * 1024 * 1024)?;
        let id = bytes_fixture.executor.submit(submission)?.id;
        signal.cleanup.store(true, Ordering::Release);
        bytes_fixture.executor.finish(
            &id,
            JobCompletion::InfrastructureFailure(
                rust_engineering_domain::job::JobInfrastructureFailure::Internal,
            ),
            1,
            CleanupObservation::Observed,
        )?;
        token += 1;
    }
    let (over_server, _, _) = new_submission(11, token, 1)?;
    assert_eq!(
        job_error(bytes_fixture.executor.submit(over_server))?,
        JobError::QuotaExceeded
    );

    let entries_fixture = new_fixture();
    let mut token = 1;
    for owner in [7_u8, 8, 9, 10] {
        for _ in 0..64 {
            let (submission, signal, _) = new_submission(owner, token, 1)?;
            let id = entries_fixture.executor.submit(submission)?.id;
            signal.cleanup.store(true, Ordering::Release);
            entries_fixture.executor.finish(
                &id,
                JobCompletion::InfrastructureFailure(
                    rust_engineering_domain::job::JobInfrastructureFailure::Internal,
                ),
                1,
                CleanupObservation::Observed,
            )?;
            token += 1;
        }
    }
    let (over_entries, _, permit) = new_submission(11, token, 1)?;
    assert_eq!(
        job_error(entries_fixture.executor.submit(over_entries))?,
        JobError::QuotaExceeded
    );
    assert!(!permit.is_held());
    Ok(())
}

#[test]
fn deadline_and_completion_mapping_release_only_after_cleanup()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = new_fixture();
    let (submission, signal, permit) = new_submission(7, 1, 1)?;
    let id = fixture.executor.submit(submission)?.id;
    fixture.executor.start(&id)?;
    signal.cleanup.store(true, Ordering::Release);
    fixture.clock.advance(300_000);
    fixture.executor.watchdog()?;
    assert!(signal.cancellation_requested());
    assert_eq!(fixture.executor.status(&id)?.state, JobState::Failed);
    assert!(!permit.is_held());

    let completed = new_fixture();
    let (submission, signal, _) = new_submission(7, 2, 1)?;
    let completed_id = completed.executor.submit(submission)?.id;
    signal.cleanup.store(true, Ordering::Release);
    completed.executor.finish(
        &completed_id,
        JobCompletion::ToolResult {
            result: JobResult::TestNextest(NextestTaskResult::new(observation()?, Vec::new(), 1)?),
            is_error: true,
        },
        1,
        CleanupObservation::Observed,
    )?;
    assert_eq!(
        completed.executor.status(&completed_id)?.state,
        JobState::Completed
    );

    let late_success = new_fixture();
    let (submission, signal, _) = new_submission(7, 3, 1)?;
    let late_id = late_success.executor.submit(submission)?.id;
    late_success.executor.cancel(&late_id)?;
    signal.cleanup.store(true, Ordering::Release);
    late_success.executor.finish(
        &late_id,
        JobCompletion::ToolResult {
            result: JobResult::TestNextest(NextestTaskResult::new(observation()?, Vec::new(), 1)?),
            is_error: false,
        },
        1,
        CleanupObservation::Observed,
    )?;
    assert_eq!(
        late_success.executor.status(&late_id)?.state,
        JobState::Completed
    );
    Ok(())
}

#[test]
fn every_phase_deadline_joins_before_releasing_capacity() -> Result<(), Box<dyn std::error::Error>>
{
    for phase in [
        JobPhase::Admission,
        JobPhase::Capture,
        JobPhase::Prepare,
        JobPhase::Execute,
        JobPhase::Collect,
        JobPhase::Publish,
    ] {
        let fixture = new_fixture();
        let (submission, signal, permit) = new_submission(7, 1, 1)?;
        let id = fixture.executor.submit(submission)?.id;
        let advance = match phase {
            JobPhase::Admission => 5_000,
            JobPhase::Capture => {
                fixture.executor.start(&id)?;
                60_000
            }
            JobPhase::Prepare => {
                fixture.executor.start(&id)?;
                fixture.executor.set_phase(&id, JobPhase::Prepare)?;
                60_000
            }
            JobPhase::Execute => {
                fixture.executor.start(&id)?;
                fixture.executor.set_phase(&id, JobPhase::Prepare)?;
                fixture.executor.set_phase(&id, JobPhase::Execute)?;
                180_000
            }
            JobPhase::Collect | JobPhase::Publish => {
                fixture.executor.start(&id)?;
                fixture.executor.set_phase(&id, JobPhase::Prepare)?;
                fixture.executor.set_phase(&id, JobPhase::Execute)?;
                fixture.executor.set_phase(&id, JobPhase::Collect)?;
                if phase == JobPhase::Publish {
                    fixture.executor.set_phase(&id, JobPhase::Publish)?;
                }
                30_000
            }
            JobPhase::Cleanup | JobPhase::Terminal => {
                return Err("cleanup handled separately".into());
            }
        };
        signal.cleanup.store(true, Ordering::Release);
        fixture.clock.advance(advance);
        fixture.executor.watchdog()?;
        assert_eq!(fixture.executor.status(&id)?.state, JobState::Failed);
        assert!(!permit.is_held());
    }

    let cleanup = new_fixture();
    let (submission, signal, permit) = new_submission(7, 1, 1)?;
    let id = cleanup.executor.submit(submission)?.id;
    cleanup.executor.cancel(&id)?;
    assert!(permit.is_held());
    signal.cleanup.store(true, Ordering::Release);
    cleanup.clock.advance(60_000);
    cleanup.executor.watchdog()?;
    assert_eq!(cleanup.executor.status(&id)?.state, JobState::Cancelled);
    assert!(!permit.is_held());
    Ok(())
}

#[test]
fn trace_event_has_only_closed_bounded_fields() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = new_fixture();
    let (submission, _, _) = new_submission(7, 1, 1)?;
    fixture.executor.submit(submission)?;
    let events = fixture.events.0.lock().map_err(|_| "events poisoned")?;
    assert_eq!(events.len(), 1);
    assert!(std::mem::size_of::<JobEvent>() <= 128);
    let rendered = format!("{:?}", events[0]);
    assert!(!rendered.contains("prj_"));
    assert!(!rendered.contains("source"));
    assert!(!rendered.contains("argument"));
    Ok(())
}
