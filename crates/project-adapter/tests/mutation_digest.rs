//! The portable half of the local mutation adapter: the plan digest that binds
//! a commit to exactly one before/after pair, and the refusal every entry point
//! of the store owes a platform ADR-053 does not qualify.
use rust_engineering_domain::{MutationCandidate, MutationKind, SourceBundle, SourceFile};
use rust_engineering_project::mutation_bytes_digest;
use rust_engineering_project::mutation_store::mutation_digest;

type TestResult = Result<(), String>;

fn file(path: &str, bytes: &[u8]) -> Result<SourceFile, String> {
    SourceFile::new(path.to_owned(), bytes.to_vec()).map_err(|error| format!("{error:?}"))
}

fn bundle(files: Vec<SourceFile>, directories: Vec<String>) -> Result<SourceBundle, String> {
    SourceBundle::with_directories(files, directories).map_err(|error| format!("{error:?}"))
}

fn candidate() -> Result<MutationCandidate, String> {
    Ok(MutationCandidate {
        kind: MutationKind::ManifestPatch,
        before: bundle(
            vec![file("Cargo.toml", b"[package]\n")?],
            vec!["src".to_owned()],
        )?,
        after: bundle(
            vec![file("Cargo.toml", b"[package]\nedition = \"2024\"\n")?],
            vec!["src".to_owned()],
        )?,
        validation: "toml_edit=0.25.13;cargo=1.98.1;operation=lints".to_owned(),
    })
}

fn digest(candidate: &MutationCandidate) -> Result<String, String> {
    mutation_digest(candidate)
        .map(|value| value.to_string())
        .map_err(|error| format!("{error:?}"))
}

#[test]
fn the_plan_digest_is_deterministic_and_canonical() -> TestResult {
    let plan = candidate()?;
    let expected = digest(&plan)?;
    assert_eq!(digest(&plan)?, expected);
    assert!(expected.starts_with("sha256:"));
    assert_eq!(expected.len(), "sha256:".len() + 64);
    // The same bundle described in a different input order is the same plan.
    let reordered = MutationCandidate {
        before: bundle(
            vec![
                file("src/lib.rs", b"fn main() {}\n")?,
                file("Cargo.toml", b"[package]\n")?,
            ],
            vec!["src".to_owned()],
        )?,
        ..candidate()?
    };
    let same = MutationCandidate {
        before: bundle(
            vec![
                file("Cargo.toml", b"[package]\n")?,
                file("src/lib.rs", b"fn main() {}\n")?,
            ],
            vec!["src".to_owned()],
        )?,
        ..candidate()?
    };
    assert_eq!(digest(&reordered)?, digest(&same)?);
    Ok(())
}

#[test]
fn every_part_of_the_plan_changes_the_digest() -> TestResult {
    let base = digest(&candidate()?)?;
    let mut seen = vec![base.clone()];

    for kind in [
        MutationKind::FormatApply,
        MutationKind::FixApply,
        MutationKind::DependencyAdd,
        MutationKind::DependencyRemove,
    ] {
        let other = digest(&MutationCandidate {
            kind,
            ..candidate()?
        })?;
        assert!(!seen.contains(&other), "{kind:?} repeats another digest");
        seen.push(other);
    }

    // A different before, after, validation provenance, declared directory or
    // path with identical bytes is a different plan, so it is a different
    // digest: length-prefixed fields cannot be shifted into one another.
    let variants = [
        MutationCandidate {
            before: bundle(vec![file("Cargo.toml", b"[package]x\n")?], vec![])?,
            ..candidate()?
        },
        MutationCandidate {
            after: bundle(vec![file("Cargo.toml", b"[package]\n")?], vec![])?,
            ..candidate()?
        },
        MutationCandidate {
            validation: "toml_edit=0.25.13;cargo=1.98.1;operation=lint".to_owned(),
            ..candidate()?
        },
        MutationCandidate {
            before: bundle(
                vec![file("Cargo.toml", b"[package]\n")?],
                vec!["src".to_owned(), "tests".to_owned()],
            )?,
            ..candidate()?
        },
        MutationCandidate {
            before: bundle(vec![file("Cargo.tom", b"l[package]\n")?], vec![])?,
            ..candidate()?
        },
    ];
    for variant in &variants {
        let other = digest(variant)?;
        assert!(!seen.contains(&other), "digest collision");
        seen.push(other);
    }
    Ok(())
}

#[test]
fn the_bytes_digest_is_the_same_canonical_spelling() -> TestResult {
    let digest = mutation_bytes_digest(b"[package]\n").map_err(|error| format!("{error:?}"))?;
    assert!(digest.to_string().starts_with("sha256:"));
    assert_eq!(digest.to_string().len(), "sha256:".len() + 64);
    assert_eq!(
        mutation_bytes_digest(b"[package]\n").map_err(|error| format!("{error:?}"))?,
        digest
    );
    assert_ne!(
        mutation_bytes_digest(b"[package]").map_err(|error| format!("{error:?}"))?,
        digest
    );
    Ok(())
}

/// ADR-053 qualifies the journalled local store on macOS only. Everywhere else
/// each entry point must refuse before it looks at a path, a lease or a plan.
#[cfg(not(target_os = "macos"))]
#[test]
fn an_unqualified_platform_refuses_every_store_entry_point() -> TestResult {
    use rust_engineering_domain::MutationError;
    use rust_engineering_project::mutation_store::NativeMutationStore;
    use std::path::{Path, PathBuf};

    let state = PathBuf::from("/nonexistent/rust-mcp-state");
    let roots = [PathBuf::from("/nonexistent/workspace")];
    assert_eq!(
        NativeMutationStore::open(&state, &roots).err(),
        Some(MutationError::UnsupportedPlatform)
    );
    assert_eq!(
        NativeMutationStore::open_for_kind(&state, &roots, MutationKind::FormatApply).err(),
        Some(MutationError::UnsupportedPlatform)
    );

    // The vendor, host-snapshot and quality-store entry points of the same
    // adapter fail closed for the same reason, and none of them reads the path
    // it was given: /etc/hosts exists and is still not examined.
    struct Proceed;
    impl rust_engineering_application::OperationControl for Proceed {
        fn check(&self) -> Result<(), rust_engineering_application::ProjectError> {
            Ok(())
        }
    }
    let existing = Path::new("/etc/hosts");
    assert!(rust_engineering_project::read_host_snapshot(existing, &Proceed).is_err());
    assert!(rust_engineering_project::inspect_cargo_vendor(existing, &Proceed).is_err());
    assert!(
        rust_engineering_project::quality_artifact_store::recover(existing)
            .err()
            .is_some_and(
                |error| error == rust_engineering_domain::QualityArtifactError::UnsupportedPlatform
            )
    );
    assert!(
        rust_engineering_project::quality_artifact_store::prune_expired(existing)
            .err()
            .is_some_and(
                |error| error == rust_engineering_domain::QualityArtifactError::UnsupportedPlatform
            )
    );
    assert!(rust_engineering_project::NativeQualityArtifactStore::open(existing).is_err());
    assert!(rust_engineering_project::NativeQualityArtifactStore::attach(existing).is_err());
    Ok(())
}
