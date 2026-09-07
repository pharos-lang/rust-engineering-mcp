# Independent Review — ADR-062 (Coverage accounting and SemVer baselines)

## 1. Verdict: **Revise**

Multiple P1 findings on contract soundness and one P1 contradiction with a sibling M3 ADR.

## 2. Findings

| ID | Sev | Location | Claim | Why wrong/risky | Fix |
|---|---|---|---|---|---|
| F1 | **P1** | ADR-062 §11 (SemVer report fields) | Per-finding fields (`item`, `lint`, `required_update`, `level`, `span`, `limitations`) are reported as if extractable from a machine-readable cargo-semver-checks output. | The officially fetched `sources/semver-readme.txt` documents **no JSON/machine-readable output flag for findings** anywhere (verified by full-text grep for `json`/`output-format`/`machine-readable`: the only JSON mentions are rustdoc's own *input* JSON and `--baseline-rustdoc <JSON_PATH>`, an input flag). The independently fetched `obi1kenobi-cargo-semver-checks-Cargo.toml.txt` shows `handlebars` (human-readable templated terminal reports) and no clap subcommand/feature suggesting a findings-JSON mode. The companion Gemini research report (`R01-plugins/report.md`) explicitly labels `--output-format json` **"unverified (from training data)"** for exactly this reason. §11's entire schema is therefore built on an unconfirmed premise: either the fields must be scraped from colored/human terminal text (fragile, ANSI-sensitive, no oracle defined anywhere) or the flag doesn't exist in v0.50.0 and the design needs a different extraction plan (e.g., trustfall/library embedding, which is a materially different — and much larger — integration than "close argv + parse output"). | Before accepting §11, confirm (against the pinned v0.50.0 binary/source, not training data) whether a machine-readable findings format exists. If not, ADR-062 must specify a bounded, calibrated text-output parser with its own discriminating oracle (or drop per-finding structured fields to a coarser summary), and this must move from "Decision" to an explicit Open Issue rather than an assumed capability. |
| F2 | **P1** | ADR-062 §8, Consequences (roundtrip with `crates/execution-adapter/src/rust_gateway.rs`) | "No new gateway, image, or execution model is introduced"; baseline is "mounted read-only exactly like the candidate" via "the same tar-based `Phase::Ingest` mechanism." | Verified against the actual code: `RustGateway::execute`/`execute_observed` (rust_gateway.rs:719-904) accepts exactly **one** `source: &SourceBundle`, creates **one** Docker volume, runs **one** `Phase::Ingest` extracting to the hardcoded `--directory=/source`, and `arguments()` emits exactly **one** `--mount=type=volume,...,target=/source...` per container (same one-volume-one-mount shape in `mutation_gateway.rs`'s parallel `MutationPhase`/`create_arguments`). Running `cargo semver-checks --baseline-root /baseline` against a candidate at `/source` requires **two** simultaneously-mounted volumes in one container plus a second ingest phase populating `/baseline` — none of which the current `Phase`/`PhaseRequest`/`Volume` abstraction supports. This is a real extension of the closed execution seam (new method signature, second volume/ingest phase, multi-mount `arguments()`), not a "no new execution model" reuse, and it isn't listed in Open Issues despite AGENTS.md requiring an ADR for architecture changes. | Add an explicit Open Issue (or a short companion decision) describing the dual-source-mount/ingest extension to `RustGateway`, and correct the Consequences claim. |
| F3 | **P1** | ADR-062 §4 vs ADR-061 (content/sensitivity policy for `kind: Html`) | HTML coverage reports are packaged into a single tar blob before storage (§4), to be retained under ADR-061's artifact-store model. | ADR-061's `kind: Html` content policy assumes the blob bytes **are** HTML markup: "HTML and SVG are attachments... any optional preview is an inert derived summary from a bounded tokenizer that rejects scripts, event handlers, `javascript:` and remote-loading URLs" and its M3-01 oracle table tests `<script>`/external-URI rejection directly against the bytes. A tar archive is not tokenizable HTML — ADR-061's sanitizer/preview pipeline cannot run on it, and its `mime_type: registered/closed MIME enum` field has no defined value for "tar-packaged HTML bundle." Neither ADR resolves this combination. | Either (a) ADR-062 retains only a single representative HTML page (no tar) under the existing `kind: Html` contract, or (b) ADR-061 gains a distinct `kind` (e.g. `HtmlBundleTar`) with its own content policy (always `application/octet-stream`, never previewed/tokenized) before either ADR is accepted. |
| F4 | **P1/P2** | ADR-062 §2 and §4 (JSON authoritative + "JSON is the only format eligible for structured content") | Full `llvm-cov` JSON export is "authoritative" and eligible for MCP structured content, while spec §26.2 (quoted in Context) requires HTML — but not JSON — to be artifact-only. | The roadmap fixes a **512 KiB complete-MCP-response** budget (`m3-quality.md` "512 KiB MCP completo por respuesta", reused verbatim in ADR-040, ADR-060, ADR-061). Raw `llvm-cov --json` export (non-`--summary-only`) includes per-region/per-function detail and can trivially exceed that for any real workspace. §2's "domain metrics extracted per file/package/aggregate" reads like the intended bounded summary, but §4's sentence conflates "retained as JSON artifact" with "eligible for structured content," and neither section states a cap/pagination policy for the per-file metric list itself (which can also blow past 512 KiB on a workspace with many files) — unlike ADR-061, which explicitly paginates its job-index Resource. | ADR-062 must explicitly split: (1) full raw JSON → artifact only, retrieved via Resource, never inlined; (2) a bounded/paginated domain summary → MCP structured content, with an explicit truncation/pagination oracle for large file counts. |
| F5 | **P2** | ADR-062 §8 (dual read-only mounts) | Both `/source` and `/baseline` are mounted read-only; no writable location is discussed for rustdoc JSON generation. | `cargo-semver-checks` generates rustdoc JSON via `cargo doc`-style compilation for **both** the baseline and candidate manifests, which requires a writable target/output location for each. Neither the fetched README nor the ADR states whether this respects `CARGO_TARGET_DIR` (already redirected to the writable `/work/target` in `rust_gateway::environment()`) for a manifest rooted outside `/source`, or whether it defaults to `<baseline_root>/target` inside the read-only `/baseline` mount, which would fail outright. This is unverified from official sources (the README has no CLI/env-var reference table, unlike llvm-cov's). | Add this as an explicit Open Issue requiring verification against the pinned binary before committing to "both roots read-only." |
| F6 | **P2** | ADR-062 §9/§10 (exit-code calibration) | The fixture list covers exit 0 (no findings), exit 100 (breaking), and several `Unavailable`/`101` paths, but omits (a) exit 0 with **warn-level-only** findings, and (b) a defensive check for exit 100 observed with an **empty** findings list. | Per the fetched README, `warn`-level findings never affect exit status, so a warn-only run also exits 0 — nothing in §10 confirms such findings are still surfaced (not silently dropped) under a `Passed`/`no_break` verdict. Symmetrically, §9's table trusts exit 100 as `Failed`/`breaking` unconditionally; if it is ever observed with zero parsed findings, the ADR's own "unknown/partial/unavailable/skip never equal pass" philosophy should apply in the opposite direction too (don't report a failure with no backing evidence) — this case has no `Blocked` fallback. This is precisely the discriminating-oracle gap the review brief flags. | Add both cases to §10's fixture list; add explicit `Blocked` handling for exit 100 with an empty/unparseable findings list. |
| F7 | P3 | ADR-062 §8 (git neutralization, already an Open Issue) | Mechanism left unspecified ("via environment variable, guest filesystem layout, or both"). | `rust_gateway::environment()` uses an explicit allowlist (no `GIT_DIR` today), so absence alone doesn't stop the tool's own upward filesystem walk from cwd (`/source`); the actual mitigation is contingent on no `.git` existing anywhere on the container's root filesystem, which is asserted but not calibrated. | Recommend committing now to setting `GIT_DIR` to a fixed nonexistent guest path as the deterministic mechanism, rather than deferring the choice entirely. |

## 3. Files read (for the orchestrator to hash)

- `docs/adr/ADR-062-coverage-accounting-and-semver-baselines.md`
- `docs/adr/ADR-061-private-quality-artifact-store.md`
- `docs/adr/ADR-060-bounded-job-execution-and-mcp-tasks.md`
- `docs/adr/ADR-031-rust-source-transfer.md`
- `docs/adr/ADR-033-toolchain-inspection.md`
- `docs/adr/ADR-040-single-capture-quality-gate.md` (partial, grep)
- `AGENTS.md`
- `docs/roadmap/m3-quality.md`
- `docs/roadmap/adr-backlog-m2-m8.md` (D18 section only, grep)
- `docs/security-model.md`
- `crates/execution-adapter/src/rust_gateway.rs`
- `crates/execution-adapter/src/mutation_gateway.rs`
- `crates/domain/src/rust_execution.rs` (enum only, grep)
- `crates/domain/src/artifact.rs`
- `docs/validation/m3-delegation/R01-plugins/report.md`
- `.../sources/index.txt`
- `.../sources/llvmcov-readme.txt`
- `.../sources/semver-readme.txt`
- `.../sources/obi1kenobi-cargo-semver-checks-Cargo.toml.txt`

## 4. Contradictions with normative sources / other M3 ADRs

- **F3** above: ADR-062 §4's tar-packaged HTML vs ADR-061's HTML-tokenizer content policy — genuine, unresolved.
- **F2** above: ADR-062 Consequences ("No new gateway... execution model") contradicts the actual single-volume/single-mount `RustGateway` implementation and is not reconciled by ADR-060 either (ADR-060 treats the gateway as an opaque "already closed joined execution boundary" without addressing multi-source jobs).
- Both ADR-061 and ADR-062 independently restate the 512 KiB MCP response budget (consistent), but only ADR-061 defines a pagination/oracle discipline for large result sets — ADR-062 should adopt the same discipline for coverage's per-file metrics (F4).

## 5. Missing decisions

- How SemVer per-finding structured data is actually extracted from `cargo-semver-checks` (F1) — this is the single largest open question for M3-04 feasibility.
- Where rustdoc JSON/build artifacts are written for the baseline vs. candidate roots (F5).
- Whether `report --json --lcov --html` can be invoked as three separate `report` calls against one profdata, or must be one combined invocation — not blocking, but worth stating explicitly in a closed argv design.
- The tar-vs-single-file HTML representation for the artifact store (F3) must be decided jointly with ADR-061 before either is accepted.

## 6. Limitations of this review

- Read-only, static review: no code was executed, no fixture was run, and the pinned `cargo-semver-checks` v0.50.0 / `cargo-llvm-cov` v0.9.0 binaries were not invoked to confirm actual CLI behavior (output formats, exit codes, target-dir handling). F1 and F5 are inferred from the absence of documentation in the fetched official README/Cargo.toml, which is strong but not conclusive evidence that the capability doesn't exist (it could be undocumented or discoverable only via `--help`).
- I did not fully read ADR-032, ADR-037, or the complete ADR-040 text, relying on targeted greps for specific quoted claims; I did not review `crates/domain/src/evidence.rs`, `value.rs`, or `quality.rs` in full to verify every cross-reference in ADR-062's Context section beyond the ones directly load-bearing for the findings above.
- I did not review `releases-summary.json` or `rust-llvm-tools-sha.txt` byte-for-byte; version/hash citations in ADR-062 were spot-checked against `index.txt` and the two READMEs only.

## 7. Proposed disposition

- F1 (semver JSON output unconfirmed): **fix now** — blocking for M3-04 feasibility; must be resolved before §11 is treated as decided.
- F2 (dual-mount gateway gap): **fix now** — Consequences claim is factually wrong and the architecture gap needs at least an Open Issue before acceptance.
- F3 (HTML tar vs ADR-061): **fix now** — direct contradiction between two co-proposed ADRs; must be reconciled before either is accepted.
- F4 (JSON/512 KiB conflation): **fix now** — contract-level ambiguity with a concrete overflow failure mode.
- F5 (writable target dir): **defer with justification** — acceptable as an explicit Open Issue if stated, since it's an implementation-time verification, but currently entirely unstated.
- F6 (exit-code oracle gaps): **fix now** — cheap to add to §10's fixture list, directly requested by the review brief.
- F7 (git neutralization determinism): **defer with justification** — already an acknowledged Open Issue; recommendation only.