# W05b/W05c/W05d — informes de los workers

| Sesión | Inicio / fin (UTC) | ms | Permisos denegados | Resultado |
| --- | --- | --- | --- | --- |
| W05b (Sonnet High) | 2026-09-12T11:44:21Z / 2026-09-12T12:06:51Z | 1343416 | 7 | **sin informe**: la sesión terminó esperando una tarea en segundo plano que el modo `-p` no reanuda |
| W05c continuación (Sonnet High) | 2026-09-12T12:41:54Z / 2026-09-12T13:17:26Z | 2126446 | 12 | **sin informe**: la sesión terminó esperando una tarea en segundo plano que el modo `-p` no reanuda |
| W05d arnés (Sonnet Medium) | 2026-09-12T13:29:23Z / 2026-09-12T13:31:07Z | 103537 | 0 | informe completo |

Lección de proceso (registrada para todos los paquetes siguientes): un worker en
`claude -p` **no debe** lanzar comandos en segundo plano; la sesión termina al
final del turno y no hay notificación que la reanude. W05b y W05c terminaron
así sin informe; el orquestador auditó el árbol ítem a ítem (ver la
[disposición V05](../V05-review-symbols-tool/disposition.md) y
`continuation.md`) y W05d cerró el último punto con informe completo.

Verificación del orquestador sobre el árbol final: `cargo fmt`, analyzer
(mcp-server) 15/15, application analyzer 9/9, protocol 47/47, codec 48/48,
dominio 35/35, `test-release-smoke`, `check-architecture`, `links-check`,
`verify-inventories`, `test-m6-runtime-unit`, `test-gate-reporting` en verde.

## W05b (Sonnet High)

I'll wait for the background `cargo check` task to complete before continuing.

## W05c continuación (Sonnet High)

I'll end this turn here and resume once the pending background compile (`cargo test -p rust-engineering-application analyzer`) finishes — a task notification will bring me back automatically.

## W05d arnés (Sonnet Medium)

All checks pass.

## Report

**Task:** Fix `crates/mcp-server/tests/analyzer_runtime.rs` so the ignored native test uses a canonical `--root` (server rejects non-canonical `..`-containing roots at startup), and make the harness surface child stderr instead of a bare `Disconnected` when the server exits before the expected response.

**Result:** Both fixes applied; fmt, clippy, and test compilation are clean.

**Files changed:**
- `crates/mcp-server/tests/analyzer_runtime.rs` (only file touched, as scoped)

**Changes:**
1. `fixture_root()` now returns `Result<PathBuf>` and calls `.canonicalize()` (error propagated via `?`, no `unwrap`/`expect`). The test resolves it once into a `root` binding, used for both `--root`, `rust.project.open`'s `path`, and reading `src/lib.rs`.
2. `Server::start` now takes `root: &Path` and passes it through to `--root` instead of recomputing the (previously non-canonical) fixture path.
3. `Server::response` no longer lets a stdout-channel disconnect bubble up as an opaque `Disconnected` error. It matches on `RecvTimeoutError` explicitly: a real disconnect calls a new `exit_error()` helper, which checks `child.try_wait()` and, if the child has exited, drains the bounded stderr receiver (same `DISCOVERY_TIMEOUT`-bounded pattern `finish()` already uses) to build a message like `server exited <status> before responding: <stderr>`. Other recv errors (timeout) are passed through unchanged.

**Tests executed:**
- `cargo fmt --all -- --check` — clean, no diff.
- `cargo clippy -p rust-engineering-mcp --all-targets --locked --offline -- -D warnings` — clean, no warnings.
- `cargo test -p rust-engineering-mcp --locked --offline --test analyzer_runtime --no-run` — compiles successfully.
- Did not run the `#[ignore]` test itself (no Docker available in this environment), per instructions.

**Evidence:** Command outputs above; clippy and fmt reported no findings, and the test binary built and linked successfully (`Executable tests/analyzer_runtime.rs (target/debug/deps/analyzer_runtime-...)`).

**Risks:** `exit_error()`'s "still running" branch (`Ok(None)`) is a defensive fallback for a stdout-EOF-without-process-exit race; it's not expected to trigger in this test's actual failure mode (server exits with status 1 immediately on rejecting the root) but keeps the function total without panicking or guessing.

**Decisions:** Put the stderr-surfacing fix in `Server::response` (where the `Disconnected` error actually originates when the harness awaits the first response) rather than literally inside `Server::start`, since `start()` itself doesn't block on any response — this matches the root cause as described (child exits before the first response is read).

**Open issues:** None. Not committing, per instructions.

