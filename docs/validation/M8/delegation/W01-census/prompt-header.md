# W01 — M8-01: censo de contratos y gate de superficie

Modelo solicitado: Claude Sonnet 5 (`claude -p --model sonnet --effort high`). Rol: worker de censo (solo documentación bajo `docs/`). Orquestador: Claude Fable 5.1. No puedes lanzar subagentes. **Nunca corras comandos en segundo plano** (la sesión termina con el turno). **No toques `crates/`, `scripts/`, `fixtures/`, `Cargo.*`, `.github/`.** No hagas commit. No hagas Docker.

## Contexto (verificado live por el orquestador, 2026-09-14)

Repo `/Users/cburgosro/Projects/rust-mcp`, rama `ai/m8-stabilization` desde `main` `e50c3fe` (M6 cerrado y calificado; M7 Deferred). Workspace `0.3.0`, Rust 1.98.1. `target/release/rust-engineering-mcp serve --stdio --root <dir-abs>` anuncia **36 tools** (`tools/list` = 392 529 bytes ≈ 10,9 KiB por tool), `resources/list` = `[]` sin sesión, `prompts/list` = `[]`. Snapshots de contrato en `crates/mcp-server/tests/snapshots/` (36 `*-tool.json` + `doctor-report.json`). Tags publicados: `v0.1.0`, `v0.3.0` (no hubo 0.4–0.7).

Lee antes de escribir: `AGENTS.md`; `docs/roadmap/m8-stabilization.md` (M8-01, «gate de superficie», §Migración/censo de formatos); `docs/roadmap/m2-m8.md` §G1–G9 y la tabla de hitos; `docs/validation/M8/delegation/D11-decision-brief.md` (clases `stable|preview|internal`, decidida); spec `docs/spec/rust-engineering-mcp-propuesta-v0.3.md` §9 (no todo es tool), §20 (tool design rules), §55–57, §78 (contexto), §85 (descriptions), §116.1 (tool explosion); `docs/tools.md`, `docs/compatibility.md`, `docs/client-configuration.md`, `docs/implementation-status.md`; los recibos de clientes `docs/validation/M{1,2,3,4,5,6}/clients.json` y sus `.md`; ADR-012 y los ADR de cada tool.

## Objetivo (M8-01 del plan)

Producir el **censo de invocaciones reales** y el **gate de superficie**, de modo que cada elemento de contrato tenga owner (crate/módulo), fuente (ADR/spec), test, consumidor real y límite; **huérfanos = 0** (o listados explícitamente). La decisión final de clase/consolidación es del orquestador: tú propones con evidencia.

## Entregables (crea exactamente estos archivos)

1. `docs/validation/M8/01-census.json` — machine-readable, con `format_version: 1`, `generated_utc`, `head_commit`, y secciones:
   - `tools[]` (36): `name`, `milestone_cut`, `adr[]`, `adapter_source` (ruta en `crates/mcp-server/src/…`), `application_entry` (use case/port), `domain_types[]`, `tests` {`contract_snapshot`, `protocol_tests[]`, `native_cuts[]`}, `annotations` (readOnlyHint/destructiveHint/idempotentHint/openWorldHint tal como los anuncia el snapshot), `writes_disk` (bool), `requires_runtime` (`none|docker-rust|docker-scanner|catalog|analyzer`), `error_codes[]` (los `code` cerrados que puede devolver; cítalos desde el código), `status_values[]`, `schema_bytes` {`input`, `output`, `description_chars`} medidos desde el snapshot, `real_consumers[]` (cliente + recibo que demuestra una invocación real: ruta + fila/ID), `proposed_class` (`stable|preview|internal`) con `class_rationale`, `surface_gate` {`why_tool_not_resource_or_prompt`, `context_cost_note`, `consolidation_candidate` (`none` o descripción con la tool con la que se fusionaría), `compat_risk_of_consolidation`}.
   - `resources[]`: URIs/templates que el servidor puede anunciar en sesión (búscalos en `crates/mcp-server/src`), productor, tests, consumidores reales (recibos M1/M5 con Resources), clase propuesta.
   - `prompts[]`: vacío si no existen (regístralo).
   - `cli_commands[]`: cada subcomando de `rust-engineering-mcp --help` (ejecútalo): flags, `format_version` del JSON de salida si existe, exit codes documentados/probados, tests, docs, clase propuesta.
   - `error_model`: dónde se definen `status`/`code` cerrados (dominio), lista completa de códigos, tests que los cubren, inconsistencias detectadas.
   - `disk_formats[]`: config host (flags de `serve`), journal/receipts M2, jobs/artifacts store M3, estado analyzer M6, catálogo SQLite + trust, índice Lance derivado, security policy, snapshot RustSec, vendor tree, `state-root` en general: para cada uno `location/layout`, `version_marker` (cómo se detecta la versión del formato; «ninguno» si no hay), `reader_writer` (módulos), `adr[]`, `tests[]`, `migration_relevant` (bool + por qué), `floor_or_trust_state` (bool).
   - `orphans[]`: elementos sin owner, fuente, test o consumidor real; vacío solo si de verdad no hay.
   - `findings[]`: contradicciones spec→ADR→código→tests→docs que descubras (p. ej. versión de workspace `0.3.0` frente a «0.6.x» en docs; hitos M3–M6 «Planned» en `m2-m8.md`; conteos desactualizados en `docs/ci.md`/`docs/tools.md`; descripciones que contradicen el estado actual). Cada finding: `id`, `severity` (P0–P3 según G8), `where`, `evidence`, `proposed_fix`, `blocks_freeze` (bool).
2. `docs/validation/M8/01-census.md` — narrativa: método (comandos ejecutados), tabla resumen por tool (nombre, corte, clase propuesta, consumidor real, bytes de schema, candidato a consolidación), **tabla del gate de superficie** (justificación tool vs Resource/prompt, coste de contexto: bytes y % del total de `tools/list`), Resources/CLI/formatos, huérfanos, findings y propuesta de reconciliación del roadmap. Enlaza cada evidencia por ruta relativa.
3. Edita `docs/roadmap/m2-m8.md`: filas M3, M4, M5 y M6 de la tabla de hitos de «Planned» al estado real con enlace a su evidencia de cierre (`docs/validation/M{3,4,5,6}/…` handoff/matriz/recibo). No cambies nada más de ese archivo.

## Reglas de evidencia

- Mide, no estimes: bytes desde los snapshots (`python3` con `json`), conteos desde el binario/los tests, consumidores desde recibos existentes (cita ruta y la fila/ID dentro del recibo). Si una tool no tiene invocación real registrada por ningún cliente, dilo (`real_consumers: []`) y proponla `preview` o consolidación, nunca la inventes.
- «Consumidor real» = una invocación registrada en un recibo de clientes (Inspector/Codex/Claude Code) o en un test nativo end-to-end; un test unitario no cuenta como consumidor.
- No propongas borrar contratos usados para llegar a ~35 por número; el plan lo prohíbe.
- Ninguna afirmación de compatibilidad sin cita a snapshot/test.

## Verificación (foreground)

`python3 -c "import json;json.load(open('docs/validation/M8/01-census.json'))"`; `python3 -B scripts/docs-hygiene.py links-check` (0 rotos en documentos vivos); comprueba que `tools[]` tiene exactamente 36 nombres iguales a los del `tools/list` live (ejecuta el binario con `printf` de tres líneas JSON-RPC: `initialize` 2025-06-18, `notifications/initialized`, `tools/list`, con `--root` a un directorio temporal absoluto bajo `/private/tmp`).

Informe final (en tu última respuesta): Task / Result / Files changed / Comandos ejecutados / Conteos (tools, resources, cli, formatos, huérfanos, findings por severidad) / Top-10 propuestas del gate de superficie / Risks / Open issues. No commit.
