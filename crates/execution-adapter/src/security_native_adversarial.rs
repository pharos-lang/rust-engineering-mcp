//! Explicit adversarial M4 deny qualification; never part of normal discovery.
use crate::*;
use rust_engineering_application::security::SecurityError;
use rust_engineering_application::{InspectionError, ProjectError};
use rust_engineering_domain::security::SecurityPolicy;
use rust_engineering_domain::{
    CargoVendorPackage, CargoVendorSnapshot, SourceBundle, SourceFile, SourceFingerprint,
};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const M4_IMAGE: &str = crate::APPROVED_M4_IMAGE;
const VENDOR_FINGERPRINT: &str =
    "sha256:743947d5788c1a4385a4b59869c5b8bd0535f7fc0d875b51288f9b26b2d0eba1";
const UNICODE_IDENT_CHECKSUM: &str =
    "sha256:e6e4313cd5fcd3dad5cafa179702e2b244f760991f45397d14d4ebf38247da75";

#[derive(Serialize)]
struct CaseEvidence {
    case: &'static str,
    expected: &'static str,
    observed: String,
    passed: bool,
    stdout_sha256: Option<String>,
    stderr_sha256: Option<String>,
    stdout: Option<String>,
    stderr: Option<String>,
}

struct Cancelled;
impl ExecutionCancellation for Cancelled {
    fn is_cancelled(&self) -> bool {
        true
    }
}

struct CancelAfter {
    started: Instant,
    delay: Duration,
}
impl ExecutionCancellation for CancelAfter {
    fn is_cancelled(&self) -> bool {
        self.started.elapsed() >= self.delay
    }
}

fn source_file(path: &str, bytes: Vec<u8>) -> Result<SourceFile, Box<dyn std::error::Error>> {
    SourceFile::new(path.into(), bytes).map_err(|error| format!("{path}: {error:?}").into())
}

fn bundle_from(root: &Path) -> Result<SourceBundle, Box<dyn std::error::Error>> {
    fn visit(
        root: &Path,
        at: &Path,
        files: &mut Vec<SourceFile>,
        directories: &mut Vec<String>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut entries = std::fs::read_dir(at)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let path = entry.path();
            let relative = path
                .strip_prefix(root)?
                .to_str()
                .ok_or("non-UTF-8 fixture path")?;
            let kind = entry.file_type()?;
            if kind.is_dir() {
                directories.push(relative.into());
                visit(root, &path, files, directories)?;
            } else if kind.is_file() {
                files.push(source_file(relative, std::fs::read(&path)?)?);
            } else {
                return Err(format!("special fixture entry: {relative}").into());
            }
        }
        Ok(())
    }

    let mut files = Vec::new();
    let mut directories = Vec::new();
    visit(root, root, &mut files, &mut directories)?;
    SourceBundle::with_directories(files, directories)
        .map_err(|error| format!("bundle: {error:?}").into())
}

fn project() -> Result<SourceBundle, Box<dyn std::error::Error>> {
    bundle_from(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/m4-deny-adversarial"))
}

fn vendor() -> Result<CargoVendorSnapshot, Box<dyn std::error::Error>> {
    let source = bundle_from(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/cargo-vendor-data/vendor"),
    )?;
    let observed = resolution_gateway::tree_fingerprint(&source)
        .map_err(|error| format!("vendor fingerprint: {error:?}"))?;
    let expected: SourceFingerprint = VENDOR_FINGERPRINT.parse()?;
    assert_eq!(observed, expected, "the approved vendor snapshot changed");
    Ok(CargoVendorSnapshot {
        source,
        tree_fingerprint: expected,
        packages: vec![CargoVendorPackage {
            name: "unicode-ident".into(),
            version: "1.0.24".into(),
            package_checksum: UNICODE_IDENT_CHECKSUM.parse()?,
        }],
    })
}

fn policy() -> Result<SecurityPolicy, Box<dyn std::error::Error>> {
    let bytes = serde_json::to_vec(&serde_json::json!({
        "schema_version": 1,
        "rules": {
            "allowed_licenses": ["MIT", "Apache-2.0", "Unicode-3.0"],
            "banned_packages": [],
            "multiple_versions": "deny",
            "wildcards": "deny"
        },
        "suppressions": []
    }))?;
    security_policy::parse_security_policy(&bytes, &digest(&bytes).parse()?, 100)
        .map_err(|error| format!("policy: {error:?}").into())
}

fn gateway(state_root: PathBuf) -> Result<RustGateway, Box<dyn std::error::Error>> {
    let gateway = RustGateway::new_m4_for_qualification(HostDockerConfig {
        executable: "/Applications/Docker.app/Contents/Resources/bin/docker".into(),
        socket: "/Users/cburgosro/.docker/run/docker.sock".into(),
        state_root,
        image_id: M4_IMAGE.into(),
    })
    .map_err(|error| format!("gateway: {error:?}"))?;
    gateway.set_verified(true);
    Ok(gateway)
}

fn inventory(gateway: &RustGateway) -> Result<(String, String), Box<dyn std::error::Error>> {
    let containers = gateway
        .inner
        .control(&[
            "container".into(),
            "ls".into(),
            "--all".into(),
            "--filter=label=org.rust-mcp.execution=true".into(),
            "--format={{.ID}}".into(),
        ])
        .map_err(|error| format!("container inventory: {error:?}"))?;
    let volumes = gateway
        .inner
        .control(&[
            "volume".into(),
            "ls".into(),
            "--filter=label=org.rust-mcp.execution=true".into(),
            "--format={{.Name}}".into(),
        ])
        .map_err(|error| format!("volume inventory: {error:?}"))?;
    assert_eq!(containers.code, Some(0));
    assert_eq!(volumes.code, Some(0));
    let containers = String::from_utf8(containers.stdout)?;
    let volumes = String::from_utf8(volumes.stdout)?;
    assert!(
        containers.trim().is_empty(),
        "leftover containers: {containers}"
    );
    assert!(volumes.trim().is_empty(), "leftover volumes: {volumes}");
    Ok((containers, volumes))
}

fn capture_evidence(
    case: &'static str,
    result: &security_gateway::SecurityExecution,
) -> CaseEvidence {
    CaseEvidence {
        case,
        expected: "exit 0 with captured license texts for workspace and unicode-ident",
        observed: format!("exit {:?}", result.capture.code),
        passed: result.capture.code == Some(0),
        stdout_sha256: Some(digest(&result.capture.stdout)),
        stderr_sha256: Some(digest(&result.capture.stderr)),
        stdout: Some(String::from_utf8_lossy(&result.capture.stdout).into_owned()),
        stderr: Some(String::from_utf8_lossy(&result.capture.stderr).into_owned()),
    }
}

fn error_evidence(
    case: &'static str,
    expected: &'static str,
    error: SecurityError,
) -> CaseEvidence {
    CaseEvidence {
        case,
        expected,
        observed: format!("{error:?}"),
        passed: true,
        stdout_sha256: None,
        stderr_sha256: None,
        stdout: None,
        stderr: None,
    }
}

fn required_error(
    result: Result<security_gateway::SecurityExecution, SecurityError>,
    message: &'static str,
) -> Result<SecurityError, Box<dyn std::error::Error>> {
    match result {
        Err(error) => Ok(error),
        Ok(_) => Err(message.into()),
    }
}

#[test]
#[ignore = "explicit provisioned M4 image and local Docker; adversarial deny qualification"]
fn m4_deny_adversarial_oracles_preserve_cleanup_and_inputs()
-> Result<(), Box<dyn std::error::Error>> {
    let nonce = state::nonce().map_err(|error| format!("nonce: {error:?}"))?;
    let state_root = PathBuf::from("/private/tmp").join(format!("m4-deny-adversarial-{nonce}"));
    std::fs::create_dir(&state_root)?;
    let gateway = gateway(state_root.clone())?;
    let source = project()?;
    let vendor = vendor()?;
    let policy = policy()?;
    let source_before = source.clone();
    let vendor_before = vendor.clone();
    let mut cases = Vec::new();

    let clean = security_gateway::execute(
        &gateway,
        &source,
        &vendor,
        &policy,
        ExecutionLimits::new_job(120_000, 1024 * 1024).ok_or("clean limits")?,
        &NeverCancel,
    )
    .map_err(|error| format!("clean: {error:?}"))?;
    assert_eq!(clean.capture.code, Some(0));
    assert!(!clean.capture.stdout_truncated && !clean.capture.stderr_truncated);
    let unicode = clean
        .metadata
        .packages
        .iter()
        .position(|package| package.name == "unicode-ident" && package.version == "1.0.24")
        .ok_or("unicode-ident absent from metadata")?;
    assert_eq!(
        clean.metadata.declared_licenses[unicode].as_deref(),
        Some("(MIT OR Apache-2.0) AND Unicode-3.0")
    );
    assert!(
        clean.metadata.license_files[unicode]
            .iter()
            .any(|(path, _)| path.ends_with("/LICENSE-MIT"))
    );
    assert!(clean.metadata.license_files[unicode].len() >= 3);
    cases.push(capture_evidence("clean-vendored-license-text", &clean));
    inventory(&gateway)?;

    let mut corrupt_lock_files = source.files().to_vec();
    let lock = corrupt_lock_files
        .iter_mut()
        .find(|file| file.path() == "Cargo.lock")
        .ok_or("lock absent")?;
    *lock = source_file(
        "Cargo.lock",
        String::from_utf8(lock.bytes().to_vec())?
            .replace(
                "e6e4313cd5fcd3dad5cafa179702e2b244f760991f45397d14d4ebf38247da75",
                "a6e4313cd5fcd3dad5cafa179702e2b244f760991f45397d14d4ebf38247da75",
            )
            .into_bytes(),
    )?;
    let corrupt_lock = SourceBundle::new(corrupt_lock_files)
        .map_err(|error| format!("corrupt lock source: {error:?}"))?;
    let error = required_error(
        security_gateway::execute(
            &gateway,
            &corrupt_lock,
            &vendor,
            &policy,
            ExecutionLimits::new_job(120_000, 1024 * 1024).ok_or("checksum limits")?,
            &NeverCancel,
        ),
        "a corrupt Cargo.lock checksum passed",
    )?;
    assert_eq!(error, SecurityError::InvalidMetadata);
    cases.push(error_evidence(
        "corrupt-lock-checksum",
        "InvalidMetadata",
        error,
    ));
    inventory(&gateway)?;

    let empty = SourceBundle::new(vec![]).map_err(|error| format!("empty vendor: {error:?}"))?;
    let missing_vendor = CargoVendorSnapshot {
        tree_fingerprint: resolution_gateway::tree_fingerprint(&empty)
            .map_err(|error| format!("empty vendor fingerprint: {error:?}"))?,
        source: empty,
        packages: vendor.packages.clone(),
    };
    let error = required_error(
        security_gateway::execute(
            &gateway,
            &source,
            &missing_vendor,
            &policy,
            ExecutionLimits::new_job(120_000, 1024 * 1024).ok_or("offline limits")?,
            &NeverCancel,
        ),
        "missing offline bytes passed",
    )?;
    assert_eq!(error, SecurityError::MissingOfflineData);
    cases.push(error_evidence(
        "missing-offline-vendor-data",
        "MissingOfflineData",
        error,
    ));
    inventory(&gateway)?;

    let mut exception_files = source.files().to_vec();
    exception_files.push(source_file(
        "deny.exceptions.toml",
        b"[[exceptions]]\n".to_vec(),
    )?);
    let exception_source = SourceBundle::new(exception_files)
        .map_err(|error| format!("exception source: {error:?}"))?;
    let error = required_error(
        security_gateway::execute(
            &gateway,
            &exception_source,
            &vendor,
            &policy,
            ExecutionLimits::new_job(120_000, 1024 * 1024).ok_or("exception limits")?,
            &NeverCancel,
        ),
        "project deny.exceptions.toml passed",
    )?;
    assert_eq!(error, SecurityError::InvalidPolicy);
    cases.push(error_evidence(
        "project-deny-exceptions",
        "InvalidPolicy",
        error,
    ));
    inventory(&gateway)?;

    let error = required_error(
        security_gateway::execute(
            &gateway,
            &source,
            &vendor,
            &policy,
            ExecutionLimits::new_job(120_000, 1024 * 1024).ok_or("pre-cancel limits")?,
            &Cancelled,
        ),
        "pre-cancelled execution passed",
    )?;
    assert!(matches!(
        error,
        SecurityError::Inspection(InspectionError::Project(ProjectError::Cancelled))
    ));
    cases.push(error_evidence(
        "cancel-before-first-phase",
        "Inspection(Project(Cancelled))",
        error,
    ));
    inventory(&gateway)?;

    let during = CancelAfter {
        started: Instant::now(),
        delay: Duration::from_millis(250),
    };
    let error = required_error(
        security_gateway::execute(
            &gateway,
            &source,
            &vendor,
            &policy,
            ExecutionLimits::new_job(120_000, 1024 * 1024).ok_or("during-cancel limits")?,
            &during,
        ),
        "mid-phase cancellation passed",
    )?;
    assert!(matches!(
        error,
        SecurityError::Inspection(InspectionError::Project(ProjectError::Cancelled))
    ));
    cases.push(error_evidence(
        "cancel-during-phase",
        "Inspection(Project(Cancelled))",
        error,
    ));
    inventory(&gateway)?;

    let error = required_error(
        security_gateway::execute(
            &gateway,
            &source,
            &vendor,
            &policy,
            ExecutionLimits::new_job(100, 1024 * 1024).ok_or("timeout limits")?,
            &NeverCancel,
        ),
        "100ms budget passed",
    )?;
    assert_eq!(error, SecurityError::Timeout);
    cases.push(error_evidence("short-timeout", "Timeout", error));
    inventory(&gateway)?;

    let error = required_error(
        security_gateway::execute(
            &gateway,
            &source,
            &vendor,
            &policy,
            ExecutionLimits::new_job(120_000, 1024).ok_or("output limits")?,
            &NeverCancel,
        ),
        "1KiB retained stream passed",
    )?;
    assert_eq!(error, SecurityError::OutputLimit);
    cases.push(error_evidence("bounded-output", "OutputLimit", error));
    let (containers, volumes) = inventory(&gateway)?;

    assert_eq!(source, source_before, "source bundle mutated");
    assert_eq!(vendor, vendor_before, "vendor snapshot mutated");
    let source_after = digest(
        &source_archive::encode(&source).map_err(|error| format!("source archive: {error:?}"))?,
    );
    let vendor_after = resolution_gateway::tree_fingerprint(&vendor.source)
        .map_err(|error| format!("post vendor fingerprint: {error:?}"))?;
    assert_eq!(vendor_after.to_string(), VENDOR_FINGERPRINT);

    let evidence = serde_json::json!({
        "schema_version": 1,
        "suite": "m4-deny-adversarial",
        "image_id": M4_IMAGE,
        "vendor_tree_fingerprint_before": VENDOR_FINGERPRINT,
        "vendor_tree_fingerprint_after": vendor_after,
        "source_archive_fingerprint_after": source_after,
        "unicode_ident_package_checksum": UNICODE_IDENT_CHECKSUM,
        "cases": cases,
        "cleanup": {
            "containers_raw": containers,
            "volumes_raw": volumes,
            "all_absent": true
        }
    });
    let output =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/m4-deny-adversarial.json");
    std::fs::write(&output, serde_json::to_vec_pretty(&evidence)?)?;
    println!(
        "M4_DENY_ADVERSARIAL {}",
        digest(&serde_json::to_vec(&evidence)?)
    );

    drop(gateway);
    std::fs::remove_dir_all(state_root)?;
    Ok(())
}
