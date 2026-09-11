//! Application boundary for one `rust.binary.bloat` execution (M5-04).
//!
//! Same shape as [`crate::benchmark`], over one artifact: the analyzer's raw
//! JSON report.
//!
//! The rule this module exists to enforce is ADR-076 §6. The size of the file
//! is measured by this product and is exact; every per-function and per-crate
//! number is `cargo-bloat`'s estimate. If the analyzer's own `file-size`
//! disagrees with the size we measured, its ranking describes some other file,
//! and **the application** is what refuses to publish it as if it described
//! this one. That refusal cannot live in the adapter: the adapter is the party
//! whose claim is being checked.
//!
//! A second claim is checked for the same reason. `cargo-bloat` 0.12.1 forces
//! `CARGO_PROFILE_<PROFILE>_STRIP=false` on every build it performs, because it
//! needs the symbol table (`src/main.rs:694-696`; calibrated in the guest and
//! recorded in `docs/validation/M5-04-bloat-calibration.json`). Every binary
//! this tool measures is therefore an *analysis build*, and its size is exact
//! for that file and not for the file a project asking for stripping would
//! ship. `MeasuredBinary::analysis_build_symbols_forced` records that, and it
//! is always true with this analyzer — so an observation that says otherwise is
//! not describing the run we performed and is refused here.
use crate::security::{SecurityCapture, SecurityError};
use crate::{
    InspectionControl, InspectionError, ProjectRegistry, ProjectSourceBackend, QualityOwnerFacts,
    QualityProjectBackend, ReferenceGenerator, RegistryClock,
};
use rust_engineering_domain::bloat::{APPROVED_CARGO_BLOAT_VERSION, BloatCompleteness};
use rust_engineering_domain::{
    ArtifactCompleteness, CargoVendorSnapshot, Clock, ProjectRef, QualityArtifactDescriptor,
    SourceBundle,
};

pub use rust_engineering_domain::bloat::{BloatObservation, BloatOptions};

/// Builds and measures one project binary, then runs the pinned size analyzer
/// over it. The implementation owns the containment and the report parsing.
pub trait ProjectBloatPort: Send + Sync {
    fn bloat(
        &self,
        source: &SourceBundle,
        vendor: &CargoVendorSnapshot,
        options: &BloatOptions,
        control: &dyn InspectionControl,
    ) -> Result<BloatObservation, SecurityError>;
}

/// Publishes the analyzer's raw JSON report as one durable artifact.
pub trait BloatPublisher: Send {
    fn publish_bloat(
        &mut self,
        capture: &SecurityCapture,
        observation: &BloatObservation,
        revalidate: &mut dyn FnMut() -> Result<QualityOwnerFacts, InspectionError>,
    ) -> Result<Vec<QualityArtifactDescriptor>, InspectionError>;
}

pub struct BloatPorts<'a, E, P> {
    pub executor: &'a E,
    pub publisher: &'a mut P,
}

pub struct PublishedBloat {
    pub observation: BloatObservation,
    pub artifacts: Vec<QualityArtifactDescriptor>,
}

impl PublishedBloat {
    /// The complete success condition of ADR-079 §2 and §3: a measurement this
    /// product validated (the domain's rule) **and** the evidence that backs it
    /// actually published.
    ///
    /// The second half belongs here and nowhere else, because publication is
    /// this layer's job. A ranking with no durable artifact behind it is an
    /// assertion the caller cannot check, so ADR-079 §3's last bullet keeps it
    /// out of `passed` — while a ranking the product's own cap bounded stays in,
    /// because that is coverage and not lost evidence.
    pub fn analysis_validated(&self) -> bool {
        self.observation.analysis_validated() && self.evidence_published()
    }

    /// Exactly one artifact backs a bloat analysis: the analyzer's raw JSON
    /// report. It is published, or there is no success to declare.
    fn evidence_published(&self) -> bool {
        !self.artifacts.is_empty()
            && self
                .artifacts
                .iter()
                .all(|descriptor| descriptor.completeness == ArtifactCompleteness::Complete)
    }
}

/// Everything that must hold before a size analysis may be published.
pub fn validate_bloat_observation(
    observation: &BloatObservation,
    options: &BloatOptions,
    vendor: &CargoVendorSnapshot,
) -> Result<(), SecurityError> {
    if !observation.consistent()
        || observation.options != *options
        || observation.vendor_fingerprint != vendor.tree_fingerprint
        || observation.runtime.execution_fingerprint != observation.execution_fingerprint
    {
        return Err(SecurityError::InvalidMetadata);
    }
    // ADR-073 §1's rule, applied to the other pinned backend: an unapproved
    // analyzer version makes the capability unavailable, never a degraded
    // measurement published under the approved contract.
    if observation.analyzer_version != APPROVED_CARGO_BLOAT_VERSION
        && observation.completeness != BloatCompleteness::Unavailable
    {
        return Err(SecurityError::InvalidMetadata);
    }
    // A measured file exists only because the analyzer built it with symbols
    // forced on (ADR-076 §6). `false` cannot be a truthful report from this
    // analyzer, so it means the observation describes some other build.
    if observation
        .measured
        .as_ref()
        .is_some_and(|measured| !measured.analysis_build_symbols_forced)
    {
        return Err(SecurityError::InvalidMetadata);
    }
    if observation.completeness == BloatCompleteness::Complete {
        let (Some(measured), Some(attribution)) = (&observation.measured, &observation.attribution)
        else {
            return Err(SecurityError::InvalidMetadata);
        };
        if attribution.reported_file_size_bytes != Some(measured.size_bytes) {
            return Err(SecurityError::InvalidMetadata);
        }
    }
    Ok(())
}

impl<B: ProjectSourceBackend + QualityProjectBackend, G: ReferenceGenerator, C: RegistryClock>
    ProjectRegistry<B, G, C>
{
    pub fn bloat_durable(
        &mut self,
        reference: &ProjectRef,
        vendor: &CargoVendorSnapshot,
        options: &BloatOptions,
        ports: BloatPorts<'_, impl ProjectBloatPort, impl BloatPublisher>,
        clock: &impl Clock,
        control: &dyn InspectionControl,
    ) -> Result<PublishedBloat, SecurityError> {
        let capture = self.capture_security(reference, clock, control)?;
        let observation = ports
            .executor
            .bloat(&capture.source, vendor, options, control)?;
        control.check()?;
        validate_bloat_observation(&observation, options, vendor)?;
        let mut revalidate = || {
            self.quality_owner_facts(reference, control)
                .map_err(InspectionError::from)
        };
        let artifacts = ports
            .publisher
            .publish_bloat(&capture, &observation, &mut revalidate)?;
        control.check()?;
        if self.resolve_inner(reference, control, true)?.fingerprint
            != capture.project_identity_fingerprint
        {
            return Err(SecurityError::InvalidMetadata);
        }
        Ok(PublishedBloat {
            observation,
            artifacts,
        })
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)] // Fixed fixtures are malformed only by mistake; fail immediately.
mod tests {
    use super::*;
    use crate::benchmark::tests::{
        Backend, Control, TestClock, descriptor, execution_fingerprint, registry, runtime,
        source_fingerprint, vendor,
    };
    use rust_engineering_domain::bloat::{
        BinaryFormat, BloatAttribution, BloatCrate, BloatExit, BloatFunction, BloatProfile,
        MeasuredBinary,
    };
    use rust_engineering_domain::{ExecutionTermination, QualityArtifactKind, SourceBundle};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const MEASURED_SIZE: u64 = 4_096;

    fn options() -> BloatOptions {
        BloatOptions::new(
            "workload".into(),
            Some("member".into()),
            BloatProfile::Release,
        )
        .expect("options")
    }

    fn attribution(reported: Option<u64>) -> BloatAttribution {
        BloatAttribution {
            estimated: true,
            reported_file_size_bytes: reported,
            text_section_size_bytes: Some(2_048),
            functions: vec![BloatFunction {
                crate_name: "member".into(),
                name: "work".into(),
                size_bytes: 512,
            }],
            crates: vec![BloatCrate {
                name: "member".into(),
                size_bytes: 1_024,
            }],
            functions_omitted_by_row_cap: 0,
            crates_omitted_by_row_cap: 0,
        }
    }

    fn measured(size_bytes: u64) -> MeasuredBinary {
        MeasuredBinary {
            size_bytes,
            sha256: format!("sha256:{}", "b".repeat(64)),
            format: BinaryFormat::Elf64Aarch64,
            analysis_build_symbols_forced: true,
        }
    }

    fn observation(options: &BloatOptions) -> BloatObservation {
        BloatObservation {
            options: options.clone(),
            analyzer_version: APPROVED_CARGO_BLOAT_VERSION.into(),
            exit: BloatExit::Passed,
            exit_code: Some(0),
            termination: ExecutionTermination::Exited,
            measured: Some(measured(MEASURED_SIZE)),
            attribution: Some(attribution(Some(MEASURED_SIZE))),
            completeness: BloatCompleteness::Complete,
            report: b"{\"file-size\":4096}".to_vec(),
            runtime: runtime(51),
            execution_fingerprint: execution_fingerprint(51),
            vendor_fingerprint: source_fingerprint(21),
            stdout: Vec::new(),
            stderr: Vec::new(),
            stdout_truncated: false,
            stderr_truncated: false,
        }
    }

    struct Executor {
        observation: BloatObservation,
        calls: Arc<AtomicUsize>,
    }
    impl ProjectBloatPort for Executor {
        fn bloat(
            &self,
            _: &SourceBundle,
            _: &CargoVendorSnapshot,
            _: &BloatOptions,
            control: &dyn InspectionControl,
        ) -> Result<BloatObservation, SecurityError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            control.check()?;
            Ok(self.observation.clone())
        }
    }

    #[derive(Default)]
    struct Publisher {
        calls: Arc<AtomicUsize>,
    }
    impl BloatPublisher for Publisher {
        fn publish_bloat(
            &mut self,
            _: &SecurityCapture,
            _: &BloatObservation,
            revalidate: &mut dyn FnMut() -> Result<QualityOwnerFacts, InspectionError>,
        ) -> Result<Vec<QualityArtifactDescriptor>, InspectionError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            revalidate()?;
            Ok(vec![descriptor(QualityArtifactKind::BloatJson)])
        }
    }

    struct Harness {
        result: Result<PublishedBloat, SecurityError>,
        executed: usize,
        published: usize,
    }

    fn run(observed: BloatObservation, options: &BloatOptions, cancel: bool) -> Harness {
        let backend = Backend::default();
        let clock = TestClock::at(100);
        let control = Control::default();
        let mut registry = registry(backend, clock.clone());
        let opened = registry.open("/trusted/project", &control).unwrap();
        let executor = Executor {
            observation: observed,
            calls: Arc::new(AtomicUsize::new(0)),
        };
        let mut publisher = Publisher::default();
        let executed = executor.calls.clone();
        let published = publisher.calls.clone();
        if cancel {
            control.cancel();
        }
        let result = registry.bloat_durable(
            &opened.project_ref,
            &vendor(),
            options,
            BloatPorts {
                executor: &executor,
                publisher: &mut publisher,
            },
            &clock,
            &control,
        );
        Harness {
            result,
            executed: executed.load(Ordering::SeqCst),
            published: published.load(Ordering::SeqCst),
        }
    }

    fn rejected(observed: BloatObservation, options: &BloatOptions) {
        let harness = run(observed, options, false);
        assert_eq!(harness.result.err(), Some(SecurityError::InvalidMetadata));
        assert_eq!(harness.published, 0);
    }

    #[test]
    fn a_validated_analysis_publishes_the_report() {
        let options = options();
        let harness = run(observation(&options), &options, false);
        let published = harness.result.expect("published");
        assert_eq!(harness.executed, 1);
        assert_eq!(published.artifacts.len(), 1);
        assert_eq!(published.artifacts[0].kind, QualityArtifactKind::BloatJson);
        assert_eq!(
            published.observation.completeness,
            BloatCompleteness::Complete
        );
    }

    /// The load-bearing test of this module: one byte of disagreement between
    /// the size this product measured and the size the analyzer reported means
    /// the attribution describes another file. The result is downgraded to
    /// `SizeMismatch` and published as that; it is never published as
    /// `Complete`.
    #[test]
    fn a_one_byte_disagreement_downgrades_the_result_instead_of_publishing_it() {
        let options = options();
        let mut claiming_complete = observation(&options);
        claiming_complete.attribution = Some(attribution(Some(MEASURED_SIZE - 1)));
        rejected(claiming_complete, &options);

        let mut downgraded = observation(&options);
        downgraded.attribution = Some(attribution(Some(MEASURED_SIZE - 1)));
        downgraded.completeness = BloatCompleteness::SizeMismatch;
        let harness = run(downgraded, &options, false);
        let published = harness.result.expect("published");
        assert_eq!(
            published.observation.completeness,
            BloatCompleteness::SizeMismatch
        );
        assert_eq!(harness.published, 1);
    }

    /// ADR-079 §2: a ranking the product's own `BLOAT_MAX_ROWS` cap bounded is
    /// published as a validated analysis, and the counters say how many rows the
    /// cap dropped. Nothing about the cap reaches validity — this is the
    /// assertion that fails if the row counters are wired back into it.
    #[test]
    fn a_ranking_bounded_by_the_product_s_own_cap_is_published_and_validated() {
        let options = options();
        let mut observed = observation(&options);
        let mut capped = attribution(Some(MEASURED_SIZE));
        capped.functions_omitted_by_row_cap = 378;
        capped.crates_omitted_by_row_cap = 2;
        observed.attribution = Some(capped);
        let harness = run(observed, &options, false);
        let published = harness.result.expect("published");
        assert_eq!(harness.published, 1);
        assert_eq!(
            published.observation.completeness,
            BloatCompleteness::Complete
        );
        assert!(published.analysis_validated());
        let declared = published
            .observation
            .attribution
            .as_ref()
            .expect("attribution");
        assert_eq!(declared.functions_omitted_by_row_cap, 378);
        assert_eq!(declared.crates_omitted_by_row_cap, 2);
        // The exact measurement is the thing `passed` rests on, and it is here.
        assert_eq!(
            published
                .observation
                .measured
                .as_ref()
                .map(|binary| binary.size_bytes),
            Some(MEASURED_SIZE)
        );
    }

    /// ADR-079 §3's last bullet: the artifact that backs the attribution is the
    /// only evidence a caller can check the ranking against. Without it — never
    /// published, or published as anything but complete — there is no success to
    /// declare, whatever the measurement said.
    #[test]
    fn an_attribution_whose_artifact_was_not_published_is_never_validated() {
        let options = options();
        let complete = PublishedBloat {
            observation: observation(&options),
            artifacts: vec![descriptor(QualityArtifactKind::BloatJson)],
        };
        assert!(complete.analysis_validated());

        let unpublished = PublishedBloat {
            observation: observation(&options),
            artifacts: Vec::new(),
        };
        assert!(!unpublished.analysis_validated());

        for degraded in [
            rust_engineering_domain::ArtifactCompleteness::Truncated,
            rust_engineering_domain::ArtifactCompleteness::Partial,
            rust_engineering_domain::ArtifactCompleteness::Invalid,
            rust_engineering_domain::ArtifactCompleteness::Unavailable,
        ] {
            let mut artifact = descriptor(QualityArtifactKind::BloatJson);
            artifact.completeness = degraded;
            let partial = PublishedBloat {
                observation: observation(&options),
                artifacts: vec![artifact],
            };
            assert!(!partial.analysis_validated(), "{degraded:?}");
        }
    }

    /// A size disagreement is publishable — the caller must see it — but never
    /// as a validated analysis, and the exact measurement still travels.
    #[test]
    fn a_published_size_mismatch_is_never_a_validated_analysis() {
        let options = options();
        let mut downgraded = observation(&options);
        downgraded.attribution = Some(attribution(Some(MEASURED_SIZE - 1)));
        downgraded.completeness = BloatCompleteness::SizeMismatch;
        let published = run(downgraded, &options, false).result.expect("published");
        assert!(!published.analysis_validated());
        assert_eq!(
            published
                .observation
                .measured
                .as_ref()
                .map(|binary| binary.size_bytes),
            Some(MEASURED_SIZE)
        );
    }

    #[test]
    fn a_complete_result_without_a_measured_binary_is_never_published() {
        let options = options();
        let mut observed = observation(&options);
        observed.measured = None;
        rejected(observed, &options);
    }

    #[test]
    fn a_complete_result_without_an_attribution_is_never_published() {
        let options = options();
        let mut observed = observation(&options);
        observed.attribution = None;
        rejected(observed, &options);
    }

    #[test]
    fn a_complete_result_whose_analyzer_reported_no_size_is_never_published() {
        let options = options();
        let mut observed = observation(&options);
        observed.attribution = Some(attribution(None));
        rejected(observed, &options);
    }

    /// `cargo-bloat` 0.12.1 always builds with symbols forced on, so a measured
    /// binary that claims otherwise is describing a build this tool cannot have
    /// performed. Publishing it would let a reader take the size for the file
    /// their stripped release ships.
    #[test]
    fn a_measured_binary_that_denies_the_forced_analysis_build_is_never_published() {
        let options = options();
        let mut observed = observation(&options);
        let mut binary = measured(MEASURED_SIZE);
        binary.analysis_build_symbols_forced = false;
        observed.measured = Some(binary);
        rejected(observed, &options);
    }

    #[test]
    fn an_attribution_that_does_not_declare_itself_estimated_is_never_published() {
        let options = options();
        let mut observed = observation(&options);
        let mut rows = attribution(Some(MEASURED_SIZE));
        rows.estimated = false;
        observed.attribution = Some(rows);
        rejected(observed, &options);
    }

    #[test]
    fn an_unapproved_analyzer_version_is_only_publishable_as_unavailable() {
        let options = options();
        let mut observed = observation(&options);
        observed.analyzer_version = "0.11.0".into();
        rejected(observed.clone(), &options);

        observed.completeness = BloatCompleteness::Unavailable;
        observed.measured = None;
        observed.attribution = None;
        let harness = run(observed, &options, false);
        assert!(harness.result.is_ok());
        assert_eq!(harness.published, 1);
    }

    #[test]
    fn options_the_executor_did_not_echo_back_are_never_published() {
        let options = options();
        let mut observed = observation(&options);
        observed.options =
            BloatOptions::new("workload".into(), None, BloatProfile::ReleaseLto).expect("options");
        rejected(observed, &options);
    }

    #[test]
    fn an_analysis_over_another_vendor_tree_is_never_published() {
        let options = options();
        let mut observed = observation(&options);
        observed.vendor_fingerprint = source_fingerprint(77);
        rejected(observed, &options);
    }

    #[test]
    fn a_runtime_describing_another_execution_is_never_published() {
        let options = options();
        let mut observed = observation(&options);
        observed.runtime = runtime(88);
        rejected(observed, &options);
    }

    #[test]
    fn cancellation_propagates_from_the_control_and_publishes_nothing() {
        let options = options();
        let harness = run(observation(&options), &options, true);
        assert_eq!(
            harness.result.err(),
            Some(SecurityError::Inspection(InspectionError::Project(
                crate::ProjectError::Cancelled
            )))
        );
        assert_eq!(harness.executed, 0);
        assert_eq!(harness.published, 0);
    }
}
