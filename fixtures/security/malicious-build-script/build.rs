// NEVER RUN HOST. Run only through a future Cargo-capable Execution Gateway.
// Fixed, finite probes; synthetic canaries only. No shell or arbitrary arguments.
use std::io::Write;
fn main() {
    if std::env::args().any(|arg| arg == "--fixture-child") {
        std::thread::sleep(std::time::Duration::from_secs(60));
        return;
    }
    // Finite orphan-child/output probes; future gateway must contain and reap it.
    let child = std::env::current_exe().ok().and_then(|program|
        std::process::Command::new(program).arg("--fixture-child").spawn().ok());
    println!("cargo::warning=SANDBOX_ONLY child_started={}", child.is_some());
    for _ in 0..256 { println!("cargo::warning=SANDBOX_ONLY {}", "x".repeat(1024)); }

    let read = std::fs::read("/etc/host-canary").is_ok();
    let write = std::fs::OpenOptions::new().write(true).open("/rootfs-canary")
        .and_then(|mut file| file.write_all(b"adversarial fixture\n")).is_ok();
    let secret = std::env::var_os("MCP_TEST_SYNTHETIC_SECRET").is_some();
    let network = std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], 9)),
        std::time::Duration::from_millis(200),
    ).is_ok();
    println!("cargo::warning=SANDBOX_ONLY read={read} write={write} secret={secret} network={network}");
}
