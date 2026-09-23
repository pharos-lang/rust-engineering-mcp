# Flujos de trabajo típicos

Cada flujo asume el servidor ya conectado ([Clientes](clients.md)) con las
flags de host necesarias ([Configuración](configuration.md)). Los nombres de
tool y su contrato completo están en
[`docs/reference/tools.md`](../reference/tools.md); esta guía solo ordena las
llamadas.

## 1. Abrir un proyecto e inspeccionarlo

Base de todo flujo: ningún tool salvo `rust.project.open` acepta una ruta de
proyecto directamente.

1. `rust.project.open { "path": "/ruta/absoluta/al/proyecto" }` → devuelve
   `project_ref`, `workspace_root` y `fingerprint`. Esta llamada **no**
   requiere runtime Docker: valida estructura y registra la referencia sin
   ejecutar Cargo — test de protocolo real por el wire MCP sin flags de
   runtime configurados, que además comprueba que no aparecen `target/` ni
   `Cargo.lock`:
   `crates/mcp-server/tests/protocol.rs::project_open_succeeds_with_host_root_in_modern_and_all_legacy_versions`.
2. Guarda `project_ref`; expira por inactividad (`--project-ttl-secs`,
   default 1800 s) y nunca sobrevive a un reinicio del servidor.
3. `rust.project.inspect { "project_ref": ... }` → packages, targets,
   features, perfiles, toolchain y dependencias declaradas. **Esta llamada sí
   requiere el runtime Docker aprobado**; sin él responde `blocked` con
   `SANDBOX_DENIED` (verificado en la misma sonda). Ver
   [Operación del runtime](../operations/runtime-provisioning.md).
4. `rust.toolchain.inspect` si necesitas versión/targets/componentes del
   runtime antes de generar código dependiente de una feature de toolchain.

Reabre el proyecto si el código cambió o si el `project_ref` caducó: los
resultados posteriores se validan contra el `fingerprint` original.

## 2. Comprobación de calidad puntual, y el gate compuesto

Ejecuta la comprobación más pequeña que responda la pregunta:

- `rust.check` — equivalente tipado a `cargo check`.
- `rust.fmt.check` — solo lectura, diff cuando es pequeño.
- `rust.clippy` — perfiles cerrados (`default`/`strict`/`pedantic`/`project`).
- `rust.test` — filtro de tests, features, target, timeout.
- `rust.dependencies.audit` — RustSec contra el snapshot del host (requiere
  `--rustsec-snapshot`/`--rustsec-sha256`).

Cuando necesitas una evaluación compuesta:

- `rust.quality.gate { "profile": "fast" }` → fmt-check + check + clippy.
- `rust.quality.gate { "profile": "standard" }` → añade test + audit.

Todas ejecutan Cargo dentro del sandbox aprobado y devuelven diagnósticos
normalizados con sus spans, no texto crudo de stdout. Lee los Resources
devueltos cuando una tool publique logs acotados en vez de inline.

## 3. Mutación con grants explícitos (preview → commit → receipt)

Requiere el grant específico de la tool y el grupo Docker completo (ver
[Configuración](configuration.md#grants-de-escritura)). Ninguna tool de
escritura actúa sin su flag `--allow-*-write`.

1. **Preview**: llama la tool de mutación (`rust.fmt.apply`,
   `rust.fix.apply`, `rust.dependency.add`, `rust.dependency.remove`,
   `rust.manifest.patch`, o en preview `rust.analyzer.action.apply`) sin
   confirmar. Devuelve un plan con diff exacto y expira (600 s); no escribe
   nada todavía.
2. Revisa el diff exacto devuelto. `build.rs` y proc macros pueden influir en
   el resultado — no asumas que un cambio de manifest es inerte.
3. **Commit**: repite la llamada con el plan/digest y una idempotency key.
   El servidor revalida la generación completa antes de escribir.
4. Reabre el proyecto y usa el **nuevo** `project_ref` que devuelve el commit
   en todas las llamadas posteriores, incluidas receipt/recovery.
5. Ejecuta `rust.check` (o el gate) después de commit para confirmar que el
   resultado compila. Las cinco tools M2 (`rust.fmt.apply`, `rust.fix.apply`,
   `rust.dependency.add`, `rust.dependency.remove`, `rust.manifest.patch`) sí
   validan el candidato con Cargo antes de exponerlo en el preview (ADR-050,
   punto 4: "Preview produce candidato/diff/digest exactos únicamente
   después de que Cargo valide el candidato en el gateway") y
   `rust.fix.apply` corre además un `cargo check` independiente
   (ADR-056); eso no sustituye tu propio `rust.check` posterior, porque la
   validación ocurre sobre el candidato, no sobre el estado final tras
   commit. `rust.analyzer.action.apply` es la única excepción: es
   **puramente estructural**, sin ninguna verificación por compilación en
   el momento de aplicarse.
6. Si aparece `recovery_required`, no toques los temporales
   `.rust-mcp-mut-*.swap` ni el journal: consulta el receipt con
   `recover: true` — ver [Solución de problemas](troubleshooting.md).

## 4. Buscar y consultar el catálogo local de crates

Requiere `--catalog-store`/`--catalog-trust` configurados (y, para semántico,
`--catalog-model-dir`/`--catalog-index-store` con el binario compilado
`--features local`):

1. `rust.catalog.status` — disponibilidad, identidad y frescura del catálogo
   (usa siempre `latest_known`, nunca una promesa de tiempo real).
2. `rust.crate.search { "query": "..." }` — léxico, semántico o híbrido según
   lo que esté disponible; sin índice semántico, cae a léxico automáticamente.
3. `rust.crate.inspect { "crate": "...", "version": "..." }` — snapshot
   autoritativo de versiones, features, dependencias y advisories.

Ninguna de las tres ejecuta red: el catálogo se sincroniza fuera del runtime
MCP — ver [Mantenimiento del catálogo](../operations/catalog-maintenance.md).

## 5. Analyzer (preview, checkout de desarrollo)

Las cinco tools `rust.analyzer.*` son clase **preview** (deuda de contrato
conocida; solo disponibles compilando desde fuente hoy, no en la release
`v0.3.0`) y requieren la imagen guest específica del analyzer:

1. `rust.analyzer.symbols` / `rust.analyzer.references` /
   `rust.analyzer.diagnostics` — solo lectura, sin flag de host adicional más
   allá de `--rust-image` apuntando a esa imagen.
2. `rust.analyzer.actions { "range": ... }` — lista code actions disponibles
   con su `action_digest`, sin aplicar nada.
3. `rust.analyzer.action.apply` — aplica una acción listada, mismo ciclo
   preview/commit/receipt que el resto de mutación M2; exige
   `--allow-analyzer-action-write WORKSPACE_ROOT`. Si el `action_digest`
   cambió desde el preview, devuelve `ACTION_STALE` en vez de aplicar algo
   distinto de lo revisado.

Ninguna tool `analyzer.*` ejecuta `build.rs`, proc macros ni `checkOnSave`; un
proyecto con `rust-analyzer.toml`/`.rust-analyzer.toml` propio se rechaza
antes de arrancar el analizador.
