pub fn add_three(value: i32) -> i32 {
    value + 3
}

pub fn is_even_and_positive(value: i32) -> bool {
    value > 0 && value % 2 == 0
}

#[cfg(test)]
mod tests {
    use super::{add_three, is_even_and_positive};

    #[test]
    fn arithmetic_is_exact() {
        assert_eq!(add_three(4), 7);
        assert_eq!(add_three(-2), 1);
    }

    #[test]
    fn boolean_is_exact() {
        assert!(is_even_and_positive(8));
        assert!(!is_even_and_positive(7));
        assert!(!is_even_and_positive(0));
        assert!(!is_even_and_positive(-4));
    }
}
