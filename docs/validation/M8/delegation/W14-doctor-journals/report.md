# W14 — M8-03: preflight pasivo `doctor.mutation_journals` + fixture de permisos revocados

Worker: Claude Sonnet 5 (`claude -p --model sonnet --effort high`), sin subagentes, sin commit.

## Task

Añadir a `doctor` una sección aditiva `mutation_journals` que, cuando el tuple
`--docker/--docker-socket/--state-root/--rust-image` está configurado, resume el
estado de los journals M2 bajo `--state-root` (pendientes/terminales/formato
desconocido, `downgrade_blocked`) con la misma lectura que `mutation list`, sin
tocar workspace ni source. Cubrir con tests el store vacío, un journal pendiente
y un `operation_kind` desconocido. Añadir un fixture nativo M2 de "permisos
revocados a mitad de operación" (D12 §8).

## Result

Implementado. `doctor` gana `mutation_journals` (aditivo, `format_version` sigue
en `1`). Se añadieron 3 tests nuevos en `crates/mcp-server/tests/doctor.rs`
(store vacío/`null`, `operation_kind` desconocido) y 1 test nuevo en
`crates/project-adapter/tests/support/native_mutation.rs` (permisos revocados
mid-operation, que también cubre y valida en profundidad el caso "pendiente").

**Bloqueo importante:** en esta sesión, cualquier invocación de `cargo`/`rustc`
(incluido `cargo --version`) fue rechazada por el sistema de permisos antes de
ejecutarse ("This command requires approval"), sin posibilidad de que un humano
la aprobara de forma interactiva. No pude ejecutar `cargo fmt`, `cargo clippy` ni
`cargo test` — ver "Verificación" abajo. El código fue escrito y revisado a mano
con mucho cuidado (tipos, imports, visibilidad cruzada de crates, formato de
JSON, límites de línea de `rustfmt`), pero **no ha compilado ni corrido en esta
sesión**. Recomiendo tratar este cambio como no verificado hasta que alguien con
permisos de ejecución corra los comandos de la sección "Verificación".

## Files changed

- `crates/mcp-server/src/doctor.rs`: nuevo struct `MutationJournalsReport`,
  función `mutation_journals(state_root)`, campo `Report::mutation_journals`,
  wiring en `inspect()`, línea humana en `Report::human()`.
- `crates/mcp-server/tests/doctor.rs`: `Continue`/`journal_args` helpers, 2 tests
  nuevos.
- `crates/mcp-server/tests/snapshots/doctor-report.json`: añadido
  `"mutation_journals": null` (único snapshot tocado).
- `crates/project-adapter/tests/support/native_mutation.rs`: 1 test nuevo
  (`revoked_destination_permissions_leave_a_recoverable_non_terminal_journal`).
- `docs/tools.md`: sección "CLI y doctor M1-14" documenta `mutation_journals`
  (aditivo desde 0.8.0, D12 §3; semántica de `unknown_format` como mínimo
  garantizado, no conteo exacto).

## Diseño de `mutation_journals`

Sin el tuple `--docker/--docker-socket/--state-root/--rust-image`:
`"mutation_journals": null` (coherente con `catalog`/`runtime`, que también son
`Option<T>` serializados como `null`).

Con el tuple, `mutation_journals()`:
1. Si `state-root/rust-mcp-mutations-v1` no es un directorio (nunca usado):
   `{pending:0, terminal:0, unknown_format:0, downgrade_blocked:false, notes:[]}`.
   Solo se hace un `Path::is_dir()` (metadato), nunca se crea el directorio
   (doctor sigue siendo pasivo).
2. Si el directorio existe, se llama al mismo API público que usa
   `mutation_cli.rs` (`NativeMutationStore::open(&journal_dir, &[]).list_records()`):
   - `Ok(records)`: se clasifica cada `MutationRecordSummary.state` — `pending`
     cuenta `RecoveryRequired`, `terminal` cuenta `Committed|NoChange|Aborted`.
     `downgrade_blocked = pending > 0`. `notes[]` explica que un binario
     anterior no interpreta `analyzer_action_apply` (y kinds nuevos futuros).
   - `Err(RecoveryRequired)`: el sniff de formato (`decode_envelope`) falla
     **todo el scan** antes de clasificar por registro (confirmado leyendo
     `scan_store`/`decode_envelope` en
     `crates/project-adapter/src/filesystem/macos/mutation.rs`) — por eso
     `unknown_format` es `1` como **mínimo garantizado, no un conteo exacto**;
     lo documento explícitamente en el código y en `docs/tools.md`.
     `downgrade_blocked: true`.
   - Cualquier otro `Err` (permisos, cuota, etc.): conservador,
     `downgrade_blocked: true`, sin inventar pending/terminal.

No se añadió ningún `Check`/`Id` nuevo al array `checks[]` (eso habría
participado en el cálculo de `status`/exit code, que el plan no pide tocar);
`mutation_journals` es un campo hermano de `catalog`/`runtime`, y la salida
humana gana **una línea** resumen, tal como pide la tarea.

## Hallazgo de arquitectura (por qué no hay un test "pending" dentro de
`tests/doctor.rs` usando solo el API público)

Rastreé a mano `commit_checked`/`recover_locked_checked` en
`crates/project-adapter/src/filesystem/macos/mutation.rs` (archivo fuera de los
permitidos, solo lectura). Hallazgo: cuando un fallo ocurre **antes** de que
cualquier archivo del workspace haya sido efectivamente intercambiado
(`published_files == 0`), el store se autorrepara de forma determinista hacia
un estado terminal (`Aborted`/`NoChange`) — por diseño, para no dejar basura
recuperable cuando no hubo efecto visible. Solo cuando el fallo ocurre
**después** de que al menos un archivo ya fue publicado (`published_files > 0`,
p. ej. justo tras el checkpoint `Published`) el store se niega a autosanar (los
archivos ya son visibles) y dejar el journal en fase no terminal
(`Published`/`Applying`/etc., que `mutation_state()` mapea a
`MutationState::RecoveryRequired`) es la única opción segura.

Ese punto de interrupción solo es alcanzable de forma determinista inyectando
un checkpoint (`commit_checked(..., |phase| ...)`), que es una función privada
de `project-adapter`, alcanzable solo desde su propio árbol de tests (como ya
hacen `native_mutation.rs` y `mutation_store.rs`). El API **público**
(`NativeMutationStore::commit()`) no expone ningún punto de interrupción
determinista; forzar el mismo resultado por timing real (matar un proceso
`serve` en el momento justo) exigiría un test-hook nuevo en
`crates/mcp-server/src/stdio/mutation.rs`, que no está en la lista de archivos
permitidos para W14, o aceptar una carrera de tiempos no determinista (que este
repo evita consistentemente: todos sus propios tests de crash usan
sincronización explícita, nunca timing puro).

**Resolución adoptada:** el test nuevo en `native_mutation.rs`
(`revoked_destination_permissions_leave_a_recoverable_non_terminal_journal`)
reproduce el caso con permisos reales (no un error inyectado artificialmente)
revocados justo después de `Published`, y verifica con el **mismo API público**
que consume `doctor` (`NativeMutationStore::list_records()`) que el registro
queda en `MutationState::RecoveryRequired` — exactamente el dato que
`mutation_journals()` clasifica como `pending`. Esto valida la rama "pending"
de `doctor` por construcción (mismo `match` sobre `MutationState`, ya ejercido
por el test de store vacío y el de formato desconocido) sin necesitar un
segundo binario ni un test-hook nuevo fuera del alcance permitido.

Si se requiere estrictamente un test que ejercite el bucket "pending" **dentro
de** `crates/mcp-server/tests/doctor.rs` invocando el binario compilado,
recomiendo una de estas dos vías en un paquete de seguimiento:
(a) exponer, detrás de un feature `test-hooks` en `project-adapter` (nuevo,
requiere tocar `Cargo.toml`), un envoltorio público mínimo de
`commit_checked` con inyección de checkpoint; o (b) añadir un test-hook por
variable de entorno en `stdio/mutation.rs` (patrón ya usado en `stdio.rs` con
`RUST_MCP_TEST_TASK_DELAY_PHASE`) para pausar de forma determinista en un punto
del commit y permitir un `SIGKILL` sincronizado desde el test. Ambas tocan
archivos fuera de la lista permitida de W14.

## Fixture "permisos revocados a mitad de operación" (D12 §8)

`revoked_destination_permissions_leave_a_recoverable_non_terminal_journal` en
`crates/project-adapter/tests/support/native_mutation.rs`:

1. Comete una mutación de un archivo (`Cargo.toml`) con permisos normales hasta
   el checkpoint `Published` (el swap ya es durable: el archivo del workspace
   tiene los bytes "after" completos, nunca parciales).
2. En ese checkpoint, revoca permisos de escritura del directorio del proyecto
   (`chmod 0o500`) — el mismo directorio contiene el clon temporal huérfano.
3. `commit_checked` devuelve `Err(RecoveryRequired)`: no puede borrar el clon
   temporal ni persistir la transición final a `Committed` (ambas necesitan
   escritura, ahora revocada), y por `published_files > 0` no se autorrepara.
4. Verifica: bytes del archivo siguen siendo exactamente los "after" (sin bytes
   parciales); `list_records()` (API público) reporta
   `MutationState::RecoveryRequired` (journal no terminal, recuperable); una
   recuperación explícita (`store.recover(...)`) mientras los permisos siguen
   revocados devuelve `Err(PermissionDenied)` **directamente** (la causa real
   se observa sin enmascarar, porque la recuperación desde fase `Published`
   propaga el error de `cleanup_temp` sin pasar por el camino de autosanado).
5. Restaura permisos (`chmod 0o700`) y confirma que una recuperación explícita
   posterior completa correctamente a `MutationState::Committed`, sin dejar el
   clon temporal.

No usé Docker; es un test nativo `#[cfg(target_os = "macos")]` (heredado del
archivo), sin `#[ignore]`, filesystem puro.

## Tests

En `crates/mcp-server/tests/doctor.rs`:
- `mutation_journals_is_null_without_state_root_and_empty_with_no_journals`:
  (a) sin flags → `mutation_journals: null`; (b) directorio de journals vacío →
  `{pending:0, terminal:0, unknown_format:0, downgrade_blocked:false, notes:[]}`.
- `mutation_journals_reports_an_unrecognized_kind_as_unknown_format_without_panicking`:
  comete un journal real vía el API público (`SecureProjects` +
  `NativeMutationStore::commit`), luego reemplaza en los bytes serializados
  `"operation":"manifest_patch"` por un valor inexistente (rompe el checksum
  del envelope a propósito, igual que el test nativo
  `unknown_journal_format_never_cleans_or_changes_source`), y confirma que
  `doctor` responde `unknown_format >= 1` y `downgrade_blocked: true` sin
  pánico ni error de proceso.

En `crates/project-adapter/tests/support/native_mutation.rs`:
- `revoked_destination_permissions_leave_a_recoverable_non_terminal_journal`
  (arriba).

## Verificación (foreground) — NO EJECUTADA, bloqueada por permisos

```text
cargo fmt --all -- --check
cargo clippy -p rust-engineering-mcp -p rust-engineering-project --all-targets --locked --offline -- -D warnings
cargo test -p rust-engineering-mcp --locked --offline --test doctor
cargo test -p rust-engineering-project --locked --offline revoked_destination_permissions_leave_a_recoverable_non_terminal_journal
git status --short crates/mcp-server/tests/snapshots
```

Ninguno de estos comandos pudo ejecutarse: toda invocación de `cargo`/`rustc`
en esta sesión (incluido `cargo --version`, sin argumentos de build) fue
denegada por el sistema de permisos antes de correr, y no había forma de que
un humano la aprobara de forma interactiva. Confirmé manualmente en su lugar:
firmas y visibilidad pública de cada tipo/función cruzado entre crates
(`ProjectLease`, `SecureProjects`, `ProjectBackend`/`ProjectSourceBackend`,
`NativeMutationStore`, `mutation_digest`, `MutationCandidate`/`MutationCommit`/
`MutationId`/`IdempotencyKey`/`SourceFile`/`SourceBundle`), el layout exacto
del JSON serializado por `encode()` (`serde_json::to_vec`, compacto, sin
espacios) para que el reemplazo de texto en el test de `unknown_format` calce
byte a byte, y el camino de código completo de `commit_checked`/
`recover_locked_checked` para el test de permisos revocados. `cargo fmt` no
pudo confirmarse; ajusté a mano el único `format!` de más de 100 columnas que
no era un literal de cadena aislado (`Report::human()`), pero el resto del
código no ha pasado por `rustfmt` real.

## Risks / Open issues

- **Sin compilar ni correr**: todo lo anterior es revisión manual rigurosa del
  código fuente, no evidencia de ejecución. Antes de mergear, alguien con
  permisos de `cargo` debe correr la sección "Verificación" completa.
- **`unknown_format` es un mínimo, no un conteo exacto** (ver "Diseño"):
  documentado en código, `docs/tools.md` y aquí; si un futuro consumidor asume
  que es preciso, sería una regresión de expectativas, no de este cambio.
- **Bucket "pending" sin ejercicio directo desde `tests/doctor.rs`** invocando
  el binario compilado (ver "Hallazgo de arquitectura"); mitigado validando el
  mismo dato (`MutationState::RecoveryRequired` vía `list_records()` público)
  desde `native_mutation.rs`, pero es una cobertura indirecta, no una
  invocación real de `doctor --json --state-root ...` sobre un journal
  pendiente. Recomiendo una revisión V03 explícita de este punto.
- `docs/tools.md`: el párrafo nuevo sigue el estilo denso existente del
  archivo; no se validó con ningún linter de docs (no existe uno declarado en
  el repo para este archivo).
