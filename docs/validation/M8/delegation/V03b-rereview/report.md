# V03b — informe del revisor (Claude Opus 5, Medium, read-only; claude 2.1.268)

Invocación: `claude -p --model opus --effort medium --tools 'Read,Grep,Glob' … < input.md` (input sha256 `515e0d5cb20495ef91693e7665f6d7556fa15b116e4d1350b188c86f8285e87a`). Inicio 2026-09-15T02:01:47Z, fin 2026-09-15T02:04:17Z, 149008 ms, 28 turnos.

---

# V03b: veredicto **Block** (limitado)

Hay tres puntos P2 sin cerrar: el oráculo de Codex (C-2), el control positivo del rollback (R-1(iii)) y la evidencia del ensayo de release, que quedó desactualizada tras S-1. Son arreglos pequeños y localizados. Los demás findings aceptados quedan cerrados sin regresiones.

## P2

**P2-1 · C-2 sigue abierto: el oráculo de Codex acepta todavía una prueba sacada del transcript del modelo.**
- En `scripts/test-m8-clients.py:1163-1169`, `unknown_tool_refused = evidence["unknown_tool_wire_refused"] or unknown_tool_event_refused`. El segundo término busca el texto `rust.not.a.real.tool` en cualquier campo `tool`/`name` de los eventos del modelo. Es justo el oráculo débil que C-2 pedía retirar.
- El docstring de `codex_protocol_evidence` dice «derived only from the wire», pero `codex_gate` no lo cumple.
- Arreglo: que `passed` exija `unknown_tool_wire_refused`. La señal de eventos puede quedar solo como dato informativo.

**P2-2 · R-1(iii): el escenario (a) puede salir `passed` sin haber evaluado el control de `doctor`.**
- En `scripts/test-m8-rollback.py:429-440`, `doctor_ok` empieza en `True`. Si `doctor` falla o no devuelve `mutation_journals.downgrade_blocked`, solo se añade un gap y `ok` sigue siendo verdadero.
- El estado global (`:734`) solo mira `status`, así que ese gap no impide el `passed`.
- El escenario (b) sí falla con su gap (`b_quality_ok = False`, `:532`), así que (a) es incoherente con él.
- D-1 y D-5 ya están en el árbol, por lo que la degradación ya no tiene justificación. Arreglo: `doctor_ok = False` en la rama `else`.

**P2-3 · La evidencia de M8-07 contradice el contrato tras S-1.**
- `docs/validation/M8/07.md:70` y `07-release-rehearsal.json:123` registran la plantilla antigua `…?offset={n}&length={n}`, con la que se hizo la «verificación manual» de `resources/templates/list`. El ensayo de release es anterior al cambio de plantilla y hay que repetirlo, o al menos esa sección, sobre los bytes commiteados.
- `docs/validation/M8/01-census.json:2706` (modificado en el árbol) también conserva el literal antiguo. Si es un censo vivo, hay que regenerarlo; si es histórico, conviene anotarlo.

## P3

- **C-3, respuesta ligada al id:** `generic_negative_wire_confirmed` y `codex_protocol_evidence` solo comprueban que la fila siguiente venga del servidor. No verifican el id ni que sea un error, porque `id` no está entre los campos permitidos del proxy (`safe_keys`, `:796`). La disposición pedía «respuesta del servidor a ese id». Es aceptable para la sesión secuencial del Inspector, pero es frágil con Codex, que puede intercalar mensajes. Propuesta: registrar un hash del id, o documentar el límite.
- **P-2:** `regression_verdict` cuenta `insufficient_samples` como si fuera «within». Con dos recibos `insufficient_samples` y uno `over`, el resultado es `not_regressed`. Debería contar como `unavailable`.
- **D-5 / `docs/tools.md`, «solo lee»:** `NativeMutationStore::open` crea `mutation-store.lock` con `OFlags::CREATE` y hace fsync del directorio (`mutation.rs:387`, `:1253-1254`). `doctor` escribe ese archivo si no existe (igual que `mutation list`). Hay que precisar el texto «passive / reads only».
- **S-1 y el oráculo del freeze:** `contract-freeze.py` y `freeze-0.8.0.json` no cubren `resources[]`. Por eso `verify --strict` pasa sin detectar el cambio de plantilla, y el único guardián es el test wire ↔ documento. Conviene declararlo o incluir las plantillas en el manifiesto.

## Verificado cerrado

- **D-1:** `kind` en el resumen, rellenado desde `operation_kind` tras `decode_envelope`. La regla es `pending > 0 ∨ kind ∉ KINDS_KNOWN_TO_0_3_0`, con recuentos por kind, la nota «or prune» y un test con un journal Committed `analyzer_action_apply` → `true`.
- **D-2 a D-4:** tests con marcador v9, bytes basura, `.DS_Store` y checksum roto, todos con conteos exactos. `Busy` tiene su propia nota.
- **D-5:** `--state-root` solo, con ruta absoluta; una ruta relativa sale con código 2.
- **D-6:** la redacción de ADR-088 §8 coincide con el test nuevo de revocación tras `Published`.
- **S-1 a S-3:**
  - la constante `QUALITY_TEMPLATE_SUFFIX` es compartida;
  - `contract --json` y `resources/templates/list` usan `{?offset,length}`;
  - el test wire ↔ documento exige igualdad exacta;
  - las listas se prueban con `VERSION` y todas las `LEGACY`.
- **Snapshots:** solo cambia `doctor-report.json` (`"mutation_journals": null`); ningún `*-tool.json` se modificó.
- **R-1(i)/(ii), R-2, R-4 a R-7:** controles de `mutation list` en HEAD y en 0.3.0 sobre el journal de control; M3 exige `validated ≥ 1` y `quarantined == 0` con ambos binarios; worktree fijado por commit y verificado limpio; `json_or_none` rechaza lo que no es objeto; los gaps de (d) están declarados.
- **P-1, P-3, P-5 y W27b:**
  - `isError != true` y `status == passed`, con descartes registrados;
  - `insufficient_samples` si `n` es menor que el del presupuesto, y una ventana mínima de 30 muestras de RSS;
  - exactamente 3 recibos con el mismo `budgets_sha256` y perfil, `indeterminate` y `--compare`;
  - `fd_after_ttl` se evalúa tras el TTL más una apertura (medido 27 → 8), con `retained_until_next_open` como dato informativo, coherente con `05.md` §Hallazgo del soak.
- **C-1, C-3 a C-7:**
  - `--run` aborta si falla una precondición obligatoria (Codex e Inspector no son opcionales) y registra `observed_versions`;
  - los negativos genéricos exigen `rpc_code` -32601/-32602;
  - `expected_code` es fijo por fila;
  - exit 1 si el recibo es `failed`;
  - `rename_all` se lee de la fuente;
  - el recibo lleva `head_commit` y `tree_dirty`, y los temporales van bajo `target/`.

Cuando se resuelvan los tres P2 y se regeneren los recibos sobre los bytes commiteados (R-3), el veredicto pasa a **Approve con findings** (los P3).
