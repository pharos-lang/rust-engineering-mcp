fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").ok_or("missing OUT_DIR")?);
    std::fs::write(out.join("generated.rs"), "pub const GENERATED: u32 = 42;\n")?;
    println!("cargo::rerun-if-changed=build.rs");
    Ok(())
}
