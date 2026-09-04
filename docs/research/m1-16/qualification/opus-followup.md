# Opus5 medium read-only follow-up

## Verdict summary

No P0 remains in the supplied evidence. Three P1 items and four P2 items are open; two P1s are one‑to‑five‑line fixes, the third is a decision (code or protocol wording). With those closed, the principal may freeze the bounded exploratory pilot.

---

## P1

**P1‑1 — Invalid model tool call terminates the run, contrary to the stated retryable‑denial rule; asymmetric across arms.**
`target/m1-16-controller/participant.py:422-429`. `admitted` requires `name in schemas and isinstance(args,dict)`; any failure takes the deny branch, which sends `-32601` **and** calls `interrupt('unadmitted_request')`, ending the turn (`status='interrupted_or_failed'`, scored as a task failure). `docs/validation/M1-16-protocol.md:209` states "Invalid requests are recorded and receive retryable denials." Concrete failure: the model emits a hallucinated or misspelled callback name (e.g. `rust_quality` instead of the declared `rust_quality_gate`), or `arguments: null` for a zero‑property tool; the run is lost. Arm B declares 17 callback names versus arm A's 6, so this failure mode is structurally more likely in B and biases the paired comparison rather than being neutral noise. This is *not* the H2 fail‑closed requirement, which concerns unadmitted **server‑originated** requests, not malformed model calls.
Minimal fix: in `participant.py:424`, keep fail‑closed interruption only for `method != 'item/tool/call'` and for the `MAX_CALLS` stop; for an `item/tool/call` whose name is undeclared or whose `arguments` is not an object, record the existing `denied_tool` event, return the same denial response, count it against the 64‑call budget, and do **not** call `interrupt`. Alternatively, amend `M1-16-protocol.md:209` to state explicitly that undeclared names and non‑object arguments stop the run, and record the arm asymmetry as a limitation. Either is acceptable; silently retaining both texts is not.

**P1‑2 — No automated pre‑run check that arm B's hybrid retrieval treatment is actually available.**
`M1-16-protocol.md:47-52` requires verified local E5/Lance identities and "If unavailable before a pair, stop as infrastructure failure rather than measuring a silently lexical treatment." `run-study.py:236-253` constructs the driver and starts the participant with no catalog/index identity probe; `verify()` only hashes the configured store paths. A degraded or lexical‑fallback index therefore yields a *measured* B result for the eight selection items rather than an infrastructure stop. (Per‑search fallback is retained after the fact in `events.jsonl`, which detects but does not prevent it.)
Minimal fix: in `run_one`, for arm B before `participant.run_participant`, issue one setup `{'op':'call','name':'rust.catalog.status'}` through the driver, store it in `receipt['setup_catalog_status']`, and raise `broker.BrokerError('catalog_identity_unavailable')` unless the reported index/model identities equal the frozen expected values in the host config. Recorded as setup, outside the 64‑call and validation budgets (`M1-16-protocol.md:207`). If the principal prefers to keep this manual, state that in the protocol and require a per‑pair receipt.

Related, same file: `run-study.py:172-174` indexes `host[key]` for five optional keys, so an incomplete host config raises an uncaught `KeyError` instead of a labelled failure. Minimal fix: `if key not in host: raise ValueError('incomplete_driver_host_configuration')`.

**P1‑3 — Selection scoring binds the corpus date to a literal in the evaluator.**
`evaluate.py:196`: `selection_candidate(candidate, label, projection, '2026-09-04')`. `corpus_date_cited` is an `all()` conjunct (`evaluate.py:46,63`), so if the frozen corpus date is anything other than this literal, **every** selection candidate in both arms fails deterministically and the eight selection items produce a uniform, meaningless zero. The value is not derived from, or cross‑checked against, any frozen input.
Minimal fix: take the date from the frozen projection provenance (or a `corpus_date` field in `tasks-and-labels.json`), fail with `ValueError('corpus_date_missing')` if absent, and add one self‑test asserting it equals the corpus value.

---

## P2

**P2‑1 — Missing selection label crashes the evaluator after it has created the output directory.** `evaluate.py:187-198`: `label` is `next(..., None)`; for `kind == 'selection'` a `None` label raises `TypeError` inside `selection_candidate`, and `output.mkdir(mode=0o700)` (line 182, no `exist_ok`) has already run, so re‑evaluation requires manual removal. Fix: after line 188, `if kind == 'selection' and label is None: raise ValueError('selection_label_not_found')`, placed before the `mkdir`.

**P2‑2 — Double close of raw descriptors during forced reader shutdown.** `participant.py:271-277`: on `reader_forced_shutdown` the code calls `os.close(stream.fileno())`, then, if the readers subsequently join, calls `stream.close()` on the same `BufferedReader`, closing a descriptor number that another thread (ps monitor, event‑log write, later `containment.observe`) may already have reused. Consequence is corrupted evidence in a run that is already `cleanup_failed`. Fix: set a `descriptors_forced = True` flag in that branch and guard the `stream.close()` loop with `if not descriptors_forced`.

**P2‑3 — Any Docker CLI stderr aborts the containment observation and halts the series.** `containment.py:41`: `if child.wait()!=0 or data['stderr']: raise ValueError('observer_command_failed')`; `run-study.py:287-291,332` then marks `cleanup_failed` and breaks the run loop. A benign daemon/context warning on stderr thus ends the study. This fails safe and preserves evidence, so it is acceptable as‑is, but the principal should either accept it explicitly or retain `stderr` bytes in the receipt and fail only on nonzero exit.

**P2‑4 — Absence observation has no recorded positive control.** `containment.py:28` filters on `label=org.rust-mcp.execution=true`. Every supplied receipt (`integrated-summary.json:31-46,88-103`) shows empty lists, so an incorrect or absent label would be indistinguishable from clean teardown. The cancellation qualifications assert that an owned Cargo execution was observed (`M1-16-protocol.md:221-223`, `target/M1-16-driver-cleanup-fix.md:168-172`); include in the frozen receipts one observation showing a **non‑empty** container/volume list during an active execution, so absence is falsifiable.

---

## Declared limitations, not defects

Correctly scoped and not blocking: no OS sandbox or V8 heap/CPU bound for the code host (ADR‑046:25‑30); no authoritative native tool inventory — pins plus the 41‑key exact config are the stated boundary (`participant.py:33-40,387`); shared byte caps do not equalise information content, with censoring recorded as infrastructure failure; provider/image/OS caches as timing confounders; one‑record advisory fixture and empty dependency/advisory collections; std‑only, four repairs and four intents, one repetition, no population claim; cross‑platform, licensing and distribution as separate gates. Cleanup acknowledgement/error preservation (`main.rs:249-306,559-581`, `broker.py:197-215`), no‑environment boundaries, frozen‑input verification before and after each run (`run-study.py:143-190,294-299`), first/final scoring including wrong‑kind first candidates (`evaluate.py:178,192-194`), failure receipts and stop‑on‑uncertainty (`run-study.py:332`) all read as sound in the supplied code.

## Freeze disposition

The principal **may** freeze the bounded exploratory pilot, conditional on: (a) resolving P1‑1 by code change or explicit protocol amendment; (b) P1‑2 implemented as a setup check or recorded as a mandated manual per‑pair step; (c) P1‑3 bound to a frozen input; (d) re‑running the participant/broker/evaluator/driver suites and one fresh disjoint exact‑model echo after any edit, per the existing requalification rule (`principal-disposition.md:49-51`); and (e) the planned byte‑for‑byte reproduction copies committed with the freeze. P2 items may be frozen with a recorded disposition. Nothing here requires the v1 96‑run study; the 24‑run feasibility pilot's stated hypotheses and non‑population scope are internally consistent.
