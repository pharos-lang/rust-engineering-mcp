# V04 — informe del auditor (Claude Opus 5, High, read-only: Read/Grep/Glob; claude 2.1.268)

**Revisión de modelo, no auditoría humana ni pentest (RR-01).** Invocación: `claude -p --model opus --effort high --tools 'Read,Grep,Glob' … < input.md` (input sha256 `d9bbb83e7bf16763be50b474735ea4356290be824dd06627e3c69a99de4feb60`). Primer intento (V04, 2026-09-15 03:15 UTC) abortado por el límite de cuota del CLI; segundo intento inicio 2026-09-15T17:03:57Z, fin 2026-09-15T17:12:34Z, 516060 ms, 85 turnos.

---

# V04: auditoría de seguridad de cierre M8

**Revisión de modelo (Claude Opus 5 High), read-only, alcance declarado; no es auditoría humana ni pentest.**

Rama `ai/m8-stabilization`, HEAD `8d77b3d`. Solo usé Read, Grep y Glob. No ejecuté tests, `git` ni binarios. Muestreé 27 controles (≥ 2 por frontera).

## Veredicto global: **Approve con findings**

- **0 P0, 0 P1 y 0 P2 de seguridad.** No hay nada que bloquee la readiness.
- **7 findings P3**: citas erróneas o desfasadas en el threat model, un efecto lateral menor de `doctor`, rutas locales en recibos, endurecimiento del workflow de release, una inconsistencia en la validación de rutas del host y la clasificación de RR-12.
- Todos los controles muestreados existen y hacen lo que dice el threat model. La única excepción parcial son las citas de `env_clear` (F-01): el control existe, pero tres de las cuatro citas apuntan a código de test.

---

## 1. Tabla de controles muestreados

| Frontera | Control | Evidencia | Veredicto |
|---|---|---|---|
| B1 | Escritura exige runtime y root ⊂ lectura | `crates/mcp-server/src/host_config.rs:263-282` | ✔ |
| B1 | Journal `rust-mcp-mutations-v1` fuera de toda root | `host_config.rs:283-293` | ✔ (solo si hay grants de escritura) |
| B1 | Lista cerrada de digests de imagen | `host_config.rs:177-184` | ✔ |
| B1 | Profiling: valor cerrado y exige runtime | `host_config.rs:91-97`, `:256-262` | ✔ |
| B1 | Política, vendor y captura fuera de las roots | `host_config.rs:206-214`, `:220-235`, `:240-255` | ✔ (ver F-06) |
| B1 | Digest del ejecutable Docker y del engine en cada ejecución | `crates/execution-adapter/src/lib.rs:517-525` | ✔ |
| B1/4.2 | `env_clear`, `--config` propio y `--host` explícito | `lib.rs:333-346`; único punto de spawn en producción: `rust_gateway.rs:1832`, `mutation_gateway.rs:369,412`, `analyzer_gateway.rs:551` | ✔ (citas erróneas, F-01) |
| B2 | Línea ≤ 1 MiB, deadline de frame 10 s | `crates/mcp-server/src/stdio/budget.rs:15,17` | ✔ |
| B2 | Admisión de 16 requests/notifications/sends | `stdio/admission.rs:24-25,107-113` | ✔ |
| B2 | Error uniforme `task unavailable` | `stdio/tasks.rs:25` | ✔ (solo verifiqué la constante) |
| B3 | `NOFOLLOW_ANY \| RESOLVE_BENEATH` en cada open; `nlink == 1`; rechazo de `..` | `crates/project-adapter/src/filesystem/macos.rs:28-29,61-69,71-83,110` | ✔ |
| B3 | Predicado `.cargo/config*` | `crates/domain/src/security.rs:39-46` | ✔ (por nombre, como se declara) |
| B3 | `rust-analyzer.toml` a cualquier profundidad y en cualquier casing | `analyzer_gateway.rs:421-433` | ✔ |
| B3 | `env_clear` en `lsp_session.rs:879` | Está dentro de `#[cfg(test)] mod tests` (`lsp_session.rs:862-881`) | ✘ cita incorrecta; control efectivo vía `lib.rs:335` |
| B4 | `--network=none`, `--read-only`, `--cap-drop=ALL`, `no-new-privileges`, ipc/cgroupns privados, pids 128, 1 CPU, 1g sin swap, tmpfs | `rust_gateway.rs:1316-1343`; probe `lib.rs:436-477`; `performance_gateway.rs:786,788` | ✔ |
| B4 | Seccomp `SCMP_ACT_ERRNO` por defecto en los 6 perfiles | línea 2 de cada `crates/execution-adapter/src/seccomp*.json` | ✔ |
| B4 | Sin `mount`, `unshare`, `ptrace`, `bpf`, `setns`, `keyctl`, `init_module`, `kexec`, `pivot_root`… | Grep sin coincidencias en los 6 perfiles | ✔ |
| B4 | `clone` con máscara de flags de namespace = 0; `clone3` devuelve ENOSYS | `seccomp-rust.json:133-153`; calibración `fixtures/security/rust-containment/checks.rs:35-54` | ✔ |
| B4 | Deltas acotados | fix: `socket` AF_INET/STREAM/0 (`seccomp-rust-fix.json:178-201`); quality: `socketpair` AF_UNIX (`seccomp-rust-quality.json:178-201`); profile: `perf_event_open` (`seccomp-rust-profile.json:204`) | ✔ (RR-05) |
| B4 | `rm --force`, verificación de ausencia y cuarentena; ejecución bloqueada en cuarentena | `lib.rs:394-419`, `:503-513`; `rust_gateway.rs:899-907` | ✔ |
| B5 | Ed25519 con contexto de dominio antes del parse JSON; presupuestos zstd; publisher/canal; versión cerrada | `crates/catalog-adapter/src/bundle.rs:143-202` | ✔ |
| B5 | Floor: `permits`, checksum y forma canónica | `bundle/floor.rs:62-86,106-108` | ✔ |
| B5 | Orden verify → newer → floor → reserve → commit → readback | `crates/mcp-server/src/catalog_cli.rs:270-291` | ✔ |
| B5 | Sync: https-only, sin proxy, sin redirects, sin raíces nativas, sin IP ni userinfo | `catalog_sync.rs:33-65,95-114` | ✔ |
| B6 | E5: revisión, tamaño y SHA-256 fijados y verificados antes de parsear | `crates/semantic-adapter/src/model.rs:4-31,40-54` | ✔ |
| B6 | ORT `download-binaries` / `load-dynamic` prohibidos | `deny.toml:20-22` | ✔ |
| B7 | Evento de auditoría M2 con campos cerrados (id y conteo, sin paths ni diffs) | `stdio/mutation/audit.rs:78-103` | ✔ |
| B7 | Redacción: rechaza secretos vacíos, solape entre chunks | `crates/artifact-adapter/src/lib.rs:262-278` | ✔ (lectura parcial del bucle) |
| B7 | State root del uid y sin escritura g/o; dirs `0700`, files `0600`, `nlink == 1` | `filesystem/macos/state_primitives.rs:42-78` | ✔ |
| B7 | Preflight de journals: `downgrade_blocked` ante Busy, RecoveryRequired o error | `crates/mcp-server/src/doctor.rs:257-341` | ✔ (efecto lateral, F-03) |
| B8 | Tag `vX.Y.Z` obligatorio | `.github/workflows/release-candidate.yml:23-29` | ✔ |
| B8 | `contents: read` global; `id-token`/`attestations` en build; `contents: write` solo en draft | `release-candidate.yml:10-11,41-44,178-179` | ✔ (líneas desfasadas en el TM, F-02; alcance del token, F-05) |
| B8 | Attestation y `gh attestation verify` con signer workflow exacto | `release-candidate.yml:139-164` | ✔ |
| B8 | Sonar: `pull_request` (no `_target`), guard de fork, token solo en el último step | `.github/workflows/sonarcloud.yml:6-7,20-22,79-81` | ✔ |

---

## 2. Findings

### F-01 · P3 · `docs/validation/M8/08-threat-model.md:110`, `:195-196`: citas de `env_clear` a código de test
- **Evidencia:** `supervisor.rs:465`, `lsp_session.rs:879` y `analyzer_gateway.rs:1645` están dentro de módulos `#[cfg(test)]` (`supervisor.rs:460-467`, `lsp_session.rs:862-881`, `analyzer_gateway.rs:1624-1647`). En producción el único `Command::new` es `lib.rs:333`, que llama a `env_clear` (`:335`). Todos los gateways lo usan (`rust_gateway.rs:1832`, `mutation_gateway.rs:369,412`, `analyzer_gateway.rs:551`). `performance_native.rs:1813` también es de test (`lib.rs:769-770`).
- **Impacto:** el control se cumple, pero la trazabilidad «control ↔ código» de G2 es falsa en 3 de 4 citas.
- **Acción:** cambiar las citas a `lib.rs:333-346` y a los cuatro call sites.

### F-02 · P3 · `08-threat-model.md:164-169`, `:350-352`; `docs/adr/ADR-089-residual-risk-register.md:98-100`: citas de B8 desfasadas e issue ya resuelto
- **Evidencia:** `release-candidate.yml` ya no exige `tools == 31`. Toma `tool_count` de `docs/validation/M8/freeze-0.8.0.json` (`:128-136`, `:200`) y lo compara en `:228`. Las líneas reales son: permisos `:41-44`, verify `:147-164`, `contents: write` `:178-179`, pin de checkout `:47`.
- **Acción:** actualizar las citas, cerrar el issue §9.1 y la consecuencia de ADR-089 con esta evidencia.

### F-03 · P3 · `crates/mcp-server/src/doctor.rs:19-23`, `:254-262`: `doctor.mutation_journals` no es puramente lectura
- **Evidencia:** `NativeMutationStore::open` crea `mutation-store.lock` con `O_CREAT` si no existe (`filesystem/macos/mutation.rs:1253`, `:384-389`). También hace `fsync` + `F_FULLFSYNC` del directorio (`:1254`, `:375-380`). `list_records` toma un `flock` exclusivo no bloqueante (`:1269`, `:400`), así que una mutación concurrente de `serve` puede recibir `LockBusy` durante el escaneo.
- **State root hostil: bien contenido.**
  - `checked_path` limita a absoluta, sin `.`/`..`, ≤ 64 componentes (`macos.rs:71-83`).
  - Apertura desde `/` con `NOFOLLOW_ANY` (`mutation.rs:330-343`), APFS, `0700` y uid (`:187-199`, `:344-345`).
  - Límites de entradas y bytes (`:23-25`, `:39`, `:1721-1745`).
  - Nombres no UTF-8 o ajenos → `RecoveryRequired` antes de abrir (`:1714-1735`).
  - Opens con `O_NONBLOCK` (`macos.rs:67`) y rechazo de no-regulares (`mutation.rs:201-204`): un FIFO no bloquea.
  - Lock no bloqueante → `Busy` (`doctor.rs:305-313`).
  - `panic`/`unwrap`/`expect` en deny (`Cargo.toml:66-69`).
  - No encontré lectura fuera del root, pánico ni bloqueo.
- **Acción:** documentar que doctor crea el lock y lo retiene brevemente (comentarios de `doctor.rs` y ADR-088 §3), o abrir en doctor sin `O_CREAT`.

### F-04 · P3 · `docs/validation/M8/03-rollback.json`, `core-gate.json:1507-1509`, `core-gate-m8-03-08.json:1568-1570`: rutas de HOME con el nombre de usuario
- **Evidencia:** 42 rutas `/Users/cburgosro/...` en `03-rollback.json` (p. ej. `:10`, `:54`), rutas de `.rustup` en los core-gate y 45 en `delegation/R01-census-traceability/report.md`.
- **Tokens, cabeceras, PEM: ninguno.**
  - Las coincidencias de `sk-` son subcadenas de «risk-register», «risk-budget» y «disk-backed».
  - `api_key` / `apiKey` corresponden a `"api_key_source": "none"` (`clients/attempt-15/receipt.json:13`) y `"apiKeySource":"none"` en los eventos.
  - `Authorization` y `ghp_` solo aparecen en el prompt de este V04.
- **Ya mitigado:** `scripts/public-export.py:77-78` sustituye `<LOCAL_HOME>` y `<LOCAL_USER>` al exportar.
- **No verificado:** en disco hay `*-events.jsonl`, `*.stderr` con stack traces que incluyen `/Users/...`, `*.stdout` y `state-*/`. Están excluidos por `.gitignore:28,64-67`, pero sin `git` no pude confirmar que no estén versionados (el Grep sobre `.git/index` no es concluyente).
- **Acción:** comprobar que `git ls-files docs/validation/M8/clients` no lista `*.stdout`, `*.stderr`, `*-events.jsonl` ni `state-*`. Publicar solo mediante `public-export.py` o relativizar las rutas en los recibos.

### F-05 · P3 · `.github/workflows/release-candidate.yml:41-47`, `:62-111`: token OIDC visible para `cargo build`
- **Evidencia:** `id-token: write` y `attestations: write` se conceden a todo el job `build`, que ejecuta los `build.rs` de dependencias y scripts Python antes de atestar. `actions/checkout` no usa `persist-credentials: false` en ningún workflow (Grep sin coincidencias en `.github/`).
- **Impacto:** una dependencia comprometida (RR-10) podría obtener un token OIDC y atestar otros bytes con la identidad del workflow de release. El impacto marginal es bajo, porque el binario ya estaría comprometido, pero debilita la afirmación «attestation ⇒ bytes calificados».
- **Acción:** mover attest y verify a un job sin compilación que consuma el artifact con los digests reconciliados. Añadir `persist-credentials: false`. Registrarlo en RR-10/RR-12.

### F-06 · P3 · `crates/mcp-server/src/host_config.rs:192-219`, `:283-293`: validación «fuera de roots» inconsistente
- **Evidencia:** política, vendor y captura se rechazan dentro de una root. En cambio `--rustsec-snapshot` (`:215-219`) y `--catalog-store/-trust/-model-dir/-index-store` (`:192-205`) no. El state root solo se comprueba contra las roots si hay grants de escritura (`:283`).
- **Mitigación existente:** SHA-256 de RustSec (`:145-151`), trust `0600`/`0700` (`catalog_store.rs:127-129,189-191`), hashes E5 (`model.rs:40-54`). No encontré un camino explotable: el código del proyecto no corre en el host.
- **Acción:** rechazar también esas rutas dentro de roots, o documentar por qué no hace falta.

### F-07 · P3 · `docs/adr/ADR-089-residual-risk-register.md:67` (RR-12): mezcla riesgos aceptados con precondiciones
- **Evidencia:** RR-12 junta riesgos estructurales (OIDC ≠ reproducibilidad, `SONAR_TOKEN`) con tareas que tienen que estar hechas antes de RC1: re-observar la branch protection (el recibo M1 aún exige el check de Windows retirado), ejecutar la provenance de 0.8.x y D14.
- **Acción:** sacar esas tres como ítems bloqueantes del checklist RC1/1.0, no como riesgo aceptado.

---

## 3. Respuestas por pregunta

### P1. Fronteras
Ver la tabla de §1. Todos los controles citados existen y se comportan como afirma el threat model, salvo las citas de F-01 y F-02. El filtrado de namespaces que el threat model no menciona explícitamente también está: `clone` con máscara `0x7E020000` y `clone3` → ENOSYS, con fixture de calibración.

### P2. Cambios de M8
- **`contract`:** el parser acepta solo `--json` o `--human` y nada más (`contract_cli.rs:10-21`). `run` delega en `stdio::contract` (`:23-25`). El documento se construye con constantes (`capability_document.rs:44-56,478-481`). No leí el cuerpo completo de `stdio::contract`, así que su ausencia de efectos queda **parcialmente verificada**.
- **`doctor.mutation_journals`:** sin lectura fuera del root, pánico ni bloqueo; con efecto lateral menor (F-03).
- **`--state-root` solo en doctor:** `doctor.rs:30-41,69-82` exige un único `--state-root` absoluto y vuelve a pasar el resto por `host_config::parse`. `serve` no cambia (`main.rs:149-156`).
- **`resources/templates/list`:** dos plantillas estáticas, `ttl 0`, caché `Private`, sin datos de sesión (`stdio.rs:838-870`). `resources/list` está vacío (`:826-836`). No revela nada.
- **`release-smoke.py`: fail-closed.**
  - `SHA256SUMS` estricto antes de abrir el tar (`:305-313`, `:634-639`).
  - Fingerprint del archivo antes y después (`:319-322`, `:360-361`).
  - Paths seguros, sin links ni no-regulares (`:232-238`, `:332-348`).
  - Hash por miembro contra el manifest (`:403-413`).
  - 36 tools con SHA-256 de schemas y annotations (`:1007-1039`).
  - Entorno limpio y process group verificado (`:694-730`).
  - Recibo con `O_EXCL | O_NOFOLLOW` (`:1137-1140`); rechaza Python optimizado (`:1240-1241`).
  - Limitación: la vinculación externa del archive depende del cross-job digest (`release-candidate.yml:211-219`) y de la attestation.
- **Rollback driver:** usa `--end-of-options` en `scripts/test-m8-rollback.py:129,232,284`. Resuelve `tag^{commit}` y reutiliza el worktree solo si HEAD coincide y está limpio (`:264-277`). `rmtree` se limita a `WORK_ROOT/worktree-<tag>` (`:257,278-281`). Es un script de desarrollo con argumentos del operador; sin finding.

### P3. Registro de riesgos
- No encuentro ningún RR que deba ser P1/P2 bloqueante para una primera versión estable en macOS. Las severidades Media/Baja son coherentes con el código revisado.
- RR-12 debe separarse (F-07).
- **Faltan o conviene añadir:**
  - (a) Exposición del token OIDC durante la compilación (F-05), en RR-10/RR-12.
  - (b) El workflow de release no tiene paso de firma de código ni notarización macOS (leí `release-candidate.yml:1-243` entero). La integridad para el usuario depende solo de `SHA256SUMS` y la attestation. Propongo RR-19 (Baja). No revisé `docs/publication.md` para ver si ya está declarado.
  - (c) Contención del lock de journals por `doctor` (F-03), Baja.

### P4. Secretos y evidencia
No hay tokens, cabeceras `Authorization`, claves ni PEM. Sí hay rutas de HOME con el nombre de usuario en recibos versionados (F-04). No pude confirmar con estas herramientas que los transcripts crudos no estén versionados.

### P5. Supply chain
- **Pins `=`:** todas las dependencias del workspace (`Cargo.toml:25-61`). Las crates internas van sin versión, como permite `deny.toml:17`. No hay versiones no-`=` inline en `crates/*/Cargo.toml` (el Grep solo cubre la forma inline).
- **`--locked`:** `ci.yml:52,59,62,65,68`; `release-candidate.yml:63,79-80,90`; `gate.py:152,155`.
- **audit/deny:**
  - CI con herramientas fijadas y fetch: `ci.yml:96-110`.
  - Gate `core`: `gate.py:190-191`, fuera de `if full` (`:192`); usa `cargo audit --no-fetch`.
  - `deny.toml:7-8` (`ignore = []`, yanked deny), `:12` (staleness ≤ 30 días), `:18-30`, `:33-36`.
  - Nota: `unmaintained = "workspace"` (`:11`) no hace fallar por dependencias transitivas; ya está declarado para `paste`.
- **Vendor:** `scripts/verify-vendor.py:11,23,25-30` y `gate.py:188`; patch en `Cargo.toml:71-72`.
- **Acciones fijadas por SHA de 40 hex:** `ci.yml:36,39,86`; `release-candidate.yml:47,50,140,167,182`; `sonarcloud.yml:30,35,79`; `codeql.yml:38,52,59`.
- **Permisos mínimos:** `ci.yml:9-10`; `release-candidate.yml:10-11,41-44,178-179`; `sonarcloud.yml:10-11`; `codeql.yml:12-16` (`security-events: write`, necesario).
- **OIDC:** `release-candidate.yml:43,139-164`.
- **Otros:** Sonar `pip --require-hashes` (`sonarcloud.yml:47`); Dependabot cargo + actions (`.github/dependabot.yml:4-41`); `.github/CODEOWNERS:2`; toolchain 1.98.1 fijado (`release-candidate.yml:57-58`).

---

## 4. Límites de esta revisión
- No ejecuté tests, gates ni `git`. El estado de versionado de los transcripts queda sin verificar.
- Muestreé 27 de los 53 controles. Lectura parcial de `stdio::contract`, del bucle de redacción y de `catalog_store.rs`.
- No revisé `docs/publication.md`, `AGENTS.md` ni los ADR-009/024/030/031/050/077/084/085/090 más allá de lo citado en el threat model.
- Los servidores MCP de claude.ai (Gmail, Google Calendar, Google Drive, Notion) necesitan autorización en la configuración de conectores de claude.ai; no los usé y no estarán disponibles hasta que se autoricen.
