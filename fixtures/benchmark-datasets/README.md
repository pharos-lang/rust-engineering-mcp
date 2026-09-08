# Datasets reales de criterion (oráculo M5-01/M5-02)

Tres archivos USTAR capturados del guest Linux ARM64 el 2026-09-08, exportados
tal cual desde `CRITERION_HOME` con
`tar --create --format=ustar --sort=name --one-file-system`. Son bytes reales de
`criterion 0.8.2`, no fixtures escritas a mano.

| Archivo | Fuente | Qué representa |
| --- | --- | --- |
| `criterion-run-1.tar` | `fixtures/benchmark` sin modificar | baseline |
| `criterion-run-2.tar` | la **misma** fuente, ejecución independiente posterior | control de auto-comparación |
| `criterion-candidate.tar` | la misma fixture con `work_unit` haciendo un 25 % más de operaciones | candidate |

Los tres se produjeron con los parámetros congelados de
[ADR-073](../../docs/adr/ADR-073-benchmark-method-and-dataset.md): warmup 3 s,
tiempo de medición 5 s, `--sample-size 30`, `--noplot`, `--color never`, bajo
`--frozen --offline`, uid 65534, `--network=none`, `--cap-drop=ALL` y el perfil
seccomp de calidad, con el vendor seleccionado por `--config` en la línea de
comandos.

## Medianas observadas (ns por iteración)

| Benchmark | run 1 | run 2 | candidate |
| --- | --- | --- | --- |
| `m5/reference` | 3299,0 | 3168,7 | 3721,9 |
| `m5/slower_125` | 4093,4 | 4101,7 | 3908,7 |
| `m5/control` | 3203,5 | 3300,7 | 2894,4 |

De ahí salen los tres oráculos que usan las pruebas:

- **run 1 contra run 2**, mismo código: `m5/reference` cambia un −3,9 %. Es ruido
  del host, no un efecto. El veredicto correcto **no** es `regression` ni
  `improvement`; con 30 muestras y esa dispersión puede ser `no_material_change`
  o `inconclusive`, y ambas cosas son respuestas honestas. La prueba afirma
  exactamente eso y no más.
- **run 1 contra candidate**, código distinto: `m5/reference` sube un +12,8 %,
  claramente por encima del umbral material del 5 %.
- **run 1 contra run 1**: mismo `execution_fingerprint`, que la comparación
  rechaza como `same_artifact`.

El ruido entre dos ejecuciones del mismo código (−3,9 %) es del mismo orden que
el umbral material (5 %). Eso no es un defecto de la fixture: es precisamente la
razón por la que el método exige un minimum detectable ratio y admite
`inconclusive` en lugar de forzar un veredicto.

Los números de esta tabla describen **estas** capturas en **este** host. No son
una tolerancia calibrada ni se generalizan a otro hardware.
