# M5 — handoff

Fecha: 2026-09-08. Rama: `ai/m5-performance`.
Base: `c6099f27415b0be3838e84d21d25eed903c8c312` (`main` == `origin/main`).

Estado: **M5 no está Done.** Tres de los cinco cortes están calificados
nativamente, uno está bloqueado por una condición reproducible y el cierre
conjunto no se ha ejecutado. Este documento dice qué está demostrado, qué no, y
con qué evidencia.

## Entrada M4 verificada live

`gh pr view 15` devuelve `MERGED`, merge `90d72f2c…`, head `e1be3a37…` y diez
checks en `SUCCESS`. `git diff --stat e1be3a37..main -- crates/ Cargo.toml
Cargo.lock scripts/` es vacío, así que los bytes calificados de M4 son los de
`main`. No se detectó discrepancia con lo que declaraba el encargo.

## Decisiones cerradas

| Decisión | ADR | Qué fija |
| --- | --- | --- |
| D23 | [ADR-073](../adr/ADR-073-benchmark-method-and-dataset.md) | Criterion 0.8.2 como único harness, parámetros congelados por el servidor, dataset v1 con muestras crudas, método estadístico completo antes de medir |
| D24 | [ADR-074](../adr/ADR-074-profiling-capability-and-containment.md) | Capability positiva del host, helper propio, una sola syscall añadida |
| Aprovisionamiento | [ADR-075](../adr/ADR-075-m5-runtime-provisioning.md) | Inputs exactos con licencia y hash, autorizados por separado |
| Contratos | [ADR-076](../adr/ADR-076-m5-performance-contracts.md) | Las cuatro tools, con tamaño exacto separado de atribución estimada |
| Admisión | [ADR-077](../adr/ADR-077-m5-runtime-admission.md) | Un digest añadido a la lista cerrada |

## Estado por corte

| Corte | Estado | Evidencia |
| --- | --- | --- |
| M5-01 `rust.benchmark.run` | **Blocked** en el positivo; negativos y controles calificados | [runtime](M5-01-runtime.json), [bloqueo](M5-01-blocker.json) |
| M5-02 `rust.benchmark.compare` | Implementado; método probado sobre datos reales del guest | [calibración](M5-01-benchmark-calibration.json) |
| M5-03 `rust.profile.flamegraph` | **Calificado nativamente**; oráculo de denegación pendiente en el recibo generado | [runtime](M5-03-runtime.json), [smoke manual](M5-03-profiling-native.json) |
| M5-04 `rust.binary.bloat` | **Calificado nativamente** | [runtime](M5-04-runtime.json), [calibración](M5-04-bloat-calibration.json) |
| M5-05 cierre | **No ejecutado** | — |

### El positivo de profiling

Es la puerta que el plan señalaba y está demostrada. En el guest calificado, con
`--cap-drop=ALL`, `no-new-privileges`, uid 65534, `--network=none`,
`perf_event_paranoid` intacto en 2, sin capability añadida, sin contenedor
privilegiado, sin `sudo` y sin cambio de `sysctl`: 195 muestras, la pila exacta
que la fixture fue diseñada para tener con 192 muestras en `known_hot_frame`,
16 CPUs muestreadas, cero frames con un path. El control de cero muestras
reporta cero como resultado y el control de denegación sigue devolviendo
`profiler_unavailable` con EPERM bajo el perfil de calidad sin modificar.

### El bloqueo de M5-01

`rust.benchmark.run` resuelve el harness desde un `CargoVendorSnapshot`
autenticado por el host. Un `SourceBundle` admite 4 096 entradas, 16 MiB en
total y 1 MiB por archivo. El cierre de criterion 0.8.2 son 6 014 archivos,
779 directorios y 156 267 469 bytes, con cuatro archivos por encima del límite
por archivo; su subconjunto compilado ronda 20 MiB. Ninguna poda lo mete dentro.

**No se subieron los límites.** Son parte del contrato de datos offline
calificado en M2/M4 y compartido por todos los flujos que usan `SourceBundle`.
Ampliarlos para poner una prueba en verde habría debilitado una frontera de
seguridad calificada sin decisión ni recalificación. Las opciones para el owner
están en [M5-01-blocker.json](M5-01-blocker.json).

Lo que sí queda demostrado es el **método**: tres capturas reales del guest
(`fixtures/benchmark-datasets`) se parsean y se comparan con control de
auto-comparación, regresión de dirección conocida y rechazo de `same_artifact`.
El bloqueo es de ingesta del vendor, no de la medición.

## Defectos reales encontrados y corregidos

| Defecto | Cómo se encontró | Consecuencia si hubiera pasado |
| --- | --- | --- |
| El kernel rechaza `perf_mmap` de un evento heredado con `cpu == -1` | smoke en el guest | Profiling parecería una denegación del sandbox para siempre |
| El perfil seccomp de profiling nunca se escribía al state dir | revisión del gateway | Docker habría rechazado el contenedor; la tool nunca podría funcionar |
| `directory_name` de criterion contiene `/` | bytes reales del guest | El parser rechazaba todo export real |
| BuildKit servía fuentes obsoletas pese a `--no-cache` | `SHA256SUMS` dentro de la imagen | La imagen habría llevado el helper equivocado |
| El vendor se definía dos veces (CARGO_HOME + `--config`) | prueba nativa | Ninguna build offline funcionaba |
| El analizador omite la clave `crate` en 81 de 634 filas | prueba nativa | Ninguna atribución se publicaba |
| Se pasaban flags de Criterion a un harness libtest | prueba nativa | Exit 101 fabricado que no dice nada del proyecto |
| Un `.cargo/config.toml` del proyecto redirigía el vendor | smoke en el guest | El proyecto podría sustituir los bytes de sus dependencias |

## Trabajo pendiente para cerrar M5

1. **MCP**: registrado. `stdio.rs` anuncia las cuatro tools, existen 32 snapshots
   —los 27 anteriores sin cambio byte a byte contra el merge M4, los cuatro
   nuevos y `doctor-report`— y los seis conteos dicen 31. Queda una brecha de
   contrato: la descripción publicada de `rust.benchmark.run` promete publicar
   «the harness output tree» y hoy solo se publica el dataset, porque
   `BenchmarkObservation` no transporta los bytes del archivo de criterion.
2. **M5-01**: decisión del owner sobre el contrato de datos offline.
3. **Gates**: `scripts/gate.py core` y `full` sobre los bytes finales, con una
   etapa `m5-runtime` en full que hoy no existe.
4. **Clientes**: Inspector y Codex stock contra las tools nuevas.
5. **Revisiones G8** independientes de contratos, estadística, seguridad y
   profiling, y sus dispositions.
6. **Docs**: README, CHANGELOG, `docs/tools.md`, `docs/architecture.md` y el
   tablero. `docs/security-model.md`, `SECURITY.md`,
   `docs/client-configuration.md`, `docs/ci.md` y `docs/compatibility.md` ya
   están sincronizados.

## Dos caminos de éxito que este runtime no alcanza

Ambos son propiedades medidas, no fallos del entorno, y los dos se publican.

1. **`rust.benchmark.compare` no emite dirección.** El governor de CPU es
   ilegible dentro del contenedor, así que es desconocido en los dos lados:
   `inconclusive` con `unobservable_hardware`. Y aunque se observara, la deriva
   entre ejecuciones del mismo código en este host es del 15 % al 29 % contra un
   umbral del 5 %, de modo que el MDR tampoco lo resuelve. El efecto real se mide
   —un cambio de fuente del +25 % sale como +24,4 % sobre las seis capturas—; lo
   que no se emite es el veredicto. Oráculo:
   `criterion_dataset::admitted_image_datasets`.
2. **`rust.binary.bloat` no devuelve `passed`.** Detallado abajo.

## Hallazgo abierto, deliberadamente sin corregir

`rust.binary.bloat` no puede devolver `passed` para ningún binario que enlace
`std`: el tope propio del producto son 256 filas, el positivo nativo omitió 378
funciones, y la tool convierte toda completeness distinta de `Complete` en
`blocked` / `EVIDENCE_INCOMPLETE`. La respuesta sí lleva los datos completos y el
tamaño exacto, así que no se pierde información; lo que está mal es la palabra.
Un ranking acotado por un límite que el producto eligió y declara es la
atribución estimada que el contrato promete, no evidencia incompleta.

Se deja abierto **a propósito**. Apareció al construir la matriz de clientes, es
decir en el momento exacto en que corregirlo pone una fila en verde, y una
corrección de contrato tomada con ese incentivo no se distingue de un ajuste al
resultado. Necesita decisión propia, cambio de ADR-076 y re-revisión, junto al
mismo defecto en el recorte por presupuesto de respuesta: hoy un solo flag
`complete` cubre dos causas que no significan lo mismo.

## Rollback

Volver a apuntar el gateway al digest M4 `sha256:25ed3626e710…`. No hay estado
que migrar y la evidencia se conserva. `tools/list` sigue devolviendo 31
definiciones —el inventario no depende de la imagen—, las veintisiete anteriores
siguen sirviendo igual y las cuatro de M5 responden `unavailable` antes de crear
contenedor alguno, que es el resultado declarado y no un fallo. La capability de profiling se revoca por separado retirando
`--allow-profiling`, sin reconstruir nada.

## Límites declarados

- Positivo solo en Linux ARM64. Mach-O y PE no quedan calificados; WASM no lo
  soporta el analizador.
- El archivo que mide `rust.binary.bloat` es un build de análisis: el analizador
  fuerza `strip=false` para leer símbolos, así que no es byte a byte el que
  enviaría un proyecto que pide stripping.
- El benchmark llamado `control` en la fixture **no es un control 1,00×**. Se
  diseñó como tal y la medición en el guest lo desmintió: salió un 2,9 % más
  rápido que `reference`. El control de auto-comparación real es el mismo
  benchmark en dos ejecuciones independientes de la misma fuente.
- `BenchmarkExit` y `BloatExit` conservan `CALIBRATED = false`; solo se
  observaron los exits 0 y 1.
- El ruido entre dos ejecuciones del mismo código (−3,9 %) es del mismo orden
  que el umbral material (5 %). Por eso el método exige MDR y admite
  `inconclusive` en lugar de forzar un veredicto.
- El id de la imagen no es reproducible entre construcciones; los binarios
  instalados sí lo son (`cargo-bloat` dio el mismo `sha256` en dos builds).
- No hay tag, release, PR ni push. M6 no está iniciado.
