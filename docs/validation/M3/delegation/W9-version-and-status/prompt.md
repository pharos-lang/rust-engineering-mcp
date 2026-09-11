# Package W9 — Bump the checkout to 0.3.0-dev, re-qualify, and repair the M3 status board (validator/integrator delegate; sole Docker owner)

M3 is merged (`main` at `b4a4213`, merge commit `57c40373`, branch `ai/m3-quality` preserved at `93991c4c`). Two defects found afterwards need fixing on `main`.

## Defect 1 — the version the binary reports contradicts every document
`Cargo.toml` still declares `version = "0.2.0-dev"`, so the merged binary identifies as `0.2.0-dev`. Eight documents already state the checkout is `0.3.0-dev`: `README.md` (three places), `SECURITY.md`, `CHANGELOG.md`, `docs/architecture.md`, `docs/client-configuration.md`, `docs/security-model.md`, `docs/compatibility.md` and `docs/implementation-status.md`. The roadmap defines M3 as the 0.3.x line. **The owner authorized bumping the code**, so the documents become true rather than being watered down.

Do this:
1. Bump the workspace version to `0.3.0-dev` and update `Cargo.lock` accordingly (`cargo check --workspace --locked --offline` will refuse if the lock disagrees; regenerate it the minimal way, `--offline`, without resolving new versions — if the lock cannot be updated offline, stop and report rather than going online).
2. Find every place the version is asserted in code or fixtures and make them agree: the CLI version output, any snapshot or test that pins `0.2.0-dev`, and the client/protocol harnesses. Search the whole repository for `0.2.0-dev`, excluding `target/`, historical M2 receipts and the `docs/validation/m3-delegation/` transcripts, which are dated records and must not be rewritten.
3. Do **not** touch the M2 or M3 receipts already written: they describe bytes that were qualified at the time and their `source_inputs` hashes must stay as they are. The new qualification below produces its own receipts.

## Defect 2 — the M3 section of the status board is misplaced and incomplete
In `docs/implementation-status.md`:
- The `## M3 — Quality` section sits at the very end of the file, wedged between historical M1 notes (`M1-13 integrada…`) and the M2 historical notes, instead of following the `## M0 — Foundation` and `## M1 — MVP / 0.1.0` sections. Move it so the milestone sections read in order, and keep every historical note intact and in its own place — do not delete or reword any M0/M1/M2 history.
- Its decision table stops after the provisioning row and is immediately followed by unrelated M1 text. Complete it: add `ADR-064` (quality seccomp profile) and `ADR-065` (coverage target volume, as amended), both Accepted 2026-09-06 by the M3 orchestrator, with links and their qualification evidence, and close the table properly.
- There is no `## M2` section although M0, M1 and M3 have one, which is what makes the tail of the file read as a jumble. Add one that gathers the M2 milestone state from the notes already in the file — summarising what is there, inventing nothing — and leave the historical trail beneath it.
- While you are in the file: verify every M3 number against the receipts it cites and fix any that disagrees.

## Then re-qualify, because the version is a qualified byte
1. `cargo fmt --check`, `cargo check --workspace --all-targets --locked --offline`, `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`, `cargo test --workspace --locked --offline`, `python3 -B scripts/check-architecture.py`.
2. `python3 -B scripts/gate.py core --report docs/validation/M3-core-gate.json` and `python3 -B scripts/gate.py full --report docs/validation/M3-full-gate.json` with `RUST_MCP_TEST_SOCKET=/Users/cburgosro/.docker/run/docker.sock`, `RUST_MCP_E5_DIR=/private/tmp/rust-mcp-e5-m009/onnx`, `ORT_LIB_LOCATION=/Users/cburgosro/Library/Caches/ort.pyke.io/dfbin/aarch64-apple-darwin/612739f75438dc0a075461e1fb454226b4a1eb175e60a7271ba966bbbb972cd4`. Preserve the current receipts under a `-v0.2.0-dev` suffix before writing the new ones, and say which. The full gate carries the M3 runtime and Rust security stages; copy their receipts to the canonical names as the previous packages did.
3. Docker hygiene after each gate: zero containers and zero volumes labelled `org.rust-mcp.execution=true`, with the command output.

## Record and push
Update `docs/validation/M3-07.md` and `docs/validation/M3-integration.json` with a short section for this change: what was bumped, why (the documents already promised it and the roadmap defines M3 as 0.3.x), that it invalidated the previous receipts, and the new gate counts, durations and receipt hashes. Then commit on `main` — one commit for the version bump and its re-qualification, one for the status-board repair if that reads better — with the repository's message style and this session's two attribution lines, and push to `origin/main`.

## CRITICAL operating instruction
The full gate takes about forty minutes. Start long commands in the background and poll from inside this same session until they exit. Never end your turn while a gate is running.

## Rules
Authorized: edits on `main`, `git add`/`commit`, `git push origin main`, `gh` reads, and the gates above. Not authorized: tagging, releasing, crates.io, force-push, history rewriting, deleting the branch, repository settings, or re-opening PR #14. Never weaken a stage, never credit a skip as a pass, never rewrite an existing dated receipt. Everything you assert must come from a command you ran.
Delivery: Task, Result, the version change and everywhere it propagated, the status-board repair, the gates with counts/durations/receipt hashes, which receipts you preserved and under what name, Docker hygiene evidence, Files changed with SHA-256, Risks, Decisions, Open issues.
