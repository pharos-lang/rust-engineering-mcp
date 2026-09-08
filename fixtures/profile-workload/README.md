# M5 profiling workload fixture

`rust-mcp-profile-workload` is the sampling-profiler oracle for M5. It is an
isolated workspace (`[workspace]` in its own `Cargo.toml`), has no dependencies,
and commits its `Cargo.lock`. It is a fixture input, never a workspace
dependency and never distributed.

## The known stack

The binary spends the overwhelming majority of its wall time inside one
`#[inline(never)] pub fn`, reached through a fixed three-deep chain:

```text
main;level_one;level_two;known_hot_frame
```

`main` parses argv (once, microseconds), calls `level_one`, and prints the
result (once, microseconds). `level_one` calls `level_two`, `level_two` calls
`known_hot_frame`, and `known_hot_frame` busy-loops. Every one of the three
functions is `#[inline(never)]`, and in `level_one` and `level_two` the inner
call is deliberately **not in tail position** — each one uses the returned value
afterwards — so neither frame can be collapsed by a sibling-call optimization.

A profiler will show runtime start-up frames *below* `main`
(`lang_start_internal` and friends) and possibly a `clock_gettime`/`mach_
absolute_time` leaf above `known_hot_frame` on some samples. The oracle is the
**suffix** `main;level_one;level_two;known_hot_frame`, which must hold for the
overwhelming majority of samples; it is not a claim that the sampled stack has
exactly four entries.

Verified on the authoring host (macOS/arm64, Rust 1.98.1), all three symbols
survive in the `release` binary:

```text
t _RNvCs..._25rust_mcp_profile_workload15known_hot_frame
t _RNvCs..._25rust_mcp_profile_workload9level_one
t _RNvCs..._25rust_mcp_profile_workload9level_two
```

## Command line

| Argv | Behaviour |
| --- | --- |
| *(none)* | busy-loop for the default **2000 ms** |
| `<MILLIS>` | busy-loop for that many milliseconds (`u64`) |
| `--zero` | zero-sample control, see below |
| anything else | usage message on stderr, **exit code 2** |

Exactly one argument is accepted. Two or more arguments, or a single argument
that is neither `--zero` nor a non-negative integer, exit 2. Observed on the
authoring host: `--zero` returned immediately; `250` took `real 0.25`; no
argument took `real 2.00`; `nope` and `1 2` both exited 2.

The budget is enforced with `std::time::Instant`: the hot loop runs 4096 inner
steps between clock reads, so `Instant::now` is a negligible share of the frame
while the budget is still honoured closely.

## The `--zero` control

`--zero` makes `known_hot_frame` return before executing a single step, and the
process exits immediately afterwards. It exists so there is a **zero-samples
control case**: a profiling run over `--zero` must produce a report with no
samples attributed to `known_hot_frame` (in practice, no samples at all), and
the adapter must surface that as bounded, honest emptiness rather than
fabricating a profile or failing.

## Why frame pointers are forced

`.cargo/config.toml` sets:

```toml
[build]
rustflags = ["-C", "force-frame-pointers=yes"]
```

A sampling profiler that unwinds by walking the frame-pointer chain (the cheap,
signal-safe method, and the one available without `.eh_frame`/DWARF CFI in the
guest) can only reconstruct `main;level_one;level_two;known_hot_frame` if every
frame in the chain actually pushes a frame record. Optimized builds routinely
omit it — on aarch64 the ABI reserves `x29` but the compiler may still elide the
record in leaf and simple frames. Forcing it is what makes the stack shape
*known* rather than *likely*. Confirmed applied: `cargo build --release -v`
shows `force-frame-pointers=yes` on the rustc invocation.

`[profile.release] debug = 1` emits line tables, so the sampler can resolve
addresses to function names and lines without paying for full `debug = 2`
information.

`target/` is generated and is git-ignored; no build artifacts belong in this
corpus.

## Sin configuración de Cargo propia

Esta fixture **no** trae `.cargo/config.toml`. Los flujos M5 rechazan una fuente
con configuración de Cargo del proyecto, porque G2 prohíbe wrappers, linkers y
runners del proyecto y ese archivo es donde vivirían.

Los frame pointers los fuerza **el gateway**, poniendo
`RUSTFLAGS=-C force-frame-pointers=yes` en el entorno reconstruido de la fase de
construcción. Para reproducirlo en el host:

```sh
cd fixtures/profile-workload
RUSTFLAGS="-C force-frame-pointers=yes" cargo build --release --offline
```

Sin ese flag el binario sigue compilando, pero las pilas muestreadas son más
cortas; eso se declara en el resultado, no se disimula.
