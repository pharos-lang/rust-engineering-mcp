//! Real-filesystem tests for ADR-078's capture. No fixture is a mock: every
//! case below drives the same code the CLI drives, over a real APFS tree.
use super::*;
use crate::OsReferences;
use rust_engineering_application::ReferenceGenerator;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Cleanup(PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Never;
impl OperationControl for Never {
    fn check(&self) -> Result<(), ProjectError> {
        Ok(())
    }
}

/// Counts the cooperative checkpoints one complete capture takes, so the
/// cancellation and mutation sweeps below cover every one of them rather than
/// a number guessed in advance.
#[derive(Default)]
struct Counting(AtomicUsize);
impl OperationControl for Counting {
    fn check(&self) -> Result<(), ProjectError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

fn checkpoints(vendor: &Path, store: &Path) -> Result<usize, String> {
    let counting = Counting::default();
    capture_vendor_tree(vendor, store, &counting).map_err(|error| format!("{error:?}"))?;
    clear(store)?;
    Ok(counting.0.load(Ordering::SeqCst))
}

fn clear(store: &Path) -> Result<(), String> {
    for name in names(store)? {
        std::fs::remove_file(store.join(name)).map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Cancels the operation on the `at`-th cooperative checkpoint.
struct CancelAt {
    calls: AtomicUsize,
    at: usize,
}
impl OperationControl for CancelAt {
    fn check(&self) -> Result<(), ProjectError> {
        if self.calls.fetch_add(1, Ordering::SeqCst) + 1 >= self.at {
            return Err(ProjectError::Cancelled);
        }
        Ok(())
    }
}

/// Runs `mutate` once, on the `at`-th checkpoint, and otherwise admits.
struct MutateAt<F: Fn() + Send + Sync> {
    calls: AtomicUsize,
    at: usize,
    mutate: F,
}
impl<F: Fn() + Send + Sync> OperationControl for MutateAt<F> {
    fn check(&self) -> Result<(), ProjectError> {
        if self.calls.fetch_add(1, Ordering::SeqCst) + 1 == self.at {
            (self.mutate)();
        }
        Ok(())
    }
}

fn scratch(label: &str) -> Result<(PathBuf, Cleanup), String> {
    let root = PathBuf::from("/private/tmp").join(format!(
        "rms-{label}-{}",
        OsReferences
            .generate()
            .map_err(|error| format!("{error:?}"))?
    ));
    std::fs::create_dir(&root).map_err(|error| error.to_string())?;
    let cleanup = Cleanup(root.clone());
    Ok((root, cleanup))
}

/// A small vendor-shaped tree, including a path the qualified `SourceBundle`
/// alphabet refuses: parentheses are the reason this contract exists.
fn tree(root: &Path) -> Result<(PathBuf, PathBuf), String> {
    let vendor = root.join("vendor");
    let store = root.join("store");
    std::fs::create_dir(&vendor).map_err(|error| error.to_string())?;
    std::fs::create_dir(&store).map_err(|error| error.to_string())?;
    let package = vendor.join("zerocopy-derive-0.8.56");
    std::fs::create_dir(&package).map_err(|error| error.to_string())?;
    std::fs::create_dir(package.join("src")).map_err(|error| error.to_string())?;
    std::fs::write(package.join("Cargo.toml"), b"[package]\n")
        .map_err(|error| error.to_string())?;
    std::fs::write(
        package.join("src/into_bytes_enum.repr(i8).expected.rs"),
        b"// parentheses\n",
    )
    .map_err(|error| error.to_string())?;
    std::fs::write(package.join("src/lib.rs"), vec![7_u8; 5000])
        .map_err(|error| error.to_string())?;
    Ok((vendor, store))
}

fn names(store: &Path) -> Result<Vec<String>, String> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(store).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        found.push(entry.file_name().to_string_lossy().into_owned());
    }
    found.sort();
    Ok(found)
}

#[test]
fn a_capture_publishes_one_artifact_named_by_its_digest_and_verifies_back() -> Result<(), String> {
    let (root, _cleanup) = scratch("capture")?;
    let (vendor, store) = tree(&root)?;
    let identity =
        capture_vendor_tree(&vendor, &store, &Never).map_err(|error| format!("{error:?}"))?;
    assert_eq!(identity.files(), 3);
    assert_eq!(identity.directories(), 2);
    assert_eq!(identity.entries(), 5);
    assert_eq!(identity.total_bytes(), 10 + 15 + 5000);

    let expected = artifact_name(identity.tree_digest()).map_err(|error| format!("{error:?}"))?;
    assert_eq!(names(&store)?, vec![expected.clone()]);

    let (verified, _file) =
        verify_vendor_capture(&store.join(&expected), identity.tree_digest(), &Never)
            .map_err(|error| format!("{error:?}"))?;
    assert_eq!(verified, identity);

    // The same tree captured twice is the same capture, reused by digest
    // rather than written again.
    let again =
        capture_vendor_tree(&vendor, &store, &Never).map_err(|error| format!("{error:?}"))?;
    assert_eq!(&again, &identity);
    assert_eq!(names(&store)?, vec![expected]);
    Ok(())
}

#[test]
fn an_artifact_whose_digest_is_not_the_declared_one_is_refused() -> Result<(), String> {
    let (root, _cleanup) = scratch("declared")?;
    let (vendor, store) = tree(&root)?;
    let identity =
        capture_vendor_tree(&vendor, &store, &Never).map_err(|error| format!("{error:?}"))?;
    let name = artifact_name(identity.tree_digest()).map_err(|error| format!("{error:?}"))?;
    let other: SourceFingerprint = format!("sha256:{}", "b".repeat(64))
        .parse()
        .map_err(|_| "fingerprint".to_owned())?;
    assert_eq!(
        verify_vendor_capture(&store.join(&name), &other, &Never)
            .err()
            .map(|error| format!("{error:?}")),
        Some(format!("{:?}", capture_error(VendorCaptureError::Digest))),
        "a capture that is not the declared one is refused, not used"
    );
    Ok(())
}

#[test]
fn a_symlink_in_the_tree_is_refused_rather_than_skipped() -> Result<(), String> {
    let (root, _cleanup) = scratch("symlink")?;
    let (vendor, store) = tree(&root)?;
    std::os::unix::fs::symlink(
        vendor.join("zerocopy-derive-0.8.56/src/lib.rs"),
        vendor.join("zerocopy-derive-0.8.56/alias.rs"),
    )
    .map_err(|error| error.to_string())?;
    assert!(
        capture_vendor_tree(&vendor, &store, &Never).is_err(),
        "a symlink is a refusal"
    );
    assert!(
        names(&store)?.is_empty(),
        "and it leaves no artifact behind"
    );
    Ok(())
}

#[test]
fn a_hard_link_in_the_tree_is_refused_rather_than_skipped() -> Result<(), String> {
    let (root, _cleanup) = scratch("hardlink")?;
    let (vendor, store) = tree(&root)?;
    std::fs::hard_link(
        vendor.join("zerocopy-derive-0.8.56/Cargo.toml"),
        vendor.join("zerocopy-derive-0.8.56/Cargo.toml.alias"),
    )
    .map_err(|error| error.to_string())?;
    assert!(
        capture_vendor_tree(&vendor, &store, &Never).is_err(),
        "a second link to the same inode is a refusal"
    );
    assert!(names(&store)?.is_empty());
    Ok(())
}

#[test]
fn a_path_outside_the_widened_alphabet_is_refused() -> Result<(), String> {
    let (root, _cleanup) = scratch("alphabet")?;
    let (vendor, store) = tree(&root)?;
    std::fs::write(vendor.join("zerocopy-derive-0.8.56/we:ird.rs"), b"x")
        .map_err(|error| error.to_string())?;
    assert!(capture_vendor_tree(&vendor, &store, &Never).is_err());
    assert!(names(&store)?.is_empty());
    Ok(())
}

#[test]
fn a_tree_that_changes_mid_capture_fails_instead_of_publishing_what_moved() -> Result<(), String> {
    let (root, _cleanup) = scratch("mutate")?;
    let (vendor, store) = tree(&root)?;
    let total = checkpoints(&vendor, &store)?;
    let target = vendor.join("zerocopy-derive-0.8.56/Cargo.toml");
    let mut refused = 0;
    // A file rewritten to the same length after the walk read it. The
    // per-entry stamps cannot see this one, because the read already
    // finished; the final re-observation of every entry can.
    for at in 1..=total {
        std::fs::write(&target, b"[package]\n").map_err(|error| error.to_string())?;
        let control = MutateAt {
            calls: AtomicUsize::new(0),
            at,
            mutate: {
                let target = target.clone();
                move || {
                    let _ = std::fs::write(&target, b"[PACKAGE]\n");
                }
            },
        };
        if capture_vendor_tree(&vendor, &store, &control).is_err() {
            refused += 1;
            assert!(
                names(&store)?.is_empty(),
                "a refused capture at checkpoint {at} left residue"
            );
        }
        clear(&store)?;
    }
    assert!(
        refused > 0,
        "a tree that moved during the capture is never published"
    );
    Ok(())
}

#[test]
fn a_file_that_grows_while_it_is_read_fails_the_capture() -> Result<(), String> {
    let (root, _cleanup) = scratch("grow")?;
    let (vendor, store) = tree(&root)?;
    let total = checkpoints(&vendor, &store)?;
    let target = vendor.join("zerocopy-derive-0.8.56/src/lib.rs");
    let mut refused = 0;
    for at in 1..=total {
        std::fs::write(&target, vec![7_u8; 5000]).map_err(|error| error.to_string())?;
        let control = MutateAt {
            calls: AtomicUsize::new(0),
            at,
            mutate: {
                let target = target.clone();
                move || {
                    let _ = std::fs::write(&target, vec![7_u8; 9000]);
                }
            },
        };
        if capture_vendor_tree(&vendor, &store, &control).is_err() {
            refused += 1;
            assert!(names(&store)?.is_empty(), "no residue at checkpoint {at}");
        }
        clear(&store)?;
    }
    assert!(refused > 0, "some checkpoint had to observe the growth");
    Ok(())
}

#[test]
fn a_cancelled_capture_leaves_nothing_behind() -> Result<(), String> {
    let (root, _cleanup) = scratch("cancel")?;
    let (vendor, store) = tree(&root)?;
    let total = checkpoints(&vendor, &store)?;
    assert!(
        total > 1,
        "a capture has cooperative checkpoints to cancel at"
    );
    for at in 1..=total {
        let control = CancelAt {
            calls: AtomicUsize::new(0),
            at,
        };
        assert_eq!(
            capture_vendor_tree(&vendor, &store, &control),
            Err(ProjectError::Cancelled),
            "cancelled at checkpoint {at} of {total}"
        );
        assert!(
            names(&store)?.is_empty(),
            "a cancelled capture at checkpoint {at} left residue"
        );
    }
    Ok(())
}

#[test]
fn the_store_may_not_be_the_tree_it_captures() -> Result<(), String> {
    let (root, _cleanup) = scratch("nested")?;
    let (vendor, _store) = tree(&root)?;
    let inside = vendor.join("zerocopy-derive-0.8.56");
    assert!(capture_vendor_tree(&vendor, &vendor, &Never).is_err());
    assert!(capture_vendor_tree(&vendor, &inside, &Never).is_err());
    Ok(())
}

#[test]
fn a_truncated_artifact_is_not_accepted_as_a_capture() -> Result<(), String> {
    let (root, _cleanup) = scratch("truncated")?;
    let (vendor, store) = tree(&root)?;
    let identity =
        capture_vendor_tree(&vendor, &store, &Never).map_err(|error| format!("{error:?}"))?;
    let name = artifact_name(identity.tree_digest()).map_err(|error| format!("{error:?}"))?;
    let path = store.join(&name);
    let bytes = std::fs::read(&path).map_err(|error| error.to_string())?;
    std::fs::write(&path, &bytes[..bytes.len() - 512]).map_err(|error| error.to_string())?;
    assert!(verify_vendor_capture(&path, identity.tree_digest(), &Never).is_err());
    Ok(())
}
