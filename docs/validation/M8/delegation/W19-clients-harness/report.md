# W19 — M8-04: arnés de matriz wire/cliente 0.8.0

## Task

Construir `scripts/test-m8-clients.py` (más `scripts/test-m8-clients-unit.py` y
`scripts/m8-inspector-session.mjs`) para calificar la matriz de clientes 0.8.0:
Inspector 2.5.0 y Codex CLI 0.154.0 obligatorios; Claude Code 2.1.268 y Gemini
CLI (`agy`) calificados antes de anunciarlos. stdio únicamente. Cablear una
etapa `core` del gate (`m8-client-harness-tests`) y las exclusiones Sonar.

## Result

Arnés entregado y verificado en modo Docker-free/client-free (`--preflight`
real ejecutado; ver más abajo). `--run` **no** se ejecutó, por instrucción
explícita del encargo — lo ejecuta el orquestador.

**Alcance real vs. lo pedido — léase antes de confiar en el recibo `--run`:**
el manifiesto de negativos Docker-free para las 31 `stable` es mecánico y
correcto (deriva sus argumentos mínimos y su vocabulario de error de las
fuentes Rust reales — `capability_document.rs::TOOL_CLASSES`, cada `enum
*Code`/`Reason`, y los arneses M2/M4/M5 ya validados — no de suposiciones), y
la validación de cada fila es deliberadamente permisiva en el código exacto
(exige `status ∈ {blocked, unavailable}` y, si hay `error_code`, que esté en
el vocabulario declarado de esa tool) precisamente porque no pude ejecutar el
binario real para fijar el código exacto que dispara primero en cada uno de
los 31 casos. Esto es una elección de diseño defendible, no una laguna
oculta, pero **no ha sido probado contra el servidor real todavía** — eso
ocurre recién en el primer `--run` del orquestador. Ver «Open issues» para el
resto de las simplificaciones deliberadas (composición en vez de
reimplementación de M2–M6, Claude Code point-driven en vez de un flujo
model-directed completo, EOF/cancel implementados pero no ejecutados aquí).

## Files changed

- `scripts/test-m8-clients.py` (nuevo, 1252 líneas): `--preflight` (versiones
  exactas de Inspector/Codex/Claude/agy, `version --json` del binario 0.8.0,
  imagen del runtime M2/M3 por digest si `--with-runtime`); `--run
  [--with-runtime]`. Contenido:
  - Inventario de 36 tools (31 `stable` + 5 `preview`), verificado contra el
    oráculo de protocolo del servidor (`crates/mcp-server/tests/protocol.rs`).
  - `TOOL_SOURCES`/`declared_error_codes`: vocabulario de error cerrado por
    tool, leído directamente de cada `enum *Code`/`Reason` en
    `crates/mcp-server/src/stdio/*.rs` (unión de `BlockedCode`+`UnavailableCode`
    para `rust.project.open`; `snake_case` para la familia de mutación
    compartida en `mutation.rs`, `SCREAMING_SNAKE_CASE` para el resto).
  - `NEGATIVE_ROWS`: una fila Docker-free por cada una de las 31 `stable`,
    con argumentos mínimos reales (no inventados): derivados de
    `docs/tools.md`, de los esquemas Rust (`Input`/`OpenInput` de cada tool)
    y de los propios arneses M2 (grants por defecto)/M4 (`execution_mode`
    síncrono)/M5 (`CALL_PLAN` de benchmark/profile/bloat). El host Docker-free
    configura un runtime completo pero inalcanzable (imagen real, binario
    docker real, socket que nunca se crea — igual que M6), sin catálogo ni
    grants de escritura, así que cada tool se refuta por su propia
    precondición (dial de runtime, catálogo ausente o grant ausente), nunca
    por un rechazo de esquema.
  - `GENERIC_NEGATIVE_ROWS`: tool desconocida, argumento inválido de esquema,
    `project_ref` bien formado pero nunca abierto, y campo fuera de un
    objeto cerrado.
  - `contract_discrepancies`: recalcula los tres hashes canónicos
    (`sha256(json.dumps(obj, sort_keys=True, separators=(",", ":"),
    ensure_ascii=False))`) más `annotations`/`stability` desde el
    `tools/list` en vivo de Inspector y los compara contra
    `docs/validation/M8/freeze-0.8.0.json`; separa discrepancias `stable`
    (fatales) de `preview` (informativas), igual semántica que
    `scripts/contract-freeze.py verify`.
  - `codex_gate` (obligatorio): `codex exec --json` con
    `-c mcp_servers.<name>.command=…`/`args=[…]` (registro efímero, nunca
    toca `~/.codex/config.toml`), `CODEX_HOME` privado con una copia de
    `auth.json`. `codex_mcp_config_args`/`toml_string` extraídos a nivel de
    módulo para poder probarlos sin credenciales.
  - `claude_gate`: subconjunto (open, project.inspect, catalog.status,
    negativo); reutiliza `claude_items`/`validate_claude_session` de
    `scripts/test-m6-clients.py` sin reimplementarlos (mismo inventario de
    36 tools).
  - `gemini_gate`: `agy mcp add`/`remove` alrededor de `agy --model
    gemini-3.8-flash-high -p … --output-format json --sandbox`; intenta un
    `HOME` privado primero y, si `agy` no lo respeta, cae a un `mcp
    add`+`remove` sobre el `HOME` real (nunca deja el servidor registrado).
  - `compose_prior_receipts`: invoca `test-m{2,3,4,5,6}-clients.py --run` como
    subprocesos y pliega sus propios recibos — **no reimplementa** sus 500+
    líneas de cobertura positiva cada uno, tal como pide el encargo.
  - `eof_gate`: un `tools/call` en vuelo, EOF de stdin sin leer la respuesta,
    exige salida acotada del proceso.
  - El driver Inspector (`run_inspector`) añade, solo en `--with-runtime`,
    una llamada real a `rust.check`, la lectura de su `rust-artifact://`
    publicado y el ciclo cancelar-reintentar (`notifications/cancelled`
    vía `client.cancelToolCall()`, delegado al `.mjs`).
- `scripts/test-m8-clients-unit.py` (nuevo, 459 líneas, **61 tests**
  herméticos): inventario, vocabulario de error (screaming vs. snake),
  completitud/validación del plan negativo (incl. rechazo de `project_ref`/
  fingerprint hard-codeados), igualdad de contrato contra un manifiesto
  sintético, validadores de fila (`validate_negative_rows`/
  `validate_generic_negative_rows`), argv del host Docker-free, escape TOML
  de Codex, preflight no-ejecutante, detector de credenciales, validador de
  metadatos de protocolo, reutilización de M3/M6, y `compose_prior_receipts`
  con `subprocess.run` mockeado.
- `scripts/m8-inspector-session.mjs` (nuevo, 209 líneas): discovery, igualdad
  de contrato (hashes canónicos recalculados en Node con la misma fórmula),
  `resources/list` vacío, las 31 filas negativas + 4 genéricas, y en modo
  `runtime` un `rust.check` real, lectura de Resource y el ciclo de
  cancelación — mismo patrón que `m6-inspector-session.mjs` (Python decide,
  Node solo ejecuta y reporta).
- `scripts/gate.py`: una etapa `core` nueva, `m8-client-harness-tests`
  (`scripts/test-m8-clients-unit.py`), junto a `m6-runtime-unit-tests`. (Nota:
  otro paquete concurrente — W18, performance — añadió su propia etapa
  inmediatamente después en el mismo commit de trabajo; ambas conviven sin
  conflicto, verificado con `git diff`.)
- `.github/workflows/sonarcloud.yml`: línea de cobertura para
  `scripts/test-m8-clients-unit.py` (añadida tras la línea de W18, ya
  presente al aplicar este cambio).
- `sonar-project.properties`: `scripts/test-m8-clients.py` y
  `scripts/m8-inspector-session.mjs` añadidos a `sonar.coverage.exclusions`
  (mismo criterio que `test-m6-runtime.py`/`m*-inspector-session.mjs`: rutas
  que solo se ejercitan con Docker/Node/Codex/Claude/agy reales, nunca en el
  runner de cobertura).

## Preflight (ejecución real)

`python3 -B scripts/test-m8-clients.py --preflight`:

```
status: blocked
unsatisfied: ['gemini_version']
inventory: {count: 36, stable_count: 31, preview_count: 5}
negative_call_plan rows: 31
generic_negative_plan rows: 4
claude_code   2.1.268 (Claude Code) -> 2.1.268 (Claude Code)   (satisfecho)
codex         codex-cli 0.154.0    -> codex-cli 0.154.0        (satisfecho)
gemini_cli    1.2.1                -> 1.2.2                    (NO satisfecho)
inspector     2.5.0                -> 2.5.0                    (satisfecho)
```

Único precondition insatisfecho: `agy` instalado en este host reporta `1.2.2`,
no el `1.2.1` fijado por el encargo. El resto (binario candidato, inventario
de 36 tools, `version --json` = 0.8.0, manifiesto de freeze, Node, bundle de
Inspector, Codex, Claude Code + sesión iniciada, `docker` presente, fixture
con manifiesto) está satisfecho. `execution_performed`/`clients_started`/
`docker_used` son `false`: nada se ejecutó.

## Tests

- `python3 -B scripts/test-m8-clients-unit.py` → **61 tests, OK**.
- `python3 -B scripts/test-gate-reporting.py` → 13 tests, OK (la nueva etapa
  `m8-client-harness-tests` no rompe las convenciones que audita este script).
- `python3 -B scripts/docs-hygiene.py links-check` → 0 rotos en documentos
  vivos (2725 resueltos; 459 rotos en «frozen records» son preexistentes,
  reportados por el propio script como tales).
- `python3 -B scripts/check-architecture.py` → PASS (verificación adicional,
  no pedida explícitamente, para descartar que los nuevos scripts rompieran
  algo).

No se ejecutó `--run` (ni con ni sin `--with-runtime`): por instrucción
explícita, eso le corresponde al orquestador.

## Risks

1. **Vocabulario de error validado por pertenencia, no por código exacto.**
   Cada fila Docker-free exige `status ∈ {blocked, unavailable}` y, si hay
   `error_code`, que esté en el vocabulario cerrado de esa tool (leído de su
   propio `enum` fuente) — pero no fija cuál de esos códigos debe salir
   primero. Es la decisión correcta dado que no pude ejecutar el binario para
   verificar el orden exacto de precondiciones en los 31 casos (a diferencia
   de M5/M6, que sí fijan un código único porque sus autores corrieron el
   binario real). El primer `--run` real revelará si algún tool cae en un
   código fuera de lo esperable para su categoría (p. ej. si `rust.catalog.status`
   nunca toca Docker y devuelve algo fuera de `{SANDBOX_DENIED,
   COMMAND_TIMEOUT, OUTPUT_LIMIT_EXCEEDED}`, la fila fallará limpiamente con
   un mensaje claro, no silenciosamente).
2. **Codex/Gemini CLI drivers no ejecutados de verdad.** `codex exec -c
   mcp_servers…` y `agy mcp add`/`-p --output-format json` están escritos
   contra la salida de `--help` real de ambos binarios (verificada en este
   host) pero nunca se han corrido contra el servidor 0.8.0. El parseo de
   eventos usa búsqueda genérica (`find_values`) en vez de asumir un esquema
   JSONL exacto, precisamente para tolerar esa incertidumbre, pero la
   clasificación `passed/partial/capacity_refused` es la primera vez que se
   ejecuta.
3. **`gemini_version` bloqueado por el host real (1.2.2 vs. 1.2.1 fijado).**
   El orquestador decide si recalibra el pin o corre con la versión instalada;
   el harness no lo decide por su cuenta.
4. **`agy`'s aislamiento de `HOME`** no fue verificable sin ejecutar `--run`
   (el `--help` de nivel superior de `agy` requiere aprobación interactiva en
   este entorno y no se forzó). El fallback a `HOME` real + `mcp remove` está
   implementado pero no ejercitado.

## Open issues

- **Claude Code no es un flujo model-directed completo como M6**: es un
  turno acotado de 5 pasos fijos (open, inspect, catalog.status, negativo,
  resumen), consistente con «subconjunto» del encargo pero más simple que el
  M6/M5 `validate_runtime_model_flow`. Si se quiere paridad total con esos
  arneses habría que portar su normalizador de transcript completo (ya
  reutilizado vía `load_m6()` para `claude_items`/`validate_claude_session`,
  pero no para un `validate_*_model_flow` propio de M8).
- **`compose_prior_receipts` invoca M2–M6 con `--run --docker-socket` a
  secas**: no reconstruye las banderas específicas de cada arnés (p. ej. M4
  necesita fixtures de seguridad, M5 necesita el vendor tree). Si esos
  arneses exigen argumentos adicionales que hoy solo inyectan en su propio
  `main()`/`run()` a partir de variables de entorno, esto ya está cubierto
  (`RUST_MCP_TEST_SOCKET` vía `--docker-socket`); si alguno necesita más,
  este composer fallará limpiamente con su `stderr_tail` en el recibo, no en
  silencio.
- **El manifiesto `docs/validation/M8/freeze-0.8.0.json`** se carga y valida
  su forma (`tool_count`, nombres) en `load_freeze_manifest`, pero la
  igualdad de contrato en sí solo se prueba de verdad contra un manifiesto
  **sintético** en el unit test (61 tests); la comparación contra el
  manifiesto real de 36 tools solo ocurre dentro de Inspector en `--run`.
