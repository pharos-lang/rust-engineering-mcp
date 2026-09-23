# Mantenimiento del catálogo

El catálogo local de crates (SQLite autoritativo + FTS5 léxico, con LanceDB
como índice semántico derivado) se administra **siempre por CLI, fuera de una
sesión MCP**. `serve` solo lee las rutas que le configures
(`--catalog-store`/`--catalog-trust`/`--catalog-model-dir`/`--catalog-index-store`,
ver [Configuración](../guides/configuration.md)); nunca sincroniza, descarga
ni reconstruye nada por su cuenta. Esta separación es una regla arquitectónica
del proyecto (`AGENTS.md`), no solo una conveniencia operativa.

## Comandos (`rust-engineering-mcp catalog ...`)

```text
catalog status --store PATH --trust PATH [--json]
catalog import SNAPSHOT --store PATH --trust PATH [--json]
catalog sync --source SNAPSHOT --store PATH --trust PATH [--json]
catalog sync --url HTTPS_URL --allow-host HOST --store PATH --trust PATH [--json]
catalog rebuild-index --store PATH --trust PATH --index-store PATH --model-dir PATH [--json]
```

- `catalog status`: reporta disponibilidad, identidad y frescura del store
  configurado; no modifica nada.
- `catalog import SNAPSHOT`: importa un snapshot firmado local (positional
  argument, una ruta de archivo) hacia `--store`/`--trust`.
- `catalog sync --source SNAPSHOT`: igual que `import` pero por la ruta de
  sincronización — sigue siendo un archivo local, sin red.
- `catalog sync --url HTTPS_URL --allow-host HOST`: **el único subcomando que
  hace una petición HTTPS real** (vía `reqwest`, en `catalog_sync.rs`).
  `--allow-host` es un allowlist explícito: el operador declara el host antes
  de que el comando pueda contactarlo. Esto corresponde al modo
  "online-controlado" de la especificación original; los otros dos modos
  descritos allí (mirror corporativo, importación air-gapped firmada) **no
  están confirmados como implementados de forma independiente** en este
  corte — trátalos como no verificados, no como disponibles.
- `catalog rebuild-index`: reconstruye los objetos LanceDB nativos a partir de
  los hechos ya presentes en SQLite y del modelo E5 ya instalado en
  `--model-dir`; requiere además `--index-store`. No descarga el modelo ni lo
  entrena — solo usa uno que el operador ya instaló.

Ningún subcommand de `catalog` acepta rutas relativas de forma implícita ni
tiene un destino por defecto: siempre pasas `--store`/`--trust` explícitos.

## Qué hace autoritativo cada store

- **SQLite** es la fuente de verdad: crates, versiones, features,
  dependencias, licencias, sources y metadata de sincronización. Todo filtro
  autoritativo (MSRV, licencia, yanked) se resuelve contra SQLite, nunca
  contra el índice vectorial.
- **LanceDB** es siempre derivado: se reconstruye con `catalog rebuild-index`
  y nunca decide un hecho por sí solo. Si su fingerprint no coincide con el
  de SQLite, el servidor lo marca inválido y cae a búsqueda léxica/metadata en
  vez de devolver resultados semánticos inconsistentes.
- El índice semántico solo existe si compilaste el binario con
  `--features local` (habilita el path E5/ORT) y configuraste
  `--catalog-model-dir`/`--catalog-index-store`.

## Freshness: `latest_known`, nunca "en vivo"

Toda respuesta que dependa de un snapshot del catálogo o de advisories
declara su `provenance`/`freshness` con el estado `latest_known` — nunca se
presenta como una consulta en tiempo real, incluso aunque el archivo se
sincronizó hace un minuto. `rust.catalog.status` y `doctor` (verificaciones
`CatalogFreshness`/`ModelFreshness`/`RustsecFreshness`) son la forma de
comprobar cuándo se sincronizó cada dataset por última vez.

## Advisories RustSec

El snapshot de advisories RustSec **no** se administra con el subcomando
`catalog`: es un archivo preparado por el operador y pasado directamente a
`serve`/`doctor` con `--rustsec-snapshot PATH --rustsec-sha256 sha256:ID` (ver
[Configuración](../guides/configuration.md)). No hay comando CLI que lo
descargue; el operador lo obtiene y verifica por su cuenta antes de
configurarlo.

## Rutas de almacenamiento

No existe un layout local por defecto. Cada ruta
(`--catalog-store`, `--catalog-trust`, `--catalog-model-dir`,
`--catalog-index-store`) es la que tú eliges y debe quedar **fuera** de toda
`--root` de proyecto. Un layout fijo del tipo
`~/.rust-engineering-mcp/{catalog,vectors,embeddings,rustsec,cache,artifacts}`
aparece descrito en la especificación original del proyecto pero no está
implementado — es una limitación conocida, no una convención que el binario
siga hoy; no la asumas al escribir scripts de operación.

## Vendor Cargo (dato relacionado, no parte del catálogo)

El directorio vendor Cargo que usan las tools de mutación y M4/M5 es un
concepto separado del catálogo de crates: es una copia offline y verificada
de las dependencias de un proyecto concreto, no metadata general del
ecosistema. Se prepara con `cargo vendor` y se inspecciona/captura con
`rust-engineering-mcp cargo-vendor inspect|capture` — ver
[Configuración](../guides/configuration.md#datos-cargo-opcionales-vendor).
