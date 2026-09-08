# M5 benchmark fixture

`rust-mcp-benchmark-fixture` is the measurement oracle for the M5 benchmark
adapter. It is an isolated workspace (`[workspace]` in its own `Cargo.toml`) and
is never a member of the repository workspace. It is a fixture input, never a
workspace dependency and never distributed.

## The three benches

`benches/perf.rs` runs with `harness = false` and declares exactly three
criterion benchmarks in a single group, `m5`:

| Bench | Workload | `step` calls for `n` | Expected design ratio |
| --- | --- | --- | --- |
| `m5/reference` | `work_unit(n)` | `n` | 1.00x, the reference |
| `m5/slower_125` | `work_slower(n)` | `n + n / 4` | 1.25x |
| `m5/control` | `work_noisy(n)` | `n` | 1.00x, noise / self-compare control |

`n` is fixed at `4096`, a multiple of four, so `n + n / 4` is exactly `5120`
operations and the truncating division introduces no rounding.

All three workloads run the same `#[inline(always)] step` operation, seeded
identically. `work_slower` differs from `work_unit` only in how many times the
loop body runs. `work_noisy` runs the loop body exactly as many times as
`work_unit` but walks the index set in descending order, so its instruction path
differs while its operation count does not; it exists so an adapter has a
self-compare case that should report "no meaningful change".

Every bench passes both its input and its output through `criterion::black_box`.
That re-export is deprecated in criterion 0.8.2 — its body is literally
`std::hint::black_box(dummy)` — so `benches/perf.rs` carries a single
file-scoped `#![allow(deprecated)]` with a comment saying why.

### The ratio is a design ratio, not a measurement

**1.25x is an expected *design* ratio derived from operation counts. It is not a
validated measurement and this fixture makes no claim about what any particular
machine will report.** Instruction-level parallelism, frequency scaling, cache
state and scheduler noise all move the observed number. A host run of
`--sample-size 10 --measurement-time 1` on macOS/arm64 during authoring produced
2.72 µs / 3.42 µs / 2.76 µs (≈1.256x and ≈1.013x), which is consistent with the
design but is a single unreplicated observation, not a pinned oracle. Any
tolerance an adapter asserts must be chosen and justified separately.

## Offline dependency resolution

### Materialize the vendor tree first

`../criterion-vendor` commits its 52 packages as pinned crates.io `.crate`
archives, not as an extracted tree. **Run the materializer once before building
this fixture:**

```text
python3 -B fixtures/criterion-vendor/materialize.py
```

It verifies every archive's checksum and safety and extracts into
`../criterion-vendor/vendor/`, which is generated and git-ignored. The archives
are the committed input; the extracted tree is not. Delete `vendor/` when you
are done — it is reproducible byte-for-byte at any time.

`.cargo/config.toml` replaces `crates-io` with the directory source at
`../criterion-vendor/vendor`, which then carries `criterion 0.8.2` and its
complete transitive closure — 52 packages — as exact crates.io content. The
relative path is what makes the fixture build on the host.

**The guest ingests this fixture at a fixed absolute path, so the gateway
overrides `source.vendored-sources.directory` through its own `CARGO_HOME`
config at ingest time.** The relative path committed here is the host-side
default; it is not the path the container uses.

`Cargo.lock` is committed and holds 53 entries: this package plus all 52
vendored ones. Three of those — `winapi`, `winapi-i686-pc-windows-gnu` and
`winapi-x86_64-pc-windows-gnu` — are never compiled on Linux or macOS, but Cargo
1.98.1 resolves `page_size 0.6.0`'s `cfg(windows)` dependency for every target,
so the build cannot start unless they are present in the source. See
`../criterion-vendor/README.md`.

## Reproducing

```text
python3 -B fixtures/criterion-vendor/materialize.py
cd fixtures/benchmark
cargo bench --offline --bench perf -- \
  --warm-up-time 1 --measurement-time 1 --sample-size 10 --noplot --color never
```

That writes `target/criterion/m5/<bench>/new/{sample.json,estimates.json,benchmark.json}`.
`sample.json` has the top-level keys `sampling_mode`, `iters` and `times`;
`estimates.json` has `mean`, `median`, `median_abs_dev`, `slope` and `std_dev`;
`benchmark.json` has `group_id`, `function_id`, `value_str`, `throughput`,
`full_id`, `directory_name` and `title`. Criterion is built with
`default-features = false, features = ["cargo_bench_support"]`, so there are no
plotters, rayon or HTML report outputs.

`target/` is generated and is git-ignored; no build artifacts belong in this
corpus.

## Sin configuración de Cargo propia

Esta fixture **no** trae `.cargo/config.toml`, y no debe traerlo. Los flujos M5
rechazan una fuente que contenga configuración de Cargo del proyecto: G2 prohíbe
wrappers, linkers y runners del proyecto, y ese archivo es además el sitio donde
un proyecto podría redirigir `source.crates-io` a un directorio que él mismo
controla, sustituyendo los bytes de las dependencias que la medición está a punto
de describir. El entorno de compilación pertenece al servidor.

Para construir en el host, pasa la sustitución de fuente por línea de comandos,
que tiene la precedencia más alta de Cargo:

```sh
python3 -B fixtures/criterion-vendor/materialize.py
cd fixtures/benchmark
cargo bench --offline --bench perf \
  --config 'source.crates-io.replace-with="vendored-sources"' \
  --config 'source.vendored-sources.directory="../criterion-vendor/vendor"' \
  -- --noplot --color never --warm-up-time 3 --measurement-time 5 --sample-size 30
```

El gateway hace exactamente lo mismo dentro del guest, con el directorio del
vendor autenticado por el host.

## El benchmark `control` no es un control 1,00x

Se diseñó como control de auto-comparación, pero la medición real en el guest lo
desmiente: `control` resultó un 2,9 % **más rápido** que `reference`
([recibo](../../docs/validation/M5-01-benchmark-calibration.json)). Recorrer el
mismo conjunto de índices en orden descendente no cuesta lo mismo que en orden
ascendente en este hardware, aunque el número de operaciones sea idéntico.

Se conserva como tercer punto de medida y como caso de familia de tres
comparaciones, pero **no** se usa como control. El control de auto-comparación
real es comparar el mismo benchmark entre dos ejecuciones independientes de la
misma fuente, que es lo que hacen `criterion-run-1.tar` y `criterion-run-2.tar`
en `fixtures/benchmark-datasets`.
