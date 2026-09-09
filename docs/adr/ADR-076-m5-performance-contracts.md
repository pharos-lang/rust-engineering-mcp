# ADR-076 — Contratos públicos de las cuatro tools M5

Fecha: 2026-09-08.

## Status

Accepted como contrato de M5-01..04. Depende de
[ADR-073](ADR-073-benchmark-method-and-dataset.md) para el método y de
[ADR-074](ADR-074-profiling-capability-and-containment.md) para el permiso de
profiling. La calificación por corte se registra en `docs/validation/M5-*`.

## Context

M5 añade cuatro tools al inventario de veintisiete. G1 congela individualmente las
trece definiciones 0.1.0 y prohíbe que una tool nueva convierta una lectura en
escritura o amplíe en silencio un enum compartido que modifique schemas viejos.
La spec §28 describe las cuatro tools en un párrafo cada una y da un ejemplo de
payload de `benchmark.compare` con `benchmark`, `baseline_ns`, `candidate_ns` y
`change_percent`; §77 exige que flamegraph SVG y benchmark JSON viajen como
artifacts y no dentro de la respuesta; §44 fija el presupuesto `benchmark = 900s`.

## Decision

### 1. Cuatro tools nuevas, cero cambios en las veintisiete existentes

Se añaden `rust.benchmark.run`, `rust.benchmark.compare`, `rust.profile.flamegraph`
y `rust.binary.bloat`. Los veintisiete snapshots previos se conservan byte a byte;
un test de contrato los compara contra la base M4. Ninguna de las cuatro escribe en
el proyecto: las cuatro son `read_only(true)`, `destructive(false)`,
`idempotent(false)`, `open_world(false)`.

### 2. Enums compartidos: se extienden solo donde ya son abiertos por diseño

`QualityArtifactKind`, `QualityMimeType`, `PayloadFormatVersion`,
`GuestArtifactName` y `PluginIdentity` reciben variantes nuevas
(`BenchmarkDataset`, `CriterionArchive`, `CollapsedStacks`, `FlamegraphSvg`,
`BloatJson`; `ImageSvgXml`; `BenchmarkDatasetV2`, `CollapsedStacksV1`,
`FlamegraphSvgV1`, `BloatJsonV1`; `Criterion`, `ProfileHelper`, `Bloat`).

Esas variantes son internas al store durable. **Ninguna aparece en el schema
público de una tool anterior**: los DTO M1–M4 declaran su propio enum cerrado por
tool mediante `#[schemars(with = …)]`, de modo que el schema publicado de
`rust.test.nextest`, `rust.coverage`, `rust.semver.check`, `rust.mutation.test` y
las cinco M4 no cambia. Un test de contrato afirma esa invariancia sobre los
snapshots existentes.

### 3. `rust.benchmark.run`

Entrada cerrada: `project_ref`, `bench_target`, `package`, `features`,
`all_features`, `no_default_features`, `run_count` (1..=3, por defecto 3),
`timeout_seconds` (1..=900, por defecto 900) y `execution_mode`. No admite flags
libres, ni rutas, ni parámetros del harness: warmup, tiempo de medición y tamaño
muestral los fija el servidor según ADR-073 y viajan en la provenance.

Salida: identidad y exit de la ejecución, resumen por benchmark (clave, muestras,
mediana, mínimo, máximo, desviación absoluta mediana, outliers contados,
`sampling_mode`, completeness) y la provenance completa. Las **muestras crudas no
viajan en la respuesta**: se publican como artifact `benchmark_dataset` en el store
privado, junto con el árbol de salida de criterion como `criterion_archive`. La
respuesta se recorta contra el techo de 512 KiB por el mismo mecanismo que M4.

`harness_unrecognized` es un resultado observado: reporta ejecución, exit y logs y
no emite dataset.

### 4. `rust.benchmark.compare`

Entrada cerrada: `project_ref`, `baseline_artifact_id`, `candidate_artifact_id`,
`timeout_seconds` (1..=30, por defecto 30). Los identificadores son opacos
(`qa_` + 32 hex) emitidos por el store; **no son rutas y el peer no puede
componerlos**. La autorización es la del proyecto propietario: un artifact de otro
owner no existe para esta llamada.

No hay `execution_mode`: la comparación es cálculo puro sobre bytes autorizados,
sin procesos, sin contenedor y sin tocar el proyecto. Nunca es un trabajo largo.

Salida: el método completo (estadístico, remuestreos, semilla, nivel de confianza,
corrección por multiplicidad y su familia, umbral material, política de outliers),
y por benchmark el veredicto, el ratio de efecto, el intervalo, las medianas, los
tamaños muestrales, los outliers contados, el MDR y las razones de
`inconclusive`. Además `baseline_only` y `candidate_only`.

Un par incompatible es un **resultado observado**, no un error de infraestructura:
`status = failed`, `error_code = INCOMPATIBLE_DATASETS` y la lista completa de
razones. `isError` permanece `false`, igual que un fallo de compilación en M1.

La salida no contiene ninguna afirmación causal ni recomendación: spec §29 prohíbe
la tool de heurísticas y §92 las recomendaciones universales.

### 5. `rust.profile.flamegraph`

Entrada cerrada: `project_ref`, `binary_target`, `frequency_hz` (1..=999, por
defecto 99), `duration_seconds` (1..=60, por defecto 10), `timeout_seconds`
(1..=300, por defecto 120) y `execution_mode`. El binario se identifica por
**nombre de target**, nunca por ruta, y se ejecuta sin argumentos del peer.

Exige la capability de profiling del host (ADR-074). Sin ella responde `blocked`
con `PROFILING_NOT_AUTHORIZED` antes de crear contenedor alguno.

Salida: identidad y parámetros del perfilador, frecuencia, duración solicitada y
observada, muestras recogidas y perdidas, número de stacks, frames totales y no
resueltos, stacks truncados, módulos vistos, estado del hijo, y un ranking acotado
de frames con muestras propias y totales. Los artifacts son el SVG saneado y los
stacks colapsados. Cero muestras es un resultado válido y declarado, no un fallo.

### 6. `rust.binary.bloat`

Entrada cerrada: `project_ref`, `binary_target`, `package`, `profile`
(`release` | `release_lto`), `timeout_seconds` (1..=300, por defecto 120) y
`execution_mode`. Sin flags libres y sin `--symbols-section` configurable.

Salida, con la distinción explícita que el plan exige:

- **Tamaño exacto**: bytes del archivo medidos por el producto dentro del guest y
  su `sha256`. Es un hecho verificable.
- **Atribución estimada**: `text_section_size_bytes` y los rankings por función y
  por crate que produce `cargo-bloat`, marcados como estimación en el propio DTO
  (`attribution.estimated = true`).

Si el tamaño exacto medido por el producto y el `file-size` reportado por
`cargo-bloat` no coinciden, la completeness es `invalid` y no se publica un ranking
como si describiera ese archivo. WASM se rechaza porque el backend no lo soporta;
Mach-O y PE no quedan calificados por el positivo ELF.

**El archivo medido es un build de análisis.** La calibración en el guest
([recibo](../validation/M5-04-bloat-calibration.json)) muestra que `cargo-bloat`
0.12.1 empuja incondicionalmente `CARGO_PROFILE_<PERFIL>_STRIP=false` porque
necesita la tabla de símbolos (`src/main.rs:694-696`); se comprobó que
`CARGO_PROFILE_RELEASE_STRIP=symbols` no tiene efecto alguno sobre el archivo
producido. El binario medido **no** es byte a byte el que enviaría un proyecto que
pide stripping. El DTO lo declara en `analysis_build_symbols_forced`, siempre
`true`, y un reporte de binario stripped es sencillamente inalcanzable con este
analizador. El tamaño sigue siendo exacto *para ese archivo*; lo que no se afirma
es que sea el artefacto distribuible del proyecto.

`release_lto` **no** se pide con `--profile`. Lo observado, y lo único que se
afirma como observado, es el
[recibo](../validation/M5-04-bloat-calibration.json): pasar `--profile
release-lto` sale con exit 1 y con `error in environment variable
CARGO_PROFILE_RELEASE: could not load config key profile.release / invalid type:
Option value, expected a boolean or string`. El mecanismo —que `cargo-bloat`
0.12.1 deriva del nombre del perfil una variable `CARGO_PROFILE_<PERFIL>_STRIP`
(`src/main.rs:690-696`) y que un perfil con guion produce una variable que Cargo
vuelve a partir sobre `profile.release`— está **inferido de la fuente del
analizador, no medido**: el recibo registra el fallo y su texto, no la variable
exacta que el analizador exportó. Se expresa entonces como `--release` más la
variable de entorno propiedad del producto `CARGO_PROFILE_RELEASE_LTO=fat`, que
es entorno cerrado y no un nombre de perfil suministrado por el peer; esa forma
sí está medida, con el binario resultante estrictamente menor. Ambos perfiles
dejan el binario bajo `<target-dir>/release/`.

### 7. Presupuestos

`run` 900 s, `profile` 300 s con 60 s de muestreo máximo, `compare` 30 s,
`bloat` 300 s. Se conservan los techos de CPU, RAM y PID del runtime calificado.
Artifacts: SVG ≤ 8 MiB, bloat ≤ 4 MiB, muestras ≤ 32 MiB, resultado ≤ 512 KiB.
La cuota se reserva antes de iniciar el trabajo.

## Alternatives considered

- **Devolver las muestras crudas en la respuesta.** Descartado por §77 y por el
  techo de 512 KiB; además obligaría a recortar la evidencia primaria.
- **Referenciar artifacts por URI.** Descartado: introduce texto componible por el
  peer donde basta un identificador opaco ya emitido por el store.
- **Reutilizar `rust.quality.gate.v2` como contenedor de estas cuatro.** Descartado:
  mezclaría un veredicto de calidad con mediciones que no lo son y cambiaría un
  contrato ya calificado.
- **Una sola tool `rust.performance` con un modo.** Descartado: convertiría cuatro
  permisos y cuatro presupuestos distintos en uno solo, y el profiling exige una
  capability que las otras tres no necesitan.

## Consequences

El inventario pasa a treinta y una tools y cinco archivos de test que afirman el
conteo deben actualizarse junto con la lista ordenada de nombres. Cuatro snapshots
nuevos se añaden y los veintisiete previos quedan bajo un test de invariancia.
El identificador opaco de artifact hace que `compare` dependa de un `run` previo
en el mismo proyecto: sin él no hay nada que comparar, y eso es deliberado.
