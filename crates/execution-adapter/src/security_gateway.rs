//! Closed three-volume cargo-deny execution from ADR-067.
use super::*;
use crate::mutation_gateway::{
    MutationVolume, VOLUME_OPTIONS, absent, cleanup_until, labels, mutation_control, parse_volume,
    query_control, remove_if_present, running, start_attached,
};
use crate::rust_gateway::RustGateway;
use rust_engineering_application::security::SecurityError;
use rust_engineering_domain::security::SecurityPolicy;
use rust_engineering_domain::{
    CargoVendorSnapshot, ExecutionFingerprint, ExecutionLimits, SourceBundle, SourceFile,
    SourceFingerprint,
};
use serde::Serialize;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

const CLEANUP: Duration = Duration::from_secs(10);
const METADATA_OUTPUT: usize = 1024 * 1024;
const SECURITY_ROOT: &str = "/security";
const VENDOR_ROOT: &str = "/rust-mcp-vendor";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(super) enum SecurityPhase {
    SourceGuardian,
    VendorGuardian,
    PolicyGuardian,
    SourceIngest,
    VendorIngest,
    PolicyIngest,
    Metadata,
    MetadataIngest,
    Deny,
    UnsafeScan,
    Miri,
    MiriOutputGuardian,
    MiriExport,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(super) struct SecurityMounts {
    pub(super) source: bool,
    pub(super) vendor: bool,
    pub(super) policy: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(super) struct SecurityPermissions {
    pub(super) source_writable: bool,
    pub(super) vendor_writable: bool,
    pub(super) policy_writable: bool,
}

impl SecurityPhase {
    pub(super) fn program(self) -> &'static str {
        match self {
            Self::SourceGuardian
            | Self::VendorGuardian
            | Self::PolicyGuardian
            | Self::MiriOutputGuardian => "/usr/bin/sleep",
            Self::SourceIngest
            | Self::VendorIngest
            | Self::PolicyIngest
            | Self::MetadataIngest
            | Self::MiriExport => "/usr/bin/tar",
            Self::Metadata => "/opt/rust/bin/cargo",
            Self::Miri => "/opt/rust-nightly-2026-09-07/bin/cargo",
            Self::Deny => "/opt/security/bin/cargo-deny",
            Self::UnsafeScan => "/opt/security/bin/rust-mcp-unsafe-helper",
        }
    }

    pub(super) fn arguments(self) -> &'static [&'static str] {
        match self {
            Self::SourceGuardian
            | Self::VendorGuardian
            | Self::PolicyGuardian
            | Self::MiriOutputGuardian => &["3600"],
            Self::SourceIngest => &[
                "--extract",
                "--file=-",
                "--directory=/source",
                "--no-same-owner",
                "--no-same-permissions",
                "--keep-old-files",
            ],
            Self::VendorIngest => &[
                "--extract",
                "--file=-",
                "--directory=/rust-mcp-vendor",
                "--no-same-owner",
                "--no-same-permissions",
                "--keep-old-files",
            ],
            Self::PolicyIngest | Self::MetadataIngest => &[
                "--extract",
                "--file=-",
                "--directory=/security",
                "--no-same-owner",
                "--no-same-permissions",
                "--keep-old-files",
            ],
            Self::Metadata => &[
                "metadata",
                "--frozen",
                "--offline",
                "--format-version=1",
                "--manifest-path=/source/Cargo.toml",
            ],
            Self::UnsafeScan => &[],
            Self::Miri => &[
                "miri",
                "nextest",
                "run",
                "--workspace",
                "--tests",
                "--manifest-path=/source/Cargo.toml",
                "--config-file=/security/miri-nextest.toml",
                "--profile=rust-mcp-miri",
                "--frozen",
                "--offline",
                "--color=never",
                "--no-fail-fast",
                "--build-jobs=1",
                "--test-threads=1",
                "--target=aarch64-unknown-linux-gnu",
            ],
            Self::MiriExport => &[
                "--create",
                "--file=-",
                "--format=ustar",
                "--no-recursion",
                "--directory=/junit/rust-mcp-miri/reports",
                "junit.xml",
            ],
            Self::Deny => &[
                "--format=json",
                "--log-level=debug",
                "--color=never",
                "--offline",
                "--frozen",
                "--manifest-path=/source/Cargo.toml",
                "check",
                "--config=/security/deny.toml",
                "--metadata-path=/security/metadata.json",
                "licenses",
                "bans",
                "sources",
                "--disable-fetch",
            ],
        }
    }

    pub(super) fn interactive(self) -> bool {
        matches!(
            self,
            Self::SourceIngest | Self::VendorIngest | Self::PolicyIngest | Self::MetadataIngest
        )
    }

    pub(super) fn mounts(self) -> SecurityMounts {
        match self {
            Self::SourceGuardian | Self::SourceIngest => SecurityMounts {
                source: true,
                vendor: false,
                policy: false,
            },
            Self::VendorGuardian | Self::VendorIngest => SecurityMounts {
                source: false,
                vendor: true,
                policy: false,
            },
            Self::PolicyGuardian | Self::PolicyIngest | Self::MetadataIngest => SecurityMounts {
                source: false,
                vendor: false,
                policy: true,
            },
            Self::MiriOutputGuardian | Self::MiriExport => SecurityMounts {
                source: false,
                vendor: false,
                policy: false,
            },
            Self::Metadata | Self::Deny | Self::UnsafeScan | Self::Miri => SecurityMounts {
                source: true,
                vendor: true,
                policy: true,
            },
        }
    }

    pub(super) fn permissions(self) -> SecurityPermissions {
        SecurityPermissions {
            source_writable: self == Self::SourceIngest,
            vendor_writable: self == Self::VendorIngest,
            policy_writable: matches!(self, Self::PolicyIngest | Self::MetadataIngest),
        }
    }

    pub(super) fn source_mounted(self) -> bool {
        self.mounts().source
    }

    pub(super) fn vendor_mounted(self) -> bool {
        self.mounts().vendor
    }

    pub(super) fn policy_mounted(self) -> bool {
        self.mounts().policy
    }

    pub(super) fn source_writable(self) -> bool {
        self.permissions().source_writable
    }

    pub(super) fn vendor_writable(self) -> bool {
        self.permissions().vendor_writable
    }

    pub(super) fn policy_writable(self) -> bool {
        self.permissions().policy_writable
    }

    pub(super) fn junit_mounted(self) -> bool {
        matches!(
            self,
            Self::Miri | Self::MiriOutputGuardian | Self::MiriExport
        )
    }
    pub(super) fn junit_writable(self) -> bool {
        self == Self::Miri
    }
    pub(super) fn profile(self) -> &'static str {
        if self == Self::Miri {
            include_str!("seccomp-rust-quality.json")
        } else {
            include_str!("seccomp-rust.json")
        }
    }
    fn profile_file(self) -> &'static str {
        if self == Self::Miri {
            "seccomp-rust-quality.json"
        } else {
            "seccomp-rust.json"
        }
    }
    pub(super) fn environment(self) -> Vec<String> {
        if self == Self::Miri {
            return crate::miri_admission::environment();
        }
        let mut environment = crate::rust_gateway::environment();
        let cargo_home = environment
            .iter_mut()
            .find(|value| value.starts_with("CARGO_HOME="));
        if let Some(cargo_home) = cargo_home {
            *cargo_home = "CARGO_HOME=/security/cargo-home".into();
        }
        environment.sort();
        environment
    }
}

pub(super) struct SecurityVolumes<'a> {
    pub(super) source: &'a MutationVolume,
    pub(super) vendor: &'a MutationVolume,
    pub(super) policy: &'a MutationVolume,
    pub(super) junit: Option<&'a MutationVolume>,
}

struct SecurityIngest<'a, 'v> {
    name: &'a str,
    operation_id: &'a str,
    volumes: &'a SecurityVolumes<'v>,
    phase: SecurityPhase,
    deadline: Instant,
    output_limit: usize,
}

pub(super) struct SecurityExecution {
    pub metadata: crate::security_metadata::PreparedSecurityMetadata,
    pub capture: super::supervisor::Capture,
    pub execution_fingerprint: ExecutionFingerprint,
    pub source_fingerprint: SourceFingerprint,
    pub vendor_fingerprint: SourceFingerprint,
    pub vendor_archive_fingerprint: SourceFingerprint,
    pub policy_fingerprint: Option<SourceFingerprint>,
    pub scan_plan: Option<crate::unsafe_scan::ScanPlan>,
    pub manifest_fingerprint: SourceFingerprint,
    pub junit: Option<Vec<u8>>,
    pub deny_config_fingerprint: SourceFingerprint,
    pub cargo_config_fingerprint: SourceFingerprint,
}

fn bytes_fingerprint(bytes: &[u8]) -> Result<SourceFingerprint, SecurityError> {
    digest(bytes)
        .parse()
        .map_err(|_| ExecutionError::Infrastructure.into())
}

fn mount_arguments(phase: SecurityPhase, volumes: &SecurityVolumes<'_>) -> Vec<String> {
    let mut arguments = Vec::new();
    for (mounted, writable, volume, target) in [
        (
            phase.source_mounted(),
            phase.source_writable(),
            volumes.source,
            "/source",
        ),
        (
            phase.vendor_mounted(),
            phase.vendor_writable(),
            volumes.vendor,
            VENDOR_ROOT,
        ),
        (
            phase.policy_mounted(),
            phase.policy_writable(),
            volumes.policy,
            SECURITY_ROOT,
        ),
    ] {
        if mounted {
            arguments.push(format!(
                "--mount=type=volume,source={},target={target},volume-nocopy,volume-driver=local{}",
                volume.name,
                if writable { "" } else { ",readonly" }
            ));
        }
    }
    if phase.junit_mounted()
        && let Some(volume) = volumes.junit
    {
        arguments.push(format!(
            "--mount=type=volume,source={},target=/junit,volume-nocopy,volume-driver=local{}",
            volume.name,
            if phase.junit_writable() {
                ""
            } else {
                ",readonly"
            }
        ));
    }
    arguments
}

fn create_arguments(
    gateway: &RustGateway,
    name: &str,
    operation_id: &str,
    volumes: &SecurityVolumes<'_>,
    phase: SecurityPhase,
) -> Result<Vec<String>, ExecutionError> {
    create_arguments_for_runtime(
        gateway.image_id(),
        gateway.inner.state.path(),
        name,
        operation_id,
        volumes,
        phase,
    )
}

fn create_arguments_for_runtime(
    image_id: &str,
    state_path: &std::path::Path,
    name: &str,
    operation_id: &str,
    volumes: &SecurityVolumes<'_>,
    phase: SecurityPhase,
) -> Result<Vec<String>, ExecutionError> {
    let mut arguments = [
        "container",
        "create",
        "--pull=never",
        "--runtime=runc",
        "--init=false",
        "--network=none",
        "--read-only",
        "--cap-drop=ALL",
        "--security-opt=no-new-privileges=true",
        "--ipc=private",
        "--cgroupns=private",
        "--pids-limit=128",
        "--cpus=1",
        "--memory=1g",
        "--memory-swap=1g",
        "--shm-size=1m",
        "--log-driver=none",
        "--no-healthcheck",
        "--tmpfs=/work:rw,exec,nosuid,nodev,size=512m,mode=1777",
        "--tmpfs=/tmp:rw,nosuid,nodev,noexec,size=64m,mode=1777",
        "--workdir=/source",
        "--hostname=sandbox",
        "--user=65534:65534",
    ]
    .map(str::to_owned)
    .to_vec();
    arguments.push(format!("--name={name}"));
    for (key, value) in labels(operation_id) {
        arguments.push(format!("--label={key}={value}"));
    }
    for value in phase.environment() {
        arguments.push(format!("--env={value}"));
    }
    let profile = state_path.join(phase.profile_file());
    arguments.push(format!(
        "--security-opt=seccomp={}",
        profile
            .to_str()
            .ok_or(ExecutionError::InvalidConfiguration)?
    ));
    arguments.extend(mount_arguments(phase, volumes));
    if phase.interactive() {
        arguments.push("--interactive".into());
    }
    arguments.push(format!("--entrypoint={}", phase.program()));
    arguments.push(image_id.into());
    arguments.extend(phase.arguments().iter().map(|value| (*value).to_owned()));
    Ok(arguments)
}

fn budget_error(
    deadline: Instant,
    cancel: &dyn ExecutionCancellation,
) -> Result<(), SecurityError> {
    if cancel.is_cancelled() {
        Err(ExecutionError::Cancelled.into())
    } else if Instant::now() >= deadline {
        Err(SecurityError::Timeout)
    } else {
        Ok(())
    }
}

fn phase_result<T>(
    result: Result<T, ExecutionError>,
    deadline: Instant,
    cancel: &dyn ExecutionCancellation,
) -> Result<T, SecurityError> {
    match result {
        Ok(value) => {
            budget_error(deadline, cancel)?;
            Ok(value)
        }
        Err(ExecutionError::CleanupUncertain) => Err(ExecutionError::CleanupUncertain.into()),
        Err(_) if cancel.is_cancelled() => Err(ExecutionError::Cancelled.into()),
        Err(_) if Instant::now() >= deadline => Err(SecurityError::Timeout),
        Err(error) => Err(error.into()),
    }
}

fn create_volume(
    gateway: &RustGateway,
    name: &str,
    operation_id: &str,
    deadline: Instant,
    cancel: &dyn ExecutionCancellation,
) -> Result<MutationVolume, SecurityError> {
    budget_error(deadline, cancel)?;
    if !phase_result(
        absent(gateway, "volume", name, deadline, cancel),
        deadline,
        cancel,
    )? {
        return Err(ExecutionError::CleanupUncertain.into());
    }
    let mut arguments = vec![
        "volume".into(),
        "create".into(),
        "--driver=local".into(),
        "--opt=type=tmpfs".into(),
        "--opt=device=tmpfs".into(),
        format!("--opt=o={VOLUME_OPTIONS}"),
    ];
    for (key, value) in labels(operation_id) {
        arguments.push(format!("--label={key}={value}"));
    }
    arguments.push(name.into());
    phase_result(
        mutation_control(gateway, &arguments, deadline, cancel),
        deadline,
        cancel,
    )?;
    let inspected = phase_result(
        query_control(
            gateway,
            &["volume".into(), "inspect".into(), name.into()],
            deadline,
            cancel,
        ),
        deadline,
        cancel,
    )?;
    if inspected.code != Some(0) {
        return Err(ExecutionError::Infrastructure.into());
    }
    parse_volume(&inspected.stdout, name, operation_id).map_err(Into::into)
}

fn create_phase(
    gateway: &RustGateway,
    name: &str,
    operation_id: &str,
    volumes: &SecurityVolumes<'_>,
    phase: SecurityPhase,
    deadline: Instant,
    cancel: &dyn ExecutionCancellation,
) -> Result<(), SecurityError> {
    budget_error(deadline, cancel)?;
    phase_result(gateway.approved_runtime(cancel), deadline, cancel)?;
    if !phase_result(
        absent(gateway, "container", name, deadline, cancel),
        deadline,
        cancel,
    )? {
        return Err(ExecutionError::CleanupUncertain.into());
    }
    let arguments = create_arguments(gateway, name, operation_id, volumes, phase)?;
    phase_result(
        mutation_control(gateway, &arguments, deadline, cancel),
        deadline,
        cancel,
    )?;
    let inspected = phase_result(
        query_control(
            gateway,
            &["container".into(), "inspect".into(), name.into()],
            deadline,
            cancel,
        ),
        deadline,
        cancel,
    )?;
    if inspected.code != Some(0) {
        return Err(ExecutionError::Infrastructure.into());
    }
    crate::rust_applied::verify_security(
        &inspected.stdout,
        gateway.image_id(),
        phase,
        volumes,
        operation_id,
    )?;
    Ok(())
}

fn completed_without_oom(
    gateway: &RustGateway,
    name: &str,
    capture: &Capture,
    deadline: Instant,
    cancel: &dyn ExecutionCancellation,
) -> Result<(), SecurityError> {
    validate_capture_completion(capture)?;
    let inspected = phase_result(
        query_control(
            gateway,
            &["container".into(), "inspect".into(), name.into()],
            deadline,
            cancel,
        ),
        deadline,
        cancel,
    )?;
    validate_completed_container(&inspected, capture)
}

fn validate_capture_completion(capture: &Capture) -> Result<(), SecurityError> {
    match capture.stop {
        Stop::Cancelled => return Err(ExecutionError::Cancelled.into()),
        Stop::TimedOut => return Err(SecurityError::Timeout),
        Stop::OutputLimit => return Err(SecurityError::OutputLimit),
        Stop::Exited => {}
    }
    if capture.stdout_truncated || capture.stderr_truncated {
        return Err(SecurityError::OutputLimit);
    }
    Ok(())
}

fn validate_completed_container(
    inspected: &Capture,
    capture: &Capture,
) -> Result<(), SecurityError> {
    let containers: Vec<Container> =
        serde_json::from_slice(&inspected.stdout).map_err(|_| ExecutionError::Infrastructure)?;
    let container = containers
        .first()
        .filter(|_| containers.len() == 1)
        .ok_or(ExecutionError::Infrastructure)?;
    if inspected.code != Some(0)
        || !container.state.completed(capture.code)
        || container.state.oom_killed
    {
        return Err(ExecutionError::Infrastructure.into());
    }
    Ok(())
}

fn finish_phase(
    gateway: &RustGateway,
    name: &str,
    operation_id: &str,
    capture: &Capture,
    deadline: Instant,
    cancel: &dyn ExecutionCancellation,
) -> Result<(), SecurityError> {
    completed_without_oom(gateway, name, capture, deadline, cancel)?;
    phase_result(
        remove_if_present(gateway, name, operation_id, deadline, cancel),
        deadline,
        cancel,
    )?;
    if !phase_result(
        absent(gateway, "container", name, deadline, cancel),
        deadline,
        cancel,
    )? {
        return Err(ExecutionError::CleanupUncertain.into());
    }
    budget_error(deadline, cancel)
}

fn start_guardian(
    gateway: &RustGateway,
    name: &str,
    operation_id: &str,
    volumes: &SecurityVolumes<'_>,
    phase: SecurityPhase,
    deadline: Instant,
    cancel: &dyn ExecutionCancellation,
) -> Result<(), SecurityError> {
    create_phase(
        gateway,
        name,
        operation_id,
        volumes,
        phase,
        deadline,
        cancel,
    )?;
    phase_result(
        mutation_control(
            gateway,
            &["container".into(), "start".into(), name.into()],
            deadline,
            cancel,
        ),
        deadline,
        cancel,
    )?;
    if !phase_result(
        running(gateway, name, operation_id, deadline, cancel),
        deadline,
        cancel,
    )? {
        return Err(ExecutionError::Infrastructure.into());
    }
    budget_error(deadline, cancel)
}

fn revalidate(
    gateway: &RustGateway,
    guardians: &[&str],
    removed: &[&str],
    operation_id: &str,
    deadline: Instant,
    cancel: &dyn ExecutionCancellation,
) -> Result<(), SecurityError> {
    for guardian in guardians {
        if !phase_result(
            running(gateway, guardian, operation_id, deadline, cancel),
            deadline,
            cancel,
        )? {
            return Err(ExecutionError::Infrastructure.into());
        }
    }
    for name in removed {
        if !phase_result(
            absent(gateway, "container", name, deadline, cancel),
            deadline,
            cancel,
        )? {
            return Err(ExecutionError::CleanupUncertain.into());
        }
    }
    budget_error(deadline, cancel)
}

fn ingest(
    gateway: &RustGateway,
    request: SecurityIngest<'_, '_>,
    archive: &[u8],
    cancel: &dyn ExecutionCancellation,
) -> Result<(), SecurityError> {
    create_phase(
        gateway,
        request.name,
        request.operation_id,
        request.volumes,
        request.phase,
        request.deadline,
        cancel,
    )?;
    let capture = phase_result(
        start_attached(
            gateway,
            request.name,
            true,
            archive,
            request.deadline,
            request.output_limit,
            cancel,
        ),
        request.deadline,
        cancel,
    )?;
    finish_phase(
        gateway,
        request.name,
        request.operation_id,
        &capture,
        request.deadline,
        cancel,
    )?;
    if capture.code != Some(0) || !capture.stdout.is_empty() || !capture.stderr.is_empty() {
        return Err(ExecutionError::Infrastructure.into());
    }
    Ok(())
}

fn policy_archive(deny_config: Option<&[u8]>, miri: bool) -> Result<Vec<u8>, SecurityError> {
    let mut entries = vec![(
        "cargo-home/config.toml",
        crate::security_policy::SECURITY_CARGO_CONFIG,
    )];
    if let Some(config) = deny_config {
        entries.push(("deny.toml", config));
    }
    if miri {
        entries.push(("miri-nextest.toml", crate::miri_admission::CONFIG));
    }
    let files = entries
        .into_iter()
        .map(|(path, bytes)| SourceFile::new(path.into(), bytes.to_vec()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ExecutionError::InvalidConfiguration)?;
    let bundle = SourceBundle::new(files).map_err(|_| ExecutionError::InvalidConfiguration)?;
    crate::mutation_archive::encode(&bundle).map_err(Into::into)
}

fn metadata_archive(path: &str, metadata: &[u8]) -> Result<Vec<u8>, SecurityError> {
    let file = SourceFile::new(path.into(), metadata.to_vec())
        .map_err(|_| SecurityError::InvalidMetadata)?;
    let bundle = SourceBundle::new(vec![file]).map_err(|_| SecurityError::InvalidMetadata)?;
    crate::mutation_archive::encode(&bundle).map_err(Into::into)
}

struct SecurityFingerprintInputs<'a> {
    source_archive: &'a [u8],
    vendor_archive: &'a [u8],
    policy_archive: &'a [u8],
    metadata_archive: &'a [u8],
    metadata: &'a crate::security_metadata::PreparedSecurityMetadata,
    policy: Option<&'a SecurityPolicy>,
    final_phase: SecurityPhase,
    vendor_fingerprint: &'a SourceFingerprint,
    deny_config: &'a [u8],
    limits: ExecutionLimits,
    capture: &'a Capture,
    junit: Option<&'a [u8]>,
}

struct CompletedSecurityWork {
    metadata: crate::security_metadata::PreparedSecurityMetadata,
    derived_archive: Vec<u8>,
    capture: Capture,
    scan_plan: Option<crate::unsafe_scan::ScanPlan>,
    manifest_fingerprint: SourceFingerprint,
    junit: Option<Vec<u8>>,
}

struct SecurityFinalizationInputs<'a> {
    source_archive: &'a [u8],
    vendor_archive: &'a [u8],
    policy_archive: &'a [u8],
    deny_config: &'a [u8],
    vendor_fingerprint: &'a SourceFingerprint,
    policy: Option<&'a SecurityPolicy>,
    final_phase: SecurityPhase,
    limits: ExecutionLimits,
    work: CompletedSecurityWork,
}

fn execution_fingerprint_for_runtime(
    configuration_fingerprint: &ExecutionFingerprint,
    image_id: &str,
    state_path: &std::path::Path,
    inputs: SecurityFingerprintInputs<'_>,
) -> Result<ExecutionFingerprint, SecurityError> {
    let volume = |name: &str, mountpoint: &str| MutationVolume {
        name: name.into(),
        driver: "local".into(),
        scope: "local".into(),
        options: std::collections::BTreeMap::from([
            ("device".into(), "tmpfs".into()),
            ("o".into(), VOLUME_OPTIONS.into()),
            ("type".into(), "tmpfs".into()),
        ]),
        labels: labels("<operation_id>"),
        mountpoint: mountpoint.into(),
        cluster_volume: None,
        status: None,
    };
    let source = volume("<source-volume>", "<source-mountpoint>");
    let vendor = volume("<vendor-volume>", "<vendor-mountpoint>");
    let policy_volume = volume("<policy-volume>", "<policy-mountpoint>");
    let volumes = SecurityVolumes {
        source: &source,
        vendor: &vendor,
        policy: &policy_volume,
        junit: None,
    };
    let mut phases = vec![
        SecurityPhase::SourceGuardian,
        SecurityPhase::VendorGuardian,
        SecurityPhase::PolicyGuardian,
        SecurityPhase::SourceIngest,
        SecurityPhase::VendorIngest,
        SecurityPhase::PolicyIngest,
        SecurityPhase::Metadata,
        SecurityPhase::MetadataIngest,
        inputs.final_phase,
    ];
    if inputs.final_phase == SecurityPhase::Miri {
        phases.extend([SecurityPhase::MiriOutputGuardian, SecurityPhase::MiriExport]);
    }
    let junit_volume = volume("<junit-volume>", "<junit-mountpoint>");
    let volumes = SecurityVolumes {
        junit: (inputs.final_phase == SecurityPhase::Miri).then_some(&junit_volume),
        ..volumes
    };
    let commands = phases
        .into_iter()
        .map(|phase| {
            create_arguments_for_runtime(
                image_id,
                state_path,
                "<container>",
                "<operation_id>",
                &volumes,
                phase,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let bytes = serde_json::to_vec(&(
        configuration_fingerprint,
        commands,
        ["--opt=type=tmpfs", "--opt=device=tmpfs", VOLUME_OPTIONS],
        digest(inputs.source_archive),
        digest(inputs.vendor_archive),
        digest(inputs.policy_archive),
        digest(inputs.metadata_archive),
        inputs.policy.map(SecurityPolicy::fingerprint),
        inputs.vendor_fingerprint,
        &inputs.metadata.original_fingerprint,
        &inputs.metadata.derived_fingerprint,
        digest(inputs.deny_config),
        digest(crate::security_policy::SECURITY_CARGO_CONFIG),
        inputs.limits,
        (
            inputs.capture.code,
            digest(&inputs.capture.stdout),
            digest(&inputs.capture.stderr),
            inputs.junit.map(digest),
        ),
        (
            digest(include_bytes!("security_gateway.rs")),
            digest(include_bytes!("security_metadata.rs")),
            digest(include_bytes!("security_policy.rs")),
            digest(include_bytes!("mutation_gateway.rs")),
            digest(include_bytes!("rust_applied.rs")),
            digest(include_bytes!("seccomp-rust.json")),
            digest(include_bytes!("miri_admission.rs")),
            digest(include_bytes!("unsafe_scan.rs")),
            digest(include_bytes!("unsafe_port.rs")),
            digest(include_bytes!("deny_json.rs")),
            digest(include_bytes!("security_port.rs")),
            digest(include_bytes!("miri_output.rs")),
            digest(include_bytes!("miri_port.rs")),
            digest(include_bytes!("seccomp-rust-quality.json")),
        ),
    ))
    .map_err(|_| ExecutionError::Infrastructure)?;
    digest(&bytes)
        .parse()
        .map_err(|_| ExecutionError::Infrastructure.into())
}

fn finalize_security_execution(
    gateway: &RustGateway,
    inputs: SecurityFinalizationInputs<'_>,
) -> Result<SecurityExecution, SecurityError> {
    let configuration_fingerprint = gateway.configuration_fingerprint()?;
    finalize_security_execution_for_runtime(
        &configuration_fingerprint,
        gateway.image_id(),
        gateway.inner.state.path(),
        inputs,
    )
}

fn finalize_security_execution_for_runtime(
    configuration_fingerprint: &ExecutionFingerprint,
    image_id: &str,
    state_path: &std::path::Path,
    inputs: SecurityFinalizationInputs<'_>,
) -> Result<SecurityExecution, SecurityError> {
    let source_fingerprint = bytes_fingerprint(inputs.source_archive)?;
    let vendor_archive_fingerprint = bytes_fingerprint(inputs.vendor_archive)?;
    let deny_config_fingerprint = bytes_fingerprint(inputs.deny_config)?;
    let cargo_config_fingerprint =
        bytes_fingerprint(crate::security_policy::SECURITY_CARGO_CONFIG)?;
    let fingerprint = execution_fingerprint_for_runtime(
        configuration_fingerprint,
        image_id,
        state_path,
        SecurityFingerprintInputs {
            source_archive: inputs.source_archive,
            vendor_archive: inputs.vendor_archive,
            policy_archive: inputs.policy_archive,
            metadata_archive: &inputs.work.derived_archive,
            metadata: &inputs.work.metadata,
            policy: inputs.policy,
            final_phase: inputs.final_phase,
            vendor_fingerprint: inputs.vendor_fingerprint,
            deny_config: inputs.deny_config,
            limits: inputs.limits,
            capture: &inputs.work.capture,
            junit: inputs.work.junit.as_deref(),
        },
    )?;
    Ok(SecurityExecution {
        metadata: inputs.work.metadata,
        capture: inputs.work.capture,
        execution_fingerprint: fingerprint,
        source_fingerprint,
        vendor_fingerprint: inputs.vendor_fingerprint.clone(),
        vendor_archive_fingerprint,
        policy_fingerprint: inputs.policy.map(|policy| policy.fingerprint().clone()),
        scan_plan: inputs.work.scan_plan,
        manifest_fingerprint: inputs.work.manifest_fingerprint,
        junit: inputs.work.junit,
        deny_config_fingerprint,
        cargo_config_fingerprint,
    })
}

#[derive(Clone, Copy)]
enum SecurityOperation<'a> {
    Deny(&'a SecurityPolicy),
    UnsafeScan,
    Miri,
}

fn operation_policy_and_phase(
    operation: SecurityOperation<'_>,
) -> (Option<&SecurityPolicy>, SecurityPhase) {
    match operation {
        SecurityOperation::Deny(policy) => (Some(policy), SecurityPhase::Deny),
        SecurityOperation::UnsafeScan => (None, SecurityPhase::UnsafeScan),
        SecurityOperation::Miri => (None, SecurityPhase::Miri),
    }
}

fn validate_vendor(vendor: &CargoVendorSnapshot) -> Result<(), SecurityError> {
    if crate::resolution_gateway::tree_fingerprint(&vendor.source)
        .map_err(|_| SecurityError::MissingOfflineData)?
        != vendor.tree_fingerprint
    {
        return Err(SecurityError::MissingOfflineData);
    }
    Ok(())
}
pub(super) fn execute_miri(
    gateway: &RustGateway,
    source: &SourceBundle,
    vendor: &CargoVendorSnapshot,
    limits: ExecutionLimits,
    cancel: &dyn ExecutionCancellation,
) -> Result<SecurityExecution, SecurityError> {
    execute_operation(
        gateway,
        source,
        vendor,
        SecurityOperation::Miri,
        limits,
        cancel,
    )
}
pub(super) fn execute_scan(
    gateway: &RustGateway,
    source: &SourceBundle,
    vendor: &CargoVendorSnapshot,
    limits: ExecutionLimits,
    cancel: &dyn ExecutionCancellation,
) -> Result<SecurityExecution, SecurityError> {
    execute_operation(
        gateway,
        source,
        vendor,
        SecurityOperation::UnsafeScan,
        limits,
        cancel,
    )
}

pub(super) fn execute(
    gateway: &RustGateway,
    source: &SourceBundle,
    vendor: &CargoVendorSnapshot,
    policy: &SecurityPolicy,
    limits: ExecutionLimits,
    cancel: &dyn ExecutionCancellation,
) -> Result<SecurityExecution, SecurityError> {
    execute_operation(
        gateway,
        source,
        vendor,
        SecurityOperation::Deny(policy),
        limits,
        cancel,
    )
}

// Remaining control allowance: 37 round trips x 250 ms + 2 s startup +
// 1 s output validation + 10 s joined cleanup, rounded up (ADR-069).
const SCAN_CONTROL_RESERVE_MS: u64 = 25_000;
fn scanner_budget_ms(remaining_ms: u64) -> Result<u64, SecurityError> {
    remaining_ms
        .checked_sub(SCAN_CONTROL_RESERVE_MS)
        .filter(|v| *v > 0)
        .map(|v| v.min(118_000))
        .ok_or(SecurityError::Timeout)
}

fn decode_miri_junit_export(
    exported: &Capture,
    miri_exit_code: Option<i32>,
) -> Result<Option<Vec<u8>>, SecurityError> {
    if exported.code == Some(0) {
        return crate::nextest_gateway::decode_single_file_tar(
            &exported.stdout,
            512 * 1024,
            "junit.xml",
        )
        .map(Some)
        .ok_or(SecurityError::InvalidMetadata);
    }
    if miri_exit_code == Some(104) {
        Ok(None)
    } else {
        Err(SecurityError::InvalidMetadata)
    }
}

fn execute_operation(
    gateway: &RustGateway,
    source: &SourceBundle,
    vendor: &CargoVendorSnapshot,
    operation: SecurityOperation<'_>,
    limits: ExecutionLimits,
    cancel: &dyn ExecutionCancellation,
) -> Result<SecurityExecution, SecurityError> {
    let (policy, final_phase) = operation_policy_and_phase(operation);
    if final_phase == SecurityPhase::Miri {
        crate::miri_admission::validate_configuration(source)?;
    }
    let started = Instant::now();
    let deadline = started + Duration::from_millis(limits.wall_ms());
    let _busy = gateway.hold_busy()?;
    if gateway.is_quarantined() {
        return Err(ExecutionError::CleanupUncertain.into());
    }
    if gateway.calibrating.load(Ordering::Acquire) || !gateway.verified.load(Ordering::Acquire) {
        return Err(ExecutionError::Denied.into());
    }
    phase_result(gateway.approved_runtime(cancel), deadline, cancel)?;
    if policy.is_some() {
        crate::security_policy::reject_project_exceptions(source)
            .map_err(|_| SecurityError::InvalidPolicy)?;
    }
    budget_error(deadline, cancel)?;
    validate_vendor(vendor)?;
    let source_archive = crate::source_archive::encode(source)?;
    budget_error(deadline, cancel)?;
    let vendor_archive = crate::source_archive::encode(&vendor.source)?;
    budget_error(deadline, cancel)?;
    let deny_config = policy
        .map(crate::security_policy::deny_config)
        .transpose()
        .map_err(|_| SecurityError::InvalidPolicy)?;
    let initial_policy_archive =
        policy_archive(deny_config.as_deref(), final_phase == SecurityPhase::Miri)?;
    budget_error(deadline, cancel)?;

    let operation_id = state::nonce()?;
    let source_volume_name = format!("rust-mcp-security-source-{operation_id}");
    let vendor_volume_name = format!("rust-mcp-security-vendor-{operation_id}");
    let policy_volume_name = format!("rust-mcp-security-policy-{operation_id}");
    let source_guardian = format!("rust-mcp-security-source-guardian-{operation_id}");
    let vendor_guardian = format!("rust-mcp-security-vendor-guardian-{operation_id}");
    let policy_guardian = format!("rust-mcp-security-policy-guardian-{operation_id}");
    let source_ingest = format!("rust-mcp-security-source-ingest-{operation_id}");
    let vendor_ingest = format!("rust-mcp-security-vendor-ingest-{operation_id}");
    let policy_ingest = format!("rust-mcp-security-policy-ingest-{operation_id}");
    let metadata_run = format!("rust-mcp-security-metadata-{operation_id}");
    let metadata_ingest = format!("rust-mcp-security-metadata-ingest-{operation_id}");
    let deny_run = format!("rust-mcp-security-engine-{operation_id}");
    let junit_volume_name = format!("rust-mcp-security-junit-{operation_id}");
    let junit_guardian = format!("rust-mcp-security-junit-guardian-{operation_id}");
    let junit_export = format!("rust-mcp-security-junit-export-{operation_id}");
    let all_names = [
        &junit_guardian[..],
        &junit_export[..],
        &source_ingest[..],
        &vendor_ingest[..],
        &policy_ingest[..],
        &metadata_run[..],
        &metadata_ingest[..],
        &deny_run[..],
        &source_guardian[..],
        &vendor_guardian[..],
        &policy_guardian[..],
    ];

    if !phase_result(
        absent(gateway, "volume", &source_volume_name, deadline, cancel),
        deadline,
        cancel,
    )? || !phase_result(
        absent(gateway, "volume", &vendor_volume_name, deadline, cancel),
        deadline,
        cancel,
    )? || !phase_result(
        absent(gateway, "volume", &policy_volume_name, deadline, cancel),
        deadline,
        cancel,
    )? {
        return Err(ExecutionError::CleanupUncertain.into());
    }

    let work = (|| -> Result<_, SecurityError> {
        let source_volume = create_volume(
            gateway,
            &source_volume_name,
            &operation_id,
            deadline,
            cancel,
        )?;
        let vendor_volume = create_volume(
            gateway,
            &vendor_volume_name,
            &operation_id,
            deadline,
            cancel,
        )?;
        let policy_volume = create_volume(
            gateway,
            &policy_volume_name,
            &operation_id,
            deadline,
            cancel,
        )?;
        let junit_volume = if final_phase == SecurityPhase::Miri {
            Some(create_volume(
                gateway,
                &junit_volume_name,
                &operation_id,
                deadline,
                cancel,
            )?)
        } else {
            None
        };
        let volumes = SecurityVolumes {
            source: &source_volume,
            vendor: &vendor_volume,
            policy: &policy_volume,
            junit: junit_volume.as_ref(),
        };

        for (name, phase) in [
            (&source_guardian, SecurityPhase::SourceGuardian),
            (&vendor_guardian, SecurityPhase::VendorGuardian),
            (&policy_guardian, SecurityPhase::PolicyGuardian),
        ] {
            start_guardian(
                gateway,
                name,
                &operation_id,
                &volumes,
                phase,
                deadline,
                cancel,
            )?;
        }
        let mut guardians = vec![
            &source_guardian[..],
            &vendor_guardian[..],
            &policy_guardian[..],
        ];
        if final_phase == SecurityPhase::Miri {
            start_guardian(
                gateway,
                &junit_guardian,
                &operation_id,
                &volumes,
                SecurityPhase::MiriOutputGuardian,
                deadline,
                cancel,
            )?;
            guardians.push(&junit_guardian);
        }

        ingest(
            gateway,
            SecurityIngest {
                name: &source_ingest,
                operation_id: &operation_id,
                volumes: &volumes,
                phase: SecurityPhase::SourceIngest,
                deadline,
                output_limit: limits.output_bytes(),
            },
            &source_archive,
            cancel,
        )?;
        revalidate(
            gateway,
            &guardians,
            &[&source_ingest],
            &operation_id,
            deadline,
            cancel,
        )?;
        ingest(
            gateway,
            SecurityIngest {
                name: &vendor_ingest,
                operation_id: &operation_id,
                volumes: &volumes,
                phase: SecurityPhase::VendorIngest,
                deadline,
                output_limit: limits.output_bytes(),
            },
            &vendor_archive,
            cancel,
        )?;
        revalidate(
            gateway,
            &guardians,
            &[&source_ingest, &vendor_ingest],
            &operation_id,
            deadline,
            cancel,
        )?;
        ingest(
            gateway,
            SecurityIngest {
                name: &policy_ingest,
                operation_id: &operation_id,
                volumes: &volumes,
                phase: SecurityPhase::PolicyIngest,
                deadline,
                output_limit: limits.output_bytes(),
            },
            &initial_policy_archive,
            cancel,
        )?;
        revalidate(
            gateway,
            &guardians,
            &[&source_ingest, &vendor_ingest, &policy_ingest],
            &operation_id,
            deadline,
            cancel,
        )?;

        create_phase(
            gateway,
            &metadata_run,
            &operation_id,
            &volumes,
            SecurityPhase::Metadata,
            deadline,
            cancel,
        )?;
        let metadata_capture = phase_result(
            start_attached(
                gateway,
                &metadata_run,
                false,
                &[],
                deadline,
                METADATA_OUTPUT,
                cancel,
            ),
            deadline,
            cancel,
        )?;
        finish_phase(
            gateway,
            &metadata_run,
            &operation_id,
            &metadata_capture,
            deadline,
            cancel,
        )?;
        if metadata_capture.code != Some(0) {
            return Err(
                if crate::resolution_gateway::missing_offline_data(&metadata_capture) {
                    SecurityError::MissingOfflineData
                } else {
                    SecurityError::InvalidMetadata
                },
            );
        }
        let metadata = crate::security_metadata::prepare(&metadata_capture.stdout, source, vendor)?;
        if final_phase == SecurityPhase::Miri {
            crate::miri_admission::validate(&metadata, source, vendor)?;
        }
        budget_error(deadline, cancel)?;
        let scan_plan = if final_phase == SecurityPhase::UnsafeScan {
            Some(crate::unsafe_scan::plan(source, vendor, &metadata)?)
        } else {
            None
        };
        let (manifest_path, manifest_bytes) = if let Some(plan) = &scan_plan {
            (
                "scan.json",
                plan.manifest_bytes(scanner_budget_ms(
                    u64::try_from(
                        deadline
                            .saturating_duration_since(Instant::now())
                            .as_millis(),
                    )
                    .map_err(|_| SecurityError::OutputLimit)?,
                )?)?,
            )
        } else {
            ("metadata.json", metadata.derived.clone())
        };
        let manifest_fingerprint = bytes_fingerprint(&manifest_bytes)?;
        let derived_archive = metadata_archive(manifest_path, &manifest_bytes)?;
        budget_error(deadline, cancel)?;
        revalidate(
            gateway,
            &guardians,
            &[
                &source_ingest,
                &vendor_ingest,
                &policy_ingest,
                &metadata_run,
            ],
            &operation_id,
            deadline,
            cancel,
        )?;
        ingest(
            gateway,
            SecurityIngest {
                name: &metadata_ingest,
                operation_id: &operation_id,
                volumes: &volumes,
                phase: SecurityPhase::MetadataIngest,
                deadline,
                output_limit: limits.output_bytes(),
            },
            &derived_archive,
            cancel,
        )?;
        revalidate(
            gateway,
            &guardians,
            &[
                &source_ingest,
                &vendor_ingest,
                &policy_ingest,
                &metadata_run,
                &metadata_ingest,
            ],
            &operation_id,
            deadline,
            cancel,
        )?;

        create_phase(
            gateway,
            &deny_run,
            &operation_id,
            &volumes,
            final_phase,
            deadline,
            cancel,
        )?;
        let execution_deadline = if final_phase == SecurityPhase::Miri {
            deadline
                .checked_sub(Duration::from_secs(10))
                .filter(|d| *d > Instant::now())
                .ok_or(SecurityError::Timeout)?
        } else {
            deadline
        };
        let deny_capture = phase_result(
            start_attached(
                gateway,
                &deny_run,
                false,
                &[],
                execution_deadline,
                limits.output_bytes(),
                cancel,
            ),
            deadline,
            cancel,
        )?;
        finish_phase(
            gateway,
            &deny_run,
            &operation_id,
            &deny_capture,
            deadline,
            cancel,
        )?;
        revalidate(
            gateway,
            &guardians,
            &[
                &source_ingest,
                &vendor_ingest,
                &policy_ingest,
                &metadata_run,
                &metadata_ingest,
                &deny_run,
            ],
            &operation_id,
            deadline,
            cancel,
        )?;
        let junit = if final_phase == SecurityPhase::Miri {
            create_phase(
                gateway,
                &junit_export,
                &operation_id,
                &volumes,
                SecurityPhase::MiriExport,
                deadline,
                cancel,
            )?;
            let exported = phase_result(
                start_attached(
                    gateway,
                    &junit_export,
                    false,
                    &[],
                    deadline,
                    1024 * 1024,
                    cancel,
                ),
                deadline,
                cancel,
            )?;
            finish_phase(
                gateway,
                &junit_export,
                &operation_id,
                &exported,
                deadline,
                cancel,
            )?;
            decode_miri_junit_export(&exported, deny_capture.code)?
        } else {
            None
        };
        Ok((
            metadata,
            derived_archive,
            deny_capture,
            scan_plan,
            manifest_fingerprint,
            junit,
        ))
    })();

    let cleanup_deadline = Instant::now() + CLEANUP;
    let source_cleanup = cleanup_until(
        gateway,
        &all_names,
        &source_volume_name,
        &operation_id,
        cleanup_deadline,
    );
    let vendor_cleanup = cleanup_until(
        gateway,
        &[],
        &vendor_volume_name,
        &operation_id,
        cleanup_deadline,
    );
    let policy_cleanup = cleanup_until(
        gateway,
        &[],
        &policy_volume_name,
        &operation_id,
        cleanup_deadline,
    );
    let junit_cleanup = if final_phase == SecurityPhase::Miri {
        cleanup_until(
            gateway,
            &[],
            &junit_volume_name,
            &operation_id,
            cleanup_deadline,
        )
    } else {
        Ok(())
    };
    source_cleanup?;
    vendor_cleanup?;
    policy_cleanup?;
    junit_cleanup?;
    let (metadata, derived_archive, capture, scan_plan, manifest_fingerprint, junit) = work?;
    budget_error(deadline, cancel)?;
    finalize_security_execution(
        gateway,
        SecurityFinalizationInputs {
            source_archive: &source_archive,
            vendor_archive: &vendor_archive,
            policy_archive: &initial_policy_archive,
            deny_config: deny_config.as_deref().unwrap_or_default(),
            vendor_fingerprint: &vendor.tree_fingerprint,
            policy,
            final_phase,
            limits,
            work: CompletedSecurityWork {
                metadata,
                derived_archive,
                capture,
                scan_plan,
                manifest_fingerprint,
                junit,
            },
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn source_fingerprint(value: u8) -> Result<SourceFingerprint, String> {
        format!("sha256:{value:064x}")
            .parse()
            .map_err(|error| format!("{error:?}"))
    }

    fn execution_fingerprint_value(value: u8) -> Result<ExecutionFingerprint, String> {
        format!("sha256:{value:064x}")
            .parse()
            .map_err(|error| format!("{error:?}"))
    }

    fn capture(code: Option<i32>, stop: Stop) -> Capture {
        Capture {
            code,
            stdout: Vec::new(),
            stderr: Vec::new(),
            stdout_truncated: false,
            stderr_truncated: false,
            stop,
            duration_ms: 1,
        }
    }

    fn metadata() -> Result<crate::security_metadata::PreparedSecurityMetadata, String> {
        Ok(crate::security_metadata::PreparedSecurityMetadata {
            derived: br#"{"packages":[]}"#.to_vec(),
            packages: Vec::new(),
            package_roots: Vec::new(),
            original_fingerprint: source_fingerprint(1)?,
            derived_fingerprint: source_fingerprint(2)?,
            declared_licenses: Vec::new(),
            license_files: Vec::new(),
            enabled_features: Vec::new(),
            dependency_indices: Vec::new(),
            workspace_members: Vec::new(),
            lock_fingerprint: source_fingerprint(3)?,
        })
    }

    fn policy() -> Result<SecurityPolicy, String> {
        use rust_engineering_domain::security::{
            SecurityLint, SecurityPolicyDocument, SecurityRules,
        };
        SecurityPolicy::new(
            SecurityPolicyDocument {
                schema_version: 1,
                rules: SecurityRules {
                    allowed_licenses: vec!["MIT".into()],
                    banned_packages: Vec::new(),
                    multiple_versions: SecurityLint::Deny,
                    wildcards: SecurityLint::Deny,
                },
                suppressions: Vec::new(),
            },
            source_fingerprint(4)?,
            source_fingerprint(5)?,
            100,
        )
        .map_err(|error| format!("{error:?}"))
    }

    #[test]
    fn scanner_reserves_control_and_cleanup_before_parser_budget() {
        assert_eq!(scanner_budget_ms(25_000), Err(SecurityError::Timeout));
        assert_eq!(scanner_budget_ms(25_001), Ok(1));
        assert_eq!(scanner_budget_ms(120_000), Ok(95_000));
        assert_eq!(scanner_budget_ms(u64::MAX), Ok(118_000));
    }

    fn volume(name: &str) -> MutationVolume {
        MutationVolume {
            name: name.into(),
            driver: "local".into(),
            scope: "local".into(),
            options: BTreeMap::from([
                ("device".into(), "tmpfs".into()),
                ("o".into(), VOLUME_OPTIONS.into()),
                ("type".into(), "tmpfs".into()),
            ]),
            labels: labels("fixture"),
            mountpoint: format!("/var/lib/docker/volumes/{name}/_data"),
            cluster_volume: None,
            status: None,
        }
    }

    #[test]
    fn cargo_and_deny_arguments_match_the_pinned_clap_contract() {
        assert_eq!(
            SecurityPhase::Metadata.arguments(),
            [
                "metadata",
                "--frozen",
                "--offline",
                "--format-version=1",
                "--manifest-path=/source/Cargo.toml",
            ]
        );
        assert_eq!(
            SecurityPhase::Deny.arguments(),
            [
                "--format=json",
                "--log-level=debug",
                "--color=never",
                "--offline",
                "--frozen",
                "--manifest-path=/source/Cargo.toml",
                "check",
                "--config=/security/deny.toml",
                "--metadata-path=/security/metadata.json",
                "licenses",
                "bans",
                "sources",
                "--disable-fetch",
            ]
        );
        assert!(!SecurityPhase::Deny.arguments().contains(&"advisories"));
    }

    #[test]
    fn phases_expose_the_literal_three_volume_access_matrix() {
        let cases = [
            (
                SecurityPhase::SourceGuardian,
                [true, false, false],
                [false, false, false],
            ),
            (
                SecurityPhase::VendorGuardian,
                [false, true, false],
                [false, false, false],
            ),
            (
                SecurityPhase::PolicyGuardian,
                [false, false, true],
                [false, false, false],
            ),
            (
                SecurityPhase::SourceIngest,
                [true, false, false],
                [true, false, false],
            ),
            (
                SecurityPhase::VendorIngest,
                [false, true, false],
                [false, true, false],
            ),
            (
                SecurityPhase::PolicyIngest,
                [false, false, true],
                [false, false, true],
            ),
            (
                SecurityPhase::Metadata,
                [true, true, true],
                [false, false, false],
            ),
            (
                SecurityPhase::MetadataIngest,
                [false, false, true],
                [false, false, true],
            ),
            (
                SecurityPhase::Deny,
                [true, true, true],
                [false, false, false],
            ),
        ];
        for (phase, mounted, writable) in cases {
            assert_eq!(
                [
                    phase.source_mounted(),
                    phase.vendor_mounted(),
                    phase.policy_mounted()
                ],
                mounted,
                "mounts for {phase:?}"
            );
            assert_eq!(
                [
                    phase.source_writable(),
                    phase.vendor_writable(),
                    phase.policy_writable()
                ],
                writable,
                "permissions for {phase:?}"
            );
        }
        assert!(SecurityPhase::MetadataIngest.interactive());
        assert!(!SecurityPhase::Metadata.interactive());
        assert!(!SecurityPhase::Deny.interactive());
    }

    #[test]
    fn mount_arguments_are_ordered_exact_and_read_only_for_both_engines() {
        let source = volume("source");
        let vendor = volume("vendor");
        let policy = volume("policy");
        let volumes = SecurityVolumes {
            source: &source,
            vendor: &vendor,
            policy: &policy,
            junit: None,
        };
        let readonly = [
            "--mount=type=volume,source=source,target=/source,volume-nocopy,volume-driver=local,readonly",
            "--mount=type=volume,source=vendor,target=/rust-mcp-vendor,volume-nocopy,volume-driver=local,readonly",
            "--mount=type=volume,source=policy,target=/security,volume-nocopy,volume-driver=local,readonly",
        ];
        assert_eq!(mount_arguments(SecurityPhase::Metadata, &volumes), readonly);
        assert_eq!(mount_arguments(SecurityPhase::Deny, &volumes), readonly);
        assert_eq!(
            mount_arguments(SecurityPhase::UnsafeScan, &volumes),
            readonly
        );
        assert_eq!(
            SecurityPhase::UnsafeScan.program(),
            "/opt/security/bin/rust-mcp-unsafe-helper"
        );
        assert!(SecurityPhase::UnsafeScan.arguments().is_empty());
        assert!(!SecurityPhase::UnsafeScan.interactive());
        assert_eq!(
            mount_arguments(SecurityPhase::MetadataIngest, &volumes),
            [
                "--mount=type=volume,source=policy,target=/security,volume-nocopy,volume-driver=local"
            ]
        );
    }

    #[test]
    fn complete_container_arguments_cover_every_phase_without_a_runtime() -> Result<(), String> {
        let source = volume("source");
        let vendor = volume("vendor");
        let policy = volume("policy");
        let junit = volume("junit");
        let volumes = SecurityVolumes {
            source: &source,
            vendor: &vendor,
            policy: &policy,
            junit: Some(&junit),
        };
        for phase in [
            SecurityPhase::SourceGuardian,
            SecurityPhase::VendorGuardian,
            SecurityPhase::PolicyGuardian,
            SecurityPhase::SourceIngest,
            SecurityPhase::VendorIngest,
            SecurityPhase::PolicyIngest,
            SecurityPhase::Metadata,
            SecurityPhase::MetadataIngest,
            SecurityPhase::Deny,
            SecurityPhase::UnsafeScan,
            SecurityPhase::Miri,
            SecurityPhase::MiriOutputGuardian,
            SecurityPhase::MiriExport,
        ] {
            let arguments = create_arguments_for_runtime(
                crate::APPROVED_M4_IMAGE,
                std::path::Path::new("/state"),
                "container",
                "fixture",
                &volumes,
                phase,
            )
            .map_err(|error| format!("{error:?}"))?;
            assert!(arguments.contains(&"--name=container".to_owned()));
            assert!(arguments.contains(&"--label=org.rust-mcp.execution=true".to_owned()));
            assert!(arguments.contains(&"--label=org.rust-mcp.rust-job=fixture".to_owned()));
            assert!(arguments.contains(&format!("--entrypoint={}", phase.program())));
            assert!(arguments.contains(&crate::APPROVED_M4_IMAGE.to_owned()));
            assert_eq!(
                arguments.contains(&"--interactive".to_owned()),
                phase.interactive()
            );
            let profile = std::path::Path::new("/state").join(phase.profile_file());
            assert!(arguments.contains(&format!("--security-opt=seccomp={}", profile.display())));
            for argument in phase.arguments() {
                assert!(
                    arguments.contains(&(*argument).to_owned()),
                    "{phase:?}: {argument}"
                );
            }
        }
        let without_junit = create_arguments_for_runtime(
            crate::APPROVED_M4_IMAGE,
            std::path::Path::new("/state"),
            "container",
            "fixture",
            &SecurityVolumes {
                junit: None,
                ..volumes
            },
            SecurityPhase::MiriOutputGuardian,
        )
        .map_err(|error| format!("{error:?}"))?;
        assert!(
            without_junit
                .iter()
                .all(|argument| !argument.contains("target=/junit"))
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn container_arguments_reject_a_non_utf8_profile_path() {
        use std::os::unix::ffi::OsStrExt;
        let source = volume("source");
        let vendor = volume("vendor");
        let policy = volume("policy");
        let volumes = SecurityVolumes {
            source: &source,
            vendor: &vendor,
            policy: &policy,
            junit: None,
        };
        let path = std::path::Path::new(std::ffi::OsStr::from_bytes(b"/state/\xff"));
        assert_eq!(
            create_arguments_for_runtime(
                crate::APPROVED_M4_IMAGE,
                path,
                "container",
                "fixture",
                &volumes,
                SecurityPhase::Deny,
            ),
            Err(ExecutionError::InvalidConfiguration)
        );
    }

    #[test]
    fn environment_changes_only_cargo_home_from_the_stable_runtime() {
        let expected = [
            "CARGO_HOME=/security/cargo-home",
            "CARGO_INCREMENTAL=0",
            "CARGO_NET_OFFLINE=true",
            "CARGO_TARGET_DIR=/work/target",
            "HOME=/work",
            "PATH=/opt/rust/bin:/usr/bin:/bin",
            "RUSTC=/opt/rust/bin/rustc",
            "RUSTDOC=/opt/rust/bin/rustdoc",
            "RUSTFMT=/opt/rust/bin/rustfmt",
            "TMPDIR=/tmp",
        ]
        .map(str::to_owned)
        .to_vec();
        for phase in [
            SecurityPhase::Metadata,
            SecurityPhase::Deny,
            SecurityPhase::PolicyIngest,
        ] {
            assert_eq!(phase.environment(), expected, "environment for {phase:?}");
        }
    }

    #[test]
    fn phase_error_mapping_preserves_cleanup_and_distinguishes_deadline_from_cancel() {
        let future = Instant::now() + Duration::from_secs(1);
        let past = Instant::now() - Duration::from_millis(1);
        assert_eq!(
            phase_result::<()>(Err(ExecutionError::Infrastructure), past, &NeverCancel),
            Err(SecurityError::Timeout)
        );
        assert!(matches!(
            phase_result::<()>(Err(ExecutionError::CleanupUncertain), past, &NeverCancel),
            Err(SecurityError::Inspection(
                rust_engineering_application::InspectionError::Execution(
                    ExecutionError::CleanupUncertain
                )
            ))
        ));
        struct Cancel;
        impl ExecutionCancellation for Cancel {
            fn is_cancelled(&self) -> bool {
                true
            }
        }
        assert!(matches!(
            phase_result::<()>(Err(ExecutionError::Infrastructure), future, &Cancel),
            Err(SecurityError::Inspection(
                rust_engineering_application::InspectionError::Project(
                    rust_engineering_application::ProjectError::Cancelled
                )
            ))
        ));
        assert_eq!(phase_result::<u8>(Ok(7), future, &NeverCancel), Ok(7));
        assert_eq!(
            phase_result::<u8>(Ok(7), past, &NeverCancel),
            Err(SecurityError::Timeout)
        );
        assert!(matches!(
            phase_result::<u8>(Ok(7), future, &Cancel),
            Err(SecurityError::Inspection(
                rust_engineering_application::InspectionError::Project(
                    rust_engineering_application::ProjectError::Cancelled
                )
            ))
        ));
        assert!(matches!(
            phase_result::<()>(Err(ExecutionError::Cancelled), future, &NeverCancel),
            Err(SecurityError::Inspection(
                rust_engineering_application::InspectionError::Project(
                    rust_engineering_application::ProjectError::Cancelled
                )
            ))
        ));
    }

    #[test]
    fn capture_and_container_completion_are_validated_before_publication() -> Result<(), String> {
        assert!(matches!(
            validate_capture_completion(&capture(None, Stop::Cancelled)),
            Err(SecurityError::Inspection(
                rust_engineering_application::InspectionError::Project(
                    rust_engineering_application::ProjectError::Cancelled
                )
            ))
        ));
        assert_eq!(
            validate_capture_completion(&capture(None, Stop::TimedOut)),
            Err(SecurityError::Timeout)
        );
        assert_eq!(
            validate_capture_completion(&capture(None, Stop::OutputLimit)),
            Err(SecurityError::OutputLimit)
        );
        let mut truncated = capture(Some(0), Stop::Exited);
        truncated.stdout_truncated = true;
        assert_eq!(
            validate_capture_completion(&truncated),
            Err(SecurityError::OutputLimit)
        );
        truncated.stdout_truncated = false;
        truncated.stderr_truncated = true;
        assert_eq!(
            validate_capture_completion(&truncated),
            Err(SecurityError::OutputLimit)
        );
        let completed = capture(Some(7), Stop::Exited);
        assert_eq!(validate_capture_completion(&completed), Ok(()));

        let inspected_json = |running: bool, exit_code: i32, oom: bool| {
            serde_json::to_vec(&serde_json::json!([{
                "State": {
                    "Running": running,
                    "Pid": if running { 1 } else { 0 },
                    "ExitCode": exit_code,
                    "Status": if running { "running" } else { "exited" },
                    "StartedAt": "2026-09-08T12:00:00Z",
                    "Error": "",
                    "OOMKilled": oom
                }
            }]))
            .map_err(|error| error.to_string())
        };
        let mut inspected = capture(Some(0), Stop::Exited);
        inspected.stdout = inspected_json(false, 7, false)?;
        assert_eq!(validate_completed_container(&inspected, &completed), Ok(()));
        inspected.stdout = b"not json".to_vec();
        assert!(validate_completed_container(&inspected, &completed).is_err());
        inspected.stdout = b"[]".to_vec();
        assert!(validate_completed_container(&inspected, &completed).is_err());
        inspected.stdout = inspected_json(false, 7, false)?;
        inspected.code = Some(1);
        assert!(validate_completed_container(&inspected, &completed).is_err());
        inspected.code = Some(0);
        inspected.stdout = inspected_json(true, 7, false)?;
        assert!(validate_completed_container(&inspected, &completed).is_err());
        inspected.stdout = inspected_json(false, 8, false)?;
        assert!(validate_completed_container(&inspected, &completed).is_err());
        inspected.stdout = inspected_json(false, 7, true)?;
        assert!(validate_completed_container(&inspected, &completed).is_err());
        Ok(())
    }

    #[test]
    fn operation_selection_and_vendor_identity_fail_closed_without_a_runtime() -> Result<(), String>
    {
        let policy = policy()?;
        assert!(matches!(
            operation_policy_and_phase(SecurityOperation::Deny(&policy)),
            (Some(_), SecurityPhase::Deny)
        ));
        assert_eq!(
            operation_policy_and_phase(SecurityOperation::UnsafeScan).1,
            SecurityPhase::UnsafeScan
        );
        assert_eq!(
            operation_policy_and_phase(SecurityOperation::Miri).1,
            SecurityPhase::Miri
        );
        let source = SourceBundle::new(Vec::new()).map_err(|error| format!("{error:?}"))?;
        let expected = crate::resolution_gateway::tree_fingerprint(&source)
            .map_err(|error| format!("{error:?}"))?;
        let mut vendor = CargoVendorSnapshot {
            source,
            tree_fingerprint: expected,
            packages: Vec::new(),
        };
        assert_eq!(validate_vendor(&vendor), Ok(()));
        vendor.tree_fingerprint = source_fingerprint(99)?;
        assert_eq!(
            validate_vendor(&vendor),
            Err(SecurityError::MissingOfflineData)
        );
        Ok(())
    }

    #[test]
    fn fingerprints_bind_runtime_phase_inputs_and_normalizers() -> Result<(), String> {
        let metadata_value = metadata()?;
        let capture_value = capture(Some(0), Stop::Exited);
        let limits = ExecutionLimits::new_job(120_000, 1024 * 1024)
            .ok_or_else(|| "invalid fixture limits".to_owned())?;
        let configuration = execution_fingerprint_value(10)?;
        let vendor = source_fingerprint(11)?;
        let policy = policy()?;
        let inputs = |phase, policy, junit| SecurityFingerprintInputs {
            source_archive: b"source",
            vendor_archive: b"vendor",
            policy_archive: b"policy",
            metadata_archive: b"metadata",
            metadata: &metadata_value,
            policy,
            final_phase: phase,
            vendor_fingerprint: &vendor,
            deny_config: b"deny",
            limits,
            capture: &capture_value,
            junit,
        };
        let deny = execution_fingerprint_for_runtime(
            &configuration,
            crate::APPROVED_M4_IMAGE,
            std::path::Path::new("/state"),
            inputs(SecurityPhase::Deny, Some(&policy), None),
        )
        .map_err(|error| format!("{error:?}"))?;
        let repeated_deny = execution_fingerprint_for_runtime(
            &configuration,
            crate::APPROVED_M4_IMAGE,
            std::path::Path::new("/state"),
            inputs(SecurityPhase::Deny, Some(&policy), None),
        )
        .map_err(|error| format!("{error:?}"))?;
        assert_eq!(deny, repeated_deny);
        let scan = execution_fingerprint_for_runtime(
            &configuration,
            crate::APPROVED_M4_IMAGE,
            std::path::Path::new("/state"),
            inputs(SecurityPhase::UnsafeScan, None, None),
        )
        .map_err(|error| format!("{error:?}"))?;
        let miri = execution_fingerprint_for_runtime(
            &configuration,
            crate::APPROVED_M4_IMAGE,
            std::path::Path::new("/state"),
            inputs(SecurityPhase::Miri, None, Some(b"junit")),
        )
        .map_err(|error| format!("{error:?}"))?;
        assert_ne!(deny, scan);
        assert_ne!(scan, miri);
        assert_ne!(deny, miri);
        let changed_miri = execution_fingerprint_for_runtime(
            &configuration,
            crate::APPROVED_M4_IMAGE,
            std::path::Path::new("/state"),
            inputs(SecurityPhase::Miri, None, Some(b"changed")),
        )
        .map_err(|error| format!("{error:?}"))?;
        assert_ne!(miri, changed_miri);
        let source_digest = bytes_fingerprint(b"source").map_err(|error| format!("{error:?}"))?;
        assert_eq!(source_digest.to_string(), digest(b"source"));

        let finalized = finalize_security_execution_for_runtime(
            &configuration,
            crate::APPROVED_M4_IMAGE,
            std::path::Path::new("/state"),
            SecurityFinalizationInputs {
                source_archive: b"source",
                vendor_archive: b"vendor",
                policy_archive: b"policy",
                deny_config: b"deny",
                vendor_fingerprint: &vendor,
                policy: Some(&policy),
                final_phase: SecurityPhase::Deny,
                limits,
                work: CompletedSecurityWork {
                    metadata: metadata()?,
                    derived_archive: b"metadata".to_vec(),
                    capture: capture(Some(0), Stop::Exited),
                    scan_plan: None,
                    manifest_fingerprint: source_fingerprint(12)?,
                    junit: None,
                },
            },
        )
        .map_err(|error| format!("{error:?}"))?;
        assert_eq!(finalized.source_fingerprint, source_digest);
        let vendor_digest = bytes_fingerprint(b"vendor").map_err(|error| format!("{error:?}"))?;
        assert_eq!(finalized.vendor_archive_fingerprint, vendor_digest);
        assert_eq!(finalized.vendor_fingerprint, vendor);
        assert_eq!(
            finalized.policy_fingerprint,
            Some(policy.fingerprint().clone())
        );
        assert_eq!(finalized.manifest_fingerprint, source_fingerprint(12)?);
        assert_eq!(finalized.capture.code, Some(0));
        assert!(finalized.junit.is_none());
        Ok(())
    }

    #[test]
    fn miri_junit_export_accepts_one_bounded_file_and_classifies_missing_output()
    -> Result<(), String> {
        let junit_file = SourceFile::new("junit.xml".into(), b"<testsuites/>".to_vec())
            .map_err(|error| format!("{error:?}"))?;
        let junit_source =
            SourceBundle::new(vec![junit_file]).map_err(|error| format!("{error:?}"))?;
        let archive =
            crate::mutation_archive::encode(&junit_source).map_err(|error| format!("{error:?}"))?;
        let mut exported = capture(Some(0), Stop::Exited);
        exported.stdout = archive;
        let decoded =
            decode_miri_junit_export(&exported, Some(0)).map_err(|error| format!("{error:?}"))?;
        assert_eq!(decoded, Some(b"<testsuites/>".to_vec()));
        exported.stdout = b"invalid archive".to_vec();
        assert_eq!(
            decode_miri_junit_export(&exported, Some(0)),
            Err(SecurityError::InvalidMetadata)
        );
        exported.code = Some(1);
        assert_eq!(decode_miri_junit_export(&exported, Some(104)), Ok(None));
        assert_eq!(
            decode_miri_junit_export(&exported, Some(1)),
            Err(SecurityError::InvalidMetadata)
        );
        Ok(())
    }

    #[test]
    fn policy_and_metadata_are_separate_non_overwriting_ingests() -> Result<(), String> {
        let policy = policy_archive(Some(b"[licenses]\nallow = [\"MIT\"]\n"), false)
            .map_err(|error| format!("{error:?}"))?;
        let cargo_only = policy_archive(None, false).map_err(|error| format!("{error:?}"))?;
        let miri = policy_archive(None, true).map_err(|error| format!("{error:?}"))?;
        let metadata = metadata_archive("metadata.json", br#"{"version":1}"#)
            .map_err(|error| format!("{error:?}"))?;
        assert_ne!(digest(&policy), digest(&metadata));
        assert_ne!(digest(&cargo_only), digest(&miri));
        assert_eq!(
            SecurityPhase::PolicyIngest.arguments()[5],
            "--keep-old-files"
        );
        assert_eq!(
            SecurityPhase::MetadataIngest.arguments()[5],
            "--keep-old-files"
        );
        assert!(policy.windows("deny.toml".len()).any(|w| w == b"deny.toml"));
        assert!(
            policy
                .windows("cargo-home/config.toml".len())
                .any(|w| w == b"cargo-home/config.toml")
        );
        assert!(
            metadata
                .windows("metadata.json".len())
                .any(|w| w == b"metadata.json")
        );
        assert!(
            miri.windows("miri-nextest.toml".len())
                .any(|w| w == b"miri-nextest.toml")
        );
        assert_eq!(
            metadata_archive("../metadata.json", br#"{"version":1}"#),
            Err(SecurityError::InvalidMetadata)
        );
        Ok(())
    }
}
