# M5 — solicitud de aprovisionamiento (G5/G7)

Estado: **Pendiente de autorización del owner**. Fecha: 2026-09-08.
Rama: `ai/m5-performance`. Base: `c6099f27415b0be3838e84d21d25eed903c8c312`.

La autorización de aprovisionamiento M4 ([ADR-066](../adr/ADR-066-m4-runtime-provisioning.md))
no se extiende a M5. Este documento es el paquete revisable que G5 exige antes de
adquirir cualquier input nuevo. **Nada de lo descrito aquí se ha ejecutado.**

## 1. Inventario verificado del estado actual

Comprobado el 2026-09-08 sobre el host macOS 26 ARM64 y las cuatro imágenes guest
locales, sin instalar ni descargar nada.

| Componente | Host `~/.cargo/bin` | Imagen `…-m4-scanner` (`sha256:25ed3626e710…`) |
| --- | --- | --- |
| `cargo-bloat` | ausente | ausente |
| `cargo-criterion` | ausente | ausente |
| `cargo-flamegraph` / `flamegraph` | ausente | ausente |
| `samply` | ausente | ausente |
| `perf` | ausente | ausente |
| `inferno-*` | ausente | ausente |
| `xctrace` / `dtrace` | presentes (Xcode/sistema) | ausentes |

`criterion` no aparece en `Cargo.lock` ni en `Cargo.toml` del workspace, ni en
ninguna fixture. `/proc/sys/kernel/perf_event_paranoid` = `2` dentro del guest.
El perfil `seccomp-rust-quality.json` permite 121 syscalls y **no** incluye
`perf_event_open`.

Conclusión: M5 no puede implementarse sobre inputs ya aprovisionados.

## 2. Lo que M5 **no** necesita

Descartados tras la prueba de viabilidad D24 (§4): `perf`, `inferno`, `samply`,
`cargo-flamegraph`, `cargo-criterion`, `xctrace`, `dtrace`. Ningún componente de
profiling de terceros entra al runtime. No se pide `sudo`, contenedor privilegiado,
cambio de `sysctl`, capability de Docker añadida ni ejecución de código de proyecto
en el host.

## 3. Paquete solicitado

### Ítem A — vendoring de `criterion` para la fixture oráculo (sin red)

- **Qué:** `criterion 0.8.2` y sus 48 dependencias transitivas necesarias para
  `aarch64-unknown-linux-gnu`, con `default-features = false` y
  `features = ["cargo_bench_support"]`.
- **Licencia:** `Apache-2.0 OR MIT` (criterion y criterion-plot). Cada dependencia
  conserva su propio texto de licencia; se recolectan en `license-texts.tar` con el
  mismo procedimiento del M4 scanner.
- **Origen:** los 49 archivos `.crate` ya están en la caché local
  `~/.cargo/registry/cache/index.crates.io-1949cf8c6b5b557f/`. **No requiere red.**
  Verificado: de 52 paquetes del lockfile, 49 presentes; los 3 ausentes
  (`winapi`, `winapi-i686-pc-windows-gnu`, `winapi-x86_64-pc-windows-gnu`) son
  exclusivos de Windows y no entran en el grafo del target Linux ARM64.
- **Hashes:** `criterion-0.8.2.crate` =
  `950046b2aa2492f9a536f5f4f9a3de7b9e2476e575e05bd6c333371add4d98f3`;
  `criterion-plot-0.8.2.crate` =
  `d8d80a2f4f5b554395e47b5d8305bc3d27813bacb73493eb1001e8f76dae29ea`.
  El `dependency-map.tsv` registrará los 49 con su `sha256` y el checksum
  publicado del índice cacheado antes de construir.
- **Alcance:** solo `fixtures/benchmark/`. No entra al `Cargo.lock` del workspace
  ni al binario distribuido.
- **Por qué es obligatorio:** D23 exige un harness exacto con muestras crudas.
  `criterion` escribe `<out>/<id>/new/sample.json` con `{sampling_mode, iters[],
  times[]}` — verificado leyendo `src/analysis/mod.rs:163` y `src/lib.rs:1519` del
  `.crate` cacheado. `libtest` solo publica mediana ± desviación, no muestras crudas.

### Ítem B — `cargo-bloat` para M5-04 (requiere red una vez)

- **Qué:** `cargo-bloat 0.12.1` (2024-05-10, no yanked) y su árbol pinneado:
  `binfarce ^0.2.1`, `json ^0.12`, `memmap2 ^0.9`, `multimap ^0.10`, `pdb ^0.8.0`,
  `pico-args ^0.5.0`, `term_size ^0.3.1`, `regex ^1.3` (opcional) más transitivas.
- **Licencia:** MIT. Repositorio: `https://github.com/RazrFalcon/cargo-bloat`.
- **Origen:** ausente de la caché local y del índice cacheado; requiere descarga
  desde crates.io **una sola vez, fuera del gate**.
- **Hashes:** se fijan con `--locked` y se registran en `dependency-map.tsv` con
  `sha256` por archivo antes de compilar, igual que ADR-063/ADR-066.
- **Procedimiento:** idéntico al de `rust-mcp-unsafe-helper`:
  `provision.py` descarga y verifica → `SHA256SUMS` → `build.sh` construye
  `--locked --offline` dentro de la imagen → `installed.json` + SBOM + notices.
- **Por qué es obligatorio:** el plan M5 nombra `cargo-bloat` como herramienta
  exacta de M5-04 y la spec §28.4 lo integra explícitamente.

### Ítem C — helper de profiling propio del proyecto (sin red)

- **Qué:** `rust-mcp-profile-helper 0.1.0`, binario construido desde fuente del
  repositorio, sin dependencias externas, análogo a `rust-mcp-unsafe-helper`.
- **Licencia:** la del proyecto (dual MIT/Apache-2.0, ADR-047).
- **Alcance:** abre un único `perf_event_open` en modo usuario
  (`exclude_kernel=1`, `exclude_hv=1`) sobre el proceso hijo que él mismo lanza,
  lee el ring buffer, emite stacks colapsados. Sin motor de scripting, sin red,
  sin lectura de procesos ajenos.
- **No requiere componentes de terceros.**

### Ítem D — reconstrucción de la imagen guest (sin red)

- **Qué:** `rust-engineering-runtime:1.98.1-arm64-m5`, derivada por digest de
  `sha256:25ed3626e710081a571a86a29521eaf2e890e796afd422ba5e409e0ce1891635`.
- Añade `/opt/perf/bin/cargo-bloat` (ítem B) y
  `/opt/perf/bin/rust-mcp-profile-helper` (ítem C). Nada más.
- El `docker build` corre con la base local y `--network=none`; los ítems A y C no
  necesitan red en ningún momento.
- Rollback: la imagen M4 `25ed…` permanece intacta y sigue siendo la aprobada
  mientras M5 no califique la nueva.

## 4. Prueba de viabilidad D24 ya ejecutada (sin aprovisionar nada)

Ejecutada el 2026-09-08 con un programa Rust sin dependencias que invoca
`perf_event_open` por syscall cruda, compilado y ejecutado dentro de la imagen M4
aprobada con exactamente las banderas del gateway (`--cap-drop=ALL`,
`--security-opt=no-new-privileges=true`, `--network=none`, `--pids-limit=128`,
`--cpus=1`, `--memory=1g`, `--user=65534:65534`), sin `sudo`, sin contenedor
privilegiado y sin tocar `perf_event_paranoid`:

| Perfil seccomp | Resultado |
| --- | --- |
| `seccomp-rust-quality.json` actual | `perf_event_open` → `-1` (EPERM). Denegado. |
| El mismo + exactamente `perf_event_open` | `fd=3`; ring buffer mapeado; `PERF_EVENT_IOC_ENABLE` → 0; `data_head=560` tras el workload. **Muestras reales recogidas.** |

Es decir: el positivo de profiling se obtiene añadiendo **una** syscall al
allowlist, sin ampliar capabilities ni privilegios. Esto habilita el ítem C y hace
innecesario `perf` de terceros.

## 5. Qué queda bloqueado si no se autoriza

| Sin ítem | Consecuencia |
| --- | --- |
| A | M5-01 sin oráculo de muestras crudas; `benchmark.run` no califica. |
| B | M5-04 bloqueado; `rust.binary.bloat` no puede entregarse. |
| C + D | M5-03 bloqueado; sin positivo nativo de profiling, M5 no cierra. |
| ninguno | M5-02 (`benchmark.compare`) es cálculo puro y se entrega igualmente. |

## 6. Compromisos

- Ninguna descarga durante un gate. El aprovisionamiento es un paso previo con su
  propio recibo (`docs/validation/M5-provisioning.json`).
- Inventario exacto con versión, `sha256`, licencia, notices y SBOM antes de construir.
- Ausencias declaradas, nunca sustituidas en silencio.
- La imagen M4 sigue siendo el rollback verificable.
