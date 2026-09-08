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
                "--lib",
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
    nonce: &'a str,
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
    nonce: &str,
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
    for (key, value) in labels(nonce) {
        arguments.push(format!("--label={key}={value}"));
    }
    for value in phase.environment() {
        arguments.push(format!("--env={value}"));
    }
    let profile = gateway.inner.state.path().join(phase.profile_file());
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
    arguments.push(gateway.image_id().into());
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
    nonce: &str,
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
    for (key, value) in labels(nonce) {
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
    parse_volume(&inspected.stdout, name, nonce).map_err(Into::into)
}

fn create_phase(
    gateway: &RustGateway,
    name: &str,
    nonce: &str,
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
    let arguments = create_arguments(gateway, name, nonce, volumes, phase)?;
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
        nonce,
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
    match capture.stop {
        Stop::Cancelled => return Err(ExecutionError::Cancelled.into()),
        Stop::TimedOut => return Err(SecurityError::Timeout),
        Stop::OutputLimit => return Err(SecurityError::OutputLimit),
        Stop::Exited => {}
    }
    if capture.stdout_truncated || capture.stderr_truncated {
        return Err(SecurityError::OutputLimit);
    }
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
    nonce: &str,
    capture: &Capture,
    deadline: Instant,
    cancel: &dyn ExecutionCancellation,
) -> Result<(), SecurityError> {
    completed_without_oom(gateway, name, capture, deadline, cancel)?;
    phase_result(
        remove_if_present(gateway, name, nonce, deadline, cancel),
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
    nonce: &str,
    volumes: &SecurityVolumes<'_>,
    phase: SecurityPhase,
    deadline: Instant,
    cancel: &dyn ExecutionCancellation,
) -> Result<(), SecurityError> {
    create_phase(gateway, name, nonce, volumes, phase, deadline, cancel)?;
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
        running(gateway, name, nonce, deadline, cancel),
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
    nonce: &str,
    deadline: Instant,
    cancel: &dyn ExecutionCancellation,
) -> Result<(), SecurityError> {
    for guardian in guardians {
        if !phase_result(
            running(gateway, guardian, nonce, deadline, cancel),
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
        request.nonce,
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
        request.nonce,
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

fn execution_fingerprint(
    gateway: &RustGateway,
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
        labels: labels("<nonce>"),
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
        .map(|phase| create_arguments(gateway, "<container>", "<nonce>", &volumes, phase))
        .collect::<Result<Vec<_>, _>>()?;
    let bytes = serde_json::to_vec(&(
        gateway.configuration_fingerprint()?,
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
            digest(include_bytes!("seccomp-rust-quality.json")),
        ),
    ))
    .map_err(|_| ExecutionError::Infrastructure)?;
    digest(&bytes)
        .parse()
        .map_err(|_| ExecutionError::Infrastructure.into())
}

#[derive(Clone, Copy)]
enum SecurityOperation<'a> {
    Deny(&'a SecurityPolicy),
    UnsafeScan,
    Miri,
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

fn execute_operation(
    gateway: &RustGateway,
    source: &SourceBundle,
    vendor: &CargoVendorSnapshot,
    operation: SecurityOperation<'_>,
    limits: ExecutionLimits,
    cancel: &dyn ExecutionCancellation,
) -> Result<SecurityExecution, SecurityError> {
    let policy = if let SecurityOperation::Deny(policy) = operation {
        Some(policy)
    } else {
        None
    };
    let final_phase = match operation {
        SecurityOperation::Deny(_) => SecurityPhase::Deny,
        SecurityOperation::UnsafeScan => SecurityPhase::UnsafeScan,
        SecurityOperation::Miri => SecurityPhase::Miri,
    };
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
    if crate::resolution_gateway::tree_fingerprint(&vendor.source)
        .map_err(|_| SecurityError::MissingOfflineData)?
        != vendor.tree_fingerprint
    {
        return Err(SecurityError::MissingOfflineData);
    }
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

    let nonce = state::nonce()?;
    let source_volume_name = format!("rust-mcp-security-source-{nonce}");
    let vendor_volume_name = format!("rust-mcp-security-vendor-{nonce}");
    let policy_volume_name = format!("rust-mcp-security-policy-{nonce}");
    let source_guardian = format!("rust-mcp-security-source-guardian-{nonce}");
    let vendor_guardian = format!("rust-mcp-security-vendor-guardian-{nonce}");
    let policy_guardian = format!("rust-mcp-security-policy-guardian-{nonce}");
    let source_ingest = format!("rust-mcp-security-source-ingest-{nonce}");
    let vendor_ingest = format!("rust-mcp-security-vendor-ingest-{nonce}");
    let policy_ingest = format!("rust-mcp-security-policy-ingest-{nonce}");
    let metadata_run = format!("rust-mcp-security-metadata-{nonce}");
    let metadata_ingest = format!("rust-mcp-security-metadata-ingest-{nonce}");
    let deny_run = format!("rust-mcp-security-engine-{nonce}");
    let junit_volume_name = format!("rust-mcp-security-junit-{nonce}");
    let junit_guardian = format!("rust-mcp-security-junit-guardian-{nonce}");
    let junit_export = format!("rust-mcp-security-junit-export-{nonce}");
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
        let source_volume = create_volume(gateway, &source_volume_name, &nonce, deadline, cancel)?;
        let vendor_volume = create_volume(gateway, &vendor_volume_name, &nonce, deadline, cancel)?;
        let policy_volume = create_volume(gateway, &policy_volume_name, &nonce, deadline, cancel)?;
        let junit_volume = if final_phase == SecurityPhase::Miri {
            Some(create_volume(
                gateway,
                &junit_volume_name,
                &nonce,
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
            start_guardian(gateway, name, &nonce, &volumes, phase, deadline, cancel)?;
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
                &nonce,
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
                nonce: &nonce,
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
            &nonce,
            deadline,
            cancel,
        )?;
        ingest(
            gateway,
            SecurityIngest {
                name: &vendor_ingest,
                nonce: &nonce,
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
            &nonce,
            deadline,
            cancel,
        )?;
        ingest(
            gateway,
            SecurityIngest {
                name: &policy_ingest,
                nonce: &nonce,
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
            &nonce,
            deadline,
            cancel,
        )?;

        create_phase(
            gateway,
            &metadata_run,
            &nonce,
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
            &nonce,
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
                plan.manifest_bytes(
                    u64::try_from(
                        deadline
                            .saturating_duration_since(Instant::now())
                            .as_millis(),
                    )
                    .map_err(|_| SecurityError::Timeout)?
                    .checked_sub(4_000)
                    .filter(|v| *v > 0)
                    .ok_or(SecurityError::Timeout)?
                    .min(118_000),
                )?,
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
            &nonce,
            deadline,
            cancel,
        )?;
        ingest(
            gateway,
            SecurityIngest {
                name: &metadata_ingest,
                nonce: &nonce,
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
            &nonce,
            deadline,
            cancel,
        )?;

        create_phase(
            gateway,
            &deny_run,
            &nonce,
            &volumes,
            final_phase,
            deadline,
            cancel,
        )?;
        let deny_capture = phase_result(
            start_attached(
                gateway,
                &deny_run,
                false,
                &[],
                deadline,
                limits.output_bytes(),
                cancel,
            ),
            deadline,
            cancel,
        )?;
        finish_phase(gateway, &deny_run, &nonce, &deny_capture, deadline, cancel)?;
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
            &nonce,
            deadline,
            cancel,
        )?;
        let junit = if final_phase == SecurityPhase::Miri {
            create_phase(
                gateway,
                &junit_export,
                &nonce,
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
            finish_phase(gateway, &junit_export, &nonce, &exported, deadline, cancel)?;
            if exported.code == Some(0) {
                Some(
                    crate::nextest_gateway::decode_single_file_tar(
                        &exported.stdout,
                        512 * 1024,
                        "junit.xml",
                    )
                    .ok_or(SecurityError::InvalidMetadata)?,
                )
            } else if deny_capture.code == Some(104) {
                None
            } else {
                return Err(SecurityError::InvalidMetadata);
            }
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
        &nonce,
        cleanup_deadline,
    );
    let vendor_cleanup = cleanup_until(gateway, &[], &vendor_volume_name, &nonce, cleanup_deadline);
    let policy_cleanup = cleanup_until(gateway, &[], &policy_volume_name, &nonce, cleanup_deadline);
    let junit_cleanup = if final_phase == SecurityPhase::Miri {
        cleanup_until(gateway, &[], &junit_volume_name, &nonce, cleanup_deadline)
    } else {
        Ok(())
    };
    source_cleanup?;
    vendor_cleanup?;
    policy_cleanup?;
    junit_cleanup?;
    let (metadata, derived_archive, capture, scan_plan, manifest_fingerprint, junit) = work?;
    budget_error(deadline, cancel)?;

    let source_fingerprint = bytes_fingerprint(&source_archive)?;
    let vendor_archive_fingerprint = bytes_fingerprint(&vendor_archive)?;
    let deny_config_fingerprint = bytes_fingerprint(deny_config.as_deref().unwrap_or_default())?;
    let cargo_config_fingerprint =
        bytes_fingerprint(crate::security_policy::SECURITY_CARGO_CONFIG)?;
    let fingerprint = execution_fingerprint(
        gateway,
        SecurityFingerprintInputs {
            source_archive: &source_archive,
            vendor_archive: &vendor_archive,
            policy_archive: &initial_policy_archive,
            metadata_archive: &derived_archive,
            metadata: &metadata,
            policy,
            final_phase,
            vendor_fingerprint: &vendor.tree_fingerprint,
            deny_config: deny_config.as_deref().unwrap_or_default(),
            limits,
            capture: &capture,
            junit: junit.as_deref(),
        },
    )?;
    Ok(SecurityExecution {
        metadata,
        capture,
        execution_fingerprint: fingerprint,
        source_fingerprint,
        vendor_fingerprint: vendor.tree_fingerprint.clone(),
        vendor_archive_fingerprint,
        policy_fingerprint: policy.map(|p| p.fingerprint().clone()),
        scan_plan,
        manifest_fingerprint,
        junit,
        deny_config_fingerprint,
        cargo_config_fingerprint,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

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
    }

    #[test]
    fn policy_and_metadata_are_separate_non_overwriting_ingests() -> Result<(), String> {
        let policy = policy_archive(Some(b"[licenses]\nallow = [\"MIT\"]\n"), false)
            .map_err(|error| format!("{error:?}"))?;
        let metadata = metadata_archive("metadata.json", br#"{"version":1}"#)
            .map_err(|error| format!("{error:?}"))?;
        assert_ne!(digest(&policy), digest(&metadata));
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
        Ok(())
    }
}
