//! Closed host boundary for the isolated, per-file Rust syntax scanner.
use crate::security_metadata::PreparedSecurityMetadata;
use rust_engineering_application::security::SecurityError;
use rust_engineering_domain::security::{SecurityPackage, SecuritySource};
use rust_engineering_domain::unsafe_scan::{
    SCAN_MAX_FILES, SCAN_MAX_FINDINGS, UnsafeCoverage, UnsafeFinding, UnsafeKind, UnsafeOrigin,
    UnsafeScanReport,
};
use rust_engineering_domain::{CargoVendorSnapshot, SourceBundle, SourceFingerprint};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
const MAX_OUTPUT_BYTES: usize = 512 * 1024;
const MAX_SOURCE_BYTES: usize = 1024 * 1024;
const MAX_BUDGET_MS: u64 = 118_000;
const SOURCE_PREFIX: &str = "/source/";
const VENDOR_PREFIX: &str = "/rust-mcp-vendor/";

#[derive(Debug)]
struct PlannedFile {
    guest_path: String,
    report_path: String,
    origin: UnsafeOrigin,
    package: Option<SecurityPackage>,
    bytes: Vec<u8>,
    fingerprint: SourceFingerprint,
}

#[derive(Debug)]
pub(super) struct ScanPlan {
    files: Vec<PlannedFile>,
    files_total: u32,
    files_omitted: u32,
}

#[derive(Clone)]
struct PackageRoot {
    root: String,
    package: SecurityPackage,
}

fn invalid() -> SecurityError {
    SecurityError::InvalidMetadata
}

fn fingerprint(bytes: &[u8]) -> Result<SourceFingerprint, SecurityError> {
    crate::digest(bytes).parse().map_err(|_| invalid())
}

fn valid_root(root: &str) -> bool {
    root.is_empty()
        || root
            .strip_suffix('/')
            .is_some_and(|relative| rust_engineering_domain::validate_source_path(relative).is_ok())
}

fn package_roots(
    metadata: &PreparedSecurityMetadata,
) -> Result<(Vec<PackageRoot>, Vec<PackageRoot>), SecurityError> {
    if metadata.packages.len() != metadata.package_roots.len() {
        return Err(invalid());
    }
    let mut workspace = Vec::new();
    let mut dependencies = Vec::new();
    let mut unique = BTreeSet::new();
    for (root, package) in metadata.package_roots.iter().zip(&metadata.packages) {
        if !valid_root(root) || !unique.insert((package.source, root.as_str())) {
            return Err(invalid());
        }
        let entry = PackageRoot {
            root: root.clone(),
            package: package.clone(),
        };
        match package.source {
            SecuritySource::Workspace => workspace.push(entry),
            SecuritySource::CratesIo => dependencies.push(entry),
            SecuritySource::Unverified => return Err(invalid()),
        }
    }
    for roots in [&mut workspace, &mut dependencies] {
        roots.sort_by(|left, right| {
            right
                .root
                .len()
                .cmp(&left.root.len())
                .then_with(|| left.root.cmp(&right.root))
        });
    }
    Ok((workspace, dependencies))
}

fn matching_package<'a>(path: &str, roots: &'a [PackageRoot]) -> Option<&'a SecurityPackage> {
    roots
        .iter()
        .find(|entry| entry.root.is_empty() || path.starts_with(&entry.root))
        .map(|entry| &entry.package)
}

fn planned_file(
    prefix: &str,
    path: &str,
    bytes: &[u8],
    origin: UnsafeOrigin,
    package: Option<&SecurityPackage>,
) -> Result<PlannedFile, SecurityError> {
    if bytes.len() > MAX_SOURCE_BYTES {
        return Err(SecurityError::OutputLimit);
    }
    Ok(PlannedFile {
        guest_path: format!("{prefix}{path}"),
        report_path: path.to_owned(),
        origin,
        package: package.cloned(),
        bytes: bytes.to_vec(),
        fingerprint: fingerprint(bytes)?,
    })
}

pub(super) fn plan(
    source: &SourceBundle,
    vendor: &CargoVendorSnapshot,
    metadata: &PreparedSecurityMetadata,
) -> Result<ScanPlan, SecurityError> {
    let (workspace_roots, dependency_roots) = package_roots(metadata)?;
    let mut candidates = Vec::new();

    // Workspace files precede dependencies so the bounded selection always retains
    // the project under inspection. SourceBundle has already sorted each group.
    for file in source
        .files()
        .iter()
        .filter(|file| file.path().ends_with(".rs"))
    {
        let package = matching_package(file.path(), &workspace_roots);
        candidates.push(planned_file(
            SOURCE_PREFIX,
            file.path(),
            file.bytes(),
            if package.is_some() {
                UnsafeOrigin::Workspace
            } else {
                UnsafeOrigin::WorkspaceUnowned
            },
            package,
        )?);
    }
    for file in vendor
        .source
        .files()
        .iter()
        .filter(|file| file.path().ends_with(".rs"))
    {
        if let Some(package) = matching_package(file.path(), &dependency_roots) {
            candidates.push(planned_file(
                VENDOR_PREFIX,
                file.path(),
                file.bytes(),
                UnsafeOrigin::Dependency,
                Some(package),
            )?);
        }
    }

    let files_total = u32::try_from(candidates.len()).map_err(|_| invalid())?;
    candidates.truncate(SCAN_MAX_FILES);
    let files_selected = u32::try_from(candidates.len()).map_err(|_| invalid())?;
    Ok(ScanPlan {
        files: candidates,
        files_total,
        files_omitted: files_total
            .checked_sub(files_selected)
            .ok_or_else(invalid)?,
    })
}

#[derive(Serialize)]
struct Manifest<'a> {
    schema_version: u32,
    budget_ms: u64,
    files: Vec<ManifestFile<'a>>,
}

#[derive(Serialize)]
struct ManifestFile<'a> {
    index: u32,
    path: &'a str,
}

impl ScanPlan {
    pub(super) fn manifest_bytes(&self, budget_ms: u64) -> Result<Vec<u8>, SecurityError> {
        if !(1..=MAX_BUDGET_MS).contains(&budget_ms) {
            return Err(invalid());
        }
        let files = self
            .files
            .iter()
            .enumerate()
            .map(|(index, file)| {
                Ok(ManifestFile {
                    index: u32::try_from(index).map_err(|_| invalid())?,
                    path: &file.guest_path,
                })
            })
            .collect::<Result<Vec<_>, SecurityError>>()?;
        let bytes = serde_json::to_vec(&Manifest {
            schema_version: 2,
            budget_ms,
            files,
        })
        .map_err(|_| invalid())?;
        if bytes.len() > MAX_MANIFEST_BYTES {
            return Err(SecurityError::OutputLimit);
        }
        Ok(bytes)
    }

    pub(super) fn parse(
        &self,
        stdout: &[u8],
        stderr: &[u8],
        exit_code: i32,
    ) -> Result<UnsafeScanReport, SecurityError> {
        if stdout.len() > MAX_OUTPUT_BYTES {
            return Err(SecurityError::OutputLimit);
        }
        if stdout.is_empty() || !stderr.is_empty() || exit_code != 0 {
            return Err(invalid());
        }
        crate::deny_json::strict_value(stdout).map_err(|_| invalid())?;
        let output: SupervisorOutput = serde_json::from_slice(stdout).map_err(|_| invalid())?;
        self.validate_output(output)
    }

    fn validate_output(&self, output: SupervisorOutput) -> Result<UnsafeScanReport, SecurityError> {
        if output.schema_version != 2
            || output.cfg_evaluated
            || output.macros_expanded
            || output.generated_sources_scanned
            || output.files.len() != self.files.len()
            || output.findings.len() > SCAN_MAX_FINDINGS
        {
            return Err(invalid());
        }

        let mut coverage = UnsafeCoverage {
            files_total: self.files_total,
            files_selected: u32::try_from(self.files.len()).map_err(|_| invalid())?,
            files_omitted: self.files_omitted,
            ..UnsafeCoverage::default()
        };
        let mut per_file_retained = vec![0_u64; self.files.len()];
        let mut findings = Vec::with_capacity(output.findings.len());
        let mut identities = BTreeSet::new();
        let mut prior_order = None;

        for raw in output.findings {
            let index = usize::try_from(raw.file_index).map_err(|_| invalid())?;
            let file = self.files.get(index).ok_or_else(invalid)?;
            if output.files[index].status != FileStatus::Parsed {
                return Err(invalid());
            }
            let kind = raw.kind.domain();
            let identity = (raw.file_index, kind, raw.byte_start, raw.byte_end);
            let order = (
                raw.file_index,
                raw.byte_start,
                raw.byte_end,
                kind,
                raw.conditional,
            );
            if !identities.insert(identity) || prior_order.is_some_and(|previous| previous >= order)
            {
                return Err(invalid());
            }
            prior_order = Some(order);
            validate_span(file, &raw, kind)?;
            per_file_retained[index] = per_file_retained[index]
                .checked_add(1)
                .ok_or_else(invalid)?;
            findings.push(UnsafeFinding {
                path: file.report_path.clone(),
                origin: file.origin,
                package: file.package.clone(),
                file_fingerprint: file.fingerprint.clone(),
                kind,
                byte_start: u32::try_from(raw.byte_start).map_err(|_| invalid())?,
                byte_end: u32::try_from(raw.byte_end).map_err(|_| invalid())?,
                line: u32::try_from(raw.line).map_err(|_| invalid())?,
                column: u32::try_from(raw.column).map_err(|_| invalid())?,
                conditional: raw.conditional,
            });
        }

        let mut total_findings = 0_u64;
        let mut omitted_findings = 0_u64;
        for (index, summary) in output.files.iter().enumerate() {
            if summary.index != u32::try_from(index).map_err(|_| invalid())?
                || summary.total_findings.checked_sub(summary.omitted_findings)
                    != Some(per_file_retained[index])
                || (summary.status != FileStatus::Parsed
                    && (summary.total_findings != 0
                        || summary.omitted_findings != 0
                        || summary.macro_boundaries_omitted != 0
                        || summary.opaque_syntax_omitted != 0))
                || summary.total_findings > MAX_SOURCE_BYTES as u64
                || summary.omitted_findings > MAX_SOURCE_BYTES as u64
                || summary.macro_boundaries_omitted > MAX_SOURCE_BYTES as u64
                || summary.opaque_syntax_omitted > MAX_SOURCE_BYTES as u64
            {
                return Err(invalid());
            }
            if matches!(summary.status, FileStatus::Parsed | FileStatus::ParseError)
                && std::str::from_utf8(&self.files[index].bytes).is_err()
            {
                return Err(invalid());
            }
            if summary.status == FileStatus::InvalidUtf8
                && std::str::from_utf8(&self.files[index].bytes).is_ok()
            {
                return Err(invalid());
            }
            total_findings = total_findings
                .checked_add(summary.total_findings)
                .ok_or_else(invalid)?;
            omitted_findings = omitted_findings
                .checked_add(summary.omitted_findings)
                .ok_or_else(invalid)?;
            coverage.macro_boundaries_omitted = coverage
                .macro_boundaries_omitted
                .checked_add(summary.macro_boundaries_omitted)
                .ok_or_else(invalid)?;
            coverage.opaque_syntax_omitted = coverage
                .opaque_syntax_omitted
                .checked_add(summary.opaque_syntax_omitted)
                .ok_or_else(invalid)?;
            match summary.status {
                FileStatus::Parsed => coverage.files_parsed += 1,
                FileStatus::ParseError => coverage.files_parse_error += 1,
                FileStatus::Crashed => coverage.files_crashed += 1,
                FileStatus::TimedOut => coverage.files_timed_out += 1,
                FileStatus::Unavailable => coverage.files_unavailable += 1,
                FileStatus::InvalidUtf8 => coverage.files_invalid_utf8 += 1,
                FileStatus::TooLarge => coverage.files_too_large += 1,
                FileStatus::BudgetExhausted => coverage.files_budget_exhausted += 1,
            }
        }
        if output.total_findings != total_findings
            || output.omitted_findings != omitted_findings
            || total_findings.checked_sub(omitted_findings)
                != Some(u64::try_from(findings.len()).map_err(|_| invalid())?)
        {
            return Err(invalid());
        }

        for file in &self.files {
            coverage.source_bytes = coverage
                .source_bytes
                .checked_add(u64::try_from(file.bytes.len()).map_err(|_| invalid())?)
                .ok_or_else(invalid)?;
            match file.origin {
                UnsafeOrigin::Workspace => coverage.workspace_files += 1,
                UnsafeOrigin::Dependency => coverage.dependency_files += 1,
                UnsafeOrigin::WorkspaceUnowned => coverage.workspace_unowned_files += 1,
            }
        }
        let syntax_complete = coverage.files_omitted == 0
            && coverage.opaque_syntax_omitted == 0
            && coverage.files_selected == coverage.files_parsed
            && omitted_findings == 0;
        let report = UnsafeScanReport {
            coverage,
            findings,
            findings_total: total_findings,
            findings_omitted: omitted_findings,
            syntax_complete,
            cfg_evaluated: false,
            macros_expanded: false,
            generated_sources_scanned: false,
        };
        if !report.validate() {
            return Err(invalid());
        }
        Ok(report)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum FileStatus {
    Parsed,
    ParseError,
    Crashed,
    TimedOut,
    Unavailable,
    InvalidUtf8,
    TooLarge,
    BudgetExhausted,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum WireKind {
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

impl WireKind {
    fn domain(self) -> UnsafeKind {
        match self {
            Self::UnsafeAttribute => UnsafeKind::UnsafeAttribute,
            Self::UnsafeBlock => UnsafeKind::UnsafeBlock,
            Self::UnsafeExternBlock => UnsafeKind::UnsafeExternBlock,
            Self::UnsafeFn => UnsafeKind::UnsafeFn,
            Self::UnsafeImpl => UnsafeKind::UnsafeImpl,
            Self::UnsafeMod => UnsafeKind::UnsafeMod,
            Self::UnsafeStatic => UnsafeKind::UnsafeStatic,
            Self::UnsafeTrait => UnsafeKind::UnsafeTrait,
            Self::ExternBlock => UnsafeKind::ExternBlock,
            Self::ExternCrate => UnsafeKind::ExternCrate,
            Self::ExternFn => UnsafeKind::ExternFn,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireFinding {
    file_index: u32,
    kind: WireKind,
    byte_start: usize,
    byte_end: usize,
    line: usize,
    column: usize,
    conditional: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileSummary {
    #[serde(rename = "i")]
    index: u32,
    #[serde(rename = "s")]
    status: FileStatus,
    #[serde(rename = "total")]
    total_findings: u64,
    #[serde(rename = "omitted")]
    omitted_findings: u64,
    #[serde(rename = "macros")]
    macro_boundaries_omitted: u64,
    #[serde(rename = "opaque")]
    opaque_syntax_omitted: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SupervisorOutput {
    schema_version: u32,
    files: Vec<FileSummary>,
    findings: Vec<WireFinding>,
    total_findings: u64,
    omitted_findings: u64,
    cfg_evaluated: bool,
    macros_expanded: bool,
    generated_sources_scanned: bool,
}

fn validate_span(
    file: &PlannedFile,
    finding: &WireFinding,
    kind: UnsafeKind,
) -> Result<(), SecurityError> {
    if finding.byte_start >= finding.byte_end || finding.byte_end > file.bytes.len() {
        return Err(invalid());
    }
    let source = std::str::from_utf8(&file.bytes).map_err(|_| invalid())?;
    let keyword = source
        .get(finding.byte_start..finding.byte_end)
        .ok_or_else(invalid)?;
    if keyword.as_bytes() != kind.keyword()
        || !keyword_boundary(source, finding.byte_start, finding.byte_end)
    {
        return Err(invalid());
    }
    let before = source.get(..finding.byte_start).ok_or_else(invalid)?;
    let line = before.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = before
        .rsplit_once('\n')
        .map_or(before, |(_, suffix)| suffix)
        .chars()
        .count()
        + 1;
    if finding.line != line || finding.column != column {
        return Err(invalid());
    }
    Ok(())
}

fn keyword_boundary(source: &str, start: usize, end: usize) -> bool {
    // Be conservative without reproducing Rust's Unicode XID tables in the host.
    // A non-ASCII scalar other than Rust's Unicode whitespace could continue an
    // identifier; Rust has no non-ASCII punctuation operators.
    let identifier = |character: char| {
        character == '_'
            || character.is_ascii_alphanumeric()
            || (!character.is_ascii()
                && !character.is_whitespace()
                && !matches!(character, '\u{200e}' | '\u{200f}'))
    };
    !source
        .get(..start)
        .and_then(|prefix| prefix.chars().next_back())
        .is_some_and(identifier)
        && !source
            .get(end..)
            .and_then(|suffix| suffix.chars().next())
            .is_some_and(identifier)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // Fixed boundary fixtures keep assertions concise.
mod tests {
    use super::*;
    use rust_engineering_domain::{CargoVendorPackage, SourceFile};

    fn hash(bytes: &[u8]) -> SourceFingerprint {
        fingerprint(bytes).unwrap()
    }

    fn file(path: &str, bytes: &[u8]) -> SourceFile {
        SourceFile::new(path.to_owned(), bytes.to_vec()).unwrap()
    }

    fn bundle(files: Vec<SourceFile>) -> SourceBundle {
        SourceBundle::new(files).unwrap()
    }

    fn package(name: &str, source: SecuritySource) -> SecurityPackage {
        SecurityPackage {
            name: name.to_owned(),
            version: "1.0.0".to_owned(),
            source,
            source_fingerprint: Some(hash(name.as_bytes())),
        }
    }

    fn metadata(packages: Vec<SecurityPackage>, roots: &[&str]) -> PreparedSecurityMetadata {
        let count = packages.len();
        PreparedSecurityMetadata {
            derived: vec![],
            packages,
            package_roots: roots.iter().map(|root| (*root).to_owned()).collect(),
            original_fingerprint: hash(b"original"),
            derived_fingerprint: hash(b"derived"),
            declared_licenses: vec![None; count],
            license_files: vec![vec![]; count],
            enabled_features: vec![vec![]; count],
            dependency_indices: vec![vec![]; count],
            workspace_members: vec![0],
            lock_fingerprint: hash(b"lock"),
        }
    }

    fn vendor(files: Vec<SourceFile>) -> CargoVendorSnapshot {
        CargoVendorSnapshot {
            source: bundle(files),
            tree_fingerprint: hash(b"vendor"),
            packages: vec![CargoVendorPackage {
                name: "used".into(),
                version: "1.0.0".into(),
                package_checksum: hash(b"used"),
            }],
        }
    }

    fn basic_plan(bytes: &[u8]) -> ScanPlan {
        plan(
            &bundle(vec![file("app/src/lib.rs", bytes)]),
            &vendor(vec![]),
            &metadata(vec![package("app", SecuritySource::Workspace)], &["app/"]),
        )
        .unwrap()
    }

    fn response(files: serde_json::Value, findings: serde_json::Value) -> Vec<u8> {
        let total = findings.as_array().unwrap().len();
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 2,
            "files": files,
            "findings": findings,
            "total_findings": total,
            "omitted_findings": 0,
            "cfg_evaluated": false,
            "macros_expanded": false,
            "generated_sources_scanned": false
        }))
        .unwrap()
    }

    #[test]
    fn plan_is_workspace_first_longest_root_and_excludes_unused_vendor() {
        let source = bundle(vec![
            file("app/src/lib.rs", b"fn app() {}"),
            file("app/nested/src/lib.rs", b"fn nested() {}"),
            file("notes.txt", b"ignored"),
            file("unowned.rs", b"fn loose() {}"),
        ]);
        let vendor = vendor(vec![
            file("unused-1.0.0/src/lib.rs", b"fn unused() {}"),
            file("used-1.0.0/src/lib.rs", b"fn used() {}"),
        ]);
        let metadata = metadata(
            vec![
                package("app", SecuritySource::Workspace),
                package("nested", SecuritySource::Workspace),
                package("used", SecuritySource::CratesIo),
            ],
            &["app/", "app/nested/", "used-1.0.0/"],
        );
        let plan = plan(&source, &vendor, &metadata).unwrap();
        assert_eq!(plan.files_total, 4);
        assert_eq!(plan.files_omitted, 0);
        assert_eq!(
            plan.files
                .iter()
                .map(|file| (
                    file.guest_path.as_str(),
                    file.origin,
                    file.package.as_ref().map(|p| p.name.as_str())
                ))
                .collect::<Vec<_>>(),
            vec![
                (
                    "/source/app/nested/src/lib.rs",
                    UnsafeOrigin::Workspace,
                    Some("nested")
                ),
                (
                    "/source/app/src/lib.rs",
                    UnsafeOrigin::Workspace,
                    Some("app")
                ),
                ("/source/unowned.rs", UnsafeOrigin::WorkspaceUnowned, None),
                (
                    "/rust-mcp-vendor/used-1.0.0/src/lib.rs",
                    UnsafeOrigin::Dependency,
                    Some("used")
                ),
            ]
        );
        assert_eq!(
            String::from_utf8(plan.manifest_bytes(118_000).unwrap()).unwrap(),
            r#"{"schema_version":2,"budget_ms":118000,"files":[{"index":0,"path":"/source/app/nested/src/lib.rs"},{"index":1,"path":"/source/app/src/lib.rs"},{"index":2,"path":"/source/unowned.rs"},{"index":3,"path":"/rust-mcp-vendor/used-1.0.0/src/lib.rs"}]}"#
        );
        assert_eq!(plan.manifest_bytes(0), Err(SecurityError::InvalidMetadata));
        assert_eq!(
            plan.manifest_bytes(MAX_BUDGET_MS + 1),
            Err(SecurityError::InvalidMetadata)
        );
    }

    #[test]
    fn selection_cap_counts_eligible_omissions() {
        let source = bundle(
            (0..SCAN_MAX_FILES)
                .map(|index| file(&format!("f{index:04}.rs"), b""))
                .collect(),
        );
        let metadata = metadata(
            vec![
                package("workspace", SecuritySource::Workspace),
                package("used", SecuritySource::CratesIo),
            ],
            &["workspace/", "used-1.0.0/"],
        );
        let plan = plan(
            &source,
            &vendor(vec![file("used-1.0.0/src/lib.rs", b"")]),
            &metadata,
        )
        .unwrap();
        assert_eq!(plan.files.len(), SCAN_MAX_FILES);
        assert_eq!(plan.files_total, 4097);
        assert_eq!(plan.files_omitted, 1);
        assert!(
            plan.files
                .iter()
                .all(|file| file.origin == UnsafeOrigin::WorkspaceUnowned)
        );
    }

    #[test]
    fn utf8_crlf_span_is_bound_to_captured_bytes_and_fingerprint() {
        let bytes = "// é\r\nunsafe fn f() {}\r\n".as_bytes();
        let plan = basic_plan(bytes);
        let stdout = response(
            serde_json::json!([{"i":0,"s":"parsed","total":1,"omitted":0,"macros":2,"opaque":0}]),
            serde_json::json!([{"file_index":0,"kind":"unsafe_fn","byte_start":7,"byte_end":13,"line":2,"column":1,"conditional":false}]),
        );
        let report = plan.parse(&stdout, b"", 0).unwrap();
        assert!(report.syntax_complete);
        assert_eq!(report.coverage.macro_boundaries_omitted, 2);
        assert_eq!(report.coverage.source_bytes, bytes.len() as u64);
        assert_eq!(report.findings[0].path, "app/src/lib.rs");
        assert_eq!(report.findings[0].file_fingerprint, hash(bytes));
    }

    #[test]
    fn coherent_finding_omissions_are_accepted_but_partial() {
        let plan = basic_plan(b"unsafe fn f() {}\n");
        let stdout = serde_json::to_vec(&serde_json::json!({
            "schema_version": 2,
            "files": [{"i":0,"s":"parsed","total":2,"omitted":1,"macros":0,"opaque":0}],
            "findings": [{"file_index":0,"kind":"unsafe_fn","byte_start":0,"byte_end":6,"line":1,"column":1,"conditional":false}],
            "total_findings": 2,
            "omitted_findings": 1,
            "cfg_evaluated": false,
            "macros_expanded": false,
            "generated_sources_scanned": false
        }))
        .unwrap();
        let report = plan.parse(&stdout, b"", 0).unwrap();
        assert_eq!(report.findings_total, 2);
        assert_eq!(report.findings_omitted, 1);
        assert!(!report.syntax_complete);
    }

    #[test]
    fn v2_kinds_and_opaque_syntax_are_bound_and_partial() {
        let source = b"unsafe trait T {}\nunsafe extern \"C\" { unsafe static X: u8; }\n";
        let plan = basic_plan(source);
        let extern_unsafe = source[1..]
            .windows(6)
            .position(|bytes| bytes == b"unsafe")
            .map(|position| position + 1)
            .unwrap_or(usize::MAX);
        let static_unsafe = source[extern_unsafe + 1..]
            .windows(6)
            .position(|bytes| bytes == b"unsafe")
            .map(|position| position + extern_unsafe + 1)
            .unwrap_or(usize::MAX);
        let extern_keyword = extern_unsafe + 7;
        let stdout = response(
            serde_json::json!([{"i":0,"s":"parsed","total":4,"omitted":0,"macros":0,"opaque":1}]),
            serde_json::json!([
                {"file_index":0,"kind":"unsafe_trait","byte_start":0,"byte_end":6,"line":1,"column":1,"conditional":false},
                {"file_index":0,"kind":"unsafe_extern_block","byte_start":extern_unsafe,"byte_end":extern_unsafe+6,"line":2,"column":1,"conditional":false},
                {"file_index":0,"kind":"extern_block","byte_start":extern_keyword,"byte_end":extern_keyword+6,"line":2,"column":8,"conditional":false},
                {"file_index":0,"kind":"unsafe_static","byte_start":static_unsafe,"byte_end":static_unsafe+6,"line":2,"column":21,"conditional":false}
            ]),
        );
        let report = plan.parse(&stdout, b"", 0).unwrap();
        assert_eq!(report.coverage.opaque_syntax_omitted, 1);
        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.kind == UnsafeKind::UnsafeTrait)
        );
        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.kind == UnsafeKind::UnsafeStatic)
        );
        assert!(!report.syntax_complete);
    }

    #[test]
    fn failed_files_are_retained_as_partial_coverage() {
        let source = bundle(vec![
            file("app/a.rs", b"fn a() {}"),
            file("app/b.rs", b"not rust"),
            file("app/c.rs", &[0xff]),
            file("app/d.rs", b"fn d() {}"),
            file("app/e.rs", b"fn e() {}"),
            file("app/f.rs", b"fn f() {}"),
            file("app/g.rs", b"fn g() {}"),
            file("app/h.rs", b"fn h() {}"),
        ]);
        let plan = plan(
            &source,
            &vendor(vec![]),
            &metadata(vec![package("app", SecuritySource::Workspace)], &["app/"]),
        )
        .unwrap();
        let stdout = response(
            serde_json::json!([
                {"i":0,"s":"parsed","total":0,"omitted":0,"macros":0,"opaque":0},
                {"i":1,"s":"parse_error","total":0,"omitted":0,"macros":0,"opaque":0},
                {"i":2,"s":"invalid_utf8","total":0,"omitted":0,"macros":0,"opaque":0},
                {"i":3,"s":"unavailable","total":0,"omitted":0,"macros":0,"opaque":0},
                {"i":4,"s":"too_large","total":0,"omitted":0,"macros":0,"opaque":0},
                {"i":5,"s":"crashed","total":0,"omitted":0,"macros":0,"opaque":0},
                {"i":6,"s":"timed_out","total":0,"omitted":0,"macros":0,"opaque":0},
                {"i":7,"s":"budget_exhausted","total":0,"omitted":0,"macros":0,"opaque":0}
            ]),
            serde_json::json!([]),
        );
        let report = plan.parse(&stdout, b"", 0).unwrap();
        assert_eq!(report.coverage.files_parsed, 1);
        assert_eq!(report.coverage.files_parse_error, 1);
        assert_eq!(report.coverage.files_invalid_utf8, 1);
        assert_eq!(report.coverage.files_unavailable, 1);
        assert_eq!(report.coverage.files_too_large, 1);
        assert_eq!(report.coverage.files_crashed, 1);
        assert_eq!(report.coverage.files_timed_out, 1);
        assert_eq!(report.coverage.files_budget_exhausted, 1);
        assert!(!report.syntax_complete);
    }

    #[test]
    fn rejects_duplicate_fields_unknown_flags_and_transport_failures() {
        let plan = basic_plan(b"fn f() {}");
        let duplicate = br#"{"schema_version":2,"schema_version":2,"files":[],"findings":[],"total_findings":0,"omitted_findings":0,"cfg_evaluated":false,"macros_expanded":false,"generated_sources_scanned":false}"#;
        assert_eq!(
            plan.parse(duplicate, b"", 0),
            Err(SecurityError::InvalidMetadata)
        );
        let unknown = br#"{"schema_version":2,"files":[{"i":0,"s":"parsed","total":0,"omitted":0,"macros":0,"opaque":0}],"findings":[],"total_findings":0,"omitted_findings":0,"cfg_evaluated":true,"macros_expanded":false,"generated_sources_scanned":false}"#;
        assert_eq!(
            plan.parse(unknown, b"", 0),
            Err(SecurityError::InvalidMetadata)
        );
        let unknown_field = br#"{"schema_version":2,"files":[{"i":0,"s":"parsed","total":0,"omitted":0,"macros":0,"opaque":0}],"findings":[],"total_findings":0,"omitted_findings":0,"cfg_evaluated":false,"macros_expanded":false,"generated_sources_scanned":false,"note":"guest canary"}"#;
        assert_eq!(
            plan.parse(unknown_field, b"", 0),
            Err(SecurityError::InvalidMetadata)
        );
        let valid = response(
            serde_json::json!([{"i":0,"s":"parsed","total":0,"omitted":0,"macros":0,"opaque":0}]),
            serde_json::json!([]),
        );
        assert_eq!(
            plan.parse(&valid, b"canary", 0),
            Err(SecurityError::InvalidMetadata)
        );
        assert_eq!(
            plan.parse(&valid, b"", 2),
            Err(SecurityError::InvalidMetadata)
        );
        assert_eq!(
            plan.parse(&vec![b' '; MAX_OUTPUT_BYTES + 1], b"", 0),
            Err(SecurityError::OutputLimit)
        );

        let finding = serde_json::json!({"file_index":0,"kind":"unsafe_fn","byte_start":0,"byte_end":6,"line":1,"column":1,"conditional":false});
        let too_many = response(
            serde_json::json!([{"i":0,"s":"parsed","total":129,"omitted":0,"macros":0,"opaque":0}]),
            serde_json::Value::Array(vec![finding; SCAN_MAX_FINDINGS + 1]),
        );
        assert_eq!(
            plan.parse(&too_many, b"", 0),
            Err(SecurityError::InvalidMetadata)
        );

        let binary_plan = basic_plan(&[0xff]);
        assert_eq!(
            binary_plan.parse(&valid, b"", 0),
            Err(SecurityError::InvalidMetadata)
        );
    }

    #[test]
    fn rejects_invalid_counts_duplicate_findings_and_invalid_spans() {
        let bytes = b"unsafe fn f() {}\n";
        let plan = basic_plan(bytes);
        let finding = serde_json::json!({"file_index":0,"kind":"unsafe_fn","byte_start":0,"byte_end":6,"line":1,"column":1,"conditional":false});
        let duplicate = response(
            serde_json::json!([{"i":0,"s":"parsed","total":2,"omitted":0,"macros":0,"opaque":0}]),
            serde_json::json!([finding.clone(), finding.clone()]),
        );
        assert_eq!(
            plan.parse(&duplicate, b"", 0),
            Err(SecurityError::InvalidMetadata)
        );

        let bad_count = response(
            serde_json::json!([{"i":0,"s":"parsed","total":2,"omitted":1,"macros":0,"opaque":0}]),
            serde_json::json!([finding.clone()]),
        );
        assert_eq!(
            plan.parse(&bad_count, b"", 0),
            Err(SecurityError::InvalidMetadata)
        );

        let bad_position = response(
            serde_json::json!([{"i":0,"s":"parsed","total":1,"omitted":0,"macros":0,"opaque":0}]),
            serde_json::json!([{"file_index":0,"kind":"unsafe_fn","byte_start":0,"byte_end":6,"line":1,"column":2,"conditional":false}]),
        );
        assert_eq!(
            plan.parse(&bad_position, b"", 0),
            Err(SecurityError::InvalidMetadata)
        );

        let wrong_keyword = response(
            serde_json::json!([{"i":0,"s":"parsed","total":1,"omitted":0,"macros":0,"opaque":0}]),
            serde_json::json!([{"file_index":0,"kind":"extern_fn","byte_start":0,"byte_end":6,"line":1,"column":1,"conditional":false}]),
        );
        assert_eq!(
            plan.parse(&wrong_keyword, b"", 0),
            Err(SecurityError::InvalidMetadata)
        );
    }
}
