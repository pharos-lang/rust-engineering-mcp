# AGENTS.md — Rust Engineering MCP

Estas instrucciones aplican a todo el repositorio. El código y las pruebas son la
evidencia del estado real. La documentación canónica del producto implementado
está en `docs/` (índice: [docs/README.md](docs/README.md)); las decisiones de
arquitectura vigentes viven en los capítulos de `docs/architecture/` y su mapa de
IDs históricos en [docs/architecture/decisions.md](docs/architecture/decisions.md).
La especificación original (propuesta v0.3), los ADR individuales, roadmaps,
recibos y revisiones de M0–M8 están en el historial de Git (último commit que los
contiene: `51fa602e`); sus cláusulas y decisiones vigentes se consolidaron en los
documentos canónicos.

## Ownership y precedencia

El agente principal actúa como Technical Owner, arquitecto, integrador y revisor
final. Ante conflictos, decide en este orden:

1. seguridad;
2. correctness;
3. requisitos y garantías documentados en `docs/architecture/` y `docs/reference/`;
4. compatibilidad MCP;
5. contratos públicos existentes;
6. testabilidad;
7. mantenibilidad;
8. simplicidad operacional;
9. rendimiento;
10. ergonomía para agentes;
11. extensibilidad futura.

No se cambia silenciosamente una decisión arquitectónica. Si la evidencia obliga a
divergir de una decisión vigente, se registra primero la nueva decisión en su
capítulo (ver "Decisiones y documentación viva") y se actualiza la documentación
afectada en el mismo cambio.

## Inicio obligatorio de cada sesión

Antes de modificar código:

1. leer este archivo y [docs/README.md](docs/README.md);
2. leer los capítulos de `docs/architecture/` y las referencias de `docs/reference/`
   que afecten al cambio, incluidas sus limitaciones documentadas;
3. si el encargo continúa un plan pendiente, leerlo completo en `.planning/`;
4. inspeccionar `git status`, el árbol real, los manifests, tests y CI;
5. continuar desde el punto de reanudación del plan vigente, sin rediseñar desde cero;
6. comprobar documentación oficial actual cuando una decisión dependa de MCP,
   `rmcp`, Cargo, SQLite, LanceDB, RustSec o una API/version cambiante.

La documentación oficial o el repositorio oficial de cada tecnología es la fuente
externa preferida. No se incorporan ejemplos de Internet sin contrastarlos con la
versión fijada en `Cargo.lock`.

## Alcance vigente

M0–M6 están integrados en `main` y la estabilización M8 (freeze de contratos,
migraciones/rollback, budgets, threat model, matriz de clientes) también; M7
(transporte remoto) quedó diferido y la preparación 1.0 (M8-09) sigue pendiente,
con sus compromisos en [.planning/deferred-commitments.md](.planning/deferred-commitments.md).
El checkout está en `0.9.0-rc.1` sin tag ni release; la release publicada es `0.3.0`.
El contrato público está congelado en el freeze `0.8.0`
([tests/baselines/contract-freeze-0.8.0.json](tests/baselines/contract-freeze-0.8.0.json),
verificado por `scripts/contract-freeze.py verify`): 36 tools registradas, con su
clase de estabilidad descrita en [docs/reference/tools.md](docs/reference/tools.md).
Todo cambio de schema, descripción, nombre o semántica sigue la política de
evolución de contratos de
[docs/architecture/mcp-and-contracts.md#política-de-evolución-de-contratos-adr-086](docs/architecture/mcp-and-contracts.md#política-de-evolución-de-contratos-adr-086)
(aditivo medido contra consumidores exhaustivos, no automático; cambios
incompatibles en 0.x solo con release minor, changelog, snapshots y notas de
migración; freeze 0.8.0 con deprecaciones anunciadas y funcionales hasta 1.0,
retiro solo de lo anunciado desde 0.8.0; desde 1.0, deprecación en minor y
retiro solo en 2.0; retirar una revisión MCP exige decisión registrada; las
tools `preview` pueden cambiar; excepción fail-closed de seguridad que nunca
reinterpreta en silencio un resultado existente) y exige regenerar y revisar
el manifest del freeze en el mismo cambio.

Los encargos pendientes autorizados viven versionados en `.planning/` y solo se
ejecutan cuando el owner lo solicita expresamente; hoy:
[.planning/implement-1.0-runtime-portability-astra.md](.planning/implement-1.0-runtime-portability-astra.md)
y el plan diferido [.planning/implement-m7.md](.planning/implement-m7.md).
No anunciar como disponible una feature, plataforma o runtime que solo figure en
un plan.

## Reglas arquitectónicas

- Usar arquitectura hexagonal: `domain` y `application` no dependen de `rmcp`,
  JSON-RPC, stdio, Cargo CLI, SQLite ni LanceDB.
- Crear ports solo en fronteras reales que protejan el dominio o habiliten pruebas.
- Implementar por cortes verticales ejecutables, no por capas de interfaces vacías.
- Mantener tipos Rust como fuente de verdad de serialización y JSON Schema cuando
  sea viable. Evitar `serde_json::Value` como modelo interno general.
- `rmcp` gestiona protocolo, negociación, JSON-RPC y transporte. No implementar un
  stack JSON-RPC paralelo.
- SQLite es la fuente autoritativa del catálogo; FTS5 es la búsqueda léxica.
  LanceDB es derivado, versionado, reconstruible y nunca decide hechos.
- Toda información de snapshot incluye provenance y freshness; usar
  `latest_known`, nunca `latest`, salvo evidencia live explícita.
- El runtime MCP no sincroniza ni descarga catálogos, advisories o modelos. Esas
  operaciones pertenecen a la CLI explícita.
- stdout queda reservado al protocolo en modo stdio; logs solo por stderr mediante
  `tracing`.

## Seguridad no negociable

- Deny-by-default para comandos, filesystem, entorno, red y ejecución de código del
  proyecto.
- Toda ejecución externa atraviesa un único Execution Gateway. No usar `sh -c`,
  `bash -c`, `cmd /c`, PowerShell con entrada del usuario ni flags arbitrarios.
- Construir programas y argumentos desde enums/tipos validados; limpiar el entorno
  y agregar solo variables permitidas.
- Las roots provienen del host confiable. Para I/O propio, operar relativo a handles
  de directorio con semántica no-follow/reparse-safe; una canonicalización previa no
  evita TOCTOU. Si el OS/adaptor no puede ofrecerla, la operación falla cerrada. Para
  procesos hijos, el sandbox OS es la frontera de containment.
- `cargo check`, Clippy y tests pueden ejecutar `build.rs` y proc macros. No
  describirlos como seguros por ser de validación.
- No afirmar que la red está bloqueada si solo se evitó pasar flags de red. Un modo
  que requiera `network_isolated` exige enforcement del sandbox; sin él, la operación
  se rechaza. El host puede escoger explícitamente otra policy que permita red cuando
  la operación lo admita, pero nunca degradar silenciosamente una petición deny.
- Timeout y cancelación terminan el árbol de procesos. Limitar stdout, stderr,
  diagnósticos, CPU/memoria y disco cuando la plataforma lo permita.
- Nunca heredar secretos o todo el entorno del host.
- Los fallos de compilación/pruebas son resultados válidos de la tool, no errores de
  infraestructura MCP.

## Flujo de implementación

Para cada corte vertical:

1. definir tipos de dominio y contrato esperado;
2. escribir pruebas discriminantes (unitarias, integración, contrato o seguridad);
3. implementar el camino completo adapter → aplicación → dominio/ports → adapter;
4. ejecutar validaciones focalizadas;
5. revisar el diff como Principal Engineer;
6. actualizar documentación canónica y, si aplica, registrar la decisión;
7. ejecutar el gate proporcional antes de marcarlo Done.

No declarar una feature terminada por compilar. Debe existir evidencia reproducible
del comportamiento y de los casos adversos relevantes.

## Calidad y pruebas

El gate local normal es:

```text
cargo fmt --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
```

Ejecutar además contract, protocol, integration y security tests cuando el corte los
afecte. Usar fixtures reales para Cargo, SQLite, FTS5, LanceDB y procesos; los mocks
no sustituyen las pruebas de frontera. `cargo audit` y `cargo deny` se ejecutan
cuando estén instalados y configurados; una ausencia se reporta, no dispara una
instalación silenciosa.

Evitar `unwrap`, `expect` y `panic!` en rutas normales. Toda excepción en código de
producción requiere una invariante demostrable y comentario local.

## Decisiones y documentación viva

Una decisión registrada es obligatoria cuando cambie contratos públicos,
arquitectura, seguridad, persistencia, compatibilidad MCP, distribución, soporte
cross-platform o dependencias estratégicas. No se crean archivos ADR sueltos: la
decisión se añade al capítulo de `docs/architecture/` (u `operations/`) que posee
el área, como subsección con ID `D-AAAA-MM-DD-<slug>`, fecha y los campos
`Context`, `Decision`, `Alternatives considered`, `Consequences` y `Status`; y se
añade una fila a [docs/architecture/decisions.md](docs/architecture/decisions.md).
Si sustituye una decisión anterior, se indica cuál y se actualiza el texto del
capítulo para que describa solo el comportamiento vigente.

Política documental:

- `docs/` describe únicamente el producto implementado: instalación, uso,
  operación, arquitectura, contratos, garantías y limitaciones actuales. No
  contiene planes, diarios de trabajo, recibos de calificación ni transcripts.
- Estructura: `docs/guides/` (usuario), `docs/reference/` (tools, CLI,
  compatibilidad, límites, formatos), `docs/architecture/` (diseño y decisiones),
  `docs/operations/` (runtime, catálogo, recuperación, verificación de release),
  `docs/development/` (pruebas y CI). Un documento por tema, no por milestone,
  tool o decisión.
- Una limitación conocida (garantía requerida sin enforcement) se documenta como
  limitación en su referencia y conserva su test; nunca se presenta como hecho.
- Los planes y encargos pendientes viven en `.planning/`, versionados y con
  enlaces verificados; al terminarse se retiran y quedan en Git.
- Inputs técnicos no viven en `docs/`: licencias en `licenses/`, baselines de
  contratos y budgets en `tests/baselines/`, datos compartidos de pruebas en
  `tests/data/`, recibos vigentes que un gate lee en `qualification/`, soporte de
  scripts en `scripts/lib/`. Los recibos nuevos de calificación son artifacts
  (`target/qualification/` como destino local por defecto, o CI), no archivos
  versionados. Un recibo que cierra una gate del checklist 1.0 o mitiga un
  riesgo residual (`RR-n`) debe además quedar adjunto al PR o conservado como
  artifact de CI retenido — `target/qualification/` local no basta como único
  lugar de conservación.
- `scripts/docs-hygiene.py` verifica enlaces, anchors y esta estructura.
- No se marca cerrado un compromiso de `.planning/deferred-commitments.md`
  sin evidencia citada (recibo, test o permalink) que lo respalde.

Mantener sincronizados con cada cambio que los afecte:

- `README.md`
- `CHANGELOG.md`
- `SECURITY.md`
- `docs/README.md` y los documentos de `docs/guides/`, `docs/reference/`,
  `docs/architecture/`, `docs/operations/` y `docs/development/` afectados.

`README.md` es la guía pública de instalación, configuración, operación y uso del
MCP; no es un registro de planificación ni un diario de implementación. Debe
actualizarse en el mismo cambio siempre que se libere, elimine o modifique una
feature, una tool, un comando, un requisito de instalación, una configuración de
cliente/host, una plataforma soportada o una limitación operativa o de seguridad
que afecte a usuarios. Conserva todos sus badges. Los detalles permanecen en los
documentos especializados enlazados desde el README.

`CHANGELOG.md` registra cambios útiles al usuario por versión, no el diario de
trabajo.

## Política de subagentes

### Modelos y esfuerzo

- Main Technical Owner: GPT-5.6 Sol, High. El host configura el agente principal;
  el agente no debe afirmar que cambió su propio modelo o esfuerzo.
- Workers normales: GPT-5.6 Sol, Medium o High según la dificultad concreta.
- Investigación sencilla y pruebas: Medium.
- Decisiones difíciles y debugging complejo: High; subir a Extra High solo ante
  una necesidad demostrada, no por disponibilidad.
- Reviewer externo: Claude Code CLI, Sonnet 5 para revisiones habituales y Opus 5
  para arquitectura o seguridad compleja; esfuerzo proporcional, sin `ultracode`.
- Verificar versión/opciones de Claude Code antes de invocarlo y usar un modelo
  explícito. Si el modelo requerido no está disponible, informar la limitación sin
  sustituirlo silenciosamente.
- La revisión externa es read-only: sin implementación, commits, merges ni cambios
  de archivos. Pedir evidencia por archivo y findings de severidad explícita.

### Fronteras de delegación

El agente principal conserva arquitectura, scope, contratos públicos, security
model, roadmap, integración y cierre de milestones. Los subagentes son workers
temporales para trabajo independiente y delimitado.

Usarlos cuando aporten paralelismo o aislamiento claro: investigación de APIs,
revisión de seguridad, adapters aislados, migrations, fixtures, pruebas, benchmarks
o análisis de logs. No usarlos cuando varios workers tocarían las mismas interfaces
centrales o cuando coordinar cuesta más que ejecutar localmente.

Antes de delegar, definir:

- objetivo concreto y Definition of Done;
- archivos permitidos y fronteras que no puede cambiar;
- restricciones relevantes;
- pruebas requeridas;
- salida esperada.

Proporcionar solo el contexto necesario. Preferir tareas read-only cuando existe
riesgo de solapamiento. Los workers que editen deben poseer archivos disjuntos. Un
subagente no puede cambiar arquitectura global, contratos públicos, dependencias
estratégicas ni decisiones fundacionales sin revisión del agente principal.

Cada resultado de subagente debe resumir:

```text
Task
Result
Files changed
Tests executed
Evidence
Risks
Decisions
Open issues
```

El agente principal revisa el diff y la evidencia, integra, ejecuta el gate conjunto
y termina el worker. Usar el número mínimo de subagentes que produzca una ventaja
real; no mantener agentes ociosos ni reutilizar contexto obsoleto.

## Commits y cierre

Si se realizan commits, deben ser pequeños y coherentes; no mezclar refactors
masivos con features. No reescribir ni descartar cambios del usuario. Antes de
declarar cerrado un encargo que cambie seguridad, contratos o distribución,
ejecutar una revisión independiente acotada de seguridad, contratos y evidencia,
y registrar el resultado y el gate final en el PR.
