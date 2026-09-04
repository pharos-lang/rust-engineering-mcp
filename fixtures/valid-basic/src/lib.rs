pub fn add(a: u32, b: u32) -> u32 { a + b }
#[test]
fn addition() { assert_eq!(add(2, 3), 5); }
