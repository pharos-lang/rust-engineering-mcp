# ADR-082 — Aprovisionamiento del runtime M6

Fecha: 2026-09-11.

## Status

Accepted tras la autorización explícita del owner del 2026-09-11 sobre el
[dossier](../roadmap/m6-provisioning-request.md) («aprobado opción a+b+c»). Las
autorizaciones M4 ([ADR-066](ADR-066-m4-runtime-provisioning.md)) y M5
([ADR-075](ADR-075-m5-runtime-provisioning.md)) no cubren estos inputs y no se
reutilizan. La admisión de la imagen en el gateway se decide aparte, con su
propia calificación nativa (patrón [ADR-077](ADR-077-m5-runtime-admission.md)).

## Context

El inventario del 2026-09-11
([registro](../validation/M6/delegation/README.md#inventario-rust-analyzer-g7--ausente-donde-se-necesita))
comprobó que ni el host macOS ARM64 ni la imagen guest M5 admitida
(`sha256:e0a5ca1661b3e49d0a3d68ee3cc0963453078d08eb7fc43c30538c16b7998aac`)
tienen `rust-analyzer` 1.98.1 ni `rust-src` 1.98.1. El toolchain host expone
otras versiones de `rust-analyzer` (1.92.0, 1.97.1, 1.99.0-nightly), pero
ninguna coincide con la versión fijada del canal, y ninguna corre en el guest:
el analyzer parsea y expande código del proyecto, que se trata como hostil, y
la configuración sola no sustituye containment. El plan M6 exige «rust-analyzer
exacto sobre un snapshot identificado» y «no distribuir RA en core sin
decisión»; sin `rust-src`, además, rust-analyzer no puede cargar `core`/`std` y
los diagnósticos sobre la biblioteca estándar de cualquier proyecto real
quedan sin resolver. G5/G7 exigen inventario exacto, autorización separada por
milestone y ausencia de descargas durante un gate.

## Decision

### 1. Dos inputs del mismo canal fijado, uno cruzado contra el manifest publicado

**`rust-analyzer` 1.98.1** (componente `rust-analyzer-preview`,
`aarch64-unknown-linux-gnu`) desde
`https://static.rust-lang.org/dist/2026-09-03/rust-analyzer-1.98.1-aarch64-unknown-linux-gnu.tar.xz`,
`sha256:a0fd960a9ab36193ae9ba4310e5f780f6ca38fa86160fae739be4ac541b6d10c`.
Licencia `MIT OR Apache-2.0`, textos tomados del propio tarball. Destino:
`/opt/analyzer/bin/rust-analyzer`, **fuera de `PATH`**, invocado solo por ruta
absoluta desde el gateway (mismo patrón que `/opt/perf/bin` en M5).

**`rust-src` 1.98.1** (target `*`) desde
`https://static.rust-lang.org/dist/2026-09-03/rust-src-1.98.1.tar.xz`,
`sha256:5c846ebcebcc7e2e0777a4cdaa12051691593f16a7e94edbae5e6241cc62d98c`.
Licencia `MIT OR Apache-2.0`. Destino: instalación estándar con el `install.sh`
del componente bajo `--prefix=/opt/rust`, de modo que quede en
`/opt/rust/lib/rustlib/src/rust/library` y `rustc --print sysroot` lo
descubra sin configuración adicional. Efecto secundario declarado: `rust-src`
también queda disponible para `cargo`/`rustc` del guest; no cambia ningún
argv cerrado existente.

Antes de descargar ninguno de los dos, `provision.py` descarga el manifest
`https://static.rust-lang.org/dist/channel-rust-1.98.1.toml` y lo verifica
contra la constante `MANIFEST_SHA256` fijada en el propio script,
`a7c8774a5fd8441c997d94c029776cbc5eb111e9d72ab5d256fa69866644347e`, que es
igual al valor ya registrado en `fixtures/rust-runtime/sources.json`
(cruzar ambos si se audita este dossier). Exige además que
sus entradas `[pkg.rust-analyzer-preview.target.aarch64-unknown-linux-gnu]` y
`[pkg.rust-src.target."*"]` declaren exactamente el `xz_url`/`xz_hash` de
arriba. Un manifest que no coincida falla cerrado; nunca ensancha el pin.

Ambos `install.sh` provienen de rust-installer, el mismo mecanismo con el que
`fixtures/rust-runtime/Dockerfile` ya instala
`rustc`/`cargo`/`rust-std`/`rustfmt` y el plugin `llvm-tools` en prefijos
arbitrarios; se usa igual aquí porque ningún componente exige un prefijo fijo,
sin necesidad de copiar el binario a mano. Verificado empíricamente: el
tarball de `rust-analyzer-preview` no trae biblioteca compartida alguna, y el
binario resultante tiene `RUNPATH=$ORIGIN/../lib` con una dependencia
dinámica de `librustc_driver-<hash>.so` (que a su vez depende de
`libLLVM.so.<versión>`), ambas ya presentes en `/opt/rust/lib` por el
`rustc` de la base. `build.sh` cierra esa dependencia con dos enlaces
simbólicos dentro de `/opt/analyzer/lib`, no con `ldconfig` (el patrón
`--disable-ldconfig` de todo este runtime evita un `ld.so.cache` global
mutable) ni con una variable de entorno nueva.

### 2. Una imagen derivada, sin nada más

`rust-engineering-runtime:1.98.1-arm64-m6` deriva **por digest** de la imagen
M5 admitida y añade exactamente los dos componentes de arriba. No cambia el
toolchain, los plugins M3, los binarios M4/M5, el usuario, el `WORKDIR` ni el
`PATH`. `rust-analyzer` no queda alcanzable por `PATH` en un contenedor de
trabajo: `build.sh` comprueba que no existen enlaces en `/usr/local/bin` ni en
`/opt/rust/bin`.

### 3. Red solo en el aprovisionamiento, nunca en la construcción

`provision.py` es el único paso de M6 autorizado a tocar la red, y solo para
las tres URLs de arriba; una ejecución repetida que encuentre en
`target/m6-provisioning/downloads/` un archivo cuyo `sha256` ya coincide no
vuelve a descargarlo. Valida que ningún miembro de cada tarball pueda escapar
de la raíz que el propio tarball declara (sin rutas absolutas, sin `..`, sin
enlaces), y produce `build-inputs.json` (SBOM), `notices/` (licencias
extraídas del tarball) y `SHA256SUMS`. `docker build` corre con
`--network=none`, igual que M4/M5.

### 4. Procedimiento y recibo verificables

`scripts/build-m6-runtime.py` comprueba antes de construir que el tag base
resuelve exactamente al digest M5 aprobado y aborta si no; ejecuta
`provision.py`; construye con `--network=none --pull=false`; y verifica sobre
la imagen resultante que ambos componentes existen, que `rust-analyzer` no es
alcanzable por `PATH`, que los binarios M3/M4/M5 siguen presentes, y que el
contexto de construcción no dejó residuos. El recibo
(`docs/validation/M6/provisioning.json`) cita esta autorización, la fecha de
aprobación del owner, este ADR, el alcance exacto de la red usada y el
`installed.json` que el propio guest calculó.

### 5. Rollback

La imagen M5 permanece intacta y sigue siendo la admitida mientras M6 no
califique. Revocar M6 es volver a apuntar el gateway a ese digest; no hay
estado que migrar.

## Alternatives considered

- **Ejecutar `rust-analyzer` en el host macOS.** Descartado: el analyzer
  parsea y expande el proyecto; la configuración no es containment
  (ADR-008/031). Todo código del proyecto corre en el gateway aislado.
- **Usar RA 1.97.1 (`stable`) o 1.99.0-nightly ya presentes en el host.**
  Descartado: no son la versión fijada del canal; el contrato promete
  identidades exactas de analyzer/toolchain.
- **Solo el componente A, sin `rust-src`.** Contrato honesto, pero los
  diagnósticos y símbolos sobre `std` quedan inutilizables para casi todo
  proyecto real; la investigación R01 pendiente debe confirmar el
  comportamiento exacto de `sysroot: omitted` como alternativa declarada.
- **Modo batch (`rust-analyzer diagnostics`/`scip`) en vez de LSP.** Exige
  igualmente el binario, no ofrece code actions, y el plan M6 fija el
  lifecycle LSP (D26).

## Consequences

El inventario de terceros del runtime crece en dos componentes del propio
proyecto Rust (`rust-analyzer`, `rust-src`), ambos `MIT OR Apache-2.0`, sin
dependencias transitivas nuevas que vendorizar. La admisión de la imagen en el
gateway y la calificación nativa de las tools que dependan de
`rust-analyzer` son decisiones separadas, con su propia calificación
(patrón ADR-077); este ADR no las concede. El rollback es el digest M5, sin
estado que migrar. Una actualización de cualquiera de los dos componentes
exige una decisión nueva, un `sha256` nuevo y una calificación nueva.
