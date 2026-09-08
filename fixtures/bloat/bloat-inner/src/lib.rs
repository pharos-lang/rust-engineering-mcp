//! Path member crate of the M5 binary-size fixture.
//!
//! Contributes exactly one large, `#[inline(never)]`, dependency-free public
//! function so that a per-crate size report has a `bloat-inner` row to attribute
//! bytes to.

/// One mixing round. Straight-line, integer-only, no allocation.
macro_rules! round {
    ($acc:ident, $k:expr) => {
        $acc = $acc
            .rotate_left((($k as u32) & 31) + 1)
            .wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
            .wrapping_add($k as u64)
            ^ ($acc >> 31);
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

/// The `bloat-inner` contribution: 256 unrolled rounds, so the emitted symbol is
/// large enough to show up as its own line in a per-function report and large
/// enough that its bytes are unambiguously attributed to this crate.
///
/// `#[inline(never)]` keeps it a distinct symbol under the baseline `release`
/// profile. Under `release-lto` the linker is free to fold or relocate it; that
/// is the point of having both profiles.
#[inline(never)]
pub fn inner_payload(seed: u64) -> u64 {
    let mut acc = seed ^ 0x1F83_D9AB_FB41_BD6B;
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

#[cfg(test)]
mod tests {
    use super::inner_payload;

    #[test]
    fn inner_payload_is_deterministic() {
        assert_eq!(inner_payload(0), inner_payload(0));
        assert_ne!(inner_payload(0), inner_payload(1));
    }
}
