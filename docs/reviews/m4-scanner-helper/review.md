## Veredicto

**Alcance revisado:** exclusivamente el paquete congelado `docs/reviews/m4-scanner-helper/inputs/` (7 archivos del inventario `inputs.json`, todos leídos). Sin ejecución, sin subagentes, sin tocar estado vivo del repo.

**Veredicto acotado:** la propiedad central de ADR-069 se sostiene en el código leído — *el supervisor no invoca el parser*. `run_supervisor` (lib.rs:262-301) solo lee el manifiesto, lanza hijos y decodifica JSON tipado; toda llamada a `syn` está en `scan_source`/`run_file_worker`, alcanzables únicamente por la rama `--file-index` (main.rs:19-22). El lanzador cumple literalmente lo exigido: binario absoluto, índice numérico validado, `env_clear` + PATH fijo, sin shell, stdin null, stderr descartado (lib.rs:304-312). El AST se retiene con `mem::forget` en ambas salidas (lib.rs:230, 258).

**No cierra M4-02.** Encontré **0 P0, 1 P1, 8 P2**. El P1 es un defecto real de este código (emisión todo-o-nada sin presupuesto global), no un hueco del gateway pendiente.

---

## P1

### P1-1 — El supervisor no puede producir cobertura parcial bajo el deadline global
`lib.rs:24` (`CHILD_TIMEOUT = 2s`), `lib.rs:262-301`, `lib.rs:303-349`

**Trigger:** un manifiesto con ~60 archivos que agoten sus 2 s (p. ej. `unary-not.rs` de `hostile/generate.py:15` con profundidad alta) consume los 120 s máximos del deadline global. El caso benigno también preocupa: 4096 archivos × 2 s = 8192 s de peor caso, y aun a ~25 ms/archivo (fork+exec+**re-parseo del manifiesto de hasta 1 MiB por hijo**, ver P2-6) 4096 archivos rozan o superan los 120 s.

**Impacto:** el supervisor solo escribe stdout una vez, al final (`emit_and_exit`, main.rs:52-67). Cuando el gateway mata el árbol por deadline global, **se pierde el 100 % de la cobertura ya calculada**, incluidos los archivos que sí parsearon. Esto contradice el espíritu de ADR-069 ("una muerte/señal/parse error/timeout deja cobertura parcial y no elimina resultados de otros archivos"): la propiedad se cumple por hijo pero se rompe en el agregado, y es barata de disparar.

**Corrección:** dar al supervisor su propio presupuesto de pared (campo explícito tipo `deadline_ms` en el manifiesto, con margen frente al deadline del gateway); dejar de lanzar hijos al agotarlo, marcar los restantes con un estado explícito (`skipped`) y emitir el JSON igual. Alternativa complementaria: emisión incremental por archivo. Nota: el manifiesto usa `deny_unknown_fields` (lib.rs:29), así que esto es un cambio de contrato consciente, no un parche silencioso.

---

## P2

### P2-1 — `>1 MiB` y no-UTF-8 colapsan en `unavailable`, indistinguibles de "archivo ausente"
`lib.rs:19`, `lib.rs:209-216`

`read_bounded` devuelve `TooLarge` y `run_file_worker` mapea **cualquier** `Err` a `FileStatus::Unavailable`, igual que un archivo inexistente o ilegible. **Trigger:** bindings FFI generados (bindgen y similares) superan 1 MiB con regularidad y son precisamente los archivos con mayor densidad de `unsafe extern fn`. **Impacto:** el punto ciego de mayor densidad de unsafe se reporta con el mismo estado que "no está", y el host no puede distinguirlo. **Corrección:** estados distintos (`too_large`, `not_utf8`) o al menos contadores separados en el resumen por archivo; documentarlo en ADR y README.

### P2-2 — Regiones `Verbatim` se ignoran sin contador de omisión
`lib.rs:699, 743, 756, 777, 788, 799` (`*::Verbatim => &[]` y los `_ => &[]` adyacentes)

**Trigger:** sintaxis válida que syn 3.0.4 conserve como `Verbatim` (formas nuevas o no modeladas). **Impacto:** `visit_*` no encuentra tokens tipados, no hay hallazgo y **no hay contador**, así que el archivo sale `parsed` con 0 findings; el diseño ya reconoce esta clase de opacidad para macros (`macro_omitted`, lib.rs:617-619) pero no para verbatim. **Corrección:** contador `verbatim_omitted` análogo, propagado al resumen por archivo.

### P2-3 — `unsafe trait` no se reporta
`lib.rs:519-676` (no hay `visit_item_trait`)

**Trigger:** `unsafe trait Foo {}` / `unsafe auto trait`. **Impacto:** cero findings, asimétrico con `unsafe_impl` (lib.rs:646-651), que sí se reporta; `unsafe trait` es la declaración de la obligación que `unsafe impl` cumple. El conjunto cerrado de ADR-069 (línea 42-43) tampoco lo enumera, así que es coherente con el ADR pero el ADR es incompleto respecto a la superficie unsafe del lenguaje. **Corrección:** añadir `unsafe_trait` a `FindingKind`, o declarar la exclusión explícitamente en ADR y README para que "0 findings" no se lea como "sin obligaciones unsafe". Verificar en el mismo pase si syn 3.0.4 modela cualificadores `unsafe`/`safe` en statics foráneos (`unsafe extern "C" { unsafe static S: u8; }`); si los modela, hoy tampoco se reportan.

### P2-4 — `#[cfg_attr(..., unsafe(no_mangle))]` no produce `unsafe_attribute`
`lib.rs:608-615`; comportamiento fijado por el test `lib.rs:878-883` (espera `total_findings == 3`)

`visit_attribute` no desciende a los tokens del atributo (decisión correcta para no interpretar macros), pero eso deja el `unsafe(...)` anidado en `cfg_attr` invisible. **Impacto:** ruta de evasión trivial para quien lea `unsafe_attribute` como el conjunto completo de atributos unsafe. **Corrección:** recorrer solo la cola de `cfg_attr` con `parse_nested_meta` (acotado, sin expansión) marcando `conditional = true`; o documentar el hueco junto a la declaración `macros_expanded=false`.

### P2-5 — La mitad supervisora no tiene ninguna prueba
`lib.rs:804-991` (los 8 tests cubren solo `scan_source` y `validate_manifest`)

Sin cobertura: `supervise_file`, `decode_worker_output`, `valid_worker_output`, `read_stream_bounded`, `run_supervisor`. **Impacto:** la única frontera donde datos cruzan un límite de proceso hacia el supervisor es exactamente la que no tiene regresión; los invariantes de `valid_worker_output` (lib.rs:367-388) son delicados (aritmética de conteos, estados permitidos) y son funciones puras, perfectamente testeables en proceso. **Corrección:** tests directos de `valid_worker_output`/`decode_worker_output` con JSON adversario (estado `crashed`, `file_index` cruzado, `byte_start >= byte_end`, conteos incoherentes) y de `read_stream_bounded` con lectores sintéticos (exactamente 512 KiB, 512 KiB+1, error a mitad). Mantener `HELPER_PATH` constante; no hace falta inyectarlo para cubrir estas cuatro funciones.

### P2-6 — TOCTOU y re-lectura del manifiesto por cada hijo
`lib.rs:201` (worker) y `lib.rs:263` (supervisor)

Supervisor y cada uno de los hasta 4096 workers leen `/security/scan.json` de forma independiente; nada liga la vista del supervisor con la del hijo. **Trigger:** reescritura del manifiesto durante el escaneo. **Impacto:** el binding índice→path deja de ser estable y los findings se atribuyen al archivo equivocado en la proyección del host, sin ninguna señal. Secundario: coste O(n²) (4096 parseos de un JSON de hasta 1 MiB), que alimenta el P1-1. **Corrección:** montar `/security` RO (gateway, pendiente) **y** ligar explícitamente — el supervisor pasa/verifica un digest del manifiesto, o el hijo recibe el path esperado y lo compara con su propia lectura.

### P2-7 — El límite de 2 s solo acota la salida del hijo, no el drenaje
`lib.rs:325`, `lib.rs:342`

`reader.join()` no tiene cota temporal. Si algún descendiente hereda el extremo de escritura del pipe, matar y reapear al hijo **no** cierra stdout y el supervisor queda bloqueado indefinidamente en ese archivo. **Estado hoy:** no explotable — el worker nunca hace fork y el rootfs es RO — es un defecto latente que se activa con cualquier cambio futuro del worker. **Corrección:** join acotado (canal + `recv_timeout` con el hilo lector desacoplado) para que el bound de 2 s cubra spawn→exit→drenaje.

### P2-8 — Contadores del hijo sin cota superior; `OutputTooLarge` descarta todo
`lib.rs:379-387` (no se acotan `omitted_findings` ni `macro_omitted`), `lib.rs:296-299`

`valid_worker_output` solo exige coherencia `total == len(findings) + omitted`, con `omitted` libre en todo el rango `u64`. **Impacto:** conteos inflados ensanchan el JSON del supervisor; el margen real es estrecho — mi cálculo a mano del peor caso con contadores de 7 dígitos da ≈485 KiB sobre 524 288 (~7 % de margen), y el propio test `lib.rs:946-979` asume 7 dígitos. Al superarlo, la respuesta **completa** se descarta con `OutputTooLarge`, repitiendo el fallo de todo-o-nada del P1-1. **Corrección:** validar los contadores contra un máximo plausible (p. ej. `MAX_SOURCE_BYTES`) y degradar antes de fallar — vaciar `findings` y conservar los resúmenes por archivo en lugar de devolver un error fatal.

---

## P3

| # | Ubicación | Detalle |
|---|---|---|
| P3-1 | `lib.rs:328-345` | Carrera en el deadline: un hijo que sale justo tras el último `try_wait` se reporta `timed_out` y se descarta su JSON válido (ventana ≤5 ms). Falla cerrado, pero pierde cobertura. Corrección: un `try_wait` final tras el kill, o consumir `captured` si es válido. |
| P3-2 | `lib.rs:231` vs `lib.rs:370-373` | El worker emite `FileStatus::Crashed` (span inválido), estado que el protocolo prohíbe al worker; el supervisor lo rechaza y produce `Crashed` por otra vía. Correcto hoy **por coincidencia**; el README (líneas 40-66) no aclara el conjunto válido por rol. Usar un estado propio (`internal_inconsistent`) o rechazarlo con un test. |
| P3-3 | `lib.rs:272-274` | El presupuesto de 128 findings lo consumen íntegro los índices más bajos; un archivo ruidoso en índice 0 oculta el resto. Los conteos se preservan, pero no está documentado. |
| P3-4 | `lib.rs:573-582`, sin `visit_pat*` | Portadores de atributos no cubiertos (`PatType`/`FnArg`, `BareFnArg`, `Variadic`, patrones): un finding bajo `#[cfg]` en esos nodos sale `conditional: false`. Falso "incondicional". |
| P3-5 | `lib.rs:296` + `main.rs:53` | Doble serialización de la salida del supervisor (CPU y memoria ×2). |
| P3-6 | `lib.rs:432` | `File::open` sin timeout: un FIFO en `/security/scan.json` bloquea al supervisor indefinidamente. Dependencia no declarada sobre el host. |
| P3-7 | `main.rs:16-18` | Con `argc == 0` el iterador queda vacío y se entra en **modo supervisor**. No alcanzable vía `Command`, pero el dispatch no lo distingue de la invocación legítima. |
| P3-8 | `lib.rs:187-193` | Paths duplicados en el manifiesto se aceptan (solo se valida contigüidad de índices) → doble conteo silencioso. |
| P3-9 | `README.md:19` | "drains at most 512 KiB of stdout" vs. implementación: drena **todo**, retiene ≤512 KiB y luego falla el archivo (`lib.rs:446-468`). El drenaje completo es correcto (evita bloquear al hijo); el texto no lo refleja. |
| P3-10 | `generate.py:34-37` | Descarta en silencio los casos >1 MiB sin mensaje ni código de salida; con `--depth 50000` es probable que `right-associative.rs` no llegue a generarse. Tampoco valida profundidades negativas. |
| P3-11 | `generate.py:12-25` | El corpus hostil no cubre el protocolo del supervisor (JSON malformado del hijo, stdout >512 KiB, hijo que no termina) ni las clases de P2-2/P2-3/P2-4. |
| P3-12 | `Cargo.toml:22-29` | Para una frontera de parser conviene `overflow-checks = true` y clippy `indexing_slicing`/`arithmetic_side_effects` como defensa en profundidad (el código ya usa `saturating_*` y `str::get` de forma consistente). |
| P3-13 | `Cargo.lock:75-109` | ADR-069 solo declara pre-adquiridos syn 3.0.4 y proc-macro2 1.0.107; el lock añade serde 1.0.229, serde_json 1.0.151, **zmij 1.0.23**, memchr 2.8.3, itoa, quote, unicode-ident. Su cobertura en el vendor/caché offline no está establecida por el ADR. Gap de aprovisionamiento, no defecto de código. |

---

## Propiedades que sí verifiqué (positivas, acotadas al paquete)

- **Frontera del parser:** ninguna ruta del supervisor llama a `syn`; el enlazado estático no implica ejecución. Dispatch cerrado en main.rs:17-28 con fallo cerrado a `invalid_arguments`.
- **Formato de cable sin texto libre:** `Finding`/`WorkerOutput`/`FileSummary` no tienen ningún campo `String`; ni diagnósticos del parser, ni paths, ni stderr del hijo (lib.rs:98-131). Cumple ADR-069 líneas 34-36.
- **Sin panics por índices:** `record` usa `self.source.get(range)` y compara contra el keyword esperado (lib.rs:492-499); un rango no alineado a carácter da `None` → `invalid_span`, no panic.
- **JSON hostil:** `serde_json` aplica su límite de recursión (128) en ambos parseos, así que un JSON profundo del hijo o del manifiesto da error, no stack overflow. El visitor del manifiesto corta en 4096 entradas durante la deserialización (lib.rs:56-68), sin materializar el resto.
- **Sin deadlock por pipe lleno:** el lector drena hasta EOF aunque haya rebasado el límite (lib.rs:450-462).
- **Columna por caracteres UTF-8, 1-based**, verificada a mano contra el test CRLF/Unicode (lib.rs:887-898): columna 20 es correcta contando escalares.
- **Cadenas unarias / stack overflow:** el desbordamiento ocurre en el hijo, muere por señal, `status.success()` es falso y el supervisor emite `crashed` y continúa (lib.rs:346-348). El drop recursivo se evita en ambas salidas de `scan_source`.
- **Sanitización de paths** (lib.rs:410-423): prefijos fijos `/source/` y `/rust-mcp-vendor/`, sin `.`/`..`/componentes vacíos/backslash, sufijo `.rs`. Alineado con el snapshot `/source` del gateway existente.

---

## Limitaciones de esta revisión

1. **Sin ejecución**: ningún comando, build, test ni caso hostil corrido. Todos los cálculos de tamaño (los ≈485 KiB de P2-8) son aritmética a mano sobre el formato serializado, no medidos.
2. **Sin fuentes de syn 3.0.4 ni proc-macro2 1.0.107** en el paquete congelado: no pude verificar `Safety`, `Type::FnPtr`/`TypeFnPtr`, `ItemMod.unsafety`, `Expr::RawAddr`, el modelado de `Verbatim`, si los enums son `non_exhaustive`, ni la semántica exacta de `Span::byte_range()` con múltiples fuentes. P2-2, P2-3 y parte de P2-4 dependen de esa API y están redactados como condicionales verificables.
3. **Sin el lado host/gateway**: el productor del manifiesto, el mapeo índice→path, la proyección del artifact, los modos de montaje y la contención de memoria/PIDs/CPU/red no están en el paquete y quedan fuera del veredicto.
4. **Sin cruce con el lock del workspace ni con el dataset vendor** (estado cambiante, excluido por consigna) — de ahí que P3-13 sea un gap a verificar, no una afirmación.
5. **No hay calificación guest del helper**: compilar y pasar los checks benignos del README no cierra M4-02. Los gaps esperados y pendientes (gateway de contención, imagen derivada, SBOM/licencias, digest, 30 cold/30 warm) son distintos de los defectos listados arriba, que son atribuibles al código congelado.
6. Ninguna afirmación de seguridad general: cero findings de este scanner no es evidencia de ausencia de unsafe, y esta revisión no evalúa el resto del sistema.
