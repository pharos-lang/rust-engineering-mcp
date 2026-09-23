# Catálogo y búsqueda

El catálogo de crates es parte del MVP, no un añadido posterior:
`rust.catalog.status`, `rust.crate.search` y `rust.crate.inspect` están
entre las 13 tools M1 congeladas (ver
[`mcp-and-contracts.md`](mcp-and-contracts.md#contratos-congelados-m1)).
Este capítulo cubre la separación entre store autoritativo y store
derivado, la firma y activación durable de snapshots, los embeddings
locales, los modos de búsqueda y las tools de lectura del catálogo. La
sincronización y el mantenimiento operativo (CLI `catalog sync`/`import`/
`rebuild-index`) tienen su propio documento en
[`operations/catalog-maintenance.md`](../operations/catalog-maintenance.md);
aquí solo se cubre el invariante de que el runtime **nunca** sincroniza.

## SQLite autoritativo, LanceDB derivado

Decisiones: ADR-016, ADR-017.

**Decisión.** SQLite (`rusqlite`, bundled) es el **único** store
autoritativo del catálogo, con migraciones monótonas y transaccionales
(`PRAGMA user_version`). Un import se construye en staging, se verifica y
se activa atómicamente; la base de datos activa se abre **solo lectura** en
runtime. LanceDB solo suministra IDs y scores de candidatos detrás del
puerto `SemanticIndex` — **SQLite rehidrata y filtra todos los hechos
finales** (MSRV, licencia, yanked, advisories); LanceDB nunca decide un
hecho por sí mismo. Cada generación de índice lleva su propio fingerprint
de snapshot, versión de esquema/índice e identidad de modelo/embedding; una
discrepancia o corrupción del índice dispara un **fallback léxico
declarado**, nunca un fallo silencioso ni una mezcla de generaciones
distintas.

**Alternativas rechazadas que siguen explicando el límite actual.** LanceDB
como fuente de verdad mezclaría hechos autoritativos con un índice pensado
para ser reconstruible; un servicio SQL externo rompería la operación
local/offline que el resto del producto exige.

**Estado actual.** Vigente; evidencia `crates/catalog-adapter/src/lib.rs`
(`SqliteCatalogRepository`), `src/{audit,records,search,inspect,
bundle}.rs`; `crates/semantic-adapter/src/index.rs`
(`snapshot_fingerprint`), `index/persistence.rs`; tests
`crates/catalog-adapter/src/tests.rs`,
`tests/{catalog,hybrid,crate_inspect,crate_search}.rs`.

## Bundles firmados y activación durable (mecanismo real de ADR-018)

Decisiones: ADR-041.

**Decisión.** El runtime MCP **nunca** sincroniza ni descarga catálogos —
esa es responsabilidad exclusiva de la CLI explícita (ver
[`operations/catalog-maintenance.md`](../operations/catalog-maintenance.md)
para ADR-018, que fija esta separación pero deja el mecanismo de firma
abierto). ADR-041 es ese mecanismo concreto: un trust root Ed25519
seleccionado por el host (`ring 0.17.14`), **sin confianza compilada ni
TOFU** (trust-on-first-use); el transporte es USTAR/zstd acotado con
verificación criptográfica **antes** de parsear cualquier payload; el orden
es **reserve-before-activate** (`floor.record` puede ir por delante del
activo, pero nunca por detrás) bajo un lock de sistema operativo exclusivo;
el reemplazo de generación es atómico y durable. La sincronización HTTPS
solo puede iniciarse desde la CLI (**nunca es invocable desde una tool de
runtime**), con una allowlist exacta de hostname, sin redirects, sin proxy y
sin credenciales, sujeta a las mismas comprobaciones de firma y rollback
que un import local.

**Precisión obligatoria sobre rollback (no hay excepción administrativa).**
Un snapshot con una secuencia anterior a la ya activada **siempre** se
rechaza — **no existe ningún flag administrativo que permita instalar
manualmente un snapshot más antiguo**. ADR-041 supersede aquí a ADR-018,
que en su redacción original sí dejaba abierta la posibilidad de un
"admin override"; el mecanismo implementado nunca lo construyó y la
recuperación **nunca hace fallback implícito** a un bundle anterior. La
única vía para revertir un catálogo es reconstruir un import completo desde
cero con una secuencia igual o mayor.

**Estado actual.** Mecanismo íntegramente implementado y testeado, pero
**sin trust anchor real de producción** — todas las claves actuales son
fixtures. Publicar un catálogo oficial sigue bloqueado por política de
trust-root, no por el mecanismo de import en sí; el propio ADR-041 declara
explícitamente que "no autoriza un publisher, licencia o release oficial".
Evidencia: `crates/catalog-adapter/src/{bundle,bundle/floor}.rs`,
`crates/mcp-server/src/catalog_sync.rs`; tests
`crates/catalog-adapter/src/bundle/tests.rs`, `catalog_sync/tests.rs`.

## Snapshots acotados en memoria, ahora con activación durable

Decisiones: ADR-026.

**Decisión histórica.** `rusqlite 0.40.2`/SQLite 3.53.2; el esquema más
FTS5 se construye en staging en memoria, acotado; el export es una imagen
de base de datos más un manifiesto versionado; el import valida tamaño y
hash antes de deserializar cualquier contenido; el runtime abre la base de
datos en modo `READONLY`; la activación reemplaza el repositorio completo
tras comprobar que la nueva secuencia es estrictamente mayor.

**Qué sigue vigente y qué fue sustituido operacionalmente.** La mecánica de
validación y las cuotas — 64 MiB de imagen, 1000 crates, 64 versiones por
crate, 100k entradas combinadas de version/feature/dependency/advisory —
siguen siendo autoritativas, confirmadas por ADR-041. Pero el encuadre
original "solo memoria, no durable, process-local" queda **sustituido
operacionalmente** por la activación durable firmada de ADR-041: no
describir el catálogo M1+ como puramente efímero.

## Embeddings locales reproducibles

Decisiones: ADR-019, ADR-027.

**Decisión.** `EmbeddingProvider`/`LocalEmbeddingProvider` con `fastembed
6.0.2` + `intfloat/multilingual-e5-small` (licencia MIT), ejecutado sobre
ONNX Runtime en CPU. La descarga remota está deshabilitada; el modelo se
carga **solo** desde un manifiesto validado con hashes y tamaños fijados en
código (`crates/semantic-adapter/src/model.rs`). Las consultas usan
prefijos `query:`/`passage:` según el rol E5 estándar. Ausencia o
incompatibilidad del componente semántico degrada a búsqueda léxica con
provenance explícita — nunca un fallo silencioso.

`lancedb 0.31.0` está vendorizado (parche solo de manifest; `lance-testing`
se movió a dev-dependencies para sacar el profiler y la dependencia
`quick-xml` vulnerable del grafo de runtime real). El índice vive en
`memory://`, fresco por cada generación, **sin spill a filesystem**.
`paste 1.0.15` (RUSTSEC-2024-0436) es una advertencia transitiva
**aceptada y documentada**, no silenciada — sigue apareciendo en
`Cargo.lock` como advertencia visible.

**Estado actual.** Vigente; el gate de benchmark ES/EN que condicionaba la
aceptación (evidencia M1-16) está satisfecho. Evidencia:
`crates/semantic-adapter/src/{index,embedding}.rs`; `Cargo.toml`
(`lancedb = "=0.31.0"`, vendor path); tests
`crates/semantic-adapter/tests/local.rs`,
`crates/catalog-adapter/tests/hybrid.rs`.

## Auditoría RustSec propia y offline

Decisiones: ADR-038.

**Decisión.** El producto pinnea `rustsec 0.32.0` **sin** features de red,
git ni binario. Usa `Advisory::from_str` más el matcher oficial de la
librería sobre registros seleccionados de la SQLite en memoria acotada —
**nunca `Database::open`**, que evitaría el I/O no-follow por handle
(ver [`execution-and-security.md`](execution-and-security.md)). El host
suministra explícitamente el snapshot y su SHA-256 esperado. La política de
freshness es `fresh ≤ 24h`, `aging ≤ 7d`, luego `stale`. Solo las fuentes
crates.io explícitas se emparejan con advisories — dependencias de path o
Git nunca se tratan silenciosamente como si fueran de crates.io. La
reconstrucción del path del lock v4 está acotada por BFS (≤8 roots, ≤32
paquetes por path) para evitar explosión combinatoria en workspaces
grandes.

**Estado actual.** Vigente; citado explícitamente por ADR-009 como su
refinamiento M1-07 (aditivo, no contradictorio). Evidencia:
`crates/catalog-adapter/src/audit.rs`, `audit/lock.rs`; tests
`crates/catalog-adapter/src/audit/tests.rs`,
`tests/inspection_runtime/audit.rs`.

## Estado del catálogo en runtime

Decisiones: ADR-042.

**Decisión.** `rust.catalog.status` usa lectores read-only no-follow
**protegidos** — nunca `CatalogStore::open`, que mantendría acoplada la
lease administrativa con el path de lectura del runtime. La generación
observada es **inmutable por sesión**: un import o rebuild administrativo
mientras el servidor sigue corriendo permanece invisible hasta el próximo
reinicio, por diseño, no por un descuido. El campo
`acquisition_allowed=false, enforcement=runtime_api_disabled` describe
**ausencia de autoridad de red en el runtime MCP**, no una afirmación de
que el sistema operativo aísla la red para todo el proceso — esa garantía
de aislamiento de red real vive en el sandbox de ejecución (ver
[`execution-and-security.md`](execution-and-security.md)), no en esta tool.

**Estado actual.** Vigente; extiende explícitamente ADR-041 ("todos los
límites de ACL/plataforma continúan aplicando"). Evidencia:
`crates/mcp-server/src/stdio/catalog.rs`; tests
`tests/catalog_status.rs`, `tests/inspection_runtime/security_catalog.rs`.

## Modos de búsqueda híbrida

Decisiones: ADR-043.

**Decisión.** `rust.crate.search` tiene un input cerrado: query, `mode`
(default `hybrid`), `limit`, filtros `msrv_lte`/`allow_yanked`/
`include_prerelease`. El modo léxico usa SQLite FTS5 con `bm25`
(**menor es mejor**, explícito para no confundir al agente con la
convención inversa de otros rankers). El modo semántico usa E5 verificado
más distancia L2 de LanceDB (**menor distancia primero**). El modo hybrid
combina ambos con **RRF determinista**: `sum(1 / (60 + rank))` — **nunca
mezcla scores crudos** de fuentes con escalas distintas. La ausencia o
invalidez del componente semántico degrada a fallback léxico explícito.

**Limitación empírica (no de implementación).** El experimento posterior
M1-16 encontró un "techo 12/12 en ambos brazos, sin equivalencia ni
causalidad demostrada" entre usar el catálogo híbrido/semántico y no
usarlo. El mecanismo está correctamente implementado y testeado, pero su
utilidad real frente al baseline léxico simple sigue siendo una **pregunta
empírica abierta**. No afirmar en ninguna documentación que el modo
híbrido o semántico mejora resultados de un agente en la práctica — solo
que está implementado y disponible.

**Estado actual.** Vigente; evidencia
`crates/mcp-server/src/stdio/crate_search.rs`,
`crates/catalog-adapter/tests/{hybrid,crate_search}.rs`; snapshot
`crate-search-tool.json`.

## Inspección paginada de crates

Decisiones: ADR-044.

**Decisión.** `rust.crate.inspect` pagina explícitamente por parámetros —
**no** un cursor firmado, porque no hay una frontera de autorización que
cruzar entre páginas; el fingerprint de la generación basta para detectar
staleness. Un cambio de generación entre dos llamadas produce
`snapshot_mismatch` explícito **antes** de leer cualquier hecho de la
página nueva. Los campos de documentación o de source ausentes se declaran
`unknown`/`not_recorded_in_snapshot` — el producto nunca inventa una URL de
docs.rs o del registry que no esté en el propio snapshot.

**Estado actual.** Vigente; última de las 13 tools M1 originales, sin
sobreclaim detectado. Evidencia:
`crates/mcp-server/src/stdio/crate_inspect.rs`,
`crates/catalog-adapter/src/inspect.rs`; tests
`crates/catalog-adapter/tests/crate_inspect.rs`.

## Hechos de supply-chain sin migrar el catálogo

Decisiones: ADR-071.

**Decisión.** `rust.supply_chain.inspect` compone captura de `ProjectRef` +
auditoría RustSec + `cargo-deny` opcional + hechos derivados de
`Cargo.lock`/metadata, **sin migrar el esquema SQLite ni adquirir datos
nuevos** — es puramente una composición sobre lo que ya existe. El
localizador Source/URL de cada paquete se **omite por completo** del
output: solo se reporta la clase de fuente (crates.io/git/path) más un hash
de los bytes literales del identificador, nunca una resolución de red hacia
esa URL. "Ausente", "no disponible" y "no consultado por límite" son tres
razones `unknown` distintas y deliberadas — el agente puede distinguir por
qué falta un dato. El response **nunca** computa un score de seguridad
agregado ni una certificación legal — eso queda explícitamente fuera de
alcance por diseño, no por omisión.

**Estado actual.** Vigente; evidencia
`crates/application/src/supply_chain.rs`,
`crates/domain/src/supply_chain.rs`,
`crates/mcp-server/src/stdio/supply_chain.rs`; tests
`tests/inspection_runtime/security.rs`,
[`docs/validation/M4/{core-gate,tools-mcp,clients,full-gate}.json`](https://github.com/pharos-lang/rust-engineering-mcp/tree/51fa602e/docs/validation/M4).

**Limitación documental.** [`docs/adr/README.md`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/README.md) marca ADR-071 como
"pendiente" — desactualizado; el cuerpo y el código muestran implementación
completa (ver [`decisions.md`](decisions.md)).

## Provenance y freshness aplicados sin excepción

El invariante fundacional de ADR-020 (ver
[`domain-and-application.md`](domain-and-application.md#provenance-y-freshness-obligatorios))
se aplica sin excepción a los tres tools de este capítulo: usar siempre
`latest_known`, nunca `latest`, y declarar siempre integridad/edad del
snapshot consultado. El domain también mantiene un `CompositeCatalog`
conceptual (ports `CatalogRepository`/`SemanticIndex`/`EmbeddingProvider`/
`CacheStore`) independiente de SQLite/LanceDB/crates.io — el nombre exacto
del tipo difiere entre `crates/domain`/`crates/application`, pero el patrón
de independencia de dominio frente al store concreto está presente. El
propio dominio también distingue **datos de proyecto** (manifiestos, lock,
metadata de Cargo — siempre de `project-adapter`) de **datos de catálogo
global** (siempre de `catalog-adapter`) — nunca se confunden ni se
mezclan en un mismo store.

**Limitación (aspiracional, sin código).** Un `CacheStore` genérico,
independiente del catálogo, keyed por tool + fingerprint + toolchain +
target + features + versión + args, apareció en la especificación
original pero no existe ningún código de ese tipo en el árbol; quedó
retriagado como mantenimiento documental únicamente, nunca programado como
trabajo de implementación.
