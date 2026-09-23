# CLI

Todos los subcomandos del binario `rust-engineering-mcp`
(`crates/mcp-server/src/main.rs`, función `invocation()`). El uso de red se
señala explícitamente en cada subcomando: el runtime MCP (`serve`) nunca
sincroniza, descarga ni instala nada por su cuenta — esa es siempre una
operación CLI explícita, aparte.

## Ayuda por comando: `<comando> --help` / `-h`

Cada comando de nivel superior acepta su propio `--help`/`-h` justo después
del comando (`doctor --help`, `serve --help`, `contract --help`,
`catalog --help`, `mutation --help`, `quality-artifacts --help`,
`cargo-vendor --help`, `capabilities --help`, `security-runtime --help`,
`version --help`): imprime la sección de ese comando por stdout y sale con
código 0. Para los comandos con subcomandos, `<comando> <subcomando> --help`
(p. ej. `catalog status --help`, `mutation prune -h`,
`cargo-vendor capture --help`, `quality-artifacts recover -h`,
`security-runtime inventory --help`) imprime la misma sección de su comando
padre — una sección por comando, no una por subcomando. `serve --help`
resuelve antes de exigir `--stdio`, así que nunca llega a
`host_config::parse` ni arranca el servidor.

`--help`/`-h` solo se reconoce justo después del comando (o del
subcomando); cualquier otra posición, o un argumento extra a continuación
(`serve --stdio --help`, `catalog status --help extra`), sigue devolviendo el
error fijo de siempre:

```text
Unsupported invocation. Use 'rust-engineering-mcp --help'.
```

con código de salida 2, igual que un comando o subcomando desconocido.
`rust-engineering-mcp help` / `--help` / `-h` (sin comando previo) imprime el
texto completo reproducido más abajo, que se compone de las mismas secciones
por comando (`crates/mcp-server/src/help.rs`) que devuelve cada
`<comando> --help` — una sola fuente de verdad para ambas superficies.

## Texto de ayuda (`help` / `--help` / `-h`)

```text
Rust Engineering MCP — development server

Usage: rust-engineering-mcp <COMMAND>

Commands:
  quality-artifacts recover --state-root PATH [--json]
  quality-artifacts prune --state-root PATH [--json]
                 Reconcile quarantined/unknown-version objects, or expire artifacts past TTL (ADR-061); local only, never called by MCP tools
  mutation list --state-root PATH [--json]
  mutation prune --state-root PATH --operation-id ID --plan-digest sha256:ID [--json]
                 Inspect journals or remove one completed local receipt explicitly
  cargo-vendor inspect --directory PATH [--json]
  cargo-vendor capture --directory PATH --into PATH [--json]
                 Fingerprint an offline vendor tree for --cargo-vendor-dir/--cargo-vendor-tree-sha256, or capture
                 the larger Criterion tree for --vendor-capture/--vendor-capture-tree-sha256; never runs Cargo
                 or touches the network
  catalog status --store PATH --trust PATH [--model-dir PATH [--index-store PATH]] [--json]
  catalog import SNAPSHOT --store PATH --trust PATH [--model-dir PATH] [--json]
  catalog sync --source SNAPSHOT --store PATH --trust PATH [--model-dir PATH] [--json]
  catalog sync --url HTTPS_URL --allow-host HOST --store PATH --trust PATH [--model-dir PATH] [--json]
  catalog rebuild-index --store PATH --trust PATH --model-dir PATH --index-store PATH [--json]
                 --store/--trust are always required together, outside every serve --root; sync accepts
                 exactly one of --source or --url with --allow-host (the only subcommand in this binary
                 that makes a real network request, and only to that declared host)
  version [--json] Show package version/build facts (-V, --version)
  security-runtime inventory [--json]
                 Show compiled security runtime requirements; does not inspect or install
  doctor [--active] [--json] [same host flags as serve --stdio]
  doctor [--active] [--json] --state-root PATH
                 Diagnose configured local state; --active calibrates the approved Rust runtime
                 (requires the full --docker/--docker-socket/--state-root/--rust-image group). The
                 lone --state-root form (no Docker group) only computes mutation_journals.
  capabilities [--json | --human] --docker PATH --docker-socket PATH --state-root PATH --probe-image sha256:ID
                 Actively probe the approved local sandbox; --json is the default format
  contract [--json | --human]
                 Static spec §56 capabilities document: all 36 tool definitions, stability,
                 canonical schema/description hashes and runtime requirements; no host access
  serve --stdio [--root PATH]... [--project-ttl-secs N]
        [--catalog-store PATH --catalog-trust PATH [--catalog-model-dir PATH [--catalog-index-store PATH]]]
        [--allow-manifest-write WORKSPACE_ROOT]...
        [--allow-fmt-write WORKSPACE_ROOT]...
        [--allow-fix-write WORKSPACE_ROOT]...
        [--allow-analyzer-action-write WORKSPACE_ROOT]...
        [--allow-dependency-add WORKSPACE_ROOT]...
        [--allow-dependency-remove WORKSPACE_ROOT]...
        [--cargo-vendor-dir PATH --cargo-vendor-tree-sha256 sha256:ID]
        [--vendor-capture PATH --vendor-capture-tree-sha256 sha256:ID]
        [--allow-profiling user-space-sampling]
        [--security-policy PATH --security-policy-sha256 sha256:ID]
        [--rustsec-snapshot PATH --rustsec-sha256 sha256:ID]
        [--docker PATH --docker-socket PATH --state-root PATH --rust-image sha256:ID]
                 Serve MCP with host-authorized physical roots (default: none). --root and every
                 --allow-*-write are each repeatable up to 16 times. --docker/--docker-socket/
                 --state-root/--rust-image is one all-or-nothing group, required by any --allow-*-write,
                 --cargo-vendor-dir, --vendor-capture and --allow-profiling. Write grants are never
                 active by default.
  help           Show this help (-h, --help)
```

Este bloque, y cada `<comando> --help`, se generan desde las mismas
constantes de `crates/mcp-server/src/help.rs` — no hay un segundo texto que
mantener sincronizado a mano.

## `serve --stdio` — arranca el servidor MCP

Todos los detalles de flags, grants y rutas de datos están en
[`../guides/configuration.md`](../guides/configuration.md). `serve` nunca
toca la red: ni siquiera `catalog sync --url` es alcanzable desde el proceso
`serve` — es un subcomando aparte. Un flag inválido o una combinación
incompleta (por ejemplo `--docker` sin `--docker-socket`) hace fallar el
**arranque** del proceso, nunca una llamada individual.

## `catalog` — administración del catálogo local (sin red salvo un caso)

```text
catalog status --store PATH --trust PATH [--model-dir PATH [--index-store PATH]] [--json]
catalog import SNAPSHOT --store PATH --trust PATH [--model-dir PATH] [--json]
catalog sync --source SNAPSHOT --store PATH --trust PATH [--model-dir PATH] [--json]
catalog sync --url HTTPS_URL --allow-host HOST --store PATH --trust PATH [--model-dir PATH] [--json]
catalog rebuild-index --store PATH --trust PATH --model-dir PATH --index-store PATH [--json]
```

**`catalog sync --url HTTPS_URL --allow-host HOST` es el único subcomando de
todo el binario que hace una petición de red real** (HTTPS, vía `reqwest` en
`catalog_sync.rs`), y solo hacia el host que el operador declaró
explícitamente con `--allow-host`. Todos los demás (`status`, `import`,
`catalog sync --source`, `rebuild-index`) son estrictamente locales. Detalle
del formato, la firma y las cuotas: [`data-formats.md`](data-formats.md) y
[`../operations/catalog-maintenance.md`](../operations/catalog-maintenance.md).

## `mutation` — administración del journal (local, sin red)

```text
mutation list --state-root PATH [--json]
mutation prune --state-root PATH --operation-id ID --plan-digest sha256:ID [--json]
```

`mutation list` sobre una `--state-root` que existe pero nunca tuvo una
mutación (sin `rust-mcp-mutations-v1`) devuelve vacío
(`status: "passed"`, `records: []`, `store_initialized: false`) sin crear ese
directorio. Una `--state-root` que no existe en absoluto es un error distinto
(`status: "blocked"`, `error_code: "not_found"`). `mutation prune` retira un
recibo terminal ya completado; no revierte ni reintenta una operación viva.
Detalle del ciclo `preview`/`commit`/`receipt`:
[`../architecture/mutation.md`](../architecture/mutation.md).

## `quality-artifacts` — administración del store durable M3+ (local, sin red)

```text
quality-artifacts recover --state-root PATH [--json]
quality-artifacts prune --state-root PATH [--json]
```

`recover` reconcilia objetos corruptos/de versión desconocida (cuarentena
explícita, nunca reinterpretación); `prune` elimina artifacts expirados por
TTL. Ambos operan sobre `rust-mcp-quality-artifacts-v1` bajo el mismo
`--state-root`. Ver [`../operations/backup-and-recovery.md`](../operations/backup-and-recovery.md).

## `cargo-vendor` — inspección de datos Cargo offline (local, sin red)

```text
cargo-vendor inspect --directory PATH [--json]
cargo-vendor capture --directory PATH --into PATH [--json]
```

Ninguno ejecuta Cargo ni descarga nada — ambos son operaciones de
verificación/preparación sobre bytes que el operador ya obtuvo con su propio
`cargo vendor`. `inspect` fingerprint-a un vendor tree estándar para
`--cargo-vendor-dir`/`--cargo-vendor-tree-sha256`. `capture` produce la
captura ampliada que necesita `rust.benchmark.run` para Criterion
(`--vendor-capture`/`--vendor-capture-tree-sha256`), nombrando el artifact por
su propio digest. Detalle de cuotas:
[`limits.md`](limits.md#vendor-cargo-offline-y-captura-de-benchmarking).

## `doctor` — diagnóstico pasivo (local; `--active` calibra el runtime)

```text
doctor [--active] [--json] [las mismas flags de host que serve --stdio]
doctor [--active] [--json] --state-root PATH
```

Sin `--active`, `doctor` solo abre los archivos/paths ya configurados con los
adapters seguros del proyecto — no ejecuta subprocesos ni adquiere la lease
administrativa de ningún store. Con `--active` y el runtime Docker
configurado, además calibra el runtime aprobado (observa rustc/cargo/componentes
dentro de la imagen). `doctor` acepta, como única excepción, `--state-root`
solo (sin el resto de la tupla Docker) únicamente para calcular la sección
`mutation_journals` — `serve` en cambio siempre exige la tupla Docker
completa si se pasa cualquiera de sus partes. Nunca sincroniza, instala ni
repara nada automáticamente: sus acciones reportadas son recomendaciones.

## `capabilities` — probes activos del sandbox aprobado (local, sin red)

```text
capabilities [--json | --human] --docker PATH --docker-socket PATH --state-root PATH --probe-image sha256:ID
```

Crea sockets locales y contenedores de prueba contra la imagen probe
declarada; no hace descargas ni consultas externas. `--json` es el formato
por defecto; `--human` representa el mismo resultado en texto.

## `contract` — documento estático de capacidades (local, sin red, sin acceso al host)

```text
contract [--json | --human]
```

Emite el documento `rust_engineering_capabilities` (`format_version: 1`): las
36 definiciones de tool, su clase de estabilidad, hashes canónicos de schema
de entrada/salida/descripción y su requisito de runtime — sin tocar ningún
archivo del host ni requerir ninguna flag. Solo `--json` es el contrato
`stable`; `--human` es informativo. Ver
[`compatibility.md`](compatibility.md#congelación-de-contrato-080-y-verificación).

## `version` — identidad del binario (local, sin red)

```text
version [--json]
--version / -V
```

`--json` añade `format_version: 1`, `package`, `version`, `compiled_local`,
`target_os` y `target_arch`. No demuestra capabilities de esa plataforma —
solo identidad de build.

## `security-runtime inventory` — inventario compilado (local, sin red)

```text
security-runtime inventory [--json]
```

Reporta las identidades de runtime de seguridad compiladas en el binario
(imagen aprobada, `cargo-deny` 0.19.7, unsafe-scanner-helper 0.1.0, Miri
nightly y su sysroot) con `installation_observed: false` explícito: **nunca
inspecciona ni instala nada** en el host — es un espejo de constantes
compiladas, no una consulta al sistema de archivos.

## `help` / `--help` / `-h`

Ver el texto reproducido arriba. `-h` y `--help` son equivalentes a `help`
como primer argumento.

## Decisiones relacionadas

Ver [`../architecture/decisions.md`](../architecture/decisions.md) para el
mapa de ADRs que fijan estos contratos de CLI (doctor, catálogo, mutación,
vendor, contrato estático).
