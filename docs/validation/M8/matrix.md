# M8 — matriz de estabilización y readiness 1.0

Estado: **en curso** (M8-01 iniciado 2026-09-14). Rama `ai/m8-stabilization` desde
`main` `e50c3fefaff03fc45b89ae899cf5736af1fd0a72` (merge de M6, PR #20). Encargo:
[implement-m8-fable-orchestrator](../../prompts/implement-m8-fable-orchestrator.md)
sobre [implement-m8](../../prompts/implement-m8.md) y el
[plan M8](../../roadmap/m8-stabilization.md). Registro de coordinación y
delegaciones: [delegation/README.md](delegation/README.md). Sin push, PR, tag,
RC ni release sin autorización separada del owner.

## DoR

| Requisito | Estado | Evidencia |
| --- | --- | --- |
| M6 cerrado y calificado, en `main` | Hecho | [registro §1](delegation/README.md#1-verificación-live-del-punto-de-partida-2026-09-14): gate `full` `sha256:69a0be14…` (42 etapas), clientes `sha256:cb11315e…`, G1–G9 |
| M7 cerrado o Deferred con decisión | Hecho (Deferred) | [m7-g0-decision.md](../../roadmap/m7-g0-decision.md) |
| Toolchain/lock/imágenes verificados live | Hecho | registro §1: Rust/Cargo 1.98.1, lock v4 590 paquetes, imágenes por digest |
| D11 decidida antes de M8-01 | Hecho | [D11-decision-brief.md](delegation/D11-decision-brief.md) → ADR-086 |
| D12–D14 preparadas | Pendiente (D13 alcance antes de M8-02; D12 en M8-03; D13/D14 en M8-07) | [backlog](../../roadmap/adr-backlog-m2-m8.md) |
| Entornos de migración y clientes reales | Parcial: Inspector 2.5.0, Claude Code 2.1.268, Codex CLI 0.154.0 (cuota OK), Gemini CLI 1.2.1 disponibles; entornos de migración se definen con el censo de formatos (M8-01) | registro §2 |

## Decisiones D11–D14

| Decisión | Estado | Registro |
| --- | --- | --- |
| D11 deprecación/freeze | **Decidida** 2026-09-14 | [brief](delegation/D11-decision-brief.md); [ADR-086](../../adr/ADR-086-deprecation-and-freeze-policy.md) (enmendado por V01 F-A/F-D) |
| D12 migraciones/rollback | Pendiente (M8-03) | — |
| D13 targets 1.0 | **Pendiente del owner** (alcance antes de M8-02) | [brief D13](delegation/D13-scope-brief.md): recomendación A (macOS ARM64) |
| D14 distribución/provenance offline | Pendiente (M8-07) | — |

## Cortes y evidencia

| ID | Corte | Estado | Evidencia |
| --- | --- | --- | --- |
| M8-01 | Censo de invocaciones reales → contratos/errores/CLI/Resources/formatos → clasificación; gate de superficie; huérfanos = 0 | **Done local** (pendiente de commit I01): 36 tools, 0 huérfanos, 2 Resources dinámicas, 15 CLI, 10 formatos; 31 `stable` / 5 `preview`; 0 consolidaciones; 11 findings dispuestos; `--help` y docs públicas corregidos | [01.md](01.md) (decisiones), [01-census.md](01-census.md), [01-census.json](01-census.json); revisiones R01/V01/V01b |
| M8-02 | Freeze 0.8.x con migration notes; before/after schema/behavior; trece M1 sin escritura implícita | Planned | — |
| M8-03 | Migración con preflight/dry-run/backup/rollback; floors/trust no retroceden; journal pendiente impide downgrade (D12) | Planned | — |
| M8-04 | Matriz wire/cliente (5 revisiones + stdio; Inspector + Codex obligatorios; Claude Code/Gemini CLI calificados) | Planned | — |
| M8-05 | Presupuestos de performance desde M5; soak con límites fijados antes de medir | Planned | — |
| M8-06 | Guías públicas reproducidas por tercero; sin texto histórico contradictorio | Planned | — |
| M8-07 | Artifacts por target, SBOM/notices/firma/provenance, smoke desde descarga limpia (D13/D14) | Planned | — |
| M8-08 | Threat model completo + auditoría independiente (paquete Opus High; modelo ≠ humano ≠ pentest) | Planned | — |
| M8-09 | Dos RC consecutivos 0.9 con contrato igual, full gate y soak verdes | Planned | — |

## Pruebas ejecutadas y no ejecutadas (§4 del encargo)

| Verificación | Estado | Motivo |
| --- | --- | --- |
| `docs-hygiene.py links-check` / `verify-inventories` | Ejecutadas en la entrada y tras cada worker (0 rotos / 0 fallos) | Obligatorias antes de cada commit |
| `cargo fmt --check`, `cargo clippy -p rust-engineering-mcp -D warnings`, `cargo test -p rust-engineering-mcp --test cli` | Ejecutadas por W03 (verdes) tras el cambio del literal de `--help` | Único cambio en `crates/` de M8-01; gate `core` sobre bytes finales en la integración de M8-02 |
| `gate.py core` / `full` | **not run** todavía | Se ejecutan al integrar cortes con cambios en `crates/`/`scripts/`; el recibo M6 `69a0be14` acredita los bytes actuales de `main` |
| Matrices de clientes | **not run** | M8-04 |

## Revisiones

| Paquete | Revisor | Veredicto | Disposición |
| --- | --- | --- | --- |
| W01 censo + `01.md` | Gemini 3.8 Flash High (read-only) | Approve con findings (F10–F13) | [R01](delegation/R01-census-traceability/disposition.md): todos reproducidos y corregidos en W01b |
| W01/W02/W03/W01b + `01.md` | Claude Sonnet 5 (High, read-only) | Block (3 P1, 5 P2, 5 P3) | [V01](delegation/V01-review-census/disposition.md): F-A/F-D/F-G/F-M + P3 aceptados (W04, `01.md`); F-J/F-K/F-L rechazados con evidencia |
| Diff W04 + `01.md` final | Claude Sonnet 5 (Medium, read-only) | Approve con findings | [V01b](delegation/V01b-rereview/disposition.md): P1 verificado como no defecto (flag existe); P3 duplicidad → M8-02 |

## Deuda heredada relevante (M6 → M8)

Ver [M6 matrix §Deuda](../M6/matrix.md#deuda-de-m6): diagnósticos semánticos
(Opción B, decisión owner), assist-readiness no determinista y liberación
asíncrona de capacidad (`SANDBOX_DENIED` permanente vs transitorio → código
retryable), re-gatear `analyzer_runtime.rs` (W05f), precisión de `admitted`
post-grant. Windows x86_64 retirado del CI (regresión stdio pre-`initialize`,
alcance D13).
