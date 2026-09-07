pub fn add_three(value: i32) -> i32 {
    value + 3
}

pub fn is_even_and_positive(value: i32) -> bool {
    value > 0 && value % 2 == 0
}

pub fn unchecked_value(value: i32) -> i32 {
    value * 2
}

#[cfg(test)]
mod tests {
    use super::{add_three, is_even_and_positive, unchecked_value};

    #[test]
    fn shared_logic_is_exact() {
        assert_eq!(add_three(4), 7);
        assert!(is_even_and_positive(8));
        assert!(!is_even_and_positive(-4));
    }

    #[test]
    fn unchecked_value_only_needs_to_return() {
        let _ = unchecked_value(11);
    }
}
