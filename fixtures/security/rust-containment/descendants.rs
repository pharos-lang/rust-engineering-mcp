//! Fixed, bounded detached descendant for container-only cleanup calibration.
#[cfg(not(all(target_os = "linux", target_arch = "aarch64")))]
compile_error!("descendant fixture is Linux ARM64 container-only");

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::FromRawFd;

const O_RDWR: i32 = 2;
const O_CLOEXEC: i32 = 0o2000000;

unsafe extern "C" {
    fn fork() -> i32;
    fn setsid() -> i32;
    fn getsid(pid: i32) -> i32;
    fn pipe2(pipe: *mut i32, flags: i32) -> i32;
    fn open(path: *const std::ffi::c_char, flags: i32, ...) -> i32;
    fn dup2(old: i32, new: i32) -> i32;
    fn close(fd: i32) -> i32;
    fn write(fd: i32, bytes: *const u8, count: usize) -> isize;
    fn waitpid(pid: i32, status: *mut i32, flags: i32) -> i32;
    fn sleep(seconds: u32) -> u32;
    fn _exit(code: i32) -> !;
}

/// Only fixed libc calls and stack data run after fork: no allocator, logging,
/// Rust destructor or panic path in either child. The intermediate child is
/// reaped by the original process; the grandchild is deliberately not reaped
/// here so that container-wide cleanup, rather than a process-group kill, matters.
unsafe fn detached_child(read_fd: i32, write_fd: i32) -> ! {
    unsafe {
        close(read_fd);
        if setsid() < 0 {
            _exit(111);
        }
        let null = open(c"/dev/null".as_ptr(), O_RDWR);
        if null < 0 {
            _exit(112);
        }
        for target in [0, 1, 2] {
            if dup2(null, target) < 0 {
                _exit(113);
            }
        }
        if null > 2 {
            close(null);
        }
        let descendant = fork();
        if descendant < 0 {
            _exit(114);
        }
        if descendant > 0 {
            let bytes = descendant.to_ne_bytes();
            if write(write_fd, bytes.as_ptr(), bytes.len()) != bytes.len() as isize {
                _exit(115);
            }
            close(write_fd);
            _exit(0);
        }
        close(write_fd);
        let mut remaining = 60;
        while remaining != 0 {
            remaining = sleep(remaining);
        }
        _exit(0);
    }
}

pub fn start(phase: &str) {
    let mut pipe = [-1; 2];
    assert_eq!(
        unsafe { pipe2(pipe.as_mut_ptr(), O_CLOEXEC) },
        0,
        "pipe2 positive control"
    );
    let first = unsafe { fork() };
    if first == 0 {
        // SAFETY: this is the single-threaded build-script child. This function
        // never returns and uses fixed libc calls only, then _exit.
        unsafe { detached_child(pipe[0], pipe[1]) }
    }
    // SAFETY: parent owns both descriptors returned by successful pipe2.
    let mut reader = unsafe { File::from_raw_fd(pipe[0]) };
    drop(unsafe { File::from_raw_fd(pipe[1]) });
    assert!(first > 0, "fork positive control");
    let mut bytes = [0; 4];
    reader
        .read_exact(&mut bytes)
        .expect("detached PID handshake");
    drop(reader);
    let mut status = -1;
    assert_eq!(
        unsafe { waitpid(first, &mut status, 0) },
        first,
        "reap intermediate child"
    );
    assert_eq!(status, 0, "intermediate child failed before handshake");
    let descendant = i32::from_ne_bytes(bytes);
    assert!(descendant > 0 && descendant != first);
    assert_eq!(
        unsafe { getsid(descendant) },
        first,
        "descendant must retain detached session"
    );
    let marker = format!(
        "/work/rust-containment-descendant-{phase}-{}.pid",
        std::process::id()
    );
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&marker)
        .expect("bounded guest descendant marker");
    writeln!(file, "pid={descendant} sid={first}").expect("write descendant marker");
    drop(file);
    println!(
        "cargo:warning=RUST_CONTAINMENT_DESCENDANT phase={phase} pid={descendant} sid={first} marker={marker}"
    );
    std::io::stdout()
        .flush()
        .expect("flush producer, not Cargo's downstream buffer");
}
