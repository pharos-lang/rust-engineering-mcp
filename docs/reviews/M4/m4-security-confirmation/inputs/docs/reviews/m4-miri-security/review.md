He leído los 15 ficheros de `docs/reviews/m4-miri-security/inputs/` (más `inputs.json`). No he editado, ejecutado, invocado Docker ni lanzado subagentes.

---

# Veredicto

**No apto para abrir la puerta todavía; el diseño de clasificación es sólido y conservador, pero el perímetro de entrada al contenedor Miri está incompleto.**

La máquina de clasificación (JUnit + exit del runner + diagnósticos JSON del testcase) es la parte más fuerte del delta: es fail‑closed en todas las ramas que revisé, `clean` es inalcanzable sin exit 0 + suite no vacía + cero findings + `complete`, y los dos invariantes (`classify_junit` y `MiriReport::validate`) son mutuamente consistentes. No encontré ningún camino en el *parser* que convierta UB en "clean" ni test failure en UB.

El hueco real no está en el parser sino en **qué configuración del proyecto sigue siendo autoridad dentro del contenedor Miri** (H‑1). Todo el resto son defectos de taxonomía de errores, de vinculación de evidencia y de poder discriminante de las pruebas adversas pendientes.

`ADVERTISEMENT_READY=false` y `MIRI_IMAGE=None` los trato como puertas temporales, no como bugs, tal y como se indica. No reclamo cierre: el rerun sobre `25ed…` y las pruebas adversas siguen siendo prerrequisito.

---

# Cobertura de esta revisión

**Verificado por lectura completa:** parser JUnit/JSON y clasificación (`miri_output.rs`), invariantes de dominio (`domain/src/miri.rs`), admisión de productores (`miri_admission.rs`), argv/env/montajes/permisos/presupuesto de las fases Miri (`security_gateway.rs`), verificador de configuración aplicada (`rust_applied.rs::verify_security` + su test), ensamblado de la observación (`miri_port.rs`), normalización y publicación (`miri_log.rs`, `quality_artifacts/security.rs`), superficie MCP (`stdio/miri.rs`, `schemas.rs`), config de nextest y ADR‑072.

**No verificable desde este conjunto** (lo asumo correcto y lo listo como dependencia, no como hallazgo): `CaptureSourceBundle` (¿qué ficheros del proyecto entran en el bundle?), `seccomp-rust-quality.json`, `VOLUME_OPTIONS` (¿el volumen `/junit` es escribible por uid 65534?), `decode_single_file_tar`, `mutation_gateway::cleanup_until`/`start_attached`, `security_policy::SECURITY_CARGO_CONFIG`, `project_metadata::declared_toolchain`, las fixtures `results/*.junit.xml` y `results/*.json` (por tanto no puedo verificar independientemente los 10 oráculos: sólo veo las aserciones, no los bytes).

---

# Hallazgos

## P1 — La configuración Cargo del proyecto sigue siendo autoridad dentro de la fase Miri

**`crates/execution-adapter/src/miri_admission.rs:39-90`** (admisión) y **`security_gateway.rs:111-128`** (argv de la fase).

`validate()` enumera con cuidado los vectores *de manifiesto* (`proc-macro`, `custom-build`, `harness=false`, alias `proc_macro`), pero **no inspecciona ningún otro fichero capturado**. El contenedor Miri corre con `--workdir=/source` y `--manifest-path=/source/Cargo.toml`, de modo que cargo lee `/source/.cargo/config.toml` (y `/source/.cargo/config`) del proyecto. `CARGO_HOME=/security/cargo-home` sólo aporta el config de **menor** precedencia: en cargo, el config más cercano al cwd gana, así que `SECURITY_CARGO_CONFIG` no neutraliza al del proyecto, lo refuerza como base.

Lo que queda al alcance del productor hostil, en orden de gravedad:

- `[env] MIRIFLAGS = { value = "-Zmiri-disable-stacked-borrows -Zmiri-disable-validation -Zmiri-disable-alignment-check", force = true }`. Si ese `[env]` alcanza el proceso del runner (nextest aplica la sección `[env]` de la config de cargo a los binarios de test; el `env` del wrapper script de `nextest.toml:6-8` también la fija y **la precedencia entre ambos no está determinada por ningún fichero de este conjunto**), el resultado es una ejecución con UB real que Miri no detecta: exit 0, todos los tests passed, `clean = true`. Es el único escenario que he encontrado capaz de producir un `clean` falso, y por eso es P1 y no P2.
- `[build] rustflags`, `[target.<triple>.*]`, `[source]` replacement, `[alias]`: alcance menor. `[source]` está contenido aguas arriba por `security_metadata::prepare` (`security_metadata.rs:124-173`: todo paquete no‑workspace debe tener `manifest_path` bajo `/rust-mcp-vendor/`, `source` exactamente crates.io y checksum coincidente con el vendor autenticado). El `runner` de `[target.*]` lo gana la env `CARGO_TARGET_<TRIPLE>_RUNNER` que fija cargo‑miri. `[alias] miri = …` parece **no** explotable porque la cola de argv (`nextest run --config-file=… --profile=…`) sólo es válida para nextest; lo menciono para que la prueba adversa lo confirme en vez de suponerlo.

**Escenario discriminante (una fixture, barata):** proyecto idéntico a `fixtures/…/uaf` más `/source/.cargo/config.toml` con el `[env] MIRIFLAGS … force = true` de arriba. Comportamiento correcto esperado: `ClassificationIntegrityUnsupported`. Comportamiento a descartar: `clean = true` (P0) o `undefined_behavior = 0` con `complete = true`.

**Corrección de coste mínimo y fail‑closed**, sin depender de resolver la precedencia real: rechazar en `miri_admission::validate` cualquier fichero capturado cuyo path sea `.cargo/config.toml`, `.cargo/config`, `rust-toolchain`, `rust-toolchain.toml` (en cualquier directorio del bundle, no sólo en la raíz), devolviendo `incompatible()`. Es coherente con la decisión ya tomada en ADR‑072 de excluir proyectos habituales para conservar integridad del productor, y convierte la pregunta de precedencia en irrelevante. Añadir `.config/nextest.toml` por defensa en profundidad (hoy neutralizado por `--config-file`, pero gratis).

Nota: si `CaptureSourceBundle` ya excluye dotfiles, esto es un no‑issue y basta con dejarlo escrito como invariante del port; hoy no hay nada en el conjunto revisado que lo afirme, y `security_policy::reject_project_exceptions(source)` (invocado sólo para Deny, `security_gateway.rs:921-924`) sugiere lo contrario: que ficheros arbitrarios del proyecto sí llegan al bundle.

## P2 — El `execution_fingerprint` no vincula el clasificador

**`crates/execution-adapter/src/security_gateway.rs:808-817`.**

La tupla de identidad incluye `security_gateway.rs`, `security_metadata.rs`, `security_policy.rs`, `mutation_gateway.rs`, `rust_applied.rs`, ambos seccomp y `miri_admission.rs`, pero **no** `miri_output.rs` ni `miri_port.rs`. El `plugin.digest` del artefacto (`quality_artifacts/security.rs:76`) es el del normalizador `miri_log.rs`, no el del clasificador.

**Escenario discriminante:** cambiar un umbral en `classify_failed_case` (`miri_output.rs:918-953`) — por ejemplo aceptar `known_categories >= 1` en vez de `== 1` — y reejecutar el mismo proyecto con la misma imagen. Se publican dos artefactos con `execution_fingerprint` idéntico, `implementation_digest` idéntico y `plugin.digest` idéntico, pero con `counts` distintos. Eso contradice directamente ADR‑072:81-82 ("Los cambios del runtime, nightly, sysroot, wrapper o **parser** exigen recalibración"): hoy nada en la evidencia detecta un cambio de parser.

**Corrección:** añadir `digest(include_bytes!("miri_output.rs"))` y `digest(include_bytes!("miri_port.rs"))` a la tupla de `execution_fingerprint`. Ojo: cambia todos los fingerprints existentes, hay que hacerlo antes de emitir recibos definitivos.

## P2 — Los oráculos de timeout y cancel no discriminan la fase que dicen cubrir

**`crates/execution-adapter/src/miri_native.rs:161-230`.**

Los casos `"timeout"` (presupuesto total 8 000 ms) y `"cancel"` (`CancelAfter` a los 8 s) se aplican al **presupuesto completo de `execute_operation`**, que incluye creación de 4 volúmenes, 4 guardianes, 3 ingests, `cargo metadata` y sus `revalidate` intermedios. En un daemon con round trips de ~250 ms (el propio código presupuesta eso en `security_gateway.rs:882-884`) el deadline expira con altísima probabilidad **antes** de crear el contenedor Miri. Las aserciones `error == SecurityError::Timeout` / `Cancelled` se cumplen igual, y el test pasa sin haber demostrado nunca lo que su nombre promete: que un test *interpretado* en bucle infinito se termina, se une y se limpia.

**Escenario discriminante:** el mismo caso con presupuesto ~120 s (suficiente para llegar a la fase Miri con el bucle `unbounded()` corriendo bajo el intérprete) y aserción adicional sobre `elapsed_ms` mínimo (p. ej. > 30 s) para probar que el deadline se agotó *dentro* de la interpretación, no en el ingest. Idem para `cancel`, cancelando después de que el contenedor Miri esté corriendo.

Relacionado y verificable en el mismo test: `slow-timeout = { period = "60s", terminate-after = 1 }` (`nextest.toml:14`) no está cubierto por ningún caso. Un test que nextest termina por lento produce un `<failure>` cuyo `type` no casa con `"test failure with exit code N"` → `parse_runner_exit` devuelve `None` → `unclassified` (fail‑closed y correcto), pero eso hoy es una suposición mía sobre la cadena de nextest, no evidencia.

## P2 — Falta reserva de presupuesto para la exportación del JUnit

**`crates/execution-adapter/src/security_gateway.rs:1218-1230` y `1247-1270`.**

La fase Miri recibe `deadline` completo en `start_attached`, y las fases `MiriExport`/`finish_phase` se ejecutan **después** contra el mismo `deadline`. No hay nada análogo a `scanner_budget_ms` (`security_gateway.rs:884-891`), que sí reserva 25 s de control y cleanup para la ruta de escaneo.

**Escenario discriminante:** un proyecto cuya suite bajo Miri consume el 99 % del presupuesto (trivial: Miri es 10²–10³ veces más lento que nativo, y el máximo son 1 800 s). El run termina correctamente y escribe `junit.xml`, y acto seguido `budget_error` en `create_phase` del export devuelve `SecurityError::Timeout`: se descarta una interpretación completa y válida, y el usuario recibe `COMMAND_TIMEOUT` sin distinguirlo de un cuelgue real. Con el máximo de 1 800 s ese desperdicio es de media hora.

**Corrección:** acotar el `start_attached` de la fase Miri a `deadline - reserva` (la reserva es determinista: create + attach + finish + revalidate del export ≈ 8–10 round trips) y dejar el `deadline` completo sólo para el export.

## P2 — `--lib --tests` puede abortar el run entero en workspaces sin ninguna librería

**`crates/execution-adapter/src/security_gateway.rs:111-128`.**

`cargo … nextest run --workspace --lib --tests` con `--lib` explícito falla ("no library targets found in packages: …") cuando **ningún** paquete del workspace tiene target lib. Todas las fixtures del oráculo (`miri_native.rs:45-56`) traen `src/lib.rs`, así que ese caso no está cubierto por ninguna evidencia.

**Escenario discriminante:** workspace de un solo crate binario con `tests/`. Si el fallo llega con exit ≠ 104, `security_gateway.rs:1280-1284` devuelve `InvalidMetadata` → `Code::InvalidProject` ("Captured interpreter inputs or evidence could not be validated"), un mensaje que culpa al proyecto de algo que es una restricción del argv fijo. Si llega con 104, `classify_without_junit` no encuentra JSON de rustc (el error es prosa de cargo, no un diagnóstico `--error-format=json`) → `unclassified` → `Blocked EVIDENCE_INCOMPLETE`. Ninguna de las dos respuestas es la correcta, que sería o bien ejecutar los tests de integración normalmente (quitando `--lib`, ya implícito en `--tests`), o bien un rechazo tipado.

Marco esto como defecto a confirmar con fixture, no como confirmado: depende del comportamiento exacto de cargo con `--workspace --lib`, que no puedo ejecutar aquí.

## P3 — Agotamiento de presupuesto reportado como entrada forjada

**`miri_port.rs:25-34`**, **`miri_output.rs:11-26, 663-675`**, **`security_gateway.rs:1271-1284`**.

Tres sitios colapsan "demasiado grande" en "inválido":

1. `miri_port.rs:34` mapea **todo** `MiriParseError` a `InvalidMetadata`, perdiendo `MiriParseError::InputLimit`, que existe precisamente para distinguirlo. Debería mapear a `SecurityError::OutputLimit` (que la superficie MCP ya sabe traducir a `OUTPUT_LIMIT_EXCEEDED`, `stdio/miri.rs:327-331`).
2. `MAX_NODES = 16_384` se agota antes que `MAX_TESTCASES = 4_096`: un testcase fallido con `<failure>` + `<system-err>` + texto consume ~7 tokens, así que el techo efectivo son ~2 300 tests fallidos, y el error emitido es `InvalidJunit` (JUnit forjado) en vez de un límite de entrada. Escenario: workspace con 2 500 tests fallando → `INVALID_PROJECT`.
3. Un `junit.xml` legítimo de entre 512 KiB y 1 MiB pasa el límite de captura pero falla en `decode_single_file_tar(…, 512*1024, …)` → `InvalidMetadata` → `INVALID_PROJECT`.

Ninguno es una brecha de seguridad (todos fail‑closed), pero los tres degradan un problema de tamaño a una acusación de forja, que es justo la distinción que el resto del diseño se toma en serio.

## P3 — `MiriCategory::Timeout` es inalcanzable

**`domain/src/miri.rs:27`** y **`miri_output.rs:891-899`**.

Ninguna ruta de `classify_junit`/`classify_without_junit` emite `MiriCategory::Timeout`; `counts.timeouts` es siempre 0 y la cláusula `timeouts == 0` de `MiriReport::validate` (`domain/src/miri.rs:100`) es trivialmente cierta. Un timeout de gateway aborta con `SecurityError::Timeout` sin informe, y un timeout por test de nextest cae en `unclassified`. La variante se publica en el esquema MCP (`stdio/miri/schemas.rs:9`) como si fuera alcanzable. O se elimina, o `classify_failed_case` reconoce el `type` de timeout de nextest y la emite — esto último es preferible porque hoy un test que **cuelga bajo UB** es indistinguible de cualquier otro `unclassified`.

## P3 — Nombres de test controlados por el proyecto llegan íntegros al agente

**`miri_output.rs:411-417`** (`validated_identity`) → `miri_output.rs:901-905` → `miri_log.rs` → `stdio/miri.rs:359-373`.

El único filtro sobre `name`/`classname` es longitud ≤ 512 y ausencia de caracteres de control. Con 128 findings eso son hasta ~64 KiB de prosa arbitraria del proyecto (incluidos bidi overrides, homoglifos e instrucciones en lenguaje natural) publicados en la respuesta MCP y en el artefacto durable, que consume un agente LLM. Es sistémico (nextest hace lo mismo), pero aquí el vector es un proyecto explícitamente tratado como hostil. Restringir a un charset de identificador de test (`[A-Za-z0-9_:$.<>#-]` y `::`) es barato y no pierde información útil.

## P3 — Campos de la observación sin vincular en la capa de aplicación

**`crates/application/src/miri.rs:57-62`.**

`miri_durable` valida `report.validate()`, `vendor_fingerprint` y `runtime.execution_fingerprint == execution_fingerprint`, pero deja sin comprobar `source_fingerprint`, `metadata_fingerprint`, `config_fingerprint`, `junit_fingerprint` y la coherencia `report.junit_present == junit_fingerprint.is_some()`. En el adaptador real esos campos están ligados estructuralmente (`miri_port.rs:38-66`), así que hoy no hay bug; pero `ProjectMiriPort` es un trait y la aplicación es la frontera donde se supone que se comprueban los invariantes del dominio. El más barato y el que más aporta: `observation.report.junit_present == observation.junit_fingerprint.is_some()`.

## P3 — Identidad del runtime autoafirmada, y la imagen medida cambió

**`crates/execution-adapter/src/miri_port.rs:52-65`** y **`miri_admission.rs:9-13`.**

`platform`, `rust_version`, `cargo_version`, `NIGHTLY_COMMIT` y `SYSROOT_HASH` son constantes de compilación; nada las re‑deriva dentro del contenedor. La única atadura es `Some(gateway.image_id()) != MIRI_IMAGE` (`miri_port.rs:18`). Eso es aceptable **si y sólo si** `MIRI_IMAGE` acaba fijado exactamente al digest sobre el que se midieron esas constantes. Los 10 oráculos se corrieron con `ecade…` y la imagen final es `25ed…` (`miri_native.rs:16,149`): aunque el cambio sea sólo el helper del scanner, `SYSROOT_HASH` y `NIGHTLY_COMMIT` deben re‑acreditarse contra `25ed…` antes de abrir la puerta, porque son las que se publican como evidencia. Lo anoto como prerrequisito, no como bug.

## P3 — Deriva de esquema no vigilada

**`crates/mcp-server/src/stdio/miri/schemas.rs`** (todo el fichero, `#[allow(dead_code)]` en `stdio/miri.rs:2`).

`schemas::Observation` es una copia manual de `domain::miri::MiriObservation` usada sólo vía `#[schemars(with = …)]` (`stdio/miri.rs:135`). No hay ninguna prueba que serialice una observación real y la valide contra el esquema publicado. Añadir un campo a `MiriReport` en el dominio no rompe nada en compilación pero deja el esquema anunciado incompleto, y como el esquema declara `deny_unknown_fields` (`schemas.rs:34`), un cliente que valide en estricto rechazaría la respuesta. Impacto actual bajo por `ADVERTISEMENT_READY=false`; el test de conformidad cuesta cinco líneas.

## P3 — `all_names` alarga el cleanup de las rutas compartidas

**`crates/execution-adapter/src/security_gateway.rs:960-972`.**

`junit_guardian` y `junit_export` se incluyen en `all_names` para **todas** las operaciones, también Deny y UnsafeScan, donde esos contenedores nunca existieron. Son ~4 round trips extra dentro de la ventana fija `CLEANUP = 10 s`, contra un presupuesto que el propio código estima a 250 ms por round trip. Con el margen actual sigue cabiendo, así que no lo veo como rotura, pero es una regresión introducida por el delta Miri sobre rutas compartidas y se elimina condicionando la lista a `final_phase == SecurityPhase::Miri`, igual que ya se hace con `junit_cleanup` en la línea 1308.

---

# Limitaciones explícitas, correctamente documentadas (no son defectos)

- Rechazo de proc macros, build scripts y harness personalizado: es la decisión central de ADR‑072:19-25, no una carencia. La implementación es doble (metadata `kind`/`crate_types` + reparseo del manifiesto para `harness`, que metadata no expone) y cubre dependencias vendorizadas, no sólo el workspace.
- `clean` sólo acredita los tests y la configuración seleccionados; sin doctests, benches ni examples; sin `--all-features`. Está en ADR‑072:24-25 y 74-75, en el `semantics` de la respuesta (`stdio/miri.rs:361`) y en la descripción de la tool. Consecuencia lógica: un proyecto con `test = false` en un target, o con `#[cfg(not(miri))]`, obtiene `clean` legítimamente con cobertura mínima — y el informe no publica la lista de tests seleccionados, sólo `counts.tests`. Es limitación documentada, aunque un consumidor sólo pueda auditar la cobertura por ese único número.
- `MIRI_IMAGE = None` y `ADVERTISEMENT_READY = false`: puertas temporales, tratadas como tales.
- Que `stdio/miri.rs:383-390` colapse UB y fallo de test ordinario en un mismo `error_code: OBSERVED_FAILURE` es una elección de diseño coherente (el detalle está en `data.observation.report.counts` y el mensaje lo dice), no un defecto de clasificación.

---

# Lo que verifiqué como correcto

Merece constar, porque es donde estaba el riesgo principal:

- **Contención del productor hostil dentro del intérprete.** Con `-Zmiri-isolation-error=abort` (`miri_admission.rs:25`) un test que intente escribir en `/junit` aborta con `unsupported operation:` → categoría `UnsupportedOperation`, nunca `clean`. Y aunque la aislación se desactivara (H‑1), forjar un `clean` seguiría exigiendo que la corrida real saliera con exit 0 y todos los tests passed, porque `classify_junit:982-991` cruza el exit global contra el esperado derivado del propio JUnit. La ausencia de `list-wrapper` en `nextest.toml` (asertada en `miri_admission.rs:146`) permite listar sin mutear, y el `-Zmiri-mute-stdout-stderr` sólo durante el run es lo que hace que `<system-err>` de un testcase contenga exclusivamente diagnósticos del intérprete: ese es el pilar de la clasificación y está bien construido.
- **Parser XML/JSON.** Cerrado (sin DTD, sin comentarios, sin CDATA, sin entidades personalizadas, sin recursión), con límites en profundidad, nodos, atributos, longitudes y testcases, cuadre de `tests/skipped/failures/errors` declarados contra observados en suite **y** raíz, y rechazo de atributos duplicados. No encontré índice fuera de rango ni panic alcanzable en `Scanner`/`decode_xml`.
- **Conservadurismo de la clasificación.** `known_categories == 1`, `system_out` no vacío → `unclassified`, exit 101 exige streams limpios, exit 1 exige diagnóstico semántico y ausencia de código rustc, y `is_abort_helper` sólo tolera el "aborting due to N previous error(s)". Los streams de nivel superior se ignoran deliberadamente cuando hay JUnit, con test que lo fija (`miri_output.rs:1315-1330`).
- **Consistencia `classify_junit` ↔ `MiriReport::validate`.** Repasé las dos definiciones de `clean` y las identidades de conteos (`tests == passed+failed+skipped`, `findings.len() + omitted == classified`): coinciden en todas las ramas, incluidas las truncaciones de `safe_miri_log` (`miri_log.rs:21-27`) y de `encode_result` (`stdio/miri.rs:410-420`), que dejan el informe todavía válido.
- **env/argv/permisos/volúmenes.** Matriz de montajes correcta (`/junit` escribible **sólo** en la fase Miri, `security_gateway.rs:230-232`), verificación de la configuración *aplicada* por el daemon extendida a las tres fases nuevas con flips de acceso, argv nulo/ausente y mutaciones de autoridad (`rust_applied.rs:1723-1861`), admisión ejecutada **antes** de crear el contenedor Miri (`security_gateway.rs:1141-1143`), y `cargo metadata` no ejecuta build scripts, así que un `build.rs` hostil nunca corre.
- **Cleanup.** Se ejecuta fuera del `work` closure y con deadline propio de 10 s, con el volumen y los dos contenedores nuevos incluidos; los guardianes (`sleep 3600`) sobreviven al presupuesto máximo de 1 800 s.

---

# Evidencia pendiente (no sustituible por esta revisión)

1. Rerun de los 10 oráculos sobre `25ed…`, con re‑acreditación de `NIGHTLY_COMMIT` y `SYSROOT_HASH` contra esa imagen.
2. Fixture adversa de **H‑1**: `/source/.cargo/config.toml` con `[env] MIRIFLAGS … force = true` sobre el caso `uaf`. Es la única prueba que separa "clean significa algo" de "clean significa lo que el proyecto quiera".
3. Casos `timeout`/`cancel` reconstruidos para expirar **dentro** de la fase Miri (P2), con aserción de `elapsed_ms`.
4. Fixture de workspace sin target lib (P2) y fixture de test terminado por `slow-timeout` (P3).
5. Confirmación operativa de que el volumen `/junit` es escribible por uid 65534 y de que `/opt/rust-nightly-2026-09-07/bin/cargo` no es un proxy de rustup (si lo fuera, un `rust-toolchain.toml` del proyecto sería autoridad; el rechazo propuesto en H‑1 lo cubre igualmente).
6. Sin cobertura visible en el conjunto revisado: `miri_port::run`, `safe_miri_log`, `miri_durable` y `encode_result` no tienen ninguna prueba entre los ficheros de entrada.