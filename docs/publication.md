# Public repository and delivery channels

IUMotion Labs publishes the source at
`https://github.com/pharos-lang/rust-engineering-mcp`. GitHub is the source and issue
channel. GitHub Releases is reserved for versioned binary delivery; crates.io
publication remains disabled.

The first public commit is a sanitized snapshot rather than a push of the private
development graph. `PUBLICATION-SNAPSHOT.json` in that public commit binds it to the
local source commit and lists every UTF-8 file whose local home/user path was replaced;
the retained copy now lives at
[`docs/release/0.1.0/PUBLICATION-SNAPSHOT.json`](release/0.1.0/PUBLICATION-SNAPSHOT.json)
next to the rest of the 0.1.0 release evidence.
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
[`docs/validation/M1/public-source-publication.json`](validation/M1/public-source-publication.json).
The cited GitHub run passed on Linux x86_64, macOS ARM64 and Windows x86_64 together
with the supply-chain job. This is source-portability evidence only; native sandbox,
filesystem, model, catalog and per-target notice gates remain separate.

The historical receipt intentionally remains bound to run `33928437393`. A separate
[live observation](validation/M1/public-ci-live-33928952807.json) records the later
green run `33928952807` on public commit `d2192037e55362e2834969db627844c2f734a50f`
and current branch protection; it does not overwrite the earlier observation or
serve as native capability evidence.

The final protected public source is `452acdbf3a634d2cc0b9d153db09718237625b9d`.
Tag `v0.1.0` and its published [GitHub Release](https://github.com/pharos-lang/rust-engineering-mcp/releases/tag/v0.1.0)
contain the single macOS ARM64 core archive, checksums and smoke receipt. The
[public release receipt](validation/M1/17-public-release.json) records final CI run
`33948778666`, SonarCloud run `33948778651`, tag-bound workflow `33948798048`,
asset hashes, independent download/smoke and attestations verified against the
exact signer workflow and source commit.

The 0.8.0/1.0 artifact boundary is unchanged from 0.1.0: one macOS ARM64 core
archive ([ADR-048](adr/ADR-048-0.1.0-qualification-and-artifact-boundary.md),
reconfirmed for 1.0 by [ADR-087](adr/ADR-087-1.0-host-scope.md)). D14
([`docs/roadmap/adr-backlog-m2-m8.md`](roadmap/adr-backlog-m2-m8.md) §D14,
[ADR-090](adr/ADR-090-offline-verification-and-incident-response.md)) keeps
GitHub OIDC provenance and adds the two sections below.

## Offline verification

Every release publishes, next to the core archive: `SHA256SUMS`, the SPDX SBOM
(`sbom.spdx.json`), third-party notices (`THIRD_PARTY_NOTICES.txt`), and the
Sigstore attestation bundle that `actions/attest-build-provenance` attaches to
the workflow run. Two verification paths exist, neither of which claims
byte-for-byte binary reproducibility:

- **With the `gh` CLI installed:**

  ```sh
  gh attestation verify --bundle <downloaded-bundle> --owner pharos-lang \
    rust-engineering-mcp-vX.Y.Z-aarch64-apple-darwin.tar.gz
  ```

  This reconstructs the Sigstore trust chain to the exact signer workflow
  (`pharos-lang/rust-engineering-mcp/.github/workflows/release-candidate.yml`)
  without network access beyond the initial, `gh`-cached fetch of Sigstore's
  public root keys. Repeat for `SHA256SUMS` and `release-smoke-receipt.json`
  if those bytes were downloaded independently of the archive.
- **Without `gh` (minimal, integrity-only):**

  ```sh
  shasum -a 256 -c SHA256SUMS
  ```

  followed by inspecting `inventory.json` inside the extracted archive for the
  expected target and version. This path confirms the installed bytes match
  the published bytes at download time; it does not authenticate the
  publisher the way `gh attestation verify` does.

## Incident response

1. **Publication credential.** The only publication credential is the
   short-lived OIDC token issued to the `release-candidate.yml` workflow,
   scoped by its minimal `permissions:` (`id-token: write` and
   `attestations: write` only in the `build` job; `contents: write` only in
   `draft`) and gated by branch/tag protection plus CODEOWNERS review of the
   workflow itself. There is no long-lived publication secret to rotate.
2. **If a published asset is compromised:** the asset is removed from the
   GitHub Release, a security advisory is published per
   [`SECURITY.md`](../SECURITY.md), and a new tag is cut from a verified clean
   source. Prior attestations are bound by `subject-path` to the exact digest
   of the compromised archive, so no new digest can reuse an old attestation
   and no old attestation validates a new digest; there is no separate
   "revocation" step beyond ceasing distribution and publishing the advisory.
3. **Separation from catalog signing.** The Ed25519 catalog-bundle signature
   ([ADR-041](adr/ADR-041-authenticated-catalog-bundles.md)) is an unrelated
   protocol: it signs catalog bundles, not release assets, and has its own
   trust file with rotation and revocation by public-key replacement,
   independent of GitHub OIDC and Sigstore.
