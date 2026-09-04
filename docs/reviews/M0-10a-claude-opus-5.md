# Revisión M0-10a ArtifactStore

Claude Code2.1.259, Opus5 High, safe/restricted read-only sin tools/MCP/persistencia.

## Scope & method

Reviewed only the six inlined artifacts (ADR-028, `domain/src/artifact.rs`, `application/src/artifact.rs`, `artifact-adapter/{Cargo.toml,src/lib.rs,src/tests.rs}`). No tools, commands or edits. I did **not** read `RegistryClock`, `ProjectRef`, or the workspace lint config, so claims that depend on those are marked as verify-items rather than confirmed defects.

**No High findings.** I could not construct any path that emits an unmasked full secret match. The containment argument holds: in-loop emission is capped at `safe = pending.len() - keep` with `keep = maxlen - 1`, so any match covering an emitted position `p < safe` ends at `p + len - 1 ≤ safe - 1 + keep = pending.len() - 1` and is therefore fully present and marked before emission. Also verified: `truncated` has **no false negatives** (every clamping path in `emit` drives `output.len()` to exactly `output_bytes`, which sets the flag), `capture_with_generator` is crate-private so callers cannot inject an ID generator, `MemoryArtifactStore`/`Entry`/secrets derive no `Debug`, `buffer` is per-capture so a lying source cannot leak bytes across artifacts, and cross-owner read isolation is tested against a *live* foreign artifact (`empty_artifacts_consume_counts_and_ownership_is_not_an_oracle`).

---

## M-1 · Over-redaction at true EOF and at output-budget truncation

**File/symbol:** `crates/artifact-adapter/src/lib.rs::redact` (trailing `for secret in secrets { for len in 1..secret.len().min(tail.len()+1) ... }`); policy text in ADR-028 ("At EOF or truncation, mask possible incomplete secret prefixes conservatively").
**Severity:** Medium — current defect (content corruption; fails *safe*, no disclosure).

The tail-prefix mask fires unconditionally at the end of `redact`, but it is only *necessary* in one of the three exit paths:

| Exit path | Can unseen bytes complete a match covering an emitted byte? |
|---|---|
| `n == 0` (true EOF) | No — all input was scanned |
| `output.len() == output_bytes` (in-loop break) | No — emitted positions are `< safe`, so by the containment argument every covering match is already in `pending` and marked |
| `consumed == input_bytes` (input cap) | **Yes** — the final `emit` flushes the `keep`-byte lookahead whose continuation was never read |

In the first two cases the implementation has already *proved* there is no match, and then masks anyway.

**Repro (true EOF, no truncation at all):** `secrets = [b"abcdef"]`, `input = b"Zab"`, default-ish limits → stored content is `Z**`, not `Zab`.
**Repro (output cap):** `secrets = [b"abcdef"]`, `input = b"Zab!!!!!!!!"`, `output_bytes = 3` → `Z**`, although byte 4 is known to be `!`.

Blast radius scales with the configured patterns: with 8 secrets, any artifact whose final bytes happen to match the first 1..len-1 bytes of any pattern is silently corrupted — for text sources with secrets beginning in common characters this is not rare.

**Fix:** thread the exit reason out of the loop and apply the tail loop only when the capture stopped on `consumed == limits.input_bytes`. This is exactly length-preserving-mask-neutral and loses no security property (the table above is the whole argument). ADR-028's redaction paragraph should be narrowed in the same change from "At EOF or truncation" to "when the input byte cap truncates the source". Note this requires updating the `prefix` predicate in `streaming_matches_independent_whole_buffer_oracle_at_all_short_cuts` — see M-3.

---

## M-2 · Clock-regression poison latches permanently with no recovery path

**File/symbol:** `crates/artifact-adapter/src/lib.rs::MemoryArtifactStore::now`, field `poisoned`.
**Severity:** Medium — current defect (availability).

`now()` sets `poisoned = true` and never clears it, so *every* subsequent `capture`/`read`/`revoke_owner`/`cleanup` returns `ClockRegression` for the remaining process lifetime. The behaviour is deliberately pinned by `expiry_at_boundary_and_clock_regression_poison` (the store still fails after `clock.set(20)`).

The security goal — never serve an artifact past its TTL — is fully achieved by the `self.entries.clear()` on the same line. After the clear there is nothing to serve, and any subsequent capture derives `created/expires` from the new clock reading, so TTL remains enforced. The latch therefore buys no confidentiality or integrity, and costs a total, unrecoverable loss of the artifact subsystem triggered by an environmental event.

**Fix (concrete benefit, no type changes):** keep `entries.clear()` and the `ClockRegression` error for the operation that observed the regression, then set `last_clock = Some(now)` and drop `poisoned`, so the next operation proceeds against the new baseline. If the latch is intentional, ADR-028 must say so explicitly — it currently says only "clock regression … fail closed", which does not imply permanence — and the store needs an explicit reset entry point, because `cleanup()` (the natural recovery call) is itself poisoned.

**Verify-item:** ADR-028 mandates "a monotonic injected clock", but nothing in `MemoryArtifactStore::new` states or checks that requirement, and the store reuses `RegistryClock` (which I did not read). If any existing `RegistryClock` implementation is `SystemTime`-based, an NTP step backwards permanently disables artifacts and destroys all live entries for every owner. At minimum, document the monotonicity precondition on `MemoryArtifactStore::new`.

---

## M-3 · The "independent oracle" restates the implementation's prefix policy and never varies `input_bytes`

**File/symbol:** `crates/artifact-adapter/src/tests.rs::streaming_matches_independent_whole_buffer_oracle_at_all_short_cuts`.
**Severity:** Medium — test strength (current gap).

Two structural weaknesses:

1. **The `prefix` predicate is a transcription, not an oracle.** It computes `raw[..n].ends_with(&secret[..length]) && position >= n - length`, which is the same `ends_with`-over-a-suffix rule the implementation applies to `tail`. The `full_match` half *is* genuinely independent (whole-buffer `windows`), and that half is what makes the 22 995 combinations valuable. But the prefix half cannot fail unless the implementation's `tail` maintenance is wrong, and in particular **it structurally cannot catch M-1** — it encodes the over-masking as expected behaviour.
2. **`input_bytes` is never varied** (`limits()` fixes it at 16 384, inputs are ≤ 8 bytes). So the sweep exercises the output-cap path only. The one path where the conservative mask is *security-necessary* — input-cap truncation with unread bytes that could complete a secret — is covered solely by two hand-written vectors in `eof_and_both_budget_suffixes_are_conservative`.

**Fix:** add `input_bytes in 1..=9` to the existing sweep (cheap: it multiplies an already-fast test) and rewrite the oracle expectation in terms of *what the store could not see* — mask position `p` iff some secret matches a window covering `p` in `raw`, **or** the capture stopped on the input cap and `raw[..n]` ends with a proper prefix of some secret covering `p`. That formulation is derived from the threat model rather than from the code, and it fails on today's implementation exactly where M-1 is.

Two smaller gaps worth one vector each: no case mixes a length-1 secret with a long one (exercises the empty `1..secret.len().min(..)` range for the short pattern while `keep > 0`), and nothing asserts `truncated == false` for an input whose length exactly equals `input_bytes` — see L-1.

---

## L-1 · `truncated` false positives on exact-fit inputs and outputs

**File/symbol:** `crates/artifact-adapter/src/lib.rs::redact` (loop-head `if consumed == limits.input_bytes` and both `if output.len() == limits.output_bytes` checks); `crates/domain/src/artifact.rs::ArtifactMetadata::truncated`.
**Severity:** Low — current defect (metadata precision).

An input of exactly `input_bytes` reports `truncated = true` without probing for EOF (deliberate, per `endless_source_bounded_no_extra_probe`), and an output that exactly fills `output_bytes` reports `truncated = true` even when the source reached EOF and nothing was dropped — pinned by the last case of `eof_and_both_budget_suffixes_are_conservative` (`b"123"` with `output_bytes = 3`).

Conservative and therefore safe, but `ArtifactMetadata::truncated` carries no doc comment, so an M1 consumer that surfaces "output was truncated" to a client will lie on exact-fit artifacts. **Fix:** document the field as "true if the capture *may* have dropped bytes; exact-fit inputs and outputs report true without an EOF probe", and add the missing exact-fit test so the semantics are pinned deliberately rather than incidentally.

---

## L-2 · `redact`'s non-empty-secret precondition is enforced in a different function

**File/symbol:** `crates/artifact-adapter/src/lib.rs::redact`, first line: `secrets.iter().map(Vec::len).max().unwrap_or(1) - 1`.
**Severity:** Low — current latent defect.

`max()` returns `Some(0)` for `vec![vec![]]`, and `0 - 1` panics. The invariant that no secret is empty lives in `MemoryArtifactStore::new`, not in `redact`. `redact` is a free function that `tests.rs` already calls directly with an inline `secrets` slice that bypasses `new` entirely, so the panic is one test edit away. **Fix:** `saturating_sub(1)`, or a `debug_assert!` stating the precondition.

Related, all proved safe but inconsistent with the `checked_add` discipline used in `admit` and for TTL: `emit`'s `cap - output.len()` and `(tail.len() + count).saturating_sub(keep)`, `redact`'s `output.len() - len`, `expire`'s `before - self.entries.len()`, and `admit`'s unchecked `owner_count += 1` (bounded by `global_count ≤ 256`). Worth confirming the workspace lint set doesn't deny `clippy::arithmetic_side_effects` for this crate, since these would need annotation.

---

## L-3 · Cross-owner quota coupling — M1 gate item, not a current defect

**File/symbol:** `crates/artifact-adapter/src/lib.rs::MemoryArtifactStore::admit`.
**Severity:** Low in M0 (process-local, single trusted host); **Medium as an M1 integration precondition**.

`admit` computes `bytes` across *all* owners against `global_bytes`/`global_count`. With defaults this means four projects at `owner_count = 64` exhaust `global_count = 256`, and sixteen at `owner_bytes = 1 MiB` exhaust `global_bytes = 16 MiB` — one project can then deny the artifact store to every other project. `QuotaExceeded` is also a coarse signal about aggregate consumption by other owners.

ADR-028 states that public errors "do not distinguish a foreign owner's artifact from absence", which `read` satisfies exactly; it does not currently acknowledge that `capture` exposes shared-budget state. This is inherent to a shared global cap and acceptable for M0, but it belongs in the ADR's Consequences and in the M1 security gate alongside live-`ProjectRef` validation, since M1 is where the callers stop being a single trusted host.

Related capacity note (Info): because `admit` reserves the full `output_bytes` rather than the actual size, an owner producing max-size outputs is capped at **4** artifacts under defaults (`owner_bytes 1 MiB / output_bytes 256 KiB`), not the `owner_count = 64` the limit struct advertises. Correct per "Reserve maximum output budget before starting", but a sizing surprise worth one line in the ADR.

---

## L-4 · Internal state exposed through error variants and timestamps if surfaced verbatim in M1

**File/symbol:** `crates/domain/src/artifact.rs::ArtifactError`; `ArtifactMetadata::{created_seconds, expires_seconds}`.
**Severity:** Low — explicit M1 work, flagged so it lands in the gate.

`ClockRegression` tells a client the store is poisoned; `IdExhausted`/`EntropyUnavailable` expose RNG state; `QuotaExceeded` exposes shared budget (L-3). Separately, `created_seconds`/`expires_seconds` are, per the domain doc comment, offsets from a *process-local monotonic origin* — serializing them to a client leaks process uptime and is not interpretable as an absolute time. M1's Resource adapter should collapse the error set to a coarse client-facing form and convert expiry to a remaining-TTL duration rather than forwarding the raw counter.

---

## Info

- **`#[serde(deny_unknown_fields)]` on `ArtifactMetadata` is inert** (`crates/domain/src/artifact.rs`) — the struct derives only `Serialize`. It reads as input validation but has no effect. Either drop it, or if `Deserialize` is added in M1 be aware that `owner`, `id` and `sha256` would become caller-settable, which is the opposite of what the ownership model wants.
- **Mislabelled unreachable errors** (`crates/artifact-adapter/src/lib.rs::capture_with_generator`): `u32::try_from(content.len()).map_err(|_| ArtifactError::InvalidLimits)` is unreachable given `output_bytes ≤ 256 KiB`, and `ArtifactId::try_from(encoded)?` can surface `InvalidId` from a call site that takes no client-supplied ID. Not worth new variants — a comment plus `debug_assert!` documents the invariant without touching the enum.
- **Bounded worst-case redaction cost**: rescanning is O(1) new start positions per input byte, so the ceiling is roughly `input_bytes × Σ|secret|` ≈ 10⁹ byte comparisons for a 1 MiB input against 8×128-byte patterns. Bounded and short-circuited in practice, but it runs under the exclusive `&mut self` borrow, so in M1 a single adversarial capture head-of-line blocks every `read`, `cleanup` and `revoke_owner`.
- **Memory hygiene**: `MemoryArtifactStore::new` calls `shrink_to_fit()` on each secret, which reallocates and leaves an unzeroed copy of the pattern in freed memory; `pending`/`buffer` likewise hold pre-redaction bytes at drop. ADR-028 explicitly disclaims guaranteed RAM erasure, so this is consistent — noted only so that if M1 tightens the claim, the `shrink_to_fit` is the first thing to remove.

---

## Suggested order

M-1 and M-3 are one change: fix the exit-path gating, restate the oracle's prefix rule from the threat model, and add the `input_bytes` sweep dimension — the strengthened test then demonstrates the fix rather than asserting the bug. M-2 is independent and cheap. L-1/L-2 are small. L-3/L-4 are ADR/gate text, not code.

## Disposición del Principal Engineer

- Sin hallazgos High ni ruta demostrada de fuga de un match completo. Se revisó
  críticamente cada supuesto; no se afirma una segunda revisión externa.
- M-1: no se cambia la política explícita ADR-028. Redactar proper-prefix al EOF y
  al corte de salida es conservador deliberado: el productor puede entregar un
  stream ya truncado antes del store. Incluso con lookahead contrario se prioriza
  minimizar exposición parcial sobre preservar sufijos coincidentes. El ADR ahora
  explica la pérdida de fidelidad; tests la fijan. No se promete solo-match-exacto.
- M-2: latch permanente es intencional cuando un Clock prometido monotónico rompe
  su contrato. RegistryClock documenta monotonicidad y MonotonicClock usa Instant,
  no SystemTime/NTP. Recuperación explícita: construir nuevo store con reloj fiable;
  no hace falta reset in-place que reutilice estado desconfiable. API rustdoc/ADR
  ahora lo explicitan. Se borran todas las entradas al detectar regresión.
- M-3: ampliado el oráculo a229,950 combinaciones, dos sets de secretos (incluye1
  byte+longitud8), caps entrada1..9 y salida1..inputcap, raw0..8 y chunks1..5.
  Full matches se calculan sobre prefijo visible de entrada; política de prefijo
  sobre corte emitido sigue siendo la acordada. Esto verifica implementación y
  particionado contra la política, no pretende validar la política con un oracle.
- L-1: truncated documenta may-have-dropped y true conservador en límite exacto;
  nuevo test de input exacto exige no sondear EOF adicional.
- L-2: private redact rechaza secret vacío antes de I/O, con test discriminante;
  restantes restas/contadores están acotados por invariantes de constructor y slice.
  Clippy real del workspace pasa; no se inventa arithmetic_side_effects como gate.
- L-3: ADR declara global quota compartida, señal gruesa de capacidad y4 artifacts
  máximos por owner bajo default de bytes. Owner isolation protege contenido; no
  se afirma aislamiento total de recursos entre owners.
- L-4: M1 debe mapear errores internos y TTL restante a su contrato público; los
  counters monotónicos no se publican hoy por MCP. Se registra esa obligación.
- Metadata es solo salida; se retira el atributo serde de unknown-fields inerte.
  Entropía/ID/contenido se producen internamente; no se abre import de metadata.
- RAM zeroization y plazo duro para fuente bloqueante están explícitamente fuera
  de este adapter. El productor de M1 debe ser no bloqueante/acotado y planificado
  fuera del reactor; cuotas de contenido no se equiparan a RSS total.
