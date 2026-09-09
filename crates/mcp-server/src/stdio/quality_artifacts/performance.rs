//! Durable M5 performance evidence in the unchanged ADR-061 artifact format.
//!
//! The three publishers below share one rule: a member is published only when
//! the observation actually carries its bytes, and its `completeness` is read
//! off that observation rather than asserted. Zero samples, a refused profiler,
//! an unrecognised harness and an unavailable analyzer are all declared
//! results (ADR-076 §3, §5, §6), so each of them legitimately publishes fewer
//! members — or none — instead of committing an artifact that claims to
//! describe evidence that does not exist.
use super::*;
use rust_engineering_application::benchmark::{BenchmarkObservation, BenchmarkPublisher};
use rust_engineering_application::bloat::{BloatObservation, BloatPublisher};
use rust_engineering_application::profile::{ProfileObservation, ProfilePublisher};
use rust_engineering_application::security::SecurityCapture;
use rust_engineering_domain::benchmark::{BenchmarkDataset, MeasurementCompleteness};
use rust_engineering_domain::benchmark_run::HarnessDetection;
use rust_engineering_domain::bloat::BloatCompleteness;
use rust_engineering_domain::profile::ProfileCompleteness;
use rust_engineering_domain::{ExecutionFingerprint, QualityArtifactDescriptor, RuntimeIdentity};

/// Every M5 artifact is at most `SymbolDerived`: a dataset and a size report
/// are derived from the project's own source, and stacks and a flame graph
/// carry symbol names. None of them is a candidate for the widest grant, so
/// this producer asks for exactly the retention its evidence needs.
const M5_RETENTION: QualityRetentionGrant = QualityRetentionGrant::SourceDerived;

#[derive(Clone)]
pub(in crate::stdio) struct DurablePerformancePublisher {
    pub(super) store: Arc<Mutex<NativeQualityArtifactStore>>,
}

impl DurablePerformancePublisher {
    pub(in crate::stdio) fn benchmark(&self) -> Arc<Mutex<dyn BenchmarkPublisher>> {
        Arc::new(Mutex::new(self.clone()))
    }
    pub(in crate::stdio) fn profile(&self) -> Arc<Mutex<dyn ProfilePublisher>> {
        Arc::new(Mutex::new(self.clone()))
    }
    pub(in crate::stdio) fn bloat(&self) -> Arc<Mutex<dyn BloatPublisher>> {
        Arc::new(Mutex::new(self.clone()))
    }

    fn publish(
        &mut self,
        capture: &SecurityCapture,
        selection: ArtifactSelection,
        runtime: ArtifactRuntime,
        members: &[JobMember<'_>],
        revalidate: &mut dyn FnMut() -> Result<QualityOwnerFacts, InspectionError>,
    ) -> Result<Vec<QualityArtifactDescriptor>, InspectionError> {
        publish_job(
            &self.store,
            Job {
                project: &capture.project_ref,
                captured_at: capture.captured_at,
                source: &capture.source,
                selection,
                runtime,
                retention: M5_RETENTION,
            },
            members,
            revalidate,
        )
    }
}

impl BenchmarkPublisher for DurablePerformancePublisher {
    fn publish_benchmark(
        &mut self,
        capture: &SecurityCapture,
        observation: &BenchmarkObservation,
        revalidate: &mut dyn FnMut() -> Result<QualityOwnerFacts, InspectionError>,
    ) -> Result<Vec<QualityArtifactDescriptor>, InspectionError> {
        // A run without a dataset published nothing to describe: an
        // unrecognised or unapproved harness, a failed execution and an
        // unparsable export are all declared omissions (ADR-076 §3), and the
        // empty vector is the application's own legitimate answer for them.
        let Some(dataset) = observation.dataset.as_ref() else {
            return Ok(Vec::new());
        };
        let bytes = serde_json::to_vec(dataset).map_err(|_| InspectionError::Internal)?;
        let members = benchmark_members(observation, dataset, &bytes);
        self.publish(
            capture,
            benchmark_selection(observation),
            performance_runtime(
                &observation.runtime,
                &observation.execution_fingerprint,
                ArtifactPlugin {
                    identity: PluginIdentity::Criterion,
                    version: 1,
                    digest: plugin_digest(b"criterion", harness_version(&observation.harness)),
                },
            )?,
            &members,
            revalidate,
        )
    }
}

impl ProfilePublisher for DurablePerformancePublisher {
    fn publish_profile(
        &mut self,
        capture: &SecurityCapture,
        observation: &ProfileObservation,
        revalidate: &mut dyn FnMut() -> Result<QualityOwnerFacts, InspectionError>,
    ) -> Result<Vec<QualityArtifactDescriptor>, InspectionError> {
        let completeness = profile_completeness(observation.completeness);
        // Zero samples and a refused `perf_event_open` render no graph and fold
        // no stacks (ADR-074 §5). An empty member is not evidence of a profile,
        // so the absent payload stays absent instead of becoming a zero-byte
        // artifact the response would then have to explain away.
        let members: Vec<JobMember<'_>> = [
            (
                QualityArtifactKind::FlamegraphSvg,
                QualityMimeType::ImageSvgXml,
                PayloadFormatVersion::FlamegraphSvgV1,
                GuestArtifactName::FlamegraphSvg,
                observation.svg.as_slice(),
            ),
            (
                QualityArtifactKind::CollapsedStacks,
                QualityMimeType::TextPlain,
                PayloadFormatVersion::CollapsedStacksV1,
                GuestArtifactName::CollapsedStacks,
                observation.stacks.as_slice(),
            ),
        ]
        .into_iter()
        .filter(|(_, _, _, _, bytes)| !bytes.is_empty())
        .map(
            |(kind, mime_type, payload_format_version, guest_name, bytes)| JobMember {
                kind,
                mime_type,
                payload_format_version,
                guest_name,
                // Both members carry resolved symbol names of the profiled
                // binary; neither carries project source text.
                sensitivity: ArtifactSensitivity::SymbolDerived,
                completeness,
                bytes,
            },
        )
        .collect();
        self.publish(
            capture,
            // A profile always names one cargo binary target.
            ArtifactSelection::Target,
            performance_runtime(
                &observation.runtime,
                &observation.execution_fingerprint,
                ArtifactPlugin {
                    identity: PluginIdentity::ProfileHelper,
                    version: 1,
                    digest: plugin_digest(b"rust-mcp-profile-helper", observation.backend),
                },
            )?,
            &members,
            revalidate,
        )
    }
}

impl BloatPublisher for DurablePerformancePublisher {
    fn publish_bloat(
        &mut self,
        capture: &SecurityCapture,
        observation: &BloatObservation,
        revalidate: &mut dyn FnMut() -> Result<QualityOwnerFacts, InspectionError>,
    ) -> Result<Vec<QualityArtifactDescriptor>, InspectionError> {
        // A build failure, an unsupported format and an unavailable analyzer
        // produce no report; the tool declares that and publishes no artifact.
        if observation.report.is_empty() {
            return Ok(Vec::new());
        }
        let members = [JobMember {
            kind: QualityArtifactKind::BloatJson,
            mime_type: QualityMimeType::ApplicationJson,
            payload_format_version: PayloadFormatVersion::BloatJsonV1,
            guest_name: GuestArtifactName::BloatJson,
            sensitivity: ArtifactSensitivity::SourceDerived,
            completeness: bloat_completeness(observation.completeness),
            bytes: &observation.report,
        }];
        self.publish(
            capture,
            // A size analysis always names one cargo binary target.
            ArtifactSelection::Target,
            performance_runtime(
                &observation.runtime,
                &observation.execution_fingerprint,
                ArtifactPlugin {
                    identity: PluginIdentity::Bloat,
                    version: 1,
                    digest: plugin_digest(b"cargo-bloat", &observation.analyzer_version),
                },
            )?,
            &members,
            revalidate,
        )
    }
}

fn performance_runtime(
    runtime: &RuntimeIdentity,
    execution_fingerprint: &ExecutionFingerprint,
    plugin: ArtifactPlugin,
) -> Result<ArtifactRuntime, InspectionError> {
    Ok(ArtifactRuntime {
        image_digest: digest_text(&runtime.image_id)?,
        toolchain_identity: toolchain_identity(runtime),
        plugin,
        implementation_digest: digest_text(&execution_fingerprint.to_string())?,
    })
}

/// Identity of the pinned backend that produced a member, over its own name and
/// the exact version the run observed. It is an identity, not a file digest:
/// the binaries live in the guest image, whose digest the descriptor already
/// carries separately.
fn plugin_digest(name: &[u8], version: &str) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"rust-mcp/quality-plugin/v1\0");
    digest.update(name);
    digest.update([0]);
    digest.update(version.as_bytes());
    digest.finalize().into()
}

/// The harness version the run resolved. A dataset only ever comes from the
/// approved harness, but an unapproved or absent one is still named rather than
/// silently reported as the approved version.
fn harness_version(harness: &HarnessDetection) -> &str {
    match harness {
        HarnessDetection::Criterion { version }
        | HarnessDetection::CriterionUnapproved { version } => version,
        HarnessDetection::Unrecognized => "",
    }
}

fn benchmark_selection(observation: &BenchmarkObservation) -> ArtifactSelection {
    if observation.selection.bench_target.is_some() {
        ArtifactSelection::Target
    } else if observation.selection.package.is_some() {
        ArtifactSelection::Package
    } else {
        ArtifactSelection::Workspace
    }
}

/// What the dataset member is, as the run observed it: a truncated sample set
/// is truncated evidence, and fewer completed repetitions than were requested,
/// or a measurement the harness never produced, is partial evidence.
fn dataset_completeness(
    observation: &BenchmarkObservation,
    dataset: &BenchmarkDataset,
) -> ArtifactCompleteness {
    let measurements = dataset.measurements();
    let any = |wanted: MeasurementCompleteness| {
        measurements
            .iter()
            .any(|measurement| measurement.completeness() == wanted)
    };
    if any(MeasurementCompleteness::Truncated) {
        ArtifactCompleteness::Truncated
    } else if any(MeasurementCompleteness::Missing)
        || observation.runs_completed < observation.runs_requested
    {
        ArtifactCompleteness::Partial
    } else {
        ArtifactCompleteness::Complete
    }
}

/// The members one benchmark run publishes, in the order they are committed.
///
/// The dataset is member zero. The criterion output tree is the second member
/// ADR-076 §3 names, and it is published only when the run actually retained
/// one repetition's tree: a run that exported none, or whose export exceeded
/// the 32 MiB ceiling of ADR-076 §7, carries a declared `archive_omission`
/// instead and publishes the dataset alone rather than a member describing
/// bytes this server does not hold. The tree travels verbatim, so it is the
/// same source-derived evidence the dataset is, in the framing the harness
/// wrote it in.
fn benchmark_members<'a>(
    observation: &'a BenchmarkObservation,
    dataset: &BenchmarkDataset,
    dataset_bytes: &'a [u8],
) -> Vec<JobMember<'a>> {
    let mut members = vec![JobMember {
        kind: QualityArtifactKind::BenchmarkDataset,
        mime_type: QualityMimeType::ApplicationJson,
        payload_format_version: PayloadFormatVersion::BenchmarkDatasetV2,
        guest_name: GuestArtifactName::BenchmarkDataset,
        sensitivity: ArtifactSensitivity::SourceDerived,
        completeness: dataset_completeness(observation, dataset),
        bytes: dataset_bytes,
    }];
    if let Some(archive) = observation.archive.as_ref() {
        members.push(JobMember {
            kind: QualityArtifactKind::CriterionArchive,
            mime_type: QualityMimeType::ApplicationXTar,
            payload_format_version: PayloadFormatVersion::UstarV1,
            guest_name: GuestArtifactName::CriterionArchive,
            sensitivity: ArtifactSensitivity::SourceDerived,
            completeness: archive_completeness(observation),
            bytes: &archive.bytes,
        });
    }
    members
}

/// What the criterion archive member is, as the run observed it.
///
/// It is never `Truncated`. The tree travels verbatim and a USTAR stream cut
/// short is not an archive, so an oversize export is refused upstream and
/// declared as an omission rather than published as a partial file.
///
/// What remains is whether the retained tree is the run's *whole* harness
/// output. ADR-073 §2 gives every repetition its own `CRITERION_HOME`, and this
/// member is one repetition's tree — never a merge of several, which would be a
/// layout criterion never wrote. So the tree is the complete harness output
/// only when a single repetition was requested and that repetition completed;
/// a three-repetition run publishes one of its three trees, and that is partial
/// evidence of the run whatever else was measured.
fn archive_completeness(observation: &BenchmarkObservation) -> ArtifactCompleteness {
    if observation.runs_requested == 1 && observation.runs_completed == 1 {
        ArtifactCompleteness::Complete
    } else {
        ArtifactCompleteness::Partial
    }
}

fn profile_completeness(value: ProfileCompleteness) -> ArtifactCompleteness {
    match value {
        ProfileCompleteness::Complete => ArtifactCompleteness::Complete,
        ProfileCompleteness::Truncated => ArtifactCompleteness::Truncated,
        // Both describe a profile the sampler really produced and really
        // could not finish: samples the kernel dropped, or none at all.
        ProfileCompleteness::LostSamples | ProfileCompleteness::NoSamples => {
            ArtifactCompleteness::Partial
        }
        ProfileCompleteness::Unavailable => ArtifactCompleteness::Unavailable,
    }
}

fn bloat_completeness(value: BloatCompleteness) -> ArtifactCompleteness {
    match value {
        BloatCompleteness::Complete => ArtifactCompleteness::Complete,
        BloatCompleteness::Truncated => ArtifactCompleteness::Truncated,
        // The analyzer's own file size disagreed with the size this product
        // measured: the ranking describes some other file (ADR-076 §6).
        BloatCompleteness::SizeMismatch => ArtifactCompleteness::Invalid,
        BloatCompleteness::UnsupportedFormat | BloatCompleteness::Unavailable => {
            ArtifactCompleteness::Unavailable
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)] // Fixed fixtures are malformed only by mistake; fail immediately.
mod tests {
    use super::*;
    use rust_engineering_domain::benchmark::BenchmarkSelection;
    use rust_engineering_domain::benchmark_run::DatasetOmission;

    fn selection(package: Option<&str>, bench_target: Option<&str>) -> BenchmarkSelection {
        BenchmarkSelection {
            package: package.map(str::to_owned),
            bench_target: bench_target.map(str::to_owned),
            features: Vec::new(),
            all_features: false,
            no_default_features: false,
            profile: "bench".into(),
        }
    }

    #[test]
    fn every_declared_member_pairs_a_kind_with_the_format_the_store_accepts() {
        // The store rejects any other pairing; restating them here keeps a
        // publisher change from reaching the store as a runtime refusal.
        for (kind, mime_type, payload_format_version, guest_name) in [
            (
                QualityArtifactKind::BenchmarkDataset,
                QualityMimeType::ApplicationJson,
                PayloadFormatVersion::BenchmarkDatasetV2,
                GuestArtifactName::BenchmarkDataset,
            ),
            (
                QualityArtifactKind::CriterionArchive,
                QualityMimeType::ApplicationXTar,
                PayloadFormatVersion::UstarV1,
                GuestArtifactName::CriterionArchive,
            ),
            (
                QualityArtifactKind::FlamegraphSvg,
                QualityMimeType::ImageSvgXml,
                PayloadFormatVersion::FlamegraphSvgV1,
                GuestArtifactName::FlamegraphSvg,
            ),
            (
                QualityArtifactKind::CollapsedStacks,
                QualityMimeType::TextPlain,
                PayloadFormatVersion::CollapsedStacksV1,
                GuestArtifactName::CollapsedStacks,
            ),
            (
                QualityArtifactKind::BloatJson,
                QualityMimeType::ApplicationJson,
                PayloadFormatVersion::BloatJsonV1,
                GuestArtifactName::BloatJson,
            ),
        ] {
            let created =
                UtcInstant::from_unix_seconds(1_788_000_000).expect("representable instant");
            let descriptor = QualityArtifactDraft {
                artifact_id: QualityArtifactId::from_random_bytes([1; 16]),
                member_index: 0,
                kind,
                mime_type,
                payload_format_version,
                completeness: ArtifactCompleteness::Complete,
                sensitivity: ArtifactSensitivity::SourceDerived,
                created_at_utc: created.clone(),
                expires_at_utc: created
                    .checked_add_seconds(QUALITY_DEFAULT_TTL_SECONDS)
                    .expect("representable expiry"),
                source: ArtifactSource {
                    captured_source_sha256: [2; 32],
                    guest_name,
                    selection: ArtifactSelection::Target,
                },
                runtime: ArtifactRuntime {
                    image_digest: [3; 32],
                    toolchain_identity: [4; 32],
                    plugin: ArtifactPlugin {
                        identity: PluginIdentity::Criterion,
                        version: 1,
                        digest: [5; 32],
                    },
                    implementation_digest: [6; 32],
                },
            }
            .into_descriptor(
                QualityJobId::from_random_bytes([7; 16]),
                [8; 32],
                [9; 32],
                64,
            );
            assert!(descriptor.is_ok(), "store rejects {kind:?}");
        }
    }

    #[test]
    fn every_m5_sensitivity_fits_the_retention_this_producer_asks_for() {
        for sensitivity in [
            ArtifactSensitivity::SourceDerived,
            ArtifactSensitivity::SymbolDerived,
        ] {
            assert!(M5_RETENTION.permits(sensitivity));
        }
        // The widest grant is never needed, so it is never requested.
        assert!(!M5_RETENTION.permits(ArtifactSensitivity::PotentiallySensitive));
    }

    #[test]
    fn completeness_is_read_off_the_observation_and_never_asserted() {
        assert_eq!(
            profile_completeness(ProfileCompleteness::Complete),
            ArtifactCompleteness::Complete
        );
        assert_eq!(
            profile_completeness(ProfileCompleteness::Truncated),
            ArtifactCompleteness::Truncated
        );
        assert_eq!(
            profile_completeness(ProfileCompleteness::LostSamples),
            ArtifactCompleteness::Partial
        );
        assert_eq!(
            profile_completeness(ProfileCompleteness::NoSamples),
            ArtifactCompleteness::Partial
        );
        assert_eq!(
            profile_completeness(ProfileCompleteness::Unavailable),
            ArtifactCompleteness::Unavailable
        );
        assert_eq!(
            bloat_completeness(BloatCompleteness::Complete),
            ArtifactCompleteness::Complete
        );
        assert_eq!(
            bloat_completeness(BloatCompleteness::SizeMismatch),
            ArtifactCompleteness::Invalid
        );
        assert_eq!(
            bloat_completeness(BloatCompleteness::Truncated),
            ArtifactCompleteness::Truncated
        );
        assert_eq!(
            bloat_completeness(BloatCompleteness::UnsupportedFormat),
            ArtifactCompleteness::Unavailable
        );
        assert_eq!(
            bloat_completeness(BloatCompleteness::Unavailable),
            ArtifactCompleteness::Unavailable
        );
    }

    #[test]
    fn a_benchmark_selection_names_the_narrowest_thing_the_run_selected() {
        let mut observation = fixture::observation();
        observation.selection = selection(None, None);
        assert_eq!(
            benchmark_selection(&observation),
            ArtifactSelection::Workspace
        );
        observation.selection = selection(Some("member"), None);
        assert_eq!(
            benchmark_selection(&observation),
            ArtifactSelection::Package
        );
        observation.selection = selection(Some("member"), Some("throughput"));
        assert_eq!(benchmark_selection(&observation), ArtifactSelection::Target);
    }

    #[test]
    fn an_incomplete_run_never_publishes_a_dataset_described_as_complete() {
        let observation = fixture::observation();
        let dataset = fixture::dataset(MeasurementCompleteness::Complete);
        assert_eq!(
            dataset_completeness(&observation, &dataset),
            ArtifactCompleteness::Complete
        );
        let mut partial = fixture::observation();
        partial.runs_completed = partial.runs_requested - 1;
        assert_eq!(
            dataset_completeness(&partial, &dataset),
            ArtifactCompleteness::Partial
        );
        assert_eq!(
            dataset_completeness(
                &observation,
                &fixture::dataset(MeasurementCompleteness::Truncated)
            ),
            ArtifactCompleteness::Truncated
        );
        assert_eq!(
            dataset_completeness(
                &observation,
                &fixture::dataset(MeasurementCompleteness::Missing)
            ),
            ArtifactCompleteness::Partial
        );
    }

    /// ADR-076 §3 promises two members. When the run retained a tree, both are
    /// declared, in that order, and the archive member carries the exported
    /// bytes verbatim rather than anything derived from them.
    #[test]
    fn a_retained_tree_is_published_as_the_second_member() {
        let observation = fixture::observation();
        let dataset = fixture::dataset(MeasurementCompleteness::Complete);
        let bytes = serde_json::to_vec(&dataset).expect("dataset bytes");
        let members = benchmark_members(&observation, &dataset, &bytes);
        assert_eq!(members.len(), 2);
        assert_eq!(members[0].kind, QualityArtifactKind::BenchmarkDataset);
        let archive = observation.archive.as_ref().expect("fixture tree");
        assert_eq!(members[1].kind, QualityArtifactKind::CriterionArchive);
        assert_eq!(members[1].mime_type, QualityMimeType::ApplicationXTar);
        assert_eq!(
            members[1].payload_format_version,
            PayloadFormatVersion::UstarV1
        );
        assert_eq!(members[1].guest_name, GuestArtifactName::CriterionArchive);
        // The tree is the project's own benchmark output; it is exactly as
        // source-derived as the dataset beside it, and no wider.
        assert_eq!(members[1].sensitivity, ArtifactSensitivity::SourceDerived);
        assert!(M5_RETENTION.permits(members[1].sensitivity));
        assert_eq!(members[1].bytes, archive.bytes.as_slice());
    }

    /// A run that retained no tree publishes the dataset alone. The absence is
    /// the observation's own declared `archive_omission`, so no member is
    /// invented to stand in for it.
    #[test]
    fn a_run_without_a_tree_publishes_the_dataset_alone() {
        let mut observation = fixture::observation();
        observation.archive = None;
        observation.archive_omission = Some(DatasetOmission::OutputTooLarge);
        let dataset = fixture::dataset(MeasurementCompleteness::Complete);
        let bytes = serde_json::to_vec(&dataset).expect("dataset bytes");
        let members = benchmark_members(&observation, &dataset, &bytes);
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].kind, QualityArtifactKind::BenchmarkDataset);
        assert_eq!(
            observation.archive_omission,
            Some(DatasetOmission::OutputTooLarge),
            "the reason travels with the observation, not as an empty member"
        );
    }

    /// One tree of a three-repetition run is part of that run's harness output,
    /// and the descriptor says so. Only a single-repetition run that completed
    /// publishes a tree that is the whole of it.
    #[test]
    fn one_repetitions_tree_never_claims_to_be_the_whole_run() {
        let mut observation = fixture::observation();
        assert_eq!(observation.runs_requested, 3);
        assert_eq!(
            archive_completeness(&observation),
            ArtifactCompleteness::Partial
        );
        observation.runs_completed = 1;
        observation.runs_requested = 1;
        assert_eq!(
            archive_completeness(&observation),
            ArtifactCompleteness::Complete
        );
        observation.runs_completed = 0;
        assert_eq!(
            archive_completeness(&observation),
            ArtifactCompleteness::Partial
        );
    }

    #[test]
    fn a_plugin_identity_separates_two_backends_and_two_versions() {
        assert_ne!(
            plugin_digest(b"criterion", "0.8.2"),
            plugin_digest(b"cargo-bloat", "0.8.2")
        );
        assert_ne!(
            plugin_digest(b"criterion", "0.8.2"),
            plugin_digest(b"criterion", "0.8.3")
        );
        assert_eq!(
            harness_version(&HarnessDetection::Criterion {
                version: "0.8.2".into()
            }),
            "0.8.2"
        );
        assert_eq!(harness_version(&HarnessDetection::Unrecognized), "");
    }

    /// A minimal validated benchmark observation, built from the same fixtures
    /// the security tool's tests use so the runtime identity is well formed.
    mod fixture {
        use super::*;
        use crate::stdio::security_tool::test_fixtures as shared;
        use rust_engineering_domain::ExecutionTermination;
        use rust_engineering_domain::benchmark::{
            BenchmarkHarness, BenchmarkIdentity, BenchmarkMeasurement, BenchmarkProvenance,
            HardwareProfile, RawSample, ResourceQuotas, SampleUnit, SamplingMode, Virtualization,
        };
        use rust_engineering_domain::benchmark_run::{BenchmarkExit, CriterionArchive};

        pub(super) fn observation() -> BenchmarkObservation {
            BenchmarkObservation {
                selection: super::selection(None, Some("throughput")),
                harness: HarnessDetection::Criterion {
                    version: "0.8.2".into(),
                },
                exit: BenchmarkExit::Passed,
                exit_code: Some(0),
                termination: ExecutionTermination::Exited,
                dataset: None,
                omission: None,
                archive: Some(CriterionArchive {
                    // The last repetition's tree: the one this observation's
                    // own exit, termination and logs describe.
                    run_index: 3,
                    bytes: b"criterion output tree".to_vec(),
                }),
                archive_omission: None,
                runs_completed: 3,
                runs_requested: 3,
                runtime: shared::runtime().expect("runtime identity"),
                execution_fingerprint: shared::execution_fingerprint('3')
                    .expect("execution fingerprint"),
                vendor_fingerprint: shared::source_fingerprint('5').expect("source fingerprint"),
                stdout: Vec::new(),
                stderr: Vec::new(),
                stdout_truncated: false,
                stderr_truncated: false,
            }
        }

        pub(super) fn dataset(completeness: MeasurementCompleteness) -> BenchmarkDataset {
            let identity = BenchmarkIdentity::new(
                "group".into(),
                None,
                None,
                "group/throughput".into(),
                "group_throughput".into(),
            )
            .expect("identity");
            let samples = if completeness == MeasurementCompleteness::Missing {
                Vec::new()
            } else {
                // One fixture execution, so every sample is repetition one.
                vec![RawSample::new(1, 1_000.0, 1).expect("sample")]
            };
            let measurement = BenchmarkMeasurement::new(
                identity,
                SamplingMode::Auto,
                samples,
                3_000,
                5_000,
                64,
                completeness,
            )
            .expect("measurement");
            let observation = observation();
            BenchmarkDataset::new(
                SampleUnit::Nanoseconds,
                vec![measurement],
                BenchmarkProvenance {
                    source_fingerprint: observation.vendor_fingerprint.to_string(),
                    harness: BenchmarkHarness::Criterion,
                    harness_version: "0.8.2".into(),
                    rust_version: observation.runtime.rust_version.clone(),
                    cargo_version: observation.runtime.cargo_version.clone(),
                    declared_toolchain: observation.runtime.declared_toolchain.clone(),
                    image_digest: observation.runtime.image_id.clone(),
                    platform: observation.runtime.platform.clone(),
                    configuration_fingerprint: observation
                        .runtime
                        .configuration_fingerprint
                        .to_string(),
                    execution_fingerprint: observation.execution_fingerprint.to_string(),
                    selection: observation.selection.clone(),
                    hardware: HardwareProfile {
                        cpu_model: None,
                        cpu_cores: None,
                        os_kernel: None,
                        arch: "aarch64".into(),
                        virtualization: Virtualization::Container,
                        cpu_governor: None,
                        quotas: ResourceQuotas {
                            cpu_quota_millicores: None,
                            memory_bytes: None,
                            pids: None,
                        },
                    },
                    run_index: 1,
                    run_count: observation.runs_requested,
                    captured_at_unix: 1_788_000_000,
                },
            )
            .expect("dataset")
        }
    }
}
