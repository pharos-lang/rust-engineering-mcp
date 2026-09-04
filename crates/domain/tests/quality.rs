use rust_engineering_domain::*;

type TestResult = Result<(), Box<dyn std::error::Error>>;
struct TestClock(u64);
impl Clock for TestClock {
    fn now(&self) -> UnixSeconds {
        UnixSeconds(self.0)
    }
}
fn execution() -> Result<CheckObservation, Box<dyn std::error::Error>> {
    let fp = format!("sha256:{:064x}", 42);
    Ok(CheckObservation {
        outcome: CheckOutcome::Passed,
        termination: ExecutionTermination::Exited,
        exit_code: Some(0),
        validation_complete: true,
        diagnostics: vec![],
        diagnostics_omitted: 0,
        stdout: String::new(),
        stderr: String::new(),
        stdout_truncated: false,
        stderr_truncated: false,
        runtime: RuntimeIdentity {
            platform: "linux/aarch64".into(),
            image_id: fp.clone(),
            configuration_fingerprint: fp.parse()?,
            execution_fingerprint: fp.parse()?,
            rust_version: "1.98.1".into(),
            cargo_version: "1.98.1".into(),
            declared_toolchain: None,
        },
        source_fingerprint: fp.parse()?,
    })
}
fn report(stage: QualityStage, observation: Option<QualityObservation>) -> QualityStageReport {
    QualityStageReport {
        stage,
        duration_ms: 0,
        status: ToolStatus::Passed,
        issue: None,
        observation,
        log: None,
        retention_remaining_seconds: None,
    }
}
fn audit(
    integrity: IntegrityStatus,
    created: Option<UnixSeconds>,
    now: u64,
) -> Result<AuditObservation, Box<dyn std::error::Error>> {
    let snapshot = SnapshotEvidence::assess(
        Provenance::new(
            SourceKind::RustsecSnapshot,
            "fixture".parse()?,
            created,
            Some(UnixSeconds(1)),
            integrity,
            false,
        )?,
        FreshnessPolicy::new("fixture-v1".parse()?, 60, 300)?,
        &TestClock(now),
    );
    let fp = format!("sha256:{:064x}", 42);
    Ok(AuditObservation {
        state: AuditState::Passed,
        issue: None,
        validation_complete: true,
        lock_fingerprint: Some(fp.parse()?),
        snapshot_fingerprint: Some(fp.parse()?),
        snapshot: Some(snapshot),
        snapshot_record_count: Some(1),
        snapshot_sequence: Some(1),
        packages_total: 1,
        crates_io_scanned: 1,
        workspace_packages_excluded: 0,
        unsupported_packages: vec![],
        findings: vec![],
        informational: vec![],
        findings_omitted: 0,
    })
}

#[test]
fn required_stage_order_and_precedence_cannot_hide_incomplete_results() {
    let mut rows: Vec<_> = QualityProfile::Fast
        .stages()
        .iter()
        .map(|stage| report(*stage, None))
        .collect();
    assert_eq!(
        quality_status(QualityProfile::Fast, &rows),
        ToolStatus::Passed
    );
    rows[0].status = ToolStatus::Failed;
    assert_eq!(
        quality_status(QualityProfile::Fast, &rows),
        ToolStatus::Failed
    );
    rows[1].status = ToolStatus::Unavailable;
    assert_eq!(
        quality_status(QualityProfile::Fast, &rows),
        ToolStatus::Unavailable
    );
    rows[2].status = ToolStatus::Blocked;
    assert_eq!(
        quality_status(QualityProfile::Fast, &rows),
        ToolStatus::Blocked
    );
    rows[0].status = ToolStatus::Cancelled;
    assert_eq!(
        quality_status(QualityProfile::Fast, &rows),
        ToolStatus::Cancelled
    );
    rows.swap(0, 1);
    assert_eq!(
        quality_status(QualityProfile::Fast, &rows),
        ToolStatus::Blocked
    );
    assert_eq!(
        quality_status(QualityProfile::Standard, &rows),
        ToolStatus::Blocked
    );
    assert_eq!(
        quality_status(QualityProfile::Fast, &[]),
        ToolStatus::Blocked
    );
    let mut absent = report(QualityStage::Check, None);
    absent.classify();
    assert_eq!(absent.status, ToolStatus::Blocked);
}

#[test]
fn passed_label_requires_complete_and_consistent_execution_facts() -> TestResult {
    for case in 0..8 {
        let mut observed = execution()?;
        match case {
            1 => observed.validation_complete = false,
            2 => observed.diagnostics_omitted = 1,
            3 => observed.exit_code = Some(101),
            4 => observed.exit_code = None,
            5 => observed.termination = ExecutionTermination::TimedOut,
            6 => {
                observed.outcome = CheckOutcome::Failed;
                observed.exit_code = Some(101);
            }
            7 => observed.termination = ExecutionTermination::Cancelled,
            _ => {}
        }
        let mut row = report(
            QualityStage::Check,
            Some(QualityObservation::Check(observed)),
        );
        row.classify();
        assert_eq!(
            row.status,
            match case {
                0 => ToolStatus::Passed,
                6 => ToolStatus::Failed,
                7 => ToolStatus::Cancelled,
                _ => ToolStatus::Blocked,
            },
            "case {case}"
        );
    }
    let mut tests = report(
        QualityStage::Test,
        Some(QualityObservation::Test(TestObservation {
            execution: execution()?,
            build_succeeded: None,
        })),
    );
    tests.classify();
    assert_eq!(tests.status, ToolStatus::Blocked);
    let mut format = report(
        QualityStage::Format,
        Some(QualityObservation::Format(FormatObservation {
            execution: execution()?,
            affected_files: vec!["src/lib.rs".into()],
            affected_files_omitted: 0,
            diff: None,
            diff_omitted: false,
        })),
    );
    format.classify();
    assert_eq!(format.status, ToolStatus::Blocked);
    Ok(())
}

#[test]
fn audit_stale_unknown_unverified_or_omitted_evidence_never_passes() -> TestResult {
    for case in 0..8 {
        let mut observation = audit(
            if case == 3 {
                IntegrityStatus::Unverified
            } else {
                IntegrityStatus::Verified
            },
            if case == 2 {
                None
            } else {
                Some(UnixSeconds(0))
            },
            if case == 1 { 301 } else { 1 },
        )?;
        match case {
            4 => observation.findings_omitted = 1,
            5 => observation.crates_io_scanned = 0,
            6 => observation.snapshot_record_count = None,
            7 => observation.unsupported_packages.push(AuditPackage {
                name: "git-dependency".into(),
                version: "1.0.0".into(),
                source: AuditSource::Unverified,
                source_fingerprint: None,
            }),
            _ => {}
        }
        let mut row = report(
            QualityStage::Audit,
            Some(QualityObservation::Audit {
                runtime: execution()?.runtime,
                observation,
            }),
        );
        row.classify();
        assert_eq!(
            row.status,
            match case {
                0 => ToolStatus::Passed,
                1 | 2 => ToolStatus::Unavailable,
                _ => ToolStatus::Blocked,
            },
            "case {case}"
        );
        if let Some(QualityObservation::Audit { observation, .. }) = row.observation {
            assert_eq!(observation.validation_complete, case == 0);
        }
    }
    Ok(())
}
