# Local semantic gate assets

The large model is deliberately outside Git. `model-receipt.json` records the
explicit development download from immutable E5 revision, MIT publisher metadata,
file sizes and SHA256. It is not a signed production distribution manifest.

The adapter accepts only owned verified bytes, never paths or URLs. Development
`tests/local.rs` is the separate filesystem loader. Its provenance describes local
verification with unknown publication age; the receipt records the earlier HTTPS
network use. Prefixes, runtime, mean pooling, normalization and dimensions are part
of index identity. Three illustrative ES/EN queries are a boundary test, not a
retrieval-quality benchmark.

Run `scripts/test-semantic.py` with `RUST_MCP_E5_DIR` pointing to the five files and
`ORT_LIB_LOCATION` to the approved static ORT 1.24.2 directory. It verifies the
native SHA256, uses `--locked --offline` and `ORT_SKIP_DOWNLOAD=1`, and enables
feature `local`. Missing assets fail the gate without fetching anything. Runtime
uses a calibrated macOS network-deny profile and a nonexistent TMPDIR. This proves
those network operations are denied; it is not the product's strict sandbox tier.

A different native library or platform needs its own reviewed receipt and gate.
Redistribution licenses, native distribution provenance, persistent index import,
full quality corpus and supported-platform performance remain M1 release gates.
