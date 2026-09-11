//! Observed interpreter evidence, not a proof of general memory safety.
use crate::{ExecutionFingerprint, InvalidCheckOptions, RuntimeIdentity, SourceFingerprint};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MiriOptions {
    timeout_seconds: u64,
}
impl MiriOptions {
    pub fn new(timeout_seconds: u64) -> Result<Self, InvalidCheckOptions> {
        if !(1..=1800).contains(&timeout_seconds) {
            return Err(InvalidCheckOptions);
        }
        Ok(Self { timeout_seconds })
    }
    pub fn timeout_seconds(&self) -> u64 {
        self.timeout_seconds
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MiriCategory {
    UndefinedBehavior,
    UnsupportedOperation,
    TestFailure,
    CompileFailure,
    Timeout,
    Unclassified,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MiriFinding {
    pub category: MiriCategory,
    pub test_name: Option<String>,
    pub test_binary: Option<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct MiriCounts {
    pub tests: u32,
    pub passed: u32,
    pub failed: u32,
    pub skipped: u32,
    pub undefined_behavior: u32,
    pub unsupported_operation: u32,
    pub test_failures: u32,
    pub compile_failures: u32,
    pub timeouts: u32,
    pub unclassified: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MiriReport {
    pub counts: MiriCounts,
    pub findings: Vec<MiriFinding>,
    pub findings_omitted: u64,
    pub complete: bool,
    pub clean: bool,
    pub junit_present: bool,
    pub exit_code: Option<i32>,
}
#[derive(Clone, Debug, Serialize)]
pub struct MiriObservation {
    pub report: MiriReport,
    pub source_fingerprint: SourceFingerprint,
    pub vendor_fingerprint: SourceFingerprint,
    pub metadata_fingerprint: SourceFingerprint,
    pub config_fingerprint: SourceFingerprint,
    pub junit_fingerprint: Option<SourceFingerprint>,
    pub runtime: RuntimeIdentity,
    pub execution_fingerprint: ExecutionFingerprint,
    pub nightly_commit: String,
    pub sysroot_fingerprint: SourceFingerprint,
}

impl MiriReport {
    pub fn validate(&self) -> bool {
        let counts = &self.counts;
        let classified = u64::from(counts.undefined_behavior)
            + u64::from(counts.unsupported_operation)
            + u64::from(counts.test_failures)
            + u64::from(counts.compile_failures)
            + u64::from(counts.timeouts)
            + u64::from(counts.unclassified);
        let clean = self.complete
            && self.junit_present
            && self.exit_code == Some(0)
            && counts.tests > 0
            && counts.passed == counts.tests
            && counts.failed == 0
            && counts.skipped == 0
            && classified == 0
            && self.findings_omitted == 0;
        self.clean == clean
            && self.findings.len() <= 128
            && u64::from(counts.tests)
                == u64::from(counts.passed) + u64::from(counts.failed) + u64::from(counts.skipped)
            && self.findings.len() as u64 + self.findings_omitted == classified
            && (!self.complete
                || (self.findings_omitted == 0
                    && counts.skipped == 0
                    && counts.unclassified == 0
                    && counts.timeouts == 0))
    }
}
