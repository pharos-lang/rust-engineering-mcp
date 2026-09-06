//! Closed `cargo mutants` execution for M3-05.
//!
//! Containment shape, in order:
//!
//! 1. the captured host source is ingested once into the ordinary source volume
//!    and is mounted **read-only** for every later phase — no mutation phase is
//!    an ingesting phase, so nothing can write back to it;
//! 2. a listing phase generates the mutant set without building or running
//!    anything, so the `max_mutants` cap is enforced before any project code
//!    executes;
//! 3. the run phase works in a container-private tmpfs (`TMPDIR`) that is
//!    created empty, is owned by the unprivileged guest user, is destroyed with
//!    the container and is mounted by no exporter: the mutated copy therefore
//!    cannot be exported even in principle;
//! 4. only report bytes leave, through three fixed-argv `tar` exporters reading
//!    the bounded `mutants.out` volume, which is mounted read-only in the
//!    exporters. `docker cp` is never used.
//!
//! Cleanup joins every owned container before either volume is removed and
//! quarantines the gateway on any uncertainty, exactly like the nextest and
//! coverage verticals.
use super::mutation_gateway::VOLUME_OPTIONS;
use super::mutation_outcomes::{
    MAX_LOCK_JSON, MAX_OUTCOMES_JSON, guest_identity, validated_report_bundle,
};
use super::nextest_gateway::decode_single_file_tar;
use super::rust_gateway::{
    Phase, PhaseRequest, RustGateway, Volume, WorkBudget, finish_work, labels,
};
use super::*;
use rust_engineering_domain::mutation_test::{MutationGuestIdentity, MutationTestCommandOptions};
use rust_engineering_domain::{RustCommand, SourceBundle};
use std::sync::Mutex;

/// Fixed guest mount point of the bounded `mutants.out` report volume.
pub(super) const MUTATION_OUTPUT_TARGET: &str = "/mutants";
const OUTCOMES_MEMBER: &str = "outcomes.json";
const LOCK_MEMBER: &str = "lock.json";

pub(super) const MAX_OUTCOMES_EXPORT: usize = MAX_OUTCOMES_JSON + 2048;
pub(super) const MAX_LOCK_EXPORT: usize = MAX_LOCK_JSON + 2048;
/// ADR-062 §4 uses the same ceiling for the coverage report bundle.
pub(super) const MAX_BUNDLE_EXPORT: usize = 8 * 1024 * 1024 + 2048;
/// `mutants --list --json` output for a capped mutant set. A listing larger
/// than this is refused rather than counted.
pub(super) const MAX_LIST_EXPORT: usize = 1024 * 1024;
/// `diff/` and `logs/` hold roughly two files per mutant plus the shared
/// directories and list files, so this bounds a full 100-mutant report with
/// headroom while keeping the entry count finite.
pub(super) const MAX_BUNDLE_ENTRIES: u16 = 512;

/// Per-phase stdout budget. The exporters carry whole files and therefore need
/// their own bounds rather than the caller's diagnostic output budget.
pub(super) fn output_limit(phase: &Phase, limits: ExecutionLimits) -> usize {
    match phase {
        Phase::ExportMutationOutcomes => MAX_OUTCOMES_EXPORT,
        Phase::ExportMutationBundle => MAX_BUNDLE_EXPORT,
        Phase::ExportMutationLock => MAX_LOCK_EXPORT,
        Phase::ListMutants(_) => MAX_LIST_EXPORT,
        _ => limits.output_bytes(),
    }
}

pub struct MutationTestExecution {
    pub result: rust_engineering_domain::ExecutionResult,
    /// Mutants generated for this selection, from the listing pass. `None` when
    /// the listing itself did not produce trustworthy output.
    pub listed: Option<u32>,
    /// The generated set exceeded the caller's cap; nothing was built or run.
    pub cap_exceeded: bool,
    pub outcomes: Option<Vec<u8>>,
    pub bundle: Option<Vec<u8>>,
    pub bundle_entries: u16,
    /// A report bundle existed but did not survive its bound or its profile.
    pub bundle_unavailable: bool,
    pub identity: MutationGuestIdentity,
}

pub(super) fn execute(
    gateway: &RustGateway,
    source: &SourceBundle,
    options: &MutationTestCommandOptions,
    limits: ExecutionLimits,
    cancel: &dyn ExecutionCancellation,
) -> Result<MutationTestExecution, ExecutionError> {
    let started = Instant::now();
    let _busy = gateway.hold_busy()?;
    if gateway.is_quarantined() {
        return Err(ExecutionError::CleanupUncertain);
    }
    if gateway.calibrating.load(Ordering::Acquire) || !gateway.verified.load(Ordering::Acquire) {
        return Err(ExecutionError::Denied);
    }
    gateway.approved_runtime(cancel)?;
    let archive = super::source_archive::encode_mutation_source(source)?;
    let budget = WorkBudget {
        started,
        deadline: started + Duration::from_millis(limits.wall_ms()),
        limits,
        cancel,
    };
    let nonce = state::nonce()?;
    let source_volume = format!("rust-mcp-source-{nonce}");
    let report_volume = format!("rust-mcp-mutants-output-{nonce}");
    let ingest = format!("rust-mcp-ingest-{nonce}");
    let guardian = format!("rust-mcp-mutants-guardian-{nonce}");
    let list = format!("rust-mcp-mutants-list-{nonce}");
    let run = format!("rust-mcp-mutants-run-{nonce}");
    let outcomes_export = format!("rust-mcp-mutants-export-outcomes-{nonce}");
    let bundle_export = format!("rust-mcp-mutants-export-bundle-{nonce}");
    let lock_export = format!("rust-mcp-mutants-export-lock-{nonce}");
    if !gateway.absent("volume", &source_volume)? || !gateway.absent("volume", &report_volume)? {
        return Err(ExecutionError::CleanupUncertain);
    }
    let outcomes: Mutex<Option<Vec<u8>>> = Mutex::new(None);
    let bundle: Mutex<Option<(Vec<u8>, u16)>> = Mutex::new(None);
    let bundle_seen = Mutex::new(false);
    let identity = Mutex::new(MutationGuestIdentity::Unavailable);
    let listed: Mutex<Option<u32>> = Mutex::new(None);
    let capped = Mutex::new(false);
    let command = RustCommand::MutationTest(options.clone());
    let listing = Phase::ListMutants(options.clone());
    let work = (|| {
        let mut create = vec!["volume".into(), "create".into(), "--driver=local".into()];
        for (key, value) in labels(&nonce) {
            create.push(format!("--label={key}={value}"));
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
        gateway.start_mutation_output_guardian(
            PhaseRequest {
                name: &guardian,
                nonce: &nonce,
                volume: &volume,
                phase: &Phase::GuardMutationOutput,
            },
            &output,
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
        // No source writer remains while untrusted project code executes.
        gateway.inner.remove(&ingest)?;

        // Cost control before execution: the listing phase builds nothing and
        // runs no test, so refusing here costs the caller one cheap pass
        // instead of an unbounded mutation run.
        let (listing_capture, listing_oom) = gateway.mutation_test_phase(
            PhaseRequest {
                name: &list,
                nonce: &nonce,
                volume: &volume,
                phase: &listing,
            },
            &output,
            false,
            &budget,
        )?;
        if listing_capture.stop != Stop::Exited {
            return Ok((listing_capture, listing_oom));
        }
        let counted = (listing_capture.code == Some(0) && !listing_capture.stdout_truncated)
            .then(|| {
                super::mutation_outcomes::count_listed_mutants(
                    &listing_capture.stdout,
                    options.max_mutants(),
                )
            })
            .flatten();
        *listed.lock().map_err(|_| ExecutionError::Infrastructure)? = counted;
        match counted {
            // Without a trustworthy denominator the run would be unbounded and
            // its counts unverifiable, so nothing is built. This is reported as
            // incomplete evidence, not as an exceeded cap: the caller's
            // selection may be perfectly reasonable.
            None => return Ok((listing_capture, listing_oom)),
            Some(count) if count > options.max_mutants() => {
                *capped.lock().map_err(|_| ExecutionError::Infrastructure)? = true;
                return Ok((listing_capture, listing_oom));
            }
            Some(_) => (),
        }
        gateway.inner.remove(&list)?;

        let (mut outcome, oom) = gateway.mutation_test_phase(
            PhaseRequest {
                name: &run,
                nonce: &nonce,
                volume: &volume,
                phase: &Phase::Run(command.clone()),
            },
            &output,
            true,
            &budget,
        )?;
        if outcome.stop != Stop::Exited {
            return Ok((outcome, oom));
        }
        gateway.inner.remove(&run)?;
        // Evidence is collected for every exit, including the mandatory
        // baseline failure path: a failing baseline is a valid tool outcome
        // that must carry its own evidence.
        for (name, phase, member) in [
            (
                &outcomes_export,
                Phase::ExportMutationOutcomes,
                OUTCOMES_MEMBER,
            ),
            (&lock_export, Phase::ExportMutationLock, LOCK_MEMBER),
        ] {
            let (exported, _) = gateway.mutation_test_phase(
                PhaseRequest {
                    name,
                    nonce: &nonce,
                    volume: &volume,
                    phase: &phase,
                },
                &output,
                false,
                &budget,
            )?;
            if exported.stop != Stop::Exited {
                return Ok((outcome, oom));
            }
            if exported.code == Some(0) && !exported.stdout_truncated {
                let bytes = decode_single_file_tar(
                    &exported.stdout,
                    output_limit(&phase, budget.limits),
                    member,
                );
                match (member, bytes) {
                    (OUTCOMES_MEMBER, bytes) => {
                        *outcomes
                            .lock()
                            .map_err(|_| ExecutionError::Infrastructure)? = bytes;
                    }
                    (_, Some(bytes)) => {
                        // Identity is asserted here and the bytes are dropped:
                        // lock.json never reaches an artifact or a response.
                        *identity
                            .lock()
                            .map_err(|_| ExecutionError::Infrastructure)? = guest_identity(&bytes);
                    }
                    (_, None) => (),
                }
            } else if !exported.stderr.is_empty() {
                outcome
                    .stderr
                    .extend_from_slice(b"\nfixed mutation exporter: ");
                outcome.stderr.extend_from_slice(&exported.stderr);
            }
            gateway.inner.remove(name)?;
        }
        let (exported, _) = gateway.mutation_test_phase(
            PhaseRequest {
                name: &bundle_export,
                nonce: &nonce,
                volume: &volume,
                phase: &Phase::ExportMutationBundle,
            },
            &output,
            false,
            &budget,
        )?;
        if exported.stop != Stop::Exited {
            return Ok((outcome, oom));
        }
        *bundle_seen
            .lock()
            .map_err(|_| ExecutionError::Infrastructure)? = true;
        if exported.code == Some(0) && !exported.stdout_truncated {
            *bundle.lock().map_err(|_| ExecutionError::Infrastructure)? =
                validated_report_bundle(&exported.stdout, MAX_BUNDLE_EXPORT, MAX_BUNDLE_ENTRIES);
        } else if !exported.stderr.is_empty() {
            outcome
                .stderr
                .extend_from_slice(b"\nfixed mutation exporter: ");
            outcome.stderr.extend_from_slice(&exported.stderr);
        }
        Ok((outcome, oom))
    })();
    let terminal = budget.stop();
    gateway.cleanup_coverage(
        &[
            &ingest,
            &list,
            &run,
            &guardian,
            &outcomes_export,
            &bundle_export,
            &lock_export,
        ],
        &source_volume,
        &report_volume,
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
    let identity_fingerprint = serde_json::to_vec(&(
        gateway.configuration_fingerprint()?,
        &command,
        limits,
        digest(&archive),
        "rust-mutation-test-profile-v1",
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
        execution_fingerprint: digest(&identity_fingerprint)
            .parse()
            .map_err(|_| ExecutionError::Infrastructure)?,
        platform: "linux/aarch64",
        image_id: gateway.image_id().into(),
    };
    let (bundle, bundle_entries) = match bundle
        .into_inner()
        .map_err(|_| ExecutionError::Infrastructure)?
    {
        Some((bytes, entries)) => (Some(bytes), entries),
        None => (None, 0),
    };
    Ok(MutationTestExecution {
        result,
        listed: listed
            .into_inner()
            .map_err(|_| ExecutionError::Infrastructure)?,
        cap_exceeded: capped
            .into_inner()
            .map_err(|_| ExecutionError::Infrastructure)?,
        outcomes: outcomes
            .into_inner()
            .map_err(|_| ExecutionError::Infrastructure)?,
        bundle_unavailable: bundle.is_none()
            && bundle_seen
                .into_inner()
                .map_err(|_| ExecutionError::Infrastructure)?,
        bundle,
        bundle_entries,
        identity: identity
            .into_inner()
            .map_err(|_| ExecutionError::Infrastructure)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_engineering_domain::mutation_test::MutationTestSelection;

    fn options(
        selection: MutationTestSelection,
    ) -> Result<MutationTestCommandOptions, ExecutionError> {
        MutationTestCommandOptions::try_from(selection).map_err(|_| ExecutionError::Infrastructure)
    }

    #[test]
    fn the_run_phase_argv_is_closed_and_carries_the_mandatory_baseline()
    -> Result<(), ExecutionError> {
        let phase = Phase::Run(RustCommand::MutationTest(options(
            MutationTestSelection::default(),
        )?));
        assert_eq!(phase.program(), "/opt/rust/bin/cargo");
        assert_eq!(
            phase.arguments(),
            [
                "mutants",
                "--no-config",
                "--dir",
                "/source",
                "--output",
                "/mutants",
                "--baseline",
                "run",
                "--no-shuffle",
                "--jobs",
                "1",
                "--jobserver",
                "false",
                "--copy-target",
                "false",
                "--copy-vcs",
                "false",
                "--cargo-arg=--offline",
                "--cargo-arg=--frozen",
                "--colors",
                "never",
                "--no-times",
                "--level",
                "info",
                "--timeout",
                "60",
                "--minimum-test-timeout",
                "60",
                "--build-timeout",
                "300",
            ]
        );
        let argv = phase.arguments();
        for forbidden in [
            "--shard",
            "--in-place",
            "--baseline=skip",
            "--shuffle",
            "--iterate",
            "--leak-dirs",
            "--in-diff",
        ] {
            assert!(
                !argv.iter().any(|arg| arg.contains(forbidden)),
                "{forbidden}"
            );
        }
        assert_eq!(phase.seccomp_profile_name(), "seccomp-rust-quality.json");
        assert!(!phase.ingesting());
        assert_eq!(phase.user(), "65534:65534");
        Ok(())
    }

    #[test]
    fn selection_and_budgets_reach_argv_in_a_fixed_order() -> Result<(), ExecutionError> {
        let selected = options(MutationTestSelection {
            package: Some("app".into()),
            features: vec!["extra".into()],
            no_default_features: true,
            target: Some("aarch64-unknown-linux-gnu".into()),
            max_mutants: 5,
            mutant_timeout_seconds: 20,
            ..Default::default()
        })?;
        let run = Phase::Run(RustCommand::MutationTest(selected.clone()));
        let argv = run.arguments();
        let tail = &argv[argv.len() - 10..];
        assert_eq!(
            tail,
            [
                "--timeout",
                "20",
                "--minimum-test-timeout",
                "20",
                "--build-timeout",
                "100",
                "--package=app",
                "--features=extra",
                "--no-default-features",
                "--cargo-arg=--target=aarch64-unknown-linux-gnu",
            ]
        );
        // The cap never appears in argv: cargo-mutants has no such flag, so it
        // is enforced by refusing an oversized listing instead.
        assert!(!argv.iter().any(|arg| arg.contains("max-mutants")));
        assert!(!argv.iter().any(|arg| arg == "5"));
        let listing = Phase::ListMutants(selected);
        assert_eq!(listing.program(), "/opt/rust/bin/cargo");
        assert_eq!(
            listing.arguments(),
            [
                "mutants",
                "--no-config",
                "--dir",
                "/source",
                "--list",
                "--json",
                "--colors",
                "never",
                "--no-times",
                "--package=app",
                "--features=extra",
                "--no-default-features",
                "--cargo-arg=--target=aarch64-unknown-linux-gnu",
            ]
        );
        let all_features = Phase::ListMutants(options(MutationTestSelection {
            all_features: true,
            ..Default::default()
        })?);
        assert!(all_features.arguments().contains(&"--all-features".into()));
        Ok(())
    }

    #[test]
    fn the_private_copy_never_leaves_and_the_source_is_never_writable() -> Result<(), ExecutionError>
    {
        let run = Phase::Run(RustCommand::MutationTest(options(
            MutationTestSelection::default(),
        )?));
        let listing = Phase::ListMutants(options(MutationTestSelection::default())?);
        for phase in [&run, &listing] {
            // Only the ingest phases mount `/source` read-write; a mutation
            // phase that became "ingesting" would be a containment regression.
            assert!(!phase.ingesting());
            assert_eq!(
                phase.extra_tmpfs(),
                Some((
                    "/mutants-scratch",
                    "rw,exec,nosuid,nodev,size=256m,mode=0700,uid=65534,gid=65534",
                ))
            );
            assert!(
                phase
                    .environment()
                    .contains(&"TMPDIR=/mutants-scratch".to_owned())
            );
            assert!(!phase.environment().contains(&"TMPDIR=/tmp".to_owned()));
        }
        // No exporter can reach the scratch mount: all three read `/mutants`.
        for phase in [
            Phase::ExportMutationOutcomes,
            Phase::ExportMutationBundle,
            Phase::ExportMutationLock,
        ] {
            assert_eq!(phase.program(), "/usr/bin/tar");
            assert!(phase.extra_tmpfs().is_none());
            let argv = phase.arguments();
            assert!(
                argv.contains(&"--directory=/mutants/mutants.out".to_owned()),
                "{argv:?}"
            );
            assert!(!argv.iter().any(|arg| arg.contains("/mutants-scratch")));
            assert!(!argv.iter().any(|arg| arg.contains("/source")));
            assert!(argv.contains(&"--create".to_owned()));
            assert!(!argv.contains(&"--extract".to_owned()));
        }
        assert!(
            Phase::ExportMutationBundle
                .arguments()
                .contains(&"--exclude=./lock.json".to_owned())
        );
        assert_eq!(Phase::GuardMutationOutput.program(), "/usr/bin/sleep");
        assert_eq!(Phase::GuardMutationOutput.arguments(), ["900"]);
        Ok(())
    }

    #[test]
    fn export_phases_have_their_own_bounds() -> Result<(), ExecutionError> {
        let limits = ExecutionLimits::new_job(60_000, 256 * 1024)
            .ok_or(ExecutionError::InvalidConfiguration)?;
        assert_eq!(
            output_limit(&Phase::ExportMutationOutcomes, limits),
            MAX_OUTCOMES_EXPORT
        );
        assert_eq!(
            output_limit(&Phase::ExportMutationBundle, limits),
            MAX_BUNDLE_EXPORT
        );
        assert_eq!(
            output_limit(&Phase::ExportMutationLock, limits),
            MAX_LOCK_EXPORT
        );
        assert_eq!(
            output_limit(
                &Phase::ListMutants(options(MutationTestSelection::default())?),
                limits
            ),
            MAX_LIST_EXPORT
        );
        assert_eq!(
            output_limit(
                &Phase::Run(RustCommand::MutationTest(options(
                    MutationTestSelection::default()
                )?)),
                limits
            ),
            limits.output_bytes()
        );
        Ok(())
    }

    #[test]
    fn the_version_probe_is_argument_free() {
        let phase = Phase::Run(RustCommand::MutantsVersion);
        assert_eq!(phase.program(), "/opt/rust/bin/cargo");
        assert_eq!(phase.arguments(), ["mutants", "--version"]);
        assert!(phase.extra_tmpfs().is_none());
    }
}
