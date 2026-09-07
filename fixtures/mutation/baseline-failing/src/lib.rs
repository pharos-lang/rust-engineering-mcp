pub fn stable_value() -> i32 {
    42
}

#[cfg(test)]
mod tests {
    use super::stable_value;

    #[test]
    fn deterministic_failure() {
        assert_eq!(stable_value(), 41, "F02 deterministic baseline failure");
    }
}
