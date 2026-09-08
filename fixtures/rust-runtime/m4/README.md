# M4 runtime provisioning fixture

This host-only fixture consumes exactly the accepted ADR-066 proposal manifest.
It checks the immutable local M3 base, downloads and verifies every enumerated
input on the trusted host, rejects unsafe archive members, compares the packaged
`cargo-deny` lock byte-for-byte with the pinned tag lock, and builds with Docker
network and pulls disabled. No project source or credential path enters the build
context.

The image and receipt are provisioning evidence only. They do not approve gateway
admission or qualify hostile project execution.
