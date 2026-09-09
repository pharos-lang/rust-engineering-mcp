# M5 — matriz de implementación y calificación

Fecha de apertura: 2026-09-08. Rama de trabajo: `ai/m5-performance`. Base:
`c6099f27415b0be3838e84d21d25eed903c8c312`.

Estado: **In progress**. Este documento se completa por corte; ninguna fila pasa
a Done sin su recibo enlazado. Un gate anterior nunca acredita código nuevo.

## Entrada M4 verificada live

| Hecho declarado por el encargo | Comprobación | Resultado |
| --- | --- | --- |
| M4 integrado por PR #15 | `gh pr view 15` | `MERGED`, `mergedAt 2026-09-08T17:58:12Z` |
| Merge commit `90d72f2c…` | `git log -1 90d72f2c…` | existe y es ancestro de `main` |
| Head validado `e1be3a37…` | `gh pr view 15 --json headRefOid` | coincide; es ancestro de `main` |
| Checks aprobados | `statusCheckRollup` | 10/10 `SUCCESS`: `portable` ×3, `supply chain`, `SonarCloud`, `SonarCloud Code Analysis`, `CodeQL` ×4 |
| Bytes calificados == `main` | `git diff --stat e1be3a37..HEAD -- crates/ Cargo.toml Cargo.lock scripts/` | vacío |
| `main` == `origin/main` | `git rev-parse` | `c6099f27…` en ambos |

No se detectó discrepancia. El gate local 33/33 de M4 se acredita por su
[recibo](M4-full-gate.json); no se volvió a ejecutar sobre el mismo código.

## Decisiones cerradas antes de implementar

| Decisión | ADR | Evidencia |
| --- | --- | --- |
| D23 — método de benchmark, dataset y comparación | [ADR-073](../adr/ADR-073-benchmark-method-and-dataset.md) | Formato `sample.json` leído del `.crate` verificado de criterion 0.8.2; método congelado antes de medir |
| D24 — capability de profiling y containment | [ADR-074](../adr/ADR-074-profiling-capability-and-containment.md) | [Prueba de capability](M5-profiling-capability-probe.json) |
| Aprovisionamiento M5 | [ADR-075](../adr/ADR-075-m5-runtime-provisioning.md) | Autorización separada del owner sobre el [dossier](../roadmap/m5-provisioning-request.md) |
| Contratos de las cuatro tools | [ADR-076](../adr/ADR-076-m5-performance-contracts.md) | Veintisiete schemas previos bajo test de invariancia |

### D24 — el positivo es alcanzable sin privilegios

| Perfil seccomp | `perf_event_open` | Veredicto |
| --- | --- | --- |
| `seccomp-rust-quality.json` | `-1`, `errno=1` | denegado |
| `seccomp-rust-profile.json` (el mismo + una syscall) | `fd=3`, ring buffer mapeado, `data_head=560` | muestras reales |

Con `--cap-drop=ALL`, `no-new-privileges`, `--network=none`, uid 65534 y
`perf_event_paranoid=2` sin tocar. Sin `sudo`, sin contenedor privilegiado, sin
`--cap-add` y sin cambio de `sysctl`.

## Cortes

| ID | Corte | Estado | Evidencia |
| --- | --- | --- | --- |
| M5-01 | `rust.benchmark.run` | **Blocked** para el positivo; negativos y controles calificados | [runtime](M5-01-runtime.json) · [bloqueo](M5-01-blocker.json) · [calibración](M5-01-benchmark-calibration.json) |
| M5-02 | `rust.benchmark.compare` | Implementado y probado sobre datasets reales del guest | [calibración](M5-01-benchmark-calibration.json) · `criterion_dataset::real_guest_datasets` |
| M5-03 | `rust.profile.flamegraph` | **Calificado nativamente**; el oráculo de denegación sigue pendiente en el recibo generado | [runtime](M5-03-runtime.json) · [smoke manual](M5-03-profiling-native.json) · [capability](M5-profiling-capability-probe.json) |
| M5-04 | `rust.binary.bloat` | **Calificado nativamente** | [runtime](M5-04-runtime.json) · [calibración](M5-04-bloat-calibration.json) |
| M5-05 | Cierre, clientes y gate conjunto | In progress | — |
| — | Admisión de imagen | Calificada | [runtime](M5-00-admission-runtime.json) · [ADR-077](../adr/ADR-077-m5-runtime-admission.md) |

### Selecciones nativas observadas

Imagen `sha256:e0a5ca1661b3e49d0a3d68ee3cc0963453078d08eb7fc43c30538c16b7998aac`.
Recibo conjunto: [M5-runtime.json](M5-runtime.json).

| Corte | Selección | Resultado |
| --- | --- | --- |
| M5-00 | `unqualified-image-refused` | passed |
| M5-01 | `positive-run-count-1` | **blocked** (ver abajo) |
| M5-01 | `pooled-run-count-2` | **blocked** (ver abajo) |
| M5-01 | `unrecognised-harness` | passed; exit 0, sin dataset, `HarnessUnrecognized`, logs reportados |
| M5-01 | `project-cargo-configuration-refused` | passed; rechazado antes de crear volumen, sin residuo |
| M5-01 | `cancellation-mid-run` | passed; `cargo bench` observado vivo, cancelado, árbol unido, sin residuo |
| M5-03 | `profile-positive` | passed |
| M5-03 | `profile-cpus-sampled` | passed |
| M5-03 | `zero-sample-control` | passed |
| M5-03 | `cancellation-during-profiling` | passed |
| M5-04 | `release-positive` | passed; tamaño medido == tamaño reportado |
| M5-04 | `release-lto` | passed; binario estrictamente menor |
| M5-04 | `missing-binary-target` | passed; fallo observado, no error de infraestructura |

### M5-01 — condición de bloqueo reproducible

`rust.benchmark.run` resuelve el harness offline desde un `CargoVendorSnapshot`
autenticado por el host, igual que `rust.miri`. Un `SourceBundle` admite como
máximo 4 096 entradas, 16 MiB en total y 1 MiB por archivo. El cierre de
`criterion 0.8.2` para `aarch64-unknown-linux-gnu` son 52 paquetes, 6 014
archivos, 779 directorios y 156 267 469 bytes, con cuatro archivos por encima
del límite por archivo. Podar los no compilados tampoco sirve, y la razón está
medida, no estimada: los cuatro archivos que superan el límite pertenecen a
paquetes solo-Windows y Cargo exige que todo paquete del lockfile esté presente
en un directory source, así que no se pueden quitar. El «subconjunto compilado
ronda los 20 MiB» que figuraba aquí era una estimación y se retira; no sostiene
nada, porque el límite por archivo ya se rompe antes de llegar a los otros dos.

**Los límites no se subieron.** Pertenecen al contrato de datos offline
calificado en M2/M4 ([ADR-055](../adr/ADR-055-offline-cargo-data-and-lock-policy.md))
y todos los flujos que comparten `SourceBundle` dependen de ellos; ampliarlos
para poner en verde una prueba debilitaría una frontera de seguridad calificada
sin decisión ni recalificación. Detalle y opciones para el owner en
[M5-01-blocker.json](M5-01-blocker.json).

Lo que **sí** queda demostrado del método: las tres capturas reales del guest
en `fixtures/benchmark-datasets` se parsean y se comparan, con control de
auto-comparación, regresión de dirección conocida y rechazo de `same_artifact`.
El bloqueo es de ingesta del vendor, no del método de medición.

## Limitaciones declaradas hasta ahora

- El benchmark `control` de la fixture **no es un control 1,00×**, pese a su
  nombre. La medición en el guest lo desmiente: salió un 2,9 % más rápido que
  `reference`. El control de auto-comparación real es el mismo benchmark en dos
  ejecuciones independientes de la misma fuente. Corregido en el código de la
  fixture, en su README y aquí.
- El ratio 1,25× de la fixture de benchmarks es un ratio **de diseño**, no una
  medición validada. Tres ejecuciones en el host dieron 1,256, 1,331 y 1,302 para
  `slower_125/reference` y 1,013, 1,002 y 0,976 para `control/reference`. Ninguna
  tolerancia se deriva de esos números; la calibración pertenece al guest.
- Toda la evidencia de fixtures registrada hasta aquí es macOS ARM64. El
  comportamiento en el guest Linux ARM64 se califica por separado.
- `BenchmarkExit` y `BloatExit` declaran `CALIBRATED = false`: la tabla de exits
  es una hipótesis hasta que un recibo del guest la registre.
- La fixture de benchmarks no compila desde un checkout limpio hasta ejecutar
  `fixtures/criterion-vendor/materialize.py`; el árbol extraído es generado.
