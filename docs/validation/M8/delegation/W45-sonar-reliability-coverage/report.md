# W45 — informes de los workers

Informes finales tal como los entregó cada worker. Las cifras de cobertura que dan
son proxies locales de macOS; la medición autoritativa está en la
[disposición](disposition.md) (Linux, reproduce SonarCloud exactamente).

## Worker Claude Opus 5 — ronda 1

**Task.** Encargo worker Claude en el worktree `rust-mcp-wt/claude` (rama `ai/sonar-claude`). Tarea A: veredicto uno a uno de los 13 `rust:S9334` con tests discriminantes. Tarea B: extracciones de funciones puras sin cambio de comportamiento en los gateways de `execution-adapter`, con sus tests. Sin commit, push ni SonarCloud.

**Result.**
- Tarea A: los 13 son falsos positivos y el código de producción no cambia. Se añadieron tests «ausente → error, `null` → `None`» donde faltaban. Verificación: se puso `#[serde(default)]` en cada uno de los 13 campos, de uno en uno, y en los 13 casos al menos un test falla.
- Tarea B: extracciones puras con tests en `rust_gateway`, `mutation_gateway`, `resolution_gateway` y `project_inspection`. Cuatro gateways pasan a usar un helper común para montar el resultado de ejecución, y cinco un único helper para el argv de `volume create`.
- Hueco de instrumentación: el `mod tests` de `rust_gateway.rs` estaba bajo `#[cfg(all(test, target_os = "macos"))]`, así que los tests de argv (portables) nunca se ejecutaban en el runner Linux de Sonar. Ahora es `#[cfg(test)]` y solo el test con Docker queda limitado a macOS.
- §4 verde; `gate.py core` 30/30.

**Files changed** (añadidas/eliminadas): `catalog-adapter/src/audit.rs` 71/0 (solo test); `domain/src/evidence.rs` 82/0 (solo test); `execution-adapter/src/project_metadata.rs` 64/0 (solo test); `rust_gateway.rs` 1086/389; `project_inspection.rs` 875/229; `mutation_gateway.rs` 411/81; `resolution_gateway.rs` 263/28; `nextest_gateway.rs` 9/43; `coverage_gateway.rs` 8/32; `mutation_test_gateway.rs` 8/32; `performance_gateway.rs` 1/12; `security_gateway.rs` 2/12. Total 2 880/858.

**Tests executed.**
- `cargo build --release --locked --offline`: pasa.
- `gate.py core`: 30/30 `passed`, `source_inputs_unchanged: true`, 2 084 tests Rust pasados y 0 fallidos.
- El primer intento falló en la etapa `architecture` por causa del worker: un test en `domain/src` usaba `serde_json::Value`, que `check-architecture.py` prohíbe en `domain`. Se reescribió sobre texto JSON y se repitió el gate.
- `contract-freeze.py verify --strict`: `passed`. `docs-hygiene` links-check: 0 rotos en documentos vivos. verify-inventories: 7/0. Snapshots sin diff.
- `cargo clippy -p rust-engineering-execution -p rust-engineering-domain --all-targets --target x86_64-unknown-linux-gnu -- -D warnings`: pasa. Compila para Linux, pero no ejecuta.

**Evidence.**
- Mutación `#[serde(default)]` campo a campo: 13/13 detectados (evidence ×3, result ×2 ya por `crates/domain/tests/contracts.rs`, audit ×2, project_metadata ×6).
- En los 37 snapshots `*-tool.json`, `error_code`/`error_message` (y `created_at`/`observed_at`/`age_seconds` donde aparecen) figuran en `required` con tipo anulable.
- Salida real de `cargo metadata` con cargo 1.98.1 (= `APPROVED_CARGO_VERSION`): siempre emite `rename`, `kind`, `target`, `source`, `registry` y `rust_version` (`null` si no aplican). Solo omite `path`, que ya tiene `#[serde(default)]`.

**S9334 verdicts.** Ver la tabla de la [disposición](disposition.md).

**Recomendación sobre el alcance de la regla.** Marcar los 13 individualmente. No usar `sonar.issue.ignore.multicriteria`: ocultaría futuros casos genuinos, porque la regla no ve `deserialize_with`.

**Coverage (proxy macOS).**
- Extraídas o compartidas, con tests de valores:
  - `rust_gateway`: `ContainerShape` (`arguments*`), `configuration_commands`/`_digest`, `concurrent_build_script_sessions`, `bounded_execution_result`.
  - `mutation_gateway`: `create_arguments`, `tmpfs_volume_arguments`, `absence_query`, configuración, `staged_result`.
  - `resolution_gateway`: `create_arguments`, `resolution_commands`/`_digest`.
  - `project_inspection`: `metadata_structure`, observaciones cargo/format/toolchain/mutation.
- Sin cubrir −1 128 y a cubrir +1 259 en `rust-engineering-execution`.

**Risks.**
- Las ediciones cambian `implementation_fingerprint` y los digests de mutation y resolution, porque incluyen los bytes de las fuentes. Pasa con cualquier edición y es intencionado.
- No se ejecutó en Linux (solo clippy con target Linux). El orden de evaluación se conservó a propósito.

**Open issues.**
- `crates/execution-adapter/src/lib.rs` tiene `#[cfg(all(test, target_os = "macos"))] mod tests`, el mismo patrón (archivo ajeno).
- Sin extraer: la orquestación Docker/LSP de `performance_gateway::execute_operation`, `security_gateway::execute_operation` y `analyzer_gateway`. Queda poca lógica pura y el riesgo es alto.

## Worker Claude Opus 5 — ronda 2

P3-1 corregido: el argv de `volume create` del volumen junit vuelve a construirse dentro de `nextest_gateway.rs`, idéntico byte a byte a la base `755d661c` (misma función `labels`, sin llamar a `mutation_gateway`). En ese archivo solo queda la llamada a `super::rust_gateway::bounded_execution_result`, que está en `implementation_fingerprint()`. Ningún test dependía del helper en esa ruta. La lista de la huella no se tocó y P3-2 no se cambió.

§4 repetido tras el cambio:
- `cargo build --release`: pasa, sha256 `9fc1829f51f2965eb66ead24920731671315f7483df108b1159c5900517e0e96`.
- `gate.py core`: 30/30 `passed`, `source_inputs_unchanged: true`, 2 084 tests, 18:40:32–18:49:33Z.
- `contract-freeze --strict`: `passed`.
- links-check: 0 rotos. Inventarios: 7/0. Snapshots vacío.

Nada más cambió; mismos 12 archivos.

## Worker Codex gpt-5.6-sol — ronda 1

Task

Completar el corte Codex de SonarCloud: localizar huecos de instrumentación, añadir pruebas portables y dejar evidencia reproducible sin commits ni cambios en archivos propiedad del worker Claude.

Result

Implementación terminada en `ai/sonar-codex`, sin commit.

- Corregidos cuatro spawns que perdían `LLVM_PROFILE_FILE`.
- Añadidas pruebas portables con aserciones reales en los siete archivos Rust priorizados.
- Incorporada cobertura hermética para `release-inventory.py`.
- Gate obligatorio: **30/30 passed**.
- Delta LCOV local Rust: **+1.080 líneas cubiertas**.
- No se añadieron exclusiones Sonar, dependencias, cambios de contrato ni snapshots.

Instrumentation gaps

- Spawns de `inspection_runtime`:
  - Causa: `env_clear()` eliminaba `LLVM_PROFILE_FILE`; el helper existente está en `crates/mcp-server/tests/inspection_runtime.rs:32`, pero no se aplicaba en `audit`, `explain`, `mutation` y `security`.
  - Arreglo: los cuatro spawns ahora pasan por `instrumented()`.
  - Liberación estimada en Linux actual: **0 líneas**, porque el target padre es `cfg(target_os = "macos")`. Sí restaura atribución para ejecuciones nativas explícitas de esos tests.

- `rust_calibration.rs`:
  - Causa: las pruebas anteriores empiezan en `crates/execution-adapter/src/rust_calibration.rs:205`, eran macOS-only, Docker y `#[ignore]`.
  - Arreglo: pruebas portables de escenarios fuente, proc-macro workspace, nombres y cancelación.
  - LCOV local liberado: **125 líneas cubiertas**.

- `semver_gateway.rs`:
  - Causa: solo se ejercía por el camino Docker dual-source.
  - Arreglo: seams puros para admisión exacta de versión y clasificación de terminación.
  - LCOV local liberado: **52 líneas**.

- `coverage_port.rs`:
  - Causa: el adapter solo era alcanzado por pruebas de inspección no portables.
  - Arreglo: pruebas de versión fijada, componente LLVM y error cerrado.
  - LCOV local liberado: **59 líneas**.

- `vendor_capture.rs`:
  - Causa: la suite anterior estaba bajo `cfg(all(test, target_os = "macos"))`.
  - Arreglo: replay, hash, rewind, digest incorrecto, cambio de longitud y cierre off-macOS portables.
  - Delta local: **114 líneas**; en Linux se espera además ejecutar por primera vez parte del código de producto previamente al 0 %.

- `catalog_cli.rs` y `lsp_session.rs`:
  - Causa: sus suites de integración relevantes eran macOS-only; `catalog_cli` lo declara en `crates/mcp-server/tests/catalog_cli.rs:1`.
  - Arreglo: parser, errores operacionales, estado LSP, clasificación de parada, presupuestos y digest probados in-process.
  - Delta local: **286** y **79 líneas**, respectivamente.

- `scripts/release-inventory.py`:
  - Causa: no tenía suite propia ni entrada bajo Coverage.py.
  - Arreglo: `scripts/test-release-inventory.py:1` prueba helpers, recorrido seguro y generate/`--check`; añadido al workflow en `.github/workflows/sonarcloud.yml:62`.
  - Proxy local `trace`: **194 líneas fuente ejecutadas**. Estimación Sonar Linux: **120–145** de las 145 líneas actualmente descubiertas.

El barrido de `crates/*/tests/**` revisó 79 sitios de spawn/comandos. Los demás helpers MCP con `env_clear()` ya preservaban el perfil; no apareció ningún `env_remove` adicional de `LLVM_PROFILE_FILE`.

Files changed

- `.github/workflows/sonarcloud.yml`: +1/−0
- `crates/execution-adapter/src/coverage_port.rs`: +85/−14
- `crates/execution-adapter/src/lsp_session.rs`: +134/−26
- `crates/execution-adapter/src/rust_calibration.rs`: +130/−0
- `crates/execution-adapter/src/semver_gateway.rs`: +73/−14
- `crates/mcp-server/src/catalog_cli.rs`: +240/−0
- `crates/mcp-server/src/stdio/quality_artifacts.rs`: +335/−0
- `crates/mcp-server/tests/inspection_runtime/audit.rs`: +1/−0
- `crates/mcp-server/tests/inspection_runtime/explain.rs`: +5/−3
- `crates/mcp-server/tests/inspection_runtime/mutation.rs`: +1/−0
- `crates/mcp-server/tests/inspection_runtime/security.rs`: +1/−0
- `crates/project-adapter/src/vendor_capture.rs`: +180/−0
- `docs/ci.md`: +3/−2
- `scripts/test-release-inventory.py`: +152/−0

Total: **+1.341/−73**.

Tests executed

Verificación obligatoria:

```text
cargo build --release --locked --offline
PASS

python3 -B scripts/gate.py core
PASS: 30/30
Report: target/gate-report.json

python3 -B scripts/contract-freeze.py verify --strict
PASS

python3 -B scripts/docs-hygiene.py links-check
PASS: 2999 enlaces resueltos, 0 rotos

python3 -B scripts/docs-hygiene.py verify-inventories
PASS: 7 inventarios, 0 fallos

git diff --stat crates/mcp-server/tests/snapshots/
PASS: salida vacía
```

Pruebas focalizadas finales:

```text
cargo test -p rust-engineering-execution --locked --offline --lib rust_calibration::portable_tests
PASS: 4

cargo test -p rust-engineering-execution --locked --offline --lib lsp_session::tests
PASS: 20

cargo test -p rust-engineering-execution --locked --offline --lib coverage_port::tests
PASS: 3

cargo test -p rust-engineering-execution --locked --offline --lib semver_gateway::tests
PASS: 2

cargo test -p rust-engineering-project --locked --offline --lib vendor_capture::portable_tests
PASS: 5 en macOS; el caso off-macOS se ejecutará en Linux

cargo test -p rust-engineering-mcp --locked --offline --bin rust-engineering-mcp catalog_cli::tests
PASS: 4

cargo test -p rust-engineering-mcp --locked --offline --bin rust-engineering-mcp stdio::quality_artifacts::tests
PASS: 13

python3 -B scripts/test-release-inventory.py
PASS: 3

python3 -B scripts/test-coverage-reports.py
PASS: 3

python3 -B scripts/test-gate-reporting.py
PASS: 13
```

Cobertura y checks adicionales:

```text
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
PASS

cargo llvm-cov clean --workspace
cargo llvm-cov --workspace --all-targets --locked --offline --lcov \
  --output-path /tmp/rust-mcp-codex-after.lcov
PASS

git diff --check
PASS
```

El primer `gate.py core` detectó usos de `expect` en pruebas nuevas; fueron eliminados y el gate completo posterior pasó. Un intento focalizado con `--lib` sobre `rust-engineering-mcp` falló porque ese package no tiene library target; se corrigió a `--bin rust-engineering-mcp` y ambas suites pasaron.

Evidence

- Gate final: `target/gate-report.json`, estado `passed`, 30 etapas, 0 no-passed.
- LCOV anterior: `/tmp/rust-mcp-codex-before.lcov`.
- LCOV posterior: `/tmp/rust-mcp-codex-after.lcov`.
- Documentación del nuevo recorrido Python actualizada en `docs/ci.md:18`.
- Ningún archivo prohibido del worker Claude aparece en `git diff`.
- `git diff --check` limpio; snapshots sin diferencias.
- No se modificó `sonar-project.properties`.

Coverage

| Archivo | Antes local | Después local | Delta cubierto |
|---|---:|---:|---:|
| `quality_artifacts.rs` | 63/737, 8,5 % | 427/980, 43,6 % | +364 |
| `catalog_cli.rs` | 212/339, 62,5 % | 498/553, 90,1 % | +286 |
| `rust_calibration.rs` | 0/398, 0,0 % | 125/487, 25,7 % | +125 |
| `vendor_capture.rs` | 231/244, 94,7 % | 345/353, 97,7 % | +114 |
| `lsp_session.rs` | 703/770, 91,3 % | 782/847, 92,3 % | +79 |
| `coverage_port.rs` | 0/103, 0,0 % | 59/150, 39,3 % | +59 |
| `semver_gateway.rs` | 0/160, 0,0 % | 52/204, 25,5 % | +52 |
| Workspace Rust | 72.456/93.086, 77,84 % | 73.536/93.909, 78,31 % | **+1.080** |

Estimación Linux: aproximadamente **1.080 líneas Rust más 120–145 Python**, antes de sumar el trabajo del worker Claude. La cifra definitiva requiere el runner Linux/SonarCloud: macOS ya cubría caminos que Linux no ejecutaba y las pruebas inline aumentan también el denominador.

Risks

- Este worker por sí solo no demuestra el umbral remoto del 80 %; el cálculo debe hacerse tras integrar ambos workers y ejecutar SonarCloud.
- Coverage.py 7.16.0 no está instalado localmente (`python3 -m coverage` devolvió `No module named coverage`). La suite Python pasó y `trace` confirmó sus caminos, pero la cifra Cobertura/Sonar solo puede obtenerse en CI.
- El passthrough de los cuatro spawns de `inspection_runtime` no produce ganancia en Linux mientras el target completo siga siendo macOS-only.
- Las pruebas inline cuentan como líneas Rust medibles; aunque están cubiertas, elevan el denominador respecto de la aritmética inicial de +1.113 sobre un total fijo.

Decisions

- Se preservaron exactamente contratos públicos y comportamiento: las extracciones de funciones son puras y reemplazan lógica idéntica.
- No se relajó seguridad, aislamiento de entorno ni validación de versiones.
- No se añadieron exclusiones, dependencias, snapshots, commits ni artefactos Docker.
- Se siguió la secuencia implementación → QA → gate de seguridad/documentación indicada por el workflow del repositorio.

Open issues

- El orquestador debe integrar ambos worktrees y ejecutar el job Linux de SonarCloud para confirmar el porcentaje final.
- Conviene revisar el nuevo denominador remoto antes de decidir si hacen falta más pruebas portables.
- No hay petición de extracción ni cambio pendiente para el worker Claude.

## Worker Codex gpt-5.6-sol — ronda 2

Task

Ronda 2 de correcciones de revisión y cobertura portable, sin commit, en `ai/sonar-codex`.

Result

Completadas las cuatro correcciones obligatorias y un segundo tramo de pruebas portables. El gate final pasó 30/30. Proyección conservadora para Linux: aproximadamente 310–335 líneas de producto adicionales cubiertas, pendiente de la medición autoritativa del orquestador.

Instrumentation gaps

- `env_clear()` eliminaba `LLVM_PROFILE_FILE` al lanzar el binario MCP desde integración: causa en `crates/mcp-server/tests/inspection_runtime.rs:385`. El helper de `crates/mcp-server/tests/inspection_runtime.rs:36` repone únicamente esa variable en cinco sitios de spawn, sin heredar PATH, credenciales ni configuración ambiente. Estimación: 120–220 líneas, parcialmente solapadas con otros tests.
- `scripts/release-inventory.py` no tenía test ejecutado bajo Coverage.py. Se añadió un arnés hermético in-process y se incorporó al workflow en `.github/workflows/sonarcloud.yml:62`. Estimación: 105–125 líneas Python.
- `rust_calibration.rs`: su ejecución real dependía de tests Docker/macOS ignorados en Linux. Los tests portables ejercen escenarios, bundles y cancelación; el orquestador observó 125 líneas cubiertas tras la ronda 1.
- `semver_gateway.rs`: el camino integral requiere Docker. Se extrajeron y probaron la admisión exacta de versión y la clasificación de terminación. Proxy local: 52 líneas cubiertas.
- `coverage_port.rs`: misma dependencia del gateway Docker. Se probaron in-process los probes cerrados de versión y componentes. Proxy local: 59 líneas.
- `vendor_capture.rs`: los tests previos eran APFS/macOS. Se añadieron replay, digest, mutación y rechazo off-macOS portables; el test focalizado alcanza aproximadamente 109 líneas de producto.
- La revisión de los demás `env_clear()` mostró que los restantes lanzan Docker o helpers de prueba, no el binario instrumentado; no se propagó el perfil indiscriminadamente.

Files changed

Total: 21 archivos, `+2542/-191`.

- `.github/workflows/sonarcloud.yml` `+1/-0`
- `crates/execution-adapter/src/capabilities.rs` `+195/-74`
- `crates/execution-adapter/src/coverage_port.rs` `+85/-14`
- `crates/execution-adapter/src/lib.rs` `+120/-56`
- `crates/execution-adapter/src/lsp_session.rs` `+136/-26`
- `crates/execution-adapter/src/nextest_port.rs` `+24/-0`
- `crates/execution-adapter/src/rust_calibration.rs` `+130/-0`
- `crates/execution-adapter/src/semver_gateway.rs` `+73/-14`
- `crates/mcp-server/src/cargo_vendor_cli.rs` `+47/-0`
- `crates/mcp-server/src/catalog_cli.rs` `+281/-0`
- `crates/mcp-server/src/stdio/catalog/provider.rs` `+186/-0`
- `crates/mcp-server/src/stdio/mutation.rs` `+533/-0`
- `crates/mcp-server/src/stdio/quality_artifacts.rs` `+339/-0`
- `crates/mcp-server/src/stdio/security_contract_tests.rs` `+49/-2`
- `crates/mcp-server/tests/inspection_runtime/audit.rs` `+1/-0`
- `crates/mcp-server/tests/inspection_runtime/explain.rs` `+5/-3`
- `crates/mcp-server/tests/inspection_runtime/mutation.rs` `+1/-0`
- `crates/mcp-server/tests/inspection_runtime/security.rs` `+1/-0`
- `crates/project-adapter/src/vendor_capture.rs` `+180/-0`
- `docs/ci.md` `+3/-2`
- `scripts/test-release-inventory.py` `+152/-0`, nuevo

Tests executed

- `cargo build --release --locked --offline` — pass final.
- `python3 -B scripts/gate.py core` — pass final, 30/30; `target/gate-report.json`.
- `python3 -B scripts/contract-freeze.py verify --strict` — `status: passed`.
- `python3 -B scripts/docs-hygiene.py links-check` — 2999 enlaces vivos, 0 rotos.
- `python3 -B scripts/docs-hygiene.py verify-inventories` — 7 inventarios, 0 fallos.
- `git diff --stat crates/mcp-server/tests/snapshots/` — salida vacía.
- `git diff --check` — pass.
- `cargo llvm-cov --workspace --all-targets --locked --offline --lcov --output-path /tmp/rust-mcp-codex-round2-final2.lcov` — pass.
- `cargo test -p rust-engineering-mcp --locked --offline stdio::mutation::tests` — 20 passed.
- `cargo test -p rust-engineering-mcp --locked --offline stdio::security_contract_tests::test_coverage_semver_and_mutation_task_routing_is_closed` — 1 passed.
- `cargo clippy -p rust-engineering-mcp --all-targets --locked --offline -- -D warnings` — pass.
- `cargo llvm-cov -p rust-engineering-mcp --locked --offline --lcov --output-path /tmp/rust-mcp-routing-focused.lcov -- stdio::security_contract_tests::test_coverage_semver_and_mutation_task_routing_is_closed` — pass; 23 líneas nuevas de `stdio.rs`.
- `cargo llvm-cov -p rust-engineering-mcp --locked --offline --lcov --output-path /tmp/rust-mcp-catalog-errors-focused.lcov -- catalog_cli::tests::operational_errors_have_exact_stable_codes_and_guidance` — pass; 82 líneas del mapeo de producto ejecutadas.
- `python3 -B scripts/test-release-inventory.py` — 3 passed.
- `python3 -B scripts/test-gate-reporting.py` — 13 passed.

Evidence

- Comentario restaurado en `crates/execution-adapter/src/lsp_session.rs:241`.
- Digest SHA-256 exacto fijado en `crates/mcp-server/src/stdio/quality_artifacts.rs:1250`.
- 27 pares exactos variante→código en `crates/mcp-server/src/catalog_cli.rs:664`.
- Rama muerta sustituida por indexación directa en `crates/mcp-server/src/stdio/quality_artifacts.rs:1329`.
- El test Linux-only de catálogo verifica `unsupported_platform` y `feature_disabled` en `crates/mcp-server/src/stdio/catalog/provider.rs:648`.
- No se modificaron snapshots, contratos públicos, dependencias, exclusiones Sonar ni archivos propiedad del worker Claude.
- El uso del workflow de sprint guio la secuencia pruebas focalizadas → cobertura → gate completo, sin ampliar el alcance arquitectónico.

Coverage

Medición local macOS de la ronda 2:

- Base post-ronda 1: `73 536/93 909`, 78,306 %, 20 373 sin cubrir.
- Full LCOV final antes del último test focalizado: `74 599/94 726`, 78,752 %, 20 127 sin cubrir.
- El test final cubre 23 líneas existentes adicionales sin ampliar el denominador de `stdio.rs`: estimación final ajustada `74 622/94 726`, 78,777 %, 20 104 sin cubrir.
- Delta local ronda 2: aproximadamente `−269` líneas sin cubrir.

Cambios principales:

- `capabilities.rs`: `182/301 → 314/396`; sin cubrir `119 → 82`, −37.
- `lib.rs`: `101/499 → 205/552`; `398 → 347`, −51.
- `nextest_port.rs`: `65/174 → 102/196`; `109 → 94`, −15.
- `cargo_vendor_cli.rs`: `85/206 → 149/245`; `121 → 96`, −25.
- `mutation.rs`: `1055/1380 → 1608/1850`; `325 → 242`, −83.
- `stdio.rs`: 23 líneas de producto adicionales cubiertas por routing portable.
- `quality_artifacts.rs`: dos líneas muertas eliminadas; sin cubrir −1.
- `catalog_cli.rs`: 82 líneas del mapeo exacto ejecutadas por el test focalizado; en macOS estaban ya cubiertas por integración, pero gran parte estaba ausente en Linux.
- `catalog/provider.rs`: 109 líneas de producto ejecutadas por los tests portables; el caso configurado off-macOS añade caminos exclusivos del runner Linux.

Estimación Linux respecto a la base proporcionada por el orquestador: `−310 a −335` líneas de producto sin cubrir. La cifra final debe confirmarse con su contenedor Linux autoritativo.

Risks

- No pude medir Linux real en este worktree. El cross-check `cargo check ... --target aarch64-unknown-linux-gnu` quedó bloqueado por ausencia de `aarch64-linux-gnu-gcc` al compilar `ring`; no se declara pass.
- Coverage.py no está instalado en el Python local (`No module named coverage`). Los tests Python directos pasan, pero el porcentaje Python corresponde medirlo en el job que instala la wheel fijada.
- La estimación Linux depende del solapamiento entre los caminos de catálogo ya cubiertos y los 82/109 caminos focalizados; se usó el extremo conservador.
- Los tests Docker/APFS históricos permanecen `ignored`; no se cuentan como ejecutados ni como evidencia nueva.

Decisions

- Se priorizaron funciones puras y rutas portables, con aserciones exactas de contrato.
- Las extracciones en `capabilities.rs` y `lib.rs` conservan el comportamiento: el gateway sigue realizando I/O y delega únicamente la evaluación/construcción determinista.
- El perfil LLVM se propaga exclusivamente a procesos del binario bajo cobertura.
- Sin commit, push, PR, dependencia nueva ni modificación de `sonar-project.properties`.

Open issues

- El orquestador debe ejecutar la medición Linux autoritativa y confirmar el delta `sin cubrir`.
- Si el resultado cae por debajo de −300, el siguiente corte portable recomendado es `stdio.rs`/`tasks.rs`; no hace falta pedir extracción al worker Claude.
- No quedaron peticiones al worker Claude ni cambios pendientes de contrato.
