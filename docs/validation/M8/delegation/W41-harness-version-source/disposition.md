# W41 — disposición del orquestador (2026-09-17)

Invocación: `claude -p --model sonnet --effort medium --permission-mode acceptEdits --disallowedTools Agent Task --allowedTools <lista> --no-session-persistence --output-format json < prompt-header.md` (CLI 2.1.274). Inicio 2026-09-17T23:20:47Z, fin 23:24:21Z, exit 0, 44 turnos, 213 628 ms, `permission_denials: 0`.

Origen: W40 subió la versión del workspace a `0.9.0-rc.1` y dos arneses host-only
exigían el literal `0.8.0` al binario del árbol (`test-m8-clients.py:92`,
`test-m8-rollback.py:708`), lo que habría convertido cualquier re-ejecución en un
fallo de preflight.

Veredicto: **aceptado**. Ambos scripts derivan ahora la versión esperada de
`[workspace.package] version` de `Cargo.toml` con `tomllib`, ruta constante bajo
`ROOT` (regla de taint W38). Verificación del orquestador: `test-m8-clients-unit.py`
130/130, `test-m8-rollback-unit.py` 42/42, `test-gate-reporting.py` 13/13; el
preflight del arnés de clientes reporta `the candidate must self-report version
0.9.0-rc.1` con `satisfied: true` contra el binario `release` del árbol. Los pines
de cliente (`INSPECTOR_VERSION`, `CODEX_VERSION`, `CLAUDE_VERSION`, `AGY_VERSION`)
quedan intactos, como exigía el encargo: cambiarlos sin volver a ejecutar la matriz
convertiría un skip en pass.

Nota para el cierre: `CLAUDE_VERSION` sigue pinado a `2.1.268` y el CLI del host ya
es `2.1.274`; si se vuelve a ejecutar la matriz de clientes sobre los bytes de RC1,
ese pin debe actualizarse **con** el recibo de la ejecución, nunca antes.
