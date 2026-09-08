//! Sampling-profiler oracle for M5.
//!
//! The process spends the overwhelming majority of its wall time inside a
//! single, named, never-inlined frame reached through a fixed three-deep call
//! chain, so a sampled stack has a known shape:
//!
//! ```text
//! main;level_one;level_two;known_hot_frame
//! ```
//!
//! Everything outside that chain — argument parsing, the final `println!` — runs
//! once and costs microseconds.
//!
//! # Usage
//!
//! ```text
//! rust-mcp-profile-workload            # busy-loop for the default 2000 ms
//! rust-mcp-profile-workload <MILLIS>   # busy-loop for <MILLIS> ms
//! rust-mcp-profile-workload --zero     # zero-sample control: return at once
//! ```
//!
//! Any other argv is rejected with exit code 2.

use std::hint::black_box;
use std::time::{Duration, Instant};

/// Millisecond budget used when no argument is given.
const DEFAULT_BUDGET_MS: u64 = 2000;

/// Exit code for a rejected command line.
const EXIT_USAGE: i32 = 2;

/// Number of inner steps between two clock reads. Large enough that
/// `Instant::now` is a negligible share of the hot frame, small enough that the
/// budget is honoured closely.
const CHUNK: u64 = 4096;

/// Seed for the busy loop.
const SEED: u64 = 0x243F_6A88_85A3_08D3;

/// What the command line asked for.
#[derive(Debug, PartialEq, Eq)]
enum Mode {
    /// Busy-loop in `known_hot_frame` for this long.
    Budget(Duration),
    /// Zero-sample control: `known_hot_frame` returns immediately.
    Zero,
}

/// Parse argv (without argv[0]).
///
/// * no argument -> the default budget
/// * one integer -> that many milliseconds
/// * `--zero` -> the zero-sample control
/// * anything else -> `Err`, which `main` turns into exit code 2
fn parse_args(args: &[String]) -> Result<Mode, String> {
    match args {
        [] => Ok(Mode::Budget(Duration::from_millis(DEFAULT_BUDGET_MS))),
        [one] if one == "--zero" => Ok(Mode::Zero),
        [one] => match one.parse::<u64>() {
            Ok(ms) => Ok(Mode::Budget(Duration::from_millis(ms))),
            Err(_) => Err(format!("not a millisecond budget: {one}")),
        },
        _ => Err(format!("expected at most one argument, got {}", args.len())),
    }
}

/// Deterministic, data-independent inner step of the busy loop.
#[inline(always)]
const fn step(acc: u64, i: u64) -> u64 {
    let mixed = acc ^ i.wrapping_mul(0x2545_F491_4F6C_DD1D);
    mixed.rotate_left(17).wrapping_add(0x9E37_79B9_7F4A_7C15)
}

/// The frame a profiler is expected to attribute essentially all samples to.
///
/// Busy-loops until `budget` has elapsed, measured with [`Instant`]. A zero
/// budget — the `--zero` control — returns immediately without executing a
/// single step, so the process exits before any sampler can take a sample of
/// this frame.
#[inline(never)]
pub fn known_hot_frame(budget: Duration) -> u64 {
    if budget.is_zero() {
        return 0;
    }
    let start = Instant::now();
    let mut acc = SEED;
    let mut i: u64 = 0;
    loop {
        let end = i + CHUNK;
        while i < end {
            acc = step(acc, i);
            i += 1;
        }
        if start.elapsed() >= budget {
            break;
        }
    }
    black_box(acc)
}

/// Second frame of the known chain.
///
/// The call to [`known_hot_frame`] is deliberately not in tail position, so it
/// cannot be turned into a sibling call that would collapse this frame away.
#[inline(never)]
fn level_two(budget: Duration) -> u64 {
    let inner = known_hot_frame(budget);
    inner.rotate_left(1)
}

/// First frame of the known chain, called directly from `main`.
///
/// The call to [`level_two`] is deliberately not in tail position, for the same
/// reason.
#[inline(never)]
fn level_one(budget: Duration) -> u64 {
    let inner = level_two(budget);
    inner.wrapping_add(1)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = match parse_args(&args) {
        Ok(mode) => mode,
        Err(message) => {
            eprintln!("rust-mcp-profile-workload: {message}");
            eprintln!("usage: rust-mcp-profile-workload [<MILLIS>|--zero]");
            std::process::exit(EXIT_USAGE);
        }
    };

    let budget = match mode {
        Mode::Budget(budget) => budget,
        Mode::Zero => Duration::ZERO,
    };

    let result = level_one(black_box(budget));
    println!("{result:016x}");
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_BUDGET_MS, Duration, Mode, known_hot_frame, level_one, parse_args};

    fn argv(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn no_argument_uses_the_default_budget() {
        assert_eq!(
            parse_args(&argv(&[])),
            Ok(Mode::Budget(Duration::from_millis(DEFAULT_BUDGET_MS)))
        );
    }

    #[test]
    fn one_integer_is_a_millisecond_budget() {
        assert_eq!(
            parse_args(&argv(&["50"])),
            Ok(Mode::Budget(Duration::from_millis(50)))
        );
    }

    #[test]
    fn zero_flag_selects_the_control() {
        assert_eq!(parse_args(&argv(&["--zero"])), Ok(Mode::Zero));
    }

    #[test]
    fn other_argv_is_rejected() {
        assert!(parse_args(&argv(&["nope"])).is_err());
        assert!(parse_args(&argv(&["50", "50"])).is_err());
        assert!(parse_args(&argv(&["--zero", "50"])).is_err());
        assert!(parse_args(&argv(&["-1"])).is_err());
    }

    #[test]
    fn zero_budget_returns_immediately() {
        assert_eq!(known_hot_frame(Duration::ZERO), 0);
    }

    #[test]
    fn a_budget_is_honoured() {
        let start = std::time::Instant::now();
        let _ = level_one(Duration::from_millis(30));
        assert!(start.elapsed() >= Duration::from_millis(30));
    }
}
