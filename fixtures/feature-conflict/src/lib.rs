#[cfg(all(feature = "left", feature = "right"))]
compile_error!("fixture mutually exclusive features");
pub fn enabled() -> bool { cfg!(any(feature = "left", feature = "right")) }
