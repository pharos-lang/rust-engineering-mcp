# Revisión independiente M0-09

Claude Code2.1.259, claude-opus-5 High, read-only safe/restricted sin tools, MCP ni persistencia. Telemetry registra Haiku auxiliar.

## Scope verdict

Boundaries hold. `crates/domain/src/semantic.rs` and `crates/application/src/semantic.rs` import only `crate::*`, `rust_engineering_domain::*`, `serde` and `std` — no IO, no `rusqlite`, no `rmcp`. `EmbeddingProvider`/`SemanticIndex` are internal ports with no tool surface. `VerifiedE5Bundle` exposes only `verify([Vec<u8>; 5])` with `pub(crate)` fields — no path, URL or caller-supplied hash. `LanceMemoryIndex` has no URI parameter, retains `_connection`, and `rebuild` assigns only after `build` returns. Merge is append-only under `merged.len() < limit`, and every returned fact comes from `repository.summary`. Accepting the pre-scoped premises (ORT host trust, local-verification provenance, lexical-first as foundation, calibration ≠ sandbox tier), the design is sound.

Below are defects I would fix before the gate is declared, not disagreements with those premises.

## A. Likely build-blocking (will surface in the in-progress build)

**High — `crates/semantic-adapter/src/index.rs` `mod tests`, `Cargo.toml` `[workspace.dependencies] tokio`.** Five `#[tokio::test]` attributes require the `macros` feature; the workspace pin is `features = ["rt","time","io-util"]` and the dev-dep is a bare `tokio.workspace = true`. Nothing in `-p rust-engineering-semantic`'s graph unifies `macros` in. `cargo check --all-targets` in `test-semantic.py` fails here.

**High — `crates/semantic-adapter/src/embedding.rs` `OfflineRuntime::initialize`.** `static OWNED: OnceLock<bool>` is initialized from `ort::init().with_telemetry(false).with_logger(...).commit()`, which returns `ort::Result<_>` in 2.0.0-rc.13, not `bool`. Needs `.is_ok()`. Independently: collapsing the error to a bool discards the only diagnostic for a failed ORT environment, and the doc comment "later calls reuse our own configuration" is true only if nothing else committed an ORT environment first — the `OnceLock` guards our re-entry, not third-party initialization.

## B. Current API / architecture defects

**Medium-High — `crates/application/src/semantic.rs` `search_hybrid`.** `provider.identity().provenance` is read *after* `index.candidates(..).await`, so `&mut dyn EmbeddingProvider` is held across the await point and the returned future is `!Send`. `SemanticIndex::candidates` is explicitly `+ Send`-bounded, so the asymmetry is unintentional. The ADR's "bounded blocking worker for CPU inference" cannot be built on a `!Send` future; this forces a signature change in M1. Fix now: `Send` supertrait on `EmbeddingProvider`, or hoist the identity/provenance clone before the await.

**Medium — `search_hybrid`, candidate rehydration.** `Ok(None) | Err(_) => return Ok(fallback(SemanticError::InvalidIndex))` maps an authoritative-store failure to a semantic-index defect and returns a successful lexical page. The same `repository` error in `search_catalog` is a hard `CatalogError`. A SQLite integrity failure mid-page becomes silent degradation with a misattributed reason. Separate `Ok(None)` (index/catalog divergence) from `Err(e)` (propagate).

**Medium — `crates/domain/src/semantic.rs` `EmbeddingIdentity`, `IndexMetadata`.** Both derive `Deserialize` with all-public fields and no `try_from = "Raw…"` guard, unlike `Provenance`, `FreshnessPolicy` and `SnapshotEvidence` in the same crate. `dimension: 0`, `dimension: u32::MAX`, `max_tokens: 0` and unbounded `model`/`revision`/`runtime` strings all deserialize successfully today. Validation currently lives only in `LanceMemoryIndex::build` and `search_hybrid`. This is a present API defect (the type is publicly deserializable now) and it is the exact surface M1's persisted-index import would land on.

**Medium — `search_hybrid` vs `crates/semantic-adapter/src/index.rs` `MAX_LIMIT`.** `query.limit()` is passed through unclamped; the adapter rejects `limit == 0 || limit > 50` with `InvalidInput`. If `CatalogQuery` admits a limit above 50, every such hybrid query silently degrades to lexical with a reason (`invalid_input`) that blames the caller's input rather than the index bound. Either clamp in the application layer or pin the two bounds with a test.

**Medium — `search_hybrid`, merge loop.** When the lexical page already holds `limit` results, the function still runs full E5 inference, an index query and up to `limit` `summary()` round-trips, discards every candidate via `merged.len() < query.limit()`, and still reports `effective_mode: Hybrid`. Confirmed by `semantic_additions_cannot_exceed_query_limit_or_payload_budget` (`limit = 1`, result is `["alpha"]` only). Clients are told "hybrid" for a page with zero semantic contribution. Short-circuit before inference, or make `effective_mode` reflect actual contribution.

**Medium — `crates/semantic-adapter/src/embedding.rs` `LocalEmbeddingProvider::embed`.** `text.chars().any(char::is_control)` rejects `\n` and `\t`. Correct and valuable for `embed_query` (it blocks `"foo\npassage: bar"` prefix injection into `format!("{prefix}: {text}")`), but crate descriptions routinely contain newlines, so `embed_passage` will hard-fail `InvalidInput` on real catalog rows during M1 index construction. Keep the strict filter for queries; normalize whitespace for passages.

**Medium — `crates/semantic-adapter/src/index.rs` `LanceMemoryIndex::build`.** `metadata` and `rows` are independent parameters; nothing binds `metadata.model` to the provider that actually produced the vectors. `search_hybrid`'s identity check compares index metadata against the provider, but that metadata is a caller assertion. `tests/local.rs` gets it right by convention (`model: provider.identity().clone()`), unenforced by type. Deriving `metadata.model` from a `&dyn EmbeddingProvider` inside the build path closes this cheaply and is materially harder to retrofit after M1 wiring.

**Medium (conditional) — `search_hybrid`, `results: CatalogPage { crates: merged, ..lexical }`.** Struct-update carries every non-`crates` field from the lexical page. `evidence` and `snapshot_fingerprint` are correct; if `CatalogPage` also holds a match count or truncation flag, it now describes the lexical set while `crates` contains semantic additions. Worth one direct check against the `CatalogPage` definition.

**Low-Medium — `LocalEmbeddingProvider::embed`.** `validate_embedding(...)?` propagates `SemanticError::InvalidIndex` out of an *embedding provider*, so a misbehaving model reports as an index defect in `HybridSearch.fallback`. Map to `Inference`. Related: `identity.normalization = L2` is declared but never configured — it relies on fastembed 6.0.2's implicit post-processing. Fails closed via `validate_embedding`, but the declared field is unenforced.

**Low — `crates/domain/src/semantic.rs` `HybridSearch`.** `effective_mode` and `fallback` are independently public and can contradict (`Hybrid` + `Some(reason)`). Every other evidence type in the crate is constructed only through an assessment. An enum would carry the invariant.

**Low — `crates/semantic-adapter/Cargo.toml`.** `serde` and `serde_json` are unconditional dependencies with no use in `lib.rs`, `model.rs`, `embedding.rs` or `index.rs`. Drop them.

## C. Supply-chain and gate defects

**Medium — `scripts/verify-vendor.py`.** Every check is a bare `assert`. Under `python -O` or `PYTHONOPTIMIZE=1` the entire vendor verification passes unconditionally, including the published-archive SHA256. `test-semantic.py` already uses `sys.exit`; match it.

**Medium — `scripts/verify-vendor.py`, `scripts/test-semantic.py`.** Neither asserts the actual objective of ADR-027: that `lance-testing`, `pprof`, `inferno` and `quick-xml` are absent from `Cargo.lock` / the built graph. The scripts verify the *shape* of the manifest delta, not its *outcome*. Add a lock/`cargo tree` assertion. Relatedly, no advisory check (`cargo audit`/`cargo deny`) runs anywhere, though the ADR reasons explicitly about RUSTSEC-2024-0436 and refuses to authorize an ignore — that refusal is currently unenforced.

**Medium — `scripts/test-semantic.py`, ORT identity.** Only `native/'libonnxruntime.a'` is hashed, but `ORT_LIB_LOCATION=str(native)` hands the whole directory to ort's build script. Any additional archive linked from that directory is unverified. Hash the directory listing plus contents, not one filename.

**Medium — `scripts/test-semantic.py`.** The gate builds and tests only `-p rust-engineering-semantic --features local`. The ADR's claim that "core-only builds exercise absence/fallback" is not exercised by any script; `crates/catalog-adapter/tests/hybrid.rs` covers the fallback logic but is never invoked here.

**Low-Medium — `scripts/test-semantic.py`, spill check.** `if nonexistent.exists()` proves only that `$TMPDIR` was not created. The profile is `(allow default) (deny network*)`, so writes to `/tmp`, `~/.cache` or the CWD are permitted and undetected, yet the printed banner reads "no temp spill directory". Narrow the printed claim or deny `file-write*` outside a scratch root.

**Low-Medium — `Cargo.toml` / ADR-027, tinyvec.** The 1.12.0 pin exists only in `Cargo.lock` with the rationale only in prose. `--locked --offline` protects CI; a routine `cargo update` silently moves to 1.13.0 and reproduces the alloc-macro failure. A `[workspace.dependencies]` entry or a lock-guard check makes the constraint self-documenting.

**Low — orchestration.** `verify-vendor.py` is not invoked by `test-semantic.py`; nothing ties vendor integrity to the semantic gate.

## D. M1 integration gaps (not current defects)

- No application-layer index-construction use case exists. `EmbeddingProvider::embed_passage` is dead code from the application's perspective; the security-critical binding of snapshot fingerprint → embeddings → `IndexMetadata` lives only in `tests/local.rs`. That orchestration must move into `crates/application` in M1 — and doing so is what resolves finding B/`LanceMemoryIndex::build` above.
- `HybridSearch` records index metadata and model evidence at page level, with no per-crate attribution of *why* a result was returned. Given this payload shape will front an MCP tool, adding attribution later is a breaking change.
- Blocking inference on an async executor: `tests/local.rs` calls `provider.embed_query` inside `rt.block_on`, i.e. the integration test demonstrates the pattern the ADR prohibits for M1.
- Persisted-index import remains out of scope; combined with the unvalidated `Deserialize` above, that is the first thing to harden when it lands.

## E. Test-fidelity notes

- **`crates/semantic-adapter/tests/local.rs`** — the comment "Verify each file individually … without cloning 487MB" contradicts the code: `broken` is a full five-file clone (~975MB peak alongside a 470MB ORT session), and only `files[4]` is corrupted. Fix the comment or the coverage.
- **`crates/application/src/semantic.rs` + `hybrid.rs`** — the real provider sets `created_at: None`, so production `model_evidence` is always `FreshnessState::Unknown`; `hybrid.rs` asserts `Aging`/`Stale` from a stub with `created = 950`. The production path is untested and the policy argument is inert there.
- **`hybrid.rs::snapshot_schema_and_complete_model_identity_are_checked_before_inference`** — `0..10` covers 8 of 10 `EmbeddingIdentity` fields (`pooling`/`normalization` are single-variant today). A destructuring binding would force breakage when a variant is added.
- **`local.rs`** — `assert_eq!(error.raw_os_error(), Some(1))` hardcodes EPERM and couples the gate to `sandbox-exec`, which is deprecated on macOS.

No tools, commands or edits were run; this is a read-only review of the supplied files. Findings A1 and A2 are compile-level and should be confirmed by the build already in progress rather than taken as a claim about its current result.

## Disposición del Principal Engineer

La revisión precede los fixes siguientes; no se afirma una segunda aprobación externa.

- A1: el gate local real compiló: el grafo transitivo habilitaba Tokio macros.
  Se declara ahora explícitamente en dev-dependencies para no depender de ello.
- A2: falso para la versión fijada. ort2.0.0-rc.13 environment.rs:659 declara
  `commit(self) -> bool`. No se cambia a is_ok(). La propiedad de configuración
  ajena falla cerrada; el runtime compartido dentro del proceso sigue siendo TCB.
- B1: la función no promete Send. SqliteCatalogRepository tampoco es Sync, por lo
  que agregar Send al provider no arregla el supuesto. M1 puede poseer repository,
  provider y runtime en un worker dedicado, ejecutar localmente el futuro y devolver
  un DTO Send. Sigue prohibido bloquear el reactor MCP. No es un bloqueo de M0.
- B2: corregido: `summary Err(e)` propaga CatalogError; solo ausencia produce
  InvalidIndex. Test con SQLite real detrás de wrapper de fallo primero reprodujo
  el bug y luego pasó. Ningún error autoritativo se declara fallback exitoso.
- B3: metadata no tiene ahora Deserialize; el formato de import M1 no existe aún.
  EmbeddingIdentity::validate limita strings, dimensión, tokens, threads y source
  kind; build/search lo aplican. No se abre una superficie de import sin validar.
- B4: CatalogQuery ya admite únicamente1..50. Rechazo de51/0 cubierto en catálogo;
  los dos bounds son compatibles. No se necesita clamp silencioso.
- B5: effective_mode informa el pipeline ejecutado, no una garantía de contribución
  de cada backend. Lexical-first está declarado y conserva la misma página si ya
  está llena. Optimización/ranking/calidad quedan para M1; no se cambia semántica
  de modo para esconder un backend ejecutado correctamente.
- B6: corregido. Pasajes normalizan whitespace (incluye newline/tab), con cap sobre
  bytes originales y rechazo de otros controles. Test real comprueba equivalencia.
- B8: build es una frontera interna que recibe una generación del integrador, no
  un import autenticado. Un trait provider también podría mentir; tomar &provider
  no autentica vectores. M1 debe construir la generación desde catálogo+provider,
  y verificar distribuciones persistidas. Esta obligación ya está en ADR-027.
- B9: CatalogPage solo contiene crates, snapshot_fingerprint y evidence; no existe
  count/truncated que quede obsoleto. Sin cambio.
- Provider vector inválido se mapea ahora a Inference. Fastembed normaliza en su
  postprocesamiento fijado; validate_embedding verifica la norma, con prueba real.
- HybridSearch es un DTO interno de salida y search_hybrid conserva sus invariantes;
  no es una entrada Deserialize/MCP. La frontera pública M1 tendrá sus DTOs/tests.
- Se quitaron serde/serde_json no utilizados del adapter.
- Scripts rechazan Python optimizado, verifican tinyvec1.12.0 y ausencia de paquetes
  indeseados en lock; test-semantic invoca verify-vendor. El gate local CI M0-11
  integrará audit/deny y core; M0-09 ya ejecutó audit y core por separado.
- Native directory completo verificado: contiene exactamente libonnxruntime.a y
  su hash fijado. Librerías de sistema/linker del host continúan en el TCB.
- Se estrecha el banner: solo TMPDIR configurado permanece ausente; no afirma
  ausencia de toda escritura posible en el filesystem. El profile calibra red.
- Comentario de la copia487MB corregido; prueba real nueva exige freshness Unknown
  para la procedencia local sin fecha. Fixtures con Clock siguen probando aging/stale.
- EPERM/macOS es intencional en este gate acotado; no se afirma soporte cross-platform.
- M1 conserva orchestration, per-result attribution/ranking, worker acotado y formatos
  persistidos como trabajo pendiente. Ninguna tool nueva anunciada en M0-09.
