//! Independently sourced facts; no aggregate security score or legal verdict.
use crate::security::{SecurityFinding, SecurityPolicyState};
use crate::{
    AuditIssue, AuditSource, AuditState, CatalogFingerprint, ExecutionFingerprint, RuntimeIdentity,
    SnapshotEvidence, SourceFingerprint,
};
use serde::Serialize;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SupplySource {
    Workspace,
    CratesIo,
    Registry,
    Git,
    Unverified,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SupplyAvailability {
    Available,
    Partial,
    Unavailable,
    Invalid,
    NotConfigured,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum YankedFact {
    Yanked,
    NotYanked,
    CrateAbsent,
    VersionAbsent,
    CatalogUnavailable,
    NotApplicable,
    NotConsulted,
}
#[derive(Clone, Debug, Serialize)]
pub struct SupplyPackage {
    pub name: String,
    pub version: String,
    pub source: SupplySource,
    /// A source locator is never exposed; its literal bytes are bound by this hash.
    pub source_fingerprint: Option<SourceFingerprint>,
    pub declared_checksum: Option<SourceFingerprint>,
    pub checksum_verified: bool,
    pub duplicate_name: bool,
    pub declared_features: Option<Vec<String>>,
    pub active_features: Option<Vec<String>>,
    pub yanked: YankedFact,
}
#[derive(Clone, Debug, Serialize)]
pub struct SupplyGraph {
    pub source_fingerprint: SourceFingerprint,
    pub lock_fingerprint: SourceFingerprint,
    pub packages: Vec<SupplyPackage>,
}
#[derive(Clone, Debug, Serialize)]
pub struct SupplyCatalog {
    pub availability: SupplyAvailability,
    pub snapshot_fingerprint: Option<CatalogFingerprint>,
    pub bundle_fingerprint: Option<SourceFingerprint>,
    pub sequence: Option<u64>,
    pub evidence: Option<SnapshotEvidence>,
    pub lookups: u32,
}
#[derive(Clone, Debug, Serialize)]
pub struct SupplyDeny {
    pub complete: bool,
    pub policy_state: SecurityPolicyState,
    pub findings: Vec<SecurityFinding>,
    pub findings_omitted: u64,
    pub policy_fingerprint: SourceFingerprint,
    pub execution_fingerprint: ExecutionFingerprint,
}
#[derive(Clone, Debug, Serialize)]
pub struct SupplyReport {
    pub source_fingerprint: SourceFingerprint,
    pub lock_fingerprint: SourceFingerprint,
    pub packages: Vec<SupplyPackage>,
    pub packages_total: u32,
    pub packages_omitted: u32,
    pub audit_availability: SupplyAvailability,
    pub audit: Option<SupplyAudit>,
    pub deny_availability: SupplyAvailability,
    pub deny: Option<SupplyDeny>,
    pub catalog: SupplyCatalog,
    pub complete: bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct SupplyObservation {
    pub report: SupplyReport,
    pub runtime: RuntimeIdentity,
    pub execution_fingerprint: ExecutionFingerprint,
}

#[derive(Clone, Debug, Serialize)]
pub struct SupplyAdvisory {
    pub advisory_id: String,
    pub package_name: String,
    pub package_version: String,
    pub package_source: AuditSource,
    pub source_fingerprint: Option<SourceFingerprint>,
    pub informational: bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct SupplyAudit {
    pub state: AuditState,
    pub issue: Option<AuditIssue>,
    pub validation_complete: bool,
    pub lock_fingerprint: Option<SourceFingerprint>,
    pub snapshot_fingerprint: Option<CatalogFingerprint>,
    pub snapshot: Option<SnapshotEvidence>,
    pub snapshot_sequence: Option<u64>,
    pub packages_total: u32,
    pub crates_io_scanned: u32,
    pub workspace_packages_excluded: u32,
    pub unsupported_packages: u32,
    pub findings: Vec<SupplyAdvisory>,
    pub findings_omitted: u64,
}
impl From<&crate::AuditObservation> for SupplyAudit {
    fn from(audit: &crate::AuditObservation) -> Self {
        let findings = audit
            .findings
            .iter()
            .map(|a| (a, false))
            .chain(audit.informational.iter().map(|a| (a, true)))
            .take(128)
            .map(|(a, informational)| SupplyAdvisory {
                advisory_id: a.advisory_id.clone(),
                package_name: a.package.name.clone(),
                package_version: a.package.version.clone(),
                package_source: a.package.source,
                source_fingerprint: a.package.source_fingerprint.clone(),
                informational,
            })
            .collect::<Vec<_>>();
        let omitted = audit
            .findings
            .len()
            .saturating_add(audit.informational.len())
            .saturating_sub(findings.len()) as u64;
        Self {
            state: audit.state,
            issue: audit.issue,
            validation_complete: audit.validation_complete && omitted == 0,
            lock_fingerprint: audit.lock_fingerprint.clone(),
            snapshot_fingerprint: audit.snapshot_fingerprint.clone(),
            snapshot: audit.snapshot.clone(),
            snapshot_sequence: audit.snapshot_sequence,
            packages_total: audit.packages_total,
            crates_io_scanned: audit.crates_io_scanned,
            workspace_packages_excluded: audit.workspace_packages_excluded,
            unsupported_packages: audit.unsupported_packages.len() as u32,
            findings,
            findings_omitted: audit.findings_omitted.saturating_add(omitted),
        }
    }
}

impl SupplyReport {
    pub fn validate(&self) -> bool {
        self.packages_total <= 4096
            && self.packages.len() <= 128
            && self.packages.len() as u64 + u64::from(self.packages_omitted)
                == u64::from(self.packages_total)
            && self.catalog.lookups <= 128
            && self.audit.as_ref().is_none_or(|a| a.findings.len() <= 128)
            && self.deny.as_ref().is_none_or(|d| d.findings.len() <= 128)
            && (!self.complete
                || (self.packages_omitted == 0
                    && self.audit_availability == SupplyAvailability::Available
                    && self.deny_availability == SupplyAvailability::Available
                    && self.catalog.availability == SupplyAvailability::Available))
    }
    /// Removes whole rows only; known engine verdicts survive truncation.
    pub fn trim_one(&mut self) -> bool {
        let removed = if self.packages.pop().is_some() {
            self.packages_omitted += 1;
            true
        } else if let Some(audit) = &mut self.audit
            && audit.findings.pop().is_some()
        {
            audit.findings_omitted += 1;
            audit.validation_complete = false;
            true
        } else if let Some(deny) = &mut self.deny
            && deny.findings.pop().is_some()
        {
            deny.findings_omitted += 1;
            deny.complete = false;
            true
        } else {
            false
        };
        if removed {
            self.complete = false;
        }
        removed
    }
}
