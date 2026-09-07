# M3 — Quality / 0.3.x

Estado: **In progress**. Entrada: cierre real M2, con su writer y gateway calificados.
Fuentes: spec §26/48/51/74–80/97 M3,
[ADR-028](../adr/ADR-028-ephemeral-artifact-store.md),
[ADR-030](../adr/ADR-030-m1-worker-admission.md),
[ADR-040](../adr/ADR-040-single-capture-quality-gate.md).
Aplican [G1–G9](m2-m8.md). Tamaño XL; sin fecha/capacidad asumida.

## Objetivo, contrato y límites de alcance

Observar calidad avanzada con tooling real y resultados parciales honestos.
Tools propuestas: `rust.test.nextest`, `rust.coverage`, `rust.semver.check`,
`rust.mutation.test`. Nuevos DTOs estrictos con selección package/features/target,
timeouts cerrados, identidad runtime/plugin/source, completeness y artifacts.
Semver recibe dos ProjectRefs autorizados, no URL/version remota ni Git ref libre.
No agregar mutation automáticamente a fast/standard ni cambiar sus resultados.
No plugin install, shell, source host writable, flags de harness libres o task
durable por defecto. No reimplementar nextest, LLVM o análisis SemVer.

## Cortes ejecutables

| ID | Flujo end-to-end | Dependencias | Oráculo/gate | Tamaño |
| --- | --- | --- | --- | --- |
| M3-01 | nextest→job admitido→gateway→JUnit/log→Resource privado | M2, D06/D17 | Pass/fail/ignore/retry/leak, ausencia tool, doc-only y cleanup activo | XL |
| M3-02 | Tool larga→negociación task→poll/cancel/expiry→resultado final | 01, D06 | rmcp exacto, IDs ajenos, sin capability, EOF, cancel/pub race | L |
| M3-03 | coverage instrumentada→métricas por paquete→merge→HTML/LCOV/JSON | 01/02, D18 | Conteos LLVM independientes, mismo run/config, denominador cero | L |
| M3-04 | Dos capturas autorizadas→semver analysis→breaking/no-break/incomplete | 01/02, D18 | API eliminada/trait/enum/features y baseline incompatible | L |
| M3-05 | Baseline tests→copia privada mutada→clasificación→artifacts/diff | 01–04 | Caught/missed/unviable/timeout, baseline fallido y source host intacto | XL |
| M3-06 | Cuatro tools→clientes/gate→inventario/docs→handoff | 01–05 | G1–G9, regresión M1/M2, Sonnet y Opus de task/persistencia | L |

Camino crítico: job/artifact con nextest→task negotiation→mutation→cierre.
Coverage y pares SemVer permiten trabajo de fixtures independiente tras D18.

## Arquitectura, tareas y Resources

Domain modela job identity/state, selectores, métricas y evidencia; application
posee task execution abstraction neutral a MCP, admisión/cancelación y ports de
cada caso. Execution adapter extiende RustCommand cerrado sobre el mismo gateway;
MCP traduce el job al mecanismo realmente negociado por rmcp. CLI detecta plugins
preaprovisionados e informa identidad/capability; no depende de PATH del peer.

D06 verifica API exacta de rmcp 3.2.0 y extensión de tareas antes de anunciarla:
[SDK](https://docs.rs/rmcp/3.2.0/rmcp/), [SEP-2663](https://modelcontextprotocol.io/seps/2663-tasks-extension).
No asumir que tareas de una revisión experimental legacy son wire-compatible.
Si cliente no admite tasks, operaciones cortas pueden responder síncronas bajo
budget; largas deben rechazarse antes de ejecutar o tener modo explícito acotado
aprobado. No convertir soporte opcional de tasks de la spec en dependencia
obligatoria sin demostrar imposibilidad del modo acotado y decidirlo en D06.
Propuesta de fallback: budget total síncrono 60 s por defecto y máximo 120 s,
sin cola ni ampliación silenciosa; D06 debe fijar valores finales tras medir el
cliente/gateway exacto y contrastar L09/L10 (bootstrap 10 s y límites stdio):
el fallback no cambia esos presupuestos ni usa una operación larga como primer request. El modo se selecciona antes de ejecutar y expiry cancela
y une el árbol; una operación que exige más tiempo se rechaza o usa Tasks.

Admisión inicial un job activo/sin cola; estado/poll/cancel no adquiere el permiso
del job. IDs opacos, owner/ProjectRef/policy revalidados, TTL fijo y sin enumeración
global. Cancelled solo después de cleanup observado; EOF cancela/join, restart no
promete reanudar jobs. Persistencia de resultados privados es distinta de ejecución
durable y autoridad del ProjectRef. Quota de SDK cancel slots M1 permanece explícita.

Artifacts ricos D17: egreso por bytes desde paths guest fijos, regulares/no links,
no archive-selected host writes, parsers bounded. Descriptor de kind/MIME/format
version/hash/size/completeness/sensitivity/TTL/source/runtime. Logs M1 mantienen
URI/retención; Resources adicionales owner-bound paginados/chunked no activan trabajo.
HTML/SVG son contenido hostil; no scripts ni recursos remotos activos en preview.
Privacidad: source y símbolos pueden contener secretos; retener/exportar requiere
permiso host. Secret scan acotado complementa redacción, no garantiza ausencia.

Presupuestos propuestos antes de fixture qualification: 32 MiB/artifact,
64 MiB/job, 128 MiB/owner, 256 MiB/global, 128 miembros/job, TTL 1 h (host hasta
24 h), 512 KiB MCP completo por respuesta. Disco exige state-root protegido y
quota/reserva; no confundir con RSS. Job default 300 s/máximo 3600 s, fases y
cleanup separados; mantener límites efectivos CPU/RAM/PID/tmpfs y fijar valores
por imagen en D06. Saturación rechaza antes de producir. No eviction de artifacts
prometidos a otro owner. Caducidad job/ProjectRef/artifacts se reconcilia al publicar.

## Oráculos por herramienta y semántica

Nextest exacto: salida estable disponible, JUnit y eventos soportados por versión;
no inferir test counts de texto humano. [Output nextest](https://nexte.st/docs/machine-readable/).
Separar selected/passed/failed/ignored/retried/leaked; sin doctests salvo etapa
distinta declarada. Setup scripts/custom profiles son ejecución de proyecto.

Coverage: LLVM/plugin exactos, conteos y porcentajes de líneas/regiones/funciones,
denominadores/exclusiones/cfg/generados/doctests explícitos. JSON autoritativo,
LCOV y HTML derivados del mismo run; Cobertura si clientes lo requieren con oracle
de conversión. Merge multi-package solo si source/target/features/instrumentación
coinciden y sin sumar dos veces archivos compartidos. Cero tests/datos no es 100%.
Baseline de cobertura identifica source/config, no solo branch name.
[cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov); CI M1 usa 0.9.0 pero esa
instalación no acredita runtime del producto: verificar release/binario exacto.

Semver: baseline/candidate capturados y revalidados en orden de locks estable,
snapshot identity distinta por root; no afirmar atomicidad entre roots externas.
Misma selección/target/rustdoc/plugin; incompatibilidad es fallo válido, herramienta
ausente o parser/version distinto es incomplete/unavailable. Calibrar exit codes
con [cargo-semver-checks](https://github.com/obi1kenobi/cargo-semver-checks) fijado;
no copiar números de otra versión. Reportar item/lint/span y limitaciones de features.

Mutation: pruebas baseline obligatorias, mutantes en copia privada mediante garantías
M2 de staging/gateway, nunca commit al host. Máximo propuesto 100 mutantes/job,
sin sharding inicial, timeout por mutante≤60 s y total≤3600 s. Outcomes/diffs con
denominador explícito, missed falla; timeout/unviable/incomplete no acreditan limpio.
[cargo-mutants outcomes](https://mutants.rs/mutants-out.html). Source y custom tests
son hostiles; spawn/red/output y edición fuera de copia deben quedar contenidos.

## Seguridad, pruebas, observabilidad y distribución

Threat delta: task ID ajeno, resultado después de revocación, artifact path/HTML
hostil, quota exhaustion, cancelación perdida, plugin sustituido, baseline falso,
fuentes mezcladas y pruebas que escriben source. G2/G3 exigen env limpio/network
deny/kill-tree; filesystem de snapshots de dependencias se incorpora por autoridad
host D05, nunca por config Cargo arbitraria. Native tests macOS/APFS+guest aprobado;
Linux/Windows portable/fail-closed hasta otro adapter.

Unit de estados/parsers/métricas; contratos de cuatro tools/tareas/resources;
protocol de cinco revisiones más extensión negociada; integration plugins reales;
security/adversarial XML entities, deeply nested JSON, external URIs, output infinito,
dos owners/TTL y active child. Fixtures con conteos conocidos y archivo repetido
entre paquetes; pares SemVer y todos los estados de mutation. Un skip no pasa.
SLI: duración por fase, task poll latency, CPU/RAM, bytes retenidos, cancel→cleanup,
omisión y plugin mismatch. Budgets medidos antes de cierre, sin SLO universal.

Provisioning explícito: versión/digest de plugin+toolchain+runtime y licencias/
notices/SBOM/provenance; source-qualified y artifact distribuido distintos.
No empaquetar quality bundle sin demanda/ADR. Rollback conserva formatos anteriores
o rechaza reader incompatible, cancela jobs y verifica cleanup, no borra artifacts
de otros owners ni restaura permisos revocados. D17 define migración de disco antes
de introducirla; el store efímero M1 no se convierte silenciosamente en persistente.

## DoR / DoD y aceptación

DoR: M2 cerrado, D06/D17/D18 y provisioning decididos, plugins exactos disponibles,
tasks spike, fixtures/budgets y privacidad aprobados. DoD: M3-01..06, G1–G9 completos,
reviews Sonnet de contratos y Opus High de lifecycle/persistencia; bug bar G8.

- [ ] Las cuatro tools ejecutan versiones exactas y resultados parciales/ausentes
  nunca cuentan como gate pasado. Fuente: spec §26/69/97 M3, ADR-006; M3-01..05.
- [ ] Un único gateway y abstracción de tareas conservan cancelación/cleanup y
  autorización de poll/results. Fuente: spec §37/51, ADR-030; M3-01/02.
- [ ] Coverage merge conserva conteos, formatos y provenance de una ejecución;
  SemVer usa baseline autorizado y compatible. Fuente: spec §26.2/26.4/104; M3-03/04.
- [ ] Mutation requiere baseline, limita costo y conserva source intacto; artifacts
  ricos privados respetan cuotas/TTL. Fuente: spec §26.3/45/77, ADR-028; M3-05, D17.
- [ ] Full/native/client/contract gates y notices/SBOM pasan sobre bytes finales,
  sin inferir distribución por herramientas instaladas en CI. Fuente: G4/G5/G7/G8.

Handoff: commit/smoke, matriz jobs/plugins/formatos, gates/reviews, límites,
recovery y fuentes; detener antes de M4.
