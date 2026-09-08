# M4 isolated scanner native fixtures

This fixture is a frozen/offline Cargo project with the authenticated
`unicode-ident 1.0.24` directory-source dependency. Its normal Rust source puts
`unsafe` and `extern` only in a comment and string; the dependency itself has two
real unsafe blocks. `cases/syntax.source` and `cases/parse-error.source` are inert
fixture bytes that the ignored native test copies to an admitted `.rs` path for
one gateway run.

The native test constructs invalid UTF-8 and bounded hostile parser inputs as
bytes. It never invokes the helper or any Rust parser on those inputs on the
host. The hostile inputs run only through `security_gateway::execute_scan` in
the calibrated Linux guest. The test also builds many small root-level files to
exercise the manifest's global budget without exceeding `SourceBundle` limits.

An admitted `SourceFile` is at most 1 MiB. The native boundary checks exactly
1 MiB; 1 MiB plus one byte is rejected by the domain constructor before the
gateway. The helper's `too_large` status remains a defense against a violated
volume/protocol premise and is covered by the helper's pure unit test.
