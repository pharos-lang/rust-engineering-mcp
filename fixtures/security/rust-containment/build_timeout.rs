//! Rename to build.rs only inside the harness-owned SourceBundle.
mod checks;
mod descendants;

fn main() {
    checks::run("timeout");
    descendants::start("timeout");
    std::thread::sleep(std::time::Duration::from_secs(60));
}
