# ADR-066 — Aprovisionamiento aislado del runtime M4

Fecha: 2026-09-07.

## Status

Accepted para adquisición y construcción, 2026-09-07. El owner respondió «si» a
la autorización separada de G5 para los inputs de este ADR y el build Docker sin
red. Este documento prepara D21; no aprueba una imagen, un perfil de sandbox ni
una capability positiva. Los hashes de binarios construidos y la calificación
nativa eran resultados pendientes al aprobar esta adquisición, no valores que
pudieran anticiparse. La admisión posterior está aceptada en
[ADR-068](ADR-068-m4-runtime-admission.md), con [inventario verificado](../validation/M4-runtime-inventory.json)
y [runtime final](../validation/M4-runtime.json). Las secciones de propuesta
y `approved_for_gateway=false` siguientes conservan el límite de esta autorización
de aprovisionamiento; no describen el estado posterior de admisión.

## Context

El runtime M3 instalado contiene Rust 1.98.1 y los cinco componentes de ADR-063.
No contiene cargo-deny ni un nightly/Miri preparado. La instalación de cargo-deny
en macOS no satisface el runtime Linux ARM64 del gateway. El nightly instalado
en el host tampoco acredita ese guest.

El release oficial de cargo-deny 0.19.7 ofrece ARM64 Linux musl, pero no GNU.
Se propone construir el paquete oficial con el Rust estable de M3 para mantener
el target GNU de la base Debian. Miri requiere un compilador, rust-src y sysroot
del mismo nightly; su preparación no puede ocurrir durante una llamada MCP.

## Decision

Decisión técnica del Technical Owner con adquisición autorizada por el owner;
quedaba pendiente la evidencia para admitir D21; ADR-068 y los recibos finales
la aportan posteriormente. El estado de la propuesta original se conserva aquí:

1. Crear un perfil M4 separado, derivado de la imagen M3 local inmutable
   `sha256:384a1742ecc53cdd3a9c0bf36c6f8b66db73ddd118aeeae6e55654ea998ae36a`,
   con config digest
   `sha256:7d4e58b9e29b2045c13d71542f7892ee071a6886a1b939c4cbfc3ff7ce40dc45`.
   Verificar ambos y `linux/arm64` antes de construir. Un tag mutable solo sirve
   como referencia local tras comprobar esa identidad. No reconstruir M3.
2. Adquirir desde el host los assets de `nightly-2026-09-07`, cargo-deny 0.19.7
   y sus dependencias exactas de registro. Verificar tamaño/SHA-256 de cada
   archivo antes de Docker. El contexto admite únicamente los inputs enumerados;
   rechazar enlaces, entradas inesperadas, escapes y duplicados al extraer.
3. Precargar las dependencias fijadas por los locks upstream y construir con
   Docker `--network=none`, sin pull. No usar la red dentro del builder.
   El cierre observado contiene 212 paquetes de registro para cargo-deny y 31
   para el sysroot; el inventario debe distinguir coincidencias entre ambos.
   Verificar también que los locks de los archivos descargados coincidan con
   los locks fijados antes de ejecutar compiladores o build scripts.
4. Conservar `/opt/rust` de M3. Instalar cargo-deny GNU en
   `/opt/security/bin/cargo-deny`, el nightly en
   `/opt/rust-nightly-2026-09-07` y el sysroot Miri en
   `/opt/miri-sysroot/2026-09-07/aarch64-unknown-linux-gnu`.
   El tag de conveniencia propuesto es
   `rust-engineering-runtime:1.98.1-arm64-m4`; producción siempre seleccionará
   el nuevo ID inmutable una vez calificado.
5. Preparar Miri solo en el builder, con Cargo/rustc del nightly fijado,
   `CARGO_NET_OFFLINE=true`, configuración Cargo offline,
   `MIRI_LIB_SRC` al rust-src instalado y `MIRI_SYSROOT` al directorio anterior.
   `cargo miri setup --offline` por sí solo no es una garantía: esta versión no
   reenvía ese argumento al Cargo interno. La red Docker permanece deshabilitada.
6. Retirar caches de adquisición/compilación, fuentes de build e instaladores
   del resultado final. Conservar licencias, notices, inventario, locks de
   provenance y los componentes necesarios del runtime. No instalar rustup ni
   gestores de paquetes. Restaurar la configuración final no privilegiada de M3.

El runtime MCP nunca adquiere estos inputs, llama setup ni hereda la configuración
del host. La admisión futura exige versiones, hashes ejecutables, digest del
sysroot, configuración aplicada y el perfil calibrado. Esta propuesta no amplía
seccomp ni mounts de ADR-064/065; cualquier necesidad comprobada se decide antes
de cambiar el gateway y se prueba con un control positivo y un ataque negativo.

### Inputs principales fijados

| Input | Bytes | SHA-256 |
| --- | ---: | --- |
| cargo-deny 0.19.7 `.crate` | 206598 | `40bd28db01a5a7876ea6b2b3f518e73b5e643b0cc5f64f4260d0c68379adb384` |
| Manifest nightly-2026-09-07 | 957702 | `b1cff0b9344c1d8070a839fc0c2286a593c52cc115642051736d7f5eeac16c1d` |
| rustc nightly ARM64 GNU | 69205712 | `6450feca96ad096466bc69c3e450a997833afc80be38da0dcc6806c5f43bce8f` |
| cargo nightly ARM64 GNU | 10561544 | `df8e618e9e477d79c67b92305154c498efee4c00437547e2a7cbcc7b7da910ef` |
| rust-std nightly ARM64 GNU | 31960356 | `919d7f6148ebf87592eecb854623eec31a2ca1b124ce7252374532aea323278b` |
| rust-src nightly | 5917776 | `9446774cba32df43a051b2c28b79432098db9d36940650108e81a4e69cd23551` |
| miri-preview nightly ARM64 GNU | 3349212 | `9f97d043080445e0c4e159dbcb4dacdd0dec274d100d92a1c64dce73746d4a7f` |

Rust está fijado al commit `5a2be9f5f075d31e3ca5526b5b029881ce441253`
(1.100.0-nightly); Cargo al commit observado `3c0b53475`.
El `library/Cargo.lock` tiene SHA-256
`8ab03296aee01aed456b167433f2d65bbeb3bd38ae9141e4d933a7f88966dc5d`.
La adquisición debe volver a contrastar estos valores; cualquier discrepancia
detiene el proceso, sin elegir automáticamente otro nightly.

El [manifest completo](../validation/m4-provisioning-proposal/manifest.json)
registra URLs, locks, tamaños, hashes y licencias declaradas de ambos cierres.
Su [comprobación local](../validation/m4-provisioning-proposal/inventory-check.json)
valida consistencia y deduplicación; no afirma haber verificado archives todavía
no descargados. Los hashes y SBOM del resultado futuro permanecen `null` y
`approved_for_gateway` permanece `false`.

### Oráculos y salida obligatoria

El verifier registra versiones/commits reales, ELF AArch64 e intérprete GNU,
`ldd` sin bibliotecas ausentes, componentes, SHA-256 de ejecutables y árbol del
sysroot, config/image IDs, licencias/notices y SBOM guest. Comprueba la ausencia
de cachés/instaladores y que consultar el sysroot preparado funciona read-only,
sin red ni escrituras. Ninguno de estos checks ejecuta source del usuario.

La comprobación del nightly fijado mostró que `cargo miri setup --print-sysroot`
siempre entra en la ruta de aprovisionamiento y puede intentar reconstruir el
sysroot. No es un probe de lectura. El [código fijado de setup](https://github.com/rust-lang/rust/blob/5a2be9f5f075d31e3ca5526b5b029881ce441253/src/tools/miri/cargo-miri/src/setup.rs#L21-L28)
omite esa ruta al ejecutar test/run con `MIRI_SYSROOT` explícito. Se conserva el
fallo original y se sustituye el probe por un test mínimo propio que verifica
`cfg(miri)`, con source y rootfs read-only y target/tmp en tmpfs limitado. La
identidad del sysroot se verifica antes y después. Esto califica su uso básico
preaprovisionado; no amplía permisos ni sustituye los adversarios del gateway.

La calificación posterior debe cubrir Miri limpio, use-after-free, uninitialized,
aliasing/race, FFI unsupported, cfg(miri), compile/test failure, timeout,
cancelación y descendientes; cargo-deny exige fixtures de licenses/bans/sources.
Los outputs adversarios solo se ejecutan dentro del gateway. Un provisioning
exitoso no sustituye estos oráculos ni constituye un cierre M4.

## Alternatives considered

- Usar cargo-deny del host: rechazado; cambia la frontera de ejecución y el target.
- Usar el asset musl: técnicamente evaluable, pero introduce un target distinto
  sin necesidad; se elige el build GNU sobre la base ya calificada.
- Builder con red, como cargo-mutants M3: se prefiere el cierre de dependencias
  explícito y la compilación sin red para hacer revisable toda adquisición.
- Actualizar `/opt/rust` o reutilizar el nightly Apple del host: mezcla toolchains
  y cambia las condiciones ya calificadas de M1–M3.
- Setup bajo demanda en MCP: incompatible con offline, autoridad y budgets.

## Consequences

La adquisición adicional queda limitada a los manifests/locks fijados; no se
requieren paquetes OS nuevos ni un pull de base. El build produce nuevos hashes
que deben registrarse y revisarse, sin afirmar reproducibilidad binaria. El
rollback conserva el runtime M3 anterior y deshabilita la admisión M4; nunca
reinterpreta un journal ni reduce floors de catálogo.

Las licencias declaradas de Rust/Miri/cargo-deny son MIT OR Apache-2.0; los
componentes y crates incorporan notices/licencias adicionales que se verifican
contra los archivos reales antes de calificación o distribución. Esta propuesta
no publica imágenes, releases, tags Git ni artifacts nuevos.

## Sources

- [Release cargo-deny 0.19.7](https://github.com/EmbarkStudios/cargo-deny/releases/tag/0.19.7).
- [Metadata oficial cargo-deny](https://crates.io/api/v1/crates/cargo-deny/0.19.7).
- [Manifest Rust fechado](https://static.rust-lang.org/dist/2026-09-07/channel-rust-nightly.toml)
  y sus sidecars oficiales SHA-256.
- [Miri en el commit Rust fijado](https://github.com/rust-lang/rust/tree/5a2be9f5f075d31e3ca5526b5b029881ce441253/src/tools/miri).
- [ADR-063](ADR-063-m3-guest-plugin-provisioning.md), [G5/G7](../roadmap/m2-m8.md)
  y [plan M4](../roadmap/m4-security.md).
