# Changelog

## Unreleased — 0.1.0-dev.1

- SonarCloud ahora importa cobertura real: LCOV de los tests Rust portables y
  Cobertura XML del control de arquitectura Python. El workflow rechaza reportes
  ausentes o vacíos, declara las versiones Python compatibles, evita clasificar el
  schema SQLite como PL/SQL y documenta las pruebas especializadas que quedan fuera
  de esta métrica.

- README reorganizado como guía pública de instalación, configuración y uso. La
  nueva guía de clientes documenta Codex, Claude Code, Gemini CLI, Cursor, VS Code
  y MCP Inspector, distinguiendo configuración disponible de compatibilidad
  calificada.

- Original project code is now dual-licensed under `MIT OR Apache-2.0`, copyright
  IUMotion Labs. The public source channel is
  `pharos-lang/rust-engineering-mcp`. Pinned GitHub Actions provide portable CI and
  a manual, OIDC-attested draft core-artifact workflow; no binary release or crates.io
  publication follows from this source publication.

- M1-17 qualification remains blocked. The fresh 19-stage full gate passed on
  macOS ARM64. MCP Inspector2.5.0 successfully called all13 public tools through
  its UI. Stock Codex0.153.0 passed direct tool/Resource, canonical inventory, repair
  and missing-runtime exercises; two model turns made no product call. The Opus5
  review was disposed without hiding its blocked verdict. Native runners, remaining
  third-party notices and production catalog-key custody still block M1.

- M1-16: completed the frozen 24-run paired utility pilot and hidden oracles.
  Both arms passed all 12 first/final candidates; there was no discordant pair or
  observed success advantage. The saturated endpoint has zero discriminating power
  and is not equivalence evidence. The MCP arm used more interactions, elapsed time
  and tokens; no causal, population or product-value claim follows.

- Prerrequisito M1-01: worker compartido sin cola, cancelación y drenaje al cierre;
  admisión de mensajes SDK y envíos acotada, deadlines de frames/escrituras y cap
  de salida. Retención conservadora de cancelaciones en rmcp3.2.0 (ADR-030).
  Se conserva el único contrato operativo rust.project.open.

- M0-08: SQLite bundled/FTS5, schema v1 y migraciones atómicas, snapshots
  verificados en memoria y consultas internas con provenance/freshness.

- M0-07: frontera de contratos tipada y reusable, validación dual schema/Serde,
  mapping de estados MCP y pruebas de errores sin reflexión; schema público intacto.

- M0-06: CLI capabilities con calibración activa, controles positivos, evidencia
  de kernel y tiers vinculados a configuración; scope exclusivo de probes confiables.
- M0-05: gateway Docker/Linux para probes cerrados, entorno reconstruido,
  presupuestos de salida/wall-time, cancelación, cleanup y fingerprint efectivo.
- M0-04: `rust.project.open`, roots explícitas del host, registro opaco con TTL y
  revalidación, manifests estructurales acotados y fingerprint de identidad.
  I/O protegido macOS 26+/APFS, fail-closed en otros adapters, schemas Rust y
  respuestas estructuradas/texto equivalentes; sin ejecutar Cargo. ADR-024.

- M0-03: MCP stdio con rmcp 3.2.0; discovery 2026-07-28 y cuatro versiones legacy,
  tools/list vacío, límites de entrada, cierre ante errores de I/O y logs solo stderr.
- M0-01: workspace mínimo y CLI sin dependencias externas; upgrade posterior del
  toolchain/MSRV a Rust 1.98.1 por el owner.
- M0-02: dominio separado con referencias/fingerprints validados, resultados y
  errores tipados, diagnósticos multipartes y provenance/freshness coherentes.
- Serde 1.0.229 para contratos base; serde_json 1.0.151 también usado por rmcp. Validación al
  deserializar, rechazo de campos desconocidos y Clock inyectable.
- CLI de ayuda y versión; rechazo explícito de modos no implementados con stdout vacío.
- Lints compartidos, rustfmt/Clippy configurados y tests del binario real.
- Documentación inicial y estrategia de modelos/revisión en AGENTS.md.

No se ha publicado ninguna release binaria. El código fuente sí es público. M0 está cerrada; los cortes M1 y su evidencia
se registran abajo y en el tablero. El gateway de probes M0 no acredita Cargo;
M1 usa un runtime Rust aprobado y calibrado por separado.

### M0-09 — Semantic foundation

- E5 local verificado, ORT sin telemetry y LanceDB memory:// por generación.
- Identidad completa, rebuild atómico y fallback léxico con facts desde SQLite.
- Gate real de inferencia/red, recibo de modelo y verificación de vendor manifest-only.

M0-10 incorpora el [corpus Rust](fixtures/README.md): fixtures compilables revisados,
diagnósticos deterministas y un adversario fuente excluido del harness del host.

### M0-10a — ArtifactStore mínimo

- Streaming en memoria con cap duro, redacción entre chunks, cuotas y TTL.
- IDs aleatorios, hash de bytes almacenados, aislamiento por owner y rollback.

### M0-11 — CI local

- Gate core/full con reportes, toolchain fijo y preflight fail-closed.
- Audit/deny, integrity receipts y matriz explícita, sin workflows remotos.

### M0-12 — Foundation cerrada

- Gate completo12 etapas:185 tests Rust distintos, corpus11 Cargo+1 input de auditoría,
  Docker real y E5/LanceDB local; evidencia y hashes de código conservados.
- Revisión independiente Opus5 High resuelta; restricciones de features reforzadas.
- Tablero actualizado y prompt para iniciar M1-01 con prerrequisitos explícitos.

M1 prerequisite: explicitly approved Rust/Cargo1.98.1 Linux ARM64 provisioning
fixture and immutable local runtime receipt; no additional operative MCP tool.

M1-01 Rust gateway prerequisite: bounded USTAR/source-volume transfer, closed
commands, applied-config verification, independent Rust seccomp profile and six
actual build-script/proc-macro/resource/descendant calibration scenarios. No new
operative MCP tool; integration and external review are tracked separately.

M1-01 project.inspect: metadata declarada capturada, provenance/freshness,
identidades de source/runtime y ProjectRef revalidado al finalizar. Workers joined,
readiness durante bootstrap y cancelación inmediata al cierre del transporte;
shutdown Rust240s acotado, sin confundir handler terminado con cleanup verificado.
Contrato/CLI/protocolo validados; gate core y Rust/MCP real aprobados, ver tablero.

M1-02: rust.toolchain.inspect observa versiones/host/canal y componentes instalados
mediante tres comandos cerrados en el gateway compartido; sin rustup/red/instalación.
Inventario tipado, fingerprints por ejecución y snapshot con ProjectRef revalidado.

### M1-03 — Cargo check y Resources

- Opciones Cargo cerradas, diagnósticos JSON normalizados con sugerencias multipart
  y resultado de compilación válido aunque falle; evidencia parcial explícita.
- Logs combinados acotados en memoria, URI opaca, autorización ProjectRef vivo,
  TTL de artifact sin renovación y lectura Resources privada sin caché.
- Rollback individual de artifacts nuevos sin expulsar logs anteriores. ADR-034.

## M1-04 — Formatting check

- `rust.fmt.check`: configured workspace formatting through the approved read-only
  captured gateway; bounded relative affected files and whole small display diff.
- Shared validation publication preserves live Resources authorization, quotas and
  freshness. No source editing, new dependencies or runtime downloads.

## M1-05 — Clippy

- Closed default/strict/pedantic/project lint profiles with structured findings and
  live-authorized logs; warning vs deny behavior explicit, no fix/source writes.
- Shared Cargo result normalization preserves check semantics; Clippy lint-family
  tags include child suggestions without claiming authenticated compiler origin.

## M1-06 — Cargo test

- Closed package/filter/features/target/timeout, actual contained test execution,
  compilation-phase evidence and bounded raw harness Resources.
- Ambiguous Cargo events after build-finished force incomplete evidence; no
  inferred test counts. Actual libtest descendants cover timeout/cancel/overflow
  plus responsive MCP discovery, backpressure and joined EOF cleanup.

## M1-07 — Local RustSec audit

- Host-expected bounded snapshots through no-follow handles; authoritative SQLite
  advisory selection and RustSec0.32.0 matching with Git/HTTP features disabled.
- Same captured lock/metadata generation, source-aware bounded paths, explicit
  stale/unknown/unsupported coverage and no false clean pass. No runtime refresh.

M1-08 / ADR-039: `rust.diagnostics.explain` accepts only an ASCII `E0000`-shaped
code and obtains bounded text from the approved installed rustc through the same
calibrated, network-denied gateway and joined workers. No project_ref, project source,
resource URI or host rustc execution is needed. Unknown codes return unavailable;
no heuristic explanation substitutes for compiler evidence. Returned text includes
content SHA, immutable runtime identity and latest_known artifact provenance/freshness.
No toolchain/image/model acquisition or native-platform qualification is implied.

M1-09 / ADR-040: `rust.quality.gate` composes fast(fmt/check/strict Clippy) or
standard(+default30s tests/offline audit) over one captured source generation, with
per-stage status, selection, repair detail and runtime evidence. One240s joined
worker; ordinary failures continue, interruption/uncertain cleanup aborts. Logs are
published as a bounded authorized group with final retention/ProjectRef checks;
rollback removes only new IDs, preserving earlier live logs. Omitted nonempty
streams make the quality verdict conservative even when command execution completed.
MCP body/envelope budgets retain stage rows and explicit omissions. No downloads,
source edits, global catalog import or new platform support. M1-10..17 remain pending.

M1-01..09: current integral gate14/14, core498,20 actual Rust gateway tests and
real E5/LanceDB/SQLite network-denied execution. Opus5 quality review resolved with
focused follow-up. Local-only integration; remaining M1/release work stays pending.

## M1-10 — Catalog acquisition and persistence

- Explicit CLI import/local-mirror sync/allowlisted HTTPS sync/status/rebuild, with
  JSON report v1; no new MCP tools or runtime acquisition.
- Domain-separated Ed25519 canonical manifests, bounded Zstd/USTAR, authenticated
  SQLite/RustSec bytes and native semantic restore bound to model/catalog identity.
- Private APFS handle I/O, protected trust file/ancestors, exclusive store lease
  and independently reserved durable sequence floor with exact-container recovery.
- Full15/15 on immutable pre-observability source; final core540, all-features
  Clippy and native CLI5+1 after reviewed floor/status/key-rotation refinements.
  [Separate source/gate receipts and review disposition](docs/validation/M1-10.md).

See [format and limits](docs/catalog-bundle-format.md). Publisher, license and
release remain unapproved; the fixture signing seed is public test data only.

## M1-11 — Read-only catalog status

- Eleventh tool, `rust.catalog.status`: closed empty input, verified component
  identities, current freshness and observable pending sequence reservation.
- Explicit host catalog/trust configuration; lazy read-only session generation,
  retained SQLite/E5/Lance handles, and independent per-call RustSec observation.
- Shared joined admission; 120s cooperative deadline and 128KiB complete result.
  Runtime acquisition remains disabled; no whole-server OS network claim.
- Gate/review recorded in [M1-11](docs/validation/M1-11.md); no M1 closure.
  [ADR-042](docs/adr/ADR-042-catalog-runtime-status.md).

## M1-12 — Bounded crate search

- Gate passed: core603 tests/10 stages, protocol35, all-features/all-targets
  Clippy, and native2 ordinary +1 explicitly run ignored E5/Lance test under
  network deny. Sonnet5 Medium review: no confirmed actionable defect.
- Twelfth tool: lexical, semantic and hybrid retrieval; SQLite version selection
  applies yanked/prerelease/MSRV filters before the result limit.
- BM25 and squared-L2 channel evidence plus deterministic RRF60 fusion; explicit
  lexical fallback, 50 candidates/channel and bounded-window accounting.
- Shared retained catalog/provider and joined worker include JSON validation,
  encoding and suffix trimming under the 512KiB complete-result budget.
- No acquisition authority, platform expansion, ranking-quality claim or M1 closure.
  [ADR-043](docs/adr/ADR-043-catalog-search-modes.md);
  [M1-12 validation](docs/validation/M1-12.md).

## M1-13 — Paged crate inspection

- Thirteenth tool with closed section/version/page input and snapshot-bound
  continuation; existing twelve tool contracts are preserved.
- SQLite scalar and collection pages expose recorded facts, explicit unknown
  documentation/source, missing crate/version outcomes and snapshot mismatch.
- Joined validation/encoding retain the shared worker; complete responses have a
  512KiB budget and preserve whole entries with progressing continuation.
- Gate passed: core629 tests/10 stages, protocol37, all-features/all-targets
  Clippy, and two local-feature tests under OS network deny, without embedding
  inference. Sonnet5 Medium review: no confirmed actionable finding.
  [ADR-044](docs/adr/ADR-044-paged-crate-inspection.md);
  [validation](docs/validation/M1-13.md). No M1 or release closure.

## M1-14 — CLI y doctor

- Doctor humano/JSON format_version1, configuración compartida con serve y checks
  tipados de catálogo, modelo, índice, RustSec, roots y runtime.
- Modo pasivo sin subprocesses; modo activo explícito mediante calibración e
  inventario del gateway Rust aprobado, sin proyecto del usuario.
- Version añade JSON de build; capabilities conserva JSON por defecto y añade
  --human. No nuevas tools MCP ni adquisiciones automáticas.
- Cancelación SIGINT/SIGTERM/SIGHUP con worker unido y cleanup; reportes limitados a128KiB.
  Warning sale0, diagnóstico fallido1 y sintaxis inválida2.
- Gate activo de doctor aprobado: calibración, SIGINT observado y cleanup de
  objetos propios. Full incorpora doctor como etapa19; este resultado focalizado
  no equivale al full conjunto ni al cierre M1.

## M1-15 — Candidatos locales

Preparados candidatos release core/local macOS arm64 con hashes, linkage, archivos de avisos y smoke de instalación offline. Doctor activo verificado en ambos ejecutables; en ese corte aún no había publicación ni licencia aprobada.
