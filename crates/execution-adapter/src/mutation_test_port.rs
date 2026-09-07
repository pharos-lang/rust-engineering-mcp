//! Bridges the closed M3-05 gateway to the application's mutation contract.
//!
//! Three independent sources are reconciled here and all three must agree
//! before the evidence is called complete:
//!
//! 1. the **listing pass**, which builds and runs nothing, supplies the
//!    `generated` denominator;
//! 2. `outcomes.json` supplies the per-scenario classes and the baseline;
//! 3. the `caught.txt`/`missed.txt`/`timeout.txt`/`unviable.txt` list files
//!    inside the exported report bundle supply an independent per-class total
//!    and the itemized mutant descriptions.
//!
//! Disagreement is never resolved in the project's favour: it downgrades the
//! evidence to invalid. Guest stdout and stderr are carried only as artifacts
//! and are never parsed.
use crate::RustGateway;
use rust_engineering_application::mutation_test::{
    MutationArtifactStreams, MutationCompleteness, MutationTestObservation,
};
use rust_engineering_application::{InspectionControl, InspectionError, ProjectError};
use rust_engineering_domain::mutation_test::{
    MUTATION_MAX_ROWS, MutationBaseline, MutationCounts, MutationMutantRow, MutationOutcomeClass,
    MutationTestCommandOptions,
};
use rust_engineering_domain::{
    ExecutionLimits, ExecutionTermination, RuntimeIdentity, SourceBundle,
};

/// Bundle member names carrying the per-class mutant lists.
const LISTS: [(&str, MutationOutcomeClass); 4] = [
    ("./missed.txt", MutationOutcomeClass::Missed),
    ("./timeout.txt", MutationOutcomeClass::Timeout),
    ("./unviable.txt", MutationOutcomeClass::Unviable),
    ("./caught.txt", MutationOutcomeClass::Caught),
];

struct Lists {
    rows: Vec<MutationMutantRow>,
    omitted: u64,
    /// The four per-class line totals matched the parsed `outcomes.json`.
    agreed: bool,
}

/// Read the four list files out of the validated bundle and compare their line
/// totals with the parsed `outcomes.json` classes. A missing list file is read
/// as an empty list, which still has to match a zero count.
fn cross_check(bundle: &[u8], counts: &MutationCounts) -> Lists {
    let mut rows = Vec::new();
    let mut omitted = 0u64;
    let mut totals = MutationCounts::default();
    for (member, class) in LISTS {
        let bytes = super::mutation_outcomes::bundle_member(bundle, member).unwrap_or(&[]);
        let Some((total, listed)) = super::mutation_outcomes::parse_list(bytes, class) else {
            return Lists {
                rows: Vec::new(),
                omitted: 0,
                agreed: false,
            };
        };
        let slot = match class {
            MutationOutcomeClass::Missed => &mut totals.missed,
            MutationOutcomeClass::Timeout => &mut totals.timeout,
            MutationOutcomeClass::Unviable => &mut totals.unviable,
            _ => &mut totals.caught,
        };
        *slot = total;
        // Most actionable rows first: a caller reading a truncated list must
        // see the surviving mutants, not an arbitrary prefix of caught ones.
        let mut kept = 0u32;
        for row in listed {
            if rows.len() >= MUTATION_MAX_ROWS {
                break;
            }
            rows.push(row);
            kept += 1;
        }
        omitted += u64::from(total.saturating_sub(kept));
    }
    let agreed = totals.caught == counts.caught
        && totals.missed == counts.missed
        && totals.timeout == counts.timeout
        && totals.unviable == counts.unviable;
    Lists {
        rows,
        omitted,
        agreed,
    }
}

pub(super) fn run(
    gateway: &RustGateway,
    source: &SourceBundle,
    options: &MutationTestCommandOptions,
    control: &dyn InspectionControl,
) -> Result<MutationTestObservation, InspectionError> {
    let wall_ms = rust_engineering_application::mutation_test::total_budget_seconds(options)
        .checked_mul(1000)
        .ok_or(InspectionError::Internal)?;
    let limits = ExecutionLimits::new_job(wall_ms, 256 * 1024).ok_or(InspectionError::Internal)?;
    let execution = gateway
        .execute_mutation_test(source, options, limits, control)
        .map_err(InspectionError::Execution)?;
    let result = execution.result;
    if result.termination == ExecutionTermination::Cancelled {
        return Err(InspectionError::Project(ProjectError::Cancelled));
    }
    let outcomes_bytes = execution.outcomes.unwrap_or_default();
    let bundle_bytes = execution.bundle.unwrap_or_default();
    let parsed = super::mutation_outcomes::parse_outcomes(&outcomes_bytes);
    let mut completeness = match (&parsed, outcomes_bytes.is_empty()) {
        (Some(_), _) => MutationCompleteness::Complete,
        (None, true) => MutationCompleteness::Unavailable,
        (None, false) => MutationCompleteness::Invalid,
    };
    let mut counts = MutationCounts::default();
    let mut baseline = MutationBaseline::Missing;
    let mut version = String::new();
    let mut rows = Vec::new();
    let mut omitted = 0u64;
    if let Some(parsed) = parsed {
        counts = parsed.counts;
        counts.generated = execution.listed.unwrap_or(0);
        baseline = parsed.baseline;
        version = parsed.version;
        // The tool's own totals, when it reports them, must match the records.
        if parsed.declared.is_some_and(|declared| {
            declared.caught != counts.caught
                || declared.missed != counts.missed
                || declared.timeout != counts.timeout
                || declared.unviable != counts.unviable
        }) {
            completeness = MutationCompleteness::Invalid;
        }
        if execution.listed.is_none() || !counts.consistent() {
            completeness = MutationCompleteness::Invalid;
        }
        if bundle_bytes.is_empty() {
            // Without the bundle the per-class lists cannot be cross-checked
            // and no mutant can be named; the counts stand but are partial.
            if completeness == MutationCompleteness::Complete {
                completeness = MutationCompleteness::Partial;
            }
        } else {
            let lists = cross_check(&bundle_bytes, &counts);
            if lists.agreed {
                rows = lists.rows;
                omitted = lists.omitted;
            } else if completeness == MutationCompleteness::Complete {
                completeness = MutationCompleteness::Invalid;
            }
        }
    }
    let bundle_unavailable = execution.bundle_unavailable || bundle_bytes.is_empty();
    let validation_complete = completeness == MutationCompleteness::Complete
        && !execution.cap_exceeded
        && !bundle_unavailable
        && omitted == 0
        && !version.is_empty()
        && result.termination == ExecutionTermination::Exited;
    let runtime = RuntimeIdentity {
        platform: result.platform.into(),
        image_id: result.image_id.clone(),
        configuration_fingerprint: gateway
            .configuration_fingerprint()
            .map_err(InspectionError::Execution)?,
        execution_fingerprint: result.execution_fingerprint.clone(),
        rust_version: super::rust_gateway::APPROVED_RUST_VERSION.into(),
        cargo_version: super::rust_gateway::APPROVED_CARGO_VERSION.into(),
        declared_toolchain: super::project_metadata::declared_toolchain(source)?,
    };
    Ok(MutationTestObservation {
        options: options.clone(),
        completeness,
        validation_complete,
        baseline,
        counts,
        mutants: rows,
        mutants_omitted: omitted,
        cap_exceeded: execution.cap_exceeded,
        mutants_version: version,
        guest_identity: execution.identity,
        termination: result.termination,
        exit_code: result.exit_code,
        runtime,
        execution_fingerprint: result.execution_fingerprint,
        artifacts: MutationArtifactStreams {
            outcomes_json: outcomes_bytes,
            report_bundle: bundle_bytes,
            stdout: result.stdout.into_bytes(),
            stderr: result.stderr.into_bytes(),
            outcomes_truncated: false,
            bundle_unavailable,
            stdout_truncated: result.stdout_truncated,
            stderr_truncated: result.stderr_truncated,
            bundle_entries: execution.bundle_entries,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tar(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut output = Vec::new();
        for (name, bytes) in entries {
            let mut header = [0u8; 512];
            header[..name.len()].copy_from_slice(name.as_bytes());
            let size = format!("{:011o}", bytes.len());
            header[124..124 + size.len()].copy_from_slice(size.as_bytes());
            header[156] = b'0';
            header[257..263].copy_from_slice(b"ustar\0");
            output.extend_from_slice(&header);
            output.extend_from_slice(bytes);
            output.resize(
                output.len() + bytes.len().div_ceil(512) * 512 - bytes.len(),
                0,
            );
        }
        output.resize(output.len() + 1024, 0);
        output
    }

    #[test]
    fn list_totals_must_match_the_parsed_classes() {
        let counts = MutationCounts {
            generated: 3,
            tested: 3,
            caught: 1,
            missed: 1,
            unviable: 1,
            ..Default::default()
        };
        let bundle = tar(&[
            ("./caught.txt", b"src/lib.rs:1:1: caught one"),
            ("./missed.txt", b"src/lib.rs:2:1: missed one"),
            ("./unviable.txt", b"src/lib.rs:3:1: unviable one"),
        ]);
        let lists = cross_check(&bundle, &counts);
        assert!(lists.agreed);
        assert_eq!(lists.omitted, 0);
        assert_eq!(lists.rows.len(), 3);
        // Missed rows come first so a truncated list still names survivors.
        assert_eq!(lists.rows[0].name(), "src/lib.rs:2:1: missed one");
        assert_eq!(lists.rows[0].class(), MutationOutcomeClass::Missed);

        // One extra line in a list file, with the same outcomes.json, is a
        // disagreement rather than a new fact.
        let forged = tar(&[
            ("./caught.txt", b"src/lib.rs:1:1: caught one\nforged"),
            ("./missed.txt", b"src/lib.rs:2:1: missed one"),
            ("./unviable.txt", b"src/lib.rs:3:1: unviable one"),
        ]);
        assert!(!cross_check(&forged, &counts).agreed);
        // A missing list file is an empty list, which must still match.
        assert!(!cross_check(&tar(&[]), &counts).agreed);
        assert!(
            cross_check(&tar(&[]), &MutationCounts::default()).agreed,
            "an empty report agrees with empty lists"
        );
        // An unreadable list file never becomes an agreement.
        let hostile = tar(&[("./missed.txt", b"line\x07bell")]);
        assert!(!cross_check(&hostile, &counts).agreed);
    }

    #[test]
    fn itemized_rows_are_capped_while_totals_stay_exact() {
        let missed = (0..MUTATION_MAX_ROWS + 5)
            .map(|index| format!("src/lib.rs:{index}:1: replace"))
            .collect::<Vec<_>>()
            .join("\n");
        let counts = MutationCounts {
            generated: 133,
            tested: 133,
            missed: 133,
            ..Default::default()
        };
        let lists = cross_check(&tar(&[("./missed.txt", missed.as_bytes())]), &counts);
        assert!(lists.agreed);
        assert_eq!(lists.rows.len(), MUTATION_MAX_ROWS);
        assert_eq!(lists.omitted, 5);
    }
}
