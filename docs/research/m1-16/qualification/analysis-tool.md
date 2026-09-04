# M1-16 descriptive analyzer — bounded worker result

Task: build a read-only analyzer of the 24 prospective runs using current runner,
broker and evaluator schemas. No model, Docker, oracle execution or corpus edits.

Result: implemented `analyze.py --results DIR --output DIR`. Output must be fresh
and outside the results tree. It writes `analysis.json`, `summary.md` and
`sources.md`; reads and writes traverse directory handles with no-follow semantics.
Use canonical absolute paths (macOS /var is a symlink; use its /private/var target).
Original outcomes remain unchanged and linked/hashed. No measured-study output was
generated; all test fixtures are explicitly synthetic and temporary.

Files changed:
- target/m1-16-controller/analyze.py
- target/m1-16-controller/test_analyze.py
- target/M1-16-analysis-tool.md

Tests executed:
`python3 -m unittest discover -s target/m1-16-controller -p test_analyze.py -v`
8/8 passed (0.055s); CLI `--help` also passed. Tests cover exact current24-run order
without importing execution adapters, unknown usage, missing runs, passing candidate
with failed infrastructure, token subset/partial coverage, paired discordance,
wrong evaluation binding/path, raw-stage timing deduplication, fallback/read/oracle
extraction, immutable source hashes, fresh output and rejected symlink artifacts.

Evidence/schema decisions:
- Current evaluator writes `evaluation/evaluation.json`; `--finalize` writes
  `evaluation/reviewed-evaluation.json`. A `final.json` is not promoted as a result.
- Identity binds all planned run fields, freeze SHA, candidate count, candidate
  SHA/kind/index/reference. A reviewed passing result is distinct from a passing
  participant/cleanup status. Deterministic false in pending evaluation is retained;
  true without reviewed evidence remains unknown. Missing review is never invented.
- All24 planned slots and12paireditems remain visible. Groups cover arm,
  repair/selection, task family R01..S04, language, arm/family and arm/family/language.
  Repair language is English; selection language follows the prompt item suffix.
- Success outputs include planned, passed, failed, unknown, evaluated denominator,
  evaluated-only rate, and observed passes/planned (explicitly not an imputed rate).
  Paired both-pass/both-fail/A-only/B-only/unknown counts remain descriptive; failed
  infrastructure is retained per arm even with an independently passing candidate.
- Task, participant, turn, run, infrastructure and oracle verification states are
  separate. Missing/invalid/started-without-receipt runs remain distinct.
- Candidate submissions and submission_revisions=max(n-1,0) are operational counts,
  not a semantic definition of repair loops. Broker validation_requests separately
  count the actual protocol validation cycles; compound stages do not inflate them.
- Tool counts come from broker request records; file/catalog/resource reads and
  search/inspect calls remain separate categories. Recorded0 differs from unknown.
- Usage uses participant.usage.total; cached input is an input subset and reasoning
  output an output subset, never added twice. Each metric records known/unknown,
  observed sum/mean/median/min/max and usage-coverage distribution. None stays null.
- Runner elapsed, participant elapsed, cleanup and broker-validation elapsed are
  distinct. Raw gateway duration is only explicit execution.duration_ms. MCP tool
  duration is not relabelled gateway-only time. All nested duration observations are
  retained with JSON pointers; nested envelopes are not summed. Broker-validation
  results take precedence over duplicate participant events for validation calls.
- First/final candidate references, distinct oracle candidates and their stage/audit
  results are preserved. Search extracts retain requested/effective mode, fallback,
  snapshot and result-window evidence. All original files under planned run dirs
  remain linked/hashed, including fields this analyzer does not interpret.

Risks:
- Gateway-only duration may remain unknown for the MCP arm because its public DTO
  exposes total tool duration rather than the raw gateway timer. This is explicit,
  not a zero or timing comparison claim.
- Read limits:16MiB for parsed JSON/event files,64MiB per other hashed artifact.
  Unreadable, unstable, symlink or over-budget artifacts are reported. Unplanned
  directory entries are listed and excluded from the fixed24-slot denominator.
- Timing and caching remain position-confounded; ES/EN share task families. No
  hypothesis tests, population statistics, confidence intervals or causal claims.
- Parent must include analyze.py in the source freeze; this worker did not edit
  runner freeze requirements or any existing controller/source/corpus artifact.

Open issues: principal schema/diff review and eventual analysis of actual runs.
No measurement or utility result has been claimed by these synthetic tests.
