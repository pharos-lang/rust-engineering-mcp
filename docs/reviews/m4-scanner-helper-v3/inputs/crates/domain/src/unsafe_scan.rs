//! Syntactic evidence only: neither presence nor absence certifies memory safety.
use crate::security::SecurityPackage;
use crate::{ExecutionFingerprint, InvalidCheckOptions, RuntimeIdentity, SourceFingerprint};
use serde::{Deserialize, Serialize};

pub const SCAN_MAX_FILES: usize = 4096;
pub const SCAN_MAX_FINDINGS: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnsafeScanOptions {
    timeout_seconds: u64,
}
impl UnsafeScanOptions {
    pub fn new(timeout_seconds: u64) -> Result<Self, InvalidCheckOptions> {
        if !(1..=120).contains(&timeout_seconds) {
            return Err(InvalidCheckOptions);
        }
        Ok(Self { timeout_seconds })
    }
    pub fn timeout_seconds(&self) -> u64 {
        self.timeout_seconds
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnsafeOrigin {
    Workspace,
    Dependency,
    WorkspaceUnowned,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnsafeKind {
    UnsafeAttribute,
    UnsafeBlock,
    UnsafeExternBlock,
    UnsafeFn,
    UnsafeImpl,
    UnsafeMod,
    UnsafeStatic,
    UnsafeTrait,
    ExternBlock,
    ExternCrate,
    ExternFn,
}
impl UnsafeKind {
    pub fn keyword(self) -> &'static [u8] {
        match self {
            Self::ExternBlock | Self::ExternCrate | Self::ExternFn => b"extern",
            _ => b"unsafe",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct UnsafeFinding {
    pub path: String,
    pub origin: UnsafeOrigin,
    pub package: Option<SecurityPackage>,
    pub file_fingerprint: SourceFingerprint,
    pub kind: UnsafeKind,
    pub byte_start: u32,
    pub byte_end: u32,
    pub line: u32,
    pub column: u32,
    pub conditional: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct UnsafeCoverage {
    pub files_total: u32,
    pub files_selected: u32,
    pub files_omitted: u32,
    pub files_parsed: u32,
    pub files_parse_error: u32,
    pub files_crashed: u32,
    pub files_timed_out: u32,
    pub files_unavailable: u32,
    pub files_invalid_utf8: u32,
    pub files_too_large: u32,
    pub files_budget_exhausted: u32,
    pub workspace_files: u32,
    pub dependency_files: u32,
    pub workspace_unowned_files: u32,
    pub source_bytes: u64,
    pub macro_boundaries_omitted: u64,
    pub opaque_syntax_omitted: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct UnsafeScanReport {
    pub coverage: UnsafeCoverage,
    pub findings: Vec<UnsafeFinding>,
    pub findings_total: u64,
    pub findings_omitted: u64,
    /// Complete only for the declared syntactic input selection; excludes macros/cfg/generated.
    pub syntax_complete: bool,
    pub cfg_evaluated: bool,
    pub macros_expanded: bool,
    pub generated_sources_scanned: bool,
}

impl UnsafeScanReport {
    pub fn validate(&self) -> bool {
        let c = &self.coverage;
        self.findings.len() <= SCAN_MAX_FINDINGS
            && self.findings_total.checked_sub(self.findings_omitted)
                == Some(self.findings.len() as u64)
            && c.files_selected as usize <= SCAN_MAX_FILES
            && c.files_total.checked_sub(c.files_omitted) == Some(c.files_selected)
            && u64::from(c.files_selected)
                == u64::from(c.files_parsed)
                    + u64::from(c.files_parse_error)
                    + u64::from(c.files_crashed)
                    + u64::from(c.files_timed_out)
                    + u64::from(c.files_unavailable)
                    + u64::from(c.files_invalid_utf8)
                    + u64::from(c.files_too_large)
                    + u64::from(c.files_budget_exhausted)
            && u64::from(c.files_selected)
                == u64::from(c.workspace_files)
                    + u64::from(c.dependency_files)
                    + u64::from(c.workspace_unowned_files)
            && !self.cfg_evaluated
            && !self.macros_expanded
            && !self.generated_sources_scanned
            && self.syntax_complete
                == (c.files_omitted == 0
                    && c.opaque_syntax_omitted == 0
                    && c.files_selected == c.files_parsed
                    && self.findings_omitted == 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn omissions_or_failed_files_cannot_claim_complete_syntax() {
        let mut report = UnsafeScanReport {
            coverage: UnsafeCoverage {
                files_total: 2,
                files_selected: 2,
                files_parsed: 1,
                files_crashed: 1,
                workspace_files: 2,
                ..Default::default()
            },
            findings: vec![],
            findings_total: 0,
            findings_omitted: 0,
            syntax_complete: false,
            cfg_evaluated: false,
            macros_expanded: false,
            generated_sources_scanned: false,
        };
        assert!(report.validate());
        report.syntax_complete = true;
        assert!(!report.validate());
        report.coverage.files_parsed = 2;
        report.coverage.files_crashed = 0;
        assert!(report.validate());
        report.findings_total = 1;
        report.findings_omitted = 1;
        assert!(!report.validate());
        report.syntax_complete = false;
        assert!(report.validate());
    }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct UnsafeObservation {
    pub report: UnsafeScanReport,
    pub source_fingerprint: SourceFingerprint,
    pub vendor_fingerprint: SourceFingerprint,
    pub vendor_archive_fingerprint: SourceFingerprint,
    pub metadata_fingerprint: SourceFingerprint,
    pub manifest_fingerprint: SourceFingerprint,
    pub runtime: RuntimeIdentity,
    pub execution_fingerprint: ExecutionFingerprint,
}
