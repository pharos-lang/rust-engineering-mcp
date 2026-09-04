//! Linux ARM64 container-only assertions, shared by build.rs and proc_macro.rs.
#[cfg(not(all(target_os = "linux", target_arch = "aarch64")))]
compile_error!("rust-containment fixtures must only compile in the approved Linux ARM64 container");

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::process::{Command, Stdio};

const AF_UNIX: i32 = 1;
const AF_INET: i32 = 2;
const AF_INET6: i32 = 10;
const SOCK_STREAM: i32 = 1;
const SOCK_DGRAM: i32 = 2;
const SOCK_SEQPACKET: i32 = 5;
const SOCK_NONBLOCK: i32 = 0o4000;
const SOCK_CLOEXEC: i32 = 0o2000000;
const EPERM: i32 = 1;
const MSG_DONTWAIT: i32 = 0x40;

unsafe extern "C" {
    fn syscall(number: std::ffi::c_long, ...) -> std::ffi::c_long;
    fn socket(domain: i32, kind: i32, protocol: i32) -> i32;
    fn socketpair(domain: i32, kind: i32, protocol: i32, pair: *mut i32) -> i32;
    fn send(fd: i32, buffer: *const u8, size: usize, flags: i32) -> isize;
    fn recv(fd: i32, buffer: *mut u8, size: usize, flags: i32) -> isize;
}

fn forbidden_syscalls() {
    // Linux ARM64 numbers: Go 1.27.1 src/syscall/zsysnum_linux_arm64.go.
    // io_uring_setup: Linux v7.0 scripts/syscall.tbl (425).
    // Every argument vector is invalid even without seccomp: no usable socket,
    // pathname, target process, BPF command or io_uring parameter buffer exists.
    // clone THREAD without SIGHAND/VM is rejected before creating a child;
    // unshare's unsupported high bit rejects before changing namespaces.
    const CLONE_NEWUSER: usize = 0x1000_0000;
    const CLONE_THREAD: usize = 0x0001_0000;
    let calls: [(&str, std::ffi::c_long, [usize; 6]); 12] = [
        ("bind", 200, [usize::MAX, 0, 0, 0, 0, 0]),
        ("connect", 203, [usize::MAX, 0, 0, 0, 0, 0]),
        ("listen", 201, [usize::MAX, 0, 0, 0, 0, 0]),
        (
            "unshare",
            97,
            [CLONE_NEWUSER | (1usize << 63), 0, 0, 0, 0, 0],
        ),
        ("setns", 268, [usize::MAX, CLONE_NEWUSER, 0, 0, 0, 0]),
        ("mount", 40, [0, 0, 0, 0, 0, 0]),
        ("ptrace", 117, [usize::MAX, usize::MAX, 0, 0, 0, 0]),
        ("mknodat", 33, [usize::MAX, 0, 0, 0, 0, 0]),
        ("keyctl", 219, [usize::MAX, 0, 0, 0, 0, 0]),
        ("bpf", 280, [usize::MAX, 0, 0, 0, 0, 0]),
        ("io_uring_setup", 425, [0, 0, 0, 0, 0, 0]),
        ("clone", 220, [CLONE_NEWUSER | CLONE_THREAD, 0, 0, 0, 0, 0]),
    ];
    for (name, number, args) in calls {
        // SAFETY: this module is Linux ARM64-only. All six variadic arguments
        // have machine-word width; pointer arguments are deliberately NULL and
        // kernel-validated, never Rust references. Invalid clone flags cannot
        // return in a child on the pinned kernel ABI.
        let result =
            unsafe { syscall(number, args[0], args[1], args[2], args[3], args[4], args[5]) };
        let error = std::io::Error::last_os_error().raw_os_error();
        assert_eq!(
            result, -1,
            "forbidden syscall unexpectedly succeeded: {name}"
        );
        assert_eq!(
            error,
            Some(EPERM),
            "forbidden syscall must return EPERM: {name}"
        );
    }
}

fn inherited_descriptors() {
    // Called after sockets() returns, so every fixture-owned socketpair is closed.
    // Jobserver pipes and compiler files are legitimate: do not require just 0..2.
    let descriptors = std::fs::read_dir("/proc/self/fd").expect("descriptor evidence");
    for (index, entry) in descriptors.enumerate() {
        assert!(index < 1024, "descriptor observation exceeded bound");
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => panic!("descriptor enumeration failed: {error}"),
        };
        let name = entry.file_name();
        let name = name.to_str().expect("numeric proc descriptor name");
        assert!(
            !name.is_empty() && name.len() <= 20 && name.bytes().all(|b| b.is_ascii_digit()),
            "invalid proc descriptor name"
        );
        let target = match std::fs::read_link(entry.path()) {
            Ok(target) => target,
            // A descriptor can close between readdir and readlink, including
            // activity from the compiler; absence is not an unknown socket.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => panic!("descriptor readlink failed: {error}"),
        };
        let target = target.as_os_str().as_encoded_bytes();
        assert!(target.len() <= 4096, "descriptor target exceeded bound");
        assert!(
            !target.starts_with(b"socket:["),
            "unexpected open socket descriptor {name}"
        );
    }
}

fn text(path: &str) -> String {
    let mut bytes = Vec::new();
    File::open(path)
        .unwrap_or_else(|error| panic!("required fixture evidence {path}: {error}"))
        .take(256 * 1024 + 1)
        .read_to_end(&mut bytes)
        .expect("read bounded fixture evidence");
    assert!(bytes.len() <= 256 * 1024, "evidence exceeded bound");
    String::from_utf8(bytes).expect("UTF-8 kernel evidence")
}

fn status_field<'a>(status: &'a str, field: &str) -> &'a str {
    status
        .lines()
        .find_map(|line| line.strip_prefix(field))
        .unwrap_or_else(|| panic!("missing process field {field}"))
        .trim()
}

fn process_security() {
    let status = text("/proc/self/status");
    for field in ["Uid:", "Gid:"] {
        let ids: Vec<_> = status_field(&status, field).split_whitespace().collect();
        assert_eq!(ids, ["65534", "65534", "65534", "65534"], "{field}");
    }
    for field in ["CapInh:", "CapPrm:", "CapEff:", "CapBnd:", "CapAmb:"] {
        assert_eq!(
            u64::from_str_radix(status_field(&status, field), 16).expect("hex capabilities"),
            0,
            "{field}"
        );
    }
    assert_eq!(status_field(&status, "NoNewPrivs:"), "1");
    assert_eq!(status_field(&status, "Seccomp:"), "2");
}

fn socket_denied(domain: i32, kind: i32) {
    // Only socket creation is attempted: never bind/connect/listen/send on it.
    let fd = unsafe { socket(domain, kind, 0) };
    let error = std::io::Error::last_os_error().raw_os_error();
    if fd >= 0 {
        // SAFETY: this call just created this otherwise unowned descriptor.
        drop(unsafe { OwnedFd::from_raw_fd(fd) });
    }
    assert_eq!(fd, -1, "socket unexpectedly allowed: {domain}/{kind}");
    assert_eq!(error, Some(EPERM), "socket must be denied by policy");
}

fn socketpair_denied(domain: i32, kind: i32, protocol: i32) {
    let mut pair = [-1; 2];
    let result = unsafe { socketpair(domain, kind, protocol, pair.as_mut_ptr()) };
    let error = std::io::Error::last_os_error().raw_os_error();
    if result == 0 {
        for fd in pair {
            // SAFETY: successful socketpair created two owned descriptors.
            drop(unsafe { OwnedFd::from_raw_fd(fd) });
        }
    }
    assert_eq!(
        result, -1,
        "socketpair unexpectedly allowed: {domain}/{kind}/{protocol}"
    );
    assert_eq!(error, Some(EPERM), "socketpair must be denied by policy");
}

fn sockets() {
    for domain in [AF_INET, AF_INET6, AF_UNIX] {
        for kind in [SOCK_STREAM, SOCK_DGRAM, SOCK_SEQPACKET] {
            socket_denied(domain, kind);
        }
    }
    for (domain, kind, protocol) in [
        (AF_UNIX, SOCK_STREAM, 0),
        (AF_UNIX, SOCK_DGRAM, 0),
        (AF_UNIX, SOCK_SEQPACKET, 1),
        (AF_INET, SOCK_SEQPACKET, 0),
        (AF_INET6, SOCK_SEQPACKET, 0),
    ] {
        socketpair_denied(domain, kind, protocol);
    }
    for flags in [0, SOCK_NONBLOCK, SOCK_CLOEXEC, SOCK_NONBLOCK | SOCK_CLOEXEC] {
        let mut pair = [-1; 2];
        assert_eq!(
            unsafe { socketpair(AF_UNIX, SOCK_SEQPACKET | flags, 0, pair.as_mut_ptr()) },
            0,
            "required private IPC denied"
        );
        // SAFETY: successful socketpair created both descriptors, owned only here.
        let left = unsafe { OwnedFd::from_raw_fd(pair[0]) };
        let right = unsafe { OwnedFd::from_raw_fd(pair[1]) };
        let message = *b"IPC!";
        assert_eq!(
            unsafe {
                send(
                    left.as_raw_fd(),
                    message.as_ptr(),
                    message.len(),
                    MSG_DONTWAIT,
                )
            },
            4
        );
        let mut received = [0; 4];
        assert_eq!(
            unsafe {
                recv(
                    right.as_raw_fd(),
                    received.as_mut_ptr(),
                    received.len(),
                    MSG_DONTWAIT,
                )
            },
            4
        );
        assert_eq!(received, message);
    }
}

fn mount_options(mountinfo: &str, path: &str) -> Vec<String> {
    let entries: Vec<_> = mountinfo
        .lines()
        .filter_map(|line| {
            let (mount, _) = line.split_once(" - ")?;
            let fields: Vec<_> = mount.split_whitespace().collect();
            (fields.get(4) == Some(&path)).then(|| {
                fields
                    .get(5)
                    .expect("mount options")
                    .split(',')
                    .map(str::to_owned)
                    .collect()
            })
        })
        .collect();
    assert_eq!(
        entries.len(),
        1,
        "require exactly one visible mount entry for {path}"
    );
    entries.into_iter().next().expect("one mount")
}

fn has(options: &[String], option: &str) -> bool {
    options.iter().any(|entry| entry == option)
}

fn denied_write(path: &str) {
    let result = OpenOptions::new().write(true).create_new(true).open(path);
    assert!(
        result.is_err(),
        "protected write unexpectedly succeeded: {path}"
    );
    let error = result.err().expect("denied write");
    assert!(
        matches!(error.raw_os_error(), Some(13 | 30)),
        "require EACCES or EROFS, got {error}"
    );
}

fn filesystems(phase: &str) {
    let mountinfo = text("/proc/self/mountinfo");
    for path in ["/", "/source"] {
        let options = mount_options(&mountinfo, path);
        assert!(
            has(&options, "ro") && !has(&options, "rw"),
            "required read-only mount {path}"
        );
    }
    let work = mount_options(&mountinfo, "/work");
    assert!(
        has(&work, "rw") && !has(&work, "noexec"),
        "work must permit compiler outputs to execute"
    );
    let temp = mount_options(&mountinfo, "/tmp");
    assert!(
        has(&temp, "rw") && has(&temp, "noexec"),
        "tmp must be noexec"
    );
    let shm = mount_options(&mountinfo, "/dev/shm");
    assert!(
        has(&shm, "rw") && has(&shm, "noexec"),
        "shared memory mount must be noexec"
    );
    let suffix = format!("rust-containment-{phase}-{}", std::process::id());
    denied_write(&format!("/{suffix}"));
    denied_write(&format!("/source/{suffix}"));
    let path = format!("/work/{suffix}");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .expect("work write positive control");
    file.write_all(b"bounded-synthetic-fixture")
        .expect("write work canary");
    drop(file);
    assert_eq!(text(&path), "bounded-synthetic-fixture");
    std::fs::remove_file(path).expect("remove work canary");
}

fn cgroups() {
    assert_eq!(text("/sys/fs/cgroup/memory.max").trim(), "1073741824");
    assert_eq!(text("/sys/fs/cgroup/memory.swap.max").trim(), "0");
    assert_eq!(text("/sys/fs/cgroup/pids.max").trim(), "128");
    let cpu = text("/sys/fs/cgroup/cpu.max");
    let fields: Vec<_> = cpu.split_whitespace().collect();
    assert_eq!(fields.len(), 2, "finite CPU quota and period required");
    let quota: u64 = fields[0].parse().expect("numeric finite CPU quota");
    let period: u64 = fields[1].parse().expect("numeric CPU period");
    assert!(period > 0);
    assert_eq!(quota, period, "CPU quota must equal one CPU");
}

pub fn run(phase: &str) {
    process_security();
    sockets();
    inherited_descriptors();
    forbidden_syscalls();
    filesystems(phase);
    cgroups();
    for name in ["MCP_TEST_SYNTHETIC_SECRET", "HOST_SECRET"] {
        assert!(
            std::env::var_os(name).is_none(),
            "synthetic host environment leaked: {name}"
        );
    }
    let status = Command::new("/usr/bin/true")
        .env_clear()
        .current_dir("/work")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("Rust process spawn positive control");
    assert!(status.success(), "trusted true command failed");
}
