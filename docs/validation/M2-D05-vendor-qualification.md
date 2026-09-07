# M2 D05 Cargo directory-source qualification

## Task

Qualify, without changing production contracts or ADRs, whether Cargo's built-in `cargo vendor` is a simpler administrative source for `dependency.add/remove` than the previously qualified local registry. The experiment had to use Cargo 1.98.1 in the approved image, private Docker tmpfs volumes, no host bind, no shell, no pull/build/install, and `--network=none` for every guest.

Official basis:

- Cargo says `cargo vendor [options] [path]` vendors locked remote dependencies and prints the source-replacement configuration: <https://doc.rust-lang.org/cargo/commands/cargo-vendor.html>.
- `--versioned-dirs` gives every crate a versioned directory; `--locked` rejects a missing or changed lock; `--offline` denies network access through Cargo; and `--respect-source-config` lets the bounded producer read the explicitly supplied source replacement.
- Cargo documents directory sources as unpacked crates primarily produced by `cargo vendor`; `.cargo-checksum.json` detects accidental modification but is not a security mechanism: <https://doc.rust-lang.org/cargo/reference/source-replacement.html#directory-sources>.
- `--config` accepts typed command-line overrides that take precedence over environment and files: <https://doc.rust-lang.org/cargo/reference/config.html#command-line-overrides>.

## Result

**Qualified candidate, not D05 acceptance.** The built-in directory-source producer is the better default for developer setup. The exact producer command in the pinned guest succeeded offline and locked:

```text
/opt/rust/bin/cargo vendor --offline --locked --respect-source-config --versioned-dirs --manifest-path=/source/Cargo.toml '--config=source.crates-io.replace-with="rust-mcp-offline"' '--config=source.rust-mcp-offline.local-registry="/rust-mcp-registry"' /source/vendor
```

It produced the three expected versioned directories and printed only the expected consumer config. A separate guest with the directory mounted read-only at `/rust-mcp-vendor` resolved:

- basic `unicode-ident = "=1.0.24"`, then the same lock under `--frozen`;
- alias + optional feature + transitive closure: `quote 1.0.47`, `proc-macro2 1.0.107`, and `unicode-ident 1.0.24`, then the same lock under `--frozen`.

Removing `quote-1.0.47` failed with Cargo exit 101 and created no candidate. Changing vendored `quote-1.0.47/Cargo.toml` also failed with exit 101 when Cargo consumed the source. The latter exposed a decisive oracle boundary: `cargo metadata --offline` returned 0 and wrote the staging lock before `cargo check --frozen` detected the altered checksum. Production must validate every approved vendored byte before resolution and must discard any failed staging lock; metadata success alone does not qualify the data.

## Comparison fit

| Property | Local registry | Directory source |
|---|---:|---:|
| Producer available with Cargo | No; documented primary producer is separately installed `cargo-local-registry` | Yes; built-in `cargo vendor` |
| Regular files in this fixture | 6 | 92 (15.333x) |
| Directories in exported tree | 7 | 22 |
| Logical bytes | 143,020 | 752,307 (5.260x) |
| Deterministic USTAR bytes | 153,600 | 839,680 (5.467x) |
| Same-run ingest lifecycle | 140,559,042 ns | 148,736,792 ns (1.058x) |
| Runtime index needed | Yes | No |
| Integrity metadata | index checksum + `.crate` hash | per-file and package hashes in `.cargo-checksum.json` |

The larger directory source cost only 8,177,750 ns more in the measured bounded ingest lifecycle. This tiny fixture is not a scaling benchmark, but developer installation simplicity outweighs its extra entries. The local registry remains useful as a compact test producer input, not the preferred user-facing provisioning format.

A common developer with a suitable, already locked provisioning workspace can use their own installed Cargo:

```text
cargo vendor --locked --versioned-dirs /private/vendor
rust-engineering-mcp cargo-vendor inspect --directory /private/vendor --json
rust-engineering-mcp serve --cargo-vendor-dir=/private/vendor --cargo-vendor-tree-sha256=sha256:<64-hex>
```

The command and flag names above are a D05 design candidate, not an implemented or public contract. The administrative `inspect` command should emit the second flag from a complete snapshot and package inventory. The runtime never invokes host Cargo, inherits host `CARGO_HOME`, or installs anything. It constructs only:

```text
--config=source.crates-io.replace-with="rust-mcp-vendor"
--config=source.rust-mcp-vendor.directory="/rust-mcp-vendor"
```

The developer workflow is one `cargo vendor` command only when an appropriate `Cargo.lock` already covers the allowed add candidates. A normal project lock cannot supply a new crate absent from that lock. Such users need a separate locked provisioning manifest whose dependency closure is the explicit administrative allowset; defining that CLI workflow remains a D05 decision.

## Files changed

- `scripts/probe-m2-vendor-data.py`
- `fixtures/cargo-vendor-data/README.md`
- `fixtures/cargo-vendor-data/manifest.json`
- `fixtures/cargo-vendor-data/vendor/**`
- `docs/validation/M2-D05-vendor-qualification.json`
- `docs/validation/M2-D05-vendor-summary.json`
- `docs/validation/M2-D05-vendor-qualification.md`

No production source, global ADR, public contract, dependency, `Cargo.lock`, corpus baseline, or previous local-registry artifact was changed by this work.

## Tests executed

```text
python3 -m py_compile scripts/probe-m2-vendor-data.py
python3 scripts/probe-m2-vendor-data.py
```

The final run matched 230 observations across 309 recorded Docker events in 7,831,052,834 ns. It verified the approved image, hardening and mounts for every container, exact generation, bounded export, complete checksum maps, basic and transitive resolution, second frozen resolution, missing data, altered checksum, read-only vendor mounts, source effects, round-trip transfer, and owned cleanup.

## Evidence

- Cargo: `cargo 1.98.1`, commit `797e8a9bca276c1c9f9f738d2a20f484fa4eea9d`.
- Approved image: `sha256:8fac70723a8d04b6ec9633ab721806b8a55f4f083a1b3f988c61bf6a00fa1909`.
- Source index commit: `3fae660f226d6b05eadcea9fb3512ecddaa33b67`.
- Source registry fingerprint: `sha256:1165e096813a84ae3989d84417031dee78fdffba3c4937c697c830c7500abbed`.
- Vendor tree fingerprint: `sha256:743947d5788c1a4385a4b59869c5b8bd0535f7fc0d875b51288f9b26b2d0eba1`.
- Script hash: `sha256:f91425c5d6014afa7528ba114b53732894c85b391d83da151dd6db0d289cfbad`.
- Raw report hash: `sha256:66200dd78b47f670585f8923f5fbb6d7f987996efe465308e23e892d237f8d4e`.
- Generation: 158,009,625 ns.
- Basic resolution/frozen: 167,892,041 / 155,929,708 ns.
- Transitive resolution/frozen: 167,651,709 / 159,629,542 ns.
- Package checksum anchors: `unicode-ident` `e6e4313cd5fcd3dad5cafa179702e2b244f760991f45397d14d4ebf38247da75`; `proc-macro2` `985e7ec9bb745e6ce6535b544d84d6cd6f7ad8bd711c398938ae983b91a766d9`; `quote` `1fbf4db142a473a8d80c26bbf18454ed458bf8d26c8219c331daecfdbd079001`.
- Final owned inventory: zero containers and zero volumes; cleanup errors: none.

The exporter ran only after the Cargo mutator container was removed. Its host parser accepted at most 8 MiB, 2,048 entries, 2 MiB per file, 4 MiB logical content, 240-byte paths, and depth 16. It rejected absolute/dot-parent paths, duplicates, PAX metadata, special files, symlinks, hardlinks, devices, FIFOs, and privileged mode bits. It materialized each accepted regular file explicitly; it never called archive extraction on the host. The observed vendor files were all regular mode `0644`.

## Risks

- Directory sources increase inode and transfer budgets substantially; realistic workspaces need a measured bound and early `limit_exceeded` result.
- `.cargo-checksum.json` provides consistency, not provenance. The administrative inspector must anchor package checksums to the provisioning lock/approved inventory and sign or explicitly trust the emitted top-level digest through host configuration.
- External writers can race a configured vendor directory. Runtime validation must open the trusted root with no-follow handle traversal, reject links and non-regular or multi-linked files, stream one bounded snapshot into private owned staging while hashing, compare that exact copied tree to the configured digest, close the host handles, and mount only the private copy read-only. Hashing and later rereading the host directory would retain a TOCTOU gap.
- Cargo metadata can leave a changed staging lock before a later integrity failure. Failed validation or Cargo results must publish zero candidate and delete private staging.
- This fixture proves one small crates.io closure on Docker Desktop arm64; it does not establish production scale or cross-platform host traversal.

## Decisions

Recommend D05 adopt a directory source as the administrative provisioning format, subject to owner review. Keep resolution in the pinned guest, network-isolated, with empty generated `CARGO_HOME`, fixed directory-source configuration, an immutable private copy, exact Cargo identity, and dataset digest in the receipt. Keep the local registry only as compact upstream evidence/fixture input.

The MCP administrative inspector owns the trust decision before any operation: validate a closed package allowset, exact name/version/package checksum, complete file checksum maps, canonical relative paths, metadata restrictions, budgets, and a deterministic whole-tree digest. An absent crate/version/feature closure returns `offline_data_missing`; an altered or structurally invalid tree returns an administrative configuration/integrity denial before Cargo; neither becomes a failed project build or an approved candidate.

## Open issues

1. D05 must define how users create the separate locked provisioning manifest for new crates absent from the target project's existing lock, including whether multiple target triples and target-specific dependencies belong in one allowset.
2. D05 must fix production budgets for crates, files, bytes, path depth, and inspection/copy time from representative workspaces.
3. D05 must define the exact administrative subcommand/flag names, ownership/permission policy, persisted inventory format, and digest algorithm; the names demonstrated here are proposals.
4. The production host reader needs native no-follow traversal and copy-while-hash support for the configured external vendor root. The existing read pipeline's canonicalization is insufficient for an external writer race.
5. The package checksum field cannot authenticate an artifact by itself; D05/D14 must define the trusted provisioning origin, notices, and SBOM linkage before distribution.
