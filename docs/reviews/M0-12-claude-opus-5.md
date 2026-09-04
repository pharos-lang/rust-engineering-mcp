# M0-12 — Independent foundation closure review

Reviewer: Claude Opus5, High, Claude Code2.1.259; read-only restricted CLI with no
tools/MCP. Session `5d6f0792-ff92-4003-9e01-0bce212d8475`; successful result,
modelUsage confirms claude-opus-5 (auxiliary Haiku telemetry also present).
Scope: supplied critical contracts/application/artifact code, gate scripts,
ADR-025/027/028/029, unit evidence M0-07..11 and current scope documentation.
This is bounded review, not a second full audit of filesystem/gateway/SQLite.

## Principal Engineer disposition

The reviewer found no P0 or demonstrated safety bug blocking M0. Four P1 items
were conditional verifications/configuration improvements, resolved as follows.
This record is not a claim of unconditional external approval or no findings.

- P1-1: root Cargo.toml already sets jsonschema default-features=false. deny.toml
  now additionally forbids resolve-http/resolve-file; final all-features deny gate
  verifies the actual graph. No runtime schema fetching is introduced.
- P1-2: inspected scripts/test-execution.sh: set -eu, required socket, fixed approved
  absolute Docker Desktop binary, private mktemp state root, reviewed offline Go
  image build, freshly inspected immutable ID. Missing inputs/build/test/capabilities
  failures exit nonzero; no skip. All needed environment keys survive gate filtering.
  docs/ci.md explicitly distinguishes these development choices from the product
  CLI's four host-selected inputs. The final gate includes fresh live capabilities.
- P1-3: inspected workspace manifests, LICENSE, README, CHANGELOG, SECURITY, tools and
  board. Product license is undecided and no grant/release is claimed; publish=false.
  Upstream vendored license files are not a product license. Only project.open is
  available. Added explicit pending license/redistribution row to compatibility.
- P1-4: M1 prerequisites now require each tool's closed nested DTOs, committed schema
  snapshot, status/isError review, fixed non-reflecting errors and full response budget.

P2 disposition:

- Lexical budget: the suggested unbounded production lexical path is not present.
  catalog-adapter/src/lib.rs::lexical serializes summaries and rejects JSON over
  MAX_SEARCH_PAYLOAD_BYTES=128*1024. search_catalog propagates that error. The hybrid
  estimate is conservative, so fallback may return the smaller already-bounded
  lexical page. Still, total MCP metadata/envelope budget is an explicit M1 obligation.
- Reduced duplicate gateway/capability prose in compatibility to scope plus links;
  added a distinct contracts heading in security-model and compatibility.
- Evidence includes actual audit JSON, capabilities JSON, toolchain identity in the
  full report, source hashes and machine-readable per-stage counts. Maintenance and
  duplicate warnings remain visible; neither is misreported as a vulnerability.
- deny now denies registry wildcards while allowing unpublished path dependencies.
  quick-xml@0.26.0 is a specific regression guard; other versions are still subject to
  audit/advisories, not exempted. Official configuration reference:
  https://embarkstudios.github.io/cargo-deny/checks/bans/cfg.html
- Semantic harness now runs --ignored for every enumerated test binary, including
  binaries with zero currently ignored tests, avoiding a future silent omission.
- ProjectTool::new constructs Contract once at server startup (stdio.rs and
  stdio/project.rs); test-only constructions are separate. The fixture harness
  intentionally runs reviewed benign build.rs under host authority; the adversarial
  source is excluded. security-model now states that distinction literally.

Validation after these changes: full gate and artifacts in ../validation/M0-12.md.
No M1 code was implemented. Remaining release and containment obligations are recorded
in ../m1-prerequisites.md, not represented as completed by this review.

## Reviewer output (verbatim)

# M0 Closure Review — Independent, Read-Only

Scope: safety/contract boundaries, evidence integrity, and M0-vs-M1 scope guard, against the supplied code (`contract.rs`, `semantic.rs`, `artifact-adapter/src/lib.rs`, `gate.py`, `test-semantic.py`, `deny.toml`), ADR-025/027/028/029, validation M0-07…M0-11, and the doc set. No files were opened and no commands run.

## Verdict

No P0. I found no safety bug that should block M0 closure. Four P1 items are cheap verifications/config fixes that make already-written claims enforceable rather than conventional. The remaining items are P2 hygiene and evidence completeness.

## P0 — none

What I actually verified rather than assumed:

- **`artifact-adapter::redact` boundary math is sound.** `keep = max_secret_len - 1` is provably sufficient: any full match starting at `p < safe` is entirely contained in `pending` (since `safe = len - maxlen + 1` and any secret has `L ≤ maxlen`), so no complete match can be emitted unmasked across a chunk boundary. `emit`'s `count.min(cap - output.len())` cannot underflow because both call sites are guarded by `output.len() < output_bytes`. The final suffix loop's `output.len() - len` cannot underflow because `len ≤ tail.len() ≤ emitted`. `tail` correctly tracks the last `keep` *original* bytes in both `count >= keep` and `count < keep` branches, including after an output-cap clamp.
- **Quota reservation is race-free.** `admit()` reserves `output_bytes` before `redact` consumes the stream; `&mut self` excludes a second writer, `expire()` can only free, and stored content ≤ `output_bytes`, so the post-capture insert cannot exceed a quota validated pre-capture. Failure paths never insert.
- **`InvalidSourceCount`** correctly rejects a lying `read` count; a hostile `ArtifactInput` cannot write past the `capacity`-length slice in safe Rust.
- **`contract.rs` leaks nothing.** `invalid_input()`/`internal()` are fixed strings with `data: None`; schema-invalid output maps to `INTERNAL_ERROR` rather than crossing the edge; `Failed` → `isError=false` matches AGENTS.md.
- **`search_hybrid` degrades, never fails open.** Every semantic defect (identity mismatch, non-finite/negative distance, duplicate/oversized/non-ASCII crate name, `summary()` returning `None`, over-limit candidate count) returns the lexical page with a declared `fallback`; only a genuine `CatalogError` propagates. Facts are rehydrated exclusively from SQLite.
- **`gate.py` fails closed** on Windows, non-macOS-ARM64 full mode, missing full-gate env, missing `rustup`/`cargo-audit`/`cargo-deny`, toolchain version mismatch, and any nonzero step; `PYTHONOPTIMIZE` is stripped by the env allowlist so subprocess assertion checks cannot be optimized away.

## P1 — resolve or explicitly verify before writing the M0 Done row

**P1-1 — `jsonschema` resolver claim is documented but not gate-enforced.**
`docs/compatibility.md` states "jsonschema 0.53.0 sin resolvers HTTP/file. El runtime no descarga schemas." `crates/mcp-server/src/stdio/contract.rs::Contract::new` calls `jsonschema::validator_for` with default options, so the claim rests entirely on the manifest's feature selection. `deny.toml` bans `ort-sys` and `lancedb` features but has no `jsonschema` entry. Verify the dependency is declared `default-features = false` (or without `resolve-http`/`resolve-file`) and add:
```toml
[[bans.features]]
crate = "jsonschema"
deny = ["resolve-http", "resolve-file"]
```
This is the same pattern already used for the other two crates and turns a prose claim into a gate.

**P1-2 — full-gate Docker preconditions vs. ADR-025's host-selected inputs.**
`scripts/gate.py` requires only `RUST_MCP_TEST_SOCKET` for full mode, and its env allowlist (`env={k:v for k,v in os.environ.items() if k in [...]}`) contains exactly one Docker-related key. ADR-025 requires a host-selected absolute Docker executable, socket, state root and immutable image ID. Confirm before closure that (a) `scripts/test-execution.sh` cannot skip or no-op when Docker/image is absent (no `command -v docker || exit 0` path — a skip here would be a silent false pass in the one step that certifies containment), and (b) it does not depend on any env var stripped by the allowlist, or, if it hardcodes paths/image ID, that this divergence from "host-selected" is stated. If it needs additional keys, add them to both the allowlist and the full-mode required-key loop.

**P1-3 — public-claim sweep on the files not supplied to this review.**
ADR-029 states licensing is a pending M1 gate and M0 "does not pretend `deny licenses` has passed"; `gate.py` correctly omits the `licenses` check. Before closure, confirm nothing contradicts that or over-advertises: `Cargo.toml [package] license`/`license-file` and any `LICENSE*` file; `README.md`, `CHANGELOG.md`, `SECURITY.md`, `docs/tools.md` listing only `rust.project.open` as available and the other twelve as unimplemented; `docs/implementation-status.md` Done rows pointing at real artifacts. Also add an explicit row to `docs/compatibility.md` recording "license/redistribution approval: pending, M1 release gate" — the table currently has rows for package version, toolchain, targets and sandbox but is silent on licensing, and silence is the one failure mode the rest of that table avoids.

**P1-4 — the M1 scope guard has no item for the tool-contract surface.**
`docs/m1-prerequisites.md` covers gateway/Rust image, project.open, build.rs adversaries, MCP workers, artifacts, catalog CLI, index, distribution, RustSec and release gates — but no item covers adding twelve tool contracts. This matters concretely because `contract.rs::closed_object` validates closure **only at the schema root** (the comment says so: nested closure "belongs to each DTO and its schema snapshot"). With one tool and one snapshot test that is fine; with thirteen it is a per-DTO obligation with no automated backstop. Add an item requiring, per new tool: closed nested DTO schemas, a committed schema snapshot test, `ToolStatus`→`isError` mapping review, and fixed non-reflecting error messages.

## P2

- **Budget asymmetry in `application/src/semantic.rs::search_hybrid`.** The 128 KiB `bound` check only guards the *hybrid* path; on `fallback(SemanticError::Budget)` the identical oversized `lexical` page is returned, and `search_catalog` alone is never bounded. The guard therefore degrades the mode without bounding the response. Move the bound to the lexical page (or to the M1 adapter edge) before `rust.crate.search` is exposed; note it in `m1-prerequisites.md` item 7/8.
- **Doc duplication invites drift.** The "Gateway M0-05", "Detección activa M0-06" and the M0-07 contract paragraph appear near-verbatim in both `docs/security-model.md` and `docs/compatibility.md` (and partially in `docs/architecture.md`), and in both files the M0-07 paragraph sits *inside* the M0-06 section. Since AGENTS.md requires these three to stay synchronized, prefer one authoritative section plus cross-links.
- **Gate report is thin as closure evidence.** `gate.py`'s `run()` records name/command/status/exit code/seconds only — no test counts, and the toolchain/tool preflight is not a recorded step. `audit` runs as `cargo audit --no-fetch` (no `--json`, unlike the M0-09 evidence run), so the `paste`/RUSTSEC-2024-0436 warning and any future unmaintained-warning drift are invisible in the artifact and never fail the gate. Persist the audit JSON as an artifact and record per-step counts, so `docs/validation/M0-12.md` is substantiated by the machine-readable report rather than by a transcript.
- **`deny.toml [bans] wildcards = "allow"`** is broader than its stated intent ("internal unpublished path crates"). `wildcards = "deny"` with `allow-wildcard-paths = true` expresses that exactly. Relatedly, `deny = [... "quick-xml@0.26.0"]` pins one version; a different vulnerable minor would fall through bans to advisories only.
- **`scripts/test-semantic.py` runs `--ignored` only for the target named `local`.** Any ignored test added to another semantic binary is silently never executed and never asserted-absent; the `binaries` loop already enumerates targets, so the `--ignored` pass could apply to all of them.
- **Two cheap verifications:** that `Contract::new()` is built once at startup (its own test is named `rejects_non_object_contracts_at_startup`) rather than per `call_tool` — it compiles two `jsonschema::Validator`s; and that `scripts/test-fixtures.py`'s 11 Cargo cases use non-building subcommands or fixtures with no `build.rs`/proc-macro, so `security-model.md`'s statement about harness processes stays literally true for the core gate.

## Acknowledged M1 gates and policy choices — not defects

Confirmed correctly scoped and documented; no action for M0: memory-only artifacts with no disk-permission claim and permanent clock poisoning (ADR-028); conservative over-redaction of partial secret prefixes and exact-cap truncation (ADR-028, security-model.md); synchronous `search_catalog`/`embed_query` called from `async fn search_hybrid` with the bounded-worker obligation deferred (module doc, architecture.md, ADR-027); lexical-first merge explicitly not a ranking (ADR-027, prerequisites item 8); `unmaintained = "workspace"` leaving transitive `paste` as a visible audit warning with `ignore = []` (ADR-027/029); `(allow default)(deny network*)` described as calibrated network deny, not a strict tier; Docker/Linux ARM64 Go probes never described as a Rust runner; `cargo deny --all-features` covering the `local` graph that the core build never compiles (ADR-029 explicitly: core alone cannot close M0). The vendored lancedb sits outside `[sources]` coverage but is covered by the `vendor` step's byte-level verification, which the gate runs in both modes.

**Scope guard holds.** Only `rust.project.open` is announced (contract test comment, compatibility wire matrix); catalog, semantic and artifact APIs are internal ports with no tool registration; the thirteen-tool list and `rust.dependencies.inspect`'s exclusion in `m1-prerequisites.md` match AGENTS.md; M2 absent. No M1 implementation has leaked.

## For the M0-12 record

- Expected full-gate total **185 = 173 core + 1 doctest + 7 unique semantic + 4 Docker** reconciles with M0-10a, M0-09 and M0-07. Note in the evidence that the semantic step's transcript will show **8** executed (7 unit + 1 `--ignored` E5 integration), of which the bundle-rejection test overlaps the core suite — so 185 unique, not 186. Any delta from 185 must be explained, not rounded.
- Attach the fresh `capabilities` JSON produced by the full run; ADR-025 forbids a saved report as runtime authority, but closure needs the *live* one from this gate.
- `docs/m1-prerequisites.md` and the modified `docs/implementation-status.md` are still untracked/uncommitted. AGENTS.md makes the board the repo-visible artifact — both must be committed for the Done row to mean anything.
- Record honestly that the full gate certifies macOS 26 ARM64 native + Docker Linux ARM64 probe containment only, with license, native Linux/Windows/x86_64, third-party MCP clients, benchmarks and Cargo-under-sandbox remaining M1 RC gates.

## Limits of this review

Bounded by design and by the read-only constraint: I did not re-review the M0-04 no-follow adapter, the M0-05/06 gateway internals, the SQLite adapter or the semantic adapter beyond their contracts here. `scripts/test-execution.sh`, `check-architecture.py`, `verify-vendor.py`, `test-fixtures.py`, the workspace manifests, `README/CHANGELOG/SECURITY/tools.md`, `implementation-status.md` and the running full-gate output were not available to me, so P1-2, P1-3 and two P2 items are stated as verifications rather than confirmed defects. I did not execute the gate or observe its results.
