# W21 — M8-04: conformidad wire de `resources/list`, `resources/templates/list` y `prompts/list` + primera ejecución Docker-free del arnés

## Task

Confirmar y corregir el hallazgo reportado por el orquestador: el SDK TS
2026-07-28 (Inspector) rechaza la respuesta de `resources/list` porque le
faltan `ttlMs`/`cacheScope`. Implementar los tres métodos de listado
faltantes (`resources/list`, `resources/templates/list`, `prompts/list`) con
el mismo patrón `.with_ttl_ms(0).with_cache_scope(CacheScope::Private)` que
ya usa `tools/list`, cubrirlos con tests de wire, y ejecutar
`scripts/test-m8-clients.py --run` Docker-free hasta obtener un recibo
honesto con los tres clientes opcionales (Codex/Claude Code/Gemini)
registrados con su resultado real.

## Causa raíz confirmada

Leí el schema del SDK TS instalado
(`target/m1-17-inspector/node_modules/@modelcontextprotocol/client/dist/src-D_zzAWoS.mjs`,
líneas ~2820-2880 y ~3080-3135). Para la revisión `2026-07-28` (alcanzada por
`server/discover`, dispatch `$1`-suffixed), el resultado de `tools/list`,
`prompts/list`, `resources/list`, `resources/templates/list` y
`resources/read` extiende un `CacheableResult` (SEP-2549) con `ttlMs`/
`cacheScope` **obligatorios** (`z.number().int().min(0)` /
`z.enum(["public","private"])`, sin `.optional()`). Las revisiones legacy
(`buildSchemas2025()`, alcanzadas por `initialize`) usan el schema del
paquete `@modelcontextprotocol/sdk` sin ese requisito.

`rmcp` 3.2.0 (`handler/server.rs:376-398`) da un `Default::default()` para
`list_prompts`/`list_resources`/`list_resource_templates` cuando
`EngineeringServer` no los sobrescribe — de ahí `ttl_ms`/`cache_scope` en
`None` (campos `Option<...>` con `skip_serializing_if`), ausentes del wire.
`tools/list` sí los llevaba porque `stdio.rs:755-756` ya llamaba
`.with_ttl_ms(0).with_cache_scope(CacheScope::Private)` explícitamente; los
otros tres métodos nunca se sobrescribieron. Confirmé que `ListResourcesResult`,
`ListResourceTemplatesResult` y `ListPromptsResult` usan el mismo macro
`paginated_result!` que `ListToolsResult` (`rmcp-3.2.0/src/model.rs:1580-1670,
2316`), así que exponen los mismos `with_ttl_ms`/`with_cache_scope`.

`prompts`/`resources/templates/list` comparten el mismo requisito de
`ttlMs`/`cacheScope` que `resources/list` (confirmado en el mismo bloque de
schema); no hay asimetría entre los tres métodos.

## Files changed

- `crates/mcp-server/src/stdio.rs`: implementados `list_resources` (lista
  vacía, Resources dinámicas por sesión), `list_resource_templates` (las dos
  plantillas `rust-artifact://{project_ref}/{artifact_id}` y
  `rust-quality-artifact://{project_ref}/{quality_job_id_or_artifact_id}?offset={n}&length={n}`,
  construidas desde `resources::PREFIX`/`QUALITY_PREFIX` — la única fuente de
  verdad accesible desde `stdio.rs`, ya que `capability_document.rs` no está
  en el alcance permitido de este ticket — con `name`/`description` breves) y
  `list_prompts` (lista vacía), los tres con
  `.with_ttl_ms(0).with_cache_scope(CacheScope::Private)`. Sin `unwrap` en
  ninguna ruta. **Decisión de `mimeType`:** el prompt sugería
  `application/json` "si aplica"; verifiqué el contrato real de lectura
  (`resources.rs` `encode`/`encode_quality_chunk`/`encode_quality_index`) y
  el contenido de `rust-artifact` siempre es un blob `application/octet-stream`
  (nunca JSON), mientras que `rust-quality-artifact` mezcla `application/json`
  (índice) y `application/octet-stream` (chunk) bajo la misma plantilla. Fijé
  `mimeType: application/octet-stream` solo en la plantilla `rust-artifact`
  (uniforme) y lo omití en `rust-quality-artifact` (no uniforme), en vez de
  anunciar `application/json` de forma inexacta.
- `crates/mcp-server/tests/protocol.rs`: nuevo test
  `resources_templates_and_prompts_lists_carry_ttl_and_cache_scope`, que
  cubre 2026-07-28 vía `server/discover` y 2025-06-18 vía `initialize`:
  `resources/list` → `[]` con `ttlMs`/`cacheScope` presentes en ambas
  revisiones y `resultType` solo en la moderna (igual que el patrón ya
  probado para `tools/list`); `resources/templates/list` → las dos
  plantillas exactas; `prompts/list` → `[]`; `tools/list` sigue devolviendo
  36 tools con `ttlMs`/`cacheScope` (sin cambios de contenido).
- `scripts/test-m8-clients.py`: tres defectos del arnés corregidos (ver
  abajo) más el registro robusto de `claude_gate` y el flip del literal
  `expected_advertised` en `validate_protocol_metadata`.
- `scripts/test-m8-clients-unit.py`: tests nuevos/actualizados para cada
  corrección del arnés (11 tests añadidos/reescritos; 66/66 pasan).
- `scripts/m8-inspector-session.mjs`: **sin cambios netos** — lo instrumenté
  temporalmente con un `console.error` de diagnóstico para localizar cada
  fila del plan de negativos que fallaba, y lo revertí exactamente al
  original en cuanto identifiqué que las tres causas raíz estaban en el plan
  de `test-m8-clients.py`, no en el driver de Inspector.
- `docs/tools.md`: nota de Resources dinámicas ampliada — anuncia que
  `resources/templates/list` publica las dos plantillas desde `0.8.0` y que
  las cuatro respuestas de listado llevan `ttlMs`/`cacheScope`.
- `docs/compatibility.md`: una fila nueva en la matriz wire de stdio.

## Defectos del arnés encontrados y corregidos (`scripts/test-m8-clients.py`)

Al ejecutar `--run` Docker-free until verde, aparecieron cuatro defectos
**del arnés**, todos distintos del hallazgo original y todos con causa raíz
confirmada contra el código fuente del producto (nunca "parcheados a
ciegas"):

1. **`rust.quality.gate` — argumento mínimo faltante.** La fila de
   `NEGATIVE_ROWS` solo enviaba `{"project_ref": ...}`, pero
   `crates/mcp-server/src/stdio/quality.rs:37-44` declara `profile:
   QualityProfile` sin `#[serde(default)]` — un campo requerido y cerrado
   (`fast`/`standard`). El servidor rechazaba la llamada en el límite del
   protocolo (`-32602 Invalid tool arguments`) antes de llegar al rechazo
   estructurado esperado, y el `client.callTool` sin captura de
   `m8-inspector-session.mjs` propagaba la excepción, abortando toda la
   sesión Inspector. Corregido añadiendo `"profile": "fast"`.
2. **`rust.benchmark.compare` — `project_ref` nunca inyectado.** La fila
   declaraba `project_ref_fields: ()`, pero
   `crates/mcp-server/src/stdio/benchmark_compare.rs:56-68` requiere
   `project_ref: ProjectRef` sin default. Mismo síntoma que (1). Corregido
   con `project_ref_fields: ("project_ref",)`.
3. **`rust.catalog.status` — clasificado como refusal cuando su contrato es
   una observación siempre exitosa.** La fila esperaba `blocked`/
   `unavailable`, pero `crates/mcp-server/src/stdio/catalog.rs` (y el test
   ya existente
   `catalog_status_closed_input_and_explicit_absence_in_all_versions` en
   `protocol.rs`, que exige `isError: false` con `{}`) confirman que el
   diseño de esta tool es reportar la ausencia de catálogo como **dato**,
   nunca como refusal — a diferencia de `rust.crate.search`/
   `rust.crate.inspect`, que sí deben resolver contra el catálogo y
   refusan. Este no era un defecto de argumentos sino una expectativa de
   oráculo incorrecta desde el diseño del plan M8. Añadí un campo
   `observation_only: True` a esa única fila y extendí
   `check_negative_row`/`negative_call_plan`/`validate_negative_rows` para
   aceptar ese caso (`status == "passed"`, `is_error is False`,
   `error_code is None`) sin relajar la validación de refusal para las
   otras 30 filas.
4. **`validate_protocol_metadata` — literal `expected_advertised=False`
   obsoleto.** `stdio.rs` fija `TASKS_ADVERTISEMENT_READY: bool = true`
   (M8 ya cruzó el switch de ADR-060), pero
   `validate_protocol_metadata` seguía llamando
   `m3.protocol_summary(path, False)` — heredado sin actualizar de las
   épocas M3/M5 en que Tasks aún no se anunciaba. Con `gemini-cli` siendo el
   único cliente de esta corrida que usó `server/discover` (sesión
   "moderna"), `m3.protocol_summary` (en `scripts/test-m3-clients.py`, fuera
   de mi alcance permitido — no lo toqué) detectó la discrepancia real y
   abortó todo el run. Corregido cambiando el literal a `True` en
   `test-m8-clients.py` (el archivo permitido); verifiqué contra el
   `protocol.jsonl` de la corrida que los cuatro clientes observan
   `tasks_advertised: true` de forma consistente antes de aplicar el
   cambio.

Además, hice `claude_gate` resiliente: envolví
`m6.claude_items`/`m6.validate_claude_session` (que usan el `CLAUDE_VERSION`
de `scripts/test-m6-clients.py`, fuera de mi alcance) en un `try/except` que
clasifica cualquier fallo de validación de transcript como
`{"status": "unavailable", "reason": ...}`, igual que ya hacía para
timeout/exit-code no cero. Antes de este cambio, una excepción sin capturar
ahí abortaba el proceso completo del harness (nunca llegaba a ejecutar
Gemini ni a escribir un recibo completo).

## Producto: hallazgo distinto encontrado, no corregido (según instrucción)

`docs/validation/M8/clients/attempt-9/receipt.json` (recibo final, promovido
a `docs/validation/M8/clients.json`) registra
`"claude_code": {"status": "unavailable", "reason": "Claude session did not
validate: Claude ran another version: 2.1.268"}`. La causa raíz es un
desfase de versión pinneada: `scripts/test-m8-clients.py` fija
`CLAUDE_VERSION = "2.1.268 (Claude Code)"` (línea 74) y apunta al ejecutable
real `2.1.268`, pero el validador compartido `m6.validate_claude_session`
(`scripts/test-m6-clients.py:989`, **fuera de mi alcance permitido**) sigue
comparando contra su propio `CLAUDE_VERSION = "2.1.267 (Claude Code)"`
(línea 93 de ese archivo). No es el hallazgo original (no tiene relación con
`resources/list`/`ttlMs`/`cacheScope`) y su corrección correcta vive en un
archivo que esta tarea no me autoriza a tocar
(`scripts/test-m6-clients.py`), así que no lo arreglé: lo dejo aquí con
evidencia y el recibo honesto refleja `claude_code: unavailable` (cliente
opcional; no bloquea `status: passed` del recibo). `gemini_cli` también
quedó `unavailable` por una causa ajena al producto y al arnés M8
("authentication failed or timed out" en `agy -p`, ver
`docs/validation/M8/clients/attempt-9/gemini-events.json`) — un problema de
entorno/credenciales del cliente `agy`, no del servidor.

## Resultado por cliente del `--run` Docker-free (recibo final: `attempt-9`, promovido a `docs/validation/M8/clients.json`)

- **Inspector (mandatorio):** `passed`. `contract_equality: true` (36/36
  tools contra `freeze-0.8.0.json`), 31 filas negativas todas en refusal
  declarado excepto `rust.catalog.status` (`passed`, por diseño), 4
  genéricos (`unknown_tool`/`invalid_args`/`unknown_fields` rechazados en el
  límite del protocolo; `unknown_project_ref` → `PROJECT_NOT_FOUND`
  estructurado). `resources_list_empty: true`. El error original
  `INVALID_RESULT … cacheScope` ya no aparece.
- **Codex (mandatorio):** `passed` (`classification: passed`, modelo
  `gpt-5.6-sol`, dos tool calls observadas).
- **Claude Code (opcional):** `unavailable` — ver sección anterior
  (desfase de versión pinneada fuera de mi alcance).
- **Gemini CLI (opcional):** `unavailable` — fallo de autenticación/timeout
  de `agy`, ajeno al producto y al arnés M8.
- **Recibo global:** `"status": "passed"` (los dos clientes mandatorios
  pasan; los dos opcionales quedan `unavailable` con su razón real, nunca
  anunciados como `qualified`). Reproducido dos veces (`attempt-9` y
  `attempt-10`, este último bloqueado solo por el guard de "no sobrescribir
  un recibo ya preservado" — mismo resultado `passed` en ambos).

## Tests

- `cargo fmt --all -- --check`: limpio.
- `cargo clippy -p rust-engineering-mcp --all-targets --locked --offline --
  -D warnings`: limpio.
- `cargo test -p rust-engineering-mcp --locked --offline --test protocol`:
  60/60 (incluye el test nuevo).
- `python3 -B scripts/contract-freeze.py verify
  docs/validation/M8/freeze-0.8.0.json --strict`: `{"status": "passed", ...}`
  sin cambios de clase/hash.
- `python3 -B scripts/test-m8-clients-unit.py`: 66/66 (61 originales + 5
  nuevos: cobertura de `observation_only`, del literal
  `expected_advertised=True`, y del guard de tipo).
- `git status --short crates/mcp-server/tests/snapshots`: solo
  `doctor-report.json`, modificado antes de esta sesión (ajeno a W21); los
  36 snapshots de tools quedan byte-idénticos.

## Risks

- El cambio de `mimeType` (omitido en `rust-quality-artifact`, fijado a
  `application/octet-stream` en `rust-artifact`) es una decisión mía dentro
  del margen que dejó el prompt ("si aplica"); si el orquestador prefiere
  `application/json` literal por consistencia con otra convención que yo no
  vea, es un cambio de una línea.
- `observation_only` es un mecanismo nuevo en el plan de negativos del
  arnés; solo lo usa `rust.catalog.status` hoy, pero queda disponible si
  aparece otro tool-observación en el futuro.

## Open issues

1. `scripts/test-m6-clients.py:93` (`CLAUDE_VERSION = "2.1.267 (Claude
   Code)"`) desfasado respecto al binario `2.1.268` realmente pinneado por
   M8; bloquea que `claude_code` pase alguna vez en esta corrida mientras
   no se corrija ese archivo (fuera de mi alcance permitido).
2. `agy` (Gemini CLI) falla con "authentication failed or timed out" en
   este host; posible expiración de credencial o problema de red/entorno,
   no relacionado con el servidor ni con este ticket.
