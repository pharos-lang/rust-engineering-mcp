use rust_engineering_domain::job::{
    ExecutionMode, JobBudget, JobDeadline, JobId, JobOwnerBinding, JobPhase, JobState,
    Milliseconds, ResultRetention, RetentionQuotas, TASK_RECORD_TTL_MS,
};

#[test]
fn job_id_is_canonical_and_entropy_is_not_embedded_as_authority() -> Result<(), serde_json::Error> {
    let id = JobId::from_random_bytes([0xab; 16]);
    assert_eq!(id.as_str(), "job_abababababababababababababababab");
    assert_eq!(id.to_string().parse::<JobId>(), Ok(id.clone()));
    assert_eq!(
        serde_json::to_string(&id)?,
        "\"job_abababababababababababababababab\""
    );

    for value in [
        "",
        "job_0",
        "job_ABABABABABABABABABABABABABABABAB",
        "job_ababababababababababababababababa",
        "prj_abababababababababababababababab",
    ] {
        assert!(value.parse::<JobId>().is_err(), "accepted {value:?}");
        assert!(serde_json::from_str::<JobId>(&format!("\"{value}\"")).is_err());
    }
    Ok(())
}

#[test]
fn budgets_deadlines_and_retention_keep_explicit_units_and_closed_bounds()
-> Result<(), Box<dyn std::error::Error>> {
    let budget = JobBudget::asynchronous_default()?;
    assert_eq!(budget.work(), Milliseconds(300_000));
    assert_eq!(budget.execute(), Milliseconds(180_000));
    assert_eq!(budget.cleanup(), Milliseconds(60_000));
    let extended = JobBudget::asynchronous_for_work(Milliseconds(301_000))?;
    assert_eq!(extended.work(), Milliseconds(301_000));
    assert_eq!(extended.capture_prepare(), Milliseconds(60_000));
    assert_eq!(extended.execute(), Milliseconds(211_000));
    assert_eq!(extended.collect_publish(), Milliseconds(30_000));
    let maximum = JobBudget::asynchronous_for_work(Milliseconds(3_600_000))?;
    assert_eq!(maximum.capture_prepare(), Milliseconds(120_000));
    assert_eq!(maximum.execute(), Milliseconds(3_360_000));
    assert_eq!(maximum.collect_publish(), Milliseconds(120_000));
    assert!(JobBudget::asynchronous_for_work(Milliseconds(3_600_001)).is_err());
    assert!(
        JobBudget::new(
            Milliseconds(300_000),
            Milliseconds(60_000),
            Milliseconds(180_000),
            Milliseconds(30_000),
            Milliseconds(60_000),
        )
        .is_ok()
    );
    assert!(
        JobBudget::new(
            Milliseconds(299_999),
            Milliseconds(60_000),
            Milliseconds(180_000),
            Milliseconds(60_000),
            Milliseconds(60_000),
        )
        .is_err()
    );
    assert!(JobDeadline::after(Milliseconds(u64::MAX), Milliseconds(1)).is_err());

    let retention = ResultRetention::fixed();
    assert_eq!(retention.ttl(), Milliseconds(TASK_RECORD_TTL_MS));
    assert_eq!(RetentionQuotas::fixed().per_owner_entries(), 64);
    assert_eq!(RetentionQuotas::fixed().server_entries(), 256);
    assert_eq!(RetentionQuotas::fixed().per_owner_bytes(), 32 * 1024 * 1024);
    assert_eq!(RetentionQuotas::fixed().server_bytes(), 128 * 1024 * 1024);
    Ok(())
}

#[test]
fn execution_mode_and_lifecycle_are_closed() -> Result<(), serde_json::Error> {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Auto);
    for (wire, mode) in [
        ("auto", ExecutionMode::Auto),
        ("task", ExecutionMode::Task),
        ("synchronous", ExecutionMode::Synchronous),
    ] {
        assert_eq!(
            serde_json::from_str::<ExecutionMode>(&format!("\"{wire}\""))?,
            mode
        );
    }
    assert!(serde_json::from_str::<ExecutionMode>("\"background\"").is_err());
    assert!(JobState::Admitted.can_transition_to(JobState::Running));
    assert!(JobState::Running.can_transition_to(JobState::Completed));
    assert!(JobState::Running.can_transition_to(JobState::Cancelled));
    assert!(JobState::Cancelled.is_terminal());
    assert_eq!(JobPhase::Cleanup.status_message(), "cleaning up");
    assert_eq!(JobPhase::Terminal.status_message(), "finished");
    Ok(())
}

#[test]
fn owner_binding_is_domain_separated_and_covers_every_physical_fact() {
    let base = JobOwnerBinding::derive((1, 2), 501, (3, 4), "/workspace");
    assert_eq!(
        base,
        JobOwnerBinding::derive((1, 2), 501, (3, 4), "/workspace")
    );
    assert_eq!(
        base.digest(),
        &[
            0x2b, 0x25, 0xab, 0x77, 0x2a, 0x86, 0x2f, 0x9a, 0x6c, 0x4d, 0x9b, 0x2c, 0xf8, 0xdf,
            0xdf, 0xe6, 0x3b, 0x14, 0xad, 0xae, 0x0a, 0x18, 0xff, 0x23, 0x00, 0x1a, 0x3e, 0xee,
            0xe1, 0xe8, 0x0c, 0x07,
        ]
    );
    for changed in [
        JobOwnerBinding::derive((9, 2), 501, (3, 4), "/workspace"),
        JobOwnerBinding::derive((1, 9), 501, (3, 4), "/workspace"),
        JobOwnerBinding::derive((1, 2), 502, (3, 4), "/workspace"),
        JobOwnerBinding::derive((1, 2), 501, (9, 4), "/workspace"),
        JobOwnerBinding::derive((1, 2), 501, (3, 9), "/workspace"),
        JobOwnerBinding::derive((1, 2), 501, (3, 4), "/workspace/other"),
    ] {
        assert_ne!(base, changed);
    }
}
