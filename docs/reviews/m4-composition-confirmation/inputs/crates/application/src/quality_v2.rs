//! Extended quality composition over exactly one candidate capture and one audit.
use crate::coverage::ProjectCoveragePort;
use crate::mutation_test::ProjectMutationTestPort;
use crate::security::{ProjectDenyPort, SecurityCapture, SecurityError, compose_security};
use crate::semver_check::{ProjectSemverPort, SemverOptions, SemverOutcome};
use crate::*;
use rust_engineering_domain::coverage::{CoverageOptions, CoverageSelection};
use rust_engineering_domain::mutation_test::MutationTestCommandOptions;
use rust_engineering_domain::quality_v2::*;
use rust_engineering_domain::security::*;
use rust_engineering_domain::semver_check::{SemverCommandOptions, SemverProjectSelection};
use rust_engineering_domain::supply_chain::{SupplyAudit, SupplyDeny};
use rust_engineering_domain::*;
use std::time::Instant;

pub struct QualityV2Options {
    pub profile: QualityV2Profile,
    pub baseline: Option<ProjectRef>,
    pub timeout_seconds: u64,
    pub mutation: Option<MutationTestCommandOptions>,
}
impl QualityV2Options {
    pub fn validate(&self) -> Result<(), InvalidCheckOptions> {
        if !(1..=3600).contains(&self.timeout_seconds)
            || (self.profile == QualityV2Profile::Release) != self.baseline.is_some()
            || self.mutation.as_ref().is_some_and(|m| {
                crate::mutation_test::total_budget_seconds(m).saturating_add(300)
                    > self.timeout_seconds
            })
        {
            return Err(InvalidCheckOptions);
        }
        Ok(())
    }
}
pub trait QualityV2Publisher: Send {
    fn publish_gate_v2(
        &mut self,
        capture: &SecurityCapture,
        observation: &QualityV2Observation,
        revalidate: &mut dyn FnMut() -> Result<QualityOwnerFacts, InspectionError>,
    ) -> Result<QualityArtifactDescriptor, InspectionError>;
}
pub struct QualityV2Ports<'a, E, A, P> {
    pub executor: &'a E,
    pub auditor: &'a A,
    pub publisher: &'a mut P,
}
pub struct QualityV2Inputs<'a> {
    pub vendor: Option<&'a CargoVendorSnapshot>,
    pub policy: Option<&'a SecurityPolicy>,
    pub options: &'a QualityV2Options,
}
pub struct PublishedQualityV2 {
    pub observation: QualityV2Observation,
    pub artifact: QualityArtifactDescriptor,
}
fn remaining(started: Instant, budget: u64) -> Result<u64, SecurityError> {
    budget
        .checked_sub(started.elapsed().as_secs())
        .filter(|s| *s > 0)
        .ok_or(SecurityError::Timeout)
}
fn bound_runtime(
    expected: &RuntimeIdentity,
    actual: &RuntimeIdentity,
) -> Result<(), SecurityError> {
    if !quality_runtime_matches(expected, actual) {
        return Err(SecurityError::InvalidMetadata);
    }
    Ok(())
}
fn failed_row(
    kind: QualityV2StageKind,
    error: InspectionError,
) -> Result<QualityV2Stage, SecurityError> {
    let issue = crate::quality::stage_issue(ProjectAuditError::Inspection(error))?;
    Ok(QualityV2Stage {
        stage: kind,
        status: issue.status(),
        issue: Some(issue),
        duration_ms: 0,
        execution_fingerprint: None,
        details: None,
    })
}
fn security_failure(
    kind: QualityV2StageKind,
    error: SecurityError,
) -> Result<QualityV2Stage, SecurityError> {
    match error {
        SecurityError::Inspection(error) => failed_row(kind, error),
        SecurityError::Timeout => Err(SecurityError::Timeout),
        error => {
            let code = match error {
                SecurityError::MissingOfflineData => OperationalErrorCode::ToolNotInstalled,
                SecurityError::OutputLimit => OperationalErrorCode::OutputLimitExceeded,
                _ => OperationalErrorCode::InvalidProject,
            };
            failed_row(kind, InspectionError::Project(ProjectError::Rejected(code)))
        }
    }
}
fn observed_row(
    kind: QualityV2StageKind,
    status: ToolStatus,
    runtime: &RuntimeIdentity,
    details: QualityV2Details,
) -> QualityV2Stage {
    let mut row = QualityV2Stage {
        stage: kind,
        status,
        issue: (status == ToolStatus::Blocked).then_some(QualityIssue::Incomplete),
        duration_ms: 0,
        execution_fingerprint: Some(runtime.execution_fingerprint.clone()),
        details: Some(details),
    };
    if row.status == ToolStatus::Passed && !row.evidence_complete() {
        row.status = ToolStatus::Blocked;
        row.issue = Some(QualityIssue::Incomplete);
    }
    row
}
fn validation_row(
    kind: QualityV2StageKind,
    stage: QualityStage,
    observation: QualityObservation,
    structure: &ProjectStructure,
) -> Result<QualityV2Stage, SecurityError> {
    bound_runtime(&structure.runtime, observation.runtime())?;
    let execution = observation
        .execution()
        .ok_or(SecurityError::InvalidMetadata)?;
    if execution.source_fingerprint != structure.source_fingerprint {
        return Err(SecurityError::InvalidMetadata);
    }
    let details = QualityV2Details::Validation {
        termination: execution.termination,
        exit_code: execution.exit_code,
        validation_complete: execution.validation_complete,
        diagnostics: execution
            .diagnostics
            .len()
            .try_into()
            .map_err(|_| SecurityError::OutputLimit)?,
        diagnostics_omitted: execution.diagnostics_omitted,
        affected_files: match &observation {
            QualityObservation::Format(f) => {
                Some(f.affected_files.len() as u64 + f.affected_files_omitted)
            }
            _ => None,
        },
        build_succeeded: match &observation {
            QualityObservation::Test(t) => t.build_succeeded,
            _ => None,
        },
    };
    let runtime = observation.runtime().clone();
    let mut row = QualityStageReport {
        stage,
        status: ToolStatus::Blocked,
        issue: None,
        duration_ms: 0,
        observation: Some(observation),
        log: None,
        retention_remaining_seconds: None,
    };
    row.classify();
    Ok(QualityV2Stage {
        stage: kind,
        status: row.status,
        issue: row.issue,
        duration_ms: 0,
        execution_fingerprint: Some(runtime.execution_fingerprint),
        details: Some(details),
    })
}
impl<B: ProjectSourceBackend + QualityProjectBackend, G: ReferenceGenerator, C: RegistryClock>
    ProjectRegistry<B, G, C>
{
    pub fn quality_gate_v2<E, A, P>(
        &mut self,
        reference: &ProjectRef,
        inputs: QualityV2Inputs<'_>,
        ports: QualityV2Ports<'_, E, A, P>,
        clock: &impl Clock,
        control: &dyn InspectionControl,
    ) -> Result<PublishedQualityV2, SecurityError>
    where
        E: ProjectFormatPort
            + ProjectCheckPort
            + ProjectClippyPort
            + ProjectTestPort
            + ProjectInspectionPort
            + ProjectDenyPort
            + ProjectCoveragePort
            + ProjectSemverPort
            + ProjectMutationTestPort,
        A: DependencyAuditPort,
        P: QualityV2Publisher,
    {
        let options = inputs.options;
        options
            .validate()
            .map_err(|_| SecurityError::InvalidMetadata)?;
        let started = Instant::now();
        let capture = self.capture_security(reference, clock, control)?;
        let structure = ports.executor.inspect(&capture.source, control)?;
        let baseline = options
            .baseline
            .as_ref()
            .map(|reference| self.capture_security(reference, clock, control))
            .transpose()?;
        let baseline_structure = baseline
            .as_ref()
            .map(|b| ports.executor.inspect(&b.source, control))
            .transpose()?;
        if let Some(b) = &baseline_structure {
            bound_runtime(&structure.runtime, &b.runtime)?;
        }
        let check = CheckOptions::try_from(CheckSelection::default())
            .map_err(|_| SecurityError::InvalidMetadata)?;
        let clippy = ClippyOptions::try_from(ClippySelection {
            lint_profile: LintProfile::Strict,
            ..Default::default()
        })
        .map_err(|_| SecurityError::InvalidMetadata)?;
        let test = TestOptions::try_from(TestSelection::default())
            .map_err(|_| SecurityError::InvalidMetadata)?;
        let mut audit = None;
        let mut stages = Vec::new();
        for kind in options.profile.stages(options.mutation.is_some()) {
            control.check()?;
            self.resolve_inner(reference, control, false)?;
            let stage_started = Instant::now();
            let seconds = remaining(started, options.timeout_seconds)?;
            use QualityV2StageKind as K;
            let mut row = match kind {
                K::Format | K::Check | K::Clippy | K::Test => {
                    let (stage, result) = match kind {
                        K::Format => (
                            QualityStage::Format,
                            ports
                                .executor
                                .format(&capture.source, control)
                                .map(QualityObservation::Format),
                        ),
                        K::Check => (
                            QualityStage::Check,
                            ports
                                .executor
                                .check(&capture.source, &check, control)
                                .map(QualityObservation::Check),
                        ),
                        K::Clippy => (
                            QualityStage::Clippy,
                            ports
                                .executor
                                .clippy(&capture.source, &clippy, control)
                                .map(QualityObservation::Clippy),
                        ),
                        _ => (
                            QualityStage::Test,
                            ports
                                .executor
                                .test(&capture.source, &test, control)
                                .map(QualityObservation::Test),
                        ),
                    };
                    match result {
                        Ok(value) => validation_row(kind, stage, value, &structure)?,
                        Err(error) => failed_row(kind, error)?,
                    }
                }
                K::Audit => match ports
                    .auditor
                    .audit(&capture.source, &structure, clock, control)
                {
                    Ok(value) => {
                        let mut normalized = value.clone();
                        normalized.normalize();
                        let status = match normalized.state {
                            AuditState::Passed => ToolStatus::Passed,
                            AuditState::Failed => ToolStatus::Failed,
                            AuditState::Incomplete => ToolStatus::Blocked,
                            AuditState::Unavailable => ToolStatus::Unavailable,
                        };
                        let row = observed_row(
                            kind,
                            status,
                            &structure.runtime,
                            QualityV2Details::Audit {
                                observation: SupplyAudit::from(&normalized),
                            },
                        );
                        audit = Some(value);
                        row
                    }
                    Err(error) => {
                        let issue = crate::quality::stage_issue(ProjectAuditError::Data(error))?;
                        QualityV2Stage {
                            stage: kind,
                            status: issue.status(),
                            issue: Some(issue),
                            duration_ms: 0,
                            execution_fingerprint: None,
                            details: None,
                        }
                    }
                },
                K::Deny => {
                    if let (Some(vendor), Some(policy)) = (inputs.vendor, inputs.policy) {
                        let deny_options = DenyOptions::try_from(DenySelection {
                            timeout_seconds: seconds.min(120),
                        })
                        .map_err(|_| SecurityError::InvalidMetadata)?;
                        match ports.executor.deny(
                            &capture.source,
                            vendor,
                            policy,
                            &deny_options,
                            control,
                        ) {
                            Ok(value) => {
                                let combined = compose_security(
                                    &structure,
                                    audit.clone().unwrap_or_else(AuditObservation::unavailable),
                                    value,
                                    vendor,
                                    policy,
                                    clock,
                                )?;
                                let status = match combined.policy_state {
                                    SecurityPolicyState::Satisfied
                                    | SecurityPolicyState::SatisfiedWithSuppressions => {
                                        ToolStatus::Passed
                                    }
                                    SecurityPolicyState::Violated => ToolStatus::Failed,
                                    SecurityPolicyState::Undetermined => ToolStatus::Blocked,
                                };
                                observed_row(
                                    kind,
                                    status,
                                    &combined.deny.runtime,
                                    QualityV2Details::Deny {
                                        observation: SupplyDeny {
                                            complete: combined.completeness
                                                == SecurityCompleteness::Complete,
                                            policy_state: combined.policy_state,
                                            findings: combined.findings,
                                            findings_omitted: combined.findings_omitted,
                                            policy_fingerprint: combined.deny.policy_fingerprint,
                                            execution_fingerprint: combined
                                                .deny
                                                .execution_fingerprint
                                                .clone(),
                                        },
                                    },
                                )
                            }
                            Err(error) => security_failure(kind, error)?,
                        }
                    } else {
                        security_failure(kind, SecurityError::MissingOfflineData)?
                    }
                }
                K::Coverage => {
                    let selection = CoverageOptions::try_from(CoverageSelection {
                        workspace: true,
                        timeout_seconds: seconds,
                        ..Default::default()
                    })
                    .map_err(|_| SecurityError::InvalidMetadata)?;
                    match ProjectCoveragePort::run(
                        ports.executor,
                        &capture.source,
                        &selection,
                        control,
                    ) {
                        Ok(value) => {
                            value.validate()?;
                            bound_runtime(&structure.runtime, &value.runtime)?;
                            let complete = value.parse_complete
                                && value.termination == ExecutionTermination::Exited
                                && value.exit_code == Some(0)
                                && value.summary.aggregate.lines.is_some()
                                && !value.artifacts.json_truncated;
                            observed_row(
                                kind,
                                if complete {
                                    ToolStatus::Passed
                                } else {
                                    ToolStatus::Blocked
                                },
                                &value.runtime,
                                QualityV2Details::Coverage {
                                    aggregate: value.summary.aggregate,
                                    parse_complete: value.parse_complete,
                                    exit_code: value.exit_code,
                                    doctests_run: value.doctests_run,
                                },
                            )
                        }
                        Err(error) => failed_row(kind, error)?,
                    }
                }
                K::Semver => {
                    let b = baseline.as_ref().ok_or(SecurityError::InvalidMetadata)?;
                    let selection =
                        SemverCommandOptions::try_from(SemverProjectSelection::default())
                            .map_err(|_| SecurityError::InvalidMetadata)?;
                    let selection = SemverOptions::new(selection.clone(), selection, seconds)
                        .map_err(|_| SecurityError::InvalidMetadata)?;
                    match ProjectSemverPort::run(
                        ports.executor,
                        &b.source,
                        &capture.source,
                        &selection,
                        control,
                    ) {
                        Ok(value) => {
                            value.validate()?;
                            bound_runtime(&structure.runtime, &value.runtime)?;
                            let status = match crate::semver_check::classify(&value) {
                                SemverOutcome::NoBreak => ToolStatus::Passed,
                                SemverOutcome::Breaking => ToolStatus::Failed,
                                SemverOutcome::Unavailable => ToolStatus::Unavailable,
                                _ => ToolStatus::Blocked,
                            };
                            observed_row(
                                kind,
                                status,
                                &value.runtime,
                                QualityV2Details::Semver {
                                    counts: value.counts,
                                    findings_completeness: value.completeness,
                                    exit_code: value.exit_code,
                                },
                            )
                        }
                        Err(error) => failed_row(kind, error)?,
                    }
                }
                K::Mutation => {
                    let mutation = options
                        .mutation
                        .as_ref()
                        .ok_or(SecurityError::InvalidMetadata)?;
                    match ProjectMutationTestPort::run(
                        ports.executor,
                        &capture.source,
                        mutation,
                        control,
                    ) {
                        Ok(value) => {
                            value.validate()?;
                            bound_runtime(&structure.runtime, &value.runtime)?;
                            let status = if value.clean() {
                                ToolStatus::Passed
                            } else if value.conclusive_failure() {
                                ToolStatus::Failed
                            } else {
                                ToolStatus::Blocked
                            };
                            observed_row(
                                kind,
                                status,
                                &value.runtime,
                                QualityV2Details::Mutation {
                                    baseline: value.baseline,
                                    counts: value.counts,
                                    validation_complete: value.validation_complete,
                                    cap_exceeded: value.cap_exceeded,
                                },
                            )
                        }
                        Err(error) => failed_row(kind, error)?,
                    }
                }
            };
            control.check()?;
            row.duration_ms = stage_started
                .elapsed()
                .as_millis()
                .try_into()
                .unwrap_or(u64::MAX);
            stages.push(row);
        }
        let baseline_evidence =
            baseline
                .as_ref()
                .zip(baseline_structure)
                .map(|(b, s)| QualityV2Baseline {
                    project_ref: b.project_ref.clone(),
                    identity_fingerprint: b.project_identity_fingerprint.clone(),
                    source_fingerprint: s.source_fingerprint,
                });
        let mut report = QualityV2Report {
            profile: options.profile,
            mutation_requested: options.mutation.is_some(),
            source_fingerprint: structure.source_fingerprint,
            baseline: baseline_evidence,
            stages,
            complete: false,
            status: ToolStatus::Blocked,
        };
        report.refresh();
        if !report.validate() {
            return Err(SecurityError::InvalidMetadata);
        }
        let observation = QualityV2Observation {
            report,
            execution_fingerprint: structure.runtime.execution_fingerprint.clone(),
            runtime: structure.runtime,
        };
        let mut revalidate = || {
            if let Some(b) = &baseline {
                self.quality_owner_facts(&b.project_ref, control)?;
            }
            self.quality_owner_facts(reference, control)
                .map_err(InspectionError::from)
        };
        let artifact = ports
            .publisher
            .publish_gate_v2(&capture, &observation, &mut revalidate)?;
        control.check()?;
        if self.resolve_inner(reference, control, true)?.fingerprint
            != capture.project_identity_fingerprint
        {
            return Err(SecurityError::InvalidMetadata);
        }
        if let Some(b) = baseline
            && self
                .resolve_inner(&b.project_ref, control, true)?
                .fingerprint
                != b.project_identity_fingerprint
        {
            return Err(SecurityError::InvalidMetadata);
        }
        if let Some(p) = inputs.policy {
            p.validate_at(clock.now().0)
                .map_err(|_| SecurityError::InvalidPolicy)?;
        }
        Ok(PublishedQualityV2 {
            observation,
            artifact,
        })
    }
}
