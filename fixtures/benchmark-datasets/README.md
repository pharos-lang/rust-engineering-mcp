# Datasets reales de criterion (oráculo M5-01/M5-02)

Nueve archivos USTAR con bytes reales de `criterion 0.8.2`, exportados tal cual
desde `CRITERION_HOME` con
`tar --create --format=ustar --sort=name --one-file-system`. Ninguno está
escrito a mano.

**No todos vienen de la misma imagen, y eso importa.** ADR-077 admite
exactamente un digest M5, y una medición tomada sobre otra imagen no acredita
nada sobre el runtime del producto.

| Archivo | Imagen | Fecha | Qué es |
| --- | --- | --- | --- |
| `criterion-baseline-1.tar` | `sha256:e0a5ca16…` (admitida) | 2026-09-09 | baseline, ejecución 1 |
| `criterion-baseline-2.tar` | `sha256:e0a5ca16…` (admitida) | 2026-09-09 | baseline, ejecución 2 |
| `criterion-baseline-3.tar` | `sha256:e0a5ca16…` (admitida) | 2026-09-09 | baseline, ejecución 3 |
| `criterion-candidate-1.tar` | `sha256:e0a5ca16…` (admitida) | 2026-09-09 | candidate, ejecución 1 |
| `criterion-candidate-2.tar` | `sha256:e0a5ca16…` (admitida) | 2026-09-09 | candidate, ejecución 2 |
| `criterion-candidate-3.tar` | `sha256:e0a5ca16…` (admitida) | 2026-09-09 | candidate, ejecución 3 |
| `criterion-run-1.tar` | `sha256:e9ecc40d…` (**no admitida**) | 2026-09-08 | fixture de parser |
| `criterion-run-2.tar` | `sha256:e9ecc40d…` (**no admitida**) | 2026-09-08 | fixture de parser |
| `criterion-candidate.tar` | `sha256:e9ecc40d…` (**no admitida**) | 2026-09-08 | fixture de parser |

El digest admitido lo declara `M5_IMAGE` en
`crates/execution-adapter/src/performance_port.rs`; la admisión está en
[ADR-077](../../docs/adr/ADR-077-m5-runtime-admission.md) y el recibo de estas
seis capturas en
[`docs/validation/M5/01-benchmark-calibration.json`](../../docs/validation/M5/01-benchmark-calibration.json).

## Las seis capturas de la imagen admitida

Las produce, y las vuelve a producir, un script:

```text
python3 -B scripts/capture-m5-benchmark-datasets.py
```

El script se niega a medir sobre cualquier imagen que no sea la que `M5_IMAGE`
nombra, no descarga nada (`--pull=never`), ningún contenedor tiene red y limpia
todos los contenedores y volúmenes que crea en cualquier salida, incluido el
fallo. Escribe además el recibo, de modo que los números publicados y los bytes
publicados salen del mismo acto.

- **baseline**: `fixtures/benchmark` sin modificar.
- **candidate**: la misma fixture con `work_unit` haciendo `n + n / 4`
  operaciones en vez de `n` — un 25 % más del mismo `step`. Solo cambia el
  límite del bucle; `work_slower`, `work_noisy`, `step`, `SEED` y
  `benches/perf.rs` son idénticos en los dos lados. La modificación se aplica a
  una copia; `fixtures/benchmark` nunca se escribe.

Cada ejecución es un contenedor distinto con su propio `CRITERION_HOME`, no una
ronda de muestreo más dentro del mismo proceso: lo que estas capturas tienen que
poder mostrar es la deriva **entre** ejecuciones. Las tres ejecuciones de un lado
comparten un único volumen de target, igual que las repeticiones de una sola
operación del producto, así que el binario se compila una vez y se mide tres
veces. Los dos lados se alternan (baseline, candidate, baseline, …), que es un
protocolo del operador y no un control que el producto implemente.

Parámetros congelados de [ADR-073](../../docs/adr/ADR-073-benchmark-method-and-dataset.md)
§2: warmup 3 s, medición 5 s, `--sample-size 30`, `--noplot`, `--color never`,
bajo `--frozen --offline`. Contención: uid 65534:65534, `--network=none`,
`--cap-drop=ALL`, `--security-opt no-new-privileges`, perfil seccomp de calidad,
raíz y fuentes montadas de solo lectura, `--pids-limit=128`, `--cpus=1`,
`--memory=1g`. El vendor lo selecciona `--config` en la línea de comandos de
cargo, que es la precedencia más alta de Cargo.

El guest calcula el sha256 de su propio `/source` antes de cada ejecución y el
recibo lo publica: las tres ejecuciones de un lado midieron los mismos bytes, y
los dos lados difieren únicamente en `src/lib.rs`.

## Medianas observadas (ns por iteración)

| Benchmark | baseline 1 | baseline 2 | baseline 3 | candidate 1 | candidate 2 | candidate 3 |
| --- | --- | --- | --- | --- | --- | --- |
| `m5/reference` | 3140,47 | 3637,42 | 3292,42 | 3906,48 | 4507,92 | 4095,82 |
| `m5/slower_125` | 3472,51 | 4469,82 | 4106,11 | 4443,32 | 4561,18 | 4101,57 |
| `m5/control` | 2776,87 | 3020,74 | 3286,67 | 3150,67 | 3265,99 | 3077,58 |

Mediana de las medianas de cada lado, y el cambio entre lados:

| Benchmark | baseline | candidate | cambio | fuente |
| --- | --- | --- | --- | --- |
| `m5/reference` | 3292,42 | 4095,82 | **+24,40 %** | **distinta** |
| `m5/slower_125` | 4106,11 | 4443,32 | +8,21 % | idéntica |
| `m5/control` | 3020,74 | 3150,67 | +4,30 % | idéntica |

`m5/reference` es el único benchmark cuyo fuente cambia, y su +24,4 % coincide
con el 25 % de más operaciones que la modificación introduce por construcción.
El +8,2 % y el +4,3 % de los otros dos **no son efectos**: su fuente es idéntica
en los seis archivos, así que lo que miden es la máquina.

## Deriva entre ejecuciones del mismo lado

| Benchmark | baseline | candidate | umbral material |
| --- | --- | --- | --- |
| `m5/reference` | 15,82 % | 15,40 % | 5 % |
| `m5/slower_125` | 28,72 % | 11,21 % | 5 % |
| `m5/control` | 18,36 % | 6,12 % | 5 % |

Recorrido entre la mayor y la menor de las tres medianas de cada lado, como
fracción de la menor. **Las seis cifras superan el umbral material del 5 %**, y
lo superan con código que no cambia dentro de cada lado.

Ese es el hecho central de este corpus y la razón de la corrección de ADR-073
§4: un intervalo que solo remuestrea las muestras de una ejecución describe la
dispersión equivocada, porque la cantidad sobre la que opina el veredicto es
cuánto se mueve el estadístico **entre** ejecuciones. Con esta deriva el
`minimum detectable ratio` que el método calcula sobre estas capturas queda muy
por encima del 5 %, así que la puerta de precisión puede negarse a emitir
dirección incluso para el +24,4 % de `m5/reference`. Eso es el método siendo
honesto sobre lo que este host permite resolver, no un defecto de las capturas:
el efecto está en los bytes, medido y reproducible, y la incertidumbre también.

Las capturas sostienen entonces tres cosas distintas, y conviene no confundirlas:

- **dirección conocida**: `m5/reference`, baseline contra candidate. El único
  cambio de fuente del corpus, con su tamaño esperado.
- **nulo real**: `m5/slower_125` y `m5/control`, baseline contra candidate. Misma
  fuente en los dos lados; cualquier dirección que se reporte ahí es deriva.
- **mismo artifact**: comparar una captura consigo misma, que la comparación
  rechaza por `same_artifact`.

Ninguna de las tres es una tolerancia calibrada. Estos números describen **estas**
capturas en **este** host.

## Los tres archivos de 2026-09-08

`criterion-run-1.tar`, `criterion-run-2.tar` y `criterion-candidate.tar` se
midieron sobre `sha256:e9ecc40d023d9d13ac3539cccb6a944cd1022da2a8b3f86ca61356086b38a209`,
que **no** es la imagen que ADR-077 admite. Se conservan intactos porque
`crates/execution-adapter/src/criterion_dataset.rs` los incrusta con
`include_bytes!` como corpus real del parser, y esa función no depende de qué
imagen los produjo. Como evidencia sobre el runtime admitido no valen nada.

Sus medianas, para que el archivo no quede sin describir:

| Benchmark | `criterion-run-1` | `criterion-run-2` | `criterion-candidate` |
| --- | --- | --- | --- |
| `m5/reference` | 3298,98 | 3168,69 | 3721,87 |
| `m5/slower_125` | 4093,39 | 4101,68 | 3908,70 |
| `m5/control` | 3203,46 | 3300,74 | 2894,36 |

Cada uno es **una** ejecución. Bajo el método corregido, un lado con menos de
tres ejecuciones no puede sostener ninguna dirección: la negativa es
`insufficient_executions` y se decide antes de mirar el intervalo. Para eso
existen las seis capturas nuevas, que son tres ejecuciones por lado.
