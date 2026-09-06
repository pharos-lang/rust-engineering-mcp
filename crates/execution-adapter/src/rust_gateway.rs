//! Cargo/source capability is distinct from the Go probe capability.
use super::*;
use rust_engineering_domain::{RustCommand, SourceBundle};
use std::collections::BTreeMap;

pub const APPROVED_RUST_IMAGE: &str =
    "sha256:384a1742ecc53cdd3a9c0bf36c6f8b66db73ddd118aeeae6e55654ea998ae36a";
// These versions were verified during explicit provisioning of the immutable ID.
// Changing the approved identity requires updating and reverifying this tuple.
pub(super) const APPROVED_RUST_VERSION: &str = "1.98.1";
pub(super) const APPROVED_CARGO_VERSION: &str = "1.98.1";
#[derive(Clone, Debug)]
pub(super) enum Phase {
    Ingest,
    /// Populates the second, always read-only SemVer baseline volume
    /// (ADR-062 §8). This phase has a dedicated writable ingest-time mount at
    /// `/baseline`; that same volume is remounted read-only for the comparison.
    IngestBaseline,
    GuardNextestOutput,
    /// Holds both ADR-065 tmpfs volumes mounted between the independent
    /// coverage run and report containers. It executes no project code; the
    /// executable target is read-only here.
    GuardCoverageVolumes,
    ExportNextest,
    ExportCoverageJson,
    ExportCoverageLcov,
    ExportCoverageHtml,
    /// Keeps the M3-05 `mutants.out` report volume alive between the run and
    /// the three fixed exporters (M2 staging guarantee, ADR-053).
    GuardMutationOutput,
    /// Generates the mutant set without building or running anything, so the
    /// `max_mutants` cap can be enforced before any project code executes.
    /// This is an adapter-internal phase: the closed `RustCommand` grammar
    /// exposes only the run itself.
    ListMutants(rust_engineering_domain::mutation_test::MutationTestCommandOptions),
    /// Fixed single-file egress of the only mutation oracle.
    ExportMutationOutcomes,
    /// Fixed egress of the bounded `diff/` and `logs/` report tree. `lock.json`
    /// is excluded here because it carries identity fields that are asserted
    /// separately and never published.
    ExportMutationBundle,
    /// Fixed single-file egress of `lock.json` for the guest-identity assertion.
    /// These bytes never leave the adapter.
    ExportMutationLock,
    Run(RustCommand),
}
impl Phase {
    /// The ADR-064 quality profile covers every phase that builds and runs test
    /// binaries. `cargo mutants` does both, for the baseline and for each
    /// mutant, so it needs exactly the same allowlist as nextest and coverage —
    /// no additional syscall was required or added for M3-05. The listing phase
    /// runs `cargo metadata` and the mutant generator only, and shares the
    /// profile so the two mutation phases differ solely in argv.
    fn quality_profile(&self) -> bool {
        matches!(
            self,
            Self::ListMutants(_)
                | Self::Run(
                    RustCommand::TestNextest(_)
                        | RustCommand::CoverageRun(_)
                        | RustCommand::CoverageReport(_)
                        | RustCommand::SemverCheck(_)
                        | RustCommand::MutationTest(_)
                )
        )
    }

    pub(super) fn seccomp_profile_name(&self) -> &'static str {
        if self.quality_profile() {
            "seccomp-rust-quality.json"
        } else {
            "seccomp-rust.json"
        }
    }

    pub(super) fn seccomp_profile_json(&self) -> &'static str {
        if self.quality_profile() {
            include_str!("seccomp-rust-quality.json")
        } else {
            include_str!("seccomp-rust.json")
        }
    }

    /// The private writable copy the mutators work in. `cargo-mutants` copies
    /// the tree into the system temporary directory, so the M3-05 phases point
    /// `TMPDIR` at this container-scoped tmpfs: it is created empty per
    /// container, is owned by the unprivileged guest user, is destroyed with
    /// the container and is reachable by no exporter, so no mutated source can
    /// ever be read back out. `/source` itself stays read-only for both phases.
    pub(super) fn extra_tmpfs(&self) -> Option<(&'static str, &'static str)> {
        matches!(
            self,
            Self::ListMutants(_) | Self::Run(RustCommand::MutationTest(_))
        )
        .then_some((MUTATION_SCRATCH_PATH, MUTATION_SCRATCH_TMPFS))
    }
    pub(super) fn ingesting(&self) -> bool {
        matches!(self, Self::Ingest | Self::IngestBaseline)
    }
    /// ADR-065's executable named tmpfs is absent from every non-coverage
    /// phase. It is writable while the instrumented tests run and while the
    /// pinned plugin merges profraw data in each report invocation; the
    /// keeper remains read-only and exporters never receive the mount.
    pub(super) fn coverage_target_writable(&self) -> Option<bool> {
        match self {
            Self::Run(RustCommand::CoverageRun(_) | RustCommand::CoverageReport(_)) => Some(true),
            Self::GuardCoverageVolumes => Some(false),
            _ => None,
        }
    }
    /// The base allowlist (`environment()`), extended for the semver `Run`
    /// phase only (ADR-062 §8) with a fixed environment that neutralizes
    /// `cargo-semver-checks`'s own git auto-detection: `GIT_DIR` points at a
    /// guest path guaranteed absent from `APPROVED_RUST_IMAGE`, and
    /// `GIT_CEILING_DIRECTORIES`/`NO_COLOR` are defense in depth (neither
    /// `--baseline-version` nor `--baseline-rev` is ever passed, so git
    /// detection is not a functioning code path regardless). `NO_COLOR=1`
    /// forces off the `anstream`/`anstyle`-driven color the pinned binary
    /// might otherwise emit; provisional pending §10/§11 calibration
    /// confirming the pinned binary's actual auto-detection behavior.
    pub(super) fn environment(&self) -> Vec<String> {
        let mut env = environment();
        if matches!(
            self,
            Self::Run(RustCommand::CoverageRun(_) | RustCommand::CoverageReport(_))
        ) {
            env.extend(
                [
                    "LLVM_COV=/opt/rust/lib/rustlib/aarch64-unknown-linux-gnu/bin/llvm-cov",
                    "LLVM_PROFDATA=/opt/rust/lib/rustlib/aarch64-unknown-linux-gnu/bin/llvm-profdata",
                    "CARGO_LLVM_COV_TARGET_DIR=/work/coverage-target",
                ]
                .map(str::to_owned),
            );
        }
        if matches!(self, Self::Run(RustCommand::SemverCheck(_))) {
            env.extend(
                [
                    "GIT_DIR=/nonexistent",
                    "GIT_CEILING_DIRECTORIES=/",
                    "NO_COLOR=1",
                ]
                .map(str::to_owned),
            );
            env.sort();
        }
        // The mutation phases replace, never extend, the base temporary
        // directory: leaving `TMPDIR=/tmp` would put the writable copy on the
        // small shared `noexec` tmpfs and silently fail every mutant build.
        if let Some((path, _)) = self.extra_tmpfs() {
            env.retain(|value| !value.starts_with("TMPDIR="));
            env.push(format!("TMPDIR={path}"));
        }
        env.sort();
        env
    }
    pub(super) fn user(&self) -> &'static str {
        if self.ingesting() {
            "0:0"
        } else {
            "65534:65534"
        }
    }
    pub(super) fn program(&self) -> &'static str {
        match self {
            Self::Ingest | Self::IngestBaseline => "/usr/bin/tar",
            Self::GuardNextestOutput | Self::GuardCoverageVolumes | Self::GuardMutationOutput => {
                "/usr/bin/sleep"
            }
            Self::ExportNextest
            | Self::ExportCoverageJson
            | Self::ExportCoverageLcov
            | Self::ExportCoverageHtml
            | Self::ExportMutationOutcomes
            | Self::ExportMutationBundle
            | Self::ExportMutationLock => "/usr/bin/tar",
            Self::ListMutants(_) => "/opt/rust/bin/cargo",
            Self::Run(RustCommand::CompilerVersion | RustCommand::Explain(_)) => {
                "/opt/rust/bin/rustc"
            }
            Self::Run(RustCommand::InstalledComponents) => "/usr/bin/cat",
            Self::Run(_) => "/opt/rust/bin/cargo",
        }
    }
    pub(super) fn arguments(&self) -> Vec<String> {
        let args: &[&str] = match self {
            Self::Ingest => &[
                "--extract",
                "--file=-",
                "--directory=/source",
                "--no-same-owner",
                "--no-same-permissions",
                "--keep-old-files",
            ],
            Self::IngestBaseline => &[
                "--extract",
                "--file=-",
                "--directory=/baseline",
                "--no-same-owner",
                "--no-same-permissions",
                "--keep-old-files",
            ],
            Self::GuardNextestOutput | Self::GuardCoverageVolumes | Self::GuardMutationOutput => {
                &["900"]
            }
            // Egress from the mutation report volume only. `--directory` names
            // the tool-written report tree, never `/source`, so a `mutants.out`
            // directory forged inside the project can never be exported.
            Self::ExportMutationOutcomes => &[
                "--create",
                "--file=-",
                "--format=ustar",
                "--no-recursion",
                "--directory=/mutants/mutants.out",
                "outcomes.json",
            ],
            Self::ExportMutationLock => &[
                "--create",
                "--file=-",
                "--format=ustar",
                "--no-recursion",
                "--directory=/mutants/mutants.out",
                "lock.json",
            ],
            Self::ExportMutationBundle => &[
                "--create",
                "--file=-",
                "--format=ustar",
                "--sort=name",
                "--one-file-system",
                "--exclude=./lock.json",
                "--directory=/mutants/mutants.out",
                ".",
            ],
            Self::ExportNextest => &[
                "--create",
                "--file=-",
                "--format=ustar",
                "--no-recursion",
                "--directory=/junit/rust-mcp/reports",
                "junit.xml",
            ],
            Self::ExportCoverageJson => &[
                "--create",
                "--file=-",
                "--format=ustar",
                "--no-recursion",
                "--directory=/work/coverage",
                "coverage.json",
            ],
            Self::ExportCoverageLcov => &[
                "--create",
                "--file=-",
                "--format=ustar",
                "--no-recursion",
                "--directory=/work/coverage",
                "lcov.info",
            ],
            Self::ExportCoverageHtml => &[
                "--create",
                "--file=-",
                "--format=ustar",
                "--sort=name",
                "--one-file-system",
                "--directory=/work/coverage/html",
                ".",
            ],
            Self::Run(RustCommand::FormatCheck) => &[
                "fmt",
                "--all",
                "--check",
                "--",
                "--color",
                "never",
                "--config",
                "disable_all_formatting=false",
            ],
            Self::Run(RustCommand::Metadata) => {
                &["metadata", "--format-version=1", "--no-deps", "--frozen"]
            }
            Self::Run(RustCommand::TestProject(_)) => &[
                "test",
                "--frozen",
                "--message-format=json",
                "--jobs=1",
                "--color=never",
            ],
            // Structured results come only from the parsed JUnit file
            // (`nextest_junit.rs`), never from stdout, so no message-format
            // flag is requested here. `--build-jobs`/`--test-threads` names
            // and values are carried from general cargo-nextest CLI
            // knowledge, not a page this package could fetch; the
            // integrator's calibration run must confirm them alongside the
            // NextestExit hypotheses. The config path is a product-owned
            // file placed under the source volume by `nextest_gateway.rs`,
            // never a guest- or caller-supplied path.
            Self::Run(RustCommand::TestNextest(_)) => &[
                "nextest",
                "run",
                "--config-file",
                super::nextest_gateway::NEXTEST_CONFIG_GUEST_PATH,
                "--profile",
                rust_engineering_domain::nextest::NEXTEST_PROFILE,
                "--frozen",
                "--offline",
                "--color=never",
                "--no-fail-fast",
                "--build-jobs=1",
                "--test-threads=1",
            ],
            Self::Run(RustCommand::CoverageRun(_)) => &[
                "llvm-cov",
                "--no-report",
                "--frozen",
                "--offline",
                "--jobs=1",
                "--color=never",
            ],
            Self::Run(RustCommand::CoverageReport(format)) => match format {
                rust_engineering_domain::coverage::CoverageReportFormat::Json => &[
                    "llvm-cov",
                    "report",
                    "--json",
                    "--output-path",
                    "/work/coverage/coverage.json",
                    "--frozen",
                    "--offline",
                    "--color=never",
                ],
                rust_engineering_domain::coverage::CoverageReportFormat::Lcov => &[
                    "llvm-cov",
                    "report",
                    "--lcov",
                    "--output-path",
                    "/work/coverage/lcov.info",
                    "--frozen",
                    "--offline",
                    "--color=never",
                ],
                rust_engineering_domain::coverage::CoverageReportFormat::Html => &[
                    "llvm-cov",
                    "report",
                    "--html",
                    "--output-dir",
                    "/work/coverage/html",
                    "--frozen",
                    "--offline",
                    "--color=never",
                ],
            },
            Self::Run(RustCommand::ClippyProject(_)) => {
                &["clippy", "--frozen", "--message-format=json", "--jobs=1"]
            }
            Self::Run(RustCommand::Check | RustCommand::CheckProject(_)) => {
                &["check", "--frozen", "--message-format=json", "--jobs=1"]
            }
            Self::Run(RustCommand::InstalledComponents) => {
                &["--", "/opt/rust/lib/rustlib/components"]
            }
            Self::Run(RustCommand::LlvmCovVersion) => &["llvm-cov", "--version"],
            Self::Run(RustCommand::CompilerVersion | RustCommand::CargoVersion) => {
                &["--version", "--verbose"]
            }
            Self::Run(RustCommand::Explain(code)) => {
                return vec![
                    "--explain".into(),
                    code.to_string(),
                    "--color".into(),
                    "never".into(),
                ];
            }
            // Closed argv per ADR-062 §8: baseline is the second, always
            // read-only mount added by `arguments_with_baseline`; the
            // candidate keeps its ordinary `/source` mount unchanged.
            Self::Run(RustCommand::SemverCheck(_)) => &[
                "semver-checks",
                "check-release",
                "--manifest-path",
                "/source/Cargo.toml",
                "--baseline-root",
                "/baseline",
                "--color",
                "never",
            ],
            // Argument-free plugin identity probe (ADR-062 §1), analogous to
            // `CargoVersion`/`LlvmCovVersion` above but naming the
            // cargo-semver-checks subcommand explicitly.
            Self::Run(RustCommand::SemverChecksVersion) => &["semver-checks", "--version"],
            Self::Run(RustCommand::MutantsVersion) => &["mutants", "--version"],
            // Listing generates the mutant set from the source text only: it
            // neither builds nor runs anything, so the cap can be enforced
            // before any project code executes. `--no-config` is mandatory on
            // both mutation phases: without it a hostile project's
            // `.cargo/mutants.toml` could redirect the output directory or add
            // cargo arguments.
            Self::ListMutants(_) => &[
                "mutants",
                "--no-config",
                "--dir",
                "/source",
                "--list",
                "--json",
                "--colors",
                "never",
                "--no-times",
            ],
            // Closed argv per the pinned cargo-mutants 27.1.0 help
            // (`docs/validation/m3-provisioning/help/cargo-mutants-help.stdout`).
            // The mandatory baseline is `--baseline run`: that binary's
            // `--baseline` accepts only `run`/`skip`, so the M3-05 "baseline
            // auto" intent is expressed as the explicit, non-skippable run.
            // No sharding, no `--in-place`, no free flags, one job, fixed
            // order, offline and frozen cargo, and an output directory inside
            // the private report volume.
            Self::Run(RustCommand::MutationTest(_)) => &[
                "mutants",
                "--no-config",
                "--dir",
                "/source",
                "--output",
                "/mutants",
                "--baseline",
                "run",
                "--no-shuffle",
                "--jobs",
                "1",
                "--jobserver",
                "false",
                "--copy-target",
                "false",
                "--copy-vcs",
                "false",
                "--cargo-arg=--offline",
                "--cargo-arg=--frozen",
                "--colors",
                "never",
                "--no-times",
                "--level",
                "info",
            ],
        };
        let mut args = args.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        if let Self::Run(RustCommand::CheckProject(options)) = self {
            if let Some(package) = options.package() {
                args.push(format!("--package={package}"));
            }
            if options.workspace() {
                args.push("--workspace".into());
            }
            if !options.features().is_empty() {
                args.push(format!("--features={}", options.features().join(",")));
            }
            if options.all_features() {
                args.push("--all-features".into());
            }
            if options.no_default_features() {
                args.push("--no-default-features".into());
            }
            if options.all_targets() {
                args.push("--all-targets".into());
            }
            if let Some(target) = options.target() {
                args.push(format!("--target={target}"));
            }
        }
        if let Self::Run(RustCommand::ClippyProject(options)) = self {
            if let Some(package) = options.package() {
                args.push(format!("--package={package}"));
            }
            if options.workspace() {
                args.push("--workspace".into());
            }
            if !options.features().is_empty() {
                args.push(format!("--features={}", options.features().join(",")));
            }
            if options.all_targets() {
                args.push("--all-targets".into());
            }
            use rust_engineering_domain::LintProfile;
            match options.lint_profile() {
                LintProfile::Default | LintProfile::Project => (),
                LintProfile::Strict => args.extend(["--", "-D", "warnings"].map(str::to_owned)),
                LintProfile::Pedantic => {
                    args.extend(["--", "-W", "clippy::pedantic"].map(str::to_owned))
                }
            }
        }
        if let Self::Run(RustCommand::TestProject(options)) = self {
            if let Some(package) = options.package() {
                args.push(format!("--package={package}"));
            }
            if !options.features().is_empty() {
                args.push(format!("--features={}", options.features().join(",")));
            }
            if options.all_features() {
                args.push("--all-features".into());
            }
            if let Some(target) = options.target() {
                args.push(format!("--target={target}"));
            }
            if let Some(filter) = options.test_filter() {
                args.push(filter.to_owned());
            }
            args.extend(["--", "--test-threads=1", "--color=never"].map(str::to_owned));
        }
        if let Self::Run(RustCommand::TestNextest(options)) = self {
            if let Some(package) = options.package() {
                args.push(format!("--package={package}"));
            }
            if !options.features().is_empty() {
                args.push(format!("--features={}", options.features().join(",")));
            }
            if options.all_features() {
                args.push("--all-features".into());
            }
            if options.no_default_features() {
                args.push("--no-default-features".into());
            }
            if let Some(target) = options.target() {
                args.push(format!("--target={target}"));
            }
            if let Some(filter) = options.test_filter() {
                args.push(filter.to_owned());
            }
        }
        if let Self::Run(RustCommand::CoverageRun(options)) = self {
            append_coverage_selection(&mut args, options);
        }
        if let Self::Run(RustCommand::SemverCheck(options)) = self {
            append_semver_selection(&mut args, options);
        }
        if let Self::Run(RustCommand::MutationTest(options)) = self {
            // Budgets first, then the selection, so argv shape is a pure
            // function of the validated options in a fixed order.
            args.extend([
                "--timeout".to_owned(),
                options.mutant_timeout_seconds().to_string(),
                // The tool would otherwise derive a longer per-mutant
                // timeout from a slow baseline; the floor keeps the
                // caller's bound authoritative.
                "--minimum-test-timeout".to_owned(),
                options.mutant_timeout_seconds().to_string(),
                "--build-timeout".to_owned(),
                options.build_timeout_seconds().to_string(),
            ]);
            append_mutation_selection(&mut args, options);
        }
        if let Self::ListMutants(options) = self {
            append_mutation_selection(&mut args, options);
        }
        args
    }
}

fn append_coverage_selection(
    args: &mut Vec<String>,
    options: &rust_engineering_domain::coverage::CoverageOptions,
) {
    if let Some(package) = options.package() {
        args.push(format!("--package={package}"));
    }
    if options.workspace() {
        args.push("--workspace".into());
    }
    if !options.features().is_empty() {
        args.push(format!("--features={}", options.features().join(",")));
    }
    if options.all_features() {
        args.push("--all-features".into());
    }
    if options.no_default_features() {
        args.push("--no-default-features".into());
    }
    if let Some(target) = options.target() {
        args.push(format!("--target={target}"));
    }
}

/// Applies the closed selection identically to both sides of the comparison
/// (ADR-062 §8): `cargo-semver-checks` itself only accepts one shared
/// `--features`/`--all-features`/etc. set, applied to both the baseline and
/// current crate, never `--baseline-features`/`--current-features`. The
/// pinned `check-release --help` (see
/// `docs/validation/m3-provisioning/help/cargo-semver-checks-check-release-help.stdout`)
/// exposes no `--no-default-features` flag; `--only-explicit-features` is the
/// closest documented equivalent ("Use no features except ones explicitly
/// added by other flags") and is used here as a provisional mapping pending
/// calibration confirmation.
fn append_semver_selection(
    args: &mut Vec<String>,
    options: &rust_engineering_domain::semver_check::SemverCommandOptions,
) {
    if let Some(package) = options.package() {
        args.push(format!("--package={package}"));
    }
    if !options.features().is_empty() {
        args.push(format!("--features={}", options.features().join(",")));
    }
    if options.all_features() {
        args.push("--all-features".into());
    }
    if options.no_default_features() {
        args.push("--only-explicit-features".into());
    }
    if let Some(target) = options.target() {
        args.push(format!("--target={target}"));
    }
}
/// Guest path of the private writable copy used by the M3-05 mutators.
pub(super) const MUTATION_SCRATCH_PATH: &str = "/mutants-scratch";
/// Same containment profile as the ADR-053 staging volume (unprivileged owner,
/// `0700`, `nosuid`, `nodev`), sized for one copied tree. `exec` is required
/// because `cargo-mutants` may place a mutant's target directory beside its
/// copy of the tree; the mount is private to a single container and is reachable
/// by no exporter.
pub(super) const MUTATION_SCRATCH_TMPFS: &str =
    "rw,exec,nosuid,nodev,size=256m,mode=0700,uid=65534,gid=65534";

/// The closed selection grammar, applied identically to the listing and the run
/// so the cap is counted over exactly the mutants that would be tested.
/// `cargo-mutants` has no `--target` of its own; the one installed triple is
/// forwarded through the documented `--cargo-arg` escape, which cannot carry a
/// caller string because [`MutationTestCommandOptions`] accepts only that
/// triple.
///
/// [`MutationTestCommandOptions`]: rust_engineering_domain::mutation_test::MutationTestCommandOptions
fn append_mutation_selection(
    args: &mut Vec<String>,
    options: &rust_engineering_domain::mutation_test::MutationTestCommandOptions,
) {
    if let Some(package) = options.package() {
        args.push(format!("--package={package}"));
    }
    if !options.features().is_empty() {
        args.push(format!("--features={}", options.features().join(",")));
    }
    if options.all_features() {
        args.push("--all-features".into());
    }
    if options.no_default_features() {
        args.push("--no-default-features".into());
    }
    if let Some(target) = options.target() {
        args.push(format!("--cargo-arg=--target={target}"));
    }
}

pub(super) fn environment() -> Vec<String> {
    let mut env = [
        "PATH=/opt/rust/bin:/usr/bin:/bin",
        "HOME=/work",
        "TMPDIR=/tmp",
        "CARGO_HOME=/opt/rust",
        "CARGO_TARGET_DIR=/work/target",
        "CARGO_INCREMENTAL=0",
        "CARGO_NET_OFFLINE=true",
        "RUSTC=/opt/rust/bin/rustc",
        "RUSTDOC=/opt/rust/bin/rustdoc",
        "RUSTFMT=/opt/rust/bin/rustfmt",
    ]
    .map(str::to_owned)
    .to_vec();
    env.sort();
    env
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct Volume {
    pub(super) name: String,
    driver: String,
    scope: String,
    options: Option<BTreeMap<String, String>>,
    labels: BTreeMap<String, String>,
    pub(super) mountpoint: String,
    cluster_volume: Option<serde_json::Value>,
    status: Option<serde_json::Value>,
}
impl Volume {
    pub(super) fn parse(bytes: &[u8], name: &str, nonce: &str) -> Result<Self, ExecutionError> {
        let mut volumes: Vec<Self> =
            serde_json::from_slice(bytes).map_err(|_| ExecutionError::Infrastructure)?;
        if volumes.len() != 1 {
            return Err(ExecutionError::InvalidConfiguration);
        }
        let v = volumes.pop().ok_or(ExecutionError::Infrastructure)?;
        if v.name != name
            || v.driver != "local"
            || v.scope != "local"
            || v.options.as_ref().is_some_and(|v| !v.is_empty())
            || v.labels != labels(nonce)
            || !v.mountpoint.starts_with("/var/lib/docker/volumes/")
            || !v.mountpoint.ends_with("/_data")
            || v.cluster_volume.is_some()
            || v.status.is_some()
        {
            return Err(ExecutionError::InvalidConfiguration);
        }
        Ok(v)
    }
}
pub(super) fn labels(nonce: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("org.rust-mcp.execution".into(), "true".into()),
        ("org.rust-mcp.rust-job".into(), nonce.into()),
    ])
}

pub(super) struct PhaseRequest<'a> {
    pub(super) name: &'a str,
    pub(super) nonce: &'a str,
    pub(super) volume: &'a Volume,
    pub(super) phase: &'a Phase,
}
fn implementation_fingerprint() -> String {
    let sources: &[&[u8]] = &[
        include_bytes!("rust_gateway.rs"),
        include_bytes!("project_inspection.rs"),
        include_bytes!("rust_applied.rs"),
        include_bytes!("rust_calibration.rs"),
        include_bytes!("nextest_gateway.rs"),
        include_bytes!("coverage_gateway.rs"),
        include_bytes!("nextest_junit.rs"),
        include_bytes!("nextest_port.rs"),
        include_bytes!("mutation_test_gateway.rs"),
        include_bytes!("mutation_outcomes.rs"),
        include_bytes!("mutation_test_port.rs"),
        include_bytes!("../../domain/src/mutation_test.rs"),
        include_bytes!("semver_output.rs"),
        include_bytes!("semver_gateway.rs"),
        include_bytes!("semver_port.rs"),
        include_bytes!("toolchain_metadata.rs"),
        include_bytes!("source_archive.rs"),
        include_bytes!("supervisor.rs"),
        include_bytes!("state.rs"),
        include_bytes!("lib.rs"),
        include_bytes!("../../domain/src/source.rs"),
        include_bytes!("../../domain/src/rust_execution.rs"),
        include_bytes!("../../domain/src/check.rs"),
        include_bytes!("../../domain/src/clippy.rs"),
        include_bytes!("../../domain/src/test_run.rs"),
        include_bytes!("../../domain/src/nextest.rs"),
        include_bytes!("../../domain/src/semver_check.rs"),
        include_bytes!("../../domain/src/explain.rs"),
        include_bytes!("../../domain/src/value.rs"),
        include_bytes!("../../../Cargo.lock"),
        include_bytes!("../../../rust-toolchain.toml"),
    ];
    let mut hash = Sha256::new();
    for source in sources {
        hash.update(source.len().to_le_bytes());
        hash.update(source);
    }
    format!(
        "sha256:{}",
        hash.finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

pub(super) fn finish_work(
    work: Result<(Capture, Option<bool>), ExecutionError>,
    terminal_signal: Option<Stop>,
) -> Result<(Capture, Option<bool>), ExecutionError> {
    // Cleanup may succeed after a verifier/control failure. Cancellation cannot
    // turn that earlier failure into evidence of a contained timed-out process.
    let (mut outcome, oom_killed) = work?;
    if let Some(stop) = terminal_signal
        && matches!(outcome.stop, Stop::Exited | Stop::Cancelled)
    {
        outcome.stop = stop;
        outcome.code = None;
    }
    Ok((outcome, oom_killed))
}

pub(super) enum Admission<'a> {
    Project,
    Calibration(Option<&'a Mutex<Option<String>>>),
}
pub(super) struct WorkBudget<'a> {
    pub(super) started: Instant,
    pub(super) deadline: Instant,
    pub(super) limits: ExecutionLimits,
    pub(super) cancel: &'a dyn ExecutionCancellation,
}
impl WorkBudget<'_> {
    pub(super) fn stop(&self) -> Option<Stop> {
        if self.cancel.is_cancelled() {
            Some(Stop::Cancelled)
        } else if Instant::now() >= self.deadline {
            Some(Stop::TimedOut)
        } else {
            None
        }
    }
    pub(super) fn stopped_capture(&self, stop: Stop) -> Capture {
        Capture {
            code: None,
            stdout: Vec::new(),
            stderr: Vec::new(),
            stdout_truncated: false,
            stderr_truncated: false,
            stop,
            duration_ms: self
                .started
                .elapsed()
                .as_millis()
                .try_into()
                .unwrap_or(u64::MAX),
        }
    }
}
impl ExecutionCancellation for WorkBudget<'_> {
    fn is_cancelled(&self) -> bool {
        self.stop().is_some()
    }
}

pub struct RustGateway {
    pub(super) inner: DockerGateway,
    pub(super) verified: AtomicBool,
    pub(super) calibrating: AtomicBool,
}
impl RustGateway {
    pub fn execute_mutation(
        &self,
        source: &SourceBundle,
        command: rust_engineering_domain::RustMutationCommand,
        limits: ExecutionLimits,
        cancel: &dyn ExecutionCancellation,
    ) -> Result<rust_engineering_domain::RustMutationExecution, ExecutionError> {
        super::mutation_gateway::execute(self, source, command, limits, cancel)
    }
    pub fn execute_nextest(
        &self,
        source: &SourceBundle,
        options: &rust_engineering_domain::nextest::NextestCommandOptions,
        limits: ExecutionLimits,
        cancel: &dyn ExecutionCancellation,
    ) -> Result<super::nextest_gateway::NextestExecution, ExecutionError> {
        super::nextest_gateway::execute(self, source, options, limits, cancel)
    }
    pub fn execute_mutation_test(
        &self,
        source: &SourceBundle,
        options: &rust_engineering_domain::mutation_test::MutationTestCommandOptions,
        limits: ExecutionLimits,
        cancel: &dyn ExecutionCancellation,
    ) -> Result<super::mutation_test_gateway::MutationTestExecution, ExecutionError> {
        super::mutation_test_gateway::execute(self, source, options, limits, cancel)
    }
    pub fn execute_coverage(
        &self,
        source: &SourceBundle,
        options: &rust_engineering_domain::coverage::CoverageOptions,
        limits: ExecutionLimits,
        cancel: &dyn ExecutionCancellation,
    ) -> Result<super::coverage_gateway::CoverageExecution, ExecutionError> {
        super::coverage_gateway::execute(self, source, options, limits, cancel)
    }
    pub fn execute_semver(
        &self,
        baseline: &SourceBundle,
        candidate: &SourceBundle,
        options: &rust_engineering_domain::semver_check::SemverCommandOptions,
        limits: ExecutionLimits,
        cancel: &dyn ExecutionCancellation,
    ) -> Result<rust_engineering_domain::ExecutionResult, ExecutionError> {
        super::semver_gateway::execute(self, baseline, candidate, options, limits, cancel)
    }
    pub fn new(config: HostDockerConfig) -> Result<Self, ExecutionError> {
        if config.image_id != APPROVED_RUST_IMAGE {
            return Err(ExecutionError::InvalidConfiguration);
        }
        let inner = DockerGateway::new(config)?;
        let existing = inner.control(&[
            "volume".into(),
            "ls".into(),
            "--filter=label=org.rust-mcp.execution=true".into(),
            "--format={{.Name}}".into(),
        ])?;
        if existing.code != Some(0) || !existing.stdout.iter().all(u8::is_ascii_whitespace) {
            return Err(ExecutionError::CleanupUncertain);
        }
        Ok(Self {
            inner,
            verified: AtomicBool::new(false),
            calibrating: AtomicBool::new(false),
        })
    }
    pub(super) fn set_verified(&self, verified: bool) {
        self.verified.store(verified, Ordering::Release);
    }
    pub(super) fn detached_observation(
        &self,
        name: &str,
    ) -> Result<Option<String>, ExecutionError> {
        if self.absent("container", name)? {
            return Ok(None);
        }
        let suffix = name
            .strip_prefix("rust-mcp-cargo-")
            .ok_or(ExecutionError::Infrastructure)?;
        if suffix.len() != 32 || !suffix.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(ExecutionError::Infrastructure);
        }
        if let Err(error) = self.owned_container(name, suffix) {
            if self.absent("container", name)? {
                return Ok(None);
            }
            return Err(error);
        }
        let top = self.inner.control(&[
            "container".into(),
            "top".into(),
            name.into(),
            "-eo".into(),
            "pid,ppid,pgid,sid,args".into(),
        ])?;
        // Completion can race observation. It cannot authorize a capability.
        if top.code != Some(0) {
            return Ok(None);
        }
        let top = String::from_utf8(top.stdout).map_err(|_| ExecutionError::Infrastructure)?;
        let mut sessions = std::collections::BTreeSet::new();
        for line in top.lines().skip(1) {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            if fields.len() >= 5 && fields[4].ends_with("/build-script-build") {
                let pid = fields[0]
                    .parse::<u64>()
                    .map_err(|_| ExecutionError::Infrastructure)?;
                let sid = fields[3]
                    .parse::<u64>()
                    .map_err(|_| ExecutionError::Infrastructure)?;
                if pid == 0 || sid == 0 {
                    return Err(ExecutionError::Infrastructure);
                }
                sessions.insert(sid);
            }
        }
        Ok((sessions.len() >= 2).then_some(top))
    }
    pub fn configuration_fingerprint(&self) -> Result<ExecutionFingerprint, ExecutionError> {
        let volume = Volume {
            name: "<volume>".into(),
            mountpoint: "<mountpoint>".into(),
            driver: "local".into(),
            scope: "local".into(),
            options: None,
            labels: labels("<nonce>"),
            cluster_volume: None,
            status: None,
        };
        let baseline_volume = Volume {
            name: "<baseline-volume>".into(),
            mountpoint: "<baseline-mountpoint>".into(),
            driver: "local".into(),
            scope: "local".into(),
            options: None,
            labels: labels("<nonce>"),
            cluster_volume: None,
            status: None,
        };
        let coverage_output_volume = super::mutation_gateway::MutationVolume {
            name: "<coverage-output-volume>".into(),
            mountpoint: "<coverage-output-mountpoint>".into(),
            driver: "local".into(),
            scope: "local".into(),
            options: BTreeMap::from([
                ("device".into(), "tmpfs".into()),
                ("o".into(), super::mutation_gateway::VOLUME_OPTIONS.into()),
                ("type".into(), "tmpfs".into()),
            ]),
            labels: labels("<nonce>"),
            cluster_volume: None,
            status: None,
        };
        let coverage_target_volume = super::mutation_gateway::MutationVolume {
            name: "<coverage-target-volume>".into(),
            mountpoint: "<coverage-target-mountpoint>".into(),
            options: BTreeMap::from([
                ("device".into(), "tmpfs".into()),
                (
                    "o".into(),
                    super::coverage_gateway::COVERAGE_TARGET_VOLUME_OPTIONS.into(),
                ),
                ("type".into(), "tmpfs".into()),
            ]),
            ..coverage_output_volume.clone()
        };
        let mut commands = Vec::new();
        for phase in [
            Phase::Ingest,
            Phase::IngestBaseline,
            Phase::GuardNextestOutput,
            Phase::ExportNextest,
            Phase::ExportCoverageJson,
            Phase::ExportCoverageLcov,
            Phase::ExportCoverageHtml,
            Phase::GuardMutationOutput,
            Phase::ExportMutationOutcomes,
            Phase::ExportMutationBundle,
            Phase::ExportMutationLock,
            Phase::ListMutants(
                rust_engineering_domain::mutation_test::MutationTestSelection::default()
                    .try_into()
                    .map_err(|_| ExecutionError::Infrastructure)?,
            ),
            Phase::Run(RustCommand::MutationTest(
                rust_engineering_domain::mutation_test::MutationTestSelection::default()
                    .try_into()
                    .map_err(|_| ExecutionError::Infrastructure)?,
            )),
            Phase::Run(RustCommand::MutantsVersion),
            Phase::Run(RustCommand::Metadata),
            Phase::Run(RustCommand::FormatCheck),
            Phase::Run(RustCommand::TestProject(
                rust_engineering_domain::TestSelection::default()
                    .try_into()
                    .map_err(|_| ExecutionError::Infrastructure)?,
            )),
            Phase::Run(RustCommand::TestNextest(
                rust_engineering_domain::nextest::NextestSelection::default()
                    .try_into()
                    .map_err(|_| ExecutionError::Infrastructure)?,
            )),
            Phase::Run(RustCommand::CoverageRun(
                rust_engineering_domain::coverage::CoverageSelection::default()
                    .try_into()
                    .map_err(|_| ExecutionError::Infrastructure)?,
            )),
            Phase::Run(RustCommand::CoverageReport(
                rust_engineering_domain::coverage::CoverageReportFormat::Json,
            )),
            Phase::Run(RustCommand::CoverageReport(
                rust_engineering_domain::coverage::CoverageReportFormat::Lcov,
            )),
            Phase::Run(RustCommand::CoverageReport(
                rust_engineering_domain::coverage::CoverageReportFormat::Html,
            )),
            Phase::Run(RustCommand::ClippyProject(
                rust_engineering_domain::ClippySelection::default()
                    .try_into()
                    .map_err(|_| ExecutionError::Infrastructure)?,
            )),
            Phase::Run(RustCommand::Check),
            Phase::Run(RustCommand::CompilerVersion),
            Phase::Run(RustCommand::Explain(
                "E0502"
                    .parse()
                    .map_err(|_| ExecutionError::Infrastructure)?,
            )),
            Phase::Run(RustCommand::CargoVersion),
            Phase::Run(RustCommand::LlvmCovVersion),
            Phase::Run(RustCommand::InstalledComponents),
            Phase::Run(RustCommand::CheckProject(
                rust_engineering_domain::CheckSelection::default()
                    .try_into()
                    .map_err(|_| ExecutionError::Infrastructure)?,
            )),
            Phase::Run(RustCommand::SemverChecksVersion),
        ] {
            let mut args = self.arguments("<container>", "<nonce>", &volume, &phase)?;
            for arg in &mut args {
                if arg.starts_with("--security-opt=seccomp=") {
                    *arg = "--security-opt=seccomp=<profile>".into();
                }
            }
            commands.push(args);
        }
        {
            let mut args = self.arguments_with_baseline(
                "<container>",
                "<nonce>",
                &volume,
                &baseline_volume,
                &Phase::Run(RustCommand::SemverCheck(
                    rust_engineering_domain::semver_check::SemverProjectSelection::default()
                        .try_into()
                        .map_err(|_| ExecutionError::Infrastructure)?,
                )),
            )?;
            for arg in &mut args {
                if arg.starts_with("--security-opt=seccomp=") {
                    *arg = "--security-opt=seccomp=<profile>".into();
                }
            }
            commands.push(args);
        }
        for phase in [
            Phase::GuardCoverageVolumes,
            Phase::Run(RustCommand::CoverageRun(
                rust_engineering_domain::coverage::CoverageSelection::default()
                    .try_into()
                    .map_err(|_| ExecutionError::Infrastructure)?,
            )),
            Phase::Run(RustCommand::CoverageReport(
                rust_engineering_domain::coverage::CoverageReportFormat::Json,
            )),
            Phase::Run(RustCommand::CoverageReport(
                rust_engineering_domain::coverage::CoverageReportFormat::Lcov,
            )),
            Phase::Run(RustCommand::CoverageReport(
                rust_engineering_domain::coverage::CoverageReportFormat::Html,
            )),
            Phase::ExportCoverageJson,
            Phase::ExportCoverageLcov,
            Phase::ExportCoverageHtml,
        ] {
            let mut args = self.arguments_with_coverage(
                "<container>",
                "<nonce>",
                &volume,
                &phase,
                &coverage_output_volume,
                &coverage_target_volume,
            )?;
            for arg in &mut args {
                if arg.starts_with("--security-opt=seccomp=") {
                    *arg = "--security-opt=seccomp=<profile>".into();
                }
            }
            commands.push(args);
        }
        let bytes = serde_json::to_vec(&(
            self.image_id(),
            &self.inner.engine,
            &self.inner.executable_digest,
            commands,
            include_str!("seccomp-rust.json"),
            include_str!("seccomp-rust-quality.json"),
            super::coverage_gateway::COVERAGE_TARGET_VOLUME_OPTIONS,
            // Receipts identify the actual verifier, archive/source limits and
            // supervisor implementation, not only a manually maintained label.
            implementation_fingerprint(),
            "rust-source-profile-v1",
        ))
        .map_err(|_| ExecutionError::Infrastructure)?;
        digest(&bytes)
            .parse()
            .map_err(|_| ExecutionError::Infrastructure)
    }
    pub fn image_id(&self) -> &str {
        self.inner.image_id()
    }
    pub fn is_quarantined(&self) -> bool {
        self.inner.is_quarantined()
    }
    pub(super) fn arguments(
        &self,
        name: &str,
        nonce: &str,
        volume: &Volume,
        phase: &Phase,
    ) -> Result<Vec<String>, ExecutionError> {
        let mut args = [
            "container",
            "create",
            "--pull=never",
            "--runtime=runc",
            "--init=false",
            "--network=none",
            "--read-only",
            "--cap-drop=ALL",
            "--security-opt=no-new-privileges=true",
            "--ipc=private",
            "--cgroupns=private",
            "--pids-limit=128",
            "--cpus=1",
            "--memory=1g",
            "--memory-swap=1g",
            "--shm-size=1m",
            "--log-driver=none",
            "--no-healthcheck",
            "--tmpfs=/work:rw,exec,nosuid,nodev,size=512m,mode=1777",
            "--tmpfs=/tmp:rw,nosuid,nodev,noexec,size=64m,mode=1777",
            "--workdir=/source",
            "--hostname=sandbox",
        ]
        .map(str::to_owned)
        .to_vec();
        args.push(format!("--name={name}"));
        args.push(format!("--user={}", phase.user()));
        if let Some((path, options)) = phase.extra_tmpfs() {
            args.push(format!("--tmpfs={path}:{options}"));
        }
        for (k, v) in labels(nonce) {
            args.push(format!("--label={k}={v}"));
        }
        for env in phase.environment() {
            args.push(format!("--env={env}"));
        }
        let profile = self.inner.state.path().join(phase.seccomp_profile_name());
        args.push(format!(
            "--security-opt=seccomp={}",
            profile
                .to_str()
                .ok_or(ExecutionError::InvalidConfiguration)?
        ));
        let source_target = if matches!(phase, Phase::IngestBaseline) {
            "/baseline"
        } else {
            "/source"
        };
        args.push(format!(
            "--mount=type=volume,source={},target={source_target},volume-nocopy,volume-driver=local{}",
            volume.name,
            if phase.ingesting() { "" } else { ",readonly" }
        ));
        if phase.ingesting() {
            args.push("--interactive".into());
        }
        args.push(format!("--entrypoint={}", phase.program()));
        args.push(self.inner.config.image_id.clone());
        args.extend(phase.arguments());
        Ok(args)
    }
    /// Adds the second, always read-only `/baseline` mount required by
    /// `RustCommand::SemverCheck` (ADR-062 §8). `volume` keeps its ordinary
    /// `/source` semantics (read-only for `Run`, unchanged from every other
    /// command); `baseline_volume` is never writable, in any phase.
    pub(super) fn arguments_with_baseline(
        &self,
        name: &str,
        nonce: &str,
        volume: &Volume,
        baseline_volume: &Volume,
        phase: &Phase,
    ) -> Result<Vec<String>, ExecutionError> {
        let mut args = self.arguments(name, nonce, volume, phase)?;
        let position = args
            .iter()
            .position(|arg| arg.starts_with("--entrypoint="))
            .ok_or(ExecutionError::Infrastructure)?;
        args.insert(
            position,
            format!(
                "--mount=type=volume,source={},target=/baseline,volume-nocopy,volume-driver=local,readonly",
                baseline_volume.name
            ),
        );
        Ok(args)
    }
    fn arguments_with_junit(
        &self,
        name: &str,
        nonce: &str,
        volume: &Volume,
        phase: &Phase,
        junit: &super::mutation_gateway::MutationVolume,
        junit_writable: bool,
    ) -> Result<Vec<String>, ExecutionError> {
        self.arguments_with_output(name, nonce, volume, phase, junit, junit_writable, "/junit")
    }
    #[allow(clippy::too_many_arguments)] // The closed mount shape stays explicit and auditable.
    fn arguments_with_output(
        &self,
        name: &str,
        nonce: &str,
        volume: &Volume,
        phase: &Phase,
        output: &super::mutation_gateway::MutationVolume,
        writable: bool,
        target: &str,
    ) -> Result<Vec<String>, ExecutionError> {
        let mut args = self.arguments(name, nonce, volume, phase)?;
        let position = args
            .iter()
            .position(|arg| arg.starts_with("--entrypoint="))
            .ok_or(ExecutionError::Infrastructure)?;
        args.insert(
            position,
            format!(
                "--mount=type=volume,source={},target={target},volume-nocopy,volume-driver=local{}",
                output.name,
                if writable { "" } else { ",readonly" }
            ),
        );
        Ok(args)
    }
    fn arguments_with_coverage(
        &self,
        name: &str,
        nonce: &str,
        volume: &Volume,
        phase: &Phase,
        output: &super::mutation_gateway::MutationVolume,
        target: &super::mutation_gateway::MutationVolume,
    ) -> Result<Vec<String>, ExecutionError> {
        let output_writable = !matches!(
            phase,
            Phase::ExportCoverageJson | Phase::ExportCoverageLcov | Phase::ExportCoverageHtml
        );
        let mut args = self.arguments_with_output(
            name,
            nonce,
            volume,
            phase,
            output,
            output_writable,
            "/work/coverage",
        )?;
        if let Some(writable) = phase.coverage_target_writable() {
            let position = args
                .iter()
                .position(|arg| arg.starts_with("--entrypoint="))
                .ok_or(ExecutionError::Infrastructure)?;
            args.insert(
                position,
                format!(
                    "--mount=type=volume,source={},target={},volume-nocopy,volume-driver=local{}",
                    target.name,
                    super::coverage_gateway::COVERAGE_TARGET_PATH,
                    if writable { "" } else { ",readonly" }
                ),
            );
        }
        Ok(args)
    }
    pub(super) fn absent(&self, kind: &str, name: &str) -> Result<bool, ExecutionError> {
        let args = if kind == "volume" {
            vec![
                "volume".into(),
                "ls".into(),
                format!("--filter=name=^{name}$"),
                "--format={{.Name}}".into(),
            ]
        } else {
            vec![
                "container".into(),
                "ls".into(),
                "--all".into(),
                format!("--filter=name=^/{name}$"),
                "--format={{.ID}}".into(),
            ]
        };
        let c = self.inner.control(&args)?;
        if c.code != Some(0) {
            return Err(ExecutionError::CleanupUncertain);
        }
        Ok(c.stdout.iter().all(u8::is_ascii_whitespace))
    }
    fn owned_container(&self, name: &str, nonce: &str) -> Result<(), ExecutionError> {
        #[derive(Deserialize)]
        struct Owned {
            #[serde(rename = "Config")]
            config: OwnedConfig,
        }
        #[derive(Deserialize)]
        struct OwnedConfig {
            #[serde(rename = "Labels")]
            labels: BTreeMap<String, String>,
            #[serde(rename = "Image")]
            image: String,
        }
        let inspect = self
            .inner
            .control(&["container".into(), "inspect".into(), name.into()])?;
        let values: Vec<Owned> = serde_json::from_slice(&inspect.stdout)
            .map_err(|_| ExecutionError::CleanupUncertain)?;
        if inspect.code != Some(0)
            || values.len() != 1
            || values[0].config.labels != labels(nonce)
            || values[0].config.image != self.image_id()
        {
            return Err(ExecutionError::CleanupUncertain);
        }
        Ok(())
    }
    fn owned_volume(&self, name: &str, nonce: &str) -> Result<(), ExecutionError> {
        let inspect = self
            .inner
            .control(&["volume".into(), "inspect".into(), name.into()])?;
        if inspect.code != Some(0) || Volume::parse(&inspect.stdout, name, nonce).is_err() {
            return Err(ExecutionError::CleanupUncertain);
        }
        Ok(())
    }
    pub(super) fn cleanup(
        &self,
        ingest: &str,
        run: &str,
        volume: &str,
        nonce: &str,
    ) -> Result<(), ExecutionError> {
        self.cleanup_inner(&[ingest, run], &[volume], nonce)
    }
    /// Join every nextest container before either of its two cross-mounted
    /// volumes is removed. The output guardian mounts the source read-only and
    /// the run container mounts JUnit read-write, so two independent cleanup
    /// passes would introduce an ordering cycle on non-terminal stops.
    #[allow(clippy::too_many_arguments)] // Closed names for four containers and two volumes.
    pub(super) fn cleanup_nextest(
        &self,
        ingest: &str,
        run: &str,
        exporter: &str,
        output_guardian: &str,
        source_volume: &str,
        junit_volume: &str,
        nonce: &str,
    ) -> Result<(), ExecutionError> {
        let mut clean = true;
        for name in [ingest, run, exporter, output_guardian] {
            match self.absent("container", name) {
                Ok(true) => (),
                Ok(false) => {
                    if self.owned_container(name, nonce).is_err()
                        || !matches!(
                            self.inner.control(&[
                                "container".into(),
                                "rm".into(),
                                "--force".into(),
                                name.into(),
                            ]),
                            Ok(ref result) if result.code == Some(0)
                        )
                        || !matches!(self.absent("container", name), Ok(true))
                    {
                        clean = false;
                    }
                }
                Err(_) => clean = false,
            }
        }
        if !clean {
            self.inner.quarantined.store(true, Ordering::Release);
            return Err(ExecutionError::CleanupUncertain);
        }
        // The ordinary source volume and the restricted local-driver tmpfs
        // volume have different verified metadata shapes. Attempt both
        // removals after all containers are gone, using each profile's
        // existing owner verifier.
        let source_clean = self
            .cleanup_inner_fallible(&[], &[source_volume], nonce)
            .is_ok();
        let output_clean = super::mutation_gateway::cleanup(self, &[], junit_volume, nonce).is_ok();
        if source_clean && output_clean {
            Ok(())
        } else {
            self.inner.quarantined.store(true, Ordering::Release);
            Err(ExecutionError::CleanupUncertain)
        }
    }
    /// Coverage has one source, one restricted report volume and ADR-065's
    /// dedicated executable target volume. Join and remove every named
    /// container before all three volumes; a partial report must not leak a
    /// live writer or executable build output into later work.
    pub(super) fn cleanup_coverage(
        &self,
        containers: &[&str],
        source_volume: &str,
        report_volume: &str,
        nonce: &str,
    ) -> Result<(), ExecutionError> {
        self.cleanup_coverage_inner(containers, source_volume, report_volume, None, nonce)
    }
    pub(super) fn cleanup_coverage_with_target(
        &self,
        containers: &[&str],
        source_volume: &str,
        report_volume: &str,
        target_volume: &str,
        nonce: &str,
    ) -> Result<(), ExecutionError> {
        self.cleanup_coverage_inner(
            containers,
            source_volume,
            report_volume,
            Some(target_volume),
            nonce,
        )
    }
    fn cleanup_coverage_inner(
        &self,
        containers: &[&str],
        source_volume: &str,
        report_volume: &str,
        target_volume: Option<&str>,
        nonce: &str,
    ) -> Result<(), ExecutionError> {
        let mut clean = true;
        for name in containers {
            match self.absent("container", name) {
                Ok(true) => (),
                Ok(false) => {
                    if self.owned_container(name, nonce).is_err()
                        || !matches!(
                            self.inner.control(&[
                                "container".into(),
                                "rm".into(),
                                "--force".into(),
                                (*name).into(),
                            ]),
                            Ok(ref result) if result.code == Some(0)
                        )
                        || !matches!(self.absent("container", name), Ok(true))
                    {
                        clean = false;
                    }
                }
                Err(_) => clean = false,
            }
        }
        let source_clean = self
            .cleanup_inner_fallible(&[], &[source_volume], nonce)
            .is_ok();
        let output_clean =
            super::mutation_gateway::cleanup(self, &[], report_volume, nonce).is_ok();
        let target_clean = target_volume.is_none_or(|target_volume| {
            super::mutation_gateway::cleanup_with_options(
                self,
                &[],
                target_volume,
                nonce,
                super::coverage_gateway::COVERAGE_TARGET_VOLUME_OPTIONS,
            )
            .is_ok()
        });
        if clean && source_clean && output_clean && target_clean {
            Ok(())
        } else {
            self.inner.quarantined.store(true, Ordering::Release);
            Err(ExecutionError::CleanupUncertain)
        }
    }
    /// Same contract as [`Self::cleanup`], extended to the second ingest
    /// container and second (baseline) volume `RustCommand::SemverCheck`
    /// adds (ADR-062 §8). Containers are always removed before either
    /// volume, exactly like the single-source path.
    #[allow(dead_code)] // Activated by the pending M3 semver vertical.
    #[allow(clippy::too_many_arguments)] // Cleanup names stay explicit; no guest data is accepted.
    pub(super) fn cleanup_with_baseline(
        &self,
        ingest: &str,
        ingest_baseline: &str,
        version: &str,
        run: &str,
        volume: &str,
        baseline_volume: &str,
        nonce: &str,
    ) -> Result<(), ExecutionError> {
        self.cleanup_inner(
            &[ingest, ingest_baseline, version, run],
            &[volume, baseline_volume],
            nonce,
        )
    }
    fn cleanup_inner(
        &self,
        containers: &[&str],
        volumes: &[&str],
        nonce: &str,
    ) -> Result<(), ExecutionError> {
        self.cleanup_inner_fallible(containers, volumes, nonce)
            .map_err(|_| {
                self.inner.quarantined.store(true, Ordering::Release);
                ExecutionError::CleanupUncertain
            })
    }
    fn cleanup_inner_fallible(
        &self,
        containers: &[&str],
        volumes: &[&str],
        nonce: &str,
    ) -> Result<(), ExecutionError> {
        // Attempt all removals even if one fails; never remove a volume before its writers.
        let mut clean = true;
        for name in containers {
            match self.absent("container", name) {
                Ok(true) => (),
                Ok(false) => {
                    if self.owned_container(name, nonce).is_err()
                        || self.inner.remove(name).is_err()
                    {
                        clean = false;
                    }
                }
                Err(_) => clean = false,
            }
        }
        if clean {
            for volume in volumes {
                match self.absent("volume", volume) {
                    Ok(true) => (),
                    Ok(false) => {
                        self.owned_volume(volume, nonce)?;
                        match self
                            .inner
                            .control(&["volume".into(), "rm".into(), (*volume).into()])
                        {
                            Ok(c) if c.code == Some(0) => (),
                            _ => clean = false,
                        }
                        if !matches!(self.absent("volume", volume), Ok(true)) {
                            clean = false;
                        }
                    }
                    Err(_) => clean = false,
                }
            }
        }
        clean.then_some(()).ok_or(ExecutionError::CleanupUncertain)
    }
    pub(super) fn phase(
        &self,
        request: PhaseRequest<'_>,
        input: &[u8],
        budget: &WorkBudget<'_>,
    ) -> Result<(Capture, Option<bool>), ExecutionError> {
        let PhaseRequest {
            name,
            nonce,
            volume,
            phase,
        } = request;
        if let Some(stop) = budget.stop() {
            return Ok((budget.stopped_capture(stop), None));
        }
        if !self.absent("container", name)? {
            return Err(ExecutionError::CleanupUncertain);
        }
        let created = self
            .inner
            .control(&self.arguments(name, nonce, volume, phase)?)?;
        if created.code != Some(0) {
            return Err(ExecutionError::Infrastructure);
        }
        if let Some(stop) = budget.stop() {
            return Ok((budget.stopped_capture(stop), None));
        }
        let inspect = self
            .inner
            .control(&["container".into(), "inspect".into(), name.into()])?;
        if inspect.code != Some(0) {
            return Err(ExecutionError::Infrastructure);
        }
        super::rust_applied::verify(&inspect.stdout, self.image_id(), phase, volume, nonce)?;
        if let Some(stop) = budget.stop() {
            return Ok((budget.stopped_capture(stop), None));
        }
        let mut command = DockerGateway::command(&self.inner.config, &self.inner.state)?;
        command.args(["container", "start", "--attach"]);
        if phase.ingesting() {
            command.arg("--interactive");
        }
        command.arg(name);
        let outcome = supervisor::run_with_input(
            command,
            budget.deadline.saturating_duration_since(Instant::now()),
            budget.limits.output_bytes(),
            budget,
            input,
        )?;
        let mut oom_killed = None;
        if outcome.stop == Stop::Exited {
            let inspected =
                self.inner
                    .control(&["container".into(), "inspect".into(), name.into()])?;
            let containers: Vec<Container> = serde_json::from_slice(&inspected.stdout)
                .map_err(|_| ExecutionError::Infrastructure)?;
            if inspected.code != Some(0)
                || containers.len() != 1
                || !containers[0].state.completed(outcome.code)
            {
                return Err(ExecutionError::Infrastructure);
            }
            oom_killed = Some(containers[0].state.oom_killed);
        }
        Ok((outcome, oom_killed))
    }
    /// Same contract as [`Self::phase`], for the one phase that mounts a
    /// second, always read-only volume at `/baseline`
    /// (`RustCommand::SemverCheck`, ADR-062 §8).
    #[allow(dead_code)] // Activated by the pending M3 semver vertical.
    pub(super) fn phase_with_baseline(
        &self,
        request: PhaseRequest<'_>,
        baseline_volume: &Volume,
        budget: &WorkBudget<'_>,
    ) -> Result<(Capture, Option<bool>), ExecutionError> {
        let PhaseRequest {
            name,
            nonce,
            volume,
            phase,
        } = request;
        if let Some(stop) = budget.stop() {
            return Ok((budget.stopped_capture(stop), None));
        }
        if !self.absent("container", name)? {
            return Err(ExecutionError::CleanupUncertain);
        }
        let created = self.inner.control(&self.arguments_with_baseline(
            name,
            nonce,
            volume,
            baseline_volume,
            phase,
        )?)?;
        if created.code != Some(0) {
            return Err(ExecutionError::Infrastructure);
        }
        if let Some(stop) = budget.stop() {
            return Ok((budget.stopped_capture(stop), None));
        }
        let inspect = self
            .inner
            .control(&["container".into(), "inspect".into(), name.into()])?;
        if inspect.code != Some(0) {
            return Err(ExecutionError::Infrastructure);
        }
        super::rust_applied::verify_semver(
            &inspect.stdout,
            self.image_id(),
            phase,
            volume,
            baseline_volume,
            nonce,
        )?;
        if let Some(stop) = budget.stop() {
            return Ok((budget.stopped_capture(stop), None));
        }
        let mut command = DockerGateway::command(&self.inner.config, &self.inner.state)?;
        command.args(["container", "start", "--attach", name]);
        let outcome = supervisor::run_with_input(
            command,
            budget.deadline.saturating_duration_since(Instant::now()),
            budget.limits.output_bytes(),
            budget,
            &[],
        )?;
        let mut oom_killed = None;
        if outcome.stop == Stop::Exited {
            let inspected =
                self.inner
                    .control(&["container".into(), "inspect".into(), name.into()])?;
            let containers: Vec<Container> = serde_json::from_slice(&inspected.stdout)
                .map_err(|_| ExecutionError::Infrastructure)?;
            if inspected.code != Some(0)
                || containers.len() != 1
                || !containers[0].state.completed(outcome.code)
            {
                return Err(ExecutionError::Infrastructure);
            }
            oom_killed = Some(containers[0].state.oom_killed);
        }
        Ok((outcome, oom_killed))
    }
    pub(super) fn nextest_phase(
        &self,
        request: PhaseRequest<'_>,
        junit: &super::mutation_gateway::MutationVolume,
        junit_writable: bool,
        budget: &WorkBudget<'_>,
    ) -> Result<(Capture, Option<bool>), ExecutionError> {
        let PhaseRequest {
            name,
            nonce,
            volume,
            phase,
        } = request;
        if let Some(stop) = budget.stop() {
            return Ok((budget.stopped_capture(stop), None));
        }
        if !self.absent("container", name)? {
            return Err(ExecutionError::CleanupUncertain);
        }
        let arguments =
            self.arguments_with_junit(name, nonce, volume, phase, junit, junit_writable)?;
        let created = self.inner.control(&arguments)?;
        if created.code != Some(0) {
            return Err(ExecutionError::Infrastructure);
        }
        let inspect = self
            .inner
            .control(&["container".into(), "inspect".into(), name.into()])?;
        if inspect.code != Some(0) {
            return Err(ExecutionError::Infrastructure);
        }
        super::rust_applied::verify_nextest(
            &inspect.stdout,
            self.image_id(),
            phase,
            volume,
            junit,
            junit_writable,
            nonce,
        )?;
        if let Some(stop) = budget.stop() {
            return Ok((budget.stopped_capture(stop), None));
        }
        let mut command = DockerGateway::command(&self.inner.config, &self.inner.state)?;
        command.args(["container", "start", "--attach", name]);
        let output_limit = if matches!(phase, Phase::ExportNextest) {
            super::nextest_gateway::MAX_JUNIT_EXPORT
        } else {
            budget.limits.output_bytes()
        };
        let outcome = supervisor::run_with_input(
            command,
            budget.deadline.saturating_duration_since(Instant::now()),
            output_limit,
            budget,
            &[],
        )?;
        let mut oom_killed = None;
        if outcome.stop == Stop::Exited {
            let inspected =
                self.inner
                    .control(&["container".into(), "inspect".into(), name.into()])?;
            let containers: Vec<Container> = serde_json::from_slice(&inspected.stdout)
                .map_err(|_| ExecutionError::Infrastructure)?;
            if inspected.code != Some(0)
                || containers.len() != 1
                || !containers[0].state.completed(outcome.code)
            {
                return Err(ExecutionError::Infrastructure);
            }
            oom_killed = Some(containers[0].state.oom_killed);
        }
        Ok((outcome, oom_killed))
    }

    pub(super) fn coverage_phase(
        &self,
        request: PhaseRequest<'_>,
        output: &super::mutation_gateway::MutationVolume,
        target: &super::mutation_gateway::MutationVolume,
        budget: &WorkBudget<'_>,
    ) -> Result<(Capture, Option<bool>), ExecutionError> {
        let PhaseRequest {
            name,
            nonce,
            volume,
            phase,
        } = request;
        if let Some(stop) = budget.stop() {
            return Ok((budget.stopped_capture(stop), None));
        }
        if !self.absent("container", name)? {
            return Err(ExecutionError::CleanupUncertain);
        }
        let arguments = self.arguments_with_coverage(name, nonce, volume, phase, output, target)?;
        if self.inner.control(&arguments)?.code != Some(0) {
            return Err(ExecutionError::Infrastructure);
        }
        let inspect = self
            .inner
            .control(&["container".into(), "inspect".into(), name.into()])?;
        if inspect.code != Some(0) {
            return Err(ExecutionError::Infrastructure);
        }
        super::rust_applied::verify_coverage(
            &inspect.stdout,
            self.image_id(),
            phase,
            volume,
            output,
            target,
            nonce,
        )?;
        if let Some(stop) = budget.stop() {
            return Ok((budget.stopped_capture(stop), None));
        }
        let mut command = DockerGateway::command(&self.inner.config, &self.inner.state)?;
        command.args(["container", "start", "--attach", name]);
        let output_limit = if matches!(
            phase,
            Phase::ExportCoverageJson | Phase::ExportCoverageLcov | Phase::ExportCoverageHtml
        ) {
            super::coverage_gateway::MAX_COVERAGE_EXPORT
        } else {
            budget.limits.output_bytes()
        };
        let outcome = supervisor::run_with_input(
            command,
            budget.deadline.saturating_duration_since(Instant::now()),
            output_limit,
            budget,
            &[],
        )?;
        let mut oom_killed = None;
        if outcome.stop == Stop::Exited {
            let inspected =
                self.inner
                    .control(&["container".into(), "inspect".into(), name.into()])?;
            let containers: Vec<Container> = serde_json::from_slice(&inspected.stdout)
                .map_err(|_| ExecutionError::Infrastructure)?;
            if inspected.code != Some(0)
                || containers.len() != 1
                || !containers[0].state.completed(outcome.code)
            {
                return Err(ExecutionError::Infrastructure);
            }
            oom_killed = Some(containers[0].state.oom_killed);
        }
        Ok((outcome, oom_killed))
    }

    /// Same contract as [`Self::coverage_phase`], for the M3-05 phases that
    /// mount the bounded `mutants.out` report volume at `/mutants`. The source
    /// volume keeps its ordinary read-only `Run` semantics: the mutators work
    /// in the container-private scratch tmpfs, never in `/source`.
    pub(super) fn mutation_test_phase(
        &self,
        request: PhaseRequest<'_>,
        output: &super::mutation_gateway::MutationVolume,
        writable: bool,
        budget: &WorkBudget<'_>,
    ) -> Result<(Capture, Option<bool>), ExecutionError> {
        let PhaseRequest {
            name,
            nonce,
            volume,
            phase,
        } = request;
        if let Some(stop) = budget.stop() {
            return Ok((budget.stopped_capture(stop), None));
        }
        if !self.absent("container", name)? {
            return Err(ExecutionError::CleanupUncertain);
        }
        let arguments = self.arguments_with_output(
            name,
            nonce,
            volume,
            phase,
            output,
            writable,
            super::mutation_test_gateway::MUTATION_OUTPUT_TARGET,
        )?;
        if self.inner.control(&arguments)?.code != Some(0) {
            return Err(ExecutionError::Infrastructure);
        }
        let inspect = self
            .inner
            .control(&["container".into(), "inspect".into(), name.into()])?;
        if inspect.code != Some(0) {
            return Err(ExecutionError::Infrastructure);
        }
        super::rust_applied::verify_mutation_test(
            &inspect.stdout,
            self.image_id(),
            phase,
            volume,
            output,
            writable,
            nonce,
        )?;
        if let Some(stop) = budget.stop() {
            return Ok((budget.stopped_capture(stop), None));
        }
        let mut command = DockerGateway::command(&self.inner.config, &self.inner.state)?;
        command.args(["container", "start", "--attach", name]);
        let outcome = supervisor::run_with_input(
            command,
            budget.deadline.saturating_duration_since(Instant::now()),
            super::mutation_test_gateway::output_limit(phase, budget.limits),
            budget,
            &[],
        )?;
        let mut oom_killed = None;
        if outcome.stop == Stop::Exited {
            let inspected =
                self.inner
                    .control(&["container".into(), "inspect".into(), name.into()])?;
            let containers: Vec<Container> = serde_json::from_slice(&inspected.stdout)
                .map_err(|_| ExecutionError::Infrastructure)?;
            if inspected.code != Some(0)
                || containers.len() != 1
                || !containers[0].state.completed(outcome.code)
            {
                return Err(ExecutionError::Infrastructure);
            }
            oom_killed = Some(containers[0].state.oom_killed);
        }
        Ok((outcome, oom_killed))
    }

    /// Keeps the Docker-managed report volume alive from creation until the
    /// last exporter has read it, exactly as the nextest and coverage verticals
    /// do for their own output volumes.
    pub(super) fn start_mutation_output_guardian(
        &self,
        request: PhaseRequest<'_>,
        output: &super::mutation_gateway::MutationVolume,
        budget: &WorkBudget<'_>,
    ) -> Result<(), ExecutionError> {
        let PhaseRequest {
            name,
            nonce,
            volume,
            phase,
        } = request;
        if !matches!(phase, Phase::GuardMutationOutput) || budget.stop().is_some() {
            return Err(ExecutionError::InvalidConfiguration);
        }
        if !self.absent("container", name)? {
            return Err(ExecutionError::CleanupUncertain);
        }
        let arguments = self.arguments_with_output(
            name,
            nonce,
            volume,
            phase,
            output,
            true,
            super::mutation_test_gateway::MUTATION_OUTPUT_TARGET,
        )?;
        if self.inner.control(&arguments)?.code != Some(0) {
            return Err(ExecutionError::Infrastructure);
        }
        let inspect = self
            .inner
            .control(&["container".into(), "inspect".into(), name.into()])?;
        if inspect.code != Some(0) {
            return Err(ExecutionError::Infrastructure);
        }
        super::rust_applied::verify_mutation_test(
            &inspect.stdout,
            self.image_id(),
            phase,
            volume,
            output,
            true,
            nonce,
        )?;
        if self
            .inner
            .control(&["container".into(), "start".into(), name.into()])?
            .code
            != Some(0)
            || !super::mutation_gateway::running(self, name, nonce, budget.deadline, budget.cancel)?
        {
            return Err(ExecutionError::Infrastructure);
        }
        Ok(())
    }

    pub(super) fn start_coverage_output_guardian(
        &self,
        request: PhaseRequest<'_>,
        output: &super::mutation_gateway::MutationVolume,
        target: &super::mutation_gateway::MutationVolume,
        budget: &WorkBudget<'_>,
    ) -> Result<(), ExecutionError> {
        let PhaseRequest {
            name,
            nonce,
            volume,
            phase,
        } = request;
        if !matches!(phase, Phase::GuardCoverageVolumes) || budget.stop().is_some() {
            return Err(ExecutionError::InvalidConfiguration);
        }
        if !self.absent("container", name)? {
            return Err(ExecutionError::CleanupUncertain);
        }
        let arguments = self.arguments_with_coverage(name, nonce, volume, phase, output, target)?;
        if self.inner.control(&arguments)?.code != Some(0) {
            return Err(ExecutionError::Infrastructure);
        }
        let inspect = self
            .inner
            .control(&["container".into(), "inspect".into(), name.into()])?;
        if inspect.code != Some(0) {
            return Err(ExecutionError::Infrastructure);
        }
        super::rust_applied::verify_coverage(
            &inspect.stdout,
            self.image_id(),
            phase,
            volume,
            output,
            target,
            nonce,
        )?;
        if self
            .inner
            .control(&["container".into(), "start".into(), name.into()])?
            .code
            != Some(0)
            || !super::mutation_gateway::running(self, name, nonce, budget.deadline, budget.cancel)?
        {
            return Err(ExecutionError::Infrastructure);
        }
        Ok(())
    }

    pub(super) fn start_nextest_output_guardian(
        &self,
        request: PhaseRequest<'_>,
        junit: &super::mutation_gateway::MutationVolume,
        budget: &WorkBudget<'_>,
    ) -> Result<(), ExecutionError> {
        let PhaseRequest {
            name,
            nonce,
            volume,
            phase,
        } = request;
        if !matches!(phase, Phase::GuardNextestOutput) || budget.stop().is_some() {
            return Err(ExecutionError::InvalidConfiguration);
        }
        if !self.absent("container", name)? {
            return Err(ExecutionError::CleanupUncertain);
        }
        let arguments = self.arguments_with_junit(name, nonce, volume, phase, junit, true)?;
        if self.inner.control(&arguments)?.code != Some(0) {
            return Err(ExecutionError::Infrastructure);
        }
        let inspect = self
            .inner
            .control(&["container".into(), "inspect".into(), name.into()])?;
        if inspect.code != Some(0) {
            return Err(ExecutionError::Infrastructure);
        }
        super::rust_applied::verify_nextest(
            &inspect.stdout,
            self.image_id(),
            phase,
            volume,
            junit,
            true,
            nonce,
        )?;
        if self
            .inner
            .control(&["container".into(), "start".into(), name.into()])?
            .code
            != Some(0)
            || !super::mutation_gateway::running(self, name, nonce, budget.deadline, budget.cancel)?
        {
            return Err(ExecutionError::Infrastructure);
        }
        Ok(())
    }
    pub fn execute(
        &self,
        source: &SourceBundle,
        command: RustCommand,
        limits: ExecutionLimits,
        cancel: &dyn ExecutionCancellation,
    ) -> Result<ExecutionResult, ExecutionError> {
        if !self.verified.load(Ordering::Acquire) {
            return Err(ExecutionError::Denied);
        }
        self.execute_observed(source, command, limits, cancel, Admission::Project)
    }
    pub(super) fn execute_calibration(
        &self,
        source: &SourceBundle,
        command: RustCommand,
        limits: ExecutionLimits,
        cancel: &dyn ExecutionCancellation,
    ) -> Result<ExecutionResult, ExecutionError> {
        self.execute_observed(
            source,
            command,
            limits,
            cancel,
            Admission::Calibration(None),
        )
    }
    pub(super) fn execute_observed(
        &self,
        source: &SourceBundle,
        command: RustCommand,
        limits: ExecutionLimits,
        cancel: &dyn ExecutionCancellation,
        admission: Admission<'_>,
    ) -> Result<ExecutionResult, ExecutionError> {
        let started = Instant::now();
        let _busy = match self.inner.busy.try_lock() {
            Ok(g) => g,
            Err(std::sync::TryLockError::WouldBlock) => return Err(ExecutionError::Busy),
            Err(_) => {
                self.inner.quarantined.store(true, Ordering::Release);
                return Err(ExecutionError::CleanupUncertain);
            }
        };
        if self.is_quarantined() {
            return Err(ExecutionError::CleanupUncertain);
        }
        // Recheck under the job lock: calibration may have revoked an earlier
        // observation while this caller was entering the gateway.
        if matches!(admission, Admission::Project)
            && (self.calibrating.load(Ordering::Acquire) || !self.verified.load(Ordering::Acquire))
        {
            return Err(ExecutionError::Denied);
        }
        if cancel.is_cancelled() {
            return Err(ExecutionError::Cancelled);
        }
        if digest(&state::executable_bytes(&self.inner.config.executable)?)
            != self.inner.executable_digest
        {
            return Err(ExecutionError::Unavailable);
        }
        let current = self
            .inner
            .control(&["info".into(), "--format={{json .}}".into()])?;
        let engine: EngineIdentity =
            serde_json::from_slice(&current.stdout).map_err(|_| ExecutionError::Unavailable)?;
        if current.code != Some(0) || engine != self.inner.engine {
            return Err(ExecutionError::Unavailable);
        }
        let archive = super::source_archive::encode(source)?;
        let budget = WorkBudget {
            started,
            deadline: started + Duration::from_millis(limits.wall_ms()),
            limits,
            cancel,
        };
        let nonce = state::nonce()?;
        let volume = format!("rust-mcp-source-{nonce}");
        let ingest = format!("rust-mcp-ingest-{nonce}");
        let run = format!("rust-mcp-cargo-{nonce}");
        let admission_scope = match admission {
            Admission::Project => "project",
            Admission::Calibration(_) => "calibration",
        };
        if let Admission::Calibration(Some(observer)) = admission {
            *observer
                .lock()
                .map_err(|_| ExecutionError::Infrastructure)? = Some(run.clone());
        }
        if !self.absent("volume", &volume)? {
            return Err(ExecutionError::CleanupUncertain);
        }
        let work = (|| {
            if let Some(stop) = budget.stop() {
                return Ok((budget.stopped_capture(stop), None));
            }
            let mut args = vec!["volume".into(), "create".into(), "--driver=local".into()];
            for (k, v) in labels(&nonce) {
                args.push(format!("--label={k}={v}"));
            }
            args.push(volume.clone());
            if self.inner.control(&args)?.code != Some(0) {
                return Err(ExecutionError::Infrastructure);
            }
            let inspect =
                self.inner
                    .control(&["volume".into(), "inspect".into(), volume.clone()])?;
            if inspect.code != Some(0) {
                return Err(ExecutionError::Infrastructure);
            }
            let v = Volume::parse(&inspect.stdout, &volume, &nonce)?;
            let (ingested, ingest_oom) = self.phase(
                PhaseRequest {
                    name: &ingest,
                    nonce: &nonce,
                    volume: &v,
                    phase: &Phase::Ingest,
                },
                &archive,
                &budget,
            )?;
            if ingested.stop != Stop::Exited {
                return Ok((ingested, ingest_oom));
            }
            if ingested.code != Some(0) {
                return Err(ExecutionError::Infrastructure);
            }
            self.inner.remove(&ingest)?;
            // No source writer remains while untrusted code executes.
            self.phase(
                PhaseRequest {
                    name: &run,
                    nonce: &nonce,
                    volume: &v,
                    phase: &Phase::Run(command.clone()),
                },
                &[],
                &budget,
            )
        })();
        let terminal_signal = budget.stop();
        self.cleanup(&ingest, &run, &volume, &nonce)?;
        let (outcome, oom_killed) = finish_work(work, terminal_signal)?;
        let (stdout, expanded_out) = bounded_text(&outcome.stdout, limits.output_bytes());
        let (stderr, expanded_err) = bounded_text(&outcome.stderr, limits.output_bytes());
        let termination = if expanded_out || expanded_err {
            ExecutionTermination::OutputLimit
        } else {
            match outcome.stop {
                Stop::Exited => ExecutionTermination::Exited,
                Stop::Cancelled => ExecutionTermination::Cancelled,
                Stop::TimedOut => ExecutionTermination::TimedOut,
                Stop::OutputLimit => ExecutionTermination::OutputLimit,
            }
        };
        let identity = serde_json::to_vec(&(
            self.configuration_fingerprint()?,
            command,
            limits,
            digest(&archive),
            admission_scope,
            "rust-source-profile-v1",
        ))
        .map_err(|_| ExecutionError::Infrastructure)?;
        Ok(ExecutionResult {
            termination,
            exit_code: if outcome.stop == Stop::Exited {
                outcome.code
            } else {
                None
            },
            oom_killed,
            stdout,
            stderr,
            stdout_truncated: outcome.stdout_truncated || expanded_out,
            stderr_truncated: outcome.stderr_truncated || expanded_err,
            duration_ms: outcome.duration_ms,
            total_duration_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
            execution_fingerprint: digest(&identity)
                .parse()
                .map_err(|_| ExecutionError::Infrastructure)?,
            platform: "linux/aarch64",
            image_id: self.image_id().into(),
        })
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use rust_engineering_domain::SourceFile;
    #[test]
    fn explain_command_accepts_only_validated_code_as_one_separate_argument()
    -> Result<(), Box<dyn std::error::Error>> {
        use rust_engineering_domain::DiagnosticCode;
        for code in ["E0502", "E0000", "E9999"] {
            let phase = Phase::Run(RustCommand::Explain(code.parse()?));
            assert_eq!(phase.program(), "/opt/rust/bin/rustc");
            assert_eq!(phase.arguments(), ["--explain", code, "--color", "never"]);
            assert_eq!(phase.user(), "65534:65534");
            assert!(!phase.ingesting());
        }
        for invalid in [
            "E0502 --help",
            "E0502;id",
            "$(id)",
            "E0502\n",
            "--help",
            "e0502",
            "E０５０２",
        ] {
            assert!(invalid.parse::<DiagnosticCode>().is_err(), "{invalid:?}");
        }
        assert_ne!(
            serde_json::to_vec(&RustCommand::Explain("E0502".parse()?))?,
            serde_json::to_vec(&RustCommand::Explain("E9999".parse()?))?
        );
        Ok(())
    }
    #[test]
    fn test_command_seals_harness_args_and_filter_position()
    -> Result<(), Box<dyn std::error::Error>> {
        use rust_engineering_domain::TestSelection;
        let options = TestSelection {
            package: Some("app".into()),
            test_filter: Some("tests::works".into()),
            features: vec!["extra".into()],
            target: Some("aarch64-unknown-linux-gnu".into()),
            timeout: 60,
            ..Default::default()
        }
        .try_into()?;
        let phase = Phase::Run(RustCommand::TestProject(options));
        assert_eq!(phase.program(), "/opt/rust/bin/cargo");
        assert_eq!(
            phase.arguments(),
            [
                "test",
                "--frozen",
                "--message-format=json",
                "--jobs=1",
                "--color=never",
                "--package=app",
                "--features=extra",
                "--target=aarch64-unknown-linux-gnu",
                "tests::works",
                "--",
                "--test-threads=1",
                "--color=never"
            ]
        );
        let all = Phase::Run(RustCommand::TestProject(
            TestSelection {
                all_features: true,
                ..Default::default()
            }
            .try_into()?,
        ));
        assert_eq!(
            all.arguments(),
            [
                "test",
                "--frozen",
                "--message-format=json",
                "--jobs=1",
                "--color=never",
                "--all-features",
                "--",
                "--test-threads=1",
                "--color=never"
            ]
        );
        Ok(())
    }
    #[test]
    fn nextest_command_seals_config_path_profile_and_selection_argv()
    -> Result<(), Box<dyn std::error::Error>> {
        use rust_engineering_domain::TestSelection;
        use rust_engineering_domain::nextest::{NextestCommandOptions, NextestSelection};
        let options: NextestCommandOptions = NextestSelection {
            package: Some("app".into()),
            test_filter: Some("tests::works".into()),
            features: vec!["extra".into()],
            target: Some("aarch64-unknown-linux-gnu".into()),
            no_default_features: true,
            timeout: 60,
            retries: 2,
            ..Default::default()
        }
        .try_into()?;
        let phase = Phase::Run(RustCommand::TestNextest(options));
        assert_eq!(phase.seccomp_profile_name(), "seccomp-rust-quality.json");
        assert_eq!(phase.program(), "/opt/rust/bin/cargo");
        assert_eq!(
            phase.arguments(),
            [
                "nextest",
                "run",
                "--config-file",
                nextest_gateway::NEXTEST_CONFIG_GUEST_PATH,
                "--profile",
                "rust-mcp",
                "--frozen",
                "--offline",
                "--color=never",
                "--no-fail-fast",
                "--build-jobs=1",
                "--test-threads=1",
                "--package=app",
                "--features=extra",
                "--no-default-features",
                "--target=aarch64-unknown-linux-gnu",
                "tests::works",
            ]
        );
        // Retries never appear in argv: they are carried only in the
        // product-owned generated config file, so argv shape is stable
        // regardless of the caller's retry request.
        let no_selection = Phase::Run(RustCommand::TestNextest(
            NextestSelection::default().try_into()?,
        ));
        assert_eq!(
            Phase::Run(RustCommand::Check).seccomp_profile_name(),
            "seccomp-rust.json"
        );
        assert_eq!(Phase::Ingest.seccomp_profile_name(), "seccomp-rust.json");
        assert_eq!(
            no_selection.arguments(),
            [
                "nextest",
                "run",
                "--config-file",
                nextest_gateway::NEXTEST_CONFIG_GUEST_PATH,
                "--profile",
                "rust-mcp",
                "--frozen",
                "--offline",
                "--color=never",
                "--no-fail-fast",
                "--build-jobs=1",
                "--test-threads=1",
            ]
        );
        assert_ne!(
            serde_json::to_vec(&RustCommand::TestNextest(
                NextestSelection::default().try_into()?
            ))?,
            serde_json::to_vec(&RustCommand::TestProject(
                TestSelection::default().try_into()?
            ))?
        );
        Ok(())
    }
    #[test]
    fn clippy_profiles_and_selections_have_closed_argv() -> Result<(), Box<dyn std::error::Error>> {
        use rust_engineering_domain::{ClippySelection, LintProfile};
        for (lint_profile, suffix) in [
            (LintProfile::Default, vec![]),
            (LintProfile::Project, vec![]),
            (LintProfile::Strict, vec!["--", "-D", "warnings"]),
            (LintProfile::Pedantic, vec!["--", "-W", "clippy::pedantic"]),
        ] {
            let options = ClippySelection {
                package: Some("app".into()),
                features: vec!["serde/derive".into()],
                all_targets: true,
                lint_profile,
                ..Default::default()
            }
            .try_into()?;
            let phase = Phase::Run(RustCommand::ClippyProject(options));
            let mut expected = vec![
                "clippy",
                "--frozen",
                "--message-format=json",
                "--jobs=1",
                "--package=app",
                "--features=serde/derive",
                "--all-targets",
            ];
            expected.extend(suffix);
            assert_eq!(phase.arguments(), expected);
            assert_eq!(phase.program(), "/opt/rust/bin/cargo");
        }
        let workspace = Phase::Run(RustCommand::ClippyProject(
            ClippySelection {
                workspace: true,
                ..Default::default()
            }
            .try_into()?,
        ));
        assert_eq!(
            workspace.arguments(),
            [
                "clippy",
                "--frozen",
                "--message-format=json",
                "--jobs=1",
                "--workspace"
            ]
        );
        Ok(())
    }
    #[test]
    fn semver_argv_environment_and_quality_profile_are_closed()
    -> Result<(), Box<dyn std::error::Error>> {
        use rust_engineering_domain::semver_check::SemverProjectSelection;
        let phase = Phase::Run(RustCommand::SemverCheck(
            SemverProjectSelection {
                package: Some("api".into()),
                features: vec!["extra".into()],
                no_default_features: true,
                target: Some("aarch64-unknown-linux-gnu".into()),
                ..Default::default()
            }
            .try_into()?,
        ));
        assert_eq!(phase.seccomp_profile_name(), "seccomp-rust-quality.json");
        assert_eq!(
            phase.arguments(),
            [
                "semver-checks",
                "check-release",
                "--manifest-path",
                "/source/Cargo.toml",
                "--baseline-root",
                "/baseline",
                "--color",
                "never",
                "--package=api",
                "--features=extra",
                "--only-explicit-features",
                "--target=aarch64-unknown-linux-gnu"
            ]
        );
        let env = phase.environment();
        assert!(env.contains(&"GIT_DIR=/nonexistent".into()));
        assert!(env.contains(&"GIT_CEILING_DIRECTORIES=/".into()));
        assert!(env.contains(&"NO_COLOR=1".into()));
        assert_eq!(
            Phase::Run(RustCommand::SemverChecksVersion).seccomp_profile_name(),
            "seccomp-rust.json"
        );
        assert_eq!(
            Phase::IngestBaseline.arguments(),
            [
                "--extract",
                "--file=-",
                "--directory=/baseline",
                "--no-same-owner",
                "--no-same-permissions",
                "--keep-old-files"
            ]
        );
        Ok(())
    }
    #[test]
    fn coverage_environment_is_scoped_once_to_coverage_phases()
    -> Result<(), Box<dyn std::error::Error>> {
        let base = environment();
        for key in ["LLVM_COV=", "LLVM_PROFDATA=", "CARGO_LLVM_COV_TARGET_DIR="] {
            assert_eq!(
                base.iter().filter(|value| value.starts_with(key)).count(),
                0
            );
        }
        for phase in [
            Phase::Run(RustCommand::CoverageRun(
                rust_engineering_domain::coverage::CoverageSelection::default().try_into()?,
            )),
            Phase::Run(RustCommand::CoverageReport(
                rust_engineering_domain::coverage::CoverageReportFormat::Json,
            )),
        ] {
            let applied = phase.environment();
            for key in ["LLVM_COV=", "LLVM_PROFDATA=", "CARGO_LLVM_COV_TARGET_DIR="] {
                assert_eq!(
                    applied
                        .iter()
                        .filter(|value| value.starts_with(key))
                        .count(),
                    1,
                    "{key} must be present exactly once"
                );
            }
            if matches!(phase, Phase::Run(RustCommand::CoverageReport(_))) {
                assert!(
                    !phase
                        .arguments()
                        .iter()
                        .any(|argument| argument.starts_with("--jobs"))
                );
            }
        }
        Ok(())
    }
    #[test]
    fn formatting_command_is_closed_and_cannot_write_source() {
        let phase = Phase::Run(RustCommand::FormatCheck);
        assert_eq!(phase.program(), "/opt/rust/bin/cargo");
        assert_eq!(
            phase.arguments(),
            [
                "fmt",
                "--all",
                "--check",
                "--",
                "--color",
                "never",
                "--config",
                "disable_all_formatting=false"
            ]
        );
        assert!(environment().contains(&"RUSTFMT=/opt/rust/bin/rustfmt".into()));
    }
    #[test]
    fn check_arguments_are_closed_separate_and_fingerprintable()
    -> Result<(), Box<dyn std::error::Error>> {
        use rust_engineering_domain::CheckSelection;
        let options = CheckSelection {
            package: Some("member".into()),
            features: vec!["z".into(), "dep/feature".into()],
            no_default_features: true,
            all_targets: true,
            target: Some("aarch64-unknown-linux-gnu".into()),
            ..Default::default()
        }
        .try_into()?;
        let phase = Phase::Run(RustCommand::CheckProject(options));
        assert_eq!(phase.program(), "/opt/rust/bin/cargo");
        assert_eq!(
            phase.arguments(),
            [
                "check",
                "--frozen",
                "--message-format=json",
                "--jobs=1",
                "--package=member",
                "--features=dep/feature,z",
                "--no-default-features",
                "--all-targets",
                "--target=aarch64-unknown-linux-gnu"
            ]
        );
        let workspace = Phase::Run(RustCommand::CheckProject(
            CheckSelection {
                workspace: true,
                all_features: true,
                ..Default::default()
            }
            .try_into()?,
        ));
        assert_eq!(
            workspace.arguments(),
            [
                "check",
                "--frozen",
                "--message-format=json",
                "--jobs=1",
                "--workspace",
                "--all-features"
            ]
        );
        assert_ne!(
            serde_json::to_vec(&RustCommand::Check)?,
            serde_json::to_vec(&RustCommand::CheckProject(
                CheckSelection::default().try_into()?
            ))?
        );
        assert!(environment().contains(&"CARGO_NET_OFFLINE=true".into()));
        Ok(())
    }
    #[test]
    fn installed_components_command_is_fixed_and_has_no_peer_arguments() {
        let phase = Phase::Run(RustCommand::InstalledComponents);
        assert_eq!(phase.program(), "/usr/bin/cat");
        assert_eq!(
            phase.arguments(),
            ["--", "/opt/rust/lib/rustlib/components"]
        );
        assert!(!phase.ingesting());
        assert_eq!(phase.user(), "65534:65534");
    }
    #[test]
    fn cancellation_and_deadlines_never_mask_harness_or_cleanup_errors() {
        for error in [
            ExecutionError::InvalidConfiguration,
            ExecutionError::Unavailable,
            ExecutionError::Denied,
            ExecutionError::Busy,
            ExecutionError::Cancelled,
            ExecutionError::Infrastructure,
            ExecutionError::CleanupUncertain,
        ] {
            for signal in [None, Some(Stop::TimedOut), Some(Stop::Cancelled)] {
                assert_eq!(finish_work(Err(error), signal).err(), Some(error));
            }
        }
    }
    #[test]
    fn completion_preserves_overflow_and_observed_oom() -> Result<(), ExecutionError> {
        let capture = Capture {
            code: None,
            stdout: vec![],
            stderr: vec![],
            stdout_truncated: true,
            stderr_truncated: false,
            stop: Stop::OutputLimit,
            duration_ms: 1,
        };
        let (capture, oom) = finish_work(Ok((capture, Some(true))), Some(Stop::Cancelled))?;
        assert_eq!(capture.stop, Stop::OutputLimit);
        assert_eq!(oom, Some(true));
        Ok(())
    }
    #[test]
    #[ignore = "Requires explicit local Docker socket and approved Rust image"]
    fn benign_source_transfer_compiles_with_empty_directory() -> Result<(), String> {
        let socket = std::env::var_os("RUST_MCP_TEST_SOCKET").ok_or("explicit socket required")?;
        let root = PathBuf::from("/private/tmp").join(format!(
            "rust-mcp-rust-test-{}",
            state::nonce().map_err(|e| format!("{e:?}"))?
        ));
        std::fs::create_dir(&root).map_err(|e| e.to_string())?;
        struct Root(PathBuf);
        impl Drop for Root {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _root = Root(root.clone());
        let gateway = RustGateway::new(HostDockerConfig {
            executable: "/Applications/Docker.app/Contents/Resources/bin/docker".into(),
            socket: socket.into(),
            state_root: root,
            image_id: APPROVED_RUST_IMAGE.into(),
        })
        .map_err(|e| format!("create: {e:?}"))?;
        let files = [
            (
                "Cargo.toml",
                "[package]\nname='m1_transfer'\nversion='0.1.0'\nedition='2024'\n",
            ),
            (
                "Cargo.lock",
                "version = 4\n[[package]]\nname = 'm1_transfer'\nversion = '0.1.0'\n",
            ),
            ("src/lib.rs", "pub fn answer() -> u8 { 42 }\n"),
            (
                "build.rs",
                "fn main() { assert!(std::path::Path::new(\"empty\").is_dir()); assert!(std::path::Path::new(&\"a\".repeat(100)).is_dir()); }\n",
            ),
        ]
        .into_iter()
        .map(|(p, b)| SourceFile::new(p.into(), b.as_bytes().to_vec()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("{e:?}"))?;
        let source = SourceBundle::with_directories(files, vec!["empty".into(), "a".repeat(100)])
            .map_err(|e| format!("{e:?}"))?;
        let limits = ExecutionLimits::new(30_000, 256 * 1024).ok_or("limits")?;
        assert!(matches!(
            gateway.execute(&source, RustCommand::Check, limits, &NeverCancel),
            Err(ExecutionError::Denied)
        ));
        let result = gateway
            .execute_calibration(&source, RustCommand::Check, limits, &NeverCancel)
            .map_err(|e| format!("execute: {e:?}"))?;
        assert_eq!(
            result.termination,
            ExecutionTermination::Exited,
            "{}",
            result.stderr
        );
        assert_eq!(result.exit_code, Some(0), "{}", result.stderr);
        assert!(result.stdout.contains("build-finished"));
        assert!(!gateway.is_quarantined());
        println!(
            "{}",
            serde_json::to_string(&result).map_err(|e| e.to_string())?
        );
        Ok(())
    }
}

#[cfg(all(test, target_os = "macos"))]
mod test_runtime;
