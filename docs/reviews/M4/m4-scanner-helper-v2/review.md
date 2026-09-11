He leído los 9 archivos del inventario congelado (`inputs.json`), completos, sin ejecutar nada.

---

# Veredicto

**Alcance:** solo `docs/reviews/M4/m4-scanner-helper-v2/inputs/` (helper `lib.rs`/`main.rs`/`Cargo.toml`/`Cargo.lock`/`README.md`, `crates/execution-adapter/src/unsafe_scan.rs`, `crates/domain/src/unsafe_scan.rs`, ADR-069 y la revisión v1). Sin ejecución, sin Docker, sin subagentes, sin tocar estado vivo del repo.

**Resultado:** el P1 de v1 está **corregido en su causa principal** (presupuesto explícito + conservación del prefijo) y **8 de 8 P2 están atendidos** (6 corregidos en código, 2 por premisa documentada + mitigación parcial). Encontré **0 P0, 1 P1 nuevo, 1 P2 nuevo (condicional, dependiente de código fuera del paquete), 8 P3**. El P1 nuevo es un defecto estructural introducido precisamente por el arreglo del P2-7 (drenaje acotado): **falla cerrado —nunca declara `complete`— pero cancela el resto del escaneo tras el primer archivo que agota su timeout.**

Ninguna evidencia incompleta se declara completa: verificado en las dos capas (`aggregate_manifest` + `validate_output` + `UnsafeScanReport::validate`).

---

## P1 nuevo

### P1-1 — Un solo archivo en timeout aborta el escaneo de todos los archivos restantes con presupuesto intacto

`lib.rs:436-439` (rama de timeout), `lib.rs:448-461` (ventana de confirmación de drenaje), `lib.rs:365-368` + `384-393` (aborto del resto), `README.md:78-80`.

**Trigger (determinista, no es solo una carrera):** cualquier archivo cuyo hijo agote su asignación (los casos hostiles de cadena unaria/profundidad que motivan ADR-069, o un archivo grande lento). En la rama de timeout se ejecuta `terminate_and_reap` y se sale del bucle; acto seguido, `remaining = deadline.saturating_duration_since(Instant::now())` es **cero por construcción** (acabamos de superar el deadline), así que la confirmación de drenaje cae en `receiver.try_recv()`, un sondeo no bloqueante emitido microsegundos después de que el hilo lector *podría* haber visto EOF. En un guest con CPU acotada el lector normalmente aún no ha sido planificado → `TryRecvError::Empty` → `drain_confirmed = false`.

**Impacto:** `aggregate_manifest:365-368` marca **todos los índices siguientes** como `budget_exhausted` y termina, con hasta 118 s de presupuesto sin usar. Con 4096 archivos y un archivo patológico en el índice k, se pierden 4095−k archivos que sí se habrían analizado. Contradice ADR-069:40-43 ("una muerte/señal/parse error/timeout deja cobertura parcial y **no elimina resultados de otros archivos**") y reintroduce en el agregado la propiedad rechazada en *Alternatives* ("un archivo hostil elimina resultados ajenos"). Secundario: la etiqueta es inexacta —el host recibe `budget_exhausted` cuando la causa fue un drenaje no confirmado— y `UnsafeCoverage` no puede distinguir ambas causas (`adapter:345`).

**Lo que NO rompe:** los resultados ya calculados se conservan (`files.push` en `lib.rs:357` ocurre antes del corte), la fila del archivo lento sale `timed_out`, y `syntax_complete` es falso (`domain:129-133`, `adapter:367-370`). Integridad intacta; se degrada cobertura y precisión de la etiqueta.

**Corrección:** dar al drenaje una gracia acotada e independiente tras `terminate_and_reap` (p. ej. `recv_timeout(CHILD_POLL_INTERVAL)` o un épsilon fijo) en lugar del `try_recv` de ventana cero; el hijo ya está segado y reapeado, así que EOF es inminente salvo descendiente con el pipe heredado. Alternativa: tratar "hijo reapeado con éxito" como condición suficiente para distinguir *EOF pendiente* de *lector realmente desprendido*. Si se conserva la política de no admitir más hijos, añadir un estado propio (`drain_unconfirmed`) para no mentir con `budget_exhausted`.

---

## Disposición de cada finding de v1

| v1 | Disposición | Evidencia |
|---|---|---|
| **P1-1** cobertura parcial bajo deadline global | **Corregido** | `budget_ms` 1..=118000 (`lib.rs:26,201`), reloj en `run_supervisor:304`, corte previo a cada hijo y marcado del resto (`lib.rs:343-348`), asignación por hijo `CHILD_TIMEOUT.min(remaining)` (`lib.rs:349`), test con reloj inyectado (`lib.rs:1363-1399`). Residual: emisión sigue siendo única al final (aceptado por ADR:82-84) y el reloj arranca **después** de `load_manifest` (P3-4) |
| **P2-1** `too_large`/no-UTF-8 = `unavailable` | **Corregido** | Estados distintos `TooLarge`/`InvalidUtf8` (`lib.rs:87-88`, `243-255`), test `lib.rs:1287-1298`, host los cuenta por separado y los cruza con los bytes capturados (`adapter:313-322`) |
| **P2-2** `Verbatim` sin contador | **Corregido** | `opaque_syntax_omitted` + `opaque()` (`lib.rs:625,669-671`) en `Item/Expr/Type/Pat/ImplItem/TraitItem/ForeignItem/TypeParamBound/Lit` (`lib.rs:710,720,730,751,761,771,816,866,874`); hace parcial la cobertura (`domain:131`). Residual P3-6 |
| **P2-3** `unsafe trait` / statics foráneos | **Corregido** | `visit_item_trait` (`lib.rs:952-957`) y `visit_foreign_item_static` con `Safety::Unsafe` (`lib.rs:959-964`); `UnsafeTrait`/`UnsafeStatic` en el enum de cable, dominio y ADR (`lib.rs:100-101`, `domain:42-43`, ADR:95) |
| **P2-4** `cfg_attr(..., unsafe(...))` | **Corregido** | `inspect_cfg_attr` (`lib.rs:673-699`) con `parse_nested`/`Punctuated<Meta>`, sin evaluar predicado ni expandir, `conditional=true` forzado (`lib.rs:691`), recursión sobre `cfg_attr` anidado, test `lib.rs:1173,1200-1205` |
| **P2-5** mitad supervisora sin pruebas | **Corregido** | `valid_worker_output`/`decode_worker_output` (`lib.rs:1286-1321`), `read_stream_bounded` exacto/desbordado/error a mitad y `read_bounded` (`lib.rs:1336-1360`), `aggregate_manifest` con `supervise` inyectado (`lib.rs:1363-1399`), peor caso de tamaño (`lib.rs:1402-1437`), límite de entradas del manifiesto (`lib.rs:1440-1448`). Residual: `supervise_file` sigue sin cobertura (P3-3) |
| **P2-6** TOCTOU / re-lectura del manifiesto | **Atendido por premisa + mitigación parcial → P3** | Premisa declarada (README:37-42; ADR:101-107: volúmenes owned, RO, sin otros writers, sin symlinks/FIFO), paths duplicados rechazados (`lib.rs:207-216`, test `1259-1261`). Sin digest, decisión consciente. **Mitigación real no declarada en v1:** el host revalida cada finding retenido contra *sus propios bytes capturados* —keyword, límite de identificador, línea y columna (`adapter:477-527`)— y liga `parsed`/`parse_error`/`invalid_utf8` a la UTF-8-idad de esos bytes (`adapter:313-322`). Una divergencia manifiesto↔captura invalida la respuesta entera en vez de misatribuir. Residual: contadores/estado de archivos sin finding retenido no están ligados (P3-5) |
| **P2-7** drenaje sin cota | **Corregido en la cota, con regresión funcional** | Canal + `recv_timeout`, lector desprendible, sin más hijos tras drenaje no confirmado (`lib.rs:427-430,448-462,365-368`). **Ver P1-1**: la rama de timeout deja ventana cero |
| **P2-8** contadores sin cota / `OutputTooLarge` todo-o-nada | **Corregido** | Contadores acotados por `MAX_SOURCE_BYTES` en las dos capas (`lib.rs:508-511`, `adapter:306-309`), claves compactas `i,s,total,omitted,macros,opaque` (`lib.rs:134-148`), degradación que borra findings y **conserva todas las filas de estado/conteo** antes de fallar (`lib.rs:306-317`), test de peor caso (`lib.rs:1402-1437`). Mi aritmética a mano sobre el peor caso real (4096 filas `parsed` con contadores de 7 dígitos ≈ 373 KiB + 128 findings ≈ 18 KiB ≈ 391 KiB) deja ~25 % de margen sobre 524 288; el peor caso alternativo (`budget_exhausted`, +10 B de estado) es más corto porque sus contadores son cero por construcción (`lib.rs:384-393`) |
| P3-1 carrera try_wait/deadline | **No corregido** | `lib.rs:431-447` sigue sin `try_wait` final tras el kill; ahora además dispara P1-1 |
| P3-2 `Crashed` emitido por el worker | **Resuelto por contrato explícito** | `valid_worker_output` admite `Crashed` del hijo y le exige contadores en cero (`lib.rs:502,521-526`), coherente con `lib.rs:268-271`. Residual doc: el README no enumera los estados válidos por rol |
| P3-3 presupuesto de 128 findings por índices bajos | **Documentado** | README:106-110; el host además ordena workspace antes que dependencias (`adapter:128-134`) |
| P3-4 portadores de atributos sin cobertura cfg | **Corregido** | `visit_pat`, `visit_fn_arg`, `visit_receiver`, `visit_field_pat`, `visit_variadic`, `visit_named_arg`, `visit_fn_ptr_variadic`, `visit_arm`, `visit_field`, `visit_field_value`, `visit_variant`, `visit_generic_param` (`lib.rs:739-863`), test de parámetro condicional (`lib.rs:1206-1215`) |
| P3-5 doble serialización | **No corregido (empeora)** | `lib.rs:306,315` + `main.rs:53`: hasta 3 serializaciones de ~400 KiB |
| P3-6 `File::open` sin timeout (FIFO) | **No corregido; cubierto por premisa** | `lib.rs:581`; ADR:104-105 excluye FIFO/symlink de la entrada admitida. En el hijo la exposición está acotada por el timeout; en el supervisor no |
| P3-7 `argc == 0` → modo supervisor | **No corregido** | `main.rs:16-18` |
| P3-8 paths duplicados | **Corregido** | `lib.rs:207-216` |
| P3-9 README vs. drenaje | **Corregido** | README:78-79 describe el drenaje a EOF con retención de 512 KiB |
| P3-10/P3-11 corpus hostil | **No verificable** | `hostile/generate.py` **no está en el paquete congelado**; README:145-148 afirma el rechazo de profundidades negativas y de peticiones >1 MiB, pero no puedo confirmarlo |
| P3-12 lints de defensa en profundidad | **Parcial** | `overflow-checks = true`, `unsafe_code = deny`, clippy `panic/unwrap_used/expect_used = deny` (`Cargo.toml:22-31`); **sin** `indexing_slicing` ni `arithmetic_side_effects` |
| P3-13 lock más ancho que el ADR | **No corregido** | `Cargo.lock` mantiene serde 1.0.229 (+`serde_core`/`serde_derive`), serde_json 1.0.151, zmij 1.0.23, memchr 2.8.3, itoa 1.0.18, quote 1.0.47, unicode-ident 1.0.24; ADR-069:20-23 sigue declarando pre-adquiridos solo syn y proc-macro2. Gap de aprovisionamiento |

---

## P2 nuevo (condicional)

### P2-1 — La completitud se calcula sobre lo capturado, y un `.rs` >1 MiB hace fallar el escaneo entero en vez de degradar
`adapter:107-109`, `adapter:165-174`, `adapter:367-370`, `domain:129-133`

**Trigger:** un archivo `.rs` benigno de más de 1 MiB (bindings generados: el caso de mayor densidad de `unsafe extern`). `planned_file` devuelve `SecurityError::OutputLimit` con `?`, así que **falla la llamada completa** en vez de producir la fila `too_large` que el protocolo v2 acaba de añadir; esa ruta del helper queda inalcanzable desde este productor.

**El segundo filo, condicional:** `files_total` cuenta solo los candidatos que ya venían en `SourceBundle`/`CargoVendorSnapshot`. Si la captura M3 **omite silenciosamente** archivos por sus propios topes (ADR-069:16 declara 4096 entradas, 16 MiB total y 1 MiB por archivo) en lugar de fallar, entonces `files_omitted == 0` aquí y `syntax_complete` puede salir `true` con archivos jamás capturados. La regla "la ausencia de findings con errores de archivo nunca produce un resultado completo" (ADR:71-73) se cumple **dentro** de la selección, pero no cubre una truncación aguas arriba.

**Por qué es condicional:** el comportamiento de `SourceFile::new`/`SourceBundle::new` ante ficheros grandes o topes de captura **no está en el paquete congelado** y la consigna me impide leer el árbol vivo. Exactamente una de estas dos cosas es cierta y ambas merecen acción: (a) la captura falla ⇒ `adapter:107` es código muerto y el fallo duro llega antes, o (b) la captura omite ⇒ `syntax_complete` puede declarar completo un escaneo con omisiones invisibles. **Corrección:** propagar un contador de omisiones de la captura hasta `UnsafeCoverage.files_omitted`, y mapear el exceso de tamaño a la fila `too_large` en vez de a un error terminal.

---

## P3 nuevos

| # | Ubicación | Detalle |
|---|---|---|
| P3-1 | `domain/src/unsafe_scan.rs:81-83,89` | `pub files_invalid_utf8:u32,` `files_too_large:u32,` `files_budget_exhausted:u32,` `opaque_syntax_omitted:u64,` sin espacio tras `:`. El archivo congelado no está rustfmt-limpio; `cargo fmt --check` del workspace fallaría sobre estos bytes |
| P3-2 | `lib.rs:225` (worker) vs `lib.rs:303` (supervisor) | Cada hijo sigue releyendo y revalidando el manifiesto de hasta 1 MiB (`load_manifest` completo, incluido el `BTreeSet` de 4096 paths). Ya no se pierde el resultado (lo acota `budget_ms`), pero el coste O(n²) ahora **se traduce en cobertura**: cada milisegundo de sobrecarga por archivo son archivos que salen `budget_exhausted`. Sin medición guest no se puede afirmar el margen |
| P3-3 | `lib.rs:395-476` | `supervise_file` sigue sin ninguna prueba (única función que cruza el límite de proceso). Es también donde vive P1-1: sería testeable inyectando el path del helper o factorizando la decisión de drenaje a una función pura sobre `(status, remaining, recv_result)` |
| P3-4 | `lib.rs:302-305` | El reloj del presupuesto arranca **después** de `load_manifest`, así que la lectura y el parseo del manifiesto consumen la reserva de 4 s del gateway (ADR:78-79), igual que las hasta 3 serializaciones finales (`lib.rs:306,315` + `main.rs:53`) y el arranque en frío del contenedor. La reserva no está presupuestada por partidas en ningún punto del paquete |
| P3-5 | `adapter:297-322` | Los contadores y el estado de un archivo **sin finding retenido** no están ligados a la identidad de los bytes capturados más allá de la clase UTF-8/no-UTF-8. Bajo la premisa de volúmenes inmutables es inalcanzable; si la premisa cae, `total/macros/opaque` pueden intercambiarse entre dos archivos UTF-8 sin señal |
| P3-6 | `lib.rs:673-686` vs README:118 | `opaque_syntax_omitted` mezcla nodos `Verbatim` con **colas de `cfg_attr` que no parsean como `Meta`** y con `cfg_attr` sin lista. Es conservador y hace parcial la cobertura, pero el README solo documenta el caso `Verbatim` |
| P3-7 | `lib.rs:989,1013,1059,1070,1091,1102,1113` | Los brazos `_ => &[]` de los enums `non_exhaustive` de syn: una variante futura **no-`Verbatim`** se sigue visitando pero pierde la herencia `cfg` (finding con `conditional:false` falso) y **no incrementa ningún contador de opacidad**, a diferencia de `Verbatim`. Asimetría silenciosa ante un bump de syn |
| P3-8 | `adapter:192` vs `domain:15` | `UnsafeScanOptions` admite `timeout_seconds` en 1..=120 y `manifest_bytes` exige `budget_ms >= 1` tras reservar 4 s. El productor de `budget_ms` no está en el paquete: si no clampa, todo timeout ≤4 s fallaría con `InvalidMetadata` antes de lanzar nada. A acreditar en la calificación del gateway |

**Observación de semántica (no defecto):** `syntax_complete` exige `opaque_syntax_omitted == 0` pero **no** `macro_boundaries_omitted == 0` (`domain:129-133`). Es coherente con el ADR (`macros_expanded=false` se declara siempre y el doc-comment de `domain:98` lo acota), pero un consumidor puede leer `syntax_complete: true` sobre un archivo lleno de macros no inspeccionadas. Merece quedar en la proyección/documentación del host, no en el código.

---

## Propiedades verificadas (positivas, acotadas al paquete)

- **Frontera del parser intacta.** Ninguna ruta del supervisor llama a `syn`: `run_supervisor` → `load_manifest`/`aggregate_manifest`/`supervise_file`/`decode_worker_output`, todas sobre JSON tipado. `syn` solo es alcanzable desde `scan_source`, y esta solo desde `--file-index` (`main.rs:19-22`). `inspect_cfg_attr`, que es recursiva y sí parsea, vive únicamente en el visitor del hijo. Un `cfg_attr` anidado hostil desborda la pila **del hijo** → `crashed` → el supervisor continúa (`lib.rs:467-471`).
- **Retención del AST** en las tres salidas de `scan_source` (`lib.rs:269,298`), evitando el drop recursivo.
- **Formato de cable sin texto libre:** `Finding`/`WorkerOutput`/`FileSummary`/`SupervisorOutput`/`FatalOutput` no tienen ningún `String` (`lib.rs:108-176`); el host además exige `stderr` vacío y `exit_code == 0` (`adapter:227-229`).
- **Spans byte/Unicode con doble verificación independiente.** El hijo exige `source.get(range) == Some(keyword)` antes de emitir (`lib.rs:642-650`), y **el host recalcula** línea (conteo de `\n`) y columna (`chars().count()`, 1-based) sobre sus propios bytes y exige frontera de identificador (`adapter:485-527`). `unsafely` con span 0..6 se rechaza; una divergencia de offsets invalida la respuesta en vez de misatribuir. El test CRLF/Unicode (`lib.rs:1220-1225`, columna 20) y el del host (`adapter:702-715`, `byte_start 7` tras `// é\r\n`) son consistentes con columnas por escalar de proc-macro2.
- **Aritmética de conteos coherente en las dos capas:** `total == retained + omitted` por archivo y global, con `checked_*` en el host (`adapter:278-280,299-300,323-354`) y `saturating_*` en el helper; el peor caso comprimido cabe con margen medido por test.
- **Degradación antes del fallo:** el borrado de `findings` conservando filas de estado (`lib.rs:306-314`) produce una salida que el host sigue aceptando (`omitted == total`, `per_file_retained == 0`).
- **Fallo cerrado del vocabulario:** `deny_unknown_fields` en manifiesto, `WorkerOutput`, `Finding`, `SupervisorOutput`, `FileSummary`, `WireFinding` en ambos lados; claves duplicadas rechazadas y probadas (`lib.rs:1316-1320`, `adapter:824-828`); el corte de 4096 entradas ocurre durante la deserialización sin materializar el resto (`lib.rs:61-73`, test `1440-1448`).
- **Lanzador:** binario absoluto constante, índice numérico ASCII validado <4096 (`main.rs:31-43`), `env_clear` + PATH fijo, sin shell, stdin null, stderr descartado (`lib.rs:398-406`); todos los hijos se reapean (`terminate_and_reap` en las tres ramas de salida), y el lector desprendido no impide la salida del proceso.

---

## Limitaciones

1. **Sin ejecución alguna:** ni build, ni tests, ni casos hostiles (tampoco en el host, por consigna). Los tamaños de peor caso son aritmética a mano; la carrera de `try_recv` (P1-1) está derivada del código, no medida.
2. **No verifiqué los sha256 de `inputs.json`** (requeriría ejecutar); confirmé que los 9 paths existen y los leí íntegros.
3. **Sin fuentes de syn 3.0.4 / proc-macro2 1.0.107** en el paquete: no puedo confirmar que `NamedArg`, `FnPtrVariadic`, `TypeFnPtr`, `Safety` en `ForeignItemStatic`, `File::frontmatter` ni la lista completa de variantes `Verbatim` sean exactamente las de esa versión, ni que `Meta` acepte `unsafe` como path (`Ident::parse_any`), de lo que dependen `visit_attribute`/`inspect_cfg_attr`. También queda sin verificar el comportamiento de `is_ident("unsafe")` frente a `r#unsafe`: si fuese `true`, el span cubriría `r#unsafe` y `record` marcaría `invalid_span` → el archivo entero saldría `crashed` (pérdida de cobertura, no falso negativo silencioso).
4. **Sin el gateway ni el productor de `budget_ms`:** montajes RO, volúmenes owned, extracción sin symlinks/FIFO, contención de memoria/PIDs/CPU/red, cleanup del árbol, digest del helper y la escritura de `/security/scan.json` **no están en el paquete**. La disposición de P2-6 se apoya en esa premisa declarada, no verificada aquí; P2-1-nuevo y P3-8 son condicionales sobre ese mismo código.
5. **`crates/execution-adapter/src/deny_json.rs`** (usado en `adapter:230`) no está en el paquete: la pasada JSON estricta previa al `serde_json::from_slice` no está verificada.
6. **`hostile/generate.py` no está en el paquete**, así que P3-10/P3-11 de v1 quedan sin confirmar pese a lo que afirma el README.
7. **El helper sigue sin calificación guest**, y eso no es un defecto del código: build offline reproducible, SBOM/licencias, imagen derivada, digest, corpus hostil ejecutado en el guest calibrado y 30 cold/30 warm siguen pendientes y son distintos de los hallazgos anteriores. **No cierro M4-02 con esta revisión.**
8. Ninguna afirmación de seguridad general: cero findings de este scanner no es evidencia de ausencia de `unsafe`, y no evalúo el resto del sistema.
