//! ADR-086 §8 stability classification (M8-02 decision 4): the five
//! `rust.analyzer.*` tools are `preview`, the other 31 are `stable`. The
//! classification is a closed table, not a heuristic on the tool name, so an
//! added or renamed tool is caught by [`tests::table_matches_the_full_tool_set`]
//! rather than silently defaulting.
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Stability {
    Stable,
    Preview,
}

/// Prefixed onto the `description` of every `preview`-class tool, at the
/// point each one builds its `Tool` definition.
pub(super) const PREVIEW_PREFIX: &str = "Preview (ADR-086): ";

const TOOL_STABILITY: &[(&str, Stability)] = &[
    (super::project::NAME, Stability::Stable),
    (super::inspection::NAME, Stability::Stable),
    (super::toolchain::NAME, Stability::Stable),
    (super::check::NAME, Stability::Stable),
    (super::format::NAME, Stability::Stable),
    (super::clippy::NAME, Stability::Stable),
    (super::testing::NAME, Stability::Stable),
    (super::nextest::NAME, Stability::Stable),
    (super::auditing::NAME, Stability::Stable),
    (super::explaining::NAME, Stability::Stable),
    (super::quality::NAME, Stability::Stable),
    (super::catalog::NAME, Stability::Stable),
    (super::crate_search::NAME, Stability::Stable),
    (super::crate_inspect::NAME, Stability::Stable),
    (super::mutation::NAME, Stability::Stable),
    (super::mutation::FORMAT_NAME, Stability::Stable),
    (super::mutation::FIX_NAME, Stability::Stable),
    (super::mutation::DEPENDENCY_ADD_NAME, Stability::Stable),
    (super::mutation::DEPENDENCY_REMOVE_NAME, Stability::Stable),
    (super::coverage::NAME, Stability::Stable),
    (super::semver::NAME, Stability::Stable),
    (super::mutation_test::NAME, Stability::Stable),
    (super::deny::NAME, Stability::Stable),
    (super::unsafe_scan::NAME, Stability::Stable),
    (super::supply_chain::NAME, Stability::Stable),
    (super::quality_v2::NAME, Stability::Stable),
    (super::miri::NAME, Stability::Stable),
    (super::benchmark::NAME, Stability::Stable),
    (super::benchmark_compare::NAME, Stability::Stable),
    (super::profile::NAME, Stability::Stable),
    (super::bloat::NAME, Stability::Stable),
    (super::analyzer::NAME, Stability::Preview),
    (super::analyzer::REFERENCES_NAME, Stability::Preview),
    (super::analyzer::DIAGNOSTICS_NAME, Stability::Preview),
    (super::analyzer::ACTIONS_NAME, Stability::Preview),
    (
        super::mutation::ANALYZER_ACTION_APPLY_NAME,
        Stability::Preview,
    ),
];

/// Total over every `&str`: an unrecognized name is `stable` by definition
/// (it is not one of the five closed `preview` names), but the closed table
/// above is exhaustively checked against the real 36-tool surface in tests,
/// so a tool this table forgets is a test failure, not a silent default.
pub(super) fn stability(tool_name: &str) -> Stability {
    TOOL_STABILITY
        .iter()
        .find(|(name, _)| *name == tool_name)
        .map(|(_, stability)| *stability)
        .unwrap_or(Stability::Stable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_matches_the_full_tool_set() -> Result<(), Box<dyn std::error::Error>> {
        let definitions = crate::stdio::capability_document::tool_definitions()?;
        assert_eq!(definitions.len(), 36);
        assert_eq!(TOOL_STABILITY.len(), 36);
        let mut table_names: Vec<&str> = TOOL_STABILITY.iter().map(|(name, _)| *name).collect();
        table_names.sort_unstable();
        table_names.dedup();
        assert_eq!(table_names.len(), 36, "duplicate or missing tool name");
        let mut real_names: Vec<String> = definitions
            .iter()
            .map(|tool| -> Result<String, Box<dyn std::error::Error>> {
                let value = serde_json::to_value(tool)?;
                Ok(value["name"]
                    .as_str()
                    .ok_or("tool definition missing name")?
                    .to_owned())
            })
            .collect::<Result<_, _>>()?;
        real_names.sort_unstable();
        assert_eq!(
            table_names,
            real_names.iter().map(String::as_str).collect::<Vec<_>>()
        );
        Ok(())
    }

    #[test]
    fn exactly_the_five_analyzer_tools_are_preview() {
        let preview: Vec<&str> = TOOL_STABILITY
            .iter()
            .filter(|(_, stability)| *stability == Stability::Preview)
            .map(|(name, _)| *name)
            .collect();
        assert_eq!(
            preview,
            [
                crate::stdio::analyzer::NAME,
                crate::stdio::analyzer::REFERENCES_NAME,
                crate::stdio::analyzer::DIAGNOSTICS_NAME,
                crate::stdio::analyzer::ACTIONS_NAME,
                crate::stdio::mutation::ANALYZER_ACTION_APPLY_NAME,
            ]
        );
    }

    #[test]
    fn only_preview_descriptions_carry_the_prefix() -> Result<(), Box<dyn std::error::Error>> {
        for tool in crate::stdio::capability_document::tool_definitions()? {
            let value = serde_json::to_value(&tool)?;
            let name = value["name"]
                .as_str()
                .ok_or("tool definition missing name")?;
            let description = value["description"]
                .as_str()
                .ok_or("tool definition missing description")?;
            let starts_with_prefix = description.starts_with(PREVIEW_PREFIX);
            assert_eq!(
                stability(name) == Stability::Preview,
                starts_with_prefix,
                "{name}: stability/description prefix disagree"
            );
        }
        Ok(())
    }

    #[test]
    fn serializes_snake_case() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(serde_json::to_value(Stability::Stable)?, "stable");
        assert_eq!(serde_json::to_value(Stability::Preview)?, "preview");
        Ok(())
    }
}
