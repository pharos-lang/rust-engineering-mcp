//! Closed dual-source gateway for `cargo semver-checks check-release`.

use super::rust_gateway::{
    Phase, PhaseRequest, RustGateway, Volume, WorkBudget, finish_work, labels,
};
use super::*;
use rust_engineering_domain::semver_check::{APPROVED_SEMVER_CHECKS_VERSION, SemverCommandOptions};
use rust_engineering_domain::{
    ExecutionLimits, ExecutionResult, ExecutionTermination, RustCommand, SourceBundle,
};

fn create_volume(gateway: &RustGateway, name: &str, nonce: &str) -> Result<Volume, ExecutionError> {
    let mut args = vec!["volume".into(), "create".into(), "--driver=local".into()];
    for (key, value) in labels(nonce) {
        args.push(format!("--label={key}={value}"));
    }
    args.push(name.into());
    if gateway.inner.control(&args)?.code != Some(0) {
        return Err(ExecutionError::Infrastructure);
    }
    let inspected = gateway
        .inner
        .control(&["volume".into(), "inspect".into(), name.into()])?;
    if inspected.code != Some(0) {
        return Err(ExecutionError::Infrastructure);
    }
    Volume::parse(&inspected.stdout, name, nonce)
}

pub(super) fn execute(
    gateway: &RustGateway,
    baseline: &SourceBundle,
    candidate: &SourceBundle,
    options: &SemverCommandOptions,
    limits: ExecutionLimits,
    cancel: &dyn ExecutionCancellation,
) -> Result<ExecutionResult, ExecutionError> {
    let started = Instant::now();
    let _busy = gateway.hold_busy()?;
    if gateway.is_quarantined()
        || gateway.calibrating.load(Ordering::Acquire)
        || !gateway.verified.load(Ordering::Acquire)
    {
        return Err(ExecutionError::Denied);
    }
    gateway.approved_runtime(cancel)?;

    let baseline_archive = super::source_archive::encode(baseline)?;
    let candidate_archive = super::source_archive::encode(candidate)?;
    let budget = WorkBudget {
        started,
        deadline: started + Duration::from_millis(limits.wall_ms()),
        limits,
        cancel,
    };
    let nonce = state::nonce()?;
    let candidate_volume = format!("rust-mcp-source-{nonce}");
    let baseline_volume = format!("rust-mcp-semver-baseline-{nonce}");
    let ingest_candidate = format!("rust-mcp-ingest-{nonce}");
    let ingest_baseline = format!("rust-mcp-semver-ingest-{nonce}");
    let version = format!("rust-mcp-semver-version-{nonce}");
    let run = format!("rust-mcp-cargo-{nonce}");
    if !gateway.absent("volume", &candidate_volume)?
        || !gateway.absent("volume", &baseline_volume)?
    {
        return Err(ExecutionError::CleanupUncertain);
    }

    let work = (|| {
        let candidate_mount = create_volume(gateway, &candidate_volume, &nonce)?;
        let (candidate_ingested, candidate_oom) = gateway.phase(
            PhaseRequest {
                name: &ingest_candidate,
                nonce: &nonce,
                volume: &candidate_mount,
                phase: &Phase::Ingest,
            },
            &candidate_archive,
            &budget,
        )?;
        if candidate_ingested.stop != Stop::Exited {
            return Ok((candidate_ingested, candidate_oom));
        }
        if candidate_ingested.code != Some(0) {
            return Err(ExecutionError::Infrastructure);
        }
        gateway.inner.remove(&ingest_candidate)?;

        let baseline_mount = create_volume(gateway, &baseline_volume, &nonce)?;
        let (baseline_ingested, baseline_oom) = gateway.phase(
            PhaseRequest {
                name: &ingest_baseline,
                nonce: &nonce,
                volume: &baseline_mount,
                phase: &Phase::IngestBaseline,
            },
            &baseline_archive,
            &budget,
        )?;
        if baseline_ingested.stop != Stop::Exited {
            return Ok((baseline_ingested, baseline_oom));
        }
        if baseline_ingested.code != Some(0) {
            return Err(ExecutionError::Infrastructure);
        }
        gateway.inner.remove(&ingest_baseline)?;

        let (observed_version, version_oom) = gateway.phase(
            PhaseRequest {
                name: &version,
                nonce: &nonce,
                volume: &candidate_mount,
                phase: &Phase::Run(RustCommand::SemverChecksVersion),
            },
            &[],
            &budget,
        )?;
        if observed_version.stop != Stop::Exited {
            return Ok((observed_version, version_oom));
        }
        let expected_version = format!("cargo-semver-checks {APPROVED_SEMVER_CHECKS_VERSION}\n");
        if observed_version.code != Some(0)
            || observed_version.stdout != expected_version.as_bytes()
        {
            return Err(ExecutionError::Unavailable);
        }
        gateway.inner.remove(&version)?;

        gateway.phase_with_baseline(
            PhaseRequest {
                name: &run,
                nonce: &nonce,
                volume: &candidate_mount,
                phase: &Phase::Run(RustCommand::SemverCheck(options.clone())),
            },
            &baseline_mount,
            &budget,
        )
    })();
    let terminal = budget.stop();
    gateway.cleanup_with_baseline(
        &ingest_candidate,
        &ingest_baseline,
        &version,
        &run,
        &candidate_volume,
        &baseline_volume,
        &nonce,
    )?;
    let (outcome, oom_killed) = finish_work(work, terminal)?;
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
        RustCommand::SemverCheck(options.clone()),
        limits,
        digest(&baseline_archive),
        digest(&candidate_archive),
        "rust-semver-dual-source-profile-v1",
    ))
    .map_err(|_| ExecutionError::Infrastructure)?;
    Ok(ExecutionResult {
        termination,
        exit_code: if outcome.stop == Stop::Exited {
            outcome.code
        } else {
            None
        },
        oom_killed,
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
    })
}
