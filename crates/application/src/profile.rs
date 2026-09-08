//! Application boundary for one `rust.profile.flamegraph` execution (M5-03).
//!
//! Same shape as [`crate::benchmark`], with one addition that is deliberately
//! *not* delegated: the host profiling capability of ADR-074 §2 is checked
//! here, in the application, before a source generation is captured and before
//! the executor is reached. Putting the gate in the adapter would make the
//! permission a property of one runtime implementation; putting it in the tool
//! would make it a property of one protocol surface. It is neither: it is the
//! rule that decides whether this operation may exist at all.
use crate::security::{SecurityCapture, SecurityError};
use crate::{
    InspectionControl, InspectionError, ProjectError, ProjectRegistry, ProjectSourceBackend,
    QualityOwnerFacts, QualityProjectBackend, ReferenceGenerator, RegistryClock,
};
use rust_engineering_domain::profile::{PROFILE_BACKEND, ProfileCompleteness, ProfileStatus};
use rust_engineering_domain::{
    CargoVendorSnapshot, Clock, OperationalErrorCode, ProjectRef, QualityArtifactDescriptor,
    SourceBundle,
};

pub use rust_engineering_domain::profile::{ProfileObservation, ProfileOptions};

/// Whether the trusted host granted this server the profiling capability.
///
/// The peer, the project, the URI and the tool annotations cannot produce
/// `Granted`; only the host configuration can (ADR-074 §2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfilingAuthorization {
    Granted,
    NotGranted,
}

/// The refusal a missing capability produces.
///
/// `SecurityError` has no dedicated variant, and adding one would widen an enum
/// four other engines already share. `OperationalErrorCode::SandboxDenied` is
/// the closest existing value and the correct one: it is the code the registry
/// itself already uses for "the host will not allow this containment", and
/// `OperationalErrorCode::status` maps it to `ToolStatus::Blocked`, which is
/// exactly the outcome ADR-076 §5 requires for `PROFILING_NOT_AUTHORIZED`.
/// `Unavailable` would have been wrong: the capability is refused, not missing.
pub const PROFILING_NOT_AUTHORIZED: SecurityError = SecurityError::Inspection(
    InspectionError::Project(ProjectError::Rejected(OperationalErrorCode::SandboxDenied)),
);

/// Samples one project binary inside the profiling sandbox. The implementation
/// owns the helper, the seccomp profile and the SVG rendering; it receives a
/// target name, never a path, and no peer argument reaches the child.
pub trait ProjectProfilePort: Send + Sync {
    fn profile(
        &self,
        source: &SourceBundle,
        vendor: &CargoVendorSnapshot,
        options: &ProfileOptions,
        control: &dyn InspectionControl,
    ) -> Result<ProfileObservation, SecurityError>;
}

/// Publishes the sanitized SVG and the collapsed stacks as durable artifacts.
pub trait ProfilePublisher: Send {
    fn publish_profile(
        &mut self,
        capture: &SecurityCapture,
        observation: &ProfileObservation,
        revalidate: &mut dyn FnMut() -> Result<QualityOwnerFacts, InspectionError>,
    ) -> Result<Vec<QualityArtifactDescriptor>, InspectionError>;
}

pub struct ProfilePorts<'a, E, P> {
    pub executor: &'a E,
    pub publisher: &'a mut P,
}

pub struct PublishedProfile {
    pub observation: ProfileObservation,
    pub artifacts: Vec<QualityArtifactDescriptor>,
}

/// Everything that must hold before a profile may be published.
///
/// Zero samples is a valid, declared result (ADR-074 §5), so nothing here
/// rejects an empty profile. What it rejects is a profile that *describes*
/// itself inconsistently.
pub fn validate_profile_observation(
    observation: &ProfileObservation,
    options: &ProfileOptions,
    vendor: &CargoVendorSnapshot,
) -> Result<(), SecurityError> {
    if !observation.consistent()
        || observation.options != *options
        || observation.backend != PROFILE_BACKEND
        || observation.vendor_fingerprint != vendor.tree_fingerprint
        || observation.runtime.execution_fingerprint != observation.execution_fingerprint
    {
        return Err(SecurityError::InvalidMetadata);
    }
    // A refused `perf_event_open` is reportable only with the errno that refused
    // it, and it cannot have rendered a graph of the samples it never took.
    if observation.status == ProfileStatus::ProfilerUnavailable
        && (observation.perf_errno.is_none() || !observation.svg.is_empty())
    {
        return Err(SecurityError::InvalidMetadata);
    }
    if observation.completeness == ProfileCompleteness::Complete
        && observation.counters.samples_collected == 0
    {
        return Err(SecurityError::InvalidMetadata);
    }
    Ok(())
}

impl<B: ProjectSourceBackend + QualityProjectBackend, G: ReferenceGenerator, C: RegistryClock>
    ProjectRegistry<B, G, C>
{
    #[allow(clippy::too_many_arguments)] // The host capability is an explicit argument, not ambient state.
    pub fn profile_durable(
        &mut self,
        reference: &ProjectRef,
        vendor: &CargoVendorSnapshot,
        options: &ProfileOptions,
        authorization: ProfilingAuthorization,
        ports: ProfilePorts<'_, impl ProjectProfilePort, impl ProfilePublisher>,
        clock: &impl Clock,
        control: &dyn InspectionControl,
    ) -> Result<PublishedProfile, SecurityError> {
        // Before the capture, before the executor, before any container.
        if authorization != ProfilingAuthorization::Granted {
            return Err(PROFILING_NOT_AUTHORIZED);
        }
        let capture = self.capture_security(reference, clock, control)?;
        let observation = ports
            .executor
            .profile(&capture.source, vendor, options, control)?;
        control.check()?;
        validate_profile_observation(&observation, options, vendor)?;
        let mut revalidate = || {
            self.quality_owner_facts(reference, control)
                .map_err(InspectionError::from)
        };
        let artifacts = ports
            .publisher
            .publish_profile(&capture, &observation, &mut revalidate)?;
        control.check()?;
        if self.resolve_inner(reference, control, true)?.fingerprint
            != capture.project_identity_fingerprint
        {
            return Err(SecurityError::InvalidMetadata);
        }
        Ok(PublishedProfile {
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
    use rust_engineering_domain::profile::{ProfileBuildOutcome, ProfileChild, ProfileCounters};
    use rust_engineering_domain::{
        ExecutionTermination, QualityArtifactKind, SourceBundle, UnixSeconds,
    };
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn options() -> ProfileOptions {
        ProfileOptions::new("workload".into(), 99, 10).expect("options")
    }

    fn observation(options: &ProfileOptions) -> ProfileObservation {
        ProfileObservation {
            options: options.clone(),
            backend: PROFILE_BACKEND,
            build: ProfileBuildOutcome::Built,
            build_exit_code: Some(0),
            status: ProfileStatus::Complete,
            counters: ProfileCounters {
                observed_duration_ms: 10_000,
                samples_collected: 990,
                samples_lost: 0,
                stacks_written: 512,
                frames_total: 4_096,
                frames_unresolved: 3,
                stacks_truncated: 0,
                modules_seen: 2,
                max_depth_applied: 127,
            },
            child: ProfileChild {
                exit_code: Some(0),
                signal: None,
            },
            perf_errno: None,
            completeness: ProfileCompleteness::Complete,
            top_frames: Vec::new(),
            stacks: b"main;work 990\n".to_vec(),
            svg: b"<svg></svg>".to_vec(),
            termination: ExecutionTermination::Exited,
            runtime: runtime(41),
            execution_fingerprint: execution_fingerprint(41),
            vendor_fingerprint: source_fingerprint(21),
            stdout: Vec::new(),
            stderr: Vec::new(),
            stdout_truncated: false,
            stderr_truncated: false,
        }
    }

    /// A port double that records every call. `panic` is denied workspace-wide,
    /// so "never invoked" is proved by a counter the test reads, not by a trap.
    struct Executor {
        observation: ProfileObservation,
        calls: Arc<AtomicUsize>,
    }
    impl ProjectProfilePort for Executor {
        fn profile(
            &self,
            _: &SourceBundle,
            _: &CargoVendorSnapshot,
            _: &ProfileOptions,
            control: &dyn InspectionControl,
        ) -> Result<ProfileObservation, SecurityError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            control.check()?;
            Ok(self.observation.clone())
        }
    }

    #[derive(Default)]
    struct Publisher {
        calls: Arc<AtomicUsize>,
    }
    impl ProfilePublisher for Publisher {
        fn publish_profile(
            &mut self,
            _: &SecurityCapture,
            _: &ProfileObservation,
            revalidate: &mut dyn FnMut() -> Result<QualityOwnerFacts, InspectionError>,
        ) -> Result<Vec<QualityArtifactDescriptor>, InspectionError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            revalidate()?;
            Ok(vec![
                descriptor(QualityArtifactKind::FlamegraphSvg),
                descriptor(QualityArtifactKind::CollapsedStacks),
            ])
        }
    }

    struct Harness {
        result: Result<PublishedProfile, SecurityError>,
        executed: usize,
        published: usize,
        captures: usize,
    }

    fn run(
        observed: ProfileObservation,
        options: &ProfileOptions,
        authorization: ProfilingAuthorization,
        cancel: bool,
    ) -> Harness {
        let backend = Backend::default();
        let clock = TestClock::at(100);
        let control = Control::default();
        let mut registry = registry(backend.clone(), clock.clone());
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
        let result = registry.profile_durable(
            &opened.project_ref,
            &vendor(),
            options,
            authorization,
            ProfilePorts {
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
            captures: backend.captures.load(Ordering::SeqCst),
        }
    }

    fn rejected(observed: ProfileObservation, options: &ProfileOptions) {
        let harness = run(observed, options, ProfilingAuthorization::Granted, false);
        assert_eq!(harness.result.err(), Some(SecurityError::InvalidMetadata));
        assert_eq!(harness.published, 0);
    }

    #[test]
    fn a_validated_profile_publishes_the_svg_and_the_collapsed_stacks() {
        let options = options();
        let harness = run(
            observation(&options),
            &options,
            ProfilingAuthorization::Granted,
            false,
        );
        let published = harness.result.expect("published");
        assert_eq!(harness.executed, 1);
        assert_eq!(published.artifacts.len(), 2);
        assert_eq!(
            published.artifacts[0].kind,
            QualityArtifactKind::FlamegraphSvg
        );
        assert_eq!(
            published.artifacts[1].kind,
            QualityArtifactKind::CollapsedStacks
        );
    }

    #[test]
    fn without_the_host_capability_no_port_is_reached_and_no_source_is_captured() {
        let options = options();
        let harness = run(
            observation(&options),
            &options,
            ProfilingAuthorization::NotGranted,
            false,
        );
        assert_eq!(harness.result.err(), Some(PROFILING_NOT_AUTHORIZED));
        // The gate runs before the capture, so nothing observed the project.
        assert_eq!(harness.captures, 0);
        assert_eq!(harness.executed, 0);
        assert_eq!(harness.published, 0);
    }

    #[test]
    fn the_refusal_is_a_blocked_sandbox_denial_not_an_unavailable_tool() {
        assert_eq!(
            PROFILING_NOT_AUTHORIZED,
            SecurityError::Inspection(InspectionError::Project(ProjectError::Rejected(
                OperationalErrorCode::SandboxDenied
            )))
        );
        assert_eq!(
            OperationalErrorCode::SandboxDenied.status(),
            rust_engineering_domain::ToolStatus::Blocked
        );
    }

    #[test]
    fn an_inconsistent_observation_is_never_published() {
        let options = options();
        let mut observed = observation(&options);
        // `LostSamples` without a single lost sample: the domain's own rule.
        observed.completeness = ProfileCompleteness::LostSamples;
        rejected(observed, &options);
    }

    #[test]
    fn options_the_executor_did_not_echo_back_are_never_published() {
        let options = options();
        let mut observed = observation(&options);
        observed.options = ProfileOptions::new("workload".into(), 999, 60).expect("options");
        rejected(observed, &options);
    }

    #[test]
    fn an_unavailable_profiler_must_carry_its_errno_and_no_graph() {
        let options = options();
        let mut base = observation(&options);
        base.status = ProfileStatus::ProfilerUnavailable;
        base.completeness = ProfileCompleteness::Unavailable;
        base.counters.samples_collected = 0;
        base.svg = Vec::new();
        base.perf_errno = Some(1);
        // The declared refusal itself is publishable evidence.
        let harness = run(
            base.clone(),
            &options,
            ProfilingAuthorization::Granted,
            false,
        );
        assert!(harness.result.is_ok());

        let mut without_errno = base.clone();
        without_errno.perf_errno = None;
        rejected(without_errno, &options);

        let mut with_a_graph = base;
        with_a_graph.svg = b"<svg></svg>".to_vec();
        // A graph rendered from samples that were never taken is not consistent
        // for `Unavailable` either, so declare a completeness that would pass.
        with_a_graph.completeness = ProfileCompleteness::NoSamples;
        rejected(with_a_graph, &options);
    }

    #[test]
    fn a_complete_profile_that_collected_nothing_is_never_published() {
        let options = options();
        let mut observed = observation(&options);
        observed.counters.samples_collected = 0;
        rejected(observed, &options);
    }

    #[test]
    fn zero_samples_is_a_declared_result_and_is_published() {
        let options = options();
        let mut observed = observation(&options);
        observed.completeness = ProfileCompleteness::NoSamples;
        observed.counters.samples_collected = 0;
        observed.status = ProfileStatus::ChildExited;
        let harness = run(observed, &options, ProfilingAuthorization::Granted, false);
        assert!(harness.result.is_ok());
        assert_eq!(harness.published, 1);
    }

    #[test]
    fn a_profile_over_another_vendor_tree_is_never_published() {
        let options = options();
        let mut observed = observation(&options);
        observed.vendor_fingerprint = source_fingerprint(77);
        rejected(observed, &options);
    }

    #[test]
    fn cancellation_propagates_from_the_control_and_publishes_nothing() {
        let options = options();
        let harness = run(
            observation(&options),
            &options,
            ProfilingAuthorization::Granted,
            true,
        );
        assert_eq!(
            harness.result.err(),
            Some(SecurityError::Inspection(InspectionError::Project(
                ProjectError::Cancelled
            )))
        );
        assert_eq!(harness.executed, 0);
        assert_eq!(harness.published, 0);
    }

    #[test]
    fn the_capture_timestamp_comes_from_the_supplied_clock() {
        // Guards the ordering: the clock is read during capture, before the port.
        let clock = TestClock::at(1_788_000_000);
        assert_eq!(Clock::now(&clock), UnixSeconds(1_788_000_000));
    }
}
