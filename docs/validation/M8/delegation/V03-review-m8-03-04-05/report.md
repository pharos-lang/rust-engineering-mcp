# V03 — informe del revisor (Claude Opus 5, High, read-only: Read/Grep/Glob; claude 2.1.268)

Invocación: `claude -p --model opus --effort high --tools 'Read,Grep,Glob' --disallowedTools Agent Task Bash Edit Write --no-session-persistence --output-format json < input.md` (input sha256 `ed93b4a9f282ef9e7f18b4ec24cbd952259d246e23d5fc5ea6efb0a245267de6`). Inicio 2026-09-15T01:00:35Z, fin 2026-09-15T01:09:50Z, exit 0, 553319 ms, 50 turnos.

---

# V03: revisión de M8-03/04/05 (solo lectura, sin ejecutar nada)

**Veredicto: Block.** Hay un P1 en `doctor.mutation_journals` y varios P2 de gate y contrato que bloquean readiness. Todo sale de leer el código. No he comprobado ningún resultado de tests ni de gate.

## 1. `doctor.rs`, `tests/doctor.rs` y el snapshot (W14/W14b)

**Lo que está bien:**
- La sección es aditiva y `format_version` sigue en 1.
- No toca el workspace: abre el store con `write_roots` vacío (`&[]`).
- `serve` no cambia.
- No hay `unwrap` ni `expect`.
- El snapshot solo añade `"mutation_journals": null`.

**Findings:**

- **D-1 · P1 · `downgrade_blocked` da falsos negativos.**
  - **Evidencia:**
    - `doctor.rs:208` calcula `downgrade_blocked: pending > 0`.
    - El propio fixture de rollback deja un journal **Committed** (`rollback_native.rs:139`). `03-rollback.json` escenario (a) muestra que con ese estado 0.3.0 responde `"status":"blocked","error_code":"recovery_required"`.
    - `scan_store` falla cerrado para todo el store (`mutation.rs:1746`).
    - Sobre ese mismo estado, `doctor` 0.8.0 diría `terminal:1, downgrade_blocked:false`.
    - `MutationRecordSummary` no lleva el kind (`domain/src/mutation.rs:105-110`), así que `doctor` no puede calcularlo. ADR-088 §3 promete journals «por fase y kind».
    - `DOWNGRADE_NOTE` propone «recover or complete», pero completar no desbloquea. Hace falta `mutation prune` con 0.8.0.
  - **Acción:**
    - Añadir `kind` al summary (cambio aditivo).
    - Calcular `downgrade_blocked = pending > 0 || existe algún kind que 0.3.0 no conoce`.
    - Añadir recuentos por kind, o corregir ADR-088 y `docs/tools.md`.
    - Corregir la nota.
    - Añadir un test sobre un journal `analyzer_action_apply` Committed que espere `true`.

- **D-2 · P3 · El test «unrecognized kind» no pasa por la rama de kind desconocido.**
  - **Evidencia:** al reescribir `operation` sin rehacer el checksum, `decode_envelope` falla en el checksum (`mutation.rs:801-805`) antes de llegar a `operation_kind` (`:858`). El resultado es el mismo `RecoveryRequired`, pero el comentario «reproduces exactly» es falso. Tampoco hay test de envelope ilegible, y el assert `>= 1` es débil.
  - **Acción:**
    - Renombrar el test.
    - Añadir casos: `-journal-v2`→`-journal-v9` (llega a `:851`), bytes basura, y un archivo ajeno en el directorio.
    - Comprobar el informe exacto: `unknown_format==1`, `pending/terminal==0` y las notas.

- **D-3 · P3 · Es «pasivo» solo respecto al workspace.**
  - **Evidencia:** `open` crea `global.lock` con `O_CREAT` (`mutation.rs:387`) y hace fsync del directorio (`:1254`). `list_records` toma un `flock` exclusivo no bloqueante (`:400`). Una mutación de `serve` que coincida con el escaneo puede recibir `Busy`, y `doctor` lo reporta como «could not be read».
  - **Acción:** documentarlo, o añadir en el adapter un open de solo lectura con lock compartido, y dar a `Busy` una nota propia.

- **D-4 · P3 · `unknown_format` mezcla casos distintos.**
  - **Evidencia:** cuenta igual un formato o kind desconocido, un checksum roto, un archivo ajeno (p. ej. `.DS_Store`, `mutation.rs:1733`) y un nombre no UTF-8. La nota dice «cannot interpret».
  - **Acción:** reformular como «unreadable or unknown».

- **D-5 · P3 · La sección exige la tupla Docker completa.**
  - **Evidencia:** `host_config.rs:167-176` la requiere, mientras que `mutation list` acepta solo `--state-root`. En el escenario (d), el `doctor` devuelve `"mutation_journals":null`.
  - **Acción:** aceptar `--state-root` solo en `doctor`, o mostrar la tupla en el procedimiento del README.

- **D-6 · P3 · El fixture de permisos revocados no coincide con la redacción de D12 §8.**
  - **Evidencia:** revoca tras `Published`, no «entre preview y commit» como dicen D12 §8 y ADR-088 §8.
  - **Acción:** alinear la redacción.

## 2. `rollback_native.rs`, `test-m8-rollback.py` y `03-rollback.json` (W15)

**Lo que está bien:**
- El test nativo usa solo API pública (`SecureProjects`, `open_for_kind`, `commit`, `mutation_digest`) y deja un journal `analyzer_action_apply` real.
- Ambos builds y el fixture usan `--locked --offline`, y la ref se resuelve con `rev-parse --verify --end-of-options`.
- No se usa shell, y se comprueban los exit codes y el JSON parseado.
- Un escenario que no corre queda `unavailable`, nunca `passed`, y se verifica la versión de cada binario.
- (c) comprueba `CATALOG_ROLLBACK` más secuencia 2 y floor 2 después.

**Findings:**

- **R-1 · P2 · El escenario (a) no tiene control positivo.**
  - **Evidencia:** `test-m8-rollback.py:283-286` solo prueba que 0.3.0 falla cerrado, no que la causa sea el kind. Cualquier otra incompatibilidad (lock, permisos, envelope) daría el mismo `recovery_required` y marcaría `passed`.
  - **Acción:** sobre el mismo estado, exigir:
    - que 0.8.0 `mutation list` pase con 1 registro;
    - que 0.3.0 liste un journal de control `manifest_patch` escrito por 0.8.0, lo que prueba de verdad que el journal es compatible;
    - que `doctor` 0.8.0 con la tupla devuelva `downgrade_blocked:true` (enlaza con D-1).

- **R-2 · P2 · La parte M3 de (b) y (d) no prueba nada.**
  - **Evidencia:** `quality-artifacts recover` corre sobre un state-root vacío, y las cuatro ejecuciones del recibo dan `"validated":0`.
  - **Acción:** crear al menos un artifact confirmado y exigir `validated >= 1, quarantined == 0` en los dos sentidos. Si no, reducir lo que afirman el recibo y ADR-088 §6(b)(d).

- **R-3 · P2 · El recibo se generó con el árbol sucio.**
  - **Evidencia:** `"head_tree_dirty": true` sobre `6fa1ef1`, así que el sha del binario head no corresponde a ningún commit.
  - **Acción:** regenerarlo sobre bytes commiteados. Lo mismo vale para `05-measurement.json` y `clients.json` (ver P-4 y C-7).

- **R-4 · P3 · La etiqueta dice «pending» y el fixture es terminal.**
  - **Evidencia:** la descripción y el docstring (l.16, l.61) hablan de un journal pendiente, pero el fixture está Committed.
  - **Acción:** renombrar el escenario, o declarar que no se puede alcanzar un journal pendiente con la API pública.

- **R-5 · P3 · `ensure_worktree` no verifica el worktree que reutiliza.**
  - **Evidencia:** pasa el tag del usuario, no el SHA resuelto, a `git worktree add` sin `--end-of-options`, y reutiliza un worktree registrado sin comprobar `HEAD == old_commit` ni que esté limpio.
  - **Acción:** usar el SHA, y comprobar `rev-parse HEAD` y `status --porcelain`.

- **R-6 · P3 · Un JSON que no sea objeto rompe el driver.**
  - **Evidencia:** en `json_or_none(...) or {}` seguido de `.get`, un JSON que sea lista o número lanza `AttributeError`, y entonces no se escribe recibo.
  - **Acción:** comprobar `isinstance(payload, dict)`.

- **R-7 · P3 · El upgrade (d) no cubre el journal M2.**
  - **Evidencia:** es el único formato sin TTL con lector legado según D12 §4, y (d) no lo ejercita.
  - **Acción:** cubrirlo o declararlo como hueco.

## 3. Listas wire en `stdio.rs` y `protocol.rs` (W21)

**Lo que está bien:**
- `list_tools` no cambia y ningún snapshot `*-tool.json` aparece modificado.
- Sin override, rmcp devolvía `Default` sin `ttlMs` (`rmcp-3.2.0 server.rs:381,388,397`). En legacy el cambio se reduce a añadir `ttlMs`/`cacheScope` (como ya hace `tools/list`, `protocol.rs:973-976`) y las 2 plantillas. Es aditivo.
- `mimeType: application/octet-stream` coincide con la lectura en blob (`resources.rs:253`).
- Omitir el MIME en `rust-quality-artifact` es correcto, porque el índice es JSON y el chunk son bytes.

**Findings:**

- **S-1 · P2 · La plantilla de quality no describe las URIs reales (contrato a punto de congelarse).**
  - **Evidencia:** `?offset={n}&length={n}` usa la misma variable para dos valores, y RFC 6570 expande ambas con el mismo valor. Además, las URIs de índice no llevan query (`resources.rs:59`), así que no encajan. El mismo literal está en `capability_document.rs:43`.
  - **Acción:** usar `…/{quality_job_id_or_artifact_id}{?offset,length}`, o dos plantillas (índice y chunk), en ambos sitios antes del freeze.

- **S-2 · P3 · «Same source of truth» es exagerado.**
  - **Evidencia:** los prefijos salen de `format!`, pero sufijos y nombres están duplicados. El test guarda solo `starts_with` (`capability_document.rs:722-723`).
  - **Acción:** una sola constante compartida, o un test que compare la lista wire con el documento.

- **S-3 · P3 · El test nuevo cubre solo una versión legacy.**
  - **Evidencia:** prueba `2025-06-18`; los tests vecinos iteran sobre `LEGACY`.
  - **Acción:** iterar sobre `LEGACY`.

- **S-4 · P3 · Falta la entrada en `CHANGELOG.md`.**
  - **Evidencia:** no hay entrada para las listas ni para el defecto `ttlMs` corregido.

## 4. Performance y soak (W18/W20)

**Lo que está bien:**
- Cold N=30 descartando el primer proceso (`measure:215-217`); warm N=30; dispatch N=30 por tool; RSS idle N=10 con 5 s y máximo.
- Raw samples en el recibo, p95 por nearest-rank, y `unavailable` no cuenta como pass.
- Los criterios del soak están fijados en código y coinciden con `05.md` (×1,2, +10, 3×50 ms, meseta 5 %). El churn va después de evaluar y fuera de `fd_growth`.
- Reglas Sonar: sin `/tmp` literal, sin `type=` y sin `shell=True`; el scratch va en `target/`.

**Findings:**

- **P-1 · P2 · No se validan las respuestas.**
  - **Evidencia:** el resultado de `call_tool` se descarta (`measure:254`, `soak:218-219`), y `request` solo falla ante un JSON-RPC `error`. Un refusal con `isError` se cronometra como dispatch válido. Con `dispatch_catalog_status_ms` p95 = 0,062 ms, merece comprobarse.
  - **Acción:** exigir `status=="passed"` e `isError!=true` en cada llamada, y registrar las que fallen como descartadas.

- **P-2 · P2 · La regla 2-de-3 no se puede aplicar tal como está.**
  - **Evidencia:**
    - `regression_verdict` acepta de 1 a 3 recibos (`measure:352`). Con un solo recibo en `over` nunca hay regresión.
    - `unavailable` cuenta como «no over», y el unit test l.120-128 lo fija.
    - No se valida que los recibos sean consecutivos ni que compartan budgets y perfil.
    - No está conectada a ninguna CLI ni al gate: `gate.py:187` solo corre los unit tests.
  - **Acción:**
    - Exigir exactamente 3 recibos.
    - Devolver `indeterminate` si hay `unavailable` y el caso no está decidido.
    - Comprobar el mismo `budgets_sha256`.
    - Exponer `--compare r1 r2 r3`.

- **P-3 · P2 · No se exige el tamaño de muestra del presupuesto.**
  - **Evidencia:** `rss_peak_core_mib` sale con `"n": 1` (`05-measurement.json:580`) frente a `n: 30` en el presupuesto, y aun así da `within`. En la práctica el pico no se midió.
  - **Acción:** marcar `insufficient_samples` si `len < row["n"]`, y fijar una ventana de muestreo mínima.

- **P-4 · P2 · La recalibración no quedó registrada.**
  - **Evidencia:** `05-measurement.json` es la primera medición con N≥30 (árbol sucio, sha `5bb2…`, distinto del binario de `clients.json`). `05.md` no la registra y `05-budgets.json` sigue `provisional: true`, pese a `05.md:24-27` («una sola vez, inmediatamente»).
  - **Acción:**
    - Registrarla en `05.md`, aunque sea «sin cambio», enlazada al sha del recibo y del budgets.
    - Hacerlo sobre bytes commiteados.
    - Añadir `head_tree_dirty` y `budgets_sha256` al recibo.

- **P-5 · P2 · El soak dejó de cubrir la ruta que falló.**
  - **Evidencia:** la fase principal ya no ejercita `rust.project.open`, el churn no tiene criterio y `--ttl-wait-seconds` vale 0 por defecto. Aun así, las notas del recibo afirman «bounded by the idle TTL… not a leak» (`soak:364-368`) sin evidencia. El cambio está documentado en `05.md` y no es un ajuste de presupuesto, pero quitó cobertura justo donde hubo fallo.
  - **Acción:** poner un criterio sobre FDs tras el TTL, o declarar el hueco residual y retirar la afirmación.

- **P-6 · P3 · `state_root_orphans` pasa por decreto.**
  - **Evidencia:** está fijado a `passed: True` con `applicable: False`, pero el soak sí usa un catalog-store que podría revisarse (`soak:118-131`).
  - **Acción:** revisar huérfanos en el catalog-store, o poner `passed: null`.

- **P-7 · P3 · Los controles de ruido son constantes declaradas, no medidas.**
  - **Evidencia:** `same_binary_all_samples: true` no se re-hashea al final, «operator-verified» no tiene flag de atestación, y no se registra `pmset` (batería o low power).
  - **Acción:** re-hashear al final, añadir un flag de atestación del operador y registrar `pmset`.

- **P-8 · P3 · Detalles del soak.**
  - **Evidencia:**
    - `reopens` nunca se incrementa, aunque el docstring describe una reapertura que no existe.
    - `request` no comprueba el id.
    - Si la última muestra trae `None`, se evalúa como «final» una muestra anterior (`soak:340-345`).

- **P-9 · P3 · Riesgo de cobertura en Sonar.**
  - **Evidencia:** `measure-m8-performance.py` y `soak-m8.py` son sources sin exclusión de cobertura (`sonar-project.properties:5,16`).
  - **Acción:** añadir más tests, o excluirlos con justificación.

## 5. Clientes: `test-m8-clients.py`, `m8-inspector-session.mjs` y `clients.json` (W19–W21)

**Lo que está bien:**
- Los hashes canónicos se recalculan desde el `tools/list` en vivo, con la misma forma que `contract-freeze.py` (claves ordenadas, separadores compactos, UTF-8 sin escapar: `mjs:28-39` frente a `freeze:54-56`), y se comparan contra `freeze-0.8.0.json`.
- Hay 30 refusals estructurados más `catalog.status` como observación. Una respuesta no estructurada da `status: null` y el harness lanza error.
- Un cliente opcional `unavailable` no hace fallar la matriz y no aparece como calificado en README, compatibility ni en el recibo.

**Findings:**

- **C-1 · P2 · `--run` no exige las versiones fijadas.**
  - **Evidencia:** nunca llama a `preconditions()`. Escribe las constantes `"version": CODEX_VERSION` (l.1003) e `INSPECTOR_VERSION` (l.840) sea cual sea el binario observado. Un Codex o Inspector distinto pasaría quedando registrado con la versión fijada.
  - **Acción:** abortar si falla alguna precondición obligatoria, y registrar las versiones observadas.

- **C-2 · P2 · El oráculo de Codex (cliente obligatorio) es débil.**
  - **Evidencia:** da `passed` con exit 0 y cualquier valor `name`/`tool` que contenga `rust.project.open` (l.996-1000); un listado de discovery puede cumplirlo. No se verifican ni el inspect ni el negativo.
  - **Acción:** derivarlo de `protocol.jsonl` (`client=="codex"`, `tools/call`, `tool` ∈ {open, inspect}, más el refusal de la tool desconocida).

- **C-3 · P2 · Los negativos genéricos aceptan cualquier excepción.**
  - **Evidencia:** `catch {}` convierte cualquier excepción en `protocol_error: true` (`mjs:83-92`): validación del lado del Inspector, timeout o caída del server. Puede que la frontera del server nunca se ejercite.
  - **Acción:** capturar `error.code` (se espera -32602/-32601) y confirmar en `protocol.jsonl` que el server respondió a ese id.

- **C-4 · P2 · `run()` devuelve 0 aunque el recibo diga `failed`.**
  - **Evidencia:** `return 0` en l.1257 también cuando Codex queda `partial`.
  - **Acción:** devolver `0 if passed else 1`.

- **C-5 · P3 · El oráculo negativo acepta demasiado.**
  - **Evidencia:** acepta `error_code: null` en un refusal (l.748) y cualquier código declarado. Los rationales no coinciden con lo observado (`deny` → `SECURITY_POLICY_INVALID`, `flamegraph` → `PROFILING_NOT_AUTHORIZED`).
  - **Acción:** exigir código no nulo y fijar el esperado por fila.

- **C-6 · P3 · La extracción de vocabulario por regex es frágil.**
  - **Evidencia:** ignora `#[serde(rename)]` y no verifica el `rename_all` del enum (l.419-437).

- **C-7 · P3 · Higiene del script y del recibo.**
  - **Evidencia:**
    - El recibo no lleva `head_commit` ni `tree_dirty`.
    - `type=pathlib.Path` en l.1265 va contra la regla «sin `type=`».
    - Hay literales `/private/tmp` (l.115, l.1206).
    - La copia de `auth.json` de Codex va a `mkdtemp()` del sistema (l.966) en vez de `target/`.
    - El docstring habla de «one structured refusal per stable tool», lo que contradice la fila de observación.

## Para desbloquear

1. **D-1:** kind en el summary, `downgrade_blocked` correcto, nota y test.
2. **Rollback:** R-1 (controles positivos en (a)) y R-2 (estado M3 no vacío).
3. **Plantilla quality:** S-1, antes del freeze.
4. **Performance:** P-1 a P-4 (validar respuestas, 2-de-3 estricto, n mínimo, recalibración registrada) y P-5 (criterio de churn o hueco declarado).
5. **Clientes:** C-1 a C-4 (pins exigidos, oráculo de Codex, negativos genéricos, exit code).
6. **Recibos:** regenerar `03-rollback.json`, `05-measurement.json` y `clients.json` sobre bytes commiteados (R-3/P-4/C-7).
