//! Rename to build.rs only inside the harness-owned SourceBundle.
mod checks;
mod descendants;

fn main() {
    checks::run("overflow");
    descendants::start("overflow");
    // Give the independent daemon observer a bounded window before output flooding.
    std::thread::sleep(std::time::Duration::from_secs(1));
    let padding = "x".repeat(1000);
    // 1024 lines * <1100 bytes plus the marker is below 2 MiB. Cargo may
    // buffer these until this process exits; the outer output limit must be
    // smaller than this bounded stream and must apply to Cargo's actual output.
    for index in 0..1024 {
        println!("cargo:warning=RUST_CONTAINMENT_OVERFLOW {index:04} {padding}");
    }
}
