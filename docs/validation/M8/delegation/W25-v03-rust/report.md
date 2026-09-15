# W25 — informe (Claude Sonnet 5, worker de implementación Rust)

Invocación: `claude -p --model sonnet --effort high`. Sin subagentes, sin
Docker, sin commit. Foreground únicamente (`cargo` vía Bash con `timeout`).

## Task

Corregir en Rust los findings D-1..D-5 y S-1..S-4 de
`docs/validation/M8/delegation/V03-review-m8-03-04-05/disposition.md` y
`report.md` §1/§3, y alinear ADR-088 §3/§8 con lo probado.

## Result por finding

- **D-1 (P1) — `downgrade_blocked` falso negativo.** `MutationRecordSummary`
  gana `kind: MutationKind` (campo aditivo, `crates/domain/src/mutation.rs`),
  rellenado en `scan_store` (`crates/project-adapter/src/filesystem/macos/mutation.rs`)
  desde el envelope ya validado (`operation_kind` allí nunca falla, porque
  `decode_envelope` ya lo comprobó para cada body antes de llegar a ese
  punto). En `doctor.rs`: `mutation_journals` publica `kinds{kind →
  {pending, terminal}}` por los seis kinds, `downgrade_blocking_kinds`, y
  `downgrade_blocked = pending > 0 || existe un kind ∉ KINDS_KNOWN_TO_0_3_0`
  (`KINDS_KNOWN_TO_0_3_0` = los cinco kinds M2 reales del enum:
  `ManifestPatch`/`manifest_patch`, `FormatApply`/`format_apply`,
  `FixApply`/`fix_apply`, `DependencyAdd`/`dependency_add`,
  `DependencyRemove`/`dependency_remove`). `DOWNGRADE_NOTE` corregida a
  «recover, complete or prune (`mutation prune`) with 0.8.0 before installing
  an older binary». Test nuevo
  `mutation_journals_downgrade_blocked_for_a_committed_kind_unknown_to_0_3_0`
  (`tests/doctor.rs`): journal Committed `analyzer_action_apply` →
  `downgrade_blocked: true`, `terminal: 1`, `pending: 0`,
  `downgrade_blocking_kinds: ["analyzer_action_apply"]`,
  `kinds.analyzer_action_apply: {pending: 0, terminal: 1}`.
- **D-2 (P3) — test de kind desconocido reproducía en realidad un checksum
  roto.** Renombrado a
  `mutation_journals_reports_a_broken_checksum_as_unknown_format_without_panicking`
  con comentario explícito de por qué reproduce esa rama, no la de kind.
  Añadido `mutation_journals_reports_unrecognized_format_marker_and_garbage_bytes_as_unknown_format`
  (marcador `-journal-v2`→`-journal-v9`, que llega directamente a la rama
  `_ => Err(RecoveryRequired)` de `decode_envelope`; y bytes basura no-JSON)
  y `mutation_journals_reports_a_foreign_file_as_unknown_format`
  (`.DS_Store`), los tres con conteos exactos:
  `unknown_format==1, pending==0, terminal==0, downgrade_blocked==true`,
  notas presentes.
- **D-3 (P3) — `Busy` reportado como «could not be read».** Nueva rama
  `Err(MutationError::Busy)` en `mutation_journals` con nota propia
  («journal busy: a mutation is in progress; rerun doctor»). Test nuevo
  `mutation_journals_reports_a_busy_note_when_the_store_lock_is_held`: toma
  el `flock` exclusivo de `mutation-store.lock` desde el proceso de test
  (vía `rustix::fs::flock`, ya dev-dependency de `mcp-server`) antes de
  invocar `doctor`, que observa el mismo `WOULDBLOCK → Busy` que dejaría una
  mutación concurrente de `serve`.
- **D-4 (P3) — nota `unknown_format` mezclaba casos.** Nota reformulada:
  «At least one journal entry is unreadable or unknown (unrecognized
  envelope format, a broken checksum, a foreign file or an operation kind
  this binary cannot interpret); the store fails closed before any
  per-record detail is available.»
- **D-5 (P3) — `mutation_journals` exigía la tupla Docker completa.**
  `doctor::parse` (`crates/mcp-server/src/doctor.rs`) intenta primero
  `host_config::parse` sin cambios; si falla, extrae un `--state-root`
  solitario (exactamente una ocurrencia, ruta absoluta) de los flags de
  host, reintenta `host_config::parse` sin ese par, y si eso tiene éxito
  guarda la ruta en `Invocation::journal_state_root`, usada como
  alternativa a `host.rust.state_root` al calcular `mutation_journals`.
  `host_config.rs` no se tocó (cambio confinado a `doctor.rs`, D-5 lo
  permitía condicionalmente). Test nuevo
  `mutation_journals_accepts_state_root_alone_without_the_full_docker_tuple`
  (incluye el caso negativo: ruta relativa sigue rechazándose, exit 2).
- **S-1 (P2 contrato) — plantilla `rust-quality-artifact` no RFC6570.**
  `…?offset={n}&length={n}` expandía `offset`/`length` desde la misma
  variable `{n}`. Nueva forma
  `…/{quality_job_id_or_artifact_id}{?offset,length}` en
  `stdio::list_resource_templates` y
  `capability_document::resource_templates()`.
- **S-2 (P3) — «misma fuente de verdad» exagerado.** Constante compartida
  `resources::QUALITY_TEMPLATE_SUFFIX` (`stdio/resources.rs`), usada por
  ambos lados; `capability_document::RESOURCE_TEMPLATES` (const `&[&str]`)
  pasa a ser la función `resource_templates() -> [String; 2]` construida
  desde `resources::PREFIX`/`QUALITY_PREFIX`/`QUALITY_TEMPLATE_SUFFIX`. Test
  nuevo en `tests/protocol.rs`,
  `resource_templates_wire_list_matches_the_contract_document`: arranca un
  server real, pide `resources/templates/list`, ejecuta `contract --json`
  como proceso aparte, y exige igualdad exacta `uri_template` ↔
  `uriTemplate` para las dos entradas, en el mismo orden.
- **S-3 (P3) — test de listas cubría una sola versión legacy.**
  `resources_templates_and_prompts_lists_carry_ttl_and_cache_scope` pasa a
  iterar `std::iter::once(VERSION).chain(LEGACY)` como sus vecinos
  (`modern_wire = version == VERSION`), cubriendo ahora las cuatro
  versiones legacy además de la vigente.
- **S-4 — CHANGELOG 0.8.0.** Tres viñetas nuevas: listas wire
  (`ttlMs`/`cacheScope` en `resources/templates/list`/`prompts/list`),
  `doctor.mutation_journals` por kind (D-1), y la plantilla RFC6570 (S-1).
- **ADR-088 §3** — reescrito: recuentos `kinds{kind → pending, terminal}` y
  la regla de bloqueo exacta (`pending > 0 ∨ kind ∉` los cinco M2), con la
  aclaración explícita de que un `analyzer_action_apply` **committed**
  bloquea igual que uno pendiente. **§8** — la redacción del fixture de
  permisos revocados pasa de «entre `preview` y `commit`» a «tras
  `Published`, antes de que el commit complete», alineada con
  `revoked_destination_permissions_leave_a_recoverable_non_terminal_journal`
  (`crates/project-adapter/tests/support/native_mutation.rs`, no tocado por
  W25 — solo se citó como evidencia).
- **`docs/tools.md`** — secciones `doctor` y Resources dinámicas
  actualizadas: `--state-root` solo para `mutation_journals` (D-5),
  `kinds`/`downgrade_blocking_kinds`/regla de bloqueo (D-1), notas `Busy`
  (D-3) y «unreadable or unknown» (D-4), plantilla RFC6570 exacta (S-1).

## Files changed

- `crates/domain/src/mutation.rs` — campo aditivo `kind` en
  `MutationRecordSummary`.
- `crates/project-adapter/src/filesystem/macos/mutation.rs` — rellena
  `kind` en `scan_store`.
- `crates/mcp-server/src/doctor.rs` — `Invocation::journal_state_root` y
  fallback de parseo (D-5); `MutationJournalsReport` con `kinds`,
  `downgrade_blocking_kinds`; `KINDS_KNOWN_TO_0_3_0`; notas `Busy`/D-4;
  `DOWNGRADE_NOTE` corregida.
- `crates/mcp-server/src/stdio.rs` — plantilla RFC6570 vía
  `resources::QUALITY_TEMPLATE_SUFFIX`.
- `crates/mcp-server/src/stdio/resources.rs` — nueva constante
  `QUALITY_TEMPLATE_SUFFIX`.
- `crates/mcp-server/src/stdio/capability_document.rs` — `RESOURCE_TEMPLATES`
  (const) → `resource_templates()` (función, misma fuente compartida); test
  actualizado.
- `crates/mcp-server/tests/doctor.rs` — 6 tests nuevos/renombrados (D-1,
  D-2×2, D-3, D-5), helpers compartidos (`write_fixture_project`,
  `commit_manifest_patch_journal`, `journal_args_state_root_only`); JSON
  esperado de la sección vacía actualizado con los campos nuevos.
- `crates/mcp-server/tests/protocol.rs` — plantilla RFC6570 en el JSON
  esperado; `resources_templates_and_prompts_lists_carry_ttl_and_cache_scope`
  itera `LEGACY` (S-3); test nuevo
  `resource_templates_wire_list_matches_the_contract_document` (S-2).
- `crates/mcp-server/tests/snapshots/doctor-report.json` — sin cambios de
  W25 (el `"mutation_journals": null` ya estaba en el árbol al empezar).
- `docs/tools.md`, `docs/adr/ADR-088-migration-rollback-policy.md`,
  `CHANGELOG.md` — documentación (ver arriba).

No se tocó `crates/mcp-server/src/host_config.rs` (D-5 se resolvió por
completo dentro de `doctor.rs`), ni `crates/mcp-server/tests/cli.rs`, ni
ningún archivo fuera de la lista permitida.

## Tests

Todo en foreground, `--locked --offline`:

- `cargo fmt --all -- --check` → limpio (workspace completo).
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`
  → limpio.
- `cargo test -p rust-engineering-mcp --locked --offline --test doctor --test cli --test protocol`
  → **86 passed** (15 cli + 10 doctor + 61 protocol), 0 failed.
- `cargo test -p rust-engineering-mcp --locked --offline --bin rust-engineering-mcp capability_document`
  → 9 passed (incluye el test S-2 renombrado).
- `cargo test -p rust-engineering-project --lib --locked --offline` → 80
  passed, 2 ignored (mediciones explícitas de release), 0 failed; incluye
  `revoked_destination_permissions_leave_a_recoverable_non_terminal_journal`.
- `cargo test -p rust-engineering-domain --locked --offline` → 213 (lib) +
  57 (13 binarios de integración) + 1 doctest = 271 passed, 0 failed.
- `python3 -B scripts/contract-freeze.py verify docs/validation/M8/freeze-0.8.0.json --strict`
  → `{"status": "passed", "class_changed": [], "format_errors": [],
  "preview_changed": [], "stable_changed": []}`.
- `git status --short crates/mcp-server/tests/snapshots` → solo
  `doctor-report.json` (diff preexistente de W14, `"mutation_journals":
  null` añadido antes de W25; ninguno de los 36 `*-tool.json` cambió).

No commit realizado.
