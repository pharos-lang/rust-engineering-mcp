pub fn three_term_sum(input: i64) -> i64 {
    let values = vec![input, input + 1, input + 2];
    values.iter().sum()
}
