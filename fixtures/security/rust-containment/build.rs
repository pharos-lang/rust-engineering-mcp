//! Container-only build-script fixture; the harness creates its Cargo manifest.
mod checks;

fn main() {
    checks::run("build");
    println!("cargo:warning=RUST_CONTAINMENT_BUILD_CHECKS_PASSED");
}
