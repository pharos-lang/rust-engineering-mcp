# Testing y CI

`scripts/gate.py` es el único entrypoint de calificación local, con reporte
máquina-legible. Ningún gate instala, refresca o descarga toolchain,
imágenes o modelos automáticamente: una herramienta o plataforma ausente
hace **fallar** la etapa correspondiente, nunca la salta en silencio ni la
marca en verde. Esta página describe la composición actual del gate, lo que
corre en GitHub CI, y cómo ejecutar solo una parte durante desarrollo.

## `scripts/gate.py core|full`

```sh
python3 -B scripts/gate.py core --report target/gate-report.json
RUST_MCP_TEST_SOCKET=/ruta/docker.sock RUST_MCP_E5_DIR=/ruta/e5/onnx ORT_LIB_LOCATION=/ruta/ort \
  python3 -B scripts/gate.py full --report target/gate-full-report.json
```

Ambos modos resuelven el toolchain Rust real vía `rustup which --toolchain 1.98.1 cargo`
(nunca lo instalan) y verifican que `cargo --version`/`rustc --version`
empiecen literalmente por `cargo 1.98.1 `/`rustc 1.98.1 ` antes de continuar.
`rustup`, `cargo-audit` y `cargo-deny` deben estar en `PATH`; su ausencia
aborta el gate con un `RuntimeError` explícito. El entorno del subproceso se
reconstruye desde una lista cerrada de variables permitidas (`HOME`, `PATH`,
`TMPDIR`, `CARGO_HOME`, `RUSTUP_HOME`, `SDKROOT`, `DEVELOPER_DIR`,
`CARGO_TARGET_DIR`, `RUST_MCP_TEST_SOCKET`, `RUST_MCP_E5_DIR`,
`ORT_LIB_LOCATION`) más `CARGO_INCREMENTAL=0`, `ORT_SKIP_DOWNLOAD=1` y
`CARGO_TERM_COLOR=never` — no hereda el entorno completo del host. `full` en
Windows falla explícitamente (`"Windows fixture harness not calibrated"`);
`full` fuera de macOS ARM64 también falla, con el mismo mensaje de "no hay
sustitución". Al final de cada corrida, el gate rehashea el inventario de
fuentes/config/fixtures trackeadas y rechaza la calificación si algo cambió
durante la ejecución (`source_inputs_unchanged`).

### `core` — 30 etapas, sin Docker

En el orden exacto en que `scripts/gate.py` las ejecuta hoy (verificar contra
el propio script si este documento y el código difieren — el script es la
fuente de verdad, no esta lista):

`fmt`, `check`, `clippy`, `test`, `doctests`, `architecture`,
`gate-reporting`, `release-artifact-tests`, `release-smoke-tests`,
`codex-qualifier-tests`, `m4-client-harness-tests`, `m4-safety-harness-tests`,
`m4-helper-fmt`, `m4-helper-tests`, `m4-provisioning-tests`, `m5-helper-fmt`,
`m5-helper-guest-clippy`, `m5-helper-tests`, `m5-vendor-tests`,
`m6-provisioning-tests`, `m6-provisioning-unit-tests`,
`m6-runtime-unit-tests`, `m8-client-harness-tests`, `contract-freeze-tests`,
`contract-freeze`, `m8-performance-unit-tests`, `vendor`, `cargo-fixtures`,
`audit`, `deny`.

Notas sobre etapas no evidentes por el nombre:

- `check`/`clippy`/`test` corren `--workspace --all-targets --locked --offline`
  (`clippy` añade `-D warnings`); `doctests` es
  `cargo test --workspace --doc --locked --offline`.
- `architecture` ejecuta `scripts/check-architecture.py` — el enforcement de
  la regla hexagonal de AGENTS.md (`domain`/`application` sin dependencias
  de `rmcp`/JSON-RPC/stdio/SQLite/LanceDB) es un análisis estático en
  Python, **no una garantía de compilador**.
- `m5-helper-guest-clippy` compila Clippy del helper de profiling
  (`fixtures/profile-helper`) **contra el target `aarch64-unknown-linux-gnu`**,
  porque su único camino de syscalls está tras `#[cfg(target_os = "linux")]`
  y en un host macOS ni `fmt` ni `test` compilan esa rama — Clippy sí
  type-checkea sin necesitar linkear.
- `contract-freeze-tests` prueba la lógica del script; `contract-freeze`
  ejecuta `scripts/contract-freeze.py verify` contra
  `tests/baselines/contract-freeze-0.8.0.json` (ver
  [`../reference/compatibility.md`](../reference/compatibility.md#congelación-de-contrato-080-y-verificación)).
- `vendor` ejecuta `scripts/verify-vendor.py`; `cargo-fixtures` ejecuta
  `scripts/test-fixtures.py` contra el Cargo real resuelto.
- `audit`/`deny` corren `cargo audit --no-fetch` y
  `cargo deny check --disable-fetch advisories bans sources` — sin red, con
  las bases de datos locales ya provisionadas.

Cada etapa marcada `require_test_groups=True` en el código falla si no
observa al menos un resumen de test reconocido (`test result: ok. N passed…`
o `Ran N tests in…`) en su salida — una etapa que no ejecuta ningún test no
puede pasar por omisión.

### `full` — añade 16 etapas nativas (macOS ARM64 + Docker)

Requiere host macOS ARM64, propietario único del daemon Docker, y las tres
variables `RUST_MCP_TEST_SOCKET`, `RUST_MCP_E5_DIR`, `ORT_LIB_LOCATION`
explícitas (sin sustitución si faltan). Añade, tras `core`:
`docker-security`, `rust-security`, `m2-runtime`, `m3-runtime`,
`m4-tampered-plugin`, `m4-inventory`, `m4-runtime`, `audit-data`, `semantic`,
`catalog`, `catalog-status`, `crate-search`, `crate-inspect`, `doctor`,
`m5-runtime`, `m6-runtime`.

Estas etapas ejercitan Docker/gateway Rust real, RustSec/SQLite bajo
network-deny, y el camino semántico con feature `local` y modelo E5 real.
Un build sin la feature `local` no puede calificar el gate `full`.

## Requisitos explícitos, sin instalación automática

`rustup` (solo para resolver el toolchain ya instalado), Rust/Cargo 1.98.1 +
`rustfmt` + Clippy, Python ≥ 3.11, dependencias del lock ya en caché,
`cargo-audit`/`cargo-deny`, bases RustSec locales suficientemente recientes.
`full` requiere además Docker Desktop/buildx arrancado con cliente y socket
locales, la imagen Rust aprobada correspondiente ya instalada
(ver [`../operations/runtime-provisioning.md`](../operations/runtime-provisioning.md)),
el modelo E5 y ORT con hash validado. Ninguno de estos se instala o refresca
automáticamente por el gate.

## CI hospedado

| Workflow | Qué ejecuta | Plataformas |
| --- | --- | --- |
| `.github/workflows/ci.yml` | fmt/check/Clippy/test/doctests + fronteras arquitectónicas + tests de release/qualification controllers (job `portable`); audit + deny de dependencias resueltas (job `supply chain`, `ubuntu-latest`) | `ubuntu-latest` (x86_64) y `macos-26` (ARM64) — **sin job de Windows** |
| `.github/workflows/codeql.yml` | Análisis CodeQL para `actions`, `python` y `rust` | `ubuntu-latest`, en schedule + push |
| `.github/workflows/sonarcloud.yml` | Cobertura Rust (`cargo-llvm-cov` 0.9.0) + Python (Coverage.py 7.16.0) antes del análisis de calidad | `ubuntu-latest` |
| `.github/workflows/release-candidate.yml` | Build/verificación/draft de release, solo `workflow_dispatch` desde un tag existente | `ubuntu-latest` (validate-ref, draft) + `macos-26` (build) |

Estos runners hospedados **no son evidencia de capability de sandbox**: dan
cobertura de portabilidad de fuente (compila y pasa tests unitarios en varias
plataformas), no prueban el runtime Docker ni el adapter de filesystem
qualified — esos solo se ejercitan en `scripts/gate.py full` sobre un host
macOS ARM64 real. La matriz de CI portable son exactamente dos plataformas
hoy; Windows x86_64 se retiró explícitamente el 2026-09-13 por una regresión
de stdio previa a `initialize` — restaurarlo es deuda de portabilidad, nunca
criterio de ninguna release (ver
[`../reference/compatibility.md`](../reference/compatibility.md#plataformas)).

## SonarCloud: exclusiones de cobertura

`sonar-project.properties` excluye del **porcentaje** de cobertura (nunca del
análisis de fiabilidad/seguridad/mantenibilidad/duplicación) cuatro grupos de
programas cuyo camino end-to-end exige un host de release real, Docker o
clientes reales instalados — nunca un crate ni un directorio completo, cada
archivo se nombra individualmente:

1. Programas de calificación maintainer-only (`scripts/release-artifact.py`,
   `scripts/release-smoke.py`, `scripts/verify-vendor.py`,
   `scripts/codex-model-qualifier.py`).
2. Sondas Docker de M2 (`scripts/probe-m2-*.py`) — retiradas en esta limpieza
   junto con `fixtures/cargo-local-registry/` (sin consumidores; D02 cerrada
   por ADR-050); si reaparecen, siguen la misma exclusión.
3. Sesiones de cliente real (`scripts/m3-inspector-session.mjs`,
   `scripts/m4-inspector-session.mjs`, etc.).
4. Arneses host-only de M8 (`scripts/measure-m8-performance.py`,
   `scripts/soak-m8.py`, `scripts/test-m8-rollback.py`,
   `scripts/test-m8-clients.py`, `scripts/m8-inspector-session.mjs`).

`sonar-project.properties` entra en el inventario que
`scripts/gate.py::source_inventory` hashea: ampliar o reducir el ámbito de
exclusión cambia el hash de input del gate, así que no es un cambio silencioso.
`sonar.test.inclusions` marca los cinco módulos `*_native.rs` de
`execution-adapter` como test (solo existen bajo `cfg(test)` y contienen los
oráculos nativos de Docker) para no contarlos como producto sin excluirlos
del análisis. Ningún archivo Rust de producto está excluido del porcentaje.

## Cómo ejecutar pruebas focalizadas

```sh
# Todo el workspace, sin red
cargo test --workspace --all-targets --locked --offline

# Un solo crate
cargo test -p rust-engineering-domain --locked --offline

# Un test exacto
cargo test --workspace --locked --offline -- nombre_del_test --exact

# Solo Clippy
cargo clippy --workspace --all-targets --locked --offline -- -D warnings

# Un script de test Python concreto (todos aceptan -B para no escribir .pyc)
python3 -B scripts/test-contract-freeze.py
python3 -B scripts/test-doctor.py   # requiere runtime nativo, no aplica fuera de `full`
```

Los tests marcados `#[ignore]` en Rust son casos nativos que requieren Docker
y la imagen aprobada correspondiente; se ejecutan explícitamente por los
harnesses de `full` con `--exact --ignored`, nunca por `cargo test` sin
filtro.

## Harness scripts por capability y su estado manual/CI

| Capability | Script(s) | Wireado en `gate.py` | Requiere |
| --- | --- | --- | --- |
| Clientes MCP reales (Inspector, Codex, Claude Code) | `scripts/test-m{2..6,8}-clients.py`, `scripts/m{3..6,8}-inspector-session.mjs` | **No** — manual únicamente; `test-m8-clients.py` compone los recibos previos de `test-m{2..6}-clients.py` (`compose_prior_receipts`), así que siguen vivos aunque ninguno esté wireado por sí solo | Clientes stock instalados; credenciales/sesión del operador |
| Unidad de esos mismos harnesses (sin cliente real) | `scripts/test-m4-clients-unit.py`, `scripts/test-m8-clients-unit.py` | Sí, `core` | Nada adicional |
| Ídem, M5/M6 | `scripts/test-m5-clients-unit.py`, `scripts/test-m6-clients-unit.py` | Parcial — `test-m6-clients-unit.py` corre en `sonarcloud.yml`; `test-m5-clients-unit.py` no está wireado en `gate.py` ni en ningún workflow a la fecha de este documento | Nada adicional |
| Aprovisionamiento de runtime (imágenes Docker) | `scripts/build-m5-runtime.py`, `scripts/build-m6-runtime.py`, `fixtures/rust-runtime/{m4-scanner,m5,m6}/provision.py` | Parcial — los tests unitarios de provisioning (`m6-provisioning-unit-tests`, `m6-runtime-unit-tests`) están en `core`; construir la imagen en sí es manual | Docker local, red solo para `fixtures/rust-runtime/m6/provision.py` (descarga verificada de `rust-src`) |
| Rendimiento/soak | `scripts/measure-m8-performance.py`, `scripts/soak-m8.py` | **No** — manual únicamente | Binario `release` real, `ps`/`lsof`/`pgrep` |
| Rollback de versión | `scripts/test-m8-rollback-unit.py`, `scripts/test-m8-rollback.py` | Parcial — `test-m8-rollback-unit.py` (`m8-rollback-unit-tests`) está en `core`; `test-m8-rollback.py` (el binario release real) sigue manual | El primero es puro (sin dependencias nativas); el segundo requiere el binario release real. Ver [`../operations/backup-and-recovery.md`](../operations/backup-and-recovery.md) |
| Provisioning del fixture M4 (variante no-scanner) | `fixtures/rust-runtime/m4/test_provision.py` | Sí, `core` (`m4-runtime-provisioning-tests`), además de `fixtures/rust-runtime/m4-scanner/test_provision.py` (`m4-provisioning-tests`) | — |
| Runtime nativo por milestone | `scripts/test-m{2,3,4,5,6}-runtime.py`, `scripts/test-rust-execution.py`, `scripts/test-execution.sh` | Sí, solo en `full` | Docker + imagen aprobada correspondiente |

`test-m5-clients-unit.py` es el único harness de unidad sin wiring de CI/gate
propio; su sujeto (`scripts/test-m5-clients.py`) permanece vivo porque
`test-m8-clients.py` lo reutiliza, así que no se retira. Limitación conocida:
hoy falla (2 failures, 8 errors) porque su oráculo de inventario fija las 31
tools de `0.3.0` y el checkout registra 36; ya fallaba igual en `51fa602e`.
Actualizar ese oráculo es requisito para incorporarlo a `gate.py`.

Las herramientas de calibración M5 de un solo uso —
`capture-m5-benchmark-datasets.py`, `capture-m5-bloat-calibration.py`,
`measure-m5-vendor-capture.py` y `simulate-m5-comparison-method.py` — se
retiraron: sus resultados quedaron congelados en constantes de código, en los
datasets de `fixtures/benchmark-datasets/` y en el recibo
`qualification/benchmark-method-simulation.json`, y ninguna estaba wireada en
`gate.py` ni en ningún workflow. Para reproducir esas calibraciones históricas,
recupera el script correspondiente del historial de Git, por ejemplo
[`scripts/capture-m5-benchmark-datasets.py` en 51fa602e](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/scripts/capture-m5-benchmark-datasets.py).

## Higiene de documentación

```sh
python3 -B scripts/docs-hygiene.py check [--report]
```

`check` corre todas las reglas de higiene documental (enlaces y anclas,
layout canónico de `docs/`, referencias a rutas retiradas, planes de
`.planning/` sin trackear, navegación desde `docs/README.md`, rutas
`docs/...` obsoletas en código, y citas al directorio de trabajo temporal de
esta limpieza) contra el árbol de Git y falla si alguna no resuelve. No está
wireado hoy en `scripts/gate.py`; ejecútalo manualmente después de mover o
renombrar documentación.

## Retención de recibos que cierran una gate o mitigan un riesgo residual

Un recibo que cierra una gate del checklist 1.0 o mitiga un riesgo residual
(`RR-n`) debe quedar adjunto al PR correspondiente o conservado como
artifact de CI retenido — nunca solo en `target/qualification/` en un
checkout local, que es el destino por defecto pero no un lugar de
conservación: no hay retención garantizada de `target/`, y el árbol
histórico de recibos por hito quedó fuera de Git en esta limpieza (ver
`.gitignore`). Citar el recibo desde el documento que cierra la gate exige
que ese recibo sea recuperable por un tercero, no solo por quien lo generó
localmente.

## Decisiones relacionadas

Ver [`../architecture/decisions.md`](../architecture/decisions.md) para el
ADR que fija la estructura del gate local y la matriz de plataforma;
[`../reference/compatibility.md`](../reference/compatibility.md) documenta
la política de freeze que `contract-freeze` verifica.
