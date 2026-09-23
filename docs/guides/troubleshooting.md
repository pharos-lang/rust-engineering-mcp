# Solución de problemas

## Primero: `doctor`

```bash
rust-engineering-mcp doctor --json
```

`doctor` es puramente pasivo: reporta el estado de capacidades **configuradas**
sin instalar, descargar ni reparar nada. Pásale los mismos flags de host que
usarías en `serve` (acepta el mismo grupo de flags). Añade `--active` para que
además calibre activamente el runtime Rust aprobado (crea y destruye un
contenedor de prueba); sin `--active` esa comprobación queda `not_checked`.

Cada verificación (`crates/mcp-server/src/doctor.rs`, `enum Id`) reporta uno
de estos estados: `available`, `unavailable`, `not_configured`, `not_checked`,
`not_used`, `warning`. Las verificaciones existentes son: `Catalog`, `Model`,
`SemanticIndex`, `Rustsec`, `CatalogFreshness`, `ModelFreshness`,
`RustsecFreshness`, `FilesystemRoots`, `Rustc`, `Cargo`, `Rustfmt`, `Clippy`,
`Sandbox`, `HostTools`, `CargoAudit`, `AuditEngine`, `OptionalTools`,
`Diagnostic`. Un `warning` en una capacidad opcional (por ejemplo
`OptionalTools`) no significa que el servidor esté roto — significa que esa
capacidad concreta no está disponible.

## Errores de tool: códigos reales

El campo `error_code` de una respuesta de tool usa
`SCREAMING_SNAKE_CASE` (`crates/domain/src/result.rs`). Los códigos comunes,
transversales a la mayoría de tools:

| `error_code` | `status` | Causa típica |
| --- | --- | --- |
| `PROJECT_NOT_FOUND` | `blocked` | `project_ref` caducado, inválido o de otro proceso; reabre con `rust.project.open`. |
| `INVALID_PROJECT` | `blocked` | El path no es un proyecto Cargo válido o falló la validación estructural. |
| `SANDBOX_DENIED` | `blocked` | Falta el grupo `--docker`/`--docker-socket`/`--state-root`/`--rust-image` completo, la calibración del runtime falló, o no hay capacidad actual. Ver [Operación del runtime](../operations/runtime-provisioning.md). `rust.project.inspect` sin runtime configurado devuelve exactamente este código; test de protocolo real por el wire MCP: `crates/mcp-server/tests/protocol.rs::inspect_live_reference_without_runtime_is_denied_without_project_execution`. |
| `NETWORK_DENIED` | `blocked` | Un intento de red no autorizado; recuerda que `catalog sync --url` es la única ruta que hace red, y solo por CLI, nunca desde `serve`. |
| `TOOL_NOT_INSTALLED` | `unavailable` | El binario auxiliar (nextest, llvm-cov, semver-checks, mutants, cargo-deny, etc.) no está en la imagen configurada — normalmente significa una imagen guest anterior a la que la tool requiere. |
| `UNSUPPORTED_PLATFORM` | `unavailable` | Host distinto de macOS ARM64/APFS; es un fallo cerrado deliberado, no un bug. |
| `OUTPUT_LIMIT_EXCEEDED` | `blocked` | El proceso ejecutado excedió un límite de tamaño de stdout/stderr/diagnósticos; revisa `truncation` en la respuesta. |
| `LOCKFILE_UPDATE_REQUIRED` | `blocked` | La operación necesitaría modificar `Cargo.lock` sin autorización para ello. |

Códigos específicos de tools de escritura/rendimiento/analyzer (no forman
parte del enum transversal, pero son cadenas reales devueltas por el código):

- `PROFILING_NOT_AUTHORIZED` (`rust.profile.flamegraph`, `status: blocked`):
  falta `--allow-profiling user-space-sampling`. Se devuelve **antes** de
  crear ningún contenedor.
- `ACTION_STALE` (`rust.analyzer.action.apply`): el `action_digest` del
  preview ya no coincide con lo que rust-analyzer devuelve ahora sobre el
  archivo; pide un preview nuevo, no reintentes el commit con el digest viejo.
- `NOT_A_DATASET` (`rust.benchmark.compare`): el `dataset_id` referenciado no
  es un artifact válido publicado por un `rust.benchmark.run` anterior del
  mismo proyecto.
- `TASKS_REQUIRED`: pediste `execution_mode=auto` o `task` con un timeout que
  excede la vía síncrona calificada (normalmente 60 s), y el peer MCP no
  declaró la extensión `io.modelcontextprotocol/tasks` en `initialize`.
- `recovery_required` (dato en la respuesta, no `error_code`): una mutación
  quedó en estado incierto tras una interrupción. **No edites** los
  temporales `.rust-mcp-mut-*.swap` ni el journal bajo `--state-root`; llama
  la misma tool con `recover: true` usando el mismo grant y `--state-root`.

## Errores de CLI

Cualquier subcomando de nivel dos con `--help`/`-h` (`serve --help`,
`doctor --help`, `catalog --help`, `contract --help`, etc.) imprime la
sección de ayuda de ese subcomando por stdout y sale con código 0; para los
subcomandos anidados (`catalog status --help`, `mutation prune -h`, etc.)
imprime la misma sección de su comando padre. `--help`/`-h` solo se resuelve
así cuando aparece justo después del comando (o del subcomando): cualquier
otro uso, o un argumento extra a continuación, sigue devolviendo:

```text
Unsupported invocation. Use 'rust-engineering-mcp --help'.
```

con **código de salida 2**, igual que un comando desconocido. `serve --help`
nunca llega a validar `--stdio` ni a arrancar el servidor. Consulta
[Configuración](configuration.md) para el detalle de cada flag.

## Configuraciones incorrectas comunes

| Síntoma | Qué revisar |
| --- | --- |
| El cliente no arranca el servidor | Ejecuta `rust-engineering-mcp version --json` con la misma ruta configurada; usa siempre una ruta absoluta al binario; revisa `stderr`/el panel de output del cliente. |
| `serve` falla al arrancar sin mensaje claro en el cliente | Ejecútalo manualmente en una terminal con los mismos flags: los errores de validación de flags (paths relativos, mitad de un par obligatorio, root solapada) salen por stderr antes de servir nada. Un `--root` que atraviesa un symlink hace fallar el arranque con `MCP project authorization initialization failed` y el cliente no recibe respuesta a `initialize`; en macOS, `/var` y `/tmp` (y por tanto `$TMPDIR`) son symlinks a `/private/...`: usa la ruta física (`/private/var/...`, `pwd -P`). |
| `rust.project.open` devuelve `unavailable`/`blocked` | Comprueba macOS/APFS, que la ruta esté dentro de un `--root` autorizado y que no atraviese symlinks. |
| Cualquier tool que ejecuta Cargo devuelve `SANDBOX_DENIED` | Verifica el grupo Docker completo y que `--rust-image` sea exactamente uno de los digests admitidos — ver [runtime-provisioning.md](../operations/runtime-provisioning.md). |
| Tool de escritura devuelve `unavailable` | Falta el `--allow-*-write WORKSPACE_ROOT` correspondiente, o la ruta no coincide byte a byte con la raíz que devolvió `rust.project.open`. |
| `rust.dependencies.audit` no disponible | Falta uno de `--rustsec-snapshot`/`--rustsec-sha256`, o solo se dio uno de los dos. |
| El catálogo aparece `not_configured` | Falta `--catalog-store` o `--catalog-trust` (deben ir juntos, absolutos, fuera de toda `--root`). |
| La búsqueda cae a léxico en vez de híbrida | El binario no se compiló con `--features local`, o falta `--catalog-model-dir`/`--catalog-index-store`. |
| `rust.profile.flamegraph` responde `blocked` sin crear contenedor | Falta `--allow-profiling user-space-sampling`, o el grupo Docker no está completo (la opción se rechaza sin él, no se degrada). |
| Un `project_ref` "deja de funcionar" | Caducó por `--project-ttl-secs` o el servidor se reinició; reabre el proyecto. |
| El cliente parece recibir texto que no es MCP | Algo escribió en `stdout`: no redirijas logs ahí ni envuelvas el binario en un script que imprima por ese canal — MCP lo reserva por completo. |
| Un store de mutación o de artifacts de calidad bloquea nuevas escrituras | Puede tener un journal/registro con bytes desconocidos o `format_version` futuro; no lo edites ni fuerces `prune`. Detén las instancias que lo usan y sigue el procedimiento de recuperación de [`docs/architecture/mutation.md`](../architecture/mutation.md). |

## Si nada de esto resuelve el problema

Revisa [`docs/reference/compatibility.md`](../reference/compatibility.md)
para la matriz de plataforma/protocolo exacta, y
[`docs/reference/limits.md`](../reference/limits.md) para los límites
numéricos que podrían estar causando un `blocked`/`OUTPUT_LIMIT_EXCEEDED`
inesperado.
