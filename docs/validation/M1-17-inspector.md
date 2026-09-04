# M1-17 — actual Inspector 2.5.0 qualification

2026-09-04. Partial client qualification; neither M1-17 nor M1 is closed.
The installed Inspector2.5.0 ran through its actual CLI and web UI. No replacement
client or handwritten bridge request is counted as Inspector evidence.

## Persistent web UI

CUA operated the Codex in-app browser against the installed Inspector web launcher,
Node24.15.0, loopback127.0.0.1, read-only configuration, private HOME/TMPDIR/storage
and default API authentication. No Chrome process, installation, authentication
override or copied host credentials were used. The product child ran under
IP-outbound-denying sandbox-exec; the Unix Docker socket was intentionally allowed.
This does not assert network isolation of Inspector or the browser itself.

The tested local release executable SHA256 is
`7a99038be57429e1db32c91d01772e7efd104691828253f45ed3bbb0e9330417`.
Its original build binding is recorded in the M1-15 accepted-source receipt; the
M1-17 [source-equivalence receipt](../release/candidate/m1-17-source-equivalence.json)
rechecked all 238 selected inputs against candidate commit `d024c7c` with zero
mismatches. This is source-input equivalence, not a reproducible rebuild.
The [UI hashes](m1-17-inspector/ui/hashes.json) additionally bind the launcher,
package-lock, product and authored fixture files. The protocol negotiates
`2025-11-25`; Inspector's initialize clientInfo reports version `0.0.0`, whereas
the installed package and visible UI report2.5.0. No other wire version is claimed.

The [native protocol Export](m1-17-inspector/ui/protocol-export.json), downloaded
through Inspector's actual Export button, records exact thirteen-tool discovery
and thirteen successful calls with `structuredContent.status=passed` and
`isError=false`. [Summary](m1-17-inspector/ui/summary.json) retains arguments,
timestamps and durations. The authored R01 reference fixture is std-only.

| Actual UI tool | Observed coverage |
| --- | --- |
| rust.project.open | Structural registration without Cargo; returned live project_ref |
| rust.project.inspect | Same-session reference; captured declarations and approved runtime |
| rust.toolchain.inspect | Same-session reference; Rust/Cargo1.98.1 and runtime inventory |
| rust.check | Default feature/target configuration, passed |
| rust.fmt.check | Captured fixture formatting, passed |
| rust.clippy | Default lint profile, passed |
| rust.test | Default selection,30-second budget, passed |
| rust.dependencies.audit | Zero-third-party closure against approved one-record RustSec fixture |
| rust.diagnostics.explain | E0502 from approved runtime, complete explanation |
| rust.quality.gate | Fast profile completed fmt/check/strict Clippy |
| rust.catalog.status | Existing signed research projection and local E5/index state |
| rust.crate.search | Hybrid query `serde` against that projection |
| rust.crate.inspect | `serde` overview against authoritative SQLite snapshot |

These are bounded `latest_known` observations, not live registry/advisory facts,
retrieval-utility measurements, all-feature/all-target coverage or a full gate.
Legal nullable JSON Schema type arrays produced Inspector portability warnings
without preventing these calls. No schema or public contract was changed.

## Cancellation, Resources and cleanup

Actual UI execution started standard gate request18 at17:18:31.749Z. The panel
showed Pending and Cancel; clicking Cancel sent notifications/cancelled at
17:18:32.031Z,282ms later, and displayed the cancellation notification. The native
export retains the request without a result. This demonstrates in-flight client
cancellation, not independently observed active Docker execution at that instant.
[Subsequent observations](m1-17-inspector/ui/post-cancellation-docker.json) found
no product-owned containers or volumes; both queries returned exit0.

**Resource read remains unqualified through this UI.** The product intentionally
returns an empty resources/list and supplies authorized opaque log URIs in tool
results ([contract](../tools.md)). Inspector's Resources pane displayed URIs(0)
and Templates(0), with no arbitrary-URI read control. Actual UI replay of
resources/list succeeded with an empty result. Its Edit/replay dialog retains the
same method; no custom bridge was substituted. The separate SDK Resource evidence
in [M1-16 checkpoint](M1-16-checkpoint.md) is not UI evidence.

Actual Disconnect showed Disconnected and product PID99183 was observed absent.
Supervisor SIGTERM joined launcher exit0; supervisor99093, launcher99100 and
product99183 were all absent ([cleanup](m1-17-inspector/ui/cleanup.json),
[session](m1-17-inspector/ui/session.json)). No forced kill or Docker removal was
performed. The temporary CUA tab was closed. Screenshots are present in the CUA
session record; no checked-in PNG artifact is claimed.

## Historical actual CLI evidence

The earlier [CLI receipt](m1-17-inspector/cli/receipt.json) binds a **different**
binary, SHA256 `b2dfef5724ce09e9f362cbcb868ca4945ebe9c771c26d7a106304254fd979ee4`,
Inspector2.5.0 and client SDK2.0.0. Its bounded transcripts are retained under
[cli/](m1-17-inspector/cli/). It establishes strict thirteen-tool discovery,
six positive calls across five distinct tools, nine explicit unavailable/error
outcomes and resources/list. One-shot CLI sessions cannot reuse project_ref.
Those unavailable outcomes are not successful project execution, and resources/list
is not a Resource read. CLI evidence is historical, not a fresh release-binary gate.
Network-control and post-run object observations are retained alongside the calls.

## Preservation and remaining work

[Preservation receipt](m1-17-inspector/preservation-receipt.json) records original
and retained SHA256/byte counts for every copied artifact. Raw target originals
are unchanged. Retained copies replace local user/repository/model path prefixes
with placeholders; protocol correlation IDs and public fixture hashes remain.
Credential-pattern inspection found no auth token/credential values in the selected
artifacts. `progressToken` is MCP correlation metadata, not a credential.

No product code, schema or compatibility promise follows from this evidence.
The final gate and independent review are complete. Native platform qualification,
licensing evidence and release-owner approvals remain tracked in the
[matrix](M1-17-matrix.md).
