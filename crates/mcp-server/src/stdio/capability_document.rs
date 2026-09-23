//! spec §56 static capabilities document (M8-02 decision 3): the
//! `rust-engineering-mcp contract` CLI subcommand. Builds the same 36 tool
//! definitions `list_tools` advertises, purely from each tool module's static
//! `definition()`/`new()` constructor -- no project root, Docker or network is
//! touched, so this runs identically on every host and CI target.
use std::collections::BTreeMap;

use rmcp::model::{ErrorData, Tool};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::resources::hex;
use super::stability::{Stability, stability};

pub(super) const DOCUMENT_KIND: &str = "rust_engineering_capabilities";
pub(super) const FORMAT_VERSION: u32 = 1;

/// Mirrors `stdio::SUPPORTED_VERSIONS`: the newest entry there. Kept as a
/// literal (not derived from `ProtocolVersion`) because the document is a
/// plain string contract, not a wire negotiation;
/// [`tests::negotiable_versions_match_supported_versions_strings`] guards
/// against the two lists silently drifting apart in value, not just length.
const PRIMARY_PROTOCOL_VERSION: &str = "2026-07-28";
/// Mirrors `stdio::SUPPORTED_VERSIONS`, oldest first.
const NEGOTIABLE_PROTOCOL_VERSIONS: &[&str] = &[
    "2024-11-05",
    "2025-03-26",
    "2025-06-18",
    "2025-11-25",
    "2026-07-28",
];
/// Kept as a literal because the document is a plain string contract, not a
/// build artifact; [`tests::sdk_literal_matches_the_rmcp_version_in_cargo_lock`]
/// guards against drifting from the `rmcp` version actually locked.
const SDK: &str = "rmcp 3.2.0";

/// Mirrors `resources::PREFIX`/`QUALITY_PREFIX`/`QUALITY_TEMPLATE_SUFFIX`
/// (spec §57): the two dynamic Resource URI templates, neither advertised in
/// `resources/list`. The quality template is built from the same
/// `resources::QUALITY_TEMPLATE_SUFFIX` constant `stdio::list_resource_templates`
/// uses (S-2), so the wire template and this static document cannot drift in
/// text; [`tests::resource_templates_are_built_from_the_shared_resources_module_source_of_truth`]
/// and `tests/protocol.rs`'s wire ↔ document comparison guard the exact value.
fn resource_templates() -> [String; 2] {
    [
        format!(
            "{}{{project_ref}}/{{artifact_id}}",
            super::resources::PREFIX
        ),
        format!(
            "{}{}",
            super::resources::QUALITY_PREFIX,
            super::resources::QUALITY_TEMPLATE_SUFFIX
        ),
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum RequiredRuntime {
    None,
    DockerRust,
    DockerScanner,
    Catalog,
    Analyzer,
}

#[derive(Clone, Copy)]
struct ToolClass {
    executes_project_code: bool,
    requires_runtime: RequiredRuntime,
}

/// Closed table (M8-02 decision 3; semantics fixed in V02-review-freeze
/// disposition P2-1), sourced from `docs/architecture/decisions.md` (historical
/// receipt: docs/validation/M8/01-census.json at 51fa602e)
/// `tools[].requires_runtime`.
///
/// `executes_project_code` is `true` iff the tool can execute build scripts,
/// proc macros, tests or binaries of the project inside the guest: `rust.check`,
/// `rust.clippy`, `rust.test`, `rust.test.nextest`, `rust.quality.gate`,
/// `rust.quality.gate.v2`, `rust.coverage`, `rust.semver.check` (`cargo
/// semver-checks` compiles the crate), `rust.mutation.test`, `rust.miri`,
/// `rust.benchmark.run`, `rust.profile.flamegraph`, `rust.binary.bloat` and
/// `rust.fix.apply` -- 14 tools, fixed by
/// [`tests::executes_project_code_matches_the_fixed_fourteen_tool_set`]. It is
/// `false` for every other tool, including `rust.fmt.check`/`rust.fmt.apply`
/// (rustfmt only, no build scripts or proc macros), `rust.benchmark.compare`
/// (reads two prior JSON reports, spawns no process) and the five
/// `rust.analyzer.*` tools (build scripts, proc macros and check-on-save stay
/// disabled; only `textDocument/*` runs).
/// [`tests::table_matches_the_full_tool_set`] checks this is exhaustive over
/// the real 36-tool surface.
const TOOL_CLASSES: &[(&str, ToolClass)] = &[
    (
        super::project::NAME,
        ToolClass {
            executes_project_code: false,
            requires_runtime: RequiredRuntime::None,
        },
    ),
    (
        super::inspection::NAME,
        ToolClass {
            executes_project_code: false,
            requires_runtime: RequiredRuntime::DockerRust,
        },
    ),
    (
        super::toolchain::NAME,
        ToolClass {
            executes_project_code: false,
            requires_runtime: RequiredRuntime::DockerRust,
        },
    ),
    (
        super::check::NAME,
        ToolClass {
            executes_project_code: true,
            requires_runtime: RequiredRuntime::DockerRust,
        },
    ),
    (
        super::format::NAME,
        ToolClass {
            executes_project_code: false,
            requires_runtime: RequiredRuntime::DockerRust,
        },
    ),
    (
        super::clippy::NAME,
        ToolClass {
            executes_project_code: true,
            requires_runtime: RequiredRuntime::DockerRust,
        },
    ),
    (
        super::testing::NAME,
        ToolClass {
            executes_project_code: true,
            requires_runtime: RequiredRuntime::DockerRust,
        },
    ),
    (
        super::nextest::NAME,
        ToolClass {
            executes_project_code: true,
            requires_runtime: RequiredRuntime::DockerRust,
        },
    ),
    (
        super::auditing::NAME,
        ToolClass {
            executes_project_code: false,
            requires_runtime: RequiredRuntime::Catalog,
        },
    ),
    (
        super::explaining::NAME,
        ToolClass {
            executes_project_code: false,
            requires_runtime: RequiredRuntime::DockerRust,
        },
    ),
    (
        super::quality::NAME,
        ToolClass {
            executes_project_code: true,
            requires_runtime: RequiredRuntime::DockerRust,
        },
    ),
    (
        super::catalog::NAME,
        ToolClass {
            executes_project_code: false,
            requires_runtime: RequiredRuntime::Catalog,
        },
    ),
    (
        super::crate_search::NAME,
        ToolClass {
            executes_project_code: false,
            requires_runtime: RequiredRuntime::Catalog,
        },
    ),
    (
        super::crate_inspect::NAME,
        ToolClass {
            executes_project_code: false,
            requires_runtime: RequiredRuntime::Catalog,
        },
    ),
    (
        super::mutation::NAME,
        ToolClass {
            executes_project_code: false,
            requires_runtime: RequiredRuntime::DockerRust,
        },
    ),
    (
        super::mutation::FORMAT_NAME,
        ToolClass {
            executes_project_code: false,
            requires_runtime: RequiredRuntime::DockerRust,
        },
    ),
    (
        super::mutation::FIX_NAME,
        ToolClass {
            executes_project_code: true,
            requires_runtime: RequiredRuntime::DockerRust,
        },
    ),
    (
        super::mutation::DEPENDENCY_ADD_NAME,
        ToolClass {
            executes_project_code: false,
            requires_runtime: RequiredRuntime::DockerRust,
        },
    ),
    (
        super::mutation::DEPENDENCY_REMOVE_NAME,
        ToolClass {
            executes_project_code: false,
            requires_runtime: RequiredRuntime::DockerRust,
        },
    ),
    (
        super::coverage::NAME,
        ToolClass {
            executes_project_code: true,
            requires_runtime: RequiredRuntime::DockerRust,
        },
    ),
    (
        super::semver::NAME,
        ToolClass {
            executes_project_code: true,
            requires_runtime: RequiredRuntime::DockerRust,
        },
    ),
    (
        super::mutation_test::NAME,
        ToolClass {
            executes_project_code: true,
            requires_runtime: RequiredRuntime::DockerRust,
        },
    ),
    (
        super::deny::NAME,
        ToolClass {
            executes_project_code: false,
            requires_runtime: RequiredRuntime::DockerRust,
        },
    ),
    (
        super::unsafe_scan::NAME,
        ToolClass {
            executes_project_code: false,
            requires_runtime: RequiredRuntime::DockerScanner,
        },
    ),
    (
        super::supply_chain::NAME,
        ToolClass {
            executes_project_code: false,
            requires_runtime: RequiredRuntime::DockerRust,
        },
    ),
    (
        super::quality_v2::NAME,
        ToolClass {
            executes_project_code: true,
            requires_runtime: RequiredRuntime::DockerRust,
        },
    ),
    (
        super::miri::NAME,
        ToolClass {
            executes_project_code: true,
            requires_runtime: RequiredRuntime::DockerRust,
        },
    ),
    (
        super::benchmark::NAME,
        ToolClass {
            executes_project_code: true,
            requires_runtime: RequiredRuntime::DockerRust,
        },
    ),
    (
        super::benchmark_compare::NAME,
        ToolClass {
            executes_project_code: false,
            requires_runtime: RequiredRuntime::None,
        },
    ),
    (
        super::profile::NAME,
        ToolClass {
            executes_project_code: true,
            requires_runtime: RequiredRuntime::DockerRust,
        },
    ),
    (
        super::bloat::NAME,
        ToolClass {
            executes_project_code: true,
            requires_runtime: RequiredRuntime::DockerRust,
        },
    ),
    (
        super::analyzer::NAME,
        ToolClass {
            executes_project_code: false,
            requires_runtime: RequiredRuntime::Analyzer,
        },
    ),
    (
        super::analyzer::REFERENCES_NAME,
        ToolClass {
            executes_project_code: false,
            requires_runtime: RequiredRuntime::Analyzer,
        },
    ),
    (
        super::analyzer::DIAGNOSTICS_NAME,
        ToolClass {
            executes_project_code: false,
            requires_runtime: RequiredRuntime::Analyzer,
        },
    ),
    (
        super::analyzer::ACTIONS_NAME,
        ToolClass {
            executes_project_code: false,
            requires_runtime: RequiredRuntime::Analyzer,
        },
    ),
    (
        super::mutation::ANALYZER_ACTION_APPLY_NAME,
        ToolClass {
            executes_project_code: false,
            requires_runtime: RequiredRuntime::Analyzer,
        },
    ),
];

fn tool_class(name: &str) -> Result<ToolClass, ErrorData> {
    TOOL_CLASSES
        .iter()
        .find(|(candidate, _)| *candidate == name)
        .map(|(_, class)| *class)
        .ok_or_else(|| ErrorData::internal_error("Unclassified tool", None))
}

/// The same 36 `Tool` definitions `EngineeringServer::list_tools` advertises,
/// in the same order, built without any host runtime.
pub(super) fn tool_definitions() -> Result<Vec<Tool>, ErrorData> {
    let mut tools = vec![
        super::project::definition()?.1,
        super::inspection::definition()?.1,
        super::toolchain::definition()?.1,
        super::check::definition()?.1,
        super::format::definition()?.1,
        super::clippy::definition()?.1,
        super::testing::definition()?.1,
        super::nextest::NextestTool::new()?.definition,
        super::auditing::definition()?.1,
        super::explaining::definition()?.1,
        super::quality::definition()?.1,
        super::catalog::definition()?.1,
        super::crate_search::definition()?.1,
        super::crate_inspect::definition()?.1,
        super::mutation::ManifestMutationTool::definition()?.1,
        super::mutation::FormatMutationTool::definition()?.1,
        super::mutation::FixMutationTool::definition()?.1,
        super::mutation::DependencyAddTool::definition()?.1,
        super::mutation::DependencyRemoveTool::definition()?.1,
        super::coverage::CoverageTool::new()?.definition,
        super::semver::SemverTool::new()?.definition,
        super::mutation_test::MutationTestTool::new()?.definition,
    ];
    if super::deny::advertised() {
        tools.push(super::deny::DenyTool::new()?.definition);
    }
    if super::unsafe_scan::advertised() {
        tools.push(super::unsafe_scan::UnsafeTool::new()?.definition);
    }
    if super::supply_chain::advertised() {
        tools.push(super::supply_chain::SupplyTool::new()?.definition);
    }
    if super::quality_v2::advertised() {
        tools.push(super::quality_v2::QualityV2Tool::new()?.definition);
    }
    if super::miri::advertised() {
        tools.push(super::miri::MiriTool::new()?.definition);
    }
    if super::benchmark::advertised() {
        tools.push(super::benchmark::BenchmarkTool::new()?.definition);
    }
    if super::benchmark_compare::advertised() {
        tools.push(super::benchmark_compare::ComparisonTool::new()?.definition);
    }
    if super::profile::advertised() {
        tools.push(super::profile::ProfileTool::new()?.definition);
    }
    if super::bloat::advertised() {
        tools.push(super::bloat::BloatTool::new()?.definition);
    }
    tools.push(super::analyzer::symbols_definition()?.1);
    tools.push(super::analyzer::references_definition()?.1);
    tools.push(super::analyzer::diagnostics_definition()?.1);
    tools.push(super::analyzer::actions_definition()?.1);
    tools.push(super::mutation::analyzer_action_definition()?.1);
    Ok(tools)
}

/// Recursively rebuilds every object with its keys in sorted order, so the
/// resulting `Value` serializes deterministically regardless of the `Map`
/// implementation `serde_json` was built with.
fn canonicalize(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let sorted: BTreeMap<String, serde_json::Value> = map
                .into_iter()
                .map(|(key, value)| (key, canonicalize(value)))
                .collect();
            let mut object = serde_json::Map::new();
            for (key, value) in sorted {
                object.insert(key, value);
            }
            serde_json::Value::Object(object)
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(canonicalize).collect())
        }
        other => other,
    }
}

/// `sha256(json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False))`,
/// fixed by [`tests::canonical_hash_matches_the_python_reference_vector`].
fn canonical_hash(value: &serde_json::Value) -> Result<String, ErrorData> {
    let canonical = canonicalize(value.clone());
    let bytes = serde_json::to_vec(&canonical)
        .map_err(|_| ErrorData::internal_error("Canonical encoding failed", None))?;
    let digest: [u8; 32] = Sha256::digest(bytes).into();
    Ok(hex(&digest))
}

pub(super) fn document() -> Result<serde_json::Value, ErrorData> {
    let definitions = tool_definitions()?;
    let mut tools = serde_json::Map::new();
    for tool in &definitions {
        let value = serde_json::to_value(tool)
            .map_err(|_| ErrorData::internal_error("Tool definition encoding failed", None))?;
        let name = value["name"]
            .as_str()
            .ok_or_else(|| ErrorData::internal_error("Tool definition missing name", None))?;
        let class = tool_class(name)?;
        let input_schema = value["inputSchema"].clone();
        let output_schema = value["outputSchema"].clone();
        let description = value
            .get("description")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let entry = serde_json::json!({
            "stability": stability(name),
            "annotations": value.get("annotations").cloned().unwrap_or(serde_json::Value::Null),
            "input_schema_sha256": canonical_hash(&input_schema)?,
            "output_schema_sha256": canonical_hash(&output_schema)?,
            "description_sha256": canonical_hash(&description)?,
            "executes_project_code": class.executes_project_code,
            "requires_runtime": class.requires_runtime,
        });
        tools.insert(name.to_owned(), entry);
    }
    let resources: Vec<serde_json::Value> = resource_templates()
        .into_iter()
        .map(|template| serde_json::json!({"uri_template": template, "stability": Stability::Stable}))
        .collect();
    Ok(serde_json::json!({
        "document_kind": DOCUMENT_KIND,
        "format_version": FORMAT_VERSION,
        "server_version": env!("CARGO_PKG_VERSION"),
        "protocol": {
            "primary_version": PRIMARY_PROTOCOL_VERSION,
            "negotiable_versions": NEGOTIABLE_PROTOCOL_VERSIONS,
            "sdk": SDK,
        },
        "tools": tools,
        "resources": resources,
        "tool_count": definitions.len(),
    }))
}

fn human_report(document: &serde_json::Value) -> String {
    let mut lines = vec![format!(
        "rust-engineering-mcp contract: {} tools ({} preview)",
        document["tool_count"],
        document["tools"]
            .as_object()
            .map(|tools| tools
                .values()
                .filter(|tool| tool["stability"] == "preview")
                .count())
            .unwrap_or(0)
    )];
    let Some(tools) = document["tools"].as_object() else {
        return lines.join("\n");
    };
    let mut names: Vec<&String> = tools.keys().collect();
    names.sort();
    for name in names {
        let tool = &tools[name];
        let annotations = &tool["annotations"];
        lines.push(format!(
            "  {name:<36} {:<7} readOnly={:<5} destructive={:<5} runtime={}",
            tool["stability"].as_str().unwrap_or("?"),
            annotations["readOnlyHint"]
                .as_bool()
                .map(|value| value.to_string())
                .unwrap_or_else(|| "?".to_owned()),
            annotations["destructiveHint"]
                .as_bool()
                .map(|value| value.to_string())
                .unwrap_or_else(|| "?".to_owned()),
            tool["requires_runtime"].as_str().unwrap_or("?"),
        ));
    }
    lines.join("\n")
}

/// `json`: `--json` (also the default); `!json`: `--human`. Flag parsing
/// itself lives in `crate::contract_cli`, which owns the CLI surface.
pub(super) fn run(json: bool) -> std::process::ExitCode {
    use std::io::Write;
    let document = match document() {
        Ok(document) => document,
        Err(error) => {
            let _ = writeln!(
                std::io::stderr().lock(),
                "rust-engineering-mcp contract: failed to build the capabilities document: {}",
                error.message
            );
            return std::process::ExitCode::FAILURE;
        }
    };
    let mut bytes = if json {
        match serde_json::to_vec(&document) {
            Ok(bytes) => bytes,
            Err(error) => {
                let _ = writeln!(
                    std::io::stderr().lock(),
                    "rust-engineering-mcp contract: failed to encode the capabilities document: {error}"
                );
                return std::process::ExitCode::FAILURE;
            }
        }
    } else {
        human_report(&document).into_bytes()
    };
    bytes.push(b'\n');
    if std::io::stdout().lock().write_all(&bytes).is_ok() {
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_hash_matches_the_python_reference_vector() -> Result<(), Box<dyn std::error::Error>>
    {
        // python3 -c "import json,hashlib; obj={'café':{'b':1,'a':[1,2,3]},'z':True,'n':None}; \
        // s=json.dumps(obj, sort_keys=True, separators=(',', ':'), ensure_ascii=False); \
        // print(hashlib.sha256(s.encode()).hexdigest())"
        let value = serde_json::json!({"café": {"b": 1, "a": [1, 2, 3]}, "z": true, "n": null});
        assert_eq!(
            canonical_hash(&value)?,
            "db49e49060e3007793819427e1995a039e7c9b663eac65749be589a064a70b44"
        );
        Ok(())
    }

    #[test]
    fn canonicalize_sorts_nested_keys_regardless_of_insertion_order()
    -> Result<(), Box<dyn std::error::Error>> {
        let a = serde_json::json!({"b": 1, "a": {"y": 2, "x": 1}});
        let b = serde_json::json!({"a": {"x": 1, "y": 2}, "b": 1});
        assert_eq!(canonicalize(a.clone()), canonicalize(b.clone()));
        assert_eq!(
            serde_json::to_vec(&canonicalize(a))?,
            serde_json::to_vec(&canonicalize(b))?
        );
        Ok(())
    }

    #[test]
    fn table_matches_the_full_tool_set() -> Result<(), Box<dyn std::error::Error>> {
        let definitions = tool_definitions()?;
        assert_eq!(definitions.len(), 36);
        for tool in &definitions {
            let value = serde_json::to_value(tool)?;
            let name = value["name"]
                .as_str()
                .ok_or("tool definition missing name")?;
            tool_class(name).map_err(|_| format!("{name} is not classified"))?;
        }
        assert_eq!(TOOL_CLASSES.len(), 36);
        Ok(())
    }

    #[test]
    fn executes_project_code_matches_the_fixed_fourteen_tool_set()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut executes: Vec<&str> = TOOL_CLASSES
            .iter()
            .filter(|(_, class)| class.executes_project_code)
            .map(|(name, _)| *name)
            .collect();
        executes.sort_unstable();
        assert_eq!(
            executes,
            [
                "rust.benchmark.run",
                "rust.binary.bloat",
                "rust.check",
                "rust.clippy",
                "rust.coverage",
                "rust.fix.apply",
                "rust.miri",
                "rust.mutation.test",
                "rust.profile.flamegraph",
                "rust.quality.gate",
                "rust.quality.gate.v2",
                "rust.semver.check",
                "rust.test",
                "rust.test.nextest",
            ]
        );
        assert_eq!(
            tool_class(super::super::benchmark_compare::NAME)?.requires_runtime,
            RequiredRuntime::None
        );
        for name in [
            super::super::analyzer::NAME,
            super::super::analyzer::REFERENCES_NAME,
            super::super::analyzer::DIAGNOSTICS_NAME,
            super::super::analyzer::ACTIONS_NAME,
            super::super::mutation::ANALYZER_ACTION_APPLY_NAME,
        ] {
            assert_eq!(
                tool_class(name)?.requires_runtime,
                RequiredRuntime::Analyzer
            );
        }
        Ok(())
    }

    #[test]
    fn document_shape_is_stable() -> Result<(), Box<dyn std::error::Error>> {
        let document = document()?;
        assert_eq!(document["document_kind"], DOCUMENT_KIND);
        assert_eq!(document["format_version"], FORMAT_VERSION);
        assert_eq!(document["tool_count"], 36);
        assert_eq!(
            document["tools"].as_object().map(|tools| tools.len()),
            Some(36)
        );
        assert_eq!(document["resources"].as_array().map(|r| r.len()), Some(2));
        assert_eq!(document["protocol"]["primary_version"], "2026-07-28");
        let check = &document["tools"]["rust.check"];
        assert_eq!(check["stability"], "stable");
        assert_eq!(check["executes_project_code"], true);
        assert_eq!(check["requires_runtime"], "docker-rust");
        assert_eq!(
            check["input_schema_sha256"].as_str().map(str::len),
            Some(64)
        );
        let preview = &document["tools"]["rust.analyzer.symbols"];
        assert_eq!(preview["stability"], "preview");
        assert_eq!(preview["requires_runtime"], "analyzer");
        Ok(())
    }

    #[test]
    fn negotiable_versions_match_supported_versions_strings()
    -> Result<(), Box<dyn std::error::Error>> {
        let supported: Vec<serde_json::Value> = super::super::SUPPORTED_VERSIONS
            .iter()
            .map(serde_json::to_value)
            .collect::<Result<_, _>>()?;
        let negotiable: Vec<serde_json::Value> = NEGOTIABLE_PROTOCOL_VERSIONS
            .iter()
            .map(|version| serde_json::Value::String((*version).to_owned()))
            .collect();
        assert_eq!(
            supported, negotiable,
            "literal versions drifted from wire versions"
        );
        assert_eq!(
            Some(PRIMARY_PROTOCOL_VERSION),
            negotiable.last().and_then(|version| version.as_str())
        );
        Ok(())
    }

    #[test]
    fn sdk_literal_matches_the_rmcp_version_in_cargo_lock() -> Result<(), Box<dyn std::error::Error>>
    {
        let lock = include_str!("../../../../Cargo.lock");
        let mut lines = lock.lines();
        let rmcp_version = loop {
            let line = lines.next().ok_or("rmcp package not found in Cargo.lock")?;
            if line == "name = \"rmcp\"" {
                let version_line = lines.next().ok_or("Cargo.lock truncated after rmcp name")?;
                let version = version_line
                    .strip_prefix("version = \"")
                    .and_then(|rest| rest.strip_suffix('"'))
                    .ok_or("unexpected Cargo.lock format for rmcp version")?;
                break version;
            }
        };
        assert_eq!(SDK, format!("rmcp {rmcp_version}"));
        Ok(())
    }

    #[test]
    fn resource_templates_are_built_from_the_shared_resources_module_source_of_truth() {
        let templates = resource_templates();
        assert_eq!(templates[0], "rust-artifact://{project_ref}/{artifact_id}");
        assert_eq!(
            templates[1],
            "rust-quality-artifact://{project_ref}/{quality_job_id_or_artifact_id}{?offset,length}"
        );
        assert!(templates[0].starts_with(super::super::resources::PREFIX));
        assert!(templates[1].starts_with(super::super::resources::QUALITY_PREFIX));
    }

    #[test]
    fn human_report_lists_every_tool_once() -> Result<(), Box<dyn std::error::Error>> {
        let document = document()?;
        let report = human_report(&document);
        assert!(report.starts_with("rust-engineering-mcp contract: 36 tools (5 preview)"));
        for tool in document["tools"].as_object().ok_or("tools object")?.keys() {
            assert!(
                report.contains(tool.as_str()),
                "{tool} missing from human report"
            );
        }
        Ok(())
    }
}
