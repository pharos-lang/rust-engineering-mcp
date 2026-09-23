# Contributing

Thank you for contributing to Rust Engineering MCP.

Open an issue before a large architectural, security, public-contract or dependency
change. Keep pull requests focused, add discriminating tests for behavior changes and
run the checks described in [`docs/development/testing.md`](docs/development/testing.md).
Do not weaken deny-by-default execution, filesystem, environment or network
boundaries to make a test pass.

If your change adds, removes or changes a feature, tool, command, install
requirement, client/host configuration, supported platform, or an operational
or security limitation visible to users, update the same canonical document
under `docs/` (and `README.md` when it affects installation or usage) in the
same change — see the documentation policy in [`AGENTS.md`](AGENTS.md)
("ADRs y documentación viva"). `docs/README.md` indexes every canonical
document.

Unless you explicitly state otherwise, any contribution intentionally submitted for
inclusion is licensed under the same `MIT OR Apache-2.0` terms as the project, without
additional terms. Do not submit secrets, private credentials, proprietary code or
data you lack permission to redistribute. Public TLS keys under
`crates/mcp-server/src/catalog_sync/test-certs/` are test fixtures only.

Report vulnerabilities through GitHub private vulnerability reporting rather than a
public issue. See `SECURITY.md`.
