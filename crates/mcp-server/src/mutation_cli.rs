//! Explicit local journal administration; never reachable through MCP tools.
use rust_engineering_domain::{MutationError, MutationId, MutationState, SourceFingerprint};
use rust_engineering_project::mutation_store::NativeMutationStore;
use serde::Serialize;
use std::{
    ffi::OsString,
    io::{self, Write},
    path::{Path, PathBuf},
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
#[cfg_attr(test, derive(Debug, PartialEq))]
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
    // Only meaningful for `list`: distinguishes "never had a journal
    // directory" (empty by construction) from an opened, scanned store.
    store_initialized: Option<bool>,
    count: u64,
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
/// Whether the journal directory exists, without creating it: `list` must
/// read passively (unlike a real mutation, which provisions state on first
/// write via `prepare_mutation_state`). `Ok(false)` means `state_root` exists
/// but was never used for a mutation; any other missing-path case, including
/// a `state_root` that itself does not exist, is reported as `NotFound` so it
/// is not silently confused with an initialized-but-empty store.
fn journal_dir_exists(state_root: &Path, journal_dir: &Path) -> Result<bool, MutationError> {
    match std::fs::metadata(journal_dir) {
        Ok(metadata) if metadata.is_dir() => Ok(true),
        Ok(_) => Err(MutationError::Io),
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(MutationError::Io),
        Err(_) => match std::fs::metadata(state_root) {
            Ok(metadata) if metadata.is_dir() => Ok(false),
            _ => Err(MutationError::NotFound),
        },
    }
}
fn execute(invocation: &Invocation) -> Result<(Vec<Record>, Option<bool>), MutationError> {
    let journal_dir = invocation.state_root.join("rust-mcp-mutations-v1");
    if matches!(invocation.action, Action::List)
        && !journal_dir_exists(&invocation.state_root, &journal_dir)?
    {
        return Ok((vec![], Some(false)));
    }
    // Open an existing private child; administration must not initialize state.
    let store = NativeMutationStore::open(&journal_dir, &[])?;
    match &invocation.action {
        Action::List => Ok((
            store
                .list_records()?
                .into_iter()
                .map(|record| Record {
                    operation_id: record.id.as_str().into(),
                    plan_digest: record.digest.to_string(),
                    state: state(record.state),
                    stored_bytes: record.stored_bytes,
                })
                .collect(),
            Some(true),
        )),
        Action::Prune { id, digest } => {
            store.prune_record(id, digest)?;
            Ok((vec![], None))
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
/// Pure: turns an `execute` outcome into the serializable report and its exit
/// code, with no filesystem, store or process access, so it is unit-testable
/// with synthetic results on every platform.
fn build_report(
    action: &'static str,
    result: Result<(Vec<Record>, Option<bool>), MutationError>,
) -> (Report, u8) {
    match result {
        Ok((records, store_initialized)) => (
            Report {
                format_version: 1,
                status: "passed",
                action,
                error_code: None,
                message: match (action, store_initialized) {
                    ("list", Some(false)) => {
                        "No mutation journal store exists yet at this state root"
                    }
                    ("list", _) => "Existing local mutation journals",
                    _ => {
                        "Terminal journal removed; its durable receipt and replay record no longer exist"
                    }
                },
                store_initialized,
                count: records.len() as u64,
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
                message: match error {
                    MutationError::NotFound => {
                        "The state root does not exist or the requested record was not found"
                    }
                    _ => {
                        "Journal administration did not complete; preserve pending evidence and use authorized recovery for interrupted operations"
                    }
                },
                store_initialized: None,
                count: 0,
                records: vec![],
            },
            1,
        ),
    }
}
/// Pure: renders the closed, size-bounded output body. `None` signals a
/// caller-visible failure (unserializable report or an output over the
/// 128 KiB bound), never a partial write.
fn render(report: &Report, json: bool) -> Option<Vec<u8>> {
    let mut bytes = if json {
        serde_json::to_vec(report).ok()?
    } else {
        let mut text = format!("{}: {}\n{}", report.action, report.status, report.message);
        for record in &report.records {
            use std::fmt::Write;
            write!(
                text,
                "\n{} {} {} {} bytes",
                record.operation_id, record.plan_digest, record.state, record.stored_bytes
            )
            .ok()?;
        }
        text.into_bytes()
    };
    bytes.push(b'\n');
    if bytes.len() > 128 * 1024 {
        return None;
    }
    Some(bytes)
}
pub fn run(invocation: Invocation) -> ExitCode {
    let action = match invocation.action {
        Action::List => "list",
        Action::Prune { .. } => "prune",
    };
    let (report, code) = build_report(action, execute(&invocation));
    let Some(bytes) = render(&report, invocation.json) else {
        return ExitCode::FAILURE;
    };
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
    /// The parser requires an absolute state root, and a leading slash is not
    /// absolute on Windows, where a path needs a drive prefix.
    #[cfg(not(windows))]
    const STATE_ROOT: &str = "/tmp/state";
    #[cfg(windows)]
    const STATE_ROOT: &str = r"C:\tmp\state";
    fn parse_strings(args: &[&str]) -> bool {
        parse(args.iter().map(OsString::from)).is_some()
    }
    #[test]
    fn admin_parser_is_closed_and_requires_explicit_exact_prune_digest() {
        assert!(parse_strings(&[
            "list",
            "--state-root",
            STATE_ROOT,
            "--json"
        ]));
        assert!(!parse_strings(&["list", "--state-root", "relative"]));
        assert!(!parse_strings(&[
            "list",
            "--state-root",
            STATE_ROOT,
            "--all"
        ]));
        assert!(!parse_strings(&["prune", "--state-root", STATE_ROOT]));
        let id = "mut_0123456789abcdef0123456789abcdef";
        let digest = format!("sha256:{}", "a".repeat(64));
        assert!(parse_strings(&[
            "prune",
            "--state-root",
            STATE_ROOT,
            "--operation-id",
            id,
            "--plan-digest",
            &digest
        ]));
        assert!(!parse_strings(&[
            "prune",
            "--state-root",
            STATE_ROOT,
            "--operation-id",
            "../journal",
            "--plan-digest",
            &digest
        ]));
    }
    fn unique_temp_dir(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "rust-mcp-mutation-cli-{label}-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
        ))
    }
    #[test]
    fn journal_dir_exists_is_false_for_a_state_root_never_used_for_a_mutation()
    -> Result<(), Box<dyn std::error::Error>> {
        let state_root = unique_temp_dir("never-initialized");
        std::fs::create_dir_all(&state_root)?;
        let journal_dir = state_root.join("rust-mcp-mutations-v1");
        let found = journal_dir_exists(&state_root, &journal_dir);
        std::fs::remove_dir_all(&state_root)?;
        assert_eq!(found, Ok(false));
        Ok(())
    }
    #[test]
    fn journal_dir_exists_is_not_found_for_a_state_root_that_does_not_exist() {
        let state_root = unique_temp_dir("nonexistent");
        let journal_dir = state_root.join("rust-mcp-mutations-v1");
        assert_eq!(
            journal_dir_exists(&state_root, &journal_dir),
            Err(MutationError::NotFound)
        );
    }
    #[test]
    fn journal_dir_exists_is_an_io_error_when_the_path_is_a_file_not_a_directory()
    -> Result<(), Box<dyn std::error::Error>> {
        let state_root = unique_temp_dir("journal-is-a-file");
        std::fs::create_dir_all(&state_root)?;
        let journal_dir = state_root.join("rust-mcp-mutations-v1");
        std::fs::write(&journal_dir, b"not a directory")?;
        let found = journal_dir_exists(&state_root, &journal_dir);
        std::fs::remove_dir_all(&state_root)?;
        assert_eq!(found, Err(MutationError::Io));
        Ok(())
    }
    #[test]
    fn execute_list_on_a_never_initialized_state_root_is_empty_without_opening_a_store()
    -> Result<(), Box<dyn std::error::Error>> {
        let state_root = unique_temp_dir("execute-never-initialized");
        std::fs::create_dir_all(&state_root)?;
        let invocation = Invocation {
            state_root: state_root.clone(),
            action: Action::List,
            json: true,
        };
        let result = execute(&invocation);
        std::fs::remove_dir_all(&state_root)?;
        assert_eq!(result, Ok((vec![], Some(false))));
        Ok(())
    }
    #[test]
    fn execute_on_a_nonexistent_state_root_is_not_found() {
        let state_root = unique_temp_dir("execute-nonexistent");
        let invocation = Invocation {
            state_root,
            action: Action::List,
            json: true,
        };
        assert_eq!(execute(&invocation), Err(MutationError::NotFound));
    }
    #[test]
    fn build_report_list_distinguishes_an_uninitialized_store_from_an_existing_one() {
        let (report, code) = build_report("list", Ok((vec![], Some(false))));
        assert_eq!(report.status, "passed");
        assert_eq!(code, 0);
        assert_eq!(report.store_initialized, Some(false));
        assert_eq!(report.count, 0);
        assert_eq!(
            report.message,
            "No mutation journal store exists yet at this state root"
        );
        let records = vec![Record {
            operation_id: "mut_0123456789abcdef0123456789abcdef".into(),
            plan_digest: format!("sha256:{}", "a".repeat(64)),
            state: "committed",
            stored_bytes: 42,
        }];
        let (report, code) = build_report("list", Ok((records, Some(true))));
        assert_eq!(code, 0);
        assert_eq!(report.store_initialized, Some(true));
        assert_eq!(report.count, 1);
        assert_eq!(report.message, "Existing local mutation journals");
    }
    #[test]
    fn build_report_prune_success_has_no_store_initialized_field() {
        let (report, code) = build_report("prune", Ok((vec![], None)));
        assert_eq!(code, 0);
        assert_eq!(report.status, "passed");
        assert!(report.store_initialized.is_none());
        assert_eq!(
            report.message,
            "Terminal journal removed; its durable receipt and replay record no longer exist"
        );
    }
    #[test]
    fn build_report_maps_not_found_and_other_errors_to_distinct_messages_and_blocked_status() {
        let (report, code) = build_report("list", Err(MutationError::NotFound));
        assert_eq!(code, 1);
        assert_eq!(report.status, "blocked");
        assert_eq!(report.error_code, Some("not_found"));
        assert_eq!(
            report.message,
            "The state root does not exist or the requested record was not found"
        );
        assert!(report.store_initialized.is_none());
        assert_eq!(report.count, 0);
        let (report, code) = build_report("prune", Err(MutationError::Busy));
        assert_eq!(code, 1);
        assert_eq!(report.error_code, Some("lock_busy"));
        assert_eq!(
            report.message,
            "Journal administration did not complete; preserve pending evidence and use authorized recovery for interrupted operations"
        );
    }
    #[test]
    fn error_code_maps_every_mutation_error_to_a_distinct_stable_code() {
        let mapped = [
            (MutationError::Invalid, "invalid_operation"),
            (MutationError::PermissionDenied, "permission_denied"),
            (MutationError::Conflict, "conflict"),
            (MutationError::Busy, "lock_busy"),
            (MutationError::Expired, "plan_expired"),
            (MutationError::NotFound, "not_found"),
            (MutationError::LimitExceeded, "limit_exceeded"),
            (MutationError::UnsupportedPlatform, "unsupported_platform"),
            (MutationError::Cancelled, "cancelled"),
            (MutationError::Io, "io"),
            (MutationError::RecoveryRequired, "recovery_required"),
        ];
        for (error, code) in mapped {
            assert_eq!(error_code(error), code);
        }
    }
    #[test]
    fn state_maps_every_mutation_state_to_a_distinct_label() {
        let mapped = [
            (MutationState::Committed, "committed"),
            (MutationState::NoChange, "no_change"),
            (MutationState::Aborted, "aborted"),
            (MutationState::RecoveryRequired, "recovery_required"),
        ];
        for (value, label) in mapped {
            assert_eq!(state(value), label);
        }
    }
    #[test]
    fn render_json_round_trips_and_human_appends_one_line_per_record()
    -> Result<(), Box<dyn std::error::Error>> {
        let (report, _) = build_report(
            "list",
            Ok((
                vec![Record {
                    operation_id: "mut_0123456789abcdef0123456789abcdef".into(),
                    plan_digest: format!("sha256:{}", "a".repeat(64)),
                    state: "committed",
                    stored_bytes: 42,
                }],
                Some(true),
            )),
        );
        let json = render(&report, true).ok_or("json render")?;
        let value: serde_json::Value = serde_json::from_slice(&json)?;
        assert_eq!(value["status"], "passed");
        assert_eq!(value["count"], 1);
        let human = render(&report, false).ok_or("human render")?;
        let human = String::from_utf8(human)?;
        assert!(human.starts_with("list: passed\nExisting local mutation journals"));
        assert!(human.contains("mut_0123456789abcdef0123456789abcdef"));
        assert!(human.ends_with("42 bytes\n"));
        Ok(())
    }
    #[test]
    fn render_refuses_output_over_the_128kib_bound() {
        let (mut report, _) = build_report("list", Ok((vec![], Some(false))));
        report.message = "over budget";
        report.records = vec![Record {
            operation_id: "x".repeat(200 * 1024),
            plan_digest: String::new(),
            state: "committed",
            stored_bytes: 0,
        }];
        assert!(render(&report, false).is_none());
        assert!(render(&report, true).is_none());
    }
}
