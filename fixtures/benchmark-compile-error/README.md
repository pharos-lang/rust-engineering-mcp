# M5 observed-compilation-failure fixture

`rust-mcp-benchmark-compile-error-fixture` is the negative twin of
`../benchmark`. It is an isolated workspace (`[workspace]` in its own
`Cargo.toml`), never a member of the repository workspace, a fixture input only.

## What it is for

ADR-080 §6 requires the harness-log recovery to be exercised from a real client,
with **an observed compilation failure** among the cases. This fixture is that
case:

- `Cargo.toml` declares `criterion =0.8.2` as a dev-dependency, exactly as
  `../benchmark` does, so `rust.benchmark.run` resolves the **approved** harness
  and the run is a measurable one on paper;
- `src/lib.rs` compiles;
- `benches/perf.rs` passes a `&str` where `work_unit` wants a `u64`, so `rustc`
  stops with `error[E0308]: mismatched types` and no benchmark ever runs.

The tool therefore observes a real execution that failed to compile: no dataset,
no criterion output tree, and a `harness_stderr` artifact holding the compiler's
own text. Before ADR-080 that text was captured and dropped, and the response
directed the caller to a `criterion_archive` that cannot exist for this run.

**The type error is the fixture. Do not fix it.** A change that makes this
package compile silently removes the only case in the corpus that produces an
observed compilation failure.

## Offline dependency resolution

Identical to `../benchmark`: `Cargo.lock` is committed and holds the same 53
entries — this package plus criterion's complete 52-package transitive closure —
and the closure itself lives in `../criterion-vendor`. Materialize it once
before building:

```text
python3 -B fixtures/criterion-vendor/materialize.py
```

The lockfile is `../benchmark/Cargo.lock` with the root package renamed; the
dependency graph is the same one, so no new archive enters the corpus.

## Sin configuración de Cargo propia

Como `../benchmark`, esta fixture **no** trae `.cargo/config.toml` y no debe
traerlo: los flujos M5 rechazan una fuente que contenga configuración de Cargo
del proyecto. Para construir en el host, pasa la sustitución de fuente por línea
de comandos:

```sh
python3 -B fixtures/criterion-vendor/materialize.py
cd fixtures/benchmark-compile-error
cargo bench --offline --bench perf \
  --config 'source.crates-io.replace-with="vendored-sources"' \
  --config 'source.vendored-sources.directory="../criterion-vendor/vendor"'
```

That command is expected to **fail**, printing `error[E0308]` and exiting
non-zero. A clean build means the fixture has been broken.
