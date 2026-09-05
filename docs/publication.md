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
an existing version tag. ADR-048 restricts 0.1.0 to one macOS ARM64 core archive
with a target-specific inventory, SPDX SBOM, third-party notices, manifest and
checksums. The workflow must install and exercise those same bytes before creating
GitHub OIDC provenance and a draft prerelease. A draft is not a supported release.
The archive contains no model, ORT, LanceDB, catalog, trust, fixtures, Docker image
or toolchain; the complete `local` profile remains qualified from source.

GitHub OIDC signs the build-provenance statement without a repository-held private
key. Signed catalog snapshots use a separate Ed25519 protocol defined by ADR-041.
IUMotion Labs will not publish an official catalog in 0.1.0, so this release creates
no production catalog key or custody obligation; fixture trust remains test-only.

The public source snapshot and portable CI qualification are recorded in
[`docs/validation/public-source-publication.json`](validation/public-source-publication.json).
The cited GitHub run passed on Linux x86_64, macOS ARM64 and Windows x86_64 together
with the supply-chain job. This is source-portability evidence only; native sandbox,
filesystem, model, catalog and per-target notice gates remain separate.

The historical receipt intentionally remains bound to run `33928437393`. A separate
[live observation](validation/public-ci-live-33928952807.json) records the later
green run `33928952807` on public commit `d2192037e55362e2834969db627844c2f734a50f`
and current branch protection; it does not overwrite the earlier observation or
serve as native capability evidence. No 0.1.0 tag or binary release exists yet.
Final documentation must add the actual
tag/commit, workflow jobs, archive/SBOM/notices hashes, verified attestation and
release URL only after observing them live.
