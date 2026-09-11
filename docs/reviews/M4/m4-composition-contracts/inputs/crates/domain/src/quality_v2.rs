//! New composition contract; the M1 profiles and stages remain frozen.
use crate::coverage::CoverageMetrics;
use crate::mutation_test::{MutationBaseline, MutationCounts};
use crate::semver_check::{SemverFindingCompleteness, SemverFindingCounts};
use crate::supply_chain::{SupplyAudit, SupplyDeny};
use crate::{
    ExecutionFingerprint, ExecutionTermination, ProjectIdentityFingerprint, ProjectRef,
    QualityIssue, RuntimeIdentity, SourceFingerprint, ToolStatus,
};
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityV2Profile {
    Strict,
    Release,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityV2StageKind {
    Format,
    Check,
    Clippy,
    Test,
    Audit,
    Deny,
    Coverage,
    Semver,
    Mutation,
}
impl QualityV2Profile {
    pub fn stages(self, mutation: bool) -> Vec<QualityV2StageKind> {
        use QualityV2StageKind::*;
        let mut stages = vec![Format, Check, Clippy, Test, Audit, Deny, Coverage];
        if self == Self::Release {
            stages.push(Semver);
        }
        if mutation {
            stages.push(Mutation);
        }
        stages
    }
}
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum QualityV2Details {
    Validation {
        termination: ExecutionTermination,
        exit_code: Option<i32>,
        validation_complete: bool,
        diagnostics: u32,
        diagnostics_omitted: u64,
        affected_files: Option<u64>,
        build_succeeded: Option<bool>,
    },
    Audit {
        observation: SupplyAudit,
    },
    Deny {
        observation: SupplyDeny,
    },
    Coverage {
        aggregate: CoverageMetrics,
        parse_complete: bool,
        exit_code: Option<i32>,
        doctests_run: bool,
    },
    Semver {
        counts: SemverFindingCounts,
        findings_completeness: SemverFindingCompleteness,
        exit_code: Option<i32>,
    },
    Mutation {
        baseline: MutationBaseline,
        counts: MutationCounts,
        validation_complete: bool,
        cap_exceeded: bool,
    },
}
#[derive(Clone, Debug, Serialize)]
pub struct QualityV2Stage {
    pub stage: QualityV2StageKind,
    pub status: ToolStatus,
    pub issue: Option<QualityIssue>,
    pub duration_ms: u64,
    pub execution_fingerprint: Option<ExecutionFingerprint>,
    pub details: Option<QualityV2Details>,
}
#[derive(Clone, Debug, Serialize)]
pub struct QualityV2Baseline {
    pub project_ref: ProjectRef,
    pub identity_fingerprint: ProjectIdentityFingerprint,
    pub source_fingerprint: SourceFingerprint,
}
#[derive(Clone, Debug, Serialize)]
pub struct QualityV2Report {
    pub profile: QualityV2Profile,
    pub mutation_requested: bool,
    pub source_fingerprint: SourceFingerprint,
    pub baseline: Option<QualityV2Baseline>,
    pub stages: Vec<QualityV2Stage>,
    pub complete: bool,
    pub status: ToolStatus,
}
impl QualityV2Report {
    pub fn refresh(&mut self) {
        self.complete = self.stages.iter().all(QualityV2Stage::evidence_complete);
        self.status = [
            ToolStatus::Cancelled,
            ToolStatus::Blocked,
            ToolStatus::Unavailable,
            ToolStatus::Failed,
        ]
        .into_iter()
        .find(|status| self.stages.iter().any(|s| s.status == *status))
        .unwrap_or(ToolStatus::Passed);
    }
    pub fn validate(&self) -> bool {
        let expected = self.profile.stages(self.mutation_requested);
        let mut normalized = self.clone();
        normalized.refresh();
        self.stages.len() == expected.len()
            && self.stages.iter().zip(expected).all(|(a, b)| a.stage == b)
            && (self.profile == QualityV2Profile::Release) == self.baseline.is_some()
            && self.complete == normalized.complete
            && self.status == normalized.status
            && self.stages.iter().all(|s| {
                s.status != ToolStatus::Passed
                    || (s.details.is_some()
                        && s.execution_fingerprint.is_some()
                        && s.issue.is_none()
                        && s.evidence_complete())
            })
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct QualityV2Observation {
    pub report: QualityV2Report,
    pub runtime: RuntimeIdentity,
    pub execution_fingerprint: ExecutionFingerprint,
}

impl QualityV2Stage {
    pub fn evidence_complete(&self) -> bool {
        match &self.details {
            Some(QualityV2Details::Validation {
                validation_complete,
                diagnostics_omitted,
                termination,
                ..
            }) => {
                *validation_complete
                    && *diagnostics_omitted == 0
                    && *termination == ExecutionTermination::Exited
            }
            Some(QualityV2Details::Audit { observation }) => observation.validation_complete,
            Some(QualityV2Details::Deny { observation }) => observation.complete,
            Some(QualityV2Details::Coverage {
                aggregate,
                parse_complete,
                exit_code,
                ..
            }) => *parse_complete && *exit_code == Some(0) && aggregate.lines.is_some(),
            Some(QualityV2Details::Semver { .. }) => {
                matches!(self.status, ToolStatus::Passed | ToolStatus::Failed)
            }
            Some(QualityV2Details::Mutation {
                validation_complete,
                ..
            }) => *validation_complete,
            None => false,
        }
    }
}
impl QualityV2Report {
    pub fn trim_one(&mut self) -> bool {
        for stage in &mut self.stages {
            let removed = match &mut stage.details {
                Some(QualityV2Details::Audit { observation }) => {
                    if observation.findings.pop().is_some() {
                        observation.findings_omitted += 1;
                        observation.validation_complete = false;
                        true
                    } else {
                        false
                    }
                }
                Some(QualityV2Details::Deny { observation }) => {
                    if observation.findings.pop().is_some() {
                        observation.findings_omitted += 1;
                        observation.complete = false;
                        true
                    } else {
                        false
                    }
                }
                _ => false,
            };
            if removed {
                if stage.status == ToolStatus::Passed {
                    stage.status = ToolStatus::Blocked;
                    stage.issue = Some(QualityIssue::Incomplete);
                }
                self.refresh();
                return true;
            }
        }
        false
    }
}
