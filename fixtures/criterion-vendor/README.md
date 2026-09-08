# Criterion offline vendor input

The pinned, offline input from which a Cargo **directory source** containing
`criterion 0.8.2` and its complete transitive dependency closure is built. It
exists so `fixtures/benchmark/` can build and run its benches with no network at
all.

**The committed input is 52 crates.io `.crate` archives — 13.84 MiB.** The
extracted tree they produce is 148 MiB and is *generated*, not committed:
`materialize.py` writes it to `vendor/`, which is git-ignored. This is the same
pattern the repository already uses for offline crate inputs in
`fixtures/rust-runtime/m4-scanner/`, where `provision.py` copies pinned `.crate`
files into a build context and `build.sh` extracts them and writes
`.cargo-checksum.json` inside the image.

Nothing is omitted and nothing is fabricated: the same 52 authentic packages,
the same checksums, stored compressed instead of extracted.

## Use it

```text
python3 -B fixtures/criterion-vendor/materialize.py
```

Run that **once** before building `fixtures/benchmark`. It verifies every
archive and extracts into `fixtures/criterion-vendor/vendor/`, which is where
`fixtures/benchmark/.cargo/config.toml` points. Other modes:

```text
python3 -B fixtures/criterion-vendor/materialize.py --verify-only
python3 -B fixtures/criterion-vendor/materialize.py --manifest <path> --output <dir>
```

`--verify-only` checks every archive's checksum and safety and writes nothing.
The script never touches the network and never needs to.

Delete `vendor/` when you are done; it is reproducible from the archives at any
time, and the materialized tree is byte-for-byte identical on every run.

Tests: `python3 -B -m unittest discover -s fixtures/criterion-vendor -p 'test_*.py'`.

## What it is for

The M5 benchmark fixture is ingested into a hardened Docker guest that runs
`cargo` with `--frozen` (`--locked --offline`), as uid 65534, with no network
whatsoever. Every dependency must therefore already be inside the repository.
The guest overrides `source.vendored-sources.directory` through its own
`CARGO_HOME` config at ingest time, because it mounts the fixture at a fixed
absolute path.

**This is a fixture input.** It is never a dependency of the repository
workspace, it is never built into any artifact, and it is never distributed. The
archives are committed on purpose: they *are* the offline input, so they cannot
be fetched inside the guest.

## Contents

52 packages: the entire closure of

```toml
[dev-dependencies.criterion]
version = "=0.8.2"
default-features = false
features = ["cargo_bench_support"]
```

resolved for `aarch64-unknown-linux-gnu`. **Nothing is omitted.** That includes
the three Windows-only packages `winapi 0.3.9`,
`winapi-i686-pc-windows-gnu 0.4.0` and `winapi-x86_64-pc-windows-gnu 0.4.0`:
`page_size 0.6.0` declares a `cfg(windows)` dependency on `winapi`, and Cargo
1.98.1 resolves target-specific dependencies for *every* target, not just the one
being built. A Linux or macOS build cannot start until they are satisfiable, even
though none of them is ever compiled there. Parking those directories reproduces
`error: no matching package named 'winapi' found ... required by page_size`.
Extracted they are 131 MiB of MinGW import libraries; as archives they are
2.5 MiB, which is the reason this directory stores archives.

`INVENTORY.json` is the manifest. Per package it records the name, version, the
Cargo lockfile package checksum (`sha256`), the sha256 and size of the stored
archive (`archive_sha256`, `archive_bytes`), the licence expression declared in
the package's own `Cargo.toml`, and the file count and total size of the
materialized tree (`files`, `bytes`). `sha256` and `archive_sha256` are the same
value — a crates.io package checksum *is* the sha256 of its `.crate` — and
`materialize.py` refuses to run if they ever disagree. `LICENSES.md` lists every
licence expression plus the relative path of every LICENSE / NOTICE / COPYING
file inside the materialized trees.

## How the archives were obtained

No network was used and none is needed; `network_used` in `INVENTORY.json`
records that.

1. A throwaway resolver crate outside the repository declared criterion exactly
   as above with a `harness = false` bench target, and `cargo generate-lockfile
   --offline` resolved 52 packages against the cached registry index.
2. For every registry package in that lock, the matching
   `~/.cargo/registry/cache/*/<name>-<version>.crate` was located and its sha256
   **verified against the `checksum` field in the generated `Cargo.lock` before
   it was copied here**, then verified again after the copy.

## What `materialize.py` guarantees

1. Every archive is checked — present, unlinked regular file, sha256 and size
   matching the manifest, and structurally safe — **before anything is written
   anywhere**. A single bad archive aborts the run with no output directory.
2. Archive safety is the same rule set as `m4-scanner/provision.py`
   (`validate_archive`): no absolute paths, no `..`, no backslashes, no name
   outside the allowlist, every member under the expected `<name>-<version>/`
   root, no duplicate members, and no symlinks, hardlinks, devices or FIFOs.
   All 5962 members of the 52 pinned archives satisfy it; a name that does not
   is a reason to stop, not to widen the allowlist.
3. Extraction writes every member explicitly at mode 0644 — no `extractall`, no
   inherited modes or timestamps.
4. `.cargo-checksum.json` is written per package in the form a Cargo directory
   source requires: `{"files":{"<relpath>":"<sha256>",...},"package":"<lock
   checksum>"}`, covering every regular file except `.cargo-checksum.json`
   itself, with `/`-separated relative paths and sorted keys.
5. The materialized file count and total size are compared against the manifest,
   so a truncated or altered archive fails even if it somehow passed step 1.

As Cargo's own generated files say: `.cargo-checksum.json` protects against
accidental modification. It is not an authentication mechanism and does not
protect against a malicious change.

## Integrity

Deterministic fingerprint of the materialized tree — sorted
`<relpath>\0<file sha256>\0` over all 6014 files under `vendor/`:

```text
sha256:854b6f12f2916ccf7f9d22483157543f51e216c7386f8f61dacf66dc4014f029
```

That is the same value the previously committed extracted tree had, which is the
evidence that moving to archive storage changed how the packages are stored and
nothing about what they contain.

To re-verify one package by hand:

```text
shasum -a 256 fixtures/criterion-vendor/criterion-0.8.2.crate
# must equal both "sha256" and "archive_sha256" for criterion in INVENTORY.json,
# and the "package" field of the materialized criterion-0.8.2/.cargo-checksum.json
```

## Committing

`.gitignore` here excludes `/vendor/` (generated) and `/__pycache__/`. Everything
else in this directory — the 52 `.crate` files, `INVENTORY.json`, `LICENSES.md`,
`materialize.py`, `test_materialize.py`, `README.md` — must be committed.

Unlike an extracted vendor tree, the archives carry no nested `.gitignore` files,
so a plain `git add fixtures/criterion-vendor` adds everything. Confirm with:

```text
git status --short fixtures/criterion-vendor
git ls-files fixtures/criterion-vendor | wc -l   # 58
git check-ignore $(find fixtures/criterion-vendor -type f) | wc -l   # 0
```

The 58 are the 52 `.crate` archives plus `.gitignore`, `INVENTORY.json`,
`LICENSES.md`, `README.md`, `materialize.py` and `test_materialize.py`.
