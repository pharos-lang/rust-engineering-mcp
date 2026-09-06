//! MCP Tasks projection over the application-owned job registry.

use super::project::Registry;
use super::{coverage::CoverageTool, nextest::NextestTool, workers};
use rmcp::model::{
    CallToolResult, CreateTaskResult, DetailedTask, ErrorData, GetTaskResult, JsonObject, Task,
    TaskPayload, TaskStatus,
};
use rust_engineering_application::job::{
    DeliveryTracker, InMemoryDeliveryTracker, InMemoryJobRegistry, JobAuthority, JobClock,
    JobError, JobEvent, JobEvents, JobExecutor, JobIds, JobResult, JobStatus, JobSubmission,
    QualityToolResult,
};
use rust_engineering_application::nextest::{NextestArtifactReference, NextestTaskResult};
use rust_engineering_application::{OperationControl, ProjectError, QualityOwnerFacts};
use rust_engineering_domain::job::{
    JobBudget, JobCompletion, JobId, JobInfrastructureFailure, JobKind, JobOwnerBinding, JobState,
    Milliseconds,
};
use rust_engineering_domain::{ArtifactId, QualityArtifactId};
use rust_engineering_domain::{ProjectRef, UtcInstant};
use std::sync::{Arc, Mutex, TryLockError};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const UNAVAILABLE: &str = "task unavailable";
const INPUT_REJECTED: &str = "task does not accept input";
const TASK_CONTROL_DEFAULT_MS: u64 = 2_000;
const TASK_CONTROL_MAX_MS: u64 = 5_000;
const AUTHORITY_CACHE_MAX: usize = 256;

#[derive(Clone)]
struct LiveAuthorityBinding {
    project_ref: ProjectRef,
    owner: JobOwnerBinding,
    facts: QualityOwnerFacts,
}

/// Stderr-only closed event adapter. No project reference, path, arguments,
/// result body, client identity, or policy value is representable here.
#[allow(dead_code)]
pub(super) struct TracingJobEvents;

impl JobEvents for TracingJobEvents {
    fn record(&self, event: JobEvent) {
        tracing::info!(
            target: "rust_engineering_mcp::jobs",
            job_id = event.job_id.as_ref().map(JobId::as_str).unwrap_or(""),
            kind = ?event.kind,
            event = ?event.event,
            phase = ?event.phase,
            state = ?event.state,
            reason = ?event.reason,
            elapsed_ms = event.elapsed_ms,
            budget_ms = event.budget_ms,
            retained_bytes = event.retained_bytes,
            retained_entries = event.retained_entries,
            "bounded job lifecycle event"
        );
    }
}

pub(super) struct Tasks {
    executor: Option<Arc<JobExecutor>>,
    delivery: Arc<dyn DeliveryTracker>,
    registry: Option<Arc<Mutex<Registry>>>,
    state_root_identity: Option<((i64, u64), u32)>,
    workers: Option<workers::Workers>,
    nextest: NextestTool,
    coverage: CoverageTool,
    liveness: Option<Arc<dyn TaskArtifactLiveness>>,
    authority_cache: Option<Arc<Mutex<Vec<LiveAuthorityBinding>>>>,
}

pub(super) struct AdmittedTask {
    pub(super) id: JobId,
    pub(super) response: CreateTaskResult,
    pub(super) signal: Arc<workers::JobExecutionSignal>,
    pub(super) permit: Arc<workers::JobWorkerPermit>,
    pub(super) executor: Arc<JobExecutor>,
}

pub(super) trait TaskArtifactLiveness: Send + Sync {
    fn ephemeral_live(&self, owner: &ProjectRef, id: &ArtifactId) -> bool;
    fn durable_live(&self, owner: &ProjectRef, id: &QualityArtifactId) -> bool;
}

impl Tasks {
    pub(super) fn production(
        registry: Arc<Mutex<Registry>>,
        state_root_identity: Option<((i64, u64), u32)>,
        workers: workers::Workers,
        liveness: Arc<dyn TaskArtifactLiveness>,
    ) -> Result<Self, ErrorData> {
        let delivery = Arc::new(InMemoryDeliveryTracker::default());
        let authority_cache = Arc::new(Mutex::new(Vec::new()));
        let executor = Arc::new(JobExecutor::new(
            Arc::new(InMemoryJobRegistry::default()),
            Arc::new(ProductionClock(Instant::now())),
            Arc::new(OsJobIds),
            Arc::new(LiveJobAuthority {
                registry: Arc::clone(&registry),
                state_root_identity,
                bindings: Arc::clone(&authority_cache),
            }),
            delivery,
            Arc::new(TracingJobEvents),
        ));
        let mut tasks = Self::new(executor)?;
        tasks.registry = Some(registry);
        tasks.state_root_identity = state_root_identity;
        tasks.workers = Some(workers);
        tasks.liveness = Some(liveness);
        tasks.authority_cache = Some(authority_cache);
        Ok(tasks)
    }

    #[cfg(test)]
    pub(super) fn dormant() -> Result<Self, ErrorData> {
        Ok(Self {
            executor: None,
            delivery: Arc::new(InMemoryDeliveryTracker::default()),
            registry: None,
            state_root_identity: None,
            workers: None,
            nextest: NextestTool::new()?,
            coverage: CoverageTool::new()?,
            liveness: None,
            authority_cache: None,
        })
    }

    #[allow(dead_code)]
    pub(super) fn new(executor: Arc<JobExecutor>) -> Result<Self, ErrorData> {
        Ok(Self {
            delivery: executor.delivery_tracker(),
            executor: Some(executor),
            registry: None,
            state_root_identity: None,
            workers: None,
            nextest: NextestTool::new()?,
            coverage: CoverageTool::new()?,
            liveness: None,
            authority_cache: None,
        })
    }

    pub(super) fn delivery_tracker(&self) -> Arc<dyn DeliveryTracker> {
        Arc::clone(&self.delivery)
    }

    pub(super) fn executor(&self) -> Option<Arc<JobExecutor>> {
        self.executor.clone()
    }

    pub(super) fn admit(
        &self,
        kind: JobKind,
        project_ref: ProjectRef,
        budget: JobBudget,
        delivery_token: rust_engineering_application::job::DeliveryToken,
        commit: impl FnOnce() -> bool,
    ) -> Result<AdmittedTask, ErrorData> {
        let executor = self.executor.as_ref().ok_or_else(internal)?.clone();
        let registry = self.registry.as_ref().ok_or_else(internal)?;
        let ((state_device, state_inode), uid) = self.state_root_identity.ok_or_else(internal)?;
        let facts = registry
            .try_lock()
            .map_err(|error| match error {
                TryLockError::WouldBlock => ErrorData::internal_error("Task worker busy", None),
                TryLockError::Poisoned(_) => internal(),
            })?
            .quality_owner_facts(&project_ref, &Proceed)
            .map_err(|_| masked())?;
        let owner = JobOwnerBinding::derive(
            (state_device, state_inode),
            uid,
            (facts.granted_root_device, facts.granted_root_inode),
            &facts.workspace_root,
        );
        let workers = self.workers.as_ref().ok_or_else(internal)?;
        let (permit, registry_permit) = workers.reserve_job().map_err(map_worker_admission)?;
        let authority_cache = self.authority_cache.as_ref().ok_or_else(internal)?;
        {
            let mut bindings = authority_cache.lock().map_err(|_| internal())?;
            if let Some(binding) = bindings
                .iter_mut()
                .find(|binding| binding.project_ref == project_ref)
            {
                *binding = LiveAuthorityBinding {
                    project_ref: project_ref.clone(),
                    owner: owner.clone(),
                    facts: facts.clone(),
                };
            } else if bindings.len() < AUTHORITY_CACHE_MAX {
                bindings.push(LiveAuthorityBinding {
                    project_ref: project_ref.clone(),
                    owner: owner.clone(),
                    facts: facts.clone(),
                });
            } else {
                return Err(internal());
            }
        }
        let signal = workers::JobExecutionSignal::new();
        let seed = executor
            .submit_guarded(
                JobSubmission {
                    kind,
                    owner,
                    project_ref,
                    policy_generation: 1,
                    budget,
                    delivery_token,
                    reserved_result_bytes: rust_engineering_domain::job::TASK_RESPONSE_MAX_BYTES,
                    signal: Arc::clone(&signal)
                        as Arc<dyn rust_engineering_application::job::JobSignal>,
                    permit: registry_permit,
                },
                commit,
            )
            .map_err(map_admission)?;
        let created_at = seed.created_at_utc.clone();
        let task = Task::new(
            seed.id.to_string(),
            TaskStatus::Working,
            created_at.clone(),
            created_at,
        )
        .with_status_message(seed.phase.status_message())
        .with_ttl_ms(seed.ttl_ms)
        .with_poll_interval_ms(seed.poll_interval_ms);
        Ok(AdmittedTask {
            id: seed.id,
            response: CreateTaskResult::new(task),
            signal,
            permit,
            executor,
        })
    }

    pub(super) async fn get(&self, task_id: &str) -> Result<GetTaskResult, ErrorData> {
        let id = parse_id(task_id)?;
        let executor = self.executor.as_ref().ok_or_else(masked)?.clone();
        let status = bounded_control(move || executor.status(&id))
            .await
            .map_err(map_lookup)?;
        self.project(status)
    }

    pub(super) async fn cancel(&self, task_id: &str) -> Result<(), ErrorData> {
        let id = parse_id(task_id)?;
        let executor = self.executor.as_ref().ok_or_else(masked)?.clone();
        bounded_control(move || executor.cancel(&id))
            .await
            .map_err(map_lookup)
    }

    pub(super) async fn update(&self, task_id: &str) -> Result<(), ErrorData> {
        let id = parse_id(task_id)?;
        let executor = self.executor.as_ref().ok_or_else(masked)?.clone();
        match bounded_control(move || executor.update(&id)).await {
            Err(JobError::InputRejected) => Err(ErrorData::invalid_params(INPUT_REJECTED, None)),
            Err(error) => Err(map_lookup(error)),
            Ok(()) => Err(ErrorData::invalid_params(INPUT_REJECTED, None)),
        }
    }

    fn project(&self, status: JobStatus) -> Result<GetTaskResult, ErrorData> {
        let task = Task::new(
            status.id.to_string(),
            wire_state(status.state),
            status.created_at_utc,
            status.updated_at_utc,
        )
        .with_status_message(status.phase.status_message())
        .with_ttl_ms(status.ttl_ms)
        .with_poll_interval_ms(status.poll_interval_ms);
        let payload = match status.state {
            JobState::Admitted | JobState::Running => TaskPayload::Working,
            JobState::Cancelled => TaskPayload::Cancelled,
            JobState::Completed => {
                let Some(JobCompletion::ToolResult { result, is_error }) = status.completion else {
                    return Err(internal());
                };
                let result = match result {
                    JobResult::TestNextest(result) => {
                        let result = self.refresh_artifacts(&status.project_ref, result)?;
                        self.nextest
                            .encode_task_result(&status.project_ref, result, is_error)?
                    }
                    JobResult::Coverage(result) => {
                        self.coverage.encode_result(&status.project_ref, result)?
                    }
                    JobResult::QualityTool(result) => {
                        let structured = refresh_encoded_artifacts(
                            self.liveness.as_deref(),
                            &status.project_ref,
                            result,
                        )?;
                        if is_error {
                            CallToolResult::structured_error(structured)
                        } else {
                            CallToolResult::structured(structured)
                        }
                    }
                };
                TaskPayload::Completed {
                    result: object(serde_json::to_value(result).map_err(|_| internal())?)?,
                }
            }
            JobState::Failed => {
                let failure = match status.completion {
                    Some(JobCompletion::InfrastructureFailure(failure)) => failure,
                    _ => JobInfrastructureFailure::Internal,
                };
                TaskPayload::Failed {
                    error: failure_object(failure)?,
                }
            }
        };
        Ok(GetTaskResult::new(DetailedTask::new(task, payload)))
    }

    fn refresh_artifacts(
        &self,
        owner: &ProjectRef,
        result: NextestTaskResult,
    ) -> Result<NextestTaskResult, ErrorData> {
        let Some(liveness) = &self.liveness else {
            return Ok(result);
        };
        let artifacts = result
            .artifacts()
            .iter()
            .cloned()
            .map(|artifact| match artifact {
                NextestArtifactReference::Ephemeral { kind, metadata }
                    if !liveness.ephemeral_live(owner, &metadata.id) =>
                {
                    NextestArtifactReference::EphemeralUnavailable { kind, metadata }
                }
                NextestArtifactReference::Durable(mut descriptor)
                    if !liveness.durable_live(owner, &descriptor.artifact_id) =>
                {
                    descriptor.completeness =
                        rust_engineering_domain::ArtifactCompleteness::Unavailable;
                    NextestArtifactReference::Durable(descriptor)
                }
                artifact => artifact,
            })
            .collect();
        result.replace_artifacts(artifacts).map_err(|_| internal())
    }
}

fn map_worker_admission(error: workers::WorkerError) -> ErrorData {
    match error {
        workers::WorkerError::Busy => ErrorData::internal_error("Task worker busy", None),
        workers::WorkerError::Cancelled
        | workers::WorkerError::TimedOut
        | workers::WorkerError::Internal => internal(),
    }
}

fn map_admission(error: JobError) -> ErrorData {
    match error {
        JobError::Busy | JobError::QuotaExceeded => {
            ErrorData::internal_error("Task worker busy", None)
        }
        JobError::Unavailable => masked(),
        _ => internal(),
    }
}

fn refresh_encoded_artifacts(
    liveness: Option<&dyn TaskArtifactLiveness>,
    owner: &ProjectRef,
    result: QualityToolResult,
) -> Result<serde_json::Value, ErrorData> {
    let mut value: serde_json::Value =
        serde_json::from_str(result.json_object()).map_err(|_| internal())?;
    let Some(liveness) = liveness else {
        return value.is_object().then_some(value).ok_or_else(internal);
    };
    let Some(artifacts) = value
        .get_mut("data")
        .and_then(|data| data.get_mut("artifacts"))
        .and_then(serde_json::Value::as_array_mut)
    else {
        return value.is_object().then_some(value).ok_or_else(internal);
    };
    for artifact in artifacts {
        let Some(uri) = artifact.get("uri").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let live = if let Some(locator) = uri.strip_prefix("rust-artifact://") {
            locator
                .split_once('/')
                .and_then(|(uri_owner, id)| {
                    (uri_owner == owner.as_str())
                        .then(|| id.parse::<ArtifactId>().ok())
                        .flatten()
                })
                .is_some_and(|id| liveness.ephemeral_live(owner, &id))
        } else if let Some(locator) = uri.strip_prefix("rust-quality-artifact://") {
            locator
                .split_once('/')
                .and_then(|(uri_owner, rest)| {
                    let id = rest.split('?').next()?;
                    (uri_owner == owner.as_str())
                        .then(|| id.parse::<QualityArtifactId>().ok())
                        .flatten()
                })
                .is_some_and(|id| liveness.durable_live(owner, &id))
        } else {
            true
        };
        if !live {
            artifact["completeness"] = serde_json::Value::String("unavailable".into());
        }
    }
    value.is_object().then_some(value).ok_or_else(internal)
}

async fn bounded_control<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, JobError> + Send + 'static,
) -> Result<T, JobError> {
    bounded_control_with(TASK_CONTROL_DEFAULT_MS, work).await
}

async fn bounded_control_with<T: Send + 'static>(
    requested_ms: u64,
    work: impl FnOnce() -> Result<T, JobError> + Send + 'static,
) -> Result<T, JobError> {
    let budget = task_control_budget(requested_ms)?;
    match tokio::time::timeout(
        std::time::Duration::from_millis(budget.0),
        tokio::task::spawn_blocking(work),
    )
    .await
    {
        Ok(Ok(result)) => result,
        Ok(Err(_)) | Err(_) => Err(JobError::Internal),
    }
}

fn task_control_budget(requested_ms: u64) -> Result<Milliseconds, JobError> {
    (requested_ms > 0 && requested_ms <= TASK_CONTROL_MAX_MS)
        .then_some(Milliseconds(requested_ms))
        .ok_or(JobError::InvalidConfiguration)
}

struct ProductionClock(Instant);
impl JobClock for ProductionClock {
    fn monotonic_millis(&self) -> Milliseconds {
        Milliseconds(self.0.elapsed().as_millis().try_into().unwrap_or(u64::MAX))
    }
    fn utc_now(&self) -> Result<String, JobError> {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| JobError::ClockFailure)?
            .as_secs();
        UtcInstant::from_unix_seconds(seconds)
            .map(|value| value.to_string())
            .map_err(|_| JobError::ClockFailure)
    }
}

struct OsJobIds;
impl JobIds for OsJobIds {
    fn random_128(&self) -> Result<[u8; 16], JobError> {
        let mut bytes = [0_u8; 16];
        getrandom::fill(&mut bytes).map_err(|_| JobError::EntropyFailure)?;
        Ok(bytes)
    }
}

struct Proceed;
impl OperationControl for Proceed {
    fn check(&self) -> Result<(), ProjectError> {
        Ok(())
    }
}

struct LiveJobAuthority {
    registry: Arc<Mutex<Registry>>,
    state_root_identity: Option<((i64, u64), u32)>,
    bindings: Arc<Mutex<Vec<LiveAuthorityBinding>>>,
}
impl JobAuthority for LiveJobAuthority {
    fn revalidate(
        &self,
        owner: &JobOwnerBinding,
        project: &ProjectRef,
        policy_generation: u64,
    ) -> bool {
        if policy_generation != 1 {
            return false;
        }
        let Some((state_root, uid)) = self.state_root_identity else {
            return false;
        };
        let cached = self.bindings.lock().ok().and_then(|bindings| {
            bindings
                .iter()
                .find(|binding| binding.project_ref == *project && binding.owner == *owner)
                .cloned()
        });
        let Some(cached) = cached else {
            return false;
        };
        if !root_identity_is_live(&cached.facts) {
            return false;
        }
        match self.registry.try_lock() {
            Ok(mut registry) => registry
                .quality_owner_facts(project, &Proceed)
                .ok()
                .is_some_and(|facts| {
                    facts == cached.facts
                        && JobOwnerBinding::derive(
                            state_root,
                            uid,
                            (facts.granted_root_device, facts.granted_root_inode),
                            &facts.workspace_root,
                        ) == *owner
                }),
            // A quality operation holds this registry while it works. The
            // admission-time binding plus a fresh no-follow root identity probe
            // keeps task control non-blocking; terminal publication performs the
            // full registry revalidation once the operation releases the lock.
            Err(TryLockError::WouldBlock) => true,
            Err(TryLockError::Poisoned(_)) => false,
        }
    }
}

#[cfg(unix)]
fn root_identity_is_live(facts: &QualityOwnerFacts) -> bool {
    use std::os::unix::fs::MetadataExt;

    std::fs::symlink_metadata(&facts.workspace_root).is_ok_and(|metadata| {
        metadata.file_type().is_dir()
            && i64::try_from(metadata.dev()).ok() == Some(facts.granted_root_device)
            && metadata.ino() == facts.granted_root_inode
    })
}

#[cfg(not(unix))]
fn root_identity_is_live(_: &QualityOwnerFacts) -> bool {
    false
}

fn wire_state(state: JobState) -> TaskStatus {
    match state {
        JobState::Admitted | JobState::Running => TaskStatus::Working,
        JobState::Completed => TaskStatus::Completed,
        JobState::Failed => TaskStatus::Failed,
        JobState::Cancelled => TaskStatus::Cancelled,
    }
}

fn parse_id(value: &str) -> Result<JobId, ErrorData> {
    value.parse().map_err(|_| masked())
}

fn map_lookup(error: JobError) -> ErrorData {
    match error {
        JobError::Unavailable => masked(),
        JobError::InputRejected => ErrorData::invalid_params(INPUT_REJECTED, None),
        JobError::CleanupUncertain | JobError::Internal => internal(),
        JobError::Busy
        | JobError::QuotaExceeded
        | JobError::InvalidConfiguration
        | JobError::ClockFailure
        | JobError::EntropyFailure
        | JobError::DeadlineExceeded => internal(),
    }
}

fn masked() -> ErrorData {
    ErrorData::invalid_params(UNAVAILABLE, None)
}

fn internal() -> ErrorData {
    ErrorData::internal_error("Task registry failed", None)
}

fn object(value: serde_json::Value) -> Result<JsonObject, ErrorData> {
    value.as_object().cloned().ok_or_else(internal)
}

fn failure_object(failure: JobInfrastructureFailure) -> Result<JsonObject, ErrorData> {
    let message = match failure {
        JobInfrastructureFailure::Internal => "task failed",
        JobInfrastructureFailure::TimedOut => "task timed out after cleanup",
        JobInfrastructureFailure::CleanupFailed => "task cleanup failed",
        JobInfrastructureFailure::ResultUnavailable => "task result unavailable",
    };
    object(serde_json::json!({"code": -32603, "message": message}))
}

#[cfg(test)]
pub(crate) mod tests;
