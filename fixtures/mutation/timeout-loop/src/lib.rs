pub fn count_to(target: i64) -> i64 {
    let mut value = 0;
    while value != target {
        value += 1;
    }
    value
}

#[cfg(test)]
mod tests {
    use super::count_to;

    #[test]
    fn count_reaches_target() {
        assert_eq!(count_to(3), 3);
    }
}
