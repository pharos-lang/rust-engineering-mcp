# M2 D05 offline Cargo registry fixture qualification

Status: **fixture candidate qualified; D05 not accepted and M2-04/M2-05 not
complete**.

This bounded experiment exercised Cargo 1.98.1 from the already approved Rust
runtime against a generated local-registry snapshot. It used no host bind, shell,
runtime network, image pull/build, install, inherited host `CARGO_HOME`, project
code execution or production mutation path.

## Identities and retained artifacts

- Approved image:
  `sha256:8fac70723a8d04b6ec9633ab721806b8a55f4f083a1b3f988c61bf6a00fa1909`.
- Cargo: `1.98.1`, commit
  `797e8a9bca276c1c9f9f738d2a20f484fa4eea9d`, Linux ARM64.
- crates.io index commit:
  `3fae660f226d6b05eadcea9fb3512ecddaa33b67`.
- Registry-tree fingerprint:
  `sha256:1165e096813a84ae3989d84417031dee78fdffba3c4937c697c830c7500abbed`.
- Fixture manifest:
  `sha256:13ce748ffdbba59be52e11b70d33211ce747ecf014b77f0bc7bf43070fa1b3a2`.
- Probe:
  `sha256:36821e340c1e99e44c937a48817aacecec2cb32f09d2d4cfd9cf703b202d3306`.
- Raw report, 2,886,831 bytes:
  `sha256:a9379a8b30edc7219e9784547c0463efa3b2afe77bf2b4053e45afe80e901905`.
- Compact summary:
  `M2-D05-offline-registry-summary.json`.

The fixture contains original crates.io artifacts selected only after their
SHA-256 matched the current `Cargo.lock` and exact official index rows at the
pinned commit:

| Crate | Version | `.crate` SHA-256 | Declared license |
| --- | --- | --- | --- |
| `unicode-ident` | 1.0.24 | `e6e4313cd5fcd3dad5cafa179702e2b244f760991f45397d14d4ebf38247da75` | `(MIT OR Apache-2.0) AND Unicode-3.0` |
| `proc-macro2` | 1.0.107 | `985e7ec9bb745e6ce6535b544d84d6cd6f7ad8bd711c398938ae983b91a766d9` | `MIT OR Apache-2.0` |
| `quote` | 1.0.47 | `1fbf4db142a473a8d80c26bbf18454ed458bf8d26c8219c331daecfdbd079001` | `MIT OR Apache-2.0` |

The retained transitive case is `quote -> proc-macro2 -> unicode-ident`. The
basic case contains only `unicode-ident`. This is a bounded source replacement,
not a complete or live crates.io snapshot.

## Exact Cargo commands

Initial resolution and lock update:

```text
/opt/rust/bin/cargo metadata --format-version=1 --offline --manifest-path=/source/Cargo.toml '--config=source.crates-io.replace-with="rust-mcp-offline"' '--config=source.rust-mcp-offline.local-registry="/rust-mcp-registry"'
```

Stability verification:

```text
/opt/rust/bin/cargo metadata --format-version=1 --frozen --manifest-path=/source/Cargo.toml '--config=source.crates-io.replace-with="rust-mcp-offline"' '--config=source.rust-mcp-offline.local-registry="/rust-mcp-registry"'
```

The quotes shown inside each `--config` argument are literal TOML syntax in one
argv element. No shell parsed either command. Every full Docker argv, stdout,
stderr, duration and hash is retained in the raw JSON report.

## Applied containment and budgets

Every container was inspected before start. The probe required:

- immutable approved image and direct fixed entrypoint;
- `network=none`, read-only rootfs, all capabilities dropped,
  no-new-privileges and the existing `seccomp-rust` profile;
- no binds, volumes-from, privilege or restart policy;
- source and registry as Docker-managed local-driver tmpfs volumes held by
  running guardians;
- source staging mounted RW only for resolution;
- registry mounted read-only at `/rust-mcp-registry`;
- empty per-container `CARGO_HOME=/work/cargo`;
- 1 CPU, 1 GiB memory/swap, 128 PIDs and 30-second command deadline;
- 512 MiB `/work`, 64 MiB `/tmp`, 8 MiB/512-inode source and
  2 MiB/512-inode registry volumes.

The explicit fixed write discriminator against `/rust-mcp-registry` failed as
expected. Registry exports after successful work matched the ingested bytes.

## Results

All **222 observations matched**. Total probe duration was 7.439 seconds. Final
owned inventory was exactly:

```json
{"containers": [], "volumes": []}
```

Cleanup errors: `[]`.

The basic case resolved `unicode-ident 1.0.24`, updated an existing lock and
passed the second frozen run even though the local registry intentionally has no
`index/config.json`. Therefore Cargo 1.98.1 did not require that file for this
local-registry source replacement.

The combined add case resolved exactly:

```text
d05-fixture 0.1.0
proc-macro2 1.0.107
quote 1.0.47
unicode-ident 1.0.24
```

The dependency was aliased as `quote-alias`, optional but activated by the root
default feature, had default features disabled, and explicitly enabled
`quote/proc-macro`. The resulting lock hash was
`sha256:bf68356f62fa9dae886549f7c8abc2452e86bfc5e90ef801bb322f5044b4d292`.
Removing the dependency and resolving again pruned the three registry packages;
the lock hash became
`sha256:fc78013f786094ad3fc84ae16fae246508d9753b3697f7157cfe59fb33a77ba1`.
Both states passed a subsequent frozen resolution.

Negative cases all exited 101:

| Case | Observed denial | Staging changes before failure |
| --- | --- | --- |
| Missing `quote` index row | `no matching package named quote` | none |
| Missing `quote-1.0.47.crate` | failed to open local artifact | `Cargo.lock` |
| Corrupted `quote-1.0.47.crate` | checksum verification failed | `Cargo.lock` |

The second and third results are an important production constraint: Cargo can
write the staging lock before discovering a missing or corrupt artifact. A
failed Cargo exit therefore requires disposal of the entire candidate staging
tree. It cannot be exported or partially published. No host source was mounted,
so these observed staging writes had no host source effect.

## Interpretation and limits

The experiment qualifies the retained fixtures and supports a D05 candidate
using an explicit local registry with a read-only guest mount, an empty ephemeral
Cargo home, first-pass `--offline` resolution and second-pass `--frozen`
verification.

It does not select production provisioning, accept D05, exercise the production
gateway verifier/exporter, implement dependency tools, prove arbitrary registry
closures, or complete M2-04/M2-05. The Python USTAR encoder/parser is experiment
code. Production must generate its own bounded archive, validate scope and
discard every failed staging tree.

Official format and behavior references:

- <https://doc.rust-lang.org/cargo/reference/source-replacement.html>
- <https://doc.rust-lang.org/cargo/reference/registry-index.html>
- <https://doc.rust-lang.org/stable/nightly-rustc/cargo/sources/registry/local/struct.LocalRegistry.html>
- <https://doc.rust-lang.org/stable/cargo/commands/cargo-metadata.html>
- <https://github.com/rust-lang/cargo/blob/797e8a9bca276c1c9f9f738d2a20f484fa4eea9d/src/cargo/ops/cargo_add/mod.rs>
- <https://github.com/rust-lang/crates.io-index/tree/3fae660f226d6b05eadcea9fb3512ecddaa33b67>
