# M2 — trazabilidad de cierre

Esta matriz enlaza requisitos, decisiones, implementación y oráculos. El estado
final de ejecución procede de [M2-07](M2-07.md) y los recibos enlazados; una fila
no sustituye un resultado del gate. La integración local no publica `0.2.0-dev`.

| Requisito | Decisión e implementación | Evidencia discriminante |
| --- | --- | --- |
| Spec §25/97: cinco tools, diffs y permisos | ADR-051/052/054/055/056/057; DTOs en `crates/mcp-server/src/stdio/mutation.rs` y `mutation/semantic_input.rs`; cinco grants en `host_config.rs` y wiring en `stdio.rs` | [Cliente](M2-clients.json): cinco preview/commit y receipt, con prompt v2; [runtime](M2-final-runtime.json): manifest, fmt, fix y dependencias; [13 contratos M1](M2-m1-contract-preservation.json) |
| Stale/replay/lock/grant, namespace coordinado | ADR-050/052/059; application mutation y publisher en `crates/project-adapter/src/filesystem/macos/mutation.rs` | `mutation_store.rs`, `support/native_mutation.rs`: identidad, symlink/hardlink, root, global lock, replay exacto, desconocidos; [runtime terminal](M2-final-runtime.json) y [probe negativo histórico](m2-d02-native-probe.json) |
| Crash/cancel/faults y recuperación conocida | ADR-052/054; journal v1/v2 y fases durables | `killed_process_recovers_each_durable_boundary`, `killed_process_rolls_forward_known_format_prefix`, `short_journal_writes_fail_closed_at_every_commit_phase`, `corrupt_store_is_quarantined_while_a_new_physical_workspace_and_store_continue`; [faults](M2-native-io-faults.json), [remediación](M2-native-remediation.json), suites completas en full |
| Cargo/rustfmt aislados, sin write bind host | ADR-053/056; execution `mutation_gateway.rs`, `rust_applied.rs`, `project_inspection.rs` | Runtime fmt/fix/postcheck/exporter, `fix_hostile_runtime` y cancel/EOF/timeout con hijos activos; [máscara seccomp real](M2-fix-socket-mask.json); Docker/rust-security en [full](M2-full-gate.json) |
| TOML conservado, resolución offline y lock | ADR-051/055/057; application `resolution.rs`, project `manifest_edit.rs`/`semantic_delta.rs`, execution `resolution_gateway.rs` | Editor LF/CRLF, no-op, aliases, targets, herencia, virtual workspace; runtime dependencies 4 casos y resolver real; [follow-up D05](M2-D05-hardening-followup.json) incluye versión ausente y preserve_presence |
| Full, clientes y revisiones independientes | G4/G5/G8; ninguna revisión sustituye pruebas | [Full](M2-full-gate.json), [runtime](M2-final-runtime.json), [logs](M2-final-log-inventory.json), [cliente](M2-clients.json), [revisiones/disposición](M2-07.md#revisiones-y-disposición) |
| Contratos y admisión M1 durante commit | ADR-024/031/050; worker/registry conservan semántica M1 | `mutation_concurrency_runtime::` comprueba 13 respuestas frente a baseline busy e invalidación posterior; protocol 38 y snapshots byte-idénticos a `aa61bce` |
| Receipt autorizado y compatibilidad de journal | ADR-052/054/059; publicación por identidad física/operación/UID actual | Application autoridad/revocación/reopen; native `legacy_v1_receipt_is_read_only_and_explicit_recovery_migrates_to_v2`, `terminal_legacy_v1_replay_migrates_only_after_exact_binding`, `unknown_journal_format_never_cleans_or_changes_source` |
| Guías públicas y límites de garantías | ADR-050..059; README, SECURITY, tools, security-model, client-configuration, compatibility | [Guía de operación](../client-configuration.md#planes-receipts-y-recovery), [matriz](M2-matrix.md); no updater gestionado, exclusión OS, CAS, atomicidad visible multiarchivo ni prueba power loss |

## Criterios transversales G1–G9

- **G1:** `check-architecture.py`, manifests domain/application, protocol/snapshots;
  core y full verifican las fronteras y contratos.
- **G2:** grants tipados, captura no-follow y sandbox con controles adversariales;
  actor host cooperante conforme a ADR-050, código del proyecto no confiable.
- **G3:** worker joined, límites de source/staging/planes/journal, cleanup y
  observabilidad ADR-058; pruebas de cancelación activa y retención terminal.
  Eventos son locales y no una auditoría forense; memoria histórica no es un cap.
- **G4:** runtime usa Cargo real, publicación APFS y cliente stock; Inspector
  acredita discovery/open/default-deny. Cancelación y fallos activos se acreditan
  en runtime, sin atribuirlos al flujo exitoso de Claude ni a Inspector.
- **G5:** full posterior a ADR-059 liga todos los inputs antes/después; build core
  separado y hashes unen cliente/binario/full. Histórico pre-059 se conserva.
- **G6:** decoders v1/v2/unknown, migración explícita y recuperación conservadora;
  no updater/downgrade gestionado y `0.1.0` no interpreta journals M2.
- **G7:** mismo Docker/imagen; vendor opcional del host, sin installs/downloads
  runtime. `Cargo.lock` fija dependencias; distribución, SBOM y firmas de otra
  release no se califican en esta integración local sin publicación.
- **G8:** Sonnet contratos/observabilidad, Opus seguridad/writer/ADR-059 y delta
  final acotado. Paquetes con hashes separan revisiones históricas de bytes nuevos;
  la [auditoría AGY de cierre](../reviews/M2-closure-agy-review.md) registra
  ejecución y disposición propias: Accepted sin P0/P1 nuevos.
- **G9:** M0/M1 cerrado, D02 resuelto y writer positivo preceden la ampliación;
  M2 Done exige evidencia conjunta e integración/smoke. M3+ requiere nueva
  autorización, incluso si M2 ya está integrado.

## Límites de composición

El runtime terminal no simula pérdida física de la respuesta ni espera TTL 600 s:
expiry se prueba con reloj inyectable y la rama de replay compartida. Las pruebas
APFS inyectan ENOSPC/escritura parcial y crash por fases específicas; no llenan el
disco host ni cortan energía. El helper `abrupt_checkpoint_helper` puede retornar
sin trabajo fuera de su variable de entorno y no cuenta como otro crash.
El máximo RSS histórico 976,666,624 B no mide todo el MCP ni se reobservó tras cada
cambio. Estas limitaciones se conservan en [M2-07](M2-07.md).
