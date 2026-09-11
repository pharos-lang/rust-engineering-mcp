# Release evidence

One directory per published version holds the receipts that qualified it; the
two remaining directories are shared by every release.

| Path | Content |
| --- | --- |
| [`0.1.0/`](0.1.0/) | First public release (macOS ARM64 core archive, ADR-047/048): the sanitized-export manifest `PUBLICATION-SNAPSHOT.json`, the local artifact receipt, the [preparation procedure](0.1.0/preparation.md), the [offline candidate design](0.1.0/offline-candidates.md), the `candidate/` build/installation receipts and the all-feature source-qualification inventory (`inventory.json`, `THIRD_PARTY_NOTICES.candidate.txt`). |
| [`0.3.0/`](0.3.0/) | v0.3.0 publication: local candidate and smoke receipts, workflow smoke receipt, publication receipt and `SHA256SUMS`. |
| [`upstream-licenses/`](upstream-licenses/README.md) | License texts fetched from upstream repositories for packages whose crate omits them, with `receipt.json` binding each text to its commit. `scripts/release-artifact.py` reads this receipt when it assembles `THIRD_PARTY_NOTICES.txt`; every text is a redistribution obligation and is never removed. |
| [`reproduction/`](reproduction/README.md) | Scripts that rebuild, package and verify the 0.1.0 local candidates. |

Receipts are immutable: a receipt that cites a pre-hygiene path describes the
same bytes now found under the version directory
(see [`docs/validation/path-map.json`](../validation/path-map.json)). Notices and license texts must never be deleted or edited.
