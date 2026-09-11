//! Explicit M4 isolated-scanner qualification; never part of runtime discovery.

use crate::*;
use rust_engineering_domain::unsafe_scan::{UnsafeKind, UnsafeOrigin, UnsafeScanReport};
use rust_engineering_domain::{
    CargoVendorPackage, CargoVendorSnapshot, SourceBundle, SourceFile, SourceFingerprint,
};
use serde::Serialize;
use std::path::{Path, PathBuf};

const M4_SCANNER_IMAGE: &str =
    "sha256:25ed3626e710081a571a86a29521eaf2e890e796afd422ba5e409e0ce1891635";
const VENDOR_FINGERPRINT: &str =
    "sha256:743947d5788c1a4385a4b59869c5b8bd0535f7fc0d875b51288f9b26b2d0eba1";
const UNICODE_IDENT_CHECKSUM: &str =
    "sha256:e6e4313cd5fcd3dad5cafa179702e2b244f760991f45397d14d4ebf38247da75";
const MAX_SOURCE_BYTES: usize = 1024 * 1024;

#[derive(Serialize)]
struct CaseEvidence {
    case: &'static str,
    elapsed_ms: u64,
    exit_code: i32,
    stdout_sha256: String,
    stderr_sha256: String,
    source_archive_sha256: String,
    vendor_tree_sha256: String,
    manifest_sha256: SourceFingerprint,
    execution_sha256: rust_engineering_domain::ExecutionFingerprint,
    report: UnsafeScanReport,
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/m4-scanner-native")
}

fn file(path: &str, bytes: Vec<u8>) -> Result<SourceFile, Box<dyn std::error::Error>> {
    SourceFile::new(path.into(), bytes).map_err(|error| format!("{path}: {error:?}").into())
}

fn project() -> Result<SourceBundle, Box<dyn std::error::Error>> {
    let root = fixture_root();
    SourceBundle::new(
        ["Cargo.toml", "Cargo.lock", "LICENSE-MIT", "src/lib.rs"]
            .into_iter()
            .map(|path| file(path, std::fs::read(root.join(path))?))
            .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?,
    )
    .map_err(|error| format!("project: {error:?}").into())
}

fn with_files(
    source: &SourceBundle,
    additions: impl IntoIterator<Item = (String, Vec<u8>)>,
) -> Result<SourceBundle, Box<dyn std::error::Error>> {
    let mut files = source.files().to_vec();
    for (path, bytes) in additions {
        files.push(file(&path, bytes)?);
    }
    SourceBundle::new(files).map_err(|error| format!("project extension: {error:?}").into())
}

fn vendor() -> Result<CargoVendorSnapshot, Box<dyn std::error::Error>> {
    fn visit(
        root: &Path,
        at: &Path,
        files: &mut Vec<SourceFile>,
        directories: &mut Vec<String>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut entries = std::fs::read_dir(at)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let path = entry.path();
            let relative = path
                .strip_prefix(root)?
                .to_str()
                .ok_or("non-UTF-8 vendor fixture path")?;
            let kind = entry.file_type()?;
            if kind.is_dir() {
                directories.push(relative.into());
                visit(root, &path, files, directories)?;
            } else if kind.is_file() {
                files.push(file(relative, std::fs::read(&path)?)?);
            } else {
                return Err(format!("special vendor fixture entry: {relative}").into());
            }
        }
        Ok(())
    }

    let root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/cargo-vendor-data/vendor");
    let mut files = Vec::new();
    let mut directories = Vec::new();
    visit(&root, &root, &mut files, &mut directories)?;
    let source = SourceBundle::with_directories(files, directories)
        .map_err(|error| format!("vendor: {error:?}"))?;
    let observed = resolution_gateway::tree_fingerprint(&source)
        .map_err(|error| format!("vendor fingerprint: {error:?}"))?;
    let expected: SourceFingerprint = VENDOR_FINGERPRINT.parse()?;
    assert_eq!(observed, expected, "approved vendor fixture changed");
    Ok(CargoVendorSnapshot {
        source,
        tree_fingerprint: expected,
        packages: vec![CargoVendorPackage {
            name: "unicode-ident".into(),
            version: "1.0.24".into(),
            package_checksum: UNICODE_IDENT_CHECKSUM.parse()?,
        }],
    })
}

fn gateway(state_root: PathBuf) -> Result<RustGateway, Box<dyn std::error::Error>> {
    let gateway = RustGateway::new_m4_for_qualification(HostDockerConfig {
        executable: "/Applications/Docker.app/Contents/Resources/bin/docker".into(),
        socket: "/Users/cburgosro/.docker/run/docker.sock".into(),
        state_root,
        image_id: M4_SCANNER_IMAGE.into(),
    })
    .map_err(|error| format!("gateway: {error:?}"))?;
    gateway.set_verified(true);
    Ok(gateway)
}

fn inventory_is_empty(gateway: &RustGateway) -> Result<(), Box<dyn std::error::Error>> {
    for (kind, all, format) in [
        ("container", true, "--format={{.ID}}"),
        ("volume", false, "--format={{.Name}}"),
    ] {
        let mut arguments = vec![kind.into(), "ls".into()];
        if all {
            arguments.push("--all".into());
        }
        arguments.push("--filter=label=org.rust-mcp.execution=true".into());
        arguments.push(format.into());
        let found = gateway
            .inner
            .control(&arguments)
            .map_err(|error| format!("{kind} inventory: {error:?}"))?;
        assert_eq!(found.code, Some(0));
        assert!(
            found.stdout.iter().all(u8::is_ascii_whitespace),
            "leftover {kind}: {}",
            String::from_utf8_lossy(&found.stdout)
        );
    }
    Ok(())
}

fn execute_case(
    gateway: &RustGateway,
    name: &'static str,
    source: &SourceBundle,
    vendor: &CargoVendorSnapshot,
    wall_ms: u64,
) -> Result<CaseEvidence, Box<dyn std::error::Error>> {
    let started = std::time::Instant::now();
    let source_before = source.clone();
    let vendor_before = vendor.clone();
    let source_bytes = source_archive::encode(source)
        .map_err(|error| format!("{name}: source archive: {error:?}"))?;
    let source_hash = digest(&source_bytes);
    let vendor_hash = vendor.tree_fingerprint.to_string();
    let execution = security_gateway::execute_scan(
        gateway,
        source,
        vendor,
        ExecutionLimits::new_job(wall_ms, 512 * 1024).ok_or("limits")?,
        &NeverCancel,
    )
    .map_err(|error| format!("{name}: {error:?}"))?;
    assert_eq!(execution.capture.code, Some(0), "{name}");
    assert!(!execution.capture.stdout_truncated, "{name}");
    assert!(!execution.capture.stderr_truncated, "{name}");
    let plan = execution.scan_plan.as_ref().ok_or("missing scan plan")?;
    let report = plan
        .parse(
            &execution.capture.stdout,
            &execution.capture.stderr,
            execution.capture.code.ok_or("missing exit")?,
        )
        .map_err(|error| format!("{name} parse: {error:?}"))?;
    assert!(report.validate(), "{name}");
    assert_eq!(*source, source_before, "{name}: source mutated");
    assert_eq!(*vendor, vendor_before, "{name}: vendor mutated");
    let source_after = source_archive::encode(source)
        .map_err(|error| format!("{name}: source archive after: {error:?}"))?;
    assert_eq!(digest(&source_after), source_hash);
    assert_eq!(vendor.tree_fingerprint.to_string(), vendor_hash);
    inventory_is_empty(gateway)?;
    Ok(CaseEvidence {
        case: name,
        elapsed_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
        exit_code: 0,
        stdout_sha256: digest(&execution.capture.stdout),
        stderr_sha256: digest(&execution.capture.stderr),
        source_archive_sha256: source_hash,
        vendor_tree_sha256: vendor_hash,
        manifest_sha256: execution.manifest_fingerprint,
        execution_sha256: execution.execution_fingerprint,
        report,
    })
}

fn fixture_case(name: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    Ok(std::fs::read(fixture_root().join("cases").join(name))?)
}

fn repeated_source(prefix: &[u8], repeated: &[u8], count: usize, suffix: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(prefix.len() + repeated.len() * count + suffix.len());
    bytes.extend_from_slice(prefix);
    for _ in 0..count {
        bytes.extend_from_slice(repeated);
    }
    bytes.extend_from_slice(suffix);
    bytes
}

#[test]
#[ignore = "explicit provisioned M4 scanner image and local Docker; isolated parser qualification"]
fn m4_scanner_native_oracles_preserve_partial_results_inputs_and_cleanup()
-> Result<(), Box<dyn std::error::Error>> {
    let nonce = state::nonce().map_err(|error| format!("nonce: {error:?}"))?;
    let state_root = PathBuf::from("/private/tmp").join(format!("m4-scanner-native-{nonce}"));
    std::fs::create_dir(&state_root)?;
    let gateway = gateway(state_root.clone())?;
    let base = project()?;
    let vendor = vendor()?;
    let mut cases = Vec::new();

    let authentic = execute_case(
        &gateway,
        "workspace-vendor-authentic",
        &base,
        &vendor,
        120_000,
    )?;
    assert_eq!(authentic.report.coverage.workspace_files, 1);
    assert!(authentic.report.coverage.dependency_files >= 2);
    assert_eq!(authentic.report.findings_total, 2);
    assert!(authentic.report.findings.iter().all(|finding| {
        finding.origin == UnsafeOrigin::Dependency
            && finding.kind == UnsafeKind::UnsafeBlock
            && finding.package.as_ref().is_some_and(|package| {
                package.name == "unicode-ident" && package.version == "1.0.24"
            })
    }));
    cases.push(authentic);

    let syntax = with_files(
        &base,
        [("src/syntax.rs".into(), fixture_case("syntax.source")?)],
    )?;
    let syntax = execute_case(
        &gateway,
        "syntax-unicode-cfg-macro",
        &syntax,
        &vendor,
        120_000,
    )?;
    let syntax_findings = syntax
        .report
        .findings
        .iter()
        .filter(|finding| finding.path == "src/syntax.rs")
        .collect::<Vec<_>>();
    assert!(syntax_findings.iter().any(|finding| {
        finding.kind == UnsafeKind::UnsafeTrait
            && finding.byte_start == 64
            && finding.line == 2
            && finding.column == 1
    }));
    for kind in [
        UnsafeKind::UnsafeTrait,
        UnsafeKind::UnsafeImpl,
        UnsafeKind::UnsafeExternBlock,
        UnsafeKind::UnsafeStatic,
        UnsafeKind::ExternBlock,
        UnsafeKind::UnsafeFn,
        UnsafeKind::UnsafeBlock,
    ] {
        assert!(syntax_findings.iter().any(|finding| finding.kind == kind));
    }
    assert!(syntax_findings.iter().any(|finding| finding.conditional));
    assert!(syntax.report.coverage.macro_boundaries_omitted >= 1);
    cases.push(syntax);

    let invalid_utf8 = with_files(
        &base,
        [(
            "src/invalid-utf8.rs".into(),
            b"fn prefix() {}\n\xff\xfe".to_vec(),
        )],
    )?;
    let invalid_utf8 = execute_case(&gateway, "invalid-utf8", &invalid_utf8, &vendor, 120_000)?;
    assert_eq!(invalid_utf8.report.coverage.files_invalid_utf8, 1);
    assert!(!invalid_utf8.report.syntax_complete);
    assert_eq!(invalid_utf8.report.findings_total, 2);
    cases.push(invalid_utf8);

    let parse_error = with_files(
        &base,
        [(
            "src/parse-error.rs".into(),
            fixture_case("parse-error.source")?,
        )],
    )?;
    let parse_error = execute_case(&gateway, "parse-error", &parse_error, &vendor, 120_000)?;
    assert_eq!(parse_error.report.coverage.files_parse_error, 1);
    assert!(!parse_error.report.syntax_complete);
    assert_eq!(parse_error.report.findings_total, 2);
    cases.push(parse_error);

    let maximum = with_files(&base, [("maximum.rs".into(), vec![b' '; MAX_SOURCE_BYTES])])?;
    let maximum = execute_case(
        &gateway,
        "exact-source-size-limit",
        &maximum,
        &vendor,
        120_000,
    )?;
    assert_eq!(maximum.report.coverage.files_too_large, 0);
    assert_eq!(
        maximum.report.coverage.files_parsed,
        maximum.report.coverage.files_selected
    );
    cases.push(maximum);
    assert!(SourceFile::new("too-large.rs".into(), vec![b' '; MAX_SOURCE_BYTES + 1]).is_err());

    let crashed = with_files(
        &base,
        [
            ("00-before.rs".into(), b"unsafe fn before() {}\n".to_vec()),
            (
                "01-crash.rs".into(),
                repeated_source(b"fn crash() { let _ = ", b"!", 250_000, b"true; }\n"),
            ),
            ("02-after.rs".into(), b"unsafe fn after() {}\n".to_vec()),
        ],
    )?;
    let crashed = execute_case(&gateway, "worker-crash", &crashed, &vendor, 120_000)?;
    assert_eq!(crashed.report.coverage.files_crashed, 1);
    assert!(
        crashed
            .report
            .findings
            .iter()
            .any(|finding| finding.path == "00-before.rs")
    );
    assert!(
        crashed
            .report
            .findings
            .iter()
            .any(|finding| finding.path == "02-after.rs")
    );
    cases.push(crashed);

    let budget_source = with_files(
        &base,
        (0..4_080).map(|index| (format!("f{index:04}.rs"), Vec::new())),
    )?;
    let budget = execute_case(
        &gateway,
        "global-budget-partial",
        &budget_source,
        &vendor,
        35_000,
    )?;
    assert!(budget.report.coverage.files_parsed > 0);
    assert!(budget.report.coverage.files_budget_exhausted > 0);
    assert!(!budget.report.syntax_complete);
    cases.push(budget);

    inventory_is_empty(&gateway)?;
    let evidence = serde_json::to_vec_pretty(&serde_json::json!({
        "schema": "rust-engineering-mcp.m4-scanner-native.v1",
        "image_id": M4_SCANNER_IMAGE,
        "status": "passed",
        "timeout_disposition": "Per-file timeout and continuation are qualified by the helper subprocess IPC tests. The native corpus does not reliably exceed the fixed 2s per-file limit; global deadline exhaustion is exercised separately without claiming a per-file timeout.",
        "too_large_disposition": "SourceFile rejects 1 MiB + 1 before gateway admission; helper too_large remains a defense validated by its unit test",
        "cases": cases,
        "cleanup": {"all_absent": true},
    }))?;
    let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/m4-scanner-native.json");
    std::fs::write(&output, &evidence)?;
    println!("M4_SCANNER_NATIVE {}", digest(&evidence));
    drop(gateway);
    std::fs::remove_dir_all(state_root)?;
    Ok(())
}
