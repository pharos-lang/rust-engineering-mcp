include!(concat!(env!("OUT_DIR"), "/generated.rs"));
#[test]
fn generated_fact() { assert_eq!(GENERATED, 42); }
