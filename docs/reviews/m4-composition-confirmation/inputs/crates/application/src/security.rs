//! Security engines consume one captured project; no protocol or runtime implementation.
use crate::{
    DependencyAuditPort, InspectionControl, ProjectInspectionPort, ProjectRegistry,
    ProjectSourceBackend, ReferenceGenerator, RegistryClock,
};
use crate::{ExecutionError, InspectionError, ProjectError};
use rust_engineering_domain::security::*;
use rust_engineering_domain::{
    AuditDataError, AuditObservation, AuditSource, CargoVendorSnapshot, Clock,
    ExecutionFingerprint, ExecutionTermination, ProjectIdentityFingerprint, ProjectRef,
    RuntimeIdentity, SourceBundle, SourceFingerprint, UnixSeconds,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecurityError {
    Inspection(InspectionError),
    Audit(AuditDataError),
    InvalidPolicy,
    InvalidMetadata,
    MissingOfflineData,
    OutputLimit,
    Timeout,
    ClassificationIntegrityUnsupported,
}
impl From<InspectionError> for SecurityError {
    fn from(value: InspectionError) -> Self {
        Self::Inspection(value)
    }
}
impl From<ExecutionError> for SecurityError {
    fn from(value: ExecutionError) -> Self {
        Self::Inspection(if value == ExecutionError::Cancelled {
            InspectionError::Project(ProjectError::Cancelled)
        } else {
            InspectionError::Execution(value)
        })
    }
}
impl From<ProjectError> for SecurityError {
    fn from(value: ProjectError) -> Self {
        Self::Inspection(InspectionError::Project(value))
    }
}
impl From<AuditDataError> for SecurityError {
    fn from(value: AuditDataError) -> Self {
        Self::Audit(value)
    }
}

#[derive(Clone, Debug)]
pub struct SecurityCapture {
    pub project_ref: ProjectRef,
    pub project_identity_fingerprint: ProjectIdentityFingerprint,
    pub captured_at: UnixSeconds,
    pub source: SourceBundle,
}
impl<B: ProjectSourceBackend, G: ReferenceGenerator, C: RegistryClock> ProjectRegistry<B, G, C> {
    pub fn capture_security(
        &mut self,
        reference: &ProjectRef,
        clock: &impl Clock,
        control: &dyn InspectionControl,
    ) -> Result<SecurityCapture, SecurityError> {
        let identity = self.resolve_inner(reference, control, false)?;
        let captured_at = clock.now();
        let source = self.source_inner(reference, control, false)?;
        control.check()?;
        Ok(SecurityCapture {
            project_ref: reference.clone(),
            project_identity_fingerprint: identity.fingerprint,
            captured_at,
            source,
        })
    }
}

#[derive(Clone, Debug, Default)]
pub struct SecurityArtifactStreams {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

#[derive(Clone, Debug)]
pub struct DenyObservation {
    pub source_fingerprint: SourceFingerprint,
    pub vendor_fingerprint: SourceFingerprint,
    pub vendor_archive_fingerprint: SourceFingerprint,
    pub policy_fingerprint: SourceFingerprint,
    pub deny_config_fingerprint: SourceFingerprint,
    pub cargo_config_fingerprint: SourceFingerprint,
    pub metadata_original_fingerprint: SourceFingerprint,
    pub metadata_derived_fingerprint: SourceFingerprint,
    pub lock_fingerprint: SourceFingerprint,
    pub runtime: RuntimeIdentity,
    pub execution_fingerprint: ExecutionFingerprint,
    pub packages: Vec<SecurityPackage>,
    pub declared_licenses: Vec<Option<String>>,
    pub license_files: Vec<Vec<(String, SourceFingerprint)>>,
    pub enabled_features: Vec<Vec<String>>,
    pub dependency_indices: Vec<Vec<usize>>,
    pub workspace_members: Vec<usize>,
    pub findings: Vec<SecurityFinding>,
    pub findings_omitted: u64,
    pub licenses: SecurityCounts,
    pub bans: SecurityCounts,
    pub sources: SecurityCounts,
    pub parse_complete: bool,
    pub termination: ExecutionTermination,
    pub exit_code: Option<i32>,
    pub artifacts: SecurityArtifactStreams,
}

impl DenyObservation {
    pub fn validate(&self) -> Result<(), SecurityError> {
        let count = self.packages.len();
        if count == 0
            || count > 4096
            || self.declared_licenses.len() != count
            || self.license_files.len() != count
            || self.enabled_features.len() != count
            || self.dependency_indices.len() != count
            || self.workspace_members.is_empty()
            || self.workspace_members.iter().any(|&i| i >= count)
            || self
                .dependency_indices
                .iter()
                .flatten()
                .any(|&i| i >= count)
            || self.findings.len() > SECURITY_MAX_FINDINGS
            || self
                .findings
                .iter()
                .any(|f| f.rule.len() > 96 || f.message.len() > 512)
            || self.runtime.execution_fingerprint != self.execution_fingerprint
            || (self.parse_complete
                && (self.termination != ExecutionTermination::Exited
                    || self.artifacts.stdout_truncated
                    || self.artifacts.stderr_truncated
                    || self.licenses.total() + self.bans.total() + self.sources.total()
                        != self.findings.len() as u64 + self.findings_omitted))
        {
            return Err(SecurityError::InvalidMetadata);
        }
        Ok(())
    }
}

/// Runs metadata and licenses/bans/sources over owned source/vendor/policy only.
/// It must neither audit nor acquire dependencies nor publish unredacted streams.
pub trait ProjectDenyPort: Send + Sync {
    fn deny(
        &self,
        source: &SourceBundle,
        vendor: &CargoVendorSnapshot,
        policy: &SecurityPolicy,
        options: &DenyOptions,
        control: &dyn InspectionControl,
    ) -> Result<DenyObservation, SecurityError>;
}

pub struct SecurityPorts<'a, E, A> {
    pub executor: &'a E,
    pub auditor: &'a A,
}

#[derive(Clone, Debug)]
pub struct SecurityObservation {
    /// The exact audit observation, unchanged, shared by every composition.
    pub audit: AuditObservation,
    pub deny: DenyObservation,
    pub findings: Vec<SecurityFinding>,
    pub findings_omitted: u64,
    pub completeness: SecurityCompleteness,
    pub policy_state: SecurityPolicyState,
    pub assessed_at: UnixSeconds,
}

/// Publication receives owned evidence and must revalidate the owner before
/// committing a descriptor. It must never retain the guest's original streams.
pub trait SecurityPublisher: Send {
    fn publish(
        &mut self,
        capture: &SecurityCapture,
        observation: &SecurityObservation,
        revalidate: &mut dyn FnMut() -> Result<crate::QualityOwnerFacts, InspectionError>,
    ) -> Result<rust_engineering_domain::QualityArtifactDescriptor, InspectionError>;
}

pub struct PublishedSecurity {
    pub observation: SecurityObservation,
    pub artifact: rust_engineering_domain::QualityArtifactDescriptor,
}

impl<B, G, C> ProjectRegistry<B, G, C>
where
    B: ProjectSourceBackend + crate::QualityProjectBackend,
    G: ReferenceGenerator,
    C: RegistryClock,
{
    #[allow(clippy::too_many_arguments)] // Explicit capture, runtime, policy and publication boundaries.
    pub fn deny_durable<E: ProjectInspectionPort + ProjectDenyPort, A: DependencyAuditPort>(
        &mut self,
        reference: &ProjectRef,
        vendor: &CargoVendorSnapshot,
        policy: &SecurityPolicy,
        options: &DenyOptions,
        ports: SecurityPorts<'_, E, A>,
        publisher: &mut dyn SecurityPublisher,
        clock: &impl Clock,
        control: &dyn InspectionControl,
    ) -> Result<PublishedSecurity, SecurityError> {
        let capture = self.capture_security(reference, clock, control)?;
        let mut observation = inspect_security(
            &capture.source,
            vendor,
            policy,
            options,
            ports,
            clock,
            control,
        )?;
        let mut revalidate = || {
            self.quality_owner_facts(reference, control)
                .map_err(InspectionError::from)
        };
        let artifact = publisher.publish(&capture, &observation, &mut revalidate)?;
        control.check()?;
        policy
            .validate_at(clock.now().0)
            .map_err(|_| SecurityError::InvalidPolicy)?;
        let identity = self.resolve_inner(reference, control, true)?;
        if identity.fingerprint != capture.project_identity_fingerprint {
            return Err(SecurityError::InvalidMetadata);
        }
        observation.deny.artifacts = SecurityArtifactStreams::default();
        Ok(PublishedSecurity {
            observation,
            artifact,
        })
    }
}

/// Standalone deny and compound profiles call this function with the SAME owned
/// source generation. One audit invocation is visible at this boundary and can
/// be counted independently. Publication still requires the caller's live lease.
pub fn inspect_security<E: ProjectInspectionPort + ProjectDenyPort, A: DependencyAuditPort>(
    source: &SourceBundle,
    vendor: &CargoVendorSnapshot,
    policy: &SecurityPolicy,
    options: &DenyOptions,
    ports: SecurityPorts<'_, E, A>,
    clock: &impl Clock,
    control: &dyn InspectionControl,
) -> Result<SecurityObservation, SecurityError> {
    if security_source_has_exceptions(source) {
        return Err(SecurityError::InvalidPolicy);
    }
    policy
        .validate_at(clock.now().0)
        .map_err(|_| SecurityError::InvalidPolicy)?;
    control.check()?;
    let structure = ports.executor.inspect(source, control)?;
    control.check()?;
    let audit = ports.auditor.audit(source, &structure, clock, control)?;
    control.check()?;
    let deny = ports
        .executor
        .deny(source, vendor, policy, options, control)?;
    control.check()?;
    compose_security(&structure, audit, deny, vendor, policy, clock)
}

/// Reuses an already observed audit; never calls an advisory matcher.
pub fn compose_security(
    structure: &rust_engineering_domain::ProjectStructure,
    audit: AuditObservation,
    mut deny: DenyObservation,
    vendor: &CargoVendorSnapshot,
    policy: &SecurityPolicy,
    clock: &impl Clock,
) -> Result<SecurityObservation, SecurityError> {
    deny.validate()?;
    let assessed_at = clock.now();
    policy
        .validate_at(assessed_at.0)
        .map_err(|_| SecurityError::InvalidPolicy)?;
    if deny.source_fingerprint != structure.source_fingerprint
        || !rust_engineering_domain::quality_runtime_matches(&deny.runtime, &structure.runtime)
        || deny.vendor_fingerprint != vendor.tree_fingerprint
        || &deny.policy_fingerprint != policy.fingerprint()
        || audit
            .lock_fingerprint
            .as_ref()
            .is_some_and(|f| f != &deny.lock_fingerprint)
    {
        return Err(SecurityError::InvalidMetadata);
    }
    let mut normalized = audit.clone();
    normalized.normalize();
    let mut completeness = if normalized.validation_complete
        && deny.parse_complete
        && deny.termination == ExecutionTermination::Exited
        && deny.findings_omitted == 0
        && audit.findings_omitted == 0
    {
        SecurityCompleteness::Complete
    } else {
        SecurityCompleteness::Partial
    };
    let mut findings = Vec::new();
    for (rows, severity) in [
        (&audit.findings, SecuritySeverity::Error),
        (&audit.informational, SecuritySeverity::Warning),
    ] {
        for finding in rows {
            findings.push(SecurityFinding {
                engine: SecurityEngine::Rustsec,
                rule: finding.advisory_id.clone(),
                package: Some(SecurityPackage {
                    name: finding.package.name.clone(),
                    version: finding.package.version.clone(),
                    source: match finding.package.source {
                        AuditSource::CratesIo => SecuritySource::CratesIo,
                        AuditSource::Workspace => SecuritySource::Workspace,
                        AuditSource::Unverified => SecuritySource::Unverified,
                    },
                    source_fingerprint: finding.package.source_fingerprint.clone(),
                }),
                severity,
                message: "RustSec advisory in the configured snapshot".into(),
                disposition: FindingDisposition::Active,
            });
        }
    }
    for row in &mut findings {
        row.apply_policy(policy, assessed_at.0);
    }
    for row in &mut deny.findings {
        row.apply_policy(policy, assessed_at.0);
    }
    findings.extend(deny.findings.iter().cloned());
    let omitted = findings.len().saturating_sub(SECURITY_MAX_FINDINGS) as u64;
    // Compute policy before bounding so omitted violations cannot disappear.
    if omitted > 0 {
        completeness = SecurityCompleteness::Partial;
    }
    let state = policy_state(completeness, &findings);
    findings.truncate(SECURITY_MAX_FINDINGS);
    Ok(SecurityObservation {
        findings_omitted: omitted
            .saturating_add(audit.findings_omitted)
            .saturating_add(deny.findings_omitted),
        audit,
        deny,
        findings,
        completeness,
        policy_state: state,
        assessed_at,
    })
}
