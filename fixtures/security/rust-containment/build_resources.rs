//! Container-only resource stress. Rename to build.rs inside the generated bundle.
//! Requires checks.rs beside it and the approved Linux ARM64 Rust gateway.
//! Never compile/run on the host. All helpers are this fixed executable, not a shell.
//! Cargo can buffer every marker until this build script exits; require successful
//! completion and independent gateway/container evidence, not markers alone.
mod checks;

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

const MIB: usize = 1024 * 1024;
const ENOSPC: i32 = 28;
const EAGAIN: i32 = 11;

fn text(path: impl AsRef<Path>) -> String {
    let mut bytes = Vec::new();
    File::open(path)
        .expect("required kernel/fixture evidence")
        .take(65537)
        .read_to_end(&mut bytes)
        .expect("bounded evidence read");
    assert!(bytes.len() <= 65536, "evidence exceeded bound");
    String::from_utf8(bytes).expect("UTF-8 evidence")
}

fn counter(path: &str, key: &str) -> u64 {
    text(path)
        .lines()
        .find_map(|line| {
            let mut fields = line.split_whitespace();
            (fields.next() == Some(key)).then(|| {
                let value = fields.next().expect("counter value");
                assert!(fields.next().is_none(), "malformed counter");
                value.parse().expect("numeric counter")
            })
        })
        .unwrap_or_else(|| panic!("missing {key} in {path}"))
}

/// Own only a newly created synthetic file in the private guest tmpfs.
struct ScratchPath(PathBuf);
impl Drop for ScratchPath {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn disk_limit(directory: &str, maximum: usize, label: &str) {
    let path = Path::new(directory).join(format!(
        "rust-containment-resource-{label}-{}",
        std::process::id()
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .expect("create private guest disk probe");
    let cleanup = ScratchPath(path.clone());
    let block = vec![0x5a; MIB];
    let mut written = 0usize;
    let mut denied = false;
    while written < maximum {
        let count = block.len().min(maximum - written);
        match file.write(&block[..count]) {
            Ok(0) => panic!("disk probe made zero progress without ENOSPC"),
            Ok(count) => written += count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => {
                assert_eq!(
                    error.raw_os_error(),
                    Some(ENOSPC),
                    "disk must fail with ENOSPC"
                );
                denied = true;
                break;
            }
        }
    }
    // Remove and close before any subsequent resource stress; tmpfs pages count
    // against memory.max. No fsync is needed for this ephemeral write oracle.
    drop(file);
    std::fs::remove_file(&path).expect("remove disk probe before next stress");
    drop(cleanup);
    assert!(
        denied,
        "disk limit not observed within finite {label} bound"
    );
    assert!(written > 0 && written <= maximum);
    println!("cargo:warning=RUST_CONTAINMENT_RESOURCE {label} ENOSPC bytes={written}");
}

/// Every started helper remains owned until kill+wait, including unwinding.
struct Helpers(Vec<Child>);
impl Helpers {
    fn stop_all(&mut self) {
        for child in &mut self.0 {
            if child
                .try_wait()
                .expect("inspect helper before cleanup")
                .is_none()
            {
                child.kill().expect("kill owned helper");
            }
            child.wait().expect("reap owned helper");
        }
        self.0.clear();
    }
}
impl Drop for Helpers {
    fn drop(&mut self) {
        for child in &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn helper(mode: &str) -> Command {
    let executable = std::env::current_exe().expect("current fixture executable");
    assert!(
        executable.starts_with("/work"),
        "fixture must execute from guest work tmpfs"
    );
    let mut command = Command::new(executable);
    command
        .arg(mode)
        .env_clear()
        .current_dir("/work")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

fn pid_limit() {
    let before = counter("/sys/fs/cgroup/pids.events", "max");
    let mut helpers = Helpers(Vec::new());
    let mut denied = false;
    for _ in 0..150 {
        match helper("--sleep60").spawn() {
            Ok(child) => helpers.0.push(child),
            Err(error) => {
                assert_eq!(
                    error.raw_os_error(),
                    Some(EAGAIN),
                    "helper creation must fail with EAGAIN"
                );
                denied = true;
                break;
            }
        }
    }
    let started = helpers.0.len();
    let after = counter("/sys/fs/cgroup/pids.events", "max");
    // Check that helpers did not simply exit and recycle slots. They are fixed
    // 60-second sleepers; the complete outer calibration deadline is 30 seconds.
    for child in &mut helpers.0 {
        assert!(
            child.try_wait().expect("inspect sleeper").is_none(),
            "sleeper exited early"
        );
    }
    helpers.stop_all();
    assert!(
        denied && started > 0 && started <= 150,
        "PID cap not reached within finite helper bound"
    );
    assert!(after > before, "pids.events max must increase");
    println!(
        "cargo:warning=RUST_CONTAINMENT_RESOURCE pids EAGAIN helpers={started} max_delta={}",
        after - before
    );
}

fn memory_marker(pid: u32) -> PathBuf {
    PathBuf::from(format!("/work/rust-containment-memory-ready-{pid}"))
}

fn memory_child() {
    // Raising one's own badness score needs no privilege. A successful write is
    // required; otherwise this fixture must not risk selecting Cargo as victim.
    std::fs::write("/proc/self/oom_score_adj", b"1000").expect("raise own OOM score");
    assert_eq!(text("/proc/self/oom_score_adj").trim(), "1000");
    let mut ready = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(memory_marker(std::process::id()))
        .expect("memory helper readiness marker");
    ready
        .write_all(b"oom_score_adj=1000\n")
        .expect("write readiness marker");
    drop(ready);
    let mut retained = Vec::new();
    // 256 * 8 MiB = exactly 2 GiB maximum. Nonzero initialization touches pages;
    // retaining every chunk prevents allocator reuse from hiding real pressure.
    for _ in 0..256 {
        let bytes = vec![0x5a; 8 * MIB];
        std::hint::black_box(&bytes);
        retained.push(bytes);
    }
    std::hint::black_box(&retained);
    // Reaching this point disproves this particular 1 GiB enforcement oracle.
    std::process::exit(91);
}

fn wait_bounded(child: &mut Child, timeout: Duration) -> ExitStatus {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().expect("inspect memory helper") {
            return status;
        }
        assert!(
            Instant::now() < deadline,
            "memory helper did not terminate within bounded wait"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn memory_limit() {
    assert_eq!(
        text("/sys/fs/cgroup/memory.oom.group").trim(),
        "0",
        "group OOM could kill the harness before it records evidence"
    );
    let parent_score: i32 = text("/proc/self/oom_score_adj")
        .trim()
        .parse()
        .expect("parent OOM score");
    assert!(
        parent_score < 1000,
        "parent must be less preferred than the memory helper"
    );
    let before = counter("/sys/fs/cgroup/memory.events", "oom_kill");
    let child = helper("--memory")
        .spawn()
        .expect("spawn preferred OOM victim");
    let marker = ScratchPath(memory_marker(child.id()));
    let mut helpers = Helpers(vec![child]);
    let status = wait_bounded(&mut helpers.0[0], Duration::from_secs(10));
    let after = counter("/sys/fs/cgroup/memory.events", "oom_kill");
    helpers.stop_all();
    assert_eq!(
        text(&marker.0),
        "oom_score_adj=1000\n",
        "helper must establish victim priority before allocation"
    );
    drop(marker);
    assert_eq!(
        status.signal(),
        Some(9),
        "memory helper must be killed with SIGKILL"
    );
    assert!(after > before, "memory.events oom_kill must increase");
    println!(
        "cargo:warning=RUST_CONTAINMENT_RESOURCE memory SIGKILL oom_kill_delta={}",
        after - before
    );
}

fn cpu_limit() {
    let before = counter("/sys/fs/cgroup/cpu.stat", "nr_throttled");
    let deadline = Instant::now() + Duration::from_millis(1200);
    std::thread::scope(|scope| {
        for seed in 1u64..=4 {
            scope.spawn(move || {
                let mut state = seed;
                while Instant::now() < deadline {
                    for _ in 0..4096 {
                        state = state
                            .wrapping_mul(6364136223846793005)
                            .wrapping_add(1)
                            .rotate_left(7);
                    }
                    std::hint::black_box(state);
                }
            });
        }
    });
    let after = counter("/sys/fs/cgroup/cpu.stat", "nr_throttled");
    // An extremely CPU-starved daemon can make this oracle inconclusive. Failure
    // rejects the run; quota metadata alone is not accepted as active throttling.
    assert!(
        after > before,
        "four busy threads must trigger CFS throttling"
    );
    println!(
        "cargo:warning=RUST_CONTAINMENT_RESOURCE cpu nr_throttled_delta={}",
        after - before
    );
}

fn main() {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    match arguments.as_slice() {
        [mode] if mode == "--sleep60" => {
            std::thread::sleep(Duration::from_secs(60));
            return;
        }
        [mode] if mode == "--memory" => {
            memory_child();
            return;
        }
        [] => {}
        _ => panic!("unexpected fixture arguments"),
    }
    checks::run("resources");
    // Filling a 512 MiB tmpfs is charged to the 1 GiB cgroup. Require enough
    // initial headroom; this test must not accidentally turn disk calibration
    // into an uncontrolled OOM of Cargo/the parent before the preferred victim.
    let baseline: u64 = text("/sys/fs/cgroup/memory.current")
        .trim()
        .parse()
        .expect("current memory");
    assert!(
        baseline <= 256 * MIB as u64,
        "insufficient initial headroom for bounded disk stress"
    );
    disk_limit("/tmp", 80 * MIB, "tmp");
    disk_limit("/work", 540 * MIB, "work");
    pid_limit();
    memory_limit();
    cpu_limit();
    println!("cargo:warning=RUST_CONTAINMENT_RESOURCES_PASSED");
}
