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
| D12 migraciones/rollback | **Decidida** 2026-09-14 (orquestador, autorización del owner) | [03.md](03.md) → [ADR-088](../../adr/ADR-088-migration-rollback-policy.md) |
| D13 targets 1.0 | **Decidida: A** (owner, 2026-09-14) | [brief D13](delegation/D13-scope-brief.md) → [ADR-087](../../adr/ADR-087-1.0-host-scope.md); spec §61/§97 aclarados |
| D14 distribución/provenance offline | **Decidida** 2026-09-15 (orquestador, autorización del owner) | [07.md](07.md) → [ADR-090](../../adr/ADR-090-offline-verification-and-incident-response.md) |

## Cortes y evidencia

| ID | Corte | Estado | Evidencia |
| --- | --- | --- | --- |
| M8-01 | Censo de invocaciones reales → contratos/errores/CLI/Resources/formatos → clasificación; gate de superficie; huérfanos = 0 | **Done local** (commits `9ecd945`, `2e2e75d`, `e445aa1`): 36 tools, 0 huérfanos, 2 Resources dinámicas, 15 CLI, 10 formatos; 31 `stable` / 5 `preview`; 0 consolidaciones; 11 findings dispuestos; `--help` y docs públicas corregidos | [01.md](01.md) (decisiones), [01-census.md](01-census.md), [01-census.json](01-census.json); revisiones R01/V01/V01b |
| M8-02 | Freeze 0.8.x con migration notes; before/after schema/behavior; trece M1 sin escritura implícita | **Done local** (commits `1bdb61b`…`b7f0528` + evidencia I02b; gate `core` verde [core-gate.json](core-gate.json) `sha256:f2c2fe69…`, 28 etapas, `source_inputs_unchanged: true`; V02/V02b dispuestos; [intentos de gate](02-gate-attempts.md): 2 fallos clasificados (B) + 1 verde): versión 0.8.0; `contract [--json]` (spec §56); `Preview (ADR-086): ` en 5 descripciones (31 snapshots byte-idénticos); manifiesto `freeze-0.8.0.json` + etapa `contract-freeze` del gate; before/after: 13 M1 = `v0.1.0`, desde `v0.3.0` solo `binary.bloat` (`$defs` description); 0 deprecaciones; ADR-087 (D13 = A) | [02.md](02.md), [02-schema-diff.json](02-schema-diff.json), [freeze-0.8.0.json](freeze-0.8.0.json), CHANGELOG «Migration notes 0.3.0 → 0.8.0» |
| M8-03 | Migración con preflight/dry-run/backup/rollback; floors/trust no retroceden; journal pendiente impide downgrade (D12) | **Done local (pendiente de V03 + gate + commit)**: D12 → ADR-088 (0 formatos a migrar; sin CLI nueva); `doctor.mutation_journals` (preflight pasivo); fixture de permisos revocados; **prueba real con dos binarios** `v0.3.0` ↔ `0.8.0` 4/4 (`03-rollback.json`); procedimiento backup/rollback documentado | [03.md](03.md), [03-formats-analysis.md](03-formats-analysis.md), [03-rollback.json](03-rollback.json), [ADR-088](../../adr/ADR-088-migration-rollback-policy.md) |
| M8-04 | Matriz wire/cliente (5 revisiones + stdio; Inspector + Codex obligatorios; Claude Code/Gemini CLI calificados) | **Done local**: Docker-free + `--with-runtime` ([clients.json](clients.json) attempt-22: Inspector docker_free y runtime `passed` incl. cancel/EOF/Resource, Codex y Claude Code `passed`, Gemini `unavailable` no anunciado; composición M2–M6 `failed` por pines de cliente antiguos — declarado, no pass); el primer `--run` destapó un **defecto wire real**: `resources/list` sin `ttlMs`/`cacheScope` en 2026-07-28 (Inspector SDK lo rechaza) → W21 implementa las tres listas + tests; luego `--run --with-runtime` | [clients.json](clients.json) (pendiente) |
| M8-05 | Presupuestos de performance desde M5; soak con límites fijados antes de medir | **Presupuestos fijados antes de medir** ([05.md](05.md), `05-budgets.json`); arnés `measure-m8-performance.py` + `soak-m8.py` (W18/W20); primera medición `core` N=30 todo `within` (con ruido de host; recalibración única en host quieto antes de RC1); soak calibración 2 `passed` (30 ciclos); magnitudes Docker pendientes (M8-09) | [05-measurement.json](05-measurement.json), [05-budgets-analysis.md](05-budgets-analysis.md) |
| M8-06 | Guías públicas reproducidas por tercero; sin texto histórico contradictorio | Planned | — |
| M8-07 | Artifacts por target, SBOM/notices/firma/provenance, smoke desde descarga limpia (D13/D14) | **Ensayo local Done** (W24): archive core 0.8.0 (10,4 MB, dentro del presupuesto 12,7 MB), inventario/SBOM/notices, smoke desde directorio limpio 13/13; defecto real en `release-smoke.py` (2 hashes) corregido; workflow RC sin literal `31`; D14 → ADR-090. **Pendiente para RC1** (requiere tag/autorización): attestations OIDC, redescarga desde GitHub, cadena tag/run/digest | [07.md](07.md), [07-release-rehearsal.json](07-release-rehearsal.json), [ADR-090](../../adr/ADR-090-offline-verification-and-incident-response.md) |
| M8-08 | Threat model completo + auditoría independiente (paquete Opus High; modelo ≠ humano ≠ pentest) | **Threat model + registro de riesgos Done** (W22, Opus High): 8 fronteras, 53 controles (6 sin oráculo declarados), RR-01…RR-18, ADR-089 Accepted; auditoría = revisiones de modelo (V02/V02b/V03/V03b + V04 de cierre pendiente); **sin auditoría humana ni pentest** (RR-01) | [08.md](08.md), [08-threat-model.md](08-threat-model.md), [ADR-089](../../adr/ADR-089-residual-risk-register.md) |
| M8-09 | Dos RC consecutivos 0.9 con contrato igual, full gate y soak verdes | Planned | — |

## Checklist 1.0

Ver [checklist-1.0.md](checklist-1.0.md) (borrador vivo con recibos por casilla).

## Pruebas ejecutadas y no ejecutadas (§4 del encargo)

| Verificación | Estado | Motivo |
| --- | --- | --- |
| `docs-hygiene.py links-check` / `verify-inventories` | Ejecutadas en la entrada y tras cada worker (0 rotos / 0 fallos) | Obligatorias antes de cada commit |
| `cargo fmt --check`, `cargo clippy -p rust-engineering-mcp -D warnings`, `cargo test -p rust-engineering-mcp --test cli` | Ejecutadas por W03 (verdes) tras el cambio del literal de `--help` | Único cambio en `crates/` de M8-01; gate `core` sobre bytes finales en la integración de M8-02 |
| `gate.py core` (bytes finales M8-02) | **Verde al 3.er intento** (2026-09-14 21:18 UTC): [core-gate.json](core-gate.json) `sha256:f2c2fe69d32ef786229de665c127f8f3ee405b04d7203ec984f3416f666617eb`, 28/28 etapas, `source_inputs_unchanged: true`. Intentos 1–2 fallaron en `protocol.rs::closed_stdout_exits_even_when_stdin_remains_open` (timeout 10 s), clasificados **(B)** con criterio previo y 14 reproducciones documentadas en [02-gate-attempts.md](02-gate-attempts.md); riesgo residual registrado | M8-02 toca `crates/mcp-server`, `scripts/`, `.github/`, `Cargo.*`; un intento previo abortado por el orquestador antes de que W10b tocara `scripts/` |
| `gate.py core` (bytes finales M8-03/04/05/07/08, 2026-09-15T02:55:11Z) | **Verde** al primer intento tras W30: [core-gate-m8-03-08.json](core-gate-m8-03-08.json) `sha256:aa58c975b180b73ddf742e15ae9cdac72539aade83b283d9d8a772ad3b82f41f`, 31 etapas (28 + `m8-performance-unit-tests`, `m8-client-harness-tests`, `contract-freeze` ya contadas), `source_inputs_unchanged: true` | Un intento previo abortado por el orquestador para incorporar W30 (V03b) |
| `gate.py core` (bytes finales de M8, HEAD `7c477db`) | **Verde** (2026-09-15 19:13 UTC): [core-gate-final.json](core-gate-final.json) `sha256:ed7ce9102bd8e940c58ddb7c6abcbb34c6af612718e228892c54be95e6e4550d`, 30/30 etapas, `source_inputs_unchanged: true` | Recibo de cierre; binario `release` reconstruido antes (`aeaa334c…`) |
| `gate.py core` (bytes finales de M8, HEAD `c0c73f39`) | **Verde al primer intento** (2026-09-18 15:12 UTC): [core-gate-final.json](core-gate-final.json) `sha256:9023c3c93f674ad04bc535d9c2c6508aebfe3e0b220f01dd26abf0c6786f8e64`, 30/30 etapas, `source_inputs_unchanged: true`, binario `sha256:f5205305…`. Corrió sobre **macOS 27.0 con Xcode completo**, no sobre el macOS 26.6.2 + Command Line Tools de los recibos anteriores | Recibo de cierre; `codex-qualifier-tests` pasó a la primera tras W44 |
| `gate.py full` (cierre, Docker/E5/ORT) | **Parcial — límite declarado, no es pass** (2026-09-18 15:21 UTC): [full-gate.json](full-gate.json) `sha256:9f37dca710c50556919f3a029ec8fe1a4eede0199a40a03b4cb938c4f5f43065`. **31 etapas pasadas, 1 fallida (`rust-security`), 14 nunca ejecutadas** (`m2-runtime`, `m3-runtime`, `m4-tampered-plugin`, `m4-inventory`, `m4-runtime`, `audit-data`, `semantic`, `catalog`, `catalog-status`, `crate-search`, `crate-inspect`, `doctor`, `m5-runtime`, `m6-runtime`): el gate aborta al primer fallo | **Causa: tres imágenes Docker aprobadas fueron borradas del host por error** (`APPROVED_RUST_IMAGE` `384a1742…`, `APPROVED_M4_IMAGE` `25ed3626…`, `APPROVED_M5_IMAGE` `e0a5ca16…`; solo sobrevive `APPROVED_M6_IMAGE` `f39a5b33…`). `docker image inspect` devuelve vacío → `images.first()` es `None` → `lib.rs:237` lo mapea a `InvalidConfiguration`. **No es defecto de producto**: `crates/execution-adapter/` es byte-idéntico a `origin/main` y el sondeo del sandbox verificó las nueve capacidades contra Docker 29.7.2. Recuperación: re-provisionar con `fixtures/rust-runtime/provision.py`, que **no reproduce los digests** (el README lo declara explícitamente: «this is not a detached signature or reproducible-build claim»), por lo que exige actualizar y recualificar las tres constantes aprobadas. **Decisión del owner (2026-09-18): se difiere junto con el soak a antes de 1.0.0** |
| Matrices de clientes | Docker-free y `--with-runtime` **ejecutadas** (`clients.json` attempt-22) | M8-04 |

## Revisiones

| Paquete | Revisor | Veredicto | Disposición |
| --- | --- | --- | --- |
| W01 censo + `01.md` | Gemini 3.8 Flash High (read-only) | Approve con findings (F10–F13) | [R01](delegation/R01-census-traceability/disposition.md): todos reproducidos y corregidos en W01b |
| W01/W02/W03/W01b + `01.md` | Claude Sonnet 5 (High, read-only) | Block (3 P1, 5 P2, 5 P3) | [V01](delegation/V01-review-census/disposition.md): F-A/F-D/F-G/F-M + P3 aceptados (W04, `01.md`); F-J/F-K/F-L rechazados con evidencia |
| Diff W04 + `01.md` final | Claude Sonnet 5 (Medium, read-only) | Approve con findings | [V01b](delegation/V01b-rereview/disposition.md): P1 verificado como no defecto (flag existe); P3 duplicidad → M8-02 |
| Freeze 0.8.0 (W05/W06/W07/W08) | Claude Opus 5 (High, read-only Read/Grep/Glob) | Block (4 P2 contrato/gate, P3) | [V02](delegation/V02-review-freeze/disposition.md): todos aceptados → W09 (Rust) + W10 (scripts/docs) |
| Correcciones W09/W10 | Claude Opus 5 (Medium, read-only) | Block (1 P2 nuevo: test no hermético) | [V02b](delegation/V02b-rereview/disposition.md): 4 P2 de V02 confirmados cerrados; P2-N1 → W10b (hermético, verificado con `GIT_DIR=/nonexistent`) |
| M8-03/04/05 (W14/W15/W18–W21) | Claude Opus 5 (High, read-only) | Block (1 P1 `downgrade_blocked`, 12 P2) | [V03](delegation/V03-review-m8-03-04-05/disposition.md): todos aceptados → W25/W26/W27/W27b/W28; recibos regenerados post-commit |
| Censo M8-01 (trazabilidad) | Gemini 3.8 Flash High | Approve con findings | [R01](delegation/R01-census-traceability/disposition.md) |

## Deuda propia de M8 (trazada)

| Origen | Deuda |
| --- | --- |
| V02 P3 | Listas duplicadas: orden de tools en `tool_definitions()` vs `list_tools`; nombres `preview` en `stability.rs`, `tests/cli.rs` y `contract-freeze.py` (cadena cubierta de extremo a extremo por protocol tests ↔ snapshots ↔ cli.rs ↔ manifiesto); unificar cuando se toque el registro |
| V02b P3 | `contract`: fallo al escribir stdout sin stderr; `verify` no contrasta `stable_count`/`preview_count`; renombrado de una `preview` con mismo conteo pasa sin `--strict` (M8-09 usa `--strict`) |
| V01b P3 | Nota `preview` con fuente única en `client-configuration.md` (hecho en W08) |
| M8-01 F8 | Conteos de etapas de `docs/ci.md` se corrigen con el recibo `core` de M8-02 |

## Deuda heredada relevante (M6 → M8)

Ver [M6 matrix §Deuda](../M6/matrix.md#deuda-de-m6): diagnósticos semánticos
(Opción B, decisión owner), assist-readiness no determinista y liberación
asíncrona de capacidad (`SANDBOX_DENIED` permanente vs transitorio → código
retryable), re-gatear `analyzer_runtime.rs` (W05f), precisión de `admitted`
post-grant. Windows x86_64 retirado del CI (regresión stdio pre-`initialize`,
alcance D13).
