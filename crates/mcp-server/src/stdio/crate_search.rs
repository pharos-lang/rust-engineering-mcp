//! Typed bounded retrieval over the immutable shared catalog generation.
mod schemas;
use super::{
    catalog::provider::CatalogProvider,
    contract::{Contract, ToolOutput},
    workers::{WorkerError, Workers},
};
use rmcp::{
    model::{CallToolRequestParams, CallToolResult, ErrorData, Tool, ToolAnnotations},
    service::{RequestContext, RoleServer},
};
use rust_engineering_application::{CatalogSearchError, InspectionControl, ProjectError};
use rust_engineering_domain::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
pub(super) const NAME: &str = "rust.crate.search";
const DEADLINE: Duration = Duration::from_secs(120);
const MAX_RESULT: usize = 512 * 1024;
fn mode() -> CrateSearchMode {
    CrateSearchMode::Hybrid
}
fn limit() -> u32 {
    10
}
#[derive(Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Filters {
    #[schemars(
        length(min = 1, max = 32),
        regex(pattern = "^(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)(\\.(0|[1-9][0-9]*))?$")
    )]
    msrv_lte: Option<String>,
    #[serde(default)]
    allow_yanked: bool,
    #[serde(default)]
    include_prerelease: bool,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Input {
    #[schemars(length(min = 1, max = 256))]
    query: String,
    #[serde(default = "mode")]
    #[schemars(with = "schemas::CrateSearchMode")]
    mode: CrateSearchMode,
    #[serde(default = "limit")]
    #[schemars(range(min = 1, max = 50))]
    limit: u32,
    #[serde(default)]
    filters: Filters,
}
impl Input {
    fn request(self) -> Result<CrateSearchRequest, ErrorData> {
        let invalid = || ErrorData::invalid_params("Invalid tool arguments", None);
        Ok(CrateSearchRequest {
            query: CatalogQuery::new(self.query, self.limit).map_err(|_| invalid())?,
            mode: self.mode,
            filters: CrateSearchFilters {
                msrv_lte: self
                    .filters
                    .msrv_lte
                    .as_deref()
                    .map(MsrvVersion::parse)
                    .transpose()
                    .map_err(|_| invalid())?,
                allow_yanked: self.filters.allow_yanked,
                include_prerelease: self.filters.include_prerelease,
            },
        })
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
    CandidateWindowOnly,
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
    #[schemars(with = "schemas::CrateSearchResult")]
    search: CrateSearchResult,
}
#[derive(Clone, Serialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum Code {
    SandboxDenied,
    CommandTimeout,
    OutputLimitExceeded,
    CatalogUnavailable,
    CatalogInvalid,
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
    result: Result<CrateSearchResult, CatalogSearchError>,
    duration_ms: u64,
) -> Result<Output, ErrorData> {
    let (outcome, summary) = match result {
        Ok(search) => (
            Outcome::Passed {
                error_code: (),
                error_message: (),
                data: Box::new(Data {
                    semantics: Semantics::LatestKnown,
                    coverage: Coverage::CandidateWindowOnly,
                    advisory_interpretation: AdvisoryInterpretation::SnapshotListedIdsOnly,
                    search,
                }),
            },
            "Bounded snapshot retrieval completed; scores are retrieval signals only",
        ),
        Err(CatalogSearchError::Project(ProjectError::Cancelled)) => (
            Outcome::Cancelled {
                error_code: (),
                error_message: (),
                data: (),
            },
            "Crate search cancelled after worker completion",
        ),
        Err(
            CatalogSearchError::Unavailable(_)
            | CatalogSearchError::Catalog(CatalogError::Unavailable),
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
        Err(CatalogSearchError::Catalog(
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
        Err(CatalogSearchError::Catalog(CatalogError::Budget)) => blocked(
            Code::OutputLimitExceeded,
            "Crate search exceeded its data budget",
        ),
        Err(CatalogSearchError::Project(ProjectError::Rejected(code))) => match code {
            OperationalErrorCode::CommandTimeout => {
                blocked(Code::CommandTimeout, "Crate search exceeded its deadline")
            }
            OperationalErrorCode::SandboxDenied => blocked(
                Code::SandboxDenied,
                "Host policy, bootstrap or current capacity denied crate search",
            ),
            OperationalErrorCode::OutputLimitExceeded => blocked(
                Code::OutputLimitExceeded,
                "Crate search exceeds the response budget",
            ),
            _ => {
                return Err(ErrorData::internal_error(
                    "Unexpected crate search failure",
                    None,
                ));
            }
        },
        Err(
            CatalogSearchError::Project(ProjectError::Internal)
            | CatalogSearchError::Catalog(CatalogError::InvalidInput),
        ) => {
            return Err(ErrorData::internal_error(
                "Crate search validation failed",
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
fn worker_error(error: WorkerError) -> CatalogSearchError {
    CatalogSearchError::Project(match error {
        WorkerError::Busy => ProjectError::Rejected(OperationalErrorCode::SandboxDenied),
        WorkerError::Cancelled => ProjectError::Cancelled,
        WorkerError::TimedOut => ProjectError::Rejected(OperationalErrorCode::CommandTimeout),
        WorkerError::Internal => ProjectError::Internal,
    })
}
fn millis(started: Instant) -> u64 {
    started.elapsed().as_millis().try_into().unwrap_or(u64::MAX)
}
struct WallClock;
impl Clock for WallClock {
    fn now(&self) -> UnixSeconds {
        UnixSeconds(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|v| v.as_secs())
                .unwrap_or(0),
        )
    }
}
pub(super) struct CrateSearchTool {
    pub(super) definition: Tool,
    contract: Arc<Contract<Input, Output>>,
    workers: Workers,
    provider: Arc<CatalogProvider>,
    ready: Arc<AtomicBool>,
}
impl CrateSearchTool {
    pub(super) fn new(
        workers: Workers,
        ready: Arc<AtomicBool>,
        provider: Arc<CatalogProvider>,
    ) -> Result<Self, ErrorData> {
        let contract = Arc::new(Contract::<Input, Output>::new()?);
        let definition=Tool::new(NAME,"Search the verified local snapshot with lexical, semantic or hybrid retrieval and authoritative SQLite version filters. Scores measure retrieval, not crate quality or safety. Bounded candidate windows and output omissions are explicit; unavailable semantics fall back to lexical with the same filters. No downloads, refresh or project authority.",(*contract.input_schema).clone())
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
            let message = "Crate search requires completed discovery; retry with a new request ID";
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
                let result = provider.search(&request, &WallClock, control);
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
                Err(CatalogSearchError::Project(error)),
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
            && data.search.results.pop().is_some()
        {
            data.search.window.omitted_by_output =
                data.search.window.omitted_by_output.saturating_add(1);
            data.search.window.returned = data.search.results.len().try_into().unwrap_or(u32::MAX);
            continue;
        }
        return contract.encode(output(
            Err(CatalogSearchError::Project(ProjectError::Rejected(
                OperationalErrorCode::OutputLimitExceeded,
            ))),
            value.duration_ms,
        )?);
    }
}

#[cfg(test)]
mod tests;
