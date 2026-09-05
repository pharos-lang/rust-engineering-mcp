//! MCP adapter: rmcp owns messages, negotiation, dispatch and transport framing.

mod admission;
mod auditing;
pub use auditing::provider::HostAuditConfig;
mod budget;
mod catalog;
mod check;
pub(crate) use catalog::provider::CatalogProvider;
pub use catalog::provider::HostCatalogConfig;
mod clippy;
mod contract;
mod crate_inspect;
mod crate_search;
mod explaining;
mod format;
mod inspection;
mod mutation;
mod project;
mod quality;
mod resources;
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
    CallToolRequestParams, CallToolResponse, CustomRequest, CustomResult, ErrorCode, ErrorData,
    Implementation, ListToolsResult, PaginatedRequestParams, ProtocolVersion,
    ReadResourceRequestParams, ReadResourceResponse, ServerCapabilities, ServerInfo, Tool,
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

pub struct HostConfig {
    pub manifest_write_roots: Vec<PathBuf>,
    pub fmt_write_roots: Vec<PathBuf>,
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
    audit: auditing::AuditTool,
    explain: explaining::ExplainTool,
    quality: quality::QualityTool,
    format: format::FormatTool,
    manifest_mutation: mutation::ManifestMutationTool,
    format_mutation: mutation::FormatMutationTool,
    toolchain: toolchain::ToolchainTool,
    ready: Arc<AtomicBool>,
    resources: resources::Resources,
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
            auditing::NAME => Some(self.audit.definition.clone()),
            explaining::NAME => Some(self.explain.definition.clone()),
            quality::NAME => Some(self.quality.definition.clone()),
            format::NAME => Some(self.format.definition.clone()),
            mutation::NAME => Some(self.manifest_mutation.definition.clone()),
            mutation::FORMAT_NAME => Some(self.format_mutation.definition.clone()),
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
                self.audit.definition.clone(),
                self.explain.definition.clone(),
                self.quality.definition.clone(),
                self.catalog.definition.clone(),
                self.crate_search.definition.clone(),
                self.crate_inspect.definition.clone(),
                self.manifest_mutation.definition.clone(),
                self.format_mutation.definition.clone(),
            ],
            ..Default::default()
        })
    }

    async fn call_tool(
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
            inspection::NAME => self.inspect.call(request, context).await.map(Into::into),
            toolchain::NAME => self.toolchain.call(request, context).await.map(Into::into),
            _ => Err(ErrorData::new(
                ErrorCode::METHOD_NOT_FOUND,
                "Unknown tool",
                None,
            )),
        }
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        self.resources.read(request, context).await
    }

    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_server_info(Implementation::new(
            "rust-engineering-mcp",
            env!("CARGO_PKG_VERSION"),
        ))
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
    if !config.manifest_write_roots.is_empty() || !config.fmt_write_roots.is_empty() {
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
            })
        }
    };
    let manifest_write_config = write_config(config.manifest_write_roots);
    let fmt_write_config = write_config(config.fmt_write_roots);
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
        mutation_plans,
    ) {
        Ok(tool) => tool,
        Err(_) => {
            tracing::error!("MCP format mutation contract initialization failed");
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
                audit,
                explain,
                quality,
                format,
                manifest_mutation,
                format_mutation,
                resources,
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
    let failed = Arc::new(IoFailure::new(workers.cancellation()));
    let (reader, writer) = rmcp::transport::stdio();
    let transport = admission::AdmittedTransport::new(
        rmcp::transport::async_rw::AsyncRwTransport::new_server(
            BudgetedReader::new(reader, Arc::clone(&failed)),
            CheckedWriter::new(writer, Arc::clone(&failed)),
        ),
        Arc::clone(&failed),
    );
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
    clean_close && !failed.occurred()
}
