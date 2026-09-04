# NEVER RUN HOST

This build script is excluded from the host harness. Do not invoke Cargo, Clippy,
Cargo tests, or rustc on it on the host. A future Cargo-capable Execution Gateway
must enforce filesystem, environment and network denial before running it.

The finite probes try fixed synthetic canaries: read `/etc/host-canary`, overwrite
an existing `/rootfs-canary`, detect only the presence of
`MCP_TEST_SYNTHETIC_SECRET`, and connect to loopback port 9 with a 200 ms deadline.
No secret value is printed. No shell, arbitrary arguments, downloads or external
addresses are used. Network success/failure alone cannot prove isolation;
future gateway tests need permissive controls and kernel-level enforcement.
