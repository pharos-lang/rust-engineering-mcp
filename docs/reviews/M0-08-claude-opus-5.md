# Revisión independiente M0-08

Claude Code2.1.259, claude-opus-5 High, read-only con tools deshabilitadas,
safe/restricted y sin permisos/persistencia. Telemetry registra Haiku auxiliar.

# M0-08 Review — bounded SQLite catalog snapshots

**Scope reviewed:** ADR-026, `crates/domain/src/catalog.rs`, `crates/application/src/catalog.rs`, `crates/catalog-adapter/**` (as pasted). Read-only; no tools run.

**Could not verify (files not supplied):** `domain/src/lib.rs` and `application/src/lib.rs` re-exports (the adapter uses `use rust_engineering_domain::*`, so a glob `pub use` could widen the public surface unnoticed), `Cargo.toml` feature selection for rusqlite (`bundled`/`serialize`/`limits`/`hooks`), `Provenance`/`CatalogFingerprint`/`SnapshotEvidence` internals. Line numbers are not cited because I reviewed pasted text; every finding is anchored to `file → symbol` plus the exact quoted line.

## Verdict on the stated gates

| Gate | Result |
|---|---|
| domain/application free of I/O, DB, SDK | **Pass** — `domain/catalog.rs` imports only `serde` + `std`; `application/catalog.rs` imports only domain; the port lives in application, the `Connection` only in the adapter. |
| Snapshot bytes only; no caller paths or caller SQL | **Pass** — `build`/`open`/`activate` take `&[u8]` + manifest; no `Connection::open(path)` anywhere; every SQL string is a literal. One private function (`apply_migration`) takes SQL as a parameter but is `fn`, not `pub`, and is only ever called with `SCHEMA` (see L1). Caller text reaches SQL only as a bound parameter. |
| Manifest trust framing (integrity ≠ authenticity) | **Pass in the ADR**, weak in the code docs (L5). |
| Deserialize hardening | **Pass** — cheap header/size/digest gates precede `deserialize`; `integrity_check` runs *before* `sqlite_schema` is read (correct order: never traverse an unverified b-tree to validate it); DEFENSIVE on, TRUSTED_SCHEMA off, views/triggers off, `ATTACH` limit 0, exact-DDL pin, FK check, FTS external-content check, record re-validation, then a fresh read-only deserialization. `bytes[18]/[19]` indexing is guarded by the short-circuited `bytes.len() < 100`. No TOCTOU: `&[u8]` cannot change between validation and use. |
| Migration atomicity | **Pass** — DDL + ledger row + `user_version` in one IMMEDIATE transaction (`user_version` is journaled), proven by `migration_is_atomic_idempotent_and_rejects_unknown_schema`. |
| Activation / rollback | **Pass** — `&mut self` serializes activation against readers at compile time (`Connection` is `Send + !Sync`); `open` cross-checks the image's `sequence` against the manifest, so the `expected.sequence <= self.metadata.sequence` gate is not manifest-spoofable; failure leaves `*self` untouched. |
| No new MCP tools | **Confirmed** — public surface is `build/open/activate/rebuild`, `Snapshot`, `SnapshotManifest`, `MAX_SNAPSHOT_BYTES`, `search_catalog`, `CatalogRepository`, and the domain records. |

## Defects

**H1 — Versions are ordered lexicographically, not by semver; the round-trip identity claim is false in general.**
`records.rs → insert`: `versions.sort_by(|a, b| a.version.cmp(&b.version));` and `records.rs → get`: `... ORDER BY version LIMIT 65`. Both are byte-order, so `["1.9.0", "1.10.0"]` round-trips as `["1.10.0", "1.9.0"]`. `sqlite_roundtrip_preserves_all_normalized_facts_and_provenance` asserts `inspect(name) == input`, and passes only because the fixture (`1.2.3`, `2.0.0-beta.1+fixture`) happens to agree in both orders. Two consequences: the "preserves all normalized facts" invariant is not what the test proves, and any consumer taking `versions.last()` as "latest" gets the wrong version (a wrong answer in an advisory tool, e.g. skipping a fixed release). Rated High because the fix changes insertion order → image bytes → fingerprint, so it is far cheaper now than after M1 pins the format. Fix: sort by `semver::Version` in `insert`, add an explicit `ordinal INTEGER NOT NULL` column, and retrieve with `ORDER BY ordinal` so retrieval order is stored rather than re-derived; or document `versions` as an unordered set and drop the ordered equality assertion.

**M1 — Feature names of real crates are rejected.**
`records.rs → validate` reuses `valid_name` for features: `if !valid_name(feature) || !features.insert(feature)`. `valid_name` allows only `[A-Za-z0-9_-]`, so `dep:tokio`, `serde/derive`, and dotted feature names all yield `InvalidInput`. These forms are ubiquitous on crates.io, so M1 ingestion would reject a large fraction of the registry. The same rule is correct for `crates.name` and `dependencies.name`. Fix: a separate `valid_feature` predicate allowing `:`, `/`, `+`, `.` with the same length/NUL rules (advisory IDs are fine under `valid_name`).

**M2 — Enforced capacity contradicts the budget declared in ADR-026.**
ADR: "1,000 crates, 64 versions per crate". `records.rs → validate`: `entries += 1 + version.features.len() + version.dependencies.len() + version.advisories.len(); if entries > 100_000 { return Err(CatalogError::Budget) }`. A full 1000 × 64 catalog is 64,000 base entries; with a modest 3 deps + 2 features per version it is ~384,000 → hard `Budget` rejection. The real capacity is roughly 15k–20k versions with realistic metadata, not 64k. Fix: align the ADR number with the enforced entry budget (or raise the entry budget deliberately) — but see M3, which the same change makes worse.

**M3 — `lexical` has a result budget, not a payload budget.**
`lib.rs → CatalogRepository::lexical` returns fully expanded `CrateRecord`s (`records::get` per hit: versions + features + dependencies + advisories). A 50-result page is bounded only by the catalog-wide entry cap, so one search can materialize a large fraction of the entire catalog (~100k rows, several MB of `String`s, ~9.6k statement preparations) before M1 serializes it into an MCP response. The ADR's "50 results" reads as a bound that it is not. Fix: give search a projection (name, description, latest version, counts) and keep full expansion for `inspect`; this also decouples M2's cap from response size.

**M4 — Capacity is unproven, and the build budget is wall-clock.**
`records.rs → insert` calls `connection.execute(...)` per row (fresh prepare each time — not `prepare_cached`), inside a single `budget()` window set in `build` before the transaction: `progress_handler(1000, ... callbacks > 10_000 || started.elapsed() > Duration::from_secs(2))`. At the documented scale that is ~100k compilations plus the FTS `'rebuild'` under ~10M VDBE steps / 2 s wall clock. No test exercises anything beyond 3 crates, so it is unknown whether a max-size catalog builds or opens without self-interrupting, and a loaded CI runner or a debug build can trip the 2 s branch non-deterministically. Fix: hoist prepared statements out of the loops (`prepare_cached`), add a capacity test at the declared budget for `build`/`open`/`rebuild`, and prefer the op-count branch over wall clock (or make the duration a constant sized against the measured worst case).

**L1 — `apply_migration` ignores its own argument when writing the ledger.**
`lib.rs → apply_migration(connection, migration: &str)` executes `migration` but records `fingerprint(SCHEMA.as_bytes())` and hardcodes `VALUES(1,?1)` / `user_version, 1`. Currently unreachable (only `SCHEMA` is passed in non-test code), but the parameter and the recorded identity are decoupled — a v2 migration would be stamped with v1's checksum. Fix: drop the parameter, or hash the parameter and pass the version explicitly.

**L2 — `PRAGMA max_page_count` does not survive `deserialize`.**
`lib.rs → empty()` sets `page_size=4096; max_page_count=16384` (= 64 MiB), but that is a pager/BtShared setting; `deserialize_read_exact` replaces the pager, so the staging and runtime databases likely run without it. `db_config` flags, `foreign_keys` and `temp_store` are connection-level and do survive (the runtime test confirms DEFENSIVE/TRUSTED_SCHEMA/temp_store). The 64 MiB ceiling therefore rests solely on the `bytes.len() > MAX_SNAPSHOT_BYTES` check. Fix: re-apply and re-verify `max_page_count` after each deserialize, and assert `PRAGMA page_count * page_size == bytes.len()`.

**L3 — Error classification collapses distinct failures into `Integrity`.**
`lib.rs → sql()` maps everything outside `OperationInterrupted | DiskFull | TooBig` to `CatalogError::Integrity`. `SQLITE_NOMEM`, `SQLITE_CANTOPEN` and `QueryReturnedNoRows` therefore report as snapshot corruption, and any FTS5 parse error arising from caller text in `lexical` becomes `Integrity` rather than `InvalidInput`. Given M0-07 centralized outcome mapping, this will surface as "your catalog is corrupt" for caller or host faults. Fix: map `NoMemory`/`CantOpen` → `Unavailable`, and MATCH-statement errors on caller text → `InvalidInput`.

**L4 — `records::all` admits 4× the rows that `validate` will accept.**
`records.rs → all` checks each of `versions/features/dependencies/advisories` against `count > 100_000` independently, then materializes everything; `validate` afterwards rejects at 100_000 *combined*. A hostile image can force up to ~400k rows into Rust `String`s before rejection. Fix: sum the four counts against the same combined budget before enumerating.

**L5 — The authenticity caveat is on the wrong type.**
The `/// Integrity relative to the host's expected manifest, not a publisher signature.` comment sits on `Snapshot`. `SnapshotManifest` — which derives `Deserialize` and is exactly what M1 will read from disk — and `open` carry no such warning. Fix: move/duplicate onto `SnapshotManifest` and `open`, stating that the manifest must arrive over a trusted channel and that `open` provides no publisher authentication.

**L6 — `inspect` and `lexical` disagree on name identity.**
`records.rs → valid_name` + `SELECT id FROM crates WHERE name=?1` is byte-exact, while `crate_fts` uses `unicode61` (case-folding). `lexical("Serde")` can return a crate that `inspect("Serde")` reports as `None`; likewise `serde-json` vs `serde_json`. Fix: document exact-match semantics, or store a normalized lookup key.

**L7 — The image is not self-describing.**
`snapshots.provenance` holds `serde_json::to_string(&provenance)` — an unversioned projection of a domain type — and `format_version` lives only in the external manifest. The fingerprint binds manifest↔bytes so mispairing is impossible, but a future `Provenance` field rename turns old images into `InvalidSnapshot` with no way to distinguish "corrupt" from "older format". Fix: store `format_version` in the `snapshots` row (schema-checksum change, so do it before M1 pins the format).

**L8 — Missing/empty snapshot rows classify as `Integrity`.**
`lib.rs → open`: `query_row("SELECT sequence,provenance FROM snapshots WHERE id=1", ...)` on a zero-row table yields `QueryReturnedNoRows` → `Integrity`, not `InvalidSnapshot`. Separately, `build(seq, prov, &[])` produces a valid empty catalog — confirm that is intended, and reject it explicitly if not.

*Info (no action required):* positional `INSERT ... VALUES` without column lists in `snapshots`/`migrations`/`features`/`dependencies`/`advisories` is safe only because the DDL is checksum-pinned; `optional()` rejects `Some("")` so callers must normalize empty repository/license strings to `None`; `description` permits newlines and control characters (a rendering concern for M1, not a storage one); the fingerprint comparison is not constant-time, which is irrelevant for an integrity check against a local expectation but would matter the day it becomes a MAC.

## Future scope (correctly deferred, not defects)

- Publisher signatures / trust roots, durable storage, archive transport, rollback administration, CLI — M1-10 per the ADR.
- **Sequence monotonicity is process-local.** `activate`'s guard lives in `self.metadata`; a restarted process can be fed an older sequence. Correctly implied by "durable host storage … still M1-10", but worth stating explicitly in the ADR consequences so it is not assumed solved.
- **Peak memory during `open`/`activate` is ~2–3× the image** (caller bytes + staging copy + runtime copy; `rebuild` similarly holds source connection + new connection + `serialize` output + `to_vec`). At the 64 MiB ceiling that is ~256 MiB transient during activation. The ADR already disclaims total allocator RAM; adding the multiplier makes the disclaimer checkable.
- **`sqlite3_deserialize` on attacker-influenced bytes remains the largest single attack surface**, since SQLite parses the header and schema before any of the defenses can run. The layering here is about as good as it gets without a signature gate — which is precisely why M1 should make signature verification a precondition of calling `open`, not a parallel check.
- No `inspect` use case exists in the application layer (only the port method). Consistent with "no new MCP tools"; noting it so it is not mistaken for an omission.

## Test gaps

1. No capacity test at the declared budget (M2/M4) — the largest fixture is 3 crates.
2. No test where semver order diverges from lexicographic order (H1 would fail such a test).
3. No `MAX_SNAPSHOT_BYTES`-exact boundary case (only `MAX + 1`).
4. `validate_ledger` is tested for a mutated checksum but not for an *extra* ledger row (`INSERT INTO migrations VALUES(2,...)`), which the `entries != [(1, ...)]` comparison should reject.
5. No test that a zero-row `snapshots` table or a zero-crate catalog is handled as intended (L8).
6. `rebuild` is tested only from a trusted self-built image, never from a hostile-but-schema-valid one.
7. `budget()` is only exercised via a synthetic `progress_handler(1, Some(|| true))`; the real 10k-callback / 2 s policy is never exercised on `build`, `open` or `lexical`.

The hostile-image matrix (`rehashed_hostile_images_fail_validation_beyond_digest`) is the strongest part of the suite — in particular `UPDATE crates SET description=...` empirically proves the FTS external-content check is active, which is the non-obvious half of `'integrity-check', 1`.

## Disposición del Principal Engineer

- H1: versiones ordenadas por SemVer al construir y leer. Search devuelve
  CrateSummary con latest_known explícito (máxima versión conocida, incluida yanked
  o prerelease; no significa latest estable/live). Prueba1.9.0 vs1.10.0 y yanked.
  No ordinal redundante: el orden deriva de versiones validadas y se aplica al leer.
- M1: corregido soporte de feature keys Unicode XID, `_`, dígitos iniciales y
  `-+.` posteriores con unicode-ident fijado1.0.24. La revisión mezcla nombres de
  features y valores de activación: `dep:tokio`/`serde/derive` son valores, no keys.
  Este campo conserva nombres; no representa el grafo de activaciones. Fuente
  oficial: https://doc.rust-lang.org/cargo/reference/features.html#the-features-section.
- M2: ADR registra presupuesto agregado100000 y límites simultáneos, no capacidad
  garantizada del producto de máximos por dimensión.
- M3: búsqueda proyecta únicamente nombre, descripción, latest_known y contador;
  no expande features/deps/advisories. Payload JSON de candidatos limitado128KiB:
  exceso devuelve Budget, sin truncamiento oculto. Test muestra50 resultados fallan
  por tamaño y la misma búsqueda con limit1 pasa. Inspect preserva facts completos.
- M4: cache de statements limitada16, budget30s/10Mopcodes. Capacity test real de
  1000crates/10000versions/60000entradas ejecuta build/open/rebuild/search. Suite
  de diez tests de integración pasó en1.96s en el host, sin prometer igual latencia
  en otros hosts. Test del handler usa ahora la policy real y supera10Mopcodes.
- L1: apply_v1_migration identifica explícitamente la versión que instala y hashea
  el SQL recibido; v2 requiere una migración nueva. Rollback e idempotencia probados.
- L2: reestablece/verifica max_page_count después de cada deserialize; exige
  page_count*page_size==bytes.len(). Rechaza páginas anexas rehasheadas; test confirma
  max_page_count16384 en conexión readonly con páginas4096.
- L3: OutOfMemory/CannotOpen/IoFailure/Busy/Locked → Unavailable. NoRows →
  InvalidSnapshot. Los términos FTS están completamente entrecomillados, por lo
  que el corpus de caracteres especiales no genera sintaxis SQL/FTS del caller;
  fallos restantes del motor conservan Integrity, sin atribuirlos al texto del user.
- L4: suma filas de las cuatro tablas antes de materializar facts y aplica el mismo
  máximo agregado. Los límites por versión se conservan también al leer.
- L5: caveat de autenticidad añadido a SnapshotManifest y open. M1 importer debe
  autenticar el manifest antes de llamar a esta API.
- L6: inspect es identidad exacta; FTS usa tokenización/case folding. El caller debe
  usar el nombre canónico devuelto por search. No se normalizan facts silenciosamente.
- L7: format_version1 se guarda también en el singleton de snapshot. user_version
  identifica schema/provenance serializado; versión desconocida devuelve
  UnsupportedSchema antes de comparar DDL. Cambios futuros exigen migración/versionado.
- L8: catálogo vacío es válido (búsqueda vacía), pero singleton ausente es
  InvalidSnapshot. Tests de ambos casos y ledger extra incluidos.

El principal revisó re-exports, Cargo features/lock y tipos de evidencia omitidos
por el paquete externo; solo tipos de dominio entran al core, no SQLite/SDK/I/O.
Monotonicidad es por instancia; durable antirollback pertenece M1. Pico de memoria
puede incluir varias copias de imagen más records: no se anuncia hard cap de RAM.
No bloqueantes pendientes aceptados en M0-08. Las correcciones se validan por el
principal y el gate, no se atribuyen a una segunda revisión externa inexistente.
