# Imagen guest M6 — `rust-engineering-runtime:1.98.1-arm64-m6`

Deriva por digest de la imagen M5 admitida
`sha256:e0a5ca1661b3e49d0a3d68ee3cc0963453078d08eb7fc43c30538c16b7998aac` y añade
exactamente dos componentes del mismo canal 1.98.1:

| Componente | Origen | Licencia | Destino |
| --- | --- | --- | --- |
| `rust-analyzer` (`rust-analyzer-preview`) | `channel-rust-1.98.1.toml`, `sha256:a0fd960a9ab3…` | MIT OR Apache-2.0 | `/opt/analyzer/bin/rust-analyzer`, **fuera de `PATH`** |
| `rust-src` | `channel-rust-1.98.1.toml`, `sha256:5c846ebceb…` | MIT OR Apache-2.0 | `/opt/rust/lib/rustlib/src/rust/library` (`rustc --print sysroot` = `/opt/rust` lo descubre sin configuración adicional) |

Nada más cambia: ni el toolchain, ni los plugins M3, ni los binarios de
seguridad M4, ni los de performance M5, ni el usuario, ni el `WORKDIR`, ni el
`PATH`. `rust-analyzer` no queda alcanzable por `PATH` en un contenedor de
trabajo; el gateway lo invocará por ruta absoluta (patrón ya usado en
`/opt/perf/bin` para M5).

## Procedimiento

```sh
python3 -B scripts/build-m6-runtime.py
```

Ese script es el procedimiento completo y deja el recibo en
`docs/validation/M6/provisioning.json`. Hace, en orden: comprobar que
`rust-engineering-runtime:1.98.1-arm64-m5` resuelve exactamente a
`sha256:e0a5ca1661b3e49d0a3d68ee3cc0963453078d08eb7fc43c30538c16b7998aac` y
abortar si no; preparar el contexto con `provision.py`; construir con
`--network=none --pull=false`; y verificar sobre la imagen resultante que
ambos componentes existen, que `rust-analyzer` **no** es alcanzable por
`PATH`, que los binarios M3/M4/M5 siguen presentes, y que el contexto de
construcción no dejó residuos.

`provision.py` es el único paso de M6 autorizado a usar la red
(`docs/roadmap/m6-provisioning-request.md`), y solo para descargar exactamente
tres URLs: el manifest `channel-rust-1.98.1.toml` (verificado por `sha256`
contra `fixtures/rust-runtime/sources.json`) y, tras cruzar sus entradas
`xz_url`/`xz_hash` contra las constantes fijadas en este dossier, los dos
tarballs `.tar.xz`. Cada descarga se guarda en
`target/m6-provisioning/downloads/`; una ejecución repetida que encuentre ahí
un archivo cuyo `sha256` ya coincide no vuelve a tocar la red. Cada miembro de
cada tarball se valida (solo ficheros/directorios regulares bajo el directorio
raíz que el propio tarball declara, sin rutas absolutas, sin `..`, sin
enlaces) antes de copiarlo al contexto de construcción. El contexto final
incluye el `Dockerfile`, `build.sh`, los dos tarballs, `build-inputs.json`
(SBOM con url/sha256/licencia/tamaño por input), `notices/` con los
`LICENSE-APACHE`/`LICENSE-MIT`/`COPYRIGHT` que cada tarball trae en su propia
raíz, y `SHA256SUMS`. `docker build` corre con `--network=none`: ni la
preparación del contexto ni la construcción acceden a la red durante un gate.

`build.sh` corre dentro de la imagen sin red: verifica `SHA256SUMS`, confirma
`rustc 1.98.1` y el host `aarch64-unknown-linux-gnu`, instala ambos
componentes con el `install.sh` que cada tarball trae —el mismo mecanismo
(rust-installer) con el que `fixtures/rust-runtime/Dockerfile` ya instala
`rustc`/`cargo`/`rust-std`/`rustfmt` y el plugin `llvm-tools` en prefijos
arbitrarios; se usa igual aquí porque nada en él exige un prefijo fijo—,
comprueba que el ELF de `rust-analyzer` es AArch64 y no le falta ninguna
biblioteca, comprueba que ni `rust-analyzer` ni ningún enlace a él quedan en
`PATH`, y deja bajo `/usr/share/doc/rust-runtime/m6` el SBOM, las licencias
recolectadas, la versión exacta reportada por el binario y un inventario
completo.

**Hallazgo empírico documentado:** el tarball de `rust-analyzer-preview` no
incluye ninguna biblioteca compartida; `readelf -d` sobre el binario instalado
muestra `RUNPATH=$ORIGIN/../lib` y una dependencia dinámica de
`librustc_driver-<hash>.so`, que a su vez depende de `libLLVM.so.<versión>`.
Ambas ya están instaladas en `/opt/rust/lib` por el componente `rustc` de la
base M1/M3/M4/M5, pero `/opt/analyzer/lib` es un prefijo separado y no las ve.
`build.sh` no usa `ldconfig` (el patrón `--disable-ldconfig` de todo este
runtime evita un `ld.so.cache` global mutable) ni `LD_LIBRARY_PATH`: enlaza
símbolicamente los dos objetos compartidos reales dentro de
`/opt/analyzer/lib`, de modo que el `RUNPATH` relativo del binario los
encuentra sin ningún cambio de entorno ni de `PATH`.

## Rollback

La imagen M5 `sha256:e0a5ca1661b3…` permanece intacta y sigue siendo la
admitida mientras M6 no esté calificada. Revocar M6 es volver a apuntar el
gateway a ese digest; no hay estado que migrar.

## Límites

La imagen se construye y se verifica en Linux ARM64. La admisión en el
gateway y la calificación nativa de las tools que usarán `rust-analyzer` son
decisiones separadas (patrón ADR-077); este trabajo solo aprovisiona la
imagen y dejó constancia del comportamiento observado, sin admitirla.
