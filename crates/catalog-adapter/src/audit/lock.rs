//! Owned Cargo.lock v4 data. No filesystem, Cargo execution, or network access.
use cargo_lock::{Lockfile, SourceId};
use rust_engineering_application::{InspectionControl, ProjectError};
use rust_engineering_domain::{
    AuditDataError, AuditPackage, AuditPath, AuditSource, OperationalErrorCode, ProjectStructure,
    SourceBundle, SourceFingerprint,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

const MAX_BYTES: usize = 1024 * 1024;
const MAX_PACKAGES: usize = 1024;
const MAX_EDGES: usize = 8192;
const MAX_NAME: usize = 128;
const MAX_VERSION: usize = 128;
const MAX_SOURCE: usize = 2048;
const MAX_DEPENDENCY: usize = MAX_NAME + MAX_VERSION + MAX_SOURCE + 4;
const MAX_PATHS: usize = 8;
const MAX_PATH_PACKAGES: usize = 32;

pub(super) struct LockGraph {
    pub lock: Lockfile,
    pub packages: Vec<AuditPackage>,
    pub roots: Vec<usize>,
    pub edges: Vec<Vec<usize>>,
    pub fingerprint: SourceFingerprint,
    pub unsupported: Vec<AuditPackage>,
    pub scanned_indices: Vec<usize>,
}

// The library accepts legacy/unknown fields and ambiguous abbreviated edges.
// A closed v4 schema must be validated before invoking its resolver.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLock {
    version: u32,
    package: Vec<RawPackage>,
    #[serde(default)]
    metadata: EmptyTable,
    #[serde(default)]
    patch: RawPatch,
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyTable {}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPatch {
    #[serde(default)]
    unused: Vec<RawPackage>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPackage {
    name: String,
    version: String,
    source: Option<String>,
    checksum: Option<String>,
    #[serde(default)]
    dependencies: Vec<String>,
}
struct Identity {
    name: String,
    version: semver::Version,
    source: Option<SourceId>,
}
struct RawDependency {
    name: String,
    version: Option<semver::Version>,
    source: Option<SourceId>,
}

fn checkpoint(control: &dyn InspectionControl) -> Result<(), AuditDataError> {
    control.check().map_err(|error| match error {
        ProjectError::Cancelled => AuditDataError::Cancelled,
        ProjectError::Rejected(OperationalErrorCode::CommandTimeout) => AuditDataError::Timeout,
        _ => AuditDataError::Internal,
    })
}

fn digest(bytes: &[u8]) -> Result<SourceFingerprint, AuditDataError> {
    let hex: String = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("sha256:{hex}")
        .parse()
        .map_err(|_| AuditDataError::Internal)
}

fn valid_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    name.len() <= MAX_NAME
        && bytes
            .next()
            .is_some_and(|b| b == b'_' || b.is_ascii_alphabetic())
        && bytes.all(|b| b == b'-' || b == b'_' || b.is_ascii_alphanumeric())
}

fn version(value: &str) -> Result<semver::Version, AuditDataError> {
    if value.is_empty() || value.len() > MAX_VERSION {
        return Err(AuditDataError::InvalidLockfile);
    }
    value.parse().map_err(|_| AuditDataError::InvalidLockfile)
}

fn source_id(value: &str) -> Result<SourceId, AuditDataError> {
    if value.is_empty()
        || value.len() > MAX_SOURCE
        || value.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(AuditDataError::InvalidLockfile);
    }
    value.parse().map_err(|_| AuditDataError::InvalidLockfile)
}

// SourceId's Ord/Eq deliberately relax Git reference/revision comparison.
// Audit identities must retain those facts rather than inherit first-match loss.
fn same_source(left: &SourceId, right: &SourceId) -> bool {
    left.kind() == right.kind() && left.url() == right.url() && left.precise() == right.precise()
}
fn same_optional_source(left: Option<&SourceId>, right: Option<&SourceId>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => same_source(left, right),
        _ => false,
    }
}

impl RawDependency {
    fn parse(value: &str) -> Result<Self, AuditDataError> {
        if value.is_empty() || value.len() > MAX_DEPENDENCY {
            return Err(AuditDataError::InvalidLockfile);
        }
        // Cargo emits a single ASCII space between components. Disallow hidden
        // whitespace/control characters and extra components in this subset.
        let parts: Vec<_> = value.split(' ').collect();
        if parts.len() > 3 || parts.iter().any(|p| p.is_empty()) || !valid_name(parts[0]) {
            return Err(AuditDataError::InvalidLockfile);
        }
        let version = parts.get(1).map(|v| version(v)).transpose()?;
        let source = parts
            .get(2)
            .map(|value| {
                let inner = value
                    .strip_prefix('(')
                    .and_then(|v| v.strip_suffix(')'))
                    .ok_or(AuditDataError::InvalidLockfile)?;
                source_id(inner)
            })
            .transpose()?;
        Ok(Self {
            name: parts[0].to_owned(),
            version,
            source,
        })
    }
    fn matches(&self, identity: &Identity) -> bool {
        self.name == identity.name
            && self.version.as_ref().is_none_or(|v| v == &identity.version)
            && self.source.as_ref().is_none_or(|source| {
                identity
                    .source
                    .as_ref()
                    .is_some_and(|candidate| same_source(source, candidate))
            })
    }
}

pub(super) fn parse(
    source: &SourceBundle,
    structure: &ProjectStructure,
    control: &dyn InspectionControl,
) -> Result<LockGraph, AuditDataError> {
    checkpoint(control)?;
    let bytes = source
        .files()
        .iter()
        .find(|f| f.path() == "Cargo.lock")
        .ok_or(AuditDataError::MissingLockfile)?
        .bytes();
    if bytes.len() > MAX_BYTES {
        return Err(AuditDataError::Budget);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| AuditDataError::InvalidLockfile)?;
    let raw: RawLock = toml::from_str(text).map_err(|_| AuditDataError::InvalidLockfile)?;
    checkpoint(control)?;
    let _ = &raw.metadata;
    if raw.version != 4 || !raw.patch.unused.is_empty() || raw.package.is_empty() {
        return Err(AuditDataError::InvalidLockfile);
    }
    if raw.package.len() > MAX_PACKAGES {
        return Err(AuditDataError::Budget);
    }
    let mut identities: Vec<Identity> = Vec::with_capacity(raw.package.len());
    let mut by_name: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    let mut edge_count = 0usize;
    for (index, package) in raw.package.iter().enumerate() {
        checkpoint(control)?;
        edge_count = edge_count
            .checked_add(package.dependencies.len())
            .ok_or(AuditDataError::Budget)?;
        if edge_count > MAX_EDGES {
            return Err(AuditDataError::Budget);
        }
        if !valid_name(&package.name)
            || package
                .checksum
                .as_ref()
                .is_some_and(|v| v.len() != 64 || !v.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err(AuditDataError::InvalidLockfile);
        }
        let identity = Identity {
            name: package.name.clone(),
            version: version(&package.version)?,
            source: package.source.as_deref().map(source_id).transpose()?,
        };
        let candidates = by_name.entry(&package.name).or_default();
        for &other in candidates.iter() {
            checkpoint(control)?;
            if identities[other].version == identity.version
                && same_optional_source(identities[other].source.as_ref(), identity.source.as_ref())
            {
                return Err(AuditDataError::InvalidLockfile);
            }
        }
        candidates.push(index);
        identities.push(identity);
    }
    let mut edges = Vec::with_capacity(raw.package.len());
    for package in &raw.package {
        checkpoint(control)?;
        let mut resolved = Vec::with_capacity(package.dependencies.len());
        let mut distinct = BTreeSet::new();
        for dependency in &package.dependencies {
            checkpoint(control)?;
            let dependency = RawDependency::parse(dependency)?;
            let mut found = None;
            for &candidate in by_name.get(dependency.name.as_str()).into_iter().flatten() {
                checkpoint(control)?;
                if dependency.matches(&identities[candidate]) {
                    if found.is_some() {
                        return Err(AuditDataError::InvalidLockfile);
                    }
                    found = Some(candidate);
                }
            }
            let index = found.ok_or(AuditDataError::InvalidLockfile)?;
            if !distinct.insert(index) {
                return Err(AuditDataError::InvalidLockfile);
            }
            resolved.push(index);
        }
        edges.push(resolved);
    }
    checkpoint(control)?;
    let lock: Lockfile = text.parse().map_err(|_| AuditDataError::InvalidLockfile)?;
    if lock.packages.len() != identities.len() {
        return Err(AuditDataError::InvalidLockfile);
    }
    for (index, package) in lock.packages.iter().enumerate() {
        checkpoint(control)?;
        let expected = &identities[index];
        if package.name.as_str() != expected.name
            || package.version != expected.version
            || !same_optional_source(package.source.as_ref(), expected.source.as_ref())
            || package.dependencies.len() != edges[index].len()
        {
            return Err(AuditDataError::InvalidLockfile);
        }
        for (dependency, &target) in package.dependencies.iter().zip(&edges[index]) {
            checkpoint(control)?;
            let expected = &identities[target];
            if dependency.name.as_str() != expected.name
                || dependency.version != expected.version
                || !same_optional_source(dependency.source.as_ref(), expected.source.as_ref())
            {
                return Err(AuditDataError::InvalidLockfile);
            }
        }
    }
    let roots = workspace_roots(structure, &identities, control)?;
    validate_reachability(&roots, &edges, control)?;
    let root_set: BTreeSet<_> = roots.iter().copied().collect();
    // cargo-lock 11.0.1 marks every parsed registry source precise="locked";
    // its programmatically constructed default has precise=None. Compare with
    // the canonical *parsed lock* representation without relaxed SourceId Eq.
    let canonical = source_id(&SourceId::default().to_string())?;
    let mut packages = Vec::with_capacity(identities.len());
    let mut unsupported = Vec::new();
    let mut scanned_indices = Vec::new();
    for (index, identity) in identities.iter().enumerate() {
        checkpoint(control)?;
        let source = if root_set.contains(&index) {
            AuditSource::Workspace
        } else if identity
            .source
            .as_ref()
            .is_some_and(|s| same_source(s, &canonical))
        {
            AuditSource::CratesIo
        } else {
            AuditSource::Unverified
        };
        let package = AuditPackage {
            name: identity.name.clone(),
            version: identity.version.to_string(),
            source,
            // Hash the captured origin, never emit credential-bearing URLs.
            source_fingerprint: raw.package[index]
                .source
                .as_deref()
                .map(|s| digest(s.as_bytes()))
                .transpose()?,
        };
        match source {
            AuditSource::CratesIo => scanned_indices.push(index),
            AuditSource::Unverified => unsupported.push(package.clone()),
            AuditSource::Workspace => {}
        }
        packages.push(package);
    }
    Ok(LockGraph {
        lock,
        packages,
        roots,
        edges,
        fingerprint: digest(bytes)?,
        unsupported,
        scanned_indices,
    })
}

fn validate_reachability(
    roots: &[usize],
    edges: &[Vec<usize>],
    control: &dyn InspectionControl,
) -> Result<(), AuditDataError> {
    let mut visited = vec![false; edges.len()];
    let mut queue = VecDeque::new();
    for &root in roots {
        checkpoint(control)?;
        visited[root] = true;
        queue.push_back(root);
    }
    while let Some(parent) = queue.pop_front() {
        checkpoint(control)?;
        for &child in &edges[parent] {
            checkpoint(control)?;
            if !visited[child] {
                visited[child] = true;
                queue.push_back(child);
            }
        }
    }
    if visited.iter().any(|seen| !seen) {
        return Err(AuditDataError::InvalidLockfile);
    }
    Ok(())
}

fn workspace_roots(
    structure: &ProjectStructure,
    identities: &[Identity],
    control: &dyn InspectionControl,
) -> Result<Vec<usize>, AuditDataError> {
    if structure.workspace_members.is_empty() {
        return Err(AuditDataError::InvalidLockfile);
    }
    if structure.workspace_members.len() > MAX_PACKAGES || structure.packages.len() > MAX_PACKAGES {
        return Err(AuditDataError::Budget);
    }
    let mut roots = BTreeSet::new();
    for member in &structure.workspace_members {
        checkpoint(control)?;
        let mut matching = structure
            .packages
            .iter()
            .filter(|p| p.package_index == *member);
        let package = matching.next().ok_or(AuditDataError::InvalidLockfile)?;
        if matching.next().is_some() {
            return Err(AuditDataError::InvalidLockfile);
        }
        let version = version(&package.version)?;
        let mut found = None;
        for (index, identity) in identities.iter().enumerate() {
            checkpoint(control)?;
            if identity.source.is_none()
                && identity.name == package.name
                && identity.version == version
            {
                if found.is_some() {
                    return Err(AuditDataError::InvalidLockfile);
                }
                found = Some(index);
            }
        }
        if !roots.insert(found.ok_or(AuditDataError::InvalidLockfile)?) {
            return Err(AuditDataError::InvalidLockfile);
        }
    }
    Ok(roots.into_iter().collect())
}

impl LockGraph {
    /// One shortest representative per reachable root. Reverse BFS explores each
    /// node once, including cycles, without enumerating exponentially many paths.
    pub fn paths(
        &self,
        index: usize,
        control: &dyn InspectionControl,
    ) -> Result<(Vec<AuditPath>, u64), AuditDataError> {
        checkpoint(control)?;
        if index >= self.packages.len() {
            return Err(AuditDataError::Internal);
        }
        let mut reverse = vec![Vec::new(); self.packages.len()];
        for (parent, children) in self.edges.iter().enumerate() {
            checkpoint(control)?;
            for &child in children {
                checkpoint(control)?;
                reverse[child].push(parent);
            }
        }
        let mut distance = vec![None; self.packages.len()];
        let mut next = vec![None; self.packages.len()];
        let mut queue = VecDeque::from([index]);
        distance[index] = Some(0usize);
        while let Some(child) = queue.pop_front() {
            checkpoint(control)?;
            let child_distance = distance[child].ok_or(AuditDataError::Internal)?;
            for &parent in &reverse[child] {
                checkpoint(control)?;
                if distance[parent].is_none() {
                    distance[parent] = Some(child_distance + 1);
                    next[parent] = Some(child);
                    queue.push_back(parent);
                }
            }
        }
        let mut paths = Vec::new();
        let mut omitted = 0;
        for &root in &self.roots {
            checkpoint(control)?;
            let Some(length) = distance[root] else {
                continue;
            };
            if paths.len() >= MAX_PATHS || length >= MAX_PATH_PACKAGES {
                omitted += 1;
                continue;
            }
            let mut packages = vec![self.packages[root].clone()];
            let mut node = root;
            while node != index {
                checkpoint(control)?;
                node = next[node].ok_or(AuditDataError::Internal)?;
                packages.push(self.packages[node].clone());
            }
            paths.push(AuditPath {
                workspace_root: self.packages[root].clone(),
                packages,
            });
        }
        Ok((paths, omitted))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_engineering_application::{ExecutionCancellation, OperationControl, ProjectError};
    use rust_engineering_domain::{
        CargoConfiguration, ProjectConfigPolicy, ProjectPackage, RuntimeIdentity, RustEdition,
        SourceFile,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    const REGISTRY: &str = "registry+https://github.com/rust-lang/crates.io-index";
    struct Control(AtomicUsize);
    impl Control {
        fn live() -> Self {
            Self(AtomicUsize::new(usize::MAX))
        }
    }
    impl OperationControl for Control {
        fn check(&self) -> Result<(), ProjectError> {
            self.0
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_sub(1))
                .map(|_| ())
                .map_err(|_| ProjectError::Cancelled)
        }
    }
    impl ExecutionCancellation for Control {
        fn is_cancelled(&self) -> bool {
            self.0.load(Ordering::Relaxed) == 0
        }
    }
    struct FailingControl {
        remaining: AtomicUsize,
        error: ProjectError,
    }
    impl OperationControl for FailingControl {
        fn check(&self) -> Result<(), ProjectError> {
            self.remaining
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_sub(1))
                .map(|_| ())
                .map_err(|_| self.error)
        }
    }
    impl ExecutionCancellation for FailingControl {
        fn is_cancelled(&self) -> bool {
            self.error == ProjectError::Cancelled && self.remaining.load(Ordering::Relaxed) == 0
        }
    }
    fn bundle(text: &str) -> Result<SourceBundle, AuditDataError> {
        SourceBundle::new(vec![
            SourceFile::new("Cargo.lock".into(), text.as_bytes().to_vec())
                .map_err(|_| AuditDataError::Budget)?,
        ])
        .map_err(|_| AuditDataError::Internal)
    }
    fn structure(names: &[&str]) -> Result<ProjectStructure, AuditDataError> {
        let fingerprint = digest(b"fixture")?;
        let execution: rust_engineering_domain::ExecutionFingerprint = fingerprint
            .to_string()
            .parse()
            .map_err(|_| AuditDataError::Internal)?;
        Ok(ProjectStructure {
            workspace_members: (0..names.len()).map(|n| n as u32).collect(),
            workspace_default_members: vec![],
            packages: names
                .iter()
                .enumerate()
                .map(|(i, &name)| ProjectPackage {
                    package_index: i as u32,
                    name: name.into(),
                    version: "0.1.0".into(),
                    manifest_path: format!("{name}/Cargo.toml"),
                    edition: RustEdition::E2024,
                    rust_version: None,
                    targets: vec![],
                    features: vec![],
                    direct_dependencies: vec![],
                })
                .collect(),
            profiles: vec![],
            cargo_configuration: CargoConfiguration {
                project_config_policy: ProjectConfigPolicy::Rejected,
                frozen: true,
                offline: true,
                incremental: false,
                target_directory_ephemeral: true,
            },
            runtime: RuntimeIdentity {
                platform: "test".into(),
                image_id: "test".into(),
                configuration_fingerprint: execution.clone(),
                execution_fingerprint: execution,
                rust_version: "1.98.1".into(),
                cargo_version: "1.98.1".into(),
                declared_toolchain: None,
            },
            source_fingerprint: fingerprint,
        })
    }
    fn package(name: &str, version: &str, source: Option<&str>, dependencies: &[&str]) -> String {
        let mut value = format!("\n[[package]]\nname = {name:?}\nversion = {version:?}\n");
        if let Some(source) = source {
            value.push_str(&format!("source = {source:?}\n"));
        }
        value.push_str(&format!("dependencies = {dependencies:?}\n"));
        value
    }
    fn parsed(text: &str, roots: &[&str]) -> Result<LockGraph, AuditDataError> {
        parse(&bundle(text)?, &structure(roots)?, &Control::live())
    }
    fn invalid(text: &str) {
        assert!(matches!(
            parsed(text, &["root"]),
            Err(AuditDataError::InvalidLockfile)
        ));
    }

    #[test]
    fn checked_in_rsa_fixture_retains_source_and_path() -> Result<(), AuditDataError> {
        let text = include_str!("../../../../fixtures/vulnerable-dependency/Cargo.lock");
        let graph = parsed(text, &["fixture-vulnerable-dependency"])?;
        assert_eq!(graph.scanned_indices.len(), 1);
        assert_eq!(graph.packages[graph.scanned_indices[0]].name, "rsa");
        assert_eq!(graph.packages[graph.scanned_indices[0]].version, "0.9.6");
        assert!(graph.unsupported.is_empty());
        let (paths, omitted) = graph.paths(graph.scanned_indices[0], &Control::live())?;
        assert_eq!(omitted, 0);
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].packages.len(), 2);
        assert_eq!(paths[0].workspace_root.source, AuditSource::Workspace);
        assert_eq!(graph.fingerprint, digest(text.as_bytes())?);
        assert_eq!(graph.lock.packages.len(), 2);
        Ok(())
    }

    #[test]
    fn multihop_workspace_paths_are_shortest_and_deterministic() -> Result<(), AuditDataError> {
        let text = format!(
            "version = 4\n{}{}{}{}",
            package("root", "0.1.0", None, &["bridge", "rsa"]),
            package("second", "0.1.0", None, &["bridge"]),
            package("bridge", "0.1.0", Some(REGISTRY), &["rsa"]),
            package("rsa", "0.9.6", Some(REGISTRY), &[])
        );
        let graph = parsed(&text, &["root", "second"])?;
        let (paths, omitted) = graph.paths(3, &Control::live())?;
        assert_eq!(omitted, 0);
        assert_eq!(paths.len(), 2);
        assert_eq!(
            paths[0]
                .packages
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            ["root", "rsa"]
        );
        assert_eq!(
            paths[1]
                .packages
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            ["second", "bridge", "rsa"]
        );
        Ok(())
    }

    #[test]
    fn ambiguous_short_edges_fail_before_library_first_match() -> Result<(), AuditDataError> {
        let alternate = "registry+https://registry.example/index";
        for dep in ["rsa", "rsa 0.9.6"] {
            invalid(&format!(
                "version = 4\n{}{}{}",
                package("root", "0.1.0", None, &[dep]),
                package("rsa", "0.9.6", Some(REGISTRY), &[]),
                package("rsa", "0.9.6", Some(alternate), &[])
            ));
        }
        for (source, target) in [(REGISTRY, 1), (alternate, 2)] {
            let dep = format!("rsa 0.9.6 ({source})");
            let other = format!(
                "rsa 0.9.6 ({})",
                if target == 1 { alternate } else { REGISTRY }
            );
            let graph = parsed(
                &format!(
                    "version = 4\n{}{}{}",
                    package("root", "0.1.0", None, &[&dep, &other]),
                    package("rsa", "0.9.6", Some(REGISTRY), &[]),
                    package("rsa", "0.9.6", Some(alternate), &[])
                ),
                &["root"],
            )?;
            assert_eq!(graph.edges[0][0], target);
            assert_eq!(graph.paths(target, &Control::live())?.0.len(), 1);
        }
        Ok(())
    }

    #[test]
    fn qualified_versions_disambiguate_but_missing_or_duplicate_nodes_fail()
    -> Result<(), AuditDataError> {
        let nodes = format!(
            "{}{}",
            package("rsa", "0.9.6", Some(REGISTRY), &[]),
            package("rsa", "0.9.7", Some(REGISTRY), &[])
        );
        let graph = parsed(
            &format!(
                "version = 4\n{}{nodes}",
                package("root", "0.1.0", None, &["rsa 0.9.7", "rsa 0.9.6"])
            ),
            &["root"],
        )?;
        assert_eq!(graph.edges[0], [2, 1]);
        for dep in [
            "rsa",
            "missing",
            "rsa 0.8.0",
            "rsa 0.9.6 (registry+https://absent.example/index)",
        ] {
            invalid(&format!(
                "version = 4\n{}{nodes}",
                package("root", "0.1.0", None, &[dep])
            ));
        }
        let root = package("root", "0.1.0", None, &[]);
        invalid(&format!("version = 4\n{root}{root}"));
        let duplicate_edges = format!(
            "version = 4\n{}{}",
            package("root", "0.1.0", None, &["rsa", "rsa 0.9.6"]),
            package("rsa", "0.9.6", Some(REGISTRY), &[])
        );
        invalid(&duplicate_edges);
        Ok(())
    }

    #[test]
    fn local_external_and_credential_sources_never_become_crates_io() -> Result<(), AuditDataError>
    {
        let mut text = format!(
            "version = 4\n{}",
            package(
                "root",
                "0.1.0",
                None,
                &[
                    "local",
                    "path",
                    "git",
                    "registry",
                    "sparse",
                    "spoof",
                    "wrongkind"
                ]
            )
        );
        for (name, source) in [
            ("local", None),
            ("path", Some("path+file:///workspace/dep")),
            (
                "git",
                Some("git+https://user:password@git.example/repo#abcdef"),
            ),
            (
                "registry",
                Some("registry+https://token:secret@example.org/index"),
            ),
            ("sparse", Some("sparse+https://index.crates.io/")),
            (
                "spoof",
                Some("registry+https://github.com/rust-lang/crates.io-index?token=secret"),
            ),
            (
                "wrongkind",
                Some("git+https://github.com/rust-lang/crates.io-index#abcdef"),
            ),
        ] {
            text.push_str(&package(name, "0.1.0", source, &[]));
        }
        let graph = parsed(&text, &["root"])?;
        assert_eq!(graph.unsupported.len(), 7);
        assert!(graph.scanned_indices.is_empty());
        assert_eq!(graph.packages[1].source, AuditSource::Unverified);
        assert!(graph.packages[1].source_fingerprint.is_none());
        let response =
            serde_json::to_string(&graph.unsupported).map_err(|_| AuditDataError::Internal)?;
        for secret in ["password", "secret", "https", "workspace/dep", "abcdef"] {
            assert!(!response.contains(secret));
        }
        assert!(graph.packages[3].source_fingerprint.is_some());
        Ok(())
    }

    #[test]
    fn canonical_registry_comparison_preserves_precise_and_exact_origin()
    -> Result<(), AuditDataError> {
        let canonical = source_id(REGISTRY)?;
        assert_eq!(canonical.precise(), Some("locked"));
        // This is an API-level distinction, not a lockfile exploit: the URL
        // parser supplies the locked marker; a URL fragment stays in the URL.
        assert!(!same_source(&canonical, &SourceId::default()));
        assert!(!same_source(
            &canonical,
            &canonical.with_precise(Some("unexpected".into()))
        ));
        assert!(same_source(
            &canonical,
            &SourceId::default().with_precise(Some("locked".into()))
        ));
        for altered in [
            format!("{REGISTRY}#locked"),
            format!("{REGISTRY}?precise=locked"),
            format!("{REGISTRY}/"),
            "registry+https://user:secret@github.com/rust-lang/crates.io-index".into(),
            "git+https://github.com/rust-lang/crates.io-index#locked".into(),
        ] {
            let source = source_id(&altered)?;
            assert!(!same_source(&source, &canonical));
            let text = format!(
                "version = 4\n{}{}{}",
                package("root", "0.1.0", None, &["verified", "altered"]),
                package("verified", "0.1.0", Some(REGISTRY), &[]),
                package("altered", "0.1.0", Some(&altered), &[])
            );
            let graph = parsed(&text, &["root"])?;
            assert_eq!(graph.scanned_indices, [1]);
            assert_eq!(graph.unsupported.len(), 1);
            assert_eq!(graph.unsupported[0].name, "altered");
        }
        Ok(())
    }

    #[test]
    fn package_and_dependency_names_use_bounded_ascii_grammar() -> Result<(), AuditDataError> {
        for name in ["ascii-name_123", "_private", "UPPER"] {
            let text = format!(
                "version = 4\n{}{}",
                package("root", "0.1.0", None, &[name]),
                package(name, "0.1.0", Some(REGISTRY), &[])
            );
            assert!(parsed(&text, &["root"]).is_ok());
        }
        for name in [
            "résumé",
            "rѕa",
            "a\u{0301}",
            "a\u{202e}b",
            "a\u{200b}b",
            "a\n",
            "a\t",
            "a\0",
            "-start",
            "0start",
        ] {
            assert!(!valid_name(name));
            let text = format!(
                "version = 4\n{}{}",
                package("root", "0.1.0", None, &[name]),
                package(name, "0.1.0", Some(REGISTRY), &[])
            );
            invalid(&text);
            assert!(matches!(
                RawDependency::parse(name),
                Err(AuditDataError::InvalidLockfile)
            ));
        }
        Ok(())
    }

    #[test]
    fn exact_git_identity_preserves_revision_and_branch() -> Result<(), AuditDataError> {
        let sources = [
            "git+https://git.example/repo?branch=main#aaaa",
            "git+https://git.example/repo?branch=main#bbbb",
        ];
        let nodes = format!(
            "{}{}",
            package("dep", "0.1.0", Some(sources[0]), &[]),
            package("dep", "0.1.0", Some(sources[1]), &[])
        );
        let dep = format!("dep 0.1.0 ({})", sources[1]);
        let other = format!("dep 0.1.0 ({})", sources[0]);
        let graph = parsed(
            &format!(
                "version = 4\n{}{nodes}",
                package("root", "0.1.0", None, &[&dep, &other])
            ),
            &["root"],
        )?;
        assert_eq!(graph.edges[0], [2, 1]);
        for dep in [
            "dep",
            "dep 0.1.0",
            "dep 0.1.0 (git+https://git.example/repo?branch=main)",
            "dep 0.1.0 (git+https://git.example/repo?branch=other#bbbb)",
        ] {
            invalid(&format!(
                "version = 4\n{}{nodes}",
                package("root", "0.1.0", None, &[dep])
            ));
        }
        Ok(())
    }

    #[test]
    fn workspace_mapping_requires_unique_local_identity_and_existing_index()
    -> Result<(), AuditDataError> {
        invalid(&format!(
            "version = 4\n{}",
            package("root", "0.1.0", Some(REGISTRY), &[])
        ));
        invalid(&format!(
            "version = 4\n{}",
            package("root", "0.2.0", None, &[])
        ));
        invalid(&format!(
            "version = 4\n{}",
            package("other", "0.1.0", None, &[])
        ));
        let source = bundle(&format!(
            "version = 4\n{}",
            package("root", "0.1.0", None, &[])
        ))?;
        for members in [vec![], vec![7], vec![0, 0]] {
            let mut metadata = structure(&["root"])?;
            metadata.workspace_members = members;
            assert!(matches!(
                parse(&source, &metadata, &Control::live()),
                Err(AuditDataError::InvalidLockfile)
            ));
        }
        let mut metadata = structure(&["root"])?;
        metadata.packages.push(metadata.packages[0].clone());
        assert!(matches!(
            parse(&source, &metadata, &Control::live()),
            Err(AuditDataError::InvalidLockfile)
        ));
        Ok(())
    }

    #[test]
    fn unsupported_schema_versions_fields_and_dependency_grammar_fail_closed() {
        let root = package("root", "0.1.0", None, &[]);
        for prefix in [
            "",
            "version = 1\n",
            "version = 2\n",
            "version = 3\n",
            "version = 5\n",
            "version = -1\n",
            "version = 4\nunknown = true\n",
        ] {
            invalid(&format!("{prefix}{root}"));
        }
        for suffix in [
            "unknown = true\n",
            "replace = 'other 0.1.0'\n",
            "[root]\nname = 'root'\nversion = '0.1.0'\n",
            "[[patch.unused]]\nname = 'unused'\nversion = '0.1.0'\n",
            "[metadata]\nignored = 'fact'\n",
            "[patch]\nunknown = []\n",
        ] {
            invalid(&format!("version = 4\n{root}{suffix}"));
        }
        for dep in [
            "",
            " rsa",
            "rsa ",
            "rsa  0.9.6",
            "rsa\t0.9.6",
            "rsa 0.9.6 extra extra",
            "rsa (registry+https://example.org)",
            "rsa 0.9.6 (registry+https://exam ple.org)",
            "rsa 0.9.6 registry+https://example.org",
        ] {
            invalid(&format!(
                "version = 4\n{}{}",
                package("root", "0.1.0", None, &[dep]),
                package("rsa", "0.9.6", Some(REGISTRY), &[])
            ));
        }
    }

    #[test]
    fn name_version_source_and_checksum_limits_are_enforced() {
        let root = package("root", "0.1.0", None, &[]);
        for (name, version, source) in [
            ("n".repeat(MAX_NAME + 1), "0.1.0".into(), None),
            ("invalid/name".into(), "0.1.0".into(), None),
            (
                "dep".into(),
                format!("0.1.0+{}", "a".repeat(MAX_VERSION)),
                None,
            ),
            ("dep".into(), "not-semver".into(), None),
            (
                "dep".into(),
                "0.1.0".into(),
                Some(format!(
                    "registry+https://example.org/{}",
                    "a".repeat(MAX_SOURCE)
                )),
            ),
            (
                "dep".into(),
                "0.1.0".into(),
                Some("registry+https://exa\nmple.org".into()),
            ),
        ] {
            invalid(&format!(
                "version = 4\n{root}{}",
                package(&name, &version, source.as_deref(), &[])
            ));
        }
        invalid(&format!("version = 4\n{root}checksum = 'broken'\n"));
    }

    #[test]
    fn package_and_edge_budgets_precede_library_resolution() -> Result<(), AuditDataError> {
        let mut text = format!("version = 4\n{}", package("root", "0.1.0", None, &[]));
        for index in 0..MAX_PACKAGES {
            text.push_str(&package(
                &format!("dep{index}"),
                "0.1.0",
                Some(REGISTRY),
                &[],
            ));
        }
        assert!(matches!(
            parsed(&text, &["root"]),
            Err(AuditDataError::Budget)
        ));
        let text = format!(
            "version = 4\n{}{}",
            package("root", "0.1.0", None, &vec!["dep"; MAX_EDGES + 1]),
            package("dep", "0.1.0", Some(REGISTRY), &[])
        );
        assert!(matches!(
            parsed(&text, &["root"]),
            Err(AuditDataError::Budget)
        ));
        // SourceBundle prevents an oversized file from reaching the parser.
        assert!(SourceFile::new("Cargo.lock".into(), vec![b' '; MAX_BYTES + 1]).is_err());
        let absent = SourceBundle::new(vec![]).map_err(|_| AuditDataError::Internal)?;
        assert!(matches!(
            parse(&absent, &structure(&["root"])?, &Control::live()),
            Err(AuditDataError::MissingLockfile)
        ));
        Ok(())
    }

    #[test]
    fn cycles_terminate_and_do_not_duplicate_workspace_paths() -> Result<(), AuditDataError> {
        let text = format!(
            "version = 4\n{}{}{}{}",
            package("root", "0.1.0", None, &["bridge"]),
            package(
                "bridge",
                "0.1.0",
                Some(REGISTRY),
                &["root", "rsa", "bridge"]
            ),
            package("rsa", "0.9.6", Some(REGISTRY), &["bridge"]),
            package("isolated", "0.1.0", None, &[])
        );
        let graph = parsed(&text, &["root", "isolated"])?;
        let (paths, omitted) = graph.paths(2, &Control::live())?;
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].packages.len(), 3);
        assert_eq!(omitted, 0);
        assert_eq!(graph.paths(0, &Control::live())?.0[0].packages.len(), 1);
        Ok(())
    }

    #[test]
    fn orphan_nodes_fail_even_when_their_names_match_advisories() {
        invalid(&format!(
            "version = 4\n{}{}",
            package("root", "0.1.0", None, &[]),
            package("rsa", "0.9.6", Some(REGISTRY), &[])
        ));
        invalid(&format!(
            "version = 4\n{}{}{}",
            package("root", "0.1.0", None, &[]),
            package("cycle_a", "0.1.0", Some(REGISTRY), &["cycle_b"]),
            package("cycle_b", "0.1.0", Some(REGISTRY), &["cycle_a"])
        ));
    }

    #[test]
    fn exponential_route_graph_keeps_one_representative_in_linear_work()
    -> Result<(), AuditDataError> {
        let mut text = format!(
            "version = 4\n{}",
            package("root", "0.1.0", None, &["a0", "b0"])
        );
        for depth in 0..24 {
            let a = format!("a{}", depth + 1);
            let b = format!("b{}", depth + 1);
            let children = if depth == 23 {
                vec!["rsa"]
            } else {
                vec![a.as_str(), b.as_str()]
            };
            for prefix in ["a", "b"] {
                text.push_str(&package(
                    &format!("{prefix}{depth}"),
                    "0.1.0",
                    Some(REGISTRY),
                    &children,
                ));
            }
        }
        text.push_str(&package("rsa", "0.9.6", Some(REGISTRY), &[]));
        let graph = parsed(&text, &["root"])?;
        // More than 16 million root-to-target routes, but fewer than 1000
        // checkpoints including reverse-edge construction and returned path.
        let (paths, omitted) = graph.paths(49, &Control(AtomicUsize::new(1000)))?;
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].packages.len(), 26);
        assert_eq!(omitted, 0);
        Ok(())
    }

    #[test]
    fn path_depth_and_root_count_omissions_count_roots_not_routes() -> Result<(), AuditDataError> {
        let mut text = "version = 4\n".to_owned();
        let root_names: Vec<_> = (0..10).map(|n| format!("root{n}")).collect();
        for name in &root_names {
            text.push_str(&package(name, "0.1.0", None, &["dep0", "dep1"]));
        }
        for index in 0..33 {
            let next = format!("dep{}", index + 1);
            let dependency = next.as_str();
            text.push_str(&package(
                &format!("dep{index}"),
                "0.1.0",
                Some(REGISTRY),
                if index == 32 {
                    &[]
                } else {
                    std::slice::from_ref(&dependency)
                },
            ));
        }
        let root_refs: Vec<_> = root_names.iter().map(String::as_str).collect();
        let graph = parsed(&text, &root_refs)?;
        let (paths, omitted) = graph.paths(40, &Control::live())?;
        assert_eq!(paths.len(), 8);
        assert_eq!(paths[0].packages.len(), 31);
        assert_eq!(omitted, 2);
        let (paths, omitted) = graph.paths(41, &Control::live())?;
        assert_eq!(paths.len(), 8);
        assert_eq!(paths[0].packages.len(), 32);
        assert_eq!(omitted, 2);
        let (paths, omitted) = graph.paths(42, &Control::live())?;
        assert!(paths.is_empty());
        assert_eq!(omitted, 10);
        Ok(())
    }

    #[test]
    fn cancellation_is_checked_during_parse_and_graph_traversal() -> Result<(), AuditDataError> {
        let text = format!(
            "version = 4\n{}{}",
            package("root", "0.1.0", None, &["dep"]),
            package("dep", "0.1.0", Some(REGISTRY), &[])
        );
        for checkpoints in [0, 1, 4, 8, 12] {
            let control = Control(AtomicUsize::new(checkpoints));
            assert!(matches!(
                parse(&bundle(&text)?, &structure(&["root"])?, &control),
                Err(AuditDataError::Cancelled)
            ));
        }
        let graph = parsed(&text, &["root"])?;
        for checkpoints in [0, 1, 4, 7] {
            assert!(matches!(
                graph.paths(1, &Control(AtomicUsize::new(checkpoints))),
                Err(AuditDataError::Cancelled)
            ));
        }
        Ok(())
    }

    #[test]
    fn checkpoints_preserve_timeout_and_distinguish_other_failures() -> Result<(), AuditDataError> {
        let text = format!(
            "version = 4\n{}{}",
            package("root", "0.1.0", None, &["dep"]),
            package("dep", "0.1.0", Some(REGISTRY), &[])
        );
        let graph = parsed(&text, &["root"])?;
        for (error, expected) in [
            (
                ProjectError::Rejected(OperationalErrorCode::CommandTimeout),
                AuditDataError::Timeout,
            ),
            (ProjectError::Cancelled, AuditDataError::Cancelled),
            (
                ProjectError::Rejected(OperationalErrorCode::SandboxDenied),
                AuditDataError::Internal,
            ),
            (ProjectError::Internal, AuditDataError::Internal),
        ] {
            for checkpoints in [0, 4, 7] {
                let make_control = || FailingControl {
                    remaining: AtomicUsize::new(checkpoints),
                    error,
                };
                assert!(matches!(
                    parse(&bundle(&text)?, &structure(&["root"])?, &make_control()),
                    Err(actual) if actual == expected
                ));
                assert!(matches!(
                    graph.paths(1, &make_control()),
                    Err(actual) if actual == expected
                ));
            }
        }
        Ok(())
    }
}
