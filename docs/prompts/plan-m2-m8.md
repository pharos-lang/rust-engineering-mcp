# Prompt para planificar M2–M8 sin implementar

Copia el bloque siguiente en una sesión nueva de Codex abierta en
`<LOCAL_HOME>/Projects/rust-mcp`.

```text
Asume el rol de Technical Owner, arquitecto, integrador y revisor final de Rust
Engineering MCP en <LOCAL_HOME>/Projects/rust-mcp. Tu única misión en esta
sesión es producir una planificación completa, ejecutable y repo-visible de M2 a
M8, incluida la estabilización 0.8–0.9 y la readiness para 1.0.0. No implementes
M2 ni posteriores, no cambies código de producción, manifests, dependencias,
schemas públicos, workflows de release ni decisiones ADR aceptadas.

BASELINE CERRADA QUE DEBES VERIFICAR LIVE, NO ASUMIR

- M0 y M1/0.1.0 están Done según docs/implementation-status.md y
  docs/validation/M0-M1-closure-matrix.md.
- El contrato público 0.1.0 contiene exactamente trece tools; no existe
  rust.dependencies.inspect en el contrato.
- El último commit privado de implementación/export previo al handoff es
  68cd00131ac71df23460723533df1c10314b0675. El commit que contiene este prompt y
  la documentación final puede ser posterior: verifica HEAD y registra ambos.
- El commit público release es
  452acdbf3a634d2cc0b9d153db09718237625b9d, etiquetado como v0.1.0.
- Release estable: https://github.com/pharos-lang/rust-engineering-mcp/releases/tag/v0.1.0.
- Workflow tag-bound final: run 33948798048. CI final de main: run 33948778666.
  SonarCloud final de main: run 33948778651.
- El artifact publicado es únicamente core para aarch64-apple-darwin. Linux y
  Windows son CI portable/fail-closed, no targets binarios ni capabilities nativas
  positivas.
- No se distribuyen catálogo oficial, clave Ed25519 de producción, modelo E5, ORT,
  LanceDB, Docker, toolchain ni fixtures. crates.io sigue deshabilitado.
- El archive público tiene SHA-256
  b499a3e32d8186d2f513fd5269e4bf50929b29b24b6448c8005af32913586fb4;
  el binario, 8f6f8c754ae3bde6cc2089ffb5c6360e5c9ebb61af7f022477ee10a30ed336ef.
- Las tres attestations verifican el signer workflow, ref v0.1.0, source commit y
  run anteriores. El recibo autoritativo es
  docs/validation/m1-17-public-release.json.
- El cierre local pasó 23/23 etapas; Inspector 2.5.0 y Codex 0.153.0 con un flujo
  model-directed verificaron el servidor real; la revisión final Opus 5 aceptó el
  candidato sin P0/P1.
- La investigación M1-16 fue acotada y saturada: no demostró equivalencia, valor
  causal ni calidad general. No conviertas esos resultados en claims de producto.
- YouTrack está deshabilitado para este repositorio. No lo consultes ni crees
  issues allí salvo una autorización futura explícita del owner y del repo.

Si el estado live difiere, preserva la evidencia histórica y registra la diferencia
antes de planificar. No cambies retroactivamente los hechos de la release 0.1.0.

INICIO OBLIGATORIO

1. Lee completamente AGENTS.md y
   docs/spec/rust-engineering-mcp-propuesta-v0.3.md.
2. Lee README.md, CHANGELOG.md, SECURITY.md, docs/architecture.md, docs/tools.md,
   docs/security-model.md, docs/compatibility.md, docs/ci.md,
   docs/publication.md y docs/implementation-status.md.
3. Lee docs/validation/M0-M1-closure-matrix.md,
   docs/validation/M1-17-matrix.md,
   docs/validation/m1-17-public-release.json,
   docs/reviews/M1-closure-final-claude-opus-5.md y todos los ADR, con atención
   especial a ADR-003/004/005/007/008/009/010/023/029/031/038/041/047/048.
4. Inspecciona git status, HEAD, árbol, manifests, Cargo.lock, tests y workflows.
   Consulta el tag/release/CI/branch protection públicos live. No uses un resultado
   histórico como si fuera el estado actual.
5. Extrae de la especificación los entregables normativos de M2, M3, M4, M5, M6,
   M7, M8 y 1.0.0. Construye una tabla de contradicciones o ambigüedades frente a
   la implementación y las fronteras reales de M1.

REGLA ABSOLUTA DE ESTA SESIÓN

Solo planificación. Puedes crear o editar documentos debajo de docs/roadmap/ y
docs/prompts/, y enlazarlos desde docs/implementation-status.md como backlog
planificado. No escribas Rust/Python/SQL, no añadas tests ejecutables, no cambies
Cargo.toml/Cargo.lock, no aceptes ADR, no expongas tools nuevas y no inicies M2.
Los comandos permitidos son de inspección, validación documental y comprobación
read-only del estado live. Si descubres un defecto real de M1, regístralo como
precondición/bloqueo y detén cualquier plan que dependa de él; no lo repares en esta
sesión.

SALIDAS REPO-VISIBLES OBLIGATORIAS

Produce, como mínimo:

1. docs/roadmap/m2-m8.md: roadmap maestro con orden, dependencias, releases,
   critical path, gates transversales y criterios de entrada/salida.
2. docs/roadmap/m2-safe-mutation.md
3. docs/roadmap/m3-quality.md
4. docs/roadmap/m4-security.md
5. docs/roadmap/m5-performance.md
6. docs/roadmap/m6-analyzer.md
7. docs/roadmap/m7-remote.md
8. docs/roadmap/m8-stabilization.md
9. docs/roadmap/traceability-m2-m8.md: cada requisito de la spec y cada deuda o
   limitación M1 mapeados a un corte, evidencia y gate, sin requisitos huérfanos.
10. docs/roadmap/adr-backlog-m2-m8.md: decisiones nuevas requeridas, fecha límite,
    alternativas a estudiar y evidencia necesaria; son propuestas, no ADR Accepted.
11. Una secuencia de prompts autosuficientes bajo docs/prompts/ para implementar y
    cerrar cada milestone de forma separada. El primero debe cubrir solo M2; los
    posteriores deben exigir verificar el cierre real del milestone anterior. Cada
    prompt debe terminar con un handoff y prohibir avanzar al siguiente milestone.
12. Actualiza únicamente la sección de backlog futuro de
    docs/implementation-status.md para enlazar el roadmap, dejando M0/M1 Done.

CONTENIDO EXIGIDO POR MILESTONE

Para cada M2–M8 define explícitamente:

- objetivo y resultado observable;
- entregables normativos y contrato público propuesto;
- fuera de alcance y antiobjetivos;
- cortes verticales pequeños, cada uno ejecutable end-to-end y con dependencias;
- cambios esperados por dominio/application/ports/adapters/MCP/CLI, sin diseñar
  interfaces vacías ni escribir la implementación;
- invariantes de seguridad, abuse cases y actualización del threat model;
- permission model, filesystem, red, entorno, ejecución de código, concurrencia,
  cancelación, cuotas, auditoría y rollback aplicables;
- migración, backward compatibility, versionado de schema/protocolo y SemVer;
- pruebas unitarias, contract, protocol, integration, security, adversariales,
  nativas, performance y clientes de terceros requeridas;
- fixtures y oráculos discriminantes; un skip nunca cuenta como pass;
- gates focalizados, gate completo, CI/targets y evidencia repo-visible a conservar;
- observabilidad, límites, budgets, SLO/SLI cuando proceda;
- distribución, inventario, licencias/notices/SBOM/provenance y soporte por target;
- riesgos, mitigaciones, criterio de rollback y decisiones ADR previas;
- Definition of Ready, Definition of Done y criterios de aceptación verificables
  por un tercero, con la fuente citada dentro del propio criterio;
- estimación relativa y critical path, sin inventar fechas ni capacidad del equipo;
- paquete de revisión independiente requerido y severidades que bloquean merge o
  release.

FRONTERAS ESPECÍFICAS A RESOLVER EN EL PLAN

M2 — Safe Mutation / 0.2.x

- Planifica fmt.apply, fix.apply, dependency.add, dependency.remove y
  manifest.patch.
- No asumas que una tool de lectura puede convertirse en escritura sin romper su
  contrato. Define nuevos DTO/schemas, permisos explícitos y deny-by-default.
- Toda mutación debe usar precondition hashes/generation, locks, staging,
  diff previo, scope de archivos autorizado, symlink/reparse-safe I/O,
  atomicidad o rollback verificable y receipt de cambios.
- Separa edición de texto, edición TOML semántica y ejecución de Cargo. No permitas
  flags arbitrarios, shell ni cambios de red implícitos. Define conflictos,
  cancelación, crash recovery, idempotencia y dirty-worktree policy.

M3 — Quality / 0.3.x

- Planifica nextest, coverage, semver-check y mutation testing sobre una task
  execution abstraction común.
- Define artifacts avanzados/Resources adicionales, retención, cuotas y privacidad.
- Mantén el Execution Gateway único y trata build.rs/proc macros/tests como código
  del proyecto. Define formatos de cobertura, merge multi-package, baselines,
  toolchain/plugin provenance y resultados parciales.

M4 — Security / 0.4.x

- Planifica cargo-deny, unsafe scan, Miri, supply-chain inspection y hardening del
  sandbox.
- Evita duplicar rust.dependencies.audit. Define qué tool responde qué pregunta,
  fuentes/advisories, freshness, false positives, suppressions auditables y fail
  closed.
- Incluye threat-model review, secret scanning de evidencias/artifacts, provenance
  policies, dependencia comprometida, catalog poisoning y escapes de containment.

M5 — Performance / 0.5.x

- Planifica benchmark, benchmark compare, flamegraph y cargo-bloat.
- Define ruido estadístico, warmup, muestras, baseline identity, hardware/OS
  provenance, regresión mínima detectable, formatos versionados y budgets de
  artifacts. No conviertas una medición en causalidad ni generalización.
- El profiling puede requerir permisos privilegiados: planifica detección y
  denegación explícita por host.

M6 — Analyzer / 0.6.x

- Planifica adapter de rust-analyzer, symbols, references, diagnostics y code
  actions.
- Define lifecycle del servidor, sincronización de documentos, snapshots,
  invalidación, cancellation, memory limits, workspace trust y versión exacta.
- Las code actions que muten deben reutilizar las garantías M2; no crear una segunda
  frontera de escritura.

M7 — Remote / 0.7.x

- Es estrictamente condicional a un caso de uso real documentado y aprobado. El
  plan debe incluir una puerta go/no-go antes de diseñar o implementar Streamable
  HTTP.
- Define la evidencia mínima del caso de uso, alternativas stdio/local, datos y
  actores, tenancy, authorization, identity, TLS, rate limits, audit, revocation,
  remote sandbox, multi-project concurrency, privacy y operación.
- Si la puerta no se satisface, el resultado correcto es Deferred; no inventes un
  usuario ni fuerces 0.7.x.

M8 — Stabilization / 0.8–0.9

- Planifica API cleanup, compatibility tests, docs, performance, security review y
  migration tooling como una ruta de estabilización, no como nuevas features sin
  límite.
- Define deprecation windows, contract freeze, wire/client matrix, upgrade and
  rollback tests, release candidates, bug bars, performance budgets, supply-chain
  hardening y auditoría externa.
- Termina con un checklist de readiness 1.0.0: contratos estables, security model,
  política SemVer, alcance cross-platform real, CLI estable, guías de integración,
  matriz de protocolo, upgrades y signing/provenance verificables.

ARQUITECTURA Y SEGURIDAD QUE EL PLAN DEBE PRESERVAR

- Arquitectura hexagonal: domain/application no dependen de rmcp, JSON-RPC, stdio,
  Cargo CLI, SQLite ni LanceDB.
- rmcp conserva protocolo/transporte; no diseñes un stack JSON-RPC paralelo.
- Un único Execution Gateway tipado, sin sh -c/bash -c/flags arbitrarios y con env
  reconstruido.
- Roots solo del host confiable; I/O propio handle-relative no-follow/reparse-safe;
  canonicalización previa no resuelve TOCTOU.
- Network isolation solo se anuncia con enforcement probado; de otro modo se
  rechaza. Timeout/cancelación terminan el árbol completo.
- SQLite sigue siendo autoritativo; LanceDB es derivado y no decide facts.
- Provenance y freshness acompañan snapshots. El runtime MCP no sincroniza ni
  descarga catálogos, advisories, runtimes o modelos.
- stdout queda reservado para MCP stdio; logs por stderr/tracing.
- Preserva exactamente las trece tools 0.1.0. Cada adición futura requiere análisis
  de compatibilidad, snapshots, docs y ADR si cambia el contrato público.

USO DEL POOL Y REVISIONES

Codex/GPT es Technical Owner y conserva arquitectura, contratos, seguridad,
integración y conclusión. Usa el mínimo de subagentes con ventaja real y fronteras
disjuntas. Una distribución inicial útil para esta sesión de planificación es:

- un worker High para M2/M6: mutación, filesystem, concurrencia y reuse de garantías;
- un worker High para M3/M4/M5: task execution, seguridad, métricas y supply chain;
- un worker Medium para M7/M8: go/no-go remoto, compatibilidad, distribución y 1.0.

Los workers son read-only respecto al código y solo proponen contenido. Cada uno
entrega Task, Result, Files changed, Tests executed, Evidence, Risks, Decisions y
Open issues. El principal reconcilia contradicciones y revisa todo el diff.

Usa Gemini 3.8 High mediante agy, si sigue disponible, para una auditoría amplia de
trazabilidad spec→roadmap y contradicciones. Usa Claude Sonnet 5 read-only para
revisar paquetes de milestone y Claude Opus 5 High read-only para revisar el threat
model, M7 condicional y la coherencia final M2–M8/1.0. Verifica primero las versiones
y modelos disponibles; no sustituyas silenciosamente. Registra modelo, versión,
effort, archivos/hashes revisados, findings por severidad y disposición. Ninguna
revisión de modelo reemplaza evidencia primaria ni decide arquitectura.

VALIDACIÓN DOCUMENTAL Y CIERRE DE ESTA SESIÓN

- Verifica links, IDs, requisitos y ausencia de términos vagos como “soportado” sin
  target/gate. Busca requisitos de spec no mapeados y dependencias circulares.
- Confirma con git diff que solo cambió planificación/documentación autorizada y
  que no hay implementación de M2.
- Ejecuta los validadores documentales existentes que no muten producto. Revisa el
  diff como Principal Engineer y somételo al review independiente indicado.
- No marques ningún milestone M2–M8 como In progress o Done. Deben quedar Planned,
  Conditional o Deferred con criterios de entrada explícitos.
- No publiques release, tag ni PR salvo autorización explícita adicional de la
  sesión nueva. Deja el checkout limpio solo si esa sesión estaba autorizada a
  commit; de otro modo entrega el diff listo y no inventes integración.

Tu respuesta final debe incluir: baseline verificada; archivos de planificación
creados; resumen y orden crítico M2–M8; contradicciones resueltas o abiertas;
ADRs propuestos; gates y DoD por milestone; decisión go/no-go de M7; ruta 0.8–0.9
y checklist 1.0; revisiones y sus findings; validación documental; confirmación
explícita de que M2 no fue implementado; y la secuencia completa de prompts/handoffs
creada para ejecutar cada milestone en sesiones futuras.
```
