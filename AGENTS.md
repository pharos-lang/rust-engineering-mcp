# AGENTS.md — Rust Engineering MCP

Estas instrucciones aplican a todo el repositorio. La especificación principal es
`docs/spec/rust-engineering-mcp-propuesta-v0.3.md`; el código y las pruebas son la
evidencia del estado real.

## Ownership y precedencia

El agente principal actúa como Technical Owner, arquitecto, integrador y revisor
final. Ante conflictos, decide en este orden:

1. seguridad;
2. correctness;
3. requisitos explícitos de la especificación;
4. compatibilidad MCP;
5. contratos públicos existentes;
6. testabilidad;
7. mantenibilidad;
8. simplicidad operacional;
9. rendimiento;
10. ergonomía para agentes;
11. extensibilidad futura.

No se cambia silenciosamente una decisión arquitectónica. Si la evidencia obliga a
divergir de la especificación, se actualizan primero el ADR pertinente, la
documentación afectada y `docs/implementation-status.md`.

## Inicio obligatorio de cada sesión

Antes de modificar código:

1. leer este archivo;
2. leer completamente la especificación cuando no esté ya en el contexto fiable de
   la sesión;
3. revisar `docs/implementation-status.md` y todos los ADRs relevantes;
4. inspeccionar `git status`, el árbol real, los manifests, tests y CI;
5. continuar desde la primera vertical pendiente, sin rediseñar desde cero;
6. comprobar documentación oficial actual cuando una decisión dependa de MCP,
   `rmcp`, Cargo, SQLite, LanceDB, RustSec o una API/version cambiante.

La documentación oficial o el repositorio oficial de cada tecnología es la fuente
externa preferida. No se incorporan ejemplos de Internet sin contrastarlos con la
versión fijada en `Cargo.lock`.

## Alcance vigente

M0 y M1/0.1.0 están cerrados. El owner autorizó el 2026-09-05 integrar primero la
planificación M2–M8 y después implementar únicamente M2. La planificación quedó
integrada localmente en `2f54b360e1e81f21e7efeff7c451cdd6f663a04f`.
No avanzar a M3 ni publicar otra release/tag sin autorización explícita adicional.
M2 sigue su [plan](docs/roadmap/m2-safe-mutation.md), incluida la puerta D02 antes
de un writer. El owner delegó resolver D02 sin cargar instalación/uso;
[ADR-050](docs/adr/ADR-050-local-coordinated-mutation.md) adopta local_coordinated,
sin exclusión OS de editores externos ni broker privilegiado. La calificación
positiva del writer sigue siendo obligatoria. El contrato público implementado conserva estas trece tools M1:

- `rust.project.open`
- `rust.project.inspect`
- `rust.toolchain.inspect`
- `rust.check`
- `rust.fmt.check`
- `rust.clippy`
- `rust.test`
- `rust.dependencies.audit`
- `rust.diagnostics.explain`
- `rust.quality.gate`
- `rust.catalog.status`
- `rust.crate.search`
- `rust.crate.inspect`

M2 autoriza implementar `rust.fmt.apply`, `rust.fix.apply`, `rust.dependency.add`,
`rust.dependency.remove` y `rust.manifest.patch` por verticales calificadas. No
anunciar tools vacías ni considerar dieciocho tools implementadas antes de su
evidencia. Las trece anteriores conservan contratos y semántica.

`rust.dependencies.inspect` aparece en una sección descriptiva de la propuesta,
pero no pertenece al alcance inmediato que la propia propuesta y la instrucción del
owner enumeran al final. Puede existir como caso de uso interno, nunca como tool M1
sin un cambio explícito de alcance y ADR si afecta el contrato.

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
6. actualizar documentación, ADR y estado;
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

## ADRs y documentación viva

Un ADR es obligatorio para decisiones que cambien contratos públicos, arquitectura,
seguridad, persistencia, compatibilidad MCP, distribución, soporte cross-platform o
dependencias estratégicas. Debe contener como mínimo `Context`, `Decision`,
`Alternatives considered`, `Consequences` y `Status`.

Mantener sincronizados:

- `README.md`
- `CHANGELOG.md`
- `SECURITY.md`
- `docs/architecture.md`
- `docs/tools.md`
- `docs/security-model.md`
- `docs/compatibility.md`
- `docs/client-configuration.md`
- `docs/adr/`
- `docs/implementation-status.md`

`README.md` es la guía pública de instalación, configuración, operación y uso del
MCP; no es un registro de planificación ni un diario de implementación. Debe
actualizarse en el mismo cambio siempre que se libere, elimine o modifique una
feature, una tool, un comando, un requisito de instalación, una configuración de
cliente/host, una plataforma soportada o una limitación operativa o de seguridad
que afecte a usuarios. Los detalles de arquitectura, hitos y evidencia permanecen
en los documentos especializados enlazados desde el README.

`docs/implementation-status.md` es el tablero repo-visible. Solo mover un elemento a
Done cuando la columna de evidencia apunte a pruebas, comandos o artifacts reales.

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
estratégicas ni ADRs fundacionales sin revisión del agente principal.

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
declarar M0 o M1 cerrado, ejecutar una revisión independiente acotada de seguridad,
contratos y evidencia, y registrar el resultado y el gate final en el tablero.
