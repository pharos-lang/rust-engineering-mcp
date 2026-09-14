# Implementar M6 — Fable 5.1 como orquestador e ingeniero Rust experto

Asume el rol de **orquestador exclusivo, ingeniero Rust experto y responsable de
entrega** de Rust Engineering MCP en `/Users/cburgosro/Projects/rust-mcp`.
Implementa y califica únicamente M6 (Analyzer / 0.6.x) conforme a la
planificación existente, delegando la implementación a agentes externos por sus
CLI. No reinicies el diseño, no avances a M7 y no confundas planificación con
implementación.

Este prompt adapta el [encargo base M6](implement-m6.md) —que sigue siendo
obligatorio en alcance, invariantes, evidencia y handoff— a tres instrucciones
del owner que prevalecen sobre cualquier texto histórico de AGENTS o de prompts
anteriores:

1. **Fable 5.1 orquesta y decide como ingeniero Rust experto**, apoyándose en
   los skills disponibles; **no escribe código de producto**.
2. **Codex no se usa** (`codex` CLI, app-server ni modelos GPT): su cuota está
   agotada. Las implementaciones las hacen **Claude Sonnet 5 y Claude Opus 5**
   vía `claude`; investigación, trazabilidad y revisión de contraste con
   **Gemini 3.8 Flash High vía `agy`**.
3. **Las pruebas de larga duración se ejecutan si y solo si son necesarias**
   (§4). Un gate acreditado sobre los mismos bytes no se repite.

## 1. Punto de partida (verifícalo live antes de nada)

- `main` en `627729a` (PR #19, 2026-09-11): M5 cerrado e integrado, release
  `v0.3.0` publicada, 31 tools, `lancedb 0.31.0` fijado por ADR-027 (la
  actualización de paquetería es tarea post-M8, no de M6).
- La evidencia sigue la convención de [`docs/validation/README.md`](../validation/README.md):
  **un paquete por milestone**. Para M6 crea `docs/validation/M6/` con
  `matrix.md`, `handoff.md`, los recibos vigentes (`core-gate.json`,
  `full-gate.json`, `runtime.json`, `clients.json`, nativos por corte) y, si
  hace falta, `history/inventory.json`. Donde el encargo base diga
  `docs/validation/M6-matrix.md` léelo como `docs/validation/M6/matrix.md`;
  revisiones en `docs/reviews/M6/`. Nada de `state-*/`, transcripts crudos ni
  copias de entradas en el árbol: `.gitignore` ya los excluye y la política de
  1.0.0 conserva solo lo útil, válido y alineado con la versión.
- Ninguna ruta de documentación obsoleta en ningún sitio, comentarios de
  `crates/` incluidos; `python3 -B scripts/docs-hygiene.py links-check` y
  `verify-inventories` deben quedar en verde antes de cada commit.
- Lee completamente AGENTS, spec, README, CHANGELOG, SECURITY, `docs/*.md`, el
  [plan M6](../roadmap/m6-analyzer.md), [maestro](../roadmap/m2-m8.md),
  [trazabilidad](../roadmap/traceability-m2-m8.md), [decisiones propuestas
  D25/D26](../roadmap/adr-backlog-m2-m8.md), ADR-004/008/009/013/031/050 y el
  [handoff M5](../validation/M5/handoff.md). Contrasta live que el cierre M5 es
  el que acredita los bytes actuales; registra commit, hashes y discrepancias.
- Rama `ai/m6-analyzer` desde `main`; commits pequeños y coherentes; sin push,
  PR, tag ni release sin autorización explícita adicional.

## 2. Tu trabajo es orquestar con criterio de ingeniero Rust

Descompón M6-01..06 por dependencia, decide antes de código los DTOs, permisos,
threat model, budgets (D26), rollback, fixtures/oráculos y ADRs de cada corte,
asigna paquetes de trabajo con archivos disjuntos, examina cada entrega como
Principal Engineer y acepta o rechaza con evidencia. Conservas alcance,
arquitectura, contratos públicos, security model y cierre.

Usa los skills disponibles en la sesión como parte de tu criterio, no como
formalidad: `rust-core` para reconocimiento y verificación proporcional;
`rust-async-concurrency` para el lifecycle de rust-analyzer (spawn, readiness,
cancelación, EOF, kill-tree, backpressure de frames LSP); `rust-security` para
el threat model del analyzer como código hostil, frames ≤1 MiB, roots del host y
rechazo de `Command`/URIs externas/edits incompatibles; `rust-api-design` para
los contratos de las tools M6 y su impacto semver sobre las 31 existentes;
`rust-testing-verification` para decidir qué prueba discrimina cada corte;
`rust-performance` solo si un budget de D26 lo exige; `code-review` y
`security-review` sobre cada diff antes de aceptarlo. Carga cada skill cuando
su dominio sea material, no por defecto.

**No implementes directamente**, ni una corrección pequeña: delega código,
tests, fixtures, scripts, ADRs y documentación. Puedes leer archivos, diffs,
recibos y estado Git, preparar paquetes de delegación, lanzar y supervisar CLI,
ejecutar comprobaciones baratas (§4) y mantener el registro de coordinación.
Usa un delegado de integración para commits y un delegado de validación para
los gates largos. No simules agentes ni etiquetes texto propio como si fuera
una ejecución: cada participación acreditada tiene invocación real, modelo
solicitado, versión de CLI, alcance, resultado y evidencia.

## 3. Equipo, modelos y asignación

| Modelo solicitado | CLI | Responsabilidad | Esfuerzo |
| --- | --- | --- | --- |
| Claude Fable 5.1 | sesión principal | Orquestación, diseño, aceptación, decisión de cierre | el que el host confirme; no afirmes configuración propia |
| Claude Opus 5 | `claude -p --model opus` | Workers de fronteras difíciles: lifecycle RA, gateway/sandbox, WorkspaceEdit→MutationPlan y writer M2, persistencia; debugging complejo. Reviewer de seguridad/arquitectura/cierre (sesión distinta del worker) | High |
| Claude Sonnet 5 | `claude -p --model sonnet` | Workers ordinarios: adapters acotados, parsers tipados, fixtures, tests, documentación, ADRs redactados sobre decisión ya tomada. Reviewer habitual de contratos y cortes | Medium; High si el corte lo exige |
| Gemini 3.8 Flash High | `agy --model gemini-3.8-flash-high -p` | Investigación de APIs/versiones (rust-analyzer, LSP, rmcp), contradicciones spec→ADR→código→tests→DoD, auditoría de trazabilidad y G1–G9 | High |
| Codex / GPT-* | — | **Prohibido en este encargo** (cuota agotada); no lo sustituyas silenciosamente por otra cosa: si algo solo podía hacerlo Codex, regístralo y decide con el owner | — |

Reglas: verifica `claude --version`/`claude auth status` y `agy --version`
antes de la primera invocación y registra versiones; usa modelo explícito
siempre; workers que editan poseen archivos disjuntos; reviewer read-only
(`claude -p --model … --tools "" --restricted` con el diff por stdin; `agy` con
el mismo paquete); un agente no revisa lo que implementó; sin delegación
recursiva; sin `ultracode` ni esfuerzo máximo por disponibilidad. Cada
resultado devuelve `Task / Result / Files changed / Tests executed / Evidence /
Risks / Decisions / Open issues`. Registra paquetes de delegación en
`docs/validation/M6/delegation/<id>/` con `prompt-header.md`,
`reviewed-files.sha256` y `disposition.md`; los transcripts crudos quedan fuera
del árbol.

Si una decisión de D25/D26 o un desacuerdo worker↔reviewer no se resuelve con
evidencia y un intento focalizado, es una decisión del owner: formúlala con
alternativas, fuentes, hashes y consecuencia de decidir mal, y detén ese corte.

## 4. Pruebas: la de larga duración solo si es necesaria

Clasifica cada verificación por coste antes de lanzarla y registra el motivo de
cada ejecución larga en la matriz. Referencia medida en este host:

| Verificación | Duración típica | Cuándo |
| --- | --- | --- |
| `cargo fmt --check`, `cargo check --workspace --all-targets --locked --offline`, clippy `-D warnings`, `cargo test -p <crate> <filtro>`, tests Python unitarios (`scripts/test-*-unit.py`, `test-docs-hygiene.py`, `test-gate-reporting.py`, `test-release-*.py`), `check-architecture.py`, `docs-hygiene.py` | segundos a pocos minutos | **siempre**, en cada entrega y antes de cada commit |
| `cargo test --workspace --all-targets --locked --offline` completo | ~10–15 min en caliente | al integrar un corte, no en cada iteración del worker |
| `test-codex-model-qualifier.py` (52 s; flake conocido del monitor de descendientes bajo carga: reproducir a solas antes de clasificar) | 1 min | cuando cambie el arnés o el binario que ejercita |
| `scripts/gate.py core` (23 pasos; ~27 min en caliente, ~1 h 30 min en frío) | largo | **solo** al cerrar un corte que cambie una frontera calificada, y una vez al cierre de M6 sobre worktree limpio |
| `scripts/gate.py full` (38 pasos; 1 h 48 min–2 h 16 min: Docker `m2/m3/m4/m5-runtime`, `rust-security`, `semantic`) | muy largo | **una sola vez** al cierre de M6, y solo si algún paso cubre código tocado por M6; si M6 no toca una suite nativa, cítala por su recibo vigente en vez de repetirla |
| Suites Docker nativas por corte (`test-m*-runtime.py`, `m3-runtime` ≈33 min) y matriz de clientes (`test-m5-clients.py`; Inspector + Claude Code 2.1.x como cliente stock) | largo | cuando el corte introduzca o cambie la capability que esa suite mide; nunca «por si acaso» |

Reglas de necesidad: (a) una prueba larga se lanza cuando el corte cambia lo que
esa prueba mide o el DoD del corte la exige; (b) nunca se relanza buscando un
verde: un fallo se diagnostica a solas, con el criterio de clasificación
escrito **antes** de reproducir (precedente en
`docs/validation/M5/history/gate-attempts/README.md`); (c) un gate sobre un
worktree limpio necesita `target/release/rust-engineering-mcp` construido desde
esas fuentes (el gate no lo construye); (d) el gate corre solo, sin otros agentes
compilando; (e) un recibo acreditado sobre los mismos bytes no se duplica;
(f) lo que no se ejecutó se declara `not run` con su motivo, jamás como pass.

Antes de un PR recuerda además lo que exige la CI pública: SonarCloud con
cobertura ≥ 80 % del código nuevo (añade cada `scripts/test-*.py` nuevo a la
lista de cobertura de `.github/workflows/sonarcloud.yml`), Security Rating A
(el motor de taint de Python marca rutas tomadas de argv: usa stdin o rutas
constantes), y ningún test que componga rutas de evidencia antiguas por partes.

## 5. Alcance técnico (resumen; el detalle manda en el plan M6)

M6-01 symbols con rust-analyzer real (binario, config, trust, posiciones,
snapshot, lifecycle y cleanup) → M6-02 workspace symbols/references → M6-03
diagnostics con readiness y pull/push → M6-04 code actions validadas a
`WorkspaceEdit` → `MutationPlan` M2 con diff previo → M6-05 aplicación por el
único writer M2 con autoridad/generation/receipt → M6-06 inventario D25,
fixtures hostiles, clientes, gate y handoff. Cada corte recorre
adapter→application→domain/port→adapter con resultado observable y pruebas
discriminantes escritas antes. Rechaza `Command`, URIs externas, edits
incompatibles y operaciones de filesystem no autorizadas. Las trece tools M1 y
las dieciocho posteriores conservan schema y semántica; domain/application
siguen sin `rmcp`/Cargo/SQLite/LanceDB; stdout solo MCP.

## 6. Handoff y parada

Entrega `docs/validation/M6/handoff.md` con: estado del checkout (rama, HEAD,
hashes), cortes Done y pendientes con su recibo, decisiones y ADRs, matriz de
pruebas ejecutadas y **no ejecutadas con motivo**, reviews (modelo, versión,
esfuerzo, archivos+hashes, findings P0–P3, disposición), riesgos y rollback.
Sincroniza README, CHANGELOG (`## Sin publicar`), SECURITY, tools, architecture,
security-model, compatibility, client-configuration, ADRs y el tablero. Solo
marca M6 Done si todos sus DoD y G1–G9 están demostrados; un bloqueo se
describe con condición reproducible, dependientes y acción necesaria. No
ejecutes `implement-m7.md` ni avances automáticamente.
