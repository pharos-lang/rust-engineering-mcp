use super::*;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CancelTaskParams, CreateTaskResult, ErrorCode,
    GetTaskParams, Implementation, ProtocolVersion, ServerInfo, Task, TaskStatus, UpdateTaskParams,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ServerHandler, ServiceExt};
use rust_engineering_application::{
    job::{
        CleanupObservation, DeliveryToken, InMemoryDeliveryTracker, InMemoryJobRegistry,
        JobAuthority, JobClock, JobEvents, JobIds, JobPermit, JobResult, JobSignal, JobSubmission,
        QualityToolResult,
    },
    nextest::{
        ArtifactStreams, NextestCompleteness, NextestCounts, NextestObservation, NextestOptions,
        NextestSelection, NextestTaskResult,
    },
};
use rust_engineering_domain::{
    ArtifactMetadata, ExecutionFingerprint, ExecutionTermination, RuntimeIdentity,
    job::{JobBudget, JobCompletion, JobKind, JobOwnerBinding, JobPhase, Milliseconds},
};
use serde_json::{Value, json};
use std::borrow::Cow;
use std::collections::BTreeSet;
use std::error::Error;
use std::io;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, ReadHalf, WriteHalf};

#[derive(Clone)]
struct TraceCapture(Arc<Mutex<Vec<u8>>>);
struct TraceWriter(Arc<Mutex<Vec<u8>>>);
impl std::io::Write for TraceWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .map_err(|_| std::io::Error::other("trace lock"))?
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct DeadArtifacts;
impl TaskArtifactLiveness for DeadArtifacts {
    fn ephemeral_live(&self, _: &rust_engineering_domain::ProjectRef, _: &ArtifactId) -> bool {
        false
    }
    fn durable_live(&self, _: &rust_engineering_domain::ProjectRef, _: &QualityArtifactId) -> bool {
        false
    }
}
impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for TraceCapture {
    type Writer = TraceWriter;
    fn make_writer(&'a self) -> Self::Writer {
        TraceWriter(Arc::clone(&self.0))
    }
}

#[derive(Default)]
struct TestClock(AtomicU64);
impl JobClock for TestClock {
    fn monotonic_millis(&self) -> Milliseconds {
        Milliseconds(self.0.load(Ordering::Acquire))
    }
    fn utc_now(&self) -> Result<String, JobError> {
        Ok("2026-09-05T12:00:00Z".into())
    }
}
#[derive(Default)]
struct TestIds(AtomicU64);
impl JobIds for TestIds {
    fn random_128(&self) -> Result<[u8; 16], JobError> {
        let mut bytes = [0; 16];
        bytes[8..].copy_from_slice(&(self.0.fetch_add(1, Ordering::AcqRel) + 1).to_be_bytes());
        Ok(bytes)
    }
}
struct TestSignal(AtomicBool);
impl JobSignal for TestSignal {
    fn request_cancellation(&self) {}
    fn cancellation_requested(&self) -> bool {
        false
    }
    fn cleanup_observed(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
    fn join_cleanup(&self, _: Milliseconds) -> CleanupObservation {
        if self.cleanup_observed() {
            CleanupObservation::Observed
        } else {
            CleanupObservation::Uncertain
        }
    }
}
struct TestPermit(AtomicBool);
impl JobPermit for TestPermit {
    fn is_held(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
    fn release_after_cleanup(&self) {
        self.0.store(false, Ordering::Release);
    }
}
struct TestAuthority(Mutex<BTreeSet<u8>>);
impl JobAuthority for TestAuthority {
    fn revalidate(
        &self,
        owner: &JobOwnerBinding,
        _: &rust_engineering_domain::ProjectRef,
        _: u64,
    ) -> bool {
        self.0
            .lock()
            .is_ok_and(|allowed| allowed.contains(&owner.digest()[0]))
    }
}
struct NoEvents;
impl JobEvents for NoEvents {
    fn record(&self, _: rust_engineering_application::job::JobEvent) {}
}

fn test_executor() -> (Arc<JobExecutor>, Arc<TestClock>, Arc<TestAuthority>) {
    let clock = Arc::new(TestClock::default());
    let authority = Arc::new(TestAuthority(Mutex::new(BTreeSet::from([7, 8]))));
    let executor = Arc::new(JobExecutor::new(
        Arc::new(InMemoryJobRegistry::default()),
        clock.clone(),
        Arc::new(TestIds::default()),
        authority.clone(),
        Arc::new(InMemoryDeliveryTracker::default()),
        Arc::new(NoEvents),
    ));
    (executor, clock, authority)
}

pub(crate) fn running_tasks_for_resource_test()
-> Result<(Tasks, String), Box<dyn std::error::Error>> {
    let (executor, _, _) = test_executor();
    let id = executor.submit(submission(7, 97)?)?.id;
    executor.start(&id)?;
    Ok((Tasks::new(executor)?, id.to_string()))
}

fn submission(owner: u8, token: u64) -> Result<JobSubmission, Box<dyn std::error::Error>> {
    Ok(JobSubmission {
        kind: JobKind::TestNextest,
        owner: JobOwnerBinding::new([owner; 32]),
        project_ref: "prj_00000000000000000000000000000001".parse()?,
        policy_generation: 1,
        budget: JobBudget::asynchronous_default()?,
        delivery_token: DeliveryToken::new(token).ok_or("delivery token")?,
        reserved_result_bytes: 512 * 1024,
        signal: Arc::new(TestSignal(AtomicBool::new(true))),
        permit: Arc::new(TestPermit(AtomicBool::new(true))),
    })
}

fn completion() -> Result<JobCompletion<JobResult>, Box<dyn std::error::Error>> {
    let status = completed_status(false)?;
    status.completion.ok_or_else(|| "completion missing".into())
}

fn encoded_completion(
    is_error: bool,
) -> Result<JobCompletion<JobResult>, Box<dyn std::error::Error>> {
    Ok(JobCompletion::ToolResult {
        result: JobResult::QualityTool(QualityToolResult::new(serde_json::to_string(
            &serde_json::json!({
                "status": if is_error { "blocked" } else { "passed" },
                "error_code": if is_error { serde_json::Value::String("SANDBOX_DENIED".into()) } else { serde_json::Value::Null },
                "error_message": serde_json::Value::Null,
                "data": serde_json::Value::Null,
                "summary": "bounded",
                "duration_ms": 1
            }),
        )?)?),
        is_error,
    })
}

fn completed_status(is_error: bool) -> Result<JobStatus, Box<dyn std::error::Error>> {
    let execution_fingerprint =
        format!("sha256:{}", "3".repeat(64)).parse::<ExecutionFingerprint>()?;
    Ok(JobStatus {
        id: "job_00000000000000000000000000000001".parse()?,
        kind: JobKind::TestNextest,
        project_ref: "prj_00000000000000000000000000000001".parse()?,
        state: JobState::Completed,
        phase: JobPhase::Terminal,
        created_at_utc: "2026-09-05T12:00:00Z".to_owned(),
        updated_at_utc: "2026-09-05T12:00:01Z".to_owned(),
        ttl_ms: 7_200_000,
        poll_interval_ms: 1_000,
        completion: Some(JobCompletion::ToolResult {
            result: JobResult::TestNextest(NextestTaskResult::new(
                NextestObservation {
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
                        execution_fingerprint: execution_fingerprint.clone(),
                        rust_version: "rustc 1.98.1".to_owned(),
                        cargo_version: "cargo 1.98.1".to_owned(),
                        declared_toolchain: None,
                    },
                    execution_fingerprint,
                    artifacts: ArtifactStreams::default(),
                },
                Vec::new(),
                1,
            )?),
            is_error,
        }),
    })
}

#[tokio::test]
async fn malformed_unknown_foreign_expired_and_revoked_share_one_mask() -> Result<(), ErrorData> {
    let tasks = Tasks::dormant()?;
    for id in [
        "bad",
        "job_00000000000000000000000000000000",
        "job_ffffffffffffffffffffffffffffffff",
    ] {
        let error = tasks.get(id).await.err().ok_or_else(internal)?;
        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
        assert_eq!(error.message, UNAVAILABLE);
        assert!(error.data.is_none());
        assert!(!error.message.contains(id));
    }
    Ok(())
}

#[tokio::test]
async fn update_masks_unknown_ids_without_echo() -> Result<(), ErrorData> {
    let tasks = Tasks::dormant()?;
    let unknown = "job_00000000000000000000000000000000";
    let error = tasks.update(unknown).await.err().ok_or_else(internal)?;
    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(error.message, UNAVAILABLE);
    assert!(error.data.is_none());
    Ok(())
}

#[test]
fn task_control_budget_accepts_just_under_five_seconds_and_rejects_one_over() {
    assert_eq!(task_control_budget(4_999), Ok(Milliseconds(4_999)));
    assert_eq!(task_control_budget(5_000), Ok(Milliseconds(5_000)));
    assert_eq!(
        task_control_budget(5_001),
        Err(JobError::InvalidConfiguration)
    );
    assert_eq!(task_control_budget(0), Err(JobError::InvalidConfiguration));
}

// The budget is enforced with a wall-clock timeout around `spawn_blocking`, so
// work that finishes only just inside it is decided by thread-pool scheduling
// latency rather than by the budget. Each case therefore sits comfortably on
// one side of its boundary: the default (2s) and maximum (5s) budgets are still
// both exercised in the completing and the exceeding direction.
#[tokio::test]
async fn task_control_deadlines_enforce_default_and_maximum_boundaries() {
    assert_eq!(
        bounded_control_with(2_000, || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            Ok(7)
        })
        .await,
        Ok(7)
    );
    assert_eq!(
        bounded_control_with(2_000, || {
            std::thread::sleep(std::time::Duration::from_millis(2_100));
            Ok(7)
        })
        .await,
        Err(JobError::Internal)
    );
    assert_eq!(
        bounded_control_with(5_000, || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            Ok(9)
        })
        .await,
        Ok(9)
    );
    assert_eq!(
        bounded_control_with(5_000, || {
            std::thread::sleep(std::time::Duration::from_millis(5_100));
            Ok(9)
        })
        .await,
        Err(JobError::Internal)
    );
}

#[tokio::test]
async fn active_job_keeps_task_controls_responsive_without_worker_admission()
-> Result<(), Box<dyn std::error::Error>> {
    let (executor, _, _) = test_executor();
    let id = executor.submit(submission(7, 41)?)?.id;
    executor.start(&id)?;
    let tasks = Tasks::new(executor)?;

    let status = tasks.get(id.as_str()).await?;
    assert_eq!(status.task.status(), TaskStatus::Working);

    let update = tasks
        .update(id.as_str())
        .await
        .err()
        .ok_or("task update unexpectedly succeeded")?;
    assert_eq!(update.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(update.message, INPUT_REJECTED);
    assert!(update.data.is_none());

    tasks.cancel(id.as_str()).await?;
    assert_eq!(
        tasks.get(id.as_str()).await?.task.status(),
        TaskStatus::Working
    );
    Ok(())
}

#[tokio::test]
async fn encoded_quality_result_preserves_tool_payload_and_is_error_at_terminal_poll()
-> Result<(), Box<dyn std::error::Error>> {
    for is_error in [false, true] {
        let (executor, _, _) = test_executor();
        let id = executor
            .submit(submission(7, if is_error { 52 } else { 51 })?)?
            .id;
        executor.start(&id)?;
        executor.finish(
            &id,
            encoded_completion(is_error)?,
            512,
            CleanupObservation::Observed,
        )?;
        let projected = Tasks::new(executor)?.get(id.as_str()).await?;
        let value = serde_json::to_value(projected)?;
        assert_eq!(value["status"], "completed");
        assert_eq!(value["result"]["isError"], is_error);
        assert_eq!(
            value["result"]["structuredContent"]["status"],
            if is_error { "blocked" } else { "passed" }
        );
        let mirrored: serde_json::Value = serde_json::from_str(
            value["result"]["content"][0]["text"]
                .as_str()
                .ok_or("missing text mirror")?,
        )?;
        assert_eq!(mirrored, value["result"]["structuredContent"]);
    }
    Ok(())
}

const PRODUCT_VERSIONS: [&str; 5] = [
    "2024-11-05",
    "2025-03-26",
    "2025-06-18",
    "2025-11-25",
    "2026-07-28",
];
const PRODUCT_TASKS_EXTENSION: &str = "io.modelcontextprotocol/tasks";

struct ProductTasksHarness {
    tasks: Tasks,
    seed: CreateTaskResult,
    advertised: bool,
}

impl ServerHandler for ProductTasksHarness {
    async fn call_tool(
        &self,
        _: CallToolRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        Ok(CallToolResponse::Task(self.seed.clone()))
    }

    async fn get_task(
        &self,
        request: GetTaskParams,
        _: RequestContext<RoleServer>,
    ) -> Result<GetTaskResult, ErrorData> {
        self.tasks.get(&request.task_id).await
    }

    async fn cancel_task(
        &self,
        request: CancelTaskParams,
        _: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        self.tasks.cancel(&request.task_id).await
    }

    async fn update_task(
        &self,
        request: UpdateTaskParams,
        _: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        self.tasks.update(&request.task_id).await
    }

    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(super::super::server_capabilities(self.advertised))
            .with_server_info(Implementation::new("rust-engineering-mcp", "test"))
    }

    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(super::super::SUPPORTED_VERSIONS)
    }
}

struct ProductWireClient {
    reader: BufReader<ReadHalf<DuplexStream>>,
    writer: WriteHalf<DuplexStream>,
}

impl ProductWireClient {
    async fn request(&mut self, value: Value, id: i64) -> Result<Value, Box<dyn Error>> {
        let mut bytes = serde_json::to_vec(&value)?;
        bytes.push(b'\n');
        self.writer.write_all(&bytes).await?;
        self.writer.flush().await?;
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(2), self.reader.read_line(&mut line))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "task response timed out"))??;
        let response: Value = serde_json::from_str(&line)?;
        assert_eq!(response["id"], id);
        Ok(response)
    }

    async fn notify(&mut self, value: Value) -> Result<(), Box<dyn Error>> {
        let mut bytes = serde_json::to_vec(&value)?;
        bytes.push(b'\n');
        self.writer.write_all(&bytes).await?;
        self.writer.flush().await?;
        Ok(())
    }
}

fn product_capabilities(tasks: bool) -> Value {
    if tasks {
        json!({"extensions": {(PRODUCT_TASKS_EXTENSION): {}}})
    } else {
        json!({})
    }
}

fn product_request(version: &str, id: i64, method: &str, params: Value, tasks: bool) -> Value {
    if version == "2026-07-28" {
        let mut params = params;
        params["_meta"] = json!({
            "io.modelcontextprotocol/protocolVersion": version,
            "io.modelcontextprotocol/clientCapabilities": product_capabilities(tasks)
        });
        json!({"jsonrpc":"2.0", "id":id, "method":method, "params":params})
    } else {
        json!({"jsonrpc":"2.0", "id":id, "method":method, "params":params})
    }
}

async fn product_bootstrap(
    client: &mut ProductWireClient,
    version: &str,
    tasks: bool,
) -> Result<Value, Box<dyn Error>> {
    if version == "2026-07-28" {
        client
            .request(
                product_request(version, 1, "server/discover", json!({}), tasks),
                1,
            )
            .await
    } else {
        let response = client
            .request(
                json!({
                    "jsonrpc":"2.0",
                    "id":1,
                    "method":"initialize",
                    "params":{
                        "protocolVersion":version,
                        "capabilities":product_capabilities(tasks),
                        "clientInfo":{"name":"rust-mcp-product-task-test","version":"1"}
                    }
                }),
                1,
            )
            .await?;
        client
            .notify(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .await?;
        Ok(response)
    }
}

async fn product_task_server(
    advertised: bool,
) -> Result<(ProductWireClient, tokio::task::JoinHandle<bool>, String), Box<dyn Error>> {
    let (executor, _, _) = test_executor();
    let seed = executor.submit(submission(7, 80)?)?;
    executor.start(&seed.id)?;
    let task = Task::new(
        seed.id.to_string(),
        TaskStatus::Working,
        seed.created_at_utc.clone(),
        seed.created_at_utc,
    )
    .with_status_message(seed.phase.status_message())
    .with_ttl_ms(seed.ttl_ms)
    .with_poll_interval_ms(seed.poll_interval_ms);
    let server = ProductTasksHarness {
        tasks: Tasks::new(executor)?,
        seed: CreateTaskResult::new(task),
        advertised,
    };
    let task_id = seed.id.to_string();
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        match server.serve(server_transport).await {
            Ok(service) => service.waiting().await.is_ok(),
            Err(_) => false,
        }
    });
    let (reader, writer) = tokio::io::split(client_transport);
    Ok((
        ProductWireClient {
            reader: BufReader::new(reader),
            writer,
        },
        server_task,
        task_id,
    ))
}

#[tokio::test]
async fn product_advertisement_and_negotiation_cover_five_versions_on_both_switch_sides()
-> Result<(), Box<dyn Error>> {
    for version in PRODUCT_VERSIONS {
        for advertised in [false, true] {
            for declared in [false, true] {
                let (mut client, server, task_id) = product_task_server(advertised).await?;
                let bootstrap = product_bootstrap(&mut client, version, declared).await?;
                let extension =
                    &bootstrap["result"]["capabilities"]["extensions"][PRODUCT_TASKS_EXTENSION];
                assert_eq!(extension == &json!({}), advertised);

                if advertised && declared {
                    let created = client
                        .request(
                            product_request(
                                version,
                                2,
                                "tools/call",
                                json!({"name":"fixture","arguments":{}}),
                                true,
                            ),
                            2,
                        )
                        .await?;
                    assert_eq!(created["result"]["taskId"], task_id, "{created}");
                    assert_eq!(created["result"]["status"], "working", "{created}");
                }

                let response = client
                    .request(
                        product_request(
                            version,
                            3,
                            "tasks/get",
                            json!({"taskId":task_id}),
                            declared,
                        ),
                        3,
                    )
                    .await?;
                match (advertised, declared) {
                    (false, _) => assert_eq!(response["error"]["code"], -32601),
                    (true, false) => {
                        assert_eq!(response["error"]["code"], -32021);
                        assert_eq!(
                            response["error"]["data"]["requiredCapabilities"]["extensions"]
                                [PRODUCT_TASKS_EXTENSION],
                            json!({})
                        );
                    }
                    (true, true) => assert_eq!(response["result"]["status"], "working"),
                }
                drop(client);
                assert!(tokio::time::timeout(Duration::from_secs(2), server).await??);
            }
        }
    }
    Ok(())
}

#[test]
fn tracing_job_events_emit_only_closed_bounded_fields() -> Result<(), Box<dyn std::error::Error>> {
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_writer(TraceCapture(Arc::clone(&bytes)))
        .finish();
    tracing::subscriber::with_default(subscriber, || {
        TracingJobEvents.record(rust_engineering_application::job::JobEvent {
            job_id: Some(JobId::from_random_bytes([1; 16])),
            kind: JobKind::TestNextest,
            event: rust_engineering_application::job::JobEventKind::Started,
            phase: JobPhase::Execute,
            state: JobState::Running,
            reason: rust_engineering_application::job::JobEventReason::None,
            elapsed_ms: 17,
            budget_ms: 300_000,
            retained_bytes: 512,
            retained_entries: 1,
        });
    });
    let rendered = String::from_utf8(bytes.lock().map_err(|_| "trace lock")?.clone())?;
    for field in [
        "job_id=",
        "kind=TestNextest",
        "event=Started",
        "phase=Execute",
        "state=Running",
        "reason=None",
        "elapsed_ms=17",
        "budget_ms=300000",
        "retained_bytes=512",
        "retained_entries=1",
    ] {
        assert!(rendered.contains(field), "missing {field}: {rendered}");
    }
    for forbidden in ["prj_", "/source", "argument", "structuredContent"] {
        assert!(
            !rendered.contains(forbidden),
            "leaked {forbidden}: {rendered}"
        );
    }
    Ok(())
}

#[test]
fn authorized_update_has_the_fixed_non_mutating_error() {
    let error = map_lookup(JobError::InputRejected);
    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(error.message, INPUT_REJECTED);
    assert!(error.data.is_none());
}

#[test]
fn task_metadata_always_has_fixed_non_null_ttl() {
    let task = Task::new(
        "job_00000000000000000000000000000000",
        TaskStatus::Working,
        "2026-09-05T12:00:00Z",
        "2026-09-05T12:00:00Z",
    )
    .with_ttl_ms(rust_engineering_domain::job::TASK_RECORD_TTL_MS)
    .with_poll_interval_ms(rust_engineering_domain::job::TASK_POLL_INTERVAL_MS);
    let value = serde_json::to_value(task).unwrap_or_default();
    assert_eq!(value["ttlMs"], 7_200_000);
    assert!(!value["ttlMs"].is_null());
    assert_eq!(value["pollIntervalMs"], 1_000);
}

#[test]
fn ordinary_error_tool_result_is_a_completed_task() {
    assert_eq!(wire_state(JobState::Completed), TaskStatus::Completed);
    assert_eq!(wire_state(JobState::Failed), TaskStatus::Failed);
    assert_eq!(wire_state(JobState::Cancelled), TaskStatus::Cancelled);
}

#[test]
fn ordinary_is_error_payload_remains_mcp_task_completed() -> Result<(), Box<dyn std::error::Error>>
{
    let projected = Tasks::dormant()?.project(completed_status(true)?)?;
    let wire = serde_json::to_value(projected)?;
    assert_eq!(wire["status"], "completed");
    assert_eq!(wire["result"]["isError"], true);
    assert_eq!(wire["result"]["structuredContent"]["status"], "blocked");
    Ok(())
}

#[test]
fn expired_stage0_member_projects_unavailable_without_changing_task_outcome()
-> Result<(), Box<dyn std::error::Error>> {
    let owner: rust_engineering_domain::ProjectRef =
        "prj_00000000000000000000000000000001".parse()?;
    let mut observed = observation_for_liveness()?;
    observed.artifacts.junit_xml = b"<testsuites/>".to_vec();
    let result = NextestTaskResult::new(
        observed,
        vec![NextestArtifactReference::Ephemeral {
            kind: rust_engineering_application::nextest::NextestArtifactKind::JunitXml,
            metadata: ArtifactMetadata {
                owner: owner.clone(),
                id: "art_00000000000000000000000000000002".parse()?,
                sha256: [7; 32],
                size_bytes: 13,
                truncated: false,
                created_seconds: 1,
                expires_seconds: 2,
            },
        }],
        1,
    )?;
    let mut tasks = Tasks::dormant()?;
    tasks.liveness = Some(Arc::new(DeadArtifacts));
    let projected = tasks.refresh_artifacts(&owner, result)?;
    assert!(matches!(
        projected.artifacts(),
        [NextestArtifactReference::EphemeralUnavailable { .. }]
    ));
    Ok(())
}

fn observation_for_liveness() -> Result<NextestObservation, Box<dyn std::error::Error>> {
    let execution_fingerprint =
        format!("sha256:{}", "3".repeat(64)).parse::<ExecutionFingerprint>()?;
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
            platform: "linux-aarch64".into(),
            image_id: format!("sha256:{}", "1".repeat(64)),
            configuration_fingerprint: format!("sha256:{}", "2".repeat(64)).parse()?,
            execution_fingerprint: execution_fingerprint.clone(),
            rust_version: "rustc 1.98.1".into(),
            cargo_version: "cargo 1.98.1".into(),
            declared_toolchain: None,
        },
        execution_fingerprint,
        artifacts: ArtifactStreams::default(),
    })
}

#[tokio::test]
async fn live_executor_masks_malformed_unknown_revoked_foreign_and_expired_ids_byte_identically()
-> Result<(), Box<dyn std::error::Error>> {
    let (executor, _, authority) = test_executor();
    let allowed = executor.submit(submission(7, 1)?)?.id;
    executor.finish(&allowed, completion()?, 1, CleanupObservation::Observed)?;
    let denied = executor.submit(submission(8, 2)?)?.id;
    executor.finish(&denied, completion()?, 1, CleanupObservation::Observed)?;
    authority.0.lock().map_err(|_| "authority")?.remove(&8);
    let tasks = Tasks::new(executor)?;
    assert!(tasks.get(allowed.as_str()).await.is_ok());
    let malformed = tasks
        .get("bad")
        .await
        .err()
        .ok_or("malformed was visible")?;
    let expected = serde_json::to_vec(&malformed)?;
    for id in ["job_ffffffffffffffffffffffffffffffff", denied.as_str()] {
        let error = tasks.get(id).await.err().ok_or("denied task was visible")?;
        assert_eq!(serde_json::to_vec(&error)?, expected);
        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
        assert_eq!(error.message, UNAVAILABLE);
        assert!(error.data.is_none());
    }

    let (expired_executor, clock, _) = test_executor();
    let expired = expired_executor.submit(submission(7, 3)?)?.id;
    expired_executor.finish(&expired, completion()?, 1, CleanupObservation::Observed)?;
    clock.0.store(7_200_000, Ordering::Release);
    expired_executor.watchdog()?;
    let expired_tasks = Tasks::new(expired_executor)?;
    let error = expired_tasks
        .get(expired.as_str())
        .await
        .err()
        .ok_or("expired visible")?;
    assert_eq!(serde_json::to_vec(&error)?, expected);
    Ok(())
}

#[test]
fn every_job_state_projects_its_declared_task_status() -> Result<(), Box<dyn std::error::Error>> {
    let tasks = Tasks::dormant()?;
    for (state, status) in [
        (JobState::Admitted, "working"),
        (JobState::Running, "working"),
        (JobState::Cancelled, "cancelled"),
    ] {
        let mut job = completed_status(false)?;
        job.state = state;
        let projected = serde_json::to_value(tasks.project(job)?)?;
        assert_eq!(projected["status"], status);
        assert_eq!(projected["taskId"], "job_00000000000000000000000000000001");
        assert_eq!(projected["ttlMs"], 7_200_000);
        assert_eq!(projected["pollIntervalMs"], 1_000);
        // Only a completed job carries a result; a working or cancelled one
        // never leaks a partial tool payload.
        assert_eq!(projected.get("result"), None);
        assert_eq!(projected.get("error"), None);
    }
    // An infrastructure failure is a failed task with a bounded reason, not a
    // completed one carrying a result.
    let mut failed = completed_status(false)?;
    failed.state = JobState::Failed;
    failed.completion = Some(JobCompletion::InfrastructureFailure(
        JobInfrastructureFailure::Internal,
    ));
    let projected = serde_json::to_value(tasks.project(failed)?)?;
    assert_eq!(projected["status"], "failed");
    assert!(projected.get("error").is_some());
    assert_eq!(projected.get("result"), None);
    // A job that claims completion without a tool result is not projectable.
    let mut empty = completed_status(false)?;
    empty.completion = None;
    assert!(tasks.project(empty).is_err());
    Ok(())
}

#[test]
fn a_dead_artifact_is_reported_unavailable_and_a_foreign_uri_is_left_alone()
-> Result<(), Box<dyn std::error::Error>> {
    let owner: rust_engineering_domain::ProjectRef =
        "prj_00000000000000000000000000000001".parse()?;
    let ephemeral =
        "rust-artifact://prj_00000000000000000000000000000001/art_00000000000000000000000000000002";
    let durable = "rust-quality-artifact://prj_00000000000000000000000000000001/qart_0123456789abcdef0123456789abcdef?offset=0&length=16";
    let encoded = |uris: &[&str]| {
        QualityToolResult::new(
            serde_json::json!({
                "status": "passed",
                "data": {
                    "artifacts": uris
                        .iter()
                        .map(|uri| serde_json::json!({"uri": uri, "completeness": "complete"}))
                        .collect::<Vec<_>>()
                }
            })
            .to_string(),
        )
    };

    // Without a liveness source the payload is returned exactly as encoded.
    let value = refresh_encoded_artifacts(None, &owner, encoded(&[ephemeral])?)?;
    assert_eq!(value["data"]["artifacts"][0]["completeness"], "complete");

    // With one that reports every artifact gone, both schemes are downgraded.
    let dead = DeadArtifacts;
    let value = refresh_encoded_artifacts(Some(&dead), &owner, encoded(&[ephemeral, durable])?)?;
    assert_eq!(value["data"]["artifacts"][0]["completeness"], "unavailable");
    assert_eq!(value["data"]["artifacts"][1]["completeness"], "unavailable");

    // A reference this owner cannot verify is reported unavailable rather than
    // claimed live: another project's URI and an unparseable id both downgrade.
    for uri in [
        "rust-artifact://prj_00000000000000000000000000000009/art_00000000000000000000000000000002",
        "rust-artifact://prj_00000000000000000000000000000001/not-an-id",
        "rust-quality-artifact://prj_00000000000000000000000000000009/qart_0123456789abcdef0123456789abcdef?offset=0&length=16",
    ] {
        let value = refresh_encoded_artifacts(Some(&dead), &owner, encoded(&[uri])?)?;
        assert_eq!(
            value["data"]["artifacts"][0]["completeness"], "unavailable",
            "{uri}"
        );
    }
    // A value that is not an artifact reference of this transport is left alone.
    let foreign = refresh_encoded_artifacts(
        Some(&dead),
        &owner,
        encoded(&["https://example.invalid/report"])?,
    )?;
    assert_eq!(foreign["data"]["artifacts"][0]["completeness"], "complete");
    // A payload without a data.artifacts array is passed through unchanged.
    let plain = QualityToolResult::new(serde_json::json!({"status": "passed"}).to_string())?;
    assert_eq!(
        refresh_encoded_artifacts(Some(&dead), &owner, plain)?["status"],
        "passed"
    );
    Ok(())
}

#[test]
fn admission_failures_are_masked_or_reported_as_capacity() {
    for error in [JobError::Busy, JobError::QuotaExceeded] {
        assert_eq!(map_admission(error).message, "Task worker busy");
    }
    assert_eq!(
        map_admission(JobError::Unavailable).code,
        rmcp::model::ErrorCode::INVALID_PARAMS
    );
    assert_eq!(
        map_admission(JobError::InputRejected).code,
        rmcp::model::ErrorCode::INTERNAL_ERROR
    );
    assert_eq!(
        map_worker_admission(workers::WorkerError::Busy).message,
        "Task worker busy"
    );
    for signal in [
        workers::WorkerError::Cancelled,
        workers::WorkerError::TimedOut,
        workers::WorkerError::Internal,
    ] {
        assert_eq!(
            map_worker_admission(signal).code,
            rmcp::model::ErrorCode::INTERNAL_ERROR
        );
    }
}
