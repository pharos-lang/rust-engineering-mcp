# M4 hardening: preserved failed attempts

These receipts preserve failed qualification attempts. They are not passes for M4.

- Tampered plugin attempt 1: the harness applied a 1 MiB file limit to a Docker copy of the approximately 8 MiB approved binary. Cleanup passed. The corrected harness copies a bounded TAR through stdout and writes the validated member in the parent.
- Tampered plugin attempt 2: Docker printed a --pause deprecation message before the committed image ID. The old parser rejected it; image discovery omitted --all and incorrectly reported cleanup verified. cleanup-repair.json explicitly invalidates that claim and records removal of the exact untagged image after inspecting its matching ownership label. commit-diagnostic.json records the actual CLI output from a separate never-started, owned fixture, also removed. The original receipt is preserved unchanged.
- Native MCP canary attempt 1: ordinary panic under the fixture's optimized profile was classified unclassified because of Miri's exact opt-level warning. ADR-072 records the narrow classifier correction and discriminating negative tests. Later successful receipts are separate evidence.

No credential data is included. These are controlled fixtures, not production incident claims.

- Core attempt 1: stale cloud-metadata fixture SHA; only its corpus entry was corrected, with the denial oracle preserved.
- Core attempt 2: strict stdout observer failed once; cause unknown because the former assertion discarded its received variant. [Diagnostic disposition](stdout-diagnostic/disposition.md) preserves 30 isolated passes and five full protocol suites. The new diagnostics retain the same Disconnected-only oracle and 10-second deadline.
- [Full attempt 1](full-attempt-1/disposition.md): legacy model qualifier cleanup failure, cause unknown. Ten detailed isolated reproductions passed both phases; no oracle/source change. Preserve `phases.repair.cleanup` if it recurs.
- [Full attempt 2](full-attempt-2/receipt.json): first 27 steps passed, then semantic failed on an empty temporary E5 directory. [Recovery](../M4-e5-local-recovery.json) reverified five exact existing local assets without acquisition. The [driver](../M4-full-gate-resume-driver.py) executed all six remaining steps through the original gate runner on the identical source inventory; [final full](../M4-full-gate.json) records 27 retained + six resumed. The original failure is unchanged.
- `native-before-final/` preserves scanner/Miri and MCP receipts superseded by the current source-bound full run. These remain historical rather than proof of the final bytes.

Successful core/full/native/client receipts are separate evidence. None of these
records converts a failed attempt into a passing attempt or proves an unobserved
root cause.
