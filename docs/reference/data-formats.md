# Formatos de datos

Formatos en disco y de transporte del proyecto: bundle del catálogo, journal
de mutación, store de artifacts de quality jobs, store de artifacts M1,
captura de vendor y el envelope de resultado de cada tool. Todos llevan su
propio marcador de versión y fallan cerrado ante una versión que no
reconocen — nunca coaccionan ni reinterpretan bytes. Reglas de
migración/rollback entre versiones del binario:
[`operations/backup-and-recovery.md`](../operations/backup-and-recovery.md).

## Envelope de resultado (todas las tools)

Toda tool devuelve el mismo envelope (`crates/domain/src/result.rs:101-111`):

```rust
pub struct OutputEnvelope<T> {
    status: ToolStatus,        // passed | failed | blocked | unavailable | cancelled
    summary: NonEmptyText,
    duration_ms: u64,
    error_code: Option<OperationalErrorCode>,  // requerido, pero puede ser null
    error_message: Option<NonEmptyText>,       // requerido, pero puede ser null
    diagnostics: Vec<Diagnostic>,
    truncation: Truncation,    // { stdout_truncated, stderr_truncated, diagnostics_omitted }
    data: T,
    evidence: Evidence,
}
```

`status` usa `snake_case` en el wire; `error_code` usa `SCREAMING_SNAKE_CASE`
(`OperationalErrorCode`, `crates/domain/src/result.rs:23-33`). `error_code`
y `error_message` son campos "requeridos pero nullable": el deserializador
exige que la clave esté presente aunque su valor sea `null`
(`result.rs:113-127`). Detalle de qué significa cada `status` y por tool:
[`reference/tools.md`](tools.md#convenciones-comunes).

## Bundle del catálogo (transporte firmado)

Transporte v1: un archivo USTAR estricto comprimido con Zstandard. Primera
entrada `manifest.json`, segunda `signature.ed25519`, después el payload en
orden ascendente estricto de path (`catalog.sqlite` obligatorio,
`rustsec.json` y `semantic.index` opcionales). Solo entradas regulares —
nunca symlinks, hardlinks, directorios ni devices; sin duplicados ni
reordenamiento.

- `BundleManifest` (`crates/catalog-adapter/src/bundle.rs:96-107`):
  `snapshot_format_version`, `catalog_schema_version`,
  `semantic_index_version` (nullable), `embedding_model_id` (nullable),
  `publisher`, `channel`, `sequence`, `catalog_provenance`, `files[]`
  (`{path, byte_length, sha256}`, `bundle.rs:89-93`). Serializado como JSON
  canónico (`serde_json::to_vec`, orden de campos fijo, sin campos
  desconocidos); una reserialización que no coincide byte a byte con el
  manifiesto firmado es `NoncanonicalManifest`.
- Firma: 64 bytes Ed25519 crudos sobre
  `SIGNING_CONTEXT = b"rust-engineering-catalog-bundle-v1\0"` + los bytes
  exactos del manifiesto (`bundle.rs:16,185-189`), verificados con
  `ring::signature::ED25519`.
- Trust root: archivo del host, modo `0600`, JSON
  `{publisher, channel, public_key}` (64 hex, Ed25519 crudo) — sin confianza
  compilada ni TOFU (`bundle.rs:42-79`).
- Cuotas: bundle ≤ 80 MiB, manifiesto ≤ 16 KiB, ≤ 16 entradas de archivo
  (`bundle.rs:13-15`); SQLite ≤ 64 MiB/1000 crates; índice nativo ≤ 16 MiB/128
  objetos/8 MiB por objeto; modelo ≤ 512 MiB con hash pinneado.
- Reserve-before-activate: `floor.record`/`floor.staging` puede ir por
  delante del activo pero **nunca por detrás**, bajo un lock OS exclusivo
  (`crates/project-adapter/src/catalog_store.rs`: `ACTIVE`/`STAGING`/`LOCK`
  en líneas 51-53; `replace_record()` en 520-585 hace
  staged→fsync→fullfsync→rename→verificación de relectura exacta, marcando
  durabilidad incierta ante cualquier fallo posterior al rename).
- `SequenceFloor::Record` (`crates/catalog-adapter/src/bundle/floor.rs:23-30`):
  `{format_version, publisher, channel, sequence, bundle_sha256, checksum}`,
  con `checksum = sha256("catalog-floor-v1\0{publisher}\0{channel}\0{sequence}\0{bundle_sha256}")`
  (`floor.rs:50-61`). Un bundle de secuencia menor que el floor se rechaza
  como `Rollback` (`bundle.rs:132-138`), incluso leído por un binario más
  viejo — es el único floor de secuencia genuino del proyecto; ningún otro
  formato en disco lo tiene.

Detalle histórico anterior a esta página:
[`docs/catalog-bundle-format.md` en 51fa602e](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/catalog-bundle-format.md)
(esta página lo supersede como referencia vigente).

## Catálogo SQLite

`PRAGMA user_version` versiona el esquema interno; `migrate()`
(`crates/catalog-adapter/src/lib.rs:113-130`) lo lee y aplica
`apply_v1_migration` dentro de una transacción `Immediate` si está en `0`
(vacío); `user_version > 1` es `UnsupportedSchema` — fail-closed ante un
esquema futuro que este binario no conoce. Cada migración registra su huella
en una tabla `migrations` (`lib.rs:131-146`), y `validate_ledger()` la
recomprueba contra el esquema esperado en memoria (`lib.rs:147-165`).
`validate_database()` exige `PRAGMA integrity_check = ["ok"]` y ausencia de
violaciones de `foreign_key_check`. La base activa se abre siempre en modo
solo lectura durante `serve` (regla arquitectónica, `AGENTS.md`); import,
sync y rebuild son operaciones exclusivas de la CLI (ver
[`operations/catalog-maintenance.md`](../operations/catalog-maintenance.md)).

## Índice semántico LanceDB (derivado, nunca autoritativo)

La identidad de una generación del índice es la combinación de fingerprint
del snapshot + `IndexMetadata.schema_version` + identidad del modelo de
embeddings (id del modelo + dimensión) — `crates/domain/src/semantic.rs:70-71`.
`LanceMemoryIndex::build`/`restore` rechaza `schema_version != 1` o un modelo
inválido antes de escribir el esquema Lance
(`crates/semantic-adapter/src/index.rs:44`). Un desajuste entre el índice y
el catálogo SQLite activo nunca corrompe estado ni mezcla generaciones:
marca la búsqueda semántica como no disponible y conserva la búsqueda
léxica/metadata sobre SQLite. Reconstrucción completa, nunca reparación
parcial: `catalog rebuild-index`.

## Journal de mutación (`rust-mcp-mutations-v1`)

Directorio `<state-root>/rust-mcp-mutations-v1`
(`crates/mcp-server/src/host_config.rs:300`). Constantes de cuota en
`crates/project-adapter/src/filesystem/macos/mutation.rs`: `MAX_JOURNALS=128`
(`:23`), `MAX_STORE_BYTES=256 MiB` (`:24`), `MAX_JOURNAL_BYTES=48 MiB`
(`:25`), `WORKSPACE_LOCK_SHARDS=64` (`:38`).

Cada entrada es un archivo `journal-<id>.json`, con staging
`.journal-<id>.json.staging` durante la escritura. El marcador de versión no
se llama `format_version`: es el campo `format` dentro del registro
(`JournalRecordV2 { format, checksum, body }`, `mutation.rs:637-643`), con
valor fijo `"rust-engineering-mcp-mutation-journal-v2"`; existe también un
lector legado `LegacyJournalRecordV1` (`"...-v1"`, `mutation.rs:691-697`).

**El "sniff" de formato falla cerrado sobre todo el store antes de
clasificar por registro**: `decode_envelope()`
(`mutation.rs:791-851`) primero deserializa solo
`JournalFormatProbe { format }` y hace `match` contra los dos valores
reconocidos; cualquier otro valor, JSON malformado o tamaño por encima de
`MAX_JOURNAL_BYTES` cae en `_ => Err(RecoveryRequired)` **antes** de tocar
workspace o store. Cada variante reconocida se revalida además por checksum
(`canonical_checksum(&body) != checksum` → `RecoveryRequired`) y por tipos
(`MutationId`, `IdempotencyKey`, `SourceFingerprint`, `operation_kind()`
cerrado) antes de confiar en su contenido. Detalle operativo de esta
propiedad (por qué un journal irreconocible bloquea el store completo, y
cómo remediarlo): [`operations/backup-and-recovery.md`](../operations/backup-and-recovery.md).

Locking: un lock global de store (`mutation-store.lock`) y, por debajo, un
lock por workspace elegido por `sha256(device_le || inode_le)[0] % 64`
(`workspace_lock_name`, `mutation.rs:1182-1188`) — siempre en ese orden.

Los planes en memoria (`preview`, antes de journal) son un concepto
distinto: `MutationPlans::TTL_SECONDS = 600`
(`crates/application/src/mutation.rs:449`), máximo 4 planes pendientes y 64
MiB agregados por store (`crates/application/src/mutation.rs:508`).

## Store de artifacts de quality jobs (`rust-mcp-quality-artifacts-v1`, M3+)

Layout (`crates/project-adapter/src/filesystem/macos/quality.rs:1-48,73-83`):

```text
<state-root>/rust-mcp-quality-artifacts-v1/
  store.lock
  clock-watermark.json
  reservation/  blob/  descriptor/  quarantine/
```

`QualityArtifactDescriptor.format_version` fijo en `1`
(`crates/domain/src/quality_artifact.rs:472-473`); cada payload lleva además
su propio `PayloadFormatVersion` cerrado por kind (`JunitXmlV1`,
`CoverageJsonV1`, `LcovV1`, `UstarV1`, `MutationDiffV1`, `Utf8LogV1`,
`BenchmarkDatasetV2`, `CollapsedStacksV1`, `FlamegraphSvgV1`, `BloatJsonV1`,
`DeclaredV1` — `quality_artifact.rs:329-343`). Una combinación
`(kind, payload_format_version)` desconocida se rechaza en
`descriptor.validate()` (`quality_artifact.rs:494-554`). El watermark de
reloj monótono del store tiene su propio `format_version` independiente
(`QualityClockWatermark`, `quality_artifact.rs:277-292`) — versiona el
testigo de reloj, no el artifact.

Cuotas: 32 MiB por artifact, 64 MiB/≤128 miembros por job, 128 MiB por
owner, 256 MiB global, TTL por defecto 3600 s/máximo 86 400 s, margen de
recuperación 49 MiB (`quality_artifact.rs:687-699`). Administración fuera de
una sesión MCP: `quality-artifacts recover`/`prune`
([`reference/cli.md`](cli.md)).

`BenchmarkDatasetV2` sustituyó a un `V1` sin conservar lector legado — a
diferencia del journal de mutación (que sí conserva un lector v1), este
store rompe y deja expirar por TTL en vez de migrar.

## Captura de vendor para Criterion (offline, content-addressed)

Cuotas (`crates/domain/src/vendor_capture.rs:41-58`): 512 MiB totales, 32 768
entradas, 8 MiB por archivo, 200 bytes por path, profundidad 16 — deliberadamente
más generosas que el `SourceBundle` estándar porque el cierre de Criterion
0.8.2 (~156 MiB) no cabe en sus cuotas.

Dos hashes independientes: un **digest de árbol** (framing canónico
domain-separado por `TREE_DOMAIN = b"rust-engineering-mcp/vendor-capture/v1\0"`,
`vendor_capture.rs:166` — la identidad que viaja en la provenance de
`rust.benchmark.run` y que `--vendor-capture-tree-sha256` fija) y un
**digest de artifact** (bytes crudos del USTAR emitido, para detectar un
artifact reescrito que decodifica al mismo árbol). Transporte USTAR con
extensión `pax` para paths cuyo último componente excede 100 bytes. Alfabeto
de path más amplio que `SourceBundle` (incluye `()+,=@[]{}~ ` además de
`[A-Za-z0-9._/-]`) para admitir paths reales de crates.io con paréntesis.

Comparar con el `SourceBundle` estándar que capturan `rust.project.open`/
`inspect` y el vendor Cargo normal (`crates/domain/src/source.rs:4-8`): 4096
entradas, profundidad 32, 100 bytes por path, 1 MiB por archivo, 16 MiB
totales, alfabeto más estrecho. Son cuotas deliberadamente distintas, nunca
intercambiables entre tools — detalle completo en
[`reference/limits.md`](limits.md).

## Store de artifacts M1 (`rust-artifact://`, en memoria)

Proceso-local, solo memoria, sin filesystem ni red
(`crates/artifact-adapter/src/lib.rs:1`). Cuotas por defecto
(`ArtifactLimits::default()`, `lib.rs:23-34`): 1 MiB de entrada, 256 KiB por
artifact, 16 MiB global (≤ 256 artifacts), 1 MiB por owner (≤ 64 artifacts),
TTL 3600 s. URI: `rust-artifact://{project_ref}/{artifact_id}`
(prefijo `"rust-artifact://"` en `crates/mcp-server/src/stdio/resources.rs:32`;
patrón completo `^rust-artifact://prj_[0-9a-f]{32}/art_[0-9a-f]{32}$`). Se
pierde íntegramente al reiniciar el proceso — no hay claim de persistencia
entre reinicios.

## Entradas de host sin estado de servidor

Config de `serve`, policy de seguridad (`cargo-deny`), snapshot de
advisories RustSec y árbol vendor de Cargo se verifican por pin SHA-256 o
por esquema **en cada invocación** — no tienen generación ni estado
persistido que migrar o revertir; se vuelven a suministrar al arrancar. La
config de host es el único de estos formatos que cambió entre `v0.3.0` y el
checkout actual (un campo/flag nuevo, `--allow-analyzer-action-write`, más
la imagen M6 admitida), de forma estrictamente aditiva, sin alterar ningún
campo existente.

El journal de mutación (`rust-mcp-mutations-v1`, arriba) **sí** es estado de
servidor persistido — no pertenece a este grupo. Su propio cambio entre
`v0.3.0` y el checkout actual también es estrictamente aditivo (una
variante nueva de `kind` de operación, `analyzer_action_apply`, sin alterar
el envelope ni el algoritmo de checksum); ver
[`operations/backup-and-recovery.md`](../operations/backup-and-recovery.md)
para la regla de compatibilidad y el mecanismo de detección de
`downgrade_blocked`.

## Decisiones relacionadas

Ver [`architecture/decisions.md`](../architecture/decisions.md) para el ADR
que fija la política de migración/rollback y los ADRs de catálogo/mutación
que fijan cada formato individual.
