use rust_engineering_application::ManifestEditor;
use rust_engineering_domain::{
    BuiltinProfile, DependencyKind, DependencyName, DependencySpec, DependencyTarget, FeatureName,
    FeatureValue, LintLevel, LintName, LintScope, LintTool, ManifestEdit, ManifestEditError,
    ProfileCodegenUnits, ProfileDebugInfo, ProfileLto, ProfileOptLevel, ProfilePanic,
    ProfileSettingEdit, ProfileSettingKey, ProfileStrip,
};
use rust_engineering_project::TomlManifestEditor;

fn name(value: &str) -> Result<LintName, ManifestEditError> {
    LintName::new(value.to_owned())
}

#[test]
fn virtual_workspace_rejects_package_lints() -> Result<(), ManifestEditError> {
    let edit = set(
        LintScope::Package,
        LintTool::Rust,
        "unsafe_code",
        LintLevel::Deny,
        None,
    )?;
    assert_eq!(
        TomlManifestEditor.apply(b"[workspace]\nmembers = []\n", &edit),
        Err(ManifestEditError::InvalidOperation)
    );
    Ok(())
}

fn set(
    scope: LintScope,
    tool: LintTool,
    lint: &str,
    level: LintLevel,
    priority: Option<i64>,
) -> Result<ManifestEdit, ManifestEditError> {
    Ok(ManifestEdit::LintSet {
        scope,
        tool,
        name: name(lint)?,
        level,
        priority,
    })
}

fn remove(scope: LintScope, tool: LintTool, lint: &str) -> Result<ManifestEdit, ManifestEditError> {
    Ok(ManifestEdit::LintRemove {
        scope,
        tool,
        name: name(lint)?,
    })
}

#[test]
fn semantic_no_op_preserves_every_byte() -> Result<(), ManifestEditError> {
    let before = b"# header\r\n[package]\r\nname = \"demo\"\r\n\r\n[lints.rust]\r\nunsafe_code = { priority = -1, level = \"deny\" } # keep\r\n";
    let edit = set(
        LintScope::Package,
        LintTool::Rust,
        "unsafe_code",
        LintLevel::Deny,
        Some(-1),
    )?;
    let after = TomlManifestEditor.apply(before, &edit)?;
    assert_eq!(after, before);

    let shorthand =
        b"[package]\nname = \"demo\"\n[lints.rust]\nunsafe_code = \"deny\" # priority zero\n";
    let explicit_zero = set(
        LintScope::Package,
        LintTool::Rust,
        "unsafe_code",
        LintLevel::Deny,
        Some(0),
    )?;
    assert_eq!(
        TomlManifestEditor.apply(shorthand, &explicit_zero)?,
        shorthand
    );

    let detailed_zero = b"[package]\nname = \"demo\"\n[lints.rust]\nunsafe_code = { level = \"deny\", priority = 0 } # explicit\n";
    let implicit_zero = set(
        LintScope::Package,
        LintTool::Rust,
        "unsafe_code",
        LintLevel::Deny,
        None,
    )?;
    assert_eq!(
        TomlManifestEditor.apply(detailed_zero, &implicit_zero)?,
        detailed_zero
    );
    Ok(())
}

#[test]
fn set_preserves_value_comment_key_spelling_and_crlf() -> Result<(), ManifestEditError> {
    let before = b"[package]\r\nname = \"demo\"\r\n\r\n[lints.rust]\r\n'unsafe_code' = \"warn\" # attached\r\nnext_lint = \"allow\" # neighbor\r\n";
    let edit = set(
        LintScope::Package,
        LintTool::Rust,
        "unsafe_code",
        LintLevel::Deny,
        Some(-2),
    )?;
    let after = TomlManifestEditor.apply(before, &edit)?;
    let text = std::str::from_utf8(&after).map_err(|_| ManifestEditError::InvalidManifest)?;
    assert!(
        text.contains("'unsafe_code' = { level = \"deny\", priority = -2 } # attached\r\n"),
        "{text:?}"
    );
    assert!(text.contains("next_lint = \"allow\" # neighbor\r\n"));

    let scalar = TomlManifestEditor.apply(
        &after,
        &set(
            LintScope::Package,
            LintTool::Rust,
            "unsafe_code",
            LintLevel::Allow,
            None,
        )?,
    )?;
    let scalar = std::str::from_utf8(&scalar).map_err(|_| ManifestEditError::InvalidManifest)?;
    assert!(
        scalar.contains("'unsafe_code' = \"allow\" # attached\r\n"),
        "{scalar:?}"
    );
    Ok(())
}

#[test]
fn inserts_standard_package_and_workspace_tables() -> Result<(), ManifestEditError> {
    let package = TomlManifestEditor.apply(
        b"[package]\nname = \"demo\"\n",
        &set(
            LintScope::Package,
            LintTool::Clippy,
            "unwrap_used",
            LintLevel::Deny,
            None,
        )?,
    )?;
    assert_eq!(
        std::str::from_utf8(&package).map_err(|_| ManifestEditError::InvalidManifest)?,
        "[package]\nname = \"demo\"\n\n[lints.clippy]\nunwrap_used = \"deny\"\n"
    );

    let workspace = TomlManifestEditor.apply(
        b"[workspace]\nmembers = []\n",
        &set(
            LintScope::Workspace,
            LintTool::Rust,
            "unsafe_code",
            LintLevel::Forbid,
            None,
        )?,
    )?;
    assert_eq!(
        std::str::from_utf8(&workspace).map_err(|_| ManifestEditError::InvalidManifest)?,
        "[workspace]\nmembers = []\n\n[workspace.lints.rust]\nunsafe_code = \"forbid\"\n"
    );
    Ok(())
}

#[test]
fn remove_drops_only_target_and_its_attached_comment() -> Result<(), ManifestEditError> {
    let before = b"[package]\nname = \"demo\"\n\n[lints.rust]\n# attached to target\nunsafe_code = \"deny\" # target suffix\n# belongs to neighbor\nunused = \"warn\" # neighbor suffix\n";
    let after = TomlManifestEditor.apply(
        before,
        &remove(LintScope::Package, LintTool::Rust, "unsafe_code")?,
    )?;
    let text = std::str::from_utf8(&after).map_err(|_| ManifestEditError::InvalidManifest)?;
    assert!(!text.contains("attached to target"));
    assert!(!text.contains("target suffix"));
    assert!(text.contains("# belongs to neighbor\nunused = \"warn\" # neighbor suffix\n"));
    assert!(text.contains("[lints.rust]\n"));
    Ok(())
}

#[test]
fn missing_remove_is_an_exact_no_op() -> Result<(), ManifestEditError> {
    let before = b"[package]\nname = \"demo\"";
    let after = TomlManifestEditor.apply(
        before,
        &remove(LintScope::Package, LintTool::Rust, "unsafe_code")?,
    )?;
    assert_eq!(after, before);
    Ok(())
}

#[test]
fn inheritance_and_missing_workspace_are_rejected() -> Result<(), ManifestEditError> {
    let inherited = b"[package]\nname = \"demo\"\n[lints]\nworkspace = true\n";
    assert_eq!(
        TomlManifestEditor.apply(
            inherited,
            &set(
                LintScope::Package,
                LintTool::Rust,
                "unsafe_code",
                LintLevel::Deny,
                None,
            )?,
        ),
        Err(ManifestEditError::InheritedLints)
    );
    assert_eq!(
        TomlManifestEditor.apply(
            b"[package]\nname = \"demo\"\n",
            &set(
                LintScope::Workspace,
                LintTool::Rust,
                "unsafe_code",
                LintLevel::Deny,
                None,
            )?,
        ),
        Err(ManifestEditError::InvalidOperation)
    );
    Ok(())
}

#[test]
fn inline_dotted_and_wrong_type_paths_are_rejected() -> Result<(), ManifestEditError> {
    let edit = set(
        LintScope::Package,
        LintTool::Rust,
        "unsafe_code",
        LintLevel::Deny,
        None,
    )?;
    for manifest in [
        "lints = { rust = { unsafe_code = \"warn\" } }\n[package]\nname = \"demo\"\n",
        "lints.rust.unsafe_code = \"warn\"\n[package]\nname = \"demo\"\n",
        "lints = []\n[package]\nname = \"demo\"\n",
        "[package]\nname = \"demo\"\n[lints]\nrust = \"bad\"\n",
    ] {
        assert_eq!(
            TomlManifestEditor.apply(manifest.as_bytes(), &edit),
            Err(ManifestEditError::UnsupportedLayout),
            "{manifest}"
        );
    }
    Ok(())
}

#[test]
fn invalid_existing_settings_are_rejected() -> Result<(), ManifestEditError> {
    let edit = set(
        LintScope::Package,
        LintTool::Rust,
        "unsafe_code",
        LintLevel::Deny,
        None,
    )?;
    for manifest in [
        "[package]\nname = \"demo\"\n[lints.rust]\nunsafe_code = \"loud\"\n",
        "[package]\nname = \"demo\"\n[lints.rust]\nunsafe_code = { level = \"deny\", extra = true }\n",
        "[package]\nname = \"demo\"\n[lints.rust]\nunsafe_code = { priority = 1 }\n",
    ] {
        assert_eq!(
            TomlManifestEditor.apply(manifest.as_bytes(), &edit),
            Err(ManifestEditError::InvalidManifest),
            "{manifest}"
        );
    }
    Ok(())
}

#[test]
fn malformed_utf8_limits_depth_and_name_injection_fail_closed() -> Result<(), ManifestEditError> {
    let edit = set(
        LintScope::Package,
        LintTool::Rust,
        "unsafe_code",
        LintLevel::Deny,
        None,
    )?;
    assert_eq!(
        TomlManifestEditor.apply(&[0xff], &edit),
        Err(ManifestEditError::InvalidManifest)
    );
    assert_eq!(
        TomlManifestEditor.apply(&vec![b' '; 256 * 1024 + 1], &edit),
        Err(ManifestEditError::LimitExceeded)
    );
    let deep = format!("value = {}1{}\n", "[".repeat(300), "]".repeat(300));
    assert_eq!(
        TomlManifestEditor.apply(deep.as_bytes(), &edit),
        Err(ManifestEditError::InvalidManifest)
    );
    for invalid in [
        "rust.bad",
        "unsafe-code",
        "x\n[lints.clippy]",
        "",
        &"x".repeat(129),
    ] {
        assert_eq!(
            LintName::new(invalid.to_owned()),
            Err(ManifestEditError::InvalidOperation),
            "{invalid:?}"
        );
    }
    Ok(())
}

#[test]
fn a_changed_mixed_newline_document_is_rejected() -> Result<(), ManifestEditError> {
    let before = b"[package]\r\nname = \"demo\"\n[lints.rust]\r\nunsafe_code = \"warn\"\n";
    assert_eq!(
        TomlManifestEditor.apply(
            before,
            &set(
                LintScope::Package,
                LintTool::Rust,
                "unsafe_code",
                LintLevel::Deny,
                None,
            )?,
        ),
        Err(ManifestEditError::UnsupportedLayout)
    );
    Ok(())
}

#[test]
fn unrelated_dotted_key_reordering_refuses_the_change() -> Result<(), ManifestEditError> {
    let before = b"[package]\nname = \"demo\"\n\n[package.metadata]\nhello.world = \"a\"\ngoodbye = \"b\"\nhello.moon = \"c\"\n\n[lints.rust]\nunsafe_code = \"warn\"\n";
    let change = set(
        LintScope::Package,
        LintTool::Rust,
        "unsafe_code",
        LintLevel::Deny,
        None,
    )?;
    assert_eq!(
        TomlManifestEditor.apply(before, &change),
        Err(ManifestEditError::UnsupportedLayout)
    );

    let no_op = set(
        LintScope::Package,
        LintTool::Rust,
        "unsafe_code",
        LintLevel::Warn,
        Some(0),
    )?;
    assert_eq!(TomlManifestEditor.apply(before, &no_op)?, before);
    Ok(())
}

#[test]
fn crlf_multiline_strings_retain_source_newlines() -> Result<(), ManifestEditError> {
    let before = b"[package]\r\nname = \"demo\"\r\ndescription = \"\"\"first\r\nsecond\"\"\"\r\n\r\n[lints.rust]\r\nunsafe_code = \"warn\"\r\n";
    let after = TomlManifestEditor.apply(
        before,
        &set(
            LintScope::Package,
            LintTool::Rust,
            "unsafe_code",
            LintLevel::Deny,
            None,
        )?,
    )?;
    let text = std::str::from_utf8(&after).map_err(|_| ManifestEditError::InvalidManifest)?;
    assert!(text.contains("description = \"\"\"first\r\nsecond\"\"\"\r\n"));
    assert!(
        after
            .iter()
            .enumerate()
            .all(|(index, byte)| *byte != b'\n' || index > 0 && after[index - 1] == b'\r')
    );
    Ok(())
}

fn feature_name(value: &str) -> Result<FeatureName, ManifestEditError> {
    FeatureName::new(value.to_owned())
}

fn feature_value(value: &str) -> Result<FeatureValue, ManifestEditError> {
    FeatureValue::new(value.to_owned())
}

fn dependency_name(value: &str) -> Result<DependencyName, ManifestEditError> {
    DependencyName::new(value.to_owned())
}

fn dependency_spec(
    requirement: &str,
    package: Option<&str>,
    features: &[&str],
    optional: bool,
    default_features: bool,
) -> Result<DependencySpec, ManifestEditError> {
    Ok(DependencySpec {
        requirement: requirement.to_owned(),
        package: package.map(dependency_name).transpose()?,
        features: features
            .iter()
            .map(|feature| feature_name(feature))
            .collect::<Result<_, _>>()?,
        optional,
        default_features,
    })
}

#[test]
fn features_set_remove_and_no_op_preserve_surrounding_text() -> Result<(), ManifestEditError> {
    let before = b"# header\r\n[package]\r\nname = \"demo\"\r\n\r\n[features]\r\ndefault = [\"dep:serde\", \"serde/derive\"] # keep\r\nneighbor = [] # untouched\r\n";
    let no_op = ManifestEdit::FeatureSet {
        name: feature_name("default")?,
        values: vec![feature_value("dep:serde")?, feature_value("serde/derive")?],
    };
    assert_eq!(TomlManifestEditor.apply(before, &no_op)?, before);

    let changed = TomlManifestEditor.apply(
        before,
        &ManifestEdit::FeatureSet {
            name: feature_name("default")?,
            values: vec![feature_value("std")?, feature_value("serde?/derive")?],
        },
    )?;
    let changed = std::str::from_utf8(&changed).map_err(|_| ManifestEditError::InvalidManifest)?;
    assert!(changed.contains(
        "default = [\"std\", \"serde?/derive\"] # keep\r\nneighbor = [] # untouched\r\n"
    ));

    let removed = TomlManifestEditor.apply(
        changed.as_bytes(),
        &ManifestEdit::FeatureRemove {
            name: feature_name("default")?,
        },
    )?;
    let removed = std::str::from_utf8(&removed).map_err(|_| ManifestEditError::InvalidManifest)?;
    assert!(!removed.contains("default ="));
    assert!(removed.contains("neighbor = [] # untouched\r\n"));
    Ok(())
}

#[test]
fn feature_edits_reject_virtual_roots_bad_values_and_touched_inline_tables()
-> Result<(), ManifestEditError> {
    let set = ManifestEdit::FeatureSet {
        name: feature_name("default")?,
        values: vec![],
    };
    assert_eq!(
        TomlManifestEditor.apply(b"[workspace]\nmembers = []\n", &set),
        Err(ManifestEditError::InvalidOperation)
    );
    assert_eq!(
        TomlManifestEditor.apply(
            b"features = { default = [] }\n[package]\nname = \"demo\"\n",
            &set
        ),
        Err(ManifestEditError::UnsupportedLayout)
    );
    let duplicate = ManifestEdit::FeatureSet {
        name: feature_name("default")?,
        values: vec![feature_value("std")?, feature_value("std")?],
    };
    assert_eq!(
        TomlManifestEditor.apply(b"[package]\nname = \"demo\"\n", &duplicate),
        Err(ManifestEditError::InvalidOperation)
    );
    Ok(())
}

#[test]
fn profiles_write_closed_values_and_remove_only_one_setting() -> Result<(), ManifestEditError> {
    let mut manifest = b"[package]\nname = \"demo\"\n\n[profile.release]\nopt-level = 2 # keep comment\ndebug = false\nunknown-future = \"untouched\"\n".to_vec();
    let edits = [
        ProfileSettingEdit::OptLevel(ProfileOptLevel::SizeMin),
        ProfileSettingEdit::Debug(ProfileDebugInfo::LineTablesOnly),
        ProfileSettingEdit::Strip(ProfileStrip::Debuginfo),
        ProfileSettingEdit::DebugAssertions(false),
        ProfileSettingEdit::OverflowChecks(true),
        ProfileSettingEdit::Lto(ProfileLto::Thin),
        ProfileSettingEdit::Panic(ProfilePanic::Abort),
        ProfileSettingEdit::Incremental(false),
        ProfileSettingEdit::CodegenUnits(ProfileCodegenUnits::new(8)?),
    ];
    for setting in edits {
        manifest = TomlManifestEditor.apply(
            &manifest,
            &ManifestEdit::ProfileSet {
                profile: BuiltinProfile::Release,
                setting,
            },
        )?;
    }
    let text = std::str::from_utf8(&manifest).map_err(|_| ManifestEditError::InvalidManifest)?;
    for expected in [
        "opt-level = \"z\" # keep comment",
        "debug = \"line-tables-only\"",
        "strip = \"debuginfo\"",
        "debug-assertions = false",
        "overflow-checks = true",
        "lto = \"thin\"",
        "panic = \"abort\"",
        "incremental = false",
        "codegen-units = 8",
        "unknown-future = \"untouched\"",
    ] {
        assert!(text.contains(expected), "missing {expected:?} in {text}");
    }
    let removed = TomlManifestEditor.apply(
        &manifest,
        &ManifestEdit::ProfileRemove {
            profile: BuiltinProfile::Release,
            setting: ProfileSettingKey::Debug,
        },
    )?;
    let removed = std::str::from_utf8(&removed).map_err(|_| ManifestEditError::InvalidManifest)?;
    assert!(!removed.contains("debug ="));
    assert!(removed.contains("debug-assertions = false"));
    assert!(removed.contains("unknown-future = \"untouched\""));
    Ok(())
}

#[test]
fn profile_equivalent_spellings_are_exact_no_ops() -> Result<(), ManifestEditError> {
    let before = b"[profile.dev]\ndebug = true\nstrip = false\n";
    assert_eq!(
        TomlManifestEditor.apply(
            before,
            &ManifestEdit::ProfileSet {
                profile: BuiltinProfile::Dev,
                setting: ProfileSettingEdit::Debug(ProfileDebugInfo::Full),
            }
        )?,
        before
    );
    assert_eq!(
        TomlManifestEditor.apply(
            before,
            &ManifestEdit::ProfileSet {
                profile: BuiltinProfile::Dev,
                setting: ProfileSettingEdit::Strip(ProfileStrip::None),
            }
        )?,
        before
    );
    Ok(())
}

#[test]
fn workspace_dependency_set_is_explicit_and_rejects_optional() -> Result<(), ManifestEditError> {
    let before = b"[workspace]\nmembers = []\n\n[workspace.dependencies]\nserde = { git = \"https://example.invalid/serde\", rev = \"bad\" } # replace\nkeep = { path = \"keep\" }\n";
    let edit = ManifestEdit::WorkspaceDependencySet {
        name: dependency_name("serde")?,
        spec: dependency_spec("1", Some("serde"), &["derive"], false, false)?,
    };
    let after = TomlManifestEditor.apply(before, &edit)?;
    let text = std::str::from_utf8(&after).map_err(|_| ManifestEditError::InvalidManifest)?;
    assert!(text.contains(
        "serde = { version = \"1\", package = \"serde\", features = [\"derive\"], default-features = false } # replace"
    ));
    assert!(text.contains("keep = { path = \"keep\" }"));

    let optional = ManifestEdit::WorkspaceDependencySet {
        name: dependency_name("serde")?,
        spec: dependency_spec("1", None, &[], true, true)?,
    };
    assert_eq!(
        TomlManifestEditor.apply(&after, &optional),
        Err(ManifestEditError::InvalidOperation)
    );
    Ok(())
}

#[test]
fn workspace_dependency_semantic_no_op_and_remove_are_local() -> Result<(), ManifestEditError> {
    let before = b"[workspace]\nmembers = []\n\n[workspace.dependencies]\nserde = { default-features = true, features = [], version = \"^1\", optional = false } # exact\nother = \"2\"\n";
    let set = ManifestEdit::WorkspaceDependencySet {
        name: dependency_name("serde")?,
        spec: dependency_spec("1", None, &[], false, true)?,
    };
    assert_eq!(TomlManifestEditor.apply(before, &set)?, before);
    let removed = TomlManifestEditor.apply(
        before,
        &ManifestEdit::WorkspaceDependencyRemove {
            name: dependency_name("serde")?,
        },
    )?;
    let removed = std::str::from_utf8(&removed).map_err(|_| ManifestEditError::InvalidManifest)?;
    assert!(!removed.contains("serde ="));
    assert!(removed.contains("other = \"2\""));
    Ok(())
}

#[test]
fn dependency_add_handles_kind_target_alias_and_conflicts() -> Result<(), ManifestEditError> {
    let mut manifest = b"[package]\nname = \"demo\"\n".to_vec();
    manifest = TomlManifestEditor.apply(
        &manifest,
        &ManifestEdit::DependencyAdd {
            kind: DependencyKind::Dev,
            target: None,
            name: dependency_name("pretty_assertions")?,
            spec: dependency_spec("1", None, &[], false, true)?,
        },
    )?;
    manifest = TomlManifestEditor.apply(
        &manifest,
        &ManifestEdit::DependencyAdd {
            kind: DependencyKind::Build,
            target: Some(DependencyTarget::new("cfg(unix)".to_owned())?),
            name: dependency_name("system_api")?,
            spec: dependency_spec("2", Some("libc"), &["extra_traits"], true, false)?,
        },
    )?;
    let text = std::str::from_utf8(&manifest).map_err(|_| ManifestEditError::InvalidManifest)?;
    assert!(text.contains("[dev-dependencies]\npretty_assertions = \"1\""));
    assert!(
        text.contains("[target.\"cfg(unix)\".build-dependencies]"),
        "{text}"
    );
    assert!(text.contains("system_api = { version = \"2\", package = \"libc\", features = [\"extra_traits\"], optional = true, default-features = false }"));

    let conflict = ManifestEdit::DependencyAdd {
        kind: DependencyKind::Dev,
        target: None,
        name: dependency_name("pretty_assertions")?,
        spec: dependency_spec("2", None, &[], false, true)?,
    };
    assert_eq!(
        TomlManifestEditor.apply(&manifest, &conflict),
        Err(ManifestEditError::Conflict)
    );
    Ok(())
}

#[test]
fn dependency_add_no_op_accepts_string_inline_equivalence_but_not_unknown_keys()
-> Result<(), ManifestEditError> {
    let expected = ManifestEdit::DependencyAdd {
        kind: DependencyKind::Normal,
        target: None,
        name: dependency_name("serde")?,
        spec: dependency_spec("1", None, &[], false, true)?,
    };
    for before in [
        b"[package]\nname = \"demo\"\n[dependencies]\nserde = \"^1\" # string\n".as_slice(),
        b"[package]\nname = \"demo\"\n[dependencies]\nserde = { version = \"1\", optional = false, default-features = true, features = [] } # inline\n".as_slice(),
    ] {
        assert_eq!(TomlManifestEditor.apply(before, &expected)?, before);
    }
    let unknown = b"[package]\nname = \"demo\"\n[dependencies]\nserde = { version = \"1\", registry = \"private\" }\n";
    assert_eq!(
        TomlManifestEditor.apply(unknown, &expected),
        Err(ManifestEditError::Conflict)
    );
    Ok(())
}

#[test]
fn dependency_remove_drops_only_local_inheritance_key() -> Result<(), ManifestEditError> {
    let before = b"[package]\nname = \"demo\"\n[dependencies]\nserde = { workspace = true, features = [\"derive\"] }\nother = \"1\"\n[features]\ndefault = [\"serde/std\"]\n[workspace]\n[workspace.dependencies]\nserde = \"1\"\n";
    let after = TomlManifestEditor.apply(
        before,
        &ManifestEdit::DependencyRemove {
            kind: DependencyKind::Normal,
            target: None,
            name: dependency_name("serde")?,
        },
    )?;
    let text = std::str::from_utf8(&after).map_err(|_| ManifestEditError::InvalidManifest)?;
    assert!(!text.contains("serde = { workspace = true"));
    assert!(text.contains("other = \"1\""));
    assert!(text.contains("default = [\"serde/std\"]"));
    assert!(text.contains("[workspace.dependencies]\nserde = \"1\""));
    Ok(())
}

#[test]
fn dependency_operations_reject_virtual_and_unsupported_touched_layouts()
-> Result<(), ManifestEditError> {
    let add = ManifestEdit::DependencyAdd {
        kind: DependencyKind::Normal,
        target: None,
        name: dependency_name("serde")?,
        spec: dependency_spec("1", None, &[], false, true)?,
    };
    assert_eq!(
        TomlManifestEditor.apply(b"[workspace]\nmembers = []\n", &add),
        Err(ManifestEditError::InvalidOperation)
    );
    for before in [
        b"dependencies = { serde = \"1\" }\n[package]\nname = \"demo\"\n".as_slice(),
        b"[package]\nname = \"demo\"\n[dependencies.serde]\nversion = \"1\"\n".as_slice(),
    ] {
        assert_eq!(
            TomlManifestEditor.apply(before, &add),
            Err(ManifestEditError::UnsupportedLayout)
        );
    }
    Ok(())
}

#[test]
fn invalid_requirements_and_feature_limits_fail_before_editing() -> Result<(), ManifestEditError> {
    for requirement in ["", "not a req", &"1".repeat(129)] {
        let edit = ManifestEdit::DependencyAdd {
            kind: DependencyKind::Normal,
            target: None,
            name: dependency_name("serde")?,
            spec: dependency_spec(requirement, None, &[], false, true)?,
        };
        assert_eq!(
            TomlManifestEditor.apply(b"[package]\nname = \"demo\"\n", &edit),
            Err(ManifestEditError::InvalidOperation)
        );
    }
    let features = (0..129)
        .map(|index| feature_name(&format!("f{index}")))
        .collect::<Result<Vec<_>, _>>()?;
    let edit = ManifestEdit::DependencyAdd {
        kind: DependencyKind::Normal,
        target: None,
        name: dependency_name("serde")?,
        spec: DependencySpec {
            requirement: "1".to_owned(),
            package: None,
            features,
            optional: false,
            default_features: true,
        },
    };
    assert_eq!(
        TomlManifestEditor.apply(b"[package]\nname = \"demo\"\n", &edit),
        Err(ManifestEditError::InvalidOperation)
    );
    Ok(())
}

#[test]
fn new_semantic_no_ops_precede_unrelated_roundtrip_rejection() -> Result<(), ManifestEditError> {
    let feature_manifest = b"[package]\nname = \"demo\"\n\n[package.metadata]\nhello.world = \"a\"\ngoodbye = \"b\"\nhello.moon = \"c\"\n\n[features]\ndefault = [\"std\"]\n";
    assert_eq!(
        TomlManifestEditor.apply(
            feature_manifest,
            &ManifestEdit::FeatureSet {
                name: feature_name("default")?,
                values: vec![feature_value("std")?],
            }
        )?,
        feature_manifest
    );

    let dependency_manifest = b"[package]\nname = \"demo\"\n\n[package.metadata]\nhello.world = \"a\"\ngoodbye = \"b\"\nhello.moon = \"c\"\n\n[dependencies]\nserde = { version = \"1\", features = [] }\n";
    assert_eq!(
        TomlManifestEditor.apply(
            dependency_manifest,
            &ManifestEdit::DependencyAdd {
                kind: DependencyKind::Normal,
                target: None,
                name: dependency_name("serde")?,
                spec: dependency_spec("1", None, &[], false, true)?,
            }
        )?,
        dependency_manifest
    );
    Ok(())
}

#[test]
fn dependency_remove_preserves_neighbors_when_removing_complete_dotted_or_table_entry()
-> Result<(), ManifestEditError> {
    let edit = ManifestEdit::DependencyRemove {
        kind: DependencyKind::Build,
        target: Some(DependencyTarget::new("cfg(unix)".into())?),
        name: dependency_name("renamed")?,
    };
    let prefix = "[package]\nname = \"demo\"\n[target.'cfg(unix)'.build-dependencies]\n";
    for (entry, suffix) in [
        (
            "renamed.workspace = true # remove inheritance\n",
            "other = \"1\" # retain neighbor\n",
        ),
        (
            "[target.'cfg(unix)'.build-dependencies.renamed]\nworkspace = true # remove inheritance\n",
            "[features]\ndefault = [] # retain neighbor\n",
        ),
    ] {
        for newline in ["\n", "\r\n"] {
            let before = format!("{prefix}{entry}{suffix}").replace('\n', newline);
            let expected = format!("{prefix}{suffix}").replace('\n', newline);
            let after = TomlManifestEditor.apply(before.as_bytes(), &edit)?;
            assert_eq!(after, expected.as_bytes());
            assert_eq!(TomlManifestEditor.apply(&after, &edit)?, after);
        }
    }
    Ok(())
}
