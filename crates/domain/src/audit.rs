//! Bounded facts about a captured lockfile and a host-expected advisory snapshot.
use crate::{
    CatalogFingerprint, InspectionSemantics, ProjectIdentityFingerprint, ProjectRef,
    RuntimeIdentity, SnapshotEvidence, SourceFingerprint,
};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditDataError {
    Unavailable,
    InvalidSnapshot,
    Integrity,
    MissingLockfile,
    InvalidLockfile,
    Budget,
    Cancelled,
    Timeout,
    SandboxDenied,
    UnsupportedPlatform,
    Internal,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditState {
    Passed,
    Failed,
    Incomplete,
    Unavailable,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditIssue {
    SnapshotUnavailable,
    SnapshotStale,
    SnapshotUnknownAge,
    UnsupportedSources,
    OutputBudget,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditSource {
    CratesIo,
    Workspace,
    Unverified,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuditPackage {
    pub name: String,
    pub version: String,
    pub source: AuditSource,
    pub source_fingerprint: Option<SourceFingerprint>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditSeverity {
    None,
    Low,
    Medium,
    High,
    Critical,
}
#[derive(Clone, Debug, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuditPath {
    pub workspace_root: AuditPackage,
    /// Includes workspace root and affected package; represents one captured lock path.
    pub packages: Vec<AuditPackage>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuditFinding {
    pub advisory_id: String,
    pub url: String,
    pub title: String,
    pub package: AuditPackage,
    pub patched_requirements: Vec<String>,
    pub unaffected_requirements: Vec<String>,
    pub severity: Option<AuditSeverity>,
    pub informational: Option<String>,
    pub paths: Vec<AuditPath>,
    pub paths_omitted: u64,
}
#[derive(Clone, Debug, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuditObservation {
    pub state: AuditState,
    pub issue: Option<AuditIssue>,
    pub validation_complete: bool,
    pub lock_fingerprint: Option<SourceFingerprint>,
    pub snapshot_fingerprint: Option<CatalogFingerprint>,
    pub snapshot: Option<SnapshotEvidence>,
    pub snapshot_record_count: Option<u32>,
    pub snapshot_sequence: Option<u64>,
    pub packages_total: u32,
    pub crates_io_scanned: u32,
    pub workspace_packages_excluded: u32,
    pub unsupported_packages: Vec<AuditPackage>,
    pub findings: Vec<AuditFinding>,
    pub informational: Vec<AuditFinding>,
    pub findings_omitted: u64,
}
impl AuditObservation {
    pub fn unavailable() -> Self {
        Self {
            state: AuditState::Unavailable,
            issue: Some(AuditIssue::SnapshotUnavailable),
            validation_complete: false,
            lock_fingerprint: None,
            snapshot_fingerprint: None,
            snapshot: None,
            snapshot_record_count: None,
            snapshot_sequence: None,
            packages_total: 0,
            crates_io_scanned: 0,
            workspace_packages_excluded: 0,
            unsupported_packages: vec![],
            findings: vec![],
            informational: vec![],
            findings_omitted: 0,
        }
    }
}
#[derive(Clone, Debug)]
pub struct ProjectAudit {
    pub project_ref: ProjectRef,
    pub project_identity_fingerprint: ProjectIdentityFingerprint,
    pub semantics: InspectionSemantics,
    pub source_fingerprint: SourceFingerprint,
    pub runtime: RuntimeIdentity,
    pub observation: AuditObservation,
    pub evidence: SnapshotEvidence,
}

impl AuditObservation {
    /// Pure conservative coverage and freshness normalization shared by compound tools.
    pub fn normalize(&mut self) {
        use crate::{FreshnessState, IntegrityStatus, SourceKind};
        let observation = self;
        if observation.snapshot.as_ref().is_some_and(|snapshot| {
            snapshot.provenance().source_kind() != SourceKind::RustsecSnapshot
                || snapshot.provenance().integrity() != IntegrityStatus::Verified
                || snapshot.provenance().network_used()
        }) {
            // Invalid integrity is not missing data. The outer result supplies the
            // precise operational code; keep valid partial evidence without inventing
            // a SnapshotUnavailable issue for a snapshot that is actually present.
            observation.validation_complete = false;
            observation.state = AuditState::Incomplete;
            return;
        }
        let unavailable_issue =
            if observation.snapshot.is_none() || observation.snapshot_fingerprint.is_none() {
                Some(AuditIssue::SnapshotUnavailable)
            } else if let Some(snapshot) = &observation.snapshot {
                if snapshot.provenance().created_at().is_none()
                    || snapshot
                        .provenance()
                        .observed_at()
                        .is_none_or(|time| time > snapshot.freshness().assessed_at())
                    || matches!(
                        snapshot.freshness().state(),
                        FreshnessState::Unknown | FreshnessState::Live
                    )
                {
                    Some(AuditIssue::SnapshotUnknownAge)
                } else if snapshot.freshness().state() != FreshnessState::Fresh {
                    Some(AuditIssue::SnapshotStale)
                } else {
                    None
                }
            } else {
                None
            };
        if let Some(issue) = unavailable_issue.or(observation.issue.filter(|issue| {
            matches!(
                issue,
                AuditIssue::SnapshotUnavailable
                    | AuditIssue::SnapshotStale
                    | AuditIssue::SnapshotUnknownAge
            )
        })) {
            observation.validation_complete = false;
            observation.state = AuditState::Unavailable;
            observation.issue = Some(issue);
            return;
        }
        observation.validation_complete &= observation
            .snapshot_record_count
            .is_some_and(|count| count > 0)
            && observation
                .snapshot_sequence
                .is_some_and(|sequence| sequence > 0)
            && u64::from(observation.crates_io_scanned)
                + u64::from(observation.workspace_packages_excluded)
                + observation.unsupported_packages.len() as u64
                == u64::from(observation.packages_total)
            && observation.lock_fingerprint.is_some()
            && observation.unsupported_packages.is_empty()
            && observation.findings_omitted == 0
            && observation
                .findings
                .iter()
                .chain(&observation.informational)
                .all(|finding| finding.paths_omitted == 0)
            && observation.issue.is_none();
        // Fresh known vulnerabilities remain failures even when coverage is partial.
        // Absence of findings is only a pass when every independent prerequisite holds.
        if !observation.findings.is_empty() {
            observation.state = AuditState::Failed;
        } else if observation.state != AuditState::Passed || !observation.validation_complete {
            observation.validation_complete = false;
            observation.state = AuditState::Incomplete;
        }
    }
}
