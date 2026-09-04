# Revisión externa M0-01

Fecha: 2026-09-03. Commit revisado: `4533abe2175455aa7635eddc62dd33da1c9a937a`.
Baseline del diff: `be27aef`. Claude Code `2.1.259`, reviewer explícito
`claude-sonnet-5`, effort `medium`, respuesta `success`, `is_error=false`, un turno.
Duración reportada: 111817 ms.

Invocación (paquete de revisión por stdin, sin herramientas):

```text
claude --print --model claude-sonnet-5 --effort medium --safe-mode --restricted --strict-mcp-config --tools '' --permission-mode dontAsk --permission-prompts none --no-session-persistence --output-format json
```

El paquete incluyó objetivo/DoD M0-01, resultados de gates, riesgos conocidos,
ADR-001/003/004 y el diff completo baseline→commit, incluido ADR-021. El reviewer
no ejecutó gates ni inspeccionó fuera del paquete. La metadata confirma el modelo
`claude-sonnet-5`; el cliente también reportó una llamada auxiliar a
`claude-haiku-4-5-20251001` (20 tokens de salida). No se configuró fallback.
Los flags restringen herramientas del reviewer; no son una afirmación de sandbox OS
para el cliente Claude Code.

SHA-256 del paquete: `9b7b3add8bf8a88cf97d1513fc279c7528c7683e078e3006c86d4a94e7707751`.

## Respuesta del reviewer

Nota de trazabilidad: esta respuesta revisó el toolchain 1.97.1 en `4533abe`.
El upgrade a 1.98.1 pertenece al commit posterior `cafe721`; se conserva aquí la
versión realmente citada por el reviewer.

## M0-01 Review — Executable Workspace Bootstrap

Scope respected: reviewed only workspace/toolchain/binary/docs bootstrap, not M0-02+/MCP/sandbox.

### Critical
None.

### High
None.

### Medium
None.

### Low
- `crates/mcp-server/src/main.rs:44` mixes `write_all` (Help) and `writeln!` (Version) for stdout writes with no functional difference — cosmetic inconsistency, not a defect.
- `Cargo.toml:3` sets `resolver = "3"` explicitly. For edition 2024 this is Cargo's default resolver, so the line is redundant. Not wrong, but I can't independently confirm behavior against actual Cargo 1.97.1 without running the toolchain — flagging as an evidence gap rather than an error.
- Commit `4533abe` bundles the CLI bootstrap with unrelated governance edits (`AGENTS.md` model/subagent policy, `docs/prompts/continue-m0-m1.md`). Doesn't affect correctness but reduces audit granularity for a milestone commit.

### Observations
- `main.rs` logic is correct and minimal: single-argument command matching, closed set of recognized flags, fixed non-reflective diagnostic on stderr, exit codes 0/2/1 exactly as documented in `README.md`. Matches ADR-021's stated bootstrap contract line for line.
- No `unsafe`, no `unwrap`/`expect`/`panic!`, no stdin read, no process spawn in production code (`crates/mcp-server/src/main.rs`) — consistent with `SECURITY.md` and `docs/security-model.md` claims.
- `Cargo.lock` correctly lists only the local path package with no external dependencies, consistent with "zero dependencies" claims throughout docs.
- License handling is coherent: no `license`/`license-file` field in either `Cargo.toml`, `publish = false` set at workspace level and inherited via `publish.workspace = true`, `LICENSE` explicitly a "pending decision" notice rather than a grant, README and CHANGELOG both state this consistently. No accidental default-license implication.
- `docs/implementation-status.md` correctly marks M0-01 "In progress" (not "Done"), correctly deferring closure to external review per `AGENTS.md`'s policy — no premature milestone-complete claim found anywhere in the diff.
- `docs/tools.md`, `docs/architecture.md`, `docs/compatibility.md`, `docs/security-model.md` all explicitly state "not implemented" for MCP/tools/sandbox/catalog — no overclaiming beyond the actual help/version binary.
- ADR-021 correctly scopes the deferral of Tokio/`rmcp`/tracing to M0-03 and cites ADR-002's pre-pin verification requirement; consistent with ADR-001/ADR-003/ADR-004 without weakening any of them.

### Missing tests
- The documented "write error → exit code 1" path (`README.md`: "un error de escritura devuelve código 1") is implemented (`main.rs` final `if result.is_err()` branch) but has no test coverage in `crates/mcp-server/tests/cli.rs`. Low risk given the code is trivial, but it's an asserted behavior with zero evidence.

### Security concerns
None. Untrusted-argument non-reflection (`crates/mcp-server/tests/cli.rs::untrusted_arguments_are_not_echoed`), non-UTF-8 argument handling without panic, and `env_clear()` test isolation are all implemented and tested consistently with `SECURITY.md`'s claims. No process execution, no stdin consumption, no network code exists to misrepresent.

### Contract/ADR violations
None found within M0-01's declared scope. ADR-001 (Rust/Tokio) is not violated — Tokio's absence is explicitly and correctly deferred via ADR-021 to M0-03, not silently dropped. ADR-004 (hexagonal) is respected by not pre-creating empty domain/adapter crates. ADR-003 (stdio) isn't implicated since no serving mode exists yet, and `serve --stdio`/`--stdio` are correctly rejected with no stdout pollution.

### Recommendation
**Accept.** The bootstrap is minimal, honestly scoped, internally consistent across code/docs/ADRs, and free of overclaiming. Only a trivial untested exit-code branch and a couple of cosmetic nits were found — neither blocks milestone closure.
