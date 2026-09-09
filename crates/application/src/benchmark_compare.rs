//! Application boundary for one `rust.benchmark.compare` call (M5-02).
//!
//! This module runs no process, captures no source and touches no project. It
//! reads two already-authorized artifacts and computes (ADR-076 §4). Three
//! properties follow from that and are enforced here:
//!
//! * **Reading does not extend authority.** Owner facts are revalidated through
//!   [`ProjectRegistry::quality_owner_facts`], which resolves *without* renewing
//!   the idle lease. A caller cannot keep a project alive by polling artifacts.
//! * **One project owns both ids.** Both reads derive their owner binding from
//!   the same live grant, so an id belonging to another project does not
//!   resolve — it does not exist for this call, exactly as ADR-076 §4 requires.
//! * **A kind is checked before a parse.** The stored descriptor must already
//!   say `benchmark_dataset`/`benchmark_dataset_v2`. A JUnit or coverage id is
//!   `NotADataset`, never a decoder failure, so a wrong id never reaches a
//!   parser and never produces a diagnostic about someone else's bytes.
use crate::quality_artifact::{QUALITY_RESOURCE_CHUNK_BYTES, QualityArtifactStore};
use crate::{
    OperationControl, ProjectError, ProjectRegistry, QualityProjectBackend, ReferenceGenerator,
    RegistryClock,
};
use rust_engineering_domain::benchmark::{BenchmarkDataset, BenchmarkError};
use rust_engineering_domain::benchmark_compare::{
    CompareError, ComparisonReport, Incompatibility, compare,
};
use rust_engineering_domain::{
    PayloadFormatVersion, ProjectRef, QualityArtifactError, QualityArtifactId, QualityArtifactKind,
};

/// Hard ceiling on the bytes one comparison will read for one artifact.
///
/// ADR-076 §7 budgets 32 MiB for samples. The store's own per-artifact ceiling
/// is the same number, but this bound is applied against the descriptor the
/// store *returns*, so a store that reports a larger artifact is refused rather
/// than trusted.
pub const COMPARE_MAX_DATASET_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompareRequest {
    pub baseline: QualityArtifactId,
    pub candidate: QualityArtifactId,
}

/// The two possible results of a comparison.
///
/// An incompatible pair is an observed result, not an infrastructure failure:
/// ADR-076 §4 keeps `isError` false for it and requires the complete list of
/// reasons, so it is a variant of the success type.
#[derive(Clone, Debug, PartialEq)]
pub enum CompareOutcome {
    Report(Box<ComparisonReport>),
    /// The refusal carries the two provenance records the check read, so a
    /// caller can see the values behind each reason without the artifacts.
    Incompatible(Box<Incompatibility>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BenchmarkCompareError {
    /// Unknown, expired, revoked, or owned by another project. Indistinguishable
    /// on purpose.
    ArtifactNotFound,
    /// The store answered, but not with the bytes it declared.
    ArtifactUnreadable,
    ArtifactTooLarge,
    /// The id resolves, but the stored descriptor is not a benchmark dataset.
    NotADataset,
    InvalidDataset(BenchmarkError),
    NoCommonBenchmark,
    Cancelled,
    Internal,
}
impl std::fmt::Display for BenchmarkCompareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ArtifactNotFound => f.write_str("artifact does not exist for this project"),
            Self::ArtifactUnreadable => f.write_str("artifact bytes could not be read whole"),
            Self::ArtifactTooLarge => f.write_str("artifact exceeds the comparison read ceiling"),
            Self::NotADataset => f.write_str("artifact is not a benchmark dataset"),
            Self::InvalidDataset(error) => write!(f, "invalid dataset: {error}"),
            Self::NoCommonBenchmark => f.write_str("datasets share no benchmark key"),
            Self::Cancelled => f.write_str("comparison was cancelled"),
            Self::Internal => f.write_str("comparison failed internally"),
        }
    }
}
impl std::error::Error for BenchmarkCompareError {}

impl From<ProjectError> for BenchmarkCompareError {
    fn from(value: ProjectError) -> Self {
        match value {
            ProjectError::Cancelled => Self::Cancelled,
            ProjectError::Internal => Self::Internal,
            // A project that no longer resolves owns no artifact this call
            // can see; that is the same answer as an unknown id.
            ProjectError::Rejected(_) => Self::ArtifactNotFound,
        }
    }
}

/// Turns stored dataset bytes into a [`BenchmarkDataset`].
///
/// This is a port and not a function because the application crate may depend
/// only on `rust-engineering-domain` (`scripts/check-architecture.py`), which
/// rules out `serde_json` here. The dataset's wire format is JSON, so the
/// execution adapter — which already owns every other parser — supplies the
/// implementation. Keeping the decoder behind a trait also keeps this module's
/// tests free of a real encoder: they exercise the authorization, the kind gate
/// and the streaming bound, which is what this layer actually owns.
pub trait DatasetDecoder: Send + Sync {
    fn decode(&self, bytes: &[u8]) -> Result<BenchmarkDataset, BenchmarkCompareError>;
}

/// Streams one stored artifact whole, under the comparison's own ceiling.
fn read_dataset_bytes(
    store: &mut impl QualityArtifactStore,
    owner_binding: [u8; 32],
    artifact_id: &QualityArtifactId,
    control: &dyn OperationControl,
) -> Result<Vec<u8>, BenchmarkCompareError> {
    let length =
        u32::try_from(QUALITY_RESOURCE_CHUNK_BYTES).map_err(|_| BenchmarkCompareError::Internal)?;
    let mut bytes: Vec<u8> = Vec::new();
    let mut offset: u64 = 0;
    let mut declared: Option<(u64, [u8; 32])> = None;
    loop {
        control.check()?;
        let chunk = store
            .read_chunk(owner_binding, artifact_id, offset, length)
            .map_err(|error| match error {
                QualityArtifactError::NotFound
                | QualityArtifactError::Unauthorized
                | QualityArtifactError::Expired
                | QualityArtifactError::InvalidId => BenchmarkCompareError::ArtifactNotFound,
                _ => BenchmarkCompareError::ArtifactUnreadable,
            })?;
        let descriptor = &chunk.descriptor;
        // The store's answer must be about the artifact this owner asked for.
        if descriptor.owner_binding != owner_binding
            || &descriptor.artifact_id != artifact_id
            || chunk.offset != offset
        {
            return Err(BenchmarkCompareError::ArtifactNotFound);
        }
        if descriptor.kind != QualityArtifactKind::BenchmarkDataset
            || descriptor.payload_format_version != PayloadFormatVersion::BenchmarkDatasetV2
        {
            return Err(BenchmarkCompareError::NotADataset);
        }
        if descriptor.size_bytes > COMPARE_MAX_DATASET_BYTES {
            return Err(BenchmarkCompareError::ArtifactTooLarge);
        }
        let identity = (descriptor.size_bytes, descriptor.sha256);
        match declared {
            None => declared = Some(identity),
            // Size or digest changing mid-stream means the bytes are not one
            // artifact; nothing is assembled out of two.
            Some(previous) if previous == identity => {}
            Some(_) => return Err(BenchmarkCompareError::ArtifactUnreadable),
        }
        if chunk.bytes.is_empty() {
            return if offset == descriptor.size_bytes {
                Ok(bytes)
            } else {
                Err(BenchmarkCompareError::ArtifactUnreadable)
            };
        }
        let read = u64::try_from(chunk.bytes.len()).map_err(|_| BenchmarkCompareError::Internal)?;
        offset = offset
            .checked_add(read)
            .ok_or(BenchmarkCompareError::Internal)?;
        if offset > descriptor.size_bytes {
            return Err(BenchmarkCompareError::ArtifactUnreadable);
        }
        bytes.extend_from_slice(&chunk.bytes);
        if offset == descriptor.size_bytes {
            return Ok(bytes);
        }
    }
}

impl<B: QualityProjectBackend, G: ReferenceGenerator, C: RegistryClock> ProjectRegistry<B, G, C> {
    /// Reads one artifact under a freshly revalidated, unrenewed owner grant.
    fn load_dataset(
        &mut self,
        reference: &ProjectRef,
        artifact_id: &QualityArtifactId,
        store: &mut impl QualityArtifactStore,
        decoder: &dyn DatasetDecoder,
        control: &dyn OperationControl,
    ) -> Result<BenchmarkDataset, BenchmarkCompareError> {
        control.check()?;
        // Revalidates the grant WITHOUT touching the lease: an artifact read
        // must not keep project authority alive.
        let facts = self.quality_owner_facts(reference, control)?;
        let owner_binding = store
            .owner_binding(&facts)
            .map_err(|_| BenchmarkCompareError::Internal)?;
        let bytes = read_dataset_bytes(store, owner_binding, artifact_id, control)?;
        let dataset = decoder.decode(&bytes)?;
        dataset
            .validate()
            .map_err(BenchmarkCompareError::InvalidDataset)?;
        Ok(dataset)
    }

    /// Compare two stored datasets owned by one project.
    ///
    /// Passing the same id twice is deliberately not short-circuited: the
    /// `same_artifact` rule belongs to the domain (ADR-073 §5), so both reads
    /// happen and the domain is what reports it.
    pub fn benchmark_compare(
        &mut self,
        reference: &ProjectRef,
        request: &CompareRequest,
        store: &mut impl QualityArtifactStore,
        decoder: &dyn DatasetDecoder,
        control: &dyn OperationControl,
    ) -> Result<CompareOutcome, BenchmarkCompareError> {
        let baseline = self.load_dataset(reference, &request.baseline, store, decoder, control)?;
        let candidate =
            self.load_dataset(reference, &request.candidate, store, decoder, control)?;
        control.check()?;
        match compare(&baseline, &candidate) {
            Ok(report) => Ok(CompareOutcome::Report(Box::new(report))),
            Err(CompareError::Incompatible(details)) => Ok(CompareOutcome::Incompatible(details)),
            Err(CompareError::NoCommonBenchmark) => Err(BenchmarkCompareError::NoCommonBenchmark),
            Err(CompareError::InvalidDataset(error)) => {
                Err(BenchmarkCompareError::InvalidDataset(error))
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)] // Fixed fixtures are malformed only by mistake; fail immediately.
mod tests {
    use super::*;
    use crate::benchmark::tests::{Backend, Control, TestClock, descriptor, registry};
    use crate::quality_artifact::{
        QualityArtifactChunk, QualityArtifactIndexPage, QualityArtifactInput, QualityIngest,
        QualityOwnerFacts, QualityReservation,
    };
    use rust_engineering_domain::benchmark::{
        BenchmarkHarness, BenchmarkIdentity, BenchmarkMeasurement, BenchmarkProvenance,
        BenchmarkSelection, HardwareProfile, MeasurementCompleteness, RawSample, ResourceQuotas,
        SampleUnit, SamplingMode, Virtualization,
    };
    use rust_engineering_domain::benchmark_compare::{ComparisonVerdict, IncompatibilityReason};
    use rust_engineering_domain::{
        PruneReport, QualityArtifactDescriptor, QualityJobId, RecoveryReport,
    };
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // -- dataset fixtures ----------------------------------------------------

    fn selection() -> BenchmarkSelection {
        BenchmarkSelection {
            package: Some("member".into()),
            bench_target: Some("throughput".into()),
            features: vec!["std".into()],
            all_features: false,
            no_default_features: false,
            profile: "bench".into(),
        }
    }

    fn measurement(key: &str, values: &[f64]) -> BenchmarkMeasurement {
        BenchmarkMeasurement::new(
            BenchmarkIdentity::new(
                "group".into(),
                Some("function".into()),
                None,
                key.into(),
                key.replace('/', "_"),
            )
            .unwrap(),
            SamplingMode::Flat,
            values
                .iter()
                .enumerate()
                // Dealt over the three executions the provenance declares, so
                // the frozen method has a between-execution estimate to use.
                .map(|(index, value)| RawSample::new(1, *value, (index % 3) as u8 + 1).unwrap())
                .collect(),
            3_000,
            5_000,
            u32::try_from(values.len()).unwrap_or(u32::MAX),
            MeasurementCompleteness::Complete,
        )
        .unwrap()
    }

    fn dataset(
        execution: &str,
        rust_version: &str,
        measurements: Vec<BenchmarkMeasurement>,
    ) -> BenchmarkDataset {
        BenchmarkDataset::new(
            SampleUnit::Nanoseconds,
            measurements,
            BenchmarkProvenance {
                source_fingerprint: format!("sha256:{}", "a".repeat(64)),
                harness: BenchmarkHarness::Criterion,
                harness_version: "0.8.2".into(),
                rust_version: rust_version.into(),
                cargo_version: "1.98.1".into(),
                declared_toolchain: Some("1.98.1".into()),
                image_digest: format!("sha256:{}", "c".repeat(64)),
                platform: "aarch64-unknown-linux-gnu".into(),
                configuration_fingerprint: format!("sha256:{}", "d".repeat(64)),
                execution_fingerprint: execution.into(),
                selection: selection(),
                hardware: HardwareProfile {
                    cpu_model: Some("Neoverse-N1".into()),
                    cpu_cores: Some(4),
                    os_kernel: Some("Linux 6.6.0".into()),
                    arch: "aarch64".into(),
                    virtualization: Virtualization::Container,
                    cpu_governor: Some("performance".into()),
                    quotas: ResourceQuotas {
                        cpu_quota_millicores: Some(2_000),
                        memory_bytes: Some(2 << 30),
                        pids: Some(256),
                    },
                },
                run_index: 1,
                run_count: 3,
                captured_at_unix: 1_757_000_000,
            },
        )
        .unwrap()
    }

    /// Deterministic jitter so every comparison below is reproducible.
    fn jitter(seed: u64, count: usize, center: f64) -> Vec<f64> {
        let mut state = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15);
        (0..count)
            .map(|_| {
                state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
                let mut z = state;
                z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
                z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
                let unit = ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64;
                center * (1.0 + (unit - 0.5) * 0.02)
            })
            .collect()
    }

    // -- store double --------------------------------------------------------

    const BASELINE: [u8; 16] = [1; 16];
    const CANDIDATE: [u8; 16] = [2; 16];
    const OTHER_KIND: [u8; 16] = [3; 16];
    const OVERSIZE: [u8; 16] = [4; 16];

    fn id(bytes: [u8; 16]) -> QualityArtifactId {
        QualityArtifactId::from_random_bytes(bytes)
    }

    /// The owner binding a live `/trusted/project` grant produces. Any other
    /// binding belongs to another project and resolves to nothing.
    const OWNER: [u8; 32] = [0x5a; 32];
    const FOREIGN_OWNER: [u8; 32] = [0x77; 32];

    struct Stored {
        descriptor: QualityArtifactDescriptor,
        bytes: Vec<u8>,
    }

    #[derive(Default)]
    struct Store {
        rows: HashMap<String, Stored>,
        reads: Arc<AtomicUsize>,
        /// Serves at most this many bytes per chunk, to exercise the loop.
        chunk_bytes: usize,
    }

    impl Store {
        fn new() -> Self {
            Self {
                rows: HashMap::new(),
                reads: Arc::new(AtomicUsize::new(0)),
                chunk_bytes: QUALITY_RESOURCE_CHUNK_BYTES,
            }
        }
        fn insert(
            &mut self,
            artifact: [u8; 16],
            kind: QualityArtifactKind,
            owner: [u8; 32],
            bytes: Vec<u8>,
            declared_size: Option<u64>,
        ) {
            let mut stored = descriptor(kind);
            stored.artifact_id = id(artifact);
            stored.owner_binding = owner;
            stored.size_bytes =
                declared_size.unwrap_or_else(|| u64::try_from(bytes.len()).unwrap_or(u64::MAX));
            self.rows.insert(
                id(artifact).to_string(),
                Stored {
                    descriptor: stored,
                    bytes,
                },
            );
        }
    }

    impl QualityArtifactStore for Store {
        fn owner_binding(
            &self,
            facts: &QualityOwnerFacts,
        ) -> Result<[u8; 32], QualityArtifactError> {
            // One granted root, one binding. A different root would bind
            // differently and therefore see nothing.
            if facts.workspace_root == "/trusted/project" {
                Ok(OWNER)
            } else {
                Ok(FOREIGN_OWNER)
            }
        }
        fn reserve(&mut self, _: &QualityReservation) -> Result<(), QualityArtifactError> {
            Err(QualityArtifactError::Unauthorized)
        }
        fn release(&mut self, _: &QualityReservation) -> Result<(), QualityArtifactError> {
            Err(QualityArtifactError::Unauthorized)
        }
        fn ingest_member(
            &mut self,
            _: &QualityReservation,
            _: u16,
            _: u64,
            _: &mut dyn QualityArtifactInput,
        ) -> Result<QualityIngest, QualityArtifactError> {
            Err(QualityArtifactError::Unauthorized)
        }
        fn publish_descriptor(
            &mut self,
            _: &QualityReservation,
            _: &QualityArtifactDescriptor,
        ) -> Result<(), QualityArtifactError> {
            Err(QualityArtifactError::Unauthorized)
        }
        fn read_chunk(
            &mut self,
            owner_binding: [u8; 32],
            artifact_id: &QualityArtifactId,
            offset: u64,
            length: u32,
        ) -> Result<QualityArtifactChunk, QualityArtifactError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            let stored = self
                .rows
                .get(&artifact_id.to_string())
                .ok_or(QualityArtifactError::NotFound)?;
            if stored.descriptor.owner_binding != owner_binding {
                return Err(QualityArtifactError::NotFound);
            }
            let start = usize::try_from(offset).map_err(|_| QualityArtifactError::NotFound)?;
            let start = start.min(stored.bytes.len());
            let want = usize::try_from(length)
                .map_err(|_| QualityArtifactError::NotFound)?
                .min(self.chunk_bytes);
            let end = start.saturating_add(want).min(stored.bytes.len());
            Ok(QualityArtifactChunk {
                descriptor: stored.descriptor.clone(),
                offset,
                bytes: stored.bytes[start..end].to_vec(),
            })
        }
        fn read_index_page(
            &mut self,
            _: [u8; 32],
            _: &QualityJobId,
            _: Option<&[u8]>,
        ) -> Result<QualityArtifactIndexPage, QualityArtifactError> {
            Err(QualityArtifactError::NotFound)
        }
        fn reconcile_recover(&mut self) -> Result<RecoveryReport, QualityArtifactError> {
            Err(QualityArtifactError::Unauthorized)
        }
        fn prune_expired(&mut self) -> Result<PruneReport, QualityArtifactError> {
            Err(QualityArtifactError::Unauthorized)
        }
    }

    // -- decoder double ------------------------------------------------------

    /// Maps stored bytes to a prepared dataset. The real decoder is JSON and
    /// lives in the execution adapter; nothing here needs a real encoder.
    #[derive(Default)]
    struct Decoder {
        rows: HashMap<Vec<u8>, BenchmarkDataset>,
        calls: Arc<AtomicUsize>,
    }
    impl DatasetDecoder for Decoder {
        fn decode(&self, bytes: &[u8]) -> Result<BenchmarkDataset, BenchmarkCompareError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.rows
                .get(bytes)
                .cloned()
                .ok_or(BenchmarkCompareError::InvalidDataset(
                    BenchmarkError::UnknownFormat,
                ))
        }
    }

    // -- harness -------------------------------------------------------------

    struct Fixture {
        registry: crate::benchmark::tests::TestRegistry,
        project: ProjectRef,
        store: Store,
        decoder: Decoder,
        control: Control,
        backend: Backend,
        clock: TestClock,
    }

    fn fixture() -> Fixture {
        let backend = Backend::default();
        let clock = TestClock::at(0);
        let control = Control::default();
        let mut registry = registry(backend.clone(), clock.clone());
        let project = registry
            .open("/trusted/project", &control)
            .unwrap()
            .project_ref;
        Fixture {
            registry,
            project,
            store: Store::new(),
            decoder: Decoder::default(),
            control,
            backend,
            clock,
        }
    }

    impl Fixture {
        fn compare(&mut self, baseline: [u8; 16], candidate: [u8; 16]) -> CompareOutcome {
            self.try_compare(baseline, candidate).expect("compared")
        }
        fn try_compare(
            &mut self,
            baseline: [u8; 16],
            candidate: [u8; 16],
        ) -> Result<CompareOutcome, BenchmarkCompareError> {
            self.registry.benchmark_compare(
                &self.project,
                &CompareRequest {
                    baseline: id(baseline),
                    candidate: id(candidate),
                },
                &mut self.store,
                &self.decoder,
                &self.control,
            )
        }
    }

    fn seeded() -> Fixture {
        let mut fixture = fixture();
        let baseline = dataset(
            "exec-baseline",
            "1.98.1",
            vec![measurement("bench/one", &jitter(1, 60, 1_000.0))],
        );
        let candidate = dataset(
            "exec-candidate",
            "1.98.1",
            vec![measurement("bench/one", &jitter(2, 60, 1_200.0))],
        );
        fixture.store.insert(
            BASELINE,
            QualityArtifactKind::BenchmarkDataset,
            OWNER,
            b"baseline-bytes".to_vec(),
            None,
        );
        fixture.store.insert(
            CANDIDATE,
            QualityArtifactKind::BenchmarkDataset,
            OWNER,
            b"candidate-bytes".to_vec(),
            None,
        );
        fixture
            .decoder
            .rows
            .insert(b"baseline-bytes".to_vec(), baseline);
        fixture
            .decoder
            .rows
            .insert(b"candidate-bytes".to_vec(), candidate);
        fixture
    }

    // -- tests ---------------------------------------------------------------

    #[test]
    fn two_authorized_datasets_reach_the_domain_and_produce_a_report() {
        let mut fixture = seeded();
        let outcome = fixture.compare(BASELINE, CANDIDATE);
        let CompareOutcome::Report(report) = outcome else {
            unreachable!("expected a report")
        };
        assert_eq!(report.compared, 1);
        assert!(report.baseline_only.is_empty());
        assert!(report.candidate_only.is_empty());
        let comparison = report.comparisons.first().expect("one comparison");
        assert_eq!(comparison.key, "bench/one");
        assert_eq!(comparison.verdict, ComparisonVerdict::Regression);
    }

    #[test]
    fn reading_artifacts_revalidates_the_owner_without_renewing_the_lease() {
        let mut fixture = seeded();
        // Nine seconds into a ten-second TTL, opened at zero.
        fixture.clock.set(9);
        let _ = fixture.compare(BASELINE, CANDIDATE);
        // One revalidation per artifact, and no touch: at the TTL boundary the
        // reference is gone. A renewing read would have kept it alive.
        assert_eq!(fixture.backend.owner_validations.load(Ordering::SeqCst), 2);
        fixture.clock.set(10);
        assert_eq!(
            fixture
                .registry
                .resolve(&fixture.project, &fixture.control)
                .err(),
            Some(ProjectError::Rejected(
                rust_engineering_domain::OperationalErrorCode::ProjectNotFound
            ))
        );
    }

    #[test]
    fn an_unknown_artifact_id_does_not_exist() {
        let mut fixture = seeded();
        assert_eq!(
            fixture.try_compare([9; 16], CANDIDATE).err(),
            Some(BenchmarkCompareError::ArtifactNotFound)
        );
        assert_eq!(fixture.decoder.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn an_artifact_owned_by_another_project_does_not_resolve() {
        let mut fixture = seeded();
        // Same id grammar, same store, a different owner binding.
        fixture.store.insert(
            CANDIDATE,
            QualityArtifactKind::BenchmarkDataset,
            FOREIGN_OWNER,
            b"candidate-bytes".to_vec(),
            None,
        );
        assert_eq!(
            fixture.try_compare(BASELINE, CANDIDATE).err(),
            Some(BenchmarkCompareError::ArtifactNotFound)
        );
    }

    #[test]
    fn a_junit_artifact_id_is_not_a_dataset_rather_than_a_parse_failure() {
        let mut fixture = seeded();
        fixture.store.insert(
            OTHER_KIND,
            QualityArtifactKind::JunitXml,
            OWNER,
            b"<testsuite/>".to_vec(),
            None,
        );
        assert_eq!(
            fixture.try_compare(OTHER_KIND, CANDIDATE).err(),
            Some(BenchmarkCompareError::NotADataset)
        );
        // The kind gate runs before a byte reaches the decoder.
        assert_eq!(fixture.decoder.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn an_artifact_larger_than_the_read_ceiling_is_refused_before_it_is_read() {
        let mut fixture = seeded();
        fixture.store.insert(
            OVERSIZE,
            QualityArtifactKind::BenchmarkDataset,
            OWNER,
            b"baseline-bytes".to_vec(),
            Some(COMPARE_MAX_DATASET_BYTES + 1),
        );
        assert_eq!(
            fixture.try_compare(OVERSIZE, CANDIDATE).err(),
            Some(BenchmarkCompareError::ArtifactTooLarge)
        );
        assert_eq!(fixture.decoder.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_store_that_stops_short_of_its_declared_size_is_unreadable() {
        let mut fixture = seeded();
        fixture.store.insert(
            OVERSIZE,
            QualityArtifactKind::BenchmarkDataset,
            OWNER,
            b"short".to_vec(),
            Some(4_096),
        );
        assert_eq!(
            fixture.try_compare(OVERSIZE, CANDIDATE).err(),
            Some(BenchmarkCompareError::ArtifactUnreadable)
        );
    }

    #[test]
    fn a_multi_chunk_artifact_is_assembled_whole() {
        let mut fixture = seeded();
        let payload: Vec<u8> = (0..1_000u32).map(|value| value as u8).collect();
        fixture.store.chunk_bytes = 7;
        fixture.store.insert(
            BASELINE,
            QualityArtifactKind::BenchmarkDataset,
            OWNER,
            payload.clone(),
            None,
        );
        let baseline = dataset(
            "exec-baseline",
            "1.98.1",
            vec![measurement("bench/one", &jitter(1, 60, 1_000.0))],
        );
        fixture.decoder.rows.insert(payload, baseline);
        assert!(matches!(
            fixture.compare(BASELINE, CANDIDATE),
            CompareOutcome::Report(_)
        ));
        assert!(fixture.store.reads.load(Ordering::SeqCst) > 100);
    }

    #[test]
    fn the_same_id_twice_reaches_the_domain_and_returns_same_artifact() {
        let mut fixture = seeded();
        let outcome = fixture.compare(BASELINE, BASELINE);
        let details = match outcome {
            CompareOutcome::Incompatible(details) => Some(details),
            CompareOutcome::Report(_) => None,
        }
        .expect("expected an incompatible pair");
        assert_eq!(details.reasons, vec![IncompatibilityReason::SameArtifact]);
        // Both sides were actually read; nothing was short-circuited here.
        assert_eq!(fixture.decoder.calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn an_incompatible_pair_is_an_observed_result_and_not_an_error() {
        let mut fixture = seeded();
        fixture.decoder.rows.insert(
            b"candidate-bytes".to_vec(),
            dataset(
                "exec-candidate",
                "1.99.0",
                vec![measurement("bench/one", &jitter(2, 60, 1_200.0))],
            ),
        );
        let details = match fixture.compare(BASELINE, CANDIDATE) {
            CompareOutcome::Incompatible(details) => Some(details),
            CompareOutcome::Report(_) => None,
        }
        .expect("expected an incompatible pair");
        assert_eq!(details.reasons, vec![IncompatibilityReason::RustVersion]);
        // The refusal reaches this layer with the two values behind it, so the
        // adapter can publish what actually differed.
        assert_eq!(details.baseline_provenance.rust_version, "1.98.1");
        assert_eq!(details.candidate_provenance.rust_version, "1.99.0");
    }

    #[test]
    fn compatible_datasets_that_share_no_key_are_an_error() {
        let mut fixture = seeded();
        fixture.decoder.rows.insert(
            b"candidate-bytes".to_vec(),
            dataset(
                "exec-candidate",
                "1.98.1",
                vec![measurement("bench/two", &jitter(2, 60, 1_200.0))],
            ),
        );
        assert_eq!(
            fixture.try_compare(BASELINE, CANDIDATE).err(),
            Some(BenchmarkCompareError::NoCommonBenchmark)
        );
    }

    #[test]
    fn a_payload_the_decoder_rejects_surfaces_as_an_invalid_dataset() {
        let mut fixture = seeded();
        fixture.decoder.rows.remove(b"baseline-bytes".as_slice());
        assert_eq!(
            fixture.try_compare(BASELINE, CANDIDATE).err(),
            Some(BenchmarkCompareError::InvalidDataset(
                BenchmarkError::UnknownFormat
            ))
        );
    }

    #[test]
    fn cancellation_propagates_from_the_control_and_reads_nothing() {
        let mut fixture = seeded();
        fixture.control.cancel();
        assert_eq!(
            fixture.try_compare(BASELINE, CANDIDATE).err(),
            Some(BenchmarkCompareError::Cancelled)
        );
        assert_eq!(fixture.store.reads.load(Ordering::SeqCst), 0);
        assert_eq!(fixture.decoder.calls.load(Ordering::SeqCst), 0);
    }
}
