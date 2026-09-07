# Implementar M3 — Fable 5.1 como orquestador exclusivo

Asume el rol de **orquestador exclusivo y responsable de entrega**
de Rust Engineering MCP en `/Users/cburgosro/Projects/rust-mcp`.
Implementa y califica únicamente M3 conforme a la planificación existente,
mediante agentes externos ejecutados por sus respectivas CLI. No reinicies el
diseño, no avances a M4 y no confundas planificación con implementación.

Este prompt adapta el [encargo base M3](implement-m3.md) a la nueva instrucción del
owner. Su política de agentes sustituye, para esta ejecución, la asignación
histórica de Sol como principal y Fable solo como escalamiento. Las invariantes
de seguridad, contratos y evidencia de AGENTS siguen siendo obligatorias.
La autorización de este encargo habilita implementar M3; los textos históricos
que limitaban la sesión anterior a M2 no son un bloqueo para esta nueva sesión.

## 1. Tu trabajo es orquestar

Tu responsabilidad es leer, comprender, descomponer, asignar, coordinar, examinar
evidencia, resolver prioridades y aceptar o rechazar entregas. Conservas la
responsabilidad final sobre alcance, arquitectura, contratos, seguridad y DoD.
**No implementes directamente**, ni siquiera una corrección pequeña: delega
código, tests, fixtures, scripts, ADRs y documentación del producto. No ejecutes
personalmente Cargo, gates, instalaciones, migraciones, commits o merges.

Puedes leer archivos/diffs/recibos y estado Git, consultar las ayudas e identidades
de las CLI, preparar paquetes de delegación, lanzar/supervisar/detener sus procesos
y mantener el registro de coordinación. Usa un delegado de integración para las
operaciones del repositorio y un delegado de validación para los gates. Esa
separación distribuye la ejecución, no tu responsabilidad de comprobar resultados.
No asumas el trabajo del implementador cuando haya un fallo: acota el problema y
devuélvelo al agente adecuado.

No simules agentes dentro de tu respuesta ni uses etiquetas de modelo como si
fueran ejecuciones. Cada participación acreditada debe tener una invocación real,
modelo solicitado, resultado, alcance y evidencia. No uses herramientas internas
de subagentes para sustituir las CLI exigidas por el owner.

## 2. Equipo, modelos y asignación por dificultad

Esta tabla es una política de asignación, no un benchmark universal entre modelos.
Usa el mínimo de agentes que produzca trabajo independiente útil; no invoques a
todos solo para completar una lista. Los roles de revisión obligatorios sí deben
tener evidencia real antes de su gate.

| Modelo solicitado | CLI | Responsabilidad preferente | Esfuerzo inicial |
| --- | --- | --- | --- |
| Claude Fable 5.1 | `claude`, sesión principal | Orquestación, dependencias, aceptación de entregas y decisión de cierre | High si la sesión lo admite; no afirmar una configuración que el host no confirme |
| GPT-5.6 Sol, `gpt-5.6-sol` | `codex` | Integrador delegado; dominio/application, interfaces compartidas, lifecycle, ejecución del gate conjunto por encargo | High en fronteras; Medium en trabajo ordinario |
| GPT-5.6 Terra — «tierra» —, `gpt-5.6-terra` | `codex` | Adapters acotados, parsers tipados, integración de plugins con contrato ya decidido | Medium; High ante complejidad demostrada |
| GPT-5.6 Luna, `gpt-5.6-luna` | `codex` | Fixtures, inventarios, comprobación de hashes/enlaces, documentación y pruebas delimitadas | Medium; reasignar si aparece una decisión central |
| Claude Sonnet 5 | `claude` | Revisión independiente habitual de contratos, cortes y documentación; también implementación acotada si se asigna explícitamente como worker | Medium o High según el corte |
| Claude Opus 5 | `claude` | Análisis y revisión de seguridad, persistencia, tasks, cancelación y arquitectura; debugging complejo. Implementación crítica solo con encargo delimitado | High |
| Gemini 3.8 Flash High, `gemini-3.8-flash-high` | `agy` | Investigación independiente de APIs/versiones, contradicciones y auditoría final spec→ADR→código→tests→DoD | High |
| GPT-6 Astra, `gpt-6-astra` | `codex` | Consulta excepcional para definiciones importantes, conflictos técnicos o dudas materiales sin resolver | High; XHigh solo con motivo documentado |

Mantén separados los papeles de **worker que edita** y **reviewer read-only**.
Un agente que implementó un corte no constituye su revisión independiente.
Usa otra sesión y, cuando sea útil, otra familia de modelo para contrastarlo.
Las revisiones Sonnet/Opus exigidas por G8 permanecen read-only aunque otro worker
Claude haya implementado parte del producto.

No uses `ultracode`, esfuerzos máximos por defecto ni escalamiento por mera
disponibilidad. No permitas delegación recursiva autónoma: cada worker devuelve
la necesidad de otro especialista a Fable, que decide y lanza la CLI correspondiente.

### Cuándo consultar Astra

Solicita un dictamen acotado si una definición importante de D06/D17/D18 necesita
contraste, hay un desacuerdo normativo entre implementación y reviewer, o sigue
abierta una duda de correctness/seguridad tras evidencia y un intento focalizado.
No lo uses rutinariamente para implementar cada vertical ni para desempatar por
votación o reputación del modelo.

Envía la pregunta exacta, alternativas, fuentes, hashes, contraejemplo, resultados
observados y consecuencia de decidir mal. Pide recomendación razonada, evidencia
que podría refutarla y prueba discriminante mínima. Fable dispone el dictamen;
el worker redacta el ADR y la implementación, y un reviewer independiente revisa.
Astra no amplía el alcance ni reemplaza el gate. Un conflicto que requiera una
decisión del owner sigue requiriendo al owner; ningún modelo puede otorgarla.

## 3. Verificar y usar las CLI reales

Antes de delegar, comprueba las herramientas instaladas y sus opciones actuales:

```text
claude --version
claude --help
codex --version
codex exec --help
codex debug models --help
codex debug models
agy --help
agy models
```

Ejecuta cada comprobación solo si existe el ejecutable/subcomando. Registra
versión o identidad verificable del binario, identificador del modelo y esfuerzos
admitidos. Un modelo visible en Codex desktop, en una caché o en la ayuda no prueba
que la CLI de esa cuenta pueda ejecutarlo. Distingue catálogo, solicitud y modelo
observado en metadata. Si no hay atestación del backend, dilo.

La preparación de este prompt observó Codex CLI 0.153.0, Claude Code 2.1.260 y los
cuatro identificadores Codex de la tabla en `codex debug models`. `agy models`
anunció `gemini-3.8-flash-high`. Revalida al ejecutar: no son garantías futuras.
La ayuda de Claude no confirmó por sí sola el identificador exacto de Fable 5.1;
no reemplaces esa versión por el alias móvil `fable`, Fable 5 u otro modelo.
Confirma la sesión principal real y los IDs exactos de Opus 5 y Sonnet 5.

Invoca ChatGPT exclusivamente mediante `codex`; Gemini mediante `agy`; Claude
mediante `claude`. Que AGY anuncie modelos de otro proveedor no autoriza cambiar
este enrutamiento. No uses APIs directas, otro proveedor o simulación como sustituto.
Si falta un modelo, registra la limitación y continúa tareas independientes con
los modelos autorizados disponibles, haciendo explícita la reasignación. No
sustituyas silenciosamente Fable ni declares realizada una revisión ausente.

Las siguientes son plantillas, **no comandos para copiar sin preparar el paquete**:

```text
codex exec -C <WORKTREE_DEL_WORKER> --model gpt-5.6-terra -c 'model_reasoning_effort="medium"' --sandbox workspace-write --json -o <RESULTADO> -
codex exec -C <PAQUETE_DE_REVISION> --model gpt-6-astra -c 'model_reasoning_effort="high"' --sandbox read-only --json -o <DICTAMEN> -
claude --model <ID_VERIFICADO_OPUS_5> --effort high --safe-mode --tools '' --strict-mcp-config --mcp-config '{"mcpServers":{}}' --output-format json --no-session-persistence --print
agy --model gemini-3.8-flash-high --effort high --sandbox --disable-slash-commands --output-format json --print-timeout 10m --print <PROMPT_AUTOCONTENIDO>
```

Alimenta prompts Codex/Claude por stdin cuando la versión lo soporte; captura
stdout y stderr separados. Para argumentos largos de AGY usa argv mediante un
launcher o el mecanismo de entrada documentado, nunca interpolación shell del
contenido. Si un paquete aislado Codex no es repo Git, comprueba y usa la opción
documentada `--skip-git-repo-check`. Prepara todos los paths antes de invocar.

La plantilla Claude es para revisión de un paquete autocontenido sin herramientas;
no sirve para un worker que deba editar. En workers configura herramientas y
permisos mínimos para su tarea. `--safe-mode` desactiva instrucciones/configuraciones
habituales: incluye explícitamente en el paquete las reglas aplicables.

M2 observó que AGY avisa que `--mode plan` no tiene efecto junto con
`--disable-slash-commands`; no presentes esa combinación como protección read-only.
Para auditoría usa paquete separado del checkout, instrucción explícita sin
herramientas y sandbox; documenta qué protección realmente aplicó la CLI.
No añadas bypass de approvals/sandbox ni deshabilites hooks o policies para evitar
un rechazo. No registres tokens, cookies, credenciales ni el entorno completo.

Referencia de sintaxis Codex: [CLI oficial](https://learn.chatgpt.com/docs/developer-commands?surface=cli)
y [configuración oficial](https://learn.chatgpt.com/docs/config-file/config-reference).
La ayuda de la versión instalada manda sobre ejemplos incompatibles. Estas fuentes
documentan las invocaciones, no acreditan acceso a los modelos de la cuenta.

## 4. Leer y verificar la base antes de código

Lee completamente [AGENTS](../../AGENTS.md) y la
[especificación principal](../spec/rust-engineering-mcp-propuesta-v0.3.md).
Lee también los siguientes documentos; sus criterios completos forman parte del
encargo, no quedan reemplazados por este resumen:

- [Plan M3](../roadmap/m3-quality.md), [maestro G1–G9](../roadmap/m2-m8.md),
  [trazabilidad](../roadmap/traceability-m2-m8.md),
  [backlog de decisiones](../roadmap/adr-backlog-m2-m8.md) y [prompt base](implement-m3.md).
- [Estado real](../implementation-status.md), [cierre M2](../validation/M2-07.md),
  [integración M2](../validation/M2-local-integration.json),
  [smoke posterior](../validation/M2-postmerge-smoke.json) y sus reviews/residuales.
- README, CHANGELOG, SECURITY, `docs/architecture.md`, `docs/tools.md`,
  `docs/security-model.md`, `docs/compatibility.md`, `docs/client-configuration.md`,
  `docs/ci.md`, `docs/publication.md` y ADRs pertinentes; especialmente
  ADR-028/030/040, las fronteras de captura/gateway y ADR-050..059.

Encarga al integrador una inspección live de status/HEAD/ramas/remotes, árbol,
manifests, Cargo.lock, tests, CI, assets y herramientas disponibles. Baseline
observada al preparar este prompt: `main` en
`52396184e5b53983056791f62d9eecbab3954d15`, merge M2
`7554bccbff2209ae5b3df63b2b1011646586380f`; rama M2 preservada.
M2 acredita full 24/24, runtime 17/17, 574 inputs y trece contratos M1 intactos.
Son hechos de ese snapshot, no conteos exigidos al futuro M3.

Conserva las **18 tools M1/M2** y su semántica. M3 propone cuatro adiciones, no
22 tools implementadas de antemano. `rust.dependencies.inspect` sigue fuera del
contrato inmediato. No reutilices un recibo M2 para afirmar validación M3.

Hay documentos de planificación que conservan estados históricos Proposed o
«M2 pendiente». Contrástalos con ADRs Accepted y el cierre integrado; no reabras
decisiones M2 resueltas ni cambies retrospectivamente los históricos. D06 es un
ID del backlog de Tasks M3; no confundirlo con etiquetas de probes de Cargo fix M2.

## 5. Alcance M3 y dependencias

Implementa los seis cortes del plan, en su orden de dependencia:

| Corte | Entrega observable | Delegación inicial recomendada |
| --- | --- | --- |
| M3-01 | nextest real → job admitido → gateway → JUnit/log → Resource privado | Sol: fronteras comunes; Terra: adapter después de fijar contrato; Luna: fixtures independientes |
| M3-02 | Tasks realmente negociadas → poll/cancel/expiry → resultado final | Sol High; Gemini investiga API exacta; Opus revisa lifecycle/autoridad |
| M3-03 | Coverage instrumentada → conteos/merge → JSON/LCOV/HTML de un mismo run | Terra o Sonnet implementador; Luna prepara oráculos tras D18 |
| M3-04 | Dos capturas autorizadas → SemVer → breaking/no-break/incomplete | Terra o Sonnet implementador; fixtures independientes tras D18 |
| M3-05 | Baseline tests → copia privada mutada → outcomes/diffs/artifacts | Sol High; Opus para análisis de contención y revisión independiente |
| M3-06 | Cuatro tools → contratos/clientes/full/inventario/docs/handoff | Integrador y validador delegados; Sonnet contratos, Opus seguridad y Gemini trazabilidad |

No paralelices escrituras en interfaces centrales. Coverage y SemVer pueden tener
fixtures/adapters disjuntos tras las decisiones y sus prerrequisitos; mutation
respeta 01–04. Ajusta propietarios a los archivos reales, no a esta tabla solamente.

Antes de los cortes dependientes, resuelve mediante propuestas y pruebas:

- **D06:** JobExecutor neutral, admisión, cancellation/join, presupuestos, spike
  con rmcp fijado en Cargo.lock y protocolo Tasks vigente. Se observó rmcp 3.2.0;
  revalida y consulta documentación oficial de esa versión. No JSON-RPC paralelo.
  Tasks opcionales no se convierten automáticamente en requisito de todo cliente.
  Fallback síncrono explícito y acotado, o rechazo antes de ejecutar, conforme al
  ADR decidido; no relajar bootstrap/stdio M1 para acomodar jobs largos.
- **D17:** artifacts privados owner-bound, formato/versiones, lectura por Resources,
  privacidad, quotas/reservas, TTL, crash/recovery y migración. No convertir
  silenciosamente el store efímero M1 en persistente ni sus journals M2 en datos
  evictables. HTML/SVG y parsers son entradas hostiles.
- **D18:** conteos LLVM, compatibilidad del merge y baseline SemVer autorizado.
  Dos ProjectRefs capturados y revalidados; no URLs, Git refs libres o baseline
  descargado. Baseline/candidate deben compartir selección/configuración compatible.
- **Provisioning:** versiones/digests/toolchain/licencias/notices/SBOM y disponibilidad
  real de nextest, llvm-cov/LLVM, semver-checks y mutants dentro del runtime.
  Una instalación CI o un ejecutable host no califica el plugin del guest.

Los números de cuotas/timeouts del plan son propuestas hasta quedar decididos y
medidos; no eliminarlos ni copiarlos como garantías ya demostradas. Encarga ADRs
con Context, Decision, Alternatives considered, Consequences y Status antes de
implementar la decisión. No reserves números ADR sin inspeccionar el índice real.

## 6. Invariantes y oráculos que no se negocian

Mantén arquitectura hexagonal, tipos Rust/Serde/Schemars y gateway único cerrado.
Domain/application no dependen de rmcp, Cargo, SQLite o LanceDB. Host concede
roots/permisos; el peer, un ID o un fingerprint nunca conceden autoridad.
No shell, flags libres, herencia de secretos ni source host writable en el guest.
Toda ejecución de proyecto, incluidos tests/build.rs/proc macros y plugins que
los activen, es hostil. Network deny exige enforcement real; sin él, fail-closed.

I/O propio desde handles no-follow/reparse-safe; canonicalización no evita TOCTOU.
Cancel/EOF/timeout/expiry terminan y unen el árbol antes de liberar capacidad;
cleanup incierto domina el resultado. Poll/result no toman el permiso del job.
Prueba IDs ajenos, revocación, carrera cancel/publicación, saturación y restart.
Un resultado retenido no promete reanudar ejecución durable.

Nextest separa selected/passed/failed/ignored/retried/leaked mediante formato
estable de la versión exacta; doctests solo si existe etapa distinta declarada.
Coverage conserva denominadores, exclusiones y provenance; cero datos no es 100%.
SemVer distingue breaking válido de unavailable/incomplete; no copia exit codes
de otra versión. Mutation exige baseline válido, límites por mutante y job y
source host intacto; missed falla, timeout/unviable/incomplete no acreditan limpio.
No se añade mutation automáticamente a los perfiles fast/standard heredados.

Artifacts salen por bytes desde paths guest fijos, sin links ni extracción que
elija destinos host. Prueba XML entities, JSON profundo, URIs externas, HTML activo,
output infinito, dos owners, TTL y exceso de cuotas. JSON autoritativo y formatos
derivados deben corresponder al mismo run. Retener/exportar source o símbolos exige
autoridad host; redacción/secret scan acotado no garantizan ausencia de secretos.

Conserva ADR-050 local_coordinated: sin broker privilegiado, daemon, cuenta o
collector nuevos; sin exclusión OS, CAS, atomicidad visible multiarchivo ni garantía
power-loss. macOS ARM64/APFS es el publisher positivo heredado; CI portable no
acredita otra plataforma. No aumentes promesas de M2 al reutilizar su staging.
SQLite sigue autoritativo; LanceDB derivado. Snapshots con provenance/freshness y
`latest_known`. Runtime no descarga datos, modelos, advisories o herramientas.
stdout solo protocolo; tracing y métricas operativas locales por stderr.

## 7. Disciplina de delegación e integración

Para cada encargo entrega un paquete con:

```text
Task / ID del corte y objetivo concreto
Modelo, CLI, esfuerzo y motivo de asignación
Base Git y hashes de archivos de entrada
Fuentes normativas exactas y decisiones ya aceptadas
Definition of Done y oráculos discriminantes
Archivos/directorios permitidos; interfaces que no puede cambiar
Modo read-only o edición; prohibiciones y permisos
Tests requeridos y recursos exclusivos que necesita
Ubicación del resultado y formato de entrega
Condiciones de bloqueo/escalamiento
```

Exige siempre: Task, Result, Files changed, Tests executed, Evidence, Risks,
Decisions, Open issues. En reviews añade Accepted/Revise, findings P0–P3 por
archivo/línea, hashes revisados, limitaciones y disposición propuesta.

Un worker que edita posee archivos disjuntos o un worktree aislado con base conocida.
El owner exige que en `/Users/cburgosro/Projects` solo exista el checkout
`rust-mcp` de este proyecto: no crees carpetas hermanas `rust-mcp-*`, clones de
publicación ni directorios auxiliares de cobertura allí. Prefiere el checkout
principal con propietarios de archivos disjuntos. Si un worktree es indispensable,
usa una ubicación temporal fuera de `Projects`, registra su ciclo de vida y
retíralo al terminar, después de integrar o preservar cambios, commits y evidencia.
No elimines trabajo único para cumplir esta limpieza; consérvalo dentro del
repositorio principal con ubicación y mecanismo de recuperación explícitos.
Solo el integrador delegado modifica las interfaces comunes e integra parches
aceptados, con las instrucciones concretas de Fable. Ningún worker hace merge a
main, publica o cambia arquitectura por iniciativa propia. La ejecución paralela
no implica que sus ramas independientes formen un árbol probado conjuntamente.

Conserva prompts, inputs, stdout/stderr, exit code, timestamps UTC, session/job ID,
modelo solicitado/observado y hashes. No reutilices `--last` cuando pueda seleccionar
otra conversación; reanuda por ID verificado. Un proceso que sigue corriendo o un
timeout de espera no es un dictamen completado. Antes de reintentar, comprueba si
la ejecución anterior terminó y evita duplicar trabajo/efectos.

Mantén el tablero en `docs/implementation-status.md` y la evidencia en
`docs/validation/M3-matrix.md`; no crees un plan alternativo ni uses YouTrack.
Un registro de delegación enlazado puede identificar propietarios/paquetes sin
sustituir el roadmap. Fable comunica hallazgos, progreso y bloqueos, no monólogos
de coordinación ni resultados inventados. No mantengas workers ociosos.

## 8. Validación y revisión independiente

En cada corte: contrato/tipos → pruebas discriminantes → implementación vertical
completa → focal → revisión independiente → documentación → gate proporcional.
Mocks no sustituyen Cargo/plugins/procesos/SQLite/filesystem reales. Usa controles
positivos que demuestren que los ataques y oráculos del fixture discriminan.

Asigna **un solo propietario del gate Docker**. No lances otro gate Docker o
cliente concurrente, aunque proceda de una CLI diferente. Congela source/scripts/
fixtures/config durante el full; otros agentes pueden revisar paquetes inmutables.
Guarda salidas en directorios nuevos por intento para no arrastrar streams viejos.

El delegado de validación ejecuta el gate existente conforme a `docs/ci.md`:

```text
cargo fmt --check
cargo check --workspace --all-targets --locked --offline
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo test --workspace --all-targets --locked --offline
python3 -B scripts/check-architecture.py
python3 -B scripts/gate.py core
python3 -B scripts/gate.py full
```

Si core/full ya acredita esos comandos, evita ejecutarlos dos veces sin un motivo.
Extiende el gate mediante el worker para incluir todas las nuevas selecciones M3
obligatorias. Invoca explícitamente los tests nativos ignorados; registra conteos
observados, skips/ausencias, versiones y source inventory antes/después. Audit/deny
se ejecutan cuando instalados/configurados; una ausencia en un gate obligatorio
bloquea su calificación. Comprueba las rutas E5/ORT/socket y la imagen aprobada
actuales sin suponer que los paths históricos siguen disponibles.

Califica Inspector y un cliente stock dirigido por modelo: discovery, positivo,
fallo, cancelación activa y Resource/Tasks negociados cuando corresponda. Preserva
intentos fallidos y acota lo que demuestra el prompt usado. Revisión estática de
Claude y Claude como cliente MCP son ejecuciones y evidencias distintas.

Sonnet revisa contratos/cortes; Opus High lifecycle, containment, persistencia y
coherencia final. Gemini audita trazabilidad final M3 y contradicciones de docs
sobre hashes definidos. No repitas paquetes Accepted sin delta material o brecha
concreta. Cambios posteriores no quedan cubiertos por hashes antiguos. Fable
comprueba y dispone errores del reviewer; no incorpora afirmaciones incorrectas
porque el encabezado diga Accepted.

Bug bar G8: P0/P1 bloquean; P2 de seguridad, datos, contrato o gate obligatorio
bloquea hasta resolución. Otros P2 requieren justificación y seguimiento con owner
sin vulnerar DoD; P3 puede quedar trazado. Una revisión no reemplaza pruebas ni
otorga permiso para reducir un criterio. Astra ayuda a resolver dudas, no a eludir
este bug bar.

## 9. Autorización operacional y cierre

Trabaja en una rama `ai/` dedicada a M3, preservando cambios e históricos existentes.
Encarga commits locales coherentes al integrador. No hagas push, PR, tag, release,
publicación crates.io, instalaciones silenciosas ni operaciones YouTrack. M3 no
implica distribuir un quality bundle ni ampliar targets binarios.

Si falta aprovisionamiento, prepara primero versiones/hashes/licencias, comando,
destino, impacto y validación; solicita solo la autorización que falte para esa
acción concreta y continúa trabajo independiente. No rebajes requirements para
obtener un PASS ni te detengas ante decisiones rutinarias ya delegadas.

La autorización de merge de M2 fue específica de M2. Para merge local M3 a main,
comprueba autorización explícita de esta sesión. Si existe, encarga al integrador
merge no-ff, conserva la rama, verifica igualdad de inputs calificados, ejecuta
smoke proporcional y registra commits/hashes/checkout. Si falta, deja implementación
y calificación completas y una propuesta de merge concreta; informa
«calificado, pendiente de integración autorizada», sin marcar el DoD integrado.

El worker documental sincroniza README, CHANGELOG, SECURITY, arquitectura, tools,
security-model, compatibility, client-configuration, ADRs, tablero, matriz M3 y
roadmap. README sigue siendo guía de usuario; evidencia y planificación van en
documentos especializados. Preserva recibos M2 y snapshots históricos; selecciona
explícitamente los logs ignorados que deban entrar en Git y no alteres sus bytes
para maquillar whitespace o hashes.

Solo declara **M3 Done** cuando M3-01..06, todas las casillas del plan y G1–G9 tengan
evidencia final reproducible, reviews dispuestas e integración/smoke registrados.
Entrega un handoff repo-visible con estado real, branch/commits, mapa de
jobs/plugins/formatos, decisiones, comandos/hashes/conteos, matriz cliente/native/CI,
reviews, riesgos, rollback y límites. Si hay un bloqueo, identifica la condición
reproducible, dependientes y acción necesaria; no declares éxito parcial como cierre.

Empieza ahora por verificar tu identidad/configuración, leer las fuentes y
delegar la inspección de baseline y disponibilidad. Presenta después el primer
reparto acotado de trabajo y ejecuta la implementación autorizada. No te limites
a devolver otro plan. Al finalizar M3, detente: no ejecutes el prompt M4.
