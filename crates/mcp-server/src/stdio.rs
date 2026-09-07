//! MCP adapter: rmcp owns messages, negotiation, dispatch and transport framing.

mod admission;
mod auditing;
pub use auditing::provider::HostAuditConfig;
mod budget;
mod catalog;
mod check;
mod clock;
pub(crate) use catalog::provider::CatalogProvider;
pub use catalog::provider::HostCatalogConfig;
mod clippy;
mod contract;
mod coverage;
mod crate_inspect;
mod crate_search;
mod explaining;
mod format;
mod inspection;
mod mutation;
mod mutation_test;
mod nextest;
mod operational;
mod project;
mod quality;
mod quality_artifacts;
mod resources;
mod semver;
mod tasks;
mod testing;
mod toolchain;
mod workers;

use std::borrow::Cow;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rmcp::model::{
    CacheScope, CallToolRequestParams, CallToolResponse, CallToolResult, CancelTaskParams,
    CustomRequest, CustomResult, ErrorCode, ErrorData, GetTaskParams, GetTaskResult,
    Implementation, ListToolsResult, PaginatedRequestParams, ProtocolVersion,
    ReadResourceRequestParams, ReadResourceResponse, ServerCapabilities, ServerInfo, Tool,
    UpdateTaskParams,
};
use rmcp::service::{QuitReason, RequestContext, RoleServer};
use rmcp::{ServerHandler, service::ServerInitializeError, service::serve_server_with_ct};
use tracing_subscriber::filter::Targets;
use tracing_subscriber::prelude::*;

use budget::{BudgetedReader, CheckedWriter, IoFailure};
use project::ProjectTool;
use rust_engineering_project::SecureProjects;

const SUPPORTED_VERSIONS: &[ProtocolVersion] = &[
    ProtocolVersion::V_2024_11_05,
    ProtocolVersion::V_2025_03_26,
    ProtocolVersion::V_2025_06_18,
    ProtocolVersion::V_2025_11_25,
    ProtocolVersion::V_2026_07_28,
];

// ADR-060 requires the five-version plus stock-client G4 gate before this flips.
const TASKS_ADVERTISEMENT_READY: bool = true;

fn tasks_advertisement_ready() -> bool {
    #[cfg(feature = "test-hooks")]
    if std::env::var_os("RUST_MCP_TEST_TASKS_READY").as_deref() == Some(std::ffi::OsStr::new("1")) {
        return true;
    }
    TASKS_ADVERTISEMENT_READY
}

fn server_capabilities(tasks_advertised: bool) -> ServerCapabilities {
    let builder = ServerCapabilities::builder()
        .enable_tools()
        .enable_resources();
    if tasks_advertised {
        builder.enable_tasks().build()
    } else {
        builder.build()
    }
}

#[derive(Clone)]
pub struct HostCargoVendorConfig {
    pub directory: PathBuf,
    pub fingerprint: rust_engineering_domain::SourceFingerprint,
}

pub struct HostConfig {
    pub manifest_write_roots: Vec<PathBuf>,
    pub fmt_write_roots: Vec<PathBuf>,
    pub fix_write_roots: Vec<PathBuf>,
    pub dependency_add_roots: Vec<PathBuf>,
    pub dependency_remove_roots: Vec<PathBuf>,
    pub cargo_vendor: Option<HostCargoVendorConfig>,
    pub catalog: Option<HostCatalogConfig>,
    pub audit: Option<HostAuditConfig>,
    pub roots: Vec<PathBuf>,
    pub ttl_seconds: u64,
    pub rust: Option<rust_engineering_execution::HostDockerConfig>,
}

struct EngineeringServer {
    catalog: catalog::CatalogTool,
    crate_search: crate_search::CrateSearchTool,
    crate_inspect: crate_inspect::CrateInspectTool,
    project: ProjectTool,
    inspect: inspection::InspectionTool,
    check: check::CheckTool,
    clippy: clippy::ClippyTool,
    testing: testing::TestTool,
    nextest: Arc<nextest::NextestTool>,
    mutation_test: Arc<mutation_test::MutationTestTool>,
    coverage: Arc<coverage::CoverageTool>,
    semver: Arc<semver::SemverTool>,
    audit: auditing::AuditTool,
    explain: explaining::ExplainTool,
    quality: quality::QualityTool,
    format: format::FormatTool,
    manifest_mutation: mutation::ManifestMutationTool,
    format_mutation: mutation::FormatMutationTool,
    fix_mutation: mutation::FixMutationTool,
    dependency_add: mutation::DependencyAddTool,
    dependency_remove: mutation::DependencyRemoveTool,
    toolchain: toolchain::ToolchainTool,
    ready: Arc<AtomicBool>,
    resources: resources::Resources,
    tasks: tasks::Tasks,
    tasks_advertised: bool,
}

#[derive(Clone)]
enum QualityInvocation {
    Nextest(Arc<nextest::NextestTool>),
    Coverage(Arc<coverage::CoverageTool>),
    Semver(Arc<semver::SemverTool>),
    Mutation(Arc<mutation_test::MutationTestTool>),
}

impl QualityInvocation {
    fn from_server(server: &EngineeringServer, name: &str) -> Option<Self> {
        match name {
            nextest::NAME => Some(Self::Nextest(Arc::clone(&server.nextest))),
            coverage::NAME => Some(Self::Coverage(Arc::clone(&server.coverage))),
            semver::NAME => Some(Self::Semver(Arc::clone(&server.semver))),
            mutation_test::NAME => Some(Self::Mutation(Arc::clone(&server.mutation_test))),
            _ => None,
        }
    }

    fn kind(&self) -> rust_engineering_application::job::JobKind {
        use rust_engineering_application::job::JobKind;
        match self {
            Self::Nextest(_) => JobKind::TestNextest,
            Self::Coverage(_) => JobKind::Coverage,
            Self::Semver(_) => JobKind::SemverCheck,
            Self::Mutation(_) => JobKind::MutationTest,
        }
    }

    fn project_ref(
        &self,
        request: &CallToolRequestParams,
    ) -> Result<rust_engineering_domain::ProjectRef, ErrorData> {
        let key = if matches!(self, Self::Semver(_)) {
            "candidate_project_ref"
        } else {
            "project_ref"
        };
        request
            .arguments
            .as_ref()
            .and_then(|arguments| arguments.get(key))
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| ErrorData::invalid_params("Invalid tool arguments", None))?
            .parse()
            .map_err(|_| ErrorData::invalid_params("Invalid tool arguments", None))
    }

    fn budget(
        &self,
        request: &CallToolRequestParams,
    ) -> Result<rust_engineering_domain::job::JobBudget, ErrorData> {
        use rust_engineering_domain::job::{JobBudget, Milliseconds};

        let arguments = request.arguments.as_ref();
        let seconds = match self {
            Self::Mutation(_) => {
                let max_mutants = arguments
                    .and_then(|value| value.get("max_mutants"))
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(100);
                let mutant_timeout = arguments
                    .and_then(|value| value.get("mutant_timeout_seconds"))
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(60);
                let build_timeout = mutant_timeout.saturating_mul(5).clamp(60, 300);
                mutant_timeout
                    .saturating_mul(max_mutants)
                    .saturating_add(build_timeout)
                    .clamp(300, 3_600)
            }
            _ => arguments
                .and_then(|value| value.get("timeout_seconds"))
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(300)
                .clamp(300, 3_600),
        };
        JobBudget::asynchronous_for_work(Milliseconds(seconds.saturating_mul(1_000)))
            .map_err(|_| ErrorData::invalid_params("Invalid tool arguments", None))
    }

    async fn call(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        match self {
            Self::Nextest(tool) => tool.call(request, context).await,
            Self::Coverage(tool) => tool.call(request, context).await,
            Self::Semver(tool) => tool.call(request, context).await,
            Self::Mutation(tool) => tool.call(request, context).await,
        }
    }
}

impl EngineeringServer {
    async fn dispatch_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        match request.name.as_ref() {
            crate_inspect::NAME => self
                .crate_inspect
                .call(request, context)
                .await
                .map(Into::into),
            crate_search::NAME => self
                .crate_search
                .call(request, context)
                .await
                .map(Into::into),
            catalog::NAME => self.catalog.call(request, context).await.map(Into::into),
            project::NAME => self.project.call(request, context).await.map(Into::into),
            check::NAME => self.check.call(request, context).await.map(Into::into),
            clippy::NAME => self.clippy.call(request, context).await.map(Into::into),
            testing::NAME => self.testing.call(request, context).await.map(Into::into),
            nextest::NAME => self.nextest.call(request, context).await.map(Into::into),
            mutation_test::NAME => self
                .mutation_test
                .call(request, context)
                .await
                .map(Into::into),
            coverage::NAME => self.coverage.call(request, context).await.map(Into::into),
            semver::NAME => self.semver.call(request, context).await.map(Into::into),
            auditing::NAME => self.audit.call(request, context).await.map(Into::into),
            explaining::NAME => self.explain.call(request, context).await.map(Into::into),
            quality::NAME => self.quality.call(request, context).await.map(Into::into),
            format::NAME => self.format.call(request, context).await.map(Into::into),
            mutation::NAME => self
                .manifest_mutation
                .call(request, context)
                .await
                .map(Into::into),
            mutation::FORMAT_NAME => self
                .format_mutation
                .call(request, context)
                .await
                .map(Into::into),
            mutation::FIX_NAME => self
                .fix_mutation
                .call(request, context)
                .await
                .map(Into::into),
            mutation::DEPENDENCY_ADD_NAME => self
                .dependency_add
                .call(request, context)
                .await
                .map(Into::into),
            mutation::DEPENDENCY_REMOVE_NAME => self
                .dependency_remove
                .call(request, context)
                .await
                .map(Into::into),
            inspection::NAME => self.inspect.call(request, context).await.map(Into::into),
            toolchain::NAME => self.toolchain.call(request, context).await.map(Into::into),
            _ => Err(ErrorData::new(
                ErrorCode::METHOD_NOT_FOUND,
                "Unknown tool",
                None,
            )),
        }
    }
}

fn task_materialization_requested(error: &ErrorData) -> bool {
    error.code == ErrorCode::INTERNAL_ERROR
        && matches!(
            error.message.as_ref(),
            "Tasks are not enabled for nextest"
                | "Tasks are not enabled for coverage"
                | "Tasks are not enabled for semver"
                | "Tasks are not enabled for mutation testing"
        )
        && error.data.is_none()
}

async fn run_quality_job(
    admitted: tasks::AdmittedTask,
    invocation: QualityInvocation,
    request: CallToolRequestParams,
    context: RequestContext<RoleServer>,
) {
    use rust_engineering_application::job::{
        CleanupObservation, JobResult, JobSignal, QualityToolResult,
    };
    use rust_engineering_domain::job::{JobCompletion, JobInfrastructureFailure, JobPhase};

    let executor = Arc::clone(&admitted.executor);
    test_task_phase_delay(JobPhase::Admission).await;
    if executor.start(&admitted.id).is_err()
        || executor.set_phase(&admitted.id, JobPhase::Prepare).is_err()
        || executor.set_phase(&admitted.id, JobPhase::Execute).is_err()
    {
        admitted.signal.observe_cleanup();
        if admitted.signal.cancellation_requested() {
            let _ = executor.observe_cleanup(&admitted.id);
        } else {
            let _ = executor.finish(
                &admitted.id,
                JobCompletion::InfrastructureFailure(JobInfrastructureFailure::Internal),
                0,
                CleanupObservation::Observed,
            );
        }
        return;
    }
    test_task_phase_delay(JobPhase::Execute).await;

    let outcome = workers::with_admitted_job(
        Arc::clone(&admitted.permit),
        invocation.call(request, context),
    )
    .await;
    test_task_phase_delay(JobPhase::Cleanup).await;
    let _ = executor.set_phase(&admitted.id, JobPhase::Collect);
    let _ = executor.set_phase(&admitted.id, JobPhase::Publish);
    test_task_phase_delay(JobPhase::Publish).await;
    if !test_force_uncertain_cleanup() {
        admitted.signal.observe_cleanup();
    }

    // The JobExecutor completion write is the task-result commit point. An
    // authority/cancellation intent observed before it wins and no tool payload
    // is published; a completion already committed still wins during cleanup.
    if admitted.signal.cancellation_requested() {
        let _ = executor.observe_cleanup(&admitted.id);
        return;
    }

    match outcome {
        Ok(result) => {
            let cancelled_result = result
                .structured_content
                .as_ref()
                .and_then(|value| value.get("status"))
                .and_then(serde_json::Value::as_str)
                == Some("cancelled");
            if cancelled_result && admitted.signal.cancellation_requested() {
                let _ = executor.observe_cleanup(&admitted.id);
                return;
            }
            let is_error = result.is_error.unwrap_or(false);
            let Some(structured) = result.structured_content else {
                let _ = executor.finish(
                    &admitted.id,
                    JobCompletion::InfrastructureFailure(JobInfrastructureFailure::Internal),
                    0,
                    CleanupObservation::Observed,
                );
                return;
            };
            let serialized = if is_error {
                CallToolResult::structured_error(structured.clone())
            } else {
                CallToolResult::structured(structured.clone())
            };
            let bytes = serde_json::to_vec(&serialized)
                .ok()
                .and_then(|bytes| u64::try_from(bytes.len()).ok())
                .unwrap_or(u64::MAX);
            let completion = serde_json::to_string(&structured)
                .ok()
                .and_then(|json| QualityToolResult::new(json).ok())
                .map(|result| JobCompletion::ToolResult {
                    result: JobResult::QualityTool(result),
                    is_error,
                })
                .unwrap_or(JobCompletion::InfrastructureFailure(
                    JobInfrastructureFailure::ResultUnavailable,
                ));
            let _ = executor.finish(
                &admitted.id,
                completion,
                bytes,
                CleanupObservation::Observed,
            );
        }
        Err(_) if admitted.signal.cancellation_requested() => {
            let _ = executor.observe_cleanup(&admitted.id);
        }
        Err(_) => {
            let _ = executor.finish(
                &admitted.id,
                JobCompletion::InfrastructureFailure(JobInfrastructureFailure::Internal),
                0,
                CleanupObservation::Observed,
            );
        }
    }
}

#[cfg(feature = "test-hooks")]
async fn test_task_phase_delay(phase: rust_engineering_domain::job::JobPhase) {
    let requested = std::env::var("RUST_MCP_TEST_TASK_DELAY_PHASE").ok();
    if requested.as_deref()
        == Some(match phase {
            rust_engineering_domain::job::JobPhase::Admission => "admission",
            rust_engineering_domain::job::JobPhase::Execute => "execute",
            rust_engineering_domain::job::JobPhase::Publish => "publish",
            rust_engineering_domain::job::JobPhase::Cleanup => "cleanup",
            _ => "",
        })
    {
        tokio::time::sleep(Duration::from_millis(750)).await;
    }
}

#[cfg(not(feature = "test-hooks"))]
async fn test_task_phase_delay(_: rust_engineering_domain::job::JobPhase) {}

#[cfg(feature = "test-hooks")]
fn test_force_uncertain_cleanup() -> bool {
    std::env::var_os("RUST_MCP_TEST_TASK_FORCE_UNCERTAIN").as_deref()
        == Some(std::ffi::OsStr::new("1"))
}

#[cfg(not(feature = "test-hooks"))]
fn test_force_uncertain_cleanup() -> bool {
    false
}

impl ServerHandler for EngineeringServer {
    fn get_tool(&self, name: &str) -> Option<Tool> {
        match name {
            crate_search::NAME => Some(self.crate_search.definition.clone()),
            crate_inspect::NAME => Some(self.crate_inspect.definition.clone()),
            catalog::NAME => Some(self.catalog.definition.clone()),
            project::NAME => Some(self.project.definition.clone()),
            check::NAME => Some(self.check.definition.clone()),
            clippy::NAME => Some(self.clippy.definition.clone()),
            testing::NAME => Some(self.testing.definition.clone()),
            nextest::NAME => Some(self.nextest.definition.clone()),
            mutation_test::NAME => Some(self.mutation_test.definition.clone()),
            coverage::NAME => Some(self.coverage.definition.clone()),
            semver::NAME => Some(self.semver.definition.clone()),
            auditing::NAME => Some(self.audit.definition.clone()),
            explaining::NAME => Some(self.explain.definition.clone()),
            quality::NAME => Some(self.quality.definition.clone()),
            format::NAME => Some(self.format.definition.clone()),
            mutation::NAME => Some(self.manifest_mutation.definition.clone()),
            mutation::FORMAT_NAME => Some(self.format_mutation.definition.clone()),
            mutation::FIX_NAME => Some(self.fix_mutation.definition.clone()),
            mutation::DEPENDENCY_ADD_NAME => Some(self.dependency_add.definition.clone()),
            mutation::DEPENDENCY_REMOVE_NAME => Some(self.dependency_remove.definition.clone()),
            inspection::NAME => Some(self.inspect.definition.clone()),
            toolchain::NAME => Some(self.toolchain.definition.clone()),
            _ => None,
        }
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: vec![
                self.project.definition.clone(),
                self.inspect.definition.clone(),
                self.toolchain.definition.clone(),
                self.check.definition.clone(),
                self.format.definition.clone(),
                self.clippy.definition.clone(),
                self.testing.definition.clone(),
                self.nextest.definition.clone(),
                self.audit.definition.clone(),
                self.explain.definition.clone(),
                self.quality.definition.clone(),
                self.catalog.definition.clone(),
                self.crate_search.definition.clone(),
                self.crate_inspect.definition.clone(),
                self.manifest_mutation.definition.clone(),
                self.format_mutation.definition.clone(),
                self.fix_mutation.definition.clone(),
                self.dependency_add.definition.clone(),
                self.dependency_remove.definition.clone(),
                self.coverage.definition.clone(),
                self.semver.definition.clone(),
                self.mutation_test.definition.clone(),
            ],
            ..Default::default()
        }
        .with_ttl_ms(0)
        .with_cache_scope(CacheScope::Private))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let invocation = QualityInvocation::from_server(self, request.name.as_ref());
        let negotiated_tasks = self.tasks_advertised
            && context
                .client_capabilities()
                .is_some_and(|capabilities| capabilities.supports_tasks());
        let preflight = workers::with_negotiated_tasks(
            negotiated_tasks,
            self.dispatch_tool(request.clone(), context.clone()),
        )
        .await;
        match (preflight, invocation) {
            (Err(error), Some(invocation))
                if negotiated_tasks && task_materialization_requested(&error) =>
            {
                if !self.ready.load(Ordering::Acquire) {
                    return workers::with_job_execution_selection(
                        invocation.call(request, context),
                    )
                    .await
                    .map(Into::into);
                }
                let project_ref = invocation.project_ref(&request)?;
                let Some(delivery_token) = admission::delivery_token(&context.extensions) else {
                    return Err(ErrorData::internal_error(
                        "Task delivery tracking is unavailable",
                        None,
                    ));
                };
                let admitted = match self.tasks.admit(
                    invocation.kind(),
                    project_ref,
                    invocation.budget(&request)?,
                    delivery_token,
                    || !context.ct.is_cancelled(),
                ) {
                    Ok(admitted) => admitted,
                    Err(error) => return Err(error),
                };
                let response = admitted.response.clone();
                let mut job_context = context;
                job_context.ct = admitted.signal.cancellation();
                tokio::spawn(run_quality_job(admitted, invocation, request, job_context));
                Ok(CallToolResponse::Task(response))
            }
            (result, _) => result,
        }
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        self.resources.read(request, context).await
    }

    async fn get_task(
        &self,
        request: GetTaskParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetTaskResult, ErrorData> {
        self.tasks.get(&request.task_id).await
    }

    async fn cancel_task(
        &self,
        request: CancelTaskParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        self.tasks.cancel(&request.task_id).await
    }

    async fn update_task(
        &self,
        request: UpdateTaskParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        self.tasks.update(&request.task_id).await
    }

    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(server_capabilities(self.tasks_advertised)).with_server_info(
            Implementation::new("rust-engineering-mcp", env!("CARGO_PKG_VERSION")),
        )
    }

    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(SUPPORTED_VERSIONS)
    }

    async fn on_custom_request(
        &self,
        _request: CustomRequest,
        _context: RequestContext<RoleServer>,
    ) -> Result<CustomResult, ErrorData> {
        // The default SDK error reflects the untrusted method name.
        Err(ErrorData::new(
            ErrorCode::METHOD_NOT_FOUND,
            "Unknown method",
            None,
        ))
    }
}

pub fn run(config: HostConfig) -> ExitCode {
    // Never read RUST_LOG: SDK diagnostics may include peer payloads. Only this
    // adapter's fixed operational messages are enabled, always on stderr.
    let subscriber = tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .with_writer(std::io::stderr)
            .with_ansi(false)
            .with_target(false)
            .without_time()
            .with_filter(Targets::new().with_target("rust_engineering_mcp", tracing::Level::INFO)),
    );
    // This thread-local subscriber covers the current-thread runtime below.
    // Revisit propagation before moving handlers to a multi-thread runtime.
    let _subscriber = tracing::subscriber::set_default(subscriber);
    let workers = workers::Workers::new();
    let project = match SecureProjects::new(&config.roots)
        .and_then(|backend| ProjectTool::new(backend, config.ttl_seconds, workers.clone()))
    {
        Ok(project) => project,
        Err(_) => {
            tracing::error!("MCP project authorization initialization failed");
            return ExitCode::FAILURE;
        }
    };
    let ready = Arc::new(AtomicBool::new(false));
    let rust_enabled = config.rust.is_some();
    if !config.manifest_write_roots.is_empty()
        || !config.fmt_write_roots.is_empty()
        || !config.fix_write_roots.is_empty()
        || !config.dependency_add_roots.is_empty()
        || !config.dependency_remove_roots.is_empty()
    {
        let Some(runtime) = config.rust.as_ref() else {
            return ExitCode::FAILURE;
        };
        let journal = runtime.state_root.join("rust-mcp-mutations-v1");
        if config
            .roots
            .iter()
            .any(|root| journal.starts_with(root) || root.starts_with(&journal))
        {
            tracing::error!("Mutation state must be outside project roots");
            return ExitCode::FAILURE;
        }
    }
    let write_config = |roots: Vec<PathBuf>| {
        if roots.is_empty() {
            None
        } else {
            config.rust.as_ref().map(|runtime| mutation::WriteConfig {
                roots,
                state_parent: runtime.state_root.clone(),
                vendor: config.cargo_vendor.clone(),
            })
        }
    };
    let manifest_write_config = write_config(config.manifest_write_roots);
    let fmt_write_config = write_config(config.fmt_write_roots);
    let fix_write_config = write_config(config.fix_write_roots);
    let dependency_add_config = write_config(config.dependency_add_roots);
    let dependency_remove_config = write_config(config.dependency_remove_roots);
    let quality_state_root = config
        .rust
        .as_ref()
        .map(|runtime| runtime.state_root.clone());
    let inspector = Arc::new(rust_engineering_execution::RustProjectInspector::new(
        config.rust,
    ));
    let inspect = match inspection::InspectionTool::new(
        project.registry(),
        workers.clone(),
        Arc::clone(&inspector),
        Arc::clone(&ready),
    ) {
        Ok(tool) => tool,
        Err(_) => {
            tracing::error!("MCP inspection contract initialization failed");
            return ExitCode::FAILURE;
        }
    };
    let toolchain = match toolchain::ToolchainTool::new(
        project.registry(),
        workers.clone(),
        Arc::clone(&inspector),
        Arc::clone(&ready),
    ) {
        Ok(tool) => tool,
        Err(_) => {
            tracing::error!("MCP toolchain contract initialization failed");
            return ExitCode::FAILURE;
        }
    };
    let resources =
        match resources::Resources::new(project.registry(), workers.clone(), Arc::clone(&ready)) {
            Ok(resources) => resources,
            Err(_) => {
                tracing::error!("MCP artifact initialization failed");
                return ExitCode::FAILURE;
            }
        };
    let quality_runtime = match quality_state_root.as_deref() {
        Some(state_root) => match quality_artifacts::attach(state_root, project.registry()) {
            Ok(runtime) => runtime,
            Err(_) => {
                tracing::error!("MCP durable quality artifact initialization failed");
                return ExitCode::FAILURE;
            }
        },
        None => None,
    };
    let resources = if let Some(runtime) = &quality_runtime {
        resources.with_quality_reader(Arc::clone(&runtime.reader))
    } else {
        resources
    };
    let check = match check::CheckTool::new(
        project.registry(),
        workers.clone(),
        Arc::clone(&inspector),
        Arc::clone(&ready),
        &resources,
    ) {
        Ok(check) => check,
        Err(_) => {
            tracing::error!("MCP check contract initialization failed");
            return ExitCode::FAILURE;
        }
    };
    let clippy = match clippy::ClippyTool::new(
        project.registry(),
        workers.clone(),
        Arc::clone(&inspector),
        Arc::clone(&ready),
        &resources,
    ) {
        Ok(clippy) => clippy,
        Err(_) => {
            tracing::error!("MCP clippy contract initialization failed");
            return ExitCode::FAILURE;
        }
    };
    let testing = match testing::TestTool::new(
        project.registry(),
        workers.clone(),
        Arc::clone(&inspector),
        Arc::clone(&ready),
        &resources,
    ) {
        Ok(testing) => testing,
        Err(_) => {
            tracing::error!("MCP testing contract initialization failed");
            return ExitCode::FAILURE;
        }
    };
    let nextest = match nextest::NextestTool::new() {
        Ok(tool) => tool.with_runtime(
            project.registry(),
            workers.clone(),
            Arc::clone(&inspector),
            Arc::clone(&ready),
            &resources,
            quality_runtime
                .as_ref()
                .map(|runtime| runtime.publisher.clone()),
        ),
        Err(_) => {
            tracing::error!("MCP nextest contract initialization failed");
            return ExitCode::FAILURE;
        }
    };
    let mutation_test = match mutation_test::MutationTestTool::new() {
        Ok(tool) => tool.with_runtime(
            project.registry(),
            workers.clone(),
            Arc::clone(&inspector),
            Arc::clone(&ready),
            &resources,
        ),
        Err(_) => {
            tracing::error!("MCP mutation test contract initialization failed");
            return ExitCode::FAILURE;
        }
    };
    let coverage = match coverage::CoverageTool::new() {
        Ok(tool) => tool.with_runtime(
            project.registry(),
            workers.clone(),
            Arc::clone(&inspector),
            Arc::clone(&ready),
            &resources,
            quality_runtime
                .as_ref()
                .map(|runtime| runtime.coverage_publisher.clone()),
        ),
        Err(_) => {
            tracing::error!("MCP coverage contract initialization failed");
            return ExitCode::FAILURE;
        }
    };
    let semver = match semver::SemverTool::new() {
        Ok(tool) => tool.with_runtime(
            project.registry(),
            workers.clone(),
            Arc::clone(&inspector),
            Arc::clone(&ready),
            &resources,
            quality_runtime
                .as_ref()
                .map(|runtime| runtime.semver_publisher.clone()),
        ),
        Err(_) => {
            tracing::error!("MCP semver contract initialization failed");
            return ExitCode::FAILURE;
        }
    };
    let format = match format::FormatTool::new(
        project.registry(),
        workers.clone(),
        Arc::clone(&inspector),
        Arc::clone(&ready),
        &resources,
    ) {
        Ok(tool) => tool,
        Err(_) => {
            tracing::error!("MCP formatting contract initialization failed");
            return ExitCode::FAILURE;
        }
    };
    let audit = match auditing::AuditTool::new(
        project.registry(),
        workers.clone(),
        Arc::clone(&inspector),
        Arc::clone(&ready),
        config.audit.clone(),
    ) {
        Ok(tool) => tool,
        Err(_) => {
            tracing::error!("MCP audit contract initialization failed");
            return ExitCode::FAILURE;
        }
    };
    let explain = match explaining::ExplainTool::new(
        workers.clone(),
        Arc::clone(&inspector),
        Arc::clone(&ready),
    ) {
        Ok(tool) => tool,
        Err(_) => {
            tracing::error!("MCP explanation contract initialization failed");
            return ExitCode::FAILURE;
        }
    };
    let catalog_provider = Arc::new(catalog::provider::CatalogProvider::new(
        config.catalog,
        config.audit.clone(),
    ));
    let crate_search = match crate_search::CrateSearchTool::new(
        workers.clone(),
        Arc::clone(&ready),
        Arc::clone(&catalog_provider),
    ) {
        Ok(tool) => tool,
        Err(_) => {
            tracing::error!("MCP crate search contract initialization failed");
            return ExitCode::FAILURE;
        }
    };
    let crate_inspect = match crate_inspect::CrateInspectTool::new(
        workers.clone(),
        Arc::clone(&ready),
        Arc::clone(&catalog_provider),
    ) {
        Ok(tool) => tool,
        Err(_) => {
            tracing::error!("MCP crate inspect contract initialization failed");
            return ExitCode::FAILURE;
        }
    };
    let catalog =
        match catalog::CatalogTool::new(workers.clone(), Arc::clone(&ready), catalog_provider) {
            Ok(tool) => tool,
            Err(_) => {
                tracing::error!("MCP catalog contract initialization failed");
                return ExitCode::FAILURE;
            }
        };
    let quality = match quality::QualityTool::new(
        project.registry(),
        workers.clone(),
        Arc::clone(&inspector),
        Arc::clone(&ready),
        &resources,
        config.audit,
    ) {
        Ok(tool) => tool,
        Err(_) => {
            tracing::error!("MCP quality gate contract initialization failed");
            return ExitCode::FAILURE;
        }
    };
    let mutation_plans = Arc::new(Mutex::new(mutation::SharedPlans::default()));
    let manifest_mutation = match mutation::ManifestMutationTool::new(
        project.registry(),
        workers.clone(),
        Arc::clone(&inspector),
        Arc::clone(&ready),
        manifest_write_config,
        Arc::clone(&mutation_plans),
    ) {
        Ok(tool) => tool,
        Err(_) => {
            tracing::error!("MCP mutation contract initialization failed");
            return ExitCode::FAILURE;
        }
    };
    let format_mutation = match mutation::FormatMutationTool::new(
        project.registry(),
        workers.clone(),
        Arc::clone(&inspector),
        Arc::clone(&ready),
        fmt_write_config,
        Arc::clone(&mutation_plans),
    ) {
        Ok(tool) => tool,
        Err(_) => {
            tracing::error!("MCP format mutation contract initialization failed");
            return ExitCode::FAILURE;
        }
    };
    let fix_mutation = match mutation::FixMutationTool::new(
        project.registry(),
        workers.clone(),
        Arc::clone(&inspector),
        Arc::clone(&ready),
        fix_write_config,
        Arc::clone(&mutation_plans),
    ) {
        Ok(tool) => tool,
        Err(_) => {
            tracing::error!("MCP fix mutation contract initialization failed");
            return ExitCode::FAILURE;
        }
    };
    let dependency_add = match mutation::DependencyAddTool::new(
        project.registry(),
        workers.clone(),
        Arc::clone(&inspector),
        Arc::clone(&ready),
        dependency_add_config,
        Arc::clone(&mutation_plans),
    ) {
        Ok(tool) => tool,
        Err(_) => {
            tracing::error!("MCP dependency add contract initialization failed");
            return ExitCode::FAILURE;
        }
    };
    let dependency_remove = match mutation::DependencyRemoveTool::new(
        project.registry(),
        workers.clone(),
        Arc::clone(&inspector),
        Arc::clone(&ready),
        dependency_remove_config,
        Arc::clone(&mutation_plans),
    ) {
        Ok(tool) => tool,
        Err(_) => {
            tracing::error!("MCP dependency remove contract initialization failed");
            return ExitCode::FAILURE;
        }
    };
    let tasks = match tasks::Tasks::production(
        project.registry(),
        quality_runtime
            .as_ref()
            .map(|runtime| runtime.state_root_identity),
        workers.clone(),
        resources.task_liveness(),
    ) {
        Ok(tasks) => tasks,
        Err(_) => {
            tracing::error!("MCP task contract initialization failed");
            return ExitCode::FAILURE;
        }
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            tracing::error!("MCP runtime initialization failed");
            return ExitCode::FAILURE;
        }
    };
    let success = runtime.block_on(async {
        let success = serve(
            EngineeringServer {
                catalog,
                crate_search,
                crate_inspect,
                project,
                inspect,
                toolchain,
                check,
                clippy,
                testing,
                nextest: Arc::new(nextest),
                mutation_test: Arc::new(mutation_test),
                coverage: Arc::new(coverage),
                semver: Arc::new(semver),
                audit,
                explain,
                quality,
                format,
                manifest_mutation,
                format_mutation,
                fix_mutation,
                dependency_add,
                dependency_remove,
                resources,
                tasks,
                tasks_advertised: tasks_advertisement_ready(),
                ready,
            },
            &workers,
        )
        .await;
        // Rust cleanup can perform twelve bounded Docker controls after the
        // current control/observer completes. Allow their deadlines and pipe
        // drain margins; kernel/daemon failure is still reported as uncertainty.
        let grace = Duration::from_secs(if rust_enabled { 240 } else { 12 });
        let drained = workers.shutdown(grace).await;
        success && drained && !inspector.is_quarantined()
    });
    // Tokio stdin uses blocking I/O that cannot always be cancelled. Bound
    // runtime cleanup; the binary then exits and releases its standard handles.
    runtime.shutdown_timeout(Duration::from_millis(100));
    if success {
        ExitCode::SUCCESS
    } else {
        tracing::error!("MCP stdio session failed");
        ExitCode::FAILURE
    }
}

async fn serve(server: EngineeringServer, workers: &workers::Workers) -> bool {
    let ready = Arc::clone(&server.ready);
    let delivery = server.tasks.delivery_tracker();
    let jobs = server.tasks.executor();
    let failed = Arc::new(IoFailure::new(workers.cancellation()));
    let watchdog_stop = tokio_util::sync::CancellationToken::new();
    let watchdog = jobs.clone().map(|executor| {
        let stop = watchdog_stop.clone();
        let failed = Arc::clone(&failed);
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    biased;
                    _ = stop.cancelled() => return true,
                    _ = tokio::time::sleep(Duration::from_millis(100)) => {
                        let executor = Arc::clone(&executor);
                        match tokio::task::spawn_blocking(move || executor.watchdog()).await {
                            Ok(Ok(())) => {}
                            Ok(Err(_)) | Err(_) => {
                                failed.record();
                                return false;
                            }
                        }
                    }
                }
            }
        })
    });
    let (reader, writer) = rmcp::transport::stdio();
    let transport = admission::AdmittedTransport::new(
        rmcp::transport::async_rw::AsyncRwTransport::new_server(
            BudgetedReader::new(reader, Arc::clone(&failed)),
            CheckedWriter::new(writer, Arc::clone(&failed)),
        ),
        Arc::clone(&failed),
    )
    .with_delivery_tracker(delivery);
    let clean_close = match serve_server_with_ct(
        admission::AdmittedService::new(server),
        transport,
        failed.cancellation(),
    )
    .await
    {
        Ok(service) => {
            // No await between bootstrap completion and readiness on this runtime.
            ready.store(true, Ordering::Release);
            matches!(service.waiting().await, Ok(QuitReason::Closed))
        }
        Err(ServerInitializeError::ConnectionClosed(_)) => true,
        // Never format SDK errors: they can contain the complete peer message.
        Err(_) => false,
    };
    watchdog_stop.cancel();
    let watchdog_clean = match watchdog {
        Some(watchdog) => watchdog.await.unwrap_or(false),
        None => true,
    };
    let jobs_clean = match jobs {
        Some(executor) => tokio::task::spawn_blocking(move || executor.shutdown_and_join().is_ok())
            .await
            .unwrap_or(false),
        None => true,
    };
    clean_close && watchdog_clean && jobs_clean && !failed.occurred()
}

#[cfg(test)]
mod tasks_advertisement_tests {
    use super::*;

    #[test]
    fn single_g4_switch_has_advertised_and_not_advertised_expectations_for_all_versions() {
        for version in SUPPORTED_VERSIONS {
            assert!(SUPPORTED_VERSIONS.contains(version));
            assert!(!server_capabilities(false).supports_tasks());
            assert!(server_capabilities(true).supports_tasks());
        }
    }
}
