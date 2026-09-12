# M6 — matriz de implementación y calificación

Estado: **In progress** (M6-01 Done local), 2026-09-12. Rama `ai/m6-analyzer` desde `main`
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
| M6-01 | symbols request → captura → RA lifecycle → document symbols → cleanup | **Done local** (Opción A): dominio+codec (`636ca32`), gateway+calibración **9/9** (`4309f33`), tool `rust.analyzer.symbols` (W05/V05/W05b–e). Evidencia de gate = 9 cortes calibrados; producto reproducido a mano de extremo a extremo. El wrapper `analyzer_runtime` queda desgateado (flake de arnés) hasta endurecerlo | [01-calibration.json](01-calibration.json), [02.md](02.md) |
| M6-02 | references (`textDocument/references`, `is_declaration` por dos peticiones) | Entregado; calificación nativa pendiente del orquestador | W06 (`crates/{application,execution-adapter,mcp-server}`); corte nativo `m6-09-references` |
| M6-03 | diagnostics (pull) | Entregado; diagnósticos de sintaxis (experimental off, Opción A); prueba de no-build-script por símbolos; calidad de diagnósticos = deuda | W06/W06d (`crates/{application,execution-adapter,mcp-server}`); corte nativo `m6-10-diagnostics-build-script-oracle` |
| M6-04 | actions → WorkspaceEdit validado → MutationPlan M2 con diff previo | **Done local** (W07/V07/W08b): candidato M2, cut nativo m6-11 verde | [03.md](03.md), [01-calibration.json](01-calibration.json) |
| M6-05 | action.apply por el writer M2 | **Done local** (W08/V08/W08b): 3 e2e de apply verdes (writer cambia disco, ACTION_STALE); Opción A no-compile-verified | [03.md](03.md) |
| M6-06 | inventario D25, fixtures hostiles, clientes, gate, handoff | Not started | — |

## Pruebas ejecutadas y no ejecutadas (§4 del encargo)

| Verificación | Estado | Motivo |
| --- | --- | --- |
| `cargo fmt/check/clippy --workspace`, tests focalizados de dominio, codec y scripts Python | Ejecutadas por el orquestador tras cada entrega | Baratas; obligatorias antes de cada commit |
| `cargo test --workspace --all-targets` | **not run** | Se ejecuta al integrar un corte completo, no en cada iteración |
| `gate.py core` / `full` | **not run** | Solo al cerrar un corte que cambie una frontera calificada y al cierre de M6 |
| Suites nativas M2–M5 | **not run** | M6 no ha tocado todavía ninguna capability que midan; se citarán por su recibo vigente si siguen sin tocarse |
| Suite nativa M6 (`test-m6-runtime.py`) | Ejecutada por W04b: **9/9** (2026-09-12T07:16–07:18Z) | Nueva capability; cada cambio de fuente invalida el recibo (la etapa `m6-runtime` del `full` la repetirá al cierre) |

## Deuda de M6

| ID | Deuda | Detalle |
| --- | --- | --- |
| M6-03 | Calidad de diagnósticos | Habilitar los diagnósticos semánticos nativos de rust-analyzer (`diagnostics.experimental.enable=true`, Opción B de [ADR-084 §3](../../adr/ADR-084-rust-analyzer-runtime-and-lsp-lifecycle.md)) una vez que la resolución de macros de la librería estándar bajo la configuración mínima sea limpia. Hoy, con esa opción habilitada, los diagnósticos experimentales producen falsos `unresolved-macro-call` para `vec!`/`assert_eq!`/`#[test]`. Requiere decisión del owner antes de habilitarse. |

## Revisiones

| Paquete | Revisor | Veredicto | Disposición |
| --- | --- | --- | --- |
| W01 aprovisionamiento | Claude Sonnet 5 (read-only) | Block (2 P2, 6 P3) | [V01](delegation/V01-review-provisioning/disposition.md): todo aceptado y corregido en W01b |
| W03 dominio + codec | Claude Sonnet 5 (read-only) | Block (1 P0, 1 P1, 3 P2, 8 P3) | [V03](delegation/V03-review-domain-codec/disposition.md): todo aceptado y corregido en W03b |
| W04 sesión/fase/admisión/calibración | Claude Opus 5 (read-only) | Block (4 P2, 8 P3; sin hallazgos de containment) | [V04](delegation/V04-review-lsp-session-gateway/disposition.md): todo aceptado y corregido en W04b; 9/9 |
| W05 tool `rust.analyzer.symbols` | Claude Sonnet 5 (read-only) | Block (2 P1, 3 P2, 3 P3) | [V05](delegation/V05-review-symbols-tool/disposition.md): aceptado, corregido en W05b–e; producto reproducido de extremo a extremo por el orquestador |
| R01 investigación | Gemini 3.8 Flash High | — | [disposición](delegation/R01-ra-research/disposition.md): P1 hashes fabricados; evidencia auxiliar |
