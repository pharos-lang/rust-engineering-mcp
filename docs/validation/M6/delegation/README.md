# Registro de coordinación M6 — orquestación Fable 5.1

Encargo: [implement-m6-fable-orchestrator](../../../prompts/implement-m6-fable-orchestrator.md)
sobre el [encargo base M6](../../../prompts/implement-m6.md) y el
[plan M6](../../../roadmap/m6-analyzer.md). Rama `ai/m6-analyzer` desde `main`
`627729a48b2912c7e3b43d6fc5678a20f0a046a0` (PR #19, 2026-09-11). Sin push, PR,
tag ni release.

Reglas del encargo que gobiernan este registro: Fable 5.1 orquesta y decide,
no escribe código de producto; Codex/GPT prohibidos; Claude Opus 5 / Sonnet 5
vía `claude -p --model …`; Gemini 3.8 Flash High vía `agy`; pruebas largas solo
si son necesarias (§4 del encargo); cada participación acreditada tiene
invocación real, modelo, versión de CLI, alcance, resultado y evidencia.

## 1. Verificación live del punto de partida (2026-09-11)

| Comprobación | Observado |
| --- | --- |
| `git rev-parse HEAD` en `main` | `627729a48b2912c7e3b43d6fc5678a20f0a046a0`; árbol limpio salvo el prompt del encargo (untracked) |
| Workspace | `0.3.0`, edition 2024, `rust-version = "1.98.1"`, `lancedb =0.31.0` (ADR-027); 8 crates |
| Toolchain host | `cargo 1.98.1 (797e8a9bc 2026-08-05)`, `rustc 1.98.1 (48a229cea 2026-09-01)`; `rust-toolchain.toml` = 1.98.1 minimal + clippy + rustfmt |
| Recibo M5 vigente sobre los bytes actuales | [`core-gate-repo-hygiene.json`](../../M5/core-gate-repo-hygiene.json): 1 149 inputs, 23/23. Contrastado por SHA-256 con el árbol de `627729a`: **27 archivos difieren**. Los 15 de `crates/` (`git diff a3ce362 HEAD -- crates/`: 20 líneas) son exclusivamente reescrituras de rutas de evidencia en comentarios/doc-comments, un mensaje de `assert!` y dos descripciones del snapshot `binary-bloat-tool.json` (ADR/`docs/validation/M5-*` → `docs/validation/M5/*`); los 12 restantes son `.gitignore`, `.github/workflows/sonarcloud.yml`, dos README de fixtures y scripts Python corregidos para SonarCloud. Ningún cambio de lógica Rust; coherente con la regla del owner de no repetir el gate por rutas de comentarios |
| Docker | `29.7.2 linux/aarch64`; imágenes locales `…-m5` `e0a5ca1661b3`, `…-m4-scanner` `25ed3626e710`, `…-m4` `95dddeb5305f`, `…-m3` `384a1742ecc5`, `…-arm64` `8fac70723a8d` |
| `docs-hygiene.py links-check` | 2 232 enlaces, 0 rotos en documentos vivos |

### Inventario rust-analyzer (G7) — ausente donde se necesita

| Dónde | Estado |
| --- | --- |
| Toolchain host 1.98.1 | Componente `rust-analyzer-aarch64-apple-darwin` **no instalado** (`rustup component list`: disponible, no instalado). `~/.cargo/bin/rust-analyzer` es el proxy de rustup y falla: «Unknown binary 'rust-analyzer' in official toolchain '1.98.1-aarch64-apple-darwin'» |
| Otros toolchains host | `1.92` → RA 1.92.0 (`932eb852…`); `stable` → RA 1.97.1 (`8bab26f4 2026-07-14`, `46c2ee0f…`); `nightly` → RA 1.99.0-nightly (`7608eb7b0 2026-08-05`, `0b5ee8b3…`). Ninguno es la versión fijada y ninguno corre en el guest |
| Imagen guest M5 `sha256:e0a5ca16…` | `/opt/rust/bin` sin `rust-analyzer`; `/opt/rust/lib/rustlib/src` **ausente** (sin `rust-src` 1.98.1). Solo existe `rust-src` nightly 2026-09-07 bajo `/opt/rust-nightly-2026-09-07` para Miri (M4), que no es el sysroot 1.98.1 |
| Manifest fijado (copia local `multirust-channel-manifest.toml` del toolchain 1.98.1, sha256 `e44f4ea0…`; `fixtures/rust-runtime/sources.json` registra el manifest publicado `a7c8774a…`) | `rust-analyzer-preview` `aarch64-unknown-linux-gnu`: `https://static.rust-lang.org/dist/2026-09-03/rust-analyzer-1.98.1-aarch64-unknown-linux-gnu.tar.xz` sha256 `a0fd960a9ab36193ae9ba4310e5f780f6ca38fa86160fae739be4ac541b6d10c` (gz `52497642…`); `rust-src` `https://static.rust-lang.org/dist/2026-09-03/rust-src-1.98.1.tar.xz` sha256 `5c846ebcebcc7e2e0777a4cdaa12051691593f16a7e94edbae5e6241cc62d98c` (gz `411c3dcc…`) |

Conclusión: el DoR de M6 («binario/sysroot exactos») **no se cumple** con los
inputs aprovisionados. Solicitud al owner en
[m6-provisioning-request](../../../roadmap/m6-provisioning-request.md).

### Arquitectura del gateway relevante para D26

`RustGateway` ejecuta cada fase como contenedor `docker container create` +
`start --attach [--interactive]` y el `supervisor` escribe todo el stdin y lee
stdout/stderr hasta la salida, con deadline, límite de bytes y cancelación.
No existe todavía una sesión dúplex (escribir frames mientras se leen
respuestas), que es lo que exige un servidor LSP. Es la frontera difícil del
corte M6-01 (worker Opus 5).

## 2. CLIs y modelos verificados antes de la primera invocación

| CLI | Versión | Comprobación |
| --- | --- | --- |
| `claude` (Claude Code) | `2.1.268` | `claude auth status`: loggedIn, `subscriptionType: max`, `apiProvider: firstParty`. Sonda `claude -p --model opus --tools "" --restricted --no-session-persistence --output-format json "Reply with exactly: OK"` → `modelUsage` con `claude-opus-5` (canonical) y el auxiliar `claude-haiku-4-5`; 1,9 s; `permission_denials: []`. Transcript sha256 `eef33efa…` (fuera del árbol) |
| `agy` | `1.2.0` | `agy models` lista `gemini-3.8-flash-high`. Sonda `-p` con `read_url`: la acción se **deniega automáticamente** en modo headless sin regla de permiso (ver [R01/attempts](R01-ra-research/attempts.md)) |
| `codex` | — | **No se usa** (decisión del owner: cuota agotada) |

Efectos: Opus 5 = High en fronteras difíciles; Sonnet 5 = Medium (High si el
corte lo exige); Gemini 3.8 Flash High para investigación/trazabilidad.
`--effort` de `claude -p` admite `low|medium|high|xhigh|max`; no se usa
`xhigh`/`max` por disponibilidad.

## 3. Paquetes de delegación

| ID | Agente | Alcance | Estado |
| --- | --- | --- | --- |
| [R01-ra-research](R01-ra-research/prompt-header.md) | Gemini 3.8 Flash High | Hechos verificables de rust-analyzer 1.98.1 / LSP 3.17 para D25/D26 (encoding, readiness, config hostil, procesos externos, sysroot, símbolos/referencias/diagnósticos/acciones, lifecycle) | **Hecho** (22:12–22:22 UTC, 24 fuentes citadas). [Disposición](R01-ra-research/disposition.md): **P1** — hashes de tarballs fabricados en Q1 (contrastados con el manifest publicado); el resto se acepta solo como mapa de verificación para la calibración nativa |
| [I01-integration](I01-integration/disposition.md) | Claude Sonnet 5 | Commit del registro de coordinación y del dossier (sin edición de archivos) | **Hecho**: `2970c96`; segundo commit `dbf401f` (registro I01) |
| [W06d-diagnostics-oracle-and-docs](W06d-diagnostics-oracle-and-docs/report.md) | Claude Sonnet 5 (High) | Opción A: oráculo de no-build-script por símbolos, `rust.analyzer.diagnostics` etiquetada como sintaxis-only, ADR-084 §3 enmendado, deuda de diagnósticos trazada | **Hecho** |
| [W06c-references-fixture](W06c-references-fixture/report.md) | Claude Sonnet 5 (Medium) | `fixtures/analyzer-references/` (uso no-test) para que el oráculo m6-09 distinga uso de declaración; corte y test e2e repuntados | **Hecho** |
| [W06-references-diagnostics](W06-references-diagnostics/report.md) | Claude Sonnet 5 (High) | `rust.analyzer.references` (M6-02) y `rust.analyzer.diagnostics` (M6-03) extremo a extremo; 34 tools | **Entregado** (tests focalizados verdes; verificado por el orquestador) |
| [V06-review-references-diagnostics](V06-review-references-diagnostics/disposition.md) | Claude Opus 5 (High, read-only) | Revisión independiente del diff W06 | **Block** (M6-03): 1 P1 (diagnostics falla toda la llamada por una entrada sobre el límite), 5 P2, 11 P3; todos aceptados |
| [W06b-references-diagnostics-fixes](W06b-references-diagnostics-fixes/prompt-header.md) | Claude Sonnet 5 (High) | P1 (truncar/omitir por entrada en el codec, nunca `ANALYZER_CRASHED`), saneo de `code`, tope de `related`, cota de vectores de references, test real de `answer_references`, oráculo de build-script discriminante; snapshots regenerados | En curso |
| [I06-integration](I06-integration/disposition.md) | Claude Sonnet 5 | M6-01 integrado: `33d7679` (tool) y `66c6490` (registros); recalibración 9/9 sobre los bytes finales antes de commitear | **Hecho** |
| [W05e-native-evidence-and-harness](W05e-native-evidence-and-harness/report.md) | Claude Sonnet 5 (High) | `02.md` (evidencia M6-01), desgate del wrapper `analyzer_runtime` del driver, endurecimiento del diagnóstico del arnés | **Hecho** |
| [V05-review-symbols-tool](V05-review-symbols-tool/disposition.md) | Claude Sonnet 5 (High, read-only) | Revisión independiente del diff W05 (contrato, mapeo de estados, frontera de información, arquitectura, tests, docs) | **Block** → 2 P1 (uno convención de la casa → documentación; `RESULT_LIMIT` → `unavailable`), 3 P2, 3 P3; más 2 hallazgos de README del orquestador y el defecto de handshake del test nativo (H) |
| [W05b-symbols-tool-fixes](W05b-symbols-tool-fixes/prompt-header.md) | Claude Sonnet 5 (High) | Correcciones V05 + README + arnés del test nativo; snapshot y hash regenerados | **Hecho en tres sesiones** ([informes](W05b-symbols-tool-fixes/report.md)): W05b y W05c terminaron sin informe (esperaban una tarea en segundo plano que `-p` no reanuda) y el orquestador auditó el árbol; W05d corrigió la causa real del `Disconnected` (raíz `--root` no canónica con `..`, rechazada al arrancar), no el handshake |
| [I05-integration](I05-integration/disposition.md) | Claude Sonnet 5 | Commits `4309f33` (sesión/fase/admisión/calibración) y `3bf42bb` (matriz y registros); el primer intento se detuvo por una ruta omitida en la lista | **Hecho** |
| [W05-symbols-tool](W05-symbols-tool/prompt-header.md) | Claude Sonnet 5 (High) | Port de aplicación, tool `rust.analyzer.symbols`, snapshot 32, protocol/wire tests, test nativo de extremo a extremo, docs públicas | **Entregado** ([informe](W05-symbols-tool/report.md)); el worker no pudo usar Docker; el orquestador reprodujo la tool contra el servidor real: `passed`, quiescent 323 ms, `health ok`, `complete` |
| [V04-review-lsp-session-gateway](V04-review-lsp-session-gateway/disposition.md) | Claude Opus 5 (High, read-only) | Revisión independiente del diff W04 (containment, límites, admisión, captura, oráculos, Python) | **Block** → 4 P2 + 8 P3, todos aceptados; sin hallazgos de containment |
| [W04b-lsp-session-gateway-fixes](W04b-lsp-session-gateway-fixes/prompt-header.md) | Claude Opus 5 (High) | F1/F2 (config fija a 17 claves, `AnalyzerWarning`), oráculo m6-03 por argv, recibos sin documentos obsoletos, `NotUtf8File`, ADR-085 alcance de admisión, EOF determinista, Python; recalibración completa | **Hecho** ([informe](W04b-lsp-session-gateway-fixes/report.md)): **9/9 cortes**, config fija 17/17 claves presentes en el schema real, `health: ok`, `complete` alcanzable; `config_digest` `a2592cfc…`; recibo [01-calibration.json](../01-calibration.json) `7b87268d…`. Desviaciones aceptadas: `CodecError::FrameLimit` nuevo; `cargo metadata` como conjunto cerrado de flags (orden real del binario) |
| [I04-integration](I04-integration/disposition.md) | Claude Sonnet 5 | Commit `636ca32` (dominio + codec + registros W03/V03/W03b/I03/W04) | **Hecho** |
| [V03-review-domain-codec](V03-review-domain-codec/claude-sonnet-5-review.md) | Claude Sonnet 5 (High, read-only) | Revisión independiente del diff W03 (traducción de posiciones, robustez del codec, conversiones, fidelidad de capabilities, tests) | **Block** → 1 P0 (recursión sin cota tras el tope de 512), 1 P1 (falta `textDocument.codeAction` en capabilities), 3 P2, 8 P3; todos aceptados |
| [W03b-domain-codec-fixes](W03b-domain-codec-fixes/prompt-header.md) | Claude Sonnet 5 (High) | Correcciones V03 + `config_digest` sin `unwrap_or_default`; enmienda de una línea a ADR-084 §4 y brief §4.3 (capability `codeAction`) | **Hecho** ([informe](W03b-domain-codec-fixes/report.md), [disposición V03](V03-review-domain-codec/disposition.md)); 28 + 45 tests. **Desviación**: la sesión Sonnet delegó en un subagente Opus 5 (prohibido); código aceptado tras verificación; desde W04 `--disallowedTools Agent Task` |
| [W04-lsp-session-gateway](W04-lsp-session-gateway/prompt-header.md) | Claude Opus 5 (High) | Sesión LSP dúplex, `Phase::Analyzer`, admisión imagen M6 (ADR-085), rechazo de `rust-analyzer.toml` en captura, calibración nativa + `test-m6-runtime.py` | **Entregado, no integrado** ([informe](W04-lsp-session-gateway/report.md), [01.md](../01.md)): 8/9 cortes nativos; F1 clave `cargo.sysrootQueryMetadata` inexistente en el binario real; F2 `health: warning` permanente por `autoreload=false`; 1 unit test de sesión falla (carrera EOF/killed) |
| [I03-integration](I03-integration/disposition.md) | Claude Sonnet 5 | Dos commits: aprovisionamiento (`1a0d0f9`) y decisiones/registros (`7f693dc`), excluyendo los archivos en curso de W03 | **Hecho** |
| [V01-review-provisioning](V01-review-provisioning/disposition.md) | Claude Sonnet 5 (High, read-only) | Revisión independiente del diff W01 (seguridad de adquisición/extracción, recibo, taint, tests) | **Block** → 2 P2 + 6 P3, todos aceptados |
| [W01b-provisioning-fixes](W01b-provisioning-fixes/prompt-header.md) | Claude Sonnet 5 (Medium) | Correcciones V01 + hallazgos del orquestador (25/40 etapas, Sonar) y reconstrucción de la imagen | **Hecho** ([informe](W01b-provisioning-fixes/report.md)); imagen `f39a5b33…`, recibo regenerado |
| [W01-provisioning](W01-provisioning/prompt-header.md) | Claude Sonnet 5 (High) | Imagen guest M6: `fixtures/rust-runtime/m6/`, `scripts/build-m6-runtime.py`, tests, wiring gate/Sonar/ci.md, ADR-082, recibo `provisioning.json` | **Hecho** ([informe](W01-provisioning/report.md)); primer build falló por `librustc_driver` no encontrado (RUNPATH `$ORIGIN/../lib`), resuelto con symlinks a `/opt/rust/lib` |
| [W02-adrs](W02-adrs/prompt-header.md) | Claude Sonnet 5 (Medium) | ADR-083 (D25) y ADR-084 (D26) sobre el [brief](D25-D26-decision-brief.md); backlog D25/D26 → Decided | **Hecho** y aceptado tras lectura completa ([informe](W02-adrs/report.md)); tensión §2.2/§4.3 del brief corregida por el orquestador |
| [W03-domain-codec](W03-domain-codec/prompt-header.md) | Claude Sonnet 5 (High) | `domain::analyzer` (posiciones/LineIndex/edits/DTOs), `execution-adapter::lsp_codec` acotado + fake-peer hostil | **Hecho** ([informe](W03-domain-codec/report.md)): 26 + 32 tests; workspace `check`/`clippy`/`fmt` verificados por el orquestador |

## 4. Decisiones del owner pendientes (paran el corte M6-01)

1. **Aprovisionamiento** de `rust-analyzer` 1.98.1 + `rust-src` 1.98.1 en una
   imagen guest M6 derivada de la M5 —
   [dossier](../../../roadmap/m6-provisioning-request.md). **Resuelta**: el
   owner aprobó la opción A+B+C el 2026-09-11; ejecuta W01.
2. **Acceso web para la investigación Gemini** (R01). **Resuelta**: la regla
   `read_url(*)` ya existía en `~/.gemini/config/projects/default-cli-project.json`
   (permisos de proyecto de `agy`); solo aplica cuando `agy` se lanza desde el
   directorio del proyecto. Las sondas denegadas se lanzaron desde el
   scratchpad. Sonda desde el repo con `--sandbox`: `read_url` permitido.

## 5. Coste de las verificaciones ejecutadas en esta sesión

Solo comprobaciones baratas (§4 del encargo): `git`, `shasum`, `rustup
component list`, `docker info/images`, un `docker run` de inspección
read-only sin red sobre la imagen M5, `docs-hygiene.py links-check`, dos
sondas de CLI. Ningún `cargo`, ningún gate, ninguna suite nativa.
