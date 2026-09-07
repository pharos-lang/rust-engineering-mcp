use std::{fs, path::PathBuf};

#[test]
fn fails_once_then_passes() {
    let marker: PathBuf = std::env::temp_dir().join("rust-mcp-f01-nextest-flaky.marker");
    if !marker.exists() {
        fs::write(&marker, b"first attempt").expect("fixture marker must be writable");
        panic!("F01 deterministic first attempt");
    }
    let _ = fs::remove_file(marker);
}
