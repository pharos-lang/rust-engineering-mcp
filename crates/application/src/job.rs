//! Owner-bound, transport-neutral lifecycle for bounded M3 jobs.

use crate::{coverage::CoverageTaskResult, nextest::NextestTaskResult};
use rust_engineering_domain::{
    ProjectRef,
    job::{
        JobBudget, JobCompletion, JobDeadline, JobId, JobInfrastructureFailure, JobOwnerBinding,
        JobPhase, JobState, Milliseconds, NON_DELIVERY_DEADLINE_MS, ResultRetention,
        RetentionQuotas,
    },
};
use std::{
    collections::HashMap,
    error::Error,
    fmt,
    sync::{Arc, Mutex},
};

pub use rust_engineering_domain::job::JobKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DeliveryToken(u64);

impl DeliveryToken {
    pub fn new(value: u64) -> Option<Self> {
        (value != 0).then_some(Self(value))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CleanupObservation {
    Observed,
    Uncertain,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobError {
    Busy,
    QuotaExceeded,
    Unavailable,
    InputRejected,
    InvalidConfiguration,
    ClockFailure,
    EntropyFailure,
    CleanupUncertain,
    DeadlineExceeded,
    Internal,
}

impl fmt::Display for JobError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Busy => "job executor busy",
            Self::QuotaExceeded => "job retention quota exceeded",
            Self::Unavailable => "job unavailable",
            Self::InputRejected => "job does not accept input",
            Self::InvalidConfiguration => "invalid job configuration",
            Self::ClockFailure => "job clock failed",
            Self::EntropyFailure => "job identifier entropy failed",
            Self::CleanupUncertain => "job cleanup uncertain",
            Self::DeadlineExceeded => "job control deadline exceeded",
            Self::Internal => "job registry failed",
        })
    }
}

impl Error for JobError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobTerminationIntent {
    ClientCancellation,
    NonDelivery,
    Deadline,
    Revoked,
    Shutdown,
}

#[derive(Clone, Debug)]
pub enum JobResult {
    TestNextest(NextestTaskResult),
    Coverage(CoverageTaskResult),
    /// A protocol-neutral, bounded JSON object produced only after a quality
    /// tool's typed output contract has validated it. The MCP adapter rebuilds
    /// the text mirror and wire envelope when the task is polled; application
    /// code neither parses the object nor depends on an MCP type.
    QualityTool(QualityToolResult),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QualityToolResult {
    json_object: String,
}

impl QualityToolResult {
    pub fn new(json_object: String) -> Result<Self, JobError> {
        if json_object.is_empty()
            || json_object.len()
                > usize::try_from(rust_engineering_domain::job::TASK_RESPONSE_MAX_BYTES)
                    .map_err(|_| JobError::InvalidConfiguration)?
        {
            return Err(JobError::InvalidConfiguration);
        }
        Ok(Self { json_object })
    }

    pub fn json_object(&self) -> &str {
        &self.json_object
    }
}

pub trait JobClock: Send + Sync {
    /// Milliseconds from one process-local monotonic origin.
    fn monotonic_millis(&self) -> Milliseconds;
    /// Bounded observational RFC3339 UTC text. It never authorizes work.
    fn utc_now(&self) -> Result<String, JobError>;
}

pub trait JobIds: Send + Sync {
    /// Supply 128 bits from the operating system random source.
    fn random_128(&self) -> Result<[u8; 16], JobError>;
}

pub trait JobSignal: Send + Sync {
    /// Must be non-blocking. The executor always invokes this after releasing
    /// the registry mutation lock.
    fn request_cancellation(&self);
    fn cancellation_requested(&self) -> bool;
    fn cleanup_observed(&self) -> bool;
    /// Wait for the real joined operation. Implementations must never abort a
    /// future as a substitute for gateway process-tree cleanup.
    fn join_cleanup(&self, timeout: Milliseconds) -> CleanupObservation;
}

/// The adapter owns the existing ADR-030 worker permit. The executor releases it
/// only after positively observed joined cleanup.
pub trait JobPermit: Send + Sync {
    fn is_held(&self) -> bool;
    fn release_after_cleanup(&self);
}

pub trait DeliveryTracker: Send + Sync {
    fn register(&self, token: DeliveryToken);
    fn mark_delivered(&self, token: DeliveryToken);
    fn was_delivered(&self, token: DeliveryToken) -> bool;
    fn forget(&self, token: DeliveryToken);
}

pub trait JobAuthority: Send + Sync {
    /// Revalidate the stored physical owner/root binding, live ProjectRef and
    /// current host-policy generation. Stored values are evidence, not grants.
    fn revalidate(
        &self,
        owner: &JobOwnerBinding,
        project: &ProjectRef,
        policy_generation: u64,
    ) -> bool;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobEventKind {
    AdmissionAccepted,
    AdmissionRejected,
    Started,
    PhaseTransition,
    CancellationIntent,
    Expired,
    CleanupObserved,
    CleanupUncertain,
    TerminalPublished,
    OrphanCancellation,
    RetentionExpired,
    ShutdownJoin,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobEventReason {
    None,
    Busy,
    Quota,
    InvalidConfiguration,
    ClientCancellation,
    NonDelivery,
    Deadline,
    Revoked,
    Shutdown,
    Infrastructure,
}

/// Closed, bounded trace data. No path, source, arguments, result or client data
/// can be represented by this structure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobEvent {
    pub job_id: Option<JobId>,
    pub kind: JobKind,
    pub event: JobEventKind,
    pub phase: JobPhase,
    pub state: JobState,
    pub reason: JobEventReason,
    pub elapsed_ms: u64,
    pub budget_ms: u64,
    pub retained_bytes: u64,
    pub retained_entries: u16,
}

pub trait JobEvents: Send + Sync {
    fn record(&self, event: JobEvent);
}

#[derive(Clone)]
pub struct JobSubmission {
    pub kind: JobKind,
    pub owner: JobOwnerBinding,
    pub project_ref: ProjectRef,
    pub policy_generation: u64,
    pub budget: JobBudget,
    pub delivery_token: DeliveryToken,
    pub reserved_result_bytes: u64,
    pub signal: Arc<dyn JobSignal>,
    pub permit: Arc<dyn JobPermit>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobSeed {
    pub id: JobId,
    pub state: JobState,
    pub phase: JobPhase,
    pub created_at_utc: String,
    pub ttl_ms: u64,
    pub poll_interval_ms: u64,
}

#[derive(Clone, Debug)]
pub struct JobStatus {
    pub id: JobId,
    pub kind: JobKind,
    pub project_ref: ProjectRef,
    pub state: JobState,
    pub phase: JobPhase,
    pub created_at_utc: String,
    pub updated_at_utc: String,
    pub ttl_ms: u64,
    pub poll_interval_ms: u64,
    pub completion: Option<JobCompletion<JobResult>>,
}

#[derive(Clone)]
pub struct JobRecord {
    id: JobId,
    kind: JobKind,
    owner: JobOwnerBinding,
    project_ref: ProjectRef,
    policy_generation: u64,
    state: JobState,
    phase: JobPhase,
    created_at_utc: String,
    updated_at_utc: String,
    created_at: Milliseconds,
    updated_at: Milliseconds,
    seed_deadline: JobDeadline,
    work_deadline: JobDeadline,
    capture_prepare_deadline: Option<JobDeadline>,
    execute_deadline: Option<JobDeadline>,
    collect_publish_deadline: Option<JobDeadline>,
    cleanup_deadline: Option<JobDeadline>,
    non_delivery_deadline: JobDeadline,
    expires_at: JobDeadline,
    budget: JobBudget,
    delivery_token: DeliveryToken,
    reserved_result_bytes: u64,
    cancellation: Option<JobTerminationIntent>,
    completion: Option<JobCompletion<JobResult>>,
    signal: Arc<dyn JobSignal>,
    permit: Arc<dyn JobPermit>,
}

impl JobRecord {
    fn status(&self) -> JobStatus {
        JobStatus {
            id: self.id.clone(),
            kind: self.kind,
            project_ref: self.project_ref.clone(),
            state: self.state,
            phase: self.phase,
            created_at_utc: self.created_at_utc.clone(),
            updated_at_utc: self.updated_at_utc.clone(),
            ttl_ms: ResultRetention::fixed().ttl().0,
            poll_interval_ms: rust_engineering_domain::job::TASK_POLL_INTERVAL_MS,
            completion: self.completion.clone(),
        }
    }
}

/// Transactional, owner-bound record store. `reserve_and_insert` must check all
/// entry and byte quotas atomically and must never evict an existing live record.
pub trait JobRegistry: Send + Sync {
    fn reserve_and_insert(
        &self,
        record: JobRecord,
        quotas: RetentionQuotas,
    ) -> Result<(), JobError>;
    fn get(&self, id: &JobId) -> Result<Option<JobRecord>, JobError>;
    fn mutate(
        &self,
        id: &JobId,
        update: &mut dyn FnMut(&mut JobRecord) -> Result<(), JobError>,
    ) -> Result<bool, JobError>;
    fn remove(&self, id: &JobId) -> Result<bool, JobError>;
    fn snapshot(&self) -> Result<Vec<JobRecord>, JobError>;
}

#[derive(Default)]
pub struct InMemoryJobRegistry {
    records: Mutex<HashMap<JobId, JobRecord>>,
}

impl JobRegistry for InMemoryJobRegistry {
    fn reserve_and_insert(
        &self,
        record: JobRecord,
        quotas: RetentionQuotas,
    ) -> Result<(), JobError> {
        let mut records = self.records.lock().map_err(|_| JobError::Internal)?;
        if records.values().any(|entry| !entry.state.is_terminal()) {
            return Err(JobError::Busy);
        }
        let owner_entries = records
            .values()
            .filter(|entry| entry.owner == record.owner)
            .count();
        let owner_bytes = records
            .values()
            .filter(|entry| entry.owner == record.owner)
            .try_fold(0_u64, |sum, entry| {
                sum.checked_add(entry.reserved_result_bytes)
            })
            .ok_or(JobError::QuotaExceeded)?;
        let server_bytes = records
            .values()
            .try_fold(0_u64, |sum, entry| {
                sum.checked_add(entry.reserved_result_bytes)
            })
            .ok_or(JobError::QuotaExceeded)?;
        if records.contains_key(&record.id)
            || owner_entries >= quotas.per_owner_entries()
            || records.len() >= quotas.server_entries()
            || owner_bytes
                .checked_add(record.reserved_result_bytes)
                .is_none_or(|bytes| bytes > quotas.per_owner_bytes())
            || server_bytes
                .checked_add(record.reserved_result_bytes)
                .is_none_or(|bytes| bytes > quotas.server_bytes())
        {
            return Err(JobError::QuotaExceeded);
        }
        records.insert(record.id.clone(), record);
        Ok(())
    }

    fn get(&self, id: &JobId) -> Result<Option<JobRecord>, JobError> {
        self.records
            .lock()
            .map(|records| records.get(id).cloned())
            .map_err(|_| JobError::Internal)
    }

    fn mutate(
        &self,
        id: &JobId,
        update: &mut dyn FnMut(&mut JobRecord) -> Result<(), JobError>,
    ) -> Result<bool, JobError> {
        let mut records = self.records.lock().map_err(|_| JobError::Internal)?;
        let Some(record) = records.get_mut(id) else {
            return Ok(false);
        };
        update(record)?;
        Ok(true)
    }

    fn remove(&self, id: &JobId) -> Result<bool, JobError> {
        self.records
            .lock()
            .map(|mut records| records.remove(id).is_some())
            .map_err(|_| JobError::Internal)
    }

    fn snapshot(&self) -> Result<Vec<JobRecord>, JobError> {
        self.records
            .lock()
            .map(|records| records.values().cloned().collect())
            .map_err(|_| JobError::Internal)
    }
}

#[derive(Default)]
pub struct InMemoryDeliveryTracker {
    delivered: Mutex<HashMap<DeliveryToken, bool>>,
}

impl DeliveryTracker for InMemoryDeliveryTracker {
    fn register(&self, token: DeliveryToken) {
        if let Ok(mut delivered) = self.delivered.lock() {
            delivered.entry(token).or_insert(false);
        }
    }

    fn mark_delivered(&self, token: DeliveryToken) {
        if let Ok(mut delivered) = self.delivered.lock()
            && let Some(state) = delivered.get_mut(&token)
        {
            *state = true;
        }
    }

    fn was_delivered(&self, token: DeliveryToken) -> bool {
        self.delivered
            .lock()
            .is_ok_and(|delivered| delivered.get(&token).copied().unwrap_or(false))
    }

    fn forget(&self, token: DeliveryToken) {
        if let Ok(mut delivered) = self.delivered.lock() {
            delivered.remove(&token);
        }
    }
}

#[derive(Clone)]
pub struct JobExecutor {
    registry: Arc<dyn JobRegistry>,
    clock: Arc<dyn JobClock>,
    ids: Arc<dyn JobIds>,
    authority: Arc<dyn JobAuthority>,
    delivery: Arc<dyn DeliveryTracker>,
    events: Arc<dyn JobEvents>,
    quotas: RetentionQuotas,
}

impl JobExecutor {
    /// Inline resident size of one registry record. Heap-owned result/source
    /// allocations remain bounded independently by the retained-byte quotas.
    pub fn resident_record_bytes() -> usize {
        std::mem::size_of::<JobRecord>()
    }

    pub fn new(
        registry: Arc<dyn JobRegistry>,
        clock: Arc<dyn JobClock>,
        ids: Arc<dyn JobIds>,
        authority: Arc<dyn JobAuthority>,
        delivery: Arc<dyn DeliveryTracker>,
        events: Arc<dyn JobEvents>,
    ) -> Self {
        Self {
            registry,
            clock,
            ids,
            authority,
            delivery,
            events,
            quotas: RetentionQuotas::fixed(),
        }
    }

    pub fn submit(&self, submission: JobSubmission) -> Result<JobSeed, JobError> {
        self.submit_guarded(submission, || true)
    }

    /// Commit one seed only if the request-side admission guard still permits it.
    ///
    /// The guard is the linearization point immediately before the atomic
    /// registry reservation/insert. Cancellation observed here releases the
    /// already-acquired worker permit and publishes no task ID. Cancellation
    /// after this point belongs to request-ID delivery tracking and must never be
    /// copied into the registry-owned [`JobSignal`].
    pub fn submit_guarded(
        &self,
        submission: JobSubmission,
        commit: impl FnOnce() -> bool,
    ) -> Result<JobSeed, JobError> {
        if !submission.permit.is_held() {
            self.reject(submission.kind, JobEventReason::Busy);
            return Err(JobError::InvalidConfiguration);
        }
        if submission.reserved_result_bytes == 0 {
            submission.permit.release_after_cleanup();
            self.reject(submission.kind, JobEventReason::InvalidConfiguration);
            return Err(JobError::InvalidConfiguration);
        }
        if submission.reserved_result_bytes > self.quotas.per_owner_bytes() {
            submission.permit.release_after_cleanup();
            self.reject(submission.kind, JobEventReason::Quota);
            return Err(JobError::QuotaExceeded);
        }
        if !self.authority.revalidate(
            &submission.owner,
            &submission.project_ref,
            submission.policy_generation,
        ) {
            submission.permit.release_after_cleanup();
            self.reject(submission.kind, JobEventReason::Revoked);
            return Err(JobError::Unavailable);
        }
        let now = self.clock.monotonic_millis();
        let prepared = (|| {
            Ok::<_, JobError>((
                bounded_timestamp(self.clock.utc_now()?)?,
                JobDeadline::after(now, Milliseconds(5_000)).map_err(|_| JobError::ClockFailure)?,
                JobDeadline::after(now, submission.budget.work())
                    .map_err(|_| JobError::ClockFailure)?,
                JobDeadline::after(now, Milliseconds(NON_DELIVERY_DEADLINE_MS))
                    .map_err(|_| JobError::ClockFailure)?,
                JobDeadline::after(now, ResultRetention::fixed().ttl())
                    .map_err(|_| JobError::ClockFailure)?,
            ))
        })();
        let (created_at_utc, seed_deadline, work_deadline, non_delivery_deadline, expires_at) =
            match prepared {
                Ok(prepared) => prepared,
                Err(error) => {
                    submission.permit.release_after_cleanup();
                    self.reject(submission.kind, JobEventReason::Infrastructure);
                    return Err(error);
                }
            };
        let mut commit = Some(commit);
        for _ in 0..4 {
            let random = match self.ids.random_128() {
                Ok(random) => random,
                Err(error) => {
                    submission.permit.release_after_cleanup();
                    self.reject(submission.kind, JobEventReason::Infrastructure);
                    return Err(error);
                }
            };
            let id = JobId::from_random_bytes(random);
            let exists = match self.registry.get(&id) {
                Ok(exists) => exists.is_some(),
                Err(error) => {
                    submission.permit.release_after_cleanup();
                    self.reject(submission.kind, JobEventReason::Infrastructure);
                    return Err(error);
                }
            };
            if exists {
                continue;
            }
            if seed_deadline.reached(self.clock.monotonic_millis()) {
                submission.permit.release_after_cleanup();
                self.reject(submission.kind, JobEventReason::Deadline);
                return Err(JobError::DeadlineExceeded);
            }
            let record = JobRecord {
                id: id.clone(),
                kind: submission.kind,
                owner: submission.owner.clone(),
                project_ref: submission.project_ref.clone(),
                policy_generation: submission.policy_generation,
                state: JobState::Admitted,
                phase: JobPhase::Admission,
                created_at_utc: created_at_utc.clone(),
                updated_at_utc: created_at_utc.clone(),
                created_at: now,
                updated_at: now,
                seed_deadline,
                work_deadline,
                capture_prepare_deadline: None,
                execute_deadline: None,
                collect_publish_deadline: None,
                cleanup_deadline: None,
                non_delivery_deadline,
                expires_at,
                budget: submission.budget,
                delivery_token: submission.delivery_token,
                reserved_result_bytes: submission.reserved_result_bytes,
                cancellation: None,
                completion: None,
                signal: Arc::clone(&submission.signal),
                permit: Arc::clone(&submission.permit),
            };
            if !commit.take().is_some_and(|commit| commit()) {
                submission.permit.release_after_cleanup();
                self.reject(submission.kind, JobEventReason::ClientCancellation);
                return Err(JobError::Unavailable);
            }
            match self.registry.reserve_and_insert(record, self.quotas) {
                Ok(()) => {
                    self.delivery.register(submission.delivery_token);
                    self.events.record(JobEvent {
                        job_id: Some(id.clone()),
                        kind: submission.kind,
                        event: JobEventKind::AdmissionAccepted,
                        phase: JobPhase::Admission,
                        state: JobState::Admitted,
                        reason: JobEventReason::None,
                        elapsed_ms: 0,
                        budget_ms: submission.budget.work().0,
                        retained_bytes: submission.reserved_result_bytes,
                        retained_entries: 1,
                    });
                    return Ok(JobSeed {
                        id,
                        state: JobState::Admitted,
                        phase: JobPhase::Admission,
                        created_at_utc,
                        ttl_ms: ResultRetention::fixed().ttl().0,
                        poll_interval_ms: rust_engineering_domain::job::TASK_POLL_INTERVAL_MS,
                    });
                }
                Err(error @ (JobError::Busy | JobError::QuotaExceeded)) => {
                    submission.permit.release_after_cleanup();
                    self.reject(
                        submission.kind,
                        if error == JobError::Busy {
                            JobEventReason::Busy
                        } else {
                            JobEventReason::Quota
                        },
                    );
                    return Err(error);
                }
                Err(error) => {
                    submission.permit.release_after_cleanup();
                    self.reject(submission.kind, JobEventReason::Infrastructure);
                    return Err(error);
                }
            }
        }
        submission.permit.release_after_cleanup();
        self.reject(submission.kind, JobEventReason::Infrastructure);
        Err(JobError::EntropyFailure)
    }

    pub fn start(&self, id: &JobId) -> Result<(), JobError> {
        self.mutate_authorized(id, |record, now, utc| {
            if record.state != JobState::Admitted || record.cancellation.is_some() {
                return Err(JobError::InvalidConfiguration);
            }
            record.state = JobState::Running;
            record.phase = JobPhase::Capture;
            record.capture_prepare_deadline = Some(
                JobDeadline::after(now, record.budget.capture_prepare())
                    .map_err(|_| JobError::ClockFailure)?,
            );
            record.updated_at = now;
            record.updated_at_utc = utc;
            Ok(())
        })?;
        self.events
            .record(self.event_for(id, JobEventKind::Started)?);
        Ok(())
    }

    pub fn set_phase(&self, id: &JobId, phase: JobPhase) -> Result<(), JobError> {
        self.mutate_authorized(id, |record, now, utc| {
            if record.state != JobState::Running || record.state.is_terminal() {
                return Err(JobError::InvalidConfiguration);
            }
            if !phase_transition(record.phase, phase) {
                return Err(JobError::InvalidConfiguration);
            }
            match phase {
                JobPhase::Execute => {
                    record.execute_deadline = Some(
                        JobDeadline::after(now, record.budget.execute())
                            .map_err(|_| JobError::ClockFailure)?,
                    );
                }
                JobPhase::Collect => {
                    record.collect_publish_deadline = Some(
                        JobDeadline::after(now, record.budget.collect_publish())
                            .map_err(|_| JobError::ClockFailure)?,
                    );
                }
                _ => {}
            }
            record.phase = phase;
            record.updated_at = now;
            record.updated_at_utc = utc;
            Ok(())
        })?;
        self.events
            .record(self.event_for(id, JobEventKind::PhaseTransition)?);
        Ok(())
    }

    pub fn status(&self, id: &JobId) -> Result<JobStatus, JobError> {
        self.authorized(id).map(|record| record.status())
    }

    pub fn cancel(&self, id: &JobId) -> Result<(), JobError> {
        self.authorized(id)?;
        self.request_termination(id, JobTerminationIntent::ClientCancellation)
    }

    pub fn update(&self, id: &JobId) -> Result<(), JobError> {
        self.authorized(id)?;
        Err(JobError::InputRejected)
    }

    pub fn finish(
        &self,
        id: &JobId,
        mut completion: JobCompletion<JobResult>,
        serialized_completion_bytes: u64,
        cleanup: CleanupObservation,
    ) -> Result<(), JobError> {
        let existing = self.registry.get(id)?.ok_or(JobError::Unavailable)?;
        if serialized_completion_bytes > existing.reserved_result_bytes
            || serialized_completion_bytes > rust_engineering_domain::job::TASK_RESPONSE_MAX_BYTES
        {
            completion =
                JobCompletion::InfrastructureFailure(JobInfrastructureFailure::ResultUnavailable);
        }
        let cleanup_observed =
            cleanup == CleanupObservation::Observed && existing.signal.cleanup_observed();
        let completion = if self.authority.revalidate(
            &existing.owner,
            &existing.project_ref,
            existing.policy_generation,
        ) {
            completion
        } else {
            self.request_termination(id, JobTerminationIntent::Revoked)?;
            JobCompletion::InfrastructureFailure(JobInfrastructureFailure::ResultUnavailable)
        };
        let now = self.clock.monotonic_millis();
        let utc = bounded_timestamp(self.clock.utc_now()?)?;
        let mut changed = false;
        let mut update = |record: &mut JobRecord| {
            if record.state.is_terminal() {
                return Ok(());
            }
            changed = true;
            record.phase = JobPhase::Cleanup;
            record.completion = Some(completion.clone());
            record.cleanup_deadline = Some(
                JobDeadline::after(now, record.budget.cleanup())
                    .map_err(|_| JobError::ClockFailure)?,
            );
            record.updated_at = now;
            record.updated_at_utc.clone_from(&utc);
            if cleanup_observed {
                finalize(record)?;
            }
            Ok(())
        };
        if !self.registry.mutate(id, &mut update)? {
            return Err(JobError::Unavailable);
        }
        if cleanup_observed && changed {
            self.release_and_event(id, JobEventKind::TerminalPublished)?;
            Ok(())
        } else if changed {
            self.events
                .record(self.event_for(id, JobEventKind::CleanupUncertain)?);
            Err(JobError::CleanupUncertain)
        } else {
            Ok(())
        }
    }

    pub fn observe_cleanup(&self, id: &JobId) -> Result<(), JobError> {
        let record = self.registry.get(id)?.ok_or(JobError::Unavailable)?;
        if !record.signal.cleanup_observed() {
            return Err(JobError::CleanupUncertain);
        }
        self.finalize_joined_cleanup(id)
    }

    fn finalize_joined_cleanup(&self, id: &JobId) -> Result<(), JobError> {
        let now = self.clock.monotonic_millis();
        let utc = bounded_timestamp(self.clock.utc_now()?)?;
        let mut changed = false;
        let mut update = |record: &mut JobRecord| {
            if !record.state.is_terminal() {
                if record.phase != JobPhase::Cleanup
                    || (record.cancellation.is_none() && record.completion.is_none())
                {
                    return Err(JobError::InvalidConfiguration);
                }
                changed = true;
                record.updated_at = now;
                record.updated_at_utc.clone_from(&utc);
                finalize(record)?;
            }
            Ok(())
        };
        if !self.registry.mutate(id, &mut update)? {
            return Err(JobError::Unavailable);
        }
        if changed {
            self.release_and_event(id, JobEventKind::CleanupObserved)?;
        }
        Ok(())
    }

    /// Independent callers schedule this bounded sweep; polling is not required
    /// for deadline, non-delivery, cleanup or retention enforcement.
    pub fn watchdog(&self) -> Result<(), JobError> {
        let now = self.clock.monotonic_millis();
        let mut cleanup_uncertain = false;
        let mut first_error = None;
        for record in self.registry.snapshot()? {
            match self.watchdog_record(record, now) {
                Ok(uncertain) => cleanup_uncertain |= uncertain,
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        if let Some(error) = first_error {
            Err(error)
        } else if cleanup_uncertain {
            Err(JobError::CleanupUncertain)
        } else {
            Ok(())
        }
    }

    fn watchdog_record(&self, record: JobRecord, now: Milliseconds) -> Result<bool, JobError> {
        if record.state.is_terminal() {
            if record.expires_at.reached(now) {
                self.registry.remove(&record.id)?;
                self.events.record(event(
                    &record,
                    JobEventKind::RetentionExpired,
                    JobEventReason::None,
                    now,
                ));
            }
            return Ok(false);
        }
        let expired = record.expires_at.reached(now);
        let phase_expired = active_deadline(&record).is_some_and(|deadline| deadline.reached(now));
        if record.phase == JobPhase::Cleanup {
            let cleanup_wait = record
                .cleanup_deadline
                .map(|deadline| Milliseconds(deadline.monotonic().0.saturating_sub(now.0)))
                .unwrap_or(Milliseconds(0));
            return match record.signal.join_cleanup(cleanup_wait) {
                CleanupObservation::Observed => {
                    self.finalize_joined_cleanup(&record.id)?;
                    Ok(false)
                }
                CleanupObservation::Uncertain if phase_expired => {
                    self.fail_cleanup(&record.id)?;
                    Ok(true)
                }
                CleanupObservation::Uncertain => {
                    self.events.record(event(
                        &record,
                        JobEventKind::CleanupUncertain,
                        reason(record.cancellation),
                        now,
                    ));
                    Ok(true)
                }
            };
        }
        if expired || record.work_deadline.reached(now) || phase_expired {
            self.request_termination(&record.id, JobTerminationIntent::Deadline)?;
            self.events
                .record(self.event_for(&record.id, JobEventKind::Expired)?);
        } else if record.non_delivery_deadline.reached(now)
            && !self.delivery.was_delivered(record.delivery_token)
        {
            self.request_termination(&record.id, JobTerminationIntent::NonDelivery)?;
            self.events.record(event(
                &record,
                JobEventKind::OrphanCancellation,
                JobEventReason::NonDelivery,
                now,
            ));
        }
        let current = self
            .registry
            .get(&record.id)?
            .ok_or(JobError::Unavailable)?;
        if current.phase != JobPhase::Cleanup {
            return Ok(false);
        }
        match current.signal.join_cleanup(current.budget.cleanup()) {
            CleanupObservation::Observed => {
                self.finalize_joined_cleanup(&current.id)?;
                Ok(false)
            }
            CleanupObservation::Uncertain => {
                self.events.record(event(
                    &current,
                    JobEventKind::CleanupUncertain,
                    reason(current.cancellation),
                    now,
                ));
                Ok(true)
            }
        }
    }

    pub fn shutdown_and_join(&self) -> Result<(), JobError> {
        let records = self.registry.snapshot()?;
        let mut uncertain = false;
        let mut first_error = None;
        for record in records
            .into_iter()
            .filter(|record| !record.state.is_terminal())
        {
            if let Err(error) = self.request_termination(&record.id, JobTerminationIntent::Shutdown)
            {
                first_error.get_or_insert(error);
                continue;
            }
            match record.signal.join_cleanup(record.budget.cleanup()) {
                CleanupObservation::Observed => {
                    if let Err(error) = self.observe_cleanup(&record.id) {
                        first_error.get_or_insert(error);
                    }
                }
                CleanupObservation::Uncertain => {
                    uncertain = true;
                    self.events.record(event(
                        &record,
                        JobEventKind::CleanupUncertain,
                        JobEventReason::Shutdown,
                        self.clock.monotonic_millis(),
                    ));
                }
            }
            match self.event_for(&record.id, JobEventKind::ShutdownJoin) {
                Ok(event) => self.events.record(event),
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        if let Some(error) = first_error {
            Err(error)
        } else if uncertain {
            Err(JobError::CleanupUncertain)
        } else {
            Ok(())
        }
    }

    pub fn delivery_tracker(&self) -> Arc<dyn DeliveryTracker> {
        Arc::clone(&self.delivery)
    }

    fn authorized(&self, id: &JobId) -> Result<JobRecord, JobError> {
        let record = self.registry.get(id)?.ok_or(JobError::Unavailable)?;
        if !self
            .authority
            .revalidate(&record.owner, &record.project_ref, record.policy_generation)
        {
            if !record.state.is_terminal() {
                let _ = self.request_termination(id, JobTerminationIntent::Revoked);
            }
            return Err(JobError::Unavailable);
        }
        Ok(record)
    }

    fn mutate_authorized(
        &self,
        id: &JobId,
        mut mutation: impl FnMut(&mut JobRecord, Milliseconds, String) -> Result<(), JobError>,
    ) -> Result<(), JobError> {
        self.authorized(id)?;
        let now = self.clock.monotonic_millis();
        let utc = bounded_timestamp(self.clock.utc_now()?)?;
        let mut update = |record: &mut JobRecord| mutation(record, now, utc.clone());
        self.registry
            .mutate(id, &mut update)?
            .then_some(())
            .ok_or(JobError::Unavailable)
    }

    fn request_termination(
        &self,
        id: &JobId,
        intent: JobTerminationIntent,
    ) -> Result<(), JobError> {
        let now = self.clock.monotonic_millis();
        let utc = bounded_timestamp(self.clock.utc_now()?)?;
        let mut signal = None;
        let mut update = |record: &mut JobRecord| {
            if record.state.is_terminal() {
                return Ok(());
            }
            if record.cancellation.is_none() {
                record.cancellation = Some(intent);
            }
            record.phase = JobPhase::Cleanup;
            if record.cleanup_deadline.is_none() {
                record.cleanup_deadline = Some(
                    JobDeadline::after(now, record.budget.cleanup())
                        .map_err(|_| JobError::ClockFailure)?,
                );
            }
            record.updated_at = now;
            record.updated_at_utc.clone_from(&utc);
            signal = Some(Arc::clone(&record.signal));
            Ok(())
        };
        if !self.registry.mutate(id, &mut update)? {
            return Err(JobError::Unavailable);
        }
        if let Some(signal) = signal {
            signal.request_cancellation();
        }
        self.events
            .record(self.event_for(id, JobEventKind::CancellationIntent)?);
        Ok(())
    }

    fn event_for(&self, id: &JobId, kind: JobEventKind) -> Result<JobEvent, JobError> {
        let record = self.registry.get(id)?.ok_or(JobError::Unavailable)?;
        Ok(event(
            &record,
            kind,
            reason(record.cancellation),
            self.clock.monotonic_millis(),
        ))
    }

    fn release_and_event(&self, id: &JobId, kind: JobEventKind) -> Result<(), JobError> {
        let record = self.registry.get(id)?.ok_or(JobError::Unavailable)?;
        record.permit.release_after_cleanup();
        self.delivery.forget(record.delivery_token);
        self.events.record(event(
            &record,
            kind,
            reason(record.cancellation),
            self.clock.monotonic_millis(),
        ));
        Ok(())
    }

    fn reject(&self, kind: JobKind, reason: JobEventReason) {
        self.events.record(JobEvent {
            job_id: None,
            kind,
            event: JobEventKind::AdmissionRejected,
            phase: JobPhase::Admission,
            state: JobState::Admitted,
            reason,
            elapsed_ms: 0,
            budget_ms: 0,
            retained_bytes: 0,
            retained_entries: 0,
        });
    }

    fn fail_cleanup(&self, id: &JobId) -> Result<(), JobError> {
        let now = self.clock.monotonic_millis();
        let utc = bounded_timestamp(self.clock.utc_now()?)?;
        let mut update = |record: &mut JobRecord| {
            if record.phase != JobPhase::Cleanup || record.state.is_terminal() {
                return Ok(());
            }
            let next = JobState::Failed;
            if !record.state.can_transition_to(next) {
                return Err(JobError::InvalidConfiguration);
            }
            record.state = next;
            record.phase = JobPhase::Terminal;
            record.completion = Some(JobCompletion::InfrastructureFailure(
                JobInfrastructureFailure::CleanupFailed,
            ));
            record.updated_at = now;
            record.updated_at_utc.clone_from(&utc);
            Ok(())
        };
        if !self.registry.mutate(id, &mut update)? {
            return Err(JobError::Unavailable);
        }
        // Deliberately do not release the ADR-030 permit: cleanup was not
        // observed, so this executor is quarantined until process shutdown.
        self.events
            .record(self.event_for(id, JobEventKind::CleanupUncertain)?);
        Ok(())
    }
}

fn bounded_timestamp(value: String) -> Result<String, JobError> {
    let bytes = value.as_bytes();
    if (20..=40).contains(&bytes.len())
        && bytes.iter().all(u8::is_ascii)
        && bytes.get(4) == Some(&b'-')
        && bytes.get(7) == Some(&b'-')
        && bytes.get(10) == Some(&b'T')
        && value.ends_with('Z')
    {
        Ok(value)
    } else {
        Err(JobError::ClockFailure)
    }
}

fn finalize(record: &mut JobRecord) -> Result<(), JobError> {
    let next = match (&record.cancellation, &record.completion) {
        (Some(JobTerminationIntent::Deadline), None) => {
            record.completion = Some(JobCompletion::InfrastructureFailure(
                JobInfrastructureFailure::TimedOut,
            ));
            JobState::Failed
        }
        (_, Some(JobCompletion::ToolResult { .. })) => JobState::Completed,
        (_, Some(JobCompletion::InfrastructureFailure(_))) => JobState::Failed,
        _ => JobState::Cancelled,
    };
    if !record.state.can_transition_to(next) {
        return Err(JobError::InvalidConfiguration);
    }
    record.state = next;
    record.phase = JobPhase::Terminal;
    Ok(())
}

fn phase_transition(current: JobPhase, next: JobPhase) -> bool {
    matches!(
        (current, next),
        (JobPhase::Capture, JobPhase::Prepare)
            | (JobPhase::Prepare, JobPhase::Execute)
            | (JobPhase::Execute, JobPhase::Collect)
            | (JobPhase::Collect, JobPhase::Publish)
    )
}

fn active_deadline(record: &JobRecord) -> Option<JobDeadline> {
    match record.phase {
        JobPhase::Admission => Some(record.seed_deadline),
        JobPhase::Capture | JobPhase::Prepare => record.capture_prepare_deadline,
        JobPhase::Execute => record.execute_deadline,
        JobPhase::Collect | JobPhase::Publish => record.collect_publish_deadline,
        JobPhase::Cleanup => record.cleanup_deadline,
        JobPhase::Terminal => None,
    }
}

fn reason(intent: Option<JobTerminationIntent>) -> JobEventReason {
    match intent {
        None => JobEventReason::None,
        Some(JobTerminationIntent::ClientCancellation) => JobEventReason::ClientCancellation,
        Some(JobTerminationIntent::NonDelivery) => JobEventReason::NonDelivery,
        Some(JobTerminationIntent::Deadline) => JobEventReason::Deadline,
        Some(JobTerminationIntent::Revoked) => JobEventReason::Revoked,
        Some(JobTerminationIntent::Shutdown) => JobEventReason::Shutdown,
    }
}

fn event(
    record: &JobRecord,
    kind: JobEventKind,
    reason: JobEventReason,
    now: Milliseconds,
) -> JobEvent {
    JobEvent {
        job_id: Some(record.id.clone()),
        kind: record.kind,
        event: kind,
        phase: record.phase,
        state: record.state,
        reason,
        elapsed_ms: now.0.saturating_sub(record.created_at.0),
        budget_ms: record.budget.work().0,
        retained_bytes: record.reserved_result_bytes,
        retained_entries: 1,
    }
}
