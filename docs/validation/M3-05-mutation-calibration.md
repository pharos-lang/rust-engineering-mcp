# M3-05 — calibración de `rust.mutation.test`

Fecha: 2026-09-06. Estado: **calificado 10/10 en Docker**.

La calificación usó `cargo-mutants 27.1.0` en la imagen aprobada
`sha256:384a1742ecc53cdd3a9c0bf36c6f8b66db73ddd118aeeae6e55654ea998ae36a`.
El recibo consolidado es [`M3-runtime.json`](M3-runtime.json). Las diez
selecciones exactas pasaron en 218.572 s en el intento final.

## Exit codes observados

| Exit | Evidencia observada | Clasificación fijada |
| ---: | --- | --- |
| 0 | `caught-all` (14/14 caught); `unviable` (1/1 unviable) | success; la aplicación solo acredita limpio si no hay missed/timeout/unviable |
| 1 | primer intento real: fuente copiada con modo 0444, cargo-mutants no pudo sobrescribir `src/lib.rs` | usage/infrastructure; nunca verdict de mutantes |
| 2 | `missed-one` (6 missed, 13 caught); hostile-output (4 missed) | missed |
| 3 | `timeout-loop` (2 timeout, 4 caught) | timeout/incomplete, nunca clean |
| 4 | `baseline-failing` (cero mutantes ejecutados) | baseline failed |

El exit 1 descubrió un defecto real de integración, no un fixture de producto: el
archivo regular de la copia privada se codificaba 0444. El encoder específico de
mutation ahora usa 0644 únicamente dentro del tar ingerido al scratch tmpfs;
`/source` sigue read-only, la copia nunca se exporta y el encoder general conserva
0444. El unit test `mutation_archive_changes_only_regular_file_mode` pincha esa
diferencia.

## `mutants.out` calibrado

`outcomes.json` 27.1.0 contiene los campos top-level `outcomes`, `total_mutants`,
`missed`, `caught`, `timeout`, `unviable`, `success`, `start_time`, `end_time` y
`cargo_mutants_version`. Cada registro contiene `scenario` (`Baseline` o un objeto
externamente tagged `Mutant`), `summary` (`Success`, `CaughtMutant`,
`MissedMutant`, `Timeout`, `Unviable` o `Failure`) y resultados de fase. El parser
acotado y `MutationExit::CALIBRATED = true` quedaron fijados contra esa forma.

El bundle USTAR observado contiene `outcomes.json`, `caught.txt`, `missed.txt`,
`timeout.txt`, `unviable.txt`, `log/` y `diff/`; `lock.json` se excluye siempre.
El caso grande observado tuvo 49 entries, por debajo de los límites 512 USTAR y
128 miembros Stage 1. Los conteos de los cuatro `.txt` se verifican contra
`outcomes.json`; cualquier contradicción es evidencia incompleta.

`lock.json` se parseó dentro del guest y todos los runs completos proyectaron
`identity: Guest`: hostname exacto `sandbox` y username dentro del allowlist
cerrado. Los valores crudos se descartan y `lock.json` nunca se publica, de modo que
el username exacto no forma parte deliberadamente del recibo.

## Oracles reales

| Fixture / control | Observación |
| --- | --- |
| `caught-all` | 14 generated, 14 caught, exit 0, 39 bundle entries |
| `missed-one` | 19 generated, 6 missed, 13 caught, exit 2, 49 entries |
| `timeout-loop` | 6 generated, 2 timeout, 4 caught, exit 3, 23 entries |
| `unviable` | 1 generated, 1 unviable, exit 0, 13 entries |
| `baseline-failing` | 3 listed, 0 ejecutados, exit 4, 11 entries |
| `hostile-output` | 4 listed/missed, exit 2, marker solo en log opaco, nunca usado como oracle |
| `max_mutants=1` | listado se detiene al segundo item; sin build, outcomes ni bundle |
| cancelación | hijo y cleanup unidos; segunda ejecución en el mismo gateway pasa |
| source/canary | ambos byte-idénticos después de cada run |

El `caught-all` necesitó agregar el límite `0` al oracle; sin él un mutante real
`>`→`>=` sobrevivía correctamente. No se debilitó la aserción.

## Shape de ejecución confirmado

Se mantiene la decisión I05: `--baseline run`; pre-pass `--list --json` para
`max_mutants`; `/source` read-only; copia mutable privada en tmpfs de contenedor
`/mutants-scratch`; volumen nombrado solo para reportes; tres exporters tar con
argv fijo; red none y `seccomp-rust-quality.json`. El bundle nunca se extrae a una
ruta host. Con Tasks aún sin anunciar, el contrato público sigue devolviendo
`TASKS_REQUIRED` para estas ejecuciones largas; esta calificación ejercita el port
real directamente.
