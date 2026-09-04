# Revisión externa M0-03

Fecha: 2026-09-03. Commit revisado: `0d6dfc0a95493474e41268170000893740443400`.
Claude Code 2.1.259, reviewer explícito `claude-sonnet-5`, effort `medium`.
Respuesta `success`, `is_error=false`, un turno, 347777 ms.

Invocación read-only con contexto por stdin y sin herramientas:

```text
claude --print --model claude-sonnet-5 --effort medium --safe-mode --restricted --strict-mcp-config --tools '' --permission-mode dontAsk --permission-prompts none --no-session-persistence --output-format json
```

Paquete: objetivo/DoD, límites y gate M0-03, ADR-002/003/004/009/012/015,
diff completo `de3fd42..0d6dfc0`, tests, documentación, ADR-023 y Cargo.lock.
La fuente de rmcp no se adjuntó; el reviewer señaló esa limitación. No ejecutó
tools, tests ni modificaciones. La verificación independiente de API y las
resoluciones del principal están en [evidencia](../validation/M0-03.md).

SHA-256 del paquete: `cef22a05283c6164a62f1190b563c53c011b2c842fe3a0a9ea140cd44a64c7e8`.
La metadata confirma `claude-sonnet-5` (29217 thinking tokens reportados) y una
llamada auxiliar del CLI a `claude-haiku-4-5-20251001` (17 tokens de salida).
No se configuró fallback ni ultracode. Los flags no acreditan sandbox OS del CLI.

## Respuesta completa

# Independent Review — M0-03 (MCP stdio bootstrap)

Reviewed statically from the diff/lockfile only (no build/test execution, no access to rmcp 3.2.0 source). Findings are scoped to what the diff itself shows.

## Critical
None found.

## High
None found. The adapter is narrowly scoped, doesn't touch domain/application, and the byte-budget/EOF logic is internally consistent with the test matrix described.

## Medium

**1. `on_custom_request` / rmcp API surface is unverifiable in this review**
`crates/mcp-server/src/stdio.rs` — `ServerHandler::on_custom_request`, `ServerInfo::new(...).with_server_info(...)`, `supported_protocol_versions`, `serve_server_with_ct`, `ServerInitializeError::ConnectionClosed`.
The DoD states "API choices verified against downloaded pinned SDK official source," but that verification was performed by the implementer, not by this review — I have no access to rmcp 3.2.0 source in this pass. I cannot independently confirm these are the correct trait method names/signatures for `rmcp = 3.2.0`, nor that `ErrorCode::METHOD_NOT_FOUND` / the `Result<CustomResult, ErrorData>` shape match the pinned version exactly. Given that `cargo check`/`clippy` reportedly passed, the code compiles against the pinned lockfile, which is decent (if indirect) evidence the API surface exists — but that alone doesn't confirm *semantic* correctness (e.g., that `on_custom_request` is actually invoked for unrecognized top-level methods rather than some other hook).
**Impact:** Undetected drift between assumed and actual SDK contract would only surface if compilation broke, which is a weak guarantee for semantic behavior.
**Fix:** Have a second reviewer diff the adapter's usage against `docs.rs/rmcp/3.2.0` (or vendor the source) as a distinct verification step before closing the gate; record the specific doc.rs anchors checked in `docs/validation/M0-03.md` (currently it only asserts crates.io metadata/licensing, not API-shape verification).

## Low

**2. Partial-chunk drop on line-budget violation lacks a regression test**
`crates/mcp-server/src/stdio/budget.rs`, `BudgetedReader::poll_read`:
```rust
for byte in chunk.filled() {
    if *byte == b'\n' { this.line_bytes = 0; }
    else if this.line_bytes == MAX_LINE_BYTES { return this.fail(); }
    else { this.line_bytes += 1; }
}
output.put_slice(chunk.filled());
```
If a single OS read returns e.g. `{"...":1}\n{OVERSIZED...` (a complete, valid, previously-unseen JSON-RPC line followed in the same read by the start of an over-budget line), the entire chunk — including the already-complete valid line — is discarded without being forwarded to `output`, so rmcp never sees it and no response is ever sent for it, even though it was well-formed.
This is **explicitly disclosed** in `docs/security-model.md` ("Una violación puede descartar otras líneas del mismo chunk… sin garantía de respuesta para requests pendientes"), so it's not an undocumented defect — it's a known, accepted tradeoff (and the session terminates immediately afterward anyway, so the client would see the closed connection regardless). However, none of the 14 protocol tests actually exercises "valid line + oversized line coalesced in one write," so the documented behavior is unverified in CI.
**Fix (optional):** Add a protocol test that writes `<valid line>\n<oversized line, no terminator>` in a single `write`/`send_bytes` call and asserts no response is emitted for the valid line before exit(1), to lock in the documented (intentional) behavior rather than leave it purely as prose.

**3. Thread-local tracing subscriber is coupled to current-thread runtime choice**
`stdio.rs::run()` uses `tracing::subscriber::set_default(subscriber)` (thread-local) rather than `tracing::subscriber::set_global_default`. This works correctly today only because the runtime is explicitly `new_current_thread()` (single OS thread executes everything, per DoD requirement). If a future change switches to a multi-threaded runtime, log output from tasks on worker threads would silently vanish (fall back to the process-wide no-op default) — no compile error, no test failure, just missing logs.
**Fix (optional, forward-looking):** either keep a comment tying this specifically to the current-thread constraint (none exists today), or switch to `set_global_default` since there's exactly one logical subscriber for the whole process lifetime anyway.

**4. Supply-chain governance covers only direct dependencies**
`docs/validation/M0-03.md` states licenses were checked for the 5 direct dependencies (rmcp, tokio, tokio-util, tracing, tracing-subscriber) but not the ~74 transitive crates pulled in (chrono, uuid, wasm-bindgen family, windows-* family, schemars, futures, etc., per `Cargo.lock`). `cargo-deny` is noted as installed but unconfigured/unrun.
**Impact:** No automated enforcement of license compatibility or dependency bans beyond `cargo audit`'s vulnerability scan; a restrictive-licensed transitive dependency could enter silently on a future `cargo update`.
**Fix (optional, can be later-scope):** add a `deny.toml` and run `cargo deny check licenses bans` in the gate before release, not required to block M0-03 specifically.

## Observations
- `Cargo.toml` for `rust-engineering-mcp` requests tokio features `["rt", "time", "io-util"]` but not `io-std`; `rmcp::transport::stdio()` presumably needs `tokio::io::stdin/stdout`, which is normally gated by `io-std`. This compiles today only via Cargo feature unification with rmcp's own transitive tokio feature requirements. Not a bug (build passed per gate), but the manifest under-declares what the crate itself actually needs — worth tightening for clarity, not urgent.
- Two `syn` versions (2.0.119 and 3.0.4) coexist in the lockfile via transitive deps — harmless, standard dependency-graph noise, minor build-time cost only.
- The JSON-RPC "Invalid Request" (`-32600`) response is documented/tested to omit the `id` key entirely (`response.get("id").is_none()`), rather than including `"id": null` as JSON-RPC 2.0 strictly requires. This is rmcp's behavior, not the adapter's, and is already correctly disclaimed in `docs/compatibility.md` ("sin ID"). Flagging only so it's tracked as an upstream deviation, not something to "fix" here.
- `runtime.shutdown_timeout(Duration::from_millis(100))` is a fixed, seemingly arbitrary bound; acceptable for a short-lived CLI process where the OS reclaims resources on exit regardless, but the 100ms figure has no stated rationale in ADR-023 beyond "bounded."

## Missing tests
- No test exercises a coalesced "valid line + oversized line in one read" (see Low #2) to lock in the documented drop-on-violation behavior.
- No test for reader I/O *error* (as opposed to EOF or budget violation) on the read side — hard to construct portably, but currently only the writer-error path is exercised (`closed_stdout_exits_even_when_stdin_remains_open`, Unix-only).
- No Windows/Linux CI run (explicitly acknowledged as out of scope for M0-11, not a gap in this gate).
- No test independently confirming rmcp API surface/semantics against SDK docs (see Medium #1) — this is a review-process gap, not a code gap.

## Security concerns
- `RUST_LOG` is deliberately never read (hardcoded `Targets` filter, no `EnvFilter::from_env`), preventing SDK-internal diagnostics (which may embed peer-controlled data) from leaking to stderr; validated adversarially by setting `RUST_LOG=trace` in every protocol test and asserting no `SECRET` leakage. Good defensive choice.
- Unknown top-level JSON-RPC methods are handled via a fixed `"Unknown method"` error instead of echoing the untrusted method name back to the client, and this is tested for leakage. Good.
- No global concurrency, memory, or output budget, and no slow-client deadline exist yet — explicitly and correctly disclaimed in `SECURITY.md`/`compatibility.md` as out of scope for this gate; consistent with the DoD's statement that these are later gates before expensive tools exist.
- `cargo-deny` policy not configured/run (see Low #4) — a supply-chain governance gap, low severity at this stage since no tools/external inputs beyond stdin exist yet.

## Contract/ADR violations
None identified. Specifically verified compliant:
- ADR-002/012: modern (`2026-07-28`) + 4 legacy versions declared and tested separately; no `latest` auto-adoption; compatibility matrix updated.
- ADR-003: stdout carries only protocol frames, stderr carries only fixed operational logs; tested with clean-exit stderr-emptiness assertions.
- ADR-004: `mcp-server` does not depend on `rust-engineering-domain` (confirmed via `Cargo.lock`); no application/port layer introduced without a real consumer.
- ADR-009: no sandbox/capability claims made for this cut; correctly deferred.
- ADR-015: not yet applicable (no tool DTOs exist); correctly not implemented prematurely.
- "No fake tools": `tools/list` returns a deterministic empty array with `tools` capability advertised, no placeholder tool defined.

## Recommendation
This is a solid, narrowly-scoped bootstrap that matches its own DoD closely, with limitations transparently documented rather than hidden. **Required before closing the gate:** an independent check of the rmcp 3.2.0 API usage against actual pinned source/docs (Medium #1), since that verification is currently self-reported by the implementer. **Optional/nice-to-have, not blocking:** add the coalesced-line regression test (Low #2), tighten the tokio feature declaration, and decide whether `cargo-deny` should gate this milestone or a later one. No changes are required to the byte-budget guard's boundary logic itself — the 1 MiB accounting (CR counted, LF excluded, reset per line, checked at exact boundary) is correct and matches its test suite's stated boundary math.
