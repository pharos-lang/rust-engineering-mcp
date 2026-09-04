use m1_16_r02::owned_label;

#[test]
fn owned_trimmed_unicode_label_survives_input_drop() {
    for (input, expected) in [
        ("", ""),
        (
            "  label
", "label",
        ),
        (" a  b ", "a  b"),
        ("\u{2003}árbol🦀\u{2003}", "árbol🦀"),
    ] {
        let value = {
            let temporary = input.to_owned();
            owned_label(&temporary)
        };
        assert_eq!(value, expected);
    }
}
