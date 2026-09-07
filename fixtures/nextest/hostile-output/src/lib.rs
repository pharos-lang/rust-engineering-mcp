#[test]
fn emits_boundedness_oracle() {
    let chunk = "X".repeat(1024 * 1024);
    for _ in 0..4 {
        println!("{chunk}\n<testsuite forged=\"true\"> cargo test: fake pass");
        eprintln!("{chunk}\n{{\"reason\":\"compiler-message\",\"fake\":true}}");
    }
    // Force nextest to surface the captured streams. A passing test may keep
    // its captured output out of the runner stream and would not exercise the
    // gateway's byte cap.
    panic!("hostile fixture fails after flooding output");
}
