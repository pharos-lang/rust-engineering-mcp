#![cfg(target_os = "macos")]
use super::*;
use rust_engineering_application::{
    ExecutionCancellation, OperationControl, ReferenceGenerator, catalog_context,
};
use rust_engineering_project::catalog_store::CatalogStore;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
};
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn fingerprint(raw: &str) -> Result<CatalogFingerprint, ContractError> {
    format!("sha256:{raw}").parse()
}

struct Control;
impl OperationControl for Control {
    fn check(&self) -> Result<(), ProjectError> {
        Ok(())
    }
}
impl ExecutionCancellation for Control {
    fn is_cancelled(&self) -> bool {
        false
    }
}
struct Time;
impl Clock for Time {
    fn now(&self) -> UnixSeconds {
        UnixSeconds(700_010)
    }
}
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let id = rust_engineering_project::OsReferences
            .generate()
            .map_err(|e| format!("{e:?}"))?;
        let root = PathBuf::from("/private/tmp").join(format!("catalog-provider-{id}"));
        fs::create_dir(&root)?;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
        let value = Self(root);
        value.write(
            "trust.json",
            &fs::read(fixtures().join("fixture-trust.json"))?,
        )?;
        Ok(value)
    }
    fn write(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
        fs::write(self.0.join(name), bytes)?;
        fs::set_permissions(self.0.join(name), fs::Permissions::from_mode(0o600))
    }
    fn trust(&self) -> Result<PublisherTrust, Box<dyn std::error::Error>> {
        Ok(PublisherTrust::parse(&fs::read(
            self.0.join("trust.json"),
        )?)?)
    }
    fn install(&self, sequence: u64) -> Result<VerifiedBundle, Box<dyn std::error::Error>> {
        let bytes = fs::read(fixtures().join(format!("fixture-{sequence}.tar.zst")))?;
        let verified = bundle::verify(&bytes, &self.trust()?)?;
        self.write("floor.record", &SequenceFloor::new(&verified).bytes()?)?;
        self.write("active.bundle", &bytes)?;
        Ok(verified)
    }
    fn config(&self) -> HostCatalogConfig {
        HostCatalogConfig {
            store: self.0.clone(),
            trust: self.0.join("trust.json"),
            model_dir: None,
            index_store: None,
        }
    }
    fn provider(&self) -> CatalogProvider {
        CatalogProvider::new(Some(self.config()), None)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
use std::io;
fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/catalog")
}
fn observe(
    provider: &CatalogProvider,
) -> Result<CatalogContextObservation, Box<dyn std::error::Error>> {
    provider
        .observe(&Control)
        .map_err(|e| format!("{e:?}").into())
}
fn available<T>(component: Component<T>) -> Result<T, &'static str> {
    match component {
        Component::Available { value } => Ok(value),
        _ => Err("expected available"),
    }
}

#[test]
fn unconfigured_status_has_no_implicit_catalog_or_audit_source() -> TestResult {
    let result = observe(&CatalogProvider::new(None, None))?;
    assert_eq!(
        result.catalog,
        unavailable(CatalogComponentUnavailable::NotConfigured)
    );
    assert_eq!(
        result.model,
        unavailable(CatalogComponentUnavailable::NotConfigured)
    );
    assert_eq!(
        result.semantic_index,
        unavailable(CatalogComponentUnavailable::NotConfigured)
    );
    assert_eq!(
        result.rustsec,
        unavailable(CatalogComponentUnavailable::NotConfigured)
    );
    assert_eq!(result.reservation, None);
    Ok(())
}

#[test]
fn authenticates_catalog_identity_counts_and_floor() -> TestResult {
    let fixture = Fixture::new()?;
    let expected = fixture.install(1)?;
    let result = observe(&fixture.provider())?;
    let catalog = available(result.catalog)?;
    assert_eq!(catalog.publisher, "fixture-only");
    assert_eq!(catalog.channel, "test");
    assert_eq!(catalog.metadata.sequence, 1);
    assert_eq!(catalog.crate_count, 1);
    assert_eq!(catalog.schema_version, 1);
    assert_eq!(
        catalog.metadata.fingerprint,
        expected.repository().metadata().fingerprint
    );
    assert_eq!(
        catalog.bundle_fingerprint,
        fingerprint(expected.fingerprint()).map_err(|e| format!("{e:?}"))?
    );
    assert_eq!(
        catalog.publisher_key_fingerprint,
        fingerprint(&fixture.trust()?.key_fingerprint()?).map_err(|e| format!("{e:?}"))?
    );
    assert!(!catalog.bundled_rustsec_available);
    let reservation = result.reservation.ok_or("missing reservation")?;
    assert_eq!(reservation.sequence, 1);
    assert_eq!(reservation.bundle_fingerprint, catalog.bundle_fingerprint);
    Ok(())
}

#[test]
fn missing_active_keeps_authenticated_reservation_visible_and_pending() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.install(2)?;
    fs::remove_file(fixture.0.join("active.bundle"))?;
    let provider = fixture.provider();
    let result = observe(&provider)?;
    assert_eq!(
        result.catalog,
        unavailable(CatalogComponentUnavailable::Missing)
    );
    assert_eq!(result.reservation.ok_or("missing floor")?.sequence, 2);
    let assessed = catalog_context(&provider, &Time, &Control).map_err(|e| format!("{e:?}"))?;
    assert!(assessed.reservation.ok_or("missing floor")?.pending);
    Ok(())
}

#[test]
fn reserved_newer_floor_preserves_older_active_as_explicit_pending_state() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.install(2)?;
    fixture.write(
        "active.bundle",
        &fs::read(fixtures().join("fixture-1.tar.zst"))?,
    )?;
    let status =
        catalog_context(&fixture.provider(), &Time, &Control).map_err(|e| format!("{e:?}"))?;
    assert_eq!(available(status.catalog)?.sequence, 1);
    let reservation = status.reservation.ok_or("floor")?;
    assert_eq!(reservation.reservation.sequence, 2);
    assert!(reservation.pending);
    Ok(())
}

#[test]
fn missing_or_corrupt_floor_never_authenticates_existing_active() -> TestResult {
    for corrupt in [false, true] {
        let fixture = Fixture::new()?;
        fixture.install(1)?;
        if corrupt {
            fixture.write("floor.record", b"corrupt")?;
        } else {
            fs::remove_file(fixture.0.join("floor.record"))?;
        }
        let result = observe(&fixture.provider())?;
        assert_eq!(
            result.catalog,
            unavailable(CatalogComponentUnavailable::Invalid)
        );
        assert_eq!(result.reservation, None);
    }
    Ok(())
}

#[test]
fn wrong_trust_key_never_authenticates_catalog_bytes() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.install(1)?;
    let mut trust = fixture.trust()?;
    trust.public_key = "00".repeat(32);
    fixture.write("trust.json", &serde_json::to_vec(&trust)?)?;
    assert_eq!(
        observe(&fixture.provider())?.catalog,
        unavailable(CatalogComponentUnavailable::Invalid)
    );
    Ok(())
}

#[test]
fn symlink_and_hardlink_active_are_denied() -> TestResult {
    for hardlink in [false, true] {
        let fixture = Fixture::new()?;
        fixture.install(1)?;
        let active = fixture.0.join("active.bundle");
        let original = fixture.0.join("original.bundle");
        fs::rename(&active, &original)?;
        if hardlink {
            fs::hard_link(&original, &active)?;
        } else {
            symlink(&original, &active)?;
        }
        assert_eq!(
            observe(&fixture.provider())?.catalog,
            unavailable(CatalogComponentUnavailable::Denied)
        );
    }
    Ok(())
}

#[test]
fn runtime_read_ignores_admin_lease_and_preserves_staging_sentinel() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.install(1)?;
    let lease = CatalogStore::open(&fixture.0)?;
    let sentinel = b"must not clean staging during runtime status";
    fixture.write("staging.bundle", sentinel)?;
    assert_eq!(
        available(observe(&fixture.provider())?.catalog)?
            .metadata
            .sequence,
        1
    );
    assert_eq!(fs::read(fixture.0.join("staging.bundle"))?, sentinel);
    drop(lease);
    Ok(())
}

#[test]
fn catalog_session_keeps_initial_generation_and_unavailable_result() -> TestResult {
    let fixture = Fixture::new()?;
    let missing = fixture.provider();
    assert_eq!(
        observe(&missing)?.catalog,
        unavailable(CatalogComponentUnavailable::Missing)
    );
    fixture.install(1)?;
    assert_eq!(
        observe(&missing)?.catalog,
        unavailable(CatalogComponentUnavailable::Missing)
    );
    let first = fixture.provider();
    assert_eq!(available(observe(&first)?.catalog)?.metadata.sequence, 1);
    fixture.install(2)?;
    assert_eq!(available(observe(&first)?.catalog)?.metadata.sequence, 1);
    assert_eq!(observe(&first)?.reservation.ok_or("floor")?.sequence, 1);
    assert_eq!(
        available(observe(&fixture.provider())?.catalog)?
            .metadata
            .sequence,
        2
    );
    Ok(())
}

#[test]
fn configured_rustsec_is_independent_and_reread_each_call() -> TestResult {
    let fixture = Fixture::new()?;
    let bytes = serde_json::to_vec(&rust_engineering_catalog::RustSecSnapshotDocument {
        format_version: 1,
        sequence: 7,
        source_id: "fixture-independent-rustsec".into(),
        created_at: Some(700_000),
        observed_at: Some(700_000),
        records: vec![rust_engineering_catalog::RustSecSnapshotRecord {
            path: "crates/rsa/RUSTSEC-2023-0071.md".into(),
            markdown: include_str!(
                "../../../../../catalog-adapter/tests/fixtures/rustsec/RUSTSEC-2023-0071.md"
            )
            .into(),
        }],
    })?;
    fixture.write("rustsec.json", &bytes)?;
    let audit = HostAuditConfig {
        path: fixture.0.join("rustsec.json"),
        fingerprint: fingerprint(&bundle::sha256(&bytes)).map_err(|e| format!("{e:?}"))?,
    };
    let provider = CatalogProvider::new(None, Some(audit));
    let result = observe(&provider)?;
    assert_eq!(
        result.catalog,
        unavailable(CatalogComponentUnavailable::NotConfigured)
    );
    let rustsec = available(result.rustsec)?;
    assert_eq!(rustsec.sequence, 7);
    assert_eq!(rustsec.record_count, 1);
    let assessed = catalog_context(&provider, &Time, &Control).map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        available(assessed.rustsec)?.evidence.freshness().state(),
        FreshnessState::Fresh
    );
    fixture.write("rustsec.json", b"changed after initial observation")?;
    assert_eq!(
        observe(&provider)?.rustsec,
        unavailable(CatalogComponentUnavailable::IdentityMismatch)
    );
    fixture.write("rustsec.json", &bytes)?;
    assert_eq!(available(observe(&provider)?.rustsec)?.sequence, 7);
    Ok(())
}

#[test]
fn rustsec_filesystem_failures_preserve_catalog_and_report_specific_reason() -> TestResult {
    for reason in [
        CatalogComponentUnavailable::Missing,
        CatalogComponentUnavailable::Denied,
        CatalogComponentUnavailable::Budget,
    ] {
        let fixture = Fixture::new()?;
        fixture.install(1)?;
        let path = fixture.0.join("rustsec.json");
        match reason {
            CatalogComponentUnavailable::Missing => {}
            CatalogComponentUnavailable::Denied => {
                fixture.write("real-rustsec.json", b"the no-follow check precedes parsing")?;
                symlink(fixture.0.join("real-rustsec.json"), &path)?;
            }
            CatalogComponentUnavailable::Budget => {
                fixture.write("rustsec.json", b"")?;
                fs::OpenOptions::new()
                    .write(true)
                    .open(&path)?
                    .set_len(rust_engineering_project::MAX_HOST_SNAPSHOT_BYTES as u64 + 1)?;
            }
            _ => return Err("unexpected test case".into()),
        }
        let audit = HostAuditConfig {
            path,
            fingerprint: fingerprint(&bundle::sha256(b"unused expected bytes"))?,
        };
        let provider = CatalogProvider::new(Some(fixture.config()), Some(audit));
        let observed = observe(&provider)?;
        assert_eq!(available(observed.catalog)?.metadata.sequence, 1);
        assert_eq!(observed.rustsec, unavailable(reason));
    }
    Ok(())
}

#[cfg(not(feature = "local"))]
#[test]
fn configured_semantic_paths_without_local_feature_do_not_attempt_loading() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.install(1)?;
    let mut config = fixture.config();
    config.model_dir = Some(fixture.0.join("missing-model"));
    config.index_store = Some(fixture.0.join("missing-index"));
    let observed = observe(&CatalogProvider::new(Some(config), None))?;
    assert_eq!(available(observed.catalog)?.metadata.sequence, 1);
    assert_eq!(
        observed.model,
        unavailable(CatalogComponentUnavailable::FeatureDisabled)
    );
    assert_eq!(
        observed.semantic_index,
        unavailable(CatalogComponentUnavailable::FeatureDisabled)
    );
    assert!(!fixture.0.join("missing-model").exists());
    assert!(!fixture.0.join("missing-index").exists());
    Ok(())
}
