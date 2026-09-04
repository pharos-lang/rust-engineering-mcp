# Revisión externa M0-04 — Claude Opus 5

Claude Code 2.1.259, `claude-opus-5`, esfuerzo High, 2026-09-03.
Read-only: safe-mode, restricted, strict-mcp-config, tools vacío, dontAsk,
permission-prompts none y sin persistencia de sesión. Exit 0, is_error=false;
612854 ms. Modelo efectivo Opus 5; CLI también reporta una llamada auxiliar Haiku
(20 tokens de salida), no se usó como sustituto del reviewer.

Paquete SHA-256: `a02aa9b1a5b6e29ae5d1258a30497ccdb820cafcb473c04b96d3ab8e678a11e8`.
Snapshot anterior a `97ef2a5`: ocho archivos (ADR-024, application/lib,
project-adapter/lib/filesystem/macos/manifest sin tests, main y stdio/project).
La corrección de observaciones repetidas y la documentación de special-file open
se incorporaron después del snapshot. Las pruebas y el SDK no estaban en el
paquete; las hipótesis del reviewer deben contrastarse con evidencia real.

La resolución por finding está en [validación M0-04](../validation/M0-04.md).
A continuación se conserva el dictamen original completo, incluidos findings
condicionales y recomendaciones que el principal no adopta.

---

# Security Review — M0-04 `rust.project.open`

**Reviewer role:** independent read-only Principal Engineer. **Method:** static reading of the supplied snapshot only. I executed nothing, ran no tests, and made no edits. Where a finding depends on the behaviour of a pinned third-party crate I could not inspect (`toml`, `schemars`, `jsonschema`), I say so explicitly rather than asserting an outcome.

**Snapshot gaps that bound this review:** `crates/mcp-server/src/stdio.rs`, the `rust-engineering-domain` crate (`ProjectRef`, `ProjectIdentityFingerprint`, `OperationalErrorCode`), `Cargo.toml`/`Cargo.lock`, and the test files were not supplied. Statements about server startup, transport limits, and `FromStr`/`Display` round-trips for `ProjectRef` are therefore conditional.

---

## Defects

### P1-1 — Output schema and serializer may disagree on every success path
`crates/mcp-server/src/stdio/project.rs:103-113` (`OpenOutput`), consumed at `:227-238` and `:305-310`.

`OpenOutput` carries `#[serde(deny_unknown_fields)]` **and** `#[serde(flatten)] outcome: Outcome`. Serde documents `deny_unknown_fields` as unsupported in combination with `flatten`; on a `Serialize`-only type it is also a behavioural no-op. It is not a no-op for **schemars**, which reads it to close the object. Because `Outcome` is an internally tagged enum, its subschema cannot be merged into the parent `properties` and must appear as a composition (`allOf`/`oneOf`). If schemars 1.2 renders the closure as `additionalProperties: false` rather than `unevaluatedProperties: false`, then `status`, `error_code`, `error_message` and `data` are unevaluated at the outer level and `self.output.is_valid(&value)` fails.

- **Trigger:** any `rust.project.open` call, including the success path.
- **Consequence:** every call returns `ErrorData::internal_error("Project output validation failed")`. Fail-closed, no confidentiality impact — but the tool never works, and the reported test coverage (parser, registry, filesystem) does not exercise this path; wire tests are stated as not yet integrated.
- **Minimum correction:** add a serialize→`output.is_valid` round-trip unit test covering all four `Outcome` variants before merge. If the rendered keyword is `additionalProperties`, remove `deny_unknown_fields` from `OpenOutput` and close the object with `unevaluatedProperties: false` in a schema post-pass. Also confirm `jsonschema` 0.53 with `default-features = false` evaluates the `$schema` draft schemars emits (`unevaluatedProperties` is a 2019-09+ keyword).

### P1-2 (conditional) — Unbounded TOML nesting can abort the process
`crates/project-adapter/src/manifest.rs:144-165` (`Validator::read`), specifically `toml::from_str` at `:159`.

`read` bounds *size* (256 KiB per manifest, 4 MiB total, 128 manifests) but never bounds *structural depth* before parsing. A 256 KiB manifest of the form `a = ` followed by ~100k `[` characters yields ~100k nesting levels. Recursive descent in the parser, the `Content` buffering forced by `#[serde(flatten)] groups: Groups` at `:80-81`, and recursive `Drop` of the resulting tree are all candidates for stack exhaustion.

- **Trigger:** a hostile `Cargo.toml` anywhere reachable inside an authorized root — e.g. a cloned third-party repository, or a path/patch dependency directory the client opens.
- **Consequence:** if the pinned `toml`/`toml_edit` does not enforce its own recursion limit, stack overflow on the `spawn_blocking` thread. Rust converts this to `SIGSEGV`/abort with no unwinding, so the entire MCP server dies. This is strictly worse than the acknowledged 10-second slot-hold: it is not cooperative, not deadline-bounded, and not recoverable. `control.check()` is never called inside the parse.
- **Minimum correction:** a linear pre-scan over the manifest bytes rejecting bracket/brace nesting greater than the already-declared graph depth bound (32) before calling `toml::from_str`, plus a fixture test. If the pinned parser does enforce a limit, record the version and the limit in the ADR so the guarantee is pinned rather than incidental.
- **Note:** I verified the code under review contains no unbounded recursion of its own. `Validator::dependencies` (`:379-464`) recurses exactly once — the inner call passes `member=false, workspace=None`, so `:412` cannot recurse again. `validate` uses an explicit `pending` stack. `joined` is iterative.

### P2-1 — Registry capacity exhaustion with no eviction and no release
`crates/application/src/lib.rs:112-114` and `:128-134`.

Capacity is 64 (`project.rs:244`). There is no identity deduplication, no LRU eviction, no `project.close` tool, and `last_used` is only refreshed by `resolve`, which is documented as an internal API and is not exposed over MCP. Entries therefore expire strictly `ttl_seconds` after creation.

- **Trigger:** 64 successful `project.open` calls. The *same* path 64 times suffices — an agent retry loop reaches this trivially.
- **Consequence:** all subsequent opens return `SANDBOX_DENIED` for up to 1800 s (default) or 86400 s (configured maximum). The client has no way to release capacity.
- **Minimum correction:** evict the least-recently-used entry when at capacity. This is semantically identical to TTL expiry, which the design already treats as safe (a stale reference yields `PROJECT_NOT_FOUND`), and it preserves the `idempotent(false)` annotation at `project.rs:233`.

### P2-2 — Mutex poisoning permanently disables the tool
`crates/mcp-server/src/stdio/project.rs:286-289`: `registry.lock().map_err(|_| ProjectError::Internal)?`.

Any panic in the blocking worker while the lock is held poisons the `Mutex` for the process lifetime.

- **Trigger:** any panic on the untrusted-manifest path — allocation failure, or a panicking path in a parsing dependency.
- **Consequence:** every subsequent call returns a JSON-RPC internal error, silently and irrecoverably, with no operator signal distinguishing it from a transient fault.
- **Minimum correction:** `lock().unwrap_or_else(std::sync::PoisonError::into_inner)`. This is safe here because `ProjectRegistry::open` inserts only after the backend succeeds (`:118-134`), so a mid-open panic cannot leave a partially registered entry. Alternatively wrap the closure body in `catch_unwind`.

### P2-3 — `O_NOFOLLOW_ANY` and `O_UNIQUE` are relied upon but never runtime-probed
`crates/project-adapter/src/filesystem/macos.rs:150-183` (`SecureProjects::new`), flags defined at `:21-23`, comment at `:18-20`.

Startup performs a positive probe (`.` resolves to the same node as `/`, `:175-178`) and a negative probe (`openat(slash, "/")` must return `ENOTCAPABLE`, `:180-183`). Both probe **`RESOLVE_BENEATH` only**. Nothing at runtime demonstrates that the kernel recognises bit `0x20000000` or `0x00002000`. The uname gate at `:164` accepts any Darwin major ≥ 25, which is a coarse proxy.

- **Trigger:** a kernel that silently ignores the flag bits. `open(2)` ignores unrecognised flags, so this fails open at the symlink layer while startup still succeeds.
- **Consequence:** ADR line 45 (“All components reject symlinks”) becomes false in production. Residual containment still holds — `RESOLVE_BENEATH` is proven and confines resolution beneath the verified root descriptor — so this is a degradation of a stated guarantee, not an escape. The problem is that the ADR asserts the guarantee unconditionally and the code comment at `:18` claims all three flags are “verified by real positive and negative fixtures” when only one is verified *at runtime*.
- **Minimum correction:** promote the `EINVAL` result the team already tested into a startup probe — assert that `openat(&slash, ".", RDONLY|DIRECTORY|CLOEXEC|NOFOLLOW|NOFOLLOW_ANY)` returns `EINVAL`, which proves the kernel parses `NOFOLLOW_ANY`. For `UNIQUE`, either add an equivalent probe or drop the flag: `FileStamp::from_stat` at `:103` already enforces `st_nlink == 1` independently, so `UNIQUE` is currently redundant defence whose failure mode is unobservable.

### P3-1 — `exclude` uses exact-path equality, not Cargo's prefix semantics
`crates/project-adapter/src/manifest.rs:524-530` and `:574`.

`excluded.contains(&directory)` is an exact `PathBuf` comparison. Cargo's `workspace.exclude` is prefix-based: excluding `vendor` excludes `vendor/**`.

- **Trigger:** `[workspace] exclude = ["vendor"]` with a path dependency resolving to `<root>/vendor/sub`.
- **Consequence:** `member` is computed `true` at `:574`, so `vendor/sub` is admitted to the `names` collision set and granted `package.workspace` / `dependency.workspace = true` inheritance at `:186-203` and `:412-435`. A workspace Cargo rejects is returned as `validation: "structural"`. This contradicts ADR line 34 (“Literal members/excludes/default members … are checked”) — it is a defect in a documented check, not one of the declared cuts.
- **Minimum correction:** replace both uses with a prefix test, `excluded.iter().any(|e| directory.starts_with(e))`, including the members/excluded overlap check at `:528`.

### P3-2 — Non-regular files are misclassified as authorization denials
`crates/project-adapter/src/filesystem/macos.rs:101-116` (`FileStamp::from_stat`) reached via `Access::file` at `:342-351`.

`from_stat` collapses “wrong file type” and “hardlinked” into a single `denied()`. `Access::file` converts only `ProjectNotFound` to `Ok(None)`, so any other error aborts the whole open.

- **Trigger:** `Cargo.toml` is a directory; or `[[bin]] path = "src"` / `build = "scripts"` names a directory. Note that opening a directory `O_RDONLY` without `O_DIRECTORY` succeeds on macOS, so the type check is the only thing catching it.
- **Consequence:** a benign manifest error is reported as `SANDBOX_DENIED` — the same code used for a genuine out-of-root boundary violation. Security telemetry cannot distinguish a hostile path escape from a typo.
- **Minimum correction:** split the checks. Return `Ok(None)` (which upstream becomes `INVALID_PROJECT` via `ok_or_else(invalid)` at `manifest.rs:150`, and `false` from `is_file`) for non-regular types; retain `denied()` for `st_nlink != 1`, which is a deliberate provenance rejection per ADR line 45/54.

### P3-3 — `SANDBOX_DENIED` conflates three unrelated conditions
`macos.rs:227` (path outside all roots), `application/src/lib.rs:113` (registry at capacity), `project.rs:273` (single worker busy), plus `application/src/lib.rs:85` (invalid registry configuration).

The client cannot distinguish retryable back-pressure from a hard authorization failure; the operator cannot alert on real boundary violations without false positives from ordinary contention. Combined with P2-1, a saturated registry is indistinguishable from an attempted escape.

- **Minimum correction:** reserve `SANDBOX_DENIED` for filesystem authorization. Map worker contention and registry capacity to a distinct blocked code (the `BlockedCode` enum already has room, or reuse `COMMAND_TIMEOUT` semantics for contention).

### P3-4 — macOS startup fails hard where other platforms degrade gracefully
`macos.rs:164` and `:192` return `Err(unsupported())` from `SecureProjects::new`, which propagates out of `ProjectTool::new` (`project.rs:224`). The non-macOS adapter instead constructs successfully (`filesystem.rs:19-21`) and reports `UNAVAILABLE` per call (`:31-33`).

- **Trigger:** Darwin < 25, a non-APFS root, or a failed flag probe.
- **Consequence:** the server likely refuses to start rather than serving `status: "unavailable"`. **Unverified** — `stdio.rs` is not in the snapshot, so the actual handling of `ProjectTool::new`'s error is unknown.
- **Minimum correction:** have the macOS adapter retain the probe failure and return `UnsupportedPlatform` from `open`/`revalidate`, matching the other platforms.

### P3-5 — Minor correctness and hygiene
- **`manifest.rs:238-243`** is unreachable dead code: `metadata` is already handled and `continue`d at `:205-209`. Delete.
- **`manifest.rs:587-593`**: `default_members` is validated against `members` *after* the traversal loop mutated it at `:577` with discovered path dependencies. Entries Cargo would reject (not literal members) are accepted. Validate against the literal member set captured before traversal.
- **`manifest.rs:46-54`** (`Groups`): `rename_all = "kebab-case"` accepts only `dev-dependencies`/`build-dependencies`. Cargo still accepts the deprecated underscore spellings, which `Manifest` silently ignores (no `deny_unknown_fields`, incompatible with `flatten`). Path dependencies declared that way are absent from the graph and from the identity fingerprint. Add explicit `alias` attributes or reject the underscore keys.
- **`application/src/lib.rs:167-172`**: `resolve` evicts the entry on *any* non-`Cancelled` error, including transient ones. Fail-closed but unnecessarily destructive; consider evicting only on `InvalidProject`/`ProjectNotFound`.
- **`manifest.rs:552`**: all `[workspace.dependencies]` path entries are traversed, including ones no member uses. Cargo does not resolve unused workspace dependencies. Over-rejection only.

---

## Acknowledged limitations — confirmed accurate, not defects

These are documented cuts and I found the code consistent with the documentation:

- **No Cargo execution, no child process.** Confirmed: no `std::process`, no `Command`, no `cargo` invocation anywhere in the snapshot. `manifest.rs:1` and ADR line 79 are accurate.
- **Zero roots grants zero access.** `SecureProjects::new(&[])` succeeds, and `open_path`'s `roots.iter().find(...).ok_or_else(denied)` (`macos.rs:223-227`) denies unconditionally. ADR line 19-20 holds.
- **Request JSON cannot add authority.** The only client-controlled input is `path`, and it is prefix-checked against the host root set before any syscall.
- **Root-relative resolution, never descendant handles.** `Access` holds `&SecureProjects` (`macos.rs:336`), and every read re-enters `open_path`, which resolves the full relative path from the original root descriptor. ADR lines 43-44 and 84-85 are accurate.
- **Full path containment for members.** `joined(..., member=true)` (`manifest.rs:106-108, 123`) rejects absolute paths and *all* `..` components, so `workspace.members` cannot name anything outside the selected root. Path dependencies may use leading `..` but remain confined by `open_path`'s host-root check.
- **No content exfiltration.** `read_file` is only ever called on `<dir>/Cargo.toml` (`manifest.rs:149-150`); every other manifest-derived path reaches only `is_file`, which returns a boolean. Manifest bytes enter the SHA-256 identity but are never returned. `Access::file` converts only `ENOENT` to `Ok(None)`, so an out-of-root path is never silently skipped — this is the key property and it holds.
- **Cooperative-only deadline; slot held past timeout.** `Control::check` (`project.rs:323-332`) and the `_permit` comment at `:284-285` match ADR lines 57-60 exactly.
- **Non-atomic snapshot; rename-ABA not excluded.** The recheck loop at `macos.rs:263-272` is best-effort by construction, exactly as ADR lines 50-54 state.
- **Ancestor workspace discovery unsupported.** Opening a member directory directly fails `INVALID_PROJECT` because `workspace` is `None` and inheritance at `manifest.rs:246-248` cannot resolve. Documented at ADR lines 29-31.
- **Implicit named-target discovery rejected.** A package whose only targets are `src/bin/*.rs`, `examples/`, `tests/` or `benches/` fails `!found` at `manifest.rs:347-349`. Documented at ADR line 33 — but note the blast radius: one such member fails the *entire* workspace open. Worth stating in the ADR consequences.
- **`/tmp` and `/var` aliases rejected.** Correct — both are symlinks, rejected by `NOFOLLOW_ANY` during root acquisition. ADR line 89 is accurate.
- **Identity excludes source and build scripts.** `build.rs` is neither observed nor hashed. ADR line 66 states this; it must remain prominent for M0-05, since a fingerprint match does **not** imply executable code is unchanged.
- **Argument parsing.** `main.rs:45-64` correctly rejects a 17th `--root`, a non-UTF-8 root, a duplicate `--project-ttl-secs`, and an out-of-range TTL, rather than silently truncating.

---

## Verdict

**Do not mark M0-04 Done yet. Conditional pass, gated on P1-1 and P1-2.**

The security core is sound. The filesystem boundary is the strongest part of this change: resolution always re-enters from the verified root descriptor, out-of-root paths are denied before any syscall, the `ENOENT`→`Ok(None)` conversion is correctly narrow, and member paths are lexically confined. I found **no P0 and no exploitable filesystem escape**. The ADR is unusually honest about its cuts, and I found no materially false claim about the *security* boundary.

The two blockers are not escapes but availability and integration correctness:

1. **P1-1** may mean the tool cannot return a successful result at all, and the stated test coverage does not reach it. A four-variant round-trip test resolves this in minutes and must exist before merge.
2. **P1-2** is the only path I found that escapes the documented DoS envelope — an uncontrolled process abort rather than a bounded 10-second slot hold. It needs either a depth pre-scan or a pinned, documented parser guarantee.

**P2-1** and **P2-2** should also land before Done: both turn a single client mistake or a single panic into extended or permanent unavailability of the only tool the server offers. **P2-3** should land before the ADR's symlink guarantee is cited by any downstream document, since the probe the team already wrote for tests is what makes the production claim true.

Two ADR text corrections follow directly from the findings and should accompany the code: line 45's unconditional "All components reject symlinks" needs the runtime probe of P2-3 to be true in production, and line 34's claim that excludes are checked is contradicted by P3-1's exact-match implementation.


---

# Verificación externa acotada de cierre

Claude Code 2.1.259, Opus 5 Medium, exit 0/is_error=false; 67765 ms.
Mismas restricciones read-only. SHA-256 del paquete reducido:
`35c6ae28a8d5a679f4e037e33030663d6287eec3117e1bc3aaa11355770ed5b4`.
La CLI registra además una llamada auxiliar Haiku de 18 tokens de salida.

## Closure check — P1-1, P1-2, and correction review only

### P1-1 (schemars flatten + `deny_unknown_fields`) — **closed**

The snapshotted runtime schema does not use the failure mode the condition anticipated. The envelope closes with **`unevaluatedProperties: false`**, not `additionalProperties: false`, and the four `oneOf` branches deliberately carry **no** `additionalProperties`. That is the only correct combination: `unevaluatedProperties` is annotation-aware, so `status`/`error_code`/`error_message`/`data` matched inside the chosen branch are "evaluated" and survive, while the flattened envelope fields (`summary`, `duration_ms`, `diagnostics`, `truncation`, `evidence`) are matched by the top-level `properties`. Nothing valid is rejected.

The new test is the right kind of evidence because it validates against `schema_for!(OpenOutput)` — the same value that is published — rather than the checked-in file, and it exercises all four `status` arms produced by `output()`. The `extra["unrecognized"]` negative assertion is load-bearing: since the branches are open, the only thing that can reject it is `unevaluatedProperties`, so the assertion proves the validator actually enforces the closing keyword rather than silently ignoring it. The `UNKNOWN_CODE` mutation covers the `$defs` enum path. `oneOf` discrimination is unambiguous (passed vs. cancelled differ on `data`; all four differ on the `status` const), so no multi-match risk.

### P1-2 (TOML nesting stack overflow) — **closed**

Pinned source shows both entry points (`parse_document`, `parse_value`) wrapping the receiver in `RecursionGuard::new(…, LIMIT)` with `LIMIT = 80`, gated on `not(feature = "unbounded")`; the feature tree confirms only `parse`, `serde`, `std` are enabled through this workspace, so the guard is compiled in. Dotted keys are handled iteratively (`Vec` push in `on_key`) with a post-hoc `LIMIT` check, so they cannot recurse regardless. The stdio test then demonstrates the end-to-end property that actually matters: 100k array / 50k inline-table / 50k dotted-key inputs under 256 KiB each yield `INVALID_PROJECT`, and a subsequent call on the same process succeeds — an aborted process could not answer request id 4.

Residual (informational, not blocking): the guard's presence is a feature-unification property, not a compile-time invariant. If any future dependency enables `toml/unbounded`, the bound silently disappears. A `compile_error!` on `cfg(feature = "unbounded")` would make that regression loud.

### Corrections — one item I cannot resolve

- **Exclude-prefix / default-members**: consistent across adapter and oracle; the oracle result (default-members *may* name automatically-included path dependencies) supersedes the earlier recommendation, and production never shells out to Cargo, so the oracle is test-only. No flaw.
- **Legacy group rejection**: fail-closed, and the target-scoped cases are covered by executable tests. No flaw.
- **`NOFOLLOW_ANY` probe**: read-only, and every non-`EINVAL` outcome (including success) yields `unsupported()`. Worst case is a false unavailability on a future kernel — availability, not safety.
- **Open question**: the hunk deleting the `key == "metadata"` special-case is safe only if `"metadata"` was added to the allow-list `matches!` arms above the visible context. If it was not, every workspace carrying `[workspace.metadata]` now returns `INVALID_PROJECT` — a common, untested false positive. The supplied excerpt truncates the arm list, so I cannot confirm this from the evidence given. Please verify that one line before closure.

## Check final del principal

Los dos P1 quedan cerrados por el reviewer. La única pregunta restante sobre
metadata se resuelve leyendo el código completo: `manifest.rs:220-226` conserva
el tratamiento de `package.metadata` y hace continue antes de la herencia. La rama
eliminada era duplicada e inalcanzable. `Workspace` (líneas 59-73) no niega campos
opacos adicionales y no pasa workspace.metadata por ese loop. El test existente
`accepts_real_package_and_retains_exact_bytes` incluye package.metadata anidada y
pasó en el gate. No hay que añadir metadata a la allowlist de campos heredables.

La sugerencia informativa `cfg(feature = "unbounded")` no detectaría una feature
de una dependencia desde este crate. Se conserva el pin/feature-tree documentado
y el test de proceso que detecta una regresión real de profundidad; no se añade
un compile_error que daría una falsa garantía. No quedan bloqueos del dictamen.
