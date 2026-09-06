#[cfg(test)]
mod tests {
    use std::io::ErrorKind;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    #[test]
    fn source_volume_does_not_execute_a_planted_program() {
        let error = Command::new("/source/source-probe")
            .status()
            .expect_err("the source archive must not preserve an executable mode");
        assert_eq!(error.kind(), ErrorKind::PermissionDenied);
    }

    #[test]
    fn report_volume_remains_noexec() {
        let planted = "/work/coverage/planted-test-binary";
        std::fs::copy(std::env::current_exe().expect("current executable"), planted)
            .expect("copy into the writable report volume");
        std::fs::set_permissions(planted, std::fs::Permissions::from_mode(0o700))
            .expect("set the executable bit before testing the mount");
        let error = Command::new(planted)
            .status()
            .expect_err("the report volume must reject execution even with mode 0700");
        assert_eq!(error.kind(), ErrorKind::PermissionDenied);
    }

    #[test]
    fn coverage_phase_cannot_create_an_ipv4_socket() {
        let error = std::net::TcpStream::connect(("127.0.0.1", 9))
            .expect_err("ADR-064 seccomp plus network-none must deny IPv4 sockets");
        assert_eq!(error.kind(), ErrorKind::PermissionDenied);
    }
}
