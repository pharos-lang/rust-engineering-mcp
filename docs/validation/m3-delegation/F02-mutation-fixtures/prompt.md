# Package F02 — Fixtures and oracle tables for M3-05 mutation testing (Luna)

Task / ID: F02. Objective: create the fixture corpus and expected-outcome tables the `rust.mutation.test` vertical (package I05) will need, plus the hostile-source cases that prove containment. No product code, no Docker, no cargo execution against the fixtures on the host.
Model/CLI/effort: GPT-5.6 Luna via codex exec, medium, workspace-write.

## Common rules
Repo /Users/cburgosro/Projects/rust-mcp, branch `ai/m3-quality` (uncommitted, shared with other workers). Orchestrator: Claude Opus 5 main session. No commits/merge/push/installs/downloads/Docker. Own ONLY: `fixtures/mutation/**` (new), `docs/validation/M3-05-fixtures.md` (new). Do not touch crates/, scripts/, other docs, or other fixtures. Never run workspace `cargo fmt`; format fixture sources with `rustfmt --edition 2024 <file>`.
Read first: `fixtures/nextest/README.md` and one of its crates (the exact conventions for an isolated edition-2024 workspace with a hand-written `Cargo.lock` version 4 and no dependencies — copy that convention exactly), `docs/roadmap/m3-quality.md` (mutation paragraph), and the official cargo-mutants documentation the orchestrator captured at `/private/tmp/claude-501/-Users-cburgosro-Projects-rust-mcp/df0fc04b-d468-46d9-b6f0-e6ac1541e3c8/scratchpad/orch/pkg-R01-plugins/sources/mutants-{out,exit,timeouts,inplace,baseline,nextest}.txt` plus the pinned help at `docs/validation/m3-provisioning/help/cargo-mutants-help.stdout` (cargo-mutants 27.1.0 as built into the M3 guest image).

## Fixtures to create (each an isolated workspace, dependency-free, deterministic)
| Directory | Shape | Expected mutation outcome (the oracle) |
| --- | --- | --- |
| `caught-all` | 2–3 small functions (arithmetic + boolean) with tests that assert exact values | every viable mutant caught; exit 0 |
| `missed-one` | same, plus one function whose test only asserts it does not panic | at least one missed mutant; exit 2; missed count ≥ 1 with the function named |
| `timeout-loop` | a function whose mutated form loops forever (e.g. `while x != target` where the mutant makes the condition never true), with a test that calls it | at least one timeout outcome; exit 3 when it dominates |
| `unviable` | a function whose mutation cannot compile (e.g. a generic/type-level construct where the default replacement does not type-check) | at least one unviable mutant; unviable count ≥ 1 |
| `baseline-failing` | a test that fails deterministically before any mutation | baseline failure; exit 4; no mutant results are trustworthy |
| `hostile-writer` | a test that attempts, and tolerates failure of: writing outside its own directory (`../canary.txt`, `/tmp/…`, `/source/../`), spawning a detached child process that outlives the test, opening a TCP connection to 127.0.0.1 and to a public address, and printing a large burst of output including fake `mutants.out`-looking lines | all attempts must be contained by the sandbox; the fixture must still terminate; forged output must never be trusted as outcomes |
Keep every fixture tiny so a full mutation run stays inside the per-mutant 60 s / total 3600 s budgets; `caught-all` and `missed-one` should have fewer than ten viable mutants each. Add a canary file in the repo-relative fixture root that the containment test can check for absence of modification.

## Deliverables
1. The seven fixture workspaces with `Cargo.toml` (`[workspace]` table, edition 2024, `publish = false`), hand-written `Cargo.lock` (version 4, single package), `src/lib.rs`, and a per-fixture `README.md` line explaining the intent.
2. `fixtures/mutation/README.md`: the oracle table above, expanded with the exact expected counts where they are deterministic, the exit code, and an explicit note that exit codes are hypotheses from the official docs until the I05 calibration run against the pinned binary records them.
3. `docs/validation/M3-05-fixtures.md`: inventory with SHA-256 of every created file, the containment expectations for `hostile-writer` (what must be denied: writes outside the private copy, surviving children, any network, unbounded output), and the list of oracles the I05 Docker tests must assert.
4. Checks: `rustfmt --edition 2024 --check` on every fixture source, `git diff --check`, and a JSON/TOML structural sanity check of the manifests (python `tomllib`). Report exit codes.
Delivery: Task, Result, Files changed (+SHA-256), Tests executed, Evidence, Risks, Decisions, Open issues.
