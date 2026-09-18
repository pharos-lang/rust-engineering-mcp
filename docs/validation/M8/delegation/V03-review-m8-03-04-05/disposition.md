# V03 — disposición del orquestador (2026-09-15)

Veredicto del revisor: **Block** (1 P1, 12 P2, P3 varios). Todos los P1/P2
aceptados; P3 aceptados salvo donde se indica. Cierre por cuatro paquetes
disjuntos y regeneración de recibos sobre bytes commiteados.

| ID | Sev | Disposición | Paquete |
| --- | --- | --- | --- |
| D-1 `downgrade_blocked` falsos negativos (kind ausente del summary) | P1 | Aceptado: `kind` en `MutationRecordSummary` (aditivo), `downgrade_blocked = pending > 0 ∨ ∃ registro con kind posterior a la línea base 0.3.0` (`analyzer_action_apply`), recuentos por kind, nota corregida («recover, complete **or prune** with 0.8.0»), test con journal Committed `analyzer_action_apply` → `true` | W25 |
| D-2, D-3, D-4, D-5 | P3 | Aceptados: tests de v9/basura/archivo ajeno con conteos exactos; nota `Busy`; «unreadable or unknown»; `doctor` acepta `--state-root` solo para esta sección (misma lectura que `mutation list`) | W25 |
| D-6 redacción fixture permisos | P3 | Aceptado: `03.md`/ADR-088 §8 se alinean con lo probado (revocación tras `Published`, antes de completar) | orquestador + W25 (ADR) |
| R-1 sin control positivo en (a) | P2 | Aceptado: 0.8.0 `mutation list` pasa con ≥1; journal de control `manifest_patch` (kind conocido por 0.3.0) listado por 0.3.0; `doctor` 0.8.0 `downgrade_blocked: true` | W26 |
| R-2 M3 vacío en (b)/(d) | P2 | Aceptado: test nativo que confirma un artifact M3 real por API; exigir `validated ≥ 1`, `quarantined == 0` | W26 |
| R-3 recibo con árbol sucio | P2 | Aceptado: el orquestador regenera `03-rollback.json`, `05-measurement.json` y `clients.json` tras el commit (`tree_dirty: false`) | orquestador |
| R-4, R-5, R-6, R-7 | P3 | Aceptados (R-7: hueco declarado si no es alcanzable con API pública) | W26 |
| S-1 plantilla quality no RFC 6570 | P2 contrato | Aceptado: `{?offset,length}` en `stdio.rs` y `capability_document.rs` (+ docs) **antes del freeze**; constante compartida (S-2) y test wire ↔ documento; S-3 iterar `LEGACY`; S-4 CHANGELOG | W25 |
| P-1 respuestas no validadas | P2 | Aceptado: `status == passed` y `isError != true` por llamada; descartes registrados | W27 |
| P-2 regla 2-de-3 inaplicable | P2 | Aceptado: exactamente 3 recibos, mismo `budgets_sha256`/perfil, `indeterminate` con `unavailable`, CLI `--compare` | W27 |
| P-3 n insuficiente en RSS pico | P2 | Aceptado: `insufficient_samples` si `len < n`; ventana mínima de muestreo | W27 |
| P-4 recalibración no registrada | P2 | Aceptado: se registra en `05.md` («sin cambio» o valores) sobre la medición en bytes commiteados; recibo con `head_tree_dirty` y `budgets_sha256` | W27 + orquestador |
| P-5 soak sin cobertura del churn | P2 | Aceptado: servidor del soak con `--project-ttl-secs 30`, `--ttl-wait-seconds 35` por defecto y criterio «FDs tras TTL ≤ meseta + 10»; retirar la afirmación sin evidencia | W27 |
| P-6, P-7, P-8, P-9 | P3 | Aceptados (P-9: más tests unitarios, sin exclusiones) | W27 |
| C-1 pins no exigidos en `--run` | P2 | Aceptado: `--run` aborta si falla una precondición obligatoria y registra versiones observadas | W28 |
| C-2 oráculo de Codex débil | P2 | Aceptado: derivar de `protocol.jsonl` (`tools/call` open + inspect + refusal de tool desconocida) | W28 |
| C-3 negativos genéricos aceptan cualquier excepción | P2 | Aceptado: `error.code` esperado (-32602/-32601) + respuesta del servidor a ese id en `protocol.jsonl` | W28 |
| C-4 exit code 0 con `failed` | P2 | Aceptado | W28 |
| C-5, C-6, C-7 | P3 | Aceptados (C-5: código no nulo y esperado por fila; C-7: `head_commit`/`tree_dirty`, sin `type=`, sin `/private/tmp`, `mkdtemp` bajo `target/`) | W28 |

Gemini CLI: `unavailable` (denegación de tools MCP en modo headless sin regla
de permiso; no se fuerza) → **no se anuncia como calificado**; documentación
lo distingue como «configuración, no calificación» (plan M8-04).
