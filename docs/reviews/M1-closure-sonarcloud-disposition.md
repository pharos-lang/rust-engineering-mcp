# M1 closure SonarCloud disposition

Date: 2026-09-05. Public PR: `#8`. Initial SonarCloud workflow run:
`33946726591`; candidate commit
`b7081973a38a608e86efa6c73741f3d5a5d442ba`.

## Initial gate result

The first public analysis correctly blocked the PR. Its Quality Gate reported
`new_coverage = 0.0%` and a new security rating of D from sixteen findings in the
three newly published Python qualification programs. Linux, macOS, Windows and the
supply-chain job all passed independently; none of those results waives SonarCloud.

A second analysis, workflow run `33947170898`, proved that the remediation was
taking effect but still blocked correctly: new coverage rose to 73.6%, and fifteen
of the sixteen findings stopped counting as new vulnerabilities. The remaining
uncovered delta was isolated to `gate.py` plus three vendor-verifier lines; the one
remaining security issue was the private-directory rejection predicate.

## Coverage disposition

The workflow previously measured only the architecture checker and the coverage
report validator. It now runs the 71 qualification/gate/export tests under the same
branch-coverage database: gate reporting (8), artifact production (11), archive
smoke (9), stock Codex qualification (39) and public-export validation (4), in
addition to the existing 3 coverage-report tests.

The three qualification programs are excluded only from Sonar's coverage metric.
Their executable main paths require a real Darwin ARM64 release host, Docker/Codex,
or both and cannot be exercised honestly by the portable Linux scanner. Their tests
still execute and fail the workflow, and their authoritative end-to-end evidence is
the candidate-bound artifact, Inspector and stock Codex receipts. They remain fully
included in Sonar static and security analysis.

The final focused additions exercise a failed child exit, the default output
stream, the impossible missing-pipe branch through a controlled mock, and real Git
resolution of `HEAD` with a negative malformed-output oracle. This covers the
observed delta without weakening the 80% Quality Gate. `verify-vendor.py` remains a
mandatory supply-chain step but is excluded from the coverage percentage because
the Sonar job does not retain its published `.crate` input; the live supply-chain
job, not fabricated scanner state, is its executable oracle.

## Security disposition

Every reported line was reviewed individually. The findings cover:

- path reads or writes that are authenticated by an approved plan digest, confined
  to an owned private output root, or opened no-follow and revalidated with `fstat`;
- the bounded event loop whose deadline derives from schema-checked, hash-approved
  budgets;
- a Docker CLI invocation whose executable hash, socket, environment and closed
  argument builders are verified and which never uses a shell;
- fixed Cargo metadata arguments with a cleaned environment and no user flags;
- in-memory tar members whose names pass the closed `safe_relative` grammar; and
- exclusive receipt creation and cleanup relative to the same resolved directory
  descriptor.

These are false positives caused by the scanner not carrying the cross-function
authentication and containment invariants. Each is suppressed with a `NOSONAR`
annotation on the exact reported line and an adjacent control-specific reason. No
file, rule or security analyzer is globally excluded.

## Existing `main` issues

The same correction also disposes the eight pre-existing open vulnerabilities on
`main`. Seven Rust S2612 findings deliberately create non-private permissions in
security tests and assert that catalog/trust reads fail closed (one is a positive
oracle for a public but non-writable ancestor); each line now explains the oracle
and carries a local `NOSONAR`. The remaining Python S8705 finding was not merely
suppressed: the public exporter now validates a bounded Git commit/ref grammar,
rejects option/range/pathological spellings, places `--end-of-options` before the
revision, and has discriminating tests. Its fixed subprocess helper is annotated to
make the no-shell, validated-argument invariant visible to the analyzer.

## Closure condition

This disposition is accepted only if the synchronized public commit receives a new
green SonarCloud Quality Gate and the complete portable/supply-chain matrix remains
green. Until then PR #8 remains blocked and cannot be merged.
