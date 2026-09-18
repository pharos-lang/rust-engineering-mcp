# M8-02 — intentos de gate `core` sobre bytes finales

## Intento 1 (2026-09-14 18:2x → 19:57 UTC) — **fallido** en la etapa `test`

Comando: `CARGO_INCREMENTAL=0 CARGO_TERM_COLOR=never python3 -B scripts/gate.py core`
(recibo parcial fuera del árbol, sha256 en la tabla). Etapas `fmt`, `check`,
`clippy` verdes; `test` falla: `crates/mcp-server/tests/protocol.rs`
`closed_stdout_exits_even_when_stdin_remains_open` — 58 passed, 1 failed.
El resto de etapas no se ejecutó.

### Criterio de clasificación (escrito ANTES de reproducir, §4 del encargo)

| Clase | Condición observable | Consecuencia |
| --- | --- | --- |
| **(A) Regresión de M8-02** | El test falla de forma determinista (≥ 2/3) sobre el árbol actual con `cargo test -p rust-engineering-mcp --test protocol --locked --offline closed_stdout_exits_even_when_stdin_remains_open`, y su causa está en el diff M8-02 (`stdio.rs`/`main.rs`/`contract_cli.rs`) o pasa sobre `HEAD` (`dbc17f5`, sin los cambios) con el mismo comando | Bloquea: corrección por worker + re-revisión + gate `core` completo de nuevo |
| **(B) Flake de host/temporización** | El test pasa ≥ 3/3 en aislamiento sobre los mismos bytes y el mensaje de fallo es de tiempo/espera (p. ej. el proceso no salió dentro del plazo) o de E/S del host; el diff M8-02 no toca el camino de cierre de stdout | No es regresión: se registra con evidencia (salida y conteos) y se repite el gate `core` completo; un `verde a la segunda` sin este registro no vale |
| **(C) Fallo preexistente** | Falla también sobre `HEAD` sin los cambios de M8-02 con el mismo comando (≥ 2/3) | No es regresión de M8-02; deuda con disposición del owner; el gate no puede cerrarse en verde hasta corregirla o desgatearla por decisión |

Ningún «pass» se declara sin haber corrido; el intento fallido queda registrado.

### Resultado de la clasificación del intento 1 → **(B) flake de host/temporización**

- Mensaje: `Error: "stdout reader (bootstrap=false) after 10.001241875s: Timeout"` —
  el arnés espera hasta `TIMEOUT` (10 s) a que el hilo lector de stdout del
  servidor suelte su handle tras cerrar el socket; en el gate corrían en paralelo
  las suites del workspace y en este host cada binario de test tarda ~30 s en
  arrancar (exec bloqueado por el agente de seguridad; ver nota M5 «poisoned
  artifact»), lo que carga el sistema.
- Reproducción en aislamiento sobre los **mismos bytes**:
  `cargo test -p rust-engineering-mcp --test protocol --locked --offline
  closed_stdout_exits_even_when_stdin_remains_open` → **3/3 `ok`** (1,78 s,
  1,82 s, 1,89 s); suite `protocol` completa una vez → **59/59 `ok`** (13,26 s).
- El diff M8-02 no toca el cierre de stdout ni el hilo lector (`stdio.rs` cambia
  solo el registro estático de definiciones; `main.rs` añade el parseo de
  `contract`).
- Recibo parcial del intento 1 (fuera del árbol): sha256
  `5fdabdcd76bf1172e4d94da5dcbb7630d1e7b0c4335233c5effe7fc1a8618112`
  (`fmt`/`check`/`clippy` passed, `test` failed).
- Consecuencia por el criterio (B): se repite el gate `core` completo (intento 2)
  sobre bytes idénticos; el test **no** se modifica ni se desgatea.

## Intento 2 (2026-09-14 19:59 → 20:29 UTC) — bytes idénticos al intento 1 — **fallido**, mismo test

- Mensaje distinto del mismo test: `Error: "closed stdout (bootstrap=false): server failed to exit before deadline"` (10 s). Etapa `test` 1 787 s (M6: 1 882 s; intento 1: 5 499 s) → el host no estaba lento; **no es solo carga**.
- Recibo parcial (fuera del árbol): sha256 `78522afb9ed2848f…`.

### Diagnóstico adicional (sin tocar el test ni el producto)

| Comando (mismos bytes) | Resultado |
| --- | --- |
| `cargo test -p rust-engineering-mcp --test protocol … closed_stdout_exits_even_when_stdin_remains_open` ×3 | 3/3 `ok` (~1,8 s) |
| `cargo test -p rust-engineering-mcp --test protocol` (59 tests, paralelo) ×1 | 59/59 `ok` (13,3 s) |
| `cargo test --workspace --all-targets --locked --offline -- closed_stdout_exits_even_when_stdin_remains_open` (unificación de features del gate) ×1 | `ok` (2,34 s) |
| Historia del test | Existe desde el snapshot inicial (`6fdd8e6`); pasó en la etapa `test` del gate `full` de M6 (`69a0be14`) con el mismo comando |
| Diff M8-02 en `stdio.rs` | Solo `mod capability_document; mod stability;` y `capability_document::run(json)`; el bucle `serve()` y el cierre de stdout no cambian |

Conclusión provisional: falla únicamente dentro de la etapa `test` completa del gate (2/2) y nunca fuera (5/5). Siguiente discriminación: ejecutar **la etapa `test` exacta del gate** (`cargo test --workspace --all-targets --locked --offline`, sin `check`/`clippy` delante) una vez.

### Discriminación adicional (2026-09-14 20:56 → 21:25 UTC), mismos bytes

| Experimento | Resultado |
| --- | --- |
| Etapa `test` exacta del gate sola (`cargo test --workspace --all-targets --locked --offline`) | **falla** (mismo test, 3.ª vez consecutiva; etapa completa 3 min con build caliente) |
| `cargo test -p rust-engineering-mcp --all-targets` (13 binarios de `mcp-server`, mismo binario de test `protocol-4b35839a`) | pasa |
| `cargo test --workspace --test protocol` ×2 (suite completa, 59 en paralelo, binario unificado) | 59/59 ×2 |
| Etapa exacta con el test instrumentado (volcado de stderr del servidor en fallo; binario de test relinkado) | pasa (el volcado no llegó a ejecutarse) |
| Etapa exacta con el test **original** (parche revertido, `git status` limpio) — E1 | **pasa** (81 binarios) |
| E2 `--exclude rust-engineering-execution`; E3 `--exclude rust-engineering-semantic --exclude rust-engineering-artifact` | pasan |
| Monitor de procesos durante la etapa | 0 servidores huérfanos antes de `protocol`; 13 servidores concurrentes durante la suite (normal) |

Lectura: el fallo se concentró en una ventana (18:2x–21:00 UTC: 3 fallos consecutivos
bajo el comando del gate) y desapareció sobre bytes idénticos (4 pasadas consecutivas
del comando exacto; 10 pasadas en total fuera de esa ventana). Causa raíz no
identificada (candidata: latencia de exec/arranque de procesos en este host, que
ese día bloqueaba ~30 s el primer exec de cada binario); ninguna evidencia apunta al
diff M8-02 (no toca `serve()` ni el cierre de stdout; el mismo binario pasa). Se
mantiene la clase **(B)** y se registra como **riesgo residual** para M8-05/09: el
arnés usa un `TIMEOUT` de 10 s para «cierre de stdout ⇒ salida»; si el flake
reaparece en un RC, requiere decisión del owner (margen del arnés o desgate
justificado), nunca un reintento silencioso.

## Intento 3 (2026-09-14 2026-09-14T21:14:06Z → 2026-09-14T21:18:45Z) — **verde**

Bytes idénticos a los intentos 1 y 2; binario `release` reconstruido en 0.8.0 antes de lanzar (la etapa de smoke lo usa). Recibo [core-gate.json](core-gate.json) `sha256:f2c2fe69d32ef786229de665c127f8f3ee405b04d7203ec984f3416f666617eb`: todas las etapas `passed`, `source_inputs_unchanged: true`; incluye las etapas nuevas `contract-freeze-tests` y `contract-freeze` (manifiesto `freeze-0.8.0.json` verificado).
