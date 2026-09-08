#[test]
fn exceeds_test_budget() { let mut n = 0u64; loop { n = n.wrapping_add(1); std::hint::black_box(n); } }
