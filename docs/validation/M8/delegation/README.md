# Registro de coordinación M8 — orquestación Fable 5.1

Encargo: [implement-m8-fable-orchestrator](../../../prompts/implement-m8-fable-orchestrator.md)
sobre el [encargo base M8](../../../prompts/implement-m8.md) y el
[plan M8](../../../roadmap/m8-stabilization.md). Rama `ai/m8-stabilization` desde
`main` `e50c3fefaff03fc45b89ae899cf5736af1fd0a72` (PR #20, merge de M6,
2026-09-14). Sin push, PR, tag, RC ni release sin autorización separada del owner.

Reglas del encargo que gobiernan este registro: Fable 5.1 orquesta y decide, no
escribe código de producto; Codex **prohibido como worker** y **obligatorio como
cliente stock bajo prueba** (M8-04); Claude Opus 5 / Sonnet 5 vía
`claude -p --model … --effort …` con prompt por stdin, `--disallowedTools Agent
Task`, sin comandos en segundo plano; Gemini 3.8 Flash High vía `agy`; revisores
read-only. Cada participación acreditada tiene invocación real, modelo, versión de
CLI, alcance, resultado y evidencia (`prompt-header.md`, `report.md` o
`disposition.md`, `transcripts.sha256`).

## 1. Verificación live del punto de partida (2026-09-14)

| Comprobación | Observado |
| --- | --- |
| `git rev-parse HEAD` en `main` tras `git pull --ff-only` | `e50c3fefaff03fc45b89ae899cf5736af1fd0a72` (merge PR #20 `ai/m6-analyzer`); árbol limpio (0 entradas en `git status --short`) |
| Rama de trabajo | `ai/m8-stabilization` creada desde ese commit |
| Workspace | `version = "0.3.0"` (workspace, 8 crates); `Cargo.lock` v4, 590 paquetes, sin cambios locales |
| Toolchain host | `rust-toolchain.toml` = `1.98.1` minimal + clippy + rustfmt; `rustc 1.98.1 (48a229cea 2026-09-01)`, `cargo 1.98.1 (797e8a9bc 2026-08-05)` |
| M6 cerrado y calificado | [M6-full-gate.json](../../M6/M6-full-gate.json) `sha256:69a0be14c1e2ae0cce07014daeba1818c49fa115aa3b67313bb0baffe07f34d0` — 42 etapas, `status: passed`, `source_inputs_unchanged: true`; [clients.json](../../M6/clients.json) `sha256:cb11315edfcb0521a4991c470f1e090ba129133bb94f0ef295d02a23f200b4c7`; [handoff](../../M6/handoff.md) y [g-disposition](../../M6/g-disposition.md) en `main` |
| M7 Deferred con decisión | [m7-g0-decision.md](../../../roadmap/m7-g0-decision.md) en `main`: no-go del owner (2026-09-13); fila M7 de [m2-m8.md](../../../roadmap/m2-m8.md) = Deferred. DoR de M8 satisfecho |
| Inventario público live | `target/release/rust-engineering-mcp serve --stdio --root <tmp>` (binario del 2026-09-12, mismo árbol de M6): `tools/list` = **36** tools, `resources/list` = `[]` (sin sesión), `prompts/list` = `[]`; respuesta `tools/list` = **392 529 bytes** (≈10,9 KiB por tool: entrada del gate de superficie M8-01) |
| Snapshots de contrato | `crates/mcp-server/tests/snapshots/`: 36 `*-tool.json` + `doctor-report.json` |
| Imágenes guest por digest | Docker `29.7.2 linux/arm64`; `rust-engineering-runtime:1.98.1-arm64-m6` `f39a5b33ee7d`, `…-m5` `e0a5ca1661b3`, `…-m4-scanner` `25ed3626e710`, `…-m4` `95dddeb5305f`, `…-m3` `384a1742ecc5`, `…-arm64` `8fac70723a8d`, `rust-mcp-probe:m0` `4a44294379a0` |
| Hygiene | `docs-hygiene.py links-check`: 2 470 enlaces, 0 rotos en documentos vivos; `verify-inventories`: 7 inventarios, 0 fallos |
| Tags publicados | `v0.1.0`, `v0.3.0` (no existe 0.4–0.7: M3–M6 quedaron calificados localmente sin release; M8 numera desde 0.8) |
| Deuda heredada visible en la entrada | Tabla de hitos de `m2-m8.md`: M3–M6 aún «Planned» (reconciliar en M8-01/06); `docs/ci.md` conserva la frase «14 etapas nativas» (undercount); Windows x86_64 retirado del CI el 2026-09-13 (regresión stdio pre-`initialize`, deuda D13) |

## 2. CLIs y modelos verificados antes de la primera invocación

| CLI | Versión | Comprobación |
| --- | --- | --- |
| `claude` (Claude Code) | `2.1.268` | `claude auth status`: loggedIn, `apiProvider: firstParty`. Sondas `claude -p --model {opus,sonnet} --tools "" --restricted --no-session-persistence --output-format json` con «Reply with exactly: OK» → `OK`; `modelUsage` `claude-opus-5` (1 368 ms) y `claude-sonnet-5` (1 201 ms), auxiliar `claude-haiku-4-5`; `permission_denials: []`. Transcripts sha256 `4d3260a8…` (opus) y `7e1945bd…` (sonnet), fuera del árbol |
| `agy` (Gemini CLI) | `1.2.1` | `agy models` lista `gemini-3.8-flash-high` |
| `codex` | `codex-cli 0.154.0` | `codex login status`: «Logged in using ChatGPT». Sonda `codex exec --skip-git-repo-check -C /private/tmp "Reply with exactly: OK"` → `OK` (cuota disponible tras el reset del 2026-09-14). **Solo como cliente stock bajo prueba (M8-04)**, nunca como worker |
| `gh` | — | Autenticado (`cburgosro9303`); `gh pr create`/`merge` los ejecuta el owner (clasificador de auto-mode) |

Efectos: Opus 5 High para seguridad/persistencia/contención/arquitectura/cierre;
Sonnet 5 High/Medium para contratos/cortes/harness/docs; Gemini 3.8 Flash High
para investigación y trazabilidad (los hashes los verifica el orquestador).

## 3. Decisiones del orquestador

| Decisión | Estado | Registro |
| --- | --- | --- |
| D11 — política de deprecación y freeze | **Decidida** 2026-09-14 (antes de M8-01, como exige el plan) | [D11-decision-brief.md](D11-decision-brief.md) → ADR-086 (W02) |
| D12 — migraciones y rollback | Pendiente (M8-03; requiere el censo de formatos de M8-01) | — |
| D13 — calificación por target 1.0 | **Decidida por el owner: A** (2026-09-14, «Aprobado A» en sesión) — 1.0 = macOS ARM64 único host positivo; Linux/Windows portabilidad no calificada | [D13-scope-brief.md](D13-scope-brief.md) → ADR-087 (W07) |
| D14 — distribución/provenance offline | Pendiente (M8-07) | — |

## 4. Paquetes de delegación

| ID | Agente | Alcance | Estado |
| --- | --- | --- | --- |
| [W01-census](W01-census/prompt-header.md) | Claude Sonnet 5 (High) | M8-01: censo de invocaciones reales → contratos/errores/CLI/Resources/formatos → clasificación propuesta stable/preview/internal; gate de superficie (36 tools, coste de contexto, consumidor real); huérfanos; reconciliación de `m2-m8.md` | **Hecho** ([informe](W01-census/report.md), [disposición](W01-census/disposition.md); decisiones en [01.md](../01.md)) |
| [W02-adr-d11](W02-adr-d11/prompt-header.md) | Claude Sonnet 5 (Medium) | ADR-086 (D11) desde el brief del orquestador; backlog D11 → Accepted; índice ADR; sección de política en `docs/compatibility.md` | **Hecho** ([informe](W02-adr-d11/report.md), [disposición](W02-adr-d11/disposition.md): aceptado; P3 terminológico `internal` vs §57 dispuesto) |
| [W03-census-fixes](W03-census-fixes/prompt-header.md) | Claude Sonnet 5 (Medium) | F1–F4 del censo: `--help` con 36 tools, conteos «31» del checkout en README/tools/architecture/client-configuration, índice ADR-078/079/081/085, línea de estado de `m2-m8.md` | **Hecho** ([informe](W03-census-fixes/report.md), [disposición](W03-census-fixes/disposition.md): aceptado; fmt/clippy/cli 13/13 verdes) |
| [R01-census-traceability](R01-census-traceability/prompt-header.md) | Gemini 3.8 Flash High (read-only, `agy`) | Auditoría de trazabilidad spec→ADR→código→tests→docs del censo y de las decisiones de `01.md` | **Hecho** (453 s): Approve con findings F10–F13, todos reproducidos por el orquestador y aceptados ([informe](R01-census-traceability/report.md), [disposición](R01-census-traceability/disposition.md)) |
| [W01b-census-corrections](W01b-census-corrections/prompt-header.md) | Claude Sonnet 5 (Medium) | F10–F13 de R01: `error_codes[]` de 5 tools desde snapshots, casing de `action.apply`, consumidor real de 3 tools M3, `client-configuration.md`/`compatibility.md` con M6 integrado | **Hecho** ([informe](W01b-census-corrections/report.md), [disposición](W01b-census-corrections/disposition.md): aceptado; 36/36 error_codes, 11 findings) |
| [V01-review-census](V01-review-census/prompt-header.md) | Claude Sonnet 5 (High, read-only, sin tools) | Revisión independiente de `01.md`, `01-census.md`, ADR-086 y del diff W02/W03/W01b | **Hecho**: Block (3 P1, 5 P2, 5 P3); [disposición](V01-review-census/disposition.md): F-A/F-D/F-G/F-M + P3 aceptados (→ W04, `01.md`), F-J/F-K/F-L rechazados con evidencia ([informe](V01-review-census/report.md)) |
| [W04-v01-fixes](W04-v01-fixes/prompt-header.md) | Claude Sonnet 5 (Medium) | Enmienda ADR-086 §1 (`experimental`, definición de «consumidor real», obligación M8-04), nota `preview` en compatibility/client-configuration, README:26, `stock_client_invoked` en el censo | **Hecho** ([informe](W04-v01-fixes/report.md), [disposición](W04-v01-fixes/disposition.md): aceptado; 33/3 verificado) |
| [V01b-rereview](V01b-rereview/prompt-header.md) | Claude Sonnet 5 (Medium, read-only, sin tools) | Re-revisión del diff W04 y del `01.md` final contra la disposición V01 | **Hecho**: Approve con findings; el P1 (flag `--allow-analyzer-action-write`) verificado como existente por el orquestador ([disposición](V01b-rereview/disposition.md)) |
| [I01-integration](I01-integration/prompt-header.md) | Claude Sonnet 5 (Low) | Tres commits de M8-01 (ADR-086; `--help`/docs; censo + registro), sin push | **Hecho**: `9ecd945`, `2e2e75d`, `e445aa1` ([disposición](I01-integration/disposition.md)); registro I01 en commit aparte |
| [W05-contract-document](W05-contract-document/prompt-header.md) | Claude Sonnet 5 (High) | M8-02: `Stability` + prefijo `Preview (ADR-086): ` en 5 descripciones (5 snapshots regenerados, 31 byte-idénticos); subcomando `contract [--json]` (spec §56) con hashes canónicos; tests portables | **Hecho** ([informe](W05-contract-document/report.md), [disposición](W05-contract-document/disposition.md)): 0 discrepancias de hash en 36 tools; incidencia: cargo test en segundo plano + `tail -f` bloqueado, liberado por el orquestador |
| [W06-contract-freeze](W06-contract-freeze/prompt-header.md) | Claude Sonnet 5 (Medium) | `scripts/contract-freeze.py` (generate/verify/diff) + tests + etapa `core`; `02-schema-diff.json` desde `v0.3.0` y `v0.1.0` (13 M1) | **Hecho** ([informe](W06-contract-freeze/report.md), [disposición](W06-contract-freeze/disposition.md)): 10/10 tests; hallazgo H1 (cambio de `binary.bloat` vive en `$defs` del schema) aceptado |
| [W07-d13-scope](W07-d13-scope/prompt-header.md) | Claude Sonnet 5 (Medium) | ADR-087 (D13 = A), nota en spec §61/§97, README/compatibility/ci.md | **Hecho** ([informe](W07-d13-scope/report.md), [disposición](W07-d13-scope/disposition.md)) |
| [W08-freeze-version](W08-freeze-version/prompt-header.md) | Claude Sonnet 5 (Medium) | Versión 0.8.0 (`Cargo.toml`/lock), CHANGELOG con migration notes 0.3.0→0.8.0, docs F5/F6, tablero, nota `preview` única | **Hecho** ([informe](W08-freeze-version/report.md), [disposición](W08-freeze-version/disposition.md)) |
| [V02-review-freeze](V02-review-freeze/prompt-header.md) | Claude Opus 5 (High, read-only: Read/Grep/Glob) | Revisión del freeze 0.8.0: contrato/CLI (W05), oráculo de freeze (W06), D13 (W07), versión y migration notes (W08) | **Hecho**: Block (4 P2 contrato/gate, P3); todos aceptados ([disposición](V02-review-freeze/disposition.md), [informe](V02-review-freeze/report.md)) |
| [W09-v02-rust-fixes](W09-v02-rust-fixes/prompt-header.md) | Claude Sonnet 5 (High) | P2-1 semántica/valores de `executes_project_code` (14 true) + test de valores; P3 literales, annotations en cli.rs, stderr en fallo | **Hecho** ([informe](W09-v02-rust-fixes/report.md), [disposición](W09-v02-rust-fixes/disposition.md): 14/14 verificado, 0 discrepancias de hash) |
| [W10-v02-script-doc-fixes](W10-v02-script-doc-fixes/prompt-header.md) | Claude Sonnet 5 (Medium) | P2-2 clase registrada en `verify`; P2-3 etapa obligatoria; P2-4 clase `stable` del subcomando/documento + docs completas + cadena de verificación; P3 procedencia/bytes/tests/redacción; censo `executes_project_code` | **Hecho** ([informe](W10-v02-script-doc-fixes/report.md), [disposición](W10-v02-script-doc-fixes/disposition.md): 20/20 tests; 30/30 stable byte-idénticos a 0.3.0) |
| [V02b-rereview](V02b-rereview/prompt-header.md) | Claude Opus 5 (Medium, read-only: Read/Grep/Glob) | Re-revisión de las correcciones W09/W10 contra la disposición V02 | **Hecho**: Block por regresión nueva P2-N1 (test `diff` no hermético; causa: encargo W10) — 4 P2 de V02 confirmados cerrados ([disposición](V02b-rereview/disposition.md), [informe](V02b-rereview/report.md)) |
| [W10b-hermetic-diff-test](W10b-hermetic-diff-test/prompt-header.md) | Claude Sonnet 5 (Medium) | Test `diff` hermético con dobles de git (P2-N1); nota `--human` fuera del contrato | **Hecho** ([informe](W10b-hermetic-diff-test/report.md)): tests verdes también con `GIT_DIR=/nonexistent` |
| [W11-sonar-coverage](W11-sonar-coverage/prompt-header.md) | Claude Sonnet 5 (Low) | `sonarcloud.yml`: ejecutar `test-contract-freeze.py` bajo coverage (fuente Python nueva; puerta 80 % de código nuevo) | **Hecho** ([disposición](W11-sonar-coverage/disposition.md)) |
| [W12-ci-doc-counts](W12-ci-doc-counts/prompt-header.md) | Claude Sonnet 5 (Low) | `docs/ci.md`: 28 etapas core / full real, etapas nativas por nombre, sección M8 (F8 del censo) | **Hecho** ([disposición](W12-ci-doc-counts/disposition.md)) |
| [I02-integration](I02-integration/prompt-header.md) | Claude Sonnet 5 (Low) | Cinco commits de M8-02 (ADR-087; freeze Rust + versión; oráculo Python + gate; docs; paquete M8) y commit de evidencia regenerada post-commit | **Hecho**: `1bdb61b` (ADR-087), `c6189d2` (freeze Rust + 0.8.0), `86dbdd8` (oráculo + gate + sonarcloud), `c78997f` (docs), `b7f0528` (paquete M8); evidencia regenerada sobre `b7f0528` (`tree_dirty: false`) en commit I02b |
