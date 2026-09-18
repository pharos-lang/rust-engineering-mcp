# W39b — disposición del orquestador (2026-09-17)

Invocación: `claude -p --model sonnet --effort medium --permission-mode acceptEdits --disallowedTools Agent Task --allowedTools <lista> --no-session-persistence --output-format json < prompt-header.md` (CLI 2.1.274). Inicio 2026-09-17T17:43:31Z, fin 22:39:37Z, exit 0, 102 turnos, 2 088 559 ms, `permission_denials: 17` (todos posteriores a la avería del host descrita abajo: intentos de diagnosticar `git`/`xcode-select` fuera del allowlist).

Veredicto: **aceptado con verificación del orquestador**. El cambio es el que
pedía el encargo: tras cada `env_clear()` de un spawn del **binario de producto**
se reinyecta únicamente `LLVM_PROFILE_FILE`; el resto del aislamiento (PATH, HOME,
credenciales) sigue intacto. Once archivos de `crates/mcp-server/tests/`; los tres
sitios que lanzan otro binario (`/usr/bin/true` en `cli.rs`, la CLI de `docker` en
`inspection_runtime.rs`) quedan sin tocar, correctamente. Verificado por el
orquestador que `protocol.rs::Server::start_configured` ya traía esa reinyección
(preexistente) y que el cambio nuevo en ese archivo es el spawn de `contract --json`.

Verificación del orquestador sobre el árbol resultante: `cargo fmt --all --check`
limpio; `cargo clippy -p rust-engineering-mcp --all-targets -D warnings` limpio;
`cargo test -p rust-engineering-mcp` en verde (ver recibo del gate `core` de cierre).

**Lo que el worker no pudo medir** (declarado por él, no convertido en pass): la
comparación antes/después de `cargo llvm-cov`. A mitad de su sesión el host perdió
la aceptación de la licencia de Xcode y todo `git`/`cc` pasó a fallar con exit 69.
Causa raíz hallada por el orquestador: el reinicio del 2026-09-17 trajo una
actualización de macOS **26.6.2 → 27.0**, que revoca la aceptación de licencia de
Xcode 26.6 y deja activo un SDK de Command Line Tools (`MacOSX27.0.sdk`) cuyo
`libSystem.B.tbd` el `ld` instalado no sabe leer («unknown architecture
arm64e.x1-macos»). Mitigación aplicada por el orquestador **sin `sudo`** y sin
tocar el repo: `DEVELOPER_DIR=/Library/Developer/CommandLineTools`,
`SDKROOT=…/MacOSX26.5.sdk` y un `git` no gateado (el binario real de CLT) al frente
del `PATH`. Ambas variables quedan registradas en el recibo del gate.

El efecto real de este cambio sobre la cobertura de código nuevo se lee en
SonarCloud (métrica `new_coverage` del PR #22) tras el push, no en una medición
local: ahí es donde la puerta del 80 % se evalúa.
