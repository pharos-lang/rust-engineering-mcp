//! Closed profiles and per-stage facts for a single captured quality gate.
use crate::*;
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityProfile {
    Fast,
    Standard,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityStage {
    Format,
    Check,
    Clippy,
    Test,
    Audit,
}
impl QualityProfile {
    pub fn stages(self) -> &'static [QualityStage] {
        match self {
            Self::Fast => &[
                QualityStage::Format,
                QualityStage::Check,
                QualityStage::Clippy,
            ],
            Self::Standard => &[
                QualityStage::Format,
                QualityStage::Check,
                QualityStage::Clippy,
                QualityStage::Test,
                QualityStage::Audit,
            ],
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "code", rename_all = "snake_case")]
pub enum QualityIssue {
    Operational(OperationalErrorCode),
    Audit(AuditDataError),
    Incomplete,
}
impl QualityIssue {
    pub fn status(self) -> ToolStatus {
        match self {
            Self::Operational(code) => code.status(),
            Self::Audit(AuditDataError::Unavailable) => ToolStatus::Unavailable,
            Self::Audit(AuditDataError::Cancelled) => ToolStatus::Cancelled,
            _ => ToolStatus::Blocked,
        }
    }
}
#[derive(Clone, Debug)]
pub enum QualityObservation {
    Format(FormatObservation),
    Check(CheckObservation),
    Clippy(CheckObservation),
    Test(TestObservation),
    Audit {
        runtime: RuntimeIdentity,
        observation: AuditObservation,
    },
}
/// Commands have different execution fingerprints; only their runtime configuration
/// must agree for a combined captured-source verdict.
pub fn quality_runtime_matches(expected: &RuntimeIdentity, actual: &RuntimeIdentity) -> bool {
    expected.platform == actual.platform
        && expected.image_id == actual.image_id
        && expected.configuration_fingerprint == actual.configuration_fingerprint
        && expected.rust_version == actual.rust_version
        && expected.cargo_version == actual.cargo_version
        && expected.declared_toolchain == actual.declared_toolchain
}
impl QualityObservation {
    pub fn runtime(&self) -> &RuntimeIdentity {
        match self {
            Self::Format(o) => &o.execution.runtime,
            Self::Check(o) | Self::Clippy(o) => &o.runtime,
            Self::Test(o) => &o.execution.runtime,
            Self::Audit { runtime, .. } => runtime,
        }
    }

    pub fn execution(&self) -> Option<&CheckObservation> {
        match self {
            Self::Format(o) => Some(&o.execution),
            Self::Check(o) | Self::Clippy(o) => Some(o),
            Self::Test(o) => Some(&o.execution),
            Self::Audit { .. } => None,
        }
    }
    pub fn execution_mut(&mut self) -> Option<&mut CheckObservation> {
        match self {
            Self::Format(o) => Some(&mut o.execution),
            Self::Check(o) | Self::Clippy(o) => Some(o),
            Self::Test(o) => Some(&mut o.execution),
            Self::Audit { .. } => None,
        }
    }
}
#[derive(Clone, Debug)]
pub struct QualityStageReport {
    pub stage: QualityStage,
    pub duration_ms: u64,
    pub status: ToolStatus,
    pub issue: Option<QualityIssue>,
    pub observation: Option<QualityObservation>,
    pub log: Option<ArtifactMetadata>,
    pub retention_remaining_seconds: Option<u64>,
}
impl QualityStageReport {
    /// Re-derive success from complete facts; never trust a passed label alone.
    pub fn classify(&mut self) {
        if let Some(observation) = &mut self.observation {
            let matching = matches!(
                (self.stage, &*observation),
                (QualityStage::Format, QualityObservation::Format(_))
                    | (QualityStage::Check, QualityObservation::Check(_))
                    | (QualityStage::Clippy, QualityObservation::Clippy(_))
                    | (QualityStage::Test, QualityObservation::Test(_))
                    | (QualityStage::Audit, QualityObservation::Audit { .. })
            );
            if !matching {
                self.status = ToolStatus::Blocked;
                self.issue = Some(QualityIssue::Incomplete);
                return;
            }

            if let QualityObservation::Audit { observation, .. } = observation {
                observation.normalize();
                self.status = match observation.state {
                    AuditState::Passed => ToolStatus::Passed,
                    AuditState::Failed => ToolStatus::Failed,
                    AuditState::Unavailable => ToolStatus::Unavailable,
                    AuditState::Incomplete => ToolStatus::Blocked,
                };
                self.issue =
                    (self.status == ToolStatus::Blocked).then_some(QualityIssue::Incomplete);
                return;
            }
            let extra_complete = match observation {
                QualityObservation::Test(o) => {
                    o.execution.exit_code != Some(0) || o.build_succeeded == Some(true)
                }
                QualityObservation::Format(o) => {
                    o.execution.exit_code != Some(0)
                        || (o.affected_files.is_empty()
                            && o.diff.as_ref().is_none_or(|d| d.is_empty())
                            && o.affected_files_omitted == 0
                            && !o.diff_omitted)
                }
                _ => true,
            };
            if let Some(execution) = observation.execution() {
                let issue = if execution.termination == ExecutionTermination::Cancelled {
                    self.status = ToolStatus::Cancelled;
                    self.issue = None;
                    return;
                } else if execution.termination == ExecutionTermination::TimedOut {
                    Some(QualityIssue::Operational(
                        OperationalErrorCode::CommandTimeout,
                    ))
                } else if execution.outcome == CheckOutcome::LockfileUpdateRequired {
                    Some(QualityIssue::Operational(
                        OperationalErrorCode::LockfileUpdateRequired,
                    ))
                } else if !execution.validation_complete
                    || execution.stdout_truncated
                    || execution.stderr_truncated
                    || !extra_complete
                    || execution.termination != ExecutionTermination::Exited
                    || execution.diagnostics_omitted != 0
                {
                    Some(QualityIssue::Incomplete)
                } else if execution.outcome == CheckOutcome::Passed
                    && execution.exit_code == Some(0)
                {
                    None
                } else if execution.outcome == CheckOutcome::Failed
                    && execution.exit_code.is_some_and(|c| c != 0)
                {
                    self.status = ToolStatus::Failed;
                    self.issue = None;
                    return;
                } else {
                    Some(QualityIssue::Incomplete)
                };
                self.status = issue.map_or(ToolStatus::Passed, QualityIssue::status);
                self.issue = issue;
            }
        } else {
            self.status = self.issue.map_or(ToolStatus::Blocked, QualityIssue::status);
        }
    }
}
pub fn quality_status(profile: QualityProfile, stages: &[QualityStageReport]) -> ToolStatus {
    if stages.len() != profile.stages().len()
        || !stages
            .iter()
            .zip(profile.stages())
            .all(|(r, s)| r.stage == *s)
    {
        return ToolStatus::Blocked;
    }
    for state in [
        ToolStatus::Cancelled,
        ToolStatus::Blocked,
        ToolStatus::Unavailable,
        ToolStatus::Failed,
    ] {
        if stages.iter().any(|r| r.status == state) {
            return state;
        }
    }
    ToolStatus::Passed
}
#[derive(Clone, Debug)]
pub struct ProjectQualityGate {
    pub project_ref: ProjectRef,
    pub project_identity_fingerprint: ProjectIdentityFingerprint,
    pub semantics: InspectionSemantics,
    pub profile: QualityProfile,
    pub source_fingerprint: Option<SourceFingerprint>,
    pub stages: Vec<QualityStageReport>,
    pub evidence: Evidence,
}
