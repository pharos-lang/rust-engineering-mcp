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
//! 3. `perf_event_open` on that pid only;
//! 4. `mmap` one metadata page plus eight data pages;
//! 5. `PERF_EVENT_IOC_ENABLE`, then `SIGCONT`;
//! 6. drain the ring until the child exits, the duration elapses or the sample
//!    cap is reached;
//! 7. `PERF_EVENT_IOC_DISABLE`, kill and reap, drain what is left.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use rust_mcp_profile_helper::{
    Arguments, EXIT_INTERNAL_FAILURE, EXIT_PROFILER_UNAVAILABLE, EXIT_SUCCESS, FoldedStacks,
    Manifest, ModuleMap, ProfileStatus, Record, RunOutcome, SampleRecord, SymbolTable,
    UNKNOWN_FRAME, classify_status, drain_ring, parse_elf_symbols, render_manifest, resolve_frame,
    user_frames,
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

const ANY_CPU: libc::c_long = -1;
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
        let manifest = blank_manifest(arguments, ProfileStatus::ChildExited, None, None, None);
        return emit(arguments, "", &manifest, EXIT_INTERNAL_FAILURE);
    };

    let fd = match open_perf_event(arguments.frequency_hz, child) {
        Ok(fd) => fd,
        Err(errno) => return abandon(arguments, spawner, child, errno),
    };
    let ring = match map_ring(fd) {
        Ok(ring) => ring,
        Err(errno) => {
            close_fd(fd);
            return abandon(arguments, spawner, child, errno);
        }
    };
    if let Err(errno) = perf_ioctl(fd, PERF_EVENT_IOC_ENABLE) {
        unmap_ring(&ring);
        close_fd(fd);
        return abandon(arguments, spawner, child, errno);
    }

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
            drain(&ring, &mut collector);
            if collector.samples >= arguments.max_samples {
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

    let _ = perf_ioctl(fd, PERF_EVENT_IOC_DISABLE);
    if !reaped {
        signal_child(child, libc::SIGKILL);
        let (code, signal) = reap(child);
        child_exit_code = code;
        child_signal = signal;
    }
    drain(&ring, &mut collector);
    let observed = started.elapsed();
    unmap_ring(&ring);
    close_fd(fd);

    outcome.samples_collected = collector.samples;
    let manifest = Manifest {
        status: classify_status(&outcome),
        frequency_hz: arguments.frequency_hz,
        requested_duration_ms: arguments.duration_ms,
        observed_duration_ms: u64::try_from(observed.as_millis()).unwrap_or(u64::MAX),
        samples_collected: collector.samples,
        samples_lost: collector.lost,
        stacks_written: u64::try_from(collector.folded.len()).unwrap_or(u64::MAX),
        frames_total: collector.frames_total,
        frames_unresolved: collector.frames_unresolved,
        stacks_truncated: collector.stacks_truncated,
        max_depth: arguments.max_depth,
        modules_seen: collector.map.modules_seen(),
        child_exit_code,
        child_signal,
        perf_errno: None,
    };
    emit(
        arguments,
        &collector.folded.render(),
        &manifest,
        EXIT_SUCCESS,
    )
}

/// The `perf_event_open` (or ring buffer) refusal path: kill the stopped child,
/// reap it, write an empty stacks file and a manifest carrying the errno.
fn abandon(
    arguments: &Arguments,
    spawner: JoinHandle<io::Result<Child>>,
    child: libc::pid_t,
    errno: i32,
) -> i32 {
    signal_child(child, libc::SIGKILL);
    let spawned = matches!(spawner.join(), Ok(Ok(_)));
    let (code, signal) = if spawned { reap(child) } else { (None, None) };
    report("profiler unavailable: perf_event_open was refused");
    let manifest = blank_manifest(
        arguments,
        ProfileStatus::ProfilerUnavailable,
        code,
        signal,
        Some(errno),
    );
    emit(arguments, "", &manifest, EXIT_PROFILER_UNAVAILABLE)
}

fn blank_manifest(
    arguments: &Arguments,
    status: ProfileStatus,
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
        child_exit_code,
        child_signal,
        perf_errno,
    }
}

fn emit(arguments: &Arguments, stacks: &str, manifest: &Manifest, code: i32) -> i32 {
    let stacks_written = fs::write(&arguments.stacks_path, stacks.as_bytes()).is_ok();
    let manifest_written = fs::write(
        &arguments.manifest_path,
        render_manifest(manifest).as_bytes(),
    )
    .is_ok();
    if stacks_written && manifest_written {
        code
    } else {
        report("internal failure: the profile outputs could not be written");
        EXIT_INTERNAL_FAILURE
    }
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

/// `perf_event_open(&attr, child, -1, -1, 0)`. Only the helper's own child is
/// ever named here; no other pid and no cgroup is reachable from this call.
fn open_perf_event(frequency_hz: u32, child: libc::pid_t) -> Result<libc::c_int, i32> {
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
            ANY_CPU,
            NO_GROUP,
            NO_FLAGS,
        )
    };
    if result < 0 {
        return Err(last_errno());
    }
    libc::c_int::try_from(result).map_err(|_| libc::EINVAL)
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

/// Consumes everything the kernel has published, then republishes `data_tail`.
fn drain(ring: &Ring, collector: &mut Collector) {
    loop {
        let head = metadata(ring, RING_DATA_HEAD).load(Ordering::Acquire);
        let tail = metadata(ring, RING_DATA_TAIL).load(Ordering::Relaxed);
        if head <= tail {
            return;
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
        let progressed = drained.tail != tail;
        for record in drained.records {
            collector.consume(record);
        }
        metadata(ring, RING_DATA_TAIL).store(drained.tail, Ordering::Release);
        if !progressed || drained.tail >= head {
            return;
        }
    }
}

// -- sample collection -------------------------------------------------------

struct Collector {
    map: ModuleMap,
    folded: FoldedStacks,
    tables: BTreeMap<String, Option<SymbolTable>>,
    samples: u64,
    lost: u64,
    frames_total: u64,
    frames_unresolved: u64,
    stacks_truncated: u64,
    max_depth: usize,
    max_samples: u64,
}

impl Collector {
    fn new(arguments: &Arguments) -> Self {
        Self {
            map: ModuleMap::new(),
            folded: FoldedStacks::new(),
            tables: BTreeMap::new(),
            samples: 0,
            lost: 0,
            frames_total: 0,
            frames_unresolved: 0,
            stacks_truncated: 0,
            max_depth: usize::try_from(arguments.max_depth).unwrap_or(1),
            max_samples: arguments.max_samples,
        }
    }

    fn consume(&mut self, record: Record) {
        match record {
            Record::Mapping(mapping) => self.map.insert(mapping),
            Record::Lost(lost) => self.lost = self.lost.saturating_add(lost),
            Record::Sample(sample) => self.consume_sample(&sample),
            Record::Ignored(_) => {}
        }
    }

    fn consume_sample(&mut self, sample: &SampleRecord) {
        if self.samples >= self.max_samples {
            return;
        }
        let stack = user_frames(sample, self.max_depth);
        let mut names = Vec::with_capacity(stack.frames.len());
        let mut unresolved: u64 = 0;
        {
            let map = &self.map;
            let tables = &mut self.tables;
            for &address in &stack.frames {
                let name = resolve_frame(map, address, |path, file_offset| {
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
                if name == UNKNOWN_FRAME {
                    unresolved = unresolved.saturating_add(1);
                }
                names.push(name);
            }
        }
        self.samples = self.samples.saturating_add(1);
        if stack.truncated {
            self.stacks_truncated = self.stacks_truncated.saturating_add(1);
        }
        self.frames_total = self
            .frames_total
            .saturating_add(u64::try_from(names.len()).unwrap_or(0));
        self.frames_unresolved = self.frames_unresolved.saturating_add(unresolved);
        self.folded.record(&names, 1);
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
