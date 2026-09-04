# M1-16 corpus draft v2 candidate

Authored 2026-09-04. No Rust compilation, tests, model runs or measurements executed.
This corpus proposes the reduced exploratory protocol; protocol v1 remains the
tracked document until the principal approves an amendment before measurement.
Four repair tasks plus four selection intents in EN/ES, one pair per prompt:
12 pairs /24 runs, six candidates, six validation requests,15min/run. No efficacy claim.

Repair initial/ is the ONLY participant workspace seed. prompt.txt is participant
input. reference/, hidden/, oracle.json and this management directory are controller-
only and MUST NOT be exposed in the agent filesystem/read closure. Hidden tests are
mounted as tests/behavior.rs only by the trusted final-oracle controller. Initial
sources contain no tests. Agent may change only src/lib.rs; Cargo.toml/Cargo.lock
are immutable. Locks are authored std-only lockv4, not Cargo-generated evidence.
Use approved Cargo/Rust1.98.1 through the existing Execution Gateway to validate
both initial fault and reference patch before freeze. No host Cargo execution.

Reference repair is one example; success is behavioral plus immutable constraints,
not byte equality. Hidden files are not already complete runnable workspaces: the
controller must copy initial/ to a fresh workspace, apply one candidate lib.rs and
mount the oracle at the stated path. Test crate imports match package names.
Enforce no tests in src/lib.rs, no lint suppression/unsafe/dependencies separately.
Review/format fixtures and hidden harness through trusted gateway before measurement.

Selection facts.json contains16 genuine cached package/version manifests, with
source paths and SHA256. sources/ pins the inspected manifests/READMEs and selected
API source files. Original cached sources are research input, not a production
catalog or distribution release. Full dependency closure was not validated.
Declared MSRV is not tested compatibility; missing MSRV remains null. An explicit
metadata-only research acquisition fetched the16 exact official crates.io version
endpoints, recording raw bytes/URL/UTC/SHA256. Yanked and publication timestamps
now come from those responses; all16 were not yanked at acquisition. Identity,
license and declared MSRV matched cached manifests without divergences. The capture
spans 2026-09-04T16:01:22Z..16:01:41Z; it is not an atomic global registry snapshot.
Later consumption is frozen snapshot evidence, never ongoing live registry state.
observed_date is collection date, not package publication. Labels are task-specific metadata
judgments, not quality rankings or guarantees. Required license expressions are
recorded claims, not legal assurance; all licensing/redistribution review is pending.

Participant selection views must expose the same facts in both arms. Any SQLite/
embedding corpus must derive faithfully from these facts. schema1 yanked and
publication fields can now use acquired metadata; registry metadata acquisition
belongs to the trusted research preparation step, not the MCP runtime. No production
snapshot is authored here. acquire_metadata.py is preserved as acquisition evidence;
rerunning it would create a new capture requiring renewed review/hashes.

This draft was authored using local Python only. The arithmetic oracle contains
72 independently precomputed cases rather than reusing the reference repair expression.
SHA256SUMS.json hashes all corpus files except itself; changes require regenerated
hashes and a new freeze decision. No chmod immutability guarantee: controller must
supply read-only corpus and enforce participant write closure. No results exist.
