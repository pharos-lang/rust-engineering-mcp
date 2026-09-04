use rust_engineering_application::{
    DiagnosticExplainPort, ExecutionCancellation, InspectionControl, InspectionError,
    OperationControl, ProjectError, explain_diagnostic,
};
use rust_engineering_domain::{
    Clock, DiagnosticCode, ExecutionTermination, ExplainObservation, FreshnessState,
    IntegrityStatus, RuntimeIdentity, SourceKind, UnixSeconds,
};
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
#[derive(Default)]
struct Control(AtomicBool);
impl OperationControl for Control {
    fn check(&self) -> std::result::Result<(), ProjectError> {
        if self.0.load(Ordering::Relaxed) {
            Err(ProjectError::Cancelled)
        } else {
            Ok(())
        }
    }
}
impl ExecutionCancellation for Control {
    fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}
struct TestClock(Cell<u64>);
impl Clock for TestClock {
    fn now(&self) -> UnixSeconds {
        UnixSeconds(self.0.get())
    }
}
struct Port<F> {
    calls: Cell<usize>,
    during: F,
}
impl<F: Fn() -> std::result::Result<ExplainObservation, InspectionError>> DiagnosticExplainPort
    for Port<F>
{
    fn explain(
        &self,
        _: &DiagnosticCode,
        _: &dyn InspectionControl,
    ) -> std::result::Result<ExplainObservation, InspectionError> {
        self.calls.set(self.calls.get() + 1);
        (self.during)()
    }
}
fn observation() -> std::result::Result<ExplainObservation, InspectionError> {
    let source = format!("sha256:{:064x}", 1);
    Ok(ExplainObservation {
        code: "E0502".parse().map_err(|_| InspectionError::Internal)?,
        explanation: Some("compiler explanation".into()),
        complete: true,
        termination: ExecutionTermination::Exited,
        exit_code: Some(0),
        stdout_truncated: false,
        stderr_truncated: false,
        content_fingerprint: source.parse().map_err(|_| InspectionError::Internal)?,
        runtime: RuntimeIdentity {
            platform: "linux/aarch64".into(),
            image_id: "approved-fixture-image".into(),
            configuration_fingerprint: source.parse().map_err(|_| InspectionError::Internal)?,
            execution_fingerprint: source.parse().map_err(|_| InspectionError::Internal)?,
            rust_version: "1.98.1".into(),
            cargo_version: "1.98.1".into(),
            declared_toolchain: None,
        },
    })
}
#[test]
fn code_grammar_rejects_shell_flags_unicode_and_wrong_width_through_serde() -> Result {
    for valid in ["E0000", "E0502", "E9999"] {
        let code: DiagnosticCode = serde_json::from_value(serde_json::json!(valid))?;
        assert_eq!(code.to_string(), valid);
    }
    for invalid in [
        "",
        "E502",
        "E00502",
        "e0502",
        "E０５０２",
        "E0502\n",
        " E0502",
        "E0502;id",
        "--help",
        "E0502 --help",
    ] {
        assert!(invalid.parse::<DiagnosticCode>().is_err(), "{invalid:?}");
        assert!(
            serde_json::from_value::<DiagnosticCode>(serde_json::json!(invalid)).is_err(),
            "{invalid:?}"
        );
    }
    Ok(())
}
#[test]
fn explanation_without_project_preserves_observation_and_capture_start_time() -> Result {
    let clock = TestClock(Cell::new(1000));
    let port = Port {
        calls: Cell::new(0),
        during: || {
            clock.0.set(1120);
            observation()
        },
    };
    let output = explain_diagnostic(&port, &"E0502".parse()?, &clock, &Control::default())
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(port.calls.get(), 1);
    assert_eq!(
        serde_json::to_value(&output.observation)?,
        serde_json::to_value(observation().map_err(|e| format!("{e:?}"))?)?
    );
    assert_eq!(serde_json::to_value(output.semantics)?, "latest_known");
    let provenance = output.evidence.provenance();
    assert_eq!(provenance.source_kind(), SourceKind::Artifact);
    assert_eq!(
        provenance.source_id().to_string(),
        output.observation.content_fingerprint.to_string()
    );
    assert_eq!(provenance.created_at(), Some(UnixSeconds(1000)));
    assert_eq!(provenance.observed_at(), Some(UnixSeconds(1120)));
    assert_eq!(provenance.integrity(), IntegrityStatus::Verified);
    assert!(!provenance.network_used());
    assert_eq!(output.evidence.freshness().state(), FreshnessState::Aging);
    assert_eq!(output.evidence.freshness().age_seconds(), Some(120));
    Ok(())
}
#[test]
fn mismatched_code_and_oversized_utf8_output_are_not_published() -> Result {
    for mismatch in [true, false] {
        let port = Port {
            calls: Cell::new(0),
            during: || {
                let mut output = observation()?;
                if mismatch {
                    output.code = "E0001".parse().map_err(|_| InspectionError::Internal)?;
                } else {
                    output.explanation = Some("é".repeat(32769));
                }
                Ok(output)
            },
        };
        assert_eq!(
            explain_diagnostic(
                &port,
                &"E0502".parse()?,
                &TestClock(Cell::new(1)),
                &Control::default()
            )
            .err(),
            Some(InspectionError::Internal)
        );
        assert_eq!(port.calls.get(), 1);
    }
    Ok(())
}
#[test]
fn exact_byte_cap_is_accepted() -> Result {
    let port = Port {
        calls: Cell::new(0),
        during: || {
            let mut output = observation()?;
            output.explanation = Some("é".repeat(32768));
            Ok(output)
        },
    };
    let output = explain_diagnostic(
        &port,
        &"E0502".parse()?,
        &TestClock(Cell::new(1)),
        &Control::default(),
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        output.observation.explanation.as_ref().map(String::len),
        Some(65536)
    );
    Ok(())
}
#[test]
fn cancellation_before_and_after_observation_prevents_publication() -> Result {
    for before in [true, false] {
        let control = Control(AtomicBool::new(before));
        let port = Port {
            calls: Cell::new(0),
            during: || {
                control.0.store(true, Ordering::Relaxed);
                observation()
            },
        };
        assert_eq!(
            explain_diagnostic(&port, &"E0502".parse()?, &TestClock(Cell::new(1)), &control).err(),
            Some(InspectionError::Project(ProjectError::Cancelled))
        );
        assert_eq!(port.calls.get(), usize::from(!before));
    }
    Ok(())
}
