# cargo-deny parser fixtures

The four `native-{clean,manifest-without-text,text-not-allowed,banned-package}`
pairs are exact outputs captured through the M4 gateway from `cargo-deny 0.19.7`
in image
`sha256:95dddeb5305f10b09b441e3cc4018ebb1a8a296d365c65106327d59f933c64e7`.
Their source artifacts were `target/m4-deny-native/*`; the native integration
test `security_native::m4_deny_native_text_licenses_and_bans_are_real_and_cleanup_is_joined`
passed all four cases. Every stdout is empty.

| Fixture | Exit | stderr SHA-256 | stdout SHA-256 |
| --- | ---: | --- | --- |
| `native-clean` | 0 | `f4ef6042306b2a7cc04b6af7285003e76ca51992fcce8318c13e7eb84e423309` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `native-manifest-without-text` | 4 | `23859d36c9b9a14e22b2e8bf21cd37f2c4616857f274ce6fea5b2b26a681fa2f` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `native-text-not-allowed` | 4 | `bdc1fbe965ef5523c542075da7edf95c991d672b6e9c8879e6b9a850d2728ef6` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `native-banned-package` | 2 | `de90e6870048d2ec75447343f19664bd9ea83fb9890b716361af28099e37a53b` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `native-workspace-wildcard` | 2 | `dc891d2898beafa9e778d40449900b58cb503a1ba389a8249b789cd4c76019eb` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `native-workspace-exact` | 0 | `1201c2a0f65bcaa57c2fedd1cdc0813e2ccea520864828de4ebff40266b6f416` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |

The workspace pair was captured by
`security_graph_native::captures_workspace_dependency_graphs_through_the_gateway`.
Its owned source contains the root, `app` and `helper` manifests, two library
sources, two MIT license files and one lockfile. The wildcard variant inherits a
renamed path dependency without `version`; the exact variant adds
`version = "=0.1.0"`. cargo-deny emits three
`unresolved-workspace-dependency` diagnostics at `bug` severity and one counted
`wildcard` error for the former. The pinned `print_diagnostics` implementation
explicitly excludes `Severity::Bug` from summary statistics. The parser accepts
only that bug-level rule with exact package identity and requires a matching
counted wildcard finding for the same package.

The cause is limited to diagnostic location lookup. `ManifestDep.dep` comes from
Cargo metadata and `ManifestDep.krate` is the dependency already resolved through
the Cargo graph in
[`src/diag/krate_spans.rs`](https://github.com/EmbarkStudios/cargo-deny/blob/759a4946dcfe93a56fb42d464e193c4c448af4e3/src/diag/krate_spans.rs#L154-L210).
The workspace declaration is then resolved and its `WorkspaceSpan` is stored
under `ws_dep.krate.id`, the dependency ID (`helper` in this fixture), at
[`src/diag/krate_spans.rs`](https://github.com/EmbarkStudios/cargo-deny/blob/759a4946dcfe93a56fb42d464e193c4c448af4e3/src/diag/krate_spans.rs#L783-L810).
The wildcard path instead asks for `workspace_span(&krate.id)`, the currently
checked parent ID (`app`), at
[`src/bans.rs`](https://github.com/EmbarkStudios/cargo-deny/blob/759a4946dcfe93a56fb42d464e193c4c448af4e3/src/bans.rs#L922-L947).
That failed lookup occurs only while adding the secondary label for the workspace
declaration. cargo-deny has already tested the effective Cargo requirement against
`VersionReq::STAR` and added the primary wildcard label, then emits the counted
`Wildcards` diagnostic and continues at
[`src/bans.rs`](https://github.com/EmbarkStudios/cargo-deny/blob/759a4946dcfe93a56fb42d464e193c4c448af4e3/src/bans.rs#L887-L968).
Other bans checks run before this block, while duplicate, build and optional
workspace-dependency checks continue after it; there is no early return caused by
this diagnostic. Consequently the missing secondary label does not omit a policy
rule or create the wildcard inference. This conclusion applies only to the exact
`unresolved-workspace-dependency` shape and pinned implementation.

`UnresolveWorkspaceDependency` constructs `Diagnostic::bug()` in
[`src/bans/diags.rs`](https://github.com/EmbarkStudios/cargo-deny/blob/759a4946dcfe93a56fb42d464e193c4c448af4e3/src/bans/diags.rs#L940-L960),
JSON renders that severity as `bug` in
[`src/diag/grapher.rs`](https://github.com/EmbarkStudios/cargo-deny/blob/759a4946dcfe93a56fb42d464e193c4c448af4e3/src/diag/grapher.rs#L211-L224),
and `print_diagnostics` deliberately adds no summary count for `Severity::Bug`
in
[`src/cargo-deny/check.rs`](https://github.com/EmbarkStudios/cargo-deny/blob/759a4946dcfe93a56fb42d464e193c4c448af4e3/src/cargo-deny/check.rs#L567-L589).

`native-license-missing.stderr.jsonl` is an earlier real provisioning probe from
`target/m4-runtime-provisioning/verification.json`. It used the same image but a
generated package named `m4-probe`, the plugin's fallback configuration, exit 4,
empty stdout and 21 JSON events; it is retained as a separate shape oracle.

`clean.stderr.jsonl` and JSON values assembled by unit tests remain synthetic
negative/structural oracles. Native source-denial, duplicate-version and combined
multi-engine goldens are still pending, so these fixtures do not qualify the
whole parser as native-calibrated.

The accepted event shapes come from the cargo-deny sources inventoried in
`docs/validation/m4-deny-research/sources.json`, pinned to commit
`759a4946dcfe93a56fb42d464e193c4c448af4e3`.
