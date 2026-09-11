# M1 closure stock Codex qualifier — Claude Opus 5

Date: 2026-09-04. Security/architecture review trail for
`scripts/codex-model-qualifier.py` and its tests. Every invocation is read-only with
explicit `claude-opus-5`, high effort, tools/MCP disabled and no session persistence.

## Review trail

The first v1 review rejected a single-session controller because it used an
incomplete raw-event denylist, inherited configuration and credential state, did
not bind lifecycle items to the turn, could not honestly observe configured and
missing runtime in one static server, executed unstaged paths, recorded requested
sandbox facts as effective facts, granted all tools and lacked integration tests.
Session `1f4cd2cb-99cc-43f0-84b5-f6a77c1bb265`, result UUID
`2f3cd5a3-a50c-4777-a4fd-011a0c15d891`.

The v2 follow-up rejected the first two-session rewrite with four P0 and eleven P1:
unpinned/mismatched Docker CLI and daemon; insufficient discrimination and positive
runtime evidence; no protocol-schema basis for submitted sandbox policy; weak raw
patch ordering; cross-phase fixture contamination; destructive cleanup and auth
rotation risk; incomplete/unretained effective-config evidence; incomplete process
cleanup; unconstrained external stores; narrow secret scanning; and circular fake
integration coverage. Session `d0f72b81-bf56-4223-afc5-3f51929122fb`, result UUID
`74dbe1ff-1f6e-456a-a8f5-329a280a8326`.

The v3 follow-up reviewed qualifier SHA-256
`7818ace895fc887f4b773a9f14c4cf1569327af32dd2fd2a52bff8be9e5695e9` and test
SHA-256 `516deff989a36e90193374e1c480f4ee7d28aa220c11a6678ec0e155f3ce04da`.
It rejected execution with two P0 and nine P1: outgoing requests were not
validated against the generated protocol subschemas; Docker-positive evidence
was not independently corroborated at the daemon boundary; residual secret
handling could retain a contaminated transcript; PID cleanup could target a
reused PID; snapshot budgets could prevent cleanup; feature/config closure was
not candidate-bound; client requests and patch content were not fully bound into
the transcript; discovery was not required; and the fake suite lacked independent
negative cases for decisive assertions. Session
`337b82a7-b689-40fb-8250-a44d7be29df4`, result UUID
`77a867ad-ae9c-465b-9957-24d3db73f12d`, canonical model
`claude-opus-5`, high effort, 41,417 output tokens including 31,486 thinking
tokens, zero web/tool calls and no permission denials.

The v4 follow-up reviewed qualifier SHA-256
`76da4f366163ad2ed7bbc41c63d7ccdc27eeb94b8d73c6b6b26b694aff6169f4` and test
SHA-256 `f9f16162969385fd1c94e3f84b3554fb5d373df5ed8b42228b99567c6d835d7e`.
It confirmed the protocol-schema, Docker lifecycle, secret containment, PGID,
request transcript, patch binding, discovery and independent-negative-test fixes,
but rejected execution with one P0 and two P1: the private inventory still applied
the fixture byte budget to staged binaries; the same fixture budget conflicted
with an allowed `target/` subtree; and the discovered feature world was closed by
membership but not forced deny-by-default for every non-approved feature. Session
`2d7855f5-30b2-41e1-915f-d5b0c5fc87de`, result UUID
`34c44a8b-f4e3-4acd-9d6f-5b3575b9d4f3`, canonical model
`claude-opus-5`, high effort, 38,772 output tokens including 29,772 thinking
tokens, zero web/tool calls and no permission denials.

The v5 follow-up reviewed qualifier SHA-256
`931b65e130adf688c7aa1d86ee4d6c109c0d47866079125d89f3d483b22517b8` and test
SHA-256 `91c741e039cbf50b9e37a9659b3ba0d14653af345bf7bbf8533cf5a079c5a5cd`.
It confirmed that all three v4 blockers were resolved and found no P0, but retained
one P1: the private tree and generated schema still shared the fixture's 512-entry
budget, which could turn a valid real run into a final false negative after a large
state tree. Session `cc8b13c1-8053-480b-a817-201b66a2619c`, result UUID
`5c5c85cf-db36-4e94-8149-ed3adce38dfa`, canonical model
`claude-opus-5`, high effort, 43,262 output tokens including 35,119 thinking
tokens, zero web/tool calls and no permission denials.

The v6 focused review accepted qualifier SHA-256
`71c23da6915caa629f09f0e83198b254038dfd1bce73104536271e8c297753ac` and test
SHA-256 `a12cd060638e044c30f2b010a7592ff5dbe165a1e70557c33b4aaf50a6f605cc`
with zero P0/P1. It confirmed distinct bounded budgets for fixture, schema bundle,
private inventory and cleanup; the 600-entry-per-subtree E2E; complete feature-world
deny-by-default; and absence of regressions in protocol, Docker, auth, lifecycle,
transcript, patch binding and evidence. Residual P2 items are conservative
false-negative/operability risks and do not permit a false pass. Session
`3db47956-46ac-441f-8061-2a78be95f017`, result UUID
`81dfc9e4-5be2-4b5e-abcc-a32b7d8ed26f`, canonical model
`claude-opus-5`, high effort, 31,347 output tokens including 26,483 thinking
tokens, zero web/tool calls and no permission denials.

A later complete-source review of the Codex 0.153 adaptations rejected schema v3
with two P1 findings: the receipt presented a full thirteen-tool descriptor hash
that neither restricted phase had observed, and the staged code-host hash was not
bound to an observed executing process. Session
`865a75e6-bba3-4d35-8305-99bb36ffff88`, result UUID
`af1644a6-5359-4902-bd3c-c808db452cb6`, canonical model `claude-opus-5`, high
effort, zero tools/web/permission denials.

The schema v4 review accepted qualifier SHA-256 from the first P1 correction and
reported zero P0/P1. It confirmed per-phase descriptor approval, observed
path-and-hash binding for every sampled descendant, mandatory code-host
observation and discriminating positive/negative tests. Session
`0883b75d-749c-4cc3-aec5-0e96a2fdf430`, result UUID
`5bd712cf-7020-46a4-a997-4f0f110cedb7`, canonical model `claude-opus-5`, high
effort, 20,332 output tokens including 15,742 thinking tokens, zero tools/web,
permission denials or subagents. It authorized execution.

The principal additionally replaced self-derived allowlist hashes with plan hashes,
added a post-run candidate identity check, combined PGID and transitive PPID
enumeration, and made unresolvable live descendants fail closed. The final focused
review accepted qualifier SHA-256
`72419d89279efc67f469f59637c81fe60da5927aba7629996c7e232a4102c93a`
and test SHA-256
`370f9dafe4263926c6be46f3ff552abe2338b7e3def8e4dcb21e93d5c6860049`
with zero P0/P1. Session `9e03f2aa-f7f8-4b5b-9c02-4adbbecda8dd`, result UUID
`af80c4c6-5f44-4da7-a5ac-540d9fef5144`, canonical model `claude-opus-5`, high
effort, 8,404 output tokens including 5,399 thinking tokens, zero tools/web,
permission denials or subagents. Remaining P2s are conservative false-failure and
sampling limits, not false-pass paths; execution remained authorized.

## Accepted execution boundary

The real turn is authorized only for a plan that independently pins the reviewed
qualifier, candidate server, Codex CLI, Docker CLI/daemon/image, generated schema,
discovered feature world, exact effective configuration and prompt hashes. A
preflight must confirm Docker quiescence and the observed event/feature formats.
Any deviation is a failed qualification, not evidence to waive an invariant.

The rejected iterations remain part of the audit trail. Schema v4 and its final
focused follow-up resolved every P0/P1 before the retained final model run. Where
the app-server protocol has no effective-policy echo, the qualification uses the
exact generated schema, submitted request, strict configuration and observable
confinement controls without inventing an echo field.
