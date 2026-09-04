//! Bounded structural Cargo-manifest validation. This is not dependency resolution.
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use rust_engineering_application::{OperationControl, ProjectError};
use rust_engineering_domain::OperationalErrorCode;
use serde::Deserialize;

use crate::{ManifestGraph, ManifestIo};

fn invalid() -> ProjectError {
    ProjectError::Rejected(OperationalErrorCode::InvalidProject)
}

type Table = BTreeMap<String, toml::Value>;
type Dependencies = BTreeMap<String, Dependency>;

#[derive(Clone, Deserialize)]
#[serde(untagged)]
enum Dependency {
    Version(String),
    Detail(Box<DependencyDetail>),
}
#[derive(Clone, Default, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct DependencyDetail {
    version: Option<String>,
    path: Option<String>,
    workspace: Option<bool>,
    git: Option<String>,
    branch: Option<String>,
    tag: Option<String>,
    rev: Option<String>,
    registry: Option<String>,
    registry_index: Option<String>,
    package: Option<String>,
    optional: Option<bool>,
    default_features: Option<bool>,
    features: Option<Vec<String>>,
    // Older Cargo spelling remains accepted but is not interpreted as authority.
    #[serde(rename = "default_features")]
    legacy_default_features: Option<bool>,
    public: Option<bool>,
}
#[derive(Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct Groups {
    #[serde(default)]
    dependencies: Dependencies,
    #[serde(default)]
    build_dependencies: Dependencies,
    #[serde(default)]
    dev_dependencies: Dependencies,
    #[serde(rename = "build_dependencies")]
    legacy_build_dependencies: Option<toml::Value>,
    #[serde(rename = "dev_dependencies")]
    legacy_dev_dependencies: Option<toml::Value>,
}
#[derive(Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct Workspace {
    #[serde(default)]
    members: Vec<String>,
    #[serde(default)]
    exclude: Vec<String>,
    default_members: Option<Vec<String>>,
    #[serde(default)]
    package: Table,
    #[serde(default)]
    dependencies: Dependencies,
    lints: Option<Table>,
    resolver: Option<String>,
}
#[derive(Deserialize)]
struct Target {
    path: Option<String>,
    name: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct Manifest {
    package: Option<Table>,
    project: Option<toml::Value>,
    workspace: Option<Workspace>,
    #[serde(flatten)]
    groups: Groups,
    #[serde(default)]
    target: BTreeMap<String, Groups>,
    #[serde(default)]
    patch: BTreeMap<String, Dependencies>,
    replace: Option<toml::Value>,
    cargo_features: Option<toml::Value>,
    lib: Option<Target>,
    #[serde(default)]
    bin: Vec<Target>,
    #[serde(default)]
    example: Vec<Target>,
    #[serde(default)]
    test: Vec<Target>,
    #[serde(default)]
    bench: Vec<Target>,
    lints: Option<Table>,
}

/// Normalize only paths whose parent traversals occur before normal components.
fn joined(base: &Path, value: &str, member: bool) -> Result<PathBuf, ProjectError> {
    if value.is_empty() || value.len() > 4096 || value.contains(['\0', '\\', '*', '?', '[', ']']) {
        return Err(invalid());
    }
    let path = Path::new(value);
    if member && path.is_absolute() {
        return Err(invalid());
    }
    let mut result = if path.is_absolute() {
        PathBuf::new()
    } else {
        base.to_path_buf()
    };
    let mut normal = false;
    for component in path.components() {
        match component {
            Component::RootDir => result.push(component),
            Component::Normal(part) => {
                normal = true;
                result.push(part);
            }
            Component::CurDir => {}
            Component::ParentDir if !member && !normal => {
                if !result.pop() {
                    return Err(invalid());
                }
            }
            _ => return Err(invalid()),
        }
    }
    Ok(result)
}

struct Validator<'a> {
    io: &'a dyn ManifestIo,
    control: &'a dyn OperationControl,
    root: &'a Path,
    bytes: usize,
    edges: usize,
    manifests: BTreeMap<PathBuf, Vec<u8>>,
    names: BTreeMap<String, PathBuf>,
}
impl Validator<'_> {
    fn read(&mut self, directory: &Path) -> Result<Manifest, ProjectError> {
        self.control.check()?;
        if self.manifests.len() >= 128 {
            return Err(invalid());
        }
        let path = directory.join("Cargo.toml");
        let bytes = self.io.read_file(&path)?.ok_or_else(invalid)?;
        if bytes.len() > 256 * 1024 {
            return Err(invalid());
        }
        self.bytes = self.bytes.checked_add(bytes.len()).ok_or_else(invalid)?;
        if self.bytes > 4 * 1024 * 1024 {
            return Err(invalid());
        }
        let input = std::str::from_utf8(&bytes).map_err(|_| invalid())?;
        let manifest: Manifest = toml::from_str(input).map_err(|_| invalid())?;
        // Cargo's deprecated aliases differ by edition and may hide path
        // dependencies if ignored. They are outside this structural subset.
        if manifest.replace.is_some()
            || manifest.cargo_features.is_some()
            || manifest.project.is_some()
            || std::iter::once(&manifest.groups)
                .chain(manifest.target.values())
                .any(|group| {
                    group.legacy_build_dependencies.is_some()
                        || group.legacy_dev_dependencies.is_some()
                })
        {
            return Err(invalid());
        }
        self.manifests.insert(path, bytes);
        Ok(manifest)
    }
    fn package(
        &mut self,
        manifest: &Manifest,
        dir: &Path,
        workspace: Option<&Workspace>,
        member: bool,
    ) -> Result<(), ProjectError> {
        self.control.check()?;
        let package = manifest.package.as_ref().ok_or_else(invalid)?;
        let name = package
            .get("name")
            .and_then(toml::Value::as_str)
            .ok_or_else(invalid)?;
        if name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        {
            return Err(invalid());
        }
        if member
            && self
                .names
                .insert(name.to_owned(), dir.to_path_buf())
                .is_some_and(|previous| previous != dir)
        {
            return Err(invalid());
        }
        if let Some(owner) = package.get("workspace") {
            if manifest.workspace.is_some()
                || joined(dir, owner.as_str().ok_or_else(invalid)?, false)? != self.root
            {
                return Err(invalid());
            }
            if !member {
                return Err(invalid());
            }
        }
        for (key, value) in package {
            if key == "metadata" {
                if !value.is_table() {
                    return Err(invalid());
                }
                continue;
            }
            let resolved = if let Some(table) = value.as_table() {
                if table.len() != 1
                    || table.get("workspace").and_then(toml::Value::as_bool) != Some(true)
                {
                    return Err(invalid());
                }
                if !member
                    || !matches!(
                        key.as_str(),
                        "authors"
                            | "categories"
                            | "description"
                            | "documentation"
                            | "edition"
                            | "exclude"
                            | "homepage"
                            | "include"
                            | "keywords"
                            | "license"
                            | "license-file"
                            | "publish"
                            | "readme"
                            | "repository"
                            | "rust-version"
                            | "version"
                    )
                {
                    return Err(invalid());
                }
                workspace
                    .and_then(|w| w.package.get(key))
                    .ok_or_else(invalid)?
            } else {
                value
            };
            match key.as_str() {
                "version" => {
                    semver::Version::parse(resolved.as_str().ok_or_else(invalid)?)
                        .map_err(|_| invalid())?;
                }
                "edition" => {
                    if !matches!(resolved.as_str(), Some("2015" | "2018" | "2021" | "2024")) {
                        return Err(invalid());
                    }
                }
                "rust-version" => {
                    let version = resolved.as_str().ok_or_else(invalid)?;
                    let parts: Vec<_> = version.split('.').collect();
                    if !(2..=3).contains(&parts.len())
                        || parts.iter().any(|p| {
                            p.is_empty()
                                || !p.bytes().all(|c| c.is_ascii_digit())
                                || (p.len() > 1 && p.starts_with('0'))
                        })
                    {
                        return Err(invalid());
                    }
                }
                "name" | "workspace" | "description" | "documentation" | "homepage" | "license"
                | "license-file" | "repository" | "links" | "default-run" => {
                    if resolved.as_str().is_none() {
                        return Err(invalid());
                    }
                }
                "authors" | "categories" | "exclude" | "include" | "keywords" => {
                    if !resolved
                        .as_array()
                        .is_some_and(|a| a.iter().all(toml::Value::is_str))
                    {
                        return Err(invalid());
                    }
                }
                "readme" => {
                    if !resolved.is_str() && !resolved.is_bool() {
                        return Err(invalid());
                    }
                }
                "publish" => {
                    if !resolved.is_bool()
                        && !resolved
                            .as_array()
                            .is_some_and(|a| a.iter().all(toml::Value::is_str))
                    {
                        return Err(invalid());
                    }
                }
                "resolver" => {
                    if !matches!(resolved.as_str(), Some("1" | "2" | "3")) {
                        return Err(invalid());
                    }
                }
                "autolib" | "autobins" | "autoexamples" | "autotests" | "autobenches"
                    if resolved.as_bool().is_none() =>
                {
                    return Err(invalid());
                }
                _ => {}
            }
        }
        if let Some(lints) = &manifest.lints
            && let Some(inherit) = lints.get("workspace")
            && (inherit.as_bool() != Some(true)
                || lints.len() != 1
                || !member
                || workspace.and_then(|w| w.lints.as_ref()).is_none())
        {
            return Err(invalid());
        }
        let mut found = false;
        if let Some(lib) = &manifest.lib {
            found |= self.target(dir, lib, Some("src/lib.rs"))?;
        } else if package.get("autolib").and_then(toml::Value::as_bool) != Some(false) {
            found |= self.io.is_file(&dir.join("src/lib.rs"))?;
        }
        for target in &manifest.bin {
            found |= self.target(dir, target, None)?;
        }
        if manifest.bin.is_empty()
            && package.get("autobins").and_then(toml::Value::as_bool) != Some(false)
        {
            found |= self.io.is_file(&dir.join("src/main.rs"))?;
        }
        for target in manifest
            .example
            .iter()
            .chain(&manifest.test)
            .chain(&manifest.bench)
        {
            self.target(dir, target, None)?;
        }
        if !found {
            return Err(invalid());
        }
        if let Some(build) = package.get("build") {
            match build {
                toml::Value::Boolean(_) => {}
                toml::Value::String(path) => {
                    if !self.io.is_file(&joined(dir, path, false)?)? {
                        return Err(invalid());
                    }
                }
                _ => return Err(invalid()),
            }
        }
        Ok(())
    }
    fn target(
        &self,
        dir: &Path,
        target: &Target,
        fallback: Option<&str>,
    ) -> Result<bool, ProjectError> {
        self.control.check()?;
        if target.name.as_ref().is_some_and(|n| n.is_empty()) {
            return Err(invalid());
        }
        let path = target.path.as_deref().or(fallback).ok_or_else(invalid)?;
        if !self.io.is_file(&joined(dir, path, false)?)? {
            return Err(invalid());
        }
        Ok(true)
    }
    fn dependencies(
        &mut self,
        dependencies: &Dependencies,
        base: &Path,
        workspace: Option<&Workspace>,
        member: bool,
        paths: &mut BTreeSet<PathBuf>,
    ) -> Result<(), ProjectError> {
        for (name, dependency) in dependencies {
            self.control.check()?;
            self.edges += 1;
            if self.edges > 512 || name.is_empty() {
                return Err(invalid());
            }
            match dependency {
                Dependency::Version(version) => {
                    semver::VersionReq::parse(version).map_err(|_| invalid())?;
                }
                Dependency::Detail(detail) => {
                    // Read each known field: unknown dependency source keys are rejected by Serde.
                    let _ = (
                        &detail.registry,
                        &detail.registry_index,
                        &detail.package,
                        &detail.optional,
                        &detail.default_features,
                        &detail.features,
                        &detail.legacy_default_features,
                        &detail.public,
                    );
                    if let Some(version) = &detail.version {
                        semver::VersionReq::parse(version).map_err(|_| invalid())?;
                    }
                    if detail.workspace.is_some() {
                        if detail.workspace != Some(true)
                            || !member
                            || detail.path.is_some()
                            || detail.version.is_some()
                            || detail.git.is_some()
                            || detail.registry.is_some()
                            || detail.registry_index.is_some()
                            || detail.package.is_some()
                            || detail.branch.is_some()
                            || detail.tag.is_some()
                            || detail.rev.is_some()
                        {
                            return Err(invalid());
                        }
                        let inherited = workspace
                            .and_then(|w| w.dependencies.get(name))
                            .ok_or_else(invalid)?;
                        if matches!(inherited, Dependency::Detail(d) if d.workspace.is_some() || d.optional == Some(true))
                        {
                            return Err(invalid());
                        }
                        let one = BTreeMap::from([(name.clone(), inherited.clone())]);
                        self.dependencies(&one, self.root, None, false, paths)?;
                    } else {
                        if detail.path.is_some() && detail.git.is_some()
                            || [
                                detail.branch.is_some(),
                                detail.tag.is_some(),
                                detail.rev.is_some(),
                            ]
                            .into_iter()
                            .filter(|v| *v)
                            .count()
                                > 1
                            || (detail.git.is_none()
                                && (detail.branch.is_some()
                                    || detail.tag.is_some()
                                    || detail.rev.is_some()))
                        {
                            return Err(invalid());
                        }
                        if let Some(path) = &detail.path {
                            paths.insert(joined(base, path, false)?);
                        } else if detail.version.is_none() && detail.git.is_none() {
                            return Err(invalid());
                        }
                    }
                }
            }
        }
        Ok(())
    }
    fn paths(
        &mut self,
        manifest: &Manifest,
        dir: &Path,
        workspace: Option<&Workspace>,
        member: bool,
    ) -> Result<BTreeSet<PathBuf>, ProjectError> {
        let mut paths = BTreeSet::new();
        for group in std::iter::once(&manifest.groups).chain(manifest.target.values()) {
            for deps in [
                &group.dependencies,
                &group.build_dependencies,
                &group.dev_dependencies,
            ] {
                self.dependencies(deps, dir, workspace, member, &mut paths)?;
            }
        }
        for deps in manifest.patch.values() {
            self.dependencies(deps, dir, workspace, member, &mut paths)?;
        }
        Ok(paths)
    }
}

pub(crate) fn validate(
    io: &dyn ManifestIo,
    root: &Path,
    control: &dyn OperationControl,
) -> Result<ManifestGraph, ProjectError> {
    let mut validator = Validator {
        io,
        control,
        root,
        bytes: 0,
        edges: 0,
        manifests: BTreeMap::new(),
        names: BTreeMap::new(),
    };
    let first = validator.read(root)?;
    if first.package.is_none() && first.workspace.is_none() {
        return Err(invalid());
    }
    let workspace = first.workspace.as_ref();
    let mut members = BTreeSet::new();
    let mut excluded = BTreeSet::new();
    if first.package.is_some() {
        members.insert(root.to_path_buf());
    }
    if let Some(w) = workspace {
        if w.resolver
            .as_ref()
            .is_some_and(|r| !matches!(r.as_str(), "1" | "2" | "3"))
        {
            return Err(invalid());
        }
        for path in &w.members {
            control.check()?;
            members.insert(joined(root, path, true)?);
        }
        for path in &w.exclude {
            control.check()?;
            excluded.insert(joined(root, path, true)?);
        }
        if members
            .iter()
            .any(|p| excluded.iter().any(|e| p.starts_with(e)))
        {
            return Err(invalid());
        }
        if members.is_empty() {
            return Err(invalid());
        }
        if w.dependencies.values().any(|d| matches!(d, Dependency::Detail(v) if v.optional == Some(true) || v.workspace.is_some())) { return Err(invalid()); }
    }
    if first.package.is_some() {
        validator.package(&first, root, workspace, true)?;
    } else if first.lib.is_some()
        || !first.bin.is_empty()
        || !first.example.is_empty()
        || !first.test.is_empty()
        || !first.bench.is_empty()
        || !first.groups.dependencies.is_empty()
        || !first.groups.dev_dependencies.is_empty()
        || !first.groups.build_dependencies.is_empty()
        || !first.target.is_empty()
    {
        return Err(invalid());
    }
    let mut paths = validator.paths(&first, root, workspace, true)?;
    if let Some(w) = workspace {
        validator.dependencies(&w.dependencies, root, None, false, &mut paths)?;
    }
    let mut pending: Vec<_> = members
        .iter()
        .filter(|p| p.as_path() != root)
        .cloned()
        .chain(paths)
        .map(|p| (p, 1_usize))
        .collect();
    while let Some((directory, depth)) = pending.pop() {
        control.check()?;
        if depth > 32 {
            return Err(invalid());
        }
        if validator
            .manifests
            .contains_key(&directory.join("Cargo.toml"))
        {
            continue;
        }
        let manifest = validator.read(&directory)?;
        let member = workspace.is_some()
            && directory.starts_with(root)
            && !excluded.iter().any(|e| directory.starts_with(e));
        if member {
            members.insert(directory.clone());
        }
        // Independent dependency workspaces and ownership discovery are intentionally unsupported.
        if manifest.workspace.is_some() {
            return Err(invalid());
        }
        validator.package(&manifest, &directory, workspace, member)?;
        for path in validator.paths(&manifest, &directory, workspace, member)? {
            pending.push((path, depth + 1));
        }
    }
    if let Some(defaults) = workspace.and_then(|w| w.default_members.as_ref()) {
        for path in defaults {
            control.check()?;
            if !members.contains(&joined(root, path, true)?) {
                return Err(invalid());
            }
        }
    }
    control.check()?;
    Ok(ManifestGraph {
        manifests: validator.manifests.into_iter().collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    struct Io {
        files: BTreeMap<PathBuf, Vec<u8>>,
        reads: RefCell<Vec<PathBuf>>,
        denied: Option<PathBuf>,
    }
    impl Io {
        fn new(files: &[(&str, &str)]) -> Self {
            Self {
                files: files
                    .iter()
                    .map(|(p, s)| (PathBuf::from(p), s.as_bytes().to_vec()))
                    .collect(),
                reads: RefCell::new(Vec::new()),
                denied: None,
            }
        }
    }
    impl ManifestIo for Io {
        fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>, ProjectError> {
            self.reads.borrow_mut().push(path.to_path_buf());
            if self.denied.as_ref().is_some_and(|p| path.starts_with(p)) {
                return Err(ProjectError::Rejected(OperationalErrorCode::SandboxDenied));
            }
            Ok(self.files.get(path).cloned())
        }
        fn is_file(&self, path: &Path) -> Result<bool, ProjectError> {
            if self.denied.as_ref().is_some_and(|p| path.starts_with(p)) {
                return Err(ProjectError::Rejected(OperationalErrorCode::SandboxDenied));
            }
            Ok(self.files.contains_key(path))
        }
    }
    struct Control;
    impl OperationControl for Control {
        fn check(&self) -> Result<(), ProjectError> {
            Ok(())
        }
    }
    fn check(io: &Io) -> Result<ManifestGraph, ProjectError> {
        validate(io, Path::new("/root"), &Control)
    }
    #[test]
    fn accepts_real_package_and_retains_exact_bytes() -> Result<(), ProjectError> {
        let source = "[package]\nname='hello'\n[package.metadata]\nanything={ nested = true }\n";
        let io = Io::new(&[
            ("/root/Cargo.toml", source),
            ("/root/src/main.rs", "fn main(){}"),
        ]);
        let graph = check(&io)?;
        assert_eq!(
            graph.manifests,
            vec![(
                PathBuf::from("/root/Cargo.toml"),
                source.as_bytes().to_vec()
            )]
        );
        Ok(())
    }
    #[test]
    fn rejects_parseable_nonprojects_and_invalid_versions() {
        for source in [
            "answer=42",
            "[package]\nname=''",
            "[package]\nname='x'\nversion='bad'",
            "[package]\nname='x'\nedition='2099'",
            "[workspace]\nmembers=[]",
            "[package]\nname='x'\nversion.workspace=true",
        ] {
            let io = Io::new(&[("/root/Cargo.toml", source), ("/root/src/main.rs", "")]);
            assert!(matches!(check(&io), Err(e) if e == invalid()), "{source}");
        }
        assert!(check(&Io::new(&[("/root/Cargo.toml", "[package]\nname='x'")])).is_err());
    }
    #[test]
    fn resolves_workspace_inheritance_and_path_closure() -> Result<(), ProjectError> {
        let io = Io::new(&[
            (
                "/root/Cargo.toml",
                "[workspace]\nmembers=['a']\ndefault-members=['a']\n[workspace.package]\nversion='1.2.3'\nedition='2024'\n[workspace.dependencies]\nb={path='b'}",
            ),
            (
                "/root/a/Cargo.toml",
                "[package]\nname='a'\nversion.workspace=true\nedition.workspace=true\n[dependencies]\nb.workspace=true",
            ),
            ("/root/a/src/lib.rs", ""),
            (
                "/root/b/Cargo.toml",
                "[package]\nname='b'\n[dependencies]\na={path='../a'}",
            ),
            ("/root/b/src/lib.rs", ""),
        ]);
        let graph = check(&io)?;
        assert_eq!(graph.manifests.len(), 3);
        assert_eq!(io.reads.borrow().len(), 3);
        Ok(())
    }
    #[test]
    fn rejects_unsupported_members_before_reading_their_paths() {
        for member in [
            "../escape",
            "/elsewhere",
            "a/../escape",
            "crates/*",
            "crates/[ab]",
        ] {
            let source = format!("[workspace]\nmembers=['{member}']");
            let io = Io::new(&[("/root/Cargo.toml", &source)]);
            assert!(check(&io).is_err());
            assert_eq!(io.reads.borrow().len(), 1);
        }
    }
    #[test]
    fn preserves_authorization_failure_for_conditional_external_dependency() {
        let mut io = Io::new(&[
            (
                "/root/Cargo.toml",
                "[package]\nname='x'\n[target.'cfg(windows)'.build-dependencies]\nexternal={path='../external'}",
            ),
            ("/root/src/lib.rs", ""),
        ]);
        io.denied = Some(PathBuf::from("/external"));
        assert!(matches!(
            check(&io),
            Err(ProjectError::Rejected(OperationalErrorCode::SandboxDenied))
        ));
    }
    #[test]
    fn accepts_authorized_external_dependency_without_inheritance() -> Result<(), ProjectError> {
        let io = Io::new(&[
            (
                "/root/Cargo.toml",
                "[package]\nname='x'\n[dev-dependencies]\nexternal={path='../external'}",
            ),
            ("/root/src/lib.rs", ""),
            ("/external/Cargo.toml", "[package]\nname='external'"),
            ("/external/src/lib.rs", ""),
        ]);
        assert_eq!(check(&io)?.manifests.len(), 2);
        Ok(())
    }
    #[test]
    fn rejects_unresolved_inheritance_sources_and_nightly_features() {
        for tail in [
            "[dependencies]\nx.workspace=true",
            "[replace]\n'x:1.0.0'={path='x'}",
            "[dependencies]\nx={path='x', git='https://invalid'}",
            "[dependencies]\nx={mystery-path='/external'}",
        ] {
            let source = format!("[package]\nname='x'\n{tail}");
            assert!(
                check(&Io::new(&[
                    ("/root/Cargo.toml", &source),
                    ("/root/src/main.rs", "")
                ]))
                .is_err()
            );
        }
        assert!(
            check(&Io::new(&[
                ("/root/Cargo.toml", "cargo-features=[]\n[package]\nname='x'"),
                ("/root/src/main.rs", "")
            ]))
            .is_err()
        );
    }
    #[test]
    fn checks_cancellation_and_per_manifest_budget() {
        struct Cancel;
        impl OperationControl for Cancel {
            fn check(&self) -> Result<(), ProjectError> {
                Err(ProjectError::Cancelled)
            }
        }
        let io = Io::new(&[]);
        assert!(matches!(
            validate(&io, Path::new("/root"), &Cancel),
            Err(ProjectError::Cancelled)
        ));
        assert!(io.reads.borrow().is_empty());
        let large = "#".repeat(256 * 1024 + 1);
        assert!(check(&Io::new(&[("/root/Cargo.toml", &large)])).is_err());
    }
    #[test]
    fn rejects_wrong_default_members_duplicate_names_and_missing_targets() {
        for root in [
            "[workspace]\nmembers=['a']\ndefault-members=['missing']",
            "[workspace]\nmembers=['a', 'b']",
        ] {
            let io = Io::new(&[
                ("/root/Cargo.toml", root),
                ("/root/a/Cargo.toml", "[package]\nname='same'"),
                ("/root/b/Cargo.toml", "[package]\nname='same'"),
                ("/root/a/src/lib.rs", ""),
                ("/root/b/src/lib.rs", ""),
            ]);
            assert!(check(&io).is_err());
        }
        let io = Io::new(&[
            (
                "/root/Cargo.toml",
                "[package]\nname='x'\n[[bin]]\nname='missing'\npath='missing.rs'",
            ),
            ("/root/src/main.rs", ""),
        ]);
        assert!(check(&io).is_err());
    }
    #[test]
    fn bounds_dependency_depth_and_edges() {
        let mut io = Io::new(&[]);
        for n in 0..35 {
            let dir = if n == 0 {
                "/root".to_owned()
            } else {
                format!("/root/p{n}")
            };
            let next = if n == 0 {
                "p1".to_owned()
            } else {
                format!("../p{}", n + 1)
            };
            io.files.insert(
                PathBuf::from(format!("{dir}/Cargo.toml")),
                format!("[package]\nname='p{n}'\n[dependencies]\nnext={{path='{next}'}}")
                    .into_bytes(),
            );
            io.files
                .insert(PathBuf::from(format!("{dir}/src/lib.rs")), Vec::new());
        }
        assert!(check(&io).is_err());
        let mut source = "[package]\nname='p'\n[dependencies]\n".to_owned();
        for n in 0..513 {
            source.push_str(&format!("p{n}='1'\n"));
        }
        assert!(
            check(&Io::new(&[
                ("/root/Cargo.toml", &source),
                ("/root/src/lib.rs", "")
            ]))
            .is_err()
        );
    }
    #[test]
    fn legacy_groups_cannot_hide_dependency_paths() {
        for section in [
            "dev_dependencies",
            "build_dependencies",
            "target.'cfg(unix)'.dev_dependencies",
            "target.'cfg(windows)'.build_dependencies",
        ] {
            let source = format!(
                "[package]\nname='x'\nedition='2021'\n[{section}]\nx={{path='../outside'}}\n"
            );
            let io = Io::new(&[("/root/Cargo.toml", &source), ("/root/src/lib.rs", "")]);
            assert!(matches!(check(&io), Err(e) if e == invalid()));
            assert_eq!(io.reads.borrow().len(), 1);
        }
        let io = Io::new(&[(
            "/root/Cargo.toml",
            "[workspace]\nmembers=['a']\n[project]\nname='alias'\n",
        )]);
        assert!(matches!(check(&io), Err(e) if e == invalid()));
    }

    #[test]
    fn excluded_subtrees_are_not_automatic_or_default_members() {
        let source =
            "[workspace]\nmembers=['app']\nexclude=['vendor']\ndefault-members=['vendor/sub']\n";
        let io = Io::new(&[
            ("/root/Cargo.toml", source),
            (
                "/root/app/Cargo.toml",
                "[package]\nname='app'\n[dependencies]\nsub={path='../vendor/sub'}\n",
            ),
            ("/root/app/src/lib.rs", ""),
            ("/root/vendor/sub/Cargo.toml", "[package]\nname='sub'\n"),
            ("/root/vendor/sub/src/lib.rs", ""),
        ]);
        assert!(matches!(check(&io), Err(e) if e == invalid()));
        // Even excluded dependencies must be included in the authorization walk.
        assert!(
            io.reads
                .borrow()
                .contains(&PathBuf::from("/root/vendor/sub/Cargo.toml"))
        );
        let mut io = io;
        io.files.insert(
            PathBuf::from("/root/Cargo.toml"),
            source.replace("exclude=['vendor']\n", "").into_bytes(),
        );
        assert!(check(&io).is_ok());
    }
}
