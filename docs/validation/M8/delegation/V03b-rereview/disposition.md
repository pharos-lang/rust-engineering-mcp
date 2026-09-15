# V03b — disposición del orquestador (2026-09-15)

Veredicto del revisor: **Block (limitado)**: 3 P2 pequeños; el resto de
findings V03 verificados como cerrados sin regresiones.

| ID | Sev | Disposición | Cierre |
| --- | --- | --- | --- |
| P2-1 C-2: `passed` de Codex admite refusal visto solo en eventos del modelo | P2 | Aceptado: `passed` exige `unknown_tool_wire_refused`; la señal de eventos queda informativa | W30 |
| P2-2 R-1(iii): `doctor_ok` arranca en `True` | P2 | Aceptado: `False` en la rama `else`; coherente con (b) | W30 |
| P2-3 evidencia M8-07 con plantilla antigua | P2 | Aceptado: `07-release-rehearsal.json` (y `07.md` §Resultado) se regeneran sobre bytes commiteados con la plantilla RFC 6570; `01-census.json` literal → actualizado (censo vivo) | W30 (censo) + orquestador (regen) |
| P3 respuesta ligada al id (proxy sin `id`) | P3 | Aceptado: documentar el límite en el recibo (`wire_confirmation: positional`) | W30 |
| P3 `insufficient_samples` cuenta como within en 2-de-3 | P3 | Aceptado: cuenta como `unavailable` | W30 |
| P3 `doctor` «solo lee» crea el lock | P3 | Aceptado: texto «no lee el workspace ni el source; abre el store (crea el lock del store si falta)» | W30 (tools.md) |
| P3 el manifiesto de freeze no cubre `resources[]` | P3 | Aceptado: declarado en `02.md`/`04.md`; el guardián es el test wire ↔ documento (`cli.rs`/`protocol.rs`) | orquestador |

El gate `core` en curso se abortó para incorporar W30; se relanza sobre los
bytes finales.
