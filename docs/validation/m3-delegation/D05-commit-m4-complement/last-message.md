Task: Verified, corrected, committed locally, and attempted push.

Result:
- Commit: `c66a3704e1ad290603a3c1d10413df90d15c2b03`
- Push blocked: DNS could not resolve `github.com`; `main` is ahead of `origin/main` by 1.
- No gate rerun: the new prompt is absent from `source_inputs`.

Corrections:
- Corrected snapshots claim to 23 unchanged snapshots; mutation is new, not an exception.
- Corrected section 6 to match M3-07: Linux CLI failure and CodeQL remain open; Tasks advertisement and `LiveJobAuthority` are accepted residuals.
- Added the complement pointer to `implement-m4.md`.
- `M3-07.md` had no next-milestone section, so it was unchanged.

Files and SHA-256:

- `docs/prompts/implement-m4-complement-m3.md` — `226db9aa5024a7d7c13fda91d50132e88f25a10aeae790cb02ccf6bf4118741e`
- `docs/prompts/implement-m4.md` — `8aee0af46c354baf9c7c864b0cd3a14c93cbb984b362f91602609c3f60a91480`

Checks:
- Receipts: core 14/14, full 25/25, runtime 62/62, Rust security 20/20, rollback 10/10.
- Identifiers and ADR titles/status verified.
- `git diff --check`: pass.
- Relative-link check: pass.
- Unrelated pre-existing untracked files preserved.

Open issue: retry `git push origin main` when network/DNS access is available.

