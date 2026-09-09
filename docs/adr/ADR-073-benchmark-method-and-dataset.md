# ADR-073 — Método de benchmark, dataset versionado y comparación (D23)

Fecha: 2026-09-08.

## Status

Accepted como contrato D23. Cierra la decisión que el
[backlog](../roadmap/adr-backlog-m2-m8.md#d23--método-de-benchmark-y-size-report)
dejaba Proposed con fecha límite M5-01/M5-04. La implementación y la calificación
nativa se registran por separado en `docs/validation/M5-*`.

## Context

M5 debe medir benchmarks existentes y comparar resultados sin afirmar causalidad.
La spec describe `rust.benchmark.run` y `rust.benchmark.compare` en §28.1/§28.2 y
un ejemplo de payload con `baseline_ns`/`candidate_ns`/`change_percent`, pero no
contiene ninguna metodología: una búsqueda exhaustiva del término `sample`,
`warmup`, `statistic`, `MDR`, `confidence` e `interval` sobre las 5 367 líneas de
la propuesta no devuelve ninguna aparición. El plan M5 propone un protocolo
(warmup 3 s, 30 muestras, tres ejecuciones, umbral 5 %) y dice explícitamente que
**es una propuesta a aprobar en D23, no un claim estadístico validado**.

`cargo bench` admite harnesses distintos. Un harness desconocido puede reportar
ejecución y logs, pero no puede producir medidas comparables. El harness libtest
(`#[bench]`) solo publica mediana ± desviación: no expone muestras crudas, así que
no permite recalcular un intervalo ni estimar precisión.

Se inspeccionó el código de `criterion 0.8.2` extraído del archivo `.crate`
verificado (`950046b2aa2492f9a536f5f4f9a3de7b9e2476e575e05bd6c333371add4d98f3`):

- `src/analysis/mod.rs:163-165` escribe `<out>/<id>/new/sample.json`.
- `src/lib.rs:1519` define ese archivo como
  `{ sampling_mode, iters: Vec<f64>, times: Vec<f64> }` — **muestras crudas**.
- `src/analysis/mod.rs:244-246` escribe `<out>/<id>/new/benchmark.json` con la
  identidad (`group_id`, `function_id`, `value_str`, `full_id`, `directory_name`).
- `src/lib.rs:146` acepta `CRITERION_HOME` para reubicar la salida.
- El harness acepta `--warm-up-time`, `--measurement-time`, `--sample-size`,
  `--noplot`, `--color` y `--output-format` como flags cerrados.

## Decision

### 1. Harness exacto

La primera integración es **Criterion 0.8.2**, con `default-features = false` y
`features = ["cargo_bench_support"]`. Es el único harness que M5 sabe medir.
Cualquier otro harness produce `harness_unrecognized`: la tool reporta ejecución,
exit y logs, y **no** emite dataset ni medidas. No se infiere un harness por el
nombre del target ni por el contenido del manifest del proyecto.

La versión se verifica antes de medir con una fase de identidad dedicada, igual
que `SemverChecksVersion` en ADR-062: si la versión observada no es exactamente
la aprobada, la operación es `unavailable`, no una medida degradada.

### 2. Parámetros congelados antes de medir

El servidor —no el proyecto ni el peer— fija estos valores y los emite en la
provenance de cada dataset:

| Parámetro | Valor | Razón |
| --- | --- | --- |
| warmup | 3 s | valor propuesto por el plan M5; queda fijado aquí |
| muestras por benchmark | 30 mínimo (`--sample-size 30`) | mínimo del plan |
| tiempo de medición | 5 s | acota el presupuesto de 900 s de spec §44 |
| repeticiones independientes | 1..=3 por llamada, por defecto 3 | plan M5 |
| unidad | nanosegundos por iteración | única unidad del dataset v1 |
| plots | desactivados (`--noplot`) | no se distribuye plotters |
| color | `never` | salida determinista |

El orden de ejecución dentro de una repetición es el orden de declaración del
harness y así se registra en el dataset. **La herramienta no alterna el orden y
no lo afirma**: alternar baseline y candidate es un protocolo del operador, que
la documentación describe, no un control que el producto implemente. Preferimos
declarar el orden real a anunciar un control que no existe.

`--sample-size`, `--warm-up-time` y `--measurement-time` se pasan como argv
cerrado. El proyecto no puede alterarlos: un `criterion.toml` o un
`Criterion::default().sample_size(..)` del proyecto sigue siendo código del
proyecto y su efecto queda registrado en el dataset como el valor **solicitado**
frente al número de muestras **observado**. Si difieren, el dataset lo declara y
la comparación lo trata como incompatibilidad de método.

### 3. Dataset versionado

Formato `rust-engineering-mcp.benchmark-dataset.v2`, con versión propia,
independiente del SemVer del servidor y del contrato de las tools (G6). Un lector
que no reconozca exactamente ese identificador y `format_version = 2` **falla
cerrado**; nunca coerciona ni migra medidas. El v1 —descrito abajo en
«Corrección»— queda retirado, no migrado: nunca se publicó, y un payload v1 se
rechaza como cualquier otro formato ajeno.

Contiene: identidad del benchmark (los cinco campos de criterion), muestras
crudas (`iterations`, `total_ns` y `run_index` por muestra), `sampling_mode`, warmup, tiempo de
medición, tamaño solicitado, completeness, y una provenance con `source_fingerprint`,
harness y versión, `rust_version`, `cargo_version`, toolchain declarado, digest de
imagen, plataforma, `configuration_fingerprint`, `execution_fingerprint`, selección
(paquete, target de bench, features, perfil), hardware (modelo de CPU, núcleos,
kernel, arquitectura, virtualización, governor) y cuotas de CPU/RAM/PID.

`run_index` es la novedad del v2: la posición 1-based de la ejecución
independiente que produjo **cada muestra**, dentro del `run_count` que declara la
provenance. Va en la muestra y no en la medición porque una medición agrupa las
repeticiones de una misma clave de benchmark; sin el campo, un lector no puede
distinguir las muestras de una ejecución de las de otra. Lo fija el gateway —que
es quien sabe qué repetición acaba de ejecutar—, nunca el proyecto: un archivo de
criterion no puede nombrar su propia repetición.

**Unknown permanece unknown.** Un campo de hardware que el runtime no puede
observar se serializa ausente y bloquea la comparación; no se rellena con un valor
plausible. Una baseline **no** se identifica por nombre de rama.

### 4. Método estadístico congelado

`rust.benchmark.compare` es cálculo puro sobre bytes autorizados. No ejecuta
procesos, no lee paths del peer y no toca el proyecto. El método se identifica en
cada informe como `rust-engineering-mcp.benchmark-comparison.v2`; el v1 lleva otro
identificador porque sus intervalos no son comparables con estos (ver
«Corrección» más abajo).

- Estadístico: **mediana** del tiempo por iteración. Es robusto frente a los
  outliers que el propio protocolo prohíbe descartar.
- Intervalo: **bootstrap percentil por conglomerados (cluster bootstrap)** con
  10 000 remuestreos, semilla fija derivada de una constante del producto
  mezclada con la clave del benchmark, de modo que el resultado es reproducible y
  no depende del orden de ejecución. **La unidad que se remuestrea es la
  ejecución, no la muestra**: en cada remuestreo se toman con reemplazo tantas
  ejecuciones como ejecuciones tenga ese lado y, dentro de cada ejecución
  extraída, tantas muestras con reemplazo como muestras reportó; la mediana se
  recalcula sobre el conjunto resultante. Así la varianza **entre** ejecuciones
  entra en el intervalo y en el `SE`. Es la varianza que el veredicto necesita:
  dos datasets solo son comparables si su `execution_fingerprint` difiere, de
  modo que la cantidad sobre la que se opina es cuánto se mueve el estadístico
  entre ejecuciones, no cuánto se movería al releer una sola.
- Confianza: 95 %. Con familia de más de una comparación se aplica **Bonferroni**:
  `1 - (1 - 0.95)/n`. La familia y la corrección se emiten en el resultado.
- Umbral material: **5 %**.
- Outliers: se cuentan con vallas de Tukey (`Q1 - 1.5·IQR`, `Q3 + 1.5·IQR`) y
  **se reportan sin eliminarlos**. No hay descarte a posteriori.
- **Minimum detectable ratio**: `MDR = (z_{1-α/2 ajustado} + z_{0.80}) · SE`, con
  `SE` la desviación típica de la distribución bootstrap del ratio. Es la
  definición explícita: el menor ratio verdadero que ese tamaño muestral y esa
  dispersión podrían detectar, con 80 % de potencia, al nivel ajustado. **El MDR
  no se iguala al umbral del 5 %.**

Veredicto, en este orden: muestra ausente o truncada, o menos de 10 muestras, o
mediana de baseline no positiva ⇒ `inconclusive` con su razón. **Menos de dos
`run_index` distintos en cualquiera de los dos lados ⇒ `inconclusive` por
`single_execution_per_side`**, antes de mirar el intervalo: con una sola
ejecución por lado no existe estimación alguna de la deriva entre ejecuciones, y
sin ella ninguna dirección distingue un cambio en el código de un cambio en la
máquina. **Dispersión degenerada ⇒ `inconclusive` por `degenerate_dispersion`**:
si el error estándar del bootstrap es cero —o los dos lados juntos
tienen menos de dos valores por iteración distintos— el `MDR` vale cero y la
puerta de precisión no puede dispararse nunca; una dispersión observada de cero
es **ausencia de información** sobre la dispersión, no precisión infinita, y un
harness que emita una constante recibiría si no el veredicto más confiado que
este método sabe producir. `MDR` mayor que el umbral ⇒ `inconclusive` por
precisión insuficiente: la ejecución no puede discriminar el umbral y no se emite
veredicto. Intervalo completamente por encima de `+5 %` ⇒ `regression`;
completamente por debajo de `-5 %` ⇒ `improvement`; completamente dentro de
`±5 %` ⇒ `no_material_change`; en cualquier otro caso `inconclusive` porque el
intervalo cruza el umbral.

#### Corrección (2026-09-08)

La forma anterior de este método —`rust-engineering-mcp.benchmark-comparison.v1`,
sobre el dataset v1— remuestreaba **las muestras dentro de una sola ejecución**.
Eso estima cuánto se movería la mediana al releer esa misma ejecución, no cuánto
se mueve entre ejecuciones, que es lo único sobre lo que el veredicto opina. El
`SE` así calculado es mucho menor, y con él el `MDR`, de modo que la puerta de
precisión dejaba pasar ruido del host como dirección.

Una revisión independiente lo demostró sobre las capturas reales del propio
proyecto, en `fixtures/benchmark-datasets/`. Entre `criterion-run-1.tar`,
`criterion-run-2.tar` y `criterion-candidate.tar` solo cambia el fuente de
`work_unit` (detrás de `m5/reference`); `work_noisy` (`m5/control`) y
`work_slower` (`m5/slower_125`) son **el mismo fuente** en los tres archivos.
Comparando `m5/control` solo —familia de uno, la corrección por multiplicidad más
laxa y por tanto el intervalo más estrecho—, baseline `criterion-run-2.tar` contra
candidate `criterion-candidate.tar`, el método v1 devolvía:

```
verdict = Improvement, effect -0.1231, interval -0.1492..-0.0756, mdr 0.0488
```

Una dirección, con intervalo que excluye el umbral del 5 % y un `MDR` que pasa su
puerta (0.0488 ≤ 0.05), para código que no cambió. Los mismos benchmarks
inalterados abarcan 14,0 % y 4,9 % entre las tres capturas. Con el método v2 el
mismo par devuelve `Inconclusive` por `single_execution_per_side`: cada captura es
**una** ejecución, y con una ejecución por lado el intervalo no puede rescatar
nada —remuestrear un único conglomerado devuelve el mismo intervalo estrecho
(-0.1494..-0.0751, `MDR` 0.0490)—, por eso la negativa es estructural y se decide
antes de mirarlo.

Lo que **no** cambia: el estadístico (mediana del tiempo por iteración), la
semilla y su derivación, los 10 000 remuestreos, el 95 % de confianza, Bonferroni,
el umbral material del 5 % y la política de outliers. Cambia la unidad de
remuestreo y se añaden dos negativas explícitas. El razonamiento original queda
arriba, corregido, no borrado: era correcto sobre qué estadístico usar y sobre no
descartar outliers, y era incorrecto al suponer que un bootstrap sobre las
muestras describía la variabilidad relevante.

### 5. Compatibilidad antes que estadística

Se rechaza la comparación, enumerando **todas** las razones, si difieren formato,
unidad, harness, versión de harness, `rust_version`, `cargo_version`, digest de
imagen, plataforma, arquitectura, selección, cuotas o modelo de CPU, o si el
modelo de CPU es desconocido en cualquiera de los dos lados. `source_fingerprint`
**puede** diferir: baseline y candidate son código distinto, y esa es la razón de
comparar. Comparar un artifact con **el mismo** `execution_fingerprint` se rechaza
como `same_artifact`; comparar dos ejecuciones independientes del mismo código es
un control legítimo y esperado.

### 6. Límites de interpretación

El resultado describe una medición en un host concreto. No se atribuye causa, no
se generaliza a otro hardware ni a otro proyecto, y no se emite ninguna
recomendación de optimización: spec §29 prohíbe una tool de heurísticas y §92
prohíbe las recomendaciones universales. Las muestras producidas por el harness
del **proyecto** se describen como observaciones de origen no autenticado; el
producto no afirma que un benchmark no pueda falsificar su propia salida. El caso
más barato de esa falsificación —emitir una constante para obtener un intervalo de
ancho cero— tiene ahora una negativa nombrada, `degenerate_dispersion`, que no
convierte la ausencia de dispersión en precisión.

## Alternatives considered

- **Harness libtest en nightly.** Descartado: sin muestras crudas, el intervalo y
  el MDR serían incomputables y el dataset perdería su propiedad esencial.
- **`cargo-criterion` como plugin externo.** Descartado: añade un componente de
  terceros al runtime sin aportar nada que `criterion` no escriba ya en disco.
- **Reutilizar los `estimates.json` de criterion.** Descartado: el intervalo
  quedaría definido por la versión del harness, no por el producto, y cambiaría en
  silencio con una actualización. Se conserva el archivo como artifact, pero el
  veredicto se calcula sobre las muestras crudas.
- **Umbral fijo del 5 % como criterio único.** Descartado: sin MDR, un umbral
  aislado convierte ruido en veredicto.
- **Media en vez de mediana.** Descartado: obliga a descartar outliers para ser
  estable, y el protocolo lo prohíbe.

## Consequences

Tres repeticiones no prueban causalidad ni generalización; el plan ya lo advierte
y el contrato lo hace explícito en la salida. Un proyecto sin criterion 0.8.2
vendorizado offline no puede medirse: es un resultado declarado, no una
degradación silenciosa. El formato v2 fija una frontera de migración: conservar
muestras crudas o volver a ejecutar, nunca transformar mediciones incompatibles en
equivalentes. Con una sola ejecución por lado el producto no emite dirección: es
menos de lo que la spec insinuaba y es lo único que las muestras sostienen; para
obtener una dirección hay que capturar al menos dos ejecuciones independientes
por lado, que es lo que el protocolo de tres repeticiones ya ejecuta. Añadir un segundo harness exigirá una decisión nueva y su propio
oráculo.
