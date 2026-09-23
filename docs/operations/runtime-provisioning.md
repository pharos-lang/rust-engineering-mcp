# Operación del runtime

Toda tool que ejecute Cargo, mute un workspace, mida rendimiento o use
rust-analyzer corre dentro de un contenedor Docker Linux ARM64 cuya imagen el
servidor admite **por digest sha256 exacto**, nunca por tag flotante. Ninguna
imagen se publica en un registry: se construye localmente con las fixtures de
este repositorio y se referencia por el ID resultante. Esta página cubre cómo
construirlas/verificarlas y qué añade cada una; los flags que configuran
`serve` con la imagen resultante están en
[Configuración](../guides/configuration.md).

El host que construye y ejecuta estas imágenes debe ser **macOS con Apple
Silicon sobre un volumen APFS**: es la única plataforma donde el adapter de
filesystem no-follow/reparse-safe que protege el I/O del servidor está
calificado (`crates/mcp-server/src/doctor.rs`). Linux y Windows fallan cerrado
en las tools de escritura y en el gateway de ejecución aunque el guest
containerizado sea Linux ARM64 en ambos casos.

## Cadena de imágenes

Cada imagen deriva por digest de la anterior y solo añade lo que su columna
describe; el resto del árbol (toolchain, plugins previos, usuario, `WORKDIR`,
`PATH`) no cambia:

| Imagen (constante de admisión) | Digest | Deriva de | Añade |
| --- | --- | --- | --- |
| `APPROVED_RUST_IMAGE` (M1, con `--plugins` = M3) | `sha256:384a1742ecc53cdd3a9c0bf36c6f8b66db73ddd118aeeae6e55654ea998ae36a` | base Debian bookworm ARM64 | Rust/Cargo/rustfmt/Clippy 1.98.1 oficiales en `/opt/rust`; con `--plugins`: `cargo-nextest` 0.9.143, `cargo-llvm-cov` 0.9.0, `llvm-tools-preview` 1.98.1, `cargo-semver-checks` 0.50.0, `cargo-mutants` 27.1.0. |
| `APPROVED_SECURITY_IMAGE` / `APPROVED_M4_IMAGE` | `sha256:25ed3626e710081a571a86a29521eaf2e890e796afd422ba5e409e0ce1891635` | imagen M3 | `cargo-deny` 0.19.7 y un toolchain nightly 2026-09-07 con Miri, en `/opt/security`. |
| `APPROVED_M5_IMAGE` | `sha256:e0a5ca1661b3e49d0a3d68ee3cc0963453078d08eb7fc43c30538c16b7998aac` | imagen M4 | `cargo-bloat` 0.12.1 y `rust-mcp-profile-helper` en `/opt/perf/bin`, fuera de `PATH` (el gateway los invoca por ruta absoluta). |
| `APPROVED_M6_IMAGE` | `sha256:f39a5b33ee7d54243664162eb635f8ec223d512042beb7cd18ecf071046b310c` | imagen M5 | `rust-analyzer` y `rust-src` 1.98.1, también fuera de `PATH`. Requerida por las cinco tools `rust.analyzer.*` (preview). |

Configura `--rust-image` con **exactamente uno** de estos digests. Un host con
la imagen M5 sigue sirviendo todas las tools M1–M5; las tools `analyzer.*`
responden `unavailable` hasta que apuntes a la imagen M6. Al revés, una imagen
más nueva no "amplía" la calificación de una tool más antigua: cada tool
conserva su propia calificación contra su propio digest.

## Construir la imagen base (M1, con plugins M3 opcional)

```bash
python3 fixtures/rust-runtime/provision.py \
  --docker /ruta/absoluta/al/cliente/docker \
  --host unix:///ruta/absoluta/docker.sock \
  --output target/m1-runtime-provisioning
python3 fixtures/rust-runtime/verify.py \
  --docker /ruta/absoluta/al/cliente/docker \
  --host unix:///ruta/absoluta/docker.sock \
  --output target/m1-runtime-provisioning
```

Añade `--plugins` a ambos comandos para construir/verificar el tag M3
(`rust-engineering-runtime:1.98.1-arm64-m3`) en vez del M1 puro; sin ese flag,
la imagen queda reconstruible sin los plugins. Esta fixture instala Rust
1.98.1 oficial sin `rustup`, descargando y verificando por TLS + checksum
contra `fixtures/rust-runtime/sources.json`; requiere autorización explícita
del operador porque **usa red** (imagen base Debian, paquetes, distribución
Rust) — no la ejecuta el runtime MCP. La verificación corre en contenedores
no-root, de solo lectura, `network=none`, sin capabilities, sin montar
ningún proyecto; nunca ejecuta código hostil.

Artifacts que quedan en el directorio de salida: `image-id`, `build.log`,
`base-inspect.json`, `image-inspect.json`, `provisioning-receipt.json`,
`verification.json`. Conserva esos artifacts junto a la imagen como evidencia
de aprovisionamiento — no son opcionales para operar con confianza esa
imagen.

## Construir la imagen M4 (seguridad avanzada)

```bash
python3 fixtures/rust-runtime/m4/provision.py \
  --docker /ruta/absoluta/al/cliente/docker \
  --host unix:///ruta/absoluta/docker.sock \
  --output target/m4-runtime-provisioning
python3 fixtures/rust-runtime/m4/verify.py \
  --docker /ruta/absoluta/al/cliente/docker \
  --host unix:///ruta/absoluta/docker.sock \
  --output target/m4-runtime-provisioning
```

Consume el manifest fijado `tests/data/m4-provisioning-manifest.json`
(ADR-066), verifica cada archivo descargado, compara el lock empaquetado de
`cargo-deny` byte a byte con el lock de su tag pinneado, y construye con red y
`pull` de Docker deshabilitados.

## Construir las imágenes M5 y M6

```bash
python3 -B scripts/build-m5-runtime.py
python3 -B scripts/build-m6-runtime.py
```

Cada script es el procedimiento completo: comprueba que la imagen base
esperada (M4 para M5; M5 para M6) resuelve exactamente al digest documentado
arriba y aborta si no; prepara el contexto de construcción con el
`provision.py` de ese milestone; construye con `--network=none --pull=false`;
y verifica sobre la imagen resultante que los binarios nuevos existen, que
**no** quedan alcanzables por `PATH`, y que los binarios de milestones
anteriores siguen presentes. Deja su recibo en
`tests/data/m5-runtime-provisioning.json` / `tests/data/m6-runtime-provisioning.json`.

`provision.py` de M5 no usa red: verifica los `.crate` ya presentes en la
caché local de Cargo contra el checksum del lockfile fijado. `provision.py`
de M6 es la única excepción autorizada a usar red en esa cadena, y solo para
tres URLs fijas (el manifest de canal Rust y los dos tarballs
`rust-analyzer-preview`/`rust-src`, cada uno verificado por sha256 antes de
copiarse al contexto); una ejecución repetida que encuentre en caché un
archivo cuyo hash ya coincide no vuelve a tocar la red.

## Rollback de una imagen

Cada imagen deja intacta la anterior. Revocar una imagen (M5 o M6) es
reconfigurar `--rust-image` al digest anterior y reiniciar el servidor; no hay
estado migrado que limpiar porque cada imagen es un artifact Docker
independiente identificado por su propio digest.

## Qué runtime NO hace el servidor

`serve` nunca construye, descarga ni verifica estas imágenes por sí mismo: la
construcción es siempre una operación explícita del operador, fuera de una
sesión MCP, exactamente como se describe arriba. El servidor solo comprueba,
en el momento de arrancar, que el digest de `--rust-image` está en su lista
de constantes admitidas.
