# ADR-057 — Operaciones TOML cerradas y selección de paquetes

Date: 2026-09-05

## Context

M2-01 acredita lints del manifest raíz. M2-04/05/06 requieren dependencias,
features, profiles y workspace dependencies. Un parche TOML arbitrario permitiría
introducir paths, wrappers o fuentes no autorizadas. Un workspace puede contener
otros Cargo.toml que no son miembros; estar bajo una read root no basta para
seleccionarlos como paquete de la operación.

## Decision

Extender el editor toml_edit fijado por ADR-051 mediante operaciones tagged y
valores tipados. Mantener el roundtrip exacto de zonas no afectadas, comentarios,
estilo LF/CRLF y no-op byte idéntico. Rechazar layouts de tablas afectados que el
editor no pueda preservar; no reescribir el documento completo como TOML genérico.
El límite de cada manifest continúa en 256 KiB.

`rust.manifest.patch` conserva lints y añade estas familias sobre Cargo.toml raíz:

- `feature_set/remove`: clave de feature ASCII acotada; array de hasta 128 valores
  de feature Cargo, incluyendo `dep:crate`, `crate/feature`, `crate?/feature`.
  No modificar implícitamente otras features al eliminar una clave.
- `profile_set/remove`: profile dev/release/test/bench, setting tipado entre
  opt-level, debug, strip, debug-assertions, overflow-checks, lto, panic,
  incremental y codegen-units; valores cerrados compatibles con Cargo. Remove
  elimina el setting, no un profile completo ni package/build overrides.
- `workspace_dependency_set/remove`: clave y spec de registry crates.io con
  requirement explícito, package alias, features y default-features. Rechazar
  optional en workspace dependencies. No introducir path, git, registry,
  registry-index, patch ni replace.

`rust.dependency.add/remove` seleccionan un manifest relativo existente, que el
metadata Cargo del source capturado debe identificar como miembro del workspace.
Default `Cargo.toml` solo si representa un paquete miembro; un workspace virtual
requiere selección explícita. Kind es normal/dev/build; el target opcional es una
clave Cargo acotada, serializada como clave TOML, nunca como argumento CLI. Cargo
valida su semántica. No aceptar traversal, paths absolutos o manifest ajeno.

Add exige requirement explícito y ofrece package alias, features, optional y
default-features. Una clave ya existente idéntica produce no-op; una definición
diferente produce conflicto, no reemplazo implícito. Remove elimina solo la clave
del paquete/kind/target seleccionado; puede retirar una referencia heredada
`workspace=true` sin modificar su definición global. Ausencia es no-op validado.
No limpiar features, tablas ajenas ni dependencias transitivas por heurística.

Las dos tools tienen permisos host separados `--allow-dependency-add` y
`--allow-dependency-remove`. Los candidatos se ligan respectivamente a tipos
`dependency_add` y `dependency_remove`, nunca intercambiables con manifest patch.
Preview/commit/receipt, límites, TTL y journal son los mecanismos existentes.

Cada edición se reparsea y valida con Cargo. Lints y profiles conservan metadata
frozen no-deps cuando no cambian resolución. Features, workspace dependencies y
add/remove usan ADR-055: datos aprobados offline, resolver sobre copia, metadata
frozen posterior y policy `preserve_presence`. Sin datos requeridos no hay
fallback silencioso. El candidato incluye solo manifest seleccionado y lock
raíz si ya existía; todos los demás bytes, paths y directorios permanecen exactos.
El resolver no puede modificar otros manifests. Un lock transitorio no se publica.

La aplicación controla la operación exacta y membresía; el publisher repite
allowlist de paths, tipos y delta semántico permitidos. No interpretar un check o
metadata exitoso como autorización. El digest liga la operación, before/after
completos, dataset, lock resuelto y validación. No se elige una versión latest.

## Alternatives considered

- TOML/JSON Pointer genérico: amplía superficie de escritura y oculta intención.
- Ejecutar cargo add/remove en host: ejecuta fuera de la frontera autorizada y
  puede incorporar configuración o red del desarrollador.
- Aceptar cualquier Cargo.toml bajo root: confunde lectura con membresía y permiso.
- Resolver con el catálogo SQLite: no contiene el registry requerido por Cargo.
- Actualizar definiciones existentes silenciosamente: una operación add no debe
  sobrescribir decisiones del desarrollador sin un contrato explícito.

## Consequences

El contrato es deliberadamente acotado; layouts o settings fuera de él se editan
con herramientas habituales del desarrollador. No hay nuevas dependencias
estratégicas ni servicio instalado. El dataset Cargo es optativo y solo necesario
para resolución. La implementación debe demostrar aliases, optional/features,
targets, herencia, workspace virtual, no-op y errores Cargo antes de Done.

## Primary sources

- [Cargo features](https://doc.rust-lang.org/cargo/reference/features.html).
- [Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html).
- [Dependency declarations](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html).
- [Workspace inheritance](https://doc.rust-lang.org/cargo/reference/workspaces.html).

## Status

Accepted para implementar M2-04/05/06. M3 y edición arbitraria siguen fuera de scope.
