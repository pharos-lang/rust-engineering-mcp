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
        let log = rust_engineering_execution::safe_unsafe_log(observation)
            .map_err(|_| InspectionError::InvalidMetadata)?;
        self.publish_normalized(
            capture,
            NormalizedEvidence {
                bytes: log.bytes,
                complete: observation.report.syntax_complete && log.findings_removed == 0,
                runtime: &observation.runtime,
                execution_fingerprint: &observation.execution_fingerprint,
                normalizer: include_bytes!("../../../../execution-adapter/src/unsafe_log.rs"),
            },
            revalidate,
        )
    }
}

impl rust_engineering_application::miri::MiriPublisher for DurableSecurityPublisher {
    fn publish_miri(
        &mut self,
        capture: &SecurityCapture,
        observation: &rust_engineering_application::miri::MiriObservation,
        revalidate: &mut dyn FnMut() -> Result<QualityOwnerFacts, InspectionError>,
    ) -> Result<rust_engineering_domain::QualityArtifactDescriptor, InspectionError> {
        let log = rust_engineering_execution::safe_miri_log(observation)
            .map_err(|_| InspectionError::InvalidMetadata)?;
        self.publish_normalized(
            capture,
            NormalizedEvidence {
                bytes: log.bytes,
                complete: observation.report.complete && log.findings_removed == 0,
                runtime: &observation.runtime,
                execution_fingerprint: &observation.execution_fingerprint,
                normalizer: include_bytes!("../../../../execution-adapter/src/miri_log.rs"),
            },
            revalidate,
        )
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
        let mut entropy = [0_u8; 16];
        getrandom::fill(&mut entropy).map_err(|_| InspectionError::Internal)?;
        let job = QualityJobId::from_random_bytes(entropy);
        getrandom::fill(&mut entropy).map_err(|_| InspectionError::Internal)?;
        let artifact = QualityArtifactId::from_random_bytes(entropy);
        let created = UtcInstant::from_unix_seconds(capture.captured_at.0)
            .map_err(|_| InspectionError::Internal)?;
        let expires = created
            .checked_add_seconds(QUALITY_DEFAULT_TTL_SECONDS)
            .map_err(|_| InspectionError::Internal)?;
        let mut toolchain = Sha256::new();
        toolchain.update(b"rust-mcp/quality-toolchain/v1\0");
        toolchain.update(evidence.runtime.rust_version.as_bytes());
        toolchain.update([0]);
        toolchain.update(evidence.runtime.cargo_version.as_bytes());
        let runtime = ArtifactRuntime {
            image_digest: digest_text(&evidence.runtime.image_id)?,
            toolchain_identity: toolchain.finalize().into(),
            plugin: ArtifactPlugin {
                identity: PluginIdentity::Builtin,
                version: 1,
                digest: Sha256::digest(evidence.normalizer).into(),
            },
            implementation_digest: digest_text(&evidence.execution_fingerprint.to_string())?,
        };
        let mut store = self.store.lock().map_err(|_| InspectionError::Internal)?;
        let mut authority = CallbackAuthority { revalidate };
        let mut access = QualityArtifactAccess {
            store: &mut *store,
            authority: &mut authority,
            retention: QualityRetentionGrant::PotentiallySensitive,
        };
        let reservation = access
            .begin(
                &capture.project_ref,
                job,
                (bytes.len() as u64).max(1),
                quality_member_charge(QualityArtifactKind::ToolLog, None).map_err(quality_error)?,
                expires.clone(),
            )
            .map_err(quality_error)?;
        let outcome = access
            .publish(
                &capture.project_ref,
                &reservation,
                QualityArtifactDraft {
                    artifact_id: artifact,
                    member_index: 0,
                    kind: QualityArtifactKind::ToolLog,
                    mime_type: QualityMimeType::TextPlain,
                    payload_format_version: PayloadFormatVersion::Utf8LogV1,
                    completeness: if evidence.complete {
                        ArtifactCompleteness::Complete
                    } else {
                        ArtifactCompleteness::Partial
                    },
                    sensitivity: ArtifactSensitivity::PotentiallySensitive,
                    created_at_utc: created,
                    expires_at_utc: expires,
                    source: ArtifactSource {
                        captured_source_sha256: source_digest(&capture.source),
                        guest_name: GuestArtifactName::ToolLog,
                        selection: ArtifactSelection::Workspace,
                    },
                    runtime,
                },
                (bytes.len() as u64).max(1),
                &mut Bytes(&bytes),
            )
            .map_err(quality_error);
        if access.finish(&reservation).is_err() {
            access.store.reconcile_recover().map_err(quality_error)?;
            return Err(InspectionError::Internal);
        }
        outcome
    }
}
