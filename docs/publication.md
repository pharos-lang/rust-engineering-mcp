# Public repository and delivery channels

IUMotion Labs publishes the source at
`https://github.com/pharos-lang/rust-engineering-mcp`. GitHub is the source and issue
channel. GitHub Releases is reserved for versioned binary delivery; crates.io
publication remains disabled.

The first public commit is a sanitized snapshot rather than a push of the private
development graph. `PUBLICATION-SNAPSHOT.json` in that public commit binds it to the
local source commit and lists every UTF-8 file whose local home/user path was replaced.
No production source is omitted. Historical receipts that hash an unredacted evidence
file continue to describe the retained local original; the public snapshot manifest
records the public file's replacement hash. The local repository keeps its full
history unchanged.

CI is defined in `.github/workflows/ci.yml`. It exercises portable source behavior on
GitHub-hosted Linux, macOS and Windows, plus audit/dependency policy. These runs are
useful cross-platform evidence but do not advertise sandbox/filesystem capabilities
that still fail closed outside qualified adapters.

The manual `.github/workflows/release-candidate.yml` workflow must be dispatched from
an existing version tag. It produces only core-profile archives, creates GitHub OIDC
provenance attestations and opens a draft prerelease. A draft is not a supported
release. Local-feature/model/catalog packages remain outside this pipeline until
their native and notice gates close.

GitHub OIDC signs the build-provenance statement without a repository-held private
key. Signed catalog snapshots use a separate Ed25519 protocol defined by ADR-041;
their production key and operational custodian remain undecided because no catalog
distribution is authorized.
