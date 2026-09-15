# W27 — correcciones V03 de performance/soak (P-1..P-9)

## Task

Aplicar la disposición del orquestador sobre
`docs/validation/M8/delegation/V03-review-m8-03-04-05/disposition.md` (filas
P-1..P-9) y `report.md` §4 a `scripts/measure-m8-performance.py`,
`scripts/soak-m8.py` y `scripts/test-m8-performance-unit.py`. Sin tocar
`05-measurement.json` (lo regenera el orquestador) ni valores de
`05-budgets.json`.

## Result

Los 9 hallazgos están aplicados. `python3 -B scripts/test-m8-performance-unit.py`
queda **OK** (76 tests, todos nuevos o migrados de la suite previa). Las dos
ejecuciones reales pedidas corrieron en foreground contra el binario release
existente:

- `python3 -B scripts/measure-m8-performance.py --repeat 5 --out
  target/m8-perf-smoke.json` → `verdict=insufficient_samples` (esperado:
  `--repeat 5 < n=30` de la mayoría de las filas del presupuesto; ver P-3).
- `python3 -B scripts/soak-m8.py --profile core --cycles 20 --hours 0.05
  --sample-every 5 --out target/m8-soak-smoke.json` → `status=failed` por
  `fd_after_ttl` (ver más abajo: es un resultado medido, no un defecto del
  arnés).

## Por hallazgo

**P-1 (respuestas no validadas).** Nueva `validate_tool_result(response,
tool_name)` en ambos scripts: exige `result.isError != true` y
`result.structuredContent.status == "passed"`.
- `measure-m8-performance.py`: `collect_valid_dispatch_samples()` reintenta
  hasta reunir `repeat` muestras válidas por tool, registrando cada descarte
  (`{tool, attempt, reason}`) en `discarded_samples` (con `phase: "dispatch"`,
  junto a los descartes de arranque que ya existían con `phase: "startup"`).
  Si no reúne `repeat` válidas dentro de `(repeat+1)*3` intentos, falla con
  `RuntimeError` explícito en vez de contaminar las estadísticas.
- `soak-m8.py`: `evaluate_cycle_calls()` valida las dos llamadas de cada ciclo
  y registra los descartes en el nuevo `call_discards[]` del recibo (con
  `cycle`); el soak es una prueba de resistencia, así que un descarte se
  **registra**, no aborta el ciclo (a diferencia del measure, que sí exige
  reintento porque calcula percentiles).

**P-2 (regla 2-de-3).** `regression_verdict()` ahora exige exactamente 3
recibos (`len(receipts) != 3` → `ValueError`, antes aceptaba 1–3), exige el
mismo `budgets_sha256` y el mismo `profile` en los 3, y por magnitud calcula
`over_count`/`unavailable_count` para devolver `outcome ∈ {regressed,
not_regressed, indeterminate}`: `indeterminate` solo cuando un `unavailable`
podría cambiar el resultado (`over_count < 2 ≤ over_count + unavailable_count`).
Nuevo CLI `--compare R1 R2 R3` (con `--out` opcional) → `run_compare()`
escribe `rust-mcp-m8-performance-regression-v1` y sale con código 1 si algún
`outcome == "regressed"`.

**P-3 (n insuficiente).** `summarize()` compara `len(values)` contra
`budget_row["n"]` **antes** de calcular el estadístico; si faltan muestras
devuelve `status/verdict = "insufficient_samples"` (nunca `within`).
`global_verdict()` prioriza `over > insufficient_samples > unavailable >
within`. Para RSS pico, `measure_dispatch_and_peak()` ahora exige una ventana
mínima de `RSS_PEAK_MIN_SAMPLES = 30` muestras a 100 ms: tras el bucle de
dispatch, espera (hasta un tope de `30 × 0,1s × 3`) a que el sampler acumule
30 muestras antes de detenerlo.

**P-4 (recalibración/recibo).** El recibo de `measure-m8-performance.py` ahora
incluye `head_tree_dirty` (via `git status --porcelain`) y `budgets_sha256`
(sha256 real del archivo de presupuestos leído, no un valor declarado). La
actualización de `05.md` con el resultado de la recalibración y el enlace a
estos shas queda para el orquestador (regenera sobre bytes commiteados), tal
como fija la disposición.

**P-5 (soak sin cobertura del churn).** El soak arranca el servidor con
`--project-ttl-secs` (nuevo flag, default 30s, propagado al binario).
`--ttl-wait-seconds` cambia su default de `0` a `35`. Nuevo criterio real
`fd_after_ttl` (`evaluate_fd_after_ttl()`): tras el churn y la espera, exige
`fd_count_after_ttl_wait ≤ plateau_fds + 10`; si no aplica (sin churn o sin
espera) queda `applicable: false, passed: true` (hueco honesto, no falso
pase). Se retiró la afirmación «bounded by the idle TTL... not a leak» del
docstring del módulo y de las notas del recibo; ambas ahora citan el número
medido y el veredicto de `fd_after_ttl`.

**P-6 (`state_root_orphans` por decreto).** Se mantiene `state_root_orphans`
con `applicable: false` (sigue siendo un hueco honesto: el perfil `core` no
tiene `--state-root`/Docker). Se añadió un criterio nuevo y real,
`catalog_store_orphans` (`check_catalog_store_orphans()`), que sí inspecciona
el directorio del catalog-store que el soak usa de verdad: compara las
entradas contra `{active.bundle, store.lock, floor.record}` (los tres
nombres reales que escribe `catalog import`, confirmados en
`crates/mcp-server/src/stdio/catalog/provider.rs` y en el módulo `macos` de
`crates/project-adapter/src/catalog_store.rs`) y falla si aparece cualquier
otro archivo.

**P-7 (controles de ruido constantes).** Ambos scripts re-hashean el binario
al final de la corrida (`binary_sha256_end`) y comparan contra el hash
inicial (`same_binary_all_samples` ahora es una comparación real, no `True`
fijo). Nuevo flag `--operator-attested` (default `false`) en ambos, expuesto
en el recibo (`noise_controls.operator_attested` / `operator_attested`).
Nueva `battery_status()` en ambos: ejecuta `pmset -g batt` si existe
`/usr/bin/pmset` y publica la salida cruda (`pmset_batt`), o `None` si no
está disponible.

**P-8 (detalles del soak).**
- `reopens` real: `should_reopen_project_ref(elapsed, ttl)` decide, en cada
  ciclo, si reabrir el `project_ref` porque el TTL del servidor ya venció
  (única señal disponible: `rust.catalog.status`/`rust.crate.search` no
  consumen `project_ref`, así que no hay forma de comprobar vigencia sin
  asumir expiración por reloj). Con `--project-ttl-secs 30` por defecto esto
  ya no es hipotético: la corrida de humo (abajo) registró 4 reaperturas
  reales en ~180 s.
- `request()` de `soak-m8.py` ahora comprueba `response["id"] ==
  identifier`, igual que `measure-m8-performance.py`.
- La muestra final ya usaba `complete_samples[-1]` (filtrado de `None` antes
  de invocar `evaluate_criteria`), así que ya no evaluaba una muestra
  anterior "stale"; se dejó sin cambios porque ya era correcto.

**P-9 (cobertura Sonar).** Ninguno de los dos scripts está en
`sonar.coverage.exclusions` y no se tocó `sonar-project.properties`. Toda la
lógica nueva de decisión se extrajo a funciones puras directamente probadas
sin mocks de proceso: `validate_tool_result`, `evaluate_cycle_calls`,
`collect_valid_dispatch_samples` (con una función `call` inyectada),
`should_reopen_project_ref`, `evaluate_fd_after_ttl`,
`check_catalog_store_orphans`, `file_sha256`, `head_tree_dirty`,
`battery_status`, `run_compare`, y la rama `insufficient_samples` de
`summarize`/`global_verdict`. El bucle de I/O que queda en
`measure_dispatch_and_peak`/`run_core_soak` es delgado (llama a las
funciones puras de arriba); no se pudo medir con `coverage.py` en este
entorno (paquete instalado sin `Coverage` utilizable), así que la garantía de
≥80% se apoya en el diseño (la lógica nueva vive en funciones puras 100%
cubiertas) más que en un número de herramienta — señalado aquí para que el
orquestador lo confirme con su propio `coverage` si lo requiere.

## Files changed

- `scripts/measure-m8-performance.py`: `validate_tool_result`,
  `collect_valid_dispatch_samples`/`_dispatch_call`, ventana mínima de 30
  muestras de RSS pico, `summarize` con rama `insufficient_samples`,
  `global_verdict` con la nueva prioridad, `regression_verdict` (3 recibos,
  `budgets_sha256`/`profile`, `indeterminate`), `run_compare` + `--compare`,
  `file_sha256`/`head_tree_dirty`/`battery_status`, recibo con
  `budgets_sha256`, `head_tree_dirty`, `binary_sha256_end`,
  `noise_controls.same_binary_all_samples` real, `operator_attested`,
  `pmset_batt`.
- `scripts/soak-m8.py`: `validate_tool_result`, `evaluate_cycle_calls`,
  `run_cycle` devuelve `(elapsed_ms, discards)`, `should_reopen_project_ref` +
  reapertura real en `run_core_soak`, `--project-ttl-secs` (default 30,
  propagado al binario), `--ttl-wait-seconds` default `0 -> 35`,
  `evaluate_fd_after_ttl` + criterio `fd_after_ttl`,
  `check_catalog_store_orphans` + criterio `catalog_store_orphans`,
  `ServerProcess.request` comprueba `id`, `head_tree_dirty`/`battery_status`,
  recibo con `call_discards`, `project_ttl_secs`, `binary_sha256_end`,
  `same_binary_all_samples`, `operator_attested`, `pmset_batt`; docstring del
  módulo y notas del recibo sin la afirmación "not a leak" no evidenciada.
- `scripts/test-m8-performance-unit.py`: reescrito con 76 tests; nuevas
  clases `RunCompareTests`, `ValidateToolResultPerfTests`/`SoakTests`,
  `CollectValidDispatchSamplesTests`, `NoiseControlHelperTests`/
  `SoakNoiseControlHelperTests`, `EvaluateCycleCallsTests`,
  `ShouldReopenProjectRefTests`, `EvaluateFdAfterTtlTests`,
  `CheckCatalogStoreOrphansTests`; `RegressionVerdictTests` y
  `SummarizeTests`/`BudgetComparisonTests` reescritas para el nuevo contrato.

## Salida del soak de humo (fd_after_ttl: resultado medido, no defecto)

```json
"project_ttl_secs": 30.0,
"project_ref_reopens": 4,
"call_discards": [],
"criteria": {
  "fd_growth": {"plateau_fds": 8, "final_fds": 8, "limit_fds": 18, "passed": true},
  "fd_after_ttl": {
    "applicable": true,
    "plateau_fds": 8,
    "limit_fds": 18,
    "measured_fds": 27,
    "passed": false
  },
  "catalog_store_orphans": {
    "applicable": true,
    "store_dir_entries": ["active.bundle", "floor.record", "store.lock"],
    "orphans": [],
    "passed": true
  }
},
"open_churn": {
  "count": 20, "ttl_wait_seconds": 35.0,
  "fd_count_before": 8, "fd_count_after": 27, "fd_count_after_ttl_wait": 27
},
"status": "failed"
```

20 aperturas (`--open-churn 20`, default) suben los FDs de 8 a 27; tras
esperar 35 s con `--project-ttl-secs 30` (5 s de margen), los FDs **no
bajan** (siguen en 27, contra un límite de meseta+10=18). Es decir: dejar
pasar el TTL sin ninguna llamada adicional no libera los descriptores por sí
solo — la reclamación parece depender de un evento posterior (otra apertura
que empuje el límite de referencias vivas), no de un temporizador de fondo.
Esto es exactamente lo que P-5 pedía sustituir: ya no hay una afirmación sin
evidencia, hay un número medido y un criterio real que lo falla. No se tocó
ningún archivo fuera de los permitidos para intentar "arreglar" este
resultado — el ajuste de la política de TTL/reclamación del servidor, si se
decide, es una tarea de código Rust fuera de esta paquete de scripts.

No commit.
