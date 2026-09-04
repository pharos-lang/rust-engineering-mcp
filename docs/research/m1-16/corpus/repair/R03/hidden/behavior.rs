use m1_16_r03::three_term_sum;

#[test]
fn preserves_sum_over_declared_domain() {
    for input in [-1_000_000, -17, -2, -1, 0, 1, 17, 999_999, 1_000_000] {
        assert_eq!(three_term_sum(input), 3 * input + 3);
    }
}
