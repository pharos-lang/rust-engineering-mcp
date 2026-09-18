# W45 — SonarCloud en `main`: 13 × `rust:S9334` y cobertura 78,5 % → ≥ 80 %

Encargo base: [sonarqube-reliability-coverage](../../../../prompts/sonarqube-reliability-coverage.md).
Orquestador: Claude Opus 5 (Claude Code, skill `project-orchestrator`). Workers en
paralelo, cada uno en su propio `git worktree` (el gate `core` rechaza la
calificación si las fuentes cambian mientras corre), con propiedad de archivos
disjunta y sin commits (integra el orquestador):

| Worker | Invocación | Worktree / rama |
| --- | --- | --- |
| Claude Opus 5 | subagente `rust-engineer`, `model: opus` | `rust-mcp-wt/claude` · `ai/sonar-claude` |
| Codex `gpt-5.6-sol` | `codex exec -m gpt-5.6-sol -c model_reasoning_effort="high" -C <worktree> --json -o <last> -` (codex-cli 0.154.0); ronda 2 con `codex exec resume <thread>` | `rust-mcp-wt/codex` · `ai/sonar-codex` |

Revisores: dos subagentes Claude Opus 5 de solo lectura, uno por entrega.

---

## Encargo al worker Claude (ronda 1)

# Worker Claude Opus — encargo SonarCloud (§1 completo + extracciones puras de §2)

Eres un worker delegado por el orquestador. **Tu único árbol de trabajo es el
worktree `/Users/cburgosro/Projects/rust-mcp-wt/claude`** (rama `ai/sonar-claude`,
creada desde `ai/sonar-reliability-coverage` = `main` + el encargo). Haz `cd` a esa
ruta en cada comando Bash y usa rutas absolutas bajo ella. **No toques nunca**
`/Users/cburgosro/Projects/rust-mcp` (árbol del orquestador) ni
`/Users/cburgosro/Projects/rust-mcp-wt/codex` (otro worker en paralelo).

Lee primero, en el worktree:

1. `docs/prompts/sonarqube-reliability-coverage.md` — el encargo completo. Tu parte
   es la de «Claude Code Opus» en §5. Sus prohibiciones y criterios aplican literalmente.
2. `AGENTS.md` — reglas del repo (seguridad, contratos, calidad, formato de entrega).

## Tarea A — §1: los 13 hallazgos `rust:S9334` (trabajo de criterio, uno a uno)

Estado live verificado por el orquestador hoy: 13 issues abiertos, todos
`rust:S9334` CRITICAL, en `crates/catalog-adapter/src/audit.rs:28,30`,
`crates/domain/src/evidence.rs:51,53,229`, `crates/domain/src/result.rs:120,122`,
`crates/execution-adapter/src/project_metadata.rs:68,89,92,97,99,103`.

Para **cada uno** aplica el procedimiento de §1 (helper `required_nullable`/`nullable`,
`deny_unknown_fields`, procedencia del JSON —externa ajena vs. contrato propio—,
participación en el contrato congelado / snapshots `crates/mcp-server/tests/snapshots/*-tool.json`).
Ojo especial con `project_metadata.rs`: parsea salida de `cargo metadata` (entrada
externa); decide con evidencia si un campo ausente allí debe fallar o tolerarse
(mira qué versiones de Cargo emiten ese campo siempre y qué hace hoy el código
cuando falta). Si alguno es un campo genuinamente opcional, arréglalo y justifícalo;
si no, el código no se toca.

Evidencia discriminante: para cada struct afectada, comprueba si ya existe un test
que demuestre «campo ausente → error; `null` explícito → `None`». Si no existe,
añádelo (test que puede fallar: si alguien pusiera `#[serde(default)]`, debe romper).
Eso fija el contrato y convierte el veredicto en algo verificable.

Prohibido: `#[serde(default)]` para bajar el contador; `// NOSONAR`; exclusiones de
archivos. No marques nada en SonarCloud (no tienes token): entrega para cada issue su
key (abajo), veredicto y un texto de justificación de 1–3 frases listo para pegar
como comentario de *false positive*. Si recomiendas además ajustar el alcance de la
regla en `sonar-project.properties`, **no lo edites** (es archivo del otro worker):
propón el texto exacto y el orquestador decide.

Keys: audit.rs:28 `AaCPhBHIM4B2wCqomAcB`, :30 `AaCPhBHIM4B2wCqomAcC`;
evidence.rs:51 `AaCPhBAYM4B2wCqomAb2`, :53 `AaCPhBAYM4B2wCqomAb3`, :229 `AaCPhBAYM4B2wCqomAb4`;
result.rs:120 `AaCPhBEFM4B2wCqomAb5`, :122 `AaCPhBEFM4B2wCqomAb6`;
project_metadata.rs:68 `AaCPhBFeM4B2wCqomAb7`, :89 `…Ab8`, :92 `…Ab9`, :97 `…Ab-`,
:99 `…Ab_`, :103 `AaCPhBFeM4B2wCqomAcA`.

## Tarea B — §2: extracciones de funciones puras + sus tests

Objetivo global del encargo: **+1 113 líneas cubiertas** en el runner Linux sin Docker
(la cobertura la produce `cargo llvm-cov --workspace --all-targets --locked` en
`.github/workflows/sonarcloud.yml`). El otro worker (Codex) busca huecos de
instrumentación y prueba los archivos que no requieren extracción. Tú haces las
extracciones sin cambio de comportamiento en los gateways con más masa sin cubrir y
**escribes tú mismo los tests de lo que extraes** (una extracción sin test no aporta
cobertura y no demuestra equivalencia). Precedente: W39 (`classify_mutation_records`,
`build_report`/`render`) — búscalo en el historial/código para imitar el estilo.

Sin cubrir en SonarCloud hoy (líneas / cobertura): `rust_gateway.rs` 1 589 / 17,9 %;
`performance_gateway.rs` 1 153 / 68,0 %; `project_inspection.rs` 816 / 36,1 %;
`security_gateway.rs` 753 / 57,8 %; `mutation_gateway.rs` 681 / 34,6 %;
`resolution_gateway.rs` 545 / 48,9 %; `analyzer_gateway.rs` 495 / 75,8 %;
`mutation_test_gateway.rs` 280 / 31,0 %; `coverage_gateway.rs` 271 / 13,4 %;
`nextest_gateway.rs` 221 / 41,4 % (todos en `crates/execution-adapter/src/`).
Prioriza por líneas cubribles por hora, no por orden. Candidatos típicos: construcción
de argv/entorno desde tipos validados, parseo de salidas, clasificación de resultados,
mapeo de errores, validación de config. Tests con aserciones reales sobre valores
(nada de ejecutar sin aseverar). Antes de extraer, mira si ya existe una función pura
sin test: a veces basta con probarla.

Reglas de extracción: comportamiento idéntico (mismos argv, mismo orden, mismos
errores, mismo entorno limpio), sin tocar contratos públicos ni snapshots, sin nuevas
dependencias (`--offline`), sin nuevos módulos en `lib.rs` (extrae dentro del mismo
archivo, en funciones privadas `fn`/`pub(crate) fn` con tests en el `#[cfg(test)] mod`
del propio archivo o en el estilo que ya use ese archivo). Respeta la seguridad de
`AGENTS.md` (gateway único, env limpio, sin `sh -c`).

## Propiedad de archivos (disjunta con Codex — obligatoria)

Puedes editar SOLO:
- `crates/domain/src/evidence.rs`, `crates/domain/src/result.rs`, `crates/domain/src/lib.rs`
  (solo tests si hace falta), `crates/catalog-adapter/src/audit.rs`,
  `crates/execution-adapter/src/project_metadata.rs`;
- `crates/execution-adapter/src/{rust_gateway,performance_gateway,project_inspection,security_gateway,mutation_gateway,resolution_gateway,analyzer_gateway,mutation_test_gateway,coverage_gateway,nextest_gateway}.rs`;
- tests nuevos de integración solo si son imprescindibles y con prefijo `sonar_claude_`
  en `crates/<crate>/tests/`.

Todo lo demás es de Codex u orquestador: en particular `crates/execution-adapter/src/lib.rs`,
`lsp_session.rs`, `rust_calibration.rs`, `semver_gateway.rs`, `coverage_port.rs`,
`crates/mcp-server/**`, `crates/project-adapter/**`, helpers de `tests/` existentes,
`scripts/**`, `.github/**`, `sonar-project.properties`, `docs/**`, `Cargo.*`.
Si necesitas algo de un archivo ajeno, **no lo toques**: descríbelo en tu entrega en
«Open issues» con el cambio exacto pedido.

## Entorno

- Docker: las imágenes aprobadas de M3/M4/M5 fueron borradas; no intentes
  reprovisionarlas; `gate.py full` no es ejecutable. Solo vía portable.
- `target/debug` y `target/release` del worktree son un clon APFS de la caché
  principal (pueden estar copiándose aún al arrancar; si `cargo` recompila mucho, es
  normal para los crates del workspace). No exportes `CARGO_TARGET_DIR`.
- El otro worker compila en paralelo en su propio worktree: habrá contención de CPU.
- Medición local opcional del delta por archivo: `cargo llvm-cov -p execution-adapter
  --locked --offline --lcov --output-path /tmp/...` (macOS difiere del runner Linux;
  úsalo como proxy, no como cifra final).
- **No hagas commit, push, PR ni nada en SonarCloud.** Deja los cambios sin commitear en
  el worktree; el orquestador revisa, commitea e integra.

## Verificación obligatoria antes de entregar (§4, completa, en tu worktree)

```sh
cargo build --release --locked --offline
python3 -B scripts/gate.py core            # 30/30 etapas, status passed
python3 -B scripts/contract-freeze.py verify --strict
python3 -B scripts/docs-hygiene.py links-check
python3 -B scripts/docs-hygiene.py verify-inventories
git diff --stat crates/mcp-server/tests/snapshots/   # debe estar vacío
```

Nada que no corrió es pass; un skip o `unavailable` no es pass. Si algo falla por
causa ajena a tu cambio, demuéstralo (p. ej., falla igual en el árbol sin tus cambios)
en vez de declararlo pass.

## Formato de entrega (tu mensaje final, en español)

```text
Task
Result
Files changed        (con líneas añadidas/eliminadas por archivo)
Tests executed       (comandos exactos + resultado; gate core: N/30 y ruta del report JSON)
Evidence
S9334 verdicts       (tabla: key | archivo:línea | veredicto FP/fix | razonamiento | justificación lista para pegar)
Coverage             (por archivo: funciones extraídas/probadas; estimación local de líneas nuevas cubiertas)
Risks
Decisions
Open issues          (peticiones a Codex/orquestador; lo que quedó sin hacer y por qué)
```

## Encargo al worker Claude (ronda 2, por `SendMessage` al mismo agente)

P3-1 de la revisión: el argv de `volume create` del volumen junit de nextest vuelve a
construirse dentro de `nextest_gateway.rs`, byte a byte como en la base `755d661c`,
porque `mutation_gateway.rs` no está en `implementation_fingerprint()` y un cambio
del helper movería el argv de nextest sin mover su huella. P3-2 (duplicado `labels`)
anotado sin cambio. Repetir §4 completo.

---

## Encargo al worker Codex (ronda 1)

# Worker Codex (gpt-5.6-sol) — encargo SonarCloud (§2: hueco de instrumentación + tests portables)

Eres un worker delegado por el orquestador (Claude Code). **Tu único árbol de trabajo
es el directorio actual, el worktree `/Users/cburgosro/Projects/rust-mcp-wt/codex`**
(rama `ai/sonar-codex`, creada desde `main` + el encargo). **No leas para editar ni
escribas nunca** en `/Users/cburgosro/Projects/rust-mcp` (árbol del orquestador) ni
en `/Users/cburgosro/Projects/rust-mcp-wt/claude` (otro worker en paralelo).

Lee primero, en este worktree:

1. `docs/prompts/sonarqube-reliability-coverage.md` — el encargo completo. Tu parte es
   la de «Codex (gpt-5.6-sol)» en §5. Sus prohibiciones y criterios aplican literalmente.
   La nota de política de §5 te habilita explícitamente como worker para este encargo.
2. `AGENTS.md` — reglas del repo (seguridad, contratos, calidad, formato de entrega).
3. `docs/validation/M8/delegation/W39b-llvm-profile-passthrough/` — precedente del
   hueco de instrumentación (`env_clear()` borraba `LLVM_PROFILE_FILE`).

## Objetivo medible

SonarCloud (`main`, medido hoy por el orquestador): 80 821 líneas a cubrir, 63 544
cubiertas, 78,5 %. Hacen falta **+1 113 líneas cubiertas** para 80 %. Las líneas a
cubrir incluyen **Rust (79 711, 16 917 sin cubrir) y Python (1 110, 360 sin cubrir)**.
La cobertura se produce en `.github/workflows/sonarcloud.yml` (Linux, sin Docker):
`cargo llvm-cov --workspace --all-targets --locked --lcov` para Rust y una lista
explícita de `python -m coverage run --append … scripts/test-*.py` para Python.

## Paso 1 — hueco de instrumentación (antes de escribir un solo test)

Busca ejecución que ocurre en el runner pero no se contabiliza. Pistas objetivas del
orquestador (verifícalas, no las asumas):

- Archivos Rust al **0,0 % exacto** en SonarCloud: `crates/execution-adapter/src/rust_calibration.rs`
  (212 líneas), `crates/execution-adapter/src/semver_gateway.rs` (160),
  `crates/project-adapter/src/vendor_capture.rs` (147), `crates/execution-adapter/src/coverage_port.rs` (103).
  Un 0,0 % exacto en un archivo con tests o con caminos CLI suele significar que el
  proceso que lo ejecuta no escribe perfil (spawn con `env_clear()` / `env_remove`
  / entorno reconstruido sin `LLVM_PROFILE_FILE`, binario lanzado fuera del perfil,
  `#[cfg]` que lo excluye en Linux, o tests `#[ignore]`/condicionados a Docker).
  Averigua cuál en cada caso.
- Python: `scripts/release-inventory.py` al 0 % (145 líneas). Compara los
  `scripts/test-*.py` existentes con la lista que ejecuta `sonarcloud.yml` bajo
  `coverage run`: puede haber tests portables que ya corren en `gate.py core` pero no
  bajo coverage (p. ej. `test-m4-clients-unit.py`, `test-m4-safety-harnesses.py`,
  `test-m5-clients-unit.py`, `test-m8-rollback-unit.py`, `test-coverage-reports.py`…),
  y subprocess de scripts de producto lanzados sin instrumentar.
- Revisa todos los helpers de spawn de los tests de integración (`crates/*/tests/**`,
  helpers comunes) y del propio código de test en busca de un patrón equivalente a W39b
  que haya sobrevivido.

Todo lo que añadas al workflow debe correr en Linux sin Docker y ser determinista.
`docs/ci.md` debe reflejar cualquier cambio del job de cobertura.
`scripts/check-coverage-reports.py` / `scripts/test-gate-reporting.py` deben seguir pasando.

## Paso 2 — tests portables in-process

Después, tests reales (con aserciones que pueden fallar) sobre, en este orden:
`crates/mcp-server/src/stdio/quality_artifacts.rs` (701 sin cubrir, 4,9 %),
`crates/mcp-server/src/catalog_cli.rs` (324, 4,4 %),
`crates/execution-adapter/src/lsp_session.rs` (442, 4,3 %),
`crates/execution-adapter/src/rust_calibration.rs` (212, 0 %); luego, si queda margen,
otros archivos de tu propiedad (lista abajo) por masa sin cubrir, p. ej.
`crates/execution-adapter/src/lib.rs` (398, 20,2 %), `crates/mcp-server/src/stdio/mutation.rs` (369),
`crates/mcp-server/src/stdio.rs` (353), `crates/mcp-server/src/stdio/catalog/provider.rs` (275, 27,1 %),
`crates/execution-adapter/src/supervisor.rs` (181, 43,3 %), `semver_gateway.rs`, `coverage_port.rs`,
`crates/project-adapter/src/vendor_capture.rs`, y Python sin cubrir.

Prioriza líneas cubribles por hora en el runner Linux sin Docker. Nada de tests que
solo ejecutan sin aseverar. Si un archivo necesita una extracción de función pura para
ser probable y es tuyo, puedes hacerla sin cambio de comportamiento; si es del otro
worker, **no lo toques**: pide la extracción exacta en «Open issues».

## Prohibiciones (literal del encargo)

- No excluyas ningún archivo Rust de producto de la cobertura; no añadas exclusiones
  de producto a `sonar-project.properties` (solo arneses host-only, con justificación
  de una frase en `docs/ci.md`).
- No `#[serde(default)]`, no `// NOSONAR`, no nuevas dependencias (`--offline`), no
  cambios de contrato público ni de `crates/mcp-server/tests/snapshots/`.
- No intentes reprovisionar imágenes Docker (M3/M4/M5 borradas; `gate.py full` no es
  ejecutable). Solo vía portable.

## Propiedad de archivos (disjunta con el worker Claude — obligatoria)

**Prohibido editar** (son del worker Claude): `crates/domain/src/{evidence,result,lib}.rs`,
`crates/catalog-adapter/src/audit.rs`, `crates/execution-adapter/src/project_metadata.rs`,
`crates/execution-adapter/src/{rust_gateway,performance_gateway,project_inspection,security_gateway,mutation_gateway,resolution_gateway,analyzer_gateway,mutation_test_gateway,coverage_gateway,nextest_gateway}.rs`,
y cualquier `crates/*/tests/sonar_claude_*.rs`.

Todo lo demás del repo es tuyo, en particular: `.github/workflows/sonarcloud.yml`,
`scripts/**`, `sonar-project.properties`, `docs/ci.md`, helpers de tests existentes,
`crates/mcp-server/**`, `crates/project-adapter/**`, y en `crates/execution-adapter/src/`
`lib.rs`, `lsp_session.rs`, `rust_calibration.rs`, `semver_gateway.rs`, `coverage_port.rs`,
`supervisor.rs` y demás archivos no listados arriba. Tests nuevos de integración con
prefijo `sonar_codex_` si creas archivos nuevos en `crates/<crate>/tests/`.

## Entorno

- `target/debug` y `target/release` de este worktree son un clon APFS de la caché
  principal; los crates del workspace se recompilarán. No exportes `CARGO_TARGET_DIR`.
- El otro worker compila en paralelo: contención de CPU esperable.
- `cargo-llvm-cov` está instalado localmente: úsalo para medir el delta por archivo
  (`cargo llvm-cov -p <crate> --locked --offline --lcov --output-path <ruta fuera del repo>`).
  macOS difiere del runner Linux: es un proxy, no la cifra final. Para Python,
  `python3 -m coverage` si está disponible; si no, dilo.
- **No hagas commit, push, PR ni nada en SonarCloud/GitHub.** Deja los cambios sin
  commitear en el worktree; el orquestador revisa, commitea e integra.

## Verificación obligatoria antes de entregar (§4, completa, en este worktree)

```sh
cargo build --release --locked --offline
python3 -B scripts/gate.py core            # 30/30 etapas, status passed
python3 -B scripts/contract-freeze.py verify --strict
python3 -B scripts/docs-hygiene.py links-check
python3 -B scripts/docs-hygiene.py verify-inventories
git diff --stat crates/mcp-server/tests/snapshots/   # debe estar vacío
```

Nada que no corrió es pass; un skip o `unavailable` no es pass. Si algo falla por causa
ajena a tu cambio, demuéstralo en vez de declararlo pass.

## Formato de entrega (tu último mensaje, en español)

```text
Task
Result
Instrumentation gaps   (cada hueco: causa raíz con archivo:línea, arreglo, líneas que libera — estimación razonada)
Files changed          (con líneas añadidas/eliminadas por archivo)
Tests executed         (comandos exactos + resultado; gate core: N/30 y ruta del report JSON)
Evidence
Coverage               (por archivo: antes/después local y estimación de líneas nuevas cubiertas en el runner Linux)
Risks
Decisions
Open issues            (peticiones al worker Claude/orquestador; lo que quedó sin hacer y por qué)
```

## Encargo al worker Codex (ronda 2, `codex exec resume`)


La revisión Opus de tu entrega dio **APROBADO CON CAMBIOS MENORES**. Mismo worktree
(`/Users/cburgosro/Projects/rust-mcp-wt/codex`), misma propiedad de archivos, mismas
prohibiciones y mismo formato de entrega. Sin commit.

## A. Correcciones obligatorias

1. `crates/execution-adapter/src/lsp_session.rs` — `classify_stop`: restaura el comentario
   que se perdió al extraer, junto al brazo `(None, _) if stdout_closed && !handshake`:
   «A peer that closed stdout on its own ended the session that way even if it then
   exited cleanly: no handshake took place.»
2. `crates/mcp-server/src/stdio/quality_artifacts.rs` (~l. 1250): `assert_ne!(digest, [0;32])`
   es débil. Compara con el digest exacto esperado del texto conocido.
3. `crates/mcp-server/src/catalog_cli.rs` (~l. 664-701): el test solo comprueba que el código
   de error no es vacío y está en `[A-Z_]`. El mapeo de `error()` es contrato de salida
   del CLI: fija pares exactos variante → código (p. ej. `UnsupportedPlatform` →
   `"UNSUPPORTED_PLATFORM"`, `SyncError::Budget` → `"OUTPUT_LIMIT_EXCEEDED"`,
   `StoreError::Changed` → `"CATALOG_STATE_CHANGED"`; verifica cada valor en el código,
   no copies estos ejemplos a ciegas).
4. `crates/mcp-server/src/stdio/quality_artifacts.rs` (~l. 1325): rama `else { assert!(!trailing.is_empty(), …) }`
   inalcanzable (añade una línea no cubierta). Sustitúyela por indexación directa del último
   elemento o equivalente sin rama muerta.

## B. Segundo tramo de cobertura (medido en Linux, no estimado)

El orquestador midió tu entrega en **Linux real** (contenedor arm64, UID no root, sin red,
mismo comando `cargo llvm-cov --workspace --all-targets --locked` del job; la base reproduce
EXACTAMENTE las cifras de SonarCloud: 79 711 a cubrir / 16 917 sin cubrir):

- Tu entrega: a cubrir 79 711 → 80 551 (+840, tests inline), cubiertas +1 296,
  **sin cubrir −456** (líneas de producto nuevas cubiertas). Todos los tests pasan en Linux.
- Proyección: todavía por debajo del 80 %. La condición real del quality gate es
  `new_coverage` sobre código nuevo; métrica útil por test: **líneas de producto sin cubrir
  que eliminas**, no líneas totales. Los tests inline suben el denominador: sé conciso.

Objetivo de este tramo: **≥ 300 líneas de producto adicionales cubiertas en Linux**
(sin cubrir −300 o más), con tests reales y concisos. Candidatos de tu propiedad, sin cubrir
en Linux ahora (sin cubrir, cubiertas/total):

```
  414  125/539  crates/execution-adapter/src/lsp_session.rs
  398  101/499  crates/execution-adapter/src/lib.rs
  369 1011/1380 crates/mcp-server/src/stdio/mutation.rs
  353  839/1192 crates/mcp-server/src/stdio.rs
  275  102/377  crates/mcp-server/src/stdio/catalog/provider.rs
  197  356/553  crates/mcp-server/src/catalog_cli.rs
  181  138/319  crates/execution-adapter/src/supervisor.rs
  176  125/301  crates/execution-adapter/src/rust_calibration.rs
  160  238/398  crates/mcp-server/src/stdio/tasks.rs
  152   52/204  crates/execution-adapter/src/semver_gateway.rs
  148  296/444  crates/application/src/validation.rs
  121   85/206  crates/mcp-server/src/cargo_vendor_cli.rs
  120  382/502  crates/mcp-server/src/doctor.rs
  119  182/301  crates/execution-adapter/src/capabilities.rs
  109   65/174  crates/execution-adapter/src/nextest_port.rs
  106   81/187  crates/mcp-server/src/stdio/quality_artifacts/security.rs
```

Prioriza el mayor número de líneas de producto cubiertas por línea de test. Recuerda que en
Linux los tests `cfg(target_os = "macos")` no corren: lo que ya cubres en macOS puede estar
sin cubrir en Linux. No toques archivos del worker Claude (lista de la ronda 1).

## C. Verificación

Repite §4 completo (build release, `gate.py core` 30/30, contract-freeze `--strict`,
docs-hygiene links-check + verify-inventories, snapshots sin diff). Entrega con el mismo
formato; en «Coverage» indica tus estimaciones locales, el orquestador vuelve a medir en Linux.
