//! Explicit host administration for ADR-061 quality artifacts.

use rust_engineering_domain::QualityArtifactError;
use rust_engineering_project::quality_artifact_store::{prune_expired, recover};
use serde::Serialize;
use std::{ffi::OsString, io::Write, path::PathBuf, process::ExitCode};

pub struct Invocation {
    action: Action,
    state_root: PathBuf,
    json: bool,
}
#[derive(Clone, Copy)]
enum Action {
    Recover,
    Prune,
}

pub fn parse(mut args: impl Iterator<Item = OsString>) -> Option<Invocation> {
    let action = match args.next()?.to_str()? {
        "recover" => Action::Recover,
        "prune" => Action::Prune,
        _ => return None,
    };
    let (mut state_root, mut json) = (None, false);
    while let Some(flag) = args.next() {
        match flag.to_str()? {
            "--state-root" if state_root.is_none() => {
                state_root = Some(PathBuf::from(args.next()?))
            }
            "--json" if !json => json = true,
            _ => return None,
        }
    }
    let state_root = state_root?;
    state_root.is_absolute().then_some(Invocation {
        action,
        state_root,
        json,
    })
}

#[derive(Serialize)]
struct Report {
    format_version: u8,
    status: &'static str,
    action: &'static str,
    error_code: Option<&'static str>,
    data: serde_json::Value,
}

fn error_code(error: QualityArtifactError) -> &'static str {
    match error {
        QualityArtifactError::Busy => "busy",
        QualityArtifactError::UnsupportedPlatform => "unsupported_platform",
        QualityArtifactError::UnsupportedStateRoot => "unsupported_state_root",
        QualityArtifactError::RecoveryRequired => "recovery_required",
        QualityArtifactError::Unauthorized => "unauthorized",
        QualityArtifactError::Expired => "expired",
        QualityArtifactError::QuotaExceeded => "quota_exceeded",
        QualityArtifactError::RetentionDenied => "retention_denied",
        QualityArtifactError::NotFound => "not_found",
        QualityArtifactError::InvalidId
        | QualityArtifactError::InvalidDescriptor
        | QualityArtifactError::InvalidTimestamp
        | QualityArtifactError::InvalidKindVersion
        | QualityArtifactError::InvalidLimit => "invalid_state",
        QualityArtifactError::Io => "io",
    }
}

pub fn run(invocation: Invocation) -> ExitCode {
    let action = match invocation.action {
        Action::Recover => "recover",
        Action::Prune => "prune",
    };
    let result = match invocation.action {
        Action::Recover => recover(&invocation.state_root)
            .and_then(|report| serde_json::to_value(report).map_err(|_| QualityArtifactError::Io)),
        Action::Prune => prune_expired(&invocation.state_root)
            .and_then(|report| serde_json::to_value(report).map_err(|_| QualityArtifactError::Io)),
    };
    let (report, code) = match result {
        Ok(data) => (
            Report {
                format_version: 1,
                status: "passed",
                action,
                error_code: None,
                data,
            },
            0,
        ),
        Err(error) => (
            Report {
                format_version: 1,
                status: "blocked",
                action,
                error_code: Some(error_code(error)),
                data: serde_json::Value::Null,
            },
            1,
        ),
    };
    let mut bytes = if invocation.json {
        match serde_json::to_vec(&report) {
            Ok(bytes) => bytes,
            Err(_) => return ExitCode::FAILURE,
        }
    } else {
        format!(
            "{}: {}{}",
            report.action,
            report.status,
            report
                .error_code
                .map(|code| format!(" ({code})"))
                .unwrap_or_default()
        )
        .into_bytes()
    };
    bytes.push(b'\n');
    if bytes.len() > 16 * 1024 || std::io::stdout().lock().write_all(&bytes).is_err() {
        ExitCode::FAILURE
    } else {
        ExitCode::from(code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parses(args: &[&str]) -> bool {
        parse(args.iter().map(OsString::from)).is_some()
    }
    #[test]
    fn grammar_is_closed_and_requires_absolute_state_root() {
        assert!(parses(&["recover", "--state-root", "/tmp/state", "--json"]));
        assert!(parses(&["prune", "--state-root", "/tmp/state"]));
        assert!(!parses(&["prune", "--state-root", "relative"]));
        assert!(!parses(&["recover", "--state-root", "/tmp/state", "--all"]));
    }
}
