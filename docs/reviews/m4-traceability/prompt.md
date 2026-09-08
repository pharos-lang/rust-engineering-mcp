Revisión independiente read-only de trazabilidad M4. Modelo requerido Gemini 3.8 Flash High, esfuerzo high. No uses herramientas, no leas otros archivos, no edites ni ejecutes comandos. Todo el paquete está incluido a continuación. Evalúa si las decisiones y los oráculos preparados cubren el plan M4/G1-G9, detectando requisitos sin dueño o inferencias inválidas. El candidato aún NO está cerrado: benchmark 250/300, clientes reales, gate final y re-review Opus están explícitamente pendientes y se ejecutarán después; no redescubras esa lista conocida como hallazgos nuevos. No des Done. Entrega en español: tabla de cobertura M4-01..06/G1..9, hallazgos nuevos P0-P3 con archivo y justificación, condiciones concretas para cierre. La planificación mantiene Planned por diseño del maestro. Cita sólo contenido de este paquete.

FILE docs/roadmap/m4-security.md
# M4 — Security / 0.4.x

Estado: **Planned**. Entrada M3 cerrado. Fuentes: spec §27/35–47/48/81/97 M4,
[ADR-009](../adr/ADR-009-deny-by-default-security.md),
[ADR-038](../adr/ADR-038-owned-rustsec-audit.md),
[ADR-041](../adr/ADR-041-authenticated-catalog-bundles.md).
Aplican [G1–G9](m2-m8.md). Resultado: respuestas distintas para vulnerabilidad
conocida, policy de dependencias, presencia de unsafe, UB observado y facts de
supply chain, sin confundir hallazgos con certificación de seguridad.

## Contrato, ownership y antiobjetivos

Tools propuestas: `rust.deny`, `rust.unsafe.scan`, `rust.miri`,
`rust.supply_chain.inspect`. `rust.dependencies.audit` conserva su contrato y
motor RustSec. D19 asigna advisories al port existente; deny compone esa misma
observación y ejecuta licenses/bans/sources sin un segundo matcher/advisory refresh.
Supply chain combina hechos, no fabrica un score de seguridad ni una aprobación
legal. Config de proyecto solo restringe host. Sin remediation, fix automático,
descargas, MIRIFLAGS libres, policy universal de unsafe o ejecución host.

## Cortes verticales

| ID | Flujo end-to-end | Depende de | Evidencia/gate | Tamaño |
| --- | --- | --- | --- | --- |
| M4-01 | deny request→captura única→audit compartido+licenses/bans/sources→findings | M3, D19 | Advisory idéntico audit, ban/license/source, supresión expirada y dataset stale | L |
| M4-02 | source autorizado→scanner syntax→unsafe/extern spans por origen | 01, D20 | Comentario/string/cfg/macro/workspace/dependency discriminados | L |
| M4-03 | Miri request→nightly/sysroot fijado→sandbox→UB/unsupported/timeout | 01, D21 | UB real y control limpio, FFI, loop/cancel/descendiente | XL |
| M4-04 | grafo capturado→audit/deny/catalog facts→supply-chain report | 01/02 | Yanked known/unknown, git/source/checksum, duplicate/features y freshness | L |
| M4-05 | quality strict/release→una captura→etapas→veredicto completo | 01/03/04, M3, D19 | Sin audit duplicado, baseline SemVer explícito y partial no-pass | L |
| M4-06 | threat model→hardening→abuse fixtures→review→gate | 01–05, D21 | G1–G9, secrets/artifacts/poisoning/escape, recalibración runtime | XL |

Camino crítico: fuentes/policy→deny→supply chain→gates; runtime Miri y hardening
son otra rama obligatoria antes de cierre. Tamaño XL; no fechas. Cada hardening se
ata a abuso reproducible, efecto concreto y control positivo; no “sandbox mejorado”.

## Semántica y arquitectura

Domain: findings con source/rule/severity/coverage/suppression, unsafe locations,
Miri outcome y graph facts tipados. Application reutiliza DependencyAuditPort,
Task execution y captura única; ports nuevos solo para scanner/Miri/deny reales.
Execution adapter usa programas/args cerrados y runtimes inmutables. Catalog
adapter sigue autoridad SQLite, RustSec fuente independiente y freshness por input.
MCP añade schemas propios y CLI inventario/doctor de tools opcionales, sin ampliar
config implícita. No tocar schemas M1 al extender perfiles; decidir DTO/perfiles
nuevos en D19 y conservar fast/standard exactamente.

Deny: [checks oficiales](https://embarkstudios.github.io/cargo-deny/checks/index.html).
Cada subcheck tiene fuente/fecha/versión/config y cobertura. Licencias requieren
source bytes offline verificados (D05), no inferir licencia de código de un string
del manifest. Unknown no es licencia permitida. Suppression propuesta: rule/source/
package version range/reason/owner/expiry/policy digest. Sin suppression global
silenciosa, ni ignorar todas las vulnerabilidades. Expirada/malformada se rechaza;
se informa hallazgo original y disposición, manteniendo audit M1 intacto.

Unsafe: ubicar unsafe blocks/fn/impl y extern, separar workspace/dependencies;
presencia no implica defecto ni ausencia implica seguridad. D20 compara AST/parser
propio limitado con scanner externo exacto; regex sola no distingue strings/macros.
Declarar cfg, expansión de macros y generado no cubierto. Sin ejecución para
expandir macros salvo permiso/sandbox separado. Fixture con falso positivo en
comentario y unsafe real de mismo texto discrimina semántica.

Miri: [repo oficial](https://github.com/rust-lang/miri), versión nightly/commit/sysroot
y digest decididos en D21. No setup automático. Clasificar UB/test failure/compile
failure/unsupported operation/timeout; modo limpio no prueba ausencia universal
de UB. No permitir flags que quiten isolation. Test fixtures use-after-free,
uninitialized/aliasing/race, limpio, FFI unsupported y cfg(miri), con resultado
esperado fijado contra el binario real. No correr ejemplos hostiles en el host.

Supply chain: captured lock graph+metadata, sources/checksums/duplicados/git,
audit y deny, yanked del catálogo con fuente/freshness, features declaradas vs
activas. ADR-044 no guarda package source/checksum/doc URLs completos: unknown
permanece unknown o D22 introduce schema/adquisición aparte. No convertir advisory
IDs de catálogo en auditoría. Ausencia de catálogo no borra findings útiles ni
produce “clean”. URL con credenciales se redacta, sin resolverla por red.

Gates propuestos en D19: strict=standard+deny+coverage; release=strict+semver con
baseline ProjectRef explícito. Mutation solo opt-in con presupuesto, nunca default.
Captura actual única para todas etapas y baseline separado identificado; audit se
reutiliza una vez. Composición application no llama tools MCP entre sí.

## Threat model, operación y distribución

Amenazas: dependencia/plugin comprometidos, malicious build.rs/proc macro,
catálogo/model poisoning, source confusion, suppression amplia/expirada, parser
hostil, secretos en artifacts y escape/quota. Mantener no-follow y red deny real,
env reconstruido, process tree cleanup y auditoría G2/G3. Runtimes nuevos exigen
calibración nueva: evidencia vieja del image M1 no autoriza Miri.

Hardening mínimo M4-06: secret canaries en logs/diagnósticos/HTML/diffs antes de
publicar; firma/hash/sequence/source mismatch de catálogo; binario plugin digest
alterado; attempts socket/cloud metadata/namespace/mount; orphan/fork/disk/output
bombs. Documentar limitación de secret scanning, allocator nativo y ACL/privileged
host; cerrar claims que no tengan enforcement, no parchear el threat model con
palabras. Revocación de tool/source bloquea nueva admisión y conserva auditoría.

Budgets: heredar M3 para artifacts/jobs, M1 para captura/grafo; defaults propuestos
scan/deny 120 s, Miri 300 s/máximo 1800 s, 128 findings visibles, 512 KiB result.
Omisiones/coverage quedan explícitas, paging owner-bound si existe artifact.
SLI: findings por fuente, stale/unknown, suppression vencida, incomplete,
timeouts/cleanup y scan/redaction bytes. No telemetría externa ni “cero findings”
como SLO de seguridad. Riesgo de falsos positivos se mitiga con fixture/origen y
supresión auditable, no desactivando controles.

Tests unit/contract/protocol/integration/security/native/performance y clientes G4;
fixtures reales RustSec/SQLite, scanner/plugin/Miri. Linux/Windows portable solo;
macOS/APFS y guest de runtime nuevo se califican positivamente. Pin/lock de nuevas
dependencias requiere auditoría, licencias/notices, SBOM/provenance y smoke de
bytes distribuidos. Security bundle opcional solo por demanda/ADR. No claves ni
catálogo oficial. Rollback deshabilita plugin/source comprometido, cuarentena,
vuelve a formatos compatibles sin retroceder floor y retestea antes de admisión.

## DoR, DoD y aceptación

DoR: M3 cerrado; D19–D22 preparados/decididos según efecto, datasets frescos con
origen, plugins y nightly exactos aprovisionados, suppressions/fixtures/budgets
cerrados. DoD: seis cortes y G1–G9, Sonnet por contrato, Opus High threat/sandbox,
sin P0/P1 ni P2 de seguridad/evidencia obligatoria. No cierre solo con Miri denied.

- [ ] Cada pregunta tiene una tool/motor dueño y deny no duplica vulnerabilidades
  ni refresh de audit. Fuente: spec §27, ADR-038; M4-01/04, D19.
- [ ] Unsafe se reporta por origen/cobertura; Miri distingue UB/unsupported y pasa
  casos reales adversos/limpios. Fuente: spec §27.2/27.3; M4-02/03.
- [ ] Suppressions, freshness, fuentes faltantes y datos desconocidos son visibles,
  sin clean parcial. Fuente: ADR-020/038/044; M4-01/04/05.
- [ ] Threat review, secret-canary y poisoning/containment tienen controles reales
  y cleanup probado. Fuente: spec §36/81, ADR-009/041; M4-06.
- [ ] Full gate, clientes, inventario/license/SBOM/provenance y distribución por
  target coinciden con claims. Fuente: G4/G5/G7/G8 y ADR-048.

Handoff: gates y source hashes, fuentes/policy/suppressions, runtime Miri, findings
dispuestos y límites de inferencia; detener antes de M5.

FILE docs/roadmap/m2-m8.md
# Roadmap M2–M8 y readiness 1.0

> Resolución posterior a la planificación: el owner delegó la decisión D02 para
> preservar instalación/uso sencillos. [ADR-050](../adr/ADR-050-local-coordinated-mutation.md)
> acepta local_coordinated: namespace host confiable, precondiciones y locks MCP,
> sin exclusión OS de editores externos. Las exigencias de exclusión fuerte y espera
> de decisión del owner que aparecen abajo son históricas y quedan sustituidas por
> ese ADR; no hay CAS, atomicidad multiarchivo ni rollback sobre bytes desconocidos.
> La [calificación positiva de M2](../validation/M2-07.md) está completada. M3+ conserva su planificación.


Estado: **M2 Done local; M3–M8 Planned/Conditional**. Fecha de planificación: 2026-09-05. Este documento es backlog,
no evidencia de implementación ni una aceptación de ADR. M0/M1 conservan Done.
La instrucción final del owner autoriza integrar esta planificación y después
implementar exclusivamente M2; las fases se validan y registran por separado.
No autoriza publicar otra release, mover v0.1.0 ni avanzar a M3.

## Baseline y autoridad

Fuentes normativas: [spec v0.3.1](../spec/rust-engineering-mcp-propuesta-v0.3.md),
[AGENTS](../../AGENTS.md), [ADRs aceptados](../adr/README.md). Los ADRs concretan los
ejemplos de la spec. Código, tests y receipts determinan lo implementado.
YouTrack no participa; el tablero del repositorio es la fuente de planificación.

La [verificación live](baseline-2026-09-05.md) distingue HEAD de planificación,
export privado, commit/tag público y evidencia histórica de M1. La release 0.1.0
no cambia. Su inventario tiene trece tools, excluyendo dependencies.inspect.
Solo el core aarch64-apple-darwin se distribuye; macOS 26 ARM64/APFS es el host
positivo y Docker Linux ARM64 el guest de ejecución. Linux/Windows son CI portable
y fail-closed. No se distribuyen catálogo oficial, trust, modelo, ORT, LanceDB,
Docker, toolchain ni fixtures. crates.io permanece deshabilitado.

## Orden, dependencias y releases propuestas

| Hito | Estado | Entrada | Resultado y release propuesta | Tamaño relativo |
| --- | --- | --- | --- | --- |
| [M2](m2-safe-mutation.md) | Done; [evidencia local](../validation/M2-07.md), sin release nueva | Baseline M1, decisiones de escritura y oráculos nativos | Cinco mutaciones transaccionales; 0.2.x | XL |
| [M3](m3-quality.md) | Planned | Cierre real M2; ejecución y persistencia de tareas decididas | nextest, cobertura, SemVer, mutation testing; 0.3.x | XL |
| [M4](m4-security.md) | Planned | M3 cerrado; fuentes y runtime de seguridad calificados | deny, unsafe, Miri, supply chain y hardening; 0.4.x | XL |
| [M5](m5-performance.md) | Planned | M4 cerrado; protocolo de medición congelado | benchmark/compare/flamegraph/bloat; 0.5.x | L |
| [M6](m6-analyzer.md) | Planned | M5 cerrado; M2 válido para acciones, M3 para lifecycle | Adapter rust-analyzer y evidencia semántica; 0.6.x | XL |
| [M7](m7-remote.md) | Conditional | M6 cerrado y caso real aprobado | Go: 0.7.x; No-go: Deferred, sin release ficticia | XL solo si Go |
| [M8](m8-stabilization.md) | Planned | M6 cerrado y M7 cerrado o Deferred con decisión | Congelación 0.8, RC y estabilidad 0.9; checklist 1.0 | XL |

S equivale a un contrato/parser pequeño; M a una frontera existente; L a una
integración externa con fixtures reales; XL a varias fronteras de seguridad o
persistencia. Son comparaciones de incertidumbre/esfuerzo, no días ni capacidad.
No hay fechas de entrega. Se recalcula el tamaño tras cada DoR.

Camino crítico: baseline → decisión transaccional M2 → filesystem/gateway de
mutación → cinco tools → cierre M2 → tareas/artifacts M3 → adapters M3 →
fuentes/sandbox M4 → medición M5 → lifecycle/acciones M6 → decisión M7 → freeze
M8 → upgrades/auditoría/RC → readiness 1.0. Si Go, remoto se inserta antes del
freeze; si Deferred, no bloquea estabilización local. Dentro de un milestone pueden
investigarse parsers/fixtures independientes; no integrar contratos centrales en
paralelo ni comenzar el siguiente milestone por haber terminado un corte.

## Contrato transversal de ejecución del plan

Estas obligaciones forman parte de cada milestone y de cada criterio que cite
G1–G9; no son una fase final opcional.

### G1 — Arquitectura y contrato

Domain contiene valores/invariantes; application compone ports reales y no conoce
rmcp, JSON-RPC, stdio, Cargo CLI, SQLite o LanceDB. Adapters adquieren bytes y
ejecutan efectos. MCP conserva DTOs Serde/Schemars cerrados, schemas derivados,
validación de entrada/salida y espejo TextContent de structuredContent. rmcp
gestiona lifecycle/transporte. No interfaces sin consumidor end-to-end.

Las trece definiciones 0.1.0 se congelan individualmente, incluyendo annotations,
defaults, enums y significado de resultados. Una nueva tool no cambia una lectura
en escritura. Cada contrato nuevo pasa ADR propuesto→decisión documentada en la
sesión de implementación→snapshot→wire tests→docs→release minor. No ampliar enums
compartidos silenciosamente si modifica schemas viejos. `failed/isError=false`
representa un fallo observado del proyecto; ausencia/policy/timeout son operativos;
ningún partial, skip o unavailable es pass de un gate obligatorio.

### G2 — Autoridad y threat model

Host confiable concede roots/efectos; peer, proyecto, URI, fingerprint y annotations
no conceden permisos. Defaults niegan escritura, ejecución y red. Configuración
del proyecto solo restringe. Cada corte documenta actores, activos, trust boundaries,
entrada hostil, abuso, control, oráculo positivo/negativo y riesgo residual.
No hay shell, flags libres, wrappers/linkers/runners del proyecto ni herencia de
secretos. Toda ejecución atraviesa el gateway tipado único y env reconstruido.
`build.rs`, proc macros, tests, Miri, benchmarks y analyzer que los active son código
del proyecto. Tool ausente se detecta y reporta; nunca se instala en tools/call.

I/O propio parte de handles de roots originales y exige no-follow/reparse-safe;
canonicalización previa no resuelve TOCTOU. Mantener las limitaciones de dispositivos,
mounts/ACL y captura no atómica de ADR-024/031 hasta que otro gate pruebe más.
Network isolation requiere enforcement con controles reales. Si falta una garantía
solicitada, rechazo; no degradación a ejecución host. SQLite decide facts y LanceDB
solo candidatos. Snapshots llevan provenance/freshness y latest_known. Ningún
runtime MCP adquiere catálogos, modelos, advisories o runtimes.

### G3 — Lifecycle, concurrencia, cuotas y auditoría

Mantener worker unido hasta terminar trabajo y cleanup; cancellation/EOF/timeout
no significan proceso terminado. Incertidumbre de cleanup domina el resultado y
cuarentena impide reutilizar el gateway. Probar hijo desacoplado activo antes de
cancelar y ausencia posterior de todos los objetos propios. M3 define scheduler
acotado sin saltarse esta frontera; M7 amplía aislamiento por tenant solo tras Go.
No relajar los caps M1 por introducir jobs largos. Cada nuevo límite se fija antes
del código con unidad, fase cubierta, valor por defecto/máximo y prueba del exceso.

Logs por tracing/stderr con job/receipt IDs opacos; métricas locales: admisión,
rechazos por razón, duración por fase, timeout, cancelación, cleanup incierto,
bytes/entradas retenidas y omitidas, conflictos/rollback donde haya mutación.
No telemetría externa por defecto. Auditoría no incluye tokens, URLs con credenciales
ni source completo. Definir retención, autorización y borrado de evidencias; redacción
literal M1 no equivale a detección universal de secretos. Describir por separado
contenido retenido, memoria nativa y tiempo de cleanup.

### G4 — Fixtures y pruebas

Cada corte exige unit tests de invariantes/parsers, contract tests de schemas,
protocol tests de negociación/errores/cancelación/Resources, integration tests con
la herramienta exacta y fixtures adversariales en el sandbox. Tests nativos en
cada target que anuncie capacidad positiva. Mock no sustituye frontera Cargo,
SQLite/FTS5/LanceDB, proceso ni filesystem. Un oráculo independiente debe discriminar
éxito, fallo del proyecto, dato falso/incompleto y fallo de infraestructura.

Matriz wire heredada: 2024-11-05, 2025-03-26, 2025-06-18, 2025-11-25,
2026-07-28 con rmcp 3.2.0; solo modificarla con decisión y evidencia nuevas.
Cada nuevo milestone exige Inspector y un cliente stock dirigido por modelo en
la versión exacta disponible: discovery→llamada positiva→fallo→cancel/Resource
cuando aplique. Conservar también intentos fallidos. No atribuir cancelación
real del árbol al mero envío de una notificación desde Inspector.

### G5 — Gates y evidencia

Primero gate focalizado de cada corte; después revisión del diff y documentación;
para cierre ejecutar una vez el gate conjunto sobre bytes finales:

```text
cargo fmt --check
cargo check --workspace --all-targets --locked --offline
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo test --workspace --all-targets --locked --offline
python3 -B scripts/check-architecture.py
python3 -B scripts/gate.py core
python3 -B scripts/gate.py full
```

El gate existente ya incorpora validaciones básicas; evitar duplicarlas si su
receipt acredita exactamente esos comandos. Full se ejecuta con las variables
host explícitas descritas en [CI](../ci.md), assets exactos y todos sus tests
ignorados nativos invocados expresamente. Core solo no cierra un milestone que
afecte el perfil local. Audit/deny solo si instalados/configurados; ausencia se
reporta y bloquea la calificación que los exige, sin instalación silenciosa.
CI portable conserva Linux x86_64/macOS ARM64/Windows x86_64 y supply chain;
SonarCloud y los checks live requeridos deben pasar antes de integración remota.

`--locked --offline` concreta reproducibilidad y el uso de dependencias ya
aprovisionadas en esta ejecución del gate de AGENTS; no modifica ese archivo ni
autoriza resolver o descargar versiones durante el gate. Si falta una dependencia,
registrar la ausencia y aprovisionarla solo con autorización explícita separada.

Conservar en la implementación futura `docs/validation/Mn-matrix.md`, receipts
por corte, comandos/exit codes/inicio-fin UTC/conteos directos, hashes de source,
schemas/binarios/fixtures, toolchain/runtime/OS/FS, controles de seguridad, lista
de skips/ausencias, reviews y dispositions. Esas rutas son artifacts requeridos
futuros, no enlaces a evidencia inexistente. Gate viejo nunca acredita código nuevo.
El smoke post-integración identifica commit y bytes, sin repetir gates caros si
solo cambia documentación y se demuestra igualdad del código previamente probado.

### G6 — Compatibilidad, migración y rollback

Versiones separadas para servidor, contrato de tool, formato de artifact/receipt,
snapshot/catálogo, índice/modelo y protocolo negociado; no parámetro version
obligatorio en cada tool. Datos desconocidos/incompatibles fallan antes de efectos.
Migración administrativa explícita, backup/reconstrucción y pruebas upgrade/rollback;
nunca downgrade de floor antirollback ni de autorización para recuperar disponibilidad.
Rollback de producto vuelve a binario anterior conservando evidencia y datos
compatibles; una transacción pendiente impide downgrade hasta reconciliación.
Cambios de manifests invalidan referencias según contrato existente; no extender
su autoridad silenciosamente. M2 especifica cómo consultar receipts tras invalidación.

### G7 — Operación y distribución

Cada corte registra inventario de herramientas externas y componentes nuevos con
versiones exactas, checksums, licencias/notices, SBOM/provenance y forma explícita
de aprovisionamiento. Un plugin instalado en CI no está instalado en el runtime.
No convertir quality/security/full bundles de §108 en obligación de empaquetar:
requieren demanda y decisión. Mantener artifact core macOS ARM64 hasta calificación
expresa por target; el perfil local, catálogo y claves siguen excluidos de distribución.
Cambiar runtime, target, fuente de datos o dependencia estratégica exige ADR y gate.
Readiness de release requiere instalar y probar los bytes empaquetados, comprobar
hashes, source/tag/workflow/run de attestations y redescarga; no basta generarlas.

### G8 — Revisión independiente y bug bar

Antes de cada cierre: Sonnet 5 read-only para contratos/cortes, Opus 5 High para
mutación/containment/persistencia/remote o coherencia final; Gemini 3.8 Flash High (`gemini-3.8-flash-high`) mediante
agy para trazabilidad global si disponible. Verificar modelo/versión/effort antes
de uso; no sustituciones silenciosas. Paquete pequeño con fuentes normativas,
diff, archivos y hashes, fixture/oráculo, gate y limitaciones. Registrar findings
P0–P3 por archivo, disposición y re-review de cambios materiales. P0/P1 bloquean
merge/release; P2 de seguridad, pérdida de datos, contrato o gate obligatorio bloquea
hasta resolución; P2 restante requiere justificación y corte/owner asignado antes
de merge, y no puede vulnerar el DoD. P3 puede ir a backlog trazado. Review de modelo
es evidencia auxiliar, no auditoría humana ni prueba primaria.

### G9 — Definition of Ready y Done común

DoR: cierre anterior verificado live; scope/DTO/permission/threat model y decisiones
previas resueltos; fixture y oráculo discriminantes definidos; herramientas/targets
disponibles o bloqueo explícito; presupuesto y rollback verificables; archivos de
workers disjuntos. No empezar una operación dependiente de un defecto real M1.

DoD: todos los cortes obligatorios y criterios locales satisfacen G1–G8; evidencia
reproducible source-bound; docs públicas/CLI/tools/seguridad/compatibilidad/changelog
sin promesas nuevas no calificadas; revisión independiente dispuesta; integración
y smoke registrados. Release publicada es un gate separado sujeto a autorización.
Estados de esta planificación permanecen Planned/Conditional/Deferred.

## Resoluciones y decisiones pendientes

La [trazabilidad](traceability-m2-m8.md) incluye contradicciones, todas las secciones
de la spec y limitaciones M1. El [backlog de decisiones](adr-backlog-m2-m8.md)
no modifica ningún ADR Accepted. Prioridad: escritura/publicación recuperable M2,
provisionamiento offline y tareas M3, datos/sandbox M4, protocolo estadístico M5,
analyzer M6, go/no-go M7 y contrato/distribución 1.0.

## Secuencia de sesiones y handoffs

Ejecutar un prompt por milestone. Cada uno exige evidencia de cierre anterior y
termina con un handoff; no autoriza continuar automáticamente.

1. [Implementar y cerrar M2](../prompts/implement-m2.md).
2. [Implementar y cerrar M3](../prompts/implement-m3.md).
3. [Implementar y cerrar M4](../prompts/implement-m4.md).
4. [Implementar y cerrar M5](../prompts/implement-m5.md).
5. [Implementar y cerrar M6](../prompts/implement-m6.md).
6. [Evaluar y, solo con Go, cerrar M7](../prompts/implement-m7.md).
7. [Estabilizar M8 y evaluar readiness 1.0](../prompts/implement-m8.md).

El índice de [revisiones y validación documental](planning-validation.md) registra
lo que efectivamente se verificó en esta sesión; un requisito listado arriba no
significa que ya pasó.

FILE docs/validation/M4-matrix.md
# M4 — matriz de implementación y calificación

Fecha: 2026-09-07. Rama de trabajo: `ai/m4-security`. Base:
`c66a3704e1ad290603a3c1d10413df90d15c2b03`.

Estado: **In progress — implementación interna M4-01..05 y calificación M4-06**. El encargo actual
autoriza implementar M4 y sustituye el límite histórico de AGENTS. No autoriza
M5 ni publicación. Deny está integrado internamente con Tasks/Resources pero sigue oculto hasta
su calificación completa. El inventario público conserva 22 tools y versión
`0.3.0-dev`.

## Entrada M3

El [recibo de comprobación](M4-prerequisites.json) recalcula los 810 inputs de
`scripts/gate.py` (46027636 bytes) contra core y full M3. Las dos diferencias son
vacías; ambos gates pasaron 14/14 y 25/25 respectivamente. SHA-256 canónico del
inventario inicial: `791856ed9325c55ade73809207e971d3fd76c0bc3abd73c5b97ead54dff0ac9e`.
No se volvieron a ejecutar gates del mismo código.

La revisión final M3 anterior precede al delta de portabilidad/Sonar y al bump
de versión integrado. Se preparó una revisión Opus 5 High read-only de
`e2ec7da..c66a370`, con 50 archivos (incluido `sonar-project.properties`):
[inputs](../reviews/m4-prerequisite/inputs.json),
[diff](../reviews/m4-prerequisite/delta.patch) y
[prompt](../reviews/m4-prerequisite/prompt.md).
La [revisión](../reviews/m4-prerequisite/review.md) no detectó regresiones de
producto, pero emitió P2-1 por exclusiones Sonar sobre archivos con pruebas
portables. Se retiraron todas las exclusiones Rust, se corrigió la documentación
y se incluyó `sonar-project.properties` en el inventario del gate. Los dos
oráculos nuevos fallan contra HEAD anterior y la suite corregida pasa 11/11:
[recibo](../reviews/m4-prerequisite/sonar-verification.json). La [confirmación Opus](../reviews/m4-prerequisite/confirmation/review.md)
aceptó el prerrequisito sin P0/P1/P2. P3-2 (hashes documentales stale) también corregido. P3-1/3/4 se conservan
como deuda explícita para evaluar en M4-06, sin afirmar que se hayan corregido.

Los recibos M3 acreditan los bytes de producto de la base. Los scripts de gate
corregidos se verifican con el recibo focalizado anterior; los fixtures M4 nuevos
no están calificados por los gates M3. El gate M4 final debe incluir todos esos
inputs nuevos y la configuración de Sonar, sin reutilizar el inventario de 810
como si describiera la implementación futura.

## Cortes

| Corte | Estado | Evidencia y pendiente para Done |
| --- | --- | --- |
| M4-01 deny | In progress — implementación completa, anuncio cerrado | Aplicación/adversariales/Tasks/Resource nativos pasados; suppressions exactas y audit único. Faltan clientes y gate final. |
| M4-02 unsafe.scan | In progress — helper v3 y gateway nativos | [Siete oráculos](M4-scanner-native.json), contención base, reserva 25 s y prefijo parcial. Confirmación Opus en curso. |
| M4-03 Miri | In progress — 19 oráculos finales pasados | [12 clasificaciones y 7 admisión/lifecycle](M4-miri-native.json), binary-only y timeout del runner. Task EOF/revocación y canaries MCP añadidos, pendientes de ejecución. |
| M4-04 supply_chain.inspect | In progress — integración completa sin migración | Parser facts, seis tests SQLite firmado, 22 tests security/Supply, contratos y MCP. Missing inputs/4097/truncación exacta probados. Faltan clientes/gate. |
| M4-05 strict/release | In progress — integración completa | Seis tests aplicación; strict/release nativos pasados, baseline explícito y mutation clean/fail/inconcluso probados. Faltan clientes/gate. |
| M4-06 hardening/cierre | In progress | ADR-070, imagen final calibrada, parser/metadata adversarial, catálogo firmado, source canaries y lifecycle preparados. Revisiones de confirmación y gate final pendientes. |

## Puertas comunes

| Gate | Estado M4 | Evidencia / condición |
| --- | --- | --- |
| G1 contratos | In progress | 23 snapshots M1–M3 preservados; cinco esquemas cerrados y validación de observaciones reales. Falta congelar los cinco snapshots nuevos y protocolo publicado. |
| G2 autoridad/seguridad | In progress | Captura única, revalidación de owner/policy y fingerprints; fuente/vendor/policy read-only, Miri integrity-admission y helper aislado. Threat model actualizado. |
| G3 lifecycle/budgets | In progress | 30 cold/30 warm por tool ejecutándose sobre un binario congelado. Nuevos tests MCP EOF/revocación pendientes. |
| G4 clientes/native | In progress | Gateway/Tasks/Resource nativos positivos; harness Inspector 2.5.0 y Codex 0.153.0 preparado, ocho tests de harness pasados. Ejecución real pendiente de calificar sync. |
| G5 gates | In progress | Clippy workspace y arquitectura pasados. Full final pendiente. Cross-Clippy Linux local intentado: falta `x86_64-linux-gnu-gcc`; no se instaló ni se acredita esa plataforma. |
| G6 rollback | In progress | Imagen M3 preservada y formatos M3 reutilizados sin migración ni retroceso de floor. Mapa y controles finales en preparación. |
| G7 inventario | In progress | Provisión autorizada de 247 inputs, SBOM/licenses/provenance y helper v3 fijado; CLI pasivo separado de doctor. Re-acreditación directa sysroot25ed preparada. |
| G8 review | In progress | Sonnet composición y Opus Miri/helper ejecutados; correcciones y pruebas P1/P2 añadidas, confirmaciones en curso. No hay disposición de cierre todavía. |
| G9 DoR/DoD | In progress | D19–D22 decididos; todos los cortes implementados internamente, ningún Done hasta cerrar las puertas anteriores. |

## Aprovisionamiento autorizado

El owner autorizó la adquisición exacta de ADR-066. La imagen resultante es
`sha256:95dddeb5305f10b09b441e3cc4018ebb1a8a296d365c65106327d59f933c64e7`;
[provisión](M4-provisioning.json), [Miri preparado](M4-prepared-miri.json) y
[contención base](M4-base-calibration.json) tienen resultados reales. El fallo
histórico de `cargo miri setup --print-sysroot` se conserva: ese comando intenta
aprovisionar aunque exista MIRI_SYSROOT; el test Miri con sysroot preaprovisionado
sí pasó y no modificó sus bytes. Ningún test básico sustituye la vertical Miri.

El [inventario](m4-provisioning-proposal/manifest.json) fija 247 inputs; el build
se ejecutó en Docker sin red. M3 permanece inmutable. El helper D20 final deriva la imagen `25ed3626…91635`,
con provisión y recalificación separadas en ADR-068/069.

[G5](../roadmap/m2-m8.md) exige: «Si falta una dependencia, registrar la ausencia
y aprovisionarla solo con autorización explícita separada». El [README del
fixture](../../fixtures/rust-runtime/README.md) también exige autorización antes
de ejecutar provisioning. Esa autorización ya se recibió en esta sesión; la
aprobación técnica de imagen/sandbox depende después de resultados y revisión,
y permanece a cargo del Technical Owner.

## Handoff de trabajo en curso

Las cinco tools y las seis verticales están implementadas, con anuncio MCP aún
cerrado. Se preservan los intentos fallidos; los receipts históricos no se usan
como gate final del código posterior. Restan mediciones, clientes, nuevos controles
MCP, confirmación de reviews y full gate source-bound. No hay commits/PR/tag/release.
La evidencia M3 sin seguimiento preexistente permanece intacta.

Rollback: seleccionar imagen M3 y retirar configuración M4; el store mantiene su
formato M3. No ejecutar el [prompt M5](../prompts/implement-m5.md).

FILE docs/validation/M4-hardening-map.md
# M4-06 hardening coverage map

Status: static investigation only, 2026-09-07. This document does not mark
M4-06, M4, or any gate complete. No Docker command or test command was run for
this review. Runtime results must come from the explicit qualification harness
and retain their own image, source, fixture, exit, count, and cleanup evidence.

## Scope and source of requirements

The controlling requirements are the threat model and minimum hardening list in
`docs/roadmap/m4-security.md`: compromised dependencies/plugins and hostile
`build.rs`/proc macros; catalog or model poisoning; source confusion; broad or
expired suppressions; hostile parsers; secrets in artifacts; containment escape
and quota exhaustion; secret canaries in logs, diagnostics, HTML, and diffs;
catalog signature/hash/sequence/source mismatch; altered plugin digest; direct
socket/cloud-metadata/namespace/mount attempts; orphan/fork/disk/output bombs;
documented native allocator, ACL/privileged-host, and secret-scanning limits; and
tool/source revocation that blocks new admission while preserving audit.

The following map names tests precisely so a final receipt can prove that one
case, rather than merely a similarly named module, executed.

## Explicit M4 runtime harness

`scripts/test-m4-runtime.py` currently selects 19 ignored tests with
`--exact --ignored --nocapture --test-threads=1` and requires the libtest line
for exactly one passing test. A static name check found the original 18 leaf functions in
the selected package and target; no misspelled or stale filter was found.

The harness reads its advertised image from
`docs/validation/M4-runtime-image.json` and exports it as
`RUST_MCP_TEST_IMAGE`:

- derived M4 runtime: `sha256:25ed3626e710081a571a86a29521eaf2e890e796afd422ba5e409e0ce1891635`;
- base security runtime: `sha256:95dddeb5305f10b09b441e3cc4018ebb1a8a296d365c65106327d59f933c64e7`;
- M3 Rust runtime: `sha256:384a1742ecc53cdd3a9c0bf36c6f8b66db73ddd118aeeae6e55654ea998ae36a`.

The effective image for each selected case is:

| Exact selection | Effective image | Reason |
| --- | --- | --- |
| `security_runtime::m4_project_output_canaries_are_absent_from_security_results_and_resources` | derived M4 `25ed…` | Passes `APPROVED_M4_IMAGE` explicitly. |
| `security_runtime::m4_miri_tasks_cancel_eof_and_revocation_join_before_releasing_authority` | derived M4 `25ed…` | Passes `APPROVED_M4_IMAGE` explicitly. |
| `rust_calibration::tests::resource_limits_are_actually_enforced` | derived M4 `25ed…` | Reads `RUST_MCP_TEST_IMAGE`. |
| `security_native::m4_scanner_runtime_base_containment_is_requalified` | derived M4 `25ed…` | Hard-codes the derived digest. |
| `security_native::m4_deny_native_text_licenses_and_bans_are_real_and_cleanup_is_joined` | base security `95dd…` | Hard-codes the base security digest and ignores `RUST_MCP_TEST_IMAGE`. |
| `security_graph_native::captures_workspace_dependency_graphs_through_the_gateway` | base security `95dd…` | Module constant hard-codes the base security digest. |
| `security_native_adversarial::m4_deny_adversarial_oracles_preserve_cleanup_and_inputs` | base security `95dd…` | Module constant hard-codes the base security digest. |
| `unsafe_native::m4_scanner_native_oracles_preserve_partial_results_inputs_and_cleanup` | derived M4 `25ed…` | Module constant hard-codes the derived digest. |
| `miri_native::m4_miri_gateway_classifies_native_oracles` | derived M4 `25ed…` | Hard-codes the derived digest. |
| `miri_native::m4_miri_rejects_native_producers_and_joins_timeout_and_cancel` | derived M4 `25ed…` | Hard-codes the derived digest. |
| `security_runtime::deny_native_mcp_tasks_policy_licenses_and_owner_bound_redacted_resource` | base security `95dd…` | Its `start` helper uses `APPROVED_SECURITY_IMAGE`. |
| `security_runtime::m4_tools_native_mcp_observations_composition_and_private_resources` | derived M4 `25ed…` | Passes the derived digest explicitly. |
| `rust_calibration::tests::observed_descendants_are_cleaned_on_timeout_cancel_and_overflow` | derived M4 `25ed…` | Reads `RUST_MCP_TEST_IMAGE`; `calibrate` executes timeout, cancel, and overflow descendant scenarios. |
| `rust_calibration::tests::actual_clippy_build_script_and_proc_macro_containment` | derived M4 `25ed…` | Reads `RUST_MCP_TEST_IMAGE`. |
| `rust_gateway::test_runtime::actual_test_runtime_containment_and_descendant_cleanup` | derived M4 `25ed…` | Reads `RUST_MCP_TEST_IMAGE`. |
| `quality_profile_allows_only_the_required_anonymous_unix_stream_pair` | derived M4 `25ed…` | The nextest integration gateway reads `RUST_MCP_TEST_IMAGE`. |
| `hostile_html_is_retained_only_as_opaque_archive_bundle` | derived M4 `25ed…` | Coverage accepts M3 or derived M4 and reads the exported value. |
| `host_source_and_canary_are_unchanged_after_every_mutation_run` | derived M4 `25ed…` | Mutation integration gateway reads `RUST_MCP_TEST_IMAGE`. |

The planned run is 15 selections on `25ed…`, four on `95dd…`, and zero
on M3 `384a…`. The harness now records `primary_image_id`,
`base_security_image_id` and the effective image per step. Several native
fixtures use this host's explicit Docker socket and do not establish portability.

The ignored test `security_native::m4_runtime_base_containment_is_requalified`
is not among the 19 selections and targets `95dd…`. Its similar name must not be
confused with the selected scanner calibration on `25ed…`.

## Threat-to-test map

| Required threat/control | Existing discriminating evidence | Image when applicable | Coverage and limitation |
| --- | --- | --- | --- |
| Malicious `build.rs` and proc macro | `rust_calibration::tests::actual_clippy_build_script_and_proc_macro_containment`; `rust_gateway::test_runtime::actual_test_runtime_containment_and_descendant_cleanup` invokes the shared containment checks in a real test flow. | `25ed…` in the harness | Selected. The fixture checks Linux ARM64, uid/gid 65534, zero capabilities, `no_new_privs`, seccomp, mount modes, environment and forbidden syscalls. |
| Socket and namespace/mount escape | `rust_calibration::tests::actual_clippy_build_script_and_proc_macro_containment`; `quality_profile_allows_only_the_required_anonymous_unix_stream_pair`; shared fixture `fixtures/security/rust-containment/checks.rs`. | `25ed…` | Selected. Network and filesystem socket creation are denied; anonymous Unix `SOCK_SEQPACKET` is the positive control. `unshare`, `setns`, `mount`, `ptrace`, `mknodat`, `keyctl`, `bpf`, `io_uring_setup`, and invalid namespace `clone` must return `EPERM`. |
| Orphan/fork and process-tree cleanup | `rust_calibration::tests::observed_descendants_are_cleaned_on_timeout_cancel_and_overflow`; `rust_gateway::test_runtime::actual_test_runtime_containment_and_descendant_cleanup`; `security_runtime::m4_miri_tasks_cancel_eof_and_revocation_join_before_releasing_authority`. | `25ed…` | Selected. Calibration and actual libtest observe detached descendants, require timeout/cancel/output-limit terminations, and verify owned objects absent. Miri covers cancel, EOF and source revocation with joined cleanup. |
| Disk, memory and output bombs | `rust_calibration::tests::resource_limits_are_actually_enforced`; calibration timeout/overflow cases inside `observed_descendants_are_cleaned_on_timeout_cancel_and_overflow`; actual libtest timeout/cancel/overflow in `actual_test_runtime_containment_and_descendant_cleanup`. | `25ed…` | Selected. The resource fixture exercises the bounded tmpfs/rlimits and the two process tests distinguish termination causes. The dedicated nextest `hostile_output_flood_is_bounded_and_reported_as_output_limit` is stronger for nextest but is not selected. |
| Real deny/plugin behavior and source confusion | `security_native::m4_deny_native_text_licenses_and_bans_are_real_and_cleanup_is_joined`; `security_graph_native::captures_workspace_dependency_graphs_through_the_gateway`; `security_native_adversarial::m4_deny_adversarial_oracles_preserve_cleanup_and_inputs`; `security_metadata::tests::declared_license_cannot_replace_captured_text_and_original_source_is_unchanged`; `security_metadata::tests::escapes_missing_files_and_forged_graphs_are_rejected_before_deny`; `security_metadata::tests::duplicate_json_missing_lock_and_changed_license_bytes_have_distinct_oracles`. | Native deny/graph/adversarial tests use `95dd…`; metadata tests are host unit tests. | Native cases cover license text, ban/source rules, a real exact vendor snapshot, corrupt lock checksum, absent offline data, project exceptions, pre/during cancellation, timeout/output bounds, immutable input and cleanup. The harness mixes their image identity as described above. |
| Hostile unsafe parser | `unsafe_native::m4_scanner_native_oracles_preserve_partial_results_inputs_and_cleanup` plus unit cases in `unsafe_scan.rs`. | `25ed…` | Selected native test distinguishes comments/strings, cfg/macro limits, workspace/vendor origin, all modeled unsafe kinds, invalid UTF-8, parse failure, 1 MiB boundary, scanner crash, global file budget, partial output, immutable inputs, and cleanup. |
| Miri classification and hostile producers | `miri_native::m4_miri_gateway_classifies_native_oracles`; `miri_native::m4_miri_rejects_native_producers_and_joins_timeout_and_cancel`. | `25ed…` | Selected. Native fixtures include clean/cfg-Miri, UAF, uninitialized access, aliasing, race, unsupported FFI, forged UB text, compile failure, empty and ignored suites, timeout, plus denial of build scripts, proc macros, custom harnesses, Cargo config and nested toolchains. |
| Catalog signature/hash/sequence/source poisoning | `bundle::tests::authenticates_real_sqlite_and_enforces_sequence`; `bundle::tests::rejects_signature_hash_identity_and_trust_errors`; `bundle::tests::rejects_signed_manifest_publisher_and_channel_mismatches`; `bundle::tests::rejects_noncanonical_signed_manifest_and_unknown_schema`; `bundle::tests::signed_catalog_tampering_and_provenance_mismatch_fail`; `stdio::catalog::supply_tests::tampered_signature_payload_hash_and_sequence_are_all_unavailable`; `project-adapter/tests/catalog_store.rs::floor_is_independent_bounded_durable_and_never_promoted_from_staging`; `floor_record_and_staging_reject_links_and_oversized_bytes`. | Host unit/integration tests; no Docker image. | Direct positive and negative controls exist for signature, payload hash, publisher/channel/source identity, sequence/floor rollback, archive shape and real SQLite authentication. These are not explicit selections in `test-m4-runtime.py`; the normal Rust gate must carry their evidence. |
| Semantic model poisoning | `catalog-adapter/tests/hybrid.rs::snapshot_schema_and_complete_model_identity_are_checked_before_inference`; `malformed_embedding_never_reaches_index`; `duplicate_unknown_excessive_or_invalid_distance_candidates_fall_back_atomically`; `successful_stub_retrieval_deduplicates_and_rehydrates_only_sqlite_facts`. | Host integration tests; no Docker image. | Negative model/schema/vector controls preserve SQLite as authority. They are outside the explicit 19-test runtime harness. |
| Broad/expired suppression and partial/stale facts | `application/tests/security.rs::suppression_requires_exact_engine_rule_package_source_and_version`; `exact_suppression_preserves_original_and_does_not_repair_missing_audit_data`; `policy_expiry_during_any_engine_stage_rejects_publication`; `stale_or_unknown_audit_cannot_pass_a_clean_deny`; `audit_or_deny_omissions_never_false_pass`; `application/tests/quality_v2.rs::partial_required_stage_never_produces_a_passed_gate`; `missing_vendor_or_policy_makes_the_deny_stage_unavailable_and_gate_incomplete`. | Host application tests; no Docker image. | Exact owner/rule/package/source/version matching, expiry at every engine stage, and incomplete/stale non-pass are discriminated. |
| Owner/source revocation and publication | `application/tests/security.rs::durable_deny_rejects_revocation_and_policy_expiry_during_publication`; `durable_supply_uses_one_capture_audit_deny_and_facts_then_revalidates_owner`; `supply_rejects_owner_revocation_and_publication_failure`; `application/tests/quality_v2.rs::publication_error_and_revocation_after_publisher_revalidation_never_return_evidence`; `security_runtime::m4_miri_tasks_cancel_eof_and_revocation_join_before_releasing_authority`. | Host tests plus Miri lifecycle on `25ed…` | Owner/source revalidation prevents late publication and the native Miri case joins cleanup before authority is released. A distinct altered/disabled plugin admission case that preserves an already published audit was not found. |
| Artifact secret redaction and ownership | `artifact-adapter/src/tests.rs::overlapping_nested_and_adjacent_matches_all_chunk_sizes`; `matches_cross_4096_boundary_and_keep_flags`; `binary_patterns_and_stored_hash_metadata`; `streaming_matches_independent_whole_buffer_oracle_at_all_short_cuts`; `upstream_truncation_is_preserved_and_hashes_only_stored_redacted_bytes`; `private_redact_rejects_empty_secret_before_reading_input`; `project-adapter/tests/quality_artifact_store.rs::owner_binding_separates_state_root_uid_and_granted_root`; `a_hardlinked_or_shortened_blob_is_never_served`; `a_planted_symlink_or_non_regular_object_is_quarantined_not_followed`; `owner_and_global_quotas_reject_before_the_gateway_and_evict_nothing`. | Host unit/integration tests; no Docker image. | Strong byte-level, truncation, rollback, ownership, no-follow and quota controls exist. The normal Rust gate must carry these because the runtime script does not select them. |
| M4 wire/resource canary | `security_runtime::m4_project_output_canaries_are_absent_from_security_results_and_resources`; `deny_native_mcp_tasks_policy_licenses_and_owner_bound_redacted_resource`; `m4_tools_native_mcp_observations_composition_and_private_resources`. | Canary/composition use `25ed…`; deny MCP uses `95dd…` | Selected. A canary emitted by hostile test output is absent from five M4 tool responses and private resources; forged `Undefined Behavior` text does not become a Miri UB finding. Deny also checks owner-bound resource revocation. |
| Opaque coverage HTML and source immutability | `coverage_runtime::hostile_html_is_retained_only_as_opaque_archive_bundle`; `mutation_runtime::host_source_and_canary_are_unchanged_after_every_mutation_run`. | `25ed…` | Selected, but the HTML test asserts only that JSON/LCOV/HTML bytes exist after a successful run. It does not itself assert secret redaction, content type safety, or absence of host extraction. The mutation test proves host bytes/canary unchanged, not that a secret is absent from a diff artifact. |

## Prepared controls pending final execution

The static gaps above now have concrete oracles, but these additions are not
credited until the final gate executes them:

- `test-m4-inventory.py` verifies six executable hashes and the complete Miri
  sysroot from the immutable image without starting any image code.
- `test-m4-tampered-plugin.py` replaces cargo-deny in a never-started container,
  commits its changed bytes to an untagged fixture image and requires production
  admission to reject the resulting digest. The original image is rechecked.
- Shared containment checks attempt `169.254.169.254:80` and require EPERM.
- `host_canary_cannot_enter_html_diffs_logs_or_diagnostics` executes an attempt to
  read a host-only token; actual coverage HTML, mutation diffs and process streams
  must exclude it. An authorized source canary must be present in HTML and a real
  generated diff as a positive contamination control. This explicitly does not
  claim arbitrary source-secret detection: M3 retains those private artifacts.
- The five-tool wire test requires an observed ordinary Miri test failure with
  the exact hostile test name, no compile failure and no forged UB, and reads
  every published artifact. Other non-executing tools must pass their controls.
- The lifecycle case waits for absence of owned objects after revocation/cancel
  before sending EOF. EOF is tested separately.
- The deny MCP case withdraws the configured policy, rejects the next admission
  and rereads the prior private artifact under its original authority.

The normal gate covers catalog/model poisoning and artifact authorization;
the complete M3 runtime suite retains its stronger nextest/mutation abuse cases.
The final receipts must bind both groups, without attributing M3-only native
selections to the M4 image. Public security docs retain the allocator,
privileged-host, ACL and unknown-secret limitations.

## Interpretation boundary

Passing the selected native tests would demonstrate the stated fixtures on their
effective images. It would not establish universal absence of UB, unsafe behavior,
secrets, supply-chain compromise, sandbox escape, or host compromise. M4-06 also
requires the normal unit/contract/integration gate, client evidence, distribution
provenance, and independent review identified by G1–G9; none is established by
this static map.

FILE docs/adr/ADR-067-security-policy-and-quality-contracts.md
# ADR-067 — Policy de seguridad, audit compartido y perfiles nuevos (D19)

Fecha: 2026-09-07.

## Status

Accepted e implementado internamente. Deny y gate v2 tienen pruebas nativas;
la publicación de las tools depende todavía de clientes, mediciones, revisión y
gate conjunto. Los detalles de salida/exit de cargo-deny 0.19.7 están contrastados
con la imagen de ADR-066 y oráculos registrados.

## Context

`rust.dependencies.audit` ya tiene un matcher RustSec y un snapshot propio con
freshness. `rust.quality.gate` acepta únicamente `fast` y `standard`, con enums
y snapshots M1 congelados. Añadir variantes a ese contrato cambia sus respuestas
y el significado de clientes existentes. Los jobs M3 y su store privado ya
resuelven admisión, cancelación, retención y Resources.

El cargo-deny 0.19.7 fijado intenta preparar crates incluso con `--metadata-path`.
Su inferencia de licencias prefiere el campo de metadata a los archivos de texto.
Usarlo sin una integración explícita incumpliría la evidencia offline requerida
por M4. El código oficial fijado y sus hashes se conservan en
[sources.json](../validation/m4-deny-research/sources.json).

## Decision

### Contratos independientes

M4-01 añade `rust.deny`. M4-05 añade `rust.quality.gate.v2` con perfiles cerrados
`strict` y `release`. El nuevo nombre es necesario para el contrato de perfiles
que exige el plan; no sustituye ni retira `rust.quality.gate`. Se conservan
literalmente las trece tools M1, incluyendo `QualityProfile`, `QualityStage`,
defaults, anotaciones y semántica de resultados. Los snapshots preexistentes se
comparan contra la base M3. La quinta tool nueva corresponde exclusivamente a
este contrato de composición; no es una vertical adicional ni habilita M5.

Inputs de deny: `project_ref`, `execution_mode` y `timeout_seconds` acotados.
El host aporta la policy, el runtime y el vendor: el peer no puede proporcionar
paths, TOML libre, comandos, flags, exceptions de cargo-deny ni nuevas fuentes.
Los DTOs propios son cerrados y se derivan de tipos Rust cuando sea viable.

Inputs de gate.v2: project, profile, execution mode, timeout y baseline ProjectRef
obligatorio para release y rechazado para strict. Mutation es opt-in, con su
selección/budget cerrado de M3 y validación del presupuesto total antes de admitir.
No se añade un parámetro de versión a las llamadas existentes.

### Captura y motores

La aplicación captura el source actual una vez y revalida su autoridad durante
las etapas y antes de publicar. Captura también una sola fuente vendor protegida,
una policy inmutable y un snapshot RustSec. La metadata frozen completa de esa
fuente conserva un fingerprint propio, paths ligados a source/vendor e identidades
de paquetes; no se acepta metadata proporcionada por el cliente.

`DependencyAuditPort` sigue siendo el único motor de advisories. Deny recibe su
`AuditObservation` sin modificarla y ejecuta únicamente licenses/bans/sources en
cargo-deny. La composición comparte esa observación ya calculada con supply chain
y gate.v2. No llama tools MCP, no ejecuta cargo-audit/cargo-deny advisories ni
refresca bases. La prueba de composición debe contar una llamada al port y
comparar la observación con audit standalone sobre los mismos inputs/reloj.

Para cargo-deny se materializa una copia derivada y identificada de la metadata:
`packages[*].license` se establece a null para exigir la inspección de los archivos
reales. Se preserva la declaración original como hecho separado, sin tratarla
como evidencia de texto. Antes de esa derivación, cada manifest/source/license-file
se valida contra el inventario exacto de source o vendor. Una referencia fuera
del paquete/source autorizado, archivo ausente o datos corruptos impide una
calificación completa. No se modifica el source capturado ni el proyecto host.

La configuración generada por el adapter usa nombres/args cerrados: `--frozen`,
`--offline`, JSON, config y metadata en paths constantes, sin inclusión de graphs
de texto y `check licenses bans sources --disable-fetch`. Cargo home se reconstruye
con el directory source de ADR-055 y offline obligatorio; Docker aplica network
none y seccomp calificado. Ni `--disable-fetch` ni metadata sustituyen enforcement.
Se incluyen dependencias normal/build/dev, todos los miembros y la selección de
features declarada en el resultado. No se excluyen silenciosamente crates privados.

Licencias se evalúan por texto con confianza fija 0.95, sin clarifications ni
exceptions libres. Las licencias aceptadas proceden de la policy del host; la
ausencia de esa lista nunca selecciona una policy legal por conveniencia.
Unknown/unlicensed y cobertura incompleta permanecen visibles. La combinación de
licencias detectadas usa la semántica conservadora del plugin, documentada como
limitación; no constituye una aprobación legal ni certificación de seguridad.

### Policy y suppressions

El host proporciona un documento cerrado versionado y SHA-256 esperado. Su lectura
usa el adapter de snapshots no-follow; el digest vincula los bytes que se parsean,
sin una reapertura posterior. El tamaño máximo de policy es 64 KiB, con hasta
128 licencias, 128 bans y 128 suppressions. El runtime no importa `deny.toml` ni
exceptions del proyecto; el proyecto no puede reducir las restricciones del host.

La versión fijada busca automáticamente `deny.exceptions.toml`,
`.deny.exceptions.toml` y `.cargo/deny.exceptions.toml` en ancestros del manifest,
incluso usando `--config`. Antes de ejecutar se rechazan esos nombres en todo el
source capturado; la imagen calificada tampoco puede contenerlos en los ancestros
de `/source`. El fixture adverso debe probar esta búsqueda real del plugin.

Source, vendor y datos generados ocupan tres volúmenes efímeros distintos. Los
tres se escriben únicamente en su fase de ingestión y quedan read-only durante
metadata y deny. El tercero contiene policy, metadata derivada y Cargo home con
config offline/directory source generada por el producto. Metadata frozen se
ejecuta antes de ingerir su copia derivada. Los fingerprints vinculan por separado
source original, vendor, policy y metadata original/derivada. Se reutiliza el
Execution Gateway y sus helpers de lifecycle; no se usa la resolución mutante M2
ni se permite crear/modificar Cargo.lock para hacer pasar una inspección.

Reglas: lista explícita de licencias, bans por paquete/rango, y tratamiento cerrado
de duplicate versions/wildcards. Sources desconocidas y Git no aprobado se deniegan.
Una ampliación posterior de fuentes exige su propia evidencia; el catálogo no
actúa como registry ni permite resolver por red.

Cada suppression contiene un ID, motor/source, regla exacta, paquete exacto,
rango de versiones no universal, reason, owner y expiry UTC. Además vincula el
digest de las reglas base. Para evitar un digest circular, `rules_digest` se
calcula sobre la representación canónica versionada de las reglas sin suppressions;
el SHA esperado por el host cubre el documento entero, incluidas las suppressions.
Los límites de strings y rangos se validan antes de efectos. ID repetido, rango
inválido/universal, owner/reason vacíos, regla global, digest distinto o
`expiry <= now` rechazan la policy completa. Una suppression de advisory solo
coincide con su ID concreto y nunca altera `AuditObservation` M1.

El resultado conserva el finding original, su severidad y su disposición
`active`/`suppressed`, más ID/owner/reason/expiry/digest de la suppression aplicada.
No elimina findings ni omisiones. Puede indicar policy satisfecha con suppressions,
pero nunca denomina ese estado "clean". Suppressions no convierten faltas de datos,
integridad, errores de parser, stale o timeout en éxito.

### Resultados, límites y oráculos

Se distinguen ejecución, cobertura y policy. Cada subcheck tiene origen, versión,
fingerprint de config/dataset, fecha y estado. Findings llevan regla, motor,
paquete/origen, severidad y disposición; como máximo 128 visibles y 512 KiB para
la respuesta MCP completa. Conteos y omisiones corresponden al conjunto completo,
y los datos retenidos usan las Resources privadas M3 después de redacción.

El parser exige una única summary JSON final con exactamente licenses/bans/sources,
sin advisories; rechaza duplicados, formato desconocido, truncación y discrepancia
entre conteos/errores/exit bitset. Los logs del plugin son entrada hostil. No se
confía en `exit=0` solo ni se acepta un summary falso después de un error de datos.
Solo los códigos y shapes contrastados con 0.19.7 habilitan parse completo.

Deny tiene deadline predeterminado 120 s y máximo 120 s. Auto/synchronous/task
reutilizan la selección de M3: sin Tasks negociadas solo se admite una selección
síncrona calificada de hasta 60 s. JobExecutor y el permit único de ADR-030 se
mantienen; poll/cancel no consumen el worker y Cancelled espera cleanup/join.
Los límites de source/vendor y artifacts son los ya establecidos en ADR-031/055/061.

Fixtures mínimos antes de Done: limpio con LICENSE real; manifest MIT sin texto
no-pasa; texto no permitido; ban por versión; source no aprobado; duplicate;
advisory real igual a audit standalone; stale/unknown snapshot; suppression exacta,
expirada/malformada; license-file escapado; metadata/summary corrupta; overflow;
cancel/timeout y revocación con cleanup. Se usan goldens del guest y mutaciones
que hagan fallar el oráculo. Las latencias se miden 30 cold + 30 warm con muestras
crudas y percentiles; los límites anteriores no son predicciones de rendimiento.

### Strict y release

`strict` ejecuta format/check/clippy/test/audit/deny/coverage sobre la captura
actual única. Audit se evalúa una vez; la etapa deny referencia su observación.
`release` añade SemVer con baseline separado, autorizado/capturado y revalidado
como en ADR-062. Mutation no se ejecuta salvo opt-in y con baseline obligatorio
de M3. Todos los motores estándar se ejecutan con el estable de la misma imagen
M4 calificada; Miri usa su nightly separado y conserva su tool independiente.

El resultado original de la etapa audit conserva su semántica M1 también en
strict/release: una suppression de deny no convierte un audit fallido en pasado.
La policy usa el parser puro `semver` ya fijado en Cargo.lock para rangos y
versiones, sin introducir una implementación alternativa ni adquirir dependencias.

Se publican todas las etapas requeridas, incluidas no ejecutadas y su razón.
Un required skip/unknown/partial/unavailable/timeout nunca equivale a passed.
Un fallo de validación es un resultado de la tool; una pérdida de authority o
cleanup incierto conserva las reglas de bloqueo/cuarentena. No se mezcla un
runtime distinto silenciosamente ni se captura otra versión del proyecto entre
etapas. El default global es 300 s, máximo 3600 s, limitado además por la suma
de presupuestos declarados y las reglas de admisión M3.

## Alternatives considered

- Ampliar el enum M1: cambia un contrato congelado y los clientes existentes.
- Invocar audit y deny standalone desde MCP para componer: recaptura el proyecto
  y duplica el motor/advisories, rompiendo la comparación de evidencia.
- TOML deny del cliente/proyecto: permite ampliar excepciones, rutas, fuentes y
  alcance sin autoridad host; la configuración se genera desde tipos validados.
- Aceptar `license = "MIT"` como evidencia suficiente: el fixture sin texto lo
  refuta y contradice el requisito de source bytes del plan M4.
- Suppression global o sin caducidad: hace invisible la pérdida de cobertura.
- Otro scheduler/store: duplica las fronteras M3 ya calificadas sin necesidad.

## Consequences

El nuevo contrato de composición añade una quinta tool M4 al discovery solo cuando
esté implementada y probada. Las cuatro tools de seguridad del plan permanecen
separadas por pregunta. La policy es configuración administrativa optativa del
host, con coste de preparar fuentes de licencia offline verificadas; no añade
instalaciones al core ni altera los defaults M1.

Rollback deshabilita la configuración/capability M4 y vuelve al binario anterior
tras EOF/cleanup, preservando artifacts compatibles, journals M2 y floors. Los
contratos nuevos se pueden retirar en el rollback sin reinterpretar los M1.
El store quality v1 conserva su formato; cualquier cambio necesario a persistencia
se decide y prueba aparte antes de modificarlo.

## Sources

- [Plan M4](../roadmap/m4-security.md), D19 y G1–G9.
- ADR-030, ADR-038, ADR-040, ADR-055, ADR-060, ADR-061, ADR-062, ADR-066.
- [Cargo-deny check](https://embarkstudios.github.io/cargo-deny/cli/check.html)
  y [config](https://embarkstudios.github.io/cargo-deny/checks/cfg.html), contrastados
  con el código del commit `759a4946dcfe93a56fb42d464e193c4c448af4e3` (0.19.7).
- `src/cargo-deny/check.rs`, `common.rs`, `stats.rs`, `src/licenses/gather.rs`
  y `src/diag/grapher.rs` en el [inventario upstream](../validation/m4-deny-research/sources.json).

### Publicación y compatibilidad del artefacto (integración M4-01)

El artefacto de deny es un informe JSON normalizado por el compositor integrado,
no una copia del stderr del plugin. Conserva SHA-256, tamaños y truncation de los
streams originales, reglas/severidades/paquetes verificados, conteos, disposition
y omisiones. Elimina todos los mensajes, labels, notes y campos libres del guest;
no se promete detectar secretos arbitrarios mediante patrones. Los datos de
identidad de paquetes y suppressions autorizadas por el host siguen siendo datos
sensibles al proyecto, accesibles solo con su autoridad viva.

El productor del artefacto usa `PluginIdentity::Builtin` versión 1 y digest del
normalizador; cargo-deny mantiene su identidad exacta independiente en el
resultado y fingerprint de ejecución. Se reutilizan ToolLog/Utf8LogV1 y el store
M3 sin añadir variantes a su formato persistido. Un binario M3 puede leer o
reconciliar esos descriptores durante rollback. Retención ausente, cuota o fallo
de publicación no se convierten en evidencia completa. Los bytes originales se
descartan después de publicar y nunca llegan al registry de Tasks.

La policy requiere flags de host `--security-policy` y
`--security-policy-sha256` juntos; MCP no acepta rutas ni overrides de policy.
La captura offline del vendor usa el par de flags M2 existente; si falta no se
anuncia una evaluación completa, también para proyectos sin dependencias. El adapter D05 vigente exige al menos
un paquete en un dataset válido; no se anuncia soporte de dataset vacío. Esta
restricción operativa es explícita y no implica que paquetes vendor no usados
entren en el grafo evaluado. La tool
permanece fuera del inventario público hasta completar su calificación y G4.

El DTO deny resume audit mediante estado, issue, completitud, provenance/freshness,
fingerprints y conteos. Los findings aparecen una vez en la lista combinada;
la `AuditObservation` completa permanece inalterada en aplicación para otras
composiciones. Este DTO nuevo no modifica el contrato audit M1. El presupuesto
512 KiB mide el `CallToolResult` serializado completo, incluido el espejo textual.
Si hay que quitar filas, incrementa omisiones y convierte el resultado en parcial;
un resultado recortado conserva el Resource autorizado y nunca pasa.

### Proyección de gate.v2

El gate nuevo conserva los defaults exactos de format/check/clippy/test de standard.
Coverage usa el workspace con features por defecto y sin doctests; SemVer aplica
la selección por defecto idéntica a ambos lados. La salida declara cada selección.
El agregado conserva estados, conteos, métricas, fingerprints y una proyección
normalizada de audit/deny. Su Resource contiene esa misma evidencia acotada, sin
streams crudos, texto de código, HTML ni diffs de las etapas. Las tools individuales
conservan sus propios artifacts de reparación y contratos existentes.

La completitud de un veredicto SemVer se decide por la clasificación M3 calificada,
independiente de su lista auxiliar de findings best-effort, que sigue declarándose
partial. No se cambia M3 ni se presenta esa lista como exhaustiva. Una clasificación
incomplete/uncalibrated sí bloquea release. Coverage sin denominadores ejecutables
no pasa el gate nuevo, aunque la tool individual pueda informar una ejecución vacía.

El timeout global abarca todas las etapas y publicación. Ninguna etapa recibe más
que el presupuesto global restante. Para mutation opt-in se exige, antes de
capturar, que su presupuesto derivado más 300 s reservados a las otras etapas
quepa en el timeout solicitado (máximo 3600 s). Sin mutation, el default sigue
siendo 300 s. Se conserva una fila requerida con razón cuando un motor no está
disponible; cleanup incierto o autoridad perdida abortan la publicación.

FILE docs/adr/ADR-069-isolated-unsafe-syntax-scanner.md
# ADR-069 — D20, scanner AST aislado por archivo

## Status

Accepted como diseño M4-02. Implementación y calificación pendientes.

Enmienda por revisión Opus: protocolo helper v2 antes de anuncio público.
La captura v1 revisada se conserva como evidencia histórica.

## Context

El scanner debe distinguir sintaxis real, strings/comentarios y macros sin ejecutar
código para expandirlas. Un AST recursivo en el servidor MCP permitiría que un
archivo hostil agotara su pila, incluso con 1 MiB de entrada y llaves poco profundas
(por ejemplo una cadena de operadores unarios). `catch_unwind` no contiene un
stack overflow. La captura limita 4096 entradas, 16 MiB total y 1 MiB por archivo.

La revisión de las fuentes oficiales fijadas en lock/cache confirma recursión en
`syn::buffer::TokenBuffer`, parser y Visit; `proc_macro2::FromStr` también documenta
posibles panics. Las dependencias ya adquiridas son syn 3.0.4
(checksum `e6275cddf4610d1775e6d1fe9469b2e77d0f39fd98fb7450901b821e0c53649f`)
y proc-macro2 1.0.107
(`985e7ec9bb745e6ce6535b544d84d6cd6f7ad8bd711c398938ae983b91a766d9`).

## Decision

Un helper privado dentro del runtime utiliza syn 3.0.4 `full,parsing,visit` y
proc-macro2 1.0.107 `span-locations`; sin nuevas versiones ni adquisición en runtime.
El supervisor inicia una copia del helper por archivo de forma secuencial. Esta
es una fase tipada del Execution Gateway: mismo ejecutable absoluto, un índice
validado de un manifiesto creado por el host, sin paths/flags del cliente ni shell.
El supervisor no parsea Rust. Cada hijo tiene un límite de 2 segundos; el gateway
impone el deadline global 1..120 s, memoria/CPU/PIDs/output y cleanup de todo el árbol.
Ningún caso hostil de parser se ejecuta sobre el host.

El manifiesto enumera determinísticamente archivos `.rs` capturados, con índices,
paths relativos autorizados, origen, paquete y fingerprints. El resultado interno
usa índices y spans de keyword, nunca texto libre de errores del parser. El host
valida rango, UTF-8, bytes `unsafe`/`extern`/atributo esperado, conteos, identidad y
límites. Una muerte/señal/parse error/timeout deja cobertura parcial y no elimina
resultados de otros archivos. El hijo conserva el AST hasta terminar el proceso,
evitando su drop recursivo; memoria queda contenida por el guest. El supervisor
continúa después de un archivo fallido, sin retry automático.

Se reportan unsafe block/fn/impl/extern block, extern block/fn/crate y las formas
modernas `unsafe mod` y `#[unsafe(...)]`, con span exacto del keyword. Los cuerpos
de macros son opacos y no cuentan como código expandido; se informa su omisión.
Los atributos cfg/cfg_attr propios o heredados hacen el hallazgo condicional;
no se evalúan. Línea/columna son 1-based, columna por caracteres UTF-8; bytes 0-based.
Se declaran `cfg_evaluated=false`, `macros_expanded=false` y
`generated_sources_scanned=false`. Ningún cero de findings significa seguridad.

La asignación a workspace/dependency usa raíces de paquete autenticadas de la
metadata congelada. Archivos capturados no asignables se conservan como
workspace_unowned; cobertura de dependencias requiere el dataset vendor esperado.
El resultado retiene 128 findings y conteos de omisión; máximo 512 KiB. No se usan
findings de unsafe como policy universal ni como veredicto de UB.

La tool recibe únicamente ProjectRef, timeout 1..120 (default 120) y modo de
ejecución. Reutiliza Tasks, worker único, captura y store M3. No requiere una
policy de deny ni ejecuta auditoría. El dataset vendor esperado sigue siendo
explícito del host. Metadata se obtiene con Cargo frozen/offline, sin compilación,
sobre los mounts existentes `/source`, `/rust-mcp-vendor` y `/security`.
La fase final del mismo gateway selecciona el helper absoluto y un manifiesto
`/security/scan.json` generado tras validar metadata. No hay expansión de macros.

Las raíces autenticadas asignan cada archivo a la raíz de paquete más larga;
los archivos vendor de paquetes fuera del grafo no forman parte de la selección.
El límite de 4096 archivos seleccionados abarca ambos orígenes. Los restantes
cuentan como omitidos y hacen parcial la cobertura. La respuesta agrega conteos
por origen y por resultado de archivo; cada finding conserva path relativo,
paquete, hash y span. La ausencia de findings con errores de archivo nunca produce
un resultado completo. El artifact contiene esta proyección normalizada, sin
código ni stderr del helper, y su productor Builtin se liga al digest del normalizador.

### Enmienda de integridad y presupuesto del helper

El manifiesto v2 añade `budget_ms` (1..118000) generado por el gateway desde su
deadline restante, reservando 25 s para controles, lanzamiento, devolución y cleanup. El
supervisor deja de iniciar archivos cuando agota ese presupuesto, conserva lo ya
analizado y marca los restantes `budget_exhausted`. Cada hijo recibe como máximo
2 s y el tiempo restante; el drenaje también es acotado. El deadline global y
cancelación del gateway siguen prevaleciendo: un kill previo a la emisión nunca
publica un resultado completo. El lector puede quedar desprendido solo al cerrar
el supervisor, sin admitir más hijos después de un drenaje no confirmado.

El protocolo distingue `invalid_utf8`, `too_large`, `budget_exhausted`,
`parse_error`, `unavailable`, `crashed` y `timed_out`. Los conteos individuales
se limitan por el máximo de bytes fuente; no se aceptan contadores u64 arbitrarios.
Los resúmenes por archivo usan claves compactas `i,s,total,omitted,macros,opaque`
para admitir 4096 filas dentro de 512 KiB aun con todos los contadores al máximo.
El host valida la misma gramática. Las primeras 128 filas de findings siguen
prioridad de archivo; las omisiones nunca se ocultan.

Se incorporan `unsafe_trait` y `unsafe_static`. `cfg_attr` anidado se inspecciona
sintácticamente solo para atributos unsafe, con `conditional=true`, sin evaluar
predicados ni expandir macros. Los nodos Verbatim cuentan como
`opaque_syntax_omitted` y hacen parcial la cobertura. Se extiende la herencia cfg
a todos los portadores de atributos recorridos por el visitor, incluidos patrones.

El binding del manifiesto y las fuentes se apoya en volúmenes inmutables: solo
el extractor confiable los escribe antes del scanner; sus mounts son RO para
supervisor e hijos, sin otros writers ni mounts host. No existe un escritor que
pueda modificar el manifiesto entre lecturas. Se rechazan paths duplicados y solo
se extraen archivos regulares; FIFO/symlink no pertenecen a la entrada admitida.
Las pruebas del gateway deben acreditar esas premisas, además de los oráculos
puros del decoder y del lector acotado del helper.

## Alternatives considered

- Regex/lexer propio: insuficiente para la sintaxis, macros y errores de Rust.
- syn en el MCP con guarda de llaves/catch_unwind: no controla la recursión real.
- Un único hijo para todo el proyecto: un archivo hostil elimina resultados ajenos.
- Expansión de macros mediante compilación: ejecución adicional fuera de este corte.

## Consequences

Un proceso por archivo tiene coste que debe medirse, sin afirmar latencia antes de
30 cold/30 warm. El helper es un nuevo asset: exige SBOM/licencias, build offline,
digest, imagen nueva y recalibración; ADR-068 no autoriza su sustitución silenciosa.
El runtime Miri ya preparado y deny permanecen inmutables hasta esa imagen derivada.
Rollback vuelve al digest previo y retira la tool sin migraciones de persistencia.

### Reserva de control y drenaje después de revisión v3

La reserva del gateway se desglosa antes de fijar el budget del helper: hasta 37
llamadas de control restantes con una asignación operacional de 250 ms (9250 ms),
2000 ms para arranque frío, 1000 ms para serialización/validación final y 10000 ms
para cleanup. Se redondea hacia arriba a 25000 ms. Con menos reserva disponible se
devuelve Timeout antes de lanzar el parser; no se convierte en InvalidMetadata.
Los 250 ms son una asignación que la calificación de latencia debe medir, no una
promesa sobre disponibilidad del daemon. Un daemon que no responda dentro del
presupuesto global exige cierre sin publicación y conserva la regla fail-closed.
El oráculo nativo de agotamiento global debe preservar prefijos y retornar dentro
del deadline con cleanup confirmado; se medirá además el margen de control.

Tras reap del hijo, el supervisor espera al menos 20 ms por EOF, cargados al reloj
global. Si el drenaje no se confirma, marca las filas restantes unavailable; reserva
budget_exhausted para agotamiento real. El builder conserva un inventario de los
11 crates cached del lock privado; no hay nueva adquisición en este helper.
Docker puede representar argumentos vacíos como Cmd:null: se normaliza únicamente
ese valor explícito a lista vacía y se compara contra el argv cerrado de cada fase.
Un campo Cmd ausente o argv diferente sigue siendo rechazado.

La identidad de ejecución liga también `unsafe_scan.rs` y `unsafe_port.rs`,
incluidos el parser y la validación host de completitud. La identidad de imagen
fija los bytes del helper. Un cambio de cualquiera invalida los recibos previos.

FILE docs/adr/ADR-071-supply-chain-facts-without-catalog-migration.md
# ADR-071 — D22, facts de supply chain con fuentes independientes

## Status

Accepted para M4-04. Implementación y calificación pendientes.

## Context

El catálogo SQLite schema 1 conserva yanked y features conocidas, pero no registra
el source ni checksum completo de cada paquete. Cargo.lock capturado sí puede
registrarlos. Mezclar ambos sin indicar la fuente convertiría una ausencia del
catálogo en una afirmación falsa. Git y registries ajenos no tienen fuentes offline
calificadas para el motor deny inicial; sus facts siguen siendo útiles.

## Decision

No hay migración SQLite ni nueva adquisición. El catálogo y sus antirollback,
firma, sequence y freshness permanecen en los ports existentes. La tool compone
una captura ProjectRef, una auditoría RustSec, una ejecución de deny cuando sus
inputs estén disponibles y facts del lock/metadata obtenidos por el adapter.
No ejecuta tools MCP desde aplicación. Cada motor conserva su estado: una falla
de deny no elimina audit ni los facts capturados; unknown/partial no produce pass.

Cada paquete conserva nombre, versión, clase de source, digest del source literal,
checksum si está registrado, duplicados y features declaradas frente a activas.
El locator Source/URL se omite por completo del resultado y artifacts; solo se
expone su clase y hash de bytes literales, sin userinfo, query ni fragmento secreto;
no se resuelve por red. Lo observado en lock no se presenta como verificación de
bytes: solo el dataset vendor autenticado puede acreditar el checksum del paquete.
Las features activas vienen exclusivamente de metadata congelada calificada;
cuando no existe, permanecen unknown. No se infieren de features del catálogo.

Yanked se consulta por nombre y versión exacta a una sola generación autenticada
del catálogo, con fingerprint/sequence/provenance/freshness. Version/crate ausente,
catálogo no disponible y no consultado por límite son unknown distintos. Las
referencias a advisories del catálogo nunca sustituyen el matcher RustSec.

Budget: trabajo Tasks 1..120 s (default 120), grafo hasta 4096 paquetes, 128 filas
visibles y 128 consultas de yanked por orden determinista. Filas/findings/consultas
omitidas se cuentan; toda reducción por el máximo de 512 KiB del resultado MCP
completo hace parcial la respuesta. El informe normalizado se publica por el store
M3, con owner/TTL/retención/provenance existentes y sin texto de diagnósticos.

La respuesta comunica facts y cobertura. No calcula un score, certificación de
seguridad ni aprobación legal. Unsafe y Miri mantienen sus preguntas independientes.

## Alternatives considered

- Migrar SQLite para replicar campos del lock: duplica autoridad sin nuevos inputs.
- Usar URLs convencionales o features del catálogo como hechos activos: inferencia
  sin evidencia del grafo ejecutado.
- Fallar todo si falta catálogo/vendor: elimina findings y facts útiles ya capturados.
- Consultar registries o Git en runtime: contradice el modelo offline y la autorización.

## Consequences

Rollback conserva los formatos persistidos y retira la tool. Deben calificarse
por separado known/unknown yanked, fuentes Git/registry, checksum declarado frente
a verificado, duplicados/features y freshness. Una futura adquisición enriquecida
o cambio de schema requiere su propio ADR y calificación; no queda autorizado aquí.

FILE docs/adr/ADR-072-miri-classification-integrity.md
# ADR-072 — D21, integridad de clasificación Miri

## Status

Accepted. El gateway sobre la imagen final pasó 12 casos de clasificación y
7 de admisión/ciclo de vida (194.27 s), incluidos binary-only, timeout del runner,
configuración hostil y cancelación observada en Miri. Evidencia:
[recibo nativo](../validation/M4-miri-native.json). La calificación conjunta y de
clientes sigue pendiente.

## Context

El Miri fijado a Rust commit `5a2be9f5f075d31e3ca5526b5b029881ce441253`
devuelve el mismo exit 1 para UB, unsupported y otros errores del intérprete.
Su clasificación está en diagnósticos. El programa interpretado puede falsificar
stdout/stderr; proc macros comparten además los streams nativos del compilador.
JSON de rustc por sí solo no autentica su productor. No se puede presentar una
clasificación confiable para cualquier proyecto Cargo con el runtime actual.

## Decision

Se implementa inicialmente un modo de integridad acotado. Antes de ejecutar Miri,
metadata congelada y manifests capturados deben acreditar que el grafo no contiene
targets proc-macro/custom-build ni harnesses personalizados. De lo contrario se
devuelve `classification_integrity_unsupported`, sin afirmar clean, UB o una
operación no soportada por el intérprete. La selección incluye pruebas libtest
del workspace; excluye doctests, benches y examples. No evalúa un runner suministrado
por el proyecto ni configura flags desde el cliente.

El gateway usa nightly-2026-09-07 y sysroot preaprovisionado de ADR-066, con paths,
image digest y binarios exactos. No invoca setup. Ejecuta `cargo miri nextest run`
con nextest 0.9.143, config del producto read-only, sin retries ni fail-fast, una
hebra y JUnit acotado. MIRIFLAGS para build/list son exactamente
`--error-format=json -Zmiri-isolation-error=abort -Zmiri-backtrace=0`.
El run-wrapper fijo `/usr/bin/env`, `target-runner="within-wrapper"`, añade
únicamente `-Zmiri-mute-stdout-stderr` durante cada test. Así libtest puede listar
pruebas y los bytes interpretados no pueden falsificar diagnósticos de ejecución.
La función experimental wrapper-scripts queda ligada al pin de nextest.

La clasificación combina terminación del gateway, estructura JUnit, exit del
runner y diagnósticos JSON de Miri del testcase. No usa los resúmenes humanos.
Se conserva más de una categoría si varias pruebas producen fallos distintos:

- Clean exige exit global 0, selección no vacía, JUnit completo/coherente, todos
  los tests seleccionados passed y ninguno skipped/failed/error/omitido.
- UB exige failure con runner exit 1 y diagnóstico error cuyo mensaje comienza
  por `Undefined Behavior:` del runtime fijado.
- Unsupported exige la misma estructura y `unsupported operation:`.
- Test failure exige la terminación normal del harness 101, sin diagnóstico
  semántico de Miri ni evidencia discordante.
- Compile failure conserva fallos build/list y post-monomorphization acreditados;
  no convierte otro fallo de infraestructura en compilación por un texto parecido.
- Timeout/cancel/límite, suite vacía, skipped, XML/JSON inválido o resultado no
  reconocido quedan explícitos y nunca clean. Leaks/deadlocks/resource exhaustion
  desconocidos conservan `unclassified` con cobertura parcial.

Work budget Tasks 300 s por defecto, 1800 s máximo; captura/metadata/verificación
forman parte del mismo deadline. El gateway limita CPU/memoria/PIDs/disco/streams,
termina y une el árbol y verifica cleanup antes de liberar el permit. El informe
normalizado contiene categorías/conteos/identidades/runtime/hashes, sin raw prose
ni contenidos fuente; 128 findings y 512 KiB de resultado MCP completo. Se publica
en el store M3 con owner/TTL/retención. Los streams originales no son Resources.

### Cierre de revisión de integridad y presupuesto

La captura nativa ya rechaza `.cargo/config` y `.cargo/config.toml` en cualquier
directorio antes de devolver un `SourceBundle`; el gateway Miri repite esa
validación para callers internos. La configuración almacenada dentro del vendor
no participa en la jerarquía Cargo desde `/source`; se comprueban los productores
de sus paquetes efectivamente resueltos, sin vetar archivos de desarrollo de
paquetes ajenos al grafo. Rechaza además configuración nextest
local y archivos `rust-toolchain` del bundle para que ningún selector de proyecto
participe en la elección del intérprete. Se ejecuta antes de crear recursos Docker.
Los rechazos son `ClassificationIntegrityUnsupported`, sin degradación de flags.

El fingerprint de ejecución incluye los bytes del clasificador y del port Miri,
además del wrapper, admisión, gateway y runtime. Se reservan 10 s del presupuesto
del job para finalizar/revalidar/exportar JUnit (la limpieza conserva su deadline
separado existente). Una petición demasiado corta falla como timeout antes de
lanzar el intérprete. Las pruebas de timeout/cancel deben observar el contenedor
Miri ejecutándose; tiempo transcurrido por sí solo no demuestra esa fase.

El parser reconoce el timeout por test del runner mediante su tipo exacto
`test timeout` y conserva la categoría Timeout con `complete=false`. Los topes de
XML, nodos e identidades se distinguen como `OutputLimit`. Los nombres se retienen
sin reescritura en un subconjunto conservador (letras/números Unicode y
`_ : $ . < > # -`, sin espacios); caracteres combinantes fuera de ese subconjunto
producen evidencia inválida, nunca un veredicto limpio.

El oráculo `binary-only` confirmó que `--lib --tests` devuelve evidencia inválida
cuando no hay librería. Se usa únicamente `--tests`, que incluye unit tests de
lib/bin e integración según [Cargo](https://doc.rust-lang.org/cargo/commands/cargo-test.html),
sin doctests, ejemplos ni benchmarks. El fixture mantiene un binario y un test de
integración para que el caso no pueda pasar por ausencia de tests.

## Alternatives considered

- Clasificar stderr del proyecto: falsificable incluso con mensajes JSON.
- Invocar directamente cargo-miri runner sobre CrateRunInfo JSON: convierte args,
  env, cwd y stdin no autenticados en autoridad; API interna no calificada.
- Admitir build scripts/proc macros y buscar patrones adicionales: no restaura
  integridad del productor.
- Declarar Miri simplemente unavailable: no implementa los positivos exigidos M4.

## Consequences

El modo inicial excluye proyectos habituales con derive/proc macros. Ampliarlo
requiere una nueva frontera y oráculos; no basta con más parsing. Nextest ejecuta
Miri por test y no observa carreras entre pruebas. Una ejecución clean acredita
solo las pruebas y configuración seleccionadas, nunca ausencia universal de UB.
La isolation de Miri no sustituye el sandbox OS del Execution Gateway.

El [oráculo empírico](../../fixtures/m4-runtime-oracles/miri-classification/README.md)
pasó clean/cfg, panic con diagnósticos falsos, UAF/uninit/alias/race, FFI, compile,
empty e ignored. Sus [recibos](../../fixtures/m4-runtime-oracles/miri-classification/results/summary.json)
no califican todavía la tool ni un gateway modificado. Los cambios del runtime,
nightly, sysroot, wrapper o parser exigen recalibración.

Fuentes oficiales fijadas: [Miri README](https://github.com/rust-lang/rust/blob/5a2be9f5f075d31e3ca5526b5b029881ce441253/src/tools/miri/README.md),
[diagnósticos](https://github.com/rust-lang/rust/blob/5a2be9f5f075d31e3ca5526b5b029881ce441253/src/tools/miri/src/diagnostics.rs),
[evaluación](https://github.com/rust-lang/rust/blob/5a2be9f5f075d31e3ca5526b5b029881ce441253/src/tools/miri/src/eval.rs),
[phases](https://github.com/rust-lang/rust/blob/5a2be9f5f075d31e3ca5526b5b029881ce441253/src/tools/miri/cargo-miri/src/phases.rs)
y [wrapper scripts](https://nexte.st/docs/configuration/wrapper-scripts/).

FILE docs/reviews/m4-security-confirmation/disposition.md
# Technical Owner disposition — confirmation snapshot

This is a scoped disposition, not M4 closure. The original external response is
preserved unchanged. Its verdict found no P0/P1 and two P2 evidence issues.

| Finding | Disposition | Evidence / remaining action |
| --- | --- | --- |
| P2-1 scanner receipt predates gateway bytes | Accepted, rerun required | The original receipt remains historical. The final source-bound full gate explicitly invokes the scanner selection and must publish its refreshed receipt before Done. |
| P2-2 scanner parser/port absent from execution fingerprint | Fixed in source | `security_gateway.rs` now includes `unsafe_scan.rs` and `unsafe_port.rs`; deny parser/port are also included for the same invariant. Final scanner/Miri reruns and confirmation bind these bytes. |
| P3-1 outer deadline shortens export reserve | Accepted limitation | The outer job deadline includes capture/vendor work and remains authoritative. Expiry fails closed and joined cleanup is mandatory. A future remaining-budget port would improve usable interpretation time; M4 does not promise a full requested duration solely inside the interpreter. |
| P3-2 overlarge JUnit archive taxonomy | Accepted follow-up | The export is bounded and never produces clean. Some oversized envelopes return InvalidMetadata rather than OutputLimit; improve the shared tar decoder's typed error without weakening validation in a later separately tested change. |
| P3-3 20 ms EOF scheduling grace | Accepted limitation | Unconfirmed drainage makes remaining files unavailable, never complete. Native corpus does not reproduce a per-file timeout; the receipt explicitly distinguishes helper IPC tests from native evidence. No unbounded wait is introduced to obtain availability. |
| P3-4 unmatched vendor files omitted | Intended scope | Only package roots authenticated as resolved graph members are selected. Unresolved packages in a larger vendor snapshot are outside the requested scan. Metadata root/graph validation is separately tested. |
| P3-5 project identities remain LLM-visible | Accepted residual | Conservative identifier syntax removes direct prose/control characters; names remain untrusted project metadata and are not instructions. Universal prompt-injection resistance is not claimed. |
| P3-6 clean coverage represented by counts | Documented limit | Tool semantics and ADR-072 limit clean to the selected tests/configuration; cfg(not(miri)) and omitted targets are not a safety proof. |
| P3-7 source Cargo config denied globally | Existing M1 boundary | This is not an M4 change. Existing capture policy and snapshots are preserved; diagnostics can be improved separately without relaxing containment. |
| P3-8 stale ADR counts/reserve | Corrected | ADR-068 now says 12/7; ADR-069 consistently reserves 25 s. |
| P3-9 application observation fields rely on trusted port | Accepted residual | Concrete adapter structurally binds capture/metadata/config; application verifies report, vendor, execution identity and JUnit presence. A malicious in-process adapter is outside the plugin threat boundary. |
| P3-10 extra absent cleanup containers | Accepted bounded overhead | At most four additional fixed daemon round trips; included in native measured cleanup. No authority or input-dependent amplification. |
| runtime constants on final image | Prepared prerequisite | `scripts/test-m4-inventory.py` reads exact image bytes without starting image code and verifies all six binaries plus the complete sysroot tree against the approved pins. Its successful final receipt remains required. |

P3 follow-ups belong to the Technical Owner's explicit backlog; they do not weaken
M4 fail-closed contracts. No review statement is substituted for native, client or
final gate evidence. The required P2 rerun/confirmation remains open here until
its receipts are linked by the final handoff.
