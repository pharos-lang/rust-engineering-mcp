# M8-03 (preparación D12) — Análisis de formatos en disco y marcadores de versión

Worker W13-formats-analysis (Claude Sonnet 5, `--effort high`), solo lectura de
código. Rama `ai/m8-stabilization`, tip `6fa1ef1` al iniciar. Comparación
`git diff v0.3.0 HEAD` (tags publicados: `v0.1.0`, `v0.3.0`; no existen
`v0.4.0`–`v0.7.0`, confirmado por `git tag`). Fuente del censo:
[`01-census.json`](01-census.json) `disk_formats[]` (10 formatos) y finding F7.
Plan: [`m8-stabilization.md`](../../roadmap/m8-stabilization.md) §M8-03 y
§«Migración, recovery y seguridad»; [`adr-backlog-m2-m8.md`](../../roadmap/adr-backlog-m2-m8.md)
§D12.

## Método y alcance de la comparación

`git log v0.3.0..HEAD --oneline -- <path>` y `git diff v0.3.0 HEAD -- <path>`
se ejecutaron por archivo (no por directorio agregado) sobre cada
`reader_writer`/`location_layout` citado en el censo. Resultado agregado:
solo **tres** archivos de los ~26 inspeccionados tienen commits desde
`v0.3.0`, y los tres cambios son estrictamente aditivos (ver formato 2 y el
host config). Todo lo demás —catálogo SQLite, bundle de confianza, índice
LanceDB, política de seguridad, auditoría RustSec, vendor tree, almacén de
artifacts M3— es **byte-idéntico** a `v0.3.0` en los archivos fuente
inspeccionados. `analyzer_native.rs`/`analyzer_gateway.rs` (formato M6) son
archivos enteramente nuevos desde `v0.3.0` (M6 se integró después del tag);
no representan un formato que cambió, sino uno que no existía en `v0.3.0`.

---

## 1. Host config (flags de `serve`)

1. **Marcador de versión:** ninguno. No hay archivo de configuración
   persistido; las flags CLI (`--root`, `--allow-*-write`,
   `--catalog-*`, `--docker*`, `--*-policy/-snapshot`) se vuelven a pasar en
   cada arranque del proceso (`crates/mcp-server/src/host_config.rs:1-40`).
2. **Versión desconocida/futura:** el parser falla cerrado antes de cualquier
   efecto — un valor de `--rust-image` que no coincide con ninguna de las
   cinco constantes `APPROVED_*_IMAGE` (incluida `APPROVED_M6_IMAGE`, añadida
   en este rango) devuelve `None` y el proceso nunca arranca
   (`crates/mcp-server/src/host_config.rs:177-184`). Mismo patrón para flags
   desconocidas fuera del conjunto reconocido (match exhaustivo, `_ =>
   return None` implícito en el bucle de flags).
3. **Cambio desde `v0.3.0`:** `changed-compatible`, puramente aditivo. Diff de
   2 commits (`4309f33`, `dffa620`) añade el campo
   `analyzer_action_write_roots: Vec::new()`, la flag
   `--allow-analyzer-action-write` y la constante `APPROVED_M6_IMAGE` a la
   lista de imágenes aceptadas (`crates/mcp-server/src/host_config.rs` diff
   completo revisado: 7 hunks, todos suma de una nueva rama `else`/flag/campo,
   ninguna rama existente se elimina o cambia de significado). No hay formato
   en disco que migrar porque no hay disco.
4. **Estado que no debe retroceder:** N/A — no hay estado persistido para
   este formato en sí. La única invariante cruzada es que las rutas de
   escritura declaradas (`config.manifest_write_roots` etc.) no pueden
   solapar con el journal de mutaciones (`crates/mcp-server/src/host_config.rs:283-292`).
5. **Recuperación:** N/A, no hay bytes que recuperar (re-suministrar flags en
   el siguiente arranque).
6. **Tests existentes:** `crates/mcp-server/src/host_config.rs` tests inline
   (bloque `#[cfg(test)] mod tests`, líneas 297+) cubren la tupla Docker
   completa; `crates/mcp-server/tests/cli.rs::rust_runtime_options_require_a_complete_unique_approved_tuple`
   (línea 207) es el test de imagen desconocida/incompleta a nivel binario.
   No hay test de "versión desconocida" per se porque no hay versión: hay
   "imagen no aprobada", que es el equivalente funcional y está cubierto.

---

## 2. M2 — journal de mutación / receipts

1. **Marcador de versión:** **existe y es explícito**, contra lo que sugiere
   la redacción de F7 del censo. Cada registro de journal serializado lleva
   un campo `format: String` con un valor de magia versionado —
   `"rust-engineering-mcp-mutation-journal-v2"` o, para registros heredados,
   `"...-v1"` (`crates/project-adapter/src/filesystem/macos/mutation.rs:637-643,
   691-697, 753-759`). F7 tiene razón en que no existe un campo llamado
   literalmente `format_version`/`schema_version` (el grep citado por F7 fue
   correcto), pero sí existe un marcador de versión a nivel de registro con
   otro nombre de campo; la corrección de matiz queda registrada aquí para
   D12. Adicionalmente, el namespace del directorio de estado
   (`rust-mcp-mutations-v1`, `crates/mcp-server/src/host_config.rs:285`) sigue
   siendo solo de nivel-namespace, sin relación con el campo `format` interno.
2. **Versión desconocida/futura:** fail-closed antes de cualquier efecto. El
   sniff de formato (`JournalFormatProbe`) primero lee solo el campo
   `format`; el `match` exhaustivo solo reconoce `"...-v2"` y `"...-v1"`, y
   cualquier otro valor cae en `_ => return Err(MutationError::RecoveryRequired)`
   (`crates/project-adapter/src/filesystem/macos/mutation.rs:791-851`, rama
   final línea 851) — antes de tocar el workspace o el store. Probado en
   `unknown_journal_format_never_cleans_or_changes_source`
   (`crates/project-adapter/tests/support/native_mutation.rs:454-510`): un
   journal con `format` mutado a `"...-v3"` hace que `receipt`/`recover`/`replay`
   devuelvan `RecoveryRequired` sin tocar `Cargo.toml` ni el árbol de estado
   (comparación de snapshots antes/después, líneas 482-508).
3. **Cambio desde `v0.3.0`:** `changed-compatible`. Único cambio real: una
   nueva variante cerrada de operación, `analyzer_action_apply`, añadida al
   `match` de `operation_kind`/`operation_name`
   (`crates/project-adapter/src/filesystem/macos/mutation.rs:930-946`, commit
   `dffa620`) y al digest de candidato
   (`crates/project-adapter/src/mutation_store.rs:44-49`, mismo commit). El
   struct `JournalBody`, el envelope `JournalRecordV2` y el algoritmo de
   checksum canónico no cambiaron ni un byte. Un binario `v0.3.0` que
   encuentre un registro con `operation: "analyzer_action_apply"` (solo
   posible en un servidor `0.8.0` con `--allow-analyzer-action-write`, flag
   que `v0.3.0` no reconoce) cae en la misma rama `_ => RecoveryRequired` de
   `operation_kind` (línea 936) — el mismo mecanismo que protege contra
   formatos futuros protege también contra el downgrade de binario.
4. **Estado que no debe retroceder:** el journal en sí no lleva floor/trust
   (`floor_or_trust_state: false` en el censo); su invariante es de
   **exclusividad y fase**, no de secuencia monótona: `require_newer_staging`
   rechaza cualquier body de staging cuya fase tenga rango menor que el body
   final ya comprometido (`crates/project-adapter/src/filesystem/macos/mutation.rs:624-635`).
   No existe hoy un chequeo explícito a nivel de *binario* de "hay un journal
   pendiente, rehúsa arrancar en modo downgrade" — la protección es
   enteramente reactiva (falla al leer/recuperar ese registro concreto), no
   proactiva a nivel de proceso. Esto es un hueco real frente al mandato del
   plan "journal pendiente impide downgrade" (ver §(c) más abajo).
5. **Recuperación:** `mutation list`/`mutation prune`
   (`crates/mcp-server/src/mutation_cli.rs:1-75`, subcomandos línea 27-29) más
   el flag explícito `recover: bool` en el input MCP de
   `rust.manifest.patch`/`fmt.apply`/`fix.apply`/`dependency.add`/`dependency.remove`
   (`crates/mcp-server/src/stdio/mutation.rs:97,126,170-173,1191-1197`,
   propagado a `mutation_receipt(..., recover, ...)` en
   `crates/application/src/mutation.rs:206-223`). La recuperación es opt-in
   explícito del cliente, nunca automática en la ruta normal de lectura —
   satisface "estado corrupto exige recuperación explícita" del plan. No hay
   subcomando de "backup"/"restore" dedicado; la durabilidad viene del propio
   protocolo staging→rename atómico con `fcntl_fullfsync`
   (`crates/project-adapter/src/filesystem/macos/mutation.rs:460-465`).
6. **Tests existentes** (todos en
   `crates/project-adapter/tests/support/native_mutation.rs` salvo donde se
   indique): `legacy_v1_receipt_is_read_only_and_explicit_recovery_migrates_to_v2`
   (línea 353, **upgrade N-1→N real**: un registro v1 en disco se lee
   read-only y solo `recover()` explícito lo reescribe como v2);
   `terminal_legacy_v1_replay_migrates_only_after_exact_binding` (línea 412);
   `unknown_journal_format_never_cleans_or_changes_source` (línea 454,
   versión desconocida/futura); `corrupt_journal_blocks_all_new_commits_before_source_write`
   (`crates/project-adapter/tests/mutation_store.rs:707`); `journal_count_quota_and_unknown_store_entry_fail_closed`
   (`crates/project-adapter/tests/mutation_store.rs:966`);
   `killed_process_rolls_forward_known_format_prefix` (línea 1015,
   crash-recovery); `unknown_untouched_bytes_stop_format_recovery_without_advancing_suffix`
   (línea 1110); `manifest_and_lock_first_swap_crash_rolls_forward_only_known_bytes`
   (línea 1164, crash tras swap); `post_swap_durability_enospc_recovers_known_after_generation`
   (línea 1635, **disk full/ENOSPC**); `corrupt_store_is_quarantined_while_a_new_physical_workspace_and_store_continue`
   (línea 1715); `protected_nested_swap_rejects_a_symlinked_parent_that_plain_swap_follows`
   (línea 2744, **symlink**); `commit_preserves_private_mode_and_extended_attributes`
   (`crates/project-adapter/tests/mutation_store.rs:810`). No se encontró un
   test explícito de "rollback N→N-1" a nivel de binario completo (solo el
   equivalente por formato desconocido descrito en el punto 3); no se
   encontró un test de "permisos revocados a mitad de operación" (solo tests
   de modo/permisos fijos al escribir).

---

## 3. M3 — almacén de artifacts de quality/jobs

1. **Marcador de versión:** doble, explícito y granular. Envelope:
   `QualityArtifactDescriptor.format_version: u8` (`crates/domain/src/quality_artifact.rs:472-473`,
   valor fijo `1`). Payload: `PayloadFormatVersion` — enum cerrado con once
   variantes específicas por tipo de artifact (`JunitXmlV1`, `CoverageJsonV1`,
   `LcovV1`, `UstarV1`, `MutationDiffV1`, `Utf8LogV1`, `BenchmarkDatasetV2`,
   `CollapsedStacksV1`, `FlamegraphSvgV1`, `BloatJsonV1`, `DeclaredV1`;
   `crates/domain/src/quality_artifact.rs:329-343`). Un tercer marcador
   independiente, `QualityClockWatermark.format_version: u8`
   (`crates/domain/src/quality_artifact.rs:277-280`), versiona el testigo de
   monotonicidad del reloj del store, no el artifact.
2. **Versión desconocida/futura:** fail-closed en tres puntos. Descriptor:
   `descriptor.validate()` rechaza `format_version != 1` y cualquier
   combinación `(kind, payload_format_version)` que no esté en la lista
   cerrada de pares válidos (`crates/domain/src/quality_artifact.rs:494-554`).
   Watermark: `QualityClockWatermark::validate()` rechaza `format_version !=
   1` (`crates/domain/src/quality_artifact.rs:288-292`). El store nativo
   demuestra ambos con tests dedicados (ver punto 6).
3. **Cambio desde `v0.3.0`:** `unchanged`. `git log v0.3.0..HEAD` sobre
   `crates/project-adapter/src/quality_artifact_store.rs`,
   `crates/domain/src/quality_artifact.rs`,
   `crates/mcp-server/src/quality_artifact_cli.rs` y
   `crates/mcp-server/src/stdio/quality_artifacts.rs` no devuelve ningún
   commit. Nota histórica (documentada por el propio censo, no verificada por
   este worker vía diff porque ocurrió antes de `v0.3.0`): `BenchmarkDatasetV2`
   reemplazó una `V1` anterior sin dejar la variante `V1` en el enum
   (`grep BenchmarkDataset` sobre `quality_artifact.rs` no devuelve ninguna
   ocurrencia de `V1`) — es decir, ese bump histórico fue una ruptura, no una
   adición retrocompatible; los artifacts M3 expiran por TTL
   (`QUALITY_MAX_TTL_SECONDS`, `crates/domain/src/quality_artifact.rs:506`),
   así que el precedente de este store es "romper y dejar expirar", distinto
   al precedente de journal M2 ("mantener lector legado y migrar
   explícitamente").
4. **Estado que no debe retroceder:** `floor_or_trust_state: false` en el
   censo — no hay floor de secuencia. La única invariante de monotonicidad es
   el reloj: `QualityClockWatermark` documenta explícitamente que "a durable
   instant later than the observed wall clock is a regression that fails the
   store closed" (`crates/domain/src/quality_artifact.rs:272-274`), probado
   por `a_durable_clock_regression_blocks_only_quality_until_recovery`
   (`crates/project-adapter/tests/quality_artifact_store.rs:1066`).
5. **Recuperación:** `quality-artifacts recover`/`prune`
   (`crates/mcp-server/src/quality_artifact_cli.rs:21-22,74-80`, delegando a
   `crates/project-adapter/src/quality_artifact_store.rs:101-104`
   `recover()`/`prune_expired()`).
6. **Tests existentes** (todos en
   `crates/project-adapter/tests/quality_artifact_store.rs`):
   `corrupt_or_unknown_objects_are_quarantined_with_a_closed_reason` (línea
   607); `an_unknown_record_version_fails_closed_and_is_never_reinterpreted`
   (línea 1712, **versión desconocida** vía `bump_record_version`, línea
   1682); `an_unknown_watermark_version_blocks_quality_and_rebases_nothing`
   (línea 1778); `operator_recover_and_prune_separate_valid_expired_and_unknown_objects`
   (línea 1891); `owner_and_global_quotas_reject_before_the_gateway_and_evict_nothing`
   (línea 911, cuota/disk-budget); `native_apfs_quality_two_processes_share_one_global_quota_view`
   (línea 2061). No se encontró test de "config antigua" (no aplica: no hay
   config, solo artifacts) ni de rollback de binario completo.

---

## 4. M6 — estado del analyzer (rust-analyzer)

1. **Marcador de versión:** no hay estado runtime persistido que versionar.
   ADR-084 fija una instancia LSP transitoria por consulta dentro de la
   imagen guest M6, semántica `latest_known` no atómica
   (`crates/execution-adapter/src/analyzer_native.rs`,
   `crates/execution-adapter/src/analyzer_gateway.rs`). El único artefacto
   con marca de esquema es el *recibo de calibración* — un artefacto de
   build-time/provisioning, no de runtime —: constante
   `RECEIPT_SCHEMA = "rust-engineering-mcp.m6-calibration.v1"`
   (`crates/execution-adapter/src/analyzer_native.rs:42`, escrito en
   `target/m6-calibration/receipt.json`, líneas 6-7,751,980).
2. **Versión desconocida/futura:** N/A para runtime state (no hay que leerlo
   de vuelta entre arranques). El recibo de calibración se verifica contra el
   esquema propio del binario en un test explícito
   (`m6_analyzer_version_and_config_schema_match_the_receipt`,
   `crates/execution-adapter/src/analyzer_native.rs:1109`, marcado
   `#[ignore]` porque requiere imagen M6 y Docker del host).
3. **Cambio desde `v0.3.0`:** N/A como "cambio de formato" — `analyzer_native.rs`
   y `analyzer_gateway.rs` son archivos enteramente nuevos en este rango (M6
   se integró después de `v0.3.0`, `git log v0.3.0..HEAD` sobre ambos
   devuelve el commit de introducción completo, no un diff incremental). No
   hay bytes previos que migrar porque el formato no existía.
4. **Estado que no debe retroceder:** N/A, no hay estado persistido.
5. **Recuperación:** N/A.
6. **Tests existentes:** `crates/mcp-server/tests/analyzer_runtime.rs`,
   `crates/mcp-server/src/stdio/analyzer/tests.rs` (contrato/lifecycle, no
   formato en disco).

---

## 5. Catálogo SQLite (autoritativo)

1. **Marcador de versión:** doble capa, ambas explícitas y aplicadas.
   Interna: `PRAGMA user_version` dentro del propio fichero SQLite
   (`crates/catalog-adapter/src/lib.rs:113-130`, `migrate()`); rechaza
   `version > 1` (línea 117-118), trata `version == 0` como store vacío y
   aplica la migración v1 (línea 120-127), fija `user_version = 1` tras
   migrar (línea 143-144). Envelope externo: `SnapshotManifest.format_version:
   u32` (`crates/catalog-adapter/src/lib.rs:17-26`), fijado a `1` al
   serializar (línea 263) y verificado al abrir (línea 273-276).
2. **Versión desconocida/futura:** fail-closed en ambas capas, antes de
   cualquier efecto. `migrate()` devuelve `CatalogError::UnsupportedSchema`
   si `user_version > 1` (línea 117-118); `SqliteCatalogRepository::open`
   devuelve el mismo error si `expected.format_version != 1` (línea
   274-276), antes de deserializar ningún byte de la base. Probado por
   `migration_is_atomic_idempotent_and_rejects_unknown_schema`
   (`crates/catalog-adapter/src/tests.rs:49-66`, línea 63-64: bump manual a
   `user_version=2` produce `Err(UnsupportedSchema)`).
3. **Cambio desde `v0.3.0`:** `unchanged`. `git log v0.3.0..HEAD` sobre
   `crates/catalog-adapter/src/lib.rs`, `crates/catalog-adapter/src/schema.sql`,
   `crates/project-adapter/src/catalog_store.rs` y
   `crates/mcp-server/src/catalog_cli.rs` no devuelve ningún commit. Todavía
   solo existe la migración v1 (nunca se ha ejercido un salto real v1→v2 en
   producción); el mecanismo está probado pero no ejercitado con datos reales
   de una versión anterior.
4. **Estado que no debe retroceder:** el catálogo SQLite en sí no lleva un
   floor de secuencia propio (eso vive en el bundle de confianza, §6); su
   invariante es de **integridad estructural**: `validate_database` compara
   el esquema vivo contra el esquema esperado recién migrado
   (`schema_rows(connection)? != schema_rows(&expected)?`, línea 197-199) y
   valida la tabla `migrations` como ledger de checksums (`validate_ledger`,
   línea 147-165). `rehashed_hostile_images_fail_validation_beyond_digest`
   (`crates/catalog-adapter/src/tests.rs:68-92`) ejercita doce mutaciones
   hostiles distintas del fichero (incluida `PRAGMA user_version=99`), todas
   rechazadas. Downgrade de binario `0.8.0 → 0.3.0`: dado que el esquema no
   cambió en absoluto desde `v0.3.0` (punto 3), un binario `0.3.0` leería sin
   problema una base escrita por `0.8.0` — no hay riesgo de regresión hoy
   porque no hay diferencia de formato que explotar.
5. **Recuperación:** `catalog status`/`import`/`sync`/`rebuild-index`
   (`crates/mcp-server/src/catalog_cli.rs:37-40`); mensaje de guía explícito
   en el propio CLI para el caso de estado de secuencia inválido/ausente:
   *"Retained sequence state is invalid or missing; restore trusted state
   from a verified backup without resetting its floor"*
   (`crates/mcp-server/src/catalog_cli.rs:325`). No existe un subcomando
   `catalog backup`; la recuperación asume que el operador conserva una
   copia externa del bundle verificado para reimportar.
6. **Tests existentes:** `migration_is_atomic_idempotent_and_rejects_unknown_schema`
   (`crates/catalog-adapter/src/tests.rs:49`);
   `rehashed_hostile_images_fail_validation_beyond_digest` (línea 68);
   `runtime_read_only_and_attach_disabled_and_real_fts` (línea 94). No se
   encontró test de upgrade v1→v2 con datos reales (no aplica: v2 no existe
   todavía) ni de rollback de binario.

---

## 6. Catálogo — bundle de confianza firmado (`--catalog-trust`)

1. **Marcador de versión:** `BundleManifest.snapshot_format_version: u32` +
   `catalog_schema_version` (`crates/catalog-adapter/src/bundle.rs:96-97`,
   ambos fijados a `1` en la construcción, línea 234), verificados en
   `verify()`: `manifest.snapshot_format_version != 1 ||
   manifest.catalog_schema_version != 1` (`crates/catalog-adapter/src/bundle.rs:200`).
   Un tercer marcador independiente versiona el **floor** de secuencia (no el
   bundle en sí): `SequenceFloor` interno `Record.format_version: u32`
   (`crates/catalog-adapter/src/bundle/floor.rs:23-24`, fijado a `1` en
   `SequenceFloor::new`, línea 39).
2. **Versión desconocida/futura:** fail-closed en ambos puntos, antes de
   cualquier efecto. `verify()` rechaza `format_version`/`schema_version`
   distintos de 1 (línea 200 citada arriba) probado por
   `rejects_noncanonical_signed_manifest_and_unknown_schema`
   (`crates/catalog-adapter/src/bundle/tests.rs:400`). `SequenceFloor::parse`
   rechaza `record.format_version != 1` (`crates/catalog-adapter/src/bundle/floor.rs:68`),
   probado explícitamente por
   `mismatch_corruption_noncanonical_and_oversized_records_fail_closed`
   (`crates/catalog-adapter/src/bundle/floor.rs:149-186`, línea 171:
   `"format_version":1` → `"format_version":2` produce `Err(InvalidState)`).
3. **Cambio desde `v0.3.0`:** `unchanged`. `git log v0.3.0..HEAD` sobre
   `crates/catalog-adapter/src/lib.rs` y `crates/mcp-server/src/catalog_sync.rs`
   no devuelve commits; `crates/catalog-adapter/src/bundle.rs` y
   `crates/catalog-adapter/src/bundle/floor.rs` tampoco están en el diff
   agregado (`git diff v0.3.0 HEAD --stat` sobre el árbol completo de
   `crates/catalog-adapter` no lista ninguno de los dos).
4. **Estado que no debe retroceder — el único floor genuino encontrado en
   este censo:** `SequenceFloor.permits(bundle)` exige
   `bundle.manifest().sequence > self.sequence() || self.matches(bundle)`
   (`crates/catalog-adapter/src/bundle/floor.rs:106-108`) — un bundle con
   secuencia menor o igual (salvo coincidencia exacta) es rechazado. Probado
   explícitamente: `let newer = SequenceFloor::new(&verify(&two, &trust)?);
   assert!(!newer.permits(&bundle))` en
   `original_cli_wire_format_and_rotation_identity_are_preserved`
   (`crates/catalog-adapter/src/bundle/floor.rs:144-145`). El floor se
   persiste por separado de la ruta activa (`catalog_cli.rs` reserva el floor
   antes de comprometer el bundle, línea 274-286) y se compara antes de
   cualquier `catalog import` (`crates/mcp-server/src/catalog_cli.rs:274`).
   El floor mismo se guarda con permisos/atomicidad propios en
   `crates/project-adapter/src/catalog_store.rs:486-510` (`read_floor`/
   `reserve_floor`, nunca promovido desde staging —
   `floor_is_independent_bounded_durable_and_never_promoted_from_staging`,
   `crates/project-adapter/tests/catalog_store.rs:432`). Downgrade de binario
   `0.8.0 → 0.3.0`: el formato del floor y del bundle no cambiaron desde
   `v0.3.0` (punto 3), así que un binario `0.3.0` aplicaría exactamente el
   mismo `permits()` — el floor sigue protegiendo contra reimportar una
   secuencia antigua aunque el binario sea más viejo.
5. **Recuperación:** mismo `catalog import`/`sync`/`status` del formato 5; el
   mensaje de `catalog_cli.rs:325` cubre explícitamente el caso "estado de
   floor inválido o ausente".
6. **Tests existentes:** `original_cli_wire_format_and_rotation_identity_are_preserved`
   (`crates/catalog-adapter/src/bundle/floor.rs:118`, cubre además rotación
   de clave de confianza manteniendo el floor);
   `mismatch_corruption_noncanonical_and_oversized_records_fail_closed`
   (línea 150); `rejects_noncanonical_signed_manifest_and_unknown_schema`
   (`crates/catalog-adapter/src/bundle/tests.rs:400`);
   `floor_is_independent_bounded_durable_and_never_promoted_from_staging`
   (`crates/project-adapter/tests/catalog_store.rs:432`);
   `floor_record_and_staging_reject_links_and_oversized_bytes`
   (`crates/project-adapter/tests/catalog_store.rs:474`, symlink/hardlink en
   el propio floor). No se encontró un test que ejercite el *downgrade de
   binario* explícitamente (solo el mecanismo de versión/floor que lo haría
   seguro por construcción, según el análisis del punto 4).

---

## 7. LanceDB — índice semántico derivado

1. **Marcador de versión:** `IndexMetadata.schema_version: u32`
   (`crates/domain/src/semantic.rs:70-71`), más la `dimension: u32` del
   modelo de embedding incrustado, acotada `1..=1024`
   (`crates/domain/src/semantic.rs:31,49`). Nótese que `IndexMetadata` no
   deriva `Deserialize` (`crates/domain/src/semantic.rs:68-69`, solo
   `Serialize`) — el dominio no expone una ruta de deserialización directa
   para metadata no confiable; la construcción/verificación vive en el
   adapter (`crates/semantic-adapter/src/index.rs`).
2. **Versión desconocida/futura:** fail-closed en la construcción/apertura
   del índice: `LanceMemoryIndex::build`/`restore` rechaza `metadata.schema_version
   != 1` (`crates/semantic-adapter/src/index.rs:44`, junto a
   `metadata.model.validate().is_err()` en la misma condición) antes de
   escribir el schema Lance. Probado por una mutación explícita
   `wrong_schema.schema_version = 2` (`crates/semantic-adapter/src/index.rs:312`).
3. **Cambio desde `v0.3.0`:** `unchanged`. `git log v0.3.0..HEAD` sobre
   `crates/domain/src/semantic.rs`, `crates/mcp-server/src/catalog_semantic.rs`
   y el directorio `crates/semantic-adapter` completo no devuelve ningún
   commit.
4. **Estado que no debe retroceder:** `floor_or_trust_state: false` —
   correcto, porque ADR-017 declara este índice **nunca autoritativo**: se
   reconstruye siempre desde el catálogo SQLite vía `catalog rebuild-index`
   (`crates/mcp-server/src/catalog_cli.rs:40`) o al importar un bundle
   (`crates/mcp-server/src/catalog_semantic.rs:76-96`,
   `validate_imported_index`, que reconstruye la metadata esperada — modelo +
   `snapshot_fingerprint` — y la compara contra el índice embebido en el
   bundle antes de aceptarlo). Un mismatch de modelo/dimensión no corrompe
   nada: simplemente invalida el índice y exige reconstrucción, exactamente
   el comportamiento que pide el plan ("modelo/dimensión incompatible
   invalida/reconstruye índice").
5. **Recuperación:** `catalog rebuild-index` (`crates/mcp-server/src/catalog_cli.rs:40`,
   implementado en `crates/mcp-server/src/catalog_semantic.rs:25-67`,
   `rebuild()`) — reconstrucción completa desde cero, no hay "reparación
   parcial" ni necesidad de ella dado que el índice es siempre derivado.
6. **Tests existentes:** `crates/semantic-adapter/src/index.rs` inline
   (línea 291: dimensiones fuera de rango `[0, 1025]`; línea 312: schema
   version desconocida; línea 444:
   `accepts_maximum_rows_dimension_and_name_length`); `crates/semantic-adapter/tests/local.rs`
   (integración con el runtime offline real). No se encontró un test
   específico de "upgrade/rollback" porque conceptualmente no aplica: no hay
   estado que sobreviva a un downgrade, solo una reconstrucción.

---

## 8. Política de seguridad (entradas de `cargo-deny`)

1. **Marcador de versión:** `SecurityPolicyDocument.schema_version: u32`
   (`crates/domain/src/security.rs:150-153`).
2. **Versión desconocida/futura:** fail-closed antes de cualquier ejecución:
   `SecurityPolicy::new` rechaza `document.schema_version != 1` junto con los
   límites de tamaño de listas (`crates/domain/src/security.rs:211-227`,
   condición línea 221). El comentario de la línea 212-214 es explícito:
   "This constructor enforces the domain constraints before any execution.
   No caller can deserialize directly into a validated policy."
3. **Cambio desde `v0.3.0`:** `unchanged`. `git log v0.3.0..HEAD` sobre
   `crates/domain/src/security.rs` y
   `crates/execution-adapter/src/security_native.rs` no devuelve commits.
4. **Estado que no debe retroceder:** `floor_or_trust_state: false` — es una
   entrada de host (`--security-policy PATH` + `--security-policy-sha256`),
   re-suministrada en cada arranque como el host config; no hay estado
   persistido por el servidor que pueda retroceder entre versiones de
   binario, solo el contenido del propio fichero que el operador controla.
5. **Recuperación:** N/A como "recuperación de estado corrupto" (es input,
   no estado); la integridad es la verificación de
   `--security-policy-sha256` en el host config, no una operación de CLI de
   este formato en particular.
6. **Tests existentes:** `malformed_global_duplicate_and_expired_policies_are_rejected_before_execution`
   (`crates/domain/src/security.rs:581-594`, mutación
   `d.schema_version = 2` en línea 583, entre otras trece mutaciones
   hostiles); `crates/mcp-server/tests/inspection_runtime/security.rs`
   (contrato end-to-end).

---

## 9. Snapshot de advisories RustSec

1. **Marcador de versión:** ninguno propio del proyecto. El formato es
   externo, propiedad de los crates `rustsec`/`cargo-lock` (pins `=0.32.0`/
   `=11.0.1`, tabla de dependencias de
   [`m8-stabilization.md`](../../roadmap/m8-stabilization.md#L169)). La única
   marca local es de **frescura**, no de formato: una `FreshnessPolicy`
   nombrada `"rustsec-host-snapshot-v1"` (`crates/catalog-adapter/src/audit.rs:242-249`)
   que evalúa antigüedad, no estructura de bytes. Integridad de contenido:
   pin SHA-256 host-suministrado (`--rustsec-sha256`), citado por ADR-038
   como "Integrity is relative to the host-expected checksum, not publisher
   authentication" (según el censo; no se encontró el texto de ADR-038 en
   este código, es cita del propio censo).
2. **Versión desconocida/futura:** no hay chequeo de "versión de formato"
   porque no hay campo de versión que leer; lo que sí existe es rechazo por
   antigüedad/estado desconocido: `AuditIssue::SnapshotUnknownAge` cuando los
   tiempos de procedencia son desconocidos o `evidence.freshness().state()`
   es `Unknown` (`crates/catalog-adapter/src/audit.rs:251-254`), y
   `AuditIssue::SnapshotStale` cuando no está `Fresh` (línea 255-256) — no
   son fail-closed en el sentido de "rechazo total": producen un
   `AuditIssue` en el `AuditObservation` (severidad degradada), no un error
   duro que bloquee el arranque. Esto es una asimetría real frente a los
   formatos 5/6/7/8 (que sí fallan cerrado con error duro ante
   incompatibilidad).
3. **Cambio desde `v0.3.0`:** `unchanged` a nivel de código propio.
   `git log v0.3.0..HEAD` sobre `crates/catalog-adapter/src/audit` (directorio
   completo) no devuelve commits. El formato externo (parsing de
   `rustsec`/`cargo-lock`) podría cambiar con una subida de esas crates,
   fuera del control de este repo — exactamente el riesgo que la propia
   tabla de dependencias de M8 ya señala como "recalificar catálogo M1 y
   migraciones".
4. **Estado que no debe retroceder:** el censo marca
   `floor_or_trust_state: true` para este formato, pero no se encontró en el
   código un mecanismo de floor/antirollback análogo al de §6 — solo el pin
   SHA-256 por invocación. La clasificación del censo aquí describe
   participación en la cadena de confianza/integridad (el snapshot debe
   coincidir con el hash que el host declaró esperar), no una secuencia
   monótona persistida que un downgrade de binario pudiera hacer retroceder.
   Vale la pena que D12 lo distinga explícitamente de §6 al documentar la
   decisión, para no asumir que existe un floor donde solo hay un pin de
   integridad puntual.
5. **Recuperación:** ninguna dedicada encontrada; el snapshot se
   re-suministra por flag de host en cada arranque, igual que la política de
   seguridad.
6. **Tests existentes:** `crates/catalog-adapter/src/audit/tests.rs`
   (`integrity_identity_duplicates_collection_and_transport_schema_fail_closed`,
   línea 187); `crates/mcp-server/tests/inspection_runtime/audit.rs`. No se
   encontró un test específico de "snapshot con SHA-256 que no coincide" a
   nivel de este módulo (la verificación del pin ocurre en la capa de host
   config, fuera del alcance inspeccionado por este worker).

---

## 10. Cargo vendor tree (datos de dependencias offline)

1. **Marcador de versión:** ninguno a nivel de esquema; el formato del árbol
   vendor es el propio de Cargo, no versionado por este proyecto
   (`crates/domain/src/vendor_capture.rs:1-9`, comentario explícito: "This is
   deliberately **not** `SourceBundle`... `SourceBundle`... shares it"). La
   integridad es un único pin SHA-256 de árbol completo
   (`--cargo-vendor-tree-sha256`), verificado por
   `VendorCaptureVerifier`/`cargo-vendor inspect`
   (`crates/mcp-server/src/cargo_vendor_cli.rs:13-25`, subcomandos
   `inspect`/`capture`). Los límites de tamaño/entradas/profundidad de ADR-078
   (`VENDOR_CAPTURE_MAX_TOTAL_BYTES`, `_MAX_ENTRIES`, `_MAX_FILE_BYTES`,
   `_MAX_PATH_BYTES`, `_MAX_DEPTH`; `crates/domain/src/vendor_capture.rs:35-59`)
   son cuotas fijas, no marcadores de versión.
2. **Versión desconocida/futura:** no aplica un chequeo de "versión"; el
   fail-closed es por límite excedido o alfabeto de path no permitido,
   aplicado *durante* la lectura, no después (comentario de diseño explícito
   en `crates/domain/src/vendor_capture.rs:13-15`: "an entry that would
   cross the per-file or the total ceiling is refused at the point that
   crosses it").
3. **Cambio desde `v0.3.0`:** `unchanged`. `git log v0.3.0..HEAD` sobre
   `crates/project-adapter/src/cargo_vendor.rs`,
   `crates/project-adapter/src/vendor_capture.rs`,
   `crates/domain/src/vendor_capture.rs` y
   `crates/mcp-server/src/cargo_vendor_cli.rs` no devuelve ningún commit.
4. **Estado que no debe retroceder:** igual que en el formato 9, el censo
   marca `floor_or_trust_state: true`, pero el mecanismo real encontrado es
   un pin de integridad de árbol completo (recalculado en cada `capture`),
   no una secuencia que pueda "retroceder" en el sentido de §6 — un
   re-capture reemplaza el hash esperado, no lo compara contra un mínimo
   anterior. D12 debería confirmar si esto es intencional (un vendor tree
   nuevo es simplemente otro insumo del host, sin historial que proteger) o
   si se espera algún tipo de continuidad.
5. **Recuperación:** `cargo-vendor inspect`/`capture`
   (`crates/mcp-server/src/cargo_vendor_cli.rs:35-45`) — un vendor tree
   dañado o desactualizado se resuelve con una nueva captura completa, no con
   una reparación incremental.
6. **Tests existentes:** `crates/project-adapter/tests/cargo_vendor.rs`
   (incluye `sha256_file`, línea 222, para verificación de integridad);
   pruebas de captura bajo `crates/project-adapter/src` (vendor_capture,
   citadas por el censo como "out of this worker's scope to inspect
   further" en `scripts/` — no verificado por este worker tampoco, fuera del
   árbol `crates/`).

---

## (a) Tabla resumen — 10 formatos

| # | Formato | Marcador de versión | Fail-closed ante versión desconocida | Cambio desde v0.3.0 | No-retroceso (floor/trust) | Recuperación | Tests de versión/crash/corrupción |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | Host config (flags) | Ninguno (stateless) | Sí — imagen no aprobada rechaza el arranque (`host_config.rs:177-184`) | changed-compatible (aditivo: flag+imagen M6) | N/A | N/A | `cli.rs::rust_runtime_options_require_a_complete_unique_approved_tuple` |
| 2 | M2 journal/receipts | Campo `format` con magia versionada (`mutation.rs:637-655`) | Sí, antes de tocar workspace/store (`mutation.rs:791-851`) | changed-compatible (nueva variante `analyzer_action_apply`) | No hay floor de secuencia; protección es por fase/formato, no proactiva a nivel binario | `mutation list/prune` + `recover: bool` explícito en el tool MCP | Extensos: legacy v1→v2, formato desconocido, ENOSPC, symlink, crash-swap (`native_mutation.rs`) |
| 3 | M3 quality artifact store | `format_version` (envelope) + `PayloadFormatVersion` (por tipo) + watermark propio | Sí, en descriptor y watermark (`quality_artifact.rs:494-554,288-292`) | unchanged | No floor; invariante es reloj monótono (`QualityClockWatermark`) | `quality-artifacts recover/prune` | `an_unknown_record_version_fails_closed_and_is_never_reinterpreted`, watermark, cuotas |
| 4 | M6 analyzer state | Ninguno (sin estado runtime); recibo de calibración `...v1` | N/A (transitorio) | N/A — código enteramente nuevo desde v0.3.0 | N/A | N/A | `analyzer_runtime.rs`, receipt schema test (`#[ignore]`) |
| 5 | Catálogo SQLite | `PRAGMA user_version` + `SnapshotManifest.format_version` | Sí, doble capa (`lib.rs:117-118,274-276`) | unchanged | Integridad estructural (ledger de migraciones); floor de secuencia vive en §6 | `catalog status/import/sync/rebuild-index` | `migration_is_atomic_idempotent_and_rejects_unknown_schema`, 12 mutaciones hostiles |
| 6 | Catálogo — bundle de confianza | `BundleManifest.{snapshot,catalog}_format_version` + `SequenceFloor.format_version` | Sí, en ambos (`bundle.rs:200`, `floor.rs:68`) | unchanged | **Único floor de secuencia genuino del censo** (`floor.rs:106-108`), probado con rechazo de secuencia menor | Igual que §5; mensaje de guía explícito en CLI (`catalog_cli.rs:325`) | `floor.rs` tests dedicados + symlink/hardlink en floor (`catalog_store.rs:474`) |
| 7 | LanceDB índice semántico | `IndexMetadata.schema_version` + `dimension` acotada | Sí, en build/restore (`index.rs:44`) | unchanged | Nunca autoritativo (ADR-017): mismatch → reconstrucción, no corrupción | `catalog rebuild-index` (reconstrucción completa) | `index.rs` mutación `schema_version=2`, límites de dimensión |
| 8 | Política de seguridad | `SecurityPolicyDocument.schema_version` | Sí, antes de ejecución (`security.rs:221`) | unchanged | N/A (input de host, no estado del servidor) | N/A (input re-suministrado) | `malformed_global_duplicate_and_expired_policies_are_rejected_before_execution` |
| 9 | Snapshot RustSec | Ninguno propio; formato externo + pin SHA-256 + política de frescura nombrada | Parcial — frescura degrada (`AuditIssue`), no bloquea con error duro | unchanged (código propio); riesgo real es la crate externa | Censo dice `true`; código muestra solo pin de integridad puntual, no floor monótono | N/A | `integrity_identity_duplicates_collection_and_transport_schema_fail_closed` |
| 10 | Cargo vendor tree | Ninguno; formato de Cargo + pin SHA-256 de árbol completo | Por cuota/alfabeto, no por versión | unchanged | Censo dice `true`; código muestra solo pin de integridad, no floor monótono | `cargo-vendor inspect/capture` (recaptura completa) | `cargo_vendor.rs::sha256_file` + suite de captura |

---

## (b) Formatos que realmente requieren migración 0.3.0 → 0.8.0

**Ninguno.** Confirmado por comparación directa de bytes/estructura
(`git diff v0.3.0 HEAD` por archivo, §Método arriba): de los 10 formatos, 8
son byte-idénticos en su código fuente de lectura/escritura desde `v0.3.0`
(formatos 3, 5, 6, 7, 8, 9, 10, y el formato 4 no existía). Los únicos dos
formatos con commits en el rango (host config y journal M2) cambiaron de
forma estrictamente aditiva: una nueva flag/imagen aprobada y una nueva
variante de un enum cerrado, ninguna de las dos altera la interpretación de
bytes ya persistidos por `v0.3.0`. Esto coincide con la expectativa explícita
del plan ("esperado: pocos o ninguno — no lo fuerces") y con la cláusula de
M8-03: **no procede añadir un CLI de migración/validación nuevo**, porque el
inventario no identifica un formato real que migrar. La versión de paquete
`0.8.0` (`Cargo.toml`) es un cambio de número de contrato/SemVer (M8-02), no
un cambio de formato en disco.

## (c) Huecos para D12, en orden de riesgo

1. **[S] M2: no hay bloqueo proactivo de downgrade a nivel de proceso cuando
   hay un journal pendiente.** Hoy la protección es enteramente reactiva: un
   binario más viejo que encuentra un registro que no entiende falla al leer
   *ese registro* (`RecoveryRequired`), pero nada impide que el proceso
   arranque normalmente y siga operando sobre otros journals mientras uno
   pendiente queda invisible hasta que alguien intenta tocarlo. Coste bajo
   porque el mecanismo de detección (`operation_kind`/formato desconocido) ya
   existe; lo que falta es un chequeo de arranque tipo "listar journals no en
   fase terminal antes de aceptar tráfico" o documentar explícitamente por
   qué la protección reactiva basta (podría bastar, dado que cada tool
   journalizado ya pasa por `decode_envelope` antes de cualquier efecto).
   Propuesta mínima: un test de aceptación que arranque un binario "viejo"
   simulado (branch de `operation_kind` recortada) contra un store con un
   journal `analyzer_action_apply` pendiente y confirme que ninguna operación
   nueva sobre *otros* journals se ve afectada, más una nota en D12 sobre si
   el diseño reactivo es la decisión final.
2. **[S] Formatos 9 y 10 (RustSec, vendor tree): el censo los marca
   `floor_or_trust_state: true` pero el código no expone un floor monótono
   real, solo un pin de integridad puntual re-suministrado por el host en
   cada invocación.** Esto no es un defecto de seguridad — un pin de
   integridad por invocación es coherente con "el host controla estos
   insumos externos, no hay estado de servidor que proteger"— pero la
   etiqueta del censo puede inducir a D12 a diseñar protecciones de
   antirollback que no aplican a estos dos formatos. Coste: aclarar en el
   brief de D12 que estos dos son "input de host con integridad puntual", no
   "estado con floor", y opcionalmente corregir la nota del campo en una
   pasada de censo posterior.
3. **[M] Formato 9 (RustSec): la respuesta a un snapshot "unknown age" o
   "stale" es una degradación (`AuditIssue`) en el `AuditObservation`, no un
   error duro que bloquee — asimétrico frente a los formatos 5/6/7/8, que
   fallan cerrado con error.** Si D12 decide que un snapshot RustSec con edad
   desconocida debe bloquear en vez de degradar (más cerca del "fail-closed
   antes de cualquier efecto" que pide el plan), el cambio toca
   `crates/catalog-adapter/src/audit.rs:251-261` y su contrato de tool
   (`rust.dependencies.audit`), lo que probablemente exige explicar la
   decisión también en D11 (¿es un cambio de contrato 0.8.0?). Coste medio:
   no es solo código, es una decisión de producto sobre severidad.
4. **[M] Formato 3 (quality artifacts): el precedente histórico
   `BenchmarkDatasetV2` reemplazó `V1` sin dejar lector legado, a diferencia
   del patrón v1→v2 del journal M2 que sí preserva lectura legada y
   migración explícita.** Hoy esto es seguro porque los artifacts expiran
   por TTL, pero si D12 quiere una política uniforme de compatibilidad entre
   formatos, vale la pena decidir explícitamente si "romper y dejar expirar"
   es aceptable para formatos con TTL corto, mientras que formatos sin TTL
   (journal, catálogo) exigen lector legado. Coste: documentar la política
   diferenciada en el brief de D12; no requiere cambio de código si se acepta
   la asimetría actual.
5. **[L] Ningún formato tiene un test que ejercite explícitamente "downgrade
   de binario completo" (arrancar un binario `v0.3.0` real contra estado
   escrito por `0.8.0`), solo los equivalentes por versión-de-formato
   desconocida dentro del binario actual.** Dado que ningún formato cambió
   de bytes entre `v0.3.0` y HEAD (§b), este hueco es hoy de bajo impacto
   práctico, pero D12/M8-03 exige explícitamente "Upgrade N-1→N y rollback
   N→N-1 se prueban para cada formato declarado compatible" — un test real
   de rollback (instalar binario `v0.3.0`, ejecutar contra estado `0.8.0`, o
   viceversa) todavía no existe para ninguno de los 10 formatos. Coste alto
   porque requiere infraestructura de dos binarios instalados
   simultáneamente en el gate nativo, no solo fixtures de bytes.
6. **[S] No se encontró ningún fixture de "permisos revocados a mitad de
   operación"** (solo tests de modo/permisos fijos al escribir) para ninguno
   de los 10 formatos. Coste bajo: extender un test existente (p. ej. sobre
   el journal M2) para revocar permisos de escritura entre `preview` y
   `commit` y confirmar fail-closed.

## (d) Fixtures del plan — existentes vs. faltantes

| Fixture (lista del plan) | Estado | Evidencia |
| --- | --- | --- |
| Versión desconocida | **Existe**, múltiple | `native_mutation.rs:454` (M2); `catalog-adapter/src/tests.rs:63-64` (SQLite); `quality_artifact_store.rs:1712,1778` (M3); `semantic-adapter/src/index.rs:312` (LanceDB); `security.rs:583` (política); `bundle/floor.rs:171` y `bundle/tests.rs:400` (bundle de confianza) |
| Config antigua | **Existe**, un caso | `native_mutation.rs:353` (journal v1 legado, M2). No se encontró para host config/política de seguridad porque son inputs stateless sin "versión antigua" que conservar |
| Migración interrumpida en cada fase | **Parcial** | `native_mutation.rs:353-366` interrumpe en `CommitCheckpoint::Staged` antes de convertir v1→v2; `native_mutation.rs:1015` (`killed_process_rolls_forward_known_format_prefix`) cubre kill de proceso. No se encontró cobertura de interrupción en *cada* fase declarada del ciclo de vida del journal (`Prepared`/`Scratch`/`Staged`/`Applying`/`Published`) de forma sistemática, solo puntos específicos |
| Disk full | **Existe** | `native_mutation.rs:1635` (`post_swap_durability_enospc_recovers_known_after_generation`, M2); `quality_artifact_store.rs:911` (cuota, M3) |
| Permisos revocados | **No encontrado** | Solo tests de modo/permisos fijos al escribir (`catalog_store.rs:239`, `mutation_store.rs:810`); ninguno revoca permisos a mitad de una operación en curso |
| Backup corrupto | **No aplica tal cual — no existe concepto de "backup" separado en ningún formato.** El más cercano es "store/journal corrupto" (`native_mutation.rs:1715` `corrupt_store_is_quarantined...`; `quality_artifact_store.rs:607` `corrupt_or_unknown_objects_are_quarantined...`), que cubre corrupción del estado activo, no de una copia de respaldo | — |
| Inode/ancestor swapped | **Existe** | `native_mutation.rs:1164` (`manifest_and_lock_first_swap_crash_rolls_forward_only_known_bytes`), `native_mutation.rs:2128` (`proven_swap_failure_returns_cause_and_records_abort`), `native_mutation.rs:2283` (`post_swap_interrupt_and_late_external_write_are_classified`) |
| Symlink/reparse/hardlink | **Existe**, extenso | `native_mutation.rs:2744` (symlink parent); `filesystem.rs:211,226,247` (symlink/hardlink denegados); `catalog_store.rs:474` (floor rechaza links) |
| Crash tras commit antes de respuesta | **Existe (parcial, vía kill de proceso)** | `native_mutation.rs:1015` (`killed_process_rolls_forward_known_format_prefix`); no se encontró un fixture que simule específicamente "el commit ya se escribió en disco pero el proceso muere antes de enviar la respuesta MCP al cliente" a nivel de protocolo (el caso está cubierto a nivel de journal/filesystem, no a nivel de la capa `stdio`) |
| Reintento idempotente | **Existe**, extenso | `mutation.rs` (dominio: `IdempotencyKey`), tests en `inspection_runtime/mutation.rs`, `inspection_runtime/mutation_concurrency.rs`, `inspection_runtime/terminal_plan.rs`, `catalog-adapter/src/tests.rs` (`migration_is_atomic_idempotent...`) |

---

## Verificación

`python3 -B scripts/docs-hygiene.py links-check` pendiente de ejecutar por el
worker antes de handoff (ver sección de resultado). Ningún archivo fuera de
`docs/validation/M8/03-formats-analysis.md` fue escrito por este worker.
