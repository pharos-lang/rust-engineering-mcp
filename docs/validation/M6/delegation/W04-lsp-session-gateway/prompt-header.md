# W04 — Duplex LSP session in the gateway, `Phase::Analyzer`, M6 image admission, capture-time `rust-analyzer.toml` rejection, and native calibration against the real rust-analyzer 1.98.1

Model requested: Claude Opus 5 (`claude -p --model opus --effort high`). Role: implementation worker for the hardest boundary of M6-01. Orchestrator: Claude Fable 5.1 (decides; does not write code). You own the files listed under "Ownership" and nothing else. The MCP tool, the application port and public docs are a later package (W05); this package ends at the execution-adapter API plus its native calibration receipt.

## Read first (in this order)

1. `AGENTS.md`; `docs/validation/M6/delegation/D25-D26-decision-brief.md` (binding; §2 and §4 especially); `docs/adr/ADR-084-rust-analyzer-runtime-and-lsp-lifecycle.md`; `docs/adr/ADR-082-m6-runtime-provisioning.md`; `docs/adr/ADR-077-m5-runtime-admission.md` (admission pattern to copy); `docs/adr/ADR-008-execution-gateway.md`; `docs/adr/ADR-031-rust-source-transfer.md`.
2. The code you build on: `crates/execution-adapter/src/supervisor.rs` (the existing one-shot supervisor: nonblocking pipes, deadline, output limits, cancellation, `ChildGuard`), `rust_gateway.rs` (`Phase`, `program()/arguments()/environment()/user()/seccomp_profile_name()`, `arguments(name, nonce, volume, phase)`, `phase()`, `run_started_container()`, `execute_observed()`, the volume/ingest flow, `absent()`, cleanup and quarantine, `implementation_fingerprint()`), `rust_applied.rs::verify` (the container-config verifier that must learn the new phase), `lib.rs` (`APPROVED_*_IMAGE` constants, module list), `performance_port.rs`/`performance_native.rs` (how M5 gates a tool on one exact image and how native `#[ignore]` tests publish receipts), `lsp_codec.rs` and `rust_engineering_domain::analyzer` (delivered by W03 — consume them as they are; if a contract there is insufficient, extend it additively and say so in the report, never change existing semantics).
3. `crates/project-adapter/src/filesystem/macos/source.rs` (line ~39: how `.cargo/config*` is rejected at capture) and its tests in `crates/project-adapter/tests/source.rs`.
4. `docs/validation/M6/provisioning.json` (the M6 image: `sha256:f39a5b33ee7d54243664162eb635f8ec223d512042beb7cd18ecf071046b310c`, tag `rust-engineering-runtime:1.98.1-arm64-m6`, `rust-analyzer 1.98.1 (48a229c 2026-09-01)`).

## Deliverables

### D1 — `crates/execution-adapter/src/lsp_session.rs`: duplex bounded session

A second supervisor, not a modification of the first. `LspSession::open(command: Command, budget: SessionBudget, cancel: &dyn ExecutionCancellation) -> Result<Self, ExecutionError>` spawns the child with piped stdin/stdout/stderr (nonblocking, `ChildGuard`-style kill-on-drop), and offers:
- `send(&mut self, message: &lsp_codec::OutgoingMessage) -> Result<(), SessionError>` (frames via `lsp_codec::encode`, writes fully or fails; counts messages ≤ 4096 and bytes);
- `recv(&mut self, timeout: Duration) -> Result<Option<lsp_codec::RawMessage>, SessionError>` (polls stdout/stderr with `rustix` poll or the same mechanism `supervisor.rs` uses; feeds the `lsp_codec::Decoder`; returns one message at a time; `None` only on EOF; stderr is captured into a bounded 1 MiB buffer that is **never** returned to callers except as `stderr_bytes()` length + sha256 for the receipt; stdout total ≤ 16 MiB);
- `request(&mut self, method, params, timeout) -> Result<lsp_codec::Response, SessionError>`: allocates the id through `lsp_codec::Correlator`, sends, and loops on `recv` until the matching response, **answering** any server→client request with `-32601` and dropping/counting notifications except the ones the caller asked to observe (`serverStatus`) via a small `Observed` accumulator;
- `close(mut self, grace: Duration) -> SessionOutcome`: `shutdown` request (bounded), `exit` notification, wait up to `grace` for exit, else kill; returns `{ exit_code: Option<i32>, stop: Exited|Killed|Timeout|Cancelled|Eof, messages, bytes_in, bytes_out, stderr_len, stderr_sha256, fatal: Option<SessionError> }`.
- Every wait respects the overall deadline and the cancellation token; a fatal codec error (`is_fatal()`), the frame/message/byte limits, deadline or cancel → the child is killed immediately and the outcome says why. No `unwrap`/`expect`/`panic!`; no `unsafe`.
- Unit tests (`#[cfg(test)]`, spawning `/bin/cat` or `/bin/sh -c` **only inside tests**, which `scripts/check-architecture.py` permits for this crate): echo round trip; oversized frame from the peer → fatal + killed; message flood → `MessageLimit`; stderr flood bounded; peer that never answers → timeout + killed and joined; cancellation mid-request → killed and joined; server→client request answered with `-32601`; EOF without shutdown → `Eof`.

### D2 — `Phase::Analyzer` and `RustGateway::execute_analyzer`

- New `Phase::Analyzer` in `rust_gateway.rs`: program `/opt/analyzer/bin/rust-analyzer`, no arguments, user `65534:65534`, base `environment()` plus `RA_LOG=error`, seccomp `seccomp-rust.json`, no extra tmpfs, `/source` read-only. Teach `rust_applied::verify` the phase (entrypoint/limits unchanged otherwise). Add the new source files to `implementation_fingerprint()`.
- New file `crates/execution-adapter/src/analyzer_gateway.rs` with `pub fn execute(gateway: &RustGateway, source: &SourceBundle, query: &AnalyzerQuery, limits: ExecutionLimits, cancel: &dyn ExecutionCancellation) -> Result<AnalyzerExecution, ExecutionError>` and the `RustGateway::execute_analyzer` wrapper (same shape as `execute_nextest`). Flow, inside the existing `busy` lock and `WorkBudget`: preflight (verified/quarantine/executable digest/engine identity exactly as `execute_observed`), volume + ingest (existing phases), create the Analyzer container, verify its config, start it with `--attach --interactive` through `LspSession` (build the `Command` with `DockerGateway::command` like `run_started_container` does), then: `initialize` (params from `lsp_codec` — brief §4.3 capabilities, §4.5 `initializationOptions`, `rootUri: file:///source`, `workspaceFolders`), assert `capabilities.positionEncoding == "utf-8"` (else `ANALYZER_CAPABILITY_MISMATCH`), `initialized`, wait for `experimental/serverStatus{quiescent:true, health != error}` within the initialize budget (60 s max), `textDocument/didOpen` of the queried file with the exact captured bytes and `version: 1`, exactly one request per `AnalyzerQuery` variant (`DocumentSymbols{file}`, `WorkspaceSymbols{query}`, `References{file, position, include_declaration}`, `Diagnostics{file}` via `textDocument/diagnostic`, `CodeActions{file, range, only}`), convert through `lsp_codec` → domain, then `close()` with a 5 s grace, then kill/rm container, verify absence, remove volume, verify absence — cleanup joined before the `busy` guard drops (G3); uncertain cleanup quarantines exactly like the other phases. Define `AnalyzerQuery` and `AnalyzerExecution { identity: AnalyzerIdentity, readiness: Quiescent|NotReady{elapsed_ms}, result: AnalyzerResult (enum per query with domain values), completeness, session: SessionSummary, termination, oom_killed }` in `rust_engineering_domain::analyzer` (append-only additions to W03's file). `AnalyzerIdentity.version` must be the real `--version` line observed in this run? No — the version line is a property of the image; capture it once per gateway at admission verification (`Phase::Run(RustCommand::…)`-style closed argv `rust-analyzer --version` is NOT available: instead read `/usr/share/doc/rust-runtime/m6/rust-analyzer-version.txt` and `installed.json` through a closed `cat` phase like `RustCommand::InstalledComponents` does, or pin them as constants verified by the calibration test — choose, justify in the report, and make the calibration assert equality with the receipt `docs/validation/M6/provisioning.json`).
- Also expose `pub fn analyzer_config_digest() -> SourceFingerprint` (from `lsp_codec::config_digest()`).

### D3 — Admission of the M6 image

`pub const APPROVED_M6_IMAGE` = `sha256:f39a5b33ee7d54243664162eb635f8ec223d512042beb7cd18ecf071046b310c` in `analyzer_gateway.rs` re-exported from `lib.rs`; `RustGateway::new` accepts it (closed list, like `APPROVED_M5_IMAGE`); `execute_analyzer` refuses any other image with `ExecutionError::Unavailable` **before** creating a container (mirror `performance_port.rs` line ~100); `crates/mcp-server/src/host_config.rs` line ~176 accepts the digest. Write `docs/adr/ADR-085-m6-runtime-admission.md` in the exact shape of ADR-077, with the single line `**Digest admitido:** \`sha256:f39a5b33…\`` (full digest) and Status "Accepted para la admisión de la imagen; la calificación de las tools se registra aparte". Do not touch ADR-077 or the M5 constants.

### D4 — Capture-time rejection of `rust-analyzer.toml`

In `crates/project-adapter/src/filesystem/macos/source.rs`, reject at any depth any regular file, directory or other object whose name is `rust-analyzer.toml` or `.rust-analyzer.toml` (case-insensitive), with the same typed rejection path `.cargo/config*` uses (`OperationalErrorCode::…` — reuse the existing code the `.cargo` rejection maps to, or add `UnsupportedProjectConfig` only if that enum already distinguishes it; report which). Unit/integration tests in `crates/project-adapter/tests/source.rs`: workspace-root file, nested crate file, uppercase variant, dot-prefixed variant, and a directory of that name — all rejected; a file named `rust-analyzer.toml.bak` accepted. Update the doc comment of that function and ADR-031's "Initial source-transfer subset" paragraph is **not** yours: leave a one-line note in your report so the orchestrator schedules the ADR-031 amendment.

### D5 — Native calibration `crates/execution-adapter/src/analyzer_native.rs`

`#[cfg(test)]` module of `#[ignore]`d tests (pattern: `performance_native.rs`), each requiring `RUST_MCP_TEST_SOCKET` and the M6 image (`RUST_MCP_TEST_IMAGE` must equal `APPROVED_M6_IMAGE`; otherwise the test fails with a clear message, never skips), run one at a time with `--exact --ignored --test-threads=1`. Each publishes a JSON cut receipt under `target/m6-calibration/cut-<name>.json` and the module rebuilds `target/m6-calibration/receipt.json` (schema `rust-engineering-mcp.m6-calibration.v1`, image id, source hashes of `analyzer_gateway.rs`/`lsp_session.rs`/`lsp_codec.rs`/`analyzer.rs`, per-cut status, timings, counts). Cuts:
1. `m6_analyzer_refuses_every_runtime_but_the_m6_image` — M5 image → `Unavailable` before any container (cheap, first).
2. `m6_analyzer_version_and_config_schema_match_the_receipt` — closed `cat` of `/usr/share/doc/rust-runtime/m6/rust-analyzer-version.txt` and `installed.json` from the guest (or the mechanism you chose in D2) equals `docs/validation/M6/provisioning.json`; and a one-off `rust-analyzer --print-config-schema` run **as a calibration-only phase** (closed argv, not exposed to tools) whose JSON is archived (sha256 + full text under `target/m6-calibration/config-schema.json`) and in which **every** key of the fixed `initializationOptions` (brief §4.5) exists — a missing key fails the cut (rust-analyzer ignores unknown keys silently).
3. `m6_document_symbols_on_valid_basic_negotiate_utf8_and_reach_quiescent` — `fixtures/valid-basic`: `positionEncoding == "utf-8"`, `serverStatus` transcript with `quiescent: true` and its elapsed ms, `health` recorded, document symbols of `src/lib.rs` (or `main.rs`) non-empty with correct 1-based Unicode-scalar positions checked against the captured bytes; session closed with `exit_code == Some(0)`; cleanup verified.
4. `m6_initialize_spawns_only_the_expected_guest_programs` — during initialize, sample `docker container top <name> -eo pid,ppid,args` repeatedly (reuse `detached_observation`'s mechanism); the set of program basenames observed must be ⊆ {`rust-analyzer`, `rustc`, `cargo`} and must never contain `build-script-build`, `proc-macro-srv`, `rustfmt`, `sh`; record the observed argv list in the receipt. Use `fixtures/build-script` (it has a real `build.rs`) to make the negative meaningful.
5. `m6_hostile_rust_analyzer_toml_is_rejected_before_any_container` — a temporary copy of `fixtures/valid-basic` plus a `rust-analyzer.toml` that sets `cargo.buildScripts.enable = true` and `check.overrideCommand`; capture through the real project adapter must reject it; assert no volume/container was created (inventory of labelled objects empty).
6. `m6_never_ready_times_out_with_joined_cleanup` — an initialize budget of 1 s against a real project large enough not to be quiescent in 1 s (`fixtures/workspace` or the repository's own `crates/domain` copied as a fixture if allowed by ADR-031 limits — pick one that reproducibly needs > 1 s); result `NotReady`, no data, container and volume absent afterwards, `busy` released.
7. `m6_cancellation_during_initialize_kills_and_joins` — cancel token flips after `initialize` is sent; `Cancelled`; absence verified; a second call on the same gateway succeeds (not quarantined).
8. `m6_analyzer_crash_mid_session_is_reported_not_masked` — after `initialized`, `docker kill --signal=KILL <container>` from the test; result `ANALYZER_CRASHED`-class (`unavailable`), `exit_code` recorded, cleanup verified.
9. `m6_frame_limit_from_a_real_peer_kills_the_session` — optional if achievable with the real binary (e.g. a `workspace/symbol` query whose answer exceeds 1 MiB on a large fixture); otherwise document why it stays a unit test in D1 and mark the cut `not_run` with the reason (never `pass`).

After the cuts pass, copy `target/m6-calibration/receipt.json` to `docs/validation/M6/01-calibration.json` and `config-schema.json` to `docs/validation/M6/01-config-schema.json`, and write `docs/validation/M6/01.md` (what was measured, commands, image, timings, observed programs, residual risks).

### D6 — `scripts/test-m6-runtime.py` (native suite driver) and gate wiring

Mirror `scripts/test-m5-runtime.py`: discovers the `#[ignore]` selections of `analyzer_native.rs`, runs the admission-refusal cut first, then each selection with `--exact --ignored --nocapture --test-threads=1` (features: none needed unless you need `test-hooks`; say so), checks the receipt digests, writes `target/m6-runtime-gate/receipt.json`. Add `run('m6-runtime', …)` to the **full** mode of `scripts/gate.py` right after `m5-runtime`, and one sentence in `docs/ci.md` (M6 section) naming the stage; the full count becomes 41 — update the number you find there. Add `scripts/test-m6-runtime.py` to `sonar.coverage.exclusions` (host/Docker-only) and, if you add `scripts/test-m6-runtime-unit.py`, to the SonarCloud coverage list.

## Verification you must run (targeted; the workspace-wide gate is the orchestrator's)

```text
cargo fmt --all -- --check
cargo check -p rust-engineering-domain -p rust-engineering-execution -p rust-engineering-project -p rust-engineering-mcp --all-targets --locked --offline
cargo clippy -p rust-engineering-domain -p rust-engineering-execution -p rust-engineering-project -p rust-engineering-mcp --all-targets --locked --offline -- -D warnings
cargo test -p rust-engineering-execution --locked --offline --lib lsp_session
cargo test -p rust-engineering-execution --locked --offline --lib analyzer_gateway
cargo test -p rust-engineering-project --locked --offline --test source
cargo test -p rust-engineering-mcp --locked --offline host_config
python3 -B scripts/check-architecture.py
RUST_MCP_TEST_SOCKET=<the Docker socket the M5 receipts used; find it in docs/validation/M5/runtime.json> RUST_MCP_TEST_IMAGE=sha256:f39a5b33ee7d54243664162eb635f8ec223d512042beb7cd18ecf071046b310c python3 -B scripts/test-m6-runtime.py
python3 -B scripts/test-gate-reporting.py
python3 -B scripts/docs-hygiene.py links-check
```

Do not run `cargo test --workspace`, `gate.py core` or `gate.py full`. Do not run other native suites (M2–M5).

## Ownership (touch nothing else)

`crates/execution-adapter/src/{lsp_session.rs,analyzer_gateway.rs,analyzer_native.rs}` (new), `crates/execution-adapter/src/{lib.rs,rust_gateway.rs,rust_applied.rs}` (minimal, additive edits), append-only additions to `crates/domain/src/analyzer.rs` and, if strictly needed, additive helpers in `crates/execution-adapter/src/lsp_codec.rs`; `crates/project-adapter/src/filesystem/macos/source.rs` + `crates/project-adapter/tests/source.rs`; `crates/mcp-server/src/host_config.rs` (one digest); `docs/adr/ADR-085-m6-runtime-admission.md`; `docs/validation/M6/{01-calibration.json,01-config-schema.json,01.md}`; `scripts/test-m6-runtime.py` (+ optional unit test), `scripts/gate.py` (one `run`), `sonar-project.properties`, `.github/workflows/sonarcloud.yml`, `docs/ci.md` (M6 section + full count). No `Cargo.toml` changes, no new dependencies, no changes to existing phases' semantics, no MCP tool. Do not commit.

## Constraints

- Everything the brief §2.4 budgets say is enforced in code with the unit and phase they name; a limit without a test of its excess is not done.
- Kill-tree/cleanup discipline is the existing one: never release `busy` before absence is verified; uncertain → quarantine.
- No `unwrap`/`expect`/`panic!`/`unsafe`; no `serde_json::Value` outside the execution adapter; doc comments state contracts and why.
- Honesty: a cut that cannot be made to pass is reported as failed/not_run with the reproducible condition; never weaken an oracle to pass. If a real-binary behaviour contradicts the brief (e.g. utf-8 not negotiated, `serverStatus` never quiescent, unexpected programs), stop that cut and report it as a finding for the orchestrator — do not redesign.

## Report (mandatory headings)

Task / Result / Files changed / Tests executed (with counts and the native receipt path) / Evidence (image id, `--version`, config-schema sha256, programs observed, timings) / Risks / Decisions / Open issues.

## Addendum (2026-09-12, after W03b)

- W03b changed two codec signatures you will call: `lsp_codec::encode` returns `Result<Vec<u8>, domain::AnalyzerError>`; `code_actions_to_candidates` takes `Vec<serde_json::Value>` (raw result elements) so one malformed element is rejected alone. `domain::ActionRejection::FileNotInSnapshot` exists; `domain::MAX_SYMBOL_DEPTH = 32`; the second value of `document_symbols_to_domain` counts both visible-cap and depth-cap omissions. `Decoder` is poisoned after a fatal error. `workspace_symbols_to_domain` exists.
- The `initialize` client capabilities now include `textDocument.codeAction` literal support (see ADR-084 §4); your calibration must confirm `textDocument/codeAction` returns `CodeAction` literals, not bare `Command`s, on a fixture with at least one quickfix.
- Process rule: you may not spawn subagents (the launcher disables the Agent/Task tools). Do the work in this session.
