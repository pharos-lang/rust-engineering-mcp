use rust_engineering_application::ManifestEditor;
use rust_engineering_domain::{
    LintLevel, LintName, LintScope, LintTool, ManifestEdit, ManifestEditError,
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
