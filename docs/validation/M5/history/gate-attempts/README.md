# Intentos de gate M5, incluidos los fallidos

Un gate que falla es evidencia, no basura. Estos recibos se conservan enteros.

## Intento 1 — 2026-09-09, `full`, **failed** en el paso 30 de 34

Recibos: [gate](../inventory.json) · [m4-runtime](../inventory.json).

Corrido sobre un worktree limpio en `b19c3cd`, porque el árbol principal tenía
trabajo de dependencias del owner sin commitear (subidas de `lancedb`,
`fastembed`, `jsonschema` y `tokio-rustls`) y un recibo no debe acreditar bytes
que quien lo firma no controla.

Veintinueve pasos pasaron, entre ellos `docker-security`, `rust-security`,
`m2-runtime`, `m3-runtime` y los cuatro pasos M5 de helper y vendor. Falló
`m4-runtime` en la última de sus catorce selecciones,
`security_runtime::deny_native_mcp_tasks_policy_licenses_and_owner_bound_redacted_resource`,
con `Error: Timeout` a los 28,1 s y exit 101.

### Condición de diagnóstico, fijada antes de correrlo

Se escribe aquí **antes** de reproducir, para que la clasificación no se elija
después de ver el resultado:

- El recibo calificado de M4 registra esa misma selección en **42,5 s y
  `passed`** ([M4-runtime.json](../../../M4/runtime.json)). El fallo llegó a los 28,1 s,
  es decir **antes** de lo que tarda normalmente: abortó contra un deadline
  interno del propio test, no contra el bound de 900 s del gate.
- El `load` de la máquina era **15,97 en 16 núcleos**, con dos agentes
  compilando en paralelo al gate. Eso incumple la regla operativa que este mismo
  proyecto ya tenía registrada tras dos flakes de la misma clase hoy —el de
  `closed_stdout_exits_even_when_stdin_remains_open` y el de
  `test-codex-model-qualifier.py`—, y el incumplimiento fue del owner, no del
  producto.

**Criterio.** Se corre esa selección **sola**, con `load` por debajo de 6:

- si **pasa**, se clasifica como flake de planificación, se registra la
  disposición y **no** se toca el test ni se amplía su timeout: ampliarlo
  convertiría un arnés que detecta cuelgues reales en uno que no;
- si **falla igual**, no es carga. Es un defecto y se persigue como tal, sin
  relanzar buscando un verde.

Nada de este intento acredita bytes finales en ningún caso: ADR-079 y ADR-080
cambian contratos que este gate midió, así que la pasada que cuenta es posterior.

### Disposición: flake de planificación, causado por el owner

Reproducido a solas el 2026-09-09 con `load` 3,25:

```
test security_runtime::deny_native_mcp_tasks_policy_licenses_and_owner_bound_redacted_resource ... ok
test result: ok. 1 passed; 0 failed. finished in 42.55s
```

**42,55 s contra los 42,505 s del recibo calificado de M4**: no solo pasa, tarda
lo mismo. El fallo del intento 1 abortó a los 28,1 s, o sea que el deadline
interno saltó *antes* de que el trabajo terminara, que es la firma de una prueba
a la que el planificador no le dio CPU a tiempo. Se cumple la primera rama del
criterio.

En consecuencia, y conforme a lo fijado antes de reproducir:

- **No se toca el test ni se amplía su timeout.** Ampliarlo convertiría un arné
  que detecta cuelgues reales en uno que no puede.
- **No se relanza el gate buscando un verde.** El intento 1 queda como está,
  fallido y conservado, y la pasada que acredite bytes finales será otra.
- La causa es del owner, no del producto: se lanzaron dos agentes compilando en
  paralelo al gate, incumpliendo la regla operativa que este proyecto ya tenía
  registrada hoy tras dos flakes de la misma clase. La regla se reafirma: **el
  gate corre solo**.

Es el tercer flake de esta clase en un día, los tres con la misma firma —un bound
de subproceso o de deadline interno que solo falla bajo carga alta— y los tres
verdes al relanzarlos a solas. La lección operativa vale más que los tres
diagnósticos por separado.


## Intento 2 — 2026-09-10, `full`, **failed** en el paso 32 de 34

Recibos: [gate](../inventory.json) ·
[log](../inventory.json) ·
[paso `semantic`](../inventory.json). Corrido a solas
sobre el worktree limpio en `c334498`, inmediatamente después del
[`core` aprobado](../../core-gate.json) sobre los mismos bytes.

Treinta y un pasos pasaron en 2 h 5 min, entre ellos `docker-security`,
`rust-security`, `m2-runtime`, `m3-runtime`, `m4-tampered-plugin`,
`m4-inventory`, `m4-runtime` y `audit-data`: 1721 tests Rust y 108 Python.
Falló `semantic` a los 103,5 s con exit 1, y `catalog`, `catalog-status`,
`crate-search`, `crate-inspect`, `doctor` y **`m5-runtime`** no llegaron a
ejecutarse: la etapa M5 del gate conjunto no tiene recibo.

### Causa, medida

`scripts/test-semantic.py` compila el workspace con `--all-features`, que es lo
único que activa la feature `local` del adapter semántico y con ella `lancedb`.
`cargo check --workspace --all-features --all-targets --locked --offline`
termina con `error[E0599]: no variant named Http found for enum error::Error`
en `lancedb-0.38.0/src/job.rs:56` y `:66`: `Error::Http` existe solo bajo
`#[cfg(feature = "remote")]` (`src/error.rs:111`) y `job.rs::decode` lo usa sin
`cfg`. **`lancedb 0.38.0` no compila con `default-features = false`**, que es
como `Cargo.toml` la fija desde la subida de dependencias del owner en
`a3cb48c`. `core` no lo detecta porque no usa `--all-features`; el `full` de M4
midió `lancedb 0.31.0`.

Comprobaciones adicionales: la caché del registry solo contiene `lancedb
0.31.0` y `0.38.0`; activar `remote` exige cambiar `Cargo.lock` y descargar
`reqwest`, `http`, `urlencoding` y `lance-namespace-impls[rest]` (`cargo
metadata --locked --offline` responde «cannot update the lock file»); el
`[patch.crates-io]` de `vendor/lancedb` es la 0.31.0 con dos ediciones de
manifest y no participa en el grafo, y `scripts/verify-vendor.py` sigue anclado
a esa versión, por lo que no puede corregir una fuente `.rs`.

### Disposición: bloqueo que exige decisión del owner

Ninguna salida cabe en la autorización de esta sesión: revertir a 0.31.0 está
prohibido; activar `remote` cambia el lock, descarga dependencias y amplía la
superficie de supply chain; vendorizar 0.38.0 con un parche de fuente rompe la
política manifest-only calificada; esperar una 0.38.x corregida requiere red.
El gate `full` conjunto —y con él la etapa `m5-runtime` dentro del conjunto—
queda **bloqueado** hasta esa decisión. La feature `local` del producto
(búsqueda semántica con LanceDB) tampoco compila en este HEAD; el binario por
defecto no la incluye.

### Sondas sobre las opciones para `lancedb 0.38.0` (2026-09-10, worktrees temporales, nada commiteado en Cargo)

Recibos en [`closure-full-attempt-2/lancedb-0.38-probes/`](../inventory.json).

- **`cargo audit`** sobre el lock anterior a `a3cb48c` (0.31.0) y el actual
  (0.38.0): idéntico —solo `paste` 1.0.15 no mantenido, permitido—. La subida no
  corrigió ningún advisory.
- **Opción 1, `remote`**: resuelve offline desde el índice en caché y añade 11
  crates ([delta del lock](../inventory.json)),
  entre ellos `axum` 0.7.9, `axum-core`, `matchit`, `tower-http` 0.5.2 (vía
  `lance-namespace-impls/rest-adapter`), `urlencoding`, `system-configuration`,
  `windows-registry`; 8 de los 11 `.crate` no están en caché. `deny.toml` prohíbe
  la feature (`[[bans.features]] crate = "lancedb" deny = ["remote", …]`).
- **Opción 3, parche `cfg` de dos brazos en `job.rs::decode`**
  ([patch](../inventory.json)):
  el adapter semántico **compila** contra la API 0.38.0
  ([check](../inventory.json)),
  pero **todas las pruebas que abren una tabla LanceDB fallan** bajo las
  condiciones del gate semántico: `lance-io 11.0.0/src/spill.rs:233` «failed to
  create temp directory for LocalSpillStore» con `TMPDIR` inexistente, incluida
  la integración real `real_offline_e5_lance_sqlite_roundtrip`
  ([salida](../inventory.json)).
  Es exactamente el segundo motivo por el que [ADR-027](../../../../adr/ADR-027-semantic-offline-foundation.md)
  descartó 0.38.0/Lance 11: crea un spill store en disco aunque la base sea
  `memory://`. Las opciones 1 y 3 comparten Lance 11 y por tanto este fallo.

## Intento 3 — 2026-09-10, `full` sobre el lock 0.31.0, **failed** en el paso 27 de 34

Recibos: [gate](../inventory.json) ·
[log](../inventory.json) ·
[paso `m3-runtime`](../inventory.json) ·
[recibo M3](../inventory.json) ·
[muestra de pila](../inventory.json). Corrido a solas
sobre el worktree limpio en `ab4eed9`, tras el `core` aprobado sobre los mismos
bytes. Veintiséis pasos pasaron —`semantic` incluido, ya con Lance 8— y la
selección 19 de M3, `tasks_runtime::tasks_revocation_during_active_child_masks_cancels_and_prevents_publication`,
fue matada por el límite de 900 s del paso.

### Diagnóstico, medido

- El límite se consumió así: 448 s de recompilación del target
  `inspection_runtime` con `--features test-hooks` (primer target con esa
  feature tras el cambio de lock) y después el binario recién enlazado no
  emitió ni una línea. Históricamente la selección tarda 23–27 s.
- Repetida a solas con su comando exacto, volvió a colgarse: el proceso de
  test estuvo diez minutos al 0 % de CPU, **sin hijo `rust-engineering-mcp`**,
  sin eventos Docker y con el hilo principal detenido en `_dyld_start`.
  `codesign -dvv` sobre ese archivo también se bloqueaba y `exec --list` del
  mismo binario no arrancaba en 45 s, mientras el binario hermano sin
  `test-hooks` arrancaba en 0,02 s y **una copia byte a byte del binario
  colgado ejecutaba en 0,72 s**.
- No es el producto ni el test: es el estado de firma/vnode del artefacto de
  build recién enlazado en este host (cargo no lo reenlaza porque lo considera
  fresco). Una primera reproducción sin `--features test-hooks` fue inválida y
  se descarta: sin la feature el servidor no expone Tasks y el test agota su
  `JOIN_TIMEOUT` de 300 s por diseño.

### Disposición

Se eliminó únicamente el artefacto envenenado bajo `target/debug/deps/`
(salida de build, fuera del inventario de fuentes del gate); cargo lo reenlazó
en 1 m 41 s y la selección pasó en 16,3 s
(`M3_TASK_REVOCATION_RECEIPT {"active_child":true,"joined_cleanup":true,"masked_ms":370,"publication_visible":false}`).
No se tocó el test ni su timeout. `full` se repite entero.

## Resultado final — 2026-09-11, `full` sobre los bytes del PR #17, **passed**

[M5-full-gate.json](../../full-gate.json) · [log](../inventory.json).
Corrido a solas sobre el worktree limpio en `45d339f` (fuentes de `34bd428`:
workspace `0.3.0`, verificador de smoke de release para 31 tools y saneado de
argumentos del utillaje M5): 38/38 pasos en 1 h 48 min, 1752 tests Rust y 108
Python, fuentes sin cambios; `semantic` en 35 s y `m5-runtime` conjunto en
364 s. El [`core`](../../core-gate.json) previo sobre los mismos bytes pasó
23/23. La pasada anterior sobre `ab4eed9` (abajo) acreditó el mismo código en
`0.3.0-dev`.

## Resultado — 2026-09-10, `full` sobre el lock 0.31.0 en `0.3.0-dev`, **passed**

[M5-full-gate.json](../../full-gate.json) · [log](../inventory.json) ·
[etapa `m5-runtime` conjunta](../inventory.json). Corrido a
solas sobre el worktree limpio en `ab4eed9` tras reenlazar el artefacto del
intento 3: 38/38 pasos en 2 h 16 min, 1752 tests Rust y 108 Python, inventario
de 1148 fuentes idéntico al inicio y al final. `semantic` pasó con Lance 8 en
219 s; `m3-runtime` completó sus selecciones en 33 min; la etapa `m5-runtime`
produjo su recibo dentro del conjunto (6/6, 368 s, residuo vacío), que se
conserva junto al [gate nativo independiente](../../native-gate.json) sin
sustituirlo. Los recibos `core` y `full` de la primera pasada (lock 0.38.0)
permanecen en [`closure-core-lock-0.38.0`](../inventory.json) y
[`closure-full-attempt-2`](../inventory.json).

## Reordenación del repositorio — 2026-09-11, `core` sobre `a3ce362`, dos intentos fallidos y uno aprobado

Recibos: [intento 1](../inventory.json) · [intento 2](../inventory.json) ·
[**aprobado**](../../core-gate-repo-hygiene.json). Corridos a solas sobre un
worktree limpio de la rama `ai/repo-hygiene` (fuentes de `a3ce362`: crates
idénticos a `v0.3.0`, `scripts/docs-hygiene.py` nuevo) con la caché de
`target/` del árbol principal compartida vía `CARGO_TARGET_DIR`.

- **Intento 1** falló en el paso 11, `m4-client-harness-tests`: el worktree no
  tenía `target/release/rust-engineering-mcp` porque la caché compartida deja
  el binario fuera de él y el gate no lo construye. Omisión del entorno del
  arnés, no señal del producto. Se construyó el binario release desde las
  fuentes del worktree (`0.3.0`, SHA-256 `0a6081cf…`) y se dejó en su sitio.
- **Intento 2** falló en el paso 10, `codex-qualifier-tests`:
  `test_fake_end_to_end_source_immutable_and_cleanup` registró
  `monitor:live descendant executable unresolved:…:ProcessLookupError`, una
  carrera del monitor de descendientes con un hijo que ya había salido. Es la
  misma clase de flake que este registro documenta más arriba para la misma
  suite. Criterio fijado antes de reproducir: a solas y con `load` bajo, si
  pasa es flake y no se toca el test. Reproducida dos veces a solas en el
  mismo worktree: 39/39 OK, `load` 2,6. Sin cambios en el test ni en su
  temporización; el gate se repite entero.
- **Intento 3, aprobado**: 23/23 pasos en 27 min, 1 717 tests Rust y 108
  Python, inventario de 1 149 fuentes idéntico al inicio y al final. Acredita
  que la reordenación no cambió ningún byte de `crates/`, `fixtures/`,
  `vendor/`, `.cargo/` ni `.github/` y que los scripts con rutas actualizadas
  pasan sus pruebas.
