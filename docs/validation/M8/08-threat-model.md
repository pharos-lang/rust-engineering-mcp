# M8-08 — Threat model 0.8.0 / 1.0 (macOS ARM64)

Estado: **threat model completo, previo a la auditoría independiente**. Fecha:
2026-09-14. Rama `ai/m8-stabilization`. Worker W22 (Claude Opus 5, High,
solo lectura del código). Encargo: plan
[M8 §M8-08 y §«Migración, recovery y seguridad»](../../roadmap/m8-stabilization.md),
G2/G3 de [m2-m8](../../roadmap/m2-m8.md). Registro de aceptación:
[ADR-089](../../adr/ADR-089-residual-risk-register.md). Modelo público:
[security-model](../../security-model.md#riesgos-residuales-10).

Este documento **no es una auditoría**. Es la entrada que la auditoría
independiente de M8-08 revisa. Esa auditoría es una revisión de modelo
(Opus 5 High, read-only, alcance declarado), no una auditoría humana ni un
pentest (RR-01).

## 0. Alcance y método

- **Producto:** `rust-engineering-mcp` 0.8.0, 36 tools (31 `stable`, 5
  `preview`, [ADR-086](../../adr/ADR-086-deprecation-and-freeze-policy.md)),
  15 subcomandos CLI y 10 formatos en disco ([censo M8-01](01-census.md)).
- **Host positivo único:** macOS 26 ARM64/APFS con gateway Docker Linux ARM64
  ([ADR-087](../../adr/ADR-087-1.0-host-scope.md)). Linux/Windows fallan
  cerrados y no se modelan como positivos (RR-02).
- **Transporte:** stdio local. HTTP/remoto está Deferred (M7) y fuera de alcance.
- **Método:** cada control cita `archivo:línea` o un test/oráculo que existe en
  el árbol del 2026-09-14. La columna *Oráculo* usa cuatro clases:
  - **N** — oráculo nativo: test sobre APFS, Docker/imagen aprobada o
    `sandbox-exec` real, ejecutado por una etapa `full` de `scripts/gate.py:188-208`
    o por la suite macOS del crate.
  - **U** — oráculo unit/contract/protocol sin frontera de SO (suficiente cuando
    el control es cálculo puro, parsing o configuración).
  - **H** — evidencia histórica o de configuración (recibo previo, YAML
    revisado); no se re-ejecuta en este corte.
  - **—** — sin oráculo. Se declara y se traslada a riesgo residual.
- **Severidad residual** (tras mitigación, para el alcance declarado):
  **Alta** = compromiso del host o pérdida de datos plausible sin acción del
  operador; **Media** = impacto relevante que requiere una condición no
  controlada por el producto (kernel, dependencia, mismo usuario, clave);
  **Baja** = disponibilidad, precisión de evidencia o límite ya declarado.

## 1. Actores y activos

| Actor | Confianza | Capacidades relevantes |
| --- | --- | --- |
| Operador del host | Confiable para conceder autoridad; puede equivocarse | Elige argv de `serve` (roots, grants, imagen, state root, catálogo, trust, políticas por SHA-256), Docker Desktop y retención |
| Cliente MCP / agente (modelo) | No confiable para autoridad | Envía JSON-RPC arbitrario, cancela, cierra stdin, repite IDs, lee Resources/Tasks |
| Proyecto analizado | Hostil por defecto | `build.rs`, proc macros, tests, benches, harness, `rust-analyzer.toml`, `.cargo/config*`, symlinks/hardlinks, bytes y nombres de archivo |
| rust-analyzer en el guest | Peer LSP potencialmente comprometido por el proyecto | Frames, respuestas tardías, peticiones servidor→cliente, edits |
| Otros procesos del mismo uid | Fuera de la frontera (mismo principal) | Leen/escriben el checkout, state root, trust y artifacts |
| Publisher del catálogo | Confiable solo si el host instala su clave | Firma bundles Ed25519 |
| Upstreams (crates.io, GitHub Actions, Hugging Face E5, ORT, imágenes base) | Parcialmente confiables, fijados por pin | Código y binarios compilados en CI y en la imagen |
| GitHub (Actions/OIDC/Releases) y SonarCloud | Infraestructura de terceros | Ejecutan workflows, emiten attestations, guardan `SONAR_TOKEN` |

| Activo | Por qué importa |
| --- | --- |
| Checkout del usuario y sus secretos | Integridad del código; confidencialidad de lo que contenga |
| Host macOS (archivos del uid, red, credenciales) | Un escape del guest o de un subprocess lo compromete |
| State root: journals M2, artifacts M3, floor/trust/catálogo | Integridad de mutaciones, anti-rollback, evidencia privada |
| Contratos y resultados MCP | Un falso `passed`/`clean` induce decisiones erróneas del agente |
| Evidencia de validación y receipts en el repo | Base de la calificación; puede filtrar credenciales de clientes |
| Pipeline de release y attestations | Cadena source → tag → run → digest del artifact |

## 2. Trust boundaries

| ID | Frontera | Lado confiable | Lado no confiable |
| --- | --- | --- | --- |
| B1 | Host / operador → servidor | argv validado de `serve`, Docker CLI por ruta y socket explícitos | Configuración parcial, rutas dentro de roots, daemon/VM Docker |
| B2 | Cliente MCP / agente → servidor | Admisión, schemas cerrados, grants del host | Todo el tráfico JSON-RPC |
| B3 | Proyecto hostil → captura, Cargo, rust-analyzer | Captura no-follow y bytes propios, argv cerrado | `build.rs`, proc macros, tests, benches, config de proyecto, peer LSP |
| B4 | Guest Docker → host | Flags del contenedor, seccomp por fase, cleanup unido | Todo proceso dentro del contenedor |
| B5 | Catálogo / bundle firmado → store | Trust del host, floor persistido | Bytes del bundle, SQLite, red de sync |
| B6 | Modelo E5 / ORT / LanceDB → proceso | Hashes fijados en código, `memory://` | Bytes del modelo, índice importado |
| B7 | Evidencia / receipts / artifacts / journals | Store owner-bound, formatos versionados | Objetos en disco, locators, logs, evidencia de clientes |
| B8 | Pipeline de publicación GitHub / OIDC | Workflow en tag, OIDC, branch protection | Acciones de terceros, PRs de forks, tokens |

## 3. Amenazas, controles y riesgo residual por frontera

### B1 — Host / operador

| Entrada hostil / abuso | Control existente | Oráculo | Residual |
| --- | --- | --- | --- |
| Grants de escritura sin runtime, root de escritura fuera de los de lectura o journal dentro de un root | `serve` se invalida: `crates/mcp-server/src/host_config.rs:263-282` (escritura exige grupo Docker y root ⊂ lectura) y `:283-293` (`rust-mcp-mutations-v1` fuera de toda root) | **U** tests del módulo `host_config.rs:297+` | Baja |
| Imagen no aprobada o tag mutable | Lista cerrada de digests `host_config.rs:177-184`; comprobación por llamada: `performance_native.rs:2731` `m5_tools_refuse_every_runtime_but_the_qualified_one`, `analyzer_native.rs:1076` `m6_analyzer_refuses_every_runtime_but_the_m6_image`; digest del ejecutable Docker `crates/execution-adapter/src/lib.rs:517` | **N** etapas `m5-runtime`, `m6-runtime` y `m4-tampered-plugin` (`scripts/test-m4-tampered-plugin.py:2-8`, `gate.py:197`) | Baja; un daemon hostil queda fuera (RR-05) |
| Profiling concedido sin gateway o con valor abierto | Valor cerrado `user-space-sampling` `host_config.rs:91-97`; sin grupo `--rust-*` la invocación es inválida `:256-262` | **N** fila de denegación de `performance_native.rs:2015` ([recibo M5-03](../M5/03-runtime.json)) | Baja; sin revocación en caliente (RR-15) |
| Política de seguridad, vendor o captura dentro de un root del proyecto (el proyecto reescribiría bytes confiables) | Rechazo de configuración `host_config.rs:206-214`, `:220-235`, `:240-255`; digest esperado de la política `crates/execution-adapter/src/security_policy.rs:26` | **U** `security_policy.rs:132-154` | Baja |
| State root, journal o trust con permisos laxos | State root sin escritura group/other y del uid efectivo `crates/project-adapter/src/filesystem/macos/state_primitives.rs:42-51`; directorios `0700` y archivos `0600` con `nlink == 1` `:53-78`; journal `mutation_state.rs:63`, `filesystem/macos/mutation.rs:191,206`; trust `catalog_store.rs:128,190,218` | **N** `commit_preserves_private_mode_and_extended_attributes` (`crates/project-adapter/tests/mutation_store.rs:810`) y **U** `catalog_store.rs:818-825` | Media: ACLs no inspeccionadas (RR-08) |
| Daemon Docker, VM de Docker Desktop o socket del host comprometidos | Solo reducción: `env_clear` y `--config` propio `lib.rs:333-346`; ninguna garantía frente a un daemon hostil (`test-m4-tampered-plugin.py:5-7` lo declara) | **—** | Media (RR-05) |

### B2 — Cliente MCP / agente

| Entrada hostil / abuso | Control existente | Oráculo | Residual |
| --- | --- | --- | --- |
| Líneas gigantes, EOF parcial, clientes lentos, flood | Línea ≤ 1 MiB `crates/mcp-server/src/stdio/budget.rs:15`, deadline de frame 10 s `:17`; 16 requests/notifications/sends `stdio/admission.rs:24-25,107-112` | **U** `budget.rs:301-303`; tests de `protocol.rs` | Baja: cancelaciones suprimidas por rmcp retienen permisos hasta reconectar (RR-18) |
| Ampliar autoridad por argumentos, URI, `taskId`, fingerprint o annotations | Autoridad solo del host (G2); errores uniformes `stdio/tasks.rs:25` (`task unavailable`); `protocol.rs:521` (open sin autoridad), `:2158` (Resources opacos, autoridad inválida uniforme), `:2989` (mutaciones cerradas y denegadas por defecto), `:1952` y `:2127` (apply sin grant denegado, también bajo contención) | **U** protocolo en las cinco revisiones MCP | Baja |
| Cancel/EOF para dejar trabajo huérfano o recuperar capacidad antes del cleanup | Worker unido hasta cleanup (security-model §M3-02); `crates/mcp-server/tests/inspection_runtime.rs:1126,1733,2572`; `nextest_runtime.rs:439`; revocación `inspection_runtime/security.rs:620` | **N** `rust-security`, `m3-runtime`, `m4-runtime` | Baja |
| Commit o replay con plan ajeno, expirado o de otro kind | Replay solo con ID/digest/key exactos ([ADR-059](../../adr/ADR-059-terminal-plan-retirement-and-durable-replay.md)); `terminal_plan_runtime::…` y `mutation_runtime::manifest_preview_commit_conflict_reopen_and_restart_receipt` (`scripts/test-m2-runtime.py:32,38`) | **N** `m2-runtime` | Baja |
| Flags o argv arbitrarios | Schemas cerrados congelados (etapa `contract-freeze`, `gate.py:186`); argv del perfilador sin paths del peer `performance_gateway.rs:3073` | **U** | Baja |
| Agotar recursos lanzando jobs o artifacts | Un worker `stdio/workers.rs:226`; cuatro planes y 64 MiB (security-model §Escritura M2); cuotas del store `quality_artifact_store.rs:911`, `:2061` | **N** tests APFS del store | Baja |

### B3 — Proyecto hostil (build.rs, proc macros, tests, benches, rust-analyzer)

| Entrada hostil / abuso | Control existente | Oráculo | Residual |
| --- | --- | --- | --- |
| `build.rs`/proc macro que usa red, lee el host o escapa durante check/Clippy | Todo corre en el guest (B4); calibración `rust_calibration.rs:199` `actual_build_script_and_proc_macro_containment` y `:258` (Clippy), aserciones de red/entorno/filesystem/procesos/cgroup `:343` | **N** `rust-security` (`scripts/test-rust-execution.py:112-113`) | Media solo por kernel/runc (RR-05) |
| Tests que dejan descendientes desacoplados | `rust_calibration.rs:129`; `rust_gateway::test_runtime::actual_test_runtime_containment_and_descendant_cleanup` (`test-rust-execution.py:116-117`); fixture `leaky` `nextest_runtime.rs:370` | **N** | Baja |
| Proc macro que falsifica eventos de Cargo o salida de herramienta | `actual_proc_macro_forgery_cannot_hide_later_cargo_failure` (`test-rust-execution.py:122-123`); `mutation_runtime.rs:385` (salida forjada no confiable) | **N** | Baja (RR-13) |
| `cargo fix` con proc macro que muta el manifest o escribe fuera | Staging guest y publisher host acotado (ADR-053/054); perfil `seccomp-rust-fix.json:178-201` añade solo `socket(AF_INET, SOCK_STREAM)` para loopback con `--network=none`; `rust_applied.rs:939`; `fix_hostile.rs:121,180` | **N** `m2-runtime` (`test-m2-runtime.py:35`) | Baja: TCP loopback interno del namespace (RR-05) |
| `.cargo/config*` que redirige sources, linker o rustflags | Predicado `crates/domain/src/security.rs:39`, aplicado antes de volumen `performance_gateway.rs:101`; Miri rechaza configuración de fuente (fixture `miri_native.rs:243`) | **N** `miri_native.rs:144`; **U** `security.rs:685-709` | Baja: predicado por nombre |
| `rust-analyzer.toml` que reactiva build scripts u `overrideCommand` | Rechazo en captura y defensa en gateway `analyzer_gateway.rs:412-430` | **N** `analyzer_native.rs:1492` | Baja |
| Peer LSP hostil: frame gigante, crash, nunca quiescent, cancelación | Codec acotado y deadlines ([ADR-084](../../adr/ADR-084-rust-analyzer-runtime-and-lsp-lifecycle.md)); `env_clear` `lsp_session.rs:879` | **N** `analyzer_native.rs:1752,1680,1558,1628` | Baja; ausencia de procesos por muestreo (RR-03) |
| Flood de salida, JUnit/tar/HTML hostiles, symlink en ruta fija | `nextest_runtime.rs:400,419`; `coverage_runtime.rs:402`; `mutation_runtime.rs:547` | **N** `m3-runtime` | Baja |
| Symlink/hardlink o sustitución del checkout durante la captura | `NOFOLLOW_ANY | RESOLVE_BENEATH` en cada open `crates/project-adapter/src/filesystem/macos.rs:27-29,63`; tests `crates/project-adapter/tests/filesystem.rs:211,226,247,398,455` | **N** suite macOS del crate | Baja: FIFO/device node y captura no atómica (RR-14) |
| Binario perfilado que pre-crea artifacts o deja nietos | Vaciado de PID namespace, `O_EXCL`, reconciliación de manifest ([ADR-074 §5.1](../../adr/ADR-074-profiling-capability-and-containment.md)); `performance_native.rs:2015` | **N** selecciones `profile-descendant-drained` y `profile-precreated-artifact-refused` ([recibo](../M5/03-runtime.json)) | Baja |
| Benchmark que falsea sus propios números | Ninguno por diseño; se declara origen no autenticado (ADR-073 §6) | **—** (no se anuncia como capability) | Baja (RR-13) |
| Secretos del host (canario) filtrados a HTML, diffs, logs o diagnósticos | Captura solo de roots concedidas; normalización M4 | **N** `m4_privacy_runtime.rs:59`, `inspection_runtime/security.rs:702`, `mutation_runtime.rs:430` | Media: los secretos *del propio source* sí se retienen (RR-07) |

### B4 — Guest Docker → host

| Entrada hostil / abuso | Control existente | Oráculo | Residual |
| --- | --- | --- | --- |
| Syscalls de escape (mount, unshare, ptrace, bpf…) | Los seis perfiles tienen `"defaultAction": "SCMP_ACT_ERRNO"` en la línea 2 (`crates/execution-adapter/src/seccomp*.json`) y ninguno lista `mount`, `unshare`, `ptrace` ni `bpf` (114–128 syscalls según perfil, inspección estática de este worker); verificación del perfil aplicado `rust_applied.rs:179`; rechazo de `unconfined`, malformado o syscall extra `applied_tests.rs:315-327` | **N** `crates/execution-adapter/tests/gateway.rs:185` (`docker-security`) y calibraciones `rust-security` | Media (RR-05) |
| Red desde el guest | `--network=none` `rust_gateway.rs:1322`, `performance_gateway.rs:786`, `lib.rs:446`; los perfiles rust/quality/profile no permiten `socket` (solo `socketpair`, `seccomp-rust-quality.json:180` para AF_UNIX stream) | **N** `rust_calibration.rs:199,343`; `nextest_runtime.rs:211`; **U** `capabilities.rs:409` | Baja |
| Fork bomb, memoria, CPU | `--pids-limit=128 --cpus=1 --memory=1g --memory-swap=1g` `rust_gateway.rs:1328-1331`; tmpfs `/work` 512 MiB y `/tmp` noexec 64 MiB `:1335-1336` | **N** `rust_calibration.rs:83` `resource_limits_are_actually_enforced` | Baja |
| Escalada de privilegios | `--cap-drop=ALL`, `no-new-privileges`, `--read-only`, `--ipc=private`, `--cgroupns=private` `rust_gateway.rs:1322-1327`; usuario por fase `:1343` | **N** calibraciones (`rust-security`, `m4-runtime`: `security_native.rs:8,38`) | Media (RR-05) |
| Cleanup incierto para reutilizar un gateway sucio | `rm --force` + verificación de ausencia, si no cuarentena `lib.rs:394-419`; ejecución bloqueada si está en cuarentena `:503-513`; `rust_gateway.rs:904` | **N** `gateway.rs:96,142`; `inspection_runtime.rs:1126` | Baja; `kill -9` del servidor deja objetos (RR-15) |
| Ampliación por profiling | Solo la fase `ProfileRun` usa el perfil profile `performance_gateway.rs:3289`, que añade solo `perf_event_open` `:3348` | **N** [probe](../M5/profiling-capability-probe.json) y `performance_native.rs:2015`; **U** tests citados | Baja |
| Vulnerabilidad de kernel LinuxKit, runc o Docker Desktop | Ningún control propio | **—** | Media (RR-05) |

### B5 — Catálogo / bundle firmado

| Entrada hostil / abuso | Control existente | Oráculo | Residual |
| --- | --- | --- | --- |
| Bundle falsificado o alterado | Firma Ed25519 con prefijo de dominio verificada antes del parsing JSON `crates/catalog-adapter/src/bundle.rs:142,174-189`; versión cerrada `:200` | **U** `bundle/tests.rs:400` | Baja |
| Rollback a una secuencia anterior | `SequenceFloor::permits` `bundle/floor.rs:106-108`; floor reservado antes de activar `crates/mcp-server/src/catalog_cli.rs:274-286` | **N** `crates/project-adapter/tests/catalog_store.rs:432,474` y prueba con dos binarios `03-rollback.json` (c) | Baja: el dueño que borra todo el estado lo resetea (RR-08) |
| SQLite hostil (esquema, `user_version`, triggers) | Validación de esquema y ledger; sin SQL del caller ([03-formats-analysis §5](03-formats-analysis.md)) | **U** `catalog-adapter/src/tests.rs:68` (doce mutaciones hostiles, SQLite real) | Baja |
| Descarga oculta o red durante tools | Sync solo por CLI, `https_only`, `no_proxy`, sin redirects `crates/mcp-server/src/catalog_sync.rs:101-103`, hostname canónico `:39,75` | **N** bajo `(deny network*)`: `scripts/test-catalog.py:27-29`, `catalog_cli.rs:226`, `catalog_status.rs:342` | Baja |
| Trust sustituido o permisivo | `0600` bajo padre `0700` `catalog_store.rs:128,190,218,313` | **U** `catalog_store.rs:818-825` | Media: ACLs (RR-08) |
| Clave de publisher comprometida o publisher malicioso elegido por el host | Ninguno técnico: la firma autentica al publisher que el host eligió, no el contenido (security-model §M1-10); no hay catálogo oficial ni clave de producción ([publication](../../publication.md)) | **—** | Baja (RR-11) |

### B6 — Modelo E5 / ORT / LanceDB

| Entrada hostil / abuso | Control existente | Oráculo | Residual |
| --- | --- | --- | --- |
| Modelo o tokenizer sustituidos | Revisión, tamaños y SHA-256 fijados en código `crates/semantic-adapter/src/model.rs:4-31`, verificados antes de parsear `:40-54` | **N** `semantic-adapter/tests/local.rs:25`; **U** `model.rs:70` | Baja: el upstream fijado no se audita (RR-11) |
| ORT dinámico o descargado | `download-binaries` y `load-dynamic` prohibidos `deny.toml:22`; el arnés exige un único `libonnxruntime.a` `scripts/test-semantic.py:11-16` | **U** etapa `deny` (`gate.py:191`) | Baja |
| Índice LanceDB envenenado | Derivado y en `memory://` `semantic-adapter/src/index/persistence.rs:237`; SQLite rehidrata y filtra cada candidato | **N** `crates/mcp-server/tests/crate_search.rs:465` | Baja |
| Telemetría o red desde ORT | ORT configurado sin telemetry (security-model §Modelo e índice) | **N** `test-semantic.py:46-49` bajo `(deny network*)` | Baja: el deny es calibración del gate, no enforcement del producto |

### B7 — Evidencia, receipts, artifacts y journals

| Entrada hostil / abuso | Control existente | Oráculo | Residual |
| --- | --- | --- | --- |
| Locator de artifact o Task usado como credencial | Binding uid + state root + root concedida; masking `stdio/tasks.rs:25`; objetos desconocidos a cuarentena | **N** `quality_artifact_store.rs:607` | Media: mismo uid (RR-08) |
| Journal corrupto, de versión futura o de un binario más nuevo | Sniff de formato y `RecoveryRequired` antes de efectos ([03-formats-analysis §2](03-formats-analysis.md)); preflight `crates/mcp-server/src/doctor.rs:186-239` | **N** `crates/project-adapter/tests/rollback_native.rs:63` y [03-rollback.json](03-rollback.json) (`status: passed`); `native_mutation.rs:3279` (permisos revocados) | Media: journal corrupto bloquea el store (RR-09) |
| Regresión de reloj para extender TTL | Watermark durable | **N** `quality_artifact_store.rs:1066` | Baja |
| Eventos de auditoría con paths, diffs o credenciales | Registro de campos cerrados, sin paths `crates/mcp-server/src/stdio/mutation/audit.rs:95-103` | **U** `audit.rs:166` | Baja |
| Credenciales de clientes versionadas en evidencia | `.gitignore:11-22`; `assert_no_credentials` `scripts/test-m3-clients.py:529-551`, invocado en `scripts/test-m8-clients.py:1084,1215` | **U** autocomprobación del arnés | Media: detección por nombre de archivo, no por contenido (RR-07) |
| Rutas home locales en la exportación pública | Sustitución por `<LOCAL_HOME>` `scripts/public-export.py:77` | **U** `scripts/test-public-export.py` | Baja |

### B8 — Pipeline de publicación GitHub / OIDC

| Entrada hostil / abuso | Control existente | Oráculo | Residual |
| --- | --- | --- | --- |
| Release desde una rama o tag no estable | `release-candidate.yml:23-29` exige `vX.Y.Z` de tipo tag; versión = tag `:70-86` | **H** [recibo público 0.1.0](../M1/17-public-release.json) | Baja |
| Robo de clave de firma | No existe clave privada en el repositorio: OIDC `release-candidate.yml:40-43,132-138`, verificación con signer workflow exacto `:140-157`; `permissions: contents: read` global `:10-11`; `contents: write` solo en el job draft `:171-172` ([publication](../../publication.md)) | **H** attestations verificadas en 0.1.0 | Media: la attestation acredita el workflow, no la reproducibilidad (RR-12) |
| Acción de terceros comprometida | Acciones fijadas por SHA (`release-candidate.yml:46`); Dependabot para `github-actions` (`.github/dependabot.yml`) | **—** | Media (RR-12) |
| PR de fork que exfiltra `SONAR_TOKEN` | Evento `pull_request` (no `pull_request_target`) y guard de fork `sonarcloud.yml:20-22`; herramientas Python con hashes `:47` | **—** (revisión de configuración) | Media: token de larga vida de un tercero (RR-12) |
| Merge sin revisión o force-push | `CODEOWNERS:2`; protección observada `strict`, `enforce_admins`, sin force-push ni borrado ([public-ci-live](../M1/public-ci-live-33928952807.json) `:43-55`) | **H** observación M1 no re-verificada | Media (RR-12) |
| Smoke del archive con inventario erróneo | `release-smoke.py:35-110` fija 36 tools y sus SHA-256 de schema | **H** | Baja: `release-candidate.yml:219` todavía exige `tools == 31`, así que un draft 0.8.x falla cerrado (issue abierto) |

## 4. Temas obligatorios del plan

### 4.1 Dependencias comprometidas

- **Controles:** `cargo audit` y `cargo deny` fijados en CI (`ci.yml:96-111`) y en
  el gate local (`gate.py:190-191`); `deny.toml:7-8` (`ignore = []`,
  `yanked = "deny"`), `:18-30` (crates y features prohibidas, p. ej. `hf-hub`,
  `download-binaries`, `remote`, `resolve-http`), `:33-35` (solo el índice de
  crates.io, git desconocido prohibido); dependencias del workspace con pins `=`
  y `Cargo.lock` verificado con `--locked` (`ci.yml`, `release-candidate.yml:62`);
  vendor LanceDB anclado por SHA-256 `scripts/verify-vendor.py:11,23,36`
  (etapa `vendor`, `gate.py:188`); imágenes guest y plugins por digest
  (`test-m4-tampered-plugin.py`).
- **Lo que NO se detecta:** una versión maliciosa sin advisory publicado; código
  malicioso en un `build.rs` de dependencia, que se ejecuta en CI y en el host
  del desarrollador (no en el runtime del producto); el gate local usa
  `cargo audit --no-fetch` con la base local (CI sí la actualiza);
  `paste 1.0.15` sigue en el lock (`Cargo.lock:4118-4119`) como aviso
  unmaintained visible (SECURITY.md §M0-09). Dependabot abre PRs semanales:
  cada subida requiere revisión de CODEOWNERS y recalificación (plan §post-M8).
  → **RR-10**.

### 4.2 Secretos en source y en evidencia

- **Controles:** entorno reconstruido (`env_clear` en `lib.rs:335`,
  `supervisor.rs:465`, `lsp_session.rs:879`, `analyzer_gateway.rs:1645`), sin
  `CARGO_HOME`/credenciales del host (security-model §Escritura M2); redacción
  literal byte a byte, solapada y entre chunks
  `crates/artifact-adapter/src/lib.rs:262-271` (tests `artifact-adapter/src/tests.rs:78,503,595`);
  supply-chain retiene clase y fingerprint de source, nunca la URL
  (security-model §M4); eventos M2 sin paths (`audit.rs:95-103`); stderr de
  rust-analyzer solo como tamaño y SHA-256 (security-model §M6); canarios del
  host ausentes de resultados (`m4_privacy_runtime.rs:59`);
  `assert_no_credentials` y `.gitignore:11-22,61-69` para evidencia de clientes.
- **Lo que NO se detecta:** secretos presentes en el propio source concedido
  (se retienen en coverage HTML, diffs de mutación, logs de Cargo y artifacts
  privados); nombres de símbolos o de archivos sensibles; tokens escritos
  dentro de transcripts o JSONL de evidencia (la comprobación es por nombre de
  archivo: `auth.json`, `installation_id`, `.netrc`, `.env`,
  `credentials.json`, `*.sqlite`); stderr del servidor bajo retención del host.
  No existe escaneo de secretos en CI (búsqueda sin coincidencias en
  `.github/`). → **RR-07**.

### 4.3 Poisoning de catálogo y modelo

- **Controles:** Ed25519 antes del parsing (`bundle.rs:174-189`), floor de
  secuencia que no retrocede ni con un binario anterior (`floor.rs:106-108`,
  [03-rollback.json](03-rollback.json)), trust `0600`/`0700`; SQLite validado
  contra el esquema esperado y sin SQL del caller; E5 por SHA-256 y tamaño
  (`model.rs:4-54`); LanceDB derivado en `memory://`, descartable y rehidratado
  por SQLite (`crate_search.rs:465`); ninguna tool adquiere assets (G2).
- **Lo que NO se detecta:** facts falsos firmados por un publisher que el host
  eligió; la seed pública de fixture (`[42; 32]`) no es trust root de
  producción; un modelo upstream sesgado solo altera candidatos, nunca facts;
  el dueño que restaura o borra todo el estado reinicia el floor. → **RR-11**, **RR-08**.

### 4.4 Escapes de containment

- **Controles:** seccomp por fase con `SCMP_ACT_ERRNO` (§B4), `--network=none`,
  `--pids-limit`, `--memory`/`--memory-swap`, `--cpus`, `--cap-drop=ALL`,
  `no-new-privileges`, rootfs read-only, `/source` read-only verificado,
  cancelación cooperativa con worker unido y cuarentena ante cleanup incierto
  (`lib.rs:394-419,503-513`); host macOS sin ejecución de código del proyecto.
  Deltas de perfil acotados: `socketpair` AF_UNIX stream (quality),
  `socket` AF_INET stream para loopback (fix), `perf_event_open` (una fase de
  profiling).
- **Lo que NO se detecta ni contiene:** 0-days del kernel LinuxKit, runc o
  Docker Desktop; un daemon Docker comprometido; efectos de canal lateral;
  procesos que vivan entre dos muestras de `docker top` (R1 de M6-01).
  Presupuestos del proceso host (catálogo, ORT, RustSec, parsers) son
  cooperativos, sin límite duro de RSS/CPU. → **RR-05**, **RR-06**.

### 4.5 Credenciales de publicación

- **Controles:** OIDC sin clave larga en el repo (`release-candidate.yml:40-43`,
  [publication](../../publication.md)); `gh attestation verify` con el signer
  workflow exacto antes de transferir (`:140-157`); reconciliación de digests
  entre jobs (`:180-223`); draft y prerelease (`:230-232`), nunca release
  automática; publicación en crates.io deshabilitada; `CODEOWNERS`; branch
  protection observada en M1. La firma Ed25519 de catálogo está separada y no
  existe clave de producción.
- **Lo que NO se garantiza:** reproducibilidad binaria; verificación offline de
  attestations (D14 pendiente); vigencia actual de la branch protection (el
  recibo lista el check `portable / x86_64-pc-windows-msvc`, retirado del CI el
  2026-09-13 según [ci.md](../../ci.md), así que la protección vigente difiere o
  bloquea merges; este worker no la consultó en vivo); `SONAR_TOKEN` es un
  secreto de larga vida de un servicio de análisis, no de firma. → **RR-12**.

## 5. Permisos, retención y borrado

| Estado | Permisos garantizados por código | Retención | Borrado por el producto | Procedimiento de operador |
| --- | --- | --- | --- | --- |
| Journals/receipts M2 (`<state-root>/rust-mcp-mutations-v1`) | Directorios `0700`, archivos `0600`, uid propio (`filesystem/macos/mutation.rs:191,206`, `mutation_state.rs:63`); fuera de toda root (`host_config.rs:283-293`); `F_FULLFSYNC` | Sin TTL; planes en memoria 600 s (`crates/application/src/mutation.rs:449`) | Solo `mutation prune` de un journal terminal con ID y digest exactos (`mutation_cli.rs:53,102,137,215`) | No borrar journals `recovery_required` ni temporales `.rust-mcp-mut-*` (security-model §Metadata M2); `doctor` → `mutation_journals` antes de un downgrade ([ADR-088](../../adr/ADR-088-migration-rollback-policy.md) §3) |
| Artifacts de calidad M3–M6 (`rust-mcp-quality-artifacts-v1`) | State root sin escritura g/o (`state_primitives.rs:42-51`); `0700`/`0600`, `nlink == 1` (`:53-78`); `fsync` + `F_FULLFSYNC` (`:81-84`) | TTL 3 600 s por defecto, 86 400 s máximo (`crates/domain/src/quality_artifact.rs:692-693`); cuotas 32/64/128/256 MiB | Reconciliación y `quality-artifacts prune` (`quality_artifact_cli.rs:22,80`); los objetos en cuarentena se conservan | `prune`/`recover` periódicos; el mismo uid con el mismo state root puede releer la evidencia |
| Catálogo, trust y floor | Trust `0600` bajo `0700` (`catalog_store.rs:128,190,218,313`); floor separado, nunca promovido desde staging | Indefinida | Ninguno; `status` puede limpiar staging bajo lock | Backup/restore como árboles ([ADR-088](../../adr/ADR-088-migration-rollback-policy.md) §7); jamás bajar el floor |
| ArtifactStore efímero (logs M1 como Resources) | Memoria del proceso, owner `ProjectRef` | TTL monotónico y cuotas | Al expirar o revocar owner (`artifact-adapter/src/lib.rs:241-259`); sin borrado seguro de RAM | — |
| Índice LanceDB y modelo | `memory://`; índice importado validado antes de activar | Proceso / store del operador | `catalog rebuild-index` | Assets fuera de roots del proyecto |
| Contenedores y volúmenes Docker | Etiquetados, eliminados y verificados por job (`lib.rs:394-419`) | Duración del job | Cleanup unido; cuarentena si es incierto | Tras `kill -9` del servidor, limpiar objetos etiquetados (security-model §M5 profiling) |
| stderr/tracing | Solo mensajes propios cerrados | Host | — | Retención y rotación de logs del cliente MCP |
| Evidencia de validación en el repo | `.gitignore:11-33,61-69` excluye homes de clientes, SQLite, state y salidas crudas | Git | — | Revisar antes de commit; RR-07 |

**Garantía de código:** permisos privados y verificados al abrir, formatos
versionados que fallan cerrados, TTL/cuotas de artifacts y borrado solo
explícito. **Procedimiento de operador:** retención de journals, backups
(que duplican datos sensibles), logs, limpieza tras kill duro y ACLs. No hay
borrado seguro en disco ni en RAM. → **RR-16**.

## 6. Capacidades anunciadas ↔ oráculo nativo

Filas tomadas de las secciones de [security-model](../../security-model.md);
etapas de `scripts/gate.py:150-208`.

| Capability anunciada | Oráculo | Etapa | Resultado |
| --- | --- | --- | --- |
| stdio acotado y admisión (M0-03, M1-01) | `budget.rs:301-303`; `crates/mcp-server/tests/protocol.rs` | `core` | Protocolo; no depende del SO |
| Roots no-follow macOS (M0-04) | `project-adapter/tests/filesystem.rs:211,226,247,398,455` | `core` (macOS) | Con oráculo nativo |
| Gateway M0 y `capabilities` (M0-05/06) | `execution-adapter/tests/gateway.rs:71,96,142,185` vía `scripts/test-execution.sh:15` | `docker-security` | Con oráculo nativo |
| Rust gateway: check/fmt/Clippy/test, contención de build.rs y proc macros (M1-01..06) | `rust_calibration.rs:83,129,199,258`; selecciones `test-rust-execution.py:97-123` | `rust-security` | Con oráculo nativo |
| Audit RustSec, explain y quality gate (M1-07..09) | `test-rust-execution.py:124-137`; `test-audit-data.py:50-53` bajo deny network | `rust-security`, `audit-data` | Con oráculo nativo |
| Bundle firmado, floor, status, search, inspect (M1-10..13) | `catalog_cli.rs:226`, `catalog_status.rs:342`, `crate_search.rs:465`, `catalog_store.rs:432,474` | `catalog`, `catalog-status`, `crate-search`, `crate-inspect` | Con oráculo nativo |
| Doctor pasivo/activo y preflight de journals (M1-14, D12) | `crates/mcp-server/tests/doctor.rs`; `scripts/test-doctor.py` | `doctor` | Con oráculo nativo |
| Mutación M2: writer host, fmt/fix/resolución guest | `native_mutation.rs` (incluido por `filesystem/macos/mutation.rs:2948`); `test-m2-runtime.py:28-39`; `fix_hostile.rs:121,180` | `m2-runtime`, suite macOS | Con oráculo nativo; **power-loss sin oráculo** (ENOSPC inyectado) |
| Jobs, Tasks y store de artifacts M3 | `nextest_runtime.rs`, `coverage_runtime.rs`, `semver_runtime.rs`, `mutation_runtime.rs`; `quality_artifact_store.rs:2000,2061,2108,2217` | `m3-runtime`, suite macOS | Con oráculo nativo |
| M4 deny/unsafe/Miri/supply chain/gate v2 y privacidad | `security_native.rs:8,38,106`, `security_native_adversarial.rs:214`, `unsafe_native.rs:221`, `miri_native.rs:6,144`, `security_graph_native.rs:98`, `m4_privacy_runtime.rs:59`, `inspection_runtime/security.rs:328,620,702` (lista `test-m4-runtime.py:70`) | `m4-runtime`, `m4-tampered-plugin`, `m4-inventory` | Con oráculo nativo |
| M5 benchmark/profile/bloat y capability de profiling | `performance_native.rs:768,953,1303,2015,2603,2731` | `m5-runtime` | Con oráculo nativo (estado por tool en la [matriz M5](../M5/matrix.md)) |
| M6 analyzer (5 tools `preview`) | `analyzer_native.rs:1076,1109,1304,1408,1492,1558,1628,1680,1752,1826,1952,2093` | `m6-runtime` (12/12 en el cierre M6) | Con oráculo nativo para containment; **e2e `analyzer_runtime.rs` desgateado** y assists no deterministas ([matriz M6](../M6/matrix.md)) |
| E5/ORT y búsqueda semántica offline | `semantic-adapter/tests/local.rs:25`; `test-semantic.py:46-49` | `semantic` | Con oráculo nativo |
| Upgrade/rollback de formatos (M8-03) | `rollback_native.rs:63`; `scripts/test-m8-rollback.py`; [03-rollback.json](03-rollback.json) | Fuera de `core`/`full` (ADR-088 §6) | Con oráculo nativo, no re-ejecutado por el gate |
| Redacción del ArtifactStore efímero | `artifact-adapter/src/tests.rs:78,503,595` | `core` | Unit; en memoria, sin frontera de SO |
| Eventos M2 sin paths (ADR-058) | `stdio/mutation/audit.rs:166` | `core` | Unit |
| Provenance de release (ADR-047/048) | `release-candidate.yml:140-157`; [recibo 0.1.0](../M1/17-public-release.json) | Workflow | Histórico; **no ejecutado para 0.8.x** |

**Capabilities anunciadas sin oráculo:** ninguna capability positiva del
security model carece de oráculo. Las siguientes propiedades están declaradas
como **no garantizadas** y por tanto no requieren oráculo, pero se registran:
supervivencia a power loss, ENOSPC real de disco, ACLs, exclusión de editores
externos, daemon Docker hostil, detección de secretos, revocación en caliente,
reproducibilidad binaria y separación `SANDBOX_DENIED` permanente/transitorio.
Brechas de oráculo sobre capabilities existentes: e2e de analyzer desgateado
(RR-03), rollback con dos binarios fuera del gate (RR-17), provenance 0.8.x no
ejecutada (RR-12).

## 7. Riesgos residuales

La aceptación, el alcance y la condición de reevaluación de cada riesgo son
normativos en [ADR-089](../../adr/ADR-089-residual-risk-register.md); esta tabla
es su resumen.

| ID | Riesgo | Severidad | Alcance | Mitigación existente | Reevaluar cuando |
| --- | --- | --- | --- | --- | --- |
| RR-01 | Auditoría independiente 1.0 = revisión de modelo, no humana ni pentest | Media | Todo el producto | Threat model citado, oráculos nativos, revisiones previas por hito | Antes de anunciar otro host, transporte remoto o catálogo/modelo oficial; ante un P0/P1 reportado |
| RR-02 | Linux y Windows no calificados | Baja | Hosts no macOS | Fail-closed; CI de portabilidad | Subprograma D13 con ADR sucesor de ADR-087 |
| RR-03 | Deuda M6 del analyzer `preview` | Media | 5 tools analyzer | Clase `preview`, oráculos M6, Inspector | Antes de promover cualquiera a `stable` |
| RR-04 | RustSec stale/edad desconocida degrada, no bloquea | Media | `rust.dependencies.audit`, gates que lo consumen | `issue` explícito, freshness en el resultado | Siguiente major o evidencia de clientes que ignoran la freshness |
| RR-05 | Kernel/runc/Docker Desktop/daemon en la TCB; deltas de seccomp | Media | Toda ejecución de código del proyecto | Seccomp deny-default, sin red, sin caps, límites, cuarentena | Advisory de kernel/runc/Docker o cambio de imagen/perfil |
| RR-06 | Presupuestos cooperativos sin límite nativo duro en el proceso host | Baja | Catálogo, ORT, RustSec, parsers | Caps de bytes/entradas, workers unidos | Transporte remoto/multi-tenant o soak fuera de budget |
| RR-07 | Sin detección universal de secretos | Media | Artifacts, logs, evidencia | Redacción literal, normalización, canarios, `assert_no_credentials` | Evidencia de repos de terceros, remoto o telemetría |
| RR-08 | Mismo uid y host malicioso fuera de frontera; ACLs no inspeccionadas | Media | State root, trust, artifacts, checkout | `0700`/`0600`, uid, `nlink`, binding owner | M7 o cambio del modelo de permisos de macOS |
| RR-09 | `local_coordinated`: sin CAS, multiarchivo no atómico, power loss no demostrado, journal corrupto bloquea | Media | 6 tools de escritura | Journal, revalidación, recovery explícito | Nuevo adapter, pérdida de datos reportada o cambio de APFS |
| RR-10 | Dependencia comprometida sin advisory | Media | Build y runtime | audit/deny, pins `=`, lock, vendor SHA-256, digests | Tarea post-M8 de paquetería o advisory RUSTSEC nuevo |
| RR-11 | Catálogo/modelo: firma ≠ corrección; sin trust root oficial | Baja | Catálogo y semántica | Ed25519, floor, SHA-256 E5, SQLite autoritativo | Decisiones D15/D16 |
| RR-12 | Publicación: OIDC ≠ reproducibilidad, branch protection no re-verificada, `SONAR_TOKEN`, D14 pendiente | Media | Artifacts y repo | OIDC, verify, pins por SHA, CODEOWNERS, draft | M8-07/D14, antes de RC1 |
| RR-13 | Evidencia producida por código del proyecto no autenticada | Baja | Tests, lints, benchmarks | Clasificación conservadora, forgery tests | Si un contrato afirmara autenticidad |
| RR-14 | Límites del filesystem macOS (FIFO/device, ACL por clone, nlink, captura no atómica) | Baja | Captura y writer | No-follow en cada componente, detección de cambios | Nueva versión mayor de macOS/APFS |
| RR-15 | Sin revocación en caliente; `kill -9` deja objetos Docker | Baja | Grants y jobs | Reinicio, cleanup unido, cuarentena | Remoto o residuos observados en soak |
| RR-16 | Retención y borrado dependen del operador; sin borrado seguro | Baja | Journals, backups, logs | TTL/cuotas de artifacts, prune explícito | Requisito de cumplimiento o remoto |
| RR-17 | Brechas de oráculo en gates obligatorios | Baja | Rollback, e2e analyzer, power loss | Oráculos manuales registrados | Cierre M8-09 (dos RC) |
| RR-18 | rmcp 3.2.0 en la TCB del protocolo | Baja | stdio | Pin `=`, admisión propia, tests de protocolo | Subida de `rmcp` |

## 8. Conteo

- Fronteras: **8** (B1–B8).
- Controles evaluados en §3: **53** — **31** con oráculo nativo (N), **12**
  con oráculo unit/contract/protocol (U), **4** con evidencia histórica o de
  configuración (H) y **6** sin oráculo (—).
- Riesgos residuales: **18** (RR-01 … RR-18).

## 9. Issues abiertos detectados (fuera de los archivos permitidos a W22)

1. `.github/workflows/release-candidate.yml:219` exige `counts.tools == 31`,
   mientras `scripts/release-smoke.py:35-72` publica 36 tools: un draft 0.8.x
   falla cerrado. Corresponde a M8-07.
2. La branch protection registrada en
   [public-ci-live](../M1/public-ci-live-33928952807.json) exige el check de
   Windows retirado el 2026-09-13; re-observarla antes de RC1 (RR-12).
3. Los `#[ignore]` de `semver_runtime.rs:203-376` todavía dicen «pending M3-04
   calibration» aunque la [matriz M3](../M3/matrix.md) está calificada: texto
   desactualizado, no un hueco de oráculo.
