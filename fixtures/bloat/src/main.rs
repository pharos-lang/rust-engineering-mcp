//! Root binary of the M5 binary-size fixture.
//!
//! Contributes exactly one large, `#[inline(never)]` public function of its own
//! and calls both it and `bloat_inner::inner_payload` through
//! `std::hint::black_box`, so neither crate's contribution can be optimized
//! away and a per-crate size report must contain a row for each.

use std::hint::black_box;

use bloat_inner::inner_payload;

/// One mixing round. Deliberately different constants from `bloat-inner`'s, so
/// the two payloads cannot be deduplicated into a single symbol by the linker.
macro_rules! round {
    ($acc:ident, $k:expr) => {
        $acc = $acc
            .rotate_left((($k as u32) & 31) + 1)
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .wrapping_sub($k as u64)
            ^ ($acc >> 29);
    };
}

/// Sixteen consecutive rounds, unrolled.
macro_rules! rounds16 {
    ($acc:ident, $base:expr) => {
        round!($acc, $base + 0);
        round!($acc, $base + 1);
        round!($acc, $base + 2);
        round!($acc, $base + 3);
        round!($acc, $base + 4);
        round!($acc, $base + 5);
        round!($acc, $base + 6);
        round!($acc, $base + 7);
        round!($acc, $base + 8);
        round!($acc, $base + 9);
        round!($acc, $base + 10);
        round!($acc, $base + 11);
        round!($acc, $base + 12);
        round!($acc, $base + 13);
        round!($acc, $base + 14);
        round!($acc, $base + 15);
    };
}

/// The `rust-mcp-bloat-fixture` contribution: 256 unrolled rounds.
#[inline(never)]
pub fn root_payload(seed: u64) -> u64 {
    let mut acc = seed ^ 0x5BE0_CD19_137E_2179;
    rounds16!(acc, 0);
    rounds16!(acc, 16);
    rounds16!(acc, 32);
    rounds16!(acc, 48);
    rounds16!(acc, 64);
    rounds16!(acc, 80);
    rounds16!(acc, 96);
    rounds16!(acc, 112);
    rounds16!(acc, 128);
    rounds16!(acc, 144);
    rounds16!(acc, 160);
    rounds16!(acc, 176);
    rounds16!(acc, 192);
    rounds16!(acc, 208);
    rounds16!(acc, 224);
    rounds16!(acc, 240);
    acc
}

fn main() {
    let seed = black_box(0x0123_4567_89AB_CDEF_u64);
    let root = black_box(root_payload(black_box(seed)));
    let inner = black_box(inner_payload(black_box(seed)));
    println!("{:016x}", root ^ inner);
}

#[cfg(test)]
mod tests {
    use super::root_payload;
    use bloat_inner::inner_payload;

    #[test]
    fn root_payload_is_deterministic() {
        assert_eq!(root_payload(0), root_payload(0));
        assert_ne!(root_payload(0), root_payload(1));
    }

    #[test]
    fn the_two_crates_contribute_distinct_payloads() {
        assert_ne!(root_payload(0), inner_payload(0));
    }
}
