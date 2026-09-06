//! Gateway phases for `rust.test.nextest`: product-owned config ingest ahead
//! of the hostile project source (so `tar --keep-old-files` makes the first,
//! trusted writer win), the closed `cargo nextest run` phase, and a fixed tar
//! export from a bounded output volume mounted read-only in the exporter.
//! A hostile symlink is represented as a tar link and rejected by the host
//! decoder; it is never followed into guest-selected bytes.
use super::mutation_gateway::{VOLUME_OPTIONS, parse_volume};
use super::rust_gateway::{
    Phase, PhaseRequest, RustGateway, Volume, WorkBudget, finish_work, labels,
};
use super::*;
use rust_engineering_domain::nextest::{NEXTEST_PROFILE, NextestCommandOptions};
use rust_engineering_domain::{RustCommand, SourceBundle};
use std::sync::Mutex;

/// Product-owned, never guest- or caller-controlled. Placed under the
/// persistent `/source` volume (not `/work`, which is a fresh, non-persistent
/// tmpfs per container) so the same file is visible to the later `Run` phase.
pub(super) const NEXTEST_CONFIG_GUEST_PATH: &str = "/source/.rust-mcp-nextest/nextest.toml";
const NEXTEST_CONFIG_DIR: &str = ".rust-mcp-nextest";
const NEXTEST_CONFIG_FILE: &str = "nextest.toml";
const JUNIT_ARCHIVE_MEMBER: &str = "junit.xml";
/// Fixed, undocumented-by-caller: leak detection is not tunable per job.
const LEAK_TIMEOUT: &str = "1s";
const SLOW_TIMEOUT_TERMINATE_AFTER: u32 = 1;
const MAX_CONFIG_ARCHIVE: usize = 16 * 1024;
pub(super) const MAX_JUNIT_EXPORT: usize = 32 * 1024 * 1024 + 2048;

pub struct NextestExecution {
    pub result: rust_engineering_domain::ExecutionResult,
    pub junit: Option<Vec<u8>>,
    pub junit_truncated: bool,
}

/// Renders the product-owned `nextest.toml`. Every value is either fixed or
/// comes from the already-validated, bounded [`NextestCommandOptions`]; no
/// free-form caller string reaches this file.
fn generate_config(options: &NextestCommandOptions) -> String {
    format!(
        "[store]\n\
         dir = \"/junit\"\n\
         \n\
         [profile.{profile}]\n\
         fail-fast = false\n\
         retries = {retries}\n\
         slow-timeout = {{ period = \"{timeout}s\", terminate-after = {terminate_after} }}\n\
         leak-timeout = {{ period = \"{leak_timeout}\", result = \"fail\" }}\n\
         \n\
         [profile.{profile}.junit]\n\
         path = \"reports/junit.xml\"\n\
         store-failure-output = true\n\
         report-skipped = \"all\"\n",
        profile = NEXTEST_PROFILE,
        retries = options.retries(),
        timeout = options.timeout(),
        terminate_after = SLOW_TIMEOUT_TERMINATE_AFTER,
        leak_timeout = LEAK_TIMEOUT,
    )
}

fn octal(field: &mut [u8], value: usize) -> Result<(), ExecutionError> {
    let text = format!("{value:0width$o}", width = field.len() - 1);
    if text.len() != field.len() - 1 {
        return Err(ExecutionError::Infrastructure);
    }
    field[..text.len()].copy_from_slice(text.as_bytes());
    Ok(())
}

fn tar_entry(
    out: &mut Vec<u8>,
    name: &str,
    bytes: &[u8],
    directory: bool,
) -> Result<(), ExecutionError> {
    if name.len() > 100 {
        return Err(ExecutionError::Infrastructure);
    }
    let padded = bytes.len().div_ceil(512) * 512;
    if out.len() + 512 + padded + 1024 > MAX_CONFIG_ARCHIVE {
        return Err(ExecutionError::Infrastructure);
    }
    let mut header = [0u8; 512];
    header[..name.len()].copy_from_slice(name.as_bytes());
    octal(&mut header[100..108], if directory { 0o755 } else { 0o444 })?;
    octal(&mut header[108..116], 0)?;
    octal(&mut header[116..124], 0)?;
    octal(&mut header[124..136], bytes.len())?;
    octal(&mut header[136..148], 0)?;
    header[148..156].fill(b' ');
    header[156] = if directory { b'5' } else { b'0' };
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    let checksum: usize = header.iter().map(|v| usize::from(*v)).sum();
    octal(&mut header[148..155], checksum)?;
    header[155] = b' ';
    out.extend_from_slice(&header);
    out.extend_from_slice(bytes);
    out.resize(out.len() + padded - bytes.len(), 0);
    Ok(())
}

/// A minimal two-entry USTAR archive (one directory, one regular file)
/// carrying only the product-owned nextest configuration text.
fn config_archive(config_text: &str) -> Result<Vec<u8>, ExecutionError> {
    let mut out = Vec::new();
    tar_entry(&mut out, &format!("{NEXTEST_CONFIG_DIR}/"), &[], true)?;
    tar_entry(
        &mut out,
        &format!("{NEXTEST_CONFIG_DIR}/{NEXTEST_CONFIG_FILE}"),
        config_text.as_bytes(),
        false,
    )?;
    out.resize(out.len() + 1024, 0);
    Ok(out)
}

fn octal_field(field: &[u8]) -> Option<usize> {
    let text = std::str::from_utf8(field).ok()?;
    let trimmed = text.trim_matches(|c: char| c == '\0' || c.is_ascii_whitespace());
    if trimmed.is_empty() {
        return Some(0);
    }
    usize::from_str_radix(trimmed, 8).ok()
}

/// Reads the single fixed regular-file entry from the closed exporter tar.
/// Links, devices, additional members and guest-selected names are rejected.
pub(super) fn decode_single_file_tar(
    bytes: &[u8],
    max_len: usize,
    expected_name: &str,
) -> Option<Vec<u8>> {
    let mut offset = 0usize;
    let mut found: Option<Vec<u8>> = None;
    while offset + 512 <= bytes.len() {
        let header = &bytes[offset..offset + 512];
        if header.iter().all(|b| *b == 0) {
            break;
        }
        let size = octal_field(&header[124..136])?;
        let typeflag = header[156];
        let padded = size.div_ceil(512) * 512;
        offset += 512;
        if offset + padded > bytes.len() || size > max_len {
            return None;
        }
        let content = &bytes[offset..offset + size];
        match typeflag {
            b'0' | 0 => {
                let name = header[..100]
                    .split(|byte| *byte == 0)
                    .next()
                    .and_then(|value| std::str::from_utf8(value).ok())?;
                if found.is_some() || name != expected_name {
                    // More than one member: not the single fixed file we asked for.
                    return None;
                }
                found = Some(content.to_vec());
            }
            b'5' => (),
            _ => return None,
        }
        offset += padded;
    }
    found
}

pub(super) fn execute(
    gateway: &RustGateway,
    source: &SourceBundle,
    options: &NextestCommandOptions,
    limits: ExecutionLimits,
    cancel: &dyn ExecutionCancellation,
) -> Result<NextestExecution, ExecutionError> {
    let started = Instant::now();
    let _busy = match gateway.inner.busy.try_lock() {
        Ok(guard) => guard,
        Err(std::sync::TryLockError::WouldBlock) => return Err(ExecutionError::Busy),
        Err(_) => {
            gateway.inner.quarantined.store(true, Ordering::Release);
            return Err(ExecutionError::CleanupUncertain);
        }
    };
    if gateway.is_quarantined() {
        return Err(ExecutionError::CleanupUncertain);
    }
    if gateway.calibrating.load(Ordering::Acquire) || !gateway.verified.load(Ordering::Acquire) {
        return Err(ExecutionError::Denied);
    }
    if cancel.is_cancelled() {
        return Err(ExecutionError::Cancelled);
    }
    if digest(&state::executable_bytes(&gateway.inner.config.executable)?)
        != gateway.inner.executable_digest
    {
        return Err(ExecutionError::Unavailable);
    }
    let current = gateway
        .inner
        .control(&["info".into(), "--format={{json .}}".into()])?;
    let engine: EngineIdentity =
        serde_json::from_slice(&current.stdout).map_err(|_| ExecutionError::Unavailable)?;
    if current.code != Some(0) || engine != gateway.inner.engine {
        return Err(ExecutionError::Unavailable);
    }
    let source_archive_bytes = super::source_archive::encode(source)?;
    let config_archive_bytes = config_archive(&generate_config(options))?;
    let budget = WorkBudget {
        started,
        deadline: started + Duration::from_millis(limits.wall_ms()),
        limits,
        cancel,
    };
    let nonce = state::nonce()?;
    let volume = format!("rust-mcp-source-{nonce}");
    let junit_volume = format!("rust-mcp-nextest-output-{nonce}");
    let ingest = format!("rust-mcp-ingest-{nonce}");
    let run = format!("rust-mcp-cargo-{nonce}");
    let output_guardian = format!("rust-mcp-nextest-guardian-{nonce}");
    let exporter = format!("rust-mcp-nextest-export-{nonce}");
    if !gateway.absent("volume", &volume)? || !gateway.absent("volume", &junit_volume)? {
        return Err(ExecutionError::CleanupUncertain);
    }
    let junit_slot: Mutex<Option<Vec<u8>>> = Mutex::new(None);
    let command = RustCommand::TestNextest(options.clone());
    let work = (|| {
        if let Some(stop) = budget.stop() {
            return Ok((budget.stopped_capture(stop), None));
        }
        let mut args = vec!["volume".into(), "create".into(), "--driver=local".into()];
        for (k, v) in labels(&nonce) {
            args.push(format!("--label={k}={v}"));
        }
        args.push(volume.clone());
        if gateway.inner.control(&args)?.code != Some(0) {
            return Err(ExecutionError::Infrastructure);
        }
        let inspect =
            gateway
                .inner
                .control(&["volume".into(), "inspect".into(), volume.clone()])?;
        if inspect.code != Some(0) {
            return Err(ExecutionError::Infrastructure);
        }
        let v = Volume::parse(&inspect.stdout, &volume, &nonce)?;

        let mut output_args = vec![
            "volume".into(),
            "create".into(),
            "--driver=local".into(),
            "--opt=type=tmpfs".into(),
            "--opt=device=tmpfs".into(),
            format!("--opt=o={VOLUME_OPTIONS}"),
        ];
        for (key, value) in labels(&nonce) {
            output_args.push(format!("--label={key}={value}"));
        }
        output_args.push(junit_volume.clone());
        if gateway.inner.control(&output_args)?.code != Some(0) {
            return Err(ExecutionError::Infrastructure);
        }
        let output_inspect =
            gateway
                .inner
                .control(&["volume".into(), "inspect".into(), junit_volume.clone()])?;
        if output_inspect.code != Some(0) {
            return Err(ExecutionError::Infrastructure);
        }
        let output_volume = parse_volume(&output_inspect.stdout, &junit_volume, &nonce)?;
        gateway.start_nextest_output_guardian(
            PhaseRequest {
                name: &output_guardian,
                nonce: &nonce,
                volume: &v,
                phase: &Phase::GuardNextestOutput,
            },
            &output_volume,
            &budget,
        )?;

        // Trusted config first: `tar --keep-old-files` in the Ingest phase
        // makes this the permanent winner even if the hostile source below
        // also contains a file at this reserved path.
        let (config_ingested, config_oom) = gateway.phase(
            PhaseRequest {
                name: &ingest,
                nonce: &nonce,
                volume: &v,
                phase: &Phase::Ingest,
            },
            &config_archive_bytes,
            &budget,
        )?;
        if config_ingested.stop != Stop::Exited {
            return Ok((config_ingested, config_oom));
        }
        if config_ingested.code != Some(0) {
            return Err(ExecutionError::Infrastructure);
        }
        gateway.inner.remove(&ingest)?;

        let (ingested, ingest_oom) = gateway.phase(
            PhaseRequest {
                name: &ingest,
                nonce: &nonce,
                volume: &v,
                phase: &Phase::Ingest,
            },
            &source_archive_bytes,
            &budget,
        )?;
        if ingested.stop != Stop::Exited {
            return Ok((ingested, ingest_oom));
        }
        if ingested.code != Some(0) {
            return Err(ExecutionError::Infrastructure);
        }
        gateway.inner.remove(&ingest)?;

        // No source writer remains while untrusted code executes.
        let (mut outcome, oom) = gateway.nextest_phase(
            PhaseRequest {
                name: &run,
                nonce: &nonce,
                volume: &v,
                phase: &Phase::Run(command.clone()),
            },
            &output_volume,
            true,
            &budget,
        )?;
        if outcome.stop == Stop::Exited {
            gateway.inner.remove(&run)?;
            let (exported, _) = gateway.nextest_phase(
                PhaseRequest {
                    name: &exporter,
                    nonce: &nonce,
                    volume: &v,
                    phase: &Phase::ExportNextest,
                },
                &output_volume,
                false,
                &budget,
            )?;
            if exported.stop == Stop::Exited
                && exported.code == Some(0)
                && !exported.stdout_truncated
                && let Some(bytes) =
                    decode_single_file_tar(&exported.stdout, MAX_JUNIT_EXPORT, JUNIT_ARCHIVE_MEMBER)
            {
                *junit_slot
                    .lock()
                    .map_err(|_| ExecutionError::Infrastructure)? = Some(bytes);
            } else if !exported.stderr.is_empty() {
                outcome
                    .stderr
                    .extend_from_slice(b"\nfixed JUnit exporter: ");
                outcome.stderr.extend_from_slice(&exported.stderr);
            }
        }
        Ok((outcome, oom))
    })();
    let terminal_signal = budget.stop();
    // A bounded-output stop can detach the Docker client while the owned run
    // container is still active. Join all four containers first: the run
    // mounts JUnit while the guardian mounts the source, creating a cross-
    // volume ordering constraint that two independent cleanup passes cannot
    // satisfy.
    gateway.cleanup_nextest(
        &ingest,
        &run,
        &exporter,
        &output_guardian,
        &volume,
        &junit_volume,
        &nonce,
    )?;
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
        gateway.configuration_fingerprint()?,
        &command,
        limits,
        digest(&source_archive_bytes),
        digest(&config_archive_bytes),
        "rust-nextest-profile-v1",
    ))
    .map_err(|_| ExecutionError::Infrastructure)?;
    let result = rust_engineering_domain::ExecutionResult {
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
        image_id: gateway.image_id().into(),
    };
    let junit_bytes = junit_slot
        .into_inner()
        .map_err(|_| ExecutionError::Infrastructure)?;
    let (junit, junit_truncated) = match junit_bytes {
        Some(bytes) if bytes.len() > limits.output_bytes() => {
            (Some(bytes[..limits.output_bytes()].to_vec()), true)
        }
        Some(bytes) => (Some(bytes), false),
        None => (None, false),
    };
    Ok(NextestExecution {
        result,
        junit,
        junit_truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_config_reflects_validated_options_and_fixed_values() -> Result<(), ExecutionError>
    {
        let options =
            NextestCommandOptions::try_from(rust_engineering_domain::nextest::NextestSelection {
                timeout: 45,
                retries: 2,
                ..Default::default()
            })
            .map_err(|_| ExecutionError::Infrastructure)?;
        let config = generate_config(&options);
        assert!(config.contains("[profile.rust-mcp]"));
        assert!(config.contains("[store]"));
        assert!(config.contains("dir = \"/junit\""));
        assert!(config.contains("retries = 2"));
        assert!(config.contains("period = \"45s\""));
        assert!(config.contains("leak-timeout = { period = \"1s\", result = \"fail\" }"));
        assert!(config.contains("path = \"reports/junit.xml\""));
        assert!(config.contains("store-failure-output = true"));
        assert!(config.contains("fail-fast = false"));
        Ok(())
    }

    #[test]
    fn config_archive_round_trips_through_the_tar_reader() -> Result<(), ExecutionError> {
        let config = generate_config(
            &NextestCommandOptions::try_from(
                rust_engineering_domain::nextest::NextestSelection::default(),
            )
            .map_err(|_| ExecutionError::Infrastructure)?,
        );
        let archive = config_archive(&config)?;
        assert!(archive.len() % 512 == 0);
        let recovered = decode_single_file_tar(
            &archive,
            MAX_CONFIG_ARCHIVE,
            ".rust-mcp-nextest/nextest.toml",
        )
        .ok_or(ExecutionError::Infrastructure)?;
        assert_eq!(recovered, config.as_bytes());
        Ok(())
    }

    #[test]
    fn tar_reader_rejects_multiple_regular_files() -> Result<(), ExecutionError> {
        let mut archive = Vec::new();
        tar_entry(&mut archive, "a", b"one", false)?;
        tar_entry(&mut archive, "b", b"two", false)?;
        archive.resize(archive.len() + 1024, 0);
        assert_eq!(decode_single_file_tar(&archive, 4096, "a"), None);
        Ok(())
    }

    #[test]
    fn tar_reader_rejects_oversized_declared_length() -> Result<(), ExecutionError> {
        let mut header = [0u8; 512];
        header[..1].copy_from_slice(b"x");
        octal(&mut header[124..136], 4096)?;
        header[156] = b'0';
        header[257..263].copy_from_slice(b"ustar\0");
        assert_eq!(decode_single_file_tar(&header, 1024, "x"), None);
        Ok(())
    }

    #[test]
    fn tar_reader_rejects_a_symlink_at_the_fixed_junit_path_without_following_it() {
        let mut header = [0u8; 512];
        header[..JUNIT_ARCHIVE_MEMBER.len()].copy_from_slice(JUNIT_ARCHIVE_MEMBER.as_bytes());
        header[156] = b'2';
        header[157..166].copy_from_slice(b"/etc/host");
        header[257..263].copy_from_slice(b"ustar\0");
        let mut archive = header.to_vec();
        archive.resize(archive.len() + 1024, 0);
        assert_eq!(
            decode_single_file_tar(&archive, MAX_JUNIT_EXPORT, JUNIT_ARCHIVE_MEMBER),
            None
        );
    }
}
