# M6 — solicitud de aprovisionamiento (G5/G7)

Estado: **Autorizado por el owner el 2026-09-11** («aprobado opción a+b+c», sesión de orquestación Fable 5.1). Fecha de la solicitud: 2026-09-11.
Rama: `ai/m6-analyzer`. Base: `main` `627729a48b2912c7e3b43d6fc5678a20f0a046a0`.

Las autorizaciones M4 ([ADR-066](../adr/ADR-066-m4-runtime-provisioning.md)) y
M5 ([ADR-075](../adr/ADR-075-m5-runtime-provisioning.md)) no se extienden a M6.
Este documento es el paquete revisable que G5/G7 exigen antes de adquirir
cualquier input nuevo. La ejecución (worker W01) queda ligada a esta autorización y a su recibo `docs/validation/M6/provisioning.json`. El plan
M6 lo anticipa: «Artifact RA de toolchain 1.98.1 es un candidato a comprobar,
no runtime ya aprobado» y «No distribuir RA en core sin decisión».

## 1. Inventario verificado del estado actual

Comprobado el 2026-09-11 sobre el host macOS ARM64 y la imagen guest M5
admitida, sin instalar ni descargar nada
([registro](../validation/M6/delegation/README.md#inventario-rust-analyzer-g7--ausente-donde-se-necesita)).

| Componente | Host (toolchain 1.98.1) | Imagen `…-m5` (`sha256:e0a5ca1661b3…`) |
| --- | --- | --- |
| `rust-analyzer` 1.98.1 | ausente (componente disponible, no instalado) | ausente |
| `rust-src` 1.98.1 (`lib/rustlib/src/rust/library`) | presente en el host | **ausente** (`/opt/rust/lib/rustlib/src` no existe) |
| Otros `rust-analyzer` | 1.92.0, 1.97.1 y 1.99.0-nightly en otros toolchains del host | ninguno |

Conclusión: M6 no puede implementarse sobre inputs ya aprovisionados. Los
binarios de otras versiones del host no sirven: no son la versión fijada y,
sobre todo, el análisis debe correr en el guest (el analyzer parsea código del
proyecto, tratado como hostil; la configuración sola no sustituye containment).

## 2. Paquete solicitado

Los tres ítems provienen del **mismo manifest fijado** que ya gobierna el
runtime (`https://static.rust-lang.org/dist/channel-rust-1.98.1.toml`, fechado
2026-09-03; sha256 publicado `a7c8774a5fd8441c997d94c029776cbc5eb111e9d72ab5d256fa69866644347e`
según `fixtures/rust-runtime/sources.json`). Los hashes de abajo se leyeron de
la copia local del manifest instalada con el toolchain 1.98.1 del host
(`multirust-channel-manifest.toml`, sha256 `e44f4ea0a633aa3e497e4dd161424419fbd7bdc203606ef840634fdd22dab9bf`);
el `provision.py` volverá a verificarlos contra el manifest publicado antes de
construir.

### Ítem A — `rust-analyzer` 1.98.1 para `aarch64-unknown-linux-gnu` (requiere red una vez)

- **Qué:** componente `rust-analyzer-preview` del canal 1.98.1.
- **Origen:** `https://static.rust-lang.org/dist/2026-09-03/rust-analyzer-1.98.1-aarch64-unknown-linux-gnu.tar.xz`
- **sha256:** `a0fd960a9ab36193ae9ba4310e5f780f6ca38fa86160fae739be4ac541b6d10c`
  (variante `.tar.gz`: `524976423062f2b1a5b960bf26047f6c740aa83bd566f41e40ba03d7500fafaf`).
- **Licencia:** rust-analyzer se publica bajo `MIT OR Apache-2.0`; el recibo
  registrará los textos de licencia contenidos en el tarball, no una afirmación.
- **Destino en la imagen:** `/opt/analyzer/bin/rust-analyzer`, **fuera de
  `PATH`**, invocado solo por ruta absoluta desde el gateway (mismo patrón que
  `/opt/perf/bin` en M5). No entra en el archive core macOS.
- **Por qué es obligatorio:** spec §32/§97 y el plan M6 exigen «rust-analyzer
  exacto sobre un snapshot identificado»; la única versión que coincide con el
  toolchain fijado es la del propio canal 1.98.1.

### Ítem B — `rust-src` 1.98.1 (requiere red una vez)

- **Qué:** componente `rust-src` (target `*`) del canal 1.98.1.
- **Origen:** `https://static.rust-lang.org/dist/2026-09-03/rust-src-1.98.1.tar.xz`
- **sha256:** `5c846ebcebcc7e2e0777a4cdaa12051691593f16a7e94edbae5e6241cc62d98c`
  (variante `.tar.gz`: `411c3dccf3782ed6eb784357999cd1bffeb65a6dd55196817992a6421c0f9689`).
- **Licencia:** la de Rust (`MIT OR Apache-2.0`), textos incluidos en el tarball.
- **Destino:** instalación estándar con el `install.sh` del componente bajo
  `--prefix=/opt/rust`, de modo que quede en
  `/opt/rust/lib/rustlib/src/rust/library` y `rustc --print sysroot` lo
  descubra sin configuración adicional.
- **Por qué:** sin `rust-src`, rust-analyzer no puede cargar `core`/`std` y
  los símbolos, referencias y diagnósticos sobre cualquier item de la biblioteca
  estándar quedan sin resolver (imports «no resueltos» falsos). La alternativa
  de declarar `sysroot: omitido` mantiene el contrato honesto pero convierte
  `rust.analyzer.diagnostics` en ruido para casi todo proyecto real. La
  confirmación exacta del comportamiento sin sysroot forma parte de la
  investigación R01 pendiente; la solicitud se hace ahora para no encadenar
  dos autorizaciones.
- **Efecto secundario a declarar:** `rust-src` también queda disponible para
  `cargo`/`rustc` del guest (por ejemplo `-Zbuild-std` en nightly no aplica:
  el toolchain es estable). No cambia ningún argv cerrado existente.

### Ítem C — imagen guest M6 derivada (sin red)

- **Qué:** `rust-engineering-runtime:1.98.1-arm64-m6`, derivada **por digest**
  de la imagen M5 admitida
  `sha256:e0a5ca1661b3e49d0a3d68ee3cc0963453078d08eb7fc43c30538c16b7998aac`.
- Añade exactamente A y B. No cambia toolchain, plugins M3, binarios M4/M5,
  usuario, `WORKDIR` ni `PATH`.
- Construcción con `--network=none --pull=false`, base nombrada por tag y
  verificada por digest inmediatamente antes (misma limitación de BuildKit que
  documenta [ci.md](../ci.md#imagen-guest-m5)); `fixtures/rust-runtime/m6/`
  (`Dockerfile`, `build.sh`, `provision.py`) y `scripts/build-m6-runtime.py`
  con recibo en `docs/validation/M6/provisioning.json`: `sha256` de cada tarball
  contra el manifest, inventario instalado (`installed.json`), textos de
  licencia, SBOM y hash del binario.
- **Admisión** en el gateway es una decisión separada con su calificación
  nativa (patrón ADR-077); construir la imagen no la admite.
- **Rollback:** la imagen M5 `e0a5ca16…` permanece intacta y admitida; revocar
  M6 es volver a apuntar el gateway a ese digest. No hay estado que migrar.

## 3. Alternativas consideradas

| Alternativa | Por qué no |
| --- | --- |
| Ejecutar rust-analyzer en el host macOS (instalar el componente 1.98.1 con rustup) | El analyzer parsea y expande el proyecto; aunque build scripts y proc macros queden desactivados por configuración, la configuración no es containment (plan M6 §Workspace trust). Contradice ADR-008/031: todo código del proyecto corre en el gateway aislado |
| Usar RA 1.97.1 (`stable`) o 1.99.0-nightly ya presentes en el host | No son la versión fijada; el contrato promete identidades exactas de analyzer/toolchain |
| Solo Ítem A, sin `rust-src` | Contrato honesto pero diagnósticos/símbolos sobre `std` inutilizables; R01 debe confirmar el comportamiento exacto. Si el owner lo prefiere, se declara `sysroot: omitted` en cada resultado |
| Modo batch (`rust-analyzer diagnostics`/`scip`) en vez de LSP | Exige igualmente el binario; no ofrece code actions; el plan M6 fija el lifecycle LSP (D26) |
| Distribuir RA dentro del archive core | No lo pide ningún corte; el binario vive solo en el guest |

## 4. Qué queda bloqueado si no se autoriza

| Sin ítem | Consecuencia |
| --- | --- |
| A | M6-01..06 completos: no existe analyzer exacto en el guest; ningún corte puede empezar con flujo real |
| B | M6-01/02/03 degradados a `sysroot: omitted`; M6-04/05 (acciones) posibles pero con acciones sobre `std` incompletas |
| C | Sin imagen calificable no hay admisión ni suite nativa; A y B no pueden usarse |

## 5. Consecuencia de decidir mal

- Autorizar y luego no calificar: coste acotado (una imagen derivada, dos
  tarballs verificados por hash); rollback inmediato al digest M5.
- No autorizar: M6 queda **Blocked** con condición reproducible («binario
  rust-analyzer 1.98.1 ausente del guest»), y todo M7/M8 que dependa de
  analyzer hereda el bloqueo. No se construye sobre un sustituto.

## 6. Compromisos

- Ninguna descarga durante un gate. El aprovisionamiento es un paso previo con
  su propio recibo (`docs/validation/M6/provisioning.json`), ejecutado por un
  worker delegado con la autorización citada en el recibo.
- Inventario exacto con versión, `sha256`, licencia, notices y SBOM antes de
  construir; ausencias declaradas, nunca sustituidas.
- La imagen M5 sigue siendo el rollback verificable.
