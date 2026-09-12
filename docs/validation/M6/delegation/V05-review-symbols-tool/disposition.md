# Disposición — revisión independiente V05 de `rust.analyzer.symbols` (W05)

Fecha: 2026-09-12. Objeto: el diff W05 (hashes en [inputs.sha256](inputs.sha256)).
Revisor: Claude Sonnet 5 (Claude Code 2.1.268, `--tools "" --restricted`, diff
por stdin, 575 s), distinto del worker. [Texto íntegro](claude-sonnet-5-review.md).
Veredicto del revisor: **Block** (2 P1, 3 P2, 3 P3).

| Hallazgo | Disposición del orquestador | Corrección (W05b) |
| --- | --- | --- |
| **P1** — `SANDBOX_DENIED` sale `blocked` en el arranque (antes de completar el descubrimiento) y `unavailable` cuando no hay runtime | **No es defecto: es la convención de la casa.** `rust.check` y `rust.project.inspect` hacen exactamente la misma sobreescritura en `bootstrap` («requires completed discovery; retry with a new request ID»). El contrato publicado sí estaba incompleto | `docs/tools.md` documenta el rechazo de arranque (`blocked/SANDBOX_DENIED` con ese mensaje, igual que las demás tools guest) separado del `unavailable/SANDBOX_DENIED` por runtime ausente o no admitido; test del camino `bootstrap` |
| **P1** — el fallback final de `encode_bounded` (aún por encima de 512 KiB tras recortar todos los símbolos) sale `blocked/RESULT_LIMIT` mientras `docs/tools.md` lo sitúa en `unavailable` | Aceptado: `unavailable/RESULT_LIMIT` (el recorte declarado con `passed` + `completeness.reasons: result_limit` sigue siendo el camino normal) | Código + test |
| P2 — `limits.*_timeout_seconds` publican las constantes fijas, no el presupuesto efectivo del llamador | Aceptado: `total` = `timeout_seconds` del llamador; `initialize`/`query` = `min(constante, total)`; documentado | Código + test |
| P2 — `name`/`detail`/`container` sin `maxLength` ni control de caracteres frente a un analyzer hostil | Aceptado con cotas concretas en la **conversión** del codec (frontera con el peer) y reflejadas en el schema: `name` y `container` ≤ 256 caracteres sin caracteres de control (si no, la entrada se **omite** y se cuenta con una nueva `OmissionKind::OversizedEntry`), `detail` recortado a 1 024 caracteres con marca `detail_truncated: true`; rutas ya acotadas a 100 ASCII por ADR-031 | Codec (`lsp_codec.rs`, ampliación aditiva) + schemas + tests |
| P2 — `CleanupUncertain`/`Internal` → `ErrorData::internal_error` en vez de `unavailable` | **No es defecto**: convención de la casa (`check.rs:388`, `inspection.rs:232`): la cuarentena del gateway es un error de infraestructura MCP, no un resultado de la tool | Sin cambio; una línea en `docs/tools.md` |
| P3 — hash del bundle dentro del `with_gateway` (sección crítica) y tras un análisis exitoso | Aceptado: se calcula antes de tomar el gateway | Código |
| P3 — `expected_project_fingerprint` sin patrón en el schema de entrada | Aceptado | Schema + snapshot regenerado (el snapshot cambia; es el único contrato nuevo, no hay semver) |
| P3 — umbral `MAX_RESULT / 4` sin justificar | Aceptado: comentario que explique el margen (envelope + espejo `text`) y test que asegure `≤ 512 KiB` codificado en el peor caso | Código + test |

Hallazgos del orquestador (documentación pública):

| Hallazgo | Corrección |
| --- | --- |
| README: «M4, M5 y M6 están en desarrollo» — falso: M4 y M5 están integrados en `main` y M5 publicado como `v0.3.0` | Restaurar «M4 y M5 cerrados e integrados; M6 en desarrollo local» |
| README: «El checkout `0.3.0` descubre 32 tools» — la release 0.3.0 descubre 31; el checkout de desarrollo (aún versión 0.3.0) descubre 32 | Distinguir release publicada (31) de checkout de desarrollo (32) |

## Evidencia nativa ejecutada por el orquestador

El worker no pudo ejecutar Docker (sandbox). El orquestador corrió
`scripts/test-m6-runtime.py` sobre el árbol W05 con la imagen `f39a5b33…`:
las nueve selecciones del gateway pasaron de nuevo (recibo del gate
`target/m6-runtime-gate`, 11:29–11:42 UTC) y la décima
(`analyzer_symbols_document_and_workspace_scope_answer_on_the_real_m6_image`)
**falló** con `Error: Disconnected` a los 1,3 s. Criterio de clasificación
escrito antes de reproducir: (H) arnés — el servidor cierra por argv/entorno/
handshake del test; (P) producto — el servidor acepta el flujo y falla en la
llamada; (I) infraestructura. Reproducción a solas con el binario real, los
mismos argumentos y el handshake MCP completo (`initialize` →
`notifications/initialized` → `rust.project.open` → `rust.analyzer.symbols`
sobre `fixtures/valid-basic`): **`passed`**, `readiness.state = quiescent`
(323 ms), `health = ok`, `completeness = complete`, `utf-8`, `rust-analyzer
1.98.1 (48a229c 2026-09-01)`, `stderr_bytes 0`, `server_requests 0`, exit 0.
Primera hipótesis del orquestador (retirada): «falta `initialize`». El test
usa la convención 2026-07-28 de `tests/inspection_runtime.rs` (versión en
`_meta` de cada petición, sin `initialize`), y esa convención también pasa a
mano. La causa real, reproducida con el binario: el test pasa `--root` como
`CARGO_MANIFEST_DIR/../../fixtures/valid-basic` (no canónica, con `..`) y el
servidor la rechaza al arrancar (`ERROR MCP project authorization
initialization failed`, exit 1) → EOF de stdout → `Disconnected`. Veredicto
**(H)**: corrección en W05d (canonicalizar la ruta; exponer el stderr del
servidor cuando muere antes de la primera respuesta). El producto respondió
correctamente de extremo a extremo en las dos convenciones de handshake.

## Estado

Sin P0. Los dos P1 se resuelven: uno por documentación (convención) y otro por
código. W05b aplica el resto; el snapshot de la tool cambia (patrón de
`expected_project_fingerprint`, cotas de `name`/`detail`/`container`) y con él el
hash fijado en `release-smoke.py`.
