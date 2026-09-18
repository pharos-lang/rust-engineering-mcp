# W45 — disposición del orquestador (2026-09-18)

Encargo: [sonarqube-reliability-coverage](../../../../prompts/sonarqube-reliability-coverage.md).
Rama de integración `ai/sonar-reliability-coverage` desde `main` `2fd9cf7e`.
Workers en worktrees separados (`ai/sonar-claude`, `ai/sonar-codex`), propiedad de
archivos disjunta, sin commits propios. Encargos exactos en
[prompt-header.md](prompt-header.md), informes en [report.md](report.md) y hashes
de transcripts, recibos y LCOV en [transcripts.sha256](transcripts.sha256).

| Participación | Modelo / invocación | Resultado |
| --- | --- | --- |
| Worker Claude, ronda 1 | Claude Opus 5, subagente `rust-engineer` (6 210 s) | §1 completo + extracciones; gate `core` 30/30 |
| Revisión de Claude | Claude Opus 5, solo lectura (458 s) | **Aprobado**: 0 P0/P1/P2; P3-1 huella de nextest, P3-2 `labels` duplicado |
| Worker Claude, ronda 2 | mismo agente (649 s) | P3-1 corregido; gate `core` 30/30, 18:40:32–18:49:33Z |
| Worker Codex, ronda 1 | `gpt-5.6-sol` high, `codex exec` (codex-cli 0.154.0) | huecos + tests portables; gate `core` 30/30 |
| Revisión de Codex, ronda 1 | Claude Opus 5, solo lectura (613 s) | **Aprobado con cambios menores**: 0 P0/P1; P2 cifra del informe; P3 aserciones débiles, rama muerta |
| Worker Codex, ronda 2 | `codex exec resume` misma sesión | 4 correcciones + segundo tramo; gate `core` 30/30, 19:08:09–19:12:31Z |
| Revisión de Codex, ronda 2 | mismo revisor (163 s) | **Aprobado con cambios menores**: extracciones `capabilities`/`lib` equivalentes; P3 `cfg` del test de `provider.rs` → aplicado por el orquestador |

Codex figura como worker con autorización explícita del owner para este encargo
(nota de política de §5 del encargo); sus recibos M8-04 como cliente stock no cambian.

## 1. Los 13 hallazgos `rust:S9334`

Veredicto: **13 falsos positivos; el código de producción no se toca.** Todos usan
`required_nullable` (domain) o `nullable` (catalog/execution): `Option<T>` significa
«presente pero anulable». La regla no ve `deserialize_with` y pide
`#[serde(default)]`, que haría que un campo ausente pasara a `None` en silencio.

Evidencia discriminante:
- El worker añadió tests «ausente → error, `null` → `None`» donde faltaban (`result.rs` ya estaba cubierto por `crates/domain/tests/contracts.rs`).
- Mutó cada campo con `#[serde(default)]` y en los 13 casos algún test falla.
- El revisor repitió la mutación en `age_seconds` y `Dependency.registry`: ambos detectados.

Justificaciones listas para pegar como comentario de *false positive*:

| Key | Archivo:línea | Justificación |
| --- | --- | --- |
| `AaCPhBHIM4B2wCqomAcB` | `crates/catalog-adapter/src/audit.rs:28` (`created_at`) | Required-nullable by design: the host must state an unknown timestamp as `null`; omission is a malformed transport and must fail. `#[serde(default)]` would silently accept a truncated snapshot. Pinned by `audit::required_nullable_contract`. |
| `AaCPhBHIM4B2wCqomAcC` | `crates/catalog-adapter/src/audit.rs:30` (`observed_at`) | Same as `created_at`: required-nullable field of our own fingerprinted transport; absence → `InvalidSnapshot`, explicit `null` → `None`. Pinned by `audit::required_nullable_contract`. |
| `AaCPhBAYM4B2wCqomAb2` | `crates/domain/src/evidence.rs:51` (`created_at`) | Field uses the `required_nullable` deserializer: present-but-nullable, published as `required` in the frozen output schema. Adding `serde(default)` would diverge from the contract. Pinned by `evidence::required_nullable_contract`. |
| `AaCPhBAYM4B2wCqomAb3` | `crates/domain/src/evidence.rs:53` (`observed_at`) | Same required-nullable contract as `created_at` (frozen schema lists it as required). Absent → error, `null` → `None`; pinned by tests. |
| `AaCPhBAYM4B2wCqomAb4` | `crates/domain/src/evidence.rs:229` (`age_seconds`) | Required-nullable freshness fact (frozen schema: required). With `serde(default)` a missing age would be indistinguishable from a legitimately unknown one. Pinned by `evidence::required_nullable_contract`. |
| `AaCPhBEFM4B2wCqomAb5` | `crates/domain/src/result.rs:120` (`error_code`) | Required-nullable envelope field of every tool output (frozen schema: required in every `oneOf` branch). Absence must fail; covered by `tests/contracts.rs::deserialization_cannot_bypass_outcome_invariants`. |
| `AaCPhBEFM4B2wCqomAb6` | `crates/domain/src/result.rs:122` (`error_message`) | Same as `error_code`: required-nullable envelope field; absence rejected by existing contract tests. |
| `AaCPhBFeM4B2wCqomAb7` | `crates/execution-adapter/src/project_metadata.rs:68` (`rust_version`) | Parses `cargo metadata` from the pinned image toolchain (cargo 1.98.1), which always emits this key (`null` when undeclared). A missing key means unexpected metadata and fails closed; the helper is documented "must not silently default". Pinned by existing and new tests. |
| `AaCPhBFeM4B2wCqomAb8` | `project_metadata.rs:89` (`rename`) | Always serialized by the pinned Cargo (`null` when unset); omission is malformed metadata and must fail, not default. Pinned by `dependency_nullable_facts_accept_null_but_reject_omission`. |
| `AaCPhBFeM4B2wCqomAb9` | `project_metadata.rs:92` (`kind`) | `null` means *normal*; defaulting a missing key would silently reclassify a dependency. Pinned Cargo always emits it; pinned by test. |
| `AaCPhBFeM4B2wCqomAb-` | `project_metadata.rs:97` (`target`) | Always emitted by the pinned Cargo; `null` = unconditional. Omission fails closed; pinned by test. |
| `AaCPhBFeM4B2wCqomAb_` | `project_metadata.rs:99` (`source`) | Always emitted; `null` identifies path dependencies, so defaulting a missing key would accept it as a path dependency. Pinned by test (path baseline). |
| `AaCPhBFeM4B2wCqomAcA` | `project_metadata.rs:103` (`registry`) | Always emitted (`null` = crates.io); omission is malformed and must fail. Pinned by test. |

Decisión: marcar los 13 individualmente en SonarCloud. No se ajusta el alcance de
la regla con `sonar.issue.ignore.multicriteria`, porque ocultaría futuros casos
genuinos. El marcado exige un token o la sesión del owner en SonarCloud, que el
orquestador no tiene: **acción pendiente del owner** (§5). No bloquea el quality
gate, cuya única condición roja era `new_coverage`.

## 2. Cobertura: medición en Linux, no estimada

**Método.**
- El runner de SonarCloud (Linux, sin Docker) se reprodujo con la imagen aprobada que sobrevive, `rust-engineering-runtime:1.98.1-arm64-m6`. La imagen no se modificó: contenedores efímeros `--network none`.
- Toolchain de la imagen: `cargo`/`rustc` 1.98.1 con `cargo-llvm-cov` y `llvm-tools`. Se ejecuta con UID 1001 (el runner no es root).
- Comando del job: `cargo llvm-cov --workspace --all-targets --locked --lcov`.
- El LCOV se agrega con el alcance de `sonar-project.properties`: se excluyen `crates/**/tests/**` y los `*_native.rs` declarados como test.

**Calibración.**
- Sobre la base `755d661c`, la medición da **79 711 líneas a cubrir y 16 917 sin cubrir**, idéntico a SonarCloud. Por archivo también coincide: `rust_gateway.rs` 1 589, `quality_artifacts.rs` 701, `lsp_session.rs` 442, `catalog_cli.rs` 324, `rust_calibration.rs` 212.
- Un primer intento como root falló `mutation_list_on_an_unreadable_state_root_is_an_io_error`, porque root lee cualquier directorio: fue un artefacto de entorno, resuelto con UID no root.

| Árbol (Linux) | Rust a cubrir | Cubiertas | Sin cubrir | % Rust | Tests |
| --- | --- | --- | --- | --- | --- |
| Base `755d661c` (= SonarCloud) | 79 711 | 62 794 | 16 917 | 78,78 | verdes |
| Solo Claude, ronda 1 | 81 371 | 65 676 | 15 695 (−1 222) | 80,71 | verdes |
| Solo Codex, ronda 1 | 80 551 | 64 090 | 16 461 (−456) | 79,56 | verdes |
| **Integrado** (Claude r2 + Codex r2) | **83 059** | **68 194** | **14 865 (−2 052)** | **82,10** | verdes |

Lectura contra el objetivo del encargo (+1 113 líneas cubiertas sobre 80 821):
- Los tests inline suben también el denominador, así que la cifra útil es la de **líneas de producto sin cubrir**: **−2 052**.
- Global, sumando Python como está hoy en SonarCloud (1 110 a cubrir / 750 cubiertas): **68 944 / 84 169 = 81,91 %**. Todavía sin la mejora de `release-inventory.py`, que Coverage.py mide en CI y aquí no: Coverage.py no está instalado en el host.
- `new_coverage` de `main`: 48 805 / 62 281 más el delta dan ≈ 82,6 %.
- Líneas nuevas del PR (Rust, mismas mediciones): 3 744 / 3 962 = **94,5 %**.

**Huecos de instrumentación encontrados** (precedente W39b):
- `rust_gateway.rs`: el `mod tests` completo era `cfg(macos)`, aunque sus tests de argv son portables.
- `rust_calibration.rs`, `vendor_capture.rs`, `semver_gateway.rs` y `coverage_port.rs` estaban al 0 % en Linux porque sus suites eran macOS/Docker/`#[ignore]`.
- `scripts/release-inventory.py` no tenía suite bajo Coverage.py.
- Cuatro spawns de `inspection_runtime` no reinyectaban `LLVM_PROFILE_FILE`. Esa suite es solo macOS, así que no aporta en Linux.

**Extracciones de producto**, todas revisadas como equivalentes (mismo argv, flags, entorno, errores y orden de evaluación):
- `rust_gateway`: `ContainerShape`, `configuration_commands`/`_digest`, `bounded_execution_result`.
- `mutation_gateway`: `create_arguments`, `tmpfs_volume_arguments`, `staged_result`.
- `resolution_gateway`: `create_arguments`, `resolution_commands`/`_digest`.
- `project_inspection`: observaciones.
- `capabilities`: `assess_capabilities`.
- `lib`: `create_arguments`.
- `lsp_session`: `classify_stop`, `parse_status`.
- `semver_gateway`: `admitted_version`, `classify_termination`.
- `coverage_port`: `version_available`, `components_available`.

`implementation_fingerprint()` hashea los bytes de estas fuentes y es observacional:
se emite en los recibos y ninguna constante aprobada lo compara. El cambio de huella
es el esperado ante cualquier edición y no rompe el runtime.

## 3. Verificación §4 sobre el árbol integrado

Ejecutada por el orquestador sobre `09d2c7e1` (todo lo integrado; el paquete
documental se añade después y el gate no inventaría `docs/`):

| Comprobación | Resultado |
| --- | --- |
| `cargo build --release --locked --offline` | pasa; `rust-engineering-mcp` sha256 `f94f93e918bbdcb12a8ff7f43333d1abb6490ad3625667944b972d34fceaa90b` |
| `python3 -B scripts/gate.py core` | **30/30 `passed`**, `source_inputs_unchanged: true`, 19:20:43–19:41:20Z, cargo/rustc 1.98.1; etapa `test` 2 063 pasados / 0 fallidos / 153 ignorados (base 1 984; Claude +31, Codex +48, unión exacta); recibo sha256 `d742c0fe0842fa4117dfe699f57c1013631f70c8bafe9539d9e8e3761f0af064` |
| `contract-freeze.py verify --strict` | `status: passed`, sin cambios stable/preview/class |
| `docs-hygiene.py links-check` | 0 rotos en documentos vivos |
| `docs-hygiene.py verify-inventories` | 7 inventarios, 0 fallos |
| `git diff --stat main -- crates/mcp-server/tests/snapshots/` | vacío |

Los 153 ignorados son los tests Docker/nativos ya existentes: no se cuentan como pass.

## 4. Integración

Commits del orquestador, que no reescriben ni mezclan el trabajo de los workers:

| Commit | Contenido | Autoría del cambio |
| --- | --- | --- |
| `755d661c` | encargo en `docs/prompts/` | orquestador |
| `9161ac46` | tests de contrato S9334 | worker Claude |
| `969964cc` | extracciones de gateways | worker Claude |
| `326bcd04` | merge `ai/sonar-claude` | orquestador |
| `7d438a22` | Coverage.py sobre `release-inventory.py` | worker Codex |
| `d84b6988` | `LLVM_PROFILE_FILE` en `inspection_runtime` | worker Codex |
| `5151b362` | tests portables `execution`/`project` | worker Codex |
| `739ee36a` | tests portables `mcp-server` | worker Codex |
| `89d48f21` | merge `ai/sonar-codex` | orquestador |
| `09d2c7e1` | P3 de la revisión: `cfg(not(feature = "local"))` en el test de `provider.rs` | orquestador |

La cifra definitiva en SonarCloud se lee en el análisis del PR y, tras el merge, en `main`.

## 5. Lo que quedó sin hacer y por qué

- **Marcar los 13 falsos positivos en SonarCloud.** Requiere token o la sesión del owner. Las justificaciones de §1 están listas para pegar. Hasta entonces, `bugs` = 13 y `reliability_rating` = D en el overall, aunque no están en código nuevo y no afectan al quality gate.
- **Cobertura Python** no medida localmente (Coverage.py ausente en el host). Se mide en el job de CI.
- **Sin extraer** (riesgo alto y poca lógica pura): la orquestación Docker/LSP de `performance_gateway::execute_operation`, `security_gateway::execute_operation` y `analyzer_gateway`.
- **P3-2** (Claude): `labels` duplicado en `rust_gateway.rs`/`mutation_gateway.rs`; comportamiento idéntico. Anotado, sin cambio.
- **Open issue del worker Claude sobre `lib.rs`**: comprobado por el orquestador, sin acción. El `#[cfg(all(test, target_os = "macos"))]` de `crates/execution-adapter/src/lib.rs:825` es `mod unsafe_native`, un arnés nativo declarado como test en `sonar.test.inclusions`, como `security_native*` y `analyzer_native`. No es un `mod tests` portable ni un hueco de medición.
- **`gate.py full`** sigue sin poder ejecutarse (imágenes M3/M4/M5 borradas). Todo lo aquí calificado es portable (`core`); la reprovisión está planificada aparte, antes de 1.0.0.
