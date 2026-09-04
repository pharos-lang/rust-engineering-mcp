use rust_engineering_application::ExecutionError;
use rust_engineering_domain::{
    ExecutionFingerprint, ExecutionLimits, SandboxCapabilities as C, SandboxEvidence,
    SandboxTier as T,
};

fn all() -> C {
    C {
        filesystem_isolated: true,
        network_isolated: true,
        environment_isolated: true,
        children_contained: true,
        wall_time_limited: true,
        output_limited: true,
        cpu_quota: true,
        memory_limited: true,
        pids_limited: true,
        disk_limited: true,
    }
}
#[test]
fn project_code_needs_opt_in_and_strict_without_downgrading() {
    for tier in [T::None, T::Restricted, T::Strict] {
        assert_eq!(
            admit_execution(tier, true, false, all()),
            Err(ExecutionError::Denied)
        );
        assert_eq!(
            admit_execution(tier, true, true, C::default()),
            Err(ExecutionError::Denied)
        );
    }
    assert!(admit_execution(T::Strict, true, true, all()).is_ok());
    assert_eq!(
        admit_execution(T::Restricted, true, true, all()),
        Err(ExecutionError::Denied)
    );
    assert_eq!(
        admit_execution(T::None, false, false, all()),
        Err(ExecutionError::Denied)
    );
}
#[test]
fn strict_rejects_each_missing_guarantee_and_restricted_has_explicit_subset()
-> Result<(), Box<dyn std::error::Error>> {
    let full = serde_json::to_value(all())?;
    for key in full.as_object().ok_or("object expected")?.keys() {
        let mut missing = full.clone();
        missing[key] = false.into();
        let caps: C = serde_json::from_value(missing)?;
        assert_eq!(
            admit_execution(T::Strict, false, false, caps),
            Err(ExecutionError::Denied),
            "{key}"
        );
        let optional = matches!(
            key.as_str(),
            "cpu_quota" | "memory_limited" | "disk_limited"
        );
        assert_eq!(
            admit_execution(T::Restricted, false, false, caps).is_ok(),
            optional,
            "{key}"
        );
    }
    Ok(())
}
#[test]
fn budgets_reject_zero_and_excess_without_clamping() {
    for (wall, bytes) in [
        (0, 1024),
        (99, 1024),
        (60001, 1024),
        (100, 1023),
        (100, 1048577),
    ] {
        assert!(ExecutionLimits::new(wall, bytes).is_none());
    }
    assert!(ExecutionLimits::new(100, 1024).is_some());
    assert!(ExecutionLimits::new(60000, 1048576).is_some());
}

fn admit_execution(tier: T, project: bool, opt_in: bool, caps: C) -> Result<(), ExecutionError> {
    let fingerprint: ExecutionFingerprint = format!("sha256:{}", "a".repeat(64))
        .parse()
        .map_err(|_| ExecutionError::InvalidConfiguration)?;
    let evidence = SandboxEvidence {
        configuration_fingerprint: fingerprint.clone(),
        capabilities: caps,
    };
    rust_engineering_application::admit_execution(tier, project, opt_in, &evidence, &fingerprint)
}
#[test]
fn capabilities_from_another_configuration_never_authorize_execution()
-> Result<(), Box<dyn std::error::Error>> {
    let a: ExecutionFingerprint = format!("sha256:{}", "a".repeat(64)).parse()?;
    let b: ExecutionFingerprint = format!("sha256:{}", "b".repeat(64)).parse()?;
    let evidence = SandboxEvidence {
        configuration_fingerprint: a,
        capabilities: all(),
    };
    assert_eq!(
        rust_engineering_application::admit_execution(T::Strict, true, true, &evidence, &b),
        Err(ExecutionError::Denied)
    );
    assert!(!C::default().satisfies(T::None));
    Ok(())
}
