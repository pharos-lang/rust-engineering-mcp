//! Durable M5 performance evidence in the unchanged ADR-061 artifact format.
//!
//! The three publishers below share one rule: a member is published only when
//! the observation actually carries its bytes, and its `completeness` is read
//! off that observation rather than asserted. Zero samples, a refused profiler,
//! an unrecognised harness and an unavailable analyzer are all declared
//! results (ADR-076 §3, §5, §6), so each of them legitimately publishes fewer
//! members — or none — instead of committing an artifact that claims to
//! describe evidence that does not exist.
//!
//! "Fewer" is not "none" for a benchmark run: ADR-080 §1 publishes each
//! repetition's harness logs whether or not a dataset came out of it, because
//! for an unrecognised harness or a failed compilation those logs are the only
//! evidence the caller can act on.
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
        // A run without a dataset is still a run that happened. Its harness
        // logs are the evidence ADR-080 §1 publishes — for an unrecognised
        // harness or a compilation failure they are the *only* evidence — so
        // the absent dataset removes one member here rather than the whole
        // publication. The empty vector remains the honest answer for a run
        // that produced no bytes of any kind.
        let bytes = match observation.dataset.as_ref() {
            Some(dataset) => serde_json::to_vec(dataset).map_err(|_| InspectionError::Internal)?,
            None => Vec::new(),
        };
        let members = benchmark_members(observation, &bytes);
        if members.is_empty() {
            return Ok(Vec::new());
        }
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

/// What one benchmark run publishes, in the exact order it is committed.
///
/// This is the single description of the member plan. The publisher below turns
/// it into `JobMember`s and the tool's response encoder turns the descriptors
/// that come back into wire artifacts; deriving both from one function is what
/// keeps a `ToolLog` descriptor — which cannot say by itself whether it is a
/// `stdout` or a `stderr`, nor which repetition it came from — from being
/// guessed at on the way out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::stdio) enum BenchmarkMemberKind {
    /// The pooled dataset. It has no `run_index`: every repetition is in it,
    /// and each of its samples carries its own.
    Dataset,
    CriterionArchive {
        run_index: u8,
    },
    HarnessStdout {
        run_index: u8,
    },
    HarnessStderr {
        run_index: u8,
    },
}

impl BenchmarkMemberKind {
    /// The store kind this member is committed under.
    pub(in crate::stdio) fn artifact_kind(self) -> QualityArtifactKind {
        match self {
            Self::Dataset => QualityArtifactKind::BenchmarkDataset,
            Self::CriterionArchive { .. } => QualityArtifactKind::CriterionArchive,
            Self::HarnessStdout { .. } | Self::HarnessStderr { .. } => QualityArtifactKind::ToolLog,
        }
    }

    /// The repetition this member is evidence of, or `None` for the pooled
    /// dataset.
    pub(in crate::stdio) fn run_index(self) -> Option<u8> {
        match self {
            Self::Dataset => None,
            Self::CriterionArchive { run_index }
            | Self::HarnessStdout { run_index }
            | Self::HarnessStderr { run_index } => Some(run_index),
        }
    }
}

/// The plan, derived from the observation alone.
///
/// The dataset comes first when there is one, then the retained criterion tree
/// when there is one, then each repetition's `stdout` and `stderr` in run
/// order. A stream that wrote nothing publishes no member: a zero-byte artifact
/// would be an absence dressed as evidence, and the repetition's entry in the
/// observation already says the stream was empty.
///
/// A run with no dataset and no tree still plans its logs. That is the whole
/// point of ADR-080: `harness_unrecognized` and an observed compilation failure
/// used to publish nothing at all.
pub(in crate::stdio) fn benchmark_member_plan(
    observation: &BenchmarkObservation,
) -> Vec<BenchmarkMemberKind> {
    let mut plan = Vec::new();
    if observation.dataset.is_some() {
        plan.push(BenchmarkMemberKind::Dataset);
    }
    if let Some(archive) = observation.archive.as_ref() {
        plan.push(BenchmarkMemberKind::CriterionArchive {
            run_index: archive.run_index,
        });
    }
    for log in &observation.logs {
        if !log.stdout.is_empty() {
            plan.push(BenchmarkMemberKind::HarnessStdout {
                run_index: log.run_index,
            });
        }
        if !log.stderr.is_empty() {
            plan.push(BenchmarkMemberKind::HarnessStderr {
                run_index: log.run_index,
            });
        }
    }
    plan
}

/// The members one benchmark run publishes, walking [`benchmark_member_plan`]
/// so the committed order and the published order cannot drift apart.
///
/// The criterion output tree is published only when the run actually retained
/// one repetition's tree: a run that exported none, or whose export exceeded
/// the 32 MiB ceiling of ADR-076 §7, carries a declared `archive_omission`
/// instead and publishes without it rather than committing a member describing
/// bytes this server does not hold. The tree travels verbatim, so it is the
/// same source-derived evidence the dataset is, in the framing the harness
/// wrote it in.
///
/// The logs are `SourceDerived` for the same reason both of those are: they
/// come from compiling and running the project's own code, so they can carry
/// fragments of its source and guest paths. That is exactly the retention this
/// producer already asks the host for, and no wider.
fn benchmark_members<'a>(
    observation: &'a BenchmarkObservation,
    dataset_bytes: &'a [u8],
) -> Vec<JobMember<'a>> {
    let empty: &[u8] = &[];
    benchmark_member_plan(observation)
        .into_iter()
        .map(|planned| match planned {
            BenchmarkMemberKind::Dataset => JobMember {
                kind: QualityArtifactKind::BenchmarkDataset,
                mime_type: QualityMimeType::ApplicationJson,
                payload_format_version: PayloadFormatVersion::BenchmarkDatasetV2,
                guest_name: GuestArtifactName::BenchmarkDataset,
                sensitivity: ArtifactSensitivity::SourceDerived,
                completeness: observation
                    .dataset
                    .as_ref()
                    .map_or(ArtifactCompleteness::Unavailable, |dataset| {
                        dataset_completeness(observation, dataset)
                    }),
                bytes: dataset_bytes,
            },
            BenchmarkMemberKind::CriterionArchive { .. } => JobMember {
                kind: QualityArtifactKind::CriterionArchive,
                mime_type: QualityMimeType::ApplicationXTar,
                payload_format_version: PayloadFormatVersion::UstarV1,
                guest_name: GuestArtifactName::CriterionArchive,
                sensitivity: ArtifactSensitivity::SourceDerived,
                completeness: archive_completeness(observation),
                bytes: observation
                    .archive
                    .as_ref()
                    .map_or(empty, |archive| archive.bytes.as_slice()),
            },
            BenchmarkMemberKind::HarnessStdout { run_index } => {
                log_member(observation, run_index, true, empty)
            }
            BenchmarkMemberKind::HarnessStderr { run_index } => {
                log_member(observation, run_index, false, empty)
            }
        })
        .collect()
}

/// One repetition's stream, as its own member.
///
/// `completeness` is the stream's own: `truncated` when the adapter cut it at
/// the published ceiling, `complete` otherwise. Unlike the criterion tree, a
/// log is not partial evidence of the run just because the run had three
/// repetitions — it is the whole of what *that* repetition wrote to *that*
/// stream, and the descriptor says so rather than borrowing the archive's
/// reasoning.
fn log_member<'a>(
    observation: &'a BenchmarkObservation,
    run_index: u8,
    stdout: bool,
    empty: &'a [u8],
) -> JobMember<'a> {
    let entry = observation
        .logs
        .iter()
        .find(|log| log.run_index == run_index);
    let (bytes, truncated) = entry.map_or((empty, false), |log| {
        if stdout {
            (log.stdout.as_slice(), log.stdout_truncated)
        } else {
            (log.stderr.as_slice(), log.stderr_truncated)
        }
    });
    JobMember {
        kind: QualityArtifactKind::ToolLog,
        mime_type: QualityMimeType::TextPlain,
        payload_format_version: PayloadFormatVersion::Utf8LogV1,
        guest_name: GuestArtifactName::ToolLog,
        sensitivity: ArtifactSensitivity::SourceDerived,
        completeness: if truncated {
            ArtifactCompleteness::Truncated
        } else {
            ArtifactCompleteness::Complete
        },
        bytes,
    }
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

/// The member published here is the analyzer's **raw JSON report**, so its
/// completeness is the report's own and never the published ranking's: the
/// product's row cap bounds what the DTO shows, not what this file contains.
/// That is why no arm maps to `Truncated` (ADR-079 §1 and §4).
fn bloat_completeness(value: BloatCompleteness) -> ArtifactCompleteness {
    match value {
        BloatCompleteness::Complete => ArtifactCompleteness::Complete,
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
    use rust_engineering_domain::benchmark_run::{BenchmarkRunLog, DatasetOmission};

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
        let mut observation = fixture::observation();
        let dataset = fixture::dataset(MeasurementCompleteness::Complete);
        observation.dataset = Some(dataset.clone());
        let bytes = serde_json::to_vec(&dataset).expect("dataset bytes");
        let members = benchmark_members(&observation, &bytes);
        // Two measurement members, then this fixture's six log members.
        assert_eq!(members.len(), 8);
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
        observation.dataset = Some(dataset.clone());
        // The one repetition wrote nothing, so no log member joins it either;
        // the dataset is genuinely alone.
        observation.logs = vec![BenchmarkRunLog {
            run_index: 3,
            stdout: Vec::new(),
            stdout_truncated: false,
            stderr: Vec::new(),
            stderr_truncated: false,
        }];
        let bytes = serde_json::to_vec(&dataset).expect("dataset bytes");
        let members = benchmark_members(&observation, &bytes);
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

    /// ADR-080 §1 and §2. Every repetition's two streams become their own
    /// members, in run order, after the two measurement members. Nothing is
    /// concatenated: each member's bytes are exactly one repetition's stream,
    /// and the plan names the repetition it belongs to.
    #[test]
    fn every_repetitions_logs_are_published_as_their_own_members() {
        let mut observation = fixture::observation();
        let dataset = fixture::dataset(MeasurementCompleteness::Complete);
        let bytes = serde_json::to_vec(&dataset).expect("dataset bytes");
        observation.dataset = Some(dataset);
        let plan = benchmark_member_plan(&observation);
        assert_eq!(
            plan,
            vec![
                BenchmarkMemberKind::Dataset,
                BenchmarkMemberKind::CriterionArchive { run_index: 3 },
                BenchmarkMemberKind::HarnessStdout { run_index: 1 },
                BenchmarkMemberKind::HarnessStderr { run_index: 1 },
                BenchmarkMemberKind::HarnessStdout { run_index: 2 },
                BenchmarkMemberKind::HarnessStderr { run_index: 2 },
                BenchmarkMemberKind::HarnessStdout { run_index: 3 },
                BenchmarkMemberKind::HarnessStderr { run_index: 3 },
            ]
        );
        let members = benchmark_members(&observation, &bytes);
        assert_eq!(members.len(), plan.len());
        for (member, planned) in members.iter().zip(plan.iter()) {
            assert_eq!(member.kind, planned.artifact_kind());
        }
        for (index, member) in members.iter().skip(2).enumerate() {
            assert_eq!(member.kind, QualityArtifactKind::ToolLog);
            assert_eq!(member.mime_type, QualityMimeType::TextPlain);
            assert_eq!(
                member.payload_format_version,
                PayloadFormatVersion::Utf8LogV1
            );
            assert_eq!(member.guest_name, GuestArtifactName::ToolLog);
            // The logs come out of compiling and running the project's own
            // code; they are exactly as source-derived as the dataset, and the
            // producer's own retention grant admits them.
            assert_eq!(member.sensitivity, ArtifactSensitivity::SourceDerived);
            assert!(M5_RETENTION.permits(member.sensitivity));
            assert_eq!(member.completeness, ArtifactCompleteness::Complete);
            let run_index = index / 2 + 1;
            let stream = if index % 2 == 0 { "stdout" } else { "stderr" };
            assert_eq!(
                member.bytes,
                format!("{stream} of repetition {run_index}").as_bytes(),
                "member {index} carries some other repetition's stream"
            );
        }
        // A concatenation would show up as one member longer than a single
        // stream; every log member is exactly one stream long.
        let widest = members
            .iter()
            .skip(2)
            .map(|member| member.bytes.len())
            .max()
            .expect("six log members");
        assert_eq!(widest, "stdout of repetition 1".len());
    }

    /// ADR-080 §1: a run with no dataset and no tree is exactly the case the
    /// old publisher answered with an empty vector, leaving `OBSERVED_FAILURE`
    /// and `harness_unrecognized` with nothing to fetch. Its logs are now the
    /// evidence, and they are published.
    #[test]
    fn a_run_without_a_dataset_still_publishes_its_logs() {
        let mut observation = fixture::observation();
        observation.harness = HarnessDetection::Unrecognized;
        observation.dataset = None;
        observation.omission = Some(DatasetOmission::HarnessUnrecognized);
        observation.archive = None;
        observation.archive_omission = Some(DatasetOmission::HarnessUnrecognized);
        observation.runs_requested = 1;
        observation.runs_completed = 1;
        observation.exit_run_index = 1;
        observation.logs = vec![BenchmarkRunLog {
            run_index: 1,
            stdout: Vec::new(),
            stdout_truncated: false,
            stderr: b"error[E0308]: mismatched types".to_vec(),
            stderr_truncated: false,
        }];
        let members = benchmark_members(&observation, &[]);
        assert_eq!(
            benchmark_member_plan(&observation),
            vec![BenchmarkMemberKind::HarnessStderr { run_index: 1 }],
            "the empty stdout publishes nothing; the stderr is the evidence"
        );
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].kind, QualityArtifactKind::ToolLog);
        assert_eq!(members[0].bytes, b"error[E0308]: mismatched types");
        assert!(observation.consistent());
    }

    /// ADR-080 §3. A cut stream is published as `truncated`; it is never
    /// committed as a complete member, and the member's bytes are the prefix
    /// that survived rather than a claim about what the harness wrote.
    #[test]
    fn a_cut_log_is_published_as_truncated_and_never_as_whole() {
        let mut observation = fixture::observation();
        observation.logs = vec![
            fixture::log(1),
            fixture::log(2),
            BenchmarkRunLog {
                run_index: 3,
                stdout: b"the prefix that survived".to_vec(),
                stdout_truncated: true,
                stderr: b"whole".to_vec(),
                stderr_truncated: false,
            },
        ];
        // The tree is member zero here (this observation has no dataset), so
        // the six log members are 1..=6 and repetition three's stdout is 5.
        let members = benchmark_members(&observation, &[]);
        assert_eq!(members.len(), 7);
        assert_eq!(
            members[5].completeness,
            ArtifactCompleteness::Truncated,
            "the cut stdout of repetition three"
        );
        assert_eq!(members[5].bytes, b"the prefix that survived");
        assert_eq!(
            members[6].completeness,
            ArtifactCompleteness::Complete,
            "the stderr beside it was not cut and does not inherit the cut"
        );
        // Truncation is per stream, so the earlier repetitions stay complete.
        assert!(
            members[1..5]
                .iter()
                .all(|member| member.completeness == ArtifactCompleteness::Complete)
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
                exit_run_index: 3,
                dataset: None,
                omission: None,
                archive: Some(CriterionArchive {
                    // The last repetition that exported a tree. Here it is also
                    // the repetition whose exit is reported; the fixture below
                    // moves them apart where that is what is under test.
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
                logs: (1..=3).map(log).collect(),
            }
        }

        /// One repetition's logs, with bytes distinct per repetition and per
        /// stream so a merged or misattributed member is visible as a value,
        /// not just as a count.
        pub(super) fn log(run_index: u8) -> BenchmarkRunLog {
            BenchmarkRunLog {
                run_index,
                stdout: format!("stdout of repetition {run_index}").into_bytes(),
                stdout_truncated: false,
                stderr: format!("stderr of repetition {run_index}").into_bytes(),
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
