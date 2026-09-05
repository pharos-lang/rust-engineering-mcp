# ADR-056 — Cargo fix y coordinación interna aislada

Date: 2026-09-05

## Context

Cargo 1.98.1 usa un servicio TCP para coordinar la aplicación de fixes. El probe
D06 demuestra que el perfil seccomp existente deniega ese mecanismo, y que
`--default-features` no es un flag válido de cargo fix en esta versión. El owner
pidió continuar M2 sin añadir servicios instalados ni carga de configuración.

## Decision

Aceptar una variante tipada `Fix` del productor de candidatos del Execution
Gateway. Reutilizar staging, exportación estricta, cuotas y cleanup de ADR-053,
el publisher de reemplazos existentes de ADR-054 y la imagen ya aprobada. No
añadir ejecutable host, daemon persistente, flags libres ni instalación adicional.

Invocación fija:

```text
/opt/rust/bin/cargo fix --workspace --all-targets --frozen --offline --allow-no-vcs --allow-dirty --allow-staged --message-format=json --color never --target-dir /target
```

Las features por defecto se seleccionan omitiendo flags de features. M2 no ofrece
edition migration, broken-code, selección arbitraria de targets ni argumentos
Cargo. Los permisos VCS se refieren exclusivamente a la copia capturada sin .git;
el plan conserva y compara los bytes dirty reales, sin reset, stash ni commit Git.

Solo esta fase usa un perfil seccomp dedicado incorporado al binario y verificado
contra la configuración aplicada: perfil existente más `socket` AF_INET,
SOCK_STREAM, protocolo 0 y `bind`, `connect`, `listen`, `accept4`, `getsockname`,
`setsockopt`, `shutdown`. Sigue `network=none`, sin interfaces externas, host
network, socket Docker, secretos ni bind host. Se permite TCP en loopback dentro
de su namespace; no afirmar denegación absoluta de sockets ni autenticidad de los
mensajes internos de Cargo. No ampliar el perfil M1, fmt, ingest ni exporter.
El target de compilación es tmpfs ejecutable, acotado y separado de source.

Éxito exige exit 0, captura completa acotada, OOM false, JSON Cargo válido con
build-finished success, export completo y cleanup confirmado. Stderr admite
progreso normal acotado. Después ejecutar `cargo check` frozen independiente sobre
el candidato, con el perfil M1. Una salida no cero, malformed, timeout, cancelación,
overflow, cambio de scope o incertidumbre descarta todos los bytes candidatos.

`rust.fix.apply` usa preview/commit/receipt y permiso host separado
`--allow-fix-write WORKSPACE_ROOT`. Solo puede reemplazar hasta 128 archivos `.rs`
existentes; manifests, lock, directorios y paths permanecen idénticos. Inicialmente
requiere lock existente aceptado por Cargo frozen. La autoridad y el digest son
de tipo `fix_apply`; no se aceptan planes o receipts de otra operación. El recibo
liga fingerprints del mutador y check, runtime, fuente y policy exactos.

Cargo fix ejecuta build.rs y proc macros dentro de source guest escribible. Ese
código puede influir en cualquier byte permitido del candidato; un check exitoso
no prueba que cada cambio provenga de una sugerencia del compilador. La revisión
del diff exacto, su autorización y la allowlist del publisher delimitan el efecto.

## Alternatives considered

- Ampliar seccomp global: añade capacidad a operaciones que no la necesitan.
- Cargo fix en host: ejecuta código del proyecto fuera de containment.
- Reimplementar rustfix y coordinación Cargo: añade un motor semántico y trabajo
  de compatibilidad que Cargo ya mantiene.
- Instalar un broker o runtime nuevo: coste operacional sin beneficio para este
  mecanismo de coordinación dentro del guest existente.
- Rechazar fix por cualquier socket: impediría el mecanismo real de Cargo aun
  manteniendo la ausencia de conectividad externa.

## Consequences

La instalación y los cuatro parámetros existentes del sandbox no cambian. Solo
se añade el opt-in de escritura de esta operación. El perfil admite coordinación
TCP interna para código no confiable; el kernel y aislamiento Docker continúan
siendo la frontera. La prueba std-only D06 no acredita workspaces arbitrarios,
dependencias offline, código hostil ni todas las salidas del compilador. Esas
pruebas y revisión del perfil de producción son necesarias antes de M2-03 Done.

## Evidence

- [D06: 121 observaciones, cancelación y cleanup](../validation/M2-D06-cargo-fix-qualification.md).
- [Cargo fix oficial](https://doc.rust-lang.org/cargo/commands/cargo-fix.html).
- [Implementación en el commit del runtime](https://github.com/rust-lang/cargo/blob/797e8a9bca276c1c9f9f738d2a20f484fa4eea9d/src/cargo/ops/fix/mod.rs).

## Status

Accepted para implementar M2-03. No cambia la calificación ni el perfil de M1.
