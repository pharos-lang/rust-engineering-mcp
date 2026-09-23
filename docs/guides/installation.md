# Instalación

Dos caminos: instalar la release publicada más reciente (`v0.3.0`, 31 tools
estables) o compilar desde el código fuente (checkout actual `0.9.0-rc.1`, 36
tools incluyendo 5 `preview`). Ambos producen el mismo binario
`rust-engineering-mcp` (`crates/mcp-server`); la diferencia es qué tools trae
compiladas y si existe evidencia de release publicada para esos bytes.

## Requisitos

Para **compilar**:

- Git;
- Rust y Cargo `1.98.1` exactos — el repositorio fija esta versión en
  `rust-toolchain.toml`; con `rustup` instalado, `cargo build` la respeta
  automáticamente, o invócala explícitamente con
  `rustup run 1.98.1 cargo build ...`;
- las dependencias fijadas por `Cargo.lock` (`cargo build --locked`).

Para **ejecutar** el binario, sea instalado o compilado:

- macOS con Apple Silicon (`aarch64-apple-darwin`) sobre un volumen **APFS**.
  Es la única plataforma con capacidades positivas verificadas
  (`crates/mcp-server/src/doctor.rs`: el chequeo de plataforma falla cerrado
  fuera de `target_os = "macos"`); Linux y Windows x86_64 no son hosts
  soportados para abrir o mutar proyectos hoy.
- Docker, solo si vas a usar tools que ejecutan Cargo (la mayoría). Ver
  [Operación del runtime](../operations/runtime-provisioning.md) para
  construir o cargar la imagen guest aprobada — el runtime nunca descarga
  imágenes por su cuenta.
- Opcional: un catálogo local de crates (SQLite + FTS5) y, si compilas con
  `--features local`, un modelo de embeddings E5 y un índice LanceDB, para
  búsqueda semántica. Ver [Mantenimiento del catálogo](../operations/catalog-maintenance.md).

No hay distribución secundaria vía `cargo install`/crates.io: el manifest fija
`publish = false` y no existe publicación en crates.io a través de
`0.9.0-rc.1`. La única distribución binaria oficial es GitHub Releases.

## Instalar la release publicada (`v0.3.0`)

`v0.3.0` es la release estable más reciente; publica exactamente tres
assets: el archivo core `.tar.gz` para `aarch64-apple-darwin`, `SHA256SUMS`
y `release-smoke-receipt.json` (`gh release view v0.3.0 --json assets`). El
SBOM SPDX (`sbom.spdx.json`) y las notices de terceros
(`THIRD_PARTY_NOTICES.txt`) no son assets separados: están **dentro** del
archive extraído, junto a `inventory.json`. La procedencia se verifica
en línea contra el registro de atestaciones de GitHub
(`actions/attest-build-provenance`), no descargando un bundle. No es una
reproducibilidad bit-a-bit firmada con Sigstore; es una cadena de confianza
verificable hacia el workflow exacto que construyó el archivo.

```bash
curl -LO https://github.com/pharos-lang/rust-engineering-mcp/releases/download/v0.3.0/rust-engineering-mcp-v0.3.0-aarch64-apple-darwin.tar.gz
curl -LO https://github.com/pharos-lang/rust-engineering-mcp/releases/download/v0.3.0/SHA256SUMS
```

### Verificación de integridad (mínima)

```bash
shasum -a 256 -c SHA256SUMS
tar -xzf rust-engineering-mcp-v0.3.0-aarch64-apple-darwin.tar.gz
cd rust-engineering-mcp-v0.3.0-aarch64-apple-darwin
./rust-engineering-mcp version --json
./rust-engineering-mcp doctor --json
```

Esto confirma que los bytes instalados coinciden con los publicados en el
momento de la descarga; no autentica al publicador.

### Verificación de procedencia (con `gh` instalado)

```bash
gh attestation verify rust-engineering-mcp-v0.3.0-aarch64-apple-darwin.tar.gz \
  --repo pharos-lang/rust-engineering-mcp \
  --signer-workflow pharos-lang/rust-engineering-mcp/.github/workflows/release-candidate.yml
```

`--signer-workflow` es lo que fija la verificación al workflow exacto que
firmó el archivo (`release-candidate.yml`) en vez de aceptar la atestación de
cualquier workflow del repositorio; es el mismo flag que el propio workflow
usa para autoverificarse antes de publicar. Este comando **consulta la API
de GitHub por red** (no es offline); no hay hoy un procedimiento de
verificación offline con bundle documentado como ejercido — ver
[`docs/operations/release-verification.md`](../operations/release-verification.md#cómo-verificar-un-artifact-descargado).
Repite la verificación para `SHA256SUMS` y `release-smoke-receipt.json` si se
descargaron por separado del archivo.

`v0.1.0` (13 tools, M1) también sigue publicada con el mismo procedimiento si
necesitas esa superficie exacta; sustituye la versión en las URLs.

**El checkout de desarrollo (`0.9.0-rc.1`) no tiene release soportada.** No
existe un tag `v0.9.0-rc.1`: el workflow de release
(`.github/workflows/release-candidate.yml`) es `workflow_dispatch`-only, exige
partir de un tag `vX.Y.Z` o `vX.Y.Z-rc.N` ya existente y produce como máximo un
**draft prerelease**, nunca publicado automáticamente. Para obtener las 36
tools de este checkout (31 estables + 5 `preview` de analyzer), compila desde
el código fuente.

## Compilar desde el código fuente

```bash
git clone https://github.com/pharos-lang/rust-engineering-mcp.git
cd rust-engineering-mcp
cargo build --release --locked -p rust-engineering-mcp
```

El toolchain `1.98.1` se resuelve automáticamente vía `rust-toolchain.toml` si
tienes `rustup`; para forzarlo explícitamente:

```bash
rustup run 1.98.1 cargo build --release --locked -p rust-engineering-mcp
```

El binario queda en `target/release/rust-engineering-mcp`. Para habilitar
búsqueda semántica (LanceDB/E5), añade `--features local` a la invocación de
`cargo build`.

Comprueba el binario y su configuración pasiva:

```bash
./target/release/rust-engineering-mcp version --json
./target/release/rust-engineering-mcp doctor --json
```

`doctor` no instala, descarga ni repara ningún componente; devuelve
`warning`/`not_configured` cuando falta una capacidad opcional. `--help`
enumera comandos y tools disponibles, y cada comando acepta también su propio
`--help`/`-h` (`serve --help`, `doctor --help`, `catalog --help`, etc.)
justo después del comando; cualquier otra colocación sigue devolviendo
`Unsupported invocation. Use 'rust-engineering-mcp --help'.` (código de salida
2). Ver [CLI](../reference/cli.md) y [Configuración](configuration.md) para
el detalle completo de cada flag.

La cabecera de `--help` se autodescribe como «Rust Engineering MCP —
development server» tanto en el checkout como en la release publicada; no
indica que el binario sea inestable ni cambia el contrato de CLI o tools.

## Siguiente paso

Con el binario instalado, sigue con
[Configuración](configuration.md) para las flags de `serve --stdio`, o
directamente con [Clientes](clients.md) para conectar un agente.
