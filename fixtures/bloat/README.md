# M5 binary-size fixture

The oracle for `rust.binary.bloat`. It is an isolated two-crate workspace
(`[workspace] members = ["bloat-inner"]` in its own `Cargo.toml`) and is never a
member of the repository workspace. It is a fixture input, never a workspace
dependency and never distributed. It has **no external dependencies at all**;
`Cargo.lock` is committed and contains only the two local packages.

## The two crates

| Crate | Kind | Contribution |
| --- | --- | --- |
| `rust-mcp-bloat-fixture` | binary `rust-mcp-bloat-fixture` | `root_payload`, 256 unrolled mixing rounds |
| `bloat-inner` | path member library | `inner_payload`, 256 unrolled mixing rounds |

Both functions are `#[inline(never)] pub fn`, integer-only and allocation-free,
and each is large enough to be its own line in a per-function report. They use
deliberately different constants so the linker cannot fold them into one symbol.

`main` calls both through `std::hint::black_box` — on the input and on the
result — and prints the combined value, so neither crate's contribution can be
dropped as dead code.

## Expected outcome

**A per-crate report of the `release` binary must contain a row for
`rust-mcp-bloat-fixture` and a row for `bloat-inner`.** That is a statement about
*presence*, not about size. Verified on the authoring host (macOS/arm64,
Rust 1.98.1), `nm` shows both symbols in both profiles, mangled with their
originating crate names:

```text
t _RNvCs..._22rust_mcp_bloat_fixture12root_payload
T _RNvCs..._11bloat_inner13inner_payload
```

### Only the file size is exact

**The only exact number this fixture yields is the size of the linked file on
disk.** Every per-function and per-crate figure any tool reports — cargo-bloat
included — is *estimated attribution*: it is derived from symbol table sizes and
debug-info ranges, it does not account for shared, folded, inlined or
linker-generated code, it excludes data that no symbol claims, and the per-crate
totals will not add up to the file size. Do not pin a byte count for either
crate; pin the presence of both rows and, if a size assertion is needed, pin the
file size only.

## Profiles

```toml
[profile.release]     # baseline: both crates survive as distinct symbols
lto = false
debug = false
strip = "none"

[profile.release-lto] # LTO variant, for cross-crate-inlining comparisons
inherits = "release"
lto = "fat"
```

`debug = false` and `strip = "none"` are explicit so the symbol table is present
but the debug info is not; that is the configuration a per-symbol report needs
and it keeps the baseline binary reproducible in size. Build the variant with
`cargo build --offline --profile release-lto`. Under fat LTO the linker may fold
or relocate either payload, which is exactly what an LTO comparison is for; the
two symbols still survived on the authoring host.

`target/` is generated and is git-ignored; no build artifacts belong in this
corpus.
