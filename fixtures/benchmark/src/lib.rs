//! Measurement oracle for the M5 benchmark adapter.
//!
//! Three pure, integer-only, allocation-free workloads whose per-iteration
//! cost stands in a *known design ratio*. Every one of them runs the same
//! inner [`step`] operation; only the number of times it runs and the order
//! of the index sequence differ.
//!
//! | Function        | `step` calls for `n` | Design ratio vs `work_unit` |
//! | --------------- | -------------------- | --------------------------- |
//! | `work_unit`     | `n`                  | 1.00x (reference)           |
//! | `work_slower`   | `n + n / 4`          | 1.25x                       |
//! | `work_noisy`    | `n`                  | 1.00x (self-compare control)|
//!
//! The 1.25x figure is exact in *operation count* only when `n` is a multiple
//! of four, because `n / 4` truncates. It is a design ratio, never a validated
//! measurement: see `README.md`.

/// Deterministic, data-independent inner step shared by all three workloads.
///
/// The result of each call feeds the next, so the loops carry a real data
/// dependency and cannot be collapsed into a closed form by the optimizer.
#[inline(always)]
const fn step(acc: u64, i: u64) -> u64 {
    let mixed = acc ^ i.wrapping_mul(0x2545_F491_4F6C_DD1D);
    mixed.rotate_left(17).wrapping_add(0x9E37_79B9_7F4A_7C15)
}

/// Seed for every workload, so all three start from the same state.
const SEED: u64 = 0x243F_6A88_85A3_08D3;

/// The 1.00x reference: `n` calls to [`step`] over the ascending indices
/// `0..n`.
///
/// The return value is meaningless on its own and must be consumed with
/// `black_box` by the caller.
#[inline(never)]
pub fn work_unit(n: u64) -> u64 {
    let mut acc = SEED;
    let mut i = 0;
    while i < n {
        acc = step(acc, i);
        i += 1;
    }
    acc
}

/// 25% more of the identical inner operation: `n + n / 4` calls to [`step`]
/// over the ascending indices `0..(n + n / 4)`.
///
/// The expected design ratio against [`work_unit`] is 1.25x, exact in
/// operation count when `n` is a multiple of four.
#[inline(never)]
pub fn work_slower(n: u64) -> u64 {
    let total = n + n / 4;
    let mut acc = SEED;
    let mut i = 0;
    while i < total {
        acc = step(acc, i);
        i += 1;
    }
    acc
}

/// The noise / self-compare control: exactly the same total work as
/// [`work_unit`] — `n` calls to [`step`] over the same index set — but walked
/// in descending order.
///
/// The pattern is fixed at compile time and depends on no input data, so the
/// only difference an adapter can observe against `reference` is measurement
/// noise. The expected design ratio is 1.00x.
#[inline(never)]
pub fn work_noisy(n: u64) -> u64 {
    let mut acc = SEED;
    let mut i = n;
    while i > 0 {
        i -= 1;
        acc = step(acc, i);
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::{SEED, work_noisy, work_slower, work_unit};

    #[test]
    fn zero_iterations_return_the_seed() {
        assert_eq!(work_unit(0), SEED);
        assert_eq!(work_slower(0), SEED);
        assert_eq!(work_noisy(0), SEED);
    }

    #[test]
    fn workloads_are_deterministic() {
        assert_eq!(work_unit(4096), work_unit(4096));
        assert_eq!(work_slower(4096), work_slower(4096));
        assert_eq!(work_noisy(4096), work_noisy(4096));
    }

    #[test]
    fn slower_runs_a_quarter_more_steps() {
        // `work_slower(n)` is `work_unit(n + n / 4)` by construction.
        assert_eq!(work_slower(4096), work_unit(4096 + 1024));
    }

    #[test]
    fn control_visits_the_same_index_set_as_the_reference() {
        // Same count, reversed order, so the accumulated values differ while
        // the operation count does not.
        assert_ne!(work_noisy(4096), work_unit(4096));
        assert_eq!(work_noisy(1), work_unit(1));
    }
}
