# W41 — los arneses M8 derivan la versión esperada del servidor de `Cargo.toml`, no de un literal `0.8.0`

Modelo solicitado: Claude Sonnet 5 (`claude -p --model sonnet --effort medium`). Rol: worker de arnés (Python). Orquestador: Claude Fable 5.1 (esta sesión continúa bajo Claude Opus 5). Sin subagentes. **Nunca en segundo plano: Bash acepta `timeout` hasta 600000 ms; `run_in_background` está PROHIBIDO.** No commit. No Docker. No red. **Archivos permitidos:** `scripts/test-m8-clients.py`, `scripts/test-m8-clients-unit.py`, `scripts/test-m8-rollback.py`, `scripts/test-m8-rollback-unit.py`. Ningún otro archivo (ni `Cargo.toml`, ni `crates/`, ni docs).

Contexto: W40 subió la versión del workspace de `0.8.0` a `0.9.0-rc.1` (tag RC1 `v0.9.0-rc.1`, M8-09) sin tocar ningún contrato — `scripts/contract-freeze.py verify --strict` sigue `passed` y `docs/validation/M8/freeze-0.8.0.json` sigue siendo el oráculo. Dos arneses host-only tienen la versión anterior como literal y ahora rechazarían el binario del árbol:

- `scripts/test-m8-clients.py:92` `SERVER_VERSION = "0.8.0"` (usado en el preflight: «the candidate must self-report version 0.8.0», y en las aserciones de la sesión del cliente).
- `scripts/test-m8-rollback.py:708-711`: `if head_version.get("version") != "0.8.0": raise DriverError(...)`.

Tarea: sustituye ambos literales por la versión leída **del propio `Cargo.toml` del workspace** en tiempo de ejecución (`[workspace.package] version`), con una sola función auxiliar por script (p. ej. `def workspace_version() -> str`) que parsee `ROOT/"Cargo.toml"` con `tomllib` (stdlib desde 3.11; el runner usa 3.13) y falle con un error claro si el campo no existe. Requisitos:

1. **No introduzcas rutas ni parámetros desde la CLI** (regla de taint de SonarCloud, precedente W38): la ruta de `Cargo.toml` es una constante derivada de `ROOT`.
2. El mensaje de error del preflight y del driver debe citar la versión esperada **calculada**, no un literal (p. ej. «the candidate must self-report version {expected}»).
3. `scripts/test-m8-rollback.py` conserva intacta la comprobación del binario antiguo contra `--old-tag` (`v0.3.0` → `0.3.0`): eso es correcto y no se toca.
4. Los comentarios/docstrings que narran el escenario histórico («v0.8.0 lists the same journal…») describen la prueba de rollback ya ejecutada con binarios `v0.3.0`/`0.8.0`: puedes generalizarlos a «el binario del árbol» donde sea trivial, pero **no** reescribas la evidencia histórica ni cambies el comportamiento.
5. Tests: en `scripts/test-m8-clients-unit.py` y `scripts/test-m8-rollback-unit.py`, adapta los dobles a la nueva fuente (parchea la función auxiliar o `SERVER_VERSION` según corresponda) y **añade** un test por script que demuestre que la versión esperada procede de `Cargo.toml` (p. ej. parcheando el contenido leído a una versión sintética `9.9.9-rc.7` y comprobando que el preflight/driver la exige). Conserva todos los tests existentes.
6. Los pines de cliente (`INSPECTOR_VERSION`, `CODEX_VERSION`, `CLAUDE_VERSION`, `AGY_VERSION`) **no se tocan**: cambiarlos sin volver a ejecutar la matriz convertiría un skip en pass.

Verificación obligatoria (reporta la salida): `python3 -B scripts/test-m8-clients-unit.py`, `python3 -B scripts/test-m8-rollback-unit.py`, `python3 -B scripts/test-gate-reporting.py`, y una comprobación directa de que la versión derivada es `0.9.0-rc.1` (p. ej. `python3 -c "import importlib.util,pathlib; …"` o el preflight del arnés sin `--run`). Informe en `docs/validation/M8/delegation/W41-harness-version-source/report.md` y en tu última respuesta. No commit.
