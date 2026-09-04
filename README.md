# Rust Engineering MCP

Servidor MCP de ingeniería Rust, publicado como código abierto por IUMotion Labs,
con M0 cerrada y M1 todavía bloqueada para distribución binaria.
`rust.project.open` registra una raíz autorizada por el host con validación
estructural acotada, acceso no-follow y referencia opaca. `rust.project.inspect`
inspecciona declaraciones capturadas mediante el runtime Rust aprobado.
`rust.toolchain.inspect` observa su inventario instalado y versiones. El acceso protegido
requiere macOS26+ y APFS, con probes que fallan cerrados si faltan garantías.
`rust.check` valida compilación capturada y devuelve diagnósticos/logs efímeros
con autorización de Resources. `rust.fmt.check` comprueba formato configurado del
workspace y devuelve archivos/diff acotado sin escribir las fuentes. `rust.clippy`
aporta findings estructurados con perfiles cerrados y logs autorizados. `rust.test`
ejecuta tests acotados, distingue compilación de ejecución y conserva salida del
harness en Resources. `rust.dependencies.audit` correlaciona el lock capturado con
un snapshot RustSec explícito verificado contra el hash del host; no descarga datos.
`rust.diagnostics.explain` devuelve texto del compilador instalado con identidad/hash.
`rust.quality.gate` compone perfiles fast/standard sobre una captura y conserva cada
etapa y sus logs. `rust.catalog.status` añade la undécima tool: observa disponibilidad
verificada y freshness del contexto local; M1-11 está validado en [su recibo](docs/validation/M1-11.md).
`rust.crate.search` incorpora la duodécima tool con modos lexical/semantic/hybrid
y filtros de versiones desde SQLite; su [gate M1-12 está aprobado](docs/validation/M1-12.md).
`rust.crate.inspect` incorpora la decimotercera tool con páginas de hechos SQLite;
su [gate M1-13 está aprobado](docs/validation/M1-13.md). La administración CLI explícita conserva
su [evidencia M1-10](docs/validation/M1-10.md).

El workspace incluye ocho crates: domain, application, MCP, project, execution,
catalog, semantic y artifact. SQLite/FTS5 conserva los facts; E5 verificado y
LanceDB aportan recuperación derivada local; ArtifactStore conserva bytes acotados
redactados en memoria. Estado, búsqueda e inspección paginada del catálogo son públicos.
Resources expone únicamente logs autorizados.

## Uso del bootstrap

Rust/Cargo **1.98.1**, edition2024, fijados en rust-toolchain.toml y manifests.
Las dependencias del lock deben estar aprovisionadas antes de trabajar offline.

```text
cargo run --locked --offline -- version
cargo run --locked --offline -- --help
cargo run --locked --offline -- serve --stdio --root /ruta/fisica/autorizada
```

El servidor admite hasta16 roots del host y `--project-ttl-secs N` (1..86400,
default1800). Sin roots no hay acceso a proyectos. stdout contiene solo protocolo;
tracing escribe stderr. EOF limpio termina con0, error de transporte/bootstrap con1,
uso inválido con2. Entrada y salida por línea limitadas a1MiB; frames incompletos al EOF se
rechazan. Frames parciales y escrituras tienen deadline total de10s. ADR-030
limita workers, peticiones y envíos; la cancelación repetida puede exigir reconectar. rmcp3.2.0 gestiona JSON-RPC, stdio y negociación; el harness prueba cinco
versiones MCP. Ver [compatibilidad y límites](docs/compatibility.md).

## Gate local

Sobre este repositorio revisado y con los prerrequisitos explícitos de [CI](docs/ci.md):

```text
python3 scripts/gate.py core
RUST_MCP_TEST_SOCKET=/ruta/docker.sock RUST_MCP_E5_DIR=/ruta/e5/onnx ORT_LIB_LOCATION=/ruta/ort python3 scripts/gate.py full
```

Core ejecuta fmt/check/Clippy/tests/doctests, contratos/protocolo, arquitectura,
vendor, fixtures Cargo, audit y deny. Full añade Docker real y semántica E5/LanceDB
bajo red denegada, incluyendo check/Clippy workspace all-features. Cargo trabaja
locked/offline y CARGO_INCREMENTAL=0. El gate no instala herramientas, modelos ni
imágenes automáticamente. Core solo no cierra M0 ni califica una distribución M1.

El [reporte M0-12](docs/validation/M0-12.md) conserva185 pruebas Rust distintas,
11 casos Cargo+1 oracle estático, diez garantías Docker y sus límites. El modelo
externo se identifica mediante [recibo y hashes](fixtures/semantic/README.md);
el build core sin feature local no califica como release M1. La advertencia de
mantenimiento de paste1.0.15 sigue visible, sin ignores de vulnerabilidades.

GitHub ejecuta además [CI portable](.github/workflows/ci.yml) en Linux x86_64,
macOS ARM64 y Windows x86_64. El [flujo de candidato](.github/workflows/release-candidate.yml)
es manual, exige un tag existente, genera provenance mediante OIDC y crea únicamente
un prerelease en borrador. Esa evidencia alojada no habilita capacidades de sandbox
que continúan fail-closed ni sustituye el gate local completo.

## Seguridad y alcance

El gateway M0 ejecuta escenarios cerrados de una imagen de probes
Go aprobada, sobre Docker/Linux ARM64 con runc/cgroupsv2. No admite Cargo, flags
arbitrarios ni host mounts. Timeout/cancel/overflow eliminan el contenedor completo;
cleanup incierto bloquea la instancia. `capabilities` hace probes activos del host:

```text
cargo run --locked --offline -- capabilities --docker /ruta/docker --docker-socket /ruta/docker.sock --state-root /directorio/privado --probe-image sha256:ID_LOCAL
```

El reporte conserva project_code_available=false: esa evidencia no certifica
build.rs ni proc macros. El [corpus adversarial](fixtures/README.md) queda excluido
del harness de ejecución directa en el host. Semantic fallback conserva facts de
SQLite y declara la degradación; un checksum aislado no autentica un publisher.
El nuevo importador M1-10 verifica firmas contra trust explícito del host.
ArtifactStore es efímero, con cuotas/TTL, redacción literal y retrieval owner-bound;
Check y fmt publican logs con ProjectRef vivo mediante Resources mínimos.

No se acredita soporte nativo Linux/Windows/x86_64, clientes MCP de terceros ni
calidad/overhead del modelo por los tres ejemplos de integración. Los requisitos
para avanzar están en [M1 prerequisites](docs/m1-prerequisites.md).

## Continuidad y documentación

- [Tablero y evidencia](docs/implementation-status.md).
- [Prompt para iniciar M1](docs/prompts/continue-m1.md).
- [Especificación v0.3.1](docs/spec/rust-engineering-mcp-propuesta-v0.3.md) y [ADRs](docs/adr/README.md).
- [Arquitectura](docs/architecture.md), [contratos](docs/domain-contracts.md) y [tools](docs/tools.md).
- [Modelo de seguridad](docs/security-model.md), [política](SECURITY.md), [changelog](CHANGELOG.md).

El desarrollo conserva ramas/merges locales y ahora publica snapshots saneados en
GitHub con Actions fijadas por commit. No se ha publicado0.1.0 como binario.
[LICENSE](LICENSE) concede `MIT OR Apache-2.0`; Cargo publish permanece deshabilitado.

El prerrequisito Rust de M1-01 incorpora captura de source con handles no-follow,
transferencia acotada a un volumen Docker y runtime1.98.1 aprobado por identidad.
La calibración real ejecuta build.rs/proc macros y prueba recursos y cleanup de
descendientes. La conexión MCP incorpora `project.inspect` junto a `project.open`; su validación
M1-01 está registrada en el tablero. El host configura explícitamente
Docker/socket/state-root/imagen según [tools](docs/tools.md); el runtime MCP nunca
aprovisiona ni descarga assets. Véase [evidencia ADR-031](docs/validation/M1-01-rust-gateway.md).

M1-08 / ADR-039: `rust.diagnostics.explain` accepts only an ASCII `E0000`-shaped
code and obtains bounded text from the approved installed rustc through the same
calibrated, network-denied gateway and joined workers. No project_ref, project source,
resource URI or host rustc execution is needed. Unknown codes return unavailable;
no heuristic explanation substitutes for compiler evidence. Returned text includes
content SHA, immutable runtime identity and latest_known artifact provenance/freshness.
No toolchain/image/model acquisition or native-platform qualification is implied.

M1-09 / ADR-040: `rust.quality.gate` composes fast(fmt/check/strict Clippy) or
standard(+default30s tests/offline audit) over one captured source generation, with
per-stage status, selection, repair detail and runtime evidence. One240s joined
worker; ordinary failures continue, interruption/uncertain cleanup aborts. Logs are
published as a bounded authorized group with final retention/ProjectRef checks;
rollback removes only new IDs, preserving earlier live logs. Omitted nonempty
streams make the quality verdict conservative even when command execution completed.
MCP body/envelope budgets retain stage rows and explicit omissions. No downloads,
source edits, global catalog import or new platform support. At the M1-09 baseline, M1-10..17 remained pending; current M1-10 evidence is below.

Corte local M1-01..09 validado: [M1-09](docs/validation/M1-09.md), core498 y
full14/14 con gateway Rust real y E5/LanceDB bajo network deny. Es evidencia
histórica; M1-10 tiene su propio recibo abajo. No hay release ni nuevas plataformas.

## M1-10 — Administración de catálogo en desarrollo

La CLI `catalog import|sync|status|rebuild-index` acepta trust del host, bundles
Ed25519/Zstd/USTAR y stores privados macOS26+/APFS. `sync --source` usa bytes
locales; `sync --url ... --allow-host ...` adquiere explícitamente por HTTPS. No
hay endpoint ni publisher oficial aprobado. En ese corte el runtime MCP no descargaba
y conservaba diez tools; M1-11 incorpora el estado de catálogo descrito abajo. [Formato, flags, recuperación y límites](docs/catalog-bundle-format.md).

La persistencia incluye un floor independiente reservado antes de activar, y
objetos Lance nativos ligados al modelo/catálogo. [Evidencia M1-10](docs/validation/M1-10.md):
full15/15 antes del ajuste final de observabilidad; core540 y CLI nativo5+1 actuales,
con hashes separados y revisión Opus5/Sonnet5. No se anuncia release, cierre M1 ni
soporte de nuevas plataformas.

## M1-11 — Estado del contexto local

`rust.catalog.status` recibe `{}` tras bootstrap, sin ProjectRef ni paths del peer.
El host configura `serve --stdio --catalog-store /store --catalog-trust /trust.json`;
el par es obligatorio si se configura catálogo. `--catalog-model-dir /e5` es opcional;
`--catalog-index-store /index` requiere modelo. Sin esa configuración, la tool
informa componentes no configurados. La feature `local` habilita carga E5/LanceDB;
core conserva SQLite y declara `feature_disabled` para semántica configurada.

La carga es lazy y de solo lectura; catálogo/modelo/índice quedan retenidos por sesión,
incluso si la primera carga informa indisponibilidad. Reiniciar permite observar
imports/rebuilds administrativos. Un índice inválido conserva SQLite disponible.
[Contrato y límites](docs/tools.md#rustcatalogstatus),
[ADR-042](docs/adr/ADR-042-catalog-runtime-status.md).

## M1-12 — Búsqueda de crates

`rust.crate.search` usa la misma generación retenida que status; no necesita
ProjectRef, nuevas rutas ni descargas. Hybrid combina rankings léxico y semántico;
si modelo/índice no están disponibles, declara fallback lexical con los mismos
filtros. SQLite decide los facts y la versión compatible seleccionada. Core admite
lexical/fallback; E5/Lance requieren `local` y assets verificados.

Los resultados describen una ventana acotada, no todo el registry ni seguridad del
crate. [Input, ranking y límites](docs/tools.md#rustcratesearch),
[ADR-043](docs/adr/ADR-043-catalog-search-modes.md),
[evidencia M1-12](docs/validation/M1-12.md).

## M1-13 — Inspección de crates

`rust.crate.inspect` pagina overview, versiones, features, dependencias e IDs de
advisories registrados. La continuación mantiene el fingerprint del snapshot y
la versión exacta cuando corresponde; documentación y source no registrados se
exponen como unknown. Comparte la generación SQLite retenida de status/search,
sin necesitar modelo ni índice. [Contrato](docs/tools.md#rustcrateinspect) y
[evidencia M1-13](docs/validation/M1-13.md). Las trece tools están implementadas;
release y calificación experimental siguen pendientes. M1 no está cerrado.

## M1-14 — Diagnóstico CLI

`version --json` informa versión, feature local compilada y target; son hechos del
binario. `doctor --json` diagnostica archivos configurados sin lanzar subprocesses;
acepta los mismos flags del host que `serve --stdio`. Puede cargar E5/Lance locales.
`doctor --active --json` añade calibración e inventario del runtime Rust aprobado,
cuando sus cuatro flags están configurados; no ejecuta el proyecto del usuario.
`capabilities --human` muestra los probes activos existentes en formato humano;
su salida por defecto sigue siendo JSON.

Doctor devuelve 0 para passed/warning, 1 para fallos del diagnóstico y 2 para sintaxis
inválida. Los servicios opcionales sin configurar y snapshots antiguos producen
warning, no una afirmación de disponibilidad universal. No instala ni repara nada.
SIGINT/SIGTERM/SIGHUP solicitan cancelación y esperan el worker y cleanup.
[Comandos y configuración](docs/tools.md#cli-y-doctor-m1-14),
[ADR-045](docs/adr/ADR-045-cli-doctor.md). El gate activo acredita calibración,
interrupción y cleanup del runtime aprobado; no cierra M1 ni las brechas de plataforma.

## M1-15 — Candidatos locales

Los [candidatos offline locales](docs/release/offline-candidates.md) contienen binarios core/local y recibos de instalación/doctor. Siguen siendo artifacts de revisión; licencia y distribución requieren decisiones explícitas.

## M1-16 — Piloto de utilidad medido

El [piloto v2](docs/validation/M1-16.md) completó24/24 ejecuciones pareadas y sus
oráculos congelados. Ambos brazos pasaron12/12 candidatos iniciales y finales; no
hubo pares discordantes ni ventaja observable de éxito. El endpoint saturado no
discrimina entre brazos ni demuestra equivalencia. El brazo MCP usó más solicitudes,
tiempo y tokens; no se permite inferencia causal o poblacional.

## M1-17 — Calificación local

El [gate full19](docs/validation/M1-17-final-gate.md) pasó en macOS ARM64 y
[MCP Inspector2.5.0](docs/validation/M1-17-inspector.md) llamó con éxito las13 tools
desde su UI persistente y envió una notificación de cancelación. El
[cliente stock Codex0.153.0](docs/validation/M1-17-codex-client.md) pasó preflights
directos de tool y Resource; sus turnos de modelo no llamaron al producto y se
conservan como calificación fallida. También confirmó inventario canónico estable,
una transición E0502→check verde y un error claro con runtime ausente. La
[revisión final](docs/validation/M1-17-review-disposition.md) conserva su veredicto
bloqueado. La [matriz](docs/validation/M1-17-matrix.md) mantiene abiertos los runners
nativos, los faltantes de licencias/notices y la custodia de la clave Ed25519 para
catálogos de producción.

## Licencia y publicación

El código original se ofrece bajo `MIT OR Apache-2.0`, a elección del usuario, con
copyright de IUMotion Labs. Ambas licencias contienen exclusiones de garantía y
límites de responsabilidad; prevalece siempre el texto legal aplicable en
[LICENSE-MIT](LICENSE-MIT) o [LICENSE-APACHE](LICENSE-APACHE). Los componentes de
terceros conservan sus propias condiciones y notices.

El repositorio oficial es `pharos-lang/rust-engineering-mcp`. GitHub es el canal de
código e incidencias y GitHub Releases será el canal inicial de binarios una vez
cerrados los gates correspondientes. [ADR-047](docs/adr/ADR-047-publication-license-and-delivery.md)
y la [nota de publicación](docs/publication.md) describen el snapshot público,
CI/CD, attestations y las claves de catálogo que siguen pendientes.
