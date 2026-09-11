// `criterion::black_box` is a deprecated re-export in criterion 0.8.2 whose
// body is exactly `std::hint::black_box(dummy)`. The M5 oracle pins the
// criterion spelling on purpose, so the deprecation is allowed here and
// nowhere else.
#![allow(deprecated)]

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use rust_mcp_benchmark_fixture::{work_noisy, work_slower, work_unit};

/// Iteration count handed to every workload. A multiple of four, so
/// `work_slower`'s `n + n / 4` is exactly 1.25 x `n` operations.
const N: u64 = 4_096;

fn m5(c: &mut Criterion) {
    let mut group = c.benchmark_group("m5");

    // 1.00x reference.
    group.bench_function("reference", |b| {
        b.iter(|| black_box(work_unit(black_box(N))));
    });

    // 1.25x by construction: 25% more of the identical inner operation.
    group.bench_function("slower_125", |b| {
        b.iter(|| black_box(work_slower(black_box(N))));
    });

    // Noise / self-compare control: same total work as `reference`.
    group.bench_function("control", |b| {
        b.iter(|| black_box(work_noisy(black_box(N))));
    });

    group.finish();
}

criterion_group!(benches, m5);
criterion_main!(benches);
