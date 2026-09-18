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

## Cierre de la sesión del 2026-09-18 (Claude Opus 5)

Rama `ai/m8-stabilization` pusheada, PR #22 con los **9 checks en verde** (macOS,
Linux, supply chain, CodeQL actions/python/rust, SonarCloud). Paquetes de la
sesión: W38, W39, W39b, W40, W41 (commiteados del árbol anterior) más **W42**,
**W43** y **W44** nuevos.

### SonarCloud: deuda de taint cerrada 22 → 6 → 2 → 0

Quality gate **OK**: seguridad A, cobertura 91,7 % (venía de 60,7 %), duplicación
2,0 %, hotspots 100 %, **0 vulnerabilidades / 0 bugs / 0 code smells**. Sin excluir
un solo archivo ni usar `# NOSONAR`.

Hicieron falta tres rondas porque cada una destapó una **forma distinta** del mismo
principio, no un resto de la anterior. La regla general, que es lo que evita una
cuarta ronda: **el motor no acepta «lo validé más arriba»; solo corta el flujo
cuando el valor que se usa se re-deriva de un dominio cerrado** (`match.group(0)`,
pertenencia a una tupla constante, `int()`/`float()`, reconstrucción campo a campo
contra un esquema). W38 cubrió rutas desde argv; W42, contenido que llega al recibo
desde argv/stdin/disco; W43, el contenido que el script relee de su propio recibo.

Efecto colateral valioso: `contract-freeze.py` y `measure-m8-performance.py` ahora
**validan sus propios recibos** al releerlos, y un snapshot con nombre o anotación
desconocidos falla ruidosamente en vez de entrar al manifiesto.

### Gates de cierre

- **`core` verde al primer intento** (2026-09-18 15:12 UTC), 30/30,
  `source_inputs_unchanged: true`:
  [core-gate-final.json](core-gate-final.json)
  `sha256:9023c3c9…`, binario `sha256:f5205305…`, HEAD `c0c73f39`.
- **`full` parcial — límite declarado**: 31 pasadas, 1 fallida, **14 sin ejecutar**.
  Ver [matrix.md](matrix.md) §Pruebas. Difiere a antes de 1.0.0 por decisión del owner.
- **Soak 8 h no ejecutado**, por decisión del owner (2026-09-18): se hará antes de
  1.0.0. Nunca se ha completado; `05-soak-core.json` no existe. **No es pass.**

### Host: dos cambios que invalidan la comparación con recibos anteriores

Los gates de cierre corrieron sobre **macOS 27.0 con Xcode completo**
(`xcode-select` → `/Applications/Xcode.app`, Apple clang 21.0.0), no sobre el
macOS 26.6.2 con Command Line Tools de los recibos previos. La licencia de Xcode,
revocada por el salto de versión, la aceptó el owner durante la sesión; la
mitigación de `SDKROOT`/`DEVELOPER_DIR` y los wrappers de `git`/`cc` se retiraron
y **no** están en vigor en estos recibos.

El salto de SO destapó **W44**, un defecto latente del arnés del qualifier: un
proceso **zombi** se contaba como vivo (`os.kill(pid,0)` tiene éxito sobre un
zombi mientras `proc_pidpath` ya da ESRCH), con una tasa de fallo medida de 5/14.
La carrera producía **falsos fallos, nunca falsos aciertos**, así que los recibos
de M8-04 anteriores siguen siendo válidos.

### Branch protection: bloqueante (a) de V04 F-07, confirmado y corregido

Re-observada en vivo: `main` exigía el contexto `portable / x86_64-pc-windows-msvc`,
retirado en `9cd6c634`, que **jamás podía reportar** — el merge estaba bloqueado de
forma permanente por la vía normal. Corregido el 2026-09-18 retirando ese contexto.
`strict: true`, 1 review y `enforce_admins: false` se mantienen.

### Deuda conocida (además de la ya registrada)

- **Tres imágenes Docker aprobadas borradas del host por error**
  (`APPROVED_RUST_IMAGE`, `APPROVED_M4_IMAGE`, `APPROVED_M5_IMAGE`; sobrevive
  `APPROVED_M6_IMAGE`). Recuperarlas exige re-provisionar, lo que **no reproduce
  los digests** y por tanto obliga a actualizar y recualificar tres constantes de
  identidad aprobada en `crates/execution-adapter`. Estimado 3–7 h. Va con el soak,
  antes de 1.0.0.
- El doble de test de `codex-model-qualifier` depende de `/usr/bin/python3` del
  sistema (shebang `#!/usr/bin/env python3` bajo `PATH=/usr/bin:/bin` fijo). Con el
  host sano no molesta, pero es un defecto de portabilidad latente.
- `tool_entry` pasa el nombre de la tool donde la firma anota `source: pathlib.Path`
  (solo afecta al mensaje de error). Corregir cuando se toque el archivo.
- Deuda previa que sigue vigente: composición de arneses M2–M6 no repetible;
  Gemini CLI no calificado; `-32601` en `resources/read` malformado;
  `measure-m8-performance.py` perdió el modo semántico E5/ORT al quitarle los flags
  de ruta (W38).

### Readiness

**Not ready**, como se esperaba hasta RC2. La decisión es del Technical Owner y
depende de cerrar las filas 6, 7, 10 y 11 de [checklist-1.0.md](checklist-1.0.md).

## Siguiente

Owner: autorizar push/PR de la rama y el tag `v0.9.0-rc.1` sobre el commit
final (M8-09); revisar RR-01 (auditoría humana) y la promoción de las 5
`preview` (Opción B, código retryable de capacidad).
