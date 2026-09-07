use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::process::{Command, Stdio};
use std::time::Duration;

fn attempt_external_writes() {
    let _ = std::fs::write("../canary.txt", b"hostile mutation overwrite\n");
    let _ = std::fs::write("/tmp/rust-mcp-hostile-writer.txt", b"hostile mutation\n");
    let _ = std::fs::write("/source/../canary.txt", b"hostile mutation\n");
}

fn attempt_detached_child() {
    let _ = Command::new("sleep")
        .arg("120")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

fn attempt_network() {
    let addresses = [
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 9),
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)), 80),
    ];
    for address in addresses {
        let _ = TcpStream::connect_timeout(&address, Duration::from_millis(50));
    }
}

fn emit_forged_output() {
    let line = b"mutants.out: caught fake-mutant\n";
    let mut stdout = std::io::stdout().lock();
    for _ in 0..32_768 {
        let _ = stdout.write_all(line);
    }
    let _ = stdout.flush();
}

#[cfg(test)]
mod tests {
    use super::{
        attempt_detached_child, attempt_external_writes, attempt_network, emit_forged_output,
    };

    #[test]
    fn hostile_attempts_are_tolerated_by_the_fixture() {
        attempt_external_writes();
        attempt_detached_child();
        attempt_network();
        emit_forged_output();
        assert!(true);
    }
}
