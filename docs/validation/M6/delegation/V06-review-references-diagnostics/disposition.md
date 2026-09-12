# Disposición — revisión independiente V06 de `rust.analyzer.references`/`.diagnostics` (W06)

Fecha: 2026-09-12. Objeto: el diff W06 (hashes en [inputs.sha256](inputs.sha256)).
Revisor: Claude Opus 5 (`claude -p --model opus --effort high --tools ""
--restricted`, diff por stdin, 356 s), distinto del worker. [Texto íntegro](claude-opus-5-review.md).
Veredicto: **Block** acotado a M6-03; M6-02 sería *Approve con P2* por sí solo.
La lógica de la diferencia de conjuntos (dos peticiones) es correcta (sin
hallazgo), y el presupuesto de la fase query se comparte de verdad.

| Hallazgo | Disposición | Corrección (W06b) |
| --- | --- | --- |
| **P1** — un solo diagnóstico con `message` vacío, `message`>4096, `code`>128 o `related`>32 hace fallar **toda** la llamada de diagnostics como `unavailable/ANALYZER_CRASHED`; alcanzable por proyectos honestos (errores largos de trait/macro), no solo hostiles | Aceptado, **bloqueante**. La construcción hostil no debe negar el servicio ni mentir sobre la causa | En `lsp_codec::diagnostics_to_domain`: truncar `message` a 4096 (con marca), truncar/`code` a 128, acotar `related` a 32, y una entrada de `message` vacío se **omite y se cuenta** (`OmissionKind::OversizedEntry`); `AnalyzerDiagnostic::new` nunca falla por contenido del peer. `completeness` refleja las omisiones/truncaciones; nunca `ANALYZER_CRASHED` por eso |
| **P2** — `code` cruza el wire sin saneo de caracteres de control (solo `message` se sanea) | Aceptado | Sanear `code` igual que `message` (reemplazo de control salvo `\n`/`\t`) en el tool layer |
| **P2** — `related` se recoge sin tope mientras el schema declara `maxItems: 32` (viola su propio schema o cae en el P1) | Aceptado | Acotar `related` a 32 en la conversión, contar el exceso como omisión |
| **P2** — la diferencia de conjuntos de references es O(n·m) sobre vectores del peer sin tope, antes del cap visible y fuera de la supervisión del presupuesto | Aceptado | Acotar los dos vectores de `Location` a un techo explícito (p. ej. `MAX_VISIBLE_RESULTS`×2) **antes** de la diferencia; contar el exceso |
| **P2** — ningún test ejecuta `answer_references`; el unit test re-implementa el filtro y pasaría aunque se borrara la función | Aceptado | Test que ejercite `answer_references` con un peer falso de dos respuestas (orden, subconjunto, presupuesto compartido, timeout en la segunda) |
| **P2** — el oráculo de build-script **no discrimina**: `src/lib.rs` es de una línea, así que `line==1` lo cumple cualquier diagnóstico; el discriminador e2e `source=="rust-analyzer"` es constante que escribe nuestra propia tool | Aceptado, **crítico** (es la prueba de que no corrió build script) | Afirmar sobre el diagnóstico **específico**: el `code` de import/macro no resuelto (p. ej. `unresolved-macro-call`/`unresolved-extern-crate`/`unresolved-import`, confirmar el exacto en la calibración) y/o que su `message` referencia el `include!`/`OUT_DIR`; usar una fixture multi-línea si hace falta para que la línea sea significativa; añadir la aserción `completeness: complete` |
| P3 — arm `request_for` de References muerto en producción + `include_declaration` hilado hasta el gateway y **ignorado** (romperá `is_declaration` en silencio si alguien lo honra) | Aceptado | Quitar `include_declaration` del `AnalyzerQuery::References` del gateway (siempre pide ambas) y el arm muerto de `request_for`; el filtrado vive en el tool/application; corregir el test de aplicación para afirmar el comportamiento real |
| P3 — flag de truncación del `related.message` se descarta | Aceptado | Propagar `related[].message_truncated` o documentar; añadir el flag |
| P3 — `omitted` suma solo `LimitVisible`; para references las ubicaciones externas/sysroot son lo común y quedan mal etiquetadas o perdidas | Aceptado | `omitted` = suma de **todas** las omisiones (el desglose por tipo ya va en `completeness.omissions[]`) |
| P3 — el oráculo nativo de references corta rangos siempre contra `src/lib.rs` ignorando `reference.file` | Aceptado | Indexar contra el archivo correcto |
| P3 — utf-8 hard-codeado en `answer_references` | Aceptado como coherente (utf-8 es obligatorio, `CapabilityMismatch` si no); confirmar que `convert` no lee otra codificación | Confirmar; sin cambio si coincide |
| P3 — primera petición lenta descarta su respuesta (fail-closed correcto) | Aceptado, documentar | Una frase en `docs/tools.md`: references consume dos mensajes y comparte el presupuesto; puede ser menos disponible que symbols al mismo `timeout_seconds` |
| P3 — `OUTPUT_LIMIT` 2→6 MiB sin registrar el máximo medido | Aceptado | Comentario con el high-water mark medido (o techo más ajustado) |
| P3 — `SANDBOX_DENIED` con dos estados (bootstrap `blocked` vs runtime `unavailable`) | **Convención de la casa** (M6-01); documentar en `docs/tools.md` para las tres tools | Doc |
| P3 — ~1400 líneas de triplicación (enum de códigos, tabla operacional, `encode_*_bounded`) entre las tres tools | Aceptado como **deuda trazada**, no bloqueante: un cambio futuro del vocabulario de estado debe tocar tres sitios | Nota en la matriz; refactor a un helper/macro compartido queda para M6-06 o cuando se toque el vocabulario |
| P3 — `#[allow(clippy::expect_used, clippy::unwrap_used)]` cubre también los tests M6-01 | Aceptado | Acotar el `allow` a los tests nuevos o al bloque mínimo |

Verificaciones del revisor que se conservan: diferencia de conjuntos sólida
(orden irrelevante, duplicados degradan bien, declaración en A / uso en B por
clave `(uri,range)`, externos filtrados aguas abajo de forma consistente);
validación de posición pre-sesión en todos los caminos; frontera de información
(solo `message`/`code`/`related.message`/`file` llevan texto del peer;
serverStatus/stderr/ResponseError.message nunca cruzan); ningún `passed` sin
respuesta; los 32 snapshots previos byte-idénticos; refactor `analyzer_prelude`/
`analyzer_finish` preserva el comportamiento de symbols; pureza de la capa de
aplicación.

## Estado

Sin P0. Un P1 (bloqueante) y cinco P2 aceptados; corrección en W06b (Sonnet
High). Los snapshots de las dos tools cambian con el saneo de `code`, el tope de
`related` y el flag de truncación → se regeneran y se re-fija el hash en
`release-smoke.py`. La calibración nativa de M6-02/03 (cortes m6-09/m6-10 +
tests e2e) se ejecuta **una sola vez** sobre los bytes finales de W06b.
