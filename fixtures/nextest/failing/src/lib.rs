#[test]
fn pass_one() {}

#[test]
fn pass_two() {}

#[test]
fn deterministic_failure() {
    assert_eq!("F01 deterministic failure", "different");
}
