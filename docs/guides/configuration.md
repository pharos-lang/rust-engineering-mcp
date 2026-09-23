# Configuración

Referencia completa de `serve --stdio` (`crates/mcp-server/src/host_config.rs`).
Todos los paths deben ser absolutos; el servidor rechaza rutas relativas y,
para varios grupos, exige que dos flags aparezcan juntos. Una combinación
inválida hace fallar el **arranque** de `serve`, no una llamada individual.

## Comando mínimo

```bash
rust-engineering-mcp serve --stdio --root /ruta/absoluta/al/proyecto
```

Sin al menos un `--root`, ninguna tool puede abrir un proyecto. `--root` es
repetible hasta **16** veces; cada valor debe ser una ruta absoluta válida en
UTF-8 (`to_str()` debe tener éxito).

## Roots y sesión

| Flag | Repetible | Notas |
| --- | --- | --- |
| `--root PATH` | hasta 16 | Raíz física autorizada para `rust.project.open`. |
| `--project-ttl-secs N` | no | `1..=86400`, default `1800`. TTL de inactividad de un `project_ref`; caduca también al reiniciar el proceso. |

Un `project_ref` no sobrevive a un reinicio del servidor ni se auto-renueva
tras el TTL: reabre el proyecto con `rust.project.open` cuando caduque.

## Runtime Docker (ejecutar Cargo)

Grupo todo-o-nada; cualquier tool que ejecute Cargo, mute el workspace o mida
rendimiento lo requiere completo:

```text
--docker PATH --docker-socket PATH --state-root PATH --rust-image sha256:ID
```

- `--docker PATH`: binario cliente Docker.
- `--docker-socket PATH`: socket Docker.
- `--state-root PATH`: directorio privado de estado; su hijo
  `rust-mcp-mutations-v1` (journal de mutación) y, si se usan tools de
  calidad M3+, `rust-mcp-quality-artifacts-v1` no deben solapar ningún
  `--root`.
- `--rust-image sha256:ID`: debe ser **exactamente** uno de los digests
  admitidos (ver [Operación del runtime](../operations/runtime-provisioning.md)
  para la lista completa e instrucciones de construcción); cualquier otro
  digest hace fallar el arranque, y una imagen "menor" simplemente no sirve
  las tools de milestones posteriores — no degrada su contrato.

Este grupo es prerrequisito de cualquier `--allow-*-write`, de
`--cargo-vendor-dir`/`--vendor-capture` y de `--allow-profiling`.

## Grants de escritura

Ninguno está activo por defecto. Cada flag es repetible hasta 16 veces, una
por raíz de workspace exacta (la que devuelve `rust.project.open`), y esa raíz
debe estar dentro de un `--root` ya autorizado:

| Flag | Habilita |
| --- | --- |
| `--allow-manifest-write WORKSPACE_ROOT` | `rust.manifest.patch` |
| `--allow-fmt-write WORKSPACE_ROOT` | `rust.fmt.apply` |
| `--allow-fix-write WORKSPACE_ROOT` | `rust.fix.apply` |
| `--allow-dependency-add WORKSPACE_ROOT` | `rust.dependency.add` |
| `--allow-dependency-remove WORKSPACE_ROOT` | `rust.dependency.remove` |
| `--allow-analyzer-action-write WORKSPACE_ROOT` | `rust.analyzer.action.apply` (preview) |

Un grant no autoriza planes, receipts o recovery de otra tool ni de otra raíz.
Detalle del ciclo preview/commit/receipt: [`docs/architecture/mutation.md`](../architecture/mutation.md).

## Datos Cargo opcionales (vendor)

Para features, workspace dependencies y `dependency.add/remove` en resolución
que lo requiera:

```text
--cargo-vendor-dir PATH --cargo-vendor-tree-sha256 sha256:ID
```

Ambos flags son obligatorios juntos; el directorio no puede solapar ninguna
root. Se preparan fuera del servidor con el Cargo del operador y
`rust-engineering-mcp cargo-vendor inspect --directory PATH --json` (no
ejecuta Cargo ni descarga nada). Requiere el grupo Docker.

Para `rust.benchmark.run` con Criterion (que no cabe en el límite de un vendor
normal), existe una captura ADR-078 separada:

```text
--vendor-capture PATH --vendor-capture-tree-sha256 sha256:ID
```

Producida con `rust-engineering-mcp cargo-vendor capture --directory PATH --into PATH [--json]`.
Ambos flags de captura, y el subcomando `cargo-vendor capture` que los
produce, están en `--help` (`serve --help`, `cargo-vendor --help`).

## Seguridad y advisories

```text
--security-policy PATH --security-policy-sha256 sha256:ID
--rustsec-snapshot PATH --rustsec-sha256 sha256:ID
```

- `--security-policy*`: policy cerrada para `rust.deny`, `rust.supply_chain.inspect`
  y `rust.quality.gate.v2`; el path debe quedar fuera de todas las roots.
- `--rustsec-snapshot*`: snapshot RustSec firmado/preparado por el operador
  para `rust.dependencies.audit` y las tools que lo consumen. Ninguno se
  descarga durante `serve`.

## Catálogo local

```text
--catalog-store PATH --catalog-trust PATH
--catalog-model-dir PATH --catalog-index-store PATH
```

`--catalog-store`/`--catalog-trust` deben ir juntos, ser absolutos y quedar
**fuera** de toda `--root`. `--catalog-index-store` (búsqueda semántica,
binario compilado con `--features local`) requiere además
`--catalog-model-dir`. No hay ruta por defecto para ninguno de los cuatro: sin
flags, el catálogo aparece `not_configured` en `doctor`/`rust.catalog.status`.
Administración (sync/import/rebuild) fuera del runtime:
[Mantenimiento del catálogo](../operations/catalog-maintenance.md).

## Profiling

```text
--allow-profiling user-space-sampling
```

Único valor aceptado; repetirla o pasar otro valor invalida el arranque de
`serve`. Requiere el grupo Docker completo. Habilita exactamente el muestreo
de espacio de usuario que usa `rust.profile.flamegraph`, con una sola syscall
añadida al perfil seccomp del contenedor de perfilado — no añade capabilities
Linux ni usa `sudo`. Sin ella, `rust.profile.flamegraph` responde `blocked`/
`PROFILING_NOT_AUTHORIZED` antes de crear ningún contenedor. Este flag está
documentado en `serve --help`.

## Variables de entorno

`serve` no lee ninguna variable de entorno de producción para configurar
roots, grants, catálogo o runtime — toda la configuración es por flags de CLI
(`host_config.rs` no llama `std::env::var` fuera de tests). El servidor
**nunca lee `RUST_LOG`**: es una decisión deliberada
(`crates/mcp-server/src/stdio.rs`, comentario junto al subscriber de
`tracing`) porque los diagnósticos del SDK MCP pueden incluir payloads del
peer; solo los logs propios del proceso salen por `stderr`, con nivel fijo. El
binario compilado con la feature `test-hooks` reacciona a
`RUST_MCP_TEST_TASK_*`/`RUST_MCP_TEST_TASKS_READY`, pero esas variables son
exclusivas de la suite de pruebas y no existen en un binario de producción sin
esa feature.

## Rutas de datos

No hay un layout local por defecto: cada ruta (`--state-root`,
`--catalog-store`, `--catalog-trust`, `--catalog-model-dir`,
`--catalog-index-store`, `--cargo-vendor-dir`, `--vendor-capture`,
`--security-policy`, `--rustsec-snapshot`) la elige y provisiona el operador de
forma explícita. Un layout fijo tipo
`~/.rust-engineering-mcp/{catalog,vectors,embeddings,rustsec,cache,artifacts}`
aparece en la especificación original pero **no está implementado**: es una
limitación documentada, no una convención activa — no asumas esa ubicación al
escribir tu propia configuración.
