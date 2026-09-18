# W41 — los arneses M8 derivan la versión esperada del servidor de `Cargo.toml`

Worker: Claude Sonnet 5 (`claude -p --model sonnet --effort medium`). Sin subagentes, sin
background, sin Docker, sin red, sin commit. Archivos tocados: `scripts/test-m8-clients.py`,
`scripts/test-m8-clients-unit.py`, `scripts/test-m8-rollback.py`,
`scripts/test-m8-rollback-unit.py`. Ningún otro archivo.

## Cambios

### `scripts/test-m8-clients.py`
- Nueva función `workspace_version() -> str`: abre `ROOT / "Cargo.toml"` con `tomllib`
  (stdlib), devuelve `manifest["workspace"]["package"]["version"]`, y levanta
  `RuntimeError` con mensaje claro si el campo no existe.
- `SERVER_VERSION = workspace_version()` en vez del literal `"0.8.0"`. Ningún otro
  módulo cambia; la ruta de `Cargo.toml` es la constante `ROOT`, nunca un parámetro CLI
  (precedente de taint W38).
- El mensaje de la precondición `candidate_version_0_8_0` (la clave se conserva; solo
  cambia el texto) ahora cita la versión calculada: `f"the candidate must self-report
  version {SERVER_VERSION}"`, en vez del literal `"0.8.0"`.
- `FREEZE_MANIFEST` (`docs/validation/M8/freeze-0.8.0.json`) no se toca: ese nombre de
  archivo es el oráculo de contrato congelado, no una comprobación de versión del
  servidor, y sigue vigente (`contract-freeze.py verify --strict` → `passed`).

### `scripts/test-m8-rollback.py`
- Misma función auxiliar `workspace_version()`, idéntica implementación (import
  `tomllib`, `ROOT / "Cargo.toml"`).
- La comprobación del binario del árbol actual pasa de
  `if head_version.get("version") != "0.8.0"` a comparar contra
  `expected_head_version = workspace_version()`, con el mensaje de error citando ambas
  versiones observada y esperada dinámicamente.
- La comprobación del binario antiguo contra `--old-tag` (`v0.3.0` → `"0.3.0"` vía
  `args.old_tag.lstrip("v")`) queda intacta, como pedía la tarea.
- Docstrings/comentarios que narran el escenario histórico (v0.3.0/v0.8.0) no se
  tocaron: describen la prueba de rollback ya ejecutada con esos binarios reales, no
  una comprobación de versión activa en el código.

### `scripts/test-m8-clients-unit.py`
- Los mocks existentes de `server_version()` que devuelven `{"version": "0.8.0"}`
  siguen intactos: solo alimentan el campo `server_version` del recibo bajo pruebas
  donde `preconditions` está también mockeada (`return_value={}`), así que no ejercitan
  la comparación con `SERVER_VERSION` y no necesitaban cambios.
- `WorkspaceVersionTests` (nueva clase, 2 tests):
  - `test_expected_server_version_is_derived_from_cargo_toml`: parchea `M8.ROOT` a un
    directorio temporal con un `Cargo.toml` sintético (`version = "9.9.9-rc.7"`) y
    confirma que `M8.workspace_version()` devuelve exactamente esa versión.
  - `test_candidate_version_precondition_requires_the_derived_version`: parchea
    `M8.SERVER_VERSION` a `"9.9.9-rc.7"` y confirma que el mensaje de la precondición
    `candidate_version_0_8_0` cita esa versión, no un literal fijo.

### `scripts/test-m8-rollback-unit.py`
- `MainAggregationTests._patch_build` y `test_rejects_a_binary_reporting_the_wrong_version`
  ahora parchean también `M.workspace_version` a `"0.8.0"` (la versión de árbol usada
  por los dobles existentes), preservando el comportamiento previo sin depender del
  `Cargo.toml` real del repo (que ya está en `0.9.0-rc.1` tras W40).
- Nuevos tests:
  - `test_rejects_a_head_binary_not_matching_the_workspace_version`: `workspace_version`
    mockeada a `"9.9.9-rc.7"` mientras el binario del árbol reporta `"0.8.0"`; confirma
    que el driver aborta y que `driver_error` cita ambas versiones.
  - `test_workspace_version_reads_the_synthetic_cargo_toml`: mismo patrón que en el otro
    archivo — `Cargo.toml` sintético bajo `M.ROOT` parcheado, confirma la lectura directa.
- Los pines de cliente (`INSPECTOR_VERSION`, `CODEX_VERSION`, `CLAUDE_VERSION`,
  `AGY_VERSION`) no se tocaron.

## Verificación

```
$ python3 -B scripts/test-m8-clients-unit.py
..................................................................................................................................
----------------------------------------------------------------------
Ran 130 tests in 2.522s

OK
```

```
$ python3 -B scripts/test-m8-rollback-unit.py
..........................................
----------------------------------------------------------------------
Ran 42 tests in 0.019s

OK
```

```
$ python3 -B scripts/test-gate-reporting.py
.............
----------------------------------------------------------------------
Ran 13 tests in 0.161s

OK
```

Comprobación directa de que la versión derivada es la del RC1 actual (`Cargo.toml`
`[workspace.package] version = "0.9.0-rc.1"`, subido por W40):

```
$ python3 -c "
import importlib.util
spec = importlib.util.spec_from_file_location('m8clients', 'scripts/test-m8-clients.py')
m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m)
print('clients SERVER_VERSION =', m.SERVER_VERSION)
"
clients SERVER_VERSION = 0.9.0-rc.1

$ python3 -c "
import importlib.util
spec = importlib.util.spec_from_file_location('m8rollback', 'scripts/test-m8-rollback.py')
m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m)
print('rollback workspace_version() =', m.workspace_version())
"
rollback workspace_version() = 0.9.0-rc.1
```

Preflight del arnés de clientes sin `--run` (no ejecuta nada, no requiere Docker ni
clientes), confirmando que el mensaje de la precondición ya cita la versión calculada
y que el binario `target/release/rust-engineering-mcp` del árbol la satisface:

```
$ python3 -B scripts/test-m8-clients.py --preflight | python3 -c "
import json, sys
print(json.dumps(json.load(sys.stdin)['preconditions']['candidate_version_0_8_0'], indent=2))
"
{
  "requirement": "the candidate must self-report version 0.9.0-rc.1",
  "satisfied": true
}
```

No commit realizado.
