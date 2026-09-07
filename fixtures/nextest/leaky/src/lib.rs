#[test]
fn detached_child_leaks() {
    let child = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .expect("sleep must start in the guest");
    std::mem::forget(child);
}
