# ADR-075 — Aprovisionamiento del runtime M5

Fecha: 2026-09-08.

## Status

Accepted tras la autorización explícita y separada del owner del 2026-09-08 sobre
el [dossier](../roadmap/m5-provisioning-request.md). La autorización M4
([ADR-066](ADR-066-m4-runtime-provisioning.md)) no cubría estos inputs y no se
reutilizó. La admisión de la imagen en el gateway se decide aparte, con su
calificación nativa.

## Context

El inventario del 2026-09-08 comprobó que ninguna herramienta de medición estaba
aprovisionada: `cargo-bloat`, `cargo-criterion`, `cargo-flamegraph`, `samply`,
`perf` e `inferno` están ausentes tanto en `~/.cargo/bin` del host como en las
cuatro imágenes guest. `criterion` no aparecía en `Cargo.lock` ni en ninguna
fixture. G5 y G7 exigen inventario exacto, autorización separada y ausencia de
descargas durante un gate.

[ADR-074](ADR-074-profiling-capability-and-containment.md) elimina la necesidad de
un perfilador de terceros: el helper es código de este repositorio.

## Decision

### 1. Tres inputs, ninguno adquirido durante un gate

**`cargo-bloat 0.12.1`** y su cierre fijado por el `Cargo.lock` publicado dentro
de su propio archivo. Veinte paquetes en total, todos con checksum verificado
contra ese lockfile:

| Paquete | Versión | Licencia | sha256 |
| --- | --- | --- | --- |
| `cargo-bloat` | 0.12.1 | MIT | `56e2c483ab55e380…` |
| `aho-corasick` | 1.1.3 | Unlicense OR MIT | `8e60d3430d3a6947…` |
| `binfarce` | 0.2.1 | MIT | `18464ccbb85e5ded…` |
| `fallible-iterator` | 0.2.0 | MIT/Apache-2.0 | `4443176a9f2c1626…` |
| `json` | 0.12.4 | MIT/Apache-2.0 | `078e285eafdfb6c4…` |
| `libc` | 0.2.154 | MIT OR Apache-2.0 | `ae743338b92ff914…` |
| `memchr` | 2.7.2 | Unlicense OR MIT | `6c8640c5d730cb13…` |
| `memmap2` | 0.9.4 | MIT OR Apache-2.0 | `fe751422e4a8caa4…` |
| `multimap` | 0.10.0 | MIT OR Apache-2.0 | `defc4c55412d8913…` |
| `pdb` | 0.8.0 | MIT OR Apache-2.0 | `82040a392923abe6…` |
| `pico-args` | 0.5.0 | MIT | `5be167a7af36ee22…` |
| `regex` | 1.10.4 | MIT OR Apache-2.0 | `c117dbdfde9c8308…` |
| `regex-automata` | 0.4.6 | MIT OR Apache-2.0 | `86b83b8b9847f9bf…` |
| `regex-syntax` | 0.8.3 | MIT OR Apache-2.0 | `adad44e29e4c8061…` |
| `scroll` | 0.11.0 | MIT | `04c565b551bafbef…` |
| `term_size` | 0.3.2 | MIT/Apache-2.0 | `1e4129646ca0ed8f…` |
| `uuid` | 1.8.0 | Apache-2.0 OR MIT | `a183cf7feeba97b4…` |
| `winapi` | 0.3.9 | MIT/Apache-2.0 | `5c839a674fcd7a98…` |
| `winapi-i686-pc-windows-gnu` | 0.4.0 | MIT/Apache-2.0 | `ac3b87c63620426d…` |
| `winapi-x86_64-pc-windows-gnu` | 0.4.0 | MIT/Apache-2.0 | `712e227841d057c1…` |

Diecisiete estaban ya en la caché local; tres —`winapi` y sus dos shims de
Windows— se descargaron una sola vez desde `static.crates.io` y se verificaron
contra el checksum del lockfile antes de escribirlos. Se conservan aunque el
target sea Linux: un árbol vendorizado incompleto se completa con las fuentes
auténticas publicadas, nunca con un sustituto fabricado.

**`criterion 0.8.2`** y sus 51 dependencias transitivas, vendorizadas en
`fixtures/criterion-vendor/` como entrada offline de la fixture de benchmarks.
Todas provienen de la caché local, con el `sha256` verificado contra el checksum
del lockfile resuelto. Licencia de criterion y criterion-plot:
`Apache-2.0 OR MIT`. El inventario completo con licencia por paquete vive en
`fixtures/criterion-vendor/INVENTORY.json` y `LICENSES.md`.

**`rust-mcp-profile-helper`**, binario construido desde
`fixtures/profile-helper/` con una única dependencia externa, `libc` fijada por
versión exacta. No es un componente de terceros.

### 2. Una imagen derivada, sin nada más

`rust-engineering-runtime:1.98.1-arm64-m5` deriva **por digest** de la imagen M4
aprobada `sha256:25ed3626e710081a571a86a29521eaf2e890e796afd422ba5e409e0ce1891635`
y añade exactamente `/opt/perf/bin/cargo-bloat` y
`/opt/perf/bin/rust-mcp-profile-helper`. No cambia el toolchain, los plugins M3,
los binarios M4, el usuario, el `WORKDIR` ni el `PATH`. Ninguno de los dos queda
alcanzable por `PATH` en un contenedor de trabajo: el gateway los invoca por ruta
absoluta y `build.sh` comprueba que no existen enlaces en `/usr/local/bin`.

`docker build` corre con `--network=none`. Ni la preparación del contexto ni la
construcción acceden a la red.

### 3. Procedimiento verificable

`provision.py` no accede a la red: toma cada `.crate` de la caché local, verifica
su `sha256` contra el checksum que Cargo registró en el lockfile fijado, valida
que ningún miembro del archivo pueda escapar de su raíz, extrae, y produce
`dependency-map.tsv`, `build-inputs.json` (SBOM) y `SHA256SUMS`. `build.sh`
verifica esos hashes dentro de la imagen, confirma `rustc 1.98.1` y el host
`aarch64-unknown-linux-gnu`, escribe cada `.cargo-checksum.json`, compila con
`--release --locked --offline --target aarch64-unknown-linux-gnu`, comprueba que
cada ELF es AArch64 y no tiene bibliotecas ausentes, y deja bajo
`/usr/share/doc/rust-runtime/m5` el SBOM, el mapa de dependencias, los textos de
licencia recolectados, el hash de cada binario y un inventario completo.

### 4. Rollback

La imagen M4 permanece intacta y sigue siendo la aprobada mientras M5 no
califique la nueva. Revocar M5 es volver a apuntar el gateway a ese digest; no hay
estado que migrar. La capability de profiling se revoca por separado, sin
reconstruir nada.

## Alternatives considered

- **Reutilizar la autorización M4.** No procede: G5 exige una autorización
  separada por milestone y el owner la concedió explícitamente para estos inputs.
- **Aprovisionar `perf` e `inferno`.** Descartado en ADR-074; el helper propio
  cubre el caso con mucha menos superficie.
- **`cargo vendor` sobre el workspace.** Descartado: metería `criterion` en el
  grafo del producto. La fixture es una entrada de prueba, nunca una dependencia
  del binario distribuido.
- **Un sustituto local de `winapi`.** Descartado por deshonesto: un árbol
  vendorizado debe contener las fuentes publicadas auténticas.

## Consequences

El inventario de terceros del runtime crece en un binario (`cargo-bloat`, MIT) y
diecinueve paquetes de construcción que no viajan en el binario final. La fixture
de benchmarks añade 52 paquetes vendorizados al repositorio como entrada offline;
no son dependencias del workspace y `scripts/check-architecture.py` sigue viendo
los mismos ocho miembros. Una actualización de cualquiera de estos componentes
exige una decisión nueva, un `sha256` nuevo y una calificación nueva.
