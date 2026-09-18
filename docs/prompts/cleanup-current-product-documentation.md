# Encargo — Limpieza profunda y documentación del producto implementado

Estado: **plan preparado; ejecución no iniciada**. Fecha: 2026-09-18.

Este prompt prepara una futura limpieza de Rust Engineering MCP. Guardarlo,
commitearlo o incluirlo en un PR no ejecuta la limpieza ni autoriza implementar
el plan de runtime/portabilidad. Se inicia cuando el owner solicite expresamente
ejecutarlo. El PR que incorpora este documento contiene únicamente planificación.

## 1. Resultado esperado y autoridad

Actúa como Technical Owner e integrador. Reestructura la documentación para que
explique el MCP realmente implementado, su instalación, uso, operación,
arquitectura, contratos, garantías y limitaciones. Consolida los ADR vigentes
en capítulos de arquitectura y retira del árbol operativo la documentación
histórica que ya no sea necesaria. Limpia scripts y fixtures sin perder pruebas
de comportamientos actuales, reproducibilidad necesaria ni materiales legales.

Entrega un README nuevo, breve y utilizable por un developer, que conserve todos
los badges y remita a documentación especializada para los detalles.

Al ejecutar este prompt quedan autorizadas las ediciones locales y los commits
coherentes de esta limpieza, incluidos tests, harnesses, CI y rutas técnicas
que deban cambiar como consecuencia directa de la reorganización. No se
autorizan nuevas funcionalidades, cambios de contratos, versiones, dependencias,
soporte de plataformas, runtime unificado, releases, tags, merge, push o PR de
la implementación salvo autorización adicional vigente. El permiso de publicar
el PR de estos planes no se hereda como permiso de publicar su ejecución.

No ejecutar otros prompts porque aparezcan como referencias. No descartar trabajo
ajeno, eliminar worktrees de otros encargos ni incorporar automáticamente ramas
de README, Sonar o runtime. La limpieza local de worktrees solicitada al crear
este plan es un encargo separado y no se repite al ejecutarlo.

## 2. Baseline y lectura inicial

Antes de modificar archivos:

1. Leer `AGENTS.md`, instrucciones de subdirectorios, la especificación completa,
   `docs/implementation-status.md`, los 90 ADR o sus sucesores y la documentación
   pública vigente. Registrar contradicciones frente al código y las pruebas.
2. Inspeccionar `git status`, worktrees, rama/base, árbol versionado, manifests,
   lockfile, tests, fixtures, scripts y workflows. Elegir una baseline integrada
   y registrar su SHA; aislar la limpieza en su propia rama/worktree. No requerir
   una nueva aprobación para elecciones rutinarias dentro del alcance autorizado.
3. Revalidar el estado de Sonar, M8 y del plan de runtime/portabilidad. Si hay trabajo
   concurrente, evitar sus archivos y acordar secuencia mediante los responsables.
   La limpieza puede preceder a portabilidad: solo documentará soporte existente.
4. Obtener el censo de tools, CLI, contratos, estabilidad, features, plataformas,
   fuentes de datos y límites desde handlers, tipos, schemas, tests y artifacts
   actuales. Contrastar con `contract --json`, ayuda y versión de un binario
   construido desde la baseline cuando existan esos comandos. No usar un binario
   viejo de `target/` como evidencia del código actual.
5. Comprobar fuentes oficiales actuales cuando se documenten comandos o decisiones
   dependientes de MCP, rmcp, Cargo, clientes, SQLite, LanceDB o RustSec. Contrastar
   la documentación con las versiones fijadas; no actualizar dependencias.

La fotografía del diagnóstico anterior es orientativa, no un objetivo de borrado:

| Categoría | Inventario observado |
| --- | ---: |
| Repositorio versionado | 2.936 archivos; 732 Markdown |
| `docs/` | 1.727 archivos; 684 Markdown; 37 scripts Python |
| `docs/validation/` | 1.099 archivos; 426 Markdown |
| `docs/reviews/` | 229 archivos; 94 Markdown |
| `docs/research/` | 185 archivos; 17 Markdown |
| `docs/roadmap/` | 37 archivos; 16 Markdown |
| `docs/prompts/` | 22 archivos Markdown |
| `docs/adr/` | 90 ADR y un índice |
| `scripts/` | 74 archivos |
| `fixtures/` | 549 archivos; 40 Markdown |

Estos números preceden a la incorporación de los nuevos prompts y pueden variar
por otros trabajos. Recontar con `git ls-files`, separando versionados, ignorados,
vendorizados y generados. No contar un movimiento como eliminación. Los rangos
estimados de 640–655 Markdown y 1.550–1.700 archivos retirados son hipótesis;
la cobertura y el valor vigente mandan sobre cualquier cifra.

## 3. Equipo y paquetes de trabajo

Utiliza subagentes cuando exista trabajo independiente y delimitado. El principal
conserva decisiones, contratos, seguridad, integración y cierre. No hace falta
ocupar todos los slots ni lanzar todos los proveedores a la vez.

| Rol | Agente/modelo previsto | Responsabilidad |
| --- | --- | --- |
| Technical Owner | Modelo principal configurado por el host; perfil de AGENTS: GPT-5.6 Sol, High | Baseline, clasificación final, arquitectura documental, revisión e integración |
| Inventario | Codex GPT-5.6 Sol, Medium | Censo, referencias directas/dinámicas, candidatos y consumidores; inicialmente read-only |
| Arquitectura documental | Codex GPT-5.6 Sol, High | Contrastar ADR y código, proponer consolidación y preservar invariantes |
| README y guías | Codex GPT-5.6 Sol, Medium | Onboarding verificable, badges, enlaces y ausencia de duplicación |
| Tests/herramientas | Codex GPT-5.6 Sol, High | Migraciones de rutas, regresiones, scripts, fixtures y CI; alcance disjunto |
| Revisor editorial independiente | Claude Code CLI, Sonnet 5, esfuerzo medium admitido | Review read-only del README, flujo de instalación, precisión y navegación |
| Revisor de arquitectura/seguridad | Claude Code CLI, Opus 5, esfuerzo high admitido | Review read-only de decisiones consolidadas, garantías, pérdidas de cobertura y distribución |
| Auditor independiente | Gemini por `agy`, `gemini-3.8-flash-high` | Read-only: contrastar inventario, candidatos de borrado, referencias, tests y criterios de aceptación |

La lista de roles es una secuencia de asignaciones, no siete sesiones simultáneas.
Usar como máximo tres workers concurrentes además del principal, o menos según
slots, cuotas y recursos. Cerrar los workers al aceptar sus entregables. Un
solo escritor por archivo; el principal es dueño de `AGENTS.md` y de la integración.
Claude y Gemini reciben paquetes pequeños con fuentes y diff concreto, no todos
los transcripts del repositorio. No delegar el cierre a un resumen de modelo.

Preflight de herramientas externas: inspeccionar `claude --version`,
`claude --help`, `agy --help` y `agy models`. En la preparación se observó
Claude Code 2.1.274 y el ID Gemini anterior en `agy models`; esto no prueba
disponibilidad futura ni que una inferencia Claude Sonnet/Opus esté habilitada.
Verificar IDs exactos, esfuerzos y modo read-only antes de invocar; no asumir
flags de otra CLI. No sustituir silenciosamente un modelo requerido ausente:
registrar la limitación y continuar trabajo independiente mientras se resuelve.

Mantener el modelo principal elegido por el host; el agente no cambia su propio
modelo. Usar los proveedores ya configurados, sin instalar CLI, cambiar cuentas,
comprar créditos ni activar fallback facturable. No inspeccionar ni exponer
secretos para registrar autenticación. Ante cuotas, guardar un checkpoint y
evitar reintentos en bucle o repetir tareas terminadas.

Cada paquete de delegación especifica: objetivo y Definition of Done, SHA de
entrada, archivos permitidos, interfaces prohibidas, restricciones, tests,
evidencia esperada y reviewer. Cada salida contiene `Task`, `Result`,
`Files changed`, `Tests executed`, `Evidence`, `Risks`, `Decisions`, `Open issues`.
Los reviewers externos no editan archivos, no hacen commits/merges ni ejecutan
proyectos del corpus; usar las restricciones reales de la CLI, no solo el prompt.

## 4. Clasificación antes de retirar archivos

Construir un inventario reproducible por archivo, con una sola clasificación:

- **Conservar:** información, datos o pruebas actuales necesarios.
- **Consolidar:** contenido vigente que pasa a un documento canónico.
- **Mover:** input técnico vigente fuera de `docs/`, conservando sus bytes.
- **Retirar:** historia, duplicación, sonda superada o archivo sin función vigente
  demostrada, con motivo y revisión.
- **Pendiente:** uso dinámico, obligación legal o dependencia aún sin resolver.

Para cada candidato registrar ruta, tipo, bytes/SHA, motivo, consumidores y prueba
que verifica la migración o cobertura equivalente. Buscar con `rg` nombres y rutas
completas/parciales, imports, `include_str!`/`include_bytes!`, globs, directorios
enumerados, manifests y plantillas; inspeccionar llamadas dinámicas. Una ausencia
de referencias textuales por sí sola no autoriza borrar. Un hash genérico del
corpus no demuestra que sus casos se ejecuten.

Guardar el inventario y un checkpoint breve en `.planning/documentation-cleanup/`
durante la ejecución. No crear un backlog paralelo ni un paquete por worker con
transcripts. Al cerrar, entregar el reporte final con el PR o sus artifacts y
retirar el estado temporal; preservar en el árbol únicamente el registro mínimo
necesario para localizar decisiones/evidencias vigentes. El historial completo
permanece en Git: no reescribirlo ni usar filter-repo/GC para esta tarea.

### Dependencias detectadas que requieren migración

Reverificar al menos:

- `scripts/test-m3-clients.py`, `test-m4-clients.py` y sus unit tests reutilizan
  `docs/validation/M1/17-codex-client/controller.py`: mover controlador y pruebas
  a `scripts/lib/` o soporte de tests, y comprobar las cadenas M3–M8.
- `scripts/release-artifact.py` carga
  `docs/release/upstream-licenses/receipt.json` y los textos allí referenciados:
  moverlos juntos a `licenses/upstream/`, actualizar consumidores y comprobar
  hashes, notices/SBOM y empaquetado. Revisar también `verify-vendor.py`.
- `scripts/contract-freeze.py`, clientes M8 y el workflow de release utilizan
  el freeze bajo `docs/validation/M8/`: preservar la baseline de contratos fuera
  de `docs/`, sin regenerarla para ocultar cambios de schema o descripción.
- `scripts/check-architecture.py` y la comparación de benchmarks remiten a la
  calificación estadística M5: conservar criterios, recibo y capacidad de
  reproducción. No activar veredictos ni cambiar umbrales como limpieza.
- Scripts runtime/provisioning leen configuraciones y recibos de M3–M6;
  rendimiento/soak M8 utiliza budgets; release/recovery necesita sus entradas.
  Distinguir lectura de inputs, escritura de resultados y menciones históricas.
- `scripts/docs-hygiene.py`, sus tests, `gate.py`, exclusiones de Sonar,
  workflows y herramientas de publicación conocen el layout antiguo.

Los recibos históricos son inmutables. Registrar origen, hash y destino al moverlos;
no reescribir un recibo para que parezca emitido sobre los bytes actuales. Si un
consumidor necesita otro formato, crear una entrada técnica derivada explícita y
probada, conservando provenance. Actualizar enlaces de documentos vivos y comentarios
del código sin falsificar la evidencia original. No conservar miles de archivos
únicamente para sostener enlaces históricos que ahora deben apuntar a Git.

## 5. Estructura documental objetivo

Objetivo orientativo: 25–30 documentos canónicos en `docs/`, sin una carpeta por
milestone ni un archivo por tool o ADR. Se puede ajustar la división con razones
de legibilidad; no recrear el volumen anterior bajo nombres nuevos.

```text
README.md
CHANGELOG.md
SECURITY.md
CONTRIBUTING.md
AGENTS.md
docs/
  README.md
  guides/
    installation.md
    configuration.md
    clients.md
    workflows.md
    troubleshooting.md
  reference/
    tools.md
    cli.md
    compatibility.md
    limits.md
    data-formats.md
  architecture/
    overview.md
    domain-and-application.md
    mcp-and-contracts.md
    execution-and-security.md
    mutation.md
    catalog-and-search.md
    jobs-and-artifacts.md
    analyzer.md
    performance.md
    decisions.md
  operations/
    runtime-provisioning.md
    catalog-maintenance.md
    backup-and-recovery.md
    release-verification.md
  development/
    testing.md
```

Fuera de `docs/`, reutilizar ubicaciones existentes cuando sirvan; crear solo las
fronteras necesarias: `licenses/upstream/` para licencias, `tests/data/` para inputs
compartidos, `tests/baselines/` para contratos/budgets, `qualification/` para recibos
vigentes necesarios y `scripts/lib/` para soporte reusable. Fixtures propios de un
crate permanecen con sus tests. No duplicar fixtures compartidos al reubicarlos.
Los resultados nuevos voluminosos van a artifacts ignorados o CI, no a `docs/`.

Eliminar `spec/`, `roadmap/`, `research/`, `reviews/`, `validation/` y `release/`
de `docs/` solo después de extraer contenido/inputs vigentes y resolver todos
sus consumidores. La especificación principal requiere además el mapa de
requisitos y la puerta de retirada de la sección 6. Los informes científicos históricos no son capacidades del
MCP; conservar un resultado solo si fundamenta una decisión o limitación actual.
`CHANGELOG.md` conserva cambios de versiones útiles al usuario, no diarios de trabajo.

**Planes pendientes:** el plan `implement-1.0-runtime-portability-astra.md` no es
basura histórica si aún no se ejecutó. Mover los encargos realmente pendientes
a `.planning/`, actualizar sus referencias y puntos de reanudación sin cambiar
objetivos, autorizaciones, roles ni puertas SSH/publicación. Si existe ejecución
activa con estado persistente, coordinar primero su migración. Este prompt sigue
localizable mientras la limpieza esté en curso; los prompts terminados quedan
en Git. Los planes pendientes deben quedar versionados, incluidos en las
exportaciones de fuente y cubiertos por validación de enlaces/anchors; no basta
que sobrevivan como archivos locales ignorados. No dejar planes aspiracionales
dentro de la documentación de producto.

## 6. Consolidación de ADR y actualización de instrucciones

Antes de retirar la especificación principal, construir también una matriz de
cláusula/requisito normativo → vigente/pendiente/sustituido/aspiracional → destino
canónico → código/tests o disposición explícita. Todo requisito vigente conserva
su obligación y evidencia; su incumplimiento se registra como limitación, no se
borra. Los compromisos pendientes autorizados pasan al plan versionado aplicable,
y las propuestas aspiracionales sin encargo se localizan en Git con su estado.
No habilitar features pendientes ni inventar nuevas obligaciones. La revisión de
C1 debe comprobar esta cobertura; retirar `docs/spec/` solo después de resolver
todas sus cláusulas normativas y actualizar la fuente de autoridad en `AGENTS.md`.

Crear una tabla completa ADR original → vigente/sustituido/histórico → sección
canónica → código/tests que lo sustentan. No basta concatenar los ADR existentes.
Leer sus enmiendas y resolver contradicciones conforme a evidencia y precedencia
de seguridad/correctness; no escoger el párrafo más reciente sin examinarlo.

En cada capítulo conservar las decisiones implementadas, contexto relevante,
justificación, alternativas cuya exclusión aún explica un límite, consecuencias
y estado actual. `architecture/decisions.md` contiene el mapa compacto con IDs
originales y referencias históricas al commit base, no una segunda explicación.
Las futuras decisiones se registran en su capítulo con ID/fecha y esos campos;
no recrear automáticamente un directorio de 90 ADR.

Preservar especialmente: hexagonalidad, gateway único, rmcp como frontera MCP,
roots confiables, I/O no-follow, deny-by-default, aislamiento comprobado,
cancelación y cleanup, cuotas, stdout de protocolo, separación CLI/runtime,
SQLite autoritativo/LanceDB derivado, provenance/freshness, journal/recovery,
`local_coordinated` y sus límites, contratos congelados, admisión por identidad,
criterios estadísticos, riesgos residuales y fronteras reales de distribución.

Actualizar `AGENTS.md` al principio de la transición, antes de retirar sus fuentes:
registrar explícitamente esta nueva política documental, fuentes canónicas,
estructura y mantenimiento de decisiones. Mantener las reglas técnicas y de
seguridad; retirar encargos históricos y listas de alcance caducadas. Esta
instrucción del owner autoriza la consolidación documental, no un cambio de
arquitectura del producto. Durante la transición mantener fuentes resolubles.

Reemplazar `implementation-status.md` por capacidades/limitaciones comprobadas en
la referencia correspondiente; extraer evidencia necesaria. No convertir tareas
pendientes en Done por desaparecer el tablero. Si falta enforcement, documentar
la limitación y preservar el test; no implementar la feature dentro de esta limpieza.

## 7. README nuevo: instalación y uso, con todos los badges

El README debe permitir que un developer entienda qué obtiene, compruebe requisitos,
instale o compile el binario, conecte su cliente y realice una primera operación.
Debe estar en español coherente con la guía actual, con comandos y nombres técnicos
exactos. Objetivo editorial de 150–220 líneas aproximadamente; priorizar que el
camino inicial sea completo y verificable sobre un límite artificial.

### Badges: requisito estricto

Capturar antes de editar todos los badges del README de la baseline, incluidos
Markdown, HTML y referencias indirectas. Conservar cantidad, orden, labels/alt,
URLs de imagen, parámetros y destinos. Hoy existen nueve:

1. CI.
2. SonarCloud Quality Gate.
3. SonarCloud Security Rating.
4. SonarCloud Reliability Rating.
5. SonarCloud Maintainability Rating.
6. SonarCloud Coverage.
7. Rust 1.98.1.
8. License MIT OR Apache-2.0.
9. M8ven Score.

La baseline manda si se agregaron otros. No convertirlos en capturas, cambiar sus
destinos o retirar uno porque esté rojo/no disponible. Mantener los targets
relativos como `rust-toolchain.toml` y `LICENSE` resolubles. La preservación se
verifica determinísticamente contra la baseline; un HEAD obtenido después de
editar no sirve como baseline. No congelar para siempre estos nueve en un test
que impediría una futura actualización legítima de Rust o un badge nuevo.

### Contenido, en este orden

1. Nombre y badges; descripción concreta del MCP y su transporte implementado.
2. Capacidades agrupadas en pocas líneas y enlace a la referencia de tools.
   Distinguir release disponible, checkout y preview sin narrar milestones.
3. Requisitos mínimos y frontera real de soporte: binario host, filesystem,
   runtime/engine y datos opcionales según evidencia actual. No presentar un
   runtime planificado, Linux o Windows futuros como disponibles.
4. Instalación recomendada con versión/artifact verificable y comprobación breve
   de integridad; alternativa concisa de compilación desde fuente enlazada a la
   guía. No inventar instaladores, imágenes publicadas, comandos o URLs.
5. Configuración mínima completa para un cliente realmente soportado, con ruta
   absoluta al binario, root autorizada y flags existentes. Señalar qué sustituir
   y enlazar los otros clientes/configuración avanzada. No usar placeholders que
   parezcan credenciales ni conceder escritura implícita.
6. Primera comprobación y uso: diagnóstico real y un flujo breve abrir proyecto →
   obtener referencia → ejecutar una operación. Si validar requiere runtime/datos
   adicionales, incluir el prerrequisito o enlazar el paso exacto; discovery solo
   no se describe como ejecución de Rust funcionando.
7. Datos operativos indispensables: `build.rs`/proc macros pueden ejecutar código,
   grants de escritura explícitos, datos offline y límites de soporte relevantes.
   Una advertencia concisa con enlaces, no un segundo modelo de seguridad.
8. Índice pequeño a instalación detallada, clientes, tools/CLI, troubleshooting,
   arquitectura, seguridad, contribución, cambios y licencia.

No copiar al README las 36 tools, todas las flags, todos los clientes, tablas de
calificación, recibos, ADR, roadmap, conteos históricos de tests ni instrucciones
de cada plugin. La guía detallada posee los procedimientos; el README resume
el camino mínimo y enlaza secciones estables. No basta dejar un índice sin
instalación funcional. Separar ayuda al usuario de instrucciones al contribuidor.

## 8. Scripts y fixtures: conservar cobertura efectiva

Clasificar por función actual, no por antigüedad ni prefijos M0–M8. Mantener gates,
regresiones, provisioning, validadores de distribución y herramientas para
reproducir calificaciones todavía relevantes. Renombrar por capacidad solo cuando
mejore el uso; no exigir una reescritura general de harnesses.

Candidatos iniciales que hay que comprobar:

| Grupo | Acción propuesta, sujeta a evidencia |
| --- | --- |
| Seis `probe-m2-*`: cargo-fix, fix-socket-mask, guest-staging, offline-registry, vendor-data, write-primitives | Retirar si solo reproducen decisiones históricas ya cubiertas; tratar imports entre sondas conjuntamente |
| `test-m3-budgets.py` | Revisar si su medición fue sustituida por budgets/gates actuales |
| `release-inventory.py` | Verificar sustitución por el inventario/empaquetado vigente |
| Capturas, medición vendor y simulación M5 | Conservar o reorganizar lo que regenere datasets/calificación actuales; no borrar por no estar en CI |
| `fixtures/cargo-local-registry/` (8 archivos) | Retirar junto a las sondas si no queda consumidor vigente |
| `fixtures/hostile-reports/` (13 archivos) | Comprobar cada caso adverso contra tests efectivos, no solo hashes/inventario |
| `fixtures/profile-probe/perf-capability-probe.rs` (1 archivo) | Resolver ausencia de consumidores y equivalencia de la prueba vigente |

Para `hostile-reports`, si el caso es relevante y no existe cobertura equivalente,
conectarlo a un test de frontera existente o generar un fixture mínimo determinista
en ese test. No implementar previews HTML u otras features ausentes para justificar
conservar corpus aspiracional. Los archives grandes pueden sustituirse por
generación acotada si mantiene exactamente el escenario; nunca extraerlos sin
controles ni ejecutarlos fuera del sandbox. Un test nuevo que descubre un fallo
real no se silencia: clasificarlo y resolver alcance antes de afirmar ausencia
de regresiones.

No retirar `test-m2-runtime.py`, los clientes antiguos reutilizados, fixtures
negativos, licenses vendorizadas, inputs offline ni snapshots de contrato para
reducir conteos. No confundir tests huérfanos con tests obsoletos: por ejemplo,
revisar el wiring de `test-m8-rollback-unit.py` si sigue probando código vigente.

## 9. Actualizaciones y validación de pruebas

Primero medir la baseline y registrar fallos preexistentes por separado. Adaptar
las pruebas antes o junto a cada movimiento; el repositorio no debe depender del
layout histórico ni de que el desarrollador conserve archivos borrados localmente.

Paquetes obligatorios:

1. **Higiene:** adaptar `docs-hygiene.py` y sus tests al layout canónico; comprobar
   enlaces locales y anchors en raíz/docs y `.planning/**/*.md`, comprobar que
   los planes pendientes están versionados/exportables, navegación desde README y fuentes de
   `AGENTS.md`. Actualizar o retirar tests de inventarios históricos solo cuando
   esa función desaparezca; no vaciar allowlists ni añadir skips para pasar.
2. **Consumidores:** actualizar imports, rutas relativas, globbing, manifests,
   unit tests Python/JS, comentarios/documentación Rust y workflows afectados.
   Probar controladores importados desde su nueva ubicación y desde un checkout
   limpio, sin `PYTHONPATH` ni cwd privados que oculten imports rotos.
3. **Datos y release:** hashes idénticos de inputs movidos, licencias completas,
   notices/SBOM, inventario de source, freeze de schemas/descripciones, budgets,
   reportes de coverage, smoke/rollback y artifact assembly según dependencias.
4. **README:** comparar badges con baseline; validar sintaxis de configuración
   y existencia de flags contra CLI real. Ensayar el quickstart en directorio
   temporal limpio y verificar una interacción MCP real. Revisar renderizado y
   enlaces; no instalar clientes ni cambiar su configuración personal. Registrar
   qué instalación se probó (artifact publicado o build local), con limitaciones.
5. **Fixtures:** demostrar coverage equivalente o nueva del caso antes de retirar
   el input. Mantener los positivos y adversos relevantes de filesystem, parser,
   sandbox, cancelación, journal, mutación, datos offline y runtime.
6. **CI:** actualizar paths-filters, matrices, uploads, working-directory,
   source inventories, Sonar/exclusiones, contratos y release. La reorganización
   no debe excluir accidentalmente las nuevas rutas de análisis o validación.

Preferir tests discriminantes del comportamiento y del wiring. No añadir tests
que solo afirmen el número de Markdown, comparen toda la prosa con un snapshot o
declaren correcto el borrado porque el archivo desapareció.

Gate mínimo final, respetando las opciones vigentes:

```text
cargo fmt --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
cargo test --workspace --doc --locked
python3 -B scripts/check-architecture.py
python3 -B scripts/gate.py core
```

Evitar duplicar comandos si `core` ya los ejecuta con opciones equivalentes:
leer su composición y guardar el mapa comando → resultado. Añadir unit tests de
los scripts afectados, contracts/protocol/integration/security correspondientes,
verificación de vendor y release tests. Ejecutar `full` si se modifican rutas,
inputs o composición de suites runtime/clientes/calificación que solo cubra ese
gate; esta limpieza previsiblemente afecta varias. Reconstruir el binario final
antes de ejecutar harnesses; registrar SHA/source inventory y runtime empleado.

Usar entornos/fixtures reales para las fronteras. `cargo audit`/`cargo deny` se
ejecutan si están instalados/configurados; reportar ausencia sin instalar ni
declarar pass. No comprar recursos ni descargar modelos/datos silenciosamente.
Si falta un requisito de una prueba obligatoria, no marcar cierre completo:
conservar cambios y reporte verificable con el bloqueo preciso. No se necesita
esperar hosts futuros para una limpieza del soporte actual.

Verificar desde un checkout limpio o una exportación de los archivos versionados,
con caches de herramientas autorizadas separadas: los archivos retirados, caches
de imports y recibos viejos locales no pueden sostener un falso verde.

## 10. Secuencia, revisión y cierre

| Corte | Responsable | Puerta de salida |
| --- | --- | --- |
| C0: baseline e inventario | Principal + worker inventario | Lista trazable, consumidores y planes activos identificados; fallos heredados registrados |
| C1: mapa normativo y layout | Principal + worker arquitectura | Spec/ADR→estado→sección→código/tests o disposición; revisión Opus; política de AGENTS explícita, sin cambio de garantías |
| C2: extraer soporte técnico | Worker tests/herramientas | Inputs/licencias/controladores movidos, hashes preservados, consumidores y pruebas focalizadas verdes |
| C3: consolidar docs y README | Workers disjuntos arquitectura/guías | Documentos canónicos, onboarding completo, badges intactos; Sonnet revisa uso/precisión |
| C4: retirar historia y huérfanos | Principal + worker inventario | Candidatos resueltos, coverage equivalente, planes pendientes preservados; auditoría Gemini |
| C5: integración final | Principal + QA distinto del autor | Checkout limpio, gates proporcionales, cero regresiones nuevas, revisión final aceptada y reporte |

Los reviewers informan findings P0/P1/P2 con archivo, evidencia e impacto; el
principal registra disposición, corrección o razón verificable de descarte.
Cero P0/P1 abiertos y cero P2 que contradigan criterios obligatorios para cerrar.
No reemplazar una revisión externa fallida por la opinión del mismo autor ni
repetir full gates sin cambios que lo justifiquen. Ningún writer actúa mientras
se califica el tip final.

Commits pequeños por propósito: política/consolidación inicial, extracción de
soporte con tests, documentación y README, retirada de historia con consumidores
resueltos. Evitar un commit de miles de borrados antes de migrar sus dependencias.
Si un movimiento y su test son inseparables, pertenecen al mismo commit.

### Criterios de aceptación

- [ ] `docs/` explica solo comportamiento implementado, operación y límites
  actuales; todas las afirmaciones materiales se contrastaron con código/tests.
- [ ] Cada requisito normativo de la spec y cada ADR está mapeado; obligaciones,
  pendientes autorizados y decisiones/garantías vigentes siguen localizables;
  `AGENTS.md` referencia las fuentes nuevas y no conserva encargos caducados.
- [ ] README permite instalación y primer uso, conserva todos los badges de la
  baseline y enlaza detalles sin duplicar el manual ni narrar milestones.
- [ ] Ningún input técnico requerido depende ya de carpetas históricas eliminadas;
  licencias, contratos, budgets y evidencia necesaria siguen verificables.
- [ ] Los scripts/fixtures retirados tienen motivo y consumidores/cobertura
  resueltos; no se eliminaron regresiones para obtener una cifra de limpieza.
- [ ] Los planes pendientes y sus puntos de reanudación permanecen localizables,
  con alcance y autorizaciones intactos; ninguna feature futura se anuncia como actual.
- [ ] Enlaces/anchors, imports, gates, CI, empaquetado y quickstart pasan las
  verificaciones aplicables desde bytes finales sin depender de archivos locales viejos.
- [ ] Sin cambios no autorizados de contratos, versiones, dependencias, autoridad,
  seguridad, formatos persistidos o soporte; reviewers y gate final aceptados.
- [ ] El informe distingue eliminados, movidos, consolidados y nuevos por tipo,
  conteos antes/después, bytes retirados del árbol, pruebas ejecutadas y límites.
- [ ] No quedan transcripts, diarios o artifacts voluminosos nuevos en `docs/`;
  el historial y la evidencia mínima necesaria se pueden localizar.

El informe final enlaza README e índice nuevos, resume decisiones y tests,
identifica el SHA calificado y expone cualquier limitación sin presentar un
resultado parcial como limpieza terminada. Los bytes retirados del checkout no
son una reducción del tamaño de `.git`. No publicar, mergear ni crear tags como
consecuencia implícita del cierre local.

## 11. Instrucción sugerida de ejecución

> Ejecuta este prompt de limpieza profunda sobre una baseline integrada. Usa los
> workers Codex y las revisiones read-only Claude/agy descritas. Reestructura docs,
> consolida ADR, actualiza AGENTS/tests/harnesses y reescribe README conservando
> todos los badges. Preserva los planes pendientes y documenta únicamente el
> producto implementado. Haz commits locales coherentes, ejecuta los gates
> proporcionales y entrega el reporte; no implementes runtime/portabilidad ni
> publiques cambios sin autorización adicional.
