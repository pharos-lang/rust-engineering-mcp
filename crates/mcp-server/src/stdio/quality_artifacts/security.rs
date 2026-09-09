//! Normalized security evidence in the unchanged M3 durable artifact format.
use super::*;
use rust_engineering_application::security::{
    SecurityCapture, SecurityObservation, SecurityPublisher,
};
use rust_engineering_domain::security::SecurityCompleteness;

#[derive(Clone)]
pub(in crate::stdio) struct DurableSecurityPublisher {
    pub(super) store: Arc<Mutex<NativeQualityArtifactStore>>,
}

impl SecurityPublisher for DurableSecurityPublisher {
    fn publish(
        &mut self,
        capture: &SecurityCapture,
        observation: &SecurityObservation,
        revalidate: &mut dyn FnMut() -> Result<QualityOwnerFacts, InspectionError>,
    ) -> Result<rust_engineering_domain::QualityArtifactDescriptor, InspectionError> {
        let log = rust_engineering_execution::safe_security_log(observation)
            .map_err(|_| InspectionError::InvalidMetadata)?;
        self.publish_normalized(
            capture,
            NormalizedEvidence {
                bytes: log.bytes,
                complete: observation.completeness == SecurityCompleteness::Complete
                    && log.findings_removed == 0,
                runtime: &observation.deny.runtime,
                execution_fingerprint: &observation.deny.execution_fingerprint,
                normalizer: include_bytes!("../../../../execution-adapter/src/security_log.rs"),
            },
            revalidate,
        )
    }
}

impl rust_engineering_application::unsafe_scan::UnsafePublisher for DurableSecurityPublisher {
    fn publish_unsafe(
        &mut self,
        capture: &SecurityCapture,
        observation: &rust_engineering_application::unsafe_scan::UnsafeObservation,
        revalidate: &mut dyn FnMut() -> Result<QualityOwnerFacts, InspectionError>,
    ) -> Result<rust_engineering_domain::QualityArtifactDescriptor, InspectionError> {
        self.publish_normalized(capture, normalized_unsafe(observation)?, revalidate)
    }
}

impl rust_engineering_application::miri::MiriPublisher for DurableSecurityPublisher {
    fn publish_miri(
        &mut self,
        capture: &SecurityCapture,
        observation: &rust_engineering_application::miri::MiriObservation,
        revalidate: &mut dyn FnMut() -> Result<QualityOwnerFacts, InspectionError>,
    ) -> Result<rust_engineering_domain::QualityArtifactDescriptor, InspectionError> {
        self.publish_normalized(capture, normalized_miri(observation)?, revalidate)
    }
}

impl rust_engineering_application::supply_chain::SupplyPublisher for DurableSecurityPublisher {
    fn publish_supply(
        &mut self,
        capture: &SecurityCapture,
        observation: &rust_engineering_domain::supply_chain::SupplyObservation,
        revalidate: &mut dyn FnMut() -> Result<QualityOwnerFacts, InspectionError>,
    ) -> Result<rust_engineering_domain::QualityArtifactDescriptor, InspectionError> {
        let log = rust_engineering_execution::safe_supply_log(observation)
            .map_err(|_| InspectionError::InvalidMetadata)?;
        self.publish_normalized(
            capture,
            NormalizedEvidence {
                bytes: log.bytes,
                complete: observation.report.complete && log.findings_removed == 0,
                runtime: &observation.runtime,
                execution_fingerprint: &observation.execution_fingerprint,
                normalizer: include_bytes!("../../../../execution-adapter/src/supply_log.rs"),
            },
            revalidate,
        )
    }
}

impl rust_engineering_application::quality_v2::QualityV2Publisher for DurableSecurityPublisher {
    fn publish_gate_v2(
        &mut self,
        capture: &SecurityCapture,
        observation: &rust_engineering_domain::quality_v2::QualityV2Observation,
        revalidate: &mut dyn FnMut() -> Result<QualityOwnerFacts, InspectionError>,
    ) -> Result<rust_engineering_domain::QualityArtifactDescriptor, InspectionError> {
        let log = rust_engineering_execution::safe_gate_v2_log(observation)
            .map_err(|_| InspectionError::InvalidMetadata)?;
        self.publish_normalized(
            capture,
            NormalizedEvidence {
                bytes: log.bytes,
                complete: observation.report.complete && log.findings_removed == 0,
                runtime: &observation.runtime,
                execution_fingerprint: &observation.execution_fingerprint,
                normalizer: include_bytes!("../../../../execution-adapter/src/quality_v2_log.rs"),
            },
            revalidate,
        )
    }
}

fn normalized_unsafe(
    observation: &rust_engineering_application::unsafe_scan::UnsafeObservation,
) -> Result<NormalizedEvidence<'_>, InspectionError> {
    let log = rust_engineering_execution::safe_unsafe_log(observation)
        .map_err(|_| InspectionError::InvalidMetadata)?;
    Ok(NormalizedEvidence {
        bytes: log.bytes,
        complete: observation.report.syntax_complete && log.findings_removed == 0,
        runtime: &observation.runtime,
        execution_fingerprint: &observation.execution_fingerprint,
        normalizer: include_bytes!("../../../../execution-adapter/src/unsafe_log.rs"),
    })
}

fn normalized_miri(
    observation: &rust_engineering_application::miri::MiriObservation,
) -> Result<NormalizedEvidence<'_>, InspectionError> {
    let log = rust_engineering_execution::safe_miri_log(observation)
        .map_err(|_| InspectionError::InvalidMetadata)?;
    Ok(NormalizedEvidence {
        bytes: log.bytes,
        complete: observation.report.complete && log.findings_removed == 0,
        runtime: &observation.runtime,
        execution_fingerprint: &observation.execution_fingerprint,
        normalizer: include_bytes!("../../../../execution-adapter/src/miri_log.rs"),
    })
}

struct NormalizedEvidence<'a> {
    bytes: Vec<u8>,
    complete: bool,
    runtime: &'a rust_engineering_domain::RuntimeIdentity,
    execution_fingerprint: &'a rust_engineering_domain::ExecutionFingerprint,
    normalizer: &'a [u8],
}
impl DurableSecurityPublisher {
    fn publish_normalized(
        &mut self,
        capture: &SecurityCapture,
        evidence: NormalizedEvidence<'_>,
        revalidate: &mut dyn FnMut() -> Result<QualityOwnerFacts, InspectionError>,
    ) -> Result<rust_engineering_domain::QualityArtifactDescriptor, InspectionError> {
        let bytes = evidence.bytes;
        publish_job(
            &self.store,
            Job {
                project: &capture.project_ref,
                captured_at: capture.captured_at,
                source: &capture.source,
                selection: ArtifactSelection::Workspace,
                runtime: ArtifactRuntime {
                    image_digest: digest_text(&evidence.runtime.image_id)?,
                    toolchain_identity: toolchain_identity(evidence.runtime),
                    plugin: ArtifactPlugin {
                        identity: PluginIdentity::Builtin,
                        version: 1,
                        digest: Sha256::digest(evidence.normalizer).into(),
                    },
                    implementation_digest: digest_text(
                        &evidence.execution_fingerprint.to_string(),
                    )?,
                },
                // Configuring the private state root is the host's explicit
                // grant for a normalized log that may still quote project text.
                retention: QualityRetentionGrant::PotentiallySensitive,
            },
            &[JobMember {
                kind: QualityArtifactKind::ToolLog,
                mime_type: QualityMimeType::TextPlain,
                payload_format_version: PayloadFormatVersion::Utf8LogV1,
                guest_name: GuestArtifactName::ToolLog,
                sensitivity: ArtifactSensitivity::PotentiallySensitive,
                completeness: if evidence.complete {
                    ArtifactCompleteness::Complete
                } else {
                    ArtifactCompleteness::Partial
                },
                bytes: &bytes,
            }],
            revalidate,
        )?
        .pop()
        .ok_or(InspectionError::Internal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stdio::security_tool::test_fixtures as fixture;
    use rust_engineering_domain::{
        miri::{MiriCounts, MiriObservation, MiriReport},
        unsafe_scan::{UnsafeCoverage, UnsafeObservation, UnsafeScanReport},
    };

    #[test]
    fn portable_normalization_preserves_complete_and_partial_semantics()
    -> Result<(), Box<dyn std::error::Error>> {
        let runtime = fixture::runtime()?;
        let execution = fixture::execution_fingerprint('3')?;
        let unsafe_observation = UnsafeObservation {
            report: UnsafeScanReport {
                coverage: UnsafeCoverage {
                    files_total: 1,
                    files_selected: 1,
                    files_parsed: 1,
                    workspace_files: 1,
                    ..Default::default()
                },
                findings: Vec::new(),
                findings_total: 0,
                findings_omitted: 0,
                syntax_complete: true,
                cfg_evaluated: false,
                macros_expanded: false,
                generated_sources_scanned: false,
            },
            source_fingerprint: fixture::source_fingerprint('4')?,
            vendor_fingerprint: fixture::source_fingerprint('5')?,
            vendor_archive_fingerprint: fixture::source_fingerprint('6')?,
            metadata_fingerprint: fixture::source_fingerprint('7')?,
            manifest_fingerprint: fixture::source_fingerprint('8')?,
            runtime: runtime.clone(),
            execution_fingerprint: execution.clone(),
        };
        let unsafe_evidence =
            normalized_unsafe(&unsafe_observation).map_err(|error| format!("{error:?}"))?;
        assert!(unsafe_evidence.complete);
        assert!(!unsafe_evidence.bytes.is_empty());
        assert!(!unsafe_evidence.normalizer.is_empty());

        let miri_observation = MiriObservation {
            report: MiriReport {
                counts: MiriCounts::default(),
                findings: Vec::new(),
                findings_omitted: 0,
                complete: false,
                clean: false,
                junit_present: false,
                exit_code: None,
            },
            source_fingerprint: fixture::source_fingerprint('4')?,
            vendor_fingerprint: fixture::source_fingerprint('5')?,
            metadata_fingerprint: fixture::source_fingerprint('6')?,
            config_fingerprint: fixture::source_fingerprint('7')?,
            junit_fingerprint: None,
            runtime,
            execution_fingerprint: execution,
            nightly_commit: "5a2be9f5f075d31e3ca5526b5b029881ce441253".into(),
            sysroot_fingerprint: fixture::source_fingerprint('8')?,
        };
        let miri_evidence =
            normalized_miri(&miri_observation).map_err(|error| format!("{error:?}"))?;
        assert!(!miri_evidence.complete);
        assert!(!miri_evidence.bytes.is_empty());
        assert!(!miri_evidence.normalizer.is_empty());
        Ok(())
    }
}
