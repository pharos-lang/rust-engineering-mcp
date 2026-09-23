# Dominio y aplicación

`crates/domain` y `crates/application` son la mitad hexagonal interior (ver
[`overview.md`](overview.md#arquitectura-hexagonal-y-fronteras-de-crate)):
sin conocimiento de `rmcp`, JSON-RPC, stdio, procesos Cargo, SQLite ni
LanceDB. Este capítulo cubre los tipos fundacionales, la obligación de
provenance/freshness y las dos primeras tools M1 que ya recorren el patrón
completo de worker admitido reutilizado por milestones posteriores.

## Tipos e invariantes del dominio base

Decisiones: ADR-022.

**Decisión.** `rust-engineering-domain` depende únicamente de Serde (más un
parser semver puro para rangos de policy de seguridad, ver ADR-067;
`serde_json` es dependencia de test, no de producción). Los tipos
fundacionales, con campos privados que rechazan valores mal formados en el
borde de deserialización:

| Tipo | Garantía |
| --- | --- |
| `ProjectRef` | `prj_` + 32 hex minúsculos; es sintaxis, no autoridad ni prueba de entropía por sí sola. |
| `ProjectIdentityFingerprint` / `ExecutionFingerprint` | Tipos incompatibles entre sí (no intercambiables, verificado por doctest compile-fail); digest `sha256:` + 64 hex; nunca se calculan preimágenes desde el fingerprint. |
| `NonEmptyText` | Conserva Unicode/texto original; rechaza vacío o solo whitespace. |
| `SourceSpan` / `ByteRange` | Coordenadas no invertidas; extremos exclusivos; posiciones de escalar Unicode 1-based en el span reportado al agente, rangos de bytes 0-based exclusivos internamente; inserciones vacías permitidas. |
| `Suggestion` | Una o varias ediciones; un reemplazo vacío significa eliminación. |
| `OutputEnvelope<T>` | status/summary/duration_ms/error_code/error_message/diagnostics/truncation/data/evidence; **nunca `serde_json::Value`** como modelo interno. |
| `SnapshotEvidence` | Provenance y freshness inseparables y coherentes con el `Clock`/policy declarados en la evaluación. |

Los paths que aparecen en un span son evidencia textual, no autorización ni
verificación de existencia; bytes y posiciones se validan de forma
independiente, sin leer el source de nuevo. Los campos opcionales de un
diagnóstico pueden omitirse, pero `error_code`/`error_message`,
`created_at`/`observed_at` y `age_seconds` exigen presencia aunque su valor
sea `null` — el vocabulario de status/error_code es cerrado, no un string
libre.

`Report<T>`/`OutputEnvelope<T>` fijan la tabla de estados:

| Caso | `status` | `error_code` / `error_message` |
| --- | --- | --- |
| Validación correcta | `passed` | null / null |
| Fallo del proyecto (p. ej. E0502) | `failed` | null / null |
| Tool ausente o plataforma no soportada | `unavailable` | código / mensaje |
| Otro error operativo (incl. timeout, `SANDBOX_DENIED`) | `blocked` | código / mensaje |
| Cancelación | `cancelled` | null / null; sigue siendo operacional |

Un `failed` con ambos campos de error nulos no es un error operativo
(`is_operational_error()` es falso) — el adapter MCP decide después cómo
transportarlo hacia el protocolo (ver
[`mcp-and-contracts.md`](mcp-and-contracts.md#álgebra-de-resultados-y-json-schema-por-tool)).
`Truncation` conserva flags de stream y cantidad de diagnósticos omitidos sin
cambiar por sí sola el resultado de la ejecución.

**Contexto.** Estos tipos son la base de los 36 contratos de tool públicos;
ninguna ADR posterior los contradice, solo los extiende (paginación,
supply-chain, analyzer).

**Alternativas rechazadas que siguen explicando el límite actual.** Un
booleano de éxito habría perdido la distinción `failed`/`blocked`/
`unavailable`/`cancelled` que un agente necesita para decidir su siguiente
paso; `serde_json::Value` como modelo interno habría empujado errores de
esquema a runtime y está expresamente prohibido por
`scripts/check-architecture.py`.

**Estado actual.** Implementado; `cargo test -p rust-engineering-domain
--locked --offline` y su doctest compile-fail (que detecta el intercambio de
tipos de fingerprint) son la evidencia reproducible del contrato de dominio.

## Provenance y freshness obligatorios

Decisiones: ADR-020.

**Decisión (invariante fundacional, transversal a todo el catálogo).** Todo
output derivado de catálogo, advisory, modelo o artifact lleva `Provenance`
(tipo de fuente, ID de snapshot o modelo, timestamps, estado de integridad,
`network_used`) y `Freshness` (`live`/`fresh`/`aging`/`stale`/`unknown`, edad,
umbral/política) — **ninguno de los dos campos es opcional** cuando el
resultado depende de un snapshot del ecosistema. La terminología obligatoria
es **`latest_known`, nunca `latest`**, salvo evidencia `live` explícita;
`latest_live` está reservado a operaciones CLI explícitas fuera del runtime
MCP en M1+ — el runtime MCP nunca lo produce. `Clock` es un puerto
inyectable; un gate de calidad puede fallar ante violación de política de
freshness; una búsqueda puede devolver datos obsoletos, siempre con
advertencia visible en el propio resultado.

La edad se calcula desde `created_at`, nunca desde la fecha de importación al
store local. Los límites `fresh`/`aging` son inclusivos; una fecha ausente o
futura produce `unknown`. Que la fuente haya usado red al crearse no
convierte una consulta posterior sobre ese snapshot en `live`. Integridad y
freshness son dimensiones independientes: un snapshot puede ser fresh con
integridad no verificada. La deserialización de una `SnapshotEvidence`
comprueba coherencia con el `assessed_at`/policy persistidos — no autentica
los datos ni los vuelve actuales; cada tool reevalúa antes de una decisión
real y aplica su propia policy (por ejemplo, `rust.dependencies.audit`).

Tipos exactos: `Clock`, `Provenance`, `FreshnessState` en
`crates/domain/src/evidence.rs`; `SnapshotEvidence::assess` recibe
provenance, una policy validada y un `Clock` — nunca consulta red,
filesystem ni el reloj real por sí mismo.

**Contexto.** El servidor opera con datos de catálogo/modelo que envejecen
sin que el proceso lo sepa (import administrativo mientras el servidor
corre, ver [`catalog-and-search.md`](catalog-and-search.md)); presentar un
snapshot como si fuera en vivo induciría al agente a decisiones erróneas
sobre disponibilidad de versiones o advisories.

**Alternativas rechazadas que siguen explicando el límite actual.**
Timestamps sueltos sin clasificación forzarían a cada consumidor a
interpretar la edad ad hoc; freshness opcional permitiría omitirla justo
cuando más importa (un snapshot viejo sin marcar).

**Estado actual.** Implementado; invariante pervasivo verificado en
`crates/domain/tests/freshness.rs` y en casi todos los fixtures de
`crates/mcp-server/tests/snapshots/*.json`. Aplica sin excepción a
`rust.crate.search`/`rust.crate.inspect`/`rust.catalog.status` (ver
[`catalog-and-search.md`](catalog-and-search.md)) y a
`rust.dependencies.audit` (ver
[`execution-and-security.md`](execution-and-security.md)).

## Sin LLM interno en el core

Decisiones: ADR-005.

**Decisión.** Ninguna inferencia generativa (OpenAI, Anthropic, Gemini u
otro proveedor) vive dentro del core ni de M1; el producto solo devuelve
evidencia/operaciones deterministas. Los embeddings locales (ADR-019/027,
ver [`catalog-and-search.md`](catalog-and-search.md)) son una capacidad de
recuperación acotada — vectores para ranking, nunca texto generado — y no
cuentan como LLM.

**Contexto.** El consumidor típico del servidor ya es un agente con su
propio LLM; duplicar razonamiento generativo dentro del servidor añadiría
red, credenciales, costo y pérdida de reproducibilidad sin necesidad.

**Alternativas rechazadas que siguen explicando el límite actual.** Un LLM
remoto para producir explicaciones rompería el principio offline-first y la
privacidad del código analizado; un modelo generativo local añadiría peso de
distribución y de recursos sin que el producto lo necesite para su función
(evidencia, no generación).

**Consecuencias.** No hay dependencia de SDK de LLM en el árbol de
dependencias del workspace actual (verificable en `Cargo.toml`); el stack de
recuperación usa `fastembed`/`ort`/`lancedb`, todo confinado a
`semantic-adapter`.

**Estado actual.** Vigente, sin contradicción.

**Riesgo residual.** No existe un enforcement automatizado (gate de CI) que
impida agregar una dependencia de LLM generativo en el futuro; la garantía
depende de la disciplina de revisión (`AGENTS.md`), no de un chequeo
mecánico como el de `scripts/check-architecture.py` para la hexagonalidad.

## Inspección de proyecto y de toolchain

Decisiones: ADR-032, ADR-033.

**Decisión.** `rust.project.inspect` y `rust.toolchain.inspect` son las dos
primeras tools que comparten el worker único admitido de ADR-030 (ver
[`mcp-and-contracts.md`](mcp-and-contracts.md#admisión-de-workers-cancelación-y-transporte)):
la misma composición posee captura de source, inicialización, calibración,
parseo y revalidación. La calibración es perezosa — ocurre tras que el
protocolo esté listo, nunca bloquea el bootstrap del servidor — y un fallo
de calibración (distinto de una cancelación) se "latchea" para el resto de
la sesión, evitando repetir fixtures de contención costosas en cada llamada
posterior. El execution adapter parsea el JSON externo de Cargo de forma
acotada y rechaza datos desconocidos o inconsistentes en vez de aceptarlos
silenciosamente. `ProjectSnapshot` lleva provenance/freshness/`latest_known`
conforme a ADR-020 cuando corresponde.

`rust.toolchain.inspect` deriva la lista de targets *instalados* únicamente
de entradas `rust-std-<triple>` observadas en el propio host — nunca de
`rustc --print target-list`, que enumera targets *soportados* por el
compilador, no instalados en la máquina. Confundir ambos habría hecho que el
agente intentara compilar para un target que el toolchain local no puede
producir.

**Contexto.** Ambas tools son la base del contexto que un agente necesita
antes de generar o corregir código: edición, MSRV, toolchain, componentes
instalados, features y policies relevantes del workspace.

**Consecuencias.** ADR-030 confirma explícitamente que ADR-032 implementa su
composición abstracta de admisión de workers — este es el patrón que
reutilizan luego `rust.check`/`.fmt.check`/`.clippy`/`.test` y, más adelante,
jobs de M3+ (ver [`mcp-and-contracts.md`](mcp-and-contracts.md) y
[`jobs-and-artifacts.md`](jobs-and-artifacts.md)).

**Estado actual.** Implementado; ambas forman parte de las 13 tools M1
congeladas (ver [`mcp-and-contracts.md`](mcp-and-contracts.md#contratos-congelados-m1)).
Evidencia: `crates/mcp-server/src/stdio/{inspection,toolchain}.rs`,
`crates/execution-adapter/src/project_inspection.rs`; tests en
`crates/mcp-server/tests/inspection_runtime.rs` y snapshots
`project-inspect-tool.json`/`toolchain-inspect-tool.json`.

## Datos de proyecto frente a datos de catálogo, y separación de errores

Los datos de un proyecto concreto (manifiestos, lockfile, metadata de Cargo,
salida de `rustc`) vienen siempre de `project-adapter`/`execution-adapter`
sobre el propio checkout — nunca del catálogo global (`catalog-adapter`),
que vive en un store SQLite separado (ver
[`catalog-and-search.md`](catalog-and-search.md)). El dominio no conoce
`serde_json::Value` como tipo interno; internamente se usan enums de error
cerrados hechos a mano en vez de `thiserror`/`anyhow` (ver
[`overview.md`](overview.md#lenguaje-interno-tipos-y-dependencias-no-adoptadas)),
siguiendo la misma regla "evitar `unwrap`/`expect`/`panic!` en rutas
normales" que fija `AGENTS.md`.

`rust.dependencies.inspect` — un caso de uso interno con acceso profundo a
dependencias — nunca se expuso como tool pública M1; alimenta
internamente inspección/auditoría/catálogo. Convertirlo en tool pública
exigiría un cambio explícito de alcance con su propia ADR, no una extensión
silenciosa de `rust.project.inspect`.
