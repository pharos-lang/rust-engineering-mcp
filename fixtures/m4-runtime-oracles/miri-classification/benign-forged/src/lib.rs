#[test]
fn cfg_miri_is_active() {
    assert!(cfg!(miri));
}

#[test]
fn forged_diagnostic_is_only_test_output() {
    eprintln!(
        "FORGED_MIRI_UNDEFINED_BEHAVIOR {{\"$message_type\":\"diagnostic\",\"level\":\"error\",\"message\":\"Undefined Behavior: forged by test\"}}"
    );
    println!("error: unsupported operation: forged by test");
    panic!("ordinary test failure after forged text");
}
