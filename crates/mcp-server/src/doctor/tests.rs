use super::*;
use rust_engineering_application::{ExecutionCancellation, OperationControl};
struct Control;
impl OperationControl for Control {
    fn check(&self) -> Result<(), ProjectError> {
        Ok(())
    }
}
impl ExecutionCancellation for Control {
    fn is_cancelled(&self) -> bool {
        false
    }
}
fn args(values: &[&str]) -> Option<Invocation> {
    parse(values.iter().map(OsString::from))
}
#[test]
fn parser_is_closed_and_keeps_flag_like_host_values() -> Result<(), &'static str> {
    assert!(args(&["--json", "--json"]).is_none());
    assert!(args(&["--active", "--active"]).is_none());
    assert!(args(&["--unknown", "value"]).is_none());
    assert!(args(&["--catalog-store", "/tmp/store"]).is_none());
    assert!(args(&["--active", "--root"]).is_none());
    let parsed = args(&["--root", "--json", "--active"]).ok_or("host value consumed")?;
    assert!(!parsed.json);
    assert!(parsed.active);
    assert_eq!(parsed.host.roots[0], std::path::PathBuf::from("--json"));
    Ok(())
}
#[test]
fn passive_without_configuration_has_bounded_warning_report()
-> Result<(), Box<dyn std::error::Error>> {
    let invocation = args(&["--json"]).ok_or("parse")?;
    let report = inspect(&invocation, &Control).map_err(|e| format!("{e:?}"))?;
    assert_eq!(report.status, Status::Warning);
    assert_eq!(report.exit_code(), 0);
    assert!(report.runtime.is_none());
    assert!(
        report
            .checks
            .iter()
            .any(|c| c.id == Id::CargoAudit && c.status == CheckStatus::NotUsed)
    );
    assert!(
        !report
            .checks
            .iter()
            .any(|c| c.scope == Scope::ApprovedRuntime && c.status == CheckStatus::Available)
    );
    assert!(serde_json::to_vec(&report)?.len() < 16 * 1024);
    assert!(report.human().len() < 16 * 1024);
    Ok(())
}
#[test]
fn passive_runtime_configuration_does_not_touch_executable_or_state()
-> Result<(), Box<dyn std::error::Error>> {
    let mut invocation = args(&[]).ok_or("parse")?;
    invocation.host.rust = Some(rust_engineering_execution::HostDockerConfig {
        executable: "/nonexistent/doctor-executable".into(),
        socket: "/nonexistent/doctor-socket".into(),
        state_root: "/nonexistent/doctor-state".into(),
        image_id: rust_engineering_execution::APPROVED_RUST_IMAGE.into(),
    });
    let report = inspect(&invocation, &Control).map_err(|e| format!("{e:?}"))?;
    assert_eq!(report.exit_code(), 0);
    assert!(
        report
            .checks
            .iter()
            .filter(
                |c| [Id::Rustc, Id::Cargo, Id::Rustfmt, Id::Clippy, Id::Sandbox].contains(&c.id)
            )
            .all(|c| c.status == CheckStatus::NotChecked)
    );
    assert!(!serde_json::to_string(&report)?.contains("doctor-executable"));
    Ok(())
}
#[test]
fn configured_failure_dominates_warning_and_success() {
    let mut report = Report::new(false);
    let component: Component<()> = Component::Unavailable {
        reason: CatalogComponentUnavailable::Denied,
    };
    report.component(Id::Catalog, Scope::CatalogSnapshot, &component, true);
    report.add(
        Id::HostTools,
        Scope::Host,
        CheckStatus::NotChecked,
        Reason::Unknown,
        Action::None,
        Status::Warning,
    );
    report.component(
        Id::Model,
        Scope::LocalModel,
        &Component::Available { value: () },
        true,
    );
    assert_eq!(report.status, Status::Failed);
    assert_eq!(report.exit_code(), 1);
}
#[test]
fn cancellation_never_becomes_success_and_failure_preserves_checks() -> Result<(), &'static str> {
    struct Cancelled;
    impl OperationControl for Cancelled {
        fn check(&self) -> Result<(), ProjectError> {
            Err(ProjectError::Cancelled)
        }
    }
    impl ExecutionCancellation for Cancelled {
        fn is_cancelled(&self) -> bool {
            true
        }
    }
    let invocation = args(&[]).ok_or("parse")?;
    assert!(matches!(
        inspect(&invocation, &Cancelled),
        Err(ProjectError::Cancelled)
    ));
    let mut report = Report::failure(false, ProjectError::Internal);
    report.record_failure(ProjectError::Cancelled);
    assert_eq!(report.checks.len(), 2);
    assert_eq!(report.exit_code(), 1);
    assert_eq!(report.checks[1].reason, Reason::Interrupted);
    Ok(())
}
#[test]
fn stale_and_unknown_snapshot_freshness_are_warnings_not_failures()
-> Result<(), Box<dyn std::error::Error>> {
    struct At;
    impl Clock for At {
        fn now(&self) -> UnixSeconds {
            UnixSeconds(1_000_000)
        }
    }
    for timestamp in [Some(UnixSeconds(1)), None] {
        let provenance = Provenance::new(
            SourceKind::RegistrySnapshot,
            "doctor-fixture".parse()?,
            timestamp,
            timestamp,
            IntegrityStatus::Verified,
            false,
        )?;
        let evidence = SnapshotEvidence::assess(
            provenance,
            FreshnessPolicy::new("doctor-fixture".parse()?, 60, 120)?,
            &At,
        );
        let mut report = Report::new(false);
        report.freshness(Id::CatalogFreshness, &evidence);
        assert_eq!(report.status, Status::Warning);
        assert_eq!(report.exit_code(), 0);
        assert_eq!(report.checks[0].action, Action::RefreshSnapshotExplicitly);
    }
    Ok(())
}
#[test]
fn optional_embedded_index_without_model_or_feature_warns_but_explicit_configuration_fails() {
    for reason in [
        CatalogComponentUnavailable::DependencyUnavailable,
        CatalogComponentUnavailable::FeatureDisabled,
    ] {
        let component: Component<()> = Component::Unavailable { reason };
        assert!(!index_source_observed(&component));
        let mut optional = Report::new(false);
        optional.component(Id::SemanticIndex, Scope::CatalogSnapshot, &component, false);
        assert_eq!(optional.exit_code(), 0);
        assert_eq!(optional.status, Status::Warning);
        let mut configured = Report::new(false);
        configured.component(Id::SemanticIndex, Scope::CatalogSnapshot, &component, true);
        assert_eq!(configured.exit_code(), 1);
    }
    let corrupt: Component<()> = Component::Unavailable {
        reason: CatalogComponentUnavailable::Invalid,
    };
    assert!(index_source_observed(&corrupt));
}

fn record(kind: MutationKind, state: MutationState) -> Result<MutationRecordSummary, &'static str> {
    Ok(MutationRecordSummary {
        id: MutationId::new("mut_00000000000000000000000000000000".into()).map_err(|_| "id")?,
        digest: format!("sha256:{}", "a".repeat(64))
            .parse()
            .map_err(|_| "digest")?,
        state,
        stored_bytes: 0,
        kind,
    })
}
#[test]
fn classify_mutation_records_reports_empty_as_unblocked_with_no_notes() {
    let report = classify_mutation_records(Ok(vec![]));
    assert_eq!(report.pending, 0);
    assert_eq!(report.terminal, 0);
    assert_eq!(report.unknown_format, 0);
    assert!(report.kinds.is_empty());
    assert!(!report.downgrade_blocked);
    assert!(report.downgrade_blocking_kinds.is_empty());
    assert!(report.notes.is_empty());
}
#[test]
fn classify_mutation_records_counts_pending_and_terminal_per_known_kind() -> Result<(), &'static str>
{
    let report = classify_mutation_records(Ok(vec![
        record(MutationKind::ManifestPatch, MutationState::RecoveryRequired)?,
        record(MutationKind::ManifestPatch, MutationState::Committed)?,
        record(MutationKind::FormatApply, MutationState::NoChange)?,
        record(MutationKind::FixApply, MutationState::Aborted)?,
    ]));
    assert_eq!(report.pending, 1);
    assert_eq!(report.terminal, 3);
    assert_eq!(report.unknown_format, 0);
    assert_eq!(report.kinds["manifest_patch"].pending, 1);
    assert_eq!(report.kinds["manifest_patch"].terminal, 1);
    assert_eq!(report.kinds["format_apply"].terminal, 1);
    assert_eq!(report.kinds["fix_apply"].terminal, 1);
    // Pending on a kind known to the oldest supported binary still blocks a downgrade.
    assert!(report.downgrade_blocked);
    assert!(report.downgrade_blocking_kinds.is_empty());
    assert_eq!(report.notes, vec![DOWNGRADE_NOTE]);
    Ok(())
}
#[test]
fn classify_mutation_records_flags_kinds_unknown_to_0_3_0_even_when_terminal()
-> Result<(), &'static str> {
    let report = classify_mutation_records(Ok(vec![
        record(MutationKind::AnalyzerActionApply, MutationState::Committed)?,
        record(MutationKind::DependencyAdd, MutationState::Committed)?,
        record(MutationKind::DependencyRemove, MutationState::NoChange)?,
    ]));
    assert_eq!(report.pending, 0);
    assert_eq!(report.terminal, 3);
    assert!(report.downgrade_blocked);
    assert_eq!(
        report.downgrade_blocking_kinds,
        vec!["analyzer_action_apply"]
    );
    assert_eq!(report.notes, vec![DOWNGRADE_NOTE]);
    Ok(())
}
#[test]
fn classify_mutation_records_reports_busy_without_touching_counts() {
    let report = classify_mutation_records(Err(MutationError::Busy));
    assert_eq!(report.pending, 0);
    assert_eq!(report.terminal, 0);
    assert_eq!(report.unknown_format, 0);
    assert!(report.kinds.is_empty());
    assert!(report.downgrade_blocked);
    assert!(report.downgrade_blocking_kinds.is_empty());
    assert_eq!(
        report.notes,
        vec!["journal busy: a mutation is in progress; rerun doctor"]
    );
}
#[test]
fn classify_mutation_records_reports_recovery_required_as_unknown_format_with_two_notes() {
    let report = classify_mutation_records(Err(MutationError::RecoveryRequired));
    assert_eq!(report.pending, 0);
    assert_eq!(report.terminal, 0);
    assert_eq!(report.unknown_format, 1);
    assert!(report.downgrade_blocked);
    assert_eq!(report.notes.len(), 2);
    assert_eq!(report.notes[1], DOWNGRADE_NOTE);
}
#[test]
fn classify_mutation_records_falls_back_to_a_generic_blocked_note_for_other_errors() {
    for error in [
        MutationError::Io,
        MutationError::UnsupportedPlatform,
        MutationError::PermissionDenied,
    ] {
        let report = classify_mutation_records(Err(error));
        assert_eq!(report.pending, 0);
        assert_eq!(report.terminal, 0);
        assert_eq!(report.unknown_format, 0);
        assert!(report.downgrade_blocked);
        assert_eq!(
            report.notes,
            vec![
                "The mutation journal store could not be read; treated as blocked until the condition is resolved."
            ]
        );
    }
}
#[test]
fn mutation_journals_is_empty_when_the_journal_directory_does_not_exist()
-> Result<(), Box<dyn std::error::Error>> {
    let state_root = std::env::temp_dir().join(format!(
        "rust-mcp-doctor-unit-{}-{:?}",
        std::process::id(),
        std::time::SystemTime::now()
    ));
    std::fs::create_dir_all(&state_root)?;
    let report = mutation_journals(&state_root);
    std::fs::remove_dir_all(&state_root)?;
    assert_eq!(report.pending, 0);
    assert_eq!(report.terminal, 0);
    assert!(!report.downgrade_blocked);
    Ok(())
}
#[test]
fn human_report_renders_the_mutation_journals_summary_line_and_null_is_omitted()
-> Result<(), &'static str> {
    let mut report = Report::new(false);
    assert!(report.mutation_journals.is_none());
    assert!(!report.human().contains("mutation_journals"));
    report.mutation_journals = Some(classify_mutation_records(Ok(vec![record(
        MutationKind::ManifestPatch,
        MutationState::RecoveryRequired,
    )?])));
    let human = report.human();
    assert!(human.contains(
        "mutation_journals: pending=1 terminal=0 unknown_format=0 downgrade_blocked=true"
    ));
    Ok(())
}
#[test]
fn failed_worker_never_attests_active_cleanup() {
    for active in [false, true] {
        let report = Report::worker_failure(active);
        assert_eq!(report.exit_code(), 1);
        assert_eq!(
            report.checks[0].reason,
            if active {
                Reason::CleanupUncertain
            } else {
                Reason::Internal
            }
        );
    }
}
