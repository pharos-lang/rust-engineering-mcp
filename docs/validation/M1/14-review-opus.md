# M1-14 independent review

Claude Code2.1.259, explicit claude-opus-5 High, tools disabled/safe/restricted,
read-only bounded source packet. modelUsage confirms Opus5 review and separately
reports a small Haiku4.5 auxiliary call; no alternate reviewer substituted.
Findings below precede corrections; see principal disposition and follow-up.

## Review: M1-14 CLI doctor — read-only contract/security review

Scope reviewed as given. No tools run; all claims are from the bounded source above. I flag where a conclusion depends on code outside the excerpt.

---

## Findings

### F1 — Medium (proven in code, latent in practice): an *unconfigured* optional dependency turns the embedded semantic index into a configured failure → exit 1

**Evidence**
- `crates/mcp-server/src/doctor.rs:364-374` — `index_configured = host…index_store.is_some() || index_source_observed(&context.semantic_index)`
- `crates/mcp-server/src/doctor.rs:566-580` — `index_source_observed` returns `true` for every `Unavailable` reason except `Missing`/`NotConfigured`, explicitly including `FeatureDisabled` and `DependencyUnavailable`
- `crates/mcp-server/src/doctor.rs:263-287` — `configured == true` ⇒ `Status::Failed` ⇒ `exit_code() == 1` (`doctor.rs:209-211`)
- Producers of those two reasons: `stdio/catalog/provider.rs:287-294` (`FeatureDisabled` when `local` is off and the bundle carries index bytes) and `provider.rs:329-331` (`DependencyUnavailable` when the bundle carries index bytes but `_model` is `None`, i.e. `--catalog-model-dir` was not passed)

**Discriminating triggers**

*A (build fact reported as host failure).* Binary built without the `local` feature — exactly what the gate builds at `scripts/test-doctor.py:268-271` — run as `doctor --catalog-store S --catalog-trust T` against a verified bundle whose `semantic_index_bytes()` is `Some`. Path: `provider.rs:287-294` → `FeatureDisabled` → `index_source_observed` true → `Failed` → exit 1. The host is correctly configured; the sole fact is a compile-time feature that `version --json` reports as a *neutral build fact* (`version.rs:25 compiled_local`).

*B (unconfigured optional service reported as failure).* `local` build, bundle with embedded index, no `--catalog-model-dir`. The `model` check itself is correctly `not_configured` / `Warning` (`doctor.rs:359-362`), but the derived index check is `unavailable` / `Failed`.

Both contradict ADR-045:32 ("Unconfigured optional services are not failures") and ADR-045:59-60 ("Unconfigured/unchecked optional facilities … produce warning/exit0").

Currently latent: `tests/doctor.rs:214-217` passes under `--no-default-features`, so `fixture-1.tar.zst` evidently carries no embedded index. A production bundle that ships one flips this.

**Minimal correction** — drop the two dependency/build reasons from the configured set (`doctor.rs:573-575`):

```rust
| CatalogComponentUnavailable::Invalid
| CatalogComponentUnavailable::IdentityMismatch
| CatalogComponentUnavailable::Denied
| CatalogComponentUnavailable::Budget
| CatalogComponentUnavailable::IoUnavailable
| CatalogComponentUnavailable::UnsupportedPlatform
```

No genuine failure is lost: whenever the index store is *explicitly* configured, the first disjunct at `doctor.rs:365-367` already marks it configured, and `provider.rs:329-331` is only reachable in that case with `_bundle == None`.

Note: `crates/mcp-server/src/doctor/tests.rs:159-177` asserts the current behaviour for `DependencyUnavailable`, so it encodes the defect and must be updated with the fix.

---

### F2 — Medium (declared-scope gap): cleanup guarantee covers only SIGINT/SIGTERM

**Evidence** — `doctor_run.rs:42-51` registers only `SignalKind::interrupt()` and `SignalKind::terminate()`.

**Trigger** — `doctor --active` in an interactive shell; close the terminal (or `kill -HUP <pid>`). SIGHUP retains its default disposition, the process dies before the gateway's joined cleanup runs, and the owned calibration container/volume survives. That leak then poisons the next gate run via `scripts/test-doctor.py:253 assert_clean()`, which is deliberately non-remediating.

The ADR states the cleanup property unconditionally ("gateway cleanup completes before exit", ADR-045:50) while committing only to two signals (ADR-045:53-54).

**Minimal correction** — add `hangup: signal(SignalKind::hangup())` to `Signals` (`doctor_run.rs:36-48`) and a third arm in `receive()` (`doctor_run.rs:50`). No new dependency; same pinned Tokio signal feature. Alternatively narrow ADR-045:50 to the two handled signals.

---

### F3 — Low (proven): a deadline-driven abort of active calibration is reported as `interrupted`

**Evidence**
- `doctor_run.rs:30-34` — `is_cancelled()` is `self.check().is_err()`, which is also true once `Instant::now() >= deadline` (`doctor_run.rs:23-24`)
- `doctor.rs:463-464` — `ExecutionError::Cancelled` ⇒ `Reason::Interrupted`

**Trigger** — active run exceeds 900 s (`doctor_run.rs:90`) with no signal sent. The five runtime checks read `unavailable (interrupted)` while the separate diagnostic check reads `deadline` (`doctor.rs:238`, added at `doctor.rs:539-541`). An operator reasonably concludes someone pressed Ctrl-C.

**Minimal correction** at `doctor.rs:463-464`:

```rust
InspectionError::Execution(ExecutionError::Cancelled)
| InspectionError::Project(ProjectError::Cancelled) => match control.check() {
    Err(ProjectError::Rejected(OperationalErrorCode::CommandTimeout)) => Reason::Deadline,
    _ => Reason::Interrupted,
},
```

Gate-compatible: on SIGINT the `cancelled` flag is checked first (`doctor_run.rs:21`), so `check()` still returns `Cancelled` and `scripts/test-doctor.py:350` continues to observe `interrupted`.

---

### F4 — Low (mechanism proven, trigger conditional): SIGINT/SIGTERM are silently swallowed during report encoding and write

**Evidence** — `doctor_run.rs:118-148`. Tokio's signal registration installs a process-global `sigaction` (via `signal-hook-registry`) that is never restored; dropping the runtime does not unregister it. After `runtime.shutdown_timeout(...)` at `doctor_run.rs:124`, nothing polls the streams, so both signals are received and discarded.

**Trigger** — stdout is a pipe whose reader stalls and the report exceeds the pipe buffer. The report is bounded at 128 KiB (`doctor_run.rs:14`) versus a typical 64 KiB pipe buffer, so `write_all` at `doctor_run.rs:144` can block indefinitely and the process is no longer terminable by SIGINT *or* SIGTERM. (SIGPIPE is unaffected — it is not registered — so the closed-reader case is fine.)

**Minimal correction** — perform the encode+write inside `block_on` before shutdown, under `tokio::select!` against `signals.receive()`, returning `ExitCode::FAILURE` on the signal arm.

---

### F5 — Low (test integrity): `doctor/tests.rs:75` cannot fail; it does not prove non-echo of host configuration

**Evidence** — `doctor/tests.rs:64-76` asserts `!serde_json::to_string(&report)?.contains("doctor-executable")`. `Report` (`doctor.rs:163-173`) has no field derived from `host.rust` on any path — `report.runtime` is only set from `ToolchainObservation` (`doctor.rs:455`), and in this test the active branch is not taken at all. The assertion is structurally incapable of failing.

**Real gap it masks** — `report.catalog` (`doctor.rs:390`) and `report.runtime` (`doctor.rs:455`) are serialized wholesale, and neither `CatalogContextStatus` nor `ToolchainObservation` is in the bounded source, so ADR-045:51 ("No arbitrary paths/secrets … echoed") is unverified for the two largest evidence subtrees. `tests/doctor.rs:214-236` inspects individual fields but never asserts the report is free of host paths.

**Minimal correction** — in `tests/doctor.rs`, after the configured-catalog run, assert the serialized report contains neither `f.0.display().to_string()` nor `"/private/tmp"`.

---

### F6 — Low (conditional leak): `capabilities` echoes a raw Debug error into the report

**Evidence** — `capabilities.rs:79` `reason: format!("{error:?}")`. Only `InvalidConfiguration` is exercised (`capabilities_cli.rs:150`, `:304`, `:315`). Any gateway-error variant carrying a payload (io error text, path, docker stderr) would be reflected into both JSON and human output, contradicting ADR-045:51. Whether such a variant exists is outside the bounded source.

**Minimal correction** — map the error to a closed `&'static str` reason code rather than `{error:?}`.

---

### F7 — Low: output-limit fallback loses the measured duration and mislabels serialization errors

**Evidence** — `doctor_run.rs:129` sets `duration_ms` on the original report; `doctor_run.rs:133-136` then *replaces* `report` with a fresh `Report::failure(...)` whose `duration_ms` is `0` (`doctor.rs:182`). The emitted overflow report therefore always claims `duration_ms: 0`, satisfying `scripts/test-doctor.py:139` only trivially. Separately, a genuine `serde_json` failure (not a size failure) takes the same branch and is reported as `output_limit`.

**Minimal correction** — re-assign `report.duration_ms` after the fallback construction; distinguish the size check from the serialization error in `encoded` (`doctor_run.rs:150-160`).

---

## Confirmed clean (areas explicitly named)

- **Early signal.** `Signals::register` (`doctor_run.rs:86-89`) precedes `spawn_blocking` (`doctor_run.rs:96`). A signal before registration cannot orphan work — nothing is spawned yet. A signal between registration and the first poll is captured: the registration exists, so the pending notification is delivered on first poll of `signals.receive()`.
- **Repeated signals.** After the first signal arm sets `cancelled` and enters `work.await` (`doctor_run.rs:101`), later SIGINT/SIGTERM are absorbed by the installed handler and no longer terminate the process, so cleanup is joined. ADR-045:57 holds for the two registered signals (see F2 for the third).
- **Deadline branch.** `doctor_run.rs:102` does not set `cancelled`, but `Control::check` (`:23-24`) and `is_cancelled` (`:32`) both observe the deadline directly, so the blocking worker and the gateway still unwind cooperatively. No detach: every arm `work.await`s.
- **Passive = no execution.** `RustProjectInspector::new` is constructed only under `invocation.active && host.rust.is_some()` (`doctor.rs:439-441`). Proven by `doctor/tests.rs:55-77` and, end-to-end, by `tests/doctor.rs:129-168` (hostile PATH with executable `rustc`/`cargo`/`docker` sentinels; no `.called` file, no `--state-root` creation).
- **Read-only catalog.** `provider.rs:139-276` uses only `read_private_optional_file`; no `CatalogStore::open`, no staging mutation. `tests/doctor.rs:211-235` holds a real administration lease across the doctor run (`let _lease`, correctly named so it is not dropped early) and pins `staging.bundle` and `active.bundle` byte-for-byte.
- **Status aggregation.** `Report::add` (`doctor.rs:196-198`) is a correct monotonic max over Passed<Warning<Failed for all six transitions; `exit_code` (`:209-211`) yields 1 only on `Failed`.
- **Parser closure.** `doctor::parse` (`doctor.rs:18-42`) consumes host values eagerly, so a host value literally equal to `--json`/`--active` is preserved (`doctor/tests.rs:24-27`); every unrecognised token reaches `host_config::parse`, whose final `position(...)?` (`host_config.rs:59-66`) rejects it. Duplicate `--json`/`--active`, duplicate `--project-ttl-secs`/`--rustsec-*`/catalog flags, a 17th `--root`, non-UTF-8 host values, incomplete Docker groups, and a non-approved `--rust-image` all funnel to `Invocation::Unsupported` → exit 2 with empty stdout (`main.rs:118`, `tests/doctor.rs:241-253`).
- **capabilities compatibility.** `json.unwrap_or(true)` (`capabilities.rs:38`) preserves the JSON default; `capabilities_cli.rs:295-307` pins the exact default bytes and proves `--json` is byte-identical to the bare invocation.
- **Bounded output.** `encoded` (`doctor_run.rs:150-160`) checks `len + 1 > 128 KiB` before the newline push, matching the 128 KiB assertions at `tests/doctor.rs:94` and `scripts/test-doctor.py:130`. Check count is bounded ≤ 19 against the gate's ≤ 32 (`scripts/test-doctor.py:141`). All JSON leaf enums are field-less, so `human()`'s `{:?}` formatting (`doctor.rs:213-227`) cannot widen.
- **Cleanup uncertainty preserved.** `ExecutionError::CleanupUncertain` maps to a distinct `Reason::CleanupUncertain` at `Status::Failed` (`doctor.rs:460-462`, `:471-480`) and is never collapsed into `Interrupted`; `scripts/test-doctor.py:352-353` asserts SIGINT does not produce it.

---

## Conditional concerns (not proven from the bounded source)

1. **`SecureProjects::new` side effects.** `doctor.rs:411` constructs it purely to observe validity. If that constructor creates or locks anything on the host, it breaks the read-only doctor property. Not visible here.
2. **Evidence subtree contents.** See F5 — `CatalogContextStatus`/`ToolchainObservation` are unaudited for path/secret content.
3. **serve parser equivalence.** `host_config.rs:1` claims "preserves serve semantics", but the bounded source contains no test pinning serve's flag handling against the pre-refactor implementation (`tests/doctor.rs:247` covers a single rejection case). If a serve parser test exists elsewhere, this is closed; otherwise a small equivalence test is warranted before merge.
4. **Concurrent import.** `provider.rs:177-266` gives up after three floor-stable attempts and returns `IoUnavailable`; with a configured catalog that becomes `Failed`/exit 1. Trigger: `doctor` run concurrently with `catalog import`. Pre-existing provider behaviour, newly surfaced as a doctor exit code.

## Non-blocking nits

- `#[serde(deny_unknown_fields)]` on Serialize-only structs (`doctor.rs:152`, `:163`; `version.rs:8`) is a no-op — it does not close the emitted schema as the placement implies.
- `RustsecFreshness` is assigned scope `CatalogSnapshot` (`doctor.rs:300-305`); the RustSec advisory snapshot is not the crate catalog snapshot.
- Duplicate `Diagnostic` check on late cancellation: `doctor.rs:539-541` and `doctor_run.rs:109-111` both call `record_failure` for the same event.
- `Reason::UnsupportedPlatform` exists but the non-unix/non-windows `Signals::register` failure (`doctor_run.rs:74-78`) falls through `record_failure`'s `_ => Reason::Internal` (`doctor.rs:242`).
- `doctor --active` with no `--docker*` flags still reports `"mode":"active"` (`doctor.rs:179`) while every runtime check is `not_configured` — a CI consumer keying on `mode` may assume calibration ran.
- `scripts/test-doctor.py:143-157` pins `scope`/`status`/`severity`/`reason` enumerations but not `action`, despite ADR-045:48 ("bounded action strings"); one added `require` closes it.
