# M3 — registro de delegación (orquestador Claude Fable 5.1)

Sesión principal: Claude Code 2.1.261, modelo `claude-fable-5-1` desde el inicio hasta el
mensaje del owner del 2026-09-06 (~03:25Z) «Continúa la ejecución con opus cambie el modelo con
este mensaje»; desde entonces el owner declara la sesión principal en Claude Opus 5 (el host
configura el modelo; el orquestador no puede atestarlo por sí mismo). La política de roles,
evidencia y aceptación no cambia con ese relevo.
CLI verificadas el 2026-09-05: `codex-cli 0.153.0` (catálogo `codex debug models`:
`gpt-6-astra`, `gpt-5.6-sol`, `gpt-5.6-terra`, `gpt-5.6-luna`, `gpt-5.5`, …; esfuerzos
low..ultra), `agy 1.1.27` (`gemini-3.8-flash-high`), `claude 2.1.261`
(`claude-opus-5`, `claude-sonnet-5`). El modelo observado en metadata se registra en
cada `meta.json`/`stdout.txt`; un identificador de catálogo no es atestación del backend.

Cada paquete tiene un directorio `<ID>/` con `prompt.md` (paquete completo enviado),
`command.txt` (argv sin el cuerpo del prompt), `started-utc.txt`, `stdout.txt`
(eventos JSONL de Codex / JSON de Claude y AGY), `stderr.txt`, `meta.json`
(modelo solicitado, esfuerzo, inicio/fin UTC, exit code) y `last-message.md` o
`report.md` (entrega). Los intentos fallidos se conservan (`attempt-*`).

Política de sandbox observada: Codex `workspace-write` no puede crear refs Git
(`.git/refs/heads/*.lock: Operation not permitted`) ni conectar al socket Docker;
las sesiones Git del integrador usan `sandbox_workspace_write.writable_roots=[".git"]`
con lista cerrada de comandos, y las sesiones de gate usan
`sandbox_workspace_write.network_access=true` (probe P00). AGY headless deniega
`read_url` y comandos; los paquetes AGY son locales y de solo lectura de archivos.

| ID | Rol | Modelo solicitado / CLI / esfuerzo | Resultado | Disposición del orquestador |
| --- | --- | --- | --- | --- |
| S00-baseline | Inspección de baseline, mapa de interfaces, propuesta de provisioning | gpt-5.6-sol / codex / high | Completo; M2 574 inputs verificados live; rama no creada por sandbox | Aceptado; la rama la creó S00b |
| S00b-branch | Crear `ai/m3-quality` desde `52396184` | gpt-5.6-sol / codex / medium | Rama creada y comprobada | Aceptado |
| R00-rmcp-tasks | Investigación SEP-2663 en rmcp 3.2.0 (fuente fijada) | gemini-3.8-flash-high / agy / high | Informe con citas a fuente; sin `tasks/result`/`tasks/list`; extensión por capability | Aceptado como evidencia de D06; corroborado por A06 |
| R01-plugins | Formatos/versiones/exit codes de los cuatro plugins | gemini-3.8-flash-high / agy / high | Intentos 1–2 denegados (`read_url`, comando); intento 3 completo sobre fuentes oficiales descargadas por el orquestador | Aceptado con sus «unverified» explícitos |
| P00-docker-probe | Sonda de acceso al daemon Docker desde Codex | gpt-5.6-luna / codex / low | REACHABLE con `network_access=true` | Informativo |
| A06-d06-adr | ADR-060 (D06) + spike wire rmcp | gpt-5.6-sol / codex / high | ADR-060 Proposed; spike 5/5, clippy limpio | En revisión V06 |
| A17-d17-adr | ADR-061 (D17) | gpt-5.6-terra / codex / high | ADR-061 Proposed | Revise (V17); revisión A17b en curso |
| A18-d18-adr | ADR-062 (D18) | claude-sonnet-5 / claude / high | ADR-062 Proposed | Revise (V18); revisión A18b en curso |
| V17-adr061-review | Revisión independiente ADR-061 | claude-opus-5 / claude / high | Revise; F1–F17 | Todas las «fix now» aceptadas; F2 decidido: frontera uid+state-root+root concedido, sin secreto |
| V18-adr062-review | Revisión independiente ADR-062 | claude-sonnet-5 / claude / high (sesión distinta de A18) | Revise; F1–F7 | Aceptadas; F1 se resuelve por calibración del binario fijado; F3 decisión conjunta `ArchiveBundle` |
| V06-adr060-review | Revisión independiente ADR-060 + spike | claude-opus-5 / claude / high | Revise; F01–F14 (P2/P3) | Todas aplicadas en A06b; permiso de job = permiso worker ADR-030; orphan por no-entrega |
| A17b-adr061-revise | Revisión de ADR-061 según V17 | gpt-5.6-terra / codex (resume) / high | ADR-061 revisado (b798a32f…) | Aceptado por el orquestador como base de D17 |
| A18b-adr062-revise | Revisión de ADR-062 según V18 | claude-sonnet-5 / claude / high | ADR-062 revisado (157d4d5c…) | Aceptado como base de D18; §11 provisional hasta calibrar el binario |
| P01-provisioning | Fixture de provisioning M3 + ADR-063 | gpt-5.6-luna / codex / high | sources.json/provision.py/Dockerfile/verify.py/README + ADR-063 | Aceptado; ejecución en P02 |
| A06b-adr060-revise | Revisión de ADR-060 + spike según V06 | gpt-5.6-sol / codex (resume) / high | ADR-060 revisado (5355954b…); spike 5/5 con matriz 5×2 | Aceptado como base de D06 |
| P02-provision-run | Construcción y verificación de la imagen M3 (descargas autorizadas) | gpt-5.6-luna / codex / high (+network_access) | Imagen `sha256:384a1742…`; verify 47/47; help de plugins capturado; 4 defectos de fixture corregidos en el camino | Aceptado; calibración del gateway pendiente (integrador); ADR-063 debe corregir la ruta `llvm-tools-preview/` |
| I00-jobexecutor | D06 en código: job model, JobExecutor/registry, handlers tasks, DTO nextest | gpt-5.6-sol / codex / high | Tipos/ejecutor/registro/watchdog, handlers tasks latentes, DTO con fingerprint de schema, 62+140 tests, protocolo 39 | Aceptado; revisión V00 en curso; integración I02 |
| I02-integration-nextest | Integración M3-01: imagen nueva + recalibración, tool 19, límites de job, runtime Docker, receipts | gpt-5.6-sol / codex / high (+network_access para el socket Docker) | Tool 19 registrada, protocolo 40/40, gate Rust 20/20 con imagen P02; `docker cp` sustituido por export cerrado; M3 runtime bloqueado: nextest exit 101 por seccomp (`socketpair AF_UNIX SOCK_STREAM`) — recibo fallido conservado | Autorizado perfil `seccomp-rust-quality.json` + ADR-064 (precedente ADR-056) con controles negativos y revisión de contención; continuación I02b |
| I02b-v00-fixes | Correcciones V00, seccomp ADR-064, requalificación runtime M3-01, Stage 1 + CLI `quality-artifacts` | gpt-5.6-sol / codex (resume) / high (+network_access) | V00-01..13 corregidos con tests; runtime M3 19/19 (407,8 s); Rust security 20/20; workspace 985 pass/81 ignored; protocolo 40/40; Stage 1 + CLI cableados; intento fallido conservado | Aceptado: M3-01 calificado en modo síncrono; ADR-064 Proposed pendiente de aceptación de contención |
| D01-docs-m3 | Sincronización documental M3 (README/CHANGELOG/SECURITY/arquitectura/seguridad/compatibilidad/cliente/estado/ADR-061 notas/ADR-063 rutas) | gpt-5.6-luna / codex / medium | 11 archivos actualizados; enlaces verificados; `docs/tools.md` intacto (ya lo tenía el integrador) | Aceptado; se revisará al cierre |
| I03-coverage | M3-03 `rust.coverage` (adapter/aplicación/MCP, sin Docker) | gpt-5.6-terra / codex / high | Parcial: contratos, parser JSON LLVM con dedupe, variantes/argv, tool 20 fail-closed (`Unavailable`), fixtures y tests Docker ignorados; bloqueado por el verificador de mounts (fuera de su ownership) | Aceptado como parcial; continuación I03b tras I02b reutilizando el volumen de reporte/export de nextest |
| I04-semver | M3-04 `rust.semver.check` (adapter/aplicación/MCP, sin Docker) | claude-sonnet-5 / claude / high (worker) | Pausa honesta a los 12 min: encontró brazos `SemverCheck` en `rust_gateway.rs` escritos por otro worker (desbloqueo de compilación tras añadir la variante temprano) | Aclarado: nadie más tiene M3-04; continuación I04b con ownership exclusivo de esos brazos |
| I04b-semver-continue | Continuar M3-04 | claude-sonnet-5 / claude / high (worker, sesión nueva) | Interrumpido a los 11 min por límite de sesión de la CLI Claude (`session limit · resets 12:20am`); dejó tipos de dominio y parser parciales | Limitación registrada; M3-04 reasignado al integrador (I04c) |
| I03b-coverage-continue | Completar M3-03 con ejecución real y calificación Docker | gpt-5.6-terra / codex (resume) / high (+network_access) | Camino síncrono real, Stage 0/1 y proyección de job implementados; calificación Docker bloqueada por edición concurrente de mutation en `rust_gateway.rs`; intento fallido conservado en `M3-03-runtime-attempt1.json` | Aceptado como implementación; la calificación Docker de coverage se agrupa en la ventana Q01 |
| I05-mutation | M3-05 `rust.mutation.test` (contención de código hostil) | claude-opus-5 / claude / high (worker) | Vertical completo no-Docker: 6 fases/2 volúmenes/3 exporters, baseline obligatorio, oráculos solo de `mutants.out`, 44 tests nuevos, tool 22, 10 tests Docker `#[ignore]`; tres desviaciones documentadas | Aceptado; calificación Docker en Q01 |
| F02-mutation-fixtures | Fixtures y oráculos de M3-05 | gpt-5.6-luna / codex / medium | 6 workspaces + canario, tabla de oráculos y containment, inventario con SHA-256 | Aceptado |
| Q01-docker-qualification | Ventana Docker única: coverage/semver/mutation + calibración + security gate | gpt-5.6-sol / codex (resume) / high (+network_access) | nextest 19/19, semver 18/18, mutation 10/10, security 20/20; coverage bloqueado (volumen persistente sin ejecución); 4 intentos conservados; exit codes de nextest/semver/mutants calibrados contra los binarios fijados | Aceptado; coverage escalado y resuelto por ADR-065 |
| S01-store-flake | Diagnóstico y corrección del test intermitente de expiración del store | claude-opus-5 / claude / high (worker) | Causa raíz: resolución de un segundo en `UtcInstant` dejaba una ventana < 1 s para fsyncs; se añadió fuente de reloj inyectable de test; 90/90 iteraciones | Aceptado |
| Q02-probe | Implementación de ADR-065 (volumen dedicado de coverage) | gpt-5.6-sol / codex (resume) / high | ADR-065 escrito e implementado con verificador y fingerprint; Docker inalcanzable desde el sandbox de Codex (el clasificador de permisos del host ya no admite el flag de red) | Aceptado como implementación; la calificación pasa al validador Claude |
| V-CONTRACTS | Revisión independiente de contratos de las cuatro tools nuevas | claude-sonnet-5 / claude / high | Revise: F1/F2 P1 en `rust.coverage` (modo `task` y código de remediación divergentes de ADR-060), F3 enum abierto, F4 falta test de cinco versiones, F5 doc sin la limitación | Todas aceptadas; se corrigen en Q02b antes de calificar |
| Q02b-docker-validator | Validador con Docker: corrige F1–F5 y ejecuta el gate M3 completo (55 selecciones) y el de seguridad | claude-opus-5 / claude / high (worker) | en curso | Propietario único de Docker |
| I06-tasks-implementation | M3-02: camino asíncrono negociado, matriz D06 sin Docker, harness de clientes y presupuestos | gpt-5.6-sol / codex (resume) / high | entregado; G4 pendiente | Calificación Docker/clientes y medición 30/30 la ejecuta el validador |
| S01-store-flake | Diagnóstico y corrección del test intermitente de expiración del store | claude-opus-5 / claude / high (worker) | en curso | Solo archivos del store |
| I04c-semver-complete | Completar M3-04 (sin Docker en esta ventana) | gpt-5.6-sol / codex (resume) / high | Vertical completo: captura baseline→candidate con revalidación, doble volumen RO, parser acotado, tool 21, 18 selecciones Docker cableadas sin ejecutar; suite no-Docker 1.042 pass | Aceptado; calibración en Q01 |
| F02-mutation-fixtures | Fixtures y oráculos de M3-05 | gpt-5.6-luna / codex / medium | en curso | — |
| V00-jobexecutor-review | Revisión independiente del núcleo D06 (I00) | claude-opus-5 / claude / high | Revise: V00-01 P1 (deadline de trabajo aplicado en Cleanup sobrescribe resultado), V00-02..07 P2, 08..13 P3 | Todas dispuestas «fix» para Sol en I02b tras la integración; V00-03/05/06/12/13 son obligaciones de integración |
| I17-artifact-store | ADR-061 en código: descriptor, ports, store APFS, Resources nuevo esquema | gpt-5.6-terra / codex / high | Parcial honesto: tipos, ports, URI, skeleton; oráculos placeholder | Aceptado como parcial; continuación I17b |
| I17b-artifact-store | Completar ADR-061 (resume Terra) | gpt-5.6-terra / codex (resume) / high | Parcial: solo floor fstatfs; se detuvo ante bloqueos compartidos | Reasignado: I17c a Claude Opus 5 (implementación crítica delimitada) |
| V17c-store-review | Revisión independiente del store ADR-061 (sesión Opus distinta de la implementadora) | claude-opus-5 / claude / high | Revise estrecho: sin P0/P1; F1–F3 P2 (probe del state root, reclamación de expirados), F4–F9 P3 | Todas «fix» en I17d; F5 decidido: cuarentena salvo marcador de truncado; F6 obligación del integrador |
| I17d-store-fixes | Aplicar V17c al store | claude-opus-5 / claude / high (worker, sesión nueva) | F1–F9 y tests faltantes aplicados; 27+4 nativos ignorados; M2 20/20; clippy limpio | Aceptado; el integrador debe cablear Stage 1, CLI `quality-artifacts recover|prune` y cargar entradas de archive al presupuesto de miembros |
| I17c-artifact-store-opus | Completar ADR-061 (store durable, oráculos, recover/prune) | claude-opus-5 / claude / high (worker) | 13/13 ítems; 18+3 nativos ignorados ejecutados en APFS; oráculos mapeados; `cargo fmt` de paquete reformateó 5 archivos ajenos (solo formato) | Aceptado pendiente de revisión independiente V17c; refactor de `StateRoot` M2 diferido (fuera de ownership) |
| I01-nextest-adapter | RustCommand nextest, parser JUnit acotado, egreso JUnit/logs | claude-sonnet-5 / claude / high (worker) | Variante cerrada, parser JUnit con tests hostiles, fases gateway (config ingest + `docker cp` export) sin Docker verificado; 7 open issues (fingerprints, tope 60 s, exit codes sin calibrar) | Aceptado como implementación pendiente de calibración Docker; el integrador cierra fingerprints/límites |
| F01-fixtures-docs | Fixtures nextest/hostiles, M3-matrix.md, Status/índice ADR-060..063 | gpt-5.6-luna / codex / medium | en curso | — |

Decisiones del owner registradas el 2026-09-05 (respuestas al orquestador): aprovisionar
los cinco componentes (cargo-mutants 27.1.0 desde fuente); aceptar la postura persistente
de D17; integrar al final mediante rama + PR con `gh`, supervisar checks y hacer merge
si pasan (sin tag ni release).
