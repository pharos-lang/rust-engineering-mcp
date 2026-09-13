# W12 — admitir la sonda `rustc --print` por lotes en el allowlist de guest (ADR-084 §7)

| Campo | Valor |
| --- | --- |
| Modelo | Claude Opus 5 (`claude -p --model opus --effort high --disallowedTools Agent Task`) |
| CLI | `claude` 2.1.267 |
| Inicio / fin (UTC) | 2026-09-13T03:02:38Z / 2026-09-13T03:07:22Z |
| Resultado | `subtype: success`, `is_error: False` |
| Origen | El gate `full` (2ª pasada) falló en `m6-runtime` grupo 3: el corte `m6-03-guest-programs` observó una sonda `rustc --print` por lotes de RA fuera del allowlist. Flake de muestreo (recibo `7f935ac1` no la muestreó), no regresión. |

---

## W12: la sonda `rustc --print` por lotes de rust-analyzer ya está admitida en el allowlist del guest

**Resultado:** hecho. fmt, check, clippy `-D warnings`, los unit tests y `check-architecture.py` pasan. No corrí Docker, así que el corte nativo `m6-03-guest-programs` sigue sin re-correr: te toca con `m6-runtime`. No hice commit.

### Archivos cambiados
- `crates/execution-adapter/src/analyzer_native.rs`: el brazo `rustc` ahora llama a un helper nuevo, más la nota fechada en el corte m6-03 y los tests. Los brazos `rust-analyzer` y `cargo` no cambian.
- `docs/adr/ADR-084-rust-analyzer-runtime-and-lsp-lifecycle.md`: una "Enmienda 2026-09-12 (calibración W12)" al final de §7.

### La regla implementada
`rustc_is_readonly_probe(arguments: &[&str]) -> bool` admite el argv si:
1. es exactamente `["-vV"]`, **o**
2. cada token pasa un parse de uno en uno contra un vocabulario cerrado, y hay al menos un `--print`:
   - **Tokens sueltos:** `-`, `-vV`, `-O`, `-Wwarnings`.
   - **`-Z`:** solo si le sigue `unstable-options`.
   - **`--crate-name`, `--crate-type`, `--target`:** consumen su valor, que tiene que ser una palabra simple (`plain_word`: no vacía, sin `-` inicial, solo `[A-Za-z0-9_-]`).
   - **`--print <kind>` y `--print=<kind>`:** el `kind` tiene que estar en `STOPPING_PRINT_KINDS`, que es `cfg`, `crate-name`, `file-names`, `split-debuginfo`, `sysroot`, `target-spec-json`.
   - Cualquier otro token hace que se rechace.

Las cuatro formas que ya estaban admitidas lo siguen estando.

**Tres puntos donde fui más estricto que el brief (a propósito):**
- **Tipos de `--print` en lista cerrada.** rustc no se para después de `--print native-static-libs` ni de `--print link-args`: esos los imprime al enlazar, así que antes compila y escribe. Con "cualquier palabra" esa sonda habría pasado y compilado.
- **Valores como palabra simple.** Así se rechaza la forma `--print KIND=PATH` (p. ej. `--print cfg=/tmp/x`), que escribe la respuesta en un archivo. También se rechazan `--target foo.json` (ruta relativa) y un valor que sea un flag (`--crate-name -o`).
- **`-Z` va emparejado con `unstable-options`.** Si no, un `unstable-options` suelto contaría como vocabulario, cuando rustc lo leería como archivo de entrada.

### Tests
Todo va en `the_guest_argv_allowlist_admits_the_expected_forms_and_nothing_else`.
- **Admitted (+2):** la sonda por lotes exacta y una variante que mezcla la forma con `=` y la forma con espacio.
- **Refused (+19):** los refused anteriores se mantienen todos, incluido `rustc --crate-name fixture /source/src/lib.rs`. Los nuevos son:
  - la sonda con `-o /tmp/x`, `--emit=obj`, `--out-dir /tmp`, `-L /source`, `--extern foo=/x` o `-C link-arg=…`;
  - la sonda con `/source/src/lib.rs` en lugar de `-`;
  - `rustc --print cfg /source/src/lib.rs` y `--target /source/hostile.json`;
  - `--crate-name -o`, `-Z unpretty=expanded`;
  - `--print=cfg=/tmp/x` y `--print cfg=/tmp/x`;
  - `native-static-libs` y `link-args` (este también mezclado con `sysroot`);
  - `rustc - --crate-name ___ --crate-type bin` sin `--print`, que compilaría el stdin;
  - `rustc --print` y `rustc --print=` sin valor.

```
test analyzer_native::unit::the_guest_argv_allowlist_admits_the_expected_forms_and_nothing_else ... ok
test result: ok. 5 passed; 0 failed; 12 ignored; 0 measured; 502 filtered out
```
Clippy terminó sin warnings, `check-architecture.py` pasó y fmt no reportó diferencias.

### Por qué lo peligroso sigue rechazado
- **Nada compila.** Hace falta al menos un `--print` y todos los tipos admitidos hacen que rustc salga después de imprimir. Los dos que siguen compilando están fuera.
- **Nada escribe.** No hay `-o`, `--out-dir`, `--emit` ni `-C…` en el vocabulario, y un valor no puede llevar `=` ni `/`, así que `KIND=PATH` no pasa.
- **No se lee nada del proyecto.** La única entrada posible es `-`. Una ruta no es vocabulario ni puede ser valor, y `-L`/`--extern` no están.
- **No se cuela nada por un valor.** Cada flag consume su valor en el parse, y un valor no puede empezar por `-`.

### Riesgos
- **Si rust-analyzer pide otro tipo de `--print`** (p. ej. `target-libdir`), el corte fallará de forma visible y habrá que añadirlo a la lista con evidencia. Es el comportamiento que queremos para un allowlist, pero es un posible fallo futuro.
- **`--crate-type` acepta cualquier palabra simple.** Como no hay compilación, no tiene efecto; no lo cerré a una lista.
- **Solo hay tests booleanos.** Comprueban si se admite o no, pero no por qué token se rechaza cada caso.

### Pendiente
- Que el orquestador re-corra `m6-runtime` para confirmar que `m6-03-guest-programs` pasa de forma determinista.
- Si ese recibo muestra una sonda `rustc` con otro token o tipo de print, habrá que ampliar la regla aquí y en la enmienda de §7.
