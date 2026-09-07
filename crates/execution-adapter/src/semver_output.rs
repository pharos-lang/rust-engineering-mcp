//! Bounded, best-effort text parser over the pinned `cargo-semver-checks`
//! binary's non-colored `check-release` output (ADR-062 §11).
//!
//! No machine-readable findings flag exists for the pinned 0.50.0 binary
//! (confirmed against the pinned `--help` output at
//! `docs/validation/m3-provisioning/help/cargo-semver-checks-check-release-help.stdout`),
//! so this parser scrapes the tool's own human-oriented `handlebars` report
//! text. The shapes below were calibrated against 0.50.0 in the approved M3
//! guest image. It never fabricates a finding it cannot support and never
//! promotes its output past [`SemverParseCompleteness::Partial`].
use rust_engineering_domain::semver_check::{
    SemverFinding, SemverFindingCounts, SemverFindingLevel, SemverRequiredUpdate,
};

/// Bounds this parser's own work independent of the (separately bounded)
/// captured stdout stream: a pathological amount of well-formed-looking text
/// still cannot make the parser itself do unbounded work.
pub(super) const MAX_PARSED_LINES: usize = 4_096;
pub(super) const MAX_PARSED_FINDINGS: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SemverParseCompleteness {
    Partial,
    Incomplete,
}

pub(super) struct SemverParsed {
    pub(super) counts: SemverFindingCounts,
    pub(super) findings: Vec<SemverFinding>,
    pub(super) findings_omitted: u64,
    pub(super) completeness: SemverParseCompleteness,
}

/// Deny/warn-level counts are
/// derived from counting individual `--- failure ` / `--- warning ` report
/// section headers rather than trusted from any single summary line, since
/// the summary line's own exact wording is unconfirmed.
fn count_sections(line: &str, counts: &mut SemverFindingCounts) {
    if line.starts_with("--- failure") {
        counts.deny = counts.deny.saturating_add(1);
    } else if line.starts_with("--- warning") {
        counts.warn = counts.warn.saturating_add(1);
    }
}

/// A best-effort single-finding header. The first form is the observed 0.50.0
/// section header: `"--- failure lint_name: item description ---"`. The second
/// form is retained for bounded compatibility with an older detailed form:
/// `"failure [lint_name]: item description, at src/lib.rs:12"`. Never the
/// only source of truth for counts (see [`count_sections`]); a line that does
/// not match this shape simply contributes no per-finding detail, never an
/// error.
fn parse_finding_line(line: &str, level: SemverFindingLevel) -> Option<SemverFinding> {
    if let Some(section) = line
        .strip_prefix("--- ")
        .and_then(|value| value.strip_suffix(" ---"))
    {
        let section = section
            .strip_prefix("failure ")
            .or_else(|| section.strip_prefix("warning "))?;
        let (lint, item) = section.split_once(": ")?;
        return SemverFinding::new(item.to_owned(), lint.to_owned(), level, None, None).ok();
    }
    let rest = line
        .strip_prefix("failure [")
        .or_else(|| line.strip_prefix("warning ["))?;
    let (lint, rest) = rest.split_once(']')?;
    let rest = rest.strip_prefix(':')?.trim();
    let (item, span) = match rest.rsplit_once(", at ") {
        Some((item, span)) => (item.trim(), Some(span.trim())),
        None => (rest, None),
    };
    let required_update = if lint.contains("major") {
        Some(SemverRequiredUpdate::Major)
    } else if lint.contains("minor") {
        Some(SemverRequiredUpdate::Minor)
    } else {
        None
    };
    SemverFinding::new(
        item.to_owned(),
        lint.to_owned(),
        level,
        required_update,
        span.map(str::to_owned),
    )
    .ok()
}

/// Parses the pinned binary's non-colored `check-release` stdout. Bounded by
/// [`MAX_PARSED_LINES`]/[`MAX_PARSED_FINDINGS`]; a stream exceeding either
/// bound yields `Incomplete`, never a silently truncated `Partial` result
/// mistaken for the whole picture.
pub(super) fn parse(stdout: &str) -> SemverParsed {
    // The calibrated section header is itself the finding. Retain the older
    // detailed grammar only as a whole-stream fallback: accepting both in one
    // report would count the same finding twice when a verbose renderer emits
    // a header followed by details.
    let has_section_headers = stdout.lines().take(MAX_PARSED_LINES + 1).any(|line| {
        let line = line.trim_start();
        line.starts_with("--- failure") || line.starts_with("--- warning")
    });
    let mut counts = SemverFindingCounts::default();
    let mut findings = Vec::new();
    let mut findings_omitted = 0u64;
    let mut lines_seen = 0usize;
    let mut truncated = false;
    let mut recognized = false;
    for line in stdout.lines() {
        lines_seen += 1;
        if lines_seen > MAX_PARSED_LINES {
            truncated = true;
            break;
        }
        let trimmed = line.trim_start();
        recognized |= trimmed.starts_with("--- failure")
            || trimmed.starts_with("--- warning")
            || trimmed.contains("No semver-breaking changes detected")
            || trimmed.contains("Summary no semver update required")
            || trimmed.contains("semver requires new");
        let is_section = trimmed.starts_with("--- failure") || trimmed.starts_with("--- warning");
        let is_detailed = trimmed.starts_with("failure [") || trimmed.starts_with("warning [");
        if has_section_headers && !is_section && is_detailed {
            continue;
        }
        if !has_section_headers && is_detailed {
            if trimmed.starts_with("failure [") {
                counts.deny = counts.deny.saturating_add(1);
            } else {
                counts.warn = counts.warn.saturating_add(1);
            }
        } else {
            count_sections(trimmed, &mut counts);
        }
        let level = if trimmed.starts_with("--- failure") || trimmed.starts_with("failure [") {
            Some(SemverFindingLevel::Deny)
        } else if trimmed.starts_with("--- warning") || trimmed.starts_with("warning [") {
            Some(SemverFindingLevel::Warn)
        } else {
            None
        };
        let Some(level) = level else { continue };
        match parse_finding_line(trimmed, level) {
            Some(finding) if findings.len() < MAX_PARSED_FINDINGS => {
                recognized = true;
                findings.push(finding);
            }
            Some(_) => {
                recognized = true;
                findings_omitted = findings_omitted.saturating_add(1);
            }
            _ => findings_omitted = findings_omitted.saturating_add(1),
        }
    }
    let completeness = if truncated || !recognized {
        SemverParseCompleteness::Incomplete
    } else {
        SemverParseCompleteness::Partial
    };
    SemverParsed {
        counts,
        findings,
        findings_omitted,
        completeness,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calibrated_golden_outputs_are_bounded_and_fail_closed() {
        let clean = parse(include_str!("../tests/fixtures/semver-clean.stdout"));
        assert_eq!(clean.counts, SemverFindingCounts::default());
        assert_eq!(clean.completeness, SemverParseCompleteness::Partial);
        let breaking = parse(include_str!("../tests/fixtures/semver-breaking.stdout"));
        assert_eq!(breaking.counts.deny, 1);
        assert_eq!(breaking.findings.len(), 1);
        let warn = parse(include_str!("../tests/fixtures/semver-warn.stdout"));
        assert_eq!(warn.counts.warn, 1);
        assert_eq!(warn.findings.len(), 1);
        assert_eq!(
            parse("unknown output\n").completeness,
            SemverParseCompleteness::Incomplete
        );
    }

    #[test]
    fn clean_output_reports_zero_counts_and_no_findings() {
        let parsed =
            parse("Checking my_crate v0.2.0 -> v0.2.0\nNo semver-breaking changes detected.\n");
        assert_eq!(parsed.counts, SemverFindingCounts::default());
        assert!(parsed.findings.is_empty());
        assert_eq!(parsed.completeness, SemverParseCompleteness::Partial);
    }

    #[test]
    fn counts_deny_and_warn_sections_independently_of_per_finding_extraction() {
        let stdout = "--- failure function_missing: removed function ---\n\
                       failure [function_missing]: pub fn answer, at src/lib.rs:1\n\
                       --- warning unnecessary_pub: pub item ---\n\
                       warning [unnecessary_pub]: pub struct Foo, at src/lib.rs:5\n";
        let parsed = parse(stdout);
        assert_eq!(parsed.counts.deny, 1);
        assert_eq!(parsed.counts.warn, 1);
        assert_eq!(parsed.findings.len(), 2);
        assert_eq!(parsed.findings[0].level(), SemverFindingLevel::Deny);
        assert_eq!(parsed.findings[0].item(), "removed function");
        assert_eq!(parsed.findings[0].lint(), "function_missing");
        assert_eq!(parsed.findings[0].span(), None);
        assert_eq!(parsed.findings[0].required_update(), None);
        assert_eq!(parsed.findings[1].level(), SemverFindingLevel::Warn);

        let detailed = parse(
            "failure [function_missing]: pub fn answer, at src/lib.rs:1\n\
             warning [unnecessary_pub]: pub struct Foo, at src/lib.rs:5\n",
        );
        assert_eq!(detailed.counts.deny, 1);
        assert_eq!(detailed.counts.warn, 1);
        assert_eq!(detailed.findings[0].item(), "pub fn answer");
        assert_eq!(detailed.findings[0].span(), Some("src/lib.rs:1"));
    }

    #[test]
    fn a_finding_header_without_the_expected_shape_contributes_no_per_finding_detail() {
        let parsed = parse("failure [oops without a colon\n");
        assert!(parsed.findings.is_empty());
        assert_eq!(parsed.findings_omitted, 1);
        assert_eq!(parsed.completeness, SemverParseCompleteness::Incomplete);
    }

    #[test]
    fn a_stream_beyond_the_line_bound_is_incomplete_never_a_silent_partial() {
        let stdout = "no-op line\n".repeat(MAX_PARSED_LINES + 1);
        let parsed = parse(&stdout);
        assert_eq!(parsed.completeness, SemverParseCompleteness::Incomplete);
    }

    #[test]
    fn findings_beyond_the_cap_are_counted_as_omitted_not_dropped_silently() {
        let stdout = "--- failure function_missing: pub fn a ---\n".repeat(MAX_PARSED_FINDINGS + 3);
        let parsed = parse(&stdout);
        assert_eq!(parsed.findings.len(), MAX_PARSED_FINDINGS);
        assert_eq!(parsed.findings_omitted, 3);
        assert_eq!(parsed.counts.deny, (MAX_PARSED_FINDINGS + 3) as u32);
    }
}
