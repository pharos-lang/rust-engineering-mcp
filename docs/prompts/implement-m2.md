# Prompt autosuficiente — implementar y cerrar M2

Asume el rol de Technical Owner, arquitecto, integrador y revisor final en el repositorio Rust Engineering MCP. Esta sesión autoriza exclusivamente M2; no avances al siguiente milestone. YouTrack está deshabilitado. Sigue la instrucción actual del owner si contradice el alcance histórico de AGENTS; no supongas autorización de publicación.

## Inicio obligatorio

Lee completamente [AGENTS](../../AGENTS.md), [spec](../spec/rust-engineering-mcp-propuesta-v0.3.md), README, CHANGELOG, SECURITY, docs/architecture.md, docs/tools.md, docs/security-model.md, docs/compatibility.md, docs/ci.md, docs/publication.md, [estado](../implementation-status.md), todos los ADR pertinentes y recibos de cierre anteriores. Lee el [maestro](../roadmap/m2-m8.md), [plan M2](../roadmap/m2-safe-mutation.md), [trazabilidad](../roadmap/traceability-m2-m8.md) y [decisiones propuestas](../roadmap/adr-backlog-m2-m8.md). Estas referencias forman parte del encargo; no sustituyas sus criterios con un resumen.

Verifica git status/HEAD/remotes, árbol, manifests/lock, tests y CI. Preserva cambios del usuario. Contrasta live el cierre requerido: M0/M1 Done con trece tools y evidencia source-bound de la release 0.1.0. Registra commit, hashes, comandos y discrepancias; un Done histórico no demuestra los bytes actuales. Si hay defecto real previo del que depende el corte, registra el bloqueo y no construyas sobre él. No rediseñes M1 ni retroedites sus hechos. Usa rama ai/ aislada y commits coherentes si están autorizados en la sesión.

## Alcance y secuencia

Implementa únicamente fmt.apply, fix.apply, dependency.add, dependency.remove y manifest.patch, mediante M2-01..07. Primero decide autoridad, root-bound escritura/exclusión y journal/recovery; no confundas flock/hash preflight con CAS o containment. Las trece tools M1 quedan intactas. Cargo genera candidatos en staging guest acotado; jamás escribe el host. Si no puede probarse la frontera de escritura, falla cerrado y no declara Done.

Ejecuta los cortes del plan por dependencia; cada uno debe recorrer adapter→application→domain/port→adapter con un resultado observable. Antes de código decide DTOs, permisos, threat model, budgets, rollback, fixtures/oráculos y ADRs con deadline en ese corte. Los Dxx son Proposed: no asumas que ya están aceptados. Actualiza primero el ADR/documentación/estado cuando se resuelva una decisión arquitectónica. No crees interfaces vacías. Las estimaciones son relativas, sin inventar fechas o capacidad.

## Invariantes obligatorias

Domain/application independientes de rmcp/Cargo/SQLite/LanceDB; rmcp conserva MCP/JSON-RPC/transporte. Gateway único tipado, sin shell/flags arbitrarios, entorno reconstruido y roots solo del host. I/O propio handle-relative no-follow/reparse-safe; canonicalización no resuelve TOCTOU. Network isolation exige enforcement; rechaza si no existe. Todo código de proyecto incluido build.rs/proc macros/tests/analyzer se trata como hostil. Cancelación/EOF/timeout termina y une el árbol antes de liberar capacidad. Cuotas por bytes/entradas/tiempo y privacidad/audit están definidas antes de efectos.

SQLite es autoritativo, LanceDB derivado. Snapshots con provenance/freshness; no latest sin evidencia live. Runtime no descarga catálogos, advisories, runtimes o modelos. stdout solo MCP, tracing por stderr. No cambiar schemas/semántica de trece tools M1 ni convertir lectura en escritura. Unknown/partial/unavailable/skip no equivalen a pass.

## Implementación, evidencia y gates

Consulta documentación oficial actual cuando la decisión dependa de MCP/rmcp/Cargo/SQLite/LanceDB/RustSec/Rust o plugins; contrasta la versión fijada, no copies ejemplos de otra versión. Mantén un inventario exacto de binarios/assets/versiones/hashes/licencias y registra ausencias; no instales silenciosamente. Cada capability positiva requiere fixture real nativo en el target declarado. Core macOS ARM64 sigue siendo el único artifact calificado hasta evidencia nueva.

Escribe pruebas discriminantes antes de cada vertical: unit/contract/protocol/integration/security, adversariales de frontera y performance cuando aplique. Usa los oráculos, presupuestos y criterios con fuentes del plan M2, más G1–G9. Conserva `docs/validation/M2-matrix.md`, receipts source-bound por corte, hashes de inputs/outputs, plataformas, exit codes, conteos y ausencias; reviews bajo docs/reviews. No marques Done por compilar.

Gate normal: `cargo fmt --check`, `cargo check --workspace --all-targets --locked --offline`, `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`, `cargo test --workspace --all-targets --locked --offline`; además `python3 -B scripts/check-architecture.py`, gate core/full del repo y casos nativos ignorados invocados explícitamente según docs/ci.md. No dupliques gates ya acreditados por un receipt del mismo código. Audit/deny cuando instalados/configurados; ausencia se declara y bloquea un gate obligatorio. Prueba Inspector y cliente stock dirigido por modelo con versiones exactas y flujo positivo/negativo/cancel/Resource aplicable. CI portable no reemplaza native positives.

Sincroniza README, CHANGELOG, SECURITY, arquitectura/tools/security/compatibility/client-configuration, ADRs y tablero cuando cambie comportamiento visible. Revisa diff como Principal Engineer. Para subagentes usa el mínimo con ventaja concreta: Sol Medium para fixtures/investigación, Sol High para fronteras difíciles; archivos disjuntos, sin delegar arquitectura/contratos/cierre. Exige Task, Result, Files changed, Tests executed, Evidence, Risks, Decisions, Open issues.

Revisión independiente read-only: verifica CLI/modelos primero; Sonnet 5 para cortes/contratos, Opus 5 High para seguridad/persistencia/arquitectura y cierre; Gemini 3.8 High mediante agy si disponible para trazabilidad. No sustituyas silenciosamente. Fable 5.1 solo ante incertidumbre excepcional justificada. Registra modelo/versión/effort/archivos+hashes/findings P0–P3/disposición. P0/P1 y P2 de seguridad/datos/contrato/gate bloquean. Revisa cambios materiales corregidos y ejecuta gate final sobre bytes finales. Review no reemplaza pruebas.

## Handoff obligatorio y parada

Entrega resultado real, commit/branch/estado del checkout, cortes Done y pendientes, decisiones, archivos, evidence/commands/hashes, CI/native/client matrix, reviews/findings/disposición, riesgos y rollback. Solo marca M2 Done si todos sus DoD y G1–G9 están demostrados. Si está bloqueado, identifica condición reproducible, dependientes y acción necesaria; no conviertas un skip en éxito. No publiques release/tag/PR sin autorización explícita adicional. Termina con handoff repo-visible y enlaza el prompt siguiente si existe; no ejecutes ese prompt ni avances automáticamente a M3.
