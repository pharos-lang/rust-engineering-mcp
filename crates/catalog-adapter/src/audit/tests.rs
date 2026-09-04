use super::*;
use rust_engineering_application::{ExecutionCancellation, OperationControl, ProjectError};
use std::sync::atomic::{AtomicUsize, Ordering};
type Result<T = ()> = std::result::Result<T, String>;
fn checked<T, E: std::fmt::Debug>(v: std::result::Result<T, E>) -> Result<T> {
    v.map_err(|e| format!("{e:?}"))
}
struct Control;
impl ExecutionCancellation for Control {
    fn is_cancelled(&self) -> bool {
        false
    }
}
impl OperationControl for Control {
    fn check(&self) -> std::result::Result<(), ProjectError> {
        Ok(())
    }
}
struct TestClock(u64);
impl Clock for TestClock {
    fn now(&self) -> UnixSeconds {
        UnixSeconds(self.0)
    }
}
const NOW: u64 = 1_788_000_000;
const RSA: &str = include_str!("../../tests/fixtures/rustsec/RUSTSEC-2023-0071.md");
fn document(markdown: &str) -> RustSecSnapshotDocument {
    RustSecSnapshotDocument {
        format_version: 1,
        sequence: 1,
        source_id: "fixture-rustsec-rsa".into(),
        created_at: Some(NOW - 10),
        observed_at: Some(NOW),
        records: vec![RustSecSnapshotRecord {
            path: "crates/rsa/RUSTSEC-2023-0071.md".into(),
            markdown: markdown.into(),
        }],
    }
}
fn encoded(document: &RustSecSnapshotDocument) -> Result<(Vec<u8>, CatalogFingerprint)> {
    let bytes = checked(serde_json::to_vec(document))?;
    let fp = checked(super::super::fingerprint(&bytes))?;
    Ok((bytes, fp))
}
fn snapshot(document: &RustSecSnapshotDocument) -> Result<RustSecSnapshot> {
    let (bytes, fp) = encoded(document)?;
    checked(RustSecSnapshot::from_bytes(&bytes, &fp, &Control))
}
#[test]
fn status_metadata_preserves_verified_snapshot_identity() -> Result {
    let document = document(RSA);
    let (_, expected) = encoded(&document)?;
    let snapshot = snapshot(&document)?;
    let metadata = snapshot.catalog_metadata();
    assert_eq!(metadata.sequence, 1);
    assert_eq!(metadata.fingerprint, expected);
    assert_eq!(metadata.provenance, snapshot.provenance);
    assert_eq!(snapshot.record_count(), 1);
    Ok(())
}
fn project(version: &str) -> Result<(SourceBundle, ProjectStructure)> {
    let lock = format!(
        "version = 4\n[[package]]\nname = \"app\"\nversion = \"0.1.0\"\ndependencies = [\"rsa\"]\n[[package]]\nname = \"rsa\"\nversion = \"{version}\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n"
    );
    let source = checked(SourceBundle::new(vec![
        checked(SourceFile::new("Cargo.lock".into(), lock.into_bytes()))?,
        checked(SourceFile::new(
            "Cargo.toml".into(),
            b"[package]\nname=\"app\"\nversion=\"0.1.0\"\nedition=\"2024\"\n".to_vec(),
        ))?,
    ]))?;
    let fp = format!("sha256:{:064x}", 1);
    let structure = ProjectStructure {
        workspace_members: vec![0],
        workspace_default_members: vec![0],
        packages: vec![ProjectPackage {
            package_index: 0,
            name: "app".into(),
            version: "0.1.0".into(),
            manifest_path: "Cargo.toml".into(),
            edition: RustEdition::E2024,
            rust_version: None,
            targets: vec![],
            features: vec![],
            direct_dependencies: vec![],
        }],
        profiles: vec![],
        cargo_configuration: CargoConfiguration {
            project_config_policy: ProjectConfigPolicy::Rejected,
            frozen: true,
            offline: true,
            incremental: false,
            target_directory_ephemeral: true,
        },
        runtime: RuntimeIdentity {
            platform: "linux/aarch64".into(),
            image_id: fp.clone(),
            configuration_fingerprint: checked(fp.parse())?,
            execution_fingerprint: checked(fp.parse())?,
            rust_version: "1.98.1".into(),
            cargo_version: "1.98.1".into(),
            declared_toolchain: None,
        },
        source_fingerprint: checked(fp.parse())?,
    };
    Ok((source, structure))
}
#[test]
fn real_advisory_without_collection_matches_and_sqlite_is_authoritative() -> Result {
    let db = snapshot(&document(RSA))?;
    let (source, structure) = project("0.9.6")?;
    let result = checked(db.audit(&source, &structure, &TestClock(NOW), &Control))?;
    assert_eq!(result.state, AuditState::Failed);
    assert!(result.validation_complete);
    assert_eq!(result.findings.len(), 1);
    let f = &result.findings[0];
    assert_eq!(f.advisory_id, "RUSTSEC-2023-0071");
    assert_eq!(f.package.name, "rsa");
    assert_eq!(f.package.version, "0.9.6");
    assert_eq!(f.severity, Some(AuditSeverity::Medium));
    assert!(f.patched_requirements.is_empty());
    assert_eq!(
        f.paths[0]
            .packages
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["app", "rsa"]
    );
    assert_eq!(result.snapshot_record_count, Some(1));
    assert_eq!(result.snapshot_sequence, Some(1));
    assert_eq!(result.crates_io_scanned, 1);
    assert_eq!(result.workspace_packages_excluded, 1);
    assert!(
        db.connection
            .execute("DELETE FROM rustsec_advisories", [])
            .is_err()
    );
    // This private test removes the authoritative SQLite record; no parallel Vec is consulted.
    checked(db.connection.pragma_update(None, "query_only", false))?;
    checked(db.connection.execute("DELETE FROM rustsec_advisories", []))?;
    checked(db.connection.pragma_update(None, "query_only", true))?;
    assert!(
        checked(db.audit(&source, &structure, &TestClock(NOW), &Control))?
            .findings
            .is_empty()
    );
    Ok(())
}
#[test]
fn official_matching_distinguishes_patched_unaffected_withdrawn_and_information() -> Result {
    let (source, structure) = project("0.9.6")?;
    for md in [
        RSA.replace("patched = []", "patched = [\">=0.9.6\"]"),
        RSA.replace("patched = []", "patched = []\nunaffected = [\">=0.9.6\"]"),
        RSA.replace(
            "package = \"rsa\"",
            "package = \"rsa\"\nwithdrawn = \"2024-01-01\"",
        ),
    ] {
        let result = checked(snapshot(&document(&md))?.audit(
            &source,
            &structure,
            &TestClock(NOW),
            &Control,
        ))?;
        assert_eq!(result.state, AuditState::Passed);
        assert!(result.findings.is_empty());
        assert!(result.informational.is_empty());
    }
    let md = RSA.replace(
        "package = \"rsa\"",
        "package = \"rsa\"\ninformational = \"unmaintained\"",
    );
    let result =
        checked(snapshot(&document(&md))?.audit(&source, &structure, &TestClock(NOW), &Control))?;
    assert_eq!(result.state, AuditState::Passed);
    assert!(result.findings.is_empty());
    assert_eq!(result.informational.len(), 1);
    assert_eq!(
        result.informational[0].informational.as_deref(),
        Some("unmaintained")
    );
    Ok(())
}
#[test]
fn integrity_identity_duplicates_collection_and_transport_schema_fail_closed() -> Result {
    let good = document(RSA);
    let (bytes, fp) = encoded(&good)?;
    let mut tampered = bytes.clone();
    tampered.push(b' ');
    assert!(matches!(
        RustSecSnapshot::from_bytes(&tampered, &fp, &Control),
        Err(AuditDataError::Integrity)
    ));
    for md in [
        RSA.replace("RUSTSEC-2023-0071", "RUSTSEC-0000-0000"),
        RSA.replace("package = \"rsa\"", "package = \"other\""),
        RSA.replace(
            "package = \"rsa\"",
            "package = \"rsa\"\ncollection = \"rust\"",
        ),
    ] {
        assert!(snapshot(&document(&md)).is_err());
    }
    let mut duplicate = document(RSA);
    duplicate.records.push(RustSecSnapshotRecord {
        path: duplicate.records[0].path.clone(),
        markdown: RSA.into(),
    });
    assert!(snapshot(&duplicate).is_err());
    let mut path = document(RSA);
    path.records[0].path = "crates/rsa/../RUSTSEC-2023-0071.md".into();
    assert!(snapshot(&path).is_err());
    let bad = bytes.strip_suffix(b"}").ok_or("json object")?;
    let bad = [bad, b",\"extra\":true}"].concat();
    let fp = checked(super::super::fingerprint(&bad))?;
    assert!(matches!(
        RustSecSnapshot::from_bytes(&bad, &fp, &Control),
        Err(AuditDataError::InvalidSnapshot)
    ));
    Ok(())
}
#[test]
fn freshness_is_reassessed_without_refresh_or_copy_age_reset() -> Result {
    let md = RSA.replace("patched = []", "patched = [\">=0.9.6\"]");
    let (source, structure) = project("0.9.6")?;
    let db = snapshot(&document(&md))?;
    assert_eq!(
        checked(db.audit(&source, &structure, &TestClock(NOW), &Control))?.state,
        AuditState::Passed
    );
    for now in [NOW + 86_391, NOW + 604_801] {
        let result = checked(db.audit(&source, &structure, &TestClock(now), &Control))?;
        assert_eq!(result.state, AuditState::Unavailable);
        assert!(!result.validation_complete);
        assert_eq!(result.issue, Some(AuditIssue::SnapshotStale));
        assert_eq!(
            result
                .snapshot
                .as_ref()
                .and_then(|s| s.provenance().created_at()),
            Some(UnixSeconds(NOW - 10))
        );
    }
    let historical = checked(snapshot(&document(RSA))?.audit(
        &source,
        &structure,
        &TestClock(NOW + 604_801),
        &Control,
    ))?;
    assert_eq!(historical.state, AuditState::Unavailable);
    assert_eq!(historical.findings.len(), 1);
    for (created, observed) in [
        (None, Some(NOW)),
        (Some(NOW + 1), Some(NOW + 2)),
        (Some(NOW - 1), None),
        (Some(NOW - 1), Some(NOW + 1)),
    ] {
        let mut d = document(&md);
        d.created_at = created;
        d.observed_at = observed;
        let result = checked(snapshot(&d)?.audit(&source, &structure, &TestClock(NOW), &Control))?;
        assert_eq!(result.state, AuditState::Unavailable);
        assert_eq!(result.issue, Some(AuditIssue::SnapshotUnknownAge));
    }
    Ok(())
}
#[test]
fn cancellation_and_snapshot_size_budgets_are_enforced() -> Result {
    struct Cancel(AtomicUsize);
    impl ExecutionCancellation for Cancel {
        fn is_cancelled(&self) -> bool {
            self.0.load(Ordering::Relaxed) == 0
        }
    }
    impl OperationControl for Cancel {
        fn check(&self) -> std::result::Result<(), ProjectError> {
            let n = self
                .0
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
                    Some(v.saturating_sub(1))
                })
                .unwrap_or(0);
            if n == 0 {
                Err(ProjectError::Cancelled)
            } else {
                Ok(())
            }
        }
    }
    let (bytes, fp) = encoded(&document(RSA))?;
    for n in [0, 1] {
        assert!(matches!(
            RustSecSnapshot::from_bytes(&bytes, &fp, &Cancel(AtomicUsize::new(n))),
            Err(AuditDataError::Cancelled)
        ));
    }
    let oversized = vec![b' '; MAX_BYTES + 1];
    assert!(matches!(
        RustSecSnapshot::from_bytes(&oversized, &fp, &Control),
        Err(AuditDataError::Budget)
    ));
    let db = snapshot(&document(RSA))?;
    let (source, structure) = project("0.9.6")?;
    assert!(matches!(
        db.audit(
            &source,
            &structure,
            &TestClock(NOW),
            &Cancel(AtomicUsize::new(0))
        ),
        Err(AuditDataError::Cancelled)
    ));
    Ok(())
}

#[test]
fn sources_never_turn_path_or_alternate_registry_into_crates_io() -> Result {
    let (source, structure) = project("0.9.6")?;
    let md=RSA.replace("package = \"rsa\"","package = \"rsa\"\nsource = \"git+https://example.invalid/rsa#0123456789abcdef0123456789abcdef01234567\"");
    assert!(snapshot(&document(&md)).is_err());
    let files = source
        .files()
        .iter()
        .map(|file| {
            let bytes = if file.path() == "Cargo.lock" {
                String::from_utf8(file.bytes().to_vec())
                    .map_err(|e| e.to_string())?
                    .replace(
                        "source = \"registry+https://github.com/rust-lang/crates.io-index\"\n",
                        "",
                    )
                    .into_bytes()
            } else {
                file.bytes().to_vec()
            };
            checked(SourceFile::new(file.path().into(), bytes))
        })
        .collect::<Result<Vec<_>>>()?;
    let source = checked(SourceBundle::new(files))?;
    let result =
        checked(snapshot(&document(RSA))?.audit(&source, &structure, &TestClock(NOW), &Control))?;
    assert_eq!(result.state, AuditState::Incomplete);
    assert!(!result.validation_complete);
    assert_eq!(result.crates_io_scanned, 0);
    assert_eq!(result.unsupported_packages.len(), 1);
    assert!(result.findings.is_empty());
    Ok(())
}
#[test]
fn aggregate_findings_budget_preserves_honest_omissions() -> Result {
    let mut doc = document(RSA);
    doc.records.clear();
    for n in 0..129 {
        let name = if n < 128 { "rsa" } else { "other" };
        let id = format!("RUSTSEC-2023-{:04}", 1000 + n);
        doc.records.push(RustSecSnapshotRecord {
            path: format!("crates/{name}/{id}.md"),
            markdown: RSA
                .replace("RUSTSEC-2023-0071", &id)
                .replace("package = \"rsa\"", &format!("package = \"{name}\"")),
        });
    }
    let (source, structure) = project("0.9.6")?;
    let files=source.files().iter().map(|file| {
        let bytes=if file.path()=="Cargo.lock" {let mut text=String::from_utf8(file.bytes().to_vec()).map_err(|e|e.to_string())?.replace("dependencies = [\"rsa\"]","dependencies = [\"rsa\",\"other\"]");text.push_str("\n[[package]]\nname=\"other\"\nversion=\"0.9.6\"\nsource=\"registry+https://github.com/rust-lang/crates.io-index\"\n");text.into_bytes()}else{file.bytes().to_vec()};checked(SourceFile::new(file.path().into(),bytes))
    }).collect::<Result<Vec<_>>>()?;
    let result = checked(snapshot(&doc)?.audit(
        &checked(SourceBundle::new(files))?,
        &structure,
        &TestClock(NOW),
        &Control,
    ))?;
    assert_eq!(result.state, AuditState::Failed);
    assert!(!result.validation_complete);
    assert_eq!(result.findings.len(), 128);
    assert_eq!(result.findings_omitted, 1);
    assert_eq!(result.issue, Some(AuditIssue::OutputBudget));
    assert!(checked(serde_json::to_vec(&result))?.len() <= MAX_PAYLOAD);
    doc.records[128].path = "crates/rsa/RUSTSEC-2023-1128.md".into();
    doc.records[128].markdown = doc.records[128]
        .markdown
        .replace("package = \"other\"", "package = \"rsa\"");
    let (bytes, fp) = encoded(&doc)?;
    assert!(matches!(
        RustSecSnapshot::from_bytes(&bytes, &fp, &Control),
        Err(AuditDataError::Budget)
    ));
    Ok(())
}

#[test]
#[ignore = "explicit macOS network-denied process; scripts/test-audit-data.py"]
fn owned_rustsec_sqlite_audit_runs_with_actual_network_deny() -> Result {
    assert_eq!(std::env::var("RUST_MCP_NETWORK_DENIED").as_deref(), Ok("1"));
    for address in ["127.0.0.1:0", "[::1]:0"] {
        let tcp = std::net::TcpListener::bind(address);
        assert!(matches!(tcp,Err(ref e) if e.kind()==std::io::ErrorKind::PermissionDenied));
        let udp = std::net::UdpSocket::bind(address);
        assert!(matches!(udp,Err(ref e) if e.kind()==std::io::ErrorKind::PermissionDenied));
    }
    let (source, structure) = project("0.9.6")?;
    let observation =
        checked(snapshot(&document(RSA))?.audit(&source, &structure, &TestClock(NOW), &Control))?;
    assert_eq!(observation.state, AuditState::Failed);
    assert!(observation.validation_complete);
    assert_eq!(observation.findings[0].advisory_id, "RUSTSEC-2023-0071");
    println!(
        "M1_AUDIT_DATA_RECEIPT {}",
        checked(serde_json::to_string(
            &serde_json::json!({"scope":"owned-rustsec-sqlite-data","network_deny_scope":"macos_test_process_only","tcp_ipv4_ipv6_denied":true,"udp_ipv4_ipv6_denied":true,"actual_rustsec_sqlite_finding":true,"snapshot_fingerprint":observation.snapshot_fingerprint,"lock_fingerprint":observation.lock_fingerprint})
        ))?
    );
    Ok(())
}

#[test]
fn empty_snapshots_and_unsafe_display_metadata_are_rejected() -> Result {
    let mut empty = document(RSA);
    empty.records.clear();
    let (bytes, fp) = encoded(&empty)?;
    assert!(matches!(
        RustSecSnapshot::from_bytes(&bytes, &fp, &Control),
        Err(AuditDataError::InvalidSnapshot)
    ));
    for label in [
        "x".repeat(129),
        "evil\u{202e}label".into(),
        "evil\u{2066}label".into(),
    ] {
        let md = RSA.replace(
            "package = \"rsa\"",
            &format!("package = \"rsa\"\ninformational = \"{label}\""),
        );
        assert!(snapshot(&document(&md)).is_err());
    }
    let md = RSA.replace("# ", "# \u{202e}");
    assert!(snapshot(&document(&md)).is_err());
    Ok(())
}

#[test]
fn explicit_canonical_advisory_source_matches_but_registry_fragment_does_not() -> Result {
    let (source, structure) = project("0.9.6")?;
    for (origin, expected) in [
        ("registry+https://github.com/rust-lang/crates.io-index", 1),
        (
            "registry+https://github.com/rust-lang/crates.io-index#other",
            0,
        ),
    ] {
        let md = RSA.replace(
            "package = \"rsa\"",
            &format!("package = \"rsa\"\nsource = \"{origin}\""),
        );
        if expected == 0 {
            assert!(snapshot(&document(&md)).is_err());
            continue;
        }
        let parsed: Advisory = checked(md.parse())?;
        assert_eq!(
            parsed.metadata.source.as_ref().and_then(|s| s.precise()),
            Some("locked")
        );
        let result = checked(snapshot(&document(&md))?.audit(
            &source,
            &structure,
            &TestClock(NOW),
            &Control,
        ))?;
        assert_eq!(result.findings.len(), expected);
    }
    Ok(())
}

#[test]
fn data_deadline_is_not_reported_as_cancellation() -> Result {
    struct Deadline;
    impl ExecutionCancellation for Deadline {
        fn is_cancelled(&self) -> bool {
            false
        }
    }
    impl OperationControl for Deadline {
        fn check(&self) -> std::result::Result<(), ProjectError> {
            Err(ProjectError::Rejected(OperationalErrorCode::CommandTimeout))
        }
    }
    let (bytes, fp) = encoded(&document(RSA))?;
    assert!(matches!(
        RustSecSnapshot::from_bytes(&bytes, &fp, &Deadline),
        Err(AuditDataError::Timeout)
    ));
    Ok(())
}
