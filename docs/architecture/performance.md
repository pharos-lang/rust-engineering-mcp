# Rendimiento: benchmarking, profiling y bloat

M5 añadió cuatro tools — `rust.benchmark.run`, `rust.benchmark.compare`,
`rust.profile.flamegraph` y `rust.binary.bloat` — con la regla explícita de
que el producto **nunca** afirma un claim de rendimiento universal; solo
entrega medición. La decisión más importante de este capítulo es que, hoy,
`rust.benchmark.compare` **no produce veredictos direccionales** —
solo mide — pese a que su ADR fundacional describe un método estadístico
completo para producirlos.

## Criterios estadísticos, leídos siempre juntos

Decisiones: ADR-073, ADR-081.

**Nunca leer ADR-073 sin ADR-081** — hacerlo induce a error sobre lo que la
tool realmente devuelve hoy.

**ADR-073 (el método, completo e implementado).** Criterion 0.8.2 es el
**único** harness reconocido; cualquier otro produce
`harness_unrecognized` sin generar dataset. El servidor congela warmup,
tiempo de medición, tamaño de muestra y repeticiones — el caller no puede
ajustarlos. El formato de dataset
`rust-engineering-mcp.benchmark-dataset.v2` (que añade `run_index` por
muestra respecto a v1) falla cerrado ante cualquier otro identificador o
versión. La comparación en sí es **cómputo puro** sobre bytes ya
autorizados — nunca ejecuta un proceso nuevo para comparar. Un gate de
compatibilidad exige que harness, versión, `rustc`, `cargo`, digest de
imagen, plataforma, selección, cuotas, modelo de CPU, fingerprint de
configuración y governor **coincidan en TODO** entre baseline y candidato,
o la comparación se rechaza enumerando cada discrepancia encontrada.
**Mínimo 3 `run_index` independientes por lado** antes de leer cualquier
intervalo (si no, `insufficient_executions`). Una dispersión de bootstrap
cero o casi-cero se trata como **información ausente**, nunca como
precisión infinita (`degenerate_dispersion`). Familias de más de 25
comparaciones desactivan bootstrap/intervalo por completo
(`family_beyond_resolution`). El governor de CPU se lee vía una sonda sysfs
cerrada y PID-safe — "todas las políticas deben coincidir" — y **solo en
el guest**, nunca en el host macOS.

**ADR-081 (aceptado al día siguiente): los veredictos direccionales son
estructuralmente inalcanzables en este entorno de medición.** El drift
entre corridas del propio host, medido entre 6.1% y 28.7%, excede
ampliamente lo que `k=3` repeticiones puede tolerar bajo un umbral material
del 5%. El código fuerza esta conclusión vía un guard explícito, leído al
final de `decide()` tras todos los demás gates de calidad de datos:

```rust
// crates/domain/src/benchmark_compare.rs:182
pub const METHOD_QUALIFIED_FOR_DIRECTION: bool = false;
```

**`rust.benchmark.compare` es, en el build actual, solo de medición — no
produce veredictos direccionales (`regression`/`improvement`/
`no_material_change`) ni los producirá hasta requalificación.** Esto es
central al capítulo, no una nota al pie.

**Criterios exactos para reabrir veredictos direccionales** (todos deben
cumplirse simultáneamente): coverage ≥ 0.93; falso-positivo-de-dirección
≤ 0.01; power ≥ 0.80, medido al **doble** del umbral material del 5% — no
al umbral mismo, corrección propia de ADR-081 tras un error matemático
detectado internamente durante su propia redacción;
no_material_change-incorrecto ≤ 0.05; presupuesto ≤ 30 s/≤ 512 MiB. El
umbral material del 5% **nunca se mueve** — relajarlo fue rechazado
explícitamente por el owner como forma de "arreglar" la cobertura
estadística. El conteo de outliers por Tukey-fence se mantiene sin
eliminación (se reporta, no se descarta).

**Alternativas rechazadas que siguen explicando el límite actual.** El
harness `libtest` no expone muestras crudas — matemáticamente imposible de
usar para este método. Un plugin externo `cargo-criterion` añadiría un
componente de runtime de terceros sin capacidad real adicional. Reusar el
`estimates.json` que el propio Criterion calcula dejaría el intervalo
definido por la versión del harness, que podría cambiar silenciosamente en
un upgrade futuro. Publicar la cobertura entregada sin arreglar el método
sería un paliativo que no resuelve el under-coverage real.

**Riesgo para revisores futuros.** Un refactor descuidado que quite o
invierta `METHOD_QUALIFIED_FOR_DIRECTION` defeaturizaría silenciosamente
toda la disciplina de requalificación descrita arriba — cualquier cambio a
esa constante exige revisar este capítulo y los criterios de reapertura
antes de aceptarse.

**Estado actual.** ADR-073 parcialmente vigente (el método sí, los
veredictos direccionales no); ADR-081 vigente y auto-consistente tras sus
propias correcciones — la conclusión está implementada en código, no solo
declarada en prosa. Evidencia: `crates/domain/src/benchmark_compare.rs`
(`InconclusiveReason`, `METHOD_QUALIFIED_FOR_DIRECTION`, función `decide()`
~1233-1325), `crates/execution-adapter/src/criterion_dataset.rs`; tests
inline en `benchmark_compare.rs` (fixtures `method_qualified: true/false`);
`crates/mcp-server/src/stdio/benchmark/tests.rs`;
`fixtures/benchmark-datasets/`; `qualification/benchmark-method-simulation.json`.

## Semántica de éxito de `rust.binary.bloat` (sustituye ADR-076 §6)

Decisiones: ADR-079.

**Decisión.** Antes de esta corrección, la semántica implícita del tool
hacía **estructuralmente imposible** que `rust.binary.bloat` devolviera
`passed` para cualquier binario que enlazara `std`: un positivo nativo
produjo 634 funciones frente al cap de 256 filas del producto
(`BLOAT_MAX_ROWS`), lo que siempre colapsaba la completitud a `Truncated`/
`blocked`/`EVIDENCE_INCOMPLETE`. La corrección separa tres conceptos que
estaban conflados en uno:

1. la **validez de la medición** decide `status`;
2. la **cobertura del ranking** (el cap `BLOAT_MAX_ROWS = 256`, propio del
   producto) se declara en la respuesta, pero **nunca decide** `status`;
3. el **recorte por presupuesto de respuesta** se declara, pero **nunca
   decide** `status`.

`passed` significa únicamente "el análisis corrió y este producto lo
validó" — **nunca** "el binario está optimizado" ni "el ranking es
exhaustivo". Un assert estático (`MAX_RESPONSE_ROWS > BLOAT_MAX_ROWS`)
garantiza en tiempo de compilación que un ranking acotado-pero-completo sea
siempre alcanzable dentro del presupuesto de respuesta.

**Alternativa rechazada.** Elevar `BLOAT_MAX_ROWS` fue considerado y
rechazado — no arregla la semántica de fondo, solo mueve el umbral: cualquier
binario más grande que el nuevo cap volvería a golpear el mismo problema.

**Estado actual.** Vigente, autoritativo sobre ADR-076 §6. Evidencia:
`crates/mcp-server/src/stdio/bloat.rs` función `outcome()` (ignora
deliberadamente ambos contadores de omisión); `crates/domain/src/bloat.rs`
(`BLOAT_MAX_ROWS`, el assert estático); tests
`crates/mcp-server/src/stdio/bloat/{tests,trim_tests}.rs`; snapshot
`binary-bloat-tool.json`.

## Contratos públicos M5

Decisiones: ADR-076.

**Decisión.** Añade las cuatro tools de este capítulo (inventario 27→31);
los 27 esquemas previos quedan congelados byte-a-byte. Las muestras crudas
de benchmark **nunca** viajan en la respuesta — solo como artifact, con un
cap de respuesta de 512 KiB. `release_lto` se logra vía `--release` más la
variable de entorno `CARGO_PROFILE_RELEASE_LTO=fat` — **no** vía
`--profile release-lto`, que se verificó explícitamente que falla. Hay un
rechazo explícito de una tool multiplexada genérica `rust.performance`
(el mismo patrón que ADR-083 rechazaría después para el analyzer, ver
[`analyzer.md`](analyzer.md)) — permisos y presupuestos distintos por
operación justifican tools separadas en vez de una sola tool con un
parámetro de "modo".

**Qué fue sustituido/completado por ADRs posteriores.** §6 (semántica de
resultado de `bloat`) queda sustituida por ADR-079 (arriba). §5 (logs del
harness) queda completada por ADR-080 (ver
[`jobs-and-artifacts.md`](jobs-and-artifacts.md#logs-del-harness-como-artifacts-privados)) —
el resto del contrato de M5 sigue vigente sin cambios.

**Estado actual.** Parcialmente vigente (por las dos sustituciones
anteriores). Evidencia:
`crates/mcp-server/src/stdio/{benchmark,benchmark_compare,bloat}.rs`;
snapshots `{benchmark-run,benchmark-compare,profile-flamegraph,
binary-bloat}-tool.json`.

## Capability de profiling y helper propio, con la corrección de §3.1 ya cerrada

Decisiones: ADR-074.

**Decisión.** En vez de provisionar `perf`/`flamegraph`/`samply`/`inferno`
de terceros, el producto construye un helper propio in-repo
(`rust-mcp-profile-helper`) que abre exactamente **un** evento
`perf_event_open` en modo usuario sobre su propio hijo lanzado
(`exclude_kernel=1`, `exclude_hv=1`, solo
`PERF_TYPE_SOFTWARE`/`PERF_COUNT_SW_CPU_CLOCK` — sin contadores de
hardware/PMU). El perfil seccomp es exactamente
`seccomp-rust-quality.json` **más una syscall** (sin `CAP_PERFMON`, sin
`--privileged`, sin cambiar `perf_event_paranoid`, sin `sudo`). La
capability se otorga **una vez**, al arrancar el servidor, por argv del
host — **sin revocación en caliente** durante la vida del proceso. La
salida SVG queda saneada **por construcción**: alfabeto cerrado de nombres
de frame, sin `<script>`, sin atributos `on*`, sin `href`, sin
`foreignObject`, sin URLs externas. El helper debe ser **PID 1 de su propio
namespace** y debe drenar (`kill(-1)`) todos sus descendientes antes de
emitir cualquier resultado — `namespace_drained: false` invalida la corrida
entera, nunca produce un éxito degradado. Ambos artifacts de salida se
abren con `O_EXCL`: un archivo pre-creado en esa ruta es una negativa
declarada, nunca una sobreescritura silenciosa.

**Alternativa rechazada especialmente relevante.** Un broker de profiling
privilegiado del lado del host se rechazó explícitamente por ser exactamente
el mismo patrón de broker privilegiado que D02/ADR-050 ya rechazó para
mutación (ver [`mutation.md`](mutation.md#local_coordinated-y-sus-límites-explícitos))
— el producto es consistente en evitar ese patrón en ambos dominios.

**§3.1 (corrección de paridad de configuración aplicada): cerrada, no
bloqueante.** El brief original de esta ADR y la revisión de checklist de
M5 marcaban §3.1 (verificación de paridad de campos namespace/PidMode/Init
entre la configuración solicitada y la aplicada realmente) como "decidido;
calificación pendiente" y potencialmente bloqueante del cierre completo de
M5. Esto quedó **corregido y re-revisado**: el commit `89ec1140` ("fix(m5):
reuse the complete applied-container security matrix") hizo que
`performance_gateway.rs` delegue en `rust_applied::verify_phase` — la misma
verificación de paridad que ya usaban las fases M1-M4 — en vez de una
verificación parcial propia de profiling. M5 quedó "Demonstrated" en sus
gates G2/G8 tras esta corrección. **No es una limitación vigente ni un
bloqueante de cierre de M5** — se documenta aquí como corregida para no
repetir la afirmación obsoleta del ADR original.

**Estado actual.** Vigente en su núcleo, con la corrección de §3.1 ya
integrada y calificada. Evidencia:
`crates/execution-adapter/src/{performance_gateway,performance_native,
performance_port,rust_applied}.rs`, `crates/application/src/profile.rs`,
`fixtures/profile-helper/`; test
`crates/mcp-server/src/stdio/profile/tests.rs`
(`an_ungranted_host_blocks_before_anything_is_dispatched`);
[`docs/validation/M5/03-runtime.json`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/validation/M5/03-runtime.json); commit `89ec1140cf599ff...`.

## Sin un subsistema de métricas estándar

**Limitación (spec §68/§93, sin ADR propia).** La especificación original
pedía dos capacidades transversales de rendimiento que el producto nunca
construyó como tales:

- **Contadores locales estándar** (§68): conteo de ejecuciones por tool,
  duración, tasa de cache-hit, fallos de proceso. No existe un subsistema
  de métricas/contadores en ejecución continua — en la práctica, esta
  necesidad se cubrió con **scripts de medición puntuales** (M8-05,
  `tests/baselines/performance-budgets.json`) que corren una vez por
  calificación, no con telemetría siempre activa del servidor. Esto es
  coherente con el resto del producto: no hay telemetría externa
  requerida ni recolectada (más allá de la que ONNX Runtime deshabilita
  explícitamente, ver [`catalog-and-search.md`](catalog-and-search.md)).
- **Un enum genérico de perfil de rendimiento** (§93): un parámetro
  agente-seleccionable `dev`/`ci`/`release`/`benchmark`/`size` que
  ajustara el comportamiento de medición. No existe ese enum en ninguna
  tool. El caso de uso concreto de "size" que la especificación citaba
  como ejemplo está cubierto por una tool dedicada,
  `rust.binary.bloat` (ADR-079, arriba) — el patrón del producto es
  siempre preferir una tool con nombre propio y presupuesto/permiso
  propio a un parámetro de "modo" genérico sobre una tool multiplexada
  (el mismo principio que ADR-076 aplica al rechazar `rust.performance`
  como tool única).

Ninguna de las dos ausencias bloquea ninguna tool existente; se documentan
aquí porque la especificación las pedía explícitamente y no deben
desaparecer silenciosamente al retirar
[`docs/spec/`](https://github.com/pharos-lang/rust-engineering-mcp/tree/51fa602e/docs/spec).
