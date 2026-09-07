pub fn generic_identity<T>(value: T) -> T {
    value
}

#[cfg(test)]
mod tests {
    use super::generic_identity;

    #[test]
    fn identity_preserves_value() {
        assert_eq!(generic_identity(17_u32), 17_u32);
        assert_eq!(generic_identity("ok"), "ok");
    }
}
