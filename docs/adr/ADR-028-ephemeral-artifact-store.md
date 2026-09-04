# ADR-028 — Bounded ephemeral artifacts in M0

## Context

The M0 deliverables include the minimum ArtifactStore; ADR-014 described private
filesystem storage and M1 Resource retrieval. No such implementation exists yet.
Adding a second durable filesystem boundary before it has a consumer would extend
the no-follow/platform surface unnecessarily. Project references are process-local.

## Decision

For M0, implement a process-local memory store with no filesystem access. This
refines ADR-014's private-directory mechanism, not its isolation/budget requirements.
Durable storage and MCP Resource integration remain explicit M1 work; this internal
API is not announced as another MCP tool. Every retrieval requires owner ProjectRef
and opaque random artifact ID; the M1 adapter must additionally resolve a live
ProjectRef and authorize the request. Artifacts disappear on process exit.

A single borrowed writer consumes a bounded, trusted nonblocking stream port.
Enforce input/output byte caps, global/per-project stored bytes and counts, TTL,
opaque128-bit OS-random IDs with bounded collision retries, hash/size metadata,
rollback on input failure, cleanup/owner revocation and conservative truncation.
Reserve maximum output budget before starting. Empty artifacts count toward quota.
Capture cannot impose a timeout on a blocking source; M1 feeds it from the bounded
Execution Gateway and retains gateway cancellation/termination responsibilities.

Redact explicitly host-provided literal byte patterns, at most8 of length1..128.
Keep enough pending bytes to cover chunk boundaries, mark all overlapping matches,
and mask each matched byte with '*'. Retain redaction flags across chunks. At EOF
or truncation, mask possible incomplete secret prefixes conservatively, even when
lookahead disproves a full match. The supplied stream may itself already be a
truncated producer output; minimizing partial-secret exposure takes precedence
over retaining coincidentally matching suffixes. This deliberate over-redaction
is part of the policy, not a claim to redact only complete literal matches. Never emit
pending bytes before the lookahead is resolved. Length-preserving masks simplify
hard output budgets. This is not automatic PII detection or guaranteed RAM erasure.

Use a monotonic injected clock for TTL. Invalid limits/arithmetic/clock regression,
entropy failure, malformed source counts and quota exhaustion fail closed. Clock
regression clears and permanently poisons the instance; recovery is explicit host
construction of a new store with a trustworthy monotonic clock. No automatic
reset trusts a clock that has violated the invariant. Public
errors do not expose content or distinguish a foreign owner's artifact from absence.

## Alternatives considered

- Durable private directories immediately: extra persistence/platform boundary
  without a current MCP artifact consumer; requires separate future security gate.
- Capture all then truncate/redact: unbounded and exposes secrets before filtering.
- IDs as authorization alone: insufficient owner binding.
- Defer the store to M1: contradicts the explicit M0 deliverables.

## Consequences

M0 has real bounded streaming/retrieval with no disk permissions to misrepresent.
Clients cannot recover artifacts across restarts. Native process memory protection
and correct host registration remain trusted. M1 must integrate live ProjectRef
validation, Resource retrieval and decide durable storage before claiming it.

Global quotas intentionally couple admission across owners; quota errors reveal
coarse shared-capacity state, not foreign artifact content/existence. Default byte
quota permits four maximum-size artifacts per owner, while64 is the separate count
ceiling for smaller ones. M1 must map internal errors to public operational codes,
expose remaining TTL rather than raw process-relative clock values, and enforce
fair scheduling around the bounded synchronous producer.

## Status

Accepted and implemented in M0-10a; security tests and principal disposition of
Opus5 review recorded in validation/M0-10a.md.
