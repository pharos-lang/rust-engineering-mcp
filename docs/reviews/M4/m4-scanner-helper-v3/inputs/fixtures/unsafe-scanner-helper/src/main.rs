use std::env;
use std::ffi::OsStr;
use std::io::{self, Write};
use std::process;

use rust_mcp_unsafe_scanner_helper::{
    FatalErrorCode, FatalOutput, MANIFEST_PATH, MAX_FILES, MAX_OUTPUT_BYTES, run_file_worker,
    run_supervisor,
};
use serde::Serialize;

const EXIT_PROTOCOL_ERROR: i32 = 2;

fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    let arguments = env::args_os().skip(1).collect::<Vec<_>>();
    match arguments.as_slice() {
        [] => emit_result(run_supervisor(MANIFEST_PATH)),
        [flag, raw_index] if flag == OsStr::new("--file-index") => {
            let outcome =
                parse_file_index(raw_index).and_then(|index| run_file_worker(MANIFEST_PATH, index));
            emit_result(outcome)
        }
        _ => emit_and_exit(
            &FatalOutput::new(FatalErrorCode::InvalidArguments),
            EXIT_PROTOCOL_ERROR,
        ),
    }
}

fn parse_file_index(raw: &OsStr) -> Result<u32, FatalErrorCode> {
    let raw = raw.to_str().ok_or(FatalErrorCode::InvalidArguments)?;
    if raw.is_empty() || !raw.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(FatalErrorCode::InvalidArguments);
    }
    let index = raw
        .parse::<u32>()
        .map_err(|_| FatalErrorCode::InvalidArguments)?;
    if usize::try_from(index).unwrap_or(usize::MAX) >= MAX_FILES {
        return Err(FatalErrorCode::InvalidArguments);
    }
    Ok(index)
}

fn emit_result<T: Serialize>(outcome: Result<T, FatalErrorCode>) -> ! {
    match outcome {
        Ok(output) => emit_and_exit(&output, 0),
        Err(code) => emit_and_exit(&FatalOutput::new(code), EXIT_PROTOCOL_ERROR),
    }
}

fn emit_and_exit<T: Serialize>(value: &T, exit_code: i32) -> ! {
    let bytes = match serde_json::to_vec(value) {
        Ok(bytes) if bytes.len() <= MAX_OUTPUT_BYTES => bytes,
        _ => {
            let fallback = br#"{"schema_version":2,"error":"output_too_large"}"#;
            let _ = io::stdout().write_all(fallback);
            process::exit(EXIT_PROTOCOL_ERROR);
        }
    };

    let mut stdout = io::stdout().lock();
    if stdout.write_all(&bytes).is_err() || stdout.flush().is_err() {
        process::exit(EXIT_PROTOCOL_ERROR);
    }
    process::exit(exit_code);
}
