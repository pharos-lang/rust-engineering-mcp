//! Single capture, sequential ports, grouped authorization and optional logs.
use crate::*;
use rust_engineering_domain::*;
use std::time::Instant;

pub struct QualityPorts<'a, E, A> {
    pub executor: &'a E,
    pub auditor: &'a A,
}

impl<B: ProjectSourceBackend, G: ReferenceGenerator, C: RegistryClock> ProjectRegistry<B, G, C> {
    pub fn quality_gate<E, A>(
        &mut self,
        reference: &ProjectRef,
        profile: QualityProfile,
        ports: QualityPorts<'_, E, A>,
        artifacts: &mut impl ArtifactStore,
        clocks: (&impl Clock, &impl RegistryClock),
        control: &dyn InspectionControl,
    ) -> Result<ProjectQualityGate, InspectionError>
    where
        E: ProjectFormatPort
            + ProjectCheckPort
            + ProjectClippyPort
            + ProjectTestPort
            + ProjectInspectionPort,
        A: DependencyAuditPort,
    {
        let captured = self.capture_validation(reference, artifacts, clocks.0, control)?;
        let check_options = CheckOptions::try_from(CheckSelection::default())
            .map_err(|_| InspectionError::Internal)?;
        let clippy_options = ClippyOptions::try_from(ClippySelection {
            lint_profile: LintProfile::Strict,
            ..Default::default()
        })
        .map_err(|_| InspectionError::Internal)?;
        let test_options = TestOptions::try_from(TestSelection::default())
            .map_err(|_| InspectionError::Internal)?;
        let mut stages = Vec::new();
        let mut fingerprint = None;
        let mut runtime = None;
        for &stage in profile.stages() {
            self.resolve_inner(reference, control, false)?;
            let started = Instant::now();
            let result: Result<QualityObservation, ProjectAuditError> = match stage {
                QualityStage::Format => ports
                    .executor
                    .format(&captured.source, control)
                    .map(QualityObservation::Format)
                    .map_err(Into::into),
                QualityStage::Check => ports
                    .executor
                    .check(&captured.source, &check_options, control)
                    .map(QualityObservation::Check)
                    .map_err(Into::into),
                QualityStage::Clippy => ports
                    .executor
                    .clippy(&captured.source, &clippy_options, control)
                    .map(QualityObservation::Clippy)
                    .map_err(Into::into),
                QualityStage::Test => ports
                    .executor
                    .test(&captured.source, &test_options, control)
                    .map(QualityObservation::Test)
                    .map_err(Into::into),
                QualityStage::Audit => match ports.executor.inspect(&captured.source, control) {
                    Ok(structure) => {
                        match_fingerprint(&mut fingerprint, &structure.source_fingerprint)?;
                        match_runtime(&mut runtime, &structure.runtime)?;
                        ports
                            .auditor
                            .audit(&captured.source, &structure, clocks.0, control)
                            .map(|observation| QualityObservation::Audit {
                                runtime: structure.runtime,
                                observation,
                            })
                            .map_err(Into::into)
                    }
                    Err(e) => Err(e.into()),
                },
            };
            control.check()?;
            let mut row = QualityStageReport {
                stage,
                duration_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
                status: ToolStatus::Blocked,
                issue: None,
                observation: None,
                log: None,
                retention_remaining_seconds: None,
            };
            match result {
                Ok(observation) => {
                    match_runtime(&mut runtime, observation.runtime())?;
                    if let Some(execution) = observation.execution() {
                        match_fingerprint(&mut fingerprint, &execution.source_fingerprint)?;
                    }
                    row.observation = Some(observation);
                }
                Err(error) => row.issue = Some(stage_issue(error)?),
            }
            row.classify();
            if row.status == ToolStatus::Cancelled {
                return Err(InspectionError::Project(ProjectError::Cancelled));
            }
            stages.push(row);
        }
        self.publish_quality(
            captured,
            (fingerprint, profile, stages),
            artifacts,
            clocks,
            control,
        )
    }
}
fn match_runtime(
    expected: &mut Option<RuntimeIdentity>,
    actual: &RuntimeIdentity,
) -> Result<(), InspectionError> {
    if expected
        .as_ref()
        .is_some_and(|identity| !quality_runtime_matches(identity, actual))
    {
        return Err(InspectionError::Internal);
    }
    if expected.is_none() {
        *expected = Some(actual.clone());
    }
    Ok(())
}
fn match_fingerprint(
    expected: &mut Option<SourceFingerprint>,
    actual: &SourceFingerprint,
) -> Result<(), InspectionError> {
    if expected.as_ref().is_some_and(|fp| fp != actual) {
        return Err(InspectionError::Internal);
    }
    if expected.is_none() {
        *expected = Some(actual.clone());
    }
    Ok(())
}
fn stage_issue(error: ProjectAuditError) -> Result<QualityIssue, InspectionError> {
    match error {
        ProjectAuditError::Inspection(error) => match error {
            InspectionError::Project(ProjectError::Rejected(code)) => {
                Ok(QualityIssue::Operational(code))
            }
            InspectionError::Execution(ExecutionError::Unavailable) => Ok(
                QualityIssue::Operational(OperationalErrorCode::ToolNotInstalled),
            ),
            InspectionError::Execution(
                ExecutionError::Denied
                | ExecutionError::Busy
                | ExecutionError::InvalidConfiguration,
            ) => Ok(QualityIssue::Operational(
                OperationalErrorCode::SandboxDenied,
            )),
            InspectionError::OutputLimit => Ok(QualityIssue::Operational(
                OperationalErrorCode::OutputLimitExceeded,
            )),
            InspectionError::InvalidMetadata => Ok(QualityIssue::Operational(
                OperationalErrorCode::InvalidProject,
            )),
            _ => Err(error),
        },
        ProjectAuditError::Data(error) => match error {
            AuditDataError::Internal => Err(InspectionError::Internal),
            AuditDataError::Cancelled => Err(InspectionError::Project(ProjectError::Cancelled)),
            AuditDataError::Timeout => Ok(QualityIssue::Operational(
                OperationalErrorCode::CommandTimeout,
            )),
            AuditDataError::SandboxDenied => Ok(QualityIssue::Operational(
                OperationalErrorCode::SandboxDenied,
            )),
            AuditDataError::UnsupportedPlatform => Ok(QualityIssue::Operational(
                OperationalErrorCode::UnsupportedPlatform,
            )),
            _ => Ok(QualityIssue::Audit(error)),
        },
    }
}
