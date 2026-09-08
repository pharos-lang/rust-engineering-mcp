# Isolated unsafe syntax scanner helper

This private fixture implements the parser boundary accepted by ADR-069. It is
not an MCP tool and is not installed by the repository build. M4 provisioning
builds it offline, inventories it, and places the exact binary at
`/opt/security/bin/rust-mcp-unsafe-helper`.

## Closed invocation protocol v2

The binary has exactly two modes:

- no arguments: supervisor;
- `--file-index <0..4095>`: one-file worker.

Both modes read `/security/scan.json`. The supervisor launches the same absolute
binary sequentially, clears its environment, sets `PATH=/usr/bin:/bin`, passes
only the validated numeric index, uses no shell, null stdin, and discards child
stderr. The fixed path is not injectable.

The manifest is at most 1 MiB and has this exact schema. `budget_ms` is in
`1..=118000`; it is derived by the gateway from its remaining deadline after
reserving four seconds for the surrounding phases. Unknown fields, duplicate
paths, missing or non-contiguous indices, more than 4096 files, non-`.rs` paths,
and paths outside the two fixed mount prefixes are rejected.

```json
{
  "schema_version": 2,
  "budget_ms": 118000,
  "files": [
    { "index": 0, "path": "/source/src/lib.rs" },
    { "index": 1, "path": "/rust-mcp-vendor/example-1.0.0/src/lib.rs" }
  ]
}
```

The gateway extracts ordinary captured files into owned volumes before the
scanner phase and mounts source, vendor, and policy volumes read-only. There are
no concurrent writers or host mounts, and extraction rejects symlinks and special
files. Under that admitted premise, repeated manifest reads see immutable regular
bytes; protocol v2 does not claim a general host-filesystem TOCTOU defense and
does not add an unnecessary caller-provided digest.

The worker reads one source file. `unavailable`, `too_large`, and `invalid_utf8`
are distinct statuses. A parsed worker response has long descriptive keys and
contains only typed status, keyword spans, bounded counts, and omission counts:

```json
{
  "schema_version": 2,
  "file_index": 0,
  "status": "parsed",
  "findings": [
    {
      "file_index": 0,
      "kind": "unsafe_block",
      "byte_start": 9,
      "byte_end": 15,
      "line": 1,
      "column": 10,
      "conditional": false
    }
  ],
  "total_findings": 1,
  "omitted_findings": 0,
  "macro_omitted": 0,
  "opaque_syntax_omitted": 0
}
```

Source size, worker stdout retention, each count, span endpoint, line, and column
are bounded. Span bytes are 0-based half-open offsets; line and Unicode-scalar
column are 1-based. Parser diagnostics and source text are never copied.

The supervisor gives each child at most two seconds and never more than its
remaining manifest budget. It drains child stdout to EOF while retaining at most
512 KiB. After a child is killed and reaped, drain confirmation receives a fixed
20 ms scheduling grace independent of the child deadline; the outer elapsed
clock charges that grace to the global budget. If EOF is still not confirmed,
the reader may remain detached only until supervisor exit: no more children are
launched and every unstarted row becomes `unavailable`. Actual global budget
exhaustion preserves completed rows and marks the remainder `budget_exhausted`.

The supervisor emits exactly one row per manifest index. Compact row keys keep
the 4096-row worst case below 512 KiB:

- `i`: index;
- `s`: one of `parsed`, `parse_error`, `crashed`, `timed_out`, `unavailable`,
  `invalid_utf8`, `too_large`, or `budget_exhausted`;
- `total`, `omitted`, `macros`, and `opaque`: bounded counters.

```json
{
  "schema_version": 2,
  "files": [
    { "i": 0, "s": "parsed", "total": 1, "omitted": 1, "macros": 0, "opaque": 0 },
    { "i": 1, "s": "budget_exhausted", "total": 0, "omitted": 0, "macros": 0, "opaque": 0 }
  ],
  "findings": [],
  "total_findings": 1,
  "omitted_findings": 1,
  "cfg_evaluated": false,
  "macros_expanded": false,
  "generated_sources_scanned": false
}
```

At most 128 findings are retained, in manifest-file priority and then source
order. Earlier files consume the retention budget first; totals and omissions
remain visible for later files. If serialization ever exceeds its measured
margin, findings are removed before file summaries, preserving all status and
count rows.

The syntax classes are `unsafe_attribute`, `unsafe_block`,
`unsafe_extern_block`, `unsafe_fn`, `unsafe_impl`, `unsafe_mod`, `unsafe_static`,
`unsafe_trait`, `extern_block`, `extern_crate`, and `extern_fn`. `cfg` and
`cfg_attr` on a finding or an enclosing syntax carrier make it conditional.
Nested `cfg_attr(..., unsafe(...))` wrappers are inspected syntactically without
evaluating the predicate or expanding attributes. Macro bodies and invocations
remain opaque and increment `macros`; syn `Verbatim` nodes increment `opaque`.
Any opaque syntax makes host coverage partial.

A parsed file with no findings is not evidence of memory safety, absence of
expanded unsafe code, or absence of generated unsafe code. Parse failures and
per-file runtime failures preserve prior results and do not copy error prose.

Fatal protocol failures use a small typed object and exit code 2:

```json
{ "schema_version": 2, "error": "manifest_invalid" }
```

The closed fatal codes are `invalid_arguments`, `manifest_unavailable`,
`manifest_too_large`, `manifest_invalid`, and `output_too_large`.

## Local verification

The helper has its own workspace and lockfile. Its benign checks are:

```text
CARGO_TARGET_DIR=target/unsafe-scanner-helper cargo fmt --check --manifest-path fixtures/unsafe-scanner-helper/Cargo.toml
CARGO_TARGET_DIR=target/unsafe-scanner-helper cargo check --locked --offline --manifest-path fixtures/unsafe-scanner-helper/Cargo.toml
CARGO_TARGET_DIR=target/unsafe-scanner-helper cargo clippy --locked --offline --manifest-path fixtures/unsafe-scanner-helper/Cargo.toml --all-targets -- -D warnings
CARGO_TARGET_DIR=target/unsafe-scanner-helper cargo test --locked --offline --manifest-path fixtures/unsafe-scanner-helper/Cargo.toml --all-targets
```

`hostile/generate.py` only defines adversarial input generators. Its default
depth is 40000, for which every generated case fits below 1 MiB. It rejects
negative depths and rejects the full request if any explicit depth makes a case
exceed 1 MiB. Do not execute generated cases with the helper on the host; they
belong in the calibrated guest after the helper is provisioned into an M4 image.
