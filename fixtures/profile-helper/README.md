# Isolated sampling profile helper

This private fixture is the sampling profiler behind the `rust.profile.flamegraph`
MCP tool. It is not an MCP tool itself and is not installed by the repository
build. Provisioning builds it offline, inventories it, and places the exact
binary in the hardened guest image as `rust-mcp-profile-helper`.

The helper has one job: launch one program, sample **that child and nothing
else** with `perf_event_open`, and write two files. It has exactly one external
dependency, `libc = "=0.2.189"`, and it has its own workspace and lockfile. The
JSON manifest is serialized by hand.

The target is `aarch64-unknown-linux-gnu`. The crate compiles everywhere so the
pure logic can be tested on the macOS ARM64 development hosts, but on any other
target the binary prints a single `unsupported target` line and exits 4 rather
than pretending to profile.

## Closed invocation protocol

The gateway builds this argv and no other. Every flag is required, appears at
most once, and takes its value as a separate argv element; `--flag=value` is not
part of the grammar and is rejected as an unknown argument.

```text
rust-mcp-profile-helper \
  --frequency-hz <1..=999> \
  --duration-ms <100..=60000> \
  --max-samples <1..=2000000> \
  --max-depth <1..=256> \
  --stacks <path> \
  --manifest <path> \
  -- <absolute-program> [args...]
```

Numeric values must be plain decimal digits: no sign, no radix prefix, no
whitespace. The program after `--` must be an absolute path. Everything after
the program is passed to it verbatim and is never interpreted by the helper.

Any deviation — an unknown flag, a missing flag, a repeated flag, a flag without
a value, a non-numeric or out-of-range value, a missing or empty `--` section, a
relative program path — exits 2 with one fixed line on stderr. The messages are
a closed set and never echo caller-supplied bytes.

## What the helper does

1. Spawns the child with `std::process::Command`, using `pre_exec` to
   `raise(SIGSTOP)` before `exec` so the counter is attached before any project
   code runs. The child inherits the helper's already-reconstructed environment
   unchanged: nothing is added and nothing is removed. Its three standard
   streams are `/dev/null`, so the run's only outputs are the two named files.
2. `waitpid(..., WUNTRACED)` until the child reports itself stopped.
3. `perf_event_open` on that pid, once per online CPU (see below).
4. `mmap` one metadata page plus eight data pages per event,
   `PROT_READ|PROT_WRITE`, `MAP_SHARED`. The guest's page size is 4096, so each
   ring is 36 KiB.
5. `ioctl(PERF_EVENT_IOC_ENABLE)` on every event, then `kill(child, SIGCONT)`.
6. Drains every ring, sleeping in 1 ms increments, until whichever comes first:
   the child exits, `--duration-ms` elapses on the monotonic clock, or
   `--max-samples` is reached.
7. `ioctl(PERF_EVENT_IOC_DISABLE)` on every event, `SIGKILL` and reap the child
   if it is still alive, then drain what is left in the rings.

`Command::spawn` blocks reading the exec status pipe until the child execs, and
the child stops before `exec`, so the spawn runs on a helper thread while the
main thread attaches the counter and continues it. This is not an optimisation:
doing both on one thread deadlocks. The behaviour is confirmed on both the
success path and the `SIGKILL`-before-`exec` path.

## `perf_event_attr`

| field | value | why |
| --- | --- | --- |
| `type` | `PERF_TYPE_SOFTWARE` (1) | no PMU counter is needed or requested |
| `config` | `PERF_COUNT_SW_CPU_CLOCK` (0) | wall-clock CPU sampling |
| `size` | 128 (`PERF_ATTR_SIZE_VER7`) | matches the struct byte-for-byte; asserted at compile time |
| `sample_freq` | `--frequency-hz` | with `freq = 1`, this is a target rate, not a period |
| `sample_type` | `0x27` | `IP\|TID\|TIME\|CALLCHAIN` |
| `disabled` | 1 | the counter is armed only after the ring is mapped |
| `exclude_kernel` | 1 | no kernel addresses are ever sampled |
| `exclude_hv` | 1 | no hypervisor addresses are ever sampled |
| `inherit` | 1 | the child's own threads and children are covered |
| `mmap` | 1 | `PERF_RECORD_MMAP` supplies the module bases |
| `comm` | 1 | task naming records |
| `freq` | 1 | `sample_freq` is a frequency |
| `pid` / `cpu` / `group_fd` / `flags` | child / 0..n / -1 / 0 | one process, one event per CPU, no group |

`read_format`, `wakeup_events`, `precise_ip`, `mmap2`, `sample_id_all`,
`build_id`, `clockid` and every other field are zero. The decoder also
understands `PERF_RECORD_MMAP2` so a future attr change does not silently lose
module bases, but the build-id body variant is refused rather than misparsed.

## Why one event per CPU

`inherit = 1` is what makes the profile follow the child's own threads and
descendants, and it is not negotiable for a useful profile. But an inherited
event cannot be opened with `cpu == -1`: `perf_event_open` accepts it and
returns a descriptor, and then `perf_mmap` refuses it with **EINVAL**, because
an inherited per-task event has no single ring buffer to map. The failure shows
up at the `mmap`, not at the open, which makes it easy to misread as a sandbox
denial. It is not one — it is the kernel's data model.

So the helper opens the same attribute once per online CPU,
`perf_event_open(&attr, child_pid, cpu, -1, 0)` for `cpu` in
`0..sysconf(_SC_NPROCESSORS_ONLN)`, each with its own ring, and merges the rings
on the way out. There is no group leader, so each event is enabled and disabled
on its own descriptor.

The CPU count is clamped: a failed or nonsensical `sysconf` degrades to a single
CPU, and an implausibly large answer is capped at 256. A CPU whose event cannot
be opened, mapped or armed is skipped; as long as one event survives, the
profile continues on the CPUs that worked and `cpus_sampled` records how many.
Only a total failure is `profiler_unavailable`.

Merging is deterministic. Each pass drains every ring, then applies all mapping
records before any sample, in ascending CPU order and preserving each ring's own
order within each class. The mapping-first rule matters: a module is announced
on the ring of whichever CPU the task happened to be running on when it mapped,
while a sample that needs it can land on a different CPU's ring, and without the
hoist that frame would resolve to `[unknown]`. Counters are totals across all
rings, `samples_lost` included.

## Why no capability or sysctl change is needed

`kernel.perf_event_paranoid = 2` denies kernel and CPU-wide profiling but still
permits **user-space** measurement of a process the caller already owns. The
helper only ever asks for that: `exclude_kernel = 1`, `exclude_hv = 1`,
`cpu = -1`, and `pid` set to a child it forked itself. Nothing here requires
`CAP_PERFMON`, `CAP_SYS_ADMIN` or a paranoia relaxation.

The helper therefore runs as uid 65534 with `--cap-drop=ALL` and
`no-new-privileges`, with `kernel.perf_event_paranoid` left at 2. It opens no
sockets, spawns no shell, writes nothing under `/proc`, reads no sysctl, calls
no `ptrace`, and never names a pid other than the one it created. If
`perf_event_open` is refused anyway, that is reported as data (see
`profiler_unavailable` below) rather than worked around.

## Outputs

### `--stacks`: collapsed/folded stacks

One LF-terminated line per distinct stack, root first, UTF-8:

```text
main;compute;inner_loop 4217
main;setup 12
```

perf reports callchains leaf first, so each stack is reversed. `PERF_CONTEXT_*`
markers are stripped and only frames recorded in user space are kept. Stacks are
truncated to `--max-depth` and the truncation is counted. Lines are sorted
byte-lexicographically and identical stacks are merged, so the file is a pure
function of the samples: rendering it twice from the same input gives the same
bytes.

Frames carry the symbol name only. **No module path is ever emitted**, and every
frame name is sanitized: each byte outside `[0-9A-Za-z_.:$<>,*&\[\]+-]` becomes
exactly one `_`, and the result is capped at 200 bytes. `;`, LF and `/` are all
outside that alphabet, so neither the folded-stack grammar nor a filesystem path
can survive sanitization. The substitution is one for one and nothing is
collapsed, so an authentic symbol name is reproduced verbatim:
`__libc_start_main` renders as `__libc_start_main`.

A frame that cannot be attributed becomes `[unknown]` and increments
`frames_unresolved`. That happens when the address is outside every executable
mapping, when the mapping has no absolute-path backing file (anonymous regions
and `[vdso]`-style names are rejected outright), when the module cannot be read
or parsed as ELF64 LSB, or when no sized `STT_FUNC` symbol covers the address.

Symbolization reads the mapping's own file: `file_offset = addr - map.addr +
map.pgoff`, then `vaddr = file_offset - p_offset + p_vaddr` for the `PT_LOAD`
segment containing that offset, then the `STT_FUNC` symbol whose
`[value, value + size)` contains the virtual address. `.symtab` is preferred and
`.dynsym` is the fallback. Parsed tables are cached by resolved path. Every
offset, count and entry size in the image is bounds-checked before use.

### `--manifest`: JSON

Exactly these keys, always all present, in this order, followed by one LF:

```json
{
  "schema": "rust-engineering-mcp.profile-helper.v1",
  "status": "complete",
  "frequency_hz": 99,
  "requested_duration_ms": 5000,
  "observed_duration_ms": 1234,
  "samples_collected": 421,
  "samples_lost": 2,
  "stacks_written": 37,
  "frames_total": 900,
  "frames_unresolved": 11,
  "stacks_truncated": 3,
  "max_depth": 128,
  "modules_seen": 5,
  "cpus_sampled": 4,
  "child_exit_code": 0,
  "child_signal": null,
  "perf_errno": null
}
```

`child_exit_code`, `child_signal` and `perf_errno` are numbers or `null`; every
other value is a plain JSON number or one of the fixed status spellings. The
serializer is hand-rolled and emits nothing it did not compute.

`cpus_sampled` is the number of per-CPU events that opened, mapped **and**
armed. It equals the online CPU count on a healthy run, is lower when some CPUs
were refused, and is `0` on the `profiler_unavailable` path. A value below the
machine's CPU count means the profile is real but covers only part of the
machine, so compare it against `nproc` before treating sample counts as
absolute.

The five statuses are closed and mutually exclusive:

- `complete` — the child ran to completion inside the window and was sampled;
- `sample_limit` — `--max-samples` was reached and the child was killed;
- `duration_limit` — `--duration-ms` elapsed and the child was killed;
- `child_exited` — the child was gone before any sample could be taken, or its
  `exec` failed;
- `profiler_unavailable` — no CPU's event survived: `perf_event_open`, the ring
  `mmap`, or the enabling `ioctl` was refused for every one of them.
  `perf_errno` carries the last errno and `cpus_sampled` is `0`.

`samples_lost` is the sum of the kernel's own `PERF_RECORD_LOST` counts across
every ring, so a profile that outran a ring on any CPU says so instead of
quietly under-reporting.

## Exit codes

- `0` — profiled. **The child's own failure is data, not an error**: a child
  that exited non-zero or died on a signal still exits 0 here and reports its
  status in the manifest.
- `2` — invalid arguments. Neither output file is written.
- `3` — profiler unavailable: no CPU's event could be opened, mapped and armed.
  An empty stacks file and a manifest with `"status":"profiler_unavailable"`,
  the errno and `"cpus_sampled":0` are still written. A run where only *some*
  CPUs were refused is not this case: it exits 0 with a lower `cpus_sampled`.
- `4` — internal or I/O failure: the child process could not be created at all
  (the `fork` failed, or it never reported itself stopped), an output file could
  not be written, or the binary is running off-target. A child that was created
  but whose `exec` failed is not this case: that is a child failure, so it exits
  0 with `"status":"child_exited"` and a null `child_exit_code`.

The helper never panics on a normal path; a panic hook and `catch_unwind` map
any residual fault to exit 4 with one fixed line.

## Threat model

The helper is a profiler running inside an untrusted-workload sandbox, so the
adversary is the profiled program itself and, secondarily, the module files it
maps.

- **It profiles only its own child.** The pid handed to `perf_event_open` comes
  from a `fork` the helper performed; there is no flag, environment variable or
  file that can name another process. Every per-CPU event names that same pid —
  the fan-out is over CPUs, never over processes — and `inherit = 1` extends
  coverage to the child's own threads and descendants and to nothing else.
- **It never reads other processes.** No `ptrace`, no `/proc/<pid>` of any other
  task, no cgroup events, no kernel or hypervisor samples. The per-CPU events
  are still per-task: each is opened with the child's pid, so it observes that
  task's time on that CPU and never another tenant's.
- **It emits no paths.** Module paths are used to open ELF files and are then
  discarded. The stacks file contains symbol names in a closed alphabet; the
  manifest contains only numbers and fixed strings. Neither output can carry a
  path, a `;`, an LF or arbitrary caller bytes, so neither can forge folded-stack
  structure or smuggle text into a downstream renderer.
- **It trusts no bytes it did not write.** Ring-buffer records are decoded with
  bounds checks on every field, records that wrap the ring end are copied into a
  contiguous scratch buffer, and a partially written trailing record is left
  unconsumed rather than guessed at. ELF images are validated for class,
  endianness, table offsets, entry sizes and counts before any symbol is read;
  a malformed module yields `[unknown]` frames, never a fault.
- **It is bounded.** At most 256 per-CPU events, 16384 executable mappings,
  16384 records decoded per ring pass, 250000 distinct folded stacks, 512 cached
  symbol tables and 512 MiB per module image are retained, and each frame name is
  capped at 200 bytes. A hostile child cannot make the helper grow without
  limit.
- **`unsafe` is confined and annotated.** Every `unsafe` block is in
  `src/linux.rs`, is a direct syscall or a fixed-offset read of the mapped
  metadata page, and carries a comment stating its invariant. `src/lib.rs` — the
  entire decoder, parser, sanitizer, aggregator and serializer — contains no
  `unsafe` at all.

Two things this does **not** claim. The effective callchain depth is also capped
by `kernel.perf_event_max_stack` (127 by default), so `--max-depth` above that
is an upper bound the kernel may not reach. And frame-pointer-based callchains
are only as complete as the profiled binary's frame pointers; a stack that the
kernel could not unwind is short, not wrong.

## Calibration

The guest this is built for reports `getconf PAGESIZE` = 4096 and kernel
`7.0.12-linuxkit aarch64`, under `perf_event_paranoid = 2`, uid 65534,
`--cap-drop=ALL`, `no-new-privileges`, `--network=none` and
`seccomp-rust-profile.json`. Each ring is therefore 9 x 4096 = 36 KiB, and the
`_SC_PAGESIZE` lookup exists so a different page size stays correct rather than
silently mismapping.

## Local verification

The helper has its own workspace and lockfile, so its checks run inside its own
directory and never at the repository root:

```text
cargo fmt --check
cargo clippy --locked --offline --all-targets -- -D warnings
cargo test --locked --offline
cargo clippy --locked --offline --target aarch64-unknown-linux-gnu --all-targets -- -D warnings
```

The first three run on the macOS ARM64 hosts, which is why all the pure logic is
target-independent: argument parsing, ring-buffer decoding, ELF symbol lookup,
frame sanitization, folded-stack aggregation and manifest serialization are all
unit tested there against synthetic byte buffers, including a synthetic ELF64
image built in the test. The fourth check type-checks the syscall path for the
real target; running it requires
`rustup target add aarch64-unknown-linux-gnu`.
