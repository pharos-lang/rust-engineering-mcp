# Legacy Codex qualifier test observation

Full attempt 1 stopped before Docker: the large-staged-binary unit fixture returned TransportCloseError during repair cleanup. The test's failure message retained generic errors but discarded the detailed phase cleanup before removing its temporary directory. Core on the same source had passed all 39 qualifier tests.

Ten isolated reproductions on unchanged source passed both phases. Their detailed process-identity and cleanup summaries are preserved here; they observed the expected native code-mode helper, joined threads, no residual PIDs, no forced cleanup and no recorded transport failure. The fixtures used only fake credentials under a private temporary directory outside the repository; their owned temporary trees were removed after retaining the summaries.

No production or harness condition was relaxed. The exact cause of the single failure is unestablished. Technical Owner follow-up: preserve the detailed phase cleanup in a future failure before diagnosing a scheduling or process-exit race. A fresh complete full gate remains required; this failure never counts as passed.
