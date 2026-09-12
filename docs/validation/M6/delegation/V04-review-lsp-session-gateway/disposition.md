# Disposición — revisión independiente V04 del paquete W04 (sesión LSP, fase `Analyzer`, admisión, captura, calibración)

Fecha: 2026-09-12. Objeto: el diff W04 (hashes en [inputs.sha256](inputs.sha256)).
Revisor: Claude Opus 5 (Claude Code 2.1.268, `claude -p --model opus --effort
high --tools "" --restricted`, diff por stdin, 611 s), distinto del worker.
[Texto íntegro](claude-opus-5-review.md). Veredicto: **Block** por cuatro P2;
el revisor no encontró ningún camino en que el hijo docker o el contenedor
sobrevivan a `busy` (objetivo 1 sin hallazgos por encima de P3).

## Hallazgos del worker (F1/F2 de [01.md](../../01.md)) — decisiones del orquestador

| Hallazgo | Decisión |
| --- | --- |
| **F1** — `cargo.sysrootQueryMetadata` no existe en el binario real (18/19 claves) | Se **elimina** la clave de la configuración fija (opción 1 del worker). El oráculo hizo su trabajo: la clave venía de R01 y el brief la adoptó «sujeta a calibración». ADR-084 §3 y brief §4.5 se enmiendan a 18 claves; el `config_digest` cambia, como debe |
| **F2** — `health: warning` permanente («Auto-reloading is disabled and the workspace has changed…») por `cargo.autoreload=false`; `complete` inalcanzable y `SysrootWarning` miente sobre la causa | Se **retira** `cargo.autoreload=false` (queda el default `true`: en una sesión transitoria sin `didChange` y con `/source` RO no hay recarga que evitar). Se renombra `IncompleteReason::SysrootWarning` → `AnalyzerWarning` (cualquier `health: warning`, sin publicar el `message`, que puede llevar texto del proyecto). Un warning sigue degradando a `incomplete`: es honesto. La calibración debe observar `health: ok` tras el cambio; si no, es hallazgo nuevo, no se relaja el mapeo |

## Hallazgos del revisor

| Hallazgo | Disposición |
| --- | --- |
| **P2** — el corte m6-03 no puede detectar el proc-macro server: RA lo lanza como `rust-analyzer proc-macro`, mismo basename que el permitido; el argv se recoge pero no se comprueba | Aceptado. El oráculo pasa a ser por **argv completo** contra una lista cerrada: `/opt/analyzer/bin/rust-analyzer` sin argumentos; `rustc`/`cargo` de `/opt/rust/bin` solo con las formas de ADR-084 §7 (`-vV`, `--print …`, `--version`, `locate-project …`, `metadata --no-deps …`, `rustc -Z unstable-options --print …`). Cualquier otro argv es fallo |
| **P2** — `publish` reconstruye el recibo con **todos** los `cut-*.json` presentes, así que un `pass` viejo puede quedar emparejado con los hashes de fuentes de hoy; el driver no concilia `cut_status` con las selecciones ejecutadas | Aceptado. El driver limpia `target/m6-calibration` antes de la suite; cada corte publica su documento con `fail` si termina sin `pass` (guard en `Drop`); el driver exige que las claves de `cut_status` sean exactamente las selecciones ejecutadas y que cada documento lleve el `started_at` de esta ejecución |
| **P2** — archivo `.rs` no UTF-8: se cuenta la omisión pero no se empuja ningún `IncompleteReason`, así que el resultado sale `Complete` (o `Infrastructure` si `with_omissions` lo rechaza) | Aceptado. Nueva `IncompleteReason::NotUtf8File`; test de gateway con una captura que contenga un `.rs` inválido |
| **P2** — la admisión del digest M6 es global (`RustGateway::new`, `host_config`), no solo de la fase `Analyzer`; nada impide correr M1–M5 sobre la M6 y ADR-085 no lo decide | Aceptado **como decisión documentada, no como cambio de código**: es el patrón de la casa (ADR-077: la imagen M5 se admitió globalmente y «las tools M1 a M4 siguen calificadas contra sus propios digests»). ADR-085 lo escribe explícitamente y añade la limitación: ejecutar tools M1–M5 sobre la imagen M6 no está calificado por sus suites (que corren sobre sus propios digests en el `full`); la diferencia M5→M6 es aditiva (`/opt/analyzer`, `rust-src` bajo `/opt/rust`), y `docs/compatibility.md` la declara en M6-06 |
| P3 — `session.duration_ms` se sobrescribe con la duración de toda la llamada | Aceptado: campos separados `session.duration_ms` (sesión) y `call.duration_ms` |
| P3 — `send` solo acotado por el deadline global, no por el presupuesto de fase; un peer que no lee stdin consume el total y se publica como `NotReady` | Aceptado: `send` recibe `until` de fase; el exceso se clasifica `TimeoutInitialize`/`TimeoutQuery`, no `NotReady` |
| P3 — resultados de `kill`/`wait` descartados; `Killed` afirmado, no verificado | Aceptado: se registran `kill_error`/`reap_error` en el outcome y, si el reap no confirma, `stop = KillUncertain` (la garantía sigue siendo la ausencia del contenedor) |
| P3 — m6-07 afirma `exit_code.is_some()` de forma racy; también falla determinista el unit test `a_request_cut_short_by_end_of_stdout_is_an_eof_failure` (14/15) | Aceptado: clasificación determinista — si stdout llegó a EOF, `stop = Eof` aunque el kill posterior sea quien cierre; el corte m6-07 comprueba `evidence.killed` y el estado del contenedor (`State.ExitCode == 137`) por inspección antes del cleanup, no el código del cliente docker |
| P3 — m6-08 solo prueba `FramingRejected`, que cubre cabecera malformada y frame excesivo | Aceptado: dos variantes `FrameTooLarge`/`MalformedHeader` (el codec ya distingue `FrameLimit` de `MalformedHeader`), y el documento del corte registra el `Content-Length` observado |
| P3 — `analyzer_configuration` solo mira `files()`, no `directories()` | Aceptado |
| P3 — dos notas de corte prometen más de lo que prueban (m6-04 «no se creó», m6-03 «el corte de diagnostics») | Aceptado: redacción corregida; el oráculo determinista de build scripts se asigna a W07 (M6-03) en el registro |
| P3 — Python: `OUTPUT` derivado de env y usado en `mkdir`/escritura; `int(env)` sin refusal propia; ruta de Docker fija en `owned_docker_state` | Aceptado: ruta constante `target/m6-runtime-gate`; timeout con validación propia; `RUST_MCP_TEST_DOCKER` respetado |
| Duda del revisor — `set_verified` como bypass | Verificado por el orquestador: `analyzer_gateway.rs` solo **lee** `verified`/`calibrating` (línea 199) y no llama a `set_verified`; no hay bypass en producción |

## Estado

Sin P0/P1. Cuatro P2 y ocho P3 aceptados; corrección en W04b (mismo
worker family, Opus 5) con re-ejecución completa de la calibración y recibo
nuevo (`01-calibration.json` cambia porque cambian las fuentes). Hasta
entonces M6-01 sigue **In progress** y el diff W04 no se integra.
