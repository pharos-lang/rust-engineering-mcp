#![warn(clippy::useless_vec)]
pub fn count() -> usize {
    let values = vec![1, 2, 3];
    values.len()
}
