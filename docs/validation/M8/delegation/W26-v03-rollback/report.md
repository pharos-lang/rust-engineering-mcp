# W26 — correcciones V03 del rollback (R-1, R-2, R-4..R-7)

## Task

Aplicar al arnés de rollback (`crates/project-adapter/tests/rollback_native.rs`,
`scripts/test-m8-rollback.py`, `scripts/test-m8-rollback-unit.py`) las
correcciones P2/P3 de `V03-review-m8-03-04-05/disposition.md` §2 (R-1, R-2,
R-4, R-5, R-6, R-7) más el campo `tree_dirty`/`version` por binario en el
recibo. Sin ejecutar el driver completo (`scripts/test-m8-rollback.py`); solo
los tests nativos nuevos y los unit tests del driver.

## Result

Las seis correcciones aplicadas y verificadas localmente. `cargo test`,
`cargo fmt --check` y `cargo clippy -D warnings` sobre los archivos tocados,
y `python3 -B scripts/test-m8-rollback-unit.py` (40 tests) en verde. El
driver completo (`scripts/test-m8-rollback.py`) no se ejecutó, por
instrucción explícita; lo correrá el orquestador sobre bytes commiteados
(R-3, fuera de este alcance).

## Files changed

### `crates/project-adapter/tests/rollback_native.rs`

- **R-4**: el test se renombró de
  `leaves_a_committed_analyzer_action_apply_journal_for_an_older_binary_to_reject`
  a `leaves_a_committed_journal_of_a_kind_unknown_to_v0_3_0_and_a_known_control_journal`,
  y la descripción del módulo ya no habla de un journal «pendiente» (el
  fixture siempre fue terminal; ver R-4 en el disposition, evidencia sobre
  `scripts/test-m8-rollback.py`, no sobre este archivo, cuya redacción del
  docstring ya no mencionaba «pending» en la versión de partida).
- **R-1(ii)**: el mismo test ahora deja, además del journal
  `analyzer_action_apply` (kind desconocido para v0.3.0), un segundo journal
  de control `manifest_patch` (kind que v0.3.0 sí conoce — mismo mecanismo de
  API pública: `SecureProjects`, `NativeMutationStore::open_for_kind`,
  `commit`, `mutation_digest`) en un **state-root disjunto**
  (`RUST_MCP_ROLLBACK_CONTROL_STATE_ROOT`, nueva env var), para que el driver
  pueda listarlo con v0.3.0 en aislamiento del journal de kind desconocido.
  El control es un `manifest_patch` real (no un no-op semántico): solo cambia
  un comentario final en `Cargo.toml`, así que `validate_manifest_patch`
  sigue aceptándolo (comparación estructural TOML, sin cambios) pero los
  bytes crudos difieren, por lo que el commit queda `Committed` (no
  `NoChange`) — verificado localmente; un no-op puro produce `NoChange`, que
  también es un estado terminal pero menos claro para el positivo.
- **R-2**: nuevo módulo `quality_artifact_fixture`, con
  `#[cfg(target_arch = "aarch64")]` adicional (el store M3 nativo,
  `NativeQualityArtifactStore`, es macOS+aarch64 solamente —
  `crates/project-adapter/src/quality_artifact_store.rs` — a diferencia del
  store M2, que es macOS sin restricción de arquitectura). El nuevo test
  `leaves_a_validated_m3_quality_artifact_for_an_older_binary_to_read`
  publica un artifact M3 real por API pública (`NativeQualityArtifactStore`:
  `reserve`, `ingest_member`, `publish_descriptor`, `read_chunk`) bajo
  `RUST_MCP_ROLLBACK_QUALITY_STATE_ROOT` y confirma en proceso, antes de
  salir, que `recover(&state_root)` ya reporta `validated>=1,
  quarantined==0`. `owner_binding` no necesita un grant `SecureProjects` real
  (es un hash puro sobre hechos declarados — ver comentario en el archivo),
  así que el fixture no depende de un proyecto abierto.

### `scripts/test-m8-rollback.py`

- **R-1**: escenario (a) reescrito con tres controles positivos sobre el
  mismo fixture:
  (i) `mutation list --json` de v0.8.0 sobre el mismo state-root exige
  `status=="passed"` y `>=1` registro;
  (ii) `mutation list --json` de v0.3.0 sobre el state-root que contiene
  **solo** el journal de control `manifest_patch` exige `status=="passed"` y
  exactamente 1 registro;
  (iii) `doctor --json --state-root <state_root>` de v0.8.0 exige
  `mutation_journals.downgrade_blocked == true`. Este último se implementó
  como control positivo real, no como hueco preparado: al probar contra el
  binario `HEAD` local, D-1 (kind-aware `downgrade_blocked`) y D-5 (`doctor`
  acepta `--state-root` solo) ya estaban aplicados por W25 en el árbol de
  trabajo — confirmado ejecutando el binario compilado contra los dos
  state-roots del fixture nativo (ver «Evidencia» abajo). El código de todos
  modos degrada a un hueco declarado (`gaps`, no `passed` ni fallo duro) si
  `doctor` rechaza `--state-root` solo o si el campo
  `mutation_journals.downgrade_blocked` no está presente, para no quedar
  frágil ante una reordenación entre W25 y W26 en una corrida futura.
- **R-2**: escenario (b) ya no ejecuta `quality-artifacts recover` sobre un
  state-root vacío. Ahora corre el fixture nativo M3 (vía nuevo helper
  `run_native_fixture`, con `--exact <test>` para no arrastrar el fixture M2)
  y exige `validated>=1, quarantined==0` bajo **ambos** binarios (nuevo
  helper `recovery_confirmed`). Si el fixture no corrió (arquitectura no
  aarch64 — detectado por `"running 0 tests"` en la salida de `cargo test`),
  se declara un hueco explícito (`gaps`) en vez de `failed` silencioso o
  `passed` vacío. Escenario (d): la dirección «M3/M2 escritos por v0.3.0» es
  inalcanzable por CLI en cualquier versión (ver R-7 abajo) — se mantiene el
  chequeo estructural existente (CLI corre limpio sobre store vacío) pero se
  renombran las variables (`qa_write_cmd`→`qa_init_cmd`,
  `qa_read_cmd`→`qa_upgrade_cmd`) y se declara el hueco explícitamente.
- **R-4**: `SCENARIO_DESCRIPTIONS["a"]` y el docstring del módulo ya no
  hablan de un journal «pending»; describen el journal terminal de kind
  desconocido más los tres controles positivos.
- **R-5**: `ensure_worktree(tag, commit)` ahora recibe el commit ya resuelto
  (no el tag mutable) y lo pasa a `git worktree add --detach <path>
  --end-of-options <commit>`. Al reutilizar un worktree registrado, nuevas
  funciones `worktree_head`/`worktree_is_clean` verifican `rev-parse HEAD ==
  commit` y `status --porcelain` vacío; cualquiera de las dos en falso aborta
  con `DriverError` (nunca reutiliza un worktree en el commit equivocado o
  con cambios locales).
- **R-6**: `json_or_none` ahora devuelve `None` también cuando el JSON es
  válido pero no es un objeto (lista, número, string, `null`), no solo
  cuando el parseo falla — así ningún `.get(...)` posterior puede lanzar
  `AttributeError`.
- **R-7**: escenario (d) declara explícitamente, en `gaps`, que ni el
  journal M2 ni el artifact M3 pueden ser escritos por v0.3.0 a través de un
  subcomando CLI (ambos solo se producen sobre una sesión MCP viva), citando
  los tests unitarios de lector legado v1→v2 del formato de journal
  (`legacy_v1_receipt_is_read_only_and_explicit_recovery_migrates_to_v2`,
  `terminal_legacy_v1_replay_migrates_only_after_exact_binding` en
  `crates/project-adapter/tests/support/native_mutation.rs`) como cobertura
  de formato en su lugar.
- **Ítem 4**: `binaries.old` y `binaries.head` llevan ahora `version` (de
  `read_version`) y `tree_dirty`. Para `old`, `tree_dirty` es el resultado
  verificado por R-5 (`worktree_is_clean`) — siempre `false` en una corrida
  que no abortó, porque un worktree sucio ahora es un `DriverError`. Para
  `head`, es el mismo booleano que `head_tree_dirty` (top-level), repetido
  junto al binario por simetría.
- Nueva función `run_native_fixture(test_name, env)`: ejecuta
  `cargo test ... --test rollback_native -- --ignored --exact <test_name>`,
  compartida entre el escenario (a) (fixture M2) y (b) (fixture M3).

### `scripts/test-m8-rollback-unit.py`

40 tests (antes 15): cobertura nueva para cada corrección —
`JsonOrNoneTests` (R-6, JSON no-objeto), `EnsureWorktreeTests` (R-5: crea con
`--end-of-options`+commit resuelto; reutiliza solo tras verificar
HEAD/limpieza; rechaza commit equivocado o worktree sucio),
`RecoveryConfirmedTests` (R-2: `validated>=1 ∧ quarantined==0`, nunca sobre
un store vacío que igual reporta `passed`), `ScenarioATests` reescrito para
los 3 controles positivos de R-1 (incluida la degradación a hueco cuando
`doctor` no soporta `--state-root` solo), `ScenarioBcQualityArtifactTests`
(R-2 en el escenario (b): pasa/falla/hueco por arquitectura),
`ScenarioDGapsTests` (R-7: siempre declara los dos huecos), y
`ScenarioResultTests` extendido para el nuevo parámetro `gaps`.

## Evidencia (tests nativos + doctor/mutation-list reales)

`cargo test -p rust-engineering-project --locked --offline --test
rollback_native -- --ignored` (con `RUST_MCP_ROLLBACK_STATE_ROOT`,
`RUST_MCP_ROLLBACK_CONTROL_STATE_ROOT`, `RUST_MCP_ROLLBACK_PROJECT_ROOT`,
`RUST_MCP_ROLLBACK_QUALITY_STATE_ROOT` apuntando a directorios frescos bajo
`target/`):

```
running 2 tests
test quality_artifact_fixture::leaves_a_validated_m3_quality_artifact_for_an_older_binary_to_read ... ok
test leaves_a_committed_journal_of_a_kind_unknown_to_v0_3_0_and_a_known_control_journal ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

Verificación manual de R-1(iii) y R-1(ii) con el binario `HEAD` (debug,
`target/debug/rust-engineering-mcp`) sobre los dos state-roots que dejó el
fixture — no forma parte del driver, solo confirma que la aserción del
driver es alcanzable hoy, no un hueco:

```
doctor --json --state-root <state>    -> mutation_journals.downgrade_blocked == true,
                                          downgrade_blocking_kinds == ["analyzer_action_apply"]
doctor --json --state-root <control>  -> mutation_journals.downgrade_blocked == false
mutation list --json --state-root <state>    -> status=passed, 1 registro (analyzer_action_apply)
mutation list --json --state-root <control>  -> status=passed, 1 registro (manifest_patch)
```

## Tests

- `cargo test -p rust-engineering-project --locked --offline --test
  rollback_native -- --ignored` → 2 passed (ver arriba).
- `python3 -B scripts/test-m8-rollback-unit.py` → 40 tests, OK.
- `cargo fmt --all -- --check crates/project-adapter/tests/rollback_native.rs`
  → limpio (verificado también con `rustfmt --check` directo sobre el
  archivo, porque `cargo fmt --all` sigue reportando diffs preexistentes en
  `crates/mcp-server/src/doctor.rs` y `crates/mcp-server/tests/doctor.rs`,
  fuera del alcance/de los archivos permitidos de este paquete — trabajo en
  curso de W25).
- `cargo clippy -p rust-engineering-project --all-targets --locked --offline
  -- -D warnings` → limpio.

## Risks

- El driver completo (`scripts/test-m8-rollback.py`) no se ejecutó en este
  paquete (instrucción explícita: lo corre el orquestador sobre bytes
  commiteados, R-3). En particular, el binario real `v0.3.0` no se construyó
  ni se ejecutó aquí; la corrección de R-1/R-2 se validó (a) leyendo el
  contrato CLI exacto en `crates/mcp-server/src/mutation_cli.rs` y
  `crates/mcp-server/src/quality_artifact_cli.rs` (nombres de campo,
  `status`/`error_code`/`records`/`data.validated`/`data.quarantined`), y (b)
  ejecutando esas mismas invocaciones contra el binario `HEAD` local (arriba)
  — no contra v0.3.0 real. La primera confirmación end-to-end con el binario
  v0.3.0 real la hace el orquestador.
- El fixture M3 (R-2) requiere macOS+aarch64; en cualquier otro host el
  escenario (b) declara el hueco explícito y falla (no hay forma honesta de
  reportarlo `passed`). Esta sesión corrió en macOS/aarch64 (`arm64`), así
  que el camino feliz sí se ejerció de punta a punta a nivel del fixture
  nativo.
- D-1/D-5 (de los que depende R-1(iii)) los está aplicando W25 en paralelo;
  al momento de escribir esto ya estaban en el árbol de trabajo (no
  commiteados) y se confirmaron funcionando localmente. Si una corrida futura
  parte de un árbol donde esos cambios se revirtieran o aún no hubieran
  aterrizado, R-1(iii) degrada automáticamente a un hueco declarado (por
  diseño, ver arriba) en vez de romper la corrida o reportar `passed` sin
  base.
- R-2/R-7 en el escenario (d): la dirección «escrito por v0.3.0» de M2/M3
  queda como hueco declarado permanentemente, no como algo por resolver en
  una próxima iteración — ningún subcomando CLI de ninguna versión puede
  producir esos artefactos; solo una sesión MCP viva puede. El disposition ya
  acepta esto como resultado final para R-7 («declararlo como hueco»); para
  R-2 en (d) específicamente se interpretó la misma salida, ya que la acción
  del disposition es simétrica («si no, reducir lo que afirman el recibo»).

## Open issues

Ninguno para este alcance. Sin commit.
