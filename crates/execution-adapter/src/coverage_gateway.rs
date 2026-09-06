//! Closed two-phase cargo-llvm-cov execution and fixed report egress.
use super::mutation_gateway::VOLUME_OPTIONS;
use super::mutation_outcomes::validated_closed_ustar;
use super::nextest_gateway::decode_single_file_tar;
use super::rust_gateway::{
    Phase, PhaseRequest, RustGateway, Volume, WorkBudget, finish_work, labels,
};
use super::*;
use rust_engineering_domain::coverage::{CoverageOptions, CoverageReportFormat};
use rust_engineering_domain::{RustCommand, SourceBundle};
use std::sync::Mutex;

pub(super) const MAX_COVERAGE_EXPORT: usize = 8 * 1024 * 1024 + 2048;
const JSON_MEMBER: &str = "coverage.json";
const LCOV_MEMBER: &str = "lcov.info";
pub(super) const COVERAGE_TARGET_PATH: &str = "/work/coverage-target";
/// ADR-065's per-job executable target. The inode density matches the existing
/// 64 MiB/8,192-inode report profile while the 512 MiB ceiling matches the
/// already-qualified `/work` build ceiling. Only `noexec` is removed.
pub(super) const COVERAGE_TARGET_VOLUME_OPTIONS: &str =
    "size=512m,nr_inodes=65536,uid=65534,gid=65534,mode=0700,nosuid,nodev";

/// Validate the opaque HTML report bundle.  Its contents are deliberately not
/// interpreted or previewed, but tar links/devices and escaping names are not
/// artifacts we can retain safely.  The exporter emits USTAR and this parser
/// accepts only regular files/directories rooted at `./`.
fn validated_html_archive(bytes: &[u8]) -> Option<Vec<u8>> {
    validated_closed_ustar(bytes, MAX_COVERAGE_EXPORT, 16_384).map(|(bytes, _)| bytes)
}

pub struct CoverageExecution {
    pub result: rust_engineering_domain::ExecutionResult,
    pub json: Option<Vec<u8>>,
    pub lcov: Option<Vec<u8>>,
    pub html: Option<Vec<u8>>,
    pub json_truncated: bool,
    pub lcov_truncated: bool,
    pub html_truncated: bool,
}

pub(super) fn execute(
    gateway: &RustGateway,
    source: &SourceBundle,
    options: &CoverageOptions,
    limits: ExecutionLimits,
    cancel: &dyn ExecutionCancellation,
) -> Result<CoverageExecution, ExecutionError> {
    let started = Instant::now();
    let _busy = gateway.hold_busy()?;
    if gateway.is_quarantined()
        || gateway.calibrating.load(Ordering::Acquire)
        || !gateway.verified.load(Ordering::Acquire)
    {
        return Err(ExecutionError::Denied);
    }
    gateway.approved_runtime(cancel)?;
    let archive = super::source_archive::encode(source)?;
    let budget = WorkBudget {
        started,
        deadline: started + Duration::from_millis(limits.wall_ms()),
        limits,
        cancel,
    };
    let nonce = state::nonce()?;
    let source_volume = format!("rust-mcp-source-{nonce}");
    let report_volume = format!("rust-mcp-coverage-output-{nonce}");
    let target_volume = format!("rust-mcp-coverage-target-{nonce}");
    let ingest = format!("rust-mcp-ingest-{nonce}");
    let run = format!("rust-mcp-coverage-run-{nonce}");
    let guardian = format!("rust-mcp-coverage-guardian-{nonce}");
    let json_report = format!("rust-mcp-coverage-report-{JSON_MEMBER}-{nonce}");
    let json_export = format!("rust-mcp-coverage-export-{JSON_MEMBER}-{nonce}");
    let lcov_report = format!("rust-mcp-coverage-report-{LCOV_MEMBER}-{nonce}");
    let lcov_export = format!("rust-mcp-coverage-export-{LCOV_MEMBER}-{nonce}");
    let html_report = format!("rust-mcp-coverage-report-html-{nonce}");
    let html_export = format!("rust-mcp-coverage-export-html-{nonce}");
    if !gateway.absent("volume", &source_volume)?
        || !gateway.absent("volume", &report_volume)?
        || !gateway.absent("volume", &target_volume)?
    {
        return Err(ExecutionError::CleanupUncertain);
    }
    let json = Mutex::new(None);
    let lcov = Mutex::new(None);
    let html = Mutex::new(None);
    let command = RustCommand::CoverageRun(options.clone());
    let work = (|| {
        let mut create = vec!["volume".into(), "create".into(), "--driver=local".into()];
        for (k, v) in labels(&nonce) {
            create.push(format!("--label={k}={v}"));
        }
        create.push(source_volume.clone());
        if gateway.inner.control(&create)?.code != Some(0) {
            return Err(ExecutionError::Infrastructure);
        }
        let inspect =
            gateway
                .inner
                .control(&["volume".into(), "inspect".into(), source_volume.clone()])?;
        if inspect.code != Some(0) {
            return Err(ExecutionError::Infrastructure);
        }
        let volume = Volume::parse(&inspect.stdout, &source_volume, &nonce)?;
        let output = super::mutation_gateway::create_tmpfs_volume(
            gateway,
            &report_volume,
            &nonce,
            VOLUME_OPTIONS,
        )?;
        let target = super::mutation_gateway::create_tmpfs_volume(
            gateway,
            &target_volume,
            &nonce,
            COVERAGE_TARGET_VOLUME_OPTIONS,
        )?;
        gateway.start_coverage_output_guardian(
            PhaseRequest {
                name: &guardian,
                nonce: &nonce,
                volume: &volume,
                phase: &Phase::GuardCoverageVolumes,
            },
            &output,
            &target,
            &budget,
        )?;
        let (ingested, oom) = gateway.phase(
            PhaseRequest {
                name: &ingest,
                nonce: &nonce,
                volume: &volume,
                phase: &Phase::Ingest,
            },
            &archive,
            &budget,
        )?;
        if ingested.stop != Stop::Exited {
            return Ok((ingested, oom));
        }
        if ingested.code != Some(0) {
            return Err(ExecutionError::Infrastructure);
        }
        gateway.inner.remove(&ingest)?;
        let (mut outcome, oom) = gateway.coverage_phase(
            PhaseRequest {
                name: &run,
                nonce: &nonce,
                volume: &volume,
                phase: &Phase::Run(command.clone()),
            },
            &output,
            &target,
            &budget,
        )?;
        if outcome.stop == Stop::Exited && outcome.code == Some(0) {
            gateway.inner.remove(&run)?;
            for (format, phase, member, slot) in [
                (
                    CoverageReportFormat::Json,
                    Phase::ExportCoverageJson,
                    JSON_MEMBER,
                    &json,
                ),
                (
                    CoverageReportFormat::Lcov,
                    Phase::ExportCoverageLcov,
                    LCOV_MEMBER,
                    &lcov,
                ),
            ] {
                let (report, export) = if member == JSON_MEMBER {
                    (&json_report, &json_export)
                } else {
                    (&lcov_report, &lcov_export)
                };
                let (reported, _) = gateway.coverage_phase(
                    PhaseRequest {
                        name: report,
                        nonce: &nonce,
                        volume: &volume,
                        phase: &Phase::Run(RustCommand::CoverageReport(format)),
                    },
                    &output,
                    &target,
                    &budget,
                )?;
                if reported.stop != Stop::Exited || reported.code != Some(0) {
                    outcome.stderr.extend_from_slice(&reported.stderr);
                    continue;
                }
                gateway.inner.remove(report)?;
                let (exported, _) = gateway.coverage_phase(
                    PhaseRequest {
                        name: export,
                        nonce: &nonce,
                        volume: &volume,
                        phase: &phase,
                    },
                    &output,
                    &target,
                    &budget,
                )?;
                if exported.stop == Stop::Exited
                    && exported.code == Some(0)
                    && !exported.stdout_truncated
                {
                    *slot.lock().map_err(|_| ExecutionError::Infrastructure)? =
                        decode_single_file_tar(&exported.stdout, MAX_COVERAGE_EXPORT, member);
                } else {
                    outcome.stderr.extend_from_slice(&exported.stderr);
                }
                gateway.inner.remove(export)?;
            }
            let (reported, _) = gateway.coverage_phase(
                PhaseRequest {
                    name: &html_report,
                    nonce: &nonce,
                    volume: &volume,
                    phase: &Phase::Run(RustCommand::CoverageReport(CoverageReportFormat::Html)),
                },
                &output,
                &target,
                &budget,
            )?;
            if reported.stop == Stop::Exited && reported.code == Some(0) {
                gateway.inner.remove(&html_report)?;
                let (exported, _) = gateway.coverage_phase(
                    PhaseRequest {
                        name: &html_export,
                        nonce: &nonce,
                        volume: &volume,
                        phase: &Phase::ExportCoverageHtml,
                    },
                    &output,
                    &target,
                    &budget,
                )?;
                if exported.stop == Stop::Exited
                    && exported.code == Some(0)
                    && !exported.stdout_truncated
                    && exported.stdout.len() <= MAX_COVERAGE_EXPORT
                {
                    *html.lock().map_err(|_| ExecutionError::Infrastructure)? =
                        validated_html_archive(&exported.stdout)
                } else {
                    outcome.stderr.extend_from_slice(&exported.stderr)
                }
                gateway.inner.remove(&html_export)?;
            } else {
                outcome.stderr.extend_from_slice(&reported.stderr)
            }
        }
        Ok((outcome, oom))
    })();
    let terminal = budget.stop();
    gateway.cleanup_coverage_with_target(
        &[
            &ingest,
            &run,
            &guardian,
            &json_report,
            &json_export,
            &lcov_report,
            &lcov_export,
            &html_report,
            &html_export,
        ],
        &source_volume,
        &report_volume,
        &target_volume,
        &nonce,
    )?;
    let (outcome, oom) = finish_work(work, terminal)?;
    let (stdout, expanded_out) = bounded_text(&outcome.stdout, limits.output_bytes());
    let (stderr, expanded_err) = bounded_text(&outcome.stderr, limits.output_bytes());
    let termination = if expanded_out || expanded_err {
        ExecutionTermination::OutputLimit
    } else {
        match outcome.stop {
            Stop::Exited => ExecutionTermination::Exited,
            Stop::Cancelled => ExecutionTermination::Cancelled,
            Stop::TimedOut => ExecutionTermination::TimedOut,
            Stop::OutputLimit => ExecutionTermination::OutputLimit,
        }
    };
    let identity = serde_json::to_vec(&(
        gateway.configuration_fingerprint()?,
        &command,
        limits,
        digest(&archive),
        "rust-coverage-profile-v1",
    ))
    .map_err(|_| ExecutionError::Infrastructure)?;
    let result = rust_engineering_domain::ExecutionResult {
        termination,
        exit_code: if outcome.stop == Stop::Exited {
            outcome.code
        } else {
            None
        },
        oom_killed: oom,
        stdout,
        stderr,
        stdout_truncated: outcome.stdout_truncated || expanded_out,
        stderr_truncated: outcome.stderr_truncated || expanded_err,
        duration_ms: outcome.duration_ms,
        total_duration_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
        execution_fingerprint: digest(&identity)
            .parse()
            .map_err(|_| ExecutionError::Infrastructure)?,
        platform: "linux/aarch64",
        image_id: gateway.image_id().into(),
    };
    let json = json
        .into_inner()
        .map_err(|_| ExecutionError::Infrastructure)?;
    let lcov = lcov
        .into_inner()
        .map_err(|_| ExecutionError::Infrastructure)?;
    let html = html
        .into_inner()
        .map_err(|_| ExecutionError::Infrastructure)?;
    Ok(CoverageExecution {
        json_truncated: json.is_none(),
        lcov_truncated: lcov.is_none(),
        html_truncated: html.is_none(),
        result,
        json,
        lcov,
        html,
    })
}

#[cfg(test)]
mod tests {
    use super::validated_html_archive;

    fn archive(entries: &[(&str, u8, &[u8])]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for (name, kind, body) in entries {
            let mut header = [0_u8; 512];
            header[..name.len()].copy_from_slice(name.as_bytes());
            header[124..136].copy_from_slice(format!("{:011o}\0", body.len()).as_bytes());
            header[108..116].copy_from_slice(b"0177776\0");
            header[116..124].copy_from_slice(b"0177776\0");
            header[156] = *kind;
            header[257..263].copy_from_slice(b"ustar\0");
            bytes.extend_from_slice(&header);
            bytes.extend_from_slice(body);
            bytes.resize(bytes.len().div_ceil(512) * 512, 0);
        }
        bytes.resize(bytes.len() + 1024, 0);
        bytes
    }

    #[test]
    fn html_archive_accepts_only_the_tar_root_and_safe_regular_descendants() {
        let valid = archive(&[
            ("./", b'5', b""),
            ("./html/", b'5', b""),
            ("./html/index.html", b'0', b"ok"),
        ]);
        assert_eq!(validated_html_archive(&valid), Some(valid.clone()));

        for invalid in [
            archive(&[("/index.html", b'0', b"x")]),
            archive(&[("./../escape", b'0', b"x")]),
            archive(&[("./link", b'2', b"")]),
            archive(&[("./", b'0', b"")]),
        ] {
            assert_eq!(validated_html_archive(&invalid), None);
        }

        let mut prefixed_escape = valid.clone();
        prefixed_escape[345..348].copy_from_slice(b"../");
        assert_eq!(validated_html_archive(&prefixed_escape), None);

        // Regression fixture for V-SEC-02: a realistic HTML tree can exceed
        // the unchanged M1 256 KiB artifact cap and must still survive the
        // Stage-1 32 MiB member ceiling as one opaque ArchiveBundle.
        let large_body = vec![b'x'; 320 * 1024];
        let large = archive(&[("./", b'5', b""), ("./html/index.html", b'0', &large_body)]);
        assert!(large.len() > 256 * 1024);
        assert_eq!(validated_html_archive(&large), Some(large.clone()));
    }
}
