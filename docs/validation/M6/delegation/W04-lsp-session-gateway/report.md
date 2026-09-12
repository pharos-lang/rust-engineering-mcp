# W04 — informe del worker

| Campo | Valor |
| --- | --- |
| Modelo | Claude Opus 5 (`claude -p --model opus --effort high --disallowedTools Agent Task`), Claude Code 2.1.268 |
| Modelos observados | claude-haiku-4-5-20251001, claude-opus-5 |
| Inicio / fin (UTC) | 2026-09-12T00:34:56Z / 2026-09-12T02:48:30Z; 2 h 13 min |
| Resultado CLI | `subtype: success`, `is_error: False`; **el mensaje final del transcript no es el informe estructurado** (la CLI cerró con «Background tasks still running after 600s; terminating» y el último mensaje del modelo fue una nota sobre un monitor obsoleto). El informe real del paquete es [`docs/validation/M6/01.md`](../../01.md), escrito por el worker, con el recibo [`01-calibration.json`](../../01-calibration.json) y el schema [`01-config-schema.json`](../../01-config-schema.json) |
| Transcript | fuera del árbol; hashes en `transcripts.sha256` |

## Resultado (según `01.md` y los recibos)

Ocho de nueve cortes nativos en verde sobre la imagen `f39a5b33…`; falla
`m6-01-identity` porque la clave fija `cargo.sysrootQueryMetadata` **no existe**
en el schema del binario real (F1) — exactamente el oráculo que ADR-084 §3
exigía. Hallazgo F2: `health: warning` permanente por `cargo.autoreload=false`.
Mediciones: `initialize` < 60 ms, quiescent 267–389 ms, sesión completa
≈1 s, corte completo 1,4–2,2 s, stderr de RA 0 bytes, 0 peticiones
servidor→cliente. La etapa `m6-runtime` del gate `full` se detiene en la
segunda selección (F1), como debe.

Verificación del orquestador sobre el árbol entregado: `cargo fmt`, `cargo
clippy --workspace -D warnings` en verde; `lsp_session` **14/15** (falla
determinista `a_request_cut_short_by_end_of_stdout_is_an_eof_failure`, la
carrera EOF/killed que el propio worker describe en el corte m6-07);
`analyzer_gateway` 14/14; `project-adapter::source` 20/20;
`check-architecture` PASS.
