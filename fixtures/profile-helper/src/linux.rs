//! The Linux sampling profiler.
//!
//! This module holds every syscall the helper makes. It compiles on any Linux
//! target so the syscall path stays type-checked, but it refuses to run on
//! anything but aarch64: the ring-buffer geometry and the ELF symbolizer are
//! only exercised and calibrated for `aarch64-unknown-linux-gnu`.
//!
//! The sequence is fixed:
//!
//! 1. fork/exec the child with `raise(SIGSTOP)` in `pre_exec`, on a helper
//!    thread because `Command::spawn` blocks until the child execs;
//! 2. `waitpid(..., WUNTRACED)` until the child is stopped;
//! 3. `perf_event_open` on that pid only, once per online CPU;
//! 4. `mmap` one metadata page plus eight data pages per event;
//! 5. `PERF_EVENT_IOC_ENABLE` on every event, then `SIGCONT`;
//! 6. drain and merge all the rings until the child exits, the duration
//!    elapses or the sample cap is reached;
//! 7. `PERF_EVENT_IOC_DISABLE`, kill and reap, empty the PID namespace, drain
//!    what is left, then write the two artifacts.
//!
//! Step 7 empties the namespace before anything is rendered or written. The
//! helper is pid 1 of a namespace of its own, and the profiled program is free
//! to double-fork: killing only the direct child leaves grandchildren running
//! across the render and both writes, and the last write before pid 1 exits is
//! the one the export phase tars. `kill(-1, SIGKILL)` plus a bounded
//! `waitpid(-1, …)` loop closes that, and it also closes the same gap on the
//! `duration_limit` and `sample_limit` paths.
//!
//! The per-CPU fan-out in steps 3 and 4 is forced by the kernel: `perf_mmap`
//! refuses an inherited event opened with `cpu == -1`, so keeping `inherit = 1`
//! means opening one event and one ring per CPU and merging them.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use rust_mcp_profile_helper::{
    Arguments, DrainPlan, EXIT_INTERNAL_FAILURE, EXIT_PROFILER_UNAVAILABLE, EXIT_SUCCESS, Manifest,
    NamespaceDrain, ProfileStatus, Record, RunOutcome, SampleTally, StackCollector, SymbolTable,
    clamp_cpu_count, classify_status, drain_plan, drain_ring, merge_ring_records,
    parse_elf_symbols, render_manifest,
};

use crate::{UNSUPPORTED, report};

// -- perf_event_attr ---------------------------------------------------------

const PERF_TYPE_SOFTWARE: u32 = 1;
const PERF_COUNT_SW_CPU_CLOCK: u64 = 0;
/// `PERF_ATTR_SIZE_VER7`: the exact byte count of [`PerfEventAttr`].
const PERF_ATTR_SIZE: u32 = 128;

const PERF_SAMPLE_IP: u64 = 1 << 0;
const PERF_SAMPLE_TID: u64 = 1 << 1;
const PERF_SAMPLE_TIME: u64 = 1 << 2;
const PERF_SAMPLE_CALLCHAIN: u64 = 1 << 5;
/// `0x27`.
const SAMPLE_TYPE: u64 =
    PERF_SAMPLE_IP | PERF_SAMPLE_TID | PERF_SAMPLE_TIME | PERF_SAMPLE_CALLCHAIN;

// The `perf_event_attr` bitfield word, LSB first as the kernel declares it.
const ATTR_DISABLED: u64 = 1 << 0;
const ATTR_INHERIT: u64 = 1 << 1;
const ATTR_EXCLUDE_KERNEL: u64 = 1 << 5;
const ATTR_EXCLUDE_HV: u64 = 1 << 6;
const ATTR_MMAP: u64 = 1 << 8;
const ATTR_COMM: u64 = 1 << 9;
const ATTR_FREQ: u64 = 1 << 10;

const PERF_EVENT_IOC_ENABLE: libc::c_long = 0x2400;
const PERF_EVENT_IOC_DISABLE: libc::c_long = 0x2401;

const NO_GROUP: libc::c_long = -1;
const NO_FLAGS: libc::c_long = 0;

/// `struct perf_event_attr` up to and including `sig_data`, which is exactly
/// the 128 bytes announced in `size`. Written once, read only by the kernel.
#[repr(C)]
#[derive(Clone, Copy)]
#[allow(
    dead_code,
    reason = "every field is read by the kernel through the attr pointer"
)]
struct PerfEventAttr {
    kind: u32,
    size: u32,
    config: u64,
    sample_freq: u64,
    sample_type: u64,
    read_format: u64,
    flags: u64,
    wakeup_events: u32,
    bp_type: u32,
    config1: u64,
    config2: u64,
    branch_sample_type: u64,
    sample_regs_user: u64,
    sample_stack_user: u32,
    clockid: i32,
    sample_regs_intr: u64,
    aux_watermark: u32,
    sample_max_stack: u16,
    reserved_2: u16,
    aux_sample_size: u32,
    reserved_3: u32,
    sig_data: u64,
}

const _: () = assert!(size_of::<PerfEventAttr>() == PERF_ATTR_SIZE as usize);

// -- ring buffer geometry ----------------------------------------------------

/// One metadata page plus `2^3` data pages.
const DATA_PAGE_COUNT: usize = 8;
/// Byte offsets of the fields we use inside `struct perf_event_mmap_page`.
const RING_DATA_HEAD: usize = 1024;
const RING_DATA_TAIL: usize = 1032;
const RING_DATA_OFFSET: usize = 1040;
const RING_DATA_SIZE: usize = 1048;

// -- policy ------------------------------------------------------------------

const POLL_INTERVAL: Duration = Duration::from_millis(1);
const CHILD_START_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_MODULE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_CACHED_MODULES: usize = 512;
/// Bound on merge rounds per drain pass, so a busy child cannot keep the
/// draining loop from returning to its own deadline checks.
const MAX_DRAIN_ROUNDS: usize = 64;
/// Bound on emptying the PID namespace. SIGKILL cannot be caught, so this is
/// reached only if the kernel keeps a task unreapable; when it expires the
/// manifest says so instead of claiming a clean run.
const NAMESPACE_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);

// -- entry point -------------------------------------------------------------

/// Profiles `arguments.program`, writes both output files and returns the
/// process exit code.
pub fn profile(arguments: &Arguments) -> i32 {
    if !cfg!(target_arch = "aarch64") {
        report(UNSUPPORTED);
        return EXIT_INTERNAL_FAILURE;
    }

    let spawner = spawn_stopped_child(arguments);
    let Some(child) = await_stopped_child(&spawner) else {
        let _ = spawner.join();
        report("internal failure: the program could not be started");
        let drained = drain_namespace(None);
        let manifest = blank_manifest(
            arguments,
            ProfileStatus::ChildExited,
            drained,
            None,
            None,
            None,
        );
        return emit(arguments, "", &manifest, EXIT_INTERNAL_FAILURE);
    };

    let mut events = match open_events(arguments.frequency_hz, child) {
        Ok(events) => events,
        Err(errno) => return abandon(arguments, spawner, child, errno),
    };
    // There is no group leader, so every event is enabled on its own. An event
    // that refuses to arm is dropped, exactly like one that refused to open.
    let mut enable_errno = libc::ENODEV;
    events.retain(|event| match perf_ioctl(event.fd, PERF_EVENT_IOC_ENABLE) {
        Ok(()) => true,
        Err(errno) => {
            enable_errno = errno;
            false
        }
    });
    if events.is_empty() {
        return abandon(arguments, spawner, child, enable_errno);
    }
    let cpus_sampled = u32::try_from(events.len()).unwrap_or(u32::MAX);

    let started = Instant::now();
    signal_child(child, libc::SIGCONT);
    // `spawn` returns as soon as the continued child reaches `exec`. A spawn
    // error here means the exec failed and the runtime already reaped the pid.
    let spawn_failed = !matches!(spawner.join(), Ok(Ok(_)));

    let mut collector = Collector::new(arguments);
    let mut outcome = RunOutcome {
        child_exited_on_its_own: spawn_failed,
        ..RunOutcome::default()
    };
    let mut child_exit_code = None;
    let mut child_signal = None;
    let mut reaped = spawn_failed;

    if !spawn_failed {
        let budget = Duration::from_millis(arguments.duration_ms);
        loop {
            drain_all(&events, &mut collector);
            if collector.tally().samples_collected >= arguments.max_samples {
                outcome.sample_limit_reached = true;
                break;
            }
            if let Some((code, signal)) = try_reap(child) {
                child_exit_code = code;
                child_signal = signal;
                reaped = true;
                outcome.child_exited_on_its_own = true;
                break;
            }
            if started.elapsed() >= budget {
                outcome.duration_elapsed = true;
                break;
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    for event in &events {
        let _ = perf_ioctl(event.fd, PERF_EVENT_IOC_DISABLE);
    }
    if !reaped {
        signal_child(child, libc::SIGKILL);
        let (code, signal) = reap(child);
        child_exit_code = code;
        child_signal = signal;
    }
    // Nothing of the profiled program may still be running while the profile is
    // rendered and written: a double-forked grandchild survives the kill above,
    // and whatever it writes last is what the export phase would tar.
    let drained = drain_namespace(Some(child));
    drain_all(&events, &mut collector);
    let observed = started.elapsed();
    // Dropping the events unmaps every ring and closes every descriptor.
    drop(events);

    let tally = collector.tally();
    outcome.samples_collected = tally.samples_collected;
    let manifest = Manifest {
        status: classify_status(&outcome),
        frequency_hz: arguments.frequency_hz,
        requested_duration_ms: arguments.duration_ms,
        observed_duration_ms: u64::try_from(observed.as_millis()).unwrap_or(u64::MAX),
        samples_collected: tally.samples_collected,
        samples_lost: tally.samples_lost,
        stacks_written: collector.stacks_written(),
        frames_total: tally.frames_total,
        frames_unresolved: tally.frames_unresolved,
        stacks_truncated: tally.stacks_truncated,
        max_depth: arguments.max_depth,
        modules_seen: collector.modules_seen(),
        cpus_sampled,
        descendants_reaped: drained.descendants_reaped,
        namespace_drained: drained.namespace_drained,
        child_exit_code,
        child_signal,
        perf_errno: None,
    };
    emit(arguments, &collector.render(), &manifest, EXIT_SUCCESS)
}

/// The refusal path, taken when no per-CPU event could be opened, mapped and
/// armed: kill the stopped child, reap it, write an empty stacks file and a
/// manifest carrying the errno and `cpus_sampled: 0`.
fn abandon(
    arguments: &Arguments,
    spawner: JoinHandle<io::Result<Child>>,
    child: libc::pid_t,
    errno: i32,
) -> i32 {
    signal_child(child, libc::SIGKILL);
    let spawned = matches!(spawner.join(), Ok(Ok(_)));
    let (code, signal) = if spawned { reap(child) } else { (None, None) };
    // The refusal path writes artifacts too, so it empties the namespace on the
    // same rule the sampled path does.
    let drained = drain_namespace(Some(child));
    report("profiler unavailable: the kernel refused the performance events");
    let manifest = blank_manifest(
        arguments,
        ProfileStatus::ProfilerUnavailable,
        drained,
        code,
        signal,
        Some(errno),
    );
    emit(arguments, "", &manifest, EXIT_PROFILER_UNAVAILABLE)
}

fn blank_manifest(
    arguments: &Arguments,
    status: ProfileStatus,
    drained: NamespaceDrain,
    child_exit_code: Option<i32>,
    child_signal: Option<i32>,
    perf_errno: Option<i32>,
) -> Manifest {
    Manifest {
        status,
        frequency_hz: arguments.frequency_hz,
        requested_duration_ms: arguments.duration_ms,
        observed_duration_ms: 0,
        samples_collected: 0,
        samples_lost: 0,
        stacks_written: 0,
        frames_total: 0,
        frames_unresolved: 0,
        stacks_truncated: 0,
        max_depth: arguments.max_depth,
        modules_seen: 0,
        cpus_sampled: 0,
        descendants_reaped: drained.descendants_reaped,
        namespace_drained: drained.namespace_drained,
        child_exit_code,
        child_signal,
        perf_errno,
    }
}

/// Creates and writes both artifacts. Neither may already exist: the helper is
/// the only writer of these two paths, so a file that is already there was put
/// there by the profiled program, and overwriting it would hide that. A refusal
/// here is an internal failure — the profiler was not unavailable, the output
/// was — so it keeps [`EXIT_INTERNAL_FAILURE`] and never becomes
/// `profiler_unavailable`.
fn emit(arguments: &Arguments, stacks: &str, manifest: &Manifest, code: i32) -> i32 {
    let stacks_written = create_new(&arguments.stacks_path, stacks.as_bytes());
    let manifest_written = create_new(
        &arguments.manifest_path,
        render_manifest(manifest).as_bytes(),
    );
    if stacks_written && manifest_written {
        code
    } else {
        report("internal failure: the profile outputs could not be written");
        EXIT_INTERNAL_FAILURE
    }
}

/// `O_WRONLY|O_CREAT|O_EXCL`: a pre-created path is `EEXIST`, never a silent
/// overwrite of somebody else's file.
fn create_new(path: &Path, bytes: &[u8]) -> bool {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .and_then(|mut file| file.write_all(bytes))
        .is_ok()
}

// -- child lifecycle ---------------------------------------------------------

/// Forks and execs the child on a helper thread with `SIGSTOP` raised before
/// `exec`. `Command::spawn` blocks on the exec status pipe until the child
/// execs, so it cannot run on the thread that has to attach the counter.
fn spawn_stopped_child(arguments: &Arguments) -> JoinHandle<io::Result<Child>> {
    let mut command = Command::new(&arguments.program);
    command.args(&arguments.program_args);
    // The child inherits the helper's already-reconstructed environment: the
    // helper adds nothing to it and removes nothing from it. Its stdio is
    // closed so the run's only outputs are the two files named on the argv.
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: the closure runs in the forked child between `fork` and `exec`,
    // where only async-signal-safe work is legal. `raise` is async-signal-safe;
    // the closure allocates nothing, takes no lock and touches no shared state.
    unsafe {
        command.pre_exec(|| {
            if libc::raise(libc::SIGSTOP) == 0 {
                Ok(())
            } else {
                Err(io::Error::last_os_error())
            }
        });
    }
    thread::spawn(move || command.spawn())
}

/// Waits for the child to report itself stopped. Returns `None` if the fork
/// itself failed, if the child died before stopping, or on timeout.
fn await_stopped_child(spawner: &JoinHandle<io::Result<Child>>) -> Option<libc::pid_t> {
    let deadline = Instant::now() + CHILD_START_TIMEOUT;
    loop {
        let mut status: libc::c_int = 0;
        // SAFETY: `status` is a live, writable `c_int`. The helper creates
        // exactly one child, so waiting on "any child" can only ever observe
        // that child; WNOHANG keeps the poll non-blocking.
        let waited = unsafe { libc::waitpid(-1, &mut status, libc::WUNTRACED | libc::WNOHANG) };
        if waited > 0 {
            return if libc::WIFSTOPPED(status) {
                Some(waited)
            } else {
                None
            };
        }
        if waited < 0 {
            let errno = last_errno();
            if errno != libc::ECHILD && errno != libc::EINTR {
                return None;
            }
        }
        if spawner.is_finished() {
            // The child can only reach `exec` after we continue it, so a
            // finished spawn before any stop means the fork failed.
            return None;
        }
        if Instant::now() >= deadline {
            return None;
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn try_reap(child: libc::pid_t) -> Option<(Option<i32>, Option<i32>)> {
    let mut status: libc::c_int = 0;
    // SAFETY: `status` is a live, writable `c_int` and `child` is this
    // process's own child; WNOHANG makes the call non-blocking.
    let waited = unsafe { libc::waitpid(child, &mut status, libc::WNOHANG) };
    (waited == child).then(|| decode_status(status))
}

fn reap(child: libc::pid_t) -> (Option<i32>, Option<i32>) {
    let mut status: libc::c_int = 0;
    // SAFETY: as above, blocking until the already-signalled child is reaped.
    let waited = unsafe { libc::waitpid(child, &mut status, 0) };
    if waited == child {
        decode_status(status)
    } else {
        (None, None)
    }
}

fn decode_status(status: libc::c_int) -> (Option<i32>, Option<i32>) {
    if libc::WIFEXITED(status) {
        (Some(libc::WEXITSTATUS(status)), None)
    } else if libc::WIFSIGNALED(status) {
        (None, Some(libc::WTERMSIG(status)))
    } else {
        (None, None)
    }
}

fn signal_child(child: libc::pid_t, signal: libc::c_int) {
    // SAFETY: `child` is this process's own child and `kill` takes only
    // scalars. A failure here is not actionable and is deliberately ignored.
    let _ = unsafe { libc::kill(child, signal) };
}

// -- PID namespace drain -----------------------------------------------------

/// One non-blocking pass of `waitpid(-1, …)`.
enum Waited {
    /// One process was reaped; there may be more.
    Reaped,
    /// Children remain but none has exited yet.
    Pending,
    /// `ECHILD`: nothing is left to wait for.
    Empty,
    /// `waitpid` failed for a reason this helper cannot act on.
    Failed,
}

fn self_pid() -> libc::pid_t {
    // SAFETY: `getpid` takes no argument, returns a scalar and cannot fail.
    unsafe { libc::getpid() }
}

/// SIGKILLs every process this namespace holds, the caller excepted.
///
/// Only ever called after [`drain_plan`] has answered
/// [`DrainPlan::WholeNamespace`], i.e. only when this process is pid 1 of its
/// own PID namespace and `-1` therefore names that namespace and nothing else.
fn kill_namespace() {
    // SAFETY: `kill` takes only scalars. The pid argument is `-1`, whose reach
    // is bounded by the caller's PID namespace and by its uid; the call site
    // has already established that this process is that namespace's init.
    let _ = unsafe { libc::kill(-1, libc::SIGKILL) };
}

fn wait_any() -> Waited {
    let mut status: libc::c_int = 0;
    // SAFETY: `status` is a live, writable `c_int`; `-1` asks about any child
    // of this process and WNOHANG keeps the call non-blocking.
    let waited = unsafe { libc::waitpid(-1, &mut status, libc::WNOHANG) };
    if waited > 0 {
        return Waited::Reaped;
    }
    if waited == 0 {
        return Waited::Pending;
    }
    match last_errno() {
        libc::ECHILD => Waited::Empty,
        libc::EINTR => Waited::Pending,
        _ => Waited::Failed,
    }
}

/// Empties the process tree before any artifact is rendered or written, and
/// reports honestly whether it managed to.
///
/// The gateway runs the sampling container with `--init=false` and this helper
/// as its entrypoint, so the helper is pid 1 of a PID namespace that holds the
/// profiled program and nothing else, and `kill(-1, SIGKILL)` reaches every
/// descendant however many times the child forked. That assumption is checked
/// rather than trusted: off pid 1, `-1` would reach processes this run never
/// created, so only `child` is signalled and the namespace is reported as not
/// drained.
///
/// Reaping is bounded by [`NAMESPACE_DRAIN_TIMEOUT`]. A deadline that expires
/// with children still alive yields `namespace_drained: false`, which is the
/// caller's signal that the artifacts were written while something could still
/// have rewritten them.
fn drain_namespace(child: Option<libc::pid_t>) -> NamespaceDrain {
    let plan = drain_plan(self_pid());
    match plan {
        DrainPlan::WholeNamespace => kill_namespace(),
        DrainPlan::DirectChildOnly => {
            if let Some(child) = child {
                signal_child(child, libc::SIGKILL);
            }
        }
    }
    let deadline = Instant::now() + NAMESPACE_DRAIN_TIMEOUT;
    let mut descendants_reaped: u64 = 0;
    loop {
        match wait_any() {
            Waited::Reaped => descendants_reaped = descendants_reaped.saturating_add(1),
            Waited::Empty => {
                return NamespaceDrain {
                    descendants_reaped,
                    // Nothing is left to reap, but only the wide plan can claim
                    // the namespace is empty: the narrow one never signalled
                    // anything but the direct child.
                    namespace_drained: plan == DrainPlan::WholeNamespace,
                };
            }
            Waited::Pending => {
                if Instant::now() >= deadline {
                    return NamespaceDrain {
                        descendants_reaped,
                        namespace_drained: false,
                    };
                }
                thread::sleep(POLL_INTERVAL);
                // Signal again: a process forked between the previous sweep and
                // its delivery was never in that sweep's process list. Repeating
                // it converges against a child that forks while dying, and the
                // container's own pid limit bounds how long that can go on.
                if plan == DrainPlan::WholeNamespace {
                    kill_namespace();
                }
            }
            Waited::Failed => {
                return NamespaceDrain {
                    descendants_reaped,
                    namespace_drained: false,
                };
            }
        }
    }
}

// -- perf event --------------------------------------------------------------

fn build_attr(frequency_hz: u32) -> PerfEventAttr {
    PerfEventAttr {
        kind: PERF_TYPE_SOFTWARE,
        size: PERF_ATTR_SIZE,
        config: PERF_COUNT_SW_CPU_CLOCK,
        sample_freq: u64::from(frequency_hz),
        sample_type: SAMPLE_TYPE,
        read_format: 0,
        flags: ATTR_DISABLED
            | ATTR_INHERIT
            | ATTR_EXCLUDE_KERNEL
            | ATTR_EXCLUDE_HV
            | ATTR_MMAP
            | ATTR_COMM
            | ATTR_FREQ,
        wakeup_events: 0,
        bp_type: 0,
        config1: 0,
        config2: 0,
        branch_sample_type: 0,
        sample_regs_user: 0,
        sample_stack_user: 0,
        clockid: 0,
        sample_regs_intr: 0,
        aux_watermark: 0,
        sample_max_stack: 0,
        reserved_2: 0,
        aux_sample_size: 0,
        reserved_3: 0,
        sig_data: 0,
    }
}

/// One armed sampling event and the ring buffer it publishes into. Dropping it
/// releases both, so every early return cleans up on its own.
struct PerfEvent {
    fd: libc::c_int,
    ring: Ring,
}

impl Drop for PerfEvent {
    fn drop(&mut self) {
        unmap_ring(&self.ring);
        close_fd(self.fd);
    }
}

fn online_cpu_count() -> usize {
    // SAFETY: `sysconf` takes a name and returns a long; no pointers involved.
    let raw = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_ONLN) };
    clamp_cpu_count(raw)
}

/// Opens one inherited event, with its own ring buffer, per online CPU.
///
/// `inherit = 1` is what makes the profile follow the child's threads, but
/// `perf_mmap` refuses an inherited event opened with `cpu == -1`: an inherited
/// per-task event has no single ring buffer to map. The counter opens fine and
/// only the `mmap` fails, with EINVAL. So the event is opened once per CPU with
/// an explicit `cpu` index, each with its own ring, and the rings are merged on
/// the way out.
///
/// A CPU whose event cannot be opened or mapped is skipped. As long as one
/// event survives, the profile continues on the CPUs that worked and reports
/// how many in `cpus_sampled`; only a total failure is `profiler_unavailable`.
fn open_events(frequency_hz: u32, child: libc::pid_t) -> Result<Vec<PerfEvent>, i32> {
    let cpus = online_cpu_count();
    let mut events = Vec::with_capacity(cpus);
    let mut last_errno = libc::ENODEV;
    for cpu in 0..cpus {
        let cpu = libc::c_long::try_from(cpu).unwrap_or(libc::c_long::MAX);
        match open_event(frequency_hz, child, cpu) {
            Ok(event) => events.push(event),
            Err(errno) => last_errno = errno,
        }
    }
    if events.is_empty() {
        return Err(last_errno);
    }
    Ok(events)
}

/// `perf_event_open(&attr, child, cpu, -1, 0)`. Only the helper's own child is
/// ever named here; no other pid and no cgroup is reachable from this call.
fn open_event(frequency_hz: u32, child: libc::pid_t, cpu: libc::c_long) -> Result<PerfEvent, i32> {
    let attr = build_attr(frequency_hz);
    // SAFETY: `attr` is a fully initialised `PerfEventAttr` whose `size` field
    // equals its real byte count, so the kernel copies exactly the bytes that
    // exist; it stays alive for the whole call and the kernel does not retain
    // the pointer.
    let result = unsafe {
        libc::syscall(
            libc::SYS_perf_event_open,
            ptr::from_ref(&attr).cast::<libc::c_void>(),
            libc::c_long::from(child),
            cpu,
            NO_GROUP,
            NO_FLAGS,
        )
    };
    if result < 0 {
        return Err(last_errno());
    }
    let Ok(fd) = libc::c_int::try_from(result) else {
        return Err(libc::EINVAL);
    };
    match map_ring(fd) {
        Ok(ring) => Ok(PerfEvent { fd, ring }),
        Err(errno) => {
            close_fd(fd);
            Err(errno)
        }
    }
}

fn perf_ioctl(fd: libc::c_int, request: libc::c_long) -> Result<(), i32> {
    // SAFETY: `fd` is a live perf event descriptor; both
    // PERF_EVENT_IOC_ENABLE and PERF_EVENT_IOC_DISABLE take no argument, so
    // the third word is an ignored zero rather than a pointer.
    let result =
        unsafe { libc::syscall(libc::SYS_ioctl, libc::c_long::from(fd), request, NO_FLAGS) };
    if result < 0 {
        Err(last_errno())
    } else {
        Ok(())
    }
}

fn close_fd(fd: libc::c_int) {
    // SAFETY: `fd` is owned by this process and is never used again.
    let _ = unsafe { libc::close(fd) };
}

// -- ring buffer -------------------------------------------------------------

struct Ring {
    base: *mut libc::c_void,
    total: usize,
    data_start: usize,
    data_size: usize,
}

fn page_size() -> usize {
    // SAFETY: `sysconf` takes a name and returns a long; no pointers involved.
    let raw = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    usize::try_from(raw)
        .ok()
        .filter(|value| value.is_power_of_two())
        .unwrap_or(4096)
}

fn map_ring(fd: libc::c_int) -> Result<Ring, i32> {
    let page = page_size();
    let total = page.checked_mul(1 + DATA_PAGE_COUNT).ok_or(libc::EINVAL)?;
    // SAFETY: a fresh shared mapping of the perf descriptor at offset zero. The
    // kernel validates the length against the event's ring buffer geometry and
    // rejects anything it cannot serve, so a success means `total` bytes are
    // mapped and readable.
    let base = unsafe {
        libc::mmap(
            ptr::null_mut(),
            total,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            fd,
            0,
        )
    };
    if base == libc::MAP_FAILED {
        return Err(last_errno());
    }

    let mut ring = Ring {
        base,
        total,
        data_start: page,
        data_size: total - page,
    };
    // Prefer the geometry the kernel published, when it is self-consistent.
    let published_start =
        usize::try_from(metadata(&ring, RING_DATA_OFFSET).load(Ordering::Relaxed)).unwrap_or(0);
    let published_size =
        usize::try_from(metadata(&ring, RING_DATA_SIZE).load(Ordering::Relaxed)).unwrap_or(0);
    if published_start >= page
        && published_size.is_power_of_two()
        && published_start
            .checked_add(published_size)
            .is_some_and(|end| end <= total)
    {
        ring.data_start = published_start;
        ring.data_size = published_size;
    }
    Ok(ring)
}

fn unmap_ring(ring: &Ring) {
    // SAFETY: `base` and `total` come from the matching `mmap` call and the
    // mapping is never touched again.
    let _ = unsafe { libc::munmap(ring.base, ring.total) };
}

/// Borrows one of the 64-bit control words in the metadata page.
fn metadata(ring: &Ring, offset: usize) -> &AtomicU64 {
    // SAFETY: `offset` is a fixed, eight-byte-aligned offset inside the first
    // page of a page-aligned mapping that is at least one page long, and the
    // kernel updates these words atomically. The borrow cannot outlive `ring`,
    // which is unmapped only after every borrow has ended.
    unsafe { &*ring.base.cast::<u8>().add(offset).cast::<AtomicU64>() }
}

/// Consumes one pass over one ring and republishes its `data_tail`. Returns an
/// empty vector when the window holds nothing but a partially written record,
/// which is the signal that this ring has made no progress.
fn drain_once(ring: &Ring) -> Vec<Record> {
    let head = metadata(ring, RING_DATA_HEAD).load(Ordering::Acquire);
    let tail = metadata(ring, RING_DATA_TAIL).load(Ordering::Relaxed);
    if head <= tail {
        return Vec::new();
    }
    // SAFETY: the data area is `data_size` bytes at `data_start` inside a
    // mapping of `total` bytes that outlives this borrow. The kernel only
    // appends at `data_head` and never rewrites the window below it that we
    // have not yet released by publishing `data_tail`, so the bytes the
    // decoder reads are stable for the duration of the borrow.
    let data = unsafe {
        std::slice::from_raw_parts(ring.base.cast::<u8>().add(ring.data_start), ring.data_size)
    };
    let drained = drain_ring(data, tail, head);
    if drained.tail != tail {
        metadata(ring, RING_DATA_TAIL).store(drained.tail, Ordering::Release);
    }
    drained.records
}

/// Drains every per-CPU ring and feeds the merged record stream to one
/// collector, so the counters and the folded stacks are totals over all CPUs.
fn drain_all(events: &[PerfEvent], collector: &mut Collector) {
    for _ in 0..MAX_DRAIN_ROUNDS {
        let mut per_ring = Vec::with_capacity(events.len());
        let mut drained_any = false;
        for event in events {
            let records = drain_once(&event.ring);
            drained_any |= !records.is_empty();
            per_ring.push(records);
        }
        if !drained_any {
            return;
        }
        for record in merge_ring_records(per_ring) {
            collector.consume(record);
        }
    }
}

// -- sample collection -------------------------------------------------------

/// The only part of collection that needs the filesystem: a cache of parsed
/// ELF symbol tables, wrapped around the syscall-free [`StackCollector`] that
/// does the module bookkeeping, folding and counting.
struct Collector {
    inner: StackCollector,
    tables: BTreeMap<String, Option<SymbolTable>>,
}

impl Collector {
    fn new(arguments: &Arguments) -> Self {
        Self {
            inner: StackCollector::new(
                usize::try_from(arguments.max_depth).unwrap_or(1),
                arguments.max_samples,
            ),
            tables: BTreeMap::new(),
        }
    }

    fn consume(&mut self, record: Record) {
        let Self { inner, tables } = self;
        inner.consume(record, |path, file_offset| {
            if !tables.contains_key(path) && tables.len() >= MAX_CACHED_MODULES {
                return None;
            }
            tables
                .entry(path.to_owned())
                .or_insert_with(|| load_symbol_table(path))
                .as_ref()
                .and_then(|table| table.resolve(file_offset))
                .map(|symbol| symbol.to_vec())
        });
    }

    fn tally(&self) -> SampleTally {
        self.inner.tally()
    }

    fn modules_seen(&self) -> u64 {
        self.inner.modules_seen()
    }

    fn stacks_written(&self) -> u64 {
        self.inner.stacks_written()
    }

    fn render(&self) -> String {
        self.inner.folded().render()
    }
}

/// Reads and parses one module's ELF symbol table. Only regular files below a
/// fixed size are opened; anything unreadable or unparseable yields `None` and
/// its frames become `[unknown]`.
fn load_symbol_table(path: &str) -> Option<SymbolTable> {
    let metadata = fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_MODULE_BYTES {
        return None;
    }
    parse_elf_symbols(&fs::read(path).ok()?).ok()
}

fn last_errno() -> i32 {
    io::Error::last_os_error().raw_os_error().unwrap_or(0)
}
