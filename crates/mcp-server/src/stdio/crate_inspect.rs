//! Typed bounded retrieval over the immutable shared catalog generation.
mod schemas;
#[cfg(test)]
mod tests;
use super::clock::WallClock;
use super::{
    catalog::provider::CatalogProvider,
    contract::{Contract, ToolOutput},
    workers::{WorkerError, Workers},
};
use rmcp::{
    model::{CallToolRequestParams, CallToolResult, ErrorData, Tool, ToolAnnotations},
    service::{RequestContext, RoleServer},
};
use rust_engineering_application::{CatalogInspectError, InspectionControl, ProjectError};
use rust_engineering_domain::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
pub(super) const NAME: &str = "rust.crate.inspect";
const DEADLINE: Duration = Duration::from_secs(120);
const MAX_RESULT: usize = 512 * 1024;
fn section() -> InspectSection {
    InspectSection::Overview
}
fn limit() -> u32 {
    20
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Input {
    #[schemars(length(min = 1, max = 64), regex(pattern = "^[A-Za-z0-9_-]+$"))]
    name: String,
    #[serde(default = "section")]
    #[schemars(with = "schemas::InspectSection")]
    section: InspectSection,
    #[schemars(length(min = 1, max = 128))]
    version: Option<String>,
    #[serde(default = "limit")]
    #[schemars(range(min = 1, max = 50))]
    limit: u32,
    #[serde(default)]
    #[schemars(range(max = 128))]
    offset: u32,
    #[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    snapshot_fingerprint: Option<String>,
}
impl Input {
    fn request(self) -> Result<CrateInspectRequest, ErrorData> {
        let invalid = || ErrorData::invalid_params("Invalid tool arguments", None);
        let request = CrateInspectRequest {
            name: self.name,
            section: self.section,
            version: self.version,
            limit: self.limit,
            offset: self.offset,
            snapshot_fingerprint: self
                .snapshot_fingerprint
                .map(|s| s.parse())
                .transpose()
                .map_err(|_| invalid())?,
        };
        request.validate().map_err(|_| invalid())?;
        Ok(request)
    }
}
#[derive(Clone, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Semantics {
    LatestKnown,
}
#[derive(Clone, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Coverage {
    SnapshotPageOnly,
}
#[derive(Clone, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum AdvisoryInterpretation {
    SnapshotListedIdsOnly,
}
#[derive(Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Data {
    semantics: Semantics,
    coverage: Coverage,
    advisory_interpretation: AdvisoryInterpretation,
    #[schemars(with = "schemas::CrateInspectResult")]
    inspection: CrateInspectResult,
}
#[derive(Clone, Serialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum Code {
    SandboxDenied,
    CommandTimeout,
    OutputLimitExceeded,
    CatalogUnavailable,
    CatalogInvalid,
    SnapshotMismatch,
}
#[derive(Clone, Serialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
enum Outcome {
    Passed {
        error_code: (),
        error_message: (),
        data: Box<Data>,
    },
    Blocked {
        error_code: Code,
        error_message: &'static str,
        data: (),
    },
    Unavailable {
        error_code: Code,
        error_message: &'static str,
        data: (),
    },
    Cancelled {
        error_code: (),
        error_message: (),
        data: (),
    },
}
#[derive(Clone, Default, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Truncation {
    stdout_truncated: bool,
    stderr_truncated: bool,
    diagnostics_omitted: u64,
}
#[derive(Clone, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum LocalEvidence {
    Local,
}
#[derive(Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Output {
    #[serde(flatten)]
    outcome: Outcome,
    summary: &'static str,
    duration_ms: u64,
    diagnostics: [(); 0],
    truncation: Truncation,
    evidence: LocalEvidence,
}
impl ToolOutput for Output {
    fn status(&self) -> ToolStatus {
        match self.outcome {
            Outcome::Passed { .. } => ToolStatus::Passed,
            Outcome::Blocked { .. } => ToolStatus::Blocked,
            Outcome::Unavailable { .. } => ToolStatus::Unavailable,
            Outcome::Cancelled { .. } => ToolStatus::Cancelled,
        }
    }
}
fn output(
    result: Result<CrateInspectResult, CatalogInspectError>,
    duration_ms: u64,
) -> Result<Output, ErrorData> {
    let (outcome, summary) = match result {
        Ok(inspection) => (
            Outcome::Passed {
                error_code: (),
                error_message: (),
                data: Box::new(Data {
                    semantics: Semantics::LatestKnown,
                    coverage: Coverage::SnapshotPageOnly,
                    advisory_interpretation: AdvisoryInterpretation::SnapshotListedIdsOnly,
                    inspection,
                }),
            },
            "Bounded snapshot facts inspected; unknown fields and page continuation are explicit",
        ),
        Err(CatalogInspectError::Project(ProjectError::Cancelled)) => (
            Outcome::Cancelled {
                error_code: (),
                error_message: (),
                data: (),
            },
            "Crate inspection cancelled after worker completion",
        ),
        Err(
            CatalogInspectError::Unavailable(_)
            | CatalogInspectError::Catalog(CatalogError::Unavailable),
        ) => {
            let message = "A verified host catalog is unavailable; inspect rust.catalog.status";
            (
                Outcome::Unavailable {
                    error_code: Code::CatalogUnavailable,
                    error_message: message,
                    data: (),
                },
                message,
            )
        }
        Err(CatalogInspectError::Catalog(
            CatalogError::InvalidSnapshot
            | CatalogError::Integrity
            | CatalogError::UnsupportedSchema
            | CatalogError::Rollback,
        )) => {
            let message = "Catalog facts could not be verified";
            (
                Outcome::Unavailable {
                    error_code: Code::CatalogInvalid,
                    error_message: message,
                    data: (),
                },
                message,
            )
        }
        Err(CatalogInspectError::Catalog(CatalogError::Budget)) => blocked(
            Code::OutputLimitExceeded,
            "Crate inspection exceeded its data budget",
        ),
        Err(CatalogInspectError::Project(ProjectError::Rejected(code))) => match code {
            OperationalErrorCode::CommandTimeout => blocked(
                Code::CommandTimeout,
                "Crate inspection exceeded its deadline",
            ),
            OperationalErrorCode::SandboxDenied => blocked(
                Code::SandboxDenied,
                "Host policy, bootstrap or current capacity denied crate inspection",
            ),
            OperationalErrorCode::OutputLimitExceeded => blocked(
                Code::OutputLimitExceeded,
                "Crate inspection exceeds the response budget",
            ),
            _ => {
                return Err(ErrorData::internal_error(
                    "Unexpected crate inspection failure",
                    None,
                ));
            }
        },
        Err(CatalogInspectError::SnapshotMismatch) => blocked(
            Code::SnapshotMismatch,
            "Requested snapshot differs from the retained catalog; restart pagination",
        ),
        Err(CatalogInspectError::Catalog(CatalogError::InvalidInput)) => {
            return Err(ErrorData::invalid_params(
                "Invalid tool arguments or page offset",
                None,
            ));
        }
        Err(CatalogInspectError::Project(ProjectError::Internal)) => {
            return Err(ErrorData::internal_error(
                "Crate inspection validation failed",
                None,
            ));
        }
    };
    Ok(Output {
        outcome,
        summary,
        duration_ms,
        diagnostics: [],
        truncation: Truncation::default(),
        evidence: LocalEvidence::Local,
    })
}
fn blocked(code: Code, message: &'static str) -> (Outcome, &'static str) {
    (
        Outcome::Blocked {
            error_code: code,
            error_message: message,
            data: (),
        },
        message,
    )
}
fn worker_error(error: WorkerError) -> CatalogInspectError {
    CatalogInspectError::Project(match error {
        WorkerError::Busy => ProjectError::Rejected(OperationalErrorCode::SandboxDenied),
        WorkerError::Cancelled => ProjectError::Cancelled,
        WorkerError::TimedOut => ProjectError::Rejected(OperationalErrorCode::CommandTimeout),
        WorkerError::Internal => ProjectError::Internal,
    })
}
fn millis(started: Instant) -> u64 {
    started.elapsed().as_millis().try_into().unwrap_or(u64::MAX)
}
pub(super) struct CrateInspectTool {
    pub(super) definition: Tool,
    contract: Arc<Contract<Input, Output>>,
    workers: Workers,
    provider: Arc<CatalogProvider>,
    ready: Arc<AtomicBool>,
}
impl CrateInspectTool {
    pub(super) fn new(
        workers: Workers,
        ready: Arc<AtomicBool>,
        provider: Arc<CatalogProvider>,
    ) -> Result<Self, ErrorData> {
        let contract = Arc::new(Contract::<Input, Output>::new()?);
        let definition=Tool::new(NAME,"Inspect authoritative SQLite crate facts by section and exact version. Continue pages with the returned snapshot fingerprint and next offset. Missing documentation or package source stays unknown; listed advisory IDs do not establish safety. No downloads, refresh, model requirement or project authority.",(*contract.input_schema).clone())
            .with_raw_output_schema(Arc::clone(&contract.output_schema))
            .with_annotations(ToolAnnotations::new().read_only(true).destructive(false).idempotent(true).open_world(false));
        Ok(Self {
            definition,
            contract,
            workers,
            provider,
            ready,
        })
    }
    pub(super) async fn call(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let request = self.contract.decode(request.arguments)?.request()?;
        let started = Instant::now();
        if !self.ready.load(Ordering::Acquire) {
            let message =
                "Crate inspection requires completed discovery; retry with a new request ID";
            let mut value = output(Err(worker_error(WorkerError::Busy)), millis(started))?;
            value.summary = message;
            value.outcome = blocked(Code::SandboxDenied, message).0;
            return self.contract.encode(value);
        }
        let provider = Arc::clone(&self.provider);
        let contract = Arc::clone(&self.contract);
        match self
            .workers
            .run_joined(context.ct, started + DEADLINE, move |control| {
                let result = provider.inspect(&request, &WallClock, control);
                let value = output(result, millis(started))?;
                // JSON validation, duplicated encoding and budget trimming retain the slot.
                encode_bounded(&contract, value, control)
            })
            .await
        {
            Ok(joined) => match (joined.result, joined.interrupted) {
                (Err(error), _) => Err(error),
                (Ok(_), Some(signal)) => self
                    .contract
                    .encode(output(Err(worker_error(signal)), millis(started))?),
                (Ok(result), None) => Ok(result),
            },
            Err(error) => self
                .contract
                .encode(output(Err(worker_error(error)), millis(started))?),
        }
    }
}
fn encode_bounded(
    contract: &Contract<Input, Output>,
    mut value: Output,
    control: &dyn InspectionControl,
) -> Result<CallToolResult, ErrorData> {
    loop {
        if let Err(error) = control.check() {
            return contract.encode(output(
                Err(CatalogInspectError::Project(error)),
                value.duration_ms,
            )?);
        }
        let encoded = contract.encode(value.clone())?;
        if serde_json::to_vec(&encoded)
            .map_err(|_| ErrorData::internal_error("Response encoding failed", None))?
            .len()
            <= MAX_RESULT
        {
            return Ok(encoded);
        }
        if let Outcome::Passed { data, .. } = &mut value.outcome
            && let InspectLookup::Found { page } = &mut data.inspection.lookup
            && trim_page(page)
        {
            continue;
        }
        return contract.encode(output(
            Err(CatalogInspectError::Project(ProjectError::Rejected(
                OperationalErrorCode::OutputLimitExceeded,
            ))),
            value.duration_ms,
        )?);
    }
}

fn trim_page(page: &mut InspectPage) -> bool {
    let removed = match &mut page.data {
        InspectPageData::Overview { .. } => false,
        InspectPageData::Versions { items } => {
            if items.len() > 1 {
                items.pop();
                true
            } else {
                false
            }
        }
        InspectPageData::Features { items, .. } | InspectPageData::Advisories { items, .. } => {
            if items.len() > 1 {
                items.pop();
                true
            } else {
                false
            }
        }
        InspectPageData::Dependencies { items, .. } => {
            if items.len() > 1 {
                items.pop();
                true
            } else {
                false
            }
        }
    };
    if removed {
        page.pagination.returned -= 1;
        page.pagination.omitted_by_output += 1;
        page.pagination.next_offset = Some(page.pagination.offset + page.pagination.returned);
    }
    removed
}
