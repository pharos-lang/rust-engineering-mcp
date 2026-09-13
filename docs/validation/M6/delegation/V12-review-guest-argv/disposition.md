# V12 — revisión de seguridad del allowlist de argv del guest (W12)

| Campo | Valor |
| --- | --- |
| Modelo | Claude Opus 5 (`claude -p --model opus --effort high`, read-only: Bash/Edit/Write prohibidos, `--permission-mode plan`) |
| CLI | `claude` 2.1.267 |
| Inicio / fin (UTC) | 2026-09-13T03:10:33Z / 2026-09-13T03:14:04Z |
| Veredicto | **Approve-with-P3** — 0 P0 / 0 P1 / 0 P2 / 2 P3 |

## Disposición del orquestador

- **P3-1 (`--target <palabra>` puede leer `<palabra>.json`)**: **corregido** en W12b — `--target` se cierra al único triple del guest M6 `aarch64-unknown-linux-gnu`, lo que hace verdadera la promesa "no lee una ruta del proyecto" de la enmienda ADR-084 §7. Estrictamente más conservador.
- **P3-2 (el oráculo no ve los límites de cada argumento: `ps`/`docker top` une el argv con espacios)**: **documentado** como limitación conocida del oráculo de muestreo m6-03 (no corregible barato desde fuera del guest; `docker top` es el mecanismo). Deuda trazada M6-06. El argv lo genera rust-analyzer, no el peer; el montaje es read-only; sin `--extern` no corren proc-macros.

Sin P0/P1/P2 → no bloquea. El corte m6-03 quedó como aserción de allowlist cerrado sobre el argv completo, estrictamente igual o más conservador que antes salvo la sonda `--print` de solo lectura.

---

## Informe literal del revisor

# V12: veredicto de la revisión read-only del allowlist de argv del guest (W12)

**Veredicto: Approve-with-P3.** No encontré ningún argv de `rustc` que el helper admita y que compile, escriba un archivo o se lleve una ruta o un flag dentro de un valor. Hay **0 P0, 0 P1, 0 P2 y 2 P3**.

**Salvedad antes de nada:** no tenía Bash en esta sesión. No pude correr `git diff` ni sacar hashes, así que no comparé el código contra la versión anterior byte a byte. Los puntos (b) y (c) están revisados sobre el código actual, contra ADR-084 §7 y el reporte de W12.

## Qué revisé

- **Modelo:** Claude Opus 5 (`claude-opus-5`), effort high, solo lectura.
- **Archivos** (hashes no disponibles; base HEAD `f01d1a6` más el working tree modificado):
  - `crates/execution-adapter/src/analyzer_native.rs`: `admitted_argv` (L62), `rustc_is_readonly_probe` (L106), `STOPPING_PRINT_KINDS` (L143), `plain_word` (L152), `admitted_metadata` (L171), `ProgramObserver` (L369), el corte m6-03 (L1389) y el unit test (L2284).
  - `crates/execution-adapter/src/rust_gateway.rs:1586`: `container_top` usa `-eo pid,ppid,args`.
  - `docs/adr/ADR-084-rust-analyzer-runtime-and-lsp-lifecycle.md`: §7 y la enmienda W12.
  - `docs/validation/M6/delegation/W12-guest-argv-print-probe/`: `report.md` y `prompt-header.md`.

## Análisis token a token

- **Tokens sueltos (`-`, `-vV`, `-O`, `-Wwarnings`):** ninguno escribe ni añade una entrada distinta de stdin. `-vV` imprime la versión y sale.
- **`-Z`:** exige que el siguiente token sea literalmente `unstable-options`. `-Z unpretty=…` queda rechazado, y la forma pegada `-Zunstable-options` también, que es lo conservador.
- **`--crate-name`, `--crate-type`, `--target`:** consumen su valor y `plain_word` lo limita a `[A-Za-z0-9_-]`, sin `-` inicial. Eso descarta `/`, `.`, `=` y `@`, así que no pasan rutas, `.json`, `KIND=PATH`, argfiles `@file` ni un flag disfrazado de valor. Las formas `--target=…` no están en el vocabulario y se rechazan.
- **`--print <k>` y `--print=<k>`:** comprueban que `k` esté exactamente en la lista cerrada. `cfg=/tmp/x` no está, ni vacío ni ausente.
- **`STOPPING_PRINT_KINDS`:** rustc deja de compilar si hay algún `--print` distinto de `native-static-libs` o `link-args`; esos dos quedan fuera de la lista. Los seis admitidos no enlazan ni escriben. `crate-name` y `file-names` solo parsean los atributos de la entrada `-` (stdin).
- **Al menos un `--print`:** sin él, `rustc -` compilaría stdin, y el caso está cubierto. `rustc` sin argumentos y `rustc --print` se rechazan.
- **Tokens desconocidos:** `-o`, `--emit`, `--out-dir`, `-L`, `--extern`, `-C…` o una ruta dan `return false`. Ningún flag admitido consume un token que luego salte una comprobación, porque el valor consumido se valida siempre.

## Findings

### P3-1: `--target <palabra simple>` no es del todo "no una ruta" (PLAUSIBLE, conviene verificarlo)
- **Argv que pasa:** `/opt/rust/bin/rustc --target hostile --print cfg`
- **Por qué importa:** si el triple no es interno, rustc busca `hostile.json` en `RUST_TARGET_PATH`. Si esa variable no está definida, la ruta vacía hace que se busque relativo al CWD, que para las sondas de rust-analyzer suele ser `/source`. También busca en `<sysroot>/lib/rustlib/hostile/target.json`.
- **Efecto:** se leería y parsearía un JSON del proyecto, sin compilar ni escribir. En versiones recientes rustc exige `-Z unstable-options` para specs JSON, y el vocabulario lo admite.
- **Por qué no es P1:** rust-analyzer ya lee todo `/source` y el montaje es de solo lectura. El problema es que el comentario del helper y la enmienda del ADR prometen algo que no se cumple del todo.
- **Arreglo propuesto:** cerrar `--target` a los triples observados (p.ej. `aarch64-unknown-linux-gnu`) o corregir el texto.

### P3-2: el oráculo no ve dónde empieza y termina cada argumento, y admitir `-` suelto abre un caso nuevo
- **Origen:** `ProgramObserver` recibe `ps args`, que une el argv con espacios (L422), y `admitted_argv` lo vuelve a partir con `split_whitespace`.
- **Argv real que pasaría:** `["/opt/rust/bin/rustc", "- --print=cfg"]`, un único archivo de entrada con ese nombre. Tras partirlo queda `-` `--print=cfg`, que se admite, pero rustc **compilaría** ese archivo.
- **Por qué antes no pasaba:** las cuatro formas fijas no tenían entrada suelta. Cualquier argumento que al partirse las imitara era para rustc una opción o un `--print` desconocido, y rustc salía con error.
- **Por qué no es P1:**
  - rust-analyzer no genera ese argv.
  - Haría falta un archivo con ese nombre en el CWD.
  - Sin `--extern` no se ejecutan proc-macros.
  - `/source` es de solo lectura.
  - Esto es un oráculo de calibración que muestrea, no un control en tiempo de ejecución.
- **Arreglo propuesto:** leer `/proc/<pid>/cmdline`, que separa con NUL, o documentar la limitación junto a la nota de muestreo.

## Verificaciones (a) a (d)

- **(a) Las cuatro formas previas siguen admitidas:** sí. `-vV` pasa por el retorno temprano, y `--print sysroot`, `--print cfg -O` y `-Z unstable-options --print target-spec-json` pasan el parse. Están en el test L2287–2290.
- **(b) Los refused previos siguen refused:** sí para todos los que conozco. `rustc --crate-name fixture /source/src/lib.rs` se rechaza porque `/source/src/lib.rs` no es vocabulario, y `/opt/rust/bin/rustc` porque no tiene `--print`. Además los 19 casos nuevos son rechazos correctos. Sin el diff no puedo demostrar que no se borrara ninguna entrada antigua.
- **(c) Brazos `rust-analyzer` y `cargo`:** el código actual coincide con §7 y con el reporte, que dice que no cambiaron, pero no lo comparé byte a byte. Aparte, y no cuenta como finding de W12: `["rustc", "-Z", "unstable-options", "--print", ..]` ya admitía cualquier cola en el brazo `cargo`.
- **(d) El corte m6-03:** sigue siendo `argv.iter().filter(|c| !admitted_argv(c))` sobre el argv completo, contra la lista cerrada (L1450–1459). No se debilitó a una comprobación de basename.

**Conclusión:** el brazo `rustc` es un superconjunto estricto de las cuatro formas anteriores, y lo único añadido es la familia de sondas `--print` de solo lectura. No se admite nada que compile o escriba. Solo quedan las dos matizaciones P3: `--target` puede leer un `<palabra>.json` y el oráculo no ve los límites de cada argumento.
