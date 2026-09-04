use rust_engineering_project::catalog_store::{
    CatalogStore, StoreError, read_catalog_file, read_model_file, read_private_optional_file,
    read_trust_file,
};

#[cfg(not(target_os = "macos"))]
#[test]
fn unsupported_platform_has_no_filesystem_fallback() {
    for path in ["/nonexistent", "../store", r"C:\store"] {
        assert!(matches!(
            CatalogStore::open(std::path::Path::new(path)),
            Err(StoreError::UnsupportedPlatform)
        ));
        assert_eq!(
            read_catalog_file(std::path::Path::new(path), 1),
            Err(StoreError::UnsupportedPlatform)
        );
        assert_eq!(
            read_model_file(std::path::Path::new(path), 1),
            Err(StoreError::UnsupportedPlatform)
        );
        assert_eq!(
            read_trust_file(std::path::Path::new(path), 1),
            Err(StoreError::UnsupportedPlatform)
        );
        assert_eq!(
            read_private_optional_file(std::path::Path::new(path), 1),
            Err(StoreError::UnsupportedPlatform)
        );
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use rust_engineering_application::ReferenceGenerator;
    use rust_engineering_project::OsReferences;
    use rust_engineering_project::catalog_store::{MAX_CATALOG_FILE_BYTES, MAX_MODEL_FILE_BYTES};
    use std::fs;
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    type TestResult = Result<(), Box<dyn std::error::Error>>;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Result<Self, Box<dyn std::error::Error>> {
            let id = OsReferences.generate().map_err(|e| format!("{e:?}"))?;
            let base = std::env::temp_dir()
                .canonicalize()?
                .join(format!("catalog-store-{id}"));
            fs::create_dir(&base)?;
            fs::set_permissions(&base, fs::Permissions::from_mode(0o700))?;
            Ok(Self(base))
        }
        fn write(&self, name: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
            let path = self.0.join(name);
            fs::write(&path, bytes)?;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
            Ok(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn is_denied(path: &Path) {
        assert!(matches!(CatalogStore::open(path), Err(StoreError::Denied)));
    }

    #[test]
    fn private_optional_reader_does_not_lock_or_clean_staging() -> TestResult {
        let f = Fixture::new()?;
        let path = f.0.join("active.bundle");
        f.write("staging.bundle", b"interrupted")?;
        f.write("floor.staging", b"reserved")?;
        assert_eq!(read_private_optional_file(&path, 64)?, None);
        assert_eq!(read_trust_file(&path, 64), Err(StoreError::Io));
        f.write("active.bundle", b"payload")?;
        assert_eq!(
            read_private_optional_file(&path, 7)?.as_deref(),
            Some(b"payload".as_slice())
        );
        assert_eq!(
            read_private_optional_file(&path, 6),
            Err(StoreError::LimitExceeded)
        );
        assert_eq!(
            read_private_optional_file(&path, MAX_CATALOG_FILE_BYTES + 1),
            Err(StoreError::LimitExceeded)
        );
        assert!(!f.0.join("store.lock").exists());
        assert_eq!(fs::read(f.0.join("staging.bundle"))?, b"interrupted");
        assert_eq!(fs::read(f.0.join("floor.staging"))?, b"reserved");
        let _store = CatalogStore::open(&f.0)?;
        assert_eq!(
            read_private_optional_file(&path, 7)?.as_deref(),
            Some(b"payload".as_slice())
        );
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644))?;
        assert_eq!(
            read_private_optional_file(&path, 7),
            Err(StoreError::Denied)
        );
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        fs::hard_link(&path, f.0.join("alias"))?;
        assert_eq!(
            read_private_optional_file(&path, 7),
            Err(StoreError::Denied)
        );
        fs::remove_file(f.0.join("alias"))?;
        symlink(&path, f.0.join("link"))?;
        assert_eq!(
            read_private_optional_file(&f.0.join("link"), 7),
            Err(StoreError::Denied)
        );
        fs::set_permissions(&f.0, fs::Permissions::from_mode(0o755))?;
        assert_eq!(
            read_private_optional_file(&path, 7),
            Err(StoreError::Denied)
        );
        Ok(())
    }

    #[test]
    fn complete_bytes_replace_atomically_and_survive_reopen() -> TestResult {
        let f = Fixture::new()?;
        let mut store = CatalogStore::open(&f.0)?;
        assert_eq!(store.read_active()?, None);
        store.commit(b"first-complete-generation")?;
        // Holding the old descriptor proves replacement did not truncate its inode.
        let old = fs::File::open(f.0.join("active.bundle"))?;
        store.commit(b"second-complete-generation\0\xff")?;
        use std::io::Read;
        let mut old_bytes = Vec::new();
        (&old).read_to_end(&mut old_bytes)?;
        assert_eq!(old_bytes, b"first-complete-generation");
        assert!(!f.0.join("staging.bundle").exists());
        drop(store);
        let reopened = CatalogStore::open(&f.0)?;
        assert_eq!(
            reopened.read_active()?.as_deref(),
            Some(b"second-complete-generation\0\xff".as_slice())
        );
        Ok(())
    }

    #[test]
    fn concurrent_open_is_nonblocking_and_lock_releases_on_drop() -> TestResult {
        let f = Fixture::new()?;
        let store = CatalogStore::open(&f.0)?;
        let start = Instant::now();
        assert!(matches!(CatalogStore::open(&f.0), Err(StoreError::Busy)));
        assert!(start.elapsed() < Duration::from_secs(2));
        drop(store);
        assert!(CatalogStore::open(&f.0).is_ok());
        Ok(())
    }

    #[test]
    fn interrupted_staging_is_discarded_without_promotion_or_fallback() -> TestResult {
        for active in [
            None,
            Some(b"complete-active".as_slice()),
            Some(b"corrupt-active".as_slice()),
        ] {
            let f = Fixture::new()?;
            if let Some(bytes) = active {
                f.write("active.bundle", bytes)?;
            }
            f.write("staging.bundle", b"interrupted-next-generation")?;
            let store = CatalogStore::open(&f.0)?;
            assert!(!f.0.join("staging.bundle").exists());
            // The store returns exact bytes; authentication must reject corruption.
            assert_eq!(store.read_active()?.as_deref(), active);
        }
        Ok(())
    }

    #[test]
    fn a_busy_open_does_not_discard_another_imports_staging() -> TestResult {
        let f = Fixture::new()?;
        let _store = CatalogStore::open(&f.0)?;
        let staged = f.write("staging.bundle", b"pending")?;
        assert!(matches!(CatalogStore::open(&f.0), Err(StoreError::Busy)));
        assert_eq!(fs::read(staged)?, b"pending");
        Ok(())
    }

    #[test]
    fn moved_root_and_replaced_lock_fail_closed() -> TestResult {
        let f = Fixture::new()?;
        let root = f.0.join("root");
        fs::create_dir(&root)?;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
        let mut store = CatalogStore::open(&root)?;
        store.commit(b"old")?;
        fs::rename(&root, f.0.join("moved"))?;
        fs::create_dir(&root)?;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
        assert_eq!(store.read_active(), Err(StoreError::Changed));
        assert_eq!(store.commit(b"new"), Err(StoreError::Changed));
        assert_eq!(fs::read(f.0.join("moved/active.bundle"))?, b"old");
        assert!(!root.join("active.bundle").exists());
        let f = Fixture::new()?;
        let mut store = CatalogStore::open(&f.0)?;
        fs::rename(f.0.join("store.lock"), f.0.join("old.lock"))?;
        f.write("store.lock", b"")?;
        assert_eq!(store.commit(b"new"), Err(StoreError::Changed));
        Ok(())
    }

    #[test]
    fn active_deletion_during_lease_is_not_a_new_store() -> TestResult {
        let f = Fixture::new()?;
        let mut store = CatalogStore::open(&f.0)?;
        store.commit(b"generation")?;
        fs::remove_file(f.0.join("active.bundle"))?;
        assert_eq!(store.read_active(), Err(StoreError::Changed));
        assert_eq!(store.commit(b"replacement"), Err(StoreError::Changed));
        Ok(())
    }

    #[test]
    fn root_and_store_files_require_private_permissions() -> TestResult {
        for mode in [0o755, 0o770, 0o1700] {
            let f = Fixture::new()?;
            fs::set_permissions(&f.0, fs::Permissions::from_mode(mode))?;
            is_denied(&f.0);
            assert!(!f.0.join("store.lock").exists());
        }
        for name in ["store.lock", "staging.bundle", "active.bundle"] {
            let f = Fixture::new()?;
            let path = f.write(name, b"")?;
            fs::set_permissions(path, fs::Permissions::from_mode(0o644))?;
            if name == "active.bundle" {
                assert_eq!(
                    CatalogStore::open(&f.0)?.read_active(),
                    Err(StoreError::Denied)
                );
            } else {
                is_denied(&f.0);
            }
        }
        Ok(())
    }

    #[test]
    fn links_and_special_files_at_every_fixed_name_are_rejected() -> TestResult {
        for name in ["store.lock", "staging.bundle", "active.bundle"] {
            for kind in ["symlink", "hardlink", "fifo", "directory"] {
                let f = Fixture::new()?;
                let outside = f.write("outside", b"")?;
                let path = f.0.join(name);
                match kind {
                    "symlink" => symlink(&outside, &path)?,
                    "hardlink" => fs::hard_link(&outside, &path)?,
                    "directory" => fs::create_dir(&path)?,
                    _ => {
                        // Fixed harness utility only; production never spawns processes.
                        assert!(
                            std::process::Command::new("/usr/bin/mkfifo")
                                .env_clear()
                                .arg(&path)
                                .status()?
                                .success()
                        );
                    }
                }
                let start = Instant::now();
                if name == "active.bundle" {
                    let mut store = CatalogStore::open(&f.0)?;
                    assert!(store.read_active().is_err(), "{name}: {kind}");
                    assert!(store.commit(b"new").is_err(), "{name}: {kind}");
                } else {
                    assert!(CatalogStore::open(&f.0).is_err(), "{name}: {kind}");
                }
                assert!(start.elapsed() < Duration::from_secs(2));
                assert_eq!(fs::read(outside)?, b"");
            }
        }
        let f = Fixture::new()?;
        symlink(&f.0, f.0.join("alias"))?;
        assert!(CatalogStore::open(&f.0.join("alias")).is_err());
        Ok(())
    }

    #[test]
    fn bounded_input_acquisition_rejects_untrusted_names_and_types() -> TestResult {
        let f = Fixture::new()?;
        let path = f.write("bundle", b"four")?;
        assert_eq!(read_catalog_file(&path, 4)?, b"four");
        assert_eq!(read_catalog_file(&path, 3), Err(StoreError::LimitExceeded));
        assert_eq!(
            read_catalog_file(&path, MAX_CATALOG_FILE_BYTES + 1),
            Err(StoreError::LimitExceeded)
        );
        for invalid in [
            "relative".to_owned(),
            format!("{}/./bundle", f.0.display()),
            format!("{}//bundle", f.0.display()),
            format!("{}/../bundle", f.0.display()),
        ] {
            assert_eq!(
                read_catalog_file(Path::new(&invalid), 4),
                Err(StoreError::InvalidPath)
            );
        }
        symlink(&path, f.0.join("alias"))?;
        assert!(read_catalog_file(&f.0.join("alias"), 4).is_err());
        fs::hard_link(&path, f.0.join("link"))?;
        assert!(read_catalog_file(&path, 4).is_err());
        assert!(read_catalog_file(&f.0, 4).is_err());
        Ok(())
    }

    #[test]
    fn model_input_has_separate_bound_without_expanding_catalog_authority() -> TestResult {
        assert_eq!(MAX_MODEL_FILE_BYTES, 512 * 1024 * 1024);
        assert_eq!(MAX_CATALOG_FILE_BYTES, 80 * 1024 * 1024);
        // The bound is checked before path validation or filesystem access.
        assert_eq!(
            read_model_file(Path::new("relative-missing"), MAX_MODEL_FILE_BYTES + 1),
            Err(StoreError::LimitExceeded)
        );
        let f = Fixture::new()?;
        let path = f.write("model", b"model-bytes")?;
        assert_eq!(
            read_model_file(&path, MAX_MODEL_FILE_BYTES)?,
            b"model-bytes"
        );
        assert_eq!(read_model_file(&path, 10), Err(StoreError::LimitExceeded));
        assert_eq!(
            read_catalog_file(&path, MAX_CATALOG_FILE_BYTES + 1),
            Err(StoreError::LimitExceeded)
        );
        symlink(&path, f.0.join("model-alias"))?;
        assert!(read_model_file(&f.0.join("model-alias"), MAX_MODEL_FILE_BYTES).is_err());
        fs::hard_link(&path, f.0.join("model-link"))?;
        assert!(read_model_file(&path, MAX_MODEL_FILE_BYTES).is_err());
        Ok(())
    }

    #[test]
    fn trust_file_requires_private_owner_file_parent_and_safe_ancestors() -> TestResult {
        let f = Fixture::new()?;
        let path = f.write("trust.json", b"host-trust")?;
        assert_eq!(read_trust_file(&path, 4096)?, b"host-trust");
        for mode in [0o644, 0o660, 0o400] {
            fs::set_permissions(&path, fs::Permissions::from_mode(mode))?;
            assert_eq!(read_trust_file(&path, 4096), Err(StoreError::Denied));
        }
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        fs::set_permissions(&f.0, fs::Permissions::from_mode(0o755))?;
        assert_eq!(read_trust_file(&path, 4096), Err(StoreError::Denied));
        fs::set_permissions(&f.0, fs::Permissions::from_mode(0o700))?;
        let shared = f.0.join("shared");
        let parent = shared.join("private");
        fs::create_dir(&shared)?;
        fs::create_dir(&parent)?;
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o700))?;
        fs::copy(&path, parent.join("trust.json"))?;
        for mode in [0o770, 0o777, 0o1777] {
            fs::set_permissions(&shared, fs::Permissions::from_mode(mode))?;
            assert_eq!(
                read_trust_file(&parent.join("trust.json"), 4096),
                Err(StoreError::Denied)
            );
        }
        fs::set_permissions(&shared, fs::Permissions::from_mode(0o755))?;
        assert_eq!(
            read_trust_file(&parent.join("trust.json"), 4096)?,
            b"host-trust"
        );
        symlink(&path, f.0.join("trust-alias"))?;
        assert!(read_trust_file(&f.0.join("trust-alias"), 4096).is_err());
        symlink(&parent, f.0.join("parent-alias"))?;
        assert!(read_trust_file(&f.0.join("parent-alias/trust.json"), 4096).is_err());
        fs::hard_link(&path, f.0.join("trust-hardlink"))?;
        assert!(read_trust_file(&path, 4096).is_err());
        Ok(())
    }

    #[test]
    fn root_owned_sticky_tmp_requires_private_immediate_child() -> TestResult {
        let id = OsReferences.generate().map_err(|e| format!("{e:?}"))?;
        let base = Path::new("/private/tmp").join(format!("catalog-trust-{id}"));
        fs::create_dir(&base)?;
        let fixture = Fixture(base);
        fs::set_permissions(&fixture.0, fs::Permissions::from_mode(0o700))?;
        let trust = fixture.write("trust.json", b"private-trust")?;
        assert_eq!(read_trust_file(&trust, 4096)?, b"private-trust");
        // A nonprivate immediate child is rejected even when the final parent
        // below it is private. Sticky protects entries; it does not grant privacy.
        let inner = fixture.0.join("inner");
        fs::create_dir(&inner)?;
        fs::set_permissions(&inner, fs::Permissions::from_mode(0o700))?;
        fs::copy(&trust, inner.join("trust.json"))?;
        fs::set_permissions(&fixture.0, fs::Permissions::from_mode(0o755))?;
        assert_eq!(
            read_trust_file(&inner.join("trust.json"), 4096),
            Err(StoreError::Denied)
        );
        Ok(())
    }

    #[test]
    fn floor_is_independent_bounded_durable_and_never_promoted_from_staging() -> TestResult {
        let f = Fixture::new()?;
        let mut store = CatalogStore::open(&f.0)?;
        assert_eq!(store.read_floor()?, None);
        store.commit(b"active-bytes")?;
        store.reserve_floor(b"floor-one")?;
        assert_eq!(
            store.read_active()?.as_deref(),
            Some(b"active-bytes".as_slice())
        );
        store.reserve_floor(&[b'f'; 4096])?;
        assert_eq!(
            store.reserve_floor(&[0; 4097]),
            Err(StoreError::LimitExceeded)
        );
        drop(store);
        f.write("floor.staging", b"partial-floor-two")?;
        f.write("staging.bundle", b"partial-active-two")?;
        let mut store = CatalogStore::open(&f.0)?;
        assert_eq!(store.read_floor()?, Some(vec![b'f'; 4096]));
        assert_eq!(
            store.read_active()?.as_deref(),
            Some(b"active-bytes".as_slice())
        );
        assert!(!f.0.join("floor.staging").exists());
        assert!(!f.0.join("staging.bundle").exists());
        fs::remove_file(f.0.join("floor.record"))?;
        assert_eq!(store.read_floor(), Err(StoreError::Changed));
        assert_eq!(store.reserve_floor(b"new-floor"), Err(StoreError::Changed));
        drop(store);
        f.write("floor.staging", b"uncommitted-first-floor")?;
        let store = CatalogStore::open(&f.0)?;
        assert_eq!(store.read_floor()?, None);
        assert_eq!(
            store.read_active()?.as_deref(),
            Some(b"active-bytes".as_slice())
        );
        Ok(())
    }

    #[test]
    fn floor_record_and_staging_reject_links_and_oversized_bytes() -> TestResult {
        for name in ["floor.record", "floor.staging"] {
            let f = Fixture::new()?;
            let outside = f.write("outside", b"outside")?;
            symlink(&outside, f.0.join(name))?;
            if name == "floor.record" {
                let mut store = CatalogStore::open(&f.0)?;
                assert!(store.read_floor().is_err());
                assert!(store.reserve_floor(b"new-floor").is_err());
            } else {
                assert!(CatalogStore::open(&f.0).is_err());
            }
            assert_eq!(fs::read(&outside)?, b"outside");
            fs::remove_file(f.0.join(name))?;
            f.write(name, &[0; 4097])?;
            if name == "floor.record" {
                assert_eq!(
                    CatalogStore::open(&f.0)?.read_floor(),
                    Err(StoreError::LimitExceeded)
                );
            } else {
                assert!(matches!(
                    CatalogStore::open(&f.0),
                    Err(StoreError::LimitExceeded)
                ));
            }
        }
        Ok(())
    }

    #[test]
    fn exact_limit_roundtrips_and_oversized_state_is_not_overwritten() -> TestResult {
        let f = Fixture::new()?;
        let mut store = CatalogStore::open(&f.0)?;
        let bytes = vec![b'x'; MAX_CATALOG_FILE_BYTES];
        store.commit(&bytes)?;
        assert_eq!(store.read_active()?.as_deref(), Some(bytes.as_slice()));
        assert_eq!(
            read_catalog_file(&f.0.join("active.bundle"), MAX_CATALOG_FILE_BYTES)?,
            bytes
        );
        let oversized = vec![0; MAX_CATALOG_FILE_BYTES + 1];
        assert_eq!(store.commit(&oversized), Err(StoreError::LimitExceeded));
        fs::OpenOptions::new()
            .write(true)
            .open(f.0.join("active.bundle"))?
            .set_len((MAX_CATALOG_FILE_BYTES + 1) as u64)?;
        assert_eq!(store.read_active(), Err(StoreError::LimitExceeded));
        assert_eq!(store.commit(b"new"), Err(StoreError::LimitExceeded));
        drop(store);
        let staged = f.write("staging.bundle", b"")?;
        fs::OpenOptions::new()
            .write(true)
            .open(&staged)?
            .set_len((MAX_CATALOG_FILE_BYTES + 1) as u64)?;
        assert!(matches!(
            CatalogStore::open(&f.0),
            Err(StoreError::LimitExceeded)
        ));
        assert!(staged.exists());
        Ok(())
    }
}
