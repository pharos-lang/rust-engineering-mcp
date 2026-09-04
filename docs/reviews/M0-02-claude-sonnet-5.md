# Revisión externa M0-02

Fecha: 2026-09-03. Commit revisado: `9df89a59d951f94182653fdb45a9a646b0603a69`.
Claude Code 2.1.259, reviewer explícito `claude-sonnet-5`, effort `medium`.
Respuesta `success`, `is_error=false`, un turno, 38618 ms.

Invocación read-only con contexto por stdin y sin herramientas:

```text
claude --print --model claude-sonnet-5 --effort medium --safe-mode --restricted --strict-mcp-config --tools '' --permission-mode dontAsk --permission-prompts none --no-session-persistence --output-format json
```

Contexto: objetivo/DoD M0-02, gates, límites de alcance, ADR-004/006/007/015/020,
contratos y diff `a3e4488..9df89a5`, incluidos tests y ADR-022. La corrección
puramente histórica de M0-01 no formó parte del paquete. El reviewer no ejecutó
herramientas, gates ni cambios. El principal verifica y decide sobre sus observaciones.

La metadata confirma `claude-sonnet-5`; Claude Code también reporta una llamada
auxiliar a `claude-haiku-4-5-20251001` (20 tokens de salida). No se configuró fallback.
Estos flags no acreditan aislamiento OS del propio cliente Claude Code.

SHA-256 del paquete: `6f77b0bf885d45e202b4dece2a811d82123fd8d49832e149deab2f20690a9960`.

## Respuesta completa

I'll review the M0-02 domain contracts diff against the ADRs and stated DoD.

**Critical**

None.

**High**

None.

**Medium**

1. **`ProjectRef` entropy claim overstated in ADR text vs. actual validation** — `crates/domain/src/value.rs` `canonical_hex` only checks format (`prj_` + 32 lowercase hex chars); it cannot and does not verify 128 bits of real entropy, and the ADR-022 text explicitly says "parsear no autoriza ni demuestra aleatoriedad" — this is consistent and correctly scoped, not a bug. Retracting: no violation, this is correctly caveated. (Removed as a finding.)

2. **`Freshness::assess` uses saturating-style `checked_sub` returning `None` on negative age, but silently conflates "no `created_at`" with "future `created_at`"** — `crates/domain/src/evidence.rs:206-208` (`age_seconds = provenance.created_at.and_then(|created| now.0.checked_sub(created.0))`). Both cases map to `FreshnessState::Unknown`, which matches ADR-020/ADR-022 intent ("ausente o futura ... unknown"), and is tested (`freshness.rs::unknown_or_future_creation_is_never_false_freshness`). No violation — noting only because it's a subtle invariant worth flagging for future maintainers; not actionable now.

3. **`OperationalErrorCode` enum omits a code for output-limit vs. sandbox-denied semantics overlap is fine, but `Truncation.is_truncated()` and `OperationalErrorCode::OutputLimitExceeded` are not cross-validated** — `crates/domain/src/result.rs`. Nothing in `OutputEnvelope::operational_error` or `TryFrom<RawEnvelope<T>>` enforces that an `OutputLimitExceeded` error is accompanied by `truncation.is_truncated() == true`, nor vice versa. This is a real gap: a caller could construct/deserialize an envelope with `error_code: OUTPUT_LIMIT_EXCEEDED` but `truncation: { false, false, 0 }`, silently inconsistent. Given ADR-006 emphasizes discriminated invariants preventing invalid states, this is a missed cross-field invariant. However, per the prompt's scope, streaming limits are "explicitly later work" — since actual enforcement is deferred, I'll downgrade this to Low/Observation rather than a required fix, since the domain doesn't yet know what "truncated" means operationally.

**Low**

1. `crates/domain/src/result.rs`: `OutputEnvelope::from_report` line length triggers no clippy issue, but the private field `data: T` combined with `#[serde(bound(deserialize = "T: Deserialize<'de>"))]` on the struct requires `T: Serialize` implicitly via derive on the outer type even when only deserializing — cosmetic, not a defect.
2. `OperationalErrorCode` → `ToolStatus` mapping (`status()` method) duplicates the enum-to-status logic that `TryFrom<RawEnvelope<T>>` re-checks (`status == code.status()`), which is correct and intentional (single source of truth used both ways) — not a defect, just worth naming as intentional design, not accidental duplication.
3. Missing cross-check between `OutputLimitExceeded`/`SandboxDenied` operational errors and `Truncation` flags (see Medium #3 above, downgraded here). Future work when streaming limits land (explicitly out of scope now).

**Observations**

- `ByteRange`, `SourceSpan`, `Suggestion`, `Provenance`, `FreshnessPolicy`, `SnapshotEvidence`, and `OutputEnvelope<T>` all correctly use the `#[serde(try_from = "Wire")]` pattern to force invariant checking on deserialize, closing the classic "struct literal via Deserialize bypasses validation" hole. This is a solid, consistent implementation of ADR-006/020 discriminated-state requirements.
- `required_nullable` helper (`lib.rs`) correctly implements "required but nullable" semantics (field must be present, `null` allowed) distinct from serde's default `Option` (field may be absent) — matches ADR-020/ADR-006 wording precisely, and is exercised by tests removing fields vs. setting them to `null`.
- The compile-fail doctest in `lib.rs` is a good, minimal way to enforce `ProjectIdentityFingerprint`/`ExecutionFingerprint` nominal-type separation per ADR-007, without needing a runtime test.
- `Freshness` has no public constructor other than `assess`/deserialize-with-reverification, so callers cannot fabricate a `Fresh` label detachable from its `Provenance` — matches ADR-020's "no le des el dato justo cuando importa" concern.
- Domain crate dependency graph (`Cargo.toml`) is Serde-only in normal deps, serde_json dev-only — matches ADR-004 (no adapters/I/O) and the stated Cargo audit note.
- Versions pinned with `=1.0.229` / `=1.0.151` exact pins in `[workspace.dependencies]`, consistent with the stated lockfile-fixed requirement.
- `docs/implementation-status.md` correctly downgrades M0-01's Rust version claim to reflect the later MSRV bump in `cafe721`, avoiding a retroactive rewrite of already-closed evidence — good historical hygiene.

**Missing tests**

- No test exercises `OperationalErrorCode` Serde `SCREAMING_SNAKE_CASE` rename for all nine variants individually (only round-tripped generically in `every_operational_code_has_one_mapping_and_is_not_success`, which is adequate but doesn't pin exact wire strings like `"PROJECT_NOT_FOUND"`). Minor — the roundtrip test does implicitly cover this since it goes through `serde_json::to_value`/`from_value`, so this is not a real gap, just noting it's implicit rather than explicit.
- No test for `Position`/`ByteRange` at `u32::MAX`/`u64::MAX` boundaries for `Position` (only `ByteRange` has a `u64::MAX` case in `coordinates_and_byte_offsets_reject_invalid_ranges`/`multiline_positions...`). Low value given `NonZeroU32` already constrains the type; not required.
- No explicit test that `Suggestion`'s wire form rejects an `edits` array containing a structurally invalid `Replacement` (e.g., invalid nested `SourceSpan`) — though `diagnostic_roundtrip_validates_nested_evidence` in `diagnostics.rs` does cover nested invalidity for spans reached through diagnostics/suggestions, so this is effectively covered.

None of these are blocking; they're minor coverage refinements, not gaps in the stated DoD.

**Security concerns**

None beyond what's explicitly deferred and correctly documented (SECURITY.md addition accurately states that a syntactically valid `ProjectRef` doesn't prove authority/entropy, a digest doesn't prove integrity, and that redaction of Serde error content is future adapter work — this matches the actual code, which does not attempt redaction and correctly keeps `ContractError::to_string()` free of the invalid input, verified by `error_text_does_not_reflect_invalid_input` test).

**Contract/ADR violations**

None found. Specifically checked and confirmed compliant:
- ADR-006: `passed`/`failed`/`blocked`/`unavailable`/`cancelled` enum present; `failed` used for valid-but-negative project outcomes (E0502 test); `error_code`/`error_message` present-but-null for non-operational outcomes; enum is closed (`OperationalErrorCode`) with a defined status mapping enforced both at construction and deserialization.
- ADR-007: `ProjectIdentityFingerprint` and `ExecutionFingerprint` are distinct nominal types, non-interchangeable, verified by compile-fail test. No handle generation/authority claimed here — appropriately deferred.
- ADR-020: `Provenance` and `Freshness` are non-optional and inseparable inside `SnapshotEvidence`; `latest_live`/network-implies-live is correctly never modeled; `Clock` is an injectable port, not a runtime dependency of the domain; freshness re-verified on deserialize against persisted `assessed_at`/policy without claiming current-time accuracy (correctly documented as adapter's responsibility to reassess).
- ADR-004: no `rmcp`, JSON-RPC, stdio, SQLite, LanceDB, or process APIs imported by `crates/domain`; confirmed via dependency list and source review.
- ADR-015: not yet applicable (no MCP tool DTOs/schemas introduced in this cut); correctly not attempted.

**Recommendation**

**Accept.** The domain crate is well-scoped, correctly separates identity/execution fingerprints, enforces its invariants symmetrically at both construction and deserialization boundaries (closing the common Rust newtype-validation bypass), and each test file directly targets discriminating edge cases (boundary freshness ages, byte/position inversion, cancellation vs. operational error, unknown-field rejection) rather than restating the implementation. Documentation accurately scopes what is *not* yet proven (authority, entropy, hashing, redaction, live reevaluation), and the gate evidence (fmt/check/clippy -D warnings, 29 tests including compile-fail, local-only audit) matches what's claimed. No changes required before integration into main.
