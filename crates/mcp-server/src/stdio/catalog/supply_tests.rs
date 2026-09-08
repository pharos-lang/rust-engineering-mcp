#![cfg(target_os = "macos")]

use super::*;
use ring::signature::Ed25519KeyPair;
use rust_engineering_application::{
    ExecutionCancellation, OperationControl, ReferenceGenerator, supply_chain::SupplyCatalogPort,
};
use rust_engineering_catalog::{
    SqliteCatalogRepository,
    bundle::{BundleFile, BundleManifest},
};
use rust_engineering_domain::supply_chain::{
    SupplyAvailability, SupplyPackage, SupplySource, YankedFact,
};
use rust_engineering_project::OsReferences;
use std::{
    fs, io,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

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

struct Time(u64);
impl Clock for Time {
    fn now(&self) -> UnixSeconds {
        UnixSeconds(self.0)
    }
}

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> TestResult<Self> {
        let id = OsReferences
            .generate()
            .map_err(|error| format!("{error:?}"))?;
        let root = PathBuf::from("/private/tmp").join(format!("catalog-supply-catalog-{id}"));
        fs::create_dir(&root)?;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
        let fixture = Self(root);
        fixture.write(
            "trust.json",
            &fs::read(fixtures().join("fixture-trust.json"))?,
        )?;
        Ok(fixture)
    }

    fn write(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
        fs::write(self.0.join(name), bytes)?;
        fs::set_permissions(self.0.join(name), fs::Permissions::from_mode(0o600))
    }

    fn trust(&self) -> TestResult<PublisherTrust> {
        Ok(PublisherTrust::parse(&fs::read(
            self.0.join("trust.json"),
        )?)?)
    }

    fn install(&self, bytes: &[u8]) -> TestResult<VerifiedBundle> {
        let verified = bundle::verify(bytes, &self.trust()?)?;
        self.write("floor.record", &SequenceFloor::new(&verified).bytes()?)?;
        self.write("active.bundle", bytes)?;
        Ok(verified)
    }

    fn install_checked(&self, sequence: u64) -> TestResult<VerifiedBundle> {
        self.install(&fs::read(
            fixtures().join(format!("fixture-{sequence}.tar.zst")),
        )?)
    }

    fn install_active_with_floor(&self, active: &[u8], floor_source: &[u8]) -> TestResult {
        let floor = bundle::verify(floor_source, &self.trust()?)?;
        self.write("floor.record", &SequenceFloor::new(&floor).bytes()?)?;
        self.write("active.bundle", active)?;
        Ok(())
    }

    fn provider(&self) -> CatalogProvider {
        CatalogProvider::new(
            Some(HostCatalogConfig {
                store: self.0.clone(),
                trust: self.0.join("trust.json"),
                model_dir: None,
                index_store: None,
            }),
            None,
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/catalog")
}

fn package(name: &str, version: &str, source: SupplySource) -> SupplyPackage {
    SupplyPackage {
        name: name.into(),
        version: version.into(),
        source,
        source_fingerprint: None,
        declared_checksum: None,
        checksum_verified: false,
        duplicate_name: false,
        declared_features: None,
        active_features: None,
        yanked: YankedFact::NotConsulted,
    }
}

fn records() -> Vec<CrateRecord> {
    vec![CrateRecord {
        name: "choice".into(),
        description: "Exact-version yanked fixture".into(),
        repository: Some("https://example.invalid/choice".into()),
        updated_at: Some(100),
        versions: vec![
            VersionRecord {
                version: "1.0.0".into(),
                yanked: false,
                rust_version: Some("1.60".into()),
                license: Some("MIT".into()),
                published_at: Some(100),
                features: vec![],
                dependencies: vec![],
                advisories: vec![],
            },
            VersionRecord {
                version: "3.0.0".into(),
                yanked: true,
                rust_version: Some("1.60".into()),
                license: Some("MIT".into()),
                published_at: Some(100),
                features: vec![],
                dependencies: vec![],
                advisories: vec![],
            },
        ],
    }]
}

enum Corruption {
    None,
    Signature,
    PayloadHash,
}

fn signed_bundle(sequence: u64, corruption: Corruption) -> TestResult<Vec<u8>> {
    let provenance = Provenance::new(
        SourceKind::RegistrySnapshot,
        "supply-catalog-seed42".parse()?,
        Some(UnixSeconds(100)),
        Some(UnixSeconds(100)),
        IntegrityStatus::Verified,
        false,
    )?;
    let snapshot = SqliteCatalogRepository::build(sequence, provenance.clone(), &records())?;
    let manifest = BundleManifest {
        snapshot_format_version: 1,
        catalog_schema_version: 1,
        semantic_index_version: None,
        embedding_model_id: None,
        publisher: "fixture-only".into(),
        channel: "test".into(),
        sequence,
        catalog_provenance: provenance,
        files: vec![BundleFile {
            path: "catalog.sqlite".into(),
            byte_length: snapshot.bytes.len() as u64,
            sha256: bundle::sha256(&snapshot.bytes),
        }],
    };
    let manifest = serde_json::to_vec(&manifest)?;
    let mut message = b"rust-engineering-catalog-bundle-v1\0".to_vec();
    message.extend_from_slice(&manifest);
    let key = Ed25519KeyPair::from_seed_unchecked(&[42; 32]).map_err(|_| "fixture key")?;
    let mut signature = key.sign(&message).as_ref().to_vec();
    let mut sqlite = snapshot.bytes;
    match corruption {
        Corruption::None => {}
        Corruption::Signature => signature[0] ^= 1,
        Corruption::PayloadHash => sqlite[0] ^= 1,
    }
    let mut archive = Vec::new();
    for (name, bytes) in [
        ("manifest.json", manifest.as_slice()),
        ("signature.ed25519", signature.as_slice()),
        ("catalog.sqlite", sqlite.as_slice()),
    ] {
        archive.extend_from_slice(&tar_header(name, bytes.len()));
        archive.extend_from_slice(bytes);
        archive.resize(archive.len().next_multiple_of(512), 0);
    }
    archive.resize(archive.len() + 1024, 0);
    Ok(zstd::stream::encode_all(archive.as_slice(), 1)?)
}

fn tar_header(name: &str, size: usize) -> [u8; 512] {
    let mut header = [0; 512];
    header[..name.len()].copy_from_slice(name.as_bytes());
    for (start, end, value) in [
        (100, 108, 0o600usize),
        (108, 116, 0),
        (116, 124, 0),
        (124, 136, size),
        (136, 148, 0),
        (329, 337, 0),
        (337, 345, 0),
    ] {
        header[start..end]
            .copy_from_slice(format!("{:0width$o}\0", value, width = end - start - 1).as_bytes());
    }
    header[156] = b'0';
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    header[148..156].fill(b' ');
    let sum: u64 = header.iter().map(|byte| u64::from(*byte)).sum();
    header[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
    header
}

fn supply(
    provider: &CatalogProvider,
    packages: &mut [SupplyPackage],
    now: u64,
) -> TestResult<rust_engineering_domain::supply_chain::SupplyCatalog> {
    provider
        .supply_catalog(packages, &Time(now), &Control)
        .map_err(|error| format!("{error:?}").into())
}

#[test]
fn exact_versions_distinguish_yanked_not_yanked_version_absent_and_crate_absent() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.install(&signed_bundle(7, Corruption::None)?)?;
    let provider = fixture.provider();
    let mut packages = vec![
        package("choice", "3.0.0", SupplySource::CratesIo),
        package("choice", "1.0.0", SupplySource::CratesIo),
        package("choice", "2.0.0", SupplySource::CratesIo),
        package("missing", "1.0.0", SupplySource::CratesIo),
    ];
    let report = supply(&provider, &mut packages, 100)?;
    assert_eq!(report.availability, SupplyAvailability::Available);
    assert_eq!(report.sequence, Some(7));
    assert_eq!(report.lookups, 4);
    assert_eq!(
        packages
            .iter()
            .map(|value| value.yanked)
            .collect::<Vec<_>>(),
        [
            YankedFact::Yanked,
            YankedFact::NotYanked,
            YankedFact::VersionAbsent,
            YankedFact::CrateAbsent,
        ]
    );
    Ok(())
}

#[test]
fn non_crates_io_sources_are_not_applicable_and_consume_no_lookup() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.install(&signed_bundle(7, Corruption::None)?)?;
    let provider = fixture.provider();
    let mut packages = vec![
        package("choice", "3.0.0", SupplySource::Git),
        package("choice", "3.0.0", SupplySource::Registry),
        package("choice", "1.0.0", SupplySource::CratesIo),
    ];
    let report = supply(&provider, &mut packages, 100)?;
    assert_eq!(report.lookups, 1);
    assert_eq!(packages[0].yanked, YankedFact::NotApplicable);
    assert_eq!(packages[1].yanked, YankedFact::NotApplicable);
    assert_eq!(packages[2].yanked, YankedFact::NotYanked);
    Ok(())
}

#[test]
fn absent_catalog_is_unknown_for_crates_io_without_fabricated_provenance() -> TestResult {
    let provider = CatalogProvider::new(None, None);
    let mut packages = vec![
        package("choice", "1.0.0", SupplySource::CratesIo),
        package("choice", "1.0.0", SupplySource::Git),
    ];
    let report = supply(&provider, &mut packages, 100)?;
    assert_eq!(report.availability, SupplyAvailability::Unavailable);
    assert!(report.snapshot_fingerprint.is_none());
    assert!(report.bundle_fingerprint.is_none());
    assert!(report.sequence.is_none());
    assert!(report.evidence.is_none());
    assert_eq!(report.lookups, 0);
    assert_eq!(packages[0].yanked, YankedFact::CatalogUnavailable);
    assert_eq!(packages[1].yanked, YankedFact::NotConsulted);
    Ok(())
}

#[test]
fn tampered_signature_payload_hash_and_sequence_are_all_unavailable() -> TestResult {
    let valid_one = signed_bundle(1, Corruption::None)?;
    let valid_two = signed_bundle(2, Corruption::None)?;
    let cases = [
        signed_bundle(1, Corruption::Signature)?,
        signed_bundle(1, Corruption::PayloadHash)?,
        valid_two,
    ];
    for active in cases {
        let fixture = Fixture::new()?;
        fixture.install_active_with_floor(&active, &valid_one)?;
        let mut packages = vec![package("choice", "1.0.0", SupplySource::CratesIo)];
        let report = supply(&fixture.provider(), &mut packages, 100)?;
        assert_eq!(report.availability, SupplyAvailability::Unavailable);
        assert_eq!(packages[0].yanked, YankedFact::CatalogUnavailable);
        assert!(report.evidence.is_none());
    }
    Ok(())
}

#[test]
fn request_clock_assesses_the_same_authenticated_generation_as_fresh_then_stale() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.install_checked(1)?;
    let provider = fixture.provider();
    let mut fresh_package = vec![package("serde", "1.0.0", SupplySource::CratesIo)];
    let fresh = supply(&provider, &mut fresh_package, 100)?;
    let mut stale_package = vec![package("serde", "1.0.0", SupplySource::CratesIo)];
    let stale = supply(&provider, &mut stale_package, 700_000)?;
    assert_eq!(fresh.sequence, Some(1));
    assert_eq!(stale.sequence, fresh.sequence);
    assert_eq!(stale.bundle_fingerprint, fresh.bundle_fingerprint);
    assert_eq!(
        fresh.evidence.ok_or("fresh evidence")?.freshness().state(),
        FreshnessState::Fresh
    );
    assert_eq!(
        stale.evidence.ok_or("stale evidence")?.freshness().state(),
        FreshnessState::Stale
    );
    Ok(())
}

#[test]
fn provider_cache_pins_one_verified_generation_across_requests() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.install_checked(1)?;
    let provider = fixture.provider();
    let mut first_package = vec![package("serde", "1.0.0", SupplySource::CratesIo)];
    let first = supply(&provider, &mut first_package, 100)?;
    fixture.install_checked(2)?;
    let mut cached_package = vec![package("serde", "1.0.0", SupplySource::CratesIo)];
    let cached = supply(&provider, &mut cached_package, 100)?;
    let mut new_package = vec![package("serde", "1.0.0", SupplySource::CratesIo)];
    let new_generation = supply(&fixture.provider(), &mut new_package, 100)?;
    assert_eq!(first.sequence, Some(1));
    assert_eq!(cached.sequence, Some(1));
    assert_eq!(cached.bundle_fingerprint, first.bundle_fingerprint);
    assert_eq!(new_generation.sequence, Some(2));
    assert_ne!(new_generation.bundle_fingerprint, first.bundle_fingerprint);
    Ok(())
}
