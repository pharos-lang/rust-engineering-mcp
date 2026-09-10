# Disposición final G1–G9 — cierre local M5

Fecha: 2026-09-10. Sucede a la [auditoría read-only](g1-g9-audit.md), que
observó `9f889af` y declaró **NOT READY** con un P2 de seguridad abierto y sin
recibos source-bound. Esta disposición se escribe después de leer los recibos
finales; no convierte una lectura en un pase.

Candidato: rama `ai/m5-performance`. Código, scripts, fixtures, manifests y
AGENTS idénticos entre `a2464c4` (gate nativo) y `c334498` (gates conjuntos) en
`crates/`, `fixtures/`, `Cargo.*` y `AGENTS.md`; `scripts/` cambió entre ambos
solo en el harness de clientes y su driver Inspector (`c3cb12a`..`dc7ce3e`), y
los gates `core`/`full` inventarían esas fuentes finales.

| Gate | Evidencia final | Disposición |
| --- | --- | --- |
| **G1 — arquitectura y contrato** | [`M5-core-gate.json`](../../M5-core-gate.json): `fmt`, `check`, `clippy -D warnings`, `test` (1648 tests), `doctests`, `architecture` (`check-architecture.py` con la guarda estadística ligada a `M5-02-method-simulation.json`) en `passed`; inventario de 1148 fuentes idéntico al inicio y al final | **Demostrado** |
| **G2 — autoridad y threat model** | P2 `verify_applied` corregido en `89ec114` (ADR-074 §3.1) y re-revisado en [closure-applied-security](../closure-applied-security/review.md); `docker-security` (4 tests) y `rust-security` aprobados dentro del intento `full` sobre `c334498`; seis selecciones nativas con imagen admitida por digest ([`M5-native-gate.json`](../../M5-native-gate.json)); cliente agentic restringido y verificado por sesión ([harness](../closure-claude-client-harness/disposition.md)) | **Demostrado** (los pasos de seguridad del `full` pasaron antes del fallo de `semantic`) |
| **G3 — lifecycle, cuotas y auditoría** | Recibos nativos por corte con `residue` vacío antes y después ([`M5-runtime.json`](../../M5-runtime.json)); cancelación de ingesta, descendiente drenado y artifact precreado rechazado; la matriz de clientes terminó sin contenedores ni volúmenes propios y con cancelación honrada cuando el cliente abandonó (attempt-4) | **Demostrado** |
| **G4 — fixtures y pruebas** | [`M5-clients.json`](../../M5-clients.json) (attempt-6): Inspector 2.5.0 con quince filas y catorce Resources leídas; Claude Code 2.1.267 `claude-sonnet-5` con los dos turnos dirigidos por modelo, IDs emitidos en sesión y contenido de Resource ligado por hash; 74 tests del oráculo Python; intentos 2–5 fallidos preservados con causa | **Demostrado**. `tools_without_a_client_positive = []` |
| **G5 — gates y evidencia** | Gate nativo independiente 6/6 sobre `a2464c4`; [`core`](../../M5-core-gate.json) 23/23 pasos sobre `c334498` (1717 tests Rust, 108 Python, fuentes sin cambios); [`full`](../../m5-gate-attempts/closure-full-attempt-2/full-gate.json) sobre `c334498`: 31/34 pasos aprobados y **fallo en `semantic`** porque `lancedb 0.38.0` no compila con `default-features = false`; `m5-runtime` conjunto no ejecutado | **BLOQUEADO** por decisión de dependencia del owner ([intentos](../../m5-gate-attempts/README.md)) |
| **G6 — compatibilidad, migración y rollback** | Los 27 snapshots heredados byte a byte idénticos a `main` (auditoría) y los tests de invariancia de `protocol.rs` dentro de `core`; formato `benchmark-dataset.v2` y receipts versionados; rollback por digest M4 y revocación de profiling documentados en `docs/compatibility.md` | **Demostrado** |
| **G7 — operación y distribución** | [`M5-provisioning.json`](../../M5-provisioning.json), ADR-075/077; README, SECURITY, `docs/tools.md`, `client-configuration.md`, `compatibility.md` sincronizados en el cierre. Instalación empaquetada, attestations y checks live pertenecen a integración/release, no autorizadas | **Demostrado para el alcance local**; integración remota pendiente de autorización |
| **G8 — revisión independiente y bug bar** | Vendor y semántica/logs (fallback Sol) sin P0–P2; applied-security re-review; harness de clientes revisado por Gemini 3.8 y Claude Sonnet 5 con todos los P1/P2 accionables corregidos y una limitación documentada (`HOME`); deuda editorial de `a2464c4` dispuesta en la matriz (cinco P3 abiertos, ninguno altera una medición) | **Demostrado**; P3 trazados |
| **G9 — DoR/DoD común** | M5-01..04 con recibos nativos y de clientes source-bound; profiling positivo (195 muestras, 0 perdidas en clientes; positivo nativo en `M5-03-runtime.json`); matriz, handoff y tablero sincronizados | **No cerrado**: el DoD exige el gate `full` sobre bytes finales (G5) |

## Estado

**NOT DONE — bloqueado en G5.** Ocho de nueve puertas están demostradas o
trazadas sobre bytes source-bound; G5 falla porque el gate `full` no puede
completarse mientras `lancedb 0.38.0` con `default-features = false` no
compile, y G9 depende de G5. La decisión pertenece al owner (versión o features
de LanceDB, o política de vendor). Hasta entonces M5 permanece `In progress` y
no se declara Done local; la integración remota, su smoke y cualquier release
siguen fuera de alcance.
