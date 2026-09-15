# M8 — handoff (estabilización 0.8 → readiness 1.0)

Estado: **borrador de cierre** (2026-09-15). Rama `ai/m8-stabilization` desde
`main` `e50c3fe` (merge de M6). Sin push, PR, tag, RC ni release (autorización
separada del owner). Orquestador: Claude Fable 5.1; código/tests/scripts/ADRs/
docs por agentes externos acreditados en [delegation/README.md](delegation/README.md)
(Sonnet/Opus 5 vía `claude -p`, Gemini 3.8 Flash vía `agy`; Codex solo como
cliente stock). Autorización del owner (2026-09-14): continuar hasta cerrar M8
asumiendo las decisiones para una primera versión estable en macOS.

## Qué se entregó (por corte)

| Corte | Resultado | Evidencia |
| --- | --- | --- |
| M8-01 | Censo de 36 tools/2 Resources/15 CLI/10 formatos; 31 `stable` / 5 `preview`; 0 consolidaciones; `--help` y docs corregidos | [01.md](01.md), [01-census.json](01-census.json) |
| M8-02 | Freeze 0.8.0: manifiesto canónico + etapa de gate; `contract [--json]` (spec §56); prefijo `Preview`; migration notes; 13 M1 = 0.1.0, 30 `stable` = 0.3.0 | [02.md](02.md), [freeze-0.8.0.json](freeze-0.8.0.json) |
| M8-03 | D12/ADR-088: 0 formatos a migrar; `doctor.mutation_journals`; rollback/upgrade real `v0.3.0` ↔ `0.8.0` 4/4; backup/rollback documentados | [03.md](03.md), [03-rollback.json](03-rollback.json) |
| M8-04 | Wire 5 revisiones (gate) + defecto real corregido (listas sin `ttlMs`/`cacheScope`); Inspector docker_free y runtime, Codex, Claude Code `passed`; Gemini no calificado | [04.md](04.md), [clients.json](clients.json) |
| M8-05 | Presupuestos fijados antes de medir y recalibrados una vez; medición N=30 `within`; soak `core` 8 h (en curso al cierre; ver §Soak) | [05.md](05.md), [05-measurement.json](05-measurement.json) |
| M8-06 | Reproducción por tercero (parcial → desviaciones corregidas: README, CHANGELOG, `mutation list`) | [06.md](06.md), [06-reproduction.md](06-reproduction.md) |
| M8-07 | D13 = A (ADR-087) + D14 (ADR-090); ensayo local de archive/SBOM/notices/smoke; workflow RC sin literal | [07.md](07.md), [07-release-rehearsal.json](07-release-rehearsal.json) |
| M8-08 | Threat model (8 fronteras, 53 controles), RR-01…RR-19 (ADR-089); auditoría de cierre V04 (modelo, 0 P0/P1/P2) | [08.md](08.md), [08-threat-model.md](08-threat-model.md) |
| M8-09 | **No iniciado**: RC1/RC2 requieren tags autorizados por el owner | — |

## Decisiones

D11 → ADR-086; D12 → ADR-088; D13 = A (owner) → ADR-087; D14 → ADR-090;
riesgos residuales → ADR-089. Presupuestos y soak: [05.md](05.md).

## Gates y matrices

Ver [matrix.md](matrix.md) §Pruebas ejecutadas: `core` verde por corte
(M8-02 `f2c2fe69…`, M8-03..08 `aa58c975…`, final `ed7ce9102bd8e940c58ddb7c6abcbb34c6af612718e228892c54be95e6e4550d` sobre `7c477db`),
`full` al cierre (pendiente), clientes `attempt-22`, rollback, ensayo de release.

## Readiness 1.0

[checklist-1.0.md](checklist-1.0.md): **not ready** hasta M8-09 (dos RC con
tags autorizados, `full` + soak verdes por RC, attestations verificadas desde
assets descargados). La decisión es del Technical Owner por evidencia.

## Riesgos y rollback

RR-01…RR-19 (ADR-089); flake conocido `closed_stdout_exits_even_when_stdin_remains_open`
bajo el gate (clasificado, [02-gate-attempts.md](02-gate-attempts.md)); Gemini CLI
no calificado; composición de arneses M2–M6 no repetible con los clientes
actuales. Rollback de producto: binario anterior + [ADR-088](../../adr/ADR-088-migration-rollback-policy.md)
(journal pendiente ⇒ resolver con 0.8.0 antes de bajar).

## Siguiente

Owner: autorizar push/PR de la rama y el tag `v0.9.0-rc.1` sobre el commit
final (M8-09); revisar RR-01 (auditoría humana) y la promoción de las 5
`preview` (Opción B, código retryable de capacidad).
