# M4-01 native adversarial cargo-deny qualification

Status: **passed (8/8 causal oracles, 1/1 ignored native test)** on macOS
ARM64 with the provisioned Linux ARM64 image
`sha256:95dddeb5305f10b09b441e3cc4018ebb1a8a296d365c65106327d59f933c64e7`.
This evidence qualifies only the bounded M4-01 deny path. It does not admit the
image for production and does not close M4.

## Command and result

```text
cargo test -p rust-engineering-execution security_native_adversarial::m4_deny_adversarial_oracles_preserve_cleanup_and_inputs -- --ignored --exact --nocapture
```

The command exited `0`. The selected unit-test binary reported `1 passed`, `0
failed`, `0 ignored`, `0 measured`, `213 filtered out`, in `19.32s`. Its emitted
oracle hash was
`sha256:c59d3da14433a07d3beac1a5c722ff00f6832530d6fb7010b1fc8e89006346e4`.
The structured evidence, including the literal test output and retained
cargo-deny stdout/stderr, is [M4-deny-adversarial.json](deny-adversarial.json),
SHA-256 `4efa6c1d32feaa2aaf7e2309943b638a05b695cb068820d045b3a67770941c9a`.

## Discriminating results

| Case | Positive control / causal oracle | Observed |
| --- | --- | --- |
| Real vendored license text | Frozen project uses `unicode-ident 1.0.24`; dependency license evidence includes `LICENSE-APACHE`, `LICENSE-MIT`, and `LICENSE-UNICODE`; deny must exit 0 | `Some(0)`, untruncated stdout/stderr, accepted `Apache-2.0 AND MIT AND Unicode-3.0` |
| Corrupt lock checksum | Exact package checksum must remain bound to the approved snapshot | `InvalidMetadata` |
| Missing offline bytes | Package remains requested but the directory-source bytes are absent | `MissingOfflineData` |
| Project exception file | A captured `deny.exceptions.toml` must not override host policy | `InvalidPolicy` before phase volumes are created |
| Pre-cancel | Already-cancelled control must stop before work | `Inspection(Project(Cancelled))` |
| Mid-phase cancel | Control flips after 250 ms during real gateway work | `Inspection(Project(Cancelled))` |
| Short timeout | 100 ms wall budget must expire during real gateway work | `Timeout` |
| Output budget | The same passing project with a 1 KiB retained-stream limit must stop on cargo-deny's JSON/debug output | `OutputLimit` |

The output-limit control is intentionally the same input and policy as the
successful 1 MiB run. This demonstrates quota enforcement rather than a failing
project. The positive run retained empty stdout and JSONL stderr with a final
summary of zero errors for licenses, bans, and sources. The stderr SHA-256 is
`sha256:7fa00a814b10ccd3a0e6d1d8b00a16e6b5a47b63471ef10b3e943358ecdf6566`;
the complete raw stream is in the JSON evidence.

## Input and cleanup integrity

The test loads the existing real directory-source fixture, verifies its tree
fingerprint is exactly
`sha256:743947d5788c1a4385a4b59869c5b8bd0535f7fc0d875b51288f9b26b2d0eba1`,
and binds `unicode-ident 1.0.24` to package checksum
`sha256:e6e4313cd5fcd3dad5cafa179702e2b244f760991f45397d14d4ebf38247da75`.
The same tree fingerprint was observed after all cases. The owned project bundle
and vendor snapshot were compared byte-for-byte with their pre-run clones; the
project archive fingerprint after the run was
`sha256:6f02f9c321af90e456e87633bd594a28486f5f56507de8c81257b42db476a528`.

After every one of the eight cases, inventory was queried only through
`gateway.inner.control`. All eight checks found empty raw container and volume
inventories for `org.rust-mcp.execution=true`. No adversarial command bypassed the
Execution Gateway, no network/download path was used, and no user fixture was
modified.

The gateway deliberately maps failures to typed errors and does not return an
internal process capture on those error paths. Accordingly, raw cargo-deny bytes
are retained for the successful control, while every negative row records the
exact typed cause instead of inventing an exit code or relying on generic
non-zero behavior.
