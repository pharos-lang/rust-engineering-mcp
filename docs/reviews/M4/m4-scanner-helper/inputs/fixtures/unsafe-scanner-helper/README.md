# Isolated unsafe syntax scanner helper

This private fixture implements the parser boundary accepted by ADR-069. It is
not an MCP tool and is not installed by the repository build. A later M4 runtime
change will build it offline, inventory it, and place the binary at the fixed guest
path `/opt/security/bin/rust-mcp-unsafe-helper`.

## Closed invocation protocol

The binary has exactly two modes:

- no arguments: supervisor;
- `--file-index <0..4095>`: one-file worker.

Both modes read `/security/scan.json`. The supervisor launches the absolute binary
`/opt/security/bin/rust-mcp-unsafe-helper` sequentially, clears its environment,
sets `PATH=/usr/bin:/bin`, passes only the validated numeric index, uses no shell,
discards stderr, and drains at most 512 KiB of stdout. Each worker receives two
seconds. It is killed and reaped on timeout; the supervisor then continues.

The manifest is at most 1 MiB and has this exact schema; unknown fields, missing or
non-contiguous indices, more than 4096 files, non-`.rs` paths, and paths outside the
two fixed mount prefixes are rejected:

```json
{
  "schema_version": 1,
  "files": [
    { "index": 0, "path": "/source/src/lib.rs" },
    { "index": 1, "path": "/rust-mcp-vendor/example-1.0.0/src/lib.rs" }
  ]
}
```

The worker reads one UTF-8 source file of at most 1 MiB. Its JSON contains only a
typed status, keyword spans, counts, and the number of opaque macro token-tree
boundaries. Parser diagnostics and source text are never copied. Span bytes are
0-based half-open offsets; line and Unicode-scalar column are 1-based.

A successful worker response has this exact shape:

```json
{
  "schema_version": 1,
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
  "macro_omitted": 0
}
```

The supervisor returns one status and total/omitted counts per file, no more than
128 findings overall, and at most 512 KiB of JSON. Status is one of `parsed`,
`parse_error`, `crashed`, `timed_out`, or `unavailable`. It always declares:

```json
{
  "cfg_evaluated": false,
  "macros_expanded": false,
  "generated_sources_scanned": false
}
```

Each file summary uses `index`, `status`, `total_findings`, `omitted_findings`,
and `macro_omitted`. Findings use `file_index`, `kind`, `byte_start`, `byte_end`,
`line`, `column`, and `conditional`. The compact `macro_omitted` wire name keeps
the worst-case 4096-file response inside the output limit.

The supervisor response has this exact shape:

```json
{
  "schema_version": 1,
  "files": [
    {
      "index": 0,
      "status": "parsed",
      "total_findings": 1,
      "omitted_findings": 1,
      "macro_omitted": 0
    }
  ],
  "findings": [],
  "total_findings": 1,
  "omitted_findings": 1,
  "cfg_evaluated": false,
  "macros_expanded": false,
  "generated_sources_scanned": false
}
```

The example omits its one known finding from `findings` to demonstrate that the
per-file and aggregate omission counts refer to the retained supervisor output.

The syntax classes are `unsafe_attribute`, `unsafe_block`,
`unsafe_extern_block`, `unsafe_fn`, `unsafe_impl`, `unsafe_mod`, `extern_block`,
`extern_crate`, and `extern_fn`. `cfg` and `cfg_attr` on a finding or its enclosing
syntax make `conditional` true. Macro bodies and invocations are opaque. A parsed
file with no findings is not evidence of memory safety, absence of expanded unsafe
code, or absence of generated unsafe code.

Fatal protocol failures use a small typed object and exit code 2:

```json
{ "schema_version": 1, "error": "manifest_invalid" }
```

The closed error codes are `invalid_arguments`, `manifest_unavailable`,
`manifest_too_large`, `manifest_invalid`, and `output_too_large`. Parse failures
and unavailable source files are file statuses and do not copy an error message.

## Local verification

The helper has its own `[workspace]` and lockfile. Its benign checks are:

```text
CARGO_TARGET_DIR=target/unsafe-scanner-helper cargo fmt --check --manifest-path fixtures/unsafe-scanner-helper/Cargo.toml
CARGO_TARGET_DIR=target/unsafe-scanner-helper cargo check --locked --offline --manifest-path fixtures/unsafe-scanner-helper/Cargo.toml
CARGO_TARGET_DIR=target/unsafe-scanner-helper cargo clippy --locked --offline --manifest-path fixtures/unsafe-scanner-helper/Cargo.toml --all-targets -- -D warnings
CARGO_TARGET_DIR=target/unsafe-scanner-helper cargo test --locked --offline --manifest-path fixtures/unsafe-scanner-helper/Cargo.toml --all-targets
```

`hostile/generate.py` only defines adversarial input generators. Do not execute
those generated cases on the host. They belong in the calibrated guest once the
helper is provisioned into an M4 image.
