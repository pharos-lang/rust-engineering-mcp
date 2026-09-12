pub fn total(values: &[u32]) -> u32 {
    let sum = values.iter().sum::<u32>();
    sum + 1
}
