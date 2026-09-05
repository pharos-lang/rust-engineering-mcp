//! ADR-055 Cargo directory-source validation at the native filesystem boundary.
#![cfg(target_os = "macos")]
use rust_engineering_application::{OperationControl, ProjectError, ReferenceGenerator};
use rust_engineering_domain::{OperationalErrorCode, SourceFingerprint};
use rust_engineering_project::{OsReferences, capture_with_expected, inspect_cargo_vendor};
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

type TestResult<T = ()> = Result<T, String>;
fn ck<T, E: std::fmt::Debug>(result: Result<T, E>) -> TestResult<T> {
    result.map_err(|error| format!("{error:?}"))
}
struct Continue;
impl OperationControl for Continue {
    fn check(&self) -> Result<(), ProjectError> {
        Ok(())
    }
}

struct Fixture {
    base: PathBuf,
    vendor: PathBuf,
}
impl Fixture {
    fn new() -> TestResult<Self> {
        let base = PathBuf::from("/private/tmp")
            .join(format!("rms-vendor-{}", ck(OsReferences.generate())?));
        let vendor = base.join("vendor");
        ck(fs::create_dir(&base))?;
        copy_tree(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures/cargo-vendor-data/vendor"),
            &vendor,
        )?;
        Ok(Self { base, vendor })
    }
    fn checksum(&self) -> PathBuf {
        self.vendor.join("proc-macro2-1.0.107/.cargo-checksum.json")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::set_permissions(&self.vendor, fs::Permissions::from_mode(0o700));
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn copy_tree(from: &Path, to: &Path) -> TestResult {
    ck(fs::create_dir(to))?;
    for entry in ck(fs::read_dir(from))? {
        let entry = ck(entry)?;
        let kind = ck(entry.file_type())?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            ck(fs::copy(entry.path(), target))?;
        } else {
            return Err("fixture contains a link or special entry".into());
        }
    }
    Ok(())
}

fn rewrite_checksum(fixture: &Fixture, edit: impl FnOnce(&mut serde_json::Value)) -> TestResult {
    let path = fixture.checksum();
    let mut value: serde_json::Value = ck(serde_json::from_slice(&ck(fs::read(&path))?))?;
    edit(&mut value);
    ck(fs::write(path, ck(serde_json::to_vec(&value))?))
}

#[test]
fn qualified_fixture_has_exact_digest_packages_and_bytes() -> TestResult {
    let fixture = Fixture::new()?;
    let snapshot = ck(inspect_cargo_vendor(&fixture.vendor, &Continue))?;
    assert_eq!(
        snapshot.tree_fingerprint.to_string(),
        "sha256:743947d5788c1a4385a4b59869c5b8bd0535f7fc0d875b51288f9b26b2d0eba1"
    );
    assert_eq!(
        snapshot
            .packages
            .iter()
            .map(|package| (package.name.as_str(), package.version.as_str()))
            .collect::<Vec<_>>(),
        [
            ("proc-macro2", "1.0.107"),
            ("quote", "1.0.47"),
            ("unicode-ident", "1.0.24"),
        ]
    );
    let expected: SourceFingerprint =
        ck("sha256:743947d5788c1a4385a4b59869c5b8bd0535f7fc0d875b51288f9b26b2d0eba1".parse())?;
    assert_eq!(
        ck(capture_with_expected(&fixture.vendor, &expected, &Continue))?,
        snapshot
    );
    Ok(())
}

#[test]
fn rejects_wrong_expected_digest() -> TestResult {
    let fixture = Fixture::new()?;
    let wrong: SourceFingerprint = ck(format!("sha256:{}", "0".repeat(64)).parse())?;
    assert_eq!(
        capture_with_expected(&fixture.vendor, &wrong, &Continue),
        Err(ProjectError::Rejected(OperationalErrorCode::InvalidProject))
    );
    Ok(())
}

#[test]
fn file_tree_fingerprint_cannot_authorize_an_extra_empty_directory() -> TestResult {
    let fixture = Fixture::new()?;
    let approved = ck(inspect_cargo_vendor(&fixture.vendor, &Continue))?;
    let extra = fixture.vendor.join("quote-1.0.47/unapproved-empty");
    ck(fs::create_dir(&extra))?;
    assert_eq!(
        capture_with_expected(&fixture.vendor, &approved.tree_fingerprint, &Continue),
        Err(ProjectError::Rejected(OperationalErrorCode::InvalidProject))
    );
    ck(fs::remove_dir(extra))?;
    assert_eq!(
        ck(capture_with_expected(
            &fixture.vendor,
            &approved.tree_fingerprint,
            &Continue
        ))?,
        approved
    );
    Ok(())
}

#[test]
fn checksum_requires_exact_files_and_valid_digests() -> TestResult {
    for case in [
        "bad",
        "missing",
        "extra",
        "traversal",
        "null-package",
        "bad-package",
        "invalid-version",
        "invalid-name",
    ] {
        let fixture = Fixture::new()?;
        match case {
            "bad" => rewrite_checksum(&fixture, |value| {
                value["files"]["Cargo.toml"] = serde_json::Value::String("0".repeat(64));
            })?,
            "missing" => ck(fs::remove_file(
                fixture.vendor.join("proc-macro2-1.0.107/Cargo.toml.orig"),
            ))?,
            "extra" => ck(fs::write(
                fixture.vendor.join("proc-macro2-1.0.107/unlisted"),
                b"not declared",
            ))?,
            "traversal" => rewrite_checksum(&fixture, |value| {
                value["files"]["../Cargo.toml"] = serde_json::Value::String("0".repeat(64));
            })?,
            "null-package" => rewrite_checksum(&fixture, |value| {
                value["package"] = serde_json::Value::Null;
            })?,
            "bad-package" => rewrite_checksum(&fixture, |value| {
                value["package"] = serde_json::Value::String("ABC".into());
            })?,
            "invalid-version" => {
                let path = fixture.vendor.join("proc-macro2-1.0.107/Cargo.toml");
                let text = ck(fs::read_to_string(&path))?;
                ck(fs::write(
                    path,
                    text.replacen("version = \"1.0.107\"", "version = \"01.0.107\"", 1),
                ))?;
            }
            "invalid-name" => {
                let path = fixture.vendor.join("proc-macro2-1.0.107/Cargo.toml");
                let text = ck(fs::read_to_string(&path))?;
                ck(fs::write(
                    path,
                    text.replacen("name = \"proc-macro2\"", "name = \"-proc-macro2\"", 1),
                ))?;
            }
            _ => unreachable!(),
        }
        assert!(
            inspect_cargo_vendor(&fixture.vendor, &Continue).is_err(),
            "{case}"
        );
    }
    Ok(())
}

#[test]
fn checksum_duplicate_keys_and_duplicate_package_identity_are_rejected() -> TestResult {
    let fixture = Fixture::new()?;
    let path = fixture.checksum();
    let text = ck(fs::read_to_string(&path))?;
    let marker = format!(
        "\"Cargo.toml\":\"{}\"",
        ck(sha256_file(
            &fixture.vendor.join("proc-macro2-1.0.107/Cargo.toml")
        ))?
    );
    let duplicated = text.replacen(&marker, &format!("{marker},{marker}"), 1);
    if duplicated == text {
        return Err("checksum fixture did not contain Cargo.toml".into());
    }
    ck(fs::write(path, duplicated))?;
    assert!(inspect_cargo_vendor(&fixture.vendor, &Continue).is_err());

    let fixture = Fixture::new()?;
    copy_tree(
        &fixture.vendor.join("proc-macro2-1.0.107"),
        &fixture.vendor.join("proc-macro2-copy-1.0.107"),
    )?;
    assert!(inspect_cargo_vendor(&fixture.vendor, &Continue).is_err());
    Ok(())
}

fn sha256_file(path: &Path) -> TestResult<String> {
    use sha2::{Digest, Sha256};
    use std::fmt::Write;
    let mut encoded = String::new();
    for byte in Sha256::digest(ck(fs::read(path))?) {
        write!(&mut encoded, "{byte:02x}").map_err(|error| error.to_string())?;
    }
    Ok(encoded)
}

#[test]
fn native_capture_rejects_links_hardlinks_and_writable_nodes() -> TestResult {
    for case in ["symlink", "hardlink", "file-mode", "directory-mode"] {
        let fixture = Fixture::new()?;
        match case {
            "symlink" => ck(symlink(
                fixture.vendor.join("proc-macro2-1.0.107/Cargo.toml"),
                fixture.vendor.join("link"),
            ))?,
            "hardlink" => ck(fs::hard_link(
                fixture.vendor.join("proc-macro2-1.0.107/Cargo.toml"),
                fixture.vendor.join("proc-macro2-1.0.107/alias"),
            ))?,
            "file-mode" => ck(fs::set_permissions(
                fixture.vendor.join("proc-macro2-1.0.107/Cargo.toml"),
                fs::Permissions::from_mode(0o666),
            ))?,
            "directory-mode" => ck(fs::set_permissions(
                &fixture.vendor,
                fs::Permissions::from_mode(0o777),
            ))?,
            _ => unreachable!(),
        }
        assert!(
            inspect_cargo_vendor(&fixture.vendor, &Continue).is_err(),
            "{case}"
        );
    }
    Ok(())
}

#[test]
fn mutation_during_capture_is_rejected() -> TestResult {
    let fixture = Fixture::new()?;
    struct Mutate {
        calls: AtomicUsize,
        path: PathBuf,
    }
    impl OperationControl for Mutate {
        fn check(&self) -> Result<(), ProjectError> {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 5 {
                fs::write(&self.path, b"late entry").map_err(|_| ProjectError::Internal)?;
            }
            Ok(())
        }
    }
    let control = Mutate {
        calls: AtomicUsize::new(0),
        path: fixture.vendor.join("late"),
    };
    assert!(inspect_cargo_vendor(&fixture.vendor, &control).is_err());
    assert!(control.calls.load(Ordering::SeqCst) > 5);
    Ok(())
}
