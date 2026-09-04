# ADR-047 — Public license and GitHub delivery

## Status

Accepted by the owner on 2026-09-04.

## Context

The project is ready for public source collaboration, while M1 binary distribution
remains blocked by native-platform and third-party notice evidence. The owner chose
IUMotion Labs as copyright holder and requested GitHub publication under the
`pharos-lang` organization with CI/CD. Public history must not expose retained local
paths. Long-lived release signing keys would add custody, rotation and revocation
work before they are necessary.

## Decision

License original project code under the user's choice of MIT or Apache-2.0, expressed
as `MIT OR Apache-2.0` in every workspace package. Copyright is held by IUMotion Labs.
Third-party material keeps its own license and notice obligations. This choice grants
permissions and disclaims warranties and liability to the extent allowed by applicable
law; it is not a guarantee against every possible claim.

The official source publisher is IUMotion Labs through
`https://github.com/pharos-lang/rust-engineering-mcp`. GitHub is the source channel;
GitHub Releases is the initial binary channel. Cargo publication remains disabled.
The first public push is a history-free snapshot with deterministic local-path
redaction. The private local history remains intact and is not reachable from the
public root commit.

CI runs locked formatting, compilation, Clippy, tests, doctests and architecture
checks on Linux x86_64, macOS ARM64 and Windows x86_64. A separate supply-chain job
runs pinned audit and dependency-policy tools. This hosted evidence does not by
itself qualify filesystem or sandbox capabilities on those platforms.

CD is manual and accepts only an existing version tag selected as the dispatch ref.
It builds the core profile for the three hosted targets, packages licenses/notices,
checks hashes, creates GitHub OIDC artifact attestations and opens a draft prerelease.
Publishing the draft remains a separate release-owner action. OIDC removes the need
for a persistent private key for GitHub build provenance.

Catalog bundles remain different: ADR-041 requires detached Ed25519 signatures that
the configured host verifies against an explicit publisher/channel trust file. The
public fixture key is never a production key. Production catalog-key generation,
offline or HSM-backed custody, rotation and revocation remain blocked until catalogs
are distributed.

## Alternatives considered

- MIT only: simple and permissive, but lacks Apache-2.0's explicit patent grant.
- Apache-2.0 only: strong patent and contribution terms, but reduces compatibility
  for consumers that prefer the conventional Rust dual-license choice.
- Export the private Git history: rejected because retained evidence contains local
  paths and the repository already requires sanitization before publication.
- Store a long-lived signing key in GitHub: unnecessary for artifact provenance and
  creates avoidable secret-custody risk.
- Publish release binaries immediately: inconsistent with the open M1 qualification
  and per-target notice gates.

## Consequences

Public users may use, modify and redistribute original code under either license and
accept the licenses' warranty/liability terms. Contributions received without a
separate agreement are expected under the same dual license. Source publication does
not mark M1 complete, qualify native security claims or authorize model/catalog
redistribution. A future binary release must still close its target-specific notice,
runtime, model and native-platform evidence.
