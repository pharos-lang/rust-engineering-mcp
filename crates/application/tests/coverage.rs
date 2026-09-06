use rust_engineering_application::coverage::{
    CoverageArtifactStreams, CoverageIdentity, CoverageObservation,
};
use rust_engineering_domain::{
    ExecutionFingerprint, ExecutionTermination, RuntimeIdentity,
    coverage::{CoverageMetrics, CoverageOptions, CoverageSelection, CoverageSummary},
};

#[test]
fn observation_rejects_doctests_and_requires_cfg_coverage() -> Result<(), Box<dyn std::error::Error>>
{
    let options = CoverageOptions::try_from(CoverageSelection::default())?;
    let fingerprint: ExecutionFingerprint =
        "sha256:0000000000000000000000000000000000000000000000000000000000000000".parse()?;
    let mut observation = CoverageObservation {
        options,
        summary: CoverageSummary {
            aggregate: CoverageMetrics::default(),
            packages: vec![],
            files: vec![],
            files_omitted: 0,
        },
        identity: CoverageIdentity {
            cargo_llvm_cov_version: "0.9.0".into(),
            manifest_path: "/source/Cargo.toml".into(),
            llvm_tools_version: "1.98.1".into(),
        },
        doctests_run: true,
        cfg_coverage_enabled: true,
        target: "aarch64-unknown-linux-gnu",
        termination: ExecutionTermination::Exited,
        exit_code: Some(0),
        parse_complete: true,
        runtime: RuntimeIdentity {
            platform: "linux/aarch64".into(),
            image_id: "sha256:0000000000000000000000000000000000000000000000000000000000000000"
                .into(),
            configuration_fingerprint: fingerprint.clone(),
            execution_fingerprint: fingerprint.clone(),
            rust_version: "1.98.1".into(),
            cargo_version: "1.98.1".into(),
            declared_toolchain: None,
        },
        execution_fingerprint: fingerprint,
        artifacts: CoverageArtifactStreams::default(),
    };
    assert!(observation.validate().is_err());
    observation.doctests_run = false;
    assert!(observation.validate().is_ok());
    Ok(())
}
