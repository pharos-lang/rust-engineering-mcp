# V02 — informe del revisor (Claude Opus 5, High, read-only: Read/Grep/Glob; claude 2.1.268)

Invocación: `claude -p --model opus --effort high --tools 'Read,Grep,Glob' --disallowedTools Agent Task Bash Edit Write --no-session-persistence --output-format json < input.md` (input sha256 `8d7edb2ffbf9a24e5c813fcfab8a4978d35db19af2a41d622b42940d04365be6`, 92 892 bytes: prompt + `git status` + diff de crates/scripts/Cargo.toml). Inicio 2026-09-14T17:32:49Z, fin 2026-09-14T17:38:44Z, exit 0, 353779 ms, 48 turnos, modelos ['claude-haiku-4-5-20251001', 'claude-opus-5'], permission_denials: [].

---

# V02: revisión del freeze 0.8.0

**Veredicto: Block.** Hay cuatro P2 de contrato o gate que bloquean la readiness; los cuatro se arreglan con poco trabajo. No hay P0 ni P1.

Lo esencial del freeze está bien:
- Solo cambian los 5 snapshots `preview`.
- El prefijo solo altera la `description`.
- El hash canónico coincide entre Rust y Python.
- `verify` detecta cualquier cambio en un `stable` y cualquier cambio de conteo.
- D13 está redactado con honestidad.

Los fallos son tres: datos falsos en `executes_project_code`, dos formas de que el gate dé un falso pass, y un subcomando CLI nuevo sin clase de estabilidad.

## Findings

### P2-1. `crates/mcp-server/src/stdio/capability_document.rs:56-316`: la tabla `executes_project_code` contradice las descripciones de las propias tools
- **`rust.benchmark.compare`:** la tabla dice `true` con `RequiredRuntime::None`. Su descripción dice «Runs no process and reads no project source».
- **Las cinco `rust.analyzer.*`:** la tabla dice `true`. Sus descripciones dicen «Build scripts, proc macros and check-on-save stay disabled; only textDocument/… runs».
- **`rust.fmt.check` y `rust.fmt.apply`:** la tabla dice `true`, pero solo ejecutan rustfmt. Las descripciones de check y clippy sí dicen «Can execute build scripts»; las de fmt no.
- **Sin fuente ni test de valores:** el comentario dice que la tabla sale de `01-census.json`, pero el censo solo tiene `requires_runtime`; `executes_project_code` no existe allí. `table_matches_the_full_tool_set` comprueba que los nombres existen, no que los valores sean correctos.
- **No hay infradeclaraciones:** revisé deny, unsafe.scan, manifest.patch y dependency.* y están bien en `false`. Pero spec §56 y §21 publican este campo como un efecto declarado, y este documento es el oráculo de las RC.
- **Acción:**
  - Corregir los 8 valores. Si la intención es «ejecución potencial conservadora», cambiar el nombre o documentar esa semántica.
  - Añadir el campo al censo con su motivo por tool.
  - Añadir un test que fije los valores, no solo los nombres.

### P2-2. `scripts/contract-freeze.py:136-151`: falso pass si una tool se reclasifica
- **El fallo:** `bucket(name, use_current=True)` usa la clase *actual* para decidir si un cambio es stable o preview. Si se añade `rust.check` a `PREVIEW_NAMES` y a la vez se cambia su schema, `verify` sin `--strict` (el modo del gate) solo avisa y devuelve 0. El conteo no cambia, así que nada más lo detecta.
- **Por qué importa ya:** es exactamente el camino que ADR-086 §1 prevé para M8-04: degradar a `preview` antes de RC1 una tool que no pase la prueba con clientes stock.
- **Acción:**
  - Para las tools que están en ambos lados, clasificar con la clase *registrada* en el manifiesto.
  - Tratar cualquier transición `stable→preview` como fallo salvo que se regenere el manifiesto a propósito.
  - Añadir un test para este caso.

### P2-3. `scripts/gate.py` (`if freeze_manifest.exists():`): la etapa se salta en silencio
- **El fallo:** si alguien borra o renombra `freeze-0.8.0.json`, la etapa `contract-freeze` desaparece y el gate `core` sigue en verde. Contradice la decisión 5 de 02.md, que hace la etapa obligatoria.
- **Acción:** ejecutarla siempre; si falta el manifiesto, el gate falla.

### P2-4. Falta la clase de estabilidad del subcomando `contract` y de su formato (CHANGELOG, `docs/compatibility.md:245`, `docs/tools.md:2049`)
- **El fallo:** ADR-086 §1 exige clase para «cada … comando CLI y formato en disco». `contract` y su `format_version: 1` nacen en el propio freeze y se anuncian como «oráculo de igualdad de contrato entre release candidates», pero no tienen clase.
- **Documentación incompleta:** las tres descripciones mencionan solo `stability`, `annotations` y los hashes de schema. Omiten `description_sha256`, `executes_project_code`, `requires_runtime`, `protocol` y `resources`.
- **Afirmación inexacta:** el CHANGELOG dice «verificado por el manifiesto … en la etapa `contract-freeze`». En realidad esa etapa verifica los snapshots. Lo que une la salida del CLI con los snapshots es `tests/cli.rs`, que corre en `cargo test`.
- **Acción:** declarar la clase del CLI y de su formato, documentar todos los campos y corregir la cadena de verificación descrita.

### P3
- **Procedencia de los JSON de evidencia** (`docs/validation/M8/freeze-0.8.0.json`, `02-schema-diff.json`): `head_commit` apunta a `dbc17f5`, pero los hashes salen de un árbol sin commit; los prefijos `Preview` no están en `dbc17f5`. Regenerarlos tras el commit, o registrar que el árbol estaba sucio. `verify` tampoco comprueba `format_version` ni `canonical`.
- **Lectura de `git show` en `contract-freeze.py:59,184`:**
  - Con `text=True` la salida se decodifica con el locale y con saltos de línea universales antes de volver a UTF-8, así que `snapshot_sha256` en una ref puede divergir. Leer bytes.
  - `--base` llega a `git` sin `--end-of-options` ni `rev-parse --verify`, lo que abre una inyección de opciones local (p. ej. `--output=`).
  - Sin `shell=True`: correcto.
- **Reglas Sonar:** sin literal `/tmp`, sin `type=` en argparse, rutas derivadas de `ROOT`. Correcto.
- **Afirmación de byte-identidad** (CHANGELOG, «30 contratos `stable` byte-idénticos a `0.3.0`»): `diff` compara hashes canónicos, no `snapshot_sha256`. La identidad de bytes solo está probada para las 13 M1 (vía `git diff`). Decir «canónicamente idénticos» o comparar también `snapshot_sha256` en `diff`.
- **Huecos en `scripts/test-contract-freeze.py`:**
  - no prueba cambios de description, annotations u outputSchema en un `stable`;
  - no prueba un desajuste solo de conteo, ni la reclasificación (P2-2);
  - no prueba el subcomando `diff`, aunque el docstring lo menciona;
  - le falta un vector no-ASCII equivalente al `café` de Rust.
- **Literales sin comprobar** (`capability_document.rs:22-38`): `PRIMARY_PROTOCOL_VERSION`, `NEGOTIABLE_PROTOCOL_VERSIONS`, `SDK` y `RESOURCE_TEMPLATES` son literales, y el test solo compara longitudes. Hoy coinciden (`rmcp 3.2.0` en el lock; `rust-artifact://` y `rust-quality-artifact://` en `resources.rs:32-33`). Comparar las cadenas contra `SUPPORTED_VERSIONS` y `PREFIX`/`QUALITY_PREFIX`.
- **Listas duplicadas:**
  - `tool_definitions()` repite a mano el orden de `list_tools`.
  - `tests/cli.rs` tiene una tercera lista de snapshots.
  - Los nombres `preview` están copiados tres veces: `stability.rs`, `cli.rs` y `contract-freeze.py`.
  - Hoy no hay falso pass, porque la cadena está cubierta de punta a punta: `protocol.rs:416-459` exige igualdad exacta entre el servidor vivo y los snapshots, `cli.rs` compara los hashes del CLI con los snapshots y el freeze compara snapshots con el manifiesto. Aun así, conviene una sola fuente compartida.
  - `cli.rs` no compara `annotations` con el snapshot.
- **Fallo silencioso** (`capability_document.rs:506-526`): si `document()` falla, sale con código 1 sin ningún mensaje en stderr.
- **Promesa sin calificar** (CHANGELOG, «Las 31 tools restantes quedan `stable`»; `compatibility.md:239`): falta la condición de ADR-086 §1, que es pasar M8-04 antes de RC1 o degradar a `preview`.
- **Redacción de `--rust-image`** (CHANGELOG): el flag existe desde M1; lo nuevo es la imagen M6. Aclararlo.

## Respuestas a las preguntas

**1. Contrato.** Correcto, salvo P2-1 y P2-4.
- Solo están modificados los 5 snapshots `rust.analyzer.*`, y en ellos solo cambia la línea `description`.
- Los `definition()` extraídos mantienen literalmente los mismos textos y annotations; `quality::build_tool` es equivalente.
- **Estático:** no toca el host. `advertised()` devuelve `true` siempre, y la variable de entorno solo existe con `test-hooks`, así que el resultado no cambia.
- **Determinista:** usa `canonicalize` y un `Map` ordenado.
- **Reproducible desde Python:** el orden de claves por UTF-8 equivale al orden por punto de código; serde no escapa no-ASCII, igual que `ensure_ascii=False`; y no hay números en notación exponencial en los snapshots (0 coincidencias).
- **Tabla cerrada:** es exhaustiva y está probada contra `tool_definitions()`. Esa función queda ligada al servidor vivo de forma transitiva, a través de los snapshots.

**2. Oráculo de freeze.**
- `verify` falla ante cambios de nombre, schema, annotations, description o conteo en un `stable`.
- **Vista completa:** cada snapshot de tool tiene exactamente 5 claves de primer nivel (`name`, `description`, `inputSchema`, `outputSchema`, `annotations`), así que el hash no se calcula sobre una vista parcial.
- **Falso pass:** posible por reclasificación (P2-2) y por ausencia del manifiesto (P2-3).
- **Límite propio del diseño:** un manifiesto regenerado en el mismo diff pasa siempre. Solo lo frena la revisión, o comparar contra el manifiesto de la RC anterior.

**3. Versión y migración.**
- La versión 0.8.0 es coherente en `Cargo.toml`, `Cargo.lock` (los 8 crates), CHANGELOG, README, compatibility e implementation-status.
- Las notas de migración son exactas:
  - 5 tools añadidas y 1 grant nuevo;
  - `bloat` solo cambia la `description` de `$defs/BloatProfile`, que ahora cita `M5/04-bloat-calibration.json`, en input y output;
  - 0 deprecaciones;
  - 13 M1 sin cambios frente a `v0.1.0`.
- Salvedades: la redacción de «byte-idénticos», la clase `stable` sin condición y la descripción de `contract` (P3 y P2-4).

**4. D13.** Correcto, sin findings.
- **ADR-087 §3:** dice «resuelto por cambio de alcance aprobado por el owner; nunca se marca como cumplido».
- **Spec §61:** conserva la matriz aspiracional y el párrafo de 0.1.0, y añade una nota.
- **Spec §97:** dice «se resuelve por cambio de alcance … no por la matriz aspiracional».
- **Hechos de 0.1.0/0.3.0:** no se reescribe ninguno.
- **README, compatibility y ci.md:** son coherentes con el ADR.

**Límites de esta revisión:** no tenía Bash, así que no ejecuté tests, `git diff` ni `contract --json`. Tampoco pude ver la diferencia de `Cargo.lock` frente a HEAD, ni comprobar si las revisiones MCP negociables cambiaron desde 0.3.0.
