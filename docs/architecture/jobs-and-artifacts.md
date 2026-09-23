# Jobs, Tasks y stores de artifacts

Los resultados grandes (logs de Cargo, reportes HTML de coverage, SVG de
flamegraph, JSON crudo de benchmark) nunca viajan inline en la respuesta de
una tool — se referencian por URI de artifact. Este capítulo cubre los dos
stores de artifacts del producto (el efímero de M1 y el durable owner-bound
de M3+), la ejecución de trabajo largo vía MCP Tasks, y la disciplina de
cuotas y cleanup que los atraviesa a todos.

## `ArtifactStore` efímero de M1 (mecanismo real, no el de ADR-014)

Decisiones: ADR-028.

**Decisión.** El store real que sirve las 13 tools M1 es **process-local y
solo memoria** — nunca filesystem — con recuperación que exige el
`ProjectRef` propietario más un ID aleatorio opaco de 128 bits
(`rust-artifact://prj_<hex>/art_<hex>`). Tiene caps de bytes de entrada y
salida, cuotas globales y por proyecto, TTL, redacción conservadora por
patrón literal (sobre-redactar se prefiere siempre a filtrar de menos), y
un reloj monótono inyectado — una regresión de ese reloj **envenena la
instancia permanentemente**, en vez de arriesgar TTLs incorrectos. Las
cifras concretas: **256 KiB por artifact, retención de 1 hora, se pierde al
reiniciar el servidor**.

**Por qué "el mecanismo real, no el de ADR-014".** ADR-014 (histórica)
describía un `ArtifactStore` con streaming cap, **directorio privado en
filesystem**, IDs aleatorios, TTL y cuotas — pero el propio ADR-014
reconoce que ese mecanismo de "directorio privado" **nunca se implementó
como tal**: "no se declara implementado ni verificado" son sus propias
palabras. El mecanismo real para M0/M1 es este, ADR-028, enteramente en
memoria. No describir un "directorio privado" como parte del contrato M1
vigente en ninguna documentación nueva.

**Estado actual.** Vigente; confirmado byte-a-byte sin cambios por ADR-061
(M3), que añade un store durable **separado** solo para artifacts de
quality jobs — no reemplaza este. Evidencia:
`crates/artifact-adapter/src/lib.rs`; tests
`crates/artifact-adapter/src/tests.rs` (19 tests).

## Cuotas por store (transversal)

Cada store del sistema declara su propio conjunto de cuotas explícitas,
todas **fail-closed**: rechazar antes de producir, nunca degradar
silenciosamente ni desalojar evidencia ya prometida a un caller.

| Store | Cuotas |
| --- | --- |
| `ArtifactStore` M1 (ADR-028) | 256 KiB/artifact, 16 MiB global, 1 MiB/owner, TTL 3600 s |
| Journal de mutación M2 (ADR-052, ver [`mutation.md`](mutation.md)) | 128 journals/256 MiB por store, 48 MiB/entrada |
| Store de artifacts de quality jobs M3+ (ADR-061) | Cuotas por-tipo, reject-before-produce, sin eviction — ver abajo |

## Ejecución de jobs acotada y MCP Tasks negociadas

Decisiones: ADR-060.

**Decisión.** Se introduce un dominio `JobId`/`JobKind`/`JobOwner`/
`JobState`/`JobPhase` **sin ninguna dependencia** de `rmcp`, JSON-RPC,
Tokio, Cargo, SQLite ni LanceDB — pura lógica de dominio. `JobExecutor`
**reutiliza el permit único de worker de ADR-030** (ver
[`mcp-and-contracts.md`](mcp-and-contracts.md#admisión-de-workers-cancelación-y-transporte))
como permit de job — **no** un segundo semáforo — retenido durante todo el
cleanup y liberado **solo tras que ese cleanup se observe
positivamente**: la incertidumbre de cleanup pone la sesión entera en
cuarentena en vez de liberar capacidad de forma falsa.

El código de producto **nunca envuelve `rmcp::TaskManager` directamente**:
un registro y watchdog propio, owner-bound, se sienta encima de la
extensión Tasks de `rmcp` solo para negociación y wire del protocolo. Un ID
de tarea desconocido, malformado, expirado, de otro `ProjectRef`, de otro
grant o inválido por política produce siempre el mismo error enmascarado
`-32602 "task unavailable"`, **sin eco del ID** que se envió (para no dar
pistas a un cliente que esté sondeando IDs ajenos). Solo hay **un job activo
por sesión stdio, sin cola** de jobs pendientes.

**Alternativas rechazadas que siguen explicando el límite actual.** Envolver
`rmcp::TaskManager` directamente habría heredado un lookup de ID que no es
owner-bound, un TTL potencialmente ilimitado, y ninguna forma de forzar
revalidación de `ProjectRef`/política ni de ligarse a la prueba de join real
del gateway. Un mapa de autoridad paralelo introduciría riesgo de
split-brain entre dos fuentes de verdad. Ejecución puramente síncrona para
trabajo largo tendría peor comportamiento de cancelación y de gestión de
slot. Persistir jobs a través de un restart del servidor no puede
reconstruir con seguridad la contención de código potencialmente hostil que
estaba en curso.

**Cancelación y cleanup son una regla dura, no una optimización**: terminar
y unir el árbol de procesos completo antes de reutilizar capacidad de
worker es obligatorio en todos los casos.

**Estado actual.** Vigente; el ADR mejor respaldado por evidencia
discriminante del corpus (15 tests D06-T01..T15). Evidencia:
`crates/domain/src/job.rs`, `crates/application/src/job.rs`,
`crates/mcp-server/src/stdio/{tasks,workers,admission}.rs`; tests
`crates/mcp-server/src/stdio/tasks/tests.rs`,
`tests/{rmcp_tasks_spike,inspection_runtime/tasks,protocol}.rs`;
[`docs/validation/M3/02*.json`](https://github.com/pharos-lang/rust-engineering-mcp/tree/51fa602e/docs/validation/M3).

## Store privado de artifacts de quality jobs

Decisiones: ADR-061, ADR-062.

**Decisión.** Un store durable **owner-bound** (calificado solo en macOS
ARM64/APFS en Stage 1; Stage 0 hace fallback sin cambios al store en
memoria de ADR-028 en otras plataformas) hermano del store de mutación, con
un esquema de descriptor v1 estricto. `owner_binding = uid + state-root +
granted-root` — **no** es aislamiento por sesión ni por peer: una sesión
posterior con el mismo uid y un grant vivo puede releer la evidencia
producida por otra sesión. Las cuotas son **por-tipo, reject-before-produce
y sin eviction** — nunca se descarta evidencia ya prometida a un caller
para hacer espacio. El commit del descriptor es atómico vía sync+rename; la
reconciliación solo confía en pares descriptor/blob **estrictamente
validados**; si no lo son, van a **cuarentena**, nunca se interpretan con un
valor por defecto.

**Limitación (debe quedar explícita, no implícita).** La ausencia de un
componente secreto en `owner_binding` (solo uid + state-root + granted-root,
sin ningún token o secreto asociado) es una garantía **deliberadamente más
débil** que un binding basado en secreto — cualquier proceso con el mismo
uid y un grant vivo sobre la misma root puede leer la evidencia. Esta
limitación debe estar explícita en la documentación pública de seguridad
del producto, no quedar implícita solo en este capítulo.

**Estado actual.** Vigente, extendido conjuntamente por ADR-062 (coverage/
semver, ver [`analyzer.md`](analyzer.md)). Evidencia:
`crates/domain/src/quality_artifact.rs` (incl. `ArchiveBundle`),
`crates/application/src/quality_artifact.rs`; tests
`crates/project-adapter/tests/quality_artifact_store.rs`,
`crates/domain/tests/quality_artifact.rs`.

## Atestación real de cleanup, no solo el retorno del worker

Decisiones: ADR-070.

**Decisión.** La atestación de cleanup debe ser evidencia **real** de que
el objeto de runtime (contenedor, volumen, proceso) desapareció — **no**
simplemente que la función del worker retornó sin error. Si el inspector de
cleanup comparte reporta cuarentena, el compositor **no** debe marcar el
cleanup como observado ni publicar el resultado de la tool al cliente. Esto
corrige un defecto real: antes de esta ADR, el compositor de Tasks marcaba
"observado" solo porque el worker había retornado, incluso cuando la job ya
estaba en cuarentena por incertidumbre de cleanup. La corrección beneficia a
las más de cinco invocaciones que comparten el mismo compositor: `rust.deny`,
`rust.quality.gate.v2`, el scanner de `unsafe`, Miri y el audit compuesto.

**Verificación contra evidencia real.**
[`docs/validation/M4/full-gate.json`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/validation/M4/full-gate.json) (`mode: full`, corrida
2026-09-08T16:47–17:41Z en macOS 26 ARM64) reporta `status: passed` en sus
33 pasos, incluyendo `m4-runtime` y `m4-tampered-plugin` — los pasos que
ejercitan la composición de cleanup/Tasks que esta ADR corrige. El propio
ADR-070 marca su evidencia inicial como parcial (1/1, 90.08 s); el gate
completo de M4 confirma que la corrección quedó calificada, no solo
propuesta.

**Estado actual.** Vigente; evidencia
`crates/application/src/job.rs` (`CleanupObservation`),
`crates/mcp-server/src/stdio/workers.rs`; test
`tests/inspection_runtime/tasks.rs`
(`tasks_eof_joins_hostile_child_and_uncertain_cleanup_fails_session`).

## Logs del harness como artifacts privados

Decisiones: ADR-080.

**Decisión.** El stdout/stderr del harness de benchmark (ver
[`performance.md`](performance.md)) se publica como artifacts privados
owner-bound con TTL, usando el store de ADR-061, identificados
**por-repetición** vía `run_index` — nunca concatenados en un solo blob.
Esto implementa lo que ADR-076 §5 prometía sin cumplir: antes de esta ADR,
tres textos publicados afirmaban que los logs "se quedaban en el artifact
de criterion", lo cual era falso — el adapter descartaba stdout/stderr por
completo.

**Limitación autodeclarada, deliberadamente no corregida aquí.** La cuota
del store se reserva **después** de ejecutar el harness, no antes. Un owner
con la cuota ya agotada paga el costo completo de compilación y ejecución
del benchmark antes de descubrir que pierde la evidencia por falta de
cupo. Esta brecha es compartida con las rutas de publicación de M3/M4 y se
deja explícitamente sin corregir en el alcance de esta ADR.

**Estado actual.** Vigente, con la brecha anterior sin corregir. Evidencia:
`crates/mcp-server/src/stdio/benchmark.rs`,
`stdio/quality_artifacts/performance.rs`,
`crates/execution-adapter/src/performance_native.rs`;
[`docs/validation/M5/{01-capture-runtime,clients}.json`](https://github.com/pharos-lang/rust-engineering-mcp/tree/51fa602e/docs/validation/M5).

## Resources y artifacts grandes: el patrón que atraviesa el producto

Ningún artifact grande (coverage HTML, SVG de flamegraph, JSON de
benchmark, logs extensos, SBOM, grafo de dependencias) se inlinea nunca en
la respuesta de una tool; todos se referencian por URI de artifact
(`rust-artifact://` o `rust-quality-artifact://`), consistentemente desde
`rust.check`/`.fmt.check` (M1, ver
[`execution-and-security.md`](execution-and-security.md)) hasta
`rust.coverage`/`.mutation.test`/`.semver.check` (M3),
`rust.benchmark.*`/`.profile.flamegraph` (M5) y las tools del analyzer
(M6). Los Resources del protocolo MCP (ADR-011, ver
[`mcp-and-contracts.md`](mcp-and-contracts.md#resources-para-contexto-ya-computado))
son el mecanismo de lectura de esos artifacts — nunca de ejecución.
