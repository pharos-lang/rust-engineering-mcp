# M8-05 (preparación) — Análisis de mediciones existentes para fijar presupuestos de performance

Worker W17-budgets-analysis (Claude Sonnet 5, `--effort high`), solo lectura de
código y evidencia. Rama `ai/m8-stabilization`, tip `6fa1ef1` al iniciar. Plan:
[`m8-stabilization.md`](../../roadmap/m8-stabilization.md) §M8-05 y
§«Performance, operación y distribución»; host positivo único macOS ARM64
([ADR-087](../../adr/ADR-087-1.0-host-scope.md)). Este documento **no fija**
presupuestos definitivos: los propone con margen explícito sobre lo medido, o
los marca como «sin medida previa» cuando no existe evidencia, exactamente como
exige el plan («jamás tras ver un fallo»).

Magnitudes cubiertas: startup cold/warm, dispatch sin Cargo, RSS idle/pico por
perfil, tamaño de binario/artifact, p95 de cancel-observed, tiempo de cleanup,
normalización/control de ruido, y una propuesta de soak.

---

## 1. Inventario de mediciones existentes

### 1.1 M5 (`docs/validation/M5/`)

M5 califica cuatro tools nuevas (`rust.benchmark.run/compare`,
`rust.profile.flamegraph`, `rust.binary.bloat`) que **miden el código del
proyecto del peer**, no el propio servidor MCP. Su evidencia no contiene
startup/RSS/dispatch del binario `rust-engineering-mcp`:

- [`matrix.md`](../M5/matrix.md): cierre Done local 2026-09-10, rama
  `ai/m5-performance`. Confirma cuatro presupuestos de **timeout** por tool
  (§1.2), no de latencia observada del servidor.
- [`04-bloat-calibration.json`](../M5/04-bloat-calibration.json)
  (`captured_at_utc: 2026-09-09T15:11:18Z`, imagen
  `sha256:e0a5ca16…`): calibra el comportamiento de `cargo-bloat` 0.12.1 sobre
  el fixture `fixtures/bloat` (un crate de prueba, no `rust-engineering-mcp`).
  No contiene bytes del binario del producto.
- [`01-benchmark-calibration.json`](../M5/01-benchmark-calibration.json),
  [`03-profiling-native.json`](../M5/03-profiling-native.json),
  [`02-method-simulation.json`](../M5/02-method-simulation.json): calibran el
  harness de benchmark/profiling (Criterion, `cargo-flamegraph`) sobre
  fixtures del dataset v2, no el arranque ni la RSS del servidor.
- [ADR-076](../../adr/ADR-076-m5-performance-contracts.md) §7: presupuestos de
  **timeout** de las cuatro tools — `run` 900 s, `profile` 300 s (60 s de
  muestreo máx.), `compare` 30 s, `bloat` 300 s — y techos de artifact (SVG
  ≤ 8 MiB, bloat ≤ 4 MiB, muestras ≤ 32 MiB, resultado ≤ 512 KiB). Son
  presupuestos de **tiempo máximo permitido a una operación del peer**, no
  mediciones de arranque/dispatch/RSS del servidor mismo.
- ADR-073 (protocolo estadístico), §«governor»: establece el patrón de
  provenance para controlar ruido — mismo binario, `configuration_fingerprint`,
  lectura uniforme de `scaling_governor` por CPU en el guest Linux
  (`/sys/devices/system/cpu/cpu{N}/cpufreq/scaling_governor`). **No aplica
  directamente al host macOS positivo** (macOS no expone `scaling_governor`;
  ver §2.6).
- ADR-081: criterios de aceptación estadística **congelados antes de medir**
  (cobertura ≥ 0,93, falsos positivos ≤ 0,01, potencia ≥ 0,80 con efecto
  2× el umbral) para el método de comparación de *benchmarks del usuario*. Es
  el precedente metodológico directo de "fijar antes, nunca tras ver un
  fallo" que este documento seguirle a M8-05, pero el criterio en sí no es
  reutilizable numéricamente para startup/RSS/dispatch.

**Conclusión de 1.1**: M5 no contiene ninguna medición de startup, dispatch sin
Cargo, RSS o cleanup del propio servidor. Su valor para M8-05 es exclusivamente
metodológico (protocolo estadístico, provenance, "nunca ajustar tras el
fallo") y como origen de los presupuestos de *timeout* de las tools de
performance (que M8-05 no toca).

### 1.2 M4 (`docs/validation/M4/`) — dispatch de tools *con* runtime Docker

- [`budgets.json`](../M4/budgets.json) +
  [`budgets/m4-budgets.json`](../M4/budgets/m4-budgets.json) +
  [`budgets/m4-budgets-inputs.json`](../M4/budgets/m4-budgets-inputs.json),
  generados por [`scripts/summarize-m4-budgets.py`](../../../scripts/summarize-m4-budgets.py):
  **300 observaciones reales** (30 cold + 30 warm × 5 tools: `rust.deny`,
  `rust.unsafe.scan`, `rust.miri`, `rust.quality.gate.v2`,
  `rust.supply_chain.inspect`), imagen
  `sha256:25ed3626e710081a571a86a29521eaf2e890e796afd422ba5e409e0ce1891635`,
  binario `sha256:d27cbc5907ce7e985f3f1c162356714bb88efb87e596619b8866faa6bcda8b5f`.
  Ejemplo: `rust.deny` cold p95 = 7125 ms, p99 = 7873 ms, N=30; warm p95 =
  7026 ms. `rust.quality.gate.v2` cold p95 = 16233 ms. Estas cifras son
  **dispatch de una tool con contenedor Docker** (incluye spin-up del guest),
  no "dispatch sin Cargo" del plan (que pide tools *docker-free*, p. ej.
  `rust.project.open`/`rust.catalog.status`). Sí son el precedente exacto de
  método: percentil `nearest-rank`, `cold_definition` = primera ejecución de
  cada tool en una sesión MCP recién calibrada, `calibration_ms` (14136 ms de
  arranque de sesión/imagen) **excluido explícitamente** del timer de
  operación (`calibration_excluded_from_operation_timer: true`). Este patrón
  — separar "arranque del entorno" de "duración de la operación" — es
  directamente aplicable a distinguir "startup cold del binario" de "dispatch
  de una tool" en M8-05.
- Fecha de contenido: `m4-budgets.json`/`m4-budgets-inputs.json` no llevan
  timestamp propio; el archivo del repo se tocó por última vez en el commit
  `627729a48b2912c7e3b43d6fc5678a20f0a046a0` (2026-09-11, reorganización de
  evidencia); la medición original es de M4 (2026-09-07/08 por el resto de la
  evidencia M4 circundante, p. ej. `image-config.json` con
  `recorded_at: 2026-09-07T22:49:25Z`).

**Conclusión de 1.2**: M4 no mide "dispatch sin Cargo" (todas sus 5 tools
pasan por el gateway Docker), pero es la fuente exacta del **método
estadístico** que M8-05 debe replicar para el host macOS: N=30 cold + N=30
warm por magnitud, percentil `nearest-rank`, exclusión explícita de la
calibración del timer de operación, receipt con `binary_sha256`/`image_id`
fijos.

### 1.3 Recibos de release — tamaño de binario/artifact

- [`docs/release/0.1.0/candidate/build-receipt.json`](../../release/0.1.0/candidate/build-receipt.json)
  (`source_commit: 3bb9b8b31301140799242e51c9d51fb4c80f99c4`, scope
  `local-review-only-not-distribution`, `cargo 1.98.1`, `rustc 1.98.1`):
  binario **core** (`--no-default-features`) = **21 049 384 bytes**
  (`sha256:32b7f921…`, 16.8 s de build); binario **local**
  (`--features local`) = **271 401 656 bytes** (`sha256:7a990 38b…`).
- [`docs/release/0.1.0/candidate/archive-receipt.json`](../../release/0.1.0/candidate/archive-receipt.json)
  (mismo scope local-review-only): archive **core** tar.gz =
  **8 799 844 bytes** comprimido / 28 312 871 bytes payload; archive **local**
  = **387 261 136 bytes** comprimido / 766 020 750 bytes payload.
- [`docs/validation/M1/17-public-release.json`](../M1/17-public-release.json)
  (`head_sha: 452acdbf3a634d2cc0b9d153db09718237625b9d`, tag `v0.1.0`, release
  pública real vía CI/GitHub Actions, no local): archive publicado
  `rust-engineering-mcp-v0.1.0-aarch64-apple-darwin.tar.gz` = **7 300 973
  bytes** (`sha256:b499a3e3…`), 13 tools, 219 paquetes, 11 miembros de
  archive. Es **menor** que el archive core local-review (8 799 844 B) del
  mismo commit aproximado — evidencia de que el build de CI y el build local
  no son bit-idénticos (distinta caché/flags de compilación), algo a tener en
  cuenta si M8-05 usa un build local como referencia de presupuesto para el
  artifact publicado por CI.
- [`docs/release/0.3.0/local-smoke-receipt.json`](../../release/0.3.0/local-smoke-receipt.json),
  [`local-candidate-smoke-receipt.json`](../../release/0.3.0/local-candidate-smoke-receipt.json),
  [`workflow-smoke-receipt.json`](../../release/0.3.0/workflow-smoke-receipt.json)
  (commit `1303af89bf54327d09a3b8b9289f81707e2d3088`, 2026-09-11): tres
  recibos del archive core `v0.3.0` (`aarch64-apple-darwin`, 31 tools, 221
  paquetes, 11 miembros). `archive_bytes` = 9 792 174 / 9 792 507 / 9 792 174
  respectivamente: `local-smoke` y `workflow-smoke` coinciden exactamente
  (9 792 174 B) pero `local-candidate-smoke` difiere en 333 bytes del mismo
  commit — variación de build a build, no de host a host. No hay recibo de
  **binario sin comprimir** para `0.3.0`; solo el tamaño del `.tar.gz`.
- **Sin receipt formal**: existe en este checkout un binario ya compilado en
  `target/release/rust-engineering-mcp`, **30 616 400 bytes**, mtime
  2026-09-14T16:14 local (mismo entorno donde el tip es `6fa1ef1`). No está
  acompañado de un log de build ni de un hash publicado, y no se sabe con
  certeza qué flags exactos lo produjeron (`default = []` en
  `crates/mcp-server/Cargo.toml` hace que `cargo build --release` sin flags
  adicionales produzca el perfil **core**, consistente con el orden de
  magnitud, pero esto es una observación ambiental, no una medición
  calificada). Se cita solo como orden de magnitud: el binario core creció
  ~46 % desde los 21 049 384 bytes calificados en 0.1.0 hasta hoy, dato
  esperable dado que el inventario de tools pasó de 13 (0.1.0) a 31 (0.3.0+,
  M2 y M5). **No se usa como número de presupuesto.**

**Conclusión de 1.3**: existen números reales y citables de tamaño de
binario/archive para 0.1.0 y 0.3.0 (perfil core), con la salvedad de que CI y
build local no coinciden exactamente y de que no hay receipt de binario
sin comprimir posterior a 0.1.0.

### 1.4 `scripts/release-smoke.py` / `release-artifact.py`

- `release-smoke.py::Transport` (líneas ~828-921) mide `duration_ms` por
  llamada JSON-RPC individual (incluida `server/discover`, equivalente al
  handshake `initialize`, y `tools/list`) usando `time.monotonic()` alrededor
  de cada `request()`. **Esto sí es una medición de dispatch/handshake real**,
  aunque su propósito original es smoke-test funcional, no benchmark:
  - `local-smoke-receipt.json` / `local-candidate-smoke-receipt.json`:
    `server/discover` = 18 ms, `tools/list` = 3 ms, las cuatro
    `tools/call` posteriores (`rust.catalog.status`, `rust.crate.search`,
    `rust.project.open`, `rust.diagnostics.explain`) = 0 ms (resolución del
    reloj de 1 ms de Python, sub-milisegundo real).
  - `workflow-smoke-receipt.json` (mismo commit, ejecutado en el runner de
    GitHub Actions, no en este host): `server/discover` = **122 ms**,
    `tools/list` = 31 ms, `tools/call rust.catalog.status` = 1 ms. La
    diferencia (18 ms local vs. 122 ms en CI) confirma que el host importa y
    que no se puede reutilizar sin repetir en el host positivo.
  - `cli.calls` (`--version`, `version --json`, `doctor --json`) vía
    `subprocess.Popen` + `time.monotonic()`: en `local-smoke-receipt.json` y
    `local-candidate-smoke-receipt.json`, `--version` = **404 ms** / 396 ms
    (primera ejecución tras extraer el archive descargado, candidato fuerte a
    reflejar el chequeo de cuarentena/Gatekeeper de macOS en el primer
    `exec`), mientras que `version --json` y `doctor --json` (mismo proceso,
    llamadas subsecuentes) bajan a 3-5 ms. En `workflow-smoke-receipt.json`
    (runner Linux de CI, sin Gatekeeper) `--version` = 12 ms. Esto es la
    **única evidencia existente de algo parecido a un "cold start" con costo
    de primer-exec** en el repo, pero conflacionado con extracción reciente
    del archive; no aísla "arranque del proceso" de "primer `exec` tras
    descarga". Método completo:
    [`scripts/release-smoke.py`](../../../scripts/release-smoke.py) función
    `run_cli` (constantes `PROCESS_TIMEOUT`, `clean_env()`).
  - `duration_ms` total del script (581/582/491 ms) mezcla spawn del proceso,
    todas las llamadas CLI y MCP, y cleanup — no aísla ninguna magnitud del
    plan por sí solo.
- `release-artifact.py`: empaqueta y hashea el archive (no mide tiempo de
  ejecución del binario; genera los `archive-receipt.json`/`build-receipt.json`
  citados en 1.3).
- Ninguno de los dos scripts mide RSS, FDs, ni número de procesos hijos.

**Conclusión de 1.4**: existe instrumentación de tiempo por llamada
(`duration_ms`) ya integrada en el smoke harness, reutilizable para medir
"tools/list tras handshake" y "dispatch de una tool sin Cargo" con cambios
mínimos (repetir el mismo binario N veces en vez de una), pero **no hay
ninguna medición previa con N>1 ni variabilidad reportada** — cada número
citado arriba es una sola observación de un solo proceso.

### 1.5 Tests de arranque/`doctor` y timeouts de protocolo

- `crates/mcp-server/tests/cli.rs`, `crates/mcp-server/tests/doctor.rs`: no
  contienen ninguna medición de tiempo (`grep` de `Instant`/`elapsed`/
  `duration` no arroja medición de latencia; `doctor.rs:77-82` usa
  `Instant::now() + Duration::from_secs(30)` como **deadline del test**, no
  como medición reportada).
- `crates/mcp-server/tests/protocol.rs:11`: `const TIMEOUT: Duration =
  Duration::from_secs(10)` es un timeout del arnés de test para
  `recv_timeout` (falla el test si no llega respuesta en 10 s); no es un
  presupuesto de producto ni una medición.
- `crates/mcp-server/tests/inspection_runtime.rs:25,27`: `JOIN_TIMEOUT =
  300 s`, `CONTROL_TIMEOUT = 15 s` — mismos, timeouts de arnés, no
  presupuestos.
- `docs/adr/ADR-084-rust-analyzer-runtime-and-lsp-lifecycle.md` §8-9 (M6, no
  M5): fija presupuestos de *timeout* del analyzer (`initialize`→quiescent
  ≤60 s, petición ≤30 s, total ≤180 s, memoria/PIDs/CPU del guest 1
  GiB/128/1) y declara explícitamente en §9 que "Cold init, tiempo hasta
  quiescent, latencia por petición, RSS pico (cgroup `memory.peak`)…
  **ninguno de estos valores es un presupuesto normativo todavía: son
  mediciones que pueden motivar reconsiderar el lifecycle**" — es decir, el
  propio M6 reconoce que esas cifras están sin medir. No aplica al perfil
  `core` sin Docker.

**Conclusión de 1.5**: no existe ninguna medición de tiempo de arranque,
dispatch o cleanup en la suite de tests de protocolo/CLI/doctor; todos los
`Duration` encontrados son timeouts del arnés de pruebas, no presupuestos de
producto ni observaciones reportadas.

### 1.6 Cancel-observed / cleanup

- `crates/mcp-server/tests/inspection_runtime/tasks.rs`, test
  `tasks_cancel_before_start_during_execution_publication_and_cleanup_waits_for_join`
  (línea 192, `#[ignore = "requires approved Docker Tasks path and
  test-hooks advertisement"]`): mide `cancel_to_cleanup_ms` (línea 275,
  `Instant::now()` desde el `tasks/cancel` hasta el estado terminal
  `cancelled`) y `poll_latency_ms` (línea 253) sobre un fixture `"slow"` con
  demora inyectada (feature-gated), **una sola ejecución**, no una
  distribución.
- Test hermano `tasks_eof_joins_hostile_child_and_uncertain_cleanup_fails_session`
  mide `eof_to_join_ms` (línea 338), también una sola ejecución.
- **Receipt real de esa ejecución**:
  [`docs/validation/M3/02-budgets.json`](../M3/02-budgets.json)
  (`task_lifecycle`, host `arm64`/`darwin`, imagen
  `sha256:384a1742ecc53cdd3a9c0bf36c6f8b66db73ddd118aeeae6e55654ea998ae36a`):
  `cancel_to_cleanup_ms = 1088`, `poll_latency_ms = 0`,
  `create_task_response_bytes = 262`, `job_record_resident_bytes = 1048`;
  `eof_to_join_ms = 346`. **N=1 para ambas** (no hay repetición ni p95: la
  clave `raw` contiene un único objeto, no un array). El mismo receipt sí
  contiene **N=30 cold + N=30 warm** para el *dispatch* (no cancel) de
  `rust.test.nextest`/`rust.coverage`/`rust.semver.check` — p. ej.
  `rust.test.nextest` cold `gateway_total` p50=1658 ms, p95=1719 ms, p99=1724
  ms; `reported_terminal_command` (el tiempo real dentro del contenedor, sin
  el overhead de arranque del gateway) cold p50=229 ms, p95=244 ms — de nuevo,
  con runtime Docker de por medio, no el perfil `core` sin Docker.
- Ambos tests de `cancel_to_cleanup_ms`/`eof_to_join_ms` están `#[ignore]`
  (requieren Docker Tasks aprobado + feature `test-hooks`) y usan
  **demoras sintéticas** vía fixture, no una carga de trabajo representativa:
  no acreditan un p95 de cancelación bajo condiciones reales de uso.

**Conclusión de 1.6**: existe exactamente **una** observación real de
`cancel_to_cleanup_ms` (1088 ms) y **una** de `eof_to_join_ms` (346 ms), con
runtime Docker, fixture sintético con demora inyectada, sin repetición y por
tanto sin p95 real ni variabilidad. No existe ninguna medición de
cancel-observed para el perfil `core` sin Docker (el plan pide p95 de
cancel-observed sin especificar perfil, pero el único host/perfil con
evidencia de cancelación medida en absoluto usa Docker).

### 1.7 RSS — lo que existe y lo que no

- **No existe ninguna medición de RSS del proceso `rust-engineering-mcp`**
  (idle o pico, perfil `core` o `local`) en ningún artifact del repo. La
  búsqueda de `RSS|rss_bytes|maxrss|ru_maxrss` en `docs/validation`, `docs/adr`
  y `scripts` no produce ningún resultado sobre el servidor MCP en sí.
- Lo que sí existe, y que **no debe confundirse** con RSS del servidor:
  - [`docs/validation/M2/07-native-memory.json`](../M2/07-native-memory.json)
    (`recorded_at_utc: 2026-09-05T18:53:35Z`, host macOS 26.6.2/Mac16,5,
    `physical_memory_bytes: 51539607552`): RSS de un **test binary aislado**
    (`target/release/deps/rust_engineering_project-…`) ejercitando un solo
    test de journaling (`measure_real_format_commit_replay_recovery_and_index_ceiling`)
    vía `/usr/bin/time -l`. `maximum_resident_set_size_bytes = 976666624`
    (post-optimización) / `1784365056` (pre). El propio documento declara en
    `scope.excludes`: **"application plan registry and MCP protocol/runtime
    allocations"** — es decir, explícitamente no mide el servidor MCP, solo
    el harness de journaling de una feature. El método (`/usr/bin/time -l`
    sobre un binario nativo en macOS ARM64, reportando
    `maximum_resident_set_size_bytes` y `peak_memory_footprint_bytes`) sí es
    directamente reutilizable para medir el binario `rust-engineering-mcp`.
  - [ADR-084](../../adr/ADR-084-rust-analyzer-runtime-and-lsp-lifecycle.md)
    §9 (M6): "RSS pico (cgroup `memory.peak`)" listado como SLI **a medir en
    M6-06**, explícitamente no medido todavía y explícitamente no normativo.
  - [`docs/validation/M5/01-vendor-capture-measurements.json`](../M5/01-vendor-capture-measurements.json)
    y `scripts/measure-m5-vendor-capture.py`: miden RSS/recursos de la
    **captura de vendor** (un proceso de red offline dentro del guest), no
    del servidor.
  - Presupuestos de contenedor ya impuestos por el gateway (no son RSS
    observada, son **techos**): `PidsLimit: 128`
    (`crates/execution-adapter/src/performance_gateway.rs:2949`),
    `memory_bytes` de 1 GiB (`performance_port.rs:155,1298`, constante
    `APPLIED_MEMORY_BYTES`) y 2 GiB (`criterion_dataset.rs:1464,1823`) según
    el tipo de job. Son cuotas normativas de un *job* dentro del contenedor
    Linux, no del proceso host macOS `serve --stdio`.

**Conclusión de 1.7**: RSS del servidor MCP en ninguno de los dos perfiles
(`core`/`local`), en ningún estado (idle/pico), **no existe** en el repo.

---

## 2. Estado por magnitud del plan: ¿existe medida? Si no, cómo medirla en este host

### 2.1 Startup cold/warm — `serve --stdio` hasta `tools/list`

**¿Existe medida?** Parcial y no calificada. `release-smoke.py` mide
`server/discover` (equivalente al handshake, incluye spawn+`initialize`) +
`tools/list` como dos llamadas separadas dentro de un proceso ya lanzado, pero
**no** mide el tiempo desde `Popen()` hasta la primera respuesta en el mismo
run con repetición. Los valores puntuales (18-122 ms para discover, 3-31 ms
para `tools/list`, N=1 cada uno, tres builds distintos del mismo commit
0.3.0) dan un orden de magnitud pero no una distribución. El único indicio de
costo de "cold" real (`--version` 396-404 ms en macOS local vs. 12 ms en CI
Linux) probablemente refleja el chequeo de cuarentena/Gatekeeper tras
extraer el archive descargado, no el coste del runtime de Rust — no debe
usarse como presupuesto de "startup cold" sin aislar esa variable.

**Cómo medirla reproduciblemente en este host:**
1. Fijar un binario: usar el mismo `target/release/rust-engineering-mcp`
   (perfil `core`, `--no-default-features` o build por defecto ya que
   `default = []`) para todas las repeticiones; registrar su `sha256` en el
   receipt.
2. `CARGO_INCREMENTAL=0` no aplica a la ejecución (solo al build); para el
   build usar `cargo build --release --locked --offline` una vez, luego medir
   siempre el mismo binario ya compilado, sin reconstruir entre repeticiones.
3. Comando por repetición: lanzar el proceso con `serve --stdio`, escribir el
   frame `initialize` seguido de `tools/list`, medir con `time.monotonic()`
   (mismo patrón que `release-smoke.py::Transport.request`) el tiempo desde
   `Popen()` hasta la respuesta completa de `tools/list` — esto captura
   "cold" real de principio a fin en una sola cifra, más el desglose por
   llamada (`initialize` vs. `tools/list`) que ya provee el harness existente.
4. **Cold**: primera repetición de una sesión de proceso nueva (nuevo
   `Popen`) — descartar cualquier calentamiento de página/caché del binario
   compartiendo el criterio de M4 (`cold_definition`: primera ejecución en
   sesión nueva). **Warm**: repetir `tools/list` en el mismo proceso ya
   iniciado (sin volver a lanzar el binario), o lanzar procesos consecutivos
   inmediatos sin purgar cachés del SO.
5. N ≥ 30 por temperatura (precedente directo: `scripts/summarize-m4-budgets.py`
   usa N=30 cold/30 warm). Descartar del cómputo cualquier proceso que no
   termine limpio (exit code / EOF ordenado, igual que valida
   `release-smoke.py::Transport.finish`).
6. Control de ruido: sin otros procesos pesados corriendo (verificar con
   `ps`/Activity Monitor antes de la corrida, no automatizable sin
   dependencia nueva); mismo binario/hash en todas las repeticiones; ejecutar
   en serie, no en paralelo (evita contención de CPU entre repeticiones);
   registrar `sw_vers`/`uname -a`/modelo de hardware una vez por corrida
   (igual que `M2/07-native-memory.json::host`).
7. Registrar por separado el "primer exec tras extracción de un archive
   descargado" (posible costo de Gatekeeper) del "startup" en sentido
   estricto: son dos magnitudes distintas y el repo solo tiene evidencia
   confusa de la primera.

### 2.2 Dispatch sin Cargo — `rust.project.open` / `rust.catalog.status` sin catálogo

**¿Existe medida?** Sí, parcial: `release-smoke.py`'s receipts (§1.4) ya
ejercitan exactamente estas dos tools (`rust.catalog.status` sin catálogo →
`CATALOG_UNAVAILABLE`; `rust.project.open` → `SANDBOX_DENIED` en el smoke
porque el sandbox de smoke no concede la raíz) con `duration_ms = 0` en local
(sub-milisegundo, resolución de reloj insuficiente) y `duration_ms = 1` en CI
Linux para `rust.catalog.status`. **No hay variabilidad ni N>1**, y el caso
de `rust.project.open` en el smoke no representa el camino feliz (abre con
sandbox denegado, no una apertura real).

**Cómo medirla reproduciblemente en este host:**
1. Mismo binario/proceso fijo; usar un fixture de proyecto real con
   `--allow-project-write`/root permitido para que `rust.project.open`
   ejecute el camino feliz, no el rechazo de sandbox.
2. Medir con el mismo patrón `Transport.request` ya existente en
   `release-smoke.py`, pero repitiendo cada llamada N≥30 veces **dentro del
   mismo proceso ya arrancado** (para aislar el costo de dispatch del costo
   de arranque medido en 2.1) y N≥30 veces en **procesos nuevos** (para medir
   el efecto de un proceso recién iniciado, ya con `tools/list` completado,
   sobre la primera llamada real).
3. Reportar p50/p95/p99/max con `nearest-rank` (mismo método que
   `scripts/summarize-m4-budgets.py`).
4. Como ambas tools son 100 % locales (sin Docker, sin red, sin subproceso
   externo), no aplica ningún control de "cold container"; el único ruido
   relevante es el propio proceso del servidor y el SO.
5. Fijar la resolución del reloj: Python (`time.monotonic()`) tiene
   resolución de microsegundos reales en macOS aunque el ejemplo de smoke
   redondeaba a `int(...*1000)` ms — usar microsegundos si la magnitud
   esperada es sub-milisegundo (los `duration_ms=0` observados sugieren que
   sí lo es), o medir en Rust directamente con `Instant` para evitar el
   overhead del propio intérprete Python en la medición.

### 2.3 RSS idle y pico por perfil `core` vs. `local`

**¿Existe medida?** No, según §1.7: cero mediciones de RSS del proceso
`rust-engineering-mcp` en ningún perfil. Lo más cercano es el precedente de
método (`/usr/bin/time -l`) usado sobre un binario de test no relacionado
(M2/07).

**Cómo medirla reproduciblemente en este host:**
1. **Idle**: lanzar `serve --stdio` (perfil `core`: build por defecto;
   perfil `local`: `--features local`), completar `initialize`+`tools/list`,
   y luego medir RSS del proceso ya quiescente sin más tráfico, usando
   `ps -o rss= -p <pid>` muestreado cada N segundos durante una ventana fija
   (p. ej. 60 s), o lanzando el proceso bajo `/usr/bin/time -l` y enviando
   EOF inmediatamente después de `tools/list` para capturar
   `maximum_resident_set_size_bytes` de todo el ciclo de vida corto (esto mide
   "idle tras arranque", no un idle sostenido; para idle sostenido hace falta
   `ps` en bucle porque `/usr/bin/time -l` solo reporta al final del proceso).
2. **Pico**: ejercitar la secuencia de tools más pesada disponible sin Docker
   para el perfil `core` (`rust.project.open` + `rust.catalog.status` +
   `rust.crate.search` sobre un fixture con catálogo real) y, para `local`,
   añadir una tool con runtime Docker (p. ej. `rust.check`) — medir con
   `ps -o rss=` muestreado durante la ejecución o con herramientas de
   `sample`/`vmmap` de macOS si se requiere un desglose de páginas.
3. N ≥ 10 repeticiones por (perfil × estado) dado el costo más alto por
   repetición que el dispatch puro; reportar min/mediana/max en vez de p95
   (con N=10 un p95 por `nearest-rank` colapsa al máximo, poco informativo —
   mismo criterio que aplicó ADR-081 al rechazar exigir percentiles finos con
   pocas muestras).
4. Control de ruido: cerrar aplicaciones en primer plano no relacionadas,
   ejecutar en serie, y — crítico para RSS en macOS con memoria comprimida —
   registrar `physical_memory_bytes` del host (como ya hace
   `M2/07-native-memory.json::host`) porque la presión de memoria del sistema
   afecta la compresión de páginas y por tanto el RSS reportado.
5. Diferenciar explícitamente `maximum_resident_set_size_bytes` (pico
   histórico del proceso, lo que reporta `/usr/bin/time -l`) de una lectura
   puntual de RSS "idle" vía `ps`, que es una instantánea, no un máximo — son
   dos consultas distintas y el plan pide ambas ("idle y pico").

### 2.4 Tamaño de `target/release/rust-engineering-mcp` y del archive core

**¿Existe medida?** Sí, calificada para 0.1.0 (§1.3): binario core 21 049 384
bytes, archive core publicado 7 300 973 bytes (CI) / 8 799 844 bytes
comprimido (build local). Para 0.3.0+ solo existe el tamaño del archive
(9 792 174-9 792 507 bytes, variación de hasta 333 bytes entre builds del
mismo commit), sin receipt de binario sin comprimir. El binario ambiental de
30 616 400 bytes en este checkout (§1.3) da un orden de magnitud para el tip
actual pero no es una medición calificada (sin log de build ni hash
publicado).

**Cómo medirla reproduciblemente en este host, para producir el número
calificado que falta:**
1. `cargo build --release --locked --offline -p rust-engineering-mcp --bin
   rust-engineering-mcp` (perfil core, sin flags de feature) sobre un
   `git status` limpio, exactamente el patrón de
   `docs/release/0.1.0/candidate/build-receipt.json`.
2. Registrar: commit, `Cargo.lock` sha256, comando exacto, `cargo`/`rustc
   --version`, bytes del binario resultante (`stat -f%z` o
   `ls -la`), sha256 del binario.
3. Repetir el build 2-3 veces en limpio (`cargo clean` entre corridas) para
   confirmar reproducibilidad de bytes exactos — el hallazgo de 1.3 (CI vs.
   local difieren) sugiere que **no** se debe asumir determinismo bit a bit
   entre entornos sin verificarlo explícitamente primero.
4. Empaquetar con el mismo `scripts/release-artifact.py` usado para 0.3.0 y
   registrar `archive_bytes`/`payload_bytes` con el mismo esquema que
   `archive-receipt.json`.
5. N=1 es aceptable aquí (el tamaño de un binario de un commit fijo con
   toolchain fija no tiene "variabilidad" en el sentido estadístico, salvo la
   pregunta de determinismo del punto 3, que si falla sí exige repetición).

### 2.5 p95 de cancel-observed

**¿Existe medida?** Una sola observación real (1088 ms, §1.6), con runtime
Docker, demora sintética inyectada por fixture, test `#[ignore]`. No hay p95
posible con N=1.

**Cómo medirla reproduciblemente en este host:**
1. Para el perfil `core` sin Docker: no existe hoy ninguna tool de larga
   duración cancelable sin runtime (`rust.project.open`/`catalog.status`
   completan en sub-milisegundo, §2.2 — no hay ventana de cancelación
   observable). El plan de soak (§4) usa `rust.check` bajo Docker
   específicamente porque el perfil `core` no tiene una operación cancelable
   de duración no trivial.
2. Para el perfil `local`/Docker: habilitar el feature `test-hooks` y correr
   `tasks_cancel_before_start_during_execution_publication_and_cleanup_waits_for_join`
   repetidamente (hoy `#[ignore]`, una sola ejecución por invocación de
   `cargo test`) — modificar el test o envolverlo en un driver Python al
   estilo `docs/validation/M3/02-budgets.json` (que ya orquesta comandos
   `cargo test --exact --ignored --nocapture` y parsea la línea impresa
   `M3_TASK_CANCEL_RECEIPT`) para repetirlo N≥30 veces y calcular p95/p99
   sobre `cancel_to_cleanup_ms`.
3. Además de repetir el mismo fixture sintético `"slow"`, medir también sobre
   una carga real no sintética (p. ej. `rust.test.nextest`/`rust.check` sobre
   un fixture de tamaño representativo) para que el p95 no dependa
   exclusivamente de una demora artificial fija.
4. Reportar `cancel_to_cleanup_ms` con `nearest-rank` p95/p99, igual criterio
   que M3/M4.

### 2.6 Tiempo de cleanup tras cancel/EOF

**¿Existe medida?** Una sola observación de `eof_to_join_ms` = 346 ms (§1.6),
mismas limitaciones que 2.5 (N=1, Docker, `#[ignore]`).

**Cómo medirla reproduciblemente en este host:** mismo procedimiento que 2.5,
usando `tasks_eof_joins_hostile_child_and_uncertain_cleanup_fails_session`
como base y repitiéndolo N≥30 veces con el mismo driver.

### Normalización / control de ruido — resumen transversal

El repo ya tiene dos patrones de control de ruido establecidos que M8-05 debe
heredar en vez de reinventar:
- **M4** (`m4-budgets.json`): mismo `binary_sha256`/`image_id` fijos para
  todas las 300 observaciones; `calibration_ms` excluido explícitamente del
  timer de la operación.
- **ADR-073/M5**: lectura uniforme del CPU governor **en el guest Linux**
  para provenance — **no aplicable tal cual en el host macOS positivo**, que
  no expone `scaling_governor`; el control de ruido equivalente en macOS es
  operativo, no leíble por software estándar: cerrar aplicaciones en primer
  plano, no ejecutar con batería en modo de bajo consumo, ejecutar en serie.
  Esto debe declararse como limitación conocida del host macOS frente al
  guest Linux, no resolverse inventando un sustituto no verificado.
- Recomendación explícita para M8-05: descartar la primera repetición de
  cada *build* nuevo (no de cada proceso: eso ya lo captura la definición
  cold/warm) como "compilación fría de caché de página del binario en disco",
  y fijar `CARGO_INCREMENTAL=0` únicamente en el build que produce el binario
  medido (no afecta la ejecución, solo garantiza que el binario no mezcla
  unidades de compilación incrementales de corridas previas).

---

## 3. Propuesta de presupuestos (no SLO)

Todos son propuestas sujetas a la regla del plan: **presupuestos, no SLO**, y
"no cambiar tras ver un fallo". Regla de regresión propuesta, uniforme para
todas las magnitudes de esta sección salvo que se indique otra cosa: **falla
si 2 de 3 repeticiones consecutivas de un run de gate superan el presupuesto**
(evita que un único outlier de ruido del host bloquee un RC, sin permitir que
una regresión sistemática pase desapercibida).

| Magnitud | Base medida | Margen propuesto | Presupuesto propuesto | Estado |
| --- | --- | --- | --- | --- |
| Startup cold (`serve --stdio`→`tools/list`) | Sin medida propia con N>1 en este host; orden de magnitud: 18-122 ms para el handshake solo, en tres entornos distintos, N=1 cada uno | — | **Sin medida previa.** Provisional: 500 ms | Sin medida previa — recalibrar tras la primera medición con N≥30, antes de cualquier RC |
| Startup warm (repetir `tools/list` en el mismo proceso) | 3-31 ms, N=1, tres entornos | p95 medido × 1,5 una vez exista N≥30 | **Sin medida previa.** Provisional: 100 ms | Sin medida previa |
| Dispatch sin Cargo (`rust.project.open`/`rust.catalog.status`) | 0-1 ms, N=1, camino de rechazo no el feliz | — | **Sin medida previa.** Provisional: 50 ms | Sin medida previa — el número real casi seguro será mucho menor; el margen es deliberadamente holgado hasta tener N≥30 del camino feliz |
| RSS idle, perfil `core` | Ninguna | — | **Sin medida previa.** No se propone un número: cualquier cifra sería inventada; §2.3 da el método | Sin medida previa |
| RSS idle, perfil `local` | Ninguna | — | **Sin medida previa** | Sin medida previa |
| RSS pico, perfil `core`/`local` | Ninguna (el más cercano, M2/07, mide un test no relacionado a 976 666 624 B) | — | **Sin medida previa** | Sin medida previa |
| Tamaño binario core (sin comprimir) | 21 049 384 B calificado en 0.1.0 (commit `3bb9b8b3`); ~30 616 400 B observado sin receipt en el tip actual | valor medido + 15 % absoluto (crecimiento esperado del inventario de tools) | **43 M B** sobre el próximo receipt calificado (a recalcular tras 2.4) | Con medida previa parcial (0.1.0), pendiente de nueva medida calificada en el tip actual |
| Tamaño archive core publicado | 7 300 973 B (0.1.0, CI) / 9 792 174 B (0.3.0, local) | valor medido más reciente × 1,3 | **12,7 M B** (sobre 9 792 174 B de 0.3.0) | Con medida previa (0.3.0), a recalibrar sobre el archive real de 0.8.0 antes del primer RC |
| p95 cancel-observed | N=1: 1088 ms, Docker, fixture sintético | — | **Sin medida previa** con rigor estadístico; el único punto de datos (1088 ms) sugiere que un presupuesto provisional de **3000 ms** (≈2,75× el único valor observado) es razonable como placeholder hasta N≥30 | Sin medida previa — recalibrar obligatoriamente tras la primera corrida con N≥30 |
| Tiempo de cleanup (EOF→join) | N=1: 346 ms, Docker | — | **Sin medida previa**; placeholder **1000 ms** (≈2,9×) | Sin medida previa |

Justificación del margen ×1,5 en p95 (donde se usa): es el múltiplo ya
implícito en la práctica de la industria para presupuestos de latencia sobre
un p95 medido con variabilidad moderada, y es consistente con dejar margen
para hosts ligeramente más cargados que el de medición sin abrir la puerta a
una regresión de 2× pasar desapercibida. Donde no hay medida, este documento
**no inventa** un p95 ficticio: marca la fila como "sin medida previa" y
propone solo un placeholder explícitamente etiquetado como tal, tal como pide
el plan ("marca claramente cuáles son «propuestos sin medida previa»").

**Regla de recalibración**: cada fila "sin medida previa" se recalibra
**exactamente una vez**, inmediatamente después de la primera medición con el
método de §2, y esa recalibración ocurre **antes de abrir el primer RC**. Tras
esa recalibración, cualquier ajuste posterior de presupuesto exige un cambio
material documentado (nuevo hardware, cambio de arquitectura del servidor,
etc.) y reinicia el contador de RC, igual que exige el plan para "cambios
materiales" en general.

---

## 4. Propuesta de soak

### 4.1 Perfil `core`, sin Docker

- **Ciclo**: `rust.project.open` (fixture fijo) → `rust.catalog.status` (con
  catálogo del fixture) → `rust.crate.search` (consulta fija sobre el
  catálogo del fixture) → cancelar la siguiente llamada en vuelo si el
  fixture incluye una operación de duración no trivial (hoy no existe una en
  el perfil `core` sin Docker según §2.5 — si no se añade ninguna, el ciclo
  de cancelación se omite honestamente para este perfil y se documenta como
  hueco, en vez de fingir una cancelación sobre una llamada que ya terminó).
- **Duración**: 8 h, como pide el plan.
- **Ciclos**: 1000, como pide el plan — a razón de 8 h / 1000 ciclos ≈ 28,8 s
  por ciclo, holgado para las latencias sub-100 ms esperadas de §2.1/2.2, con
  margen para overhead de instrumentación y una pausa deliberada entre ciclos
  (para no convertir el soak en una prueba de saturación de CPU, que no es su
  objetivo).
- **Cuotas pequeñas**: reutilizar fixtures ya existentes con catálogos
  acotados (evitar el catálogo completo/pesado); no crear proyectos nuevos en
  cada ciclo (reutilizar el mismo `project_ref`/directorio para que el
  crecimiento observado sea atribuible al servidor, no a N fixtures
  distintos acumulando archivos).
- **Métricas de crecimiento** (muestreadas cada N ciclos, p. ej. cada 20):
  - RSS del proceso servidor (`ps -o rss=`).
  - FDs abiertos (`lsof -p <pid> | wc -l` en macOS).
  - Procesos hijos vivos (`pgrep -P <pid>` — el perfil `core` no debería tener
    ninguno de forma sostenida, ya que no lanza runtime).
  - Archivos en el state-root (conteo y bytes totales, `find <state-root>
    -type f | wc -l` y `du -sh`).
- **Criterios de fallo, fijados antes de correr**:
  - RSS al final del soak > RSS tras el primer 5 % de los ciclos × 1,2 (un
    crecimiento sostenido >20 % sobre la meseta temprana indica leak; el
    factor 1,2 es holgado a propósito para no confundir fragmentación normal
    del allocator con leak real).
  - FDs abiertos al final > FDs tras el primer 5 % de los ciclos + 10 (margen
    absoluto pequeño porque un servidor `core` sin Docker no debería abrir
    FDs de forma no acotada por ciclo de tool read-only).
  - Cualquier archivo huérfano en el state-root que no corresponda a los
    artifacts esperados del ciclo.
  - Cualquier ciclo con `duration_ms` > presupuesto de dispatch × 3 (detecta
    degradación catastrófica puntual, no solo tendencia).

### 4.2 Perfil `local`, con Docker (`rust.check` cancel/EOF)

- **Ciclo**: lanzar `rust.check` con `execution_mode: task`, cancelar a mitad
  de ejecución en una fracción de los ciclos y dejar completar en el resto;
  en una fracción adicional, cerrar la conexión stdio a mitad de una llamada
  (EOF) para ejercitar el camino de `tasks_eof_joins_hostile_child…`.
- **Cuotas pequeñas**: mismo criterio de contenedor que ya impone el gateway
  (`PidsLimit: 128`, memoria 1-2 GiB por job, `crates/execution-adapter/src/performance_gateway.rs:2949`,
  `performance_port.rs:155`) — no se proponen cuotas nuevas, se reutilizan
  las ya calificadas.
- **Duración/ciclos**: proporcionalmente menor que 4.1 dado el costo por
  ciclo mucho mayor (spin-up de contenedor); proponer 8 h con el número de
  ciclos que quepan dentro de esa ventana dado el `gateway_total` ya medido
  en M3 (~1,6-2,8 s por operación con contenedor, §1.6) — del orden de
  8*3600/2.5 ≈ 11 500 ciclos posibles en el tiempo, pero limitar
  explícitamente a **1000 ciclos** como pide el plan (mismo número que
  `core`, para comparar crecimiento entre perfiles bajo el mismo N) dejando
  el resto de la ventana de 8 h como margen, no como más ciclos.
- **Métricas de crecimiento**: además de las de 4.1, contar contenedores
  Docker huérfanos (`docker ps -a --filter` por label del proyecto) y
  volúmenes/redes no limpiados, dado que ADR-075/077 ya identifican esa
  superficie como parte del containment de M5.
- **Criterios de fallo**: los mismos de 4.1 más "cualquier contenedor o
  volumen Docker que sobreviva al ciclo que lo creó" (fail-closed, cero
  tolerancia, no es una magnitud con margen).

### 4.3 Coste de máquina estimado

- `core` sin Docker: 8 h de un proceso ligero (sin runtime, sin contenedor);
  costo de máquina despreciable frente al resto del gate — comparable a
  dejar un proceso Rust idle/con llamadas locales corriendo, sin uso de CPU
  significativo entre ciclos dado el margen de ~28,8 s por ciclo de 4.1.
  Puede correr en paralelo a otro trabajo del mismo host sin runtime Docker
  activo.
- `local` con Docker: 1000 ciclos de `rust.check` con contenedor, cada uno
  con el costo de spin-up ya medido en M3/M4 (gateway_total del orden de
  segundos) — el costo real está dominado por el **tiempo de pared** (8 h
  reservadas), no por CPU sostenida al 100 %, porque cada ciclo tiene tiempo
  de espera de I/O de contenedor. Requiere el host macOS con Docker Desktop
  activo durante toda la ventana; no puede compartirse con otro trabajo que
  también use el gateway Docker del mismo host (el gateway ya serializa
  `busy`, ADR-084 §8), así que bloquea ese recurso 8 h.
- Ninguna cifra de coste monetario existe en el repo para extrapolar (no hay
  receipts de costo de CI/runner); esta estimación es solo de tiempo de pared
  y exclusividad de recursos, no de costo en dólares.

---

## Verificación

`python3 -B scripts/docs-hygiene.py links-check` pendiente de ejecutar por el
worker antes de handoff (ver informe final). Ningún archivo fuera de
`docs/validation/M8/05-budgets-analysis.md` fue escrito por este worker.
