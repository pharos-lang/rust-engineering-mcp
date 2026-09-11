//! Library half of the observed-compilation-failure fixture.
//!
//! It compiles. The failure this fixture exists to produce belongs to
//! `benches/perf.rs` alone, so a run that fails here would be a broken fixture
//! rather than the case under test.

/// Deterministic, allocation-free unit of work, taking and returning `u64`.
///
/// The bench calls it with the wrong argument type on purpose; the signature is
/// what makes that a type error rather than a runtime one.
#[inline(never)]
pub fn work_unit(n: u64) -> u64 {
    let mut acc = 0x243F_6A88_85A3_08D3_u64;
    let mut i = 0;
    while i < n {
        acc = acc ^ i.wrapping_mul(0x2545_F491_4F6C_DD1D);
        acc = acc.rotate_left(17).wrapping_add(0x9E37_79B9_7F4A_7C15);
        i += 1;
    }
    acc
}
