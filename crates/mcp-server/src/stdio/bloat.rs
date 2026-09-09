//! M5-04: `rust.binary.bloat`, one size analysis of one project binary.
//!
//! ADR-076 §6 is the contract this module implements, and its central rule is
//! a separation that must survive every mapping in this file: `measured` is a
//! fact this product verified itself inside the guest (an exact file size and
//! its `sha256`), and `attribution` is `cargo-bloat`'s estimate of where that
//! size went. The two never merge into one number, and the DTO in
//! `bloat::schemas` marks the estimate as an estimate on every field that
//! carries it.
//!
//! The measured file is also, unavoidably, an **analysis build**: the pinned
//! analyzer forces symbol stripping off on every build it performs because it
//! needs the symbol table, so the file this tool measures is not
//! byte-identical to what a project asking for stripping would ship
//! (`docs/validation/M5-04-bloat-calibration.json`). `measured.
//! analysis_build_symbols_forced` is always `true` and says so.
//!
//! If the analyzer's own reported file size disagrees with the size this
//! product measured, the attribution describes some other file. The
//! application layer (`rust_engineering_application::bloat::
//! validate_bloat_observation`) is what refuses to publish that disagreement
//! as `complete`; this module surfaces the resulting `SizeMismatch`
//! completeness as a declared, visible state rather than smoothing it away.
//!
//! ADR-079 adds the second rule this file must not blur, and [`outcome`] is
//! where it is enforced. Three things used to collapse into one `complete`
//! boolean, and the boolean decided the status:
//!
//! 1. **validity of the measurement** — did the analysis run, and does the
//!    exact size this product measured agree with what the analyzer reported;
//! 2. **coverage of the ranking** against the product's own `BLOAT_MAX_ROWS`
//!    cap;
//! 3. **trimming of the response** to fit the fixed byte budget.
//!
//! Only (1) decides the status. (2) and (3) are declared separately, with their
//! own counters, in `attribution.ranking_cap` and `attribution.response_trim`,
//! so a reader can tell "the product bounded the ranking at 256" from "the
//! response did not fit" from "the evidence is not valid". `passed` therefore
//! means *analysis executed and validated* and nothing more — never that the
//! binary is optimized, and never that the attribution is exhaustive.
#[allow(dead_code)]
mod schemas;
use super::{
    HostCargoVendorConfig,
    benchmark::{ModeSelection, mode_selection},
    clock::WallClock,
    nextest::ExecutionModeDto,
    project::Registry,
    security_tool::{
        CommonFailure, MAX_RESULT_BYTES, artifact_fields, capture_vendor, classify_error,
        define_fallible_security_outcome, define_security_response_methods, define_security_tool,
        encode_bounded, run_joined_security,
    },
    workers::Workers,
};
use rmcp::{
    model::{CallToolRequestParams, CallToolResult, ErrorData},
    service::{RequestContext, RoleServer},
};
use rust_engineering_application::InspectionError;
use rust_engineering_application::bloat::{
    BloatOptions, BloatPorts, BloatPublisher, ProjectBloatPort, PublishedBloat,
};
use rust_engineering_application::security::SecurityError;
use rust_engineering_domain::bloat::{BLOAT_MAX_ROWS, BinaryFormat, BloatCompleteness, BloatExit};
use rust_engineering_domain::{
    ArtifactCompleteness, ExecutionTermination, ProjectRef, QualityArtifactDescriptor,
    QualityArtifactKind, RuntimeIdentity,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

pub(super) const NAME: &str = "rust.binary.bloat";
const DEFAULT_TIMEOUT_SECONDS: u64 = 120;
/// The ranking is bounded, and the bound belongs to the product (ADR-076
/// §5). It is a ceiling, not a target: it is deliberately far above the
/// analyzer parser's own row cap ([`BLOAT_MAX_ROWS`]), so the 512 KiB
/// response budget — enforced row by row in [`trim_lowest_ranked_row`] — is
/// what actually binds, not this cap.
const MAX_RESPONSE_ROWS: usize = 4_096;
const _: () = assert!(MAX_RESPONSE_ROWS > BLOAT_MAX_ROWS);
/// The response budget this tool's trimming serves, declared to the caller in
/// `attribution.response_trim.budget_bytes` so the second limit is as visible as
/// the first. It is the same budget [`encode_bounded`] enforces.
const RESPONSE_BUDGET_BYTES: u32 = MAX_RESULT_BYTES as u32;

pub(super) fn advertised() -> bool {
    super::security_tool::advertised("RUST_MCP_TEST_BLOAT_READY")
}

#[derive(Clone, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Input {
    #[schemars(with = "String", regex(pattern = "^prj_[0-9a-f]{32}$"))]
    project_ref: ProjectRef,
    /// A cargo target name, never a path, and the child receives no peer
    /// argument.
    #[schemars(
        length(min = 1, max = 64),
        regex(pattern = "^[A-Za-z0-9_][A-Za-z0-9_-]{0,63}$")
    )]
    binary_target: String,
    #[serde(default)]
    #[schemars(
        length(min = 1, max = 64),
        regex(pattern = "^[A-Za-z0-9_][A-Za-z0-9_-]{0,63}$")
    )]
    package: Option<String>,
    #[serde(default)]
    profile: schemas::BloatProfile,
    #[serde(default = "default_timeout")]
    #[schemars(range(min = 1, max = 300))]
    timeout_seconds: u64,
    #[serde(default)]
    execution_mode: ExecutionModeDto,
}
fn default_timeout() -> u64 {
    DEFAULT_TIMEOUT_SECONDS
}
impl Input {
    fn options(&self) -> Result<BloatOptions, ErrorData> {
        BloatOptions::new(
            self.binary_target.clone(),
            self.package.clone(),
            domain_profile(self.profile),
        )
        .map_err(|_| ErrorData::invalid_params("Invalid tool arguments", None))
    }
}

fn domain_profile(value: schemas::BloatProfile) -> rust_engineering_domain::bloat::BloatProfile {
    match value {
        schemas::BloatProfile::Release => rust_engineering_domain::bloat::BloatProfile::Release,
        schemas::BloatProfile::ReleaseLto => {
            rust_engineering_domain::bloat::BloatProfile::ReleaseLto
        }
    }
}
fn schema_profile(value: rust_engineering_domain::bloat::BloatProfile) -> schemas::BloatProfile {
    match value {
        rust_engineering_domain::bloat::BloatProfile::Release => schemas::BloatProfile::Release,
        rust_engineering_domain::bloat::BloatProfile::ReleaseLto => {
            schemas::BloatProfile::ReleaseLto
        }
    }
}

#[derive(Clone, Copy, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum Code {
    TasksRequired,
    SandboxDenied,
    MissingOfflineData,
    ArtifactUnavailable,
    ToolNotInstalled,
    InvalidProject,
    ProjectNotFound,
    CommandTimeout,
    OutputLimitExceeded,
    EvidenceIncomplete,
    ObservedFailure,
    AnalyzerUnavailable,
    UnsupportedFormat,
    SizeMismatch,
}
define_fallible_security_outcome!(Code, &'static str, ());

#[derive(Clone, Copy, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
enum ArtifactKind {
    BloatJson,
}
#[derive(Clone, serde::Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Artifact {
    kind: ArtifactKind,
    #[schemars(length(min = 1, max = 512))]
    uri: String,
    #[schemars(regex(pattern = "^[0-9a-f]{64}$"))]
    sha256: String,
    size_bytes: u64,
    completeness: schemas::ArtifactCompleteness,
}
#[derive(Clone, serde::Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Data {
    #[schemars(regex(pattern = "^prj_[0-9a-f]{32}$"))]
    project_ref: String,
    semantics: &'static str,
    observation: schemas::Observation,
    /// At most one: the analyzer's JSON report, published only when the
    /// analysis produced one. A build or analysis failure publishes none.
    #[schemars(length(max = 1))]
    artifacts: Vec<Artifact>,
}

/// The runtime this tool measures through.
///
/// `executor` and `publisher` are optional for the same reason as the
/// benchmark and profiling tools': the M5 execution vertical and the durable
/// JSON-report publisher land separately, and until both are attached the
/// tool answers a declared `unavailable`, never a protocol error.
pub(super) struct Runtime {
    pub(super) registry: Arc<Mutex<Registry>>,
    pub(super) workers: Workers,
    pub(super) ready: Arc<AtomicBool>,
    pub(super) vendor: Option<HostCargoVendorConfig>,
    pub(super) executor: Option<Arc<dyn ProjectBloatPort>>,
    pub(super) publisher: Option<Arc<Mutex<dyn BloatPublisher>>>,
}

/// `BloatPorts` takes sized ports; these two forward a shared handle into
/// that shape without asking the application to know about `dyn`.
struct DynExecutor<'a>(&'a dyn ProjectBloatPort);
impl ProjectBloatPort for DynExecutor<'_> {
    fn bloat(
        &self,
        source: &rust_engineering_domain::SourceBundle,
        vendor: &rust_engineering_domain::CargoVendorSnapshot,
        options: &BloatOptions,
        control: &dyn rust_engineering_application::InspectionControl,
    ) -> Result<rust_engineering_application::bloat::BloatObservation, SecurityError> {
        self.0.bloat(source, vendor, options, control)
    }
}
struct DynPublisher<'a>(&'a mut dyn BloatPublisher);
impl BloatPublisher for DynPublisher<'_> {
    fn publish_bloat(
        &mut self,
        capture: &rust_engineering_application::security::SecurityCapture,
        observation: &rust_engineering_application::bloat::BloatObservation,
        revalidate: &mut dyn FnMut() -> Result<
            rust_engineering_application::QualityOwnerFacts,
            InspectionError,
        >,
    ) -> Result<Vec<QualityArtifactDescriptor>, InspectionError> {
        self.0.publish_bloat(capture, observation, revalidate)
    }
}

define_security_tool!(
    BloatTool,
    "Measure one project binary's exact file size inside the approved sandbox, then run the pinned cargo-bloat analyzer over it and publish its raw JSON report as a private artifact. The binary is named by cargo target, never by path. The response separates two things that must never be read as one: `measured` is a fact this product verified itself (exact size and sha256), and `attribution` is cargo-bloat's estimate of where that size went, marked estimated on every field. The measured file is an analysis build: the analyzer forces symbol stripping off to read symbols, so its size is exact for that file but it is not what a project asking for stripping would ship. If the analyzer's own reported size disagrees with the size this product measured, the attribution is published as a declared size mismatch, never silently merged into the exact measurement. A passed result means the analysis executed and this product validated it against its own measurement; it never means the binary is optimized or that the attribution is exhaustive. The ranking is bounded by a cap this product owns, and the response may drop further rows to fit its byte budget: both are declared separately, with their own counts, in `attribution.ranking_cap` and `attribution.response_trim`, and neither is incomplete evidence."
);

impl BloatTool {
    pub(super) async fn call(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call_with_token(request, context.ct).await
    }

    async fn call_with_token(
        &self,
        request: CallToolRequestParams,
        request_token: tokio_util::sync::CancellationToken,
    ) -> Result<CallToolResult, ErrorData> {
        let input = self.contract.decode(request.arguments)?;
        let options = input.options()?;
        if matches!(
            mode_selection(input.execution_mode),
            ModeSelection::TasksRequired
        ) {
            return self.blocked(
                Code::TasksRequired,
                "Binary size analysis is not admitted as an MCP Task",
                None,
                0,
            );
        }
        let runtime = self
            .runtime
            .as_ref()
            .ok_or_else(|| ErrorData::internal_error("Bloat runtime is not configured", None))?;
        if !runtime.ready.load(Ordering::Acquire) {
            return self.blocked(
                Code::SandboxDenied,
                "Discovery must complete before analyzing binary size",
                None,
                0,
            );
        }
        let Some(vendor) = runtime.vendor.clone() else {
            return self.unavailable(
                Code::MissingOfflineData,
                "Host-authenticated offline vendor is required",
                0,
            );
        };
        let Some(publisher) = runtime.publisher.clone() else {
            return self.unavailable(
                Code::ArtifactUnavailable,
                "Durable bloat evidence is unavailable",
                0,
            );
        };
        let Some(executor) = runtime.executor.clone() else {
            return self.unavailable(
                Code::ToolNotInstalled,
                "Approved size analysis runtime is unavailable",
                0,
            );
        };
        let registry = Arc::clone(&runtime.registry);
        let reference = input.project_ref.clone();
        let (result, duration) = run_joined_security(
            &runtime.workers,
            request_token,
            input.timeout_seconds,
            "Bloat analysis worker unavailable",
            move |control| {
                let vendor = capture_vendor(&vendor, control)?;
                let executor = DynExecutor(executor.as_ref());
                let mut published = publisher
                    .lock()
                    .map_err(|_| SecurityError::Inspection(InspectionError::Internal))?;
                let mut published = DynPublisher(&mut *published);
                registry
                    .lock()
                    .map_err(|_| SecurityError::Inspection(InspectionError::Internal))?
                    .bloat_durable(
                        &reference,
                        &vendor,
                        &options,
                        BloatPorts {
                            executor: &executor,
                            publisher: &mut published,
                        },
                        &WallClock,
                        control,
                    )
            },
        )
        .await?;
        match result {
            Ok(result) => self.encode_result(&input.project_ref, result, duration),
            Err(error) => self.error(error, duration),
        }
    }
    define_security_response_methods!(self, ());

    fn error(&self, error: SecurityError, duration_ms: u64) -> Result<CallToolResult, ErrorData> {
        let (code, message) = match classify_error(error) {
            CommonFailure::Cancelled => {
                return self
                    .cancelled("Bloat analysis cancelled after joined cleanup", duration_ms);
            }
            CommonFailure::ToolNotInstalled => {
                return self.unavailable(
                    Code::ToolNotInstalled,
                    "Approved size analysis runtime is unavailable",
                    duration_ms,
                );
            }
            CommonFailure::Timeout => {
                (Code::CommandTimeout, "Bloat analysis exceeded its deadline")
            }
            CommonFailure::MissingOfflineData => (
                Code::MissingOfflineData,
                "Offline dependency source is missing or invalid",
            ),
            CommonFailure::OutputLimit => (
                Code::OutputLimitExceeded,
                "Bloat analysis evidence exceeded its fixed budget",
            ),
            CommonFailure::ProjectNotFound => (
                Code::ProjectNotFound,
                "Project authority is missing or expired",
            ),
            CommonFailure::SandboxDenied => (
                Code::SandboxDenied,
                "Approved size analysis execution could not be established",
            ),
            CommonFailure::Specific(_) => (
                Code::InvalidProject,
                "Captured bloat inputs or evidence could not be validated",
            ),
        };
        self.blocked(code, message, None, duration_ms)
    }

    fn encode_result(
        &self,
        reference: &ProjectRef,
        result: PublishedBloat,
        duration_ms: u64,
    ) -> Result<CallToolResult, ErrorData> {
        let artifacts = result
            .artifacts
            .iter()
            .map(|descriptor| artifact(reference, descriptor))
            .collect::<Result<Vec<_>, _>>()?;
        // ADR-079 §2 and §3, decided once and by the layer that owns the rule:
        // the application knows both whether the measurement validated and
        // whether the evidence behind it was published. Trimming below cannot
        // change it, which is exactly what keeps a response that did not fit
        // from reading as evidence that is not valid.
        let validated = result.analysis_validated();
        let data = Box::new(Data {
            project_ref: reference.to_string(),
            semantics: "measured_file_size_is_exact_attribution_rankings_are_estimated",
            observation: observation(&result.observation, validated),
            artifacts,
        });
        encode_bounded(
            &self.contract,
            data,
            duration_ms,
            "Bloat analysis serialization failed",
            |data, duration_ms| Output {
                outcome: outcome(data),
                summary: "Exact measured file size and cargo-bloat's estimated attribution",
                duration_ms,
            },
            trim_lowest_ranked_row,
            |duration_ms| Output {
                outcome: Outcome::Blocked {
                    error_code: Code::OutputLimitExceeded,
                    error_message: "Bloat analysis response exceeds its fixed budget",
                    data: None,
                },
                summary: "Bloat analysis response exceeds its fixed budget",
                duration_ms,
            },
        )
    }
}

/// The lowest-ranked function row leaves first; once none remain, the
/// lowest-ranked crate row leaves. `measured`, `completeness`,
/// `analysis_validated` and `analyzer_version` are never touched. Returns
/// `false` once there is nothing left to drop — an absent `attribution`, or one
/// whose `functions` and `crates` are both already empty — which is exactly the
/// signal [`encode_bounded`] uses to fall back to its `exhausted` output.
///
/// Every row dropped here is counted in `response_trim` and never in
/// `ranking_cap` (ADR-079 §1). The two causes stay apart in the payload, and
/// neither reaches `analysis_validated`: a response that had to shed rows to fit
/// 512 KiB is still a valid measurement, and the caller can see precisely which
/// limit cost it which rows.
fn trim_lowest_ranked_row(data: &mut Data) -> bool {
    let Some(attribution) = data.observation.attribution.as_mut() else {
        return false;
    };
    if attribution.functions.pop().is_some() {
        attribution.response_trim.functions_omitted = attribution
            .response_trim
            .functions_omitted
            .saturating_add(1);
        return true;
    }
    if attribution.crates.pop().is_some() {
        attribution.response_trim.crates_omitted =
            attribution.response_trim.crates_omitted.saturating_add(1);
        return true;
    }
    false
}

/// The status mapping, and the single place ADR-079 §2's meaning of `passed`
/// is decided: the analysis executed and this product validated it. Neither
/// omission counter is read here, on purpose.
fn outcome(data: &Data) -> Outcome {
    let observation = &data.observation;
    if observation.analysis_validated {
        return Outcome::Passed {
            error_code: (),
            error_message: (),
            data: Box::new(data.clone()),
        };
    }
    if matches!(
        observation.exit,
        schemas::BloatExit::CompilationFailed | schemas::BloatExit::AnalysisFailed
    ) {
        return Outcome::Failed {
            error_code: Code::ObservedFailure,
            error_message: "The analyzed binary target did not build, does not exist, or the analyzer refused this request",
            data: Box::new(data.clone()),
        };
    }
    let (error_code, error_message) = match observation.completeness {
        schemas::BloatCompleteness::Unavailable => (
            Code::AnalyzerUnavailable,
            "The approved size analyzer is unavailable or unapproved; no measurement was produced",
        ),
        schemas::BloatCompleteness::UnsupportedFormat => (
            Code::UnsupportedFormat,
            "The binary format is not supported by the pinned size analyzer",
        ),
        schemas::BloatCompleteness::SizeMismatch => (
            Code::SizeMismatch,
            "The analyzer's reported file size disagrees with the size this product measured; the attribution is not published as a description of the measured file",
        ),
        // A valid measurement that still did not validate: the analyzer's exit
        // was not an observed clean run, or the artifact backing the
        // attribution was not published complete (ADR-079 §3). A bounded
        // ranking never lands here — it is declared, not incomplete.
        schemas::BloatCompleteness::Complete => (
            Code::EvidenceIncomplete,
            "The analyzer's exit was not an observed clean run, or the artifact backing this attribution was not published",
        ),
    };
    Outcome::Blocked {
        error_code,
        error_message,
        data: Some(Box::new(data.clone())),
    }
}

fn artifact(
    reference: &ProjectRef,
    descriptor: &QualityArtifactDescriptor,
) -> Result<Artifact, ErrorData> {
    let kind = match descriptor.kind {
        QualityArtifactKind::BloatJson => ArtifactKind::BloatJson,
        _ => {
            return Err(ErrorData::internal_error(
                "Bloat artifact kind is invalid",
                None,
            ));
        }
    };
    let fields = artifact_fields(reference, descriptor, "Invalid bloat artifact descriptor")?;
    Ok(Artifact {
        kind,
        uri: fields.uri,
        sha256: fields.sha256,
        size_bytes: fields.size_bytes,
        completeness: match fields.completeness {
            ArtifactCompleteness::Complete => schemas::ArtifactCompleteness::Complete,
            ArtifactCompleteness::Truncated => schemas::ArtifactCompleteness::Truncated,
            ArtifactCompleteness::Partial => schemas::ArtifactCompleteness::Partial,
            ArtifactCompleteness::Invalid => schemas::ArtifactCompleteness::Invalid,
            ArtifactCompleteness::Unavailable => schemas::ArtifactCompleteness::Unavailable,
        },
    })
}

fn observation(
    value: &rust_engineering_application::bloat::BloatObservation,
    analysis_validated: bool,
) -> schemas::Observation {
    let attribution = value.attribution.as_ref().map(|attribution| {
        let mut functions: Vec<schemas::BloatFunction> = attribution
            .functions
            .iter()
            .map(|function| schemas::BloatFunction {
                crate_name: function.crate_name.clone(),
                name: function.name.clone(),
                size_bytes: function.size_bytes,
            })
            .collect();
        // Ranked so the trimming strategy drops the least weighted row first.
        functions.sort_by(|left, right| {
            right
                .size_bytes
                .cmp(&left.size_bytes)
                .then_with(|| left.crate_name.cmp(&right.crate_name))
                .then_with(|| left.name.cmp(&right.name))
        });
        // The DTO's own ceiling sits above `BLOAT_MAX_ROWS`, so it can only act
        // on a ranking the product's cap did not already bound. It is another
        // product cap, never the response budget, and it is counted as such.
        let extra_functions =
            u32::try_from(functions.len().saturating_sub(MAX_RESPONSE_ROWS)).unwrap_or(u32::MAX);
        functions.truncate(MAX_RESPONSE_ROWS);

        let mut crates: Vec<schemas::BloatCrate> = attribution
            .crates
            .iter()
            .map(|krate| schemas::BloatCrate {
                name: krate.name.clone(),
                size_bytes: krate.size_bytes,
            })
            .collect();
        crates.sort_by(|left, right| {
            right
                .size_bytes
                .cmp(&left.size_bytes)
                .then_with(|| left.name.cmp(&right.name))
        });
        let extra_crates =
            u32::try_from(crates.len().saturating_sub(MAX_RESPONSE_ROWS)).unwrap_or(u32::MAX);
        crates.truncate(MAX_RESPONSE_ROWS);

        schemas::BloatAttribution {
            estimated: attribution.estimated,
            reported_file_size_bytes: attribution.reported_file_size_bytes,
            text_section_size_bytes: attribution.text_section_size_bytes,
            functions,
            crates,
            ranking_cap: schemas::BloatRankingCap {
                max_rows: u32::try_from(BLOAT_MAX_ROWS).unwrap_or(u32::MAX),
                functions_omitted: attribution
                    .functions_omitted_by_row_cap
                    .saturating_add(extra_functions),
                crates_omitted: attribution
                    .crates_omitted_by_row_cap
                    .saturating_add(extra_crates),
            },
            // Nothing is trimmed yet: `trim_lowest_ranked_row` is what fills
            // this in, and only if the encoded response does not fit.
            response_trim: schemas::BloatResponseTrim {
                budget_bytes: RESPONSE_BUDGET_BYTES,
                functions_omitted: 0,
                crates_omitted: 0,
            },
        }
    });
    let measured = value
        .measured
        .as_ref()
        .map(|measured| schemas::MeasuredBinary {
            size_bytes: measured.size_bytes,
            sha256: measured.sha256.clone(),
            format: format(measured.format),
            analysis_build_symbols_forced: measured.analysis_build_symbols_forced,
        });
    let exit = exit(value.exit);
    let completeness = completeness(value.completeness);
    schemas::Observation {
        binary_target: value.options.binary_target().to_owned(),
        package: value.options.package().map(str::to_owned),
        profile: schema_profile(value.options.profile()),
        analyzer_version: value.analyzer_version.clone(),
        exit,
        exit_code: value.exit_code,
        termination: termination(value.termination),
        analysis_validated,
        measured,
        attribution,
        completeness,
        runtime: runtime_identity(&value.runtime),
        execution_fingerprint: value.execution_fingerprint.to_string(),
        vendor_fingerprint: value.vendor_fingerprint.to_string(),
        stdout_truncated: value.stdout_truncated,
        stderr_truncated: value.stderr_truncated,
    }
}

fn format(value: BinaryFormat) -> schemas::BinaryFormat {
    match value {
        BinaryFormat::Elf64Aarch64 => schemas::BinaryFormat::Elf64Aarch64,
        BinaryFormat::OtherElf => schemas::BinaryFormat::OtherElf,
        BinaryFormat::MachO => schemas::BinaryFormat::MachO,
        BinaryFormat::Pe => schemas::BinaryFormat::Pe,
        BinaryFormat::Wasm => schemas::BinaryFormat::Wasm,
        BinaryFormat::Unknown => schemas::BinaryFormat::Unknown,
    }
}

fn exit(value: BloatExit) -> schemas::BloatExit {
    match value {
        BloatExit::Passed => schemas::BloatExit::Passed,
        BloatExit::AnalysisFailed => schemas::BloatExit::AnalysisFailed,
        BloatExit::CompilationFailed => schemas::BloatExit::CompilationFailed,
        BloatExit::Uncalibrated => schemas::BloatExit::Uncalibrated,
        BloatExit::Incomplete => schemas::BloatExit::Incomplete,
    }
}

fn completeness(value: BloatCompleteness) -> schemas::BloatCompleteness {
    match value {
        BloatCompleteness::Complete => schemas::BloatCompleteness::Complete,
        BloatCompleteness::SizeMismatch => schemas::BloatCompleteness::SizeMismatch,
        BloatCompleteness::UnsupportedFormat => schemas::BloatCompleteness::UnsupportedFormat,
        BloatCompleteness::Unavailable => schemas::BloatCompleteness::Unavailable,
    }
}

fn termination(value: ExecutionTermination) -> schemas::ExecutionTermination {
    match value {
        ExecutionTermination::Exited => schemas::ExecutionTermination::Exited,
        ExecutionTermination::TimedOut => schemas::ExecutionTermination::TimedOut,
        ExecutionTermination::Cancelled => schemas::ExecutionTermination::Cancelled,
        ExecutionTermination::OutputLimit => schemas::ExecutionTermination::OutputLimit,
    }
}

fn runtime_identity(value: &RuntimeIdentity) -> schemas::RuntimeIdentity {
    schemas::RuntimeIdentity {
        platform: value.platform.clone(),
        image_id: value.image_id.clone(),
        configuration_fingerprint: value.configuration_fingerprint.to_string(),
        execution_fingerprint: value.execution_fingerprint.to_string(),
        rust_version: value.rust_version.clone(),
        cargo_version: value.cargo_version.clone(),
        declared_toolchain: value.declared_toolchain.clone(),
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod trim_tests;
