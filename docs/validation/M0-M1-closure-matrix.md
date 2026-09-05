# M0/M1 closure traceability matrix

Updated: 2026-09-05. Status: **M0/M1 Done**. This matrix is the live closure view requested by the M0/M1
handoff prompt. It distinguishes code implementation, executable evidence, owner
decisions and external delivery state. Historical receipts remain unchanged.

## Verified starting state

| Item | Live observation | Evidence type |
| --- | --- | --- |
| Local checkout | `b6bbfdb126518c61ac12f133fd3e3c0a15113c25` was clean on `main`; it advances the expected `e8a2336…` only with the closure-prompt merge. | Git observation |
| Public source | `d2192037e55362e2834969db627844c2f734a50f` on `pharos-lang/rust-engineering-mcp`. | GitHub live |
| Public CI | Run `33928952807` passed Linux x86_64, macOS ARM64, Windows x86_64 and supply chain. | GitHub live; source portability only |
| Branch protection | Strict required checks, admin enforcement, no force push and no deletion. | GitHub live |
| Tags/releases | None at the start of closure. | GitHub live |
| Contract | Exactly thirteen tools and thirteen checked-in schema snapshots; no `rust.dependencies.inspect`. | Code/contract tests |

The self-auditable inventory is: `rust.project.open`, `rust.project.inspect`,
`rust.toolchain.inspect`, `rust.check`, `rust.fmt.check`, `rust.clippy`,
`rust.test`, `rust.dependencies.audit`, `rust.diagnostics.explain`,
`rust.quality.gate`, `rust.catalog.status`, `rust.crate.search` and
`rust.crate.inspect`.

## M0 deliverables

| Deliverable | Implementation | Evidence | Owner decision/external dependency | Closure |
| --- | --- | --- | --- | --- |
| Repository, ADRs and architecture | Present through ADR-048; hexagonal boundaries retained. | M0-01/02 and architecture checks. | None open. | Done |
| Domain model and typed contracts | Implemented in domain/application without rmcp, Cargo, SQLite or LanceDB dependencies. | M0-02/07 gates and schema tests. | None open. | Done |
| Project validation and host roots | macOS/APFS handle-relative no-follow implementation; other OS fail closed. | M0-04 adversarial tests. | ADR-048 makes macOS ARM64 the positive 0.1.0 host. | Done |
| Execution Gateway and sandbox capabilities | Single typed Docker gateway, clean env, network/filesystem/process/resource enforcement and cleanup. | M0-05/06 and later Rust execution gates. | Docker Linux ARM64 is a guest containment profile, not a native Linux host claim. | Done |
| MCP stdio/rmcp/JSON-RPC/schema boundary | Implemented with five negotiated wire versions and bounded transport. | M0-03/07 protocol and contract suites. | None open. | Done |
| Artifact store | Bounded, ephemeral, owner-bound Resources path. | M0-10a plus M1 live Resource tests. | None open. | Done |
| SQLite/FTS5 catalog | Authoritative bounded snapshots and migrations. | M0-08 and M1-10..13. | No official 0.1.0 catalog is distributed. | Done |
| LanceDB/EmbeddingProvider | Derived index and verified local E5 provider behind `local`; lexical fallback is explicit. | M0-09, M1-10..12 and bounded ES/EN benchmark. | Local/model bytes are not release artifacts under ADR-048. | Done |
| Logging, fixtures and CI | stderr-only logging, hostile fixtures, local core/full gate and portable GitHub CI. | M0-10..12 and public run `33928952807`. | Positive host scope is explicit in ADR-048. | Done |

No concrete contradiction from the current audit invalidates a historical M0 Done
row. Platform limitations are part of the fail-closed contract, not hidden passes.

## M1 verticals

| ID | Deliverable | Implementation/evidence | Decision or external state | Closure |
| --- | --- | --- | --- | --- |
| M1-01 | `rust.project.inspect` | Integrated, contract and real Docker runtime evidence. | Qualified host profile only. | Done |
| M1-02 | `rust.toolchain.inspect` | Integrated, installed runtime observation. | Qualified host profile only. | Done |
| M1-03 | `rust.check` | Integrated with structured diagnostics and Resources. | Strict gateway required. | Done |
| M1-04 | `rust.fmt.check` | Integrated with bounded diff/source immutability. | Restricted gateway required. | Done |
| M1-05 | `rust.clippy` | Integrated with closed profiles and hostile proc-macro/build evidence. | Strict gateway required. | Done |
| M1-06 | `rust.test` | Integrated with R2 timeout/cancel/kill-tree evidence. | Strict gateway required. | Done |
| M1-07 | `rust.dependencies.audit` | Integrated RustSec owned-data path and source-aware lock graph. | Snapshot must be host supplied. | Done |
| M1-08 | `rust.diagnostics.explain` | Integrated against the approved compiler. | Restricted gateway required. | Done |
| M1-09 | `rust.quality.gate` | Integrated single-capture fast/standard composition. | Compound stage policy retained. | Done |
| M1-10 | Catalog CLI | Signed import/sync/rebuild mechanics, floor and recovery implemented. | No official catalog in 0.1.0; fixture remains tests only. | Done |
| M1-11 | `rust.catalog.status` | Integrated component identity/freshness and degraded states. | No packaged catalog/model. | Done |
| M1-12 | `rust.crate.search` | Integrated lexical/semantic/hybrid/fallback behavior. | Semantic positive path is source-qualified, not in the core archive. | Done |
| M1-13 | `rust.crate.inspect` | Integrated authoritative paged facts. | No packaged catalog. | Done |
| M1-14 | CLI/doctor | Integrated passive/active diagnostics and cleanup. | Final archive smoke still required. | Done |
| M1-15 | Documentation/release | Product license/source channel and local core archive passed; [tagged public receipt](m1-17-public-release.json) binds inventory/SBOM/notices/install smoke, attestations and release. | GitHub Release `v0.1.0` published for macOS ARM64. | Done |
| M1-16 | Bounded utility experiment | 24/24 pilot plus native retrieval benchmark completed; ceiling effect and higher observed MCP cost retained. | No equivalence or causal product-value claim. | Done |
| M1-17 | 0.1.0 qualification | Final Inspector, stock Codex, full 23/23 gate, Opus review, public CI, SonarCloud and downloaded-artifact smoke passed. | Tag `v0.1.0`, OIDC attestations and stable release verified. | Done |

## Specification M1 Definition of Done

| Requirement | Current evidence | Classification | Status |
| --- | --- | --- | --- |
| Input/output schemas documented and derived from Rust | Thirteen snapshots, typed DTOs and `docs/tools.md`. | Implementation + tests | Satisfied |
| `structuredContent` primary responses | Contract/protocol suites and Inspector 13/13. | Implementation + evidence | Satisfied |
| Breaking-schema tests | Snapshot and canonical descriptor tests. | Tests | Satisfied |
| Linux/macOS/Windows core/protocol/catalog CI | Final public run `33948778666`; non-macOS positive I/O suites are intentionally unavailable. | Portable evidence + ADR-048 | Satisfied for declared portable scope |
| Security tests for each advertised capability/fail-closed elsewhere | macOS/APFS and Docker adversarial gates; non-macOS unsupported tests. | Security evidence + scope decision | Satisfied for advertised capabilities |
| Structured output and diagnostics | Typed contracts, rustc/Cargo cases and clients. | Implementation + evidence | Satisfied |
| Timeouts/cancellation/kill-tree | M1-03/06/09/14 and full gate. | Security evidence | Satisfied |
| Filesystem restrictions | No-follow APFS/root races and bounded imports. | Security evidence | Satisfied for positive host |
| Network-disabled operation | Positive/negative IPv4/IPv6 and guest containment controls. | Security evidence | Satisfied for positive host |
| Offline snapshot import | Authenticated import/floor/recovery M1-10. | Implementation + evidence | Satisfied |
| SQLite lexical and LanceDB semantic search | M1-12 native source-bound gate and bounded benchmark. | Implementation + evidence | Satisfied on positive host |
| Fallback without valid semantic assets | M1-11/12 degraded tests and core behavior. | Implementation + evidence | Satisfied |
| Provenance/freshness | Domain contracts and M1-10..13. | Implementation + evidence | Satisfied |
| Integration tests | Core/full suites and [final full v2 receipt](m1-17-final-gate-v2.json). | Evidence | Satisfied on final source |
| MCP Inspector | Inspector 2.5.0 historical UI plus final core-binary CLI receipt. | Third-party client evidence | Satisfied with documented CLI limitations |

## Closure result

Protected PRs #8/#9, final public CI and SonarCloud on
`452acdbf3a634d2cc0b9d153db09718237625b9d`, tag-bound workflow `33948798048`,
three verified attestations and the stable GitHub Release satisfy the remaining
delivery state. The [final public receipt](m1-17-public-release.json) is authoritative.

M2 remains unimplemented and outside this matrix. Planning starts from
[`docs/prompts/plan-m2-m8.md`](../prompts/plan-m2-m8.md).
