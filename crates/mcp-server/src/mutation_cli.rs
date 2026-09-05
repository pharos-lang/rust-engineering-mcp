//! Explicit local journal administration; never reachable through MCP tools.
use rust_engineering_domain::{MutationError, MutationId, MutationState, SourceFingerprint};
use rust_engineering_project::mutation_store::NativeMutationStore;
use serde::Serialize;
use std::{
    ffi::OsString,
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
    time::Duration,
};

pub struct Invocation {
    state_root: PathBuf,
    action: Action,
    json: bool,
}
enum Action {
    List,
    Prune {
        id: MutationId,
        digest: SourceFingerprint,
    },
}

pub fn parse(mut args: impl Iterator<Item = OsString>) -> Option<Invocation> {
    let command = args.next()?.into_string().ok()?;
    if command != "list" && command != "prune" {
        return None;
    }
    let (mut state_root, mut id, mut digest, mut json) = (None, None, None, false);
    while let Some(flag) = args.next() {
        match flag.to_str()? {
            "--state-root" if state_root.is_none() => {
                state_root = Some(PathBuf::from(args.next()?))
            }
            "--operation-id" if id.is_none() => {
                id = Some(MutationId::new(args.next()?.into_string().ok()?).ok()?)
            }
            "--plan-digest" if digest.is_none() => {
                digest = Some(args.next()?.into_string().ok()?.parse().ok()?)
            }
            "--json" if !json => json = true,
            _ => return None,
        }
    }
    let state_root = state_root?;
    if !state_root.is_absolute() {
        return None;
    }
    let action = match (command.as_str(), id, digest) {
        ("list", None, None) => Action::List,
        ("prune", Some(id), Some(digest)) => Action::Prune { id, digest },
        _ => return None,
    };
    Some(Invocation {
        state_root,
        action,
        json,
    })
}
#[derive(Serialize)]
struct Record {
    operation_id: String,
    plan_digest: String,
    state: &'static str,
    stored_bytes: u64,
}
#[derive(Serialize)]
struct Report {
    format_version: u32,
    status: &'static str,
    action: &'static str,
    error_code: Option<&'static str>,
    message: &'static str,
    records: Vec<Record>,
}
fn state(value: MutationState) -> &'static str {
    match value {
        MutationState::Committed => "committed",
        MutationState::NoChange => "no_change",
        MutationState::Aborted => "aborted",
        MutationState::RecoveryRequired => "recovery_required",
    }
}
fn execute(invocation: &Invocation) -> Result<Vec<Record>, MutationError> {
    // Open an existing private child; administration must not initialize state.
    let store =
        NativeMutationStore::open(&invocation.state_root.join("rust-mcp-mutations-v1"), &[])?;
    match &invocation.action {
        Action::List => Ok(store
            .list_records()?
            .into_iter()
            .map(|record| Record {
                operation_id: record.id.as_str().into(),
                plan_digest: record.digest.to_string(),
                state: state(record.state),
                stored_bytes: record.stored_bytes,
            })
            .collect()),
        Action::Prune { id, digest } => {
            store.prune_record(id, digest)?;
            Ok(vec![])
        }
    }
}
fn error_code(error: MutationError) -> &'static str {
    match error {
        MutationError::Invalid => "invalid_operation",
        MutationError::PermissionDenied => "permission_denied",
        MutationError::Conflict => "conflict",
        MutationError::Busy => "lock_busy",
        MutationError::Expired => "plan_expired",
        MutationError::NotFound => "not_found",
        MutationError::LimitExceeded => "limit_exceeded",
        MutationError::UnsupportedPlatform => "unsupported_platform",
        MutationError::Cancelled => "cancelled",
        MutationError::Io => "io",
        MutationError::RecoveryRequired => "recovery_required",
    }
}
pub fn run(invocation: Invocation) -> ExitCode {
    let action = match invocation.action {
        Action::List => "list",
        Action::Prune { .. } => "prune",
    };
    let (report, code) = match execute(&invocation) {
        Ok(records) => (
            Report {
                format_version: 1,
                status: "passed",
                action,
                error_code: None,
                message: if action == "list" {
                    "Existing local mutation journals"
                } else {
                    "Terminal journal removed; its durable receipt and replay record no longer exist"
                },
                records,
            },
            0,
        ),
        Err(error) => (
            Report {
                format_version: 1,
                status: "blocked",
                action,
                error_code: Some(error_code(error)),
                message: "Journal administration did not complete; preserve pending evidence and use authorized recovery for interrupted operations",
                records: vec![],
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
        let mut text = format!("{}: {}\n{}", report.action, report.status, report.message);
        for record in &report.records {
            use std::fmt::Write;
            if write!(
                text,
                "\n{} {} {} {} bytes",
                record.operation_id, record.plan_digest, record.state, record.stored_bytes
            )
            .is_err()
            {
                return ExitCode::FAILURE;
            }
        }
        text.into_bytes()
    };
    bytes.push(b'\n');
    if bytes.len() > 128 * 1024 {
        return ExitCode::FAILURE;
    }
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => return ExitCode::FAILURE,
    };
    // No native resources remain while output is delivered. Bound a stalled pipe.
    let result = runtime.block_on(async {
        let output = tokio::task::spawn_blocking(move || {
            let mut out = io::stdout().lock();
            out.write_all(&bytes)?;
            out.flush()
        });
        match tokio::time::timeout(Duration::from_secs(5), output).await {
            Ok(Ok(Ok(()))) => ExitCode::from(code),
            _ => ExitCode::FAILURE,
        }
    });
    runtime.shutdown_timeout(Duration::from_millis(100));
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    fn parse_strings(args: &[&str]) -> bool {
        parse(args.iter().map(OsString::from)).is_some()
    }
    #[test]
    fn admin_parser_is_closed_and_requires_explicit_exact_prune_digest() {
        assert!(parse_strings(&[
            "list",
            "--state-root",
            "/tmp/state",
            "--json"
        ]));
        assert!(!parse_strings(&["list", "--state-root", "relative"]));
        assert!(!parse_strings(&[
            "list",
            "--state-root",
            "/tmp/state",
            "--all"
        ]));
        assert!(!parse_strings(&["prune", "--state-root", "/tmp/state"]));
        let id = "mut_0123456789abcdef0123456789abcdef";
        let digest = format!("sha256:{}", "a".repeat(64));
        assert!(parse_strings(&[
            "prune",
            "--state-root",
            "/tmp/state",
            "--operation-id",
            id,
            "--plan-digest",
            &digest
        ]));
        assert!(!parse_strings(&[
            "prune",
            "--state-root",
            "/tmp/state",
            "--operation-id",
            "../journal",
            "--plan-digest",
            &digest
        ]));
    }
}
