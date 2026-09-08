# Technical Owner disposition — confirmation snapshot

This is a scoped disposition, not M4 closure. The original external response is
preserved unchanged. Its verdict found no P0/P1 and two P2 evidence issues.

| Finding | Disposition | Evidence / remaining action |
| --- | --- | --- |
| P2-1 scanner receipt predates gateway bytes | Closed by final evidence confirmation | The original receipt remains historical. [Final scanner](../../validation/M4-scanner-native.json) passes seven cases on current source/config hashes within [runtime 19/19](../../validation/M4-runtime.json). |
| P2-2 scanner parser/port absent from execution fingerprint | Fixed in source | `security_gateway.rs` now includes `unsafe_scan.rs` and `unsafe_port.rs`; deny parser/port are also included for the same invariant. Final scanner/Miri reruns and confirmation bind these bytes. |
| P3-1 outer deadline shortens export reserve | Accepted limitation | The outer job deadline includes capture/vendor work and remains authoritative. Expiry fails closed and joined cleanup is mandatory. A future remaining-budget port would improve usable interpretation time; M4 does not promise a full requested duration solely inside the interpreter. |
| P3-2 overlarge JUnit archive taxonomy | Accepted follow-up | The export is bounded and never produces clean. Some oversized envelopes return InvalidMetadata rather than OutputLimit; improve the shared tar decoder's typed error without weakening validation in a later separately tested change. |
| P3-3 20 ms EOF scheduling grace | Accepted limitation | Unconfirmed drainage makes remaining files unavailable, never complete. Native corpus does not reproduce a per-file timeout; the receipt explicitly distinguishes helper IPC tests from native evidence. No unbounded wait is introduced to obtain availability. |
| P3-4 unmatched vendor files omitted | Intended scope | Only package roots authenticated as resolved graph members are selected. Unresolved packages in a larger vendor snapshot are outside the requested scan. Metadata root/graph validation is separately tested. |
| P3-5 project identities remain LLM-visible | Accepted residual | Conservative identifier syntax removes direct prose/control characters; names remain untrusted project metadata and are not instructions. Universal prompt-injection resistance is not claimed. |
| P3-6 clean coverage represented by counts | Documented limit | Tool semantics and ADR-072 limit clean to the selected tests/configuration; cfg(not(miri)) and omitted targets are not a safety proof. |
| P3-7 source Cargo config denied globally | Existing M1 boundary | This is not an M4 change. Existing capture policy and snapshots are preserved; diagnostics can be improved separately without relaxing containment. |
| P3-8 stale ADR counts/reserve | Corrected | ADR-068 now says 13/7; ADR-069 consistently reserves 25 s. |
| P3-9 application observation fields rely on trusted port | Accepted residual | Concrete adapter structurally binds capture/metadata/config; application verifies report, vendor, execution identity and JUnit presence. A malicious in-process adapter is outside the plugin threat boundary. |
| P3-10 extra absent cleanup containers | Accepted bounded overhead | At most four additional fixed daemon round trips; included in native measured cleanup. No authority or input-dependent amplification. |
| runtime constants on final image | Prepared prerequisite | `scripts/test-m4-inventory.py` reads exact image bytes without starting image code and verifies all six binaries plus the complete sysroot tree against the approved pins. Its [successful final receipt](../../validation/M4-runtime-inventory.json) verifies all pins. |

P3 follow-ups belong to the Technical Owner's explicit backlog; they do not weaken
M4 fail-closed contracts. No review statement is substituted for native, client or
final gate evidence. The required P2 native rerun and [final independent confirmation](../m4-final-evidence/review.md)
are complete; closure is recorded in the [handoff](../../validation/M4-handoff.md).
