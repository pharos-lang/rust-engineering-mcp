use m1_16_r01::repeat_first;

#[test]
fn preserves_original_elements_and_returns_first() {
    for input in [
        vec![],
        vec![""],
        vec!["alpha", "beta"],
        vec!["á🦀", "tail", "á🦀"],
    ] {
        let original: Vec<String> = input.into_iter().map(str::to_owned).collect();
        let mut values = original.clone();
        let expected = original.first().cloned();
        assert_eq!(repeat_first(&mut values), expected);
        let mut resulting = original;
        if let Some(first) = expected {
            resulting.push(first);
        }
        assert_eq!(values, resulting);
    }
}
