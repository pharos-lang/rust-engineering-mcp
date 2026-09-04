# Revisión independiente M0-07

Claude Code CLI 2.1.259; modelo principal claude-sonnet-5, esfuerzo medium.
Read-only, tools deshabilitadas, safe-mode/restricted, sin permisos ni sesión persistente.
Telemetry también registra Haiku auxiliar; no se atribuye el dictamen a ese modelo.
Paquete: contrato completo, project handler, ADR-015 y schema snapshot.

## Findings

**No blocking defects found.** The contract/schema/error-mapping logic is internally consistent and well-tested. Findings below are concerns to confirm with the owner or genuine future-scope, not bugs to fix now.

### Worth confirming (not blocking)

1. **Global single-slot concurrency (Medium) — `crates/mcp-server/src/stdio/project.rs:227` (`Arc::new(Semaphore::new(1))`, `try_acquire_owned()`)**
   A non-blocking `try_acquire` on a semaphore of size 1 means any second concurrent `project.open` call fails immediately with `SandboxDenied`, rather than queuing. If two legitimate clients (e.g. two IDE windows) call `project.open` at nearly the same moment, one gets a spurious "sandbox denied" error even though nothing was actually denied by policy — the error code conflates resource contention with policy denial. If this is intentional M0 scope-limiting, fine; otherwise worth flagging that fail-fast vs. queue-and-wait is a real behavioral choice with UX impact.

2. **Nested-object closure not enforced by `Contract` (Low, future-scope) — `crates/mcp-server/src/stdio/contract.rs:66` (`closed_object`)**
   `closed_object` only inspects the schema root (`additionalProperties`/`unevaluatedProperties` at top level). Nested `$defs` like `OpenData`/`Truncation` are closed today only because each struct separately carries `#[serde(deny_unknown_fields)]`. `Contract::new` doesn't verify this recursively, so a future M1 DTO author could forget `deny_unknown_fields` on a nested struct and the contract's startup validator would not catch it. Worth a code comment or a recursive check before more DTOs are added (correctly called out already in the ADR as a semantic-not-schema concern, but the recursive gap itself isn't documented).

3. **`maxLength` unit mismatch for user-controlled strings (Low) — `crates/mcp-server/src/stdio/project.rs:37` (`workspace_root`), `contract.rs` test fixture**
   JSON Schema `maxLength` counts UTF-16 code units; if any Rust-side length check (in the domain layer, not shown) counts bytes or chars, a legitimate multi-byte-UTF-8 `workspace_root` could pass Rust validation but fail schema validation on encode, surfacing as a generic `InternalError` with no indication of cause (by design, since payloads aren't reflected). Low likelihood/impact given the 4096 budget, but worth a unicode-path test case if not already covered elsewhere.

### Out of scope for this review

`crates/mcp-server/src/stdio.rs`, `CHANGELOG.md`, `README.md`, and the docs under `docs/` are listed as modified in git status but their contents weren't supplied here, so tool registration/dispatch wiring in `stdio.rs` (e.g., how `ProjectTool::call` is reached, whether `request.name` routing is exhaustive) was not reviewed.

### Things verified as correct

- `decode`/`encode` never reflect payloads or schema-engine diagnostics into `ErrorData.data` (both `invalid_input()` and `internal()` pass `None`).
- Status→wire mapping is exhaustive and matches the ADR refinement: Passed/Failed → `Ok` structured result (`isError=false`); Blocked/Unavailable/Cancelled → structured error (`isError=true`); malformed input → `InvalidParams`; schema-invariant violations on output → fixed `InternalError`.
- `OperationalErrorCode` → `BlockedCode`/`UnavailableCode` match in `failure()` is exhaustive (no wildcard arm), so the compiler enforces coverage as new codes are added.
- Cancellation/timeout handling correctly retains the semaphore permit through actual worker completion by moving it into the `spawn_blocking` closure — avoids the classic `tokio::time::timeout` + `spawn_blocking` slot-leak bug, with an accurate comment explaining why.
- Test coverage is solid: missing-required-field schema rejection, non-object root rejection at startup, whitespace-only/oversized input rejection, wire-text/structuredContent equality, and unknown/inconsistent `error_code` rejection are all exercised.
- `project.open` output/input schema snapshot is unchanged and matches the fixtures.

## Verdict

**Approve.** No correctness, schema, error-mapping, or security defects found in the supplied sources. Recommend the technical owner confirm item 1 (concurrency fail-fast semantics) is intentional before merge, and track items 2–3 as low-priority follow-ups for M1.

## Disposición del Technical Owner

Sin bloqueantes. (1) Fail-fast y capacidad de un worker son policy explícita ADR-024;
SANDBOX_DENIED incluye presupuestos del servidor, por diseño y con tests.
(2) Se documenta que cierre de objetos anidados pertenece a DTOs y snapshots,
no a la comprobación de raíz; schemas actuales sí cierran esos objetos.
(3) La afirmación del reviewer sobre UTF-16 es incorrecta: JSON Schema cuenta
caracteres Unicode. El adapter de filesystem impone 4096 bytes, más restrictivo
que 4096 caracteres: un path aceptado por ese presupuesto no puede exceder
maxLength al serializar. El borde puede aceptar inicialmente más bytes para un
path Unicode, pero el adapter lo rechaza bajo su propio límite antes de I/O.
El principal revisó stdio.rs (solo declaración del módulo añadida), documentación,
diff y gate; la revisión externa no se presenta como cobertura de archivos omitidos.
