// The one deliberate defect in this corpus.
//
// `work_unit` takes a `u64`; this passes it a `&str`, so `rustc` stops with
// `error[E0308]: mismatched types` and `cargo bench` never runs a benchmark.
// The harness resolution still succeeds — criterion 0.8.2 is a real, resolved
// dev-dependency — so the tool observes the approved harness AND a failed
// compilation, which is exactly the case ADR-080 §6 exercises: the caller's
// only usable evidence is the compiler's own text in the `harness_stderr`
// artifact.
//
// Do not "fix" this file.
#![allow(deprecated)]

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use rust_mcp_benchmark_compile_error_fixture::work_unit;

const N: u64 = 4_096;

fn m5(c: &mut Criterion) {
    let mut group = c.benchmark_group("m5");
    group.bench_function("reference", |b| {
        // DELIBERATE TYPE ERROR: `work_unit` wants a `u64`.
        b.iter(|| black_box(work_unit(black_box("this is not a u64"))));
    });
    group.finish();
    let _ = N;
}

criterion_group!(benches, m5);
criterion_main!(benches);
