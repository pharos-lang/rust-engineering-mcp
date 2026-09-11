# Disposición final G1–G9 — cierre local M5

Fecha: 2026-09-10. Sucede a la [auditoría read-only](g1-g9-audit.md), que
observó `9f889af` y declaró **NOT READY** con un P2 de seguridad abierto y sin
recibos source-bound. Esta disposición se escribe después de leer los recibos
finales; no convierte una lectura en un pase.

Candidato: rama `ai/m5-performance`, lock con `lancedb 0.31.0` (opción 2a) y
workspace `0.3.0`. Código Rust idéntico a `a2464c4` salvo el `cfg` de un tipo
de test solo-macOS; `scripts/` cambió en el harness de clientes, su driver
Inspector, el verificador de smoke de release y el saneado de argumentos del
utillaje M5 (quality gate de SonarCloud). Clientes (`34bd428`) y `core`/`full`
(`45d339f`, mismas fuentes) miden esos bytes; la suite nativa independiente
(`37805f6`) mide el mismo código Rust y el `full` la repite.

| Gate | Evidencia final | Disposición |
| --- | --- | --- |
| **G1 — arquitectura y contrato** | [`M5-core-gate.json`](../../M5-core-gate.json): `fmt`, `check`, `clippy -D warnings`, `test` (1648 tests), `doctests`, `architecture` (`check-architecture.py` con la guarda estadística ligada a `M5-02-method-simulation.json`) en `passed`; inventario de 1148 fuentes idéntico al inicio y al final | **Demostrado** |
| **G2 — autoridad y threat model** | P2 `verify_applied` corregido en `89ec114` (ADR-074 §3.1) y re-revisado en [closure-applied-security](../closure-applied-security/review.md); `docker-security` (4 tests) y `rust-security` aprobados dentro del intento `full` sobre `c334498`; seis selecciones nativas con imagen admitida por digest ([`M5-native-gate.json`](../../M5-native-gate.json)); cliente agentic restringido y verificado por sesión ([harness](../closure-claude-client-harness/disposition.md)) | **Demostrado** (los pasos de seguridad del `full` pasaron antes del fallo de `semantic`) |
| **G3 — lifecycle, cuotas y auditoría** | Recibos nativos por corte con `residue` vacío antes y después ([`M5-runtime.json`](../../M5-runtime.json), y la [medición conjunta](../../m5-gate-attempts/closure-full-lock-0.31.0/m5-runtime-joint/native-receipt.json) dentro de `full`); cancelación de ingesta, descendiente drenado y artifact precreado rechazado; la matriz de clientes terminó sin contenedores ni volúmenes propios y con cancelación honrada cuando el cliente abandonó (attempt-4) | **Demostrado** |
| **G4 — fixtures y pruebas** | [`M5-clients.json`](../../M5-clients.json) (attempt-9, workspace 0.3.0; attempt-7/-8 midieron lo mismo en 0.3.0-dev y tras el bump): Inspector 2.5.0 con quince filas y catorce Resources leídas; Claude Code 2.1.267 `claude-sonnet-5` con los dos turnos dirigidos por modelo, IDs emitidos en sesión y contenido de Resource ligado por hash; 74 tests del oráculo Python; intentos 2–5 fallidos preservados con causa | **Demostrado**. `tools_without_a_client_positive = []` |
| **G5 — gates y evidencia** | Gate nativo independiente 6/6 ([`M5-native-gate.json`](../../M5-native-gate.json)); [`core`](../../M5-core-gate.json) 23/23 (1717 tests Rust, 108 Python); [`full`](../../M5-full-gate.json) 38/38 en 2 h 16 min (1752 tests Rust, 108 Python) con `semantic` y `m5-runtime` dentro del conjunto; fuentes sin cambios en los tres. El `full` sobre el lock 0.38.0 falló en `semantic` y se conserva ([intentos](../../m5-gate-attempts/README.md)) | **Demostrado** |
| **G6 — compatibilidad, migración y rollback** | Los 27 snapshots heredados byte a byte idénticos a `main` (auditoría) y los tests de invariancia de `protocol.rs` dentro de `core`; formato `benchmark-dataset.v2` y receipts versionados; rollback por digest M4 y revocación de profiling documentados en `docs/compatibility.md` | **Demostrado** |
| **G7 — operación y distribución** | [`M5-provisioning.json`](../../M5-provisioning.json), ADR-075/077; README, SECURITY, `docs/tools.md`, `client-configuration.md`, `compatibility.md` sincronizados en el cierre. Instalación empaquetada, attestations y checks live pertenecen a integración/release, no autorizadas | **Demostrado para el alcance local**; integración remota pendiente de autorización |
| **G8 — revisión independiente y bug bar** | Vendor y semántica/logs (fallback Sol) sin P0–P2; applied-security re-review; harness de clientes revisado por Gemini 3.8 y Claude Sonnet 5 con todos los P1/P2 accionables corregidos y una limitación documentada (`HOME`); deuda editorial de `a2464c4` dispuesta en la matriz (cinco P3 abiertos, ninguno altera una medición) | **Demostrado**; P3 trazados |
| **G9 — DoR/DoD común** | M5-01..05 con recibos nativos, de clientes y de gates source-bound; profiling positivo (193 muestras, 0 perdidas en clientes; positivo nativo en `M5-03-runtime.json`); matriz, handoff y tablero sincronizados | **Demostrado** |

## Estado

**DONE LOCAL.** Las nueve puertas están demostradas sobre bytes source-bound
del lock con `lancedb 0.31.0`. Límites declarados: `METHOD_QUALIFIED_FOR_DIRECTION
= false` (sin veredictos direccionales), governor del guest no del host, target
positivo macOS ARM64 con guest Linux ARM64, cinco P3 editoriales y dos P3 de
harness trazados, acceso del cliente Claude Code a `HOME` documentado. La
integración remota, su smoke y cualquier release siguen pendientes de
autorización; M6 no está iniciado.
