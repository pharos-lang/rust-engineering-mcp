# Disposición — revisión independiente V03 del dominio analyzer y el codec LSP (W03)

Fecha: 2026-09-12. Objeto: `crates/domain/src/analyzer.rs`,
`crates/execution-adapter/src/lsp_codec.rs` y las dos líneas de `lib.rs`
(hashes en [inputs.sha256](inputs.sha256)). Revisor: Claude Sonnet 5 (Claude
Code 2.1.268, `claude -p --model sonnet --effort high --tools "" --restricted`,
diff por stdin, 603 s), distinto del worker. [Texto íntegro](claude-sonnet-5-review.md).
Veredicto del revisor: **Block**.

| Hallazgo | Disposición | Corrección (W03b) |
| --- | --- | --- |
| **P0** — recursión sin cota en `walk_document_symbol` una vez alcanzado el tope de 512 (stack overflow con un árbol hostil) | Aceptado | `MAX_SYMBOL_DEPTH = 32` en dominio, comprobado antes de descender en todo camino; aritmética comprobada; test con 512 hermanos + cadena de 10 000 niveles |
| **P1** — capabilities sin `textDocument.codeAction` (un servidor conforme puede responder solo `Command`s, que se rechazan → cero acciones en silencio) | Aceptado; además era un hueco del brief/ADR-084 | `codeActionLiteralSupport` con los siete kinds, `isPreferredSupport`, `dataSupport=false`, `disabledSupport=false`, sin `resolveSupport`; test; ADR-084 §4 y brief §4.3 enmendados |
| P2 — rescan O(n²) de cabeceras por `feed()` | Aceptado | Estado incremental (offset de cabecera / `Content-Length` pendiente); test con contador de scans bajo `#[cfg(test)]` |
| P2 — `NotUtf8` como cajón de sastre para «archivo sin índice» | Aceptado | Nueva razón `ActionRejection::FileNotInSnapshot`; `NotUtf8` solo para UTF-8 inválido; contrato: el llamador pasa índices de **todos** los `.rs` capturados |
| P2 — `references.excludeTests`/`lru.capacity` no verificables sin el binario | Aceptado como límite: la calibración W04 valida cada clave contra `--print-config-schema` real | Doc comment que nombra ese oráculo |
| P3 — `Content-Length` con `+` | Aceptado | Solo dígitos ASCII; tests `+10`, ` 10`, `010`, `0x10` |
| P3 — mensaje con `method` y `result`/`error`; claves desconocidas | Aceptado | `MalformedMessage` en ambos casos |
| P3 — desempate de edits con mismo inicio | Aceptado | Orden `(start, end)`; mismo inicio → `OverlappingRanges`, también dos inserciones vacías |
| P3 — aislamiento por elemento en acciones | Aceptado | `code_actions_to_candidates` recibe `Vec<serde_json::Value>` y convierte elemento a elemento |
| P3 — decoder sin estado «envenenado» | Aceptado | `poisoned`: tras un error fatal todo `feed()` devuelve el mismo error |
| P3 — falta `workspace_symbols_to_domain` | Aceptado | Añadida, orden determinista, ≤512, externos contados |
| P3 — tests faltantes (`EditLimit`, `BytesLimit`, `apply_edits` desordenado, profundidad) | Aceptado | Añadidos |
| Orquestador — `config_digest` con `unwrap_or_default` | — | Propaga `AnalyzerError::Invalid`; `encode` devuelve `Result` |

## Estado tras W03b

28 tests de dominio + 45 de codec en verde; `cargo fmt`, `check-architecture`
y (por el worker) `cargo check/clippy --workspace` en verde; verificado por el
orquestador. Sin P0–P2 abiertos. Dos decisiones de W03b que W04/W05 deben
conocer: `encode` ahora devuelve `Result`; el segundo valor de
`document_symbols_to_domain` suma omisiones por tope visible y por profundidad
(W05 puede separarlas si el contrato lo necesita).

Desviación de proceso registrada en el [informe W03b](../W03b-domain-codec-fixes/report.md):
la sesión Sonnet delegó en un subagente Opus 5 (delegación recursiva
prohibida); código aceptado tras verificación, y desde W04 los workers se
lanzan con `--disallowedTools Agent Task`.
