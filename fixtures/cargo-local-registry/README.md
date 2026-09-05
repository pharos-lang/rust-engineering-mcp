# M2 D05 Cargo local-registry fixture

This fixture is qualification evidence, not a production registry or a runtime fallback.
It contains exact crates.io artifacts and one exact official index row per retained
version. `manifest.json` records the pinned index commit, Cargo.lock checksums, file
hashes, declared licenses and the deterministic registry-tree fingerprint.

The runtime experiment ingests `registry/` through a Python-generated USTAR into a
Docker-managed tmpfs volume and mounts it read-only. It never bind-mounts this host
directory and never inherits the host Cargo home. No `index/config.json` is included
so the experiment discriminates whether Cargo 1.98.1 local-registry source
replacement needs one.

The retained closure is `quote 1.0.47 -> proc-macro2 1.0.107 -> unicode-ident
1.0.24`; `unicode-ident` also serves as a one-package basic case.
