# W34 — `mutation list --state-root <nunca usado>` lista vacío, no `blocked/io`

Worker: Claude Sonnet 5 (`claude -p --model sonnet --effort high`), sin subagentes, sin commit, sin Docker.

## Diagnóstico

`crates/mcp-server/src/mutation_cli.rs::execute` llamaba directamente a
`NativeMutationStore::open(&state_root.join("rust-mcp-mutations-v1"), &[])`
para `list` y `prune`. En macOS, `StateRoot::open`
(`crates/project-adapter/src/filesystem/macos/mutation.rs:325-354`) abre ese
directorio con `openat(...)` sin crearlo; si no existe, `openat` falla con
`ENOENT`, y `mutation_io` (línea 41-53 del mismo archivo) solo distingue
`WOULDBLOCK`/`LOOP`/`ACCESS`/`PERM`/`ENOTCAPABLE` — todo lo demás, incluido
`ENOENT`, cae al catch-all `MutationError::Io`. De ahí el `status: "blocked",
error_code: "io"` del hallazgo F-2 sobre un directorio que nunca tuvo una
mutación.

`doctor` no tiene este problema porque `mutation_journals`
(`crates/mcp-server/src/doctor.rs:257-261`) comprueba `!journal_dir.is_dir()`
**antes** de llamar a `NativeMutationStore::open` y devuelve un reporte vacío
sin abrir nada — por eso D-5 documenta ambos caminos como «la misma lectura
mínima», pero solo `doctor` tenía el guard. `mutation_cli.rs` nunca lo tuvo.

No hizo falta tocar `crates/project-adapter/src/filesystem/macos/mutation.rs`:
el store no necesitó un nuevo mecanismo para distinguir «no inicializado»,
porque la comprobación se puede hacer con seguridad *antes* de invocar
`NativeMutationStore::open` (igual que hace `doctor`), sin relajar ninguna
verificación de seguridad del store en sí (esas siguen intactas para
cualquier store que sí exista).

## Cambio

**`crates/mcp-server/src/mutation_cli.rs`**

- Nueva función `journal_dir_exists(state_root, journal_dir)` que hace un
  `std::fs::metadata` puro (sin crear nada) y distingue tres casos por el
  `io::ErrorKind` real, no por un simple `is_dir()` booleano (que confundiría
  «no existe» con «no se puede leer»):
  - el directorio de journals existe → `Ok(true)`, sigue el camino normal.
  - el directorio de journals no existe (`NotFound`) pero `state_root` sí
    es un directorio real → `Ok(false)`: store nunca inicializado.
  - el directorio de journals no existe y `state_root` tampoco (o cualquier
    otro fallo al comprobarlo) → `Err(NotFound)`: ruta inexistente, sigue
    siendo un error.
  - cualquier otro fallo de metadata (p. ej. `state_root` sin permiso de
    ejecución/búsqueda, así que ni siquiera se puede determinar si el hijo
    existe) → `Err(Io)`.
- `execute` solo aplica este guard a `Action::List` (antes de abrir el
  store); `prune` no cambia — sigue exigiendo un store existente, coherente
  con que solo tiene sentido podar un registro concreto de un store real.
- Cuando el guard detecta «nunca inicializado», `execute` devuelve
  `(vec![], Some(false))` sin tocar el filesystem; en cualquier otro éxito de
  `list` devuelve `Some(true)`; `prune` sigue devolviendo `None` (el campo no
  es informativo para esa acción).
- `Report` gana dos campos aditivos: `store_initialized: Option<bool>` (solo
  relevante para `list`; `null` para `prune` y para cualquier error) y
  `count: u64` (`records.len()`, siempre presente).
- El mensaje de `list` sobre un store vacío pasa a «No mutation journal store
  exists yet at this state root» (en vez del genérico de «recovery»). El
  mensaje de error para `MutationError::NotFound` (que ahora también cubre
  «`state_root` no existe en absoluto», además del uso previo «registro no
  encontrado» en `prune`) pasa a «The state root does not exist or the
  requested record was not found» — más preciso que el texto anterior de
  «interrupted operations», que era engañoso para este caso.

**`crates/mcp-server/tests/cli.rs`** — tres tests nuevos:

- `mutation_list_on_a_never_initialized_state_root_is_empty`: `--state-root`
  a un directorio real recién creado (sin `rust-mcp-mutations-v1`) → exit 0,
  `status: "passed"`, `records: []`, `count: 0`, `store_initialized: false`.
- `mutation_list_on_a_nonexistent_state_root_is_an_error`: `--state-root` a
  una ruta que no existe en absoluto → exit ≠ 0, `status: "blocked"`,
  `error_code: "not_found"`.
- `mutation_list_on_an_unreadable_state_root_is_an_io_error` (`#[cfg(unix)]`):
  `--state-root` a un directorio real con `chmod 000` (sin permiso de
  búsqueda, así que ni `state_root` ni el hijo son legibles) → exit ≠ 0,
  `status: "blocked"`, `error_code: "io"`. Restaura permisos antes de limpiar
  el directorio, incluso si la aserción de exit code falla.

**`docs/tools.md`** — dos frases añadidas junto a la descripción existente de
`mutation_journals`/`mutation list`:
1. Junto al párrafo M1-14 de `doctor`, una aclaración de que `doctor` trata
   una `--state-root` inexistente como vacía mientras `mutation list` la
   rechaza con `not_found` — la única divergencia real entre ambos caminos,
   para que la frase «misma lectura mínima» no quede engañosa.
2. Junto a la sección de la CLI `mutation list/prune`, el contrato nuevo:
   store nunca inicializado → `passed`/`records: []`/`count: 0`/
   `store_initialized: false` sin crear el directorio; ruta inexistente →
   sigue siendo error.

## Por qué la ruta inexistente sigue siendo un error (decisión explícita, no bug)

Por diseño del orquestador: `state_root` que existe pero nunca tuvo actividad
(caso F-2 real, dos directorios probados por el tercero) es indistinguible de
«store legítimamente vacío» y debe listarse como tal. Una `--state-root` que
no existe en absoluto es casi siempre un error de operador (ruta mal
escrita, montaje no disponible) y perder esa señal sería peor que el propio
F-2. `doctor` sí colapsa ambos casos en «vacío» (no se tocó, fuera de la
lista de archivos permitidos), así que queda una divergencia menor entre
`doctor` y `mutation list` para ese único sub-caso; documentada en
`docs/tools.md` en vez de ocultada.

## Verificación

- `cargo fmt --all -- --check` → limpio (tras corregir dos `match` arms que
  `rustfmt` prefiere en bloque por longitud de línea).
- `cargo clippy -p rust-engineering-mcp -p rust-engineering-project --all-targets --locked --offline -- -D warnings` → sin warnings.
- `cargo test -p rust-engineering-mcp --locked --offline --test cli --test doctor` → 23/23 (`cli`, 20 preexistentes + 3 nuevos) y 10/10 (`doctor`, sin cambios) OK.
- `cargo test -p rust-engineering-mcp --locked --offline --bin rust-engineering-mcp mutation_cli` → 1/1 OK (test unitario preexistente del parser, sin cambios de comportamiento).
- `git status --short crates/mcp-server/tests/snapshots` → sin salida (sin cambios).

## Archivos tocados

`crates/mcp-server/src/mutation_cli.rs`, `crates/mcp-server/tests/cli.rs`,
`docs/tools.md`. No se tocó `crates/project-adapter/src/filesystem/macos/mutation.rs`
(no hizo falta). No se tocó ningún otro archivo fuera de la lista permitida;
el `git status` de partida ya traía cambios de otros workers concurrentes
(`CHANGELOG.md`, workflows, ADRs, `scripts/test-m8-clients*.py`, etc.) que no
toqué. Sin commit.
