# M6 — matriz de implementación y calificación

Estado: **M6-01..06 Done local — gate `full` verde sobre bytes finales**, 2026-09-12
(gate UTC 2026-09-13). Rama `ai/m6-analyzer` desde `main`
`627729a48b2912c7e3b43d6fc5678a20f0a046a0`. Encargo:
[implement-m6-fable-orchestrator](../../prompts/implement-m6-fable-orchestrator.md)
sobre [implement-m6](../../prompts/implement-m6.md) y el
[plan M6](../../roadmap/m6-analyzer.md). Registro de coordinación y
delegaciones: [delegation/README.md](delegation/README.md). Sin push, PR, tag
ni release.

## Contrato vigente

| Decisión | Contrato |
| --- | --- |
| [ADR-082](../../adr/ADR-082-m6-runtime-provisioning.md) | rust-analyzer 1.98.1 + rust-src 1.98.1 en la imagen `…-m6` derivada por digest de la M5; red solo en `provision.py`; recibo [provisioning.json](provisioning.json) |
| [ADR-083](../../adr/ADR-083-analyzer-contract-and-actions.md) | D25: cinco tools (`rust.analyzer.symbols`/`references`/`diagnostics`/`actions`/`action.apply`), apply por el writer M2 con `MutationKind::AnalyzerActionApply`, hover/definition/rename Deferred |
| [ADR-084](../../adr/ADR-084-rust-analyzer-runtime-and-lsp-lifecycle.md) | D26: instancia transitoria por consulta, `Phase::Analyzer` con sesión dúplex, config fija con `config_digest`, capabilities mínimas, readiness `serverStatus`, pull diagnostics, rechazo de `rust-analyzer.toml` en captura, budgets |
| [ADR-085](../../adr/ADR-085-m6-runtime-admission.md) | Admisión de la imagen M6 `sha256:f39a5b33…`; alcance global como ADR-077, M1–M5 siguen calificadas contra sus propios digests (limitación a declarar en compatibility en M6-06) |

## DoR

| Requisito | Estado | Evidencia |
| --- | --- | --- |
| Cierre M5 verificado live | Hecho | [registro §1](delegation/README.md#1-verificación-live-del-punto-de-partida-2026-09-11): 27 archivos difieren del recibo `core-gate-repo-hygiene.json`, todos por rutas de evidencia en comentarios o scripts Python |
| D25/D26 decididos | Hecho | ADR-083/084; [brief](delegation/D25-D26-decision-brief.md) |
| Binario/sysroot exactos | Hecho | [provisioning.json](provisioning.json): imagen `f39a5b33…`, `rust-analyzer 1.98.1 (48a229c 2026-09-01)`, rust-src presente |
| Capabilities/config/readiness probados con el binario real | Hecho: `utf-8` negociado, quiescent 353–385 ms, `health: ok`, 17/17 claves fijas presentes en `--print-config-schema` (F1/F2 de la primera pasada resueltos por enmienda de ADR-084 §3) | [01.md](01.md), [01-config-schema.json](01-config-schema.json) |
| Fixtures/budgets/permisos | Budgets fijados (ADR-084 §8); fixtures hostiles en M6-06 | — |

## Cortes y evidencia

| ID | Corte | Estado | Evidencia |
| --- | --- | --- | --- |
| M6-01 | symbols request → captura → RA lifecycle → document symbols → cleanup | **Done local** (Opción A, cualificado en el gate `full`): dominio+codec (`636ca32`), gateway+calibración **9/9** (`4309f33`), tool `rust.analyzer.symbols` (W05/V05/W05b–e). Evidencia de gate = 9 cortes calibrados; producto reproducido a mano de extremo a extremo. El wrapper `analyzer_runtime` queda desgateado (flake de arnés) hasta endurecerlo | [01-calibration.json](01-calibration.json), [02.md](02.md) |
| M6-02 | references (`textDocument/references`, `is_declaration` por dos peticiones) | **Done local**: corte nativo `m6-09` verde en el gate `full` | W06 (`crates/{application,execution-adapter,mcp-server}`); corte nativo `m6-09-references` |
| M6-03 | diagnostics (pull) | **Done local**: sintaxis-only (Opción A); no-build-script por símbolos; corte `m6-10` verde en el gate `full`; calidad semántica = deuda | W06/W06d (`crates/{application,execution-adapter,mcp-server}`); corte nativo `m6-10-diagnostics-build-script-oracle` |
| M6-04 | actions → WorkspaceEdit validado → MutationPlan M2 con diff previo | **Done local** (W07/V07/W08b): candidato M2, cut nativo m6-11 verde | [03.md](03.md), [01-calibration.json](01-calibration.json) |
| M6-05 | action.apply por el writer M2 | **Done local** (W08/V08/W08b): 3 e2e de apply verdes (writer cambia disco, ACTION_STALE); Opción A no-compile-verified | [03.md](03.md) |
| M6-06 | inventario D25, fixtures hostiles, clientes, gate, handoff | **Done local** | gate [`full`](M6-full-gate.json) `sha256:69a0be14…` (42 etapas, `source_inputs_unchanged: true`); [clientes](clients.json) `sha256:cb11315e…`; [G1–G9](g-disposition.md); [handoff](handoff.md) |

## Pruebas ejecutadas y no ejecutadas (§4 del encargo)

| Verificación | Estado | Motivo |
| --- | --- | --- |
| `cargo fmt/check/clippy --workspace`, tests focalizados de dominio, codec y scripts Python | Ejecutadas por el orquestador tras cada entrega | Baratas; obligatorias antes de cada commit |
| `cargo test --workspace --all-targets` | **not run** | Se ejecuta al integrar un corte completo, no en cada iteración |
| `gate.py full` (bytes finales) | **Ejecutada** 2026-09-13T03:20→04:59Z: **42 etapas verdes**, `source_inputs_unchanged: true` | Cierre M6; recibo [M6-full-gate.json](M6-full-gate.json) `sha256:69a0be14…`. Destapó y corrigió W11 (pines 31→36) y W12/W12b (allowlist argv del guest) |
| Suites nativas M2–M5 (runtime) | **Ejecutadas en el gate `full`** | M6 tocó el writer M2 compartido y `configuration_fingerprint`; re-cualificadas verdes (m2/m3/m4/m5-runtime) |
| Suite nativa M6 (`test-m6-runtime.py`, 12 cortes) | **Ejecutada en la etapa `m6-runtime` del gate `full`**: 12/12 verdes sobre bytes finales; recibo standalone [01-calibration.json](01-calibration.json) `sha256:7f935ac1…` corrobora (fuentes byte-idénticas) | Nueva capability |

## Matriz de clientes (G4/G8)

Recibo [clients.json](clients.json) `sha256:cb11315e…`, `status: passed`. **Inspector**
2.5.0 (determinista, autoritativo): matriz completa de los 5 tools incluido `apply`
preview→commit→receipt (`write_lifecycle: performed`) + negativos `ACTION_STALE` y
`FILE_NOT_IN_SNAPSHOT`. **Claude Code** 2.1.267 (dirigido por modelo,
`claude-sonnet-5`): discovery (36) + `symbols` document + `actions` positivos + el
negativo `FILE_NOT_IN_SNAPSHOT`; 3 lecturas secundarias con rechazo transitorio de
capacidad (`capacity_refused`, W09f). Docker-free: 5/5 `SANDBOX_DENIED` en ambos
clientes. Fixes de arnés W09b–W09f (fingerprint, Tasks, robustez de assists, pacing,
tolerancia de capacidad).

## Deuda de M6

| ID | Deuda | Detalle |
| --- | --- | --- |
| M6-03 | Calidad de diagnósticos | Habilitar los diagnósticos semánticos nativos de rust-analyzer (`diagnostics.experimental.enable=true`, Opción B de [ADR-084 §3](../../adr/ADR-084-rust-analyzer-runtime-and-lsp-lifecycle.md)) una vez que la resolución de macros de la librería estándar bajo la configuración mínima sea limpia. Hoy, con esa opción habilitada, los diagnósticos experimentales producen falsos `unresolved-macro-call` para `vec!`/`assert_eq!`/`#[test]`. Requiere decisión del owner antes de habilitarse. |

| M6-04 | Assist-readiness no-determinista | `rust.analyzer.actions`/`.action.apply` pueden devolver `actions: []` (`completeness: complete`) de forma intermitente para la misma petición: los assists de rust-analyzer no están garantizados por `serverStatus` quiescent en la instancia transitoria por consulta. Prueba autoritativa de escritura = Inspector + e2e nativos + `m6-11`. Futuro: reintento acotado o readiness de assists más fuerte. |
| M6-04 | Liberación asíncrona de capacidad | Una llamada de analyzer exitosa tarda 2–13 s; la capacidad acotada se libera de forma asíncrona, así que una llamada inmediata posterior recibe un `SANDBOX_DENIED` instantáneo. `SANDBOX_DENIED` mezcla el rechazo permanente (sin grant `--rust`) con el transitorio (capacidad); considerar un código retryable distinto (`CAPACITY`/`LOCK_BUSY`). El arnés W09f lo tolera y registra; Inspector espacia y no lo golpea. |
| M6-05 | No compile-verificado (Opción A) | Una code action aplicada no está compile-verificada; el llamador corre `rust.check`. |
| W05f | `analyzer_runtime.rs` desgateado | El wrapper e2e sigue desgateado del `m6-runtime` por flake de arnés (Timeout de M6-01); re-gatearlo tras endurecerlo. |
| V12 P3-2 | Fronteras de argumento del oráculo | El oráculo `m6-03` re-parte el argv unido por espacios de `docker top`; no ve fronteras reales de argumento (no corregible barato desde fuera del guest). |
| M6-06 | Audit `admitted` post-grant | Precisión de `admitted` en el audit tras denegaciones post-grant; flag por archivo generado/vendorizado en el preview. |

## Revisiones

| Paquete | Revisor | Veredicto | Disposición |
| --- | --- | --- | --- |
| W01 aprovisionamiento | Claude Sonnet 5 (read-only) | Block (2 P2, 6 P3) | [V01](delegation/V01-review-provisioning/disposition.md): todo aceptado y corregido en W01b |
| W03 dominio + codec | Claude Sonnet 5 (read-only) | Block (1 P0, 1 P1, 3 P2, 8 P3) | [V03](delegation/V03-review-domain-codec/disposition.md): todo aceptado y corregido en W03b |
| W04 sesión/fase/admisión/calibración | Claude Opus 5 (read-only) | Block (4 P2, 8 P3; sin hallazgos de containment) | [V04](delegation/V04-review-lsp-session-gateway/disposition.md): todo aceptado y corregido en W04b; 9/9 |
| W05 tool `rust.analyzer.symbols` | Claude Sonnet 5 (read-only) | Block (2 P1, 3 P2, 3 P3) | [V05](delegation/V05-review-symbols-tool/disposition.md): aceptado, corregido en W05b–e; producto reproducido de extremo a extremo por el orquestador |
| R01 investigación | Gemini 3.8 Flash High | — | [disposición](delegation/R01-ra-research/disposition.md): P1 hashes fabricados; evidencia auxiliar |
