# ADR-072 — D21, integridad de clasificación Miri

## Status

Accepted. El gateway sobre la imagen final pasó 13 casos de clasificación y
7 de admisión/ciclo de vida (202.007 s), incluidos el panic con warning de
optimizaciones, binary-only, timeout del runner, configuración hostil y
cancelación observada. El [recibo nativo renovado](../validation/M4/miri-native.json)
se vincula al [runtime final 19/19](../validation/M4/runtime.json), al
[full 33/33](../validation/M4/full-gate.json) y a los
[clientes calificados](../validation/M4/clients.json). El full conserva la
reanudación documentada después de corregir la ruta local de E5; no hubo cambios
de source ni descarga. La confirmación independiente final sigue el handoff.

## Context

El Miri fijado a Rust commit `5a2be9f5f075d31e3ca5526b5b029881ce441253`
devuelve el mismo exit 1 para UB, unsupported y otros errores del intérprete.
Su clasificación está en diagnósticos. El programa interpretado puede falsificar
stdout/stderr; proc macros comparten además los streams nativos del compilador.
JSON de rustc por sí solo no autentica su productor. No se puede presentar una
clasificación confiable para cualquier proyecto Cargo con el runtime actual.

## Decision

Se implementa inicialmente un modo de integridad acotado. Antes de ejecutar Miri,
metadata congelada y manifests capturados deben acreditar que el grafo no contiene
targets proc-macro/custom-build ni harnesses personalizados. De lo contrario se
devuelve `classification_integrity_unsupported`, sin afirmar clean, UB o una
operación no soportada por el intérprete. La selección incluye pruebas libtest
del workspace; excluye doctests, benches y examples. No evalúa un runner suministrado
por el proyecto ni configura flags desde el cliente.

El gateway usa nightly-2026-09-07 y sysroot preaprovisionado de ADR-066, con paths,
image digest y binarios exactos. No invoca setup. Ejecuta `cargo miri nextest run`
con nextest 0.9.143, config del producto read-only, sin retries ni fail-fast, una
hebra y JUnit acotado. MIRIFLAGS para build/list son exactamente
`--error-format=json -Zmiri-isolation-error=abort -Zmiri-backtrace=0`.
El run-wrapper fijo `/usr/bin/env`, `target-runner="within-wrapper"`, añade
únicamente `-Zmiri-mute-stdout-stderr` durante cada test. Así libtest puede listar
pruebas y los bytes interpretados no pueden falsificar diagnósticos de ejecución.
La función experimental wrapper-scripts queda ligada al pin de nextest.

La clasificación combina terminación del gateway, estructura JUnit, exit del
runner y diagnósticos JSON de Miri del testcase. No usa los resúmenes humanos.
Se conserva más de una categoría si varias pruebas producen fallos distintos:

- Clean exige exit global 0, selección no vacía, JUnit completo/coherente, todos
  los tests seleccionados passed y ninguno skipped/failed/error/omitido.
- UB exige failure con runner exit 1 y diagnóstico error cuyo mensaje comienza
  por `Undefined Behavior:` del runtime fijado.
- Unsupported exige la misma estructura y `unsupported operation:`.
- Test failure exige la terminación normal del harness 101, sin diagnóstico
  semántico de Miri ni evidencia discordante.
- Compile failure conserva fallos build/list y post-monomorphization acreditados;
  no convierte otro fallo de infraestructura en compilación por un texto parecido.
- Timeout/cancel/límite, suite vacía, skipped, XML/JSON inválido o resultado no
  reconocido quedan explícitos y nunca clean. Leaks/deadlocks/resource exhaustion
  desconocidos conservan `unclassified` con cobertura parcial.

Work budget Tasks 300 s por defecto, 1800 s máximo; captura/metadata/verificación
forman parte del mismo deadline. El gateway limita CPU/memoria/PIDs/disco/streams,
termina y une el árbol y verifica cleanup antes de liberar el permit. El informe
normalizado contiene categorías/conteos/identidades/runtime/hashes, sin raw prose
ni contenidos fuente; 128 findings y 512 KiB de resultado MCP completo. Se publica
en el store M3 con owner/TTL/retención. Los streams originales no son Resources.

### Cierre de revisión de integridad y presupuesto

La captura nativa ya rechaza `.cargo/config` y `.cargo/config.toml` en cualquier
directorio antes de devolver un `SourceBundle`; el gateway Miri repite esa
validación para callers internos. La configuración almacenada dentro del vendor
no participa en la jerarquía Cargo desde `/source`; se comprueban los productores
de sus paquetes efectivamente resueltos, sin vetar archivos de desarrollo de
paquetes ajenos al grafo. Rechaza además configuración nextest
local y archivos `rust-toolchain` del bundle para que ningún selector de proyecto
participe en la elección del intérprete. Se ejecuta antes de crear recursos Docker.
Los rechazos son `ClassificationIntegrityUnsupported`, sin degradación de flags.

El fingerprint de ejecución incluye los bytes del clasificador y del port Miri,
además del wrapper, admisión, gateway y runtime. Se reservan 10 s del presupuesto
del job para finalizar/revalidar/exportar JUnit (la limpieza conserva su deadline
separado existente). Una petición demasiado corta falla como timeout antes de
lanzar el intérprete. Las pruebas de timeout/cancel deben observar el contenedor
Miri ejecutándose; tiempo transcurrido por sí solo no demuestra esa fase.

El parser reconoce el timeout por test del runner mediante su tipo exacto
`test timeout` y conserva la categoría Timeout con `complete=false`. Los topes de
XML, nodos e identidades se distinguen como `OutputLimit` dentro del parser.
El envelope USTAR rechazado por el exportador compartido conserva
`InvalidMetadata`, incluido un envelope demasiado grande; ambos casos son
incompletos y nunca clean. Afinar esa distinción del exportador queda como P3
del Technical Owner, sin cambiar el contrato M3 en este corte. Los nombres se retienen
sin reescritura en un subconjunto conservador (letras/números Unicode y
`_ : $ . < > # -`, sin espacios); caracteres combinantes fuera de ese subconjunto
producen evidencia inválida, nunca un veredicto limpio.

El oráculo `binary-only` confirmó que `--lib --tests` devuelve evidencia inválida
cuando no hay librería. Se usa únicamente `--tests`, que incluye unit tests de
lib/bin e integración según [Cargo](https://doc.rust-lang.org/cargo/commands/cargo-test.html),
sin doctests, ejemplos ni benchmarks. El fixture mantiene un binario y un test de
integración para que el caso no pueda pasar por ausencia de tests.

## Alternatives considered

- Clasificar stderr del proyecto: falsificable incluso con mensajes JSON.
- Invocar directamente cargo-miri runner sobre CrateRunInfo JSON: convierte args,
  env, cwd y stdin no autenticados en autoridad; API interna no calificada.
- Admitir build scripts/proc macros y buscar patrones adicionales: no restaura
  integridad del productor.
- Declarar Miri simplemente unavailable: no implementa los positivos exigidos M4.

## Consequences

El modo inicial excluye proyectos habituales con derive/proc macros. Ampliarlo
requiere una nueva frontera y oráculos; no basta con más parsing. Nextest ejecuta
Miri por test y no observa carreras entre pruebas. Una ejecución clean acredita
solo las pruebas y configuración seleccionadas, nunca ausencia universal de UB.
La isolation de Miri no sustituye el sandbox OS del Execution Gateway.

El [oráculo empírico](../../fixtures/m4-runtime-oracles/miri-classification/README.md)
pasó clean/cfg, panic con diagnósticos falsos, UAF/uninit/alias/race, FFI, compile,
empty e ignored. Sus [recibos](../../fixtures/m4-runtime-oracles/miri-classification/results/summary.json)
no califican todavía la tool ni un gateway modificado. Los cambios del runtime,
nightly, sysroot, wrapper o parser exigen recalibración.

Fuentes oficiales fijadas: [Miri README](https://github.com/rust-lang/rust/blob/5a2be9f5f075d31e3ca5526b5b029881ce441253/src/tools/miri/README.md),
[diagnósticos](https://github.com/rust-lang/rust/blob/5a2be9f5f075d31e3ca5526b5b029881ce441253/src/tools/miri/src/diagnostics.rs),
[evaluación](https://github.com/rust-lang/rust/blob/5a2be9f5f075d31e3ca5526b5b029881ce441253/src/tools/miri/src/eval.rs),
[phases](https://github.com/rust-lang/rust/blob/5a2be9f5f075d31e3ca5526b5b029881ce441253/src/tools/miri/cargo-miri/src/phases.rs)
y [wrapper scripts](https://nexte.st/docs/configuration/wrapper-scripts/).

### Warning de optimización y panic ordinario

El control MCP con `[profile.dev] opt-level=1` mostró una diferencia real: el
nightly fijado emite dentro del JUnit un warning JSON propio indicando que Miri
ignora optimizaciones. El test termina con exit 101 y streams interpretados
silenciados. Rechazar cualquier JSON convertía este panic en Unclassified.
Se admite exclusivamente ese mensaje exacto del nightly, level warning, code
null y spans/children vacíos al clasificar el exit 101 como TestFailure. Cualquier
otro diagnóstico, discrepancia en esos campos, error o mezcla mantiene las reglas
conservadoras. El campo rendered y claves auxiliares se descartan; no participan
en la clasificación ni se publican.
El caso optimized-panic y mutaciones de su warning discriminan esta excepción.
No se modifica runtime, flags, imagen, perfiles ni ejecución del proyecto.
