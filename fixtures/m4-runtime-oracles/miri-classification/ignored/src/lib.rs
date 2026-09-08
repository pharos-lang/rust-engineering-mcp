#[test]
#[ignore = "qualification proves skipped-only is not clean"]
fn ignored_test_must_not_credit_clean() {
    panic!("this ignored body must not run");
}
