# V02b — informe del revisor (Claude Opus 5, Medium, read-only: Read/Grep/Glob; claude 2.1.268)

Invocación: `claude -p --model opus --effort medium --tools 'Read,Grep,Glob' --disallowedTools Agent Task Bash Edit Write --no-session-persistence --output-format json < input.md` (input sha256 `5315a673d48c7f0d4d59e9d58f4c4aac5bef9d9240ead7533300892083cdad13`). Inicio 2026-09-14T18:14:44Z, fin 2026-09-14T18:16:43Z, exit 0, 118245 ms, 19 turnos.

---

# V02b: re-revisión de W09/W10

**Veredicto: Block.** Los cuatro P2 de V02 están cerrados en el código. El bloqueo es una regresión nueva: un test de W10 falla en cuanto se hace commit, así que el gate `core` sobre los bytes finales saldría en rojo. El arreglo es pequeño.

## Nuevo

**P2-N1: `DiffTests` depende de que el árbol no esté commiteado.**
- **Dónde:** `scripts/test-contract-freeze.py:266-280`.
- **Qué hace:** llama a `CF.cmd_diff("HEAD", …)` sobre el repo real y espera `len(changed) == 5` y `len(unchanged) == 31`.
- **Por qué pasa hoy:** los 5 snapshots `analyzer-*-tool.json` están modificados sin commit (`git status`: `M`).
- **Por qué rompe:** tras el commit, HEAD y el árbol coinciden, así que da `changed == []` y el test falla. Como `contract-freeze-tests` se ejecuta siempre (`gate.py:183`), el gate `core` falla. Además el test no es hermético.
- **Arreglo:** que no dependa de HEAD. Por ejemplo, fijar la base en `v0.3.0` y afirmar lo que ya recoge `02-schema-diff.json` (5 añadidas y solo `rust.binary.bloat` cambiada), o simular `git_bytes`/`snapshot_names_at_commit` con un fixture.

## Cierre de lo dispuesto en V02

**P2-1: `executes_project_code`. Cerrado.**
- `capability_document.rs:81-334` pone a `true` exactamente las 14 tools dispuestas, y el test `executes_project_code_matches_the_fixed_fourteen_tool_set` (`:606`) fija la lista literal ordenada.
- `rust.fmt.check`, `rust.fmt.apply`, `rust.benchmark.compare` y las cinco `rust.analyzer.*` quedan en `false`.
- El censo tiene el campo y su motivo en las 36 tools.

**P2-2: clase registrada. Cerrado.**
- `cmd_verify` clasifica con `old["stability"]` (`contract-freeze.py:174,185`).
- Cualquier cambio de clase va a `class_changed` y hace fallar siempre (`:181,210`).
- Un `tool_count` distinto también falla (`:187`).
- Los tests de reclasificación (`:221`) y de conteo (`:209`) detectan los dos casos.

**P2-3: etapa obligatoria. Cerrado.**
- La etapa está en la rama común del gate, sin `if` (`gate.py:184-185`).
- Si falta el manifiesto: mensaje por stderr y exit 1 (`:151-153`), con test en `:234`.

**P2-4: clase y docs de `contract`. Cerrado.**
- El subcomando y el documento (`format_version: 1`) figuran como `stable` desde 0.8.0 en CHANGELOG, `docs/tools.md` y `docs/compatibility.md`.
- Los tres documentos listan todos los campos que emite `document()`.
- Los tres describen la cadena real: protocol tests con servidor vivo == snapshots, `cli.rs` con CLI == snapshots, `contract-freeze` con snapshots == manifiesto.

**P3 aceptados:**
- **Cerrados:**
  - `git show` ya lee bytes y `resolve_commit` usa `--verify --end-of-options`.
  - `diff` compara también `snapshot_sha256`.
  - Hay vector no-ASCII en Python (`:89`) y en Rust (`café`, `:569`).
  - Los literales de protocolo, SDK y plantillas se comprueban por valor contra `SUPPORTED_VERSIONS`, `Cargo.lock` y los prefijos de `resources`.
  - `cli.rs` compara `annotations`.
  - Los fallos al construir o codificar el documento se escriben en stderr.
  - La promesa de «31 stable» ya incluye la condición M8-04.
  - La redacción de `--rust-image` está corregida (CHANGELOG:16).
- **Pendientes del orquestador:** `freeze-0.8.0.json` y `02-schema-diff.json` se generaron con el script anterior (`head_commit` `dbc17f5`). No tienen `tree_dirty` y el diff no tiene `bytes_identical`. Mientras no se regeneren tras el commit, la afirmación del CHANGELOG de «30 contratos `stable` byte-idénticos a `0.3.0`» no tiene respaldo en esos artefactos.

## P3 nuevos o residuales
- **`--human` sin clase declarada:** los docs y el CHANGELOG escriben `contract [--json]` y omiten `--human`, aunque `--help` sí lo lista. Conviene declarar que el formato `--human` no forma parte del contrato `stable`.
- **Cambio de nombre de una preview:** si una tool preview se quita y se añade otra preview con el mismo conteo, pasa sin `--strict`. Es coherente con ADR-086 y M8-09 usa `--strict`; lo anoto sin más.
- **Recuentos no verificados:** `verify` no comprueba `stable_count`/`preview_count` del manifiesto. El cambio de clase por tool ya lo cubre.
- **Fallo al escribir stdout:** `capability_document.rs:552-556` sale con FAILURE sin escribir en stderr. Es aceptable, pero es el único caso silencioso que queda.

## Regresiones
- **Hash canónico:** el hash de Rust sigue la especificación de Python y `cli.rs` lo reimplementa por separado para compararlo.
- **Snapshots:** este diff solo añade tests a `cli.rs` y no toca ningún snapshot.
- **Tests nuevos:** detectan los casos que deben detectar (conteos, prefijo, clase y hashes por tool).
- **Sin ejecutar:** es una revisión de solo lectura. Doy por buenas las cifras que dio el orquestador (20/20 tests de Python, 0 discrepancias de hash); no las he reproducido.
