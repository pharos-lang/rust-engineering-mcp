# M1-17 stock Codex model-directed qualification

Status: **passed** on 2026-09-04 for the final core server binary SHA-256
`ebcb292c71d863aabb900874651230d0a16d5c93f68da84afb84bd89f4977edf`.

Codex CLI/[app-server](https://learn.chatgpt.com/docs/app-server) 0.153.0 ran two ephemeral model turns with `gpt-5.6-sol`
at medium effort. The controller used the app-server protocol, generated and
validated its exact request schemas, submitted a closed effective configuration,
disabled network access and exposed only the per-phase product tools. This is the
documented embedding boundary for Codex integrations; the protocol client remains
responsible for lifecycle and event handling.

The missing-runtime phase allowed only `rust.project.open` and `rust.check`. The
model called both and observed the product's exact structured `SANDBOX_DENIED`
result without editing the fixture or touching Docker. The repair phase allowed
only open, inspect, check and quality gate. The model opened and inspected the
fixture, called check, observed `E0502`, emitted one source-only file change,
reopened the project and called check again successfully in the pinned Rust
1.98.1 Linux ARM64 image.

Plan schema v4 binds each phase to the exact descriptor map it exposes, rather
than claiming a full inventory hash that the phase never observed. The controller
also binds every sampled descendant to an approved executable path and plan hash,
requires observation of `codex-code-mode-host`, rechecks candidate hashes after
both turns, verifies source/auth/canary integrity and removes its owned private
tree. Its 39 discriminating tests pass, including descriptor drift, unapproved
descendant and post-run binary-tamper negatives.

The [sanitized receipt](m1-17-codex-model/receipt.json) binds the private raw receipt
and transcript by SHA-256 without publishing local paths or account telemetry.
The raw receipt SHA-256 is `629fe2aa837218b7df839c936c11978bab4ba6aa00fa64fabc45e961aa3ab3b5`;
the raw transcript SHA-256 is
`318c8600863b86ba2b6caed65b88df6779ce7c921c24ea7f33483899a9bff45c`.

Two independent Opus 5 High reviews accepted the corrected qualifier with zero
P0/P1. Residual sampling races can cause a conservative false failure but do not
create a passing receipt; kernel-level process event attribution is not claimed.
The review trail is preserved in
[M1 closure stock Codex qualifier](../reviews/M1-closure-codex-qualifier-claude-opus-5.md).
