# M1-14 principal disposition

Read-only external review: Claude Code2.1.259, explicit Opus5 High, then Opus5
Medium follow-up. Actual modelUsage confirms Opus5 and separately reports auxiliary
Haiku4.5 calls from the CLI; no substituted reviewer or execution authority.

- F1 accepted: absence of an optional model/local feature for an embedded index
  warns unless an index path is explicitly configured. Corrupt embedded data still
  fails. This corrects the principal's earlier overbroad configured inference;
  the discriminating unit test covers optional vs explicit failure.
- F2 accepted: register SIGHUP as well as SIGINT/SIGTERM before work. All three
  are now tested against observed running calibration Cargo jobs, with joined
  cleanup and no owned containers/volumes left. SIGKILL/crashes are excluded.
- F3 accepted: deadline-driven cancellation maps runtime checks to deadline;
  signals retain interrupted, with cancellation winning an overlap.
- F4 accepted: signal observation continues through bounded delivery. Observation
  and gateway work always join first. Only the response writer can outlive its
  await on a blocked reader; it owns bytes/stdout and ends with process exit.
  Explicit ADR exception,5s output timer and100ms runtime shutdown. Two real
  prefilled64KiB pipe cases pass with no drain/forced kill: signal and timer.
  Opus follow-up confirms the resource ownership distinction is sound.
- F5 accepted as stronger coverage: configured signed-catalog output is checked
  for absence of the host path and private-temp prefix, beyond passive unit checks.
- F6 conditional concern rejected: application/src/execution.rs ExecutionError
  is an existing closed fieldless enum. Debug cannot contain a path/stderr/secret.
  Preserve the previous capabilities JSON reason bytes and its nine CLI cases.
- F7 accepted: overflow fallback preserves measured duration; serialization failure
  maps Internal separately from OutputLimit. Normal typed serialization has no
  arbitrary failing serializer.

Follow-up: active worker JoinError now reports cleanup_uncertain; merely joining
an unwound worker does not attest explicit gateway cleanup. A new unit test covers
active vs passive classification. Writer explicitly flushes. Queued repeated
signals may discard the report only after cleanup; this is an intentional delivery
failure, documented with exit1. Rare runtime/signal setup failures can exit1 before
JSON exists; no diagnostic was completed. Observation remains cooperative, including
uncooperative native hangs, as required rather than detaching resource owners.

SecureProjects::new opens/validates roots through the existing safe adapter; it
creates no gateway or catalog administration. Parser extraction was reviewed
against the exact previous main.rs and all historical CLI tests are retained.
Catalog/runtime evidence types contain identities/facts, not supplied host paths.
Concurrent import retry exhaustion is the existing provider's truthful unavailable
observation, not an invitation to acquire an administrative lease.

Final core initially exposed a race in the historical closed-output test: closing
only our UnixStream reader descriptor is insufficient if a concurrently spawning
child briefly holds a duplicate before exec. The harness now shuts down the endpoint
before dropping it, retaining every assertion and production behavior. Final core
passes645 tests/10stages. Actual doctor gate4 active +2 stalled output cases also
passes on final production source. All-features/all-targets Clippy was repeated on final source after the native
release build freed its Cargo slot, and passed; its own hash is in the summary.

No confirmed actionable security/correctness finding remains in this cut. M1
closure still requires distribution, native platform/client and experimental gates.
