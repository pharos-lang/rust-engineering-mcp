# Reproduce local review candidates

These exact scripts were run from `target/m1-15-candidate/`. Copy them there before
execution: their ROOT/OUT are location-dependent. Start with a fresh private output
directory; preserve any existing receipts/artifacts rather than deleting or
overwriting an earlier installation. Do not run the copies from this docs directory.

Required: installed Rust/Cargo1.98.1, cached dependencies, exact existing ORT archive
and five E5 files from the recorded receipts; macOS arm64 native toolchain. No script
installs or refreshes tools. In order run `build-candidates.py`,
`verify-installation.py`, `package-candidates.py`, then `verify-active.py`. Active
verification requires the existing approved Docker image/socket and exclusive
execution slot; it calibrates real project execution and joins cleanup.

These scripts operate on trusted, locally generated inputs in an owned directory,
not arbitrary downloaded archives or concurrent hostile local writers. Installation
uses hash-checked unpacked files; packaging streams and verifies every archive
member without extracting it. Source changes invalidate the candidate identity.
Keep generated source/lock/build/asset receipts with artifacts and compare against
the desired commit before promotion. The accepted-source receipt was an additional
principal comparison with the merged Git tree.

No distribution authorization, publisher signature, license grant, uninstall of
global software or production artifact promotion is performed. Removal is limited
to the freshly created installation subtree identified by its ownership manifest;
retain audit receipts separately and never remove shared caches, Docker objects or
user data. Local ad hoc codesign integrity is not publisher signing/notarization.
