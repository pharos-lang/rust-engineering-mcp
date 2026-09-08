//! Bounded security evidence which never publishes the guest's original prose.

use rust_engineering_application::security::{SecurityError, SecurityObservation};
use rust_engineering_domain::security::{
    FindingDisposition, SECURITY_MAX_FINDINGS, SecurityCompleteness, SecurityCounts,
    SecurityEngine, SecurityPackage, SecurityPolicyState, SecuritySeverity,
};
use serde::Serialize;

const SCHEMA_VERSION: u32 = 1;
const MAX_LOG_BYTES: usize = 256 * 1024;

#[derive(Serialize)]
struct StreamEvidence {
    sha256: String,
    bytes: u64,
    truncated: bool,
}

#[derive(Serialize)]
struct RawEvidence {
    stdout: StreamEvidence,
    stderr: StreamEvidence,
}

#[derive(Serialize)]
struct ParseEvidence {
    complete: bool,
    audit_complete: bool,
    licenses: SecurityCounts,
    bans: SecurityCounts,
    sources: SecurityCounts,
    findings_total: u64,
    findings_emitted: u64,
    findings_omitted: u64,
}

#[derive(Serialize)]
struct SafeFinding<'a> {
    engine: SecurityEngine,
    rule: &'a str,
    package: Option<&'a SecurityPackage>,
    severity: SecuritySeverity,
    message: &'static str,
    disposition: &'a FindingDisposition,
}

#[derive(Serialize)]
struct SecurityLog<'a> {
    schema_version: u32,
    raw: &'a RawEvidence,
    parse: ParseEvidence,
    completeness: SecurityCompleteness,
    policy_state: SecurityPolicyState,
    findings: &'a [SafeFinding<'a>],
}

fn static_message(engine: SecurityEngine) -> &'static str {
    match engine {
        SecurityEngine::Rustsec => "RustSec advisory matched a validated package",
        SecurityEngine::Licenses => "License policy rule matched a validated package",
        SecurityEngine::Bans => "Dependency policy rule matched a validated package",
        SecurityEngine::Sources => "Source policy rule matched a validated package",
    }
}

fn stream(bytes: &[u8], truncated: bool) -> Result<StreamEvidence, SecurityError> {
    Ok(StreamEvidence {
        sha256: crate::digest(bytes),
        bytes: u64::try_from(bytes.len()).map_err(|_| SecurityError::OutputLimit)?,
        truncated,
    })
}

/// Serializes the verified security projection without guest-controlled prose.
///
/// Findings are removed as whole rows when the durable artifact budget is
/// reached. The returned value is therefore always complete JSON, and its
/// omission counter covers both upstream omissions and rows removed here.
pub struct SafeSecurityLog {
    pub bytes: Vec<u8>,
    pub findings_removed: u64,
}

pub fn safe_security_log(
    observation: &SecurityObservation,
) -> Result<SafeSecurityLog, SecurityError> {
    observation.deny.validate()?;
    if observation.findings.len() > SECURITY_MAX_FINDINGS {
        return Err(SecurityError::InvalidMetadata);
    }

    let raw = RawEvidence {
        stdout: stream(
            &observation.deny.artifacts.stdout,
            observation.deny.artifacts.stdout_truncated,
        )?,
        stderr: stream(
            &observation.deny.artifacts.stderr,
            observation.deny.artifacts.stderr_truncated,
        )?,
    };
    let findings: Vec<_> = observation
        .findings
        .iter()
        .map(|finding| SafeFinding {
            engine: finding.engine,
            rule: &finding.rule,
            package: finding.package.as_ref(),
            severity: finding.severity,
            message: static_message(finding.engine),
            disposition: &finding.disposition,
        })
        .collect();
    let findings_total = u64::try_from(findings.len())
        .unwrap_or(u64::MAX)
        .saturating_add(observation.findings_omitted);
    let mut emitted = findings.len();

    loop {
        let removed = findings.len().saturating_sub(emitted);
        let parse = ParseEvidence {
            complete: observation.deny.parse_complete,
            audit_complete: observation.audit.validation_complete,
            licenses: observation.deny.licenses,
            bans: observation.deny.bans,
            sources: observation.deny.sources,
            findings_total,
            findings_emitted: u64::try_from(emitted).unwrap_or(u64::MAX),
            findings_omitted: observation
                .findings_omitted
                .saturating_add(u64::try_from(removed).unwrap_or(u64::MAX)),
        };
        let document = SecurityLog {
            schema_version: SCHEMA_VERSION,
            raw: &raw,
            parse,
            completeness: observation.completeness,
            policy_state: observation.policy_state,
            findings: &findings[..emitted],
        };
        let bytes = serde_json::to_vec(&document).map_err(|_| SecurityError::InvalidMetadata)?;
        if bytes.len() <= MAX_LOG_BYTES {
            return Ok(SafeSecurityLog {
                bytes,
                findings_removed: removed as u64,
            });
        }
        if emitted == 0 {
            return Err(SecurityError::OutputLimit);
        }
        emitted -= 1;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // Fixed fixtures should fail immediately when malformed.
mod tests {
    use super::*;
    use rust_engineering_application::security::{DenyObservation, SecurityArtifactStreams};
    use rust_engineering_domain::{
        AuditFinding, AuditObservation, AuditPackage, AuditSource, ExecutionFingerprint,
        ExecutionTermination, RuntimeIdentity, SourceFingerprint,
    };
    use serde_json::Value;

    const STREAM_CANARY: &str = "STREAM_CANARY_f9739f07";
    const PLUGIN_CANARY: &str = "PLUGIN_FREE_TEXT_CANARY_41d7a62e";

    fn source_fingerprint(byte: char) -> SourceFingerprint {
        format!("sha256:{}", byte.to_string().repeat(64))
            .parse()
            .unwrap()
    }

    fn execution_fingerprint(byte: char) -> ExecutionFingerprint {
        format!("sha256:{}", byte.to_string().repeat(64))
            .parse()
            .unwrap()
    }

    fn package(name: String) -> SecurityPackage {
        SecurityPackage {
            name,
            version: "1.2.3".into(),
            source: rust_engineering_domain::security::SecuritySource::CratesIo,
            source_fingerprint: Some(source_fingerprint('a')),
        }
    }

    fn observation(
        findings: Vec<rust_engineering_domain::security::SecurityFinding>,
    ) -> SecurityObservation {
        let execution = execution_fingerprint('b');
        let package = package("fixture".into());
        let mut audit = AuditObservation::unavailable();
        audit.findings.push(AuditFinding {
            advisory_id: "RUSTSEC-2026-0001".into(),
            url: format!("https://guest.invalid/{PLUGIN_CANARY}"),
            title: format!("guest title label note timestamp {PLUGIN_CANARY}"),
            package: AuditPackage {
                name: "fixture".into(),
                version: "1.2.3".into(),
                source: AuditSource::CratesIo,
                source_fingerprint: Some(source_fingerprint('a')),
            },
            patched_requirements: vec![format!(">=9.9.9-{PLUGIN_CANARY}")],
            unaffected_requirements: vec![],
            severity: None,
            informational: Some(PLUGIN_CANARY.into()),
            paths: vec![],
            paths_omitted: 0,
        });
        SecurityObservation {
            audit,
            deny: DenyObservation {
                source_fingerprint: source_fingerprint('1'),
                vendor_fingerprint: source_fingerprint('2'),
                vendor_archive_fingerprint: source_fingerprint('3'),
                policy_fingerprint: source_fingerprint('4'),
                deny_config_fingerprint: source_fingerprint('5'),
                cargo_config_fingerprint: source_fingerprint('6'),
                metadata_original_fingerprint: source_fingerprint('7'),
                metadata_derived_fingerprint: source_fingerprint('8'),
                lock_fingerprint: source_fingerprint('9'),
                runtime: RuntimeIdentity {
                    platform: "linux/arm64".into(),
                    image_id: format!("sha256:{}", "c".repeat(64)),
                    configuration_fingerprint: execution_fingerprint('d'),
                    execution_fingerprint: execution.clone(),
                    rust_version: "rustc 1.98.1".into(),
                    cargo_version: "cargo 1.98.1".into(),
                    declared_toolchain: None,
                },
                execution_fingerprint: execution,
                packages: vec![package],
                declared_licenses: vec![Some("MIT".into())],
                license_files: vec![vec![]],
                enabled_features: vec![vec![]],
                dependency_indices: vec![vec![]],
                workspace_members: vec![0],
                findings: vec![],
                findings_omitted: 0,
                licenses: SecurityCounts {
                    errors: 1,
                    warnings: 2,
                    notes: 3,
                    helps: 4,
                },
                bans: SecurityCounts::default(),
                sources: SecurityCounts::default(),
                parse_complete: false,
                termination: ExecutionTermination::Exited,
                exit_code: Some(2),
                artifacts: SecurityArtifactStreams {
                    stdout: format!("stdout {STREAM_CANARY}").into_bytes(),
                    stderr: vec![0, 1, 2, 0xff],
                    stdout_truncated: true,
                    stderr_truncated: false,
                },
            },
            findings,
            findings_omitted: 7,
            completeness: SecurityCompleteness::Partial,
            policy_state: SecurityPolicyState::Violated,
            assessed_at: rust_engineering_domain::UnixSeconds(1_999_999_999),
        }
    }

    fn finding(name: String) -> rust_engineering_domain::security::SecurityFinding {
        rust_engineering_domain::security::SecurityFinding {
            engine: SecurityEngine::Licenses,
            rule: "license-not-allowed".into(),
            package: Some(package(name)),
            severity: SecuritySeverity::Error,
            message: format!("guest label note timestamp url {PLUGIN_CANARY}"),
            disposition: FindingDisposition::Active,
        }
    }

    #[test]
    fn hashes_raw_streams_without_copying_stream_or_plugin_prose() {
        let observation = observation(vec![finding("fixture".into())]);
        let log = safe_security_log(&observation).unwrap();
        assert_eq!(log.findings_removed, 0);
        let bytes = log.bytes;
        let text = std::str::from_utf8(&bytes).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();

        assert!(bytes.len() <= MAX_LOG_BYTES);
        assert!(!text.contains(STREAM_CANARY));
        assert!(!text.contains(PLUGIN_CANARY));
        assert_eq!(value["schema_version"], 1);
        assert_eq!(
            value["raw"]["stdout"]["sha256"],
            crate::digest(format!("stdout {STREAM_CANARY}").as_bytes())
        );
        assert_eq!(
            value["raw"]["stdout"]["bytes"],
            format!("stdout {STREAM_CANARY}").len()
        );
        assert_eq!(value["raw"]["stdout"]["truncated"], true);
        assert_eq!(
            value["raw"]["stderr"]["sha256"],
            crate::digest(&[0, 1, 2, 0xff])
        );
        assert_eq!(value["raw"]["stderr"]["bytes"], 4);
        assert_eq!(value["parse"]["findings_total"], 8);
        assert_eq!(value["parse"]["findings_emitted"], 1);
        assert_eq!(value["parse"]["findings_omitted"], 7);
        assert_eq!(
            value["findings"][0]["message"],
            "License policy rule matched a validated package"
        );
    }

    #[test]
    fn removes_whole_rows_and_accounts_for_every_size_omission() {
        let findings = (0..SECURITY_MAX_FINDINGS)
            .map(|index| finding(format!("pkg-{index}-{}", "x".repeat(4096))))
            .collect();
        let log = safe_security_log(&observation(findings)).unwrap();
        assert!(log.findings_removed > 0);
        let bytes = log.bytes;
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        let emitted = value["parse"]["findings_emitted"].as_u64().unwrap();
        let omitted = value["parse"]["findings_omitted"].as_u64().unwrap();

        assert!(bytes.len() <= MAX_LOG_BYTES);
        assert!(emitted < SECURITY_MAX_FINDINGS as u64);
        assert_eq!(omitted, 7 + SECURITY_MAX_FINDINGS as u64 - emitted);
        assert_eq!(value["findings"].as_array().unwrap().len() as u64, emitted);
    }
}
