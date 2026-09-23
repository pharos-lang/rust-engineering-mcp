# Plan y encargo — Runtime único y portabilidad gradual 1.0, orquestado por Astra

Estado: **plan preparado; implementación no iniciada**. Fecha: 2026-09-18.

Este documento es el punto de entrada de una futura sesión de **GPT-6 Astra**.
Su creación no inicia los trabajos, no cierra SonarCloud/M8, no modifica ADRs
aceptados y no autoriza publicaciones. Ejecutarlo requiere una instrucción
explícita del owner. Al retomarlo, verificar el repositorio: las referencias de
entrada son una fotografía, no una afirmación del estado futuro.

## 1. Objetivo y secuencia obligatoria

Entregar una instalación sencilla del Rust Engineering MCP con un runtime lógico
único, conservar los contratos y garantías actuales y habilitar hosts por etapas:

| Etapa | Versión objetivo | Host que se incorpora | Condición para comenzar |
| --- | --- | --- | --- |
| Preparación | Sin release | Ninguno | Encargo de ejecución y baseline posterior a los ajustes Sonar aceptada |
| macOS | `1.0.0-rc.1` | macOS ARM64/APFS, sobre la familia actualmente calificada | Preparación aprobada; no requiere SSH externo |
| Linux | `1.0.0-rc.2` | Linux x86_64, filesystem local calificado | macOS listo, notificación al owner y acceso SSH Linux entregado por él |
| Windows | `1.0.0-rc.3` | Windows x86_64 nativo/NTFS | Linux listo, notificación al owner y acceso SSH Windows entregado por él |

Los tags correspondientes serían `v1.0.0-rc.1`, `v1.0.0-rc.2` y
`v1.0.0-rc.3`. Las RC son acumulativas: rc.2 conserva macOS y rc.3 conserva
macOS y Linux. No reutilizar ni mover un tag publicado para corregir un fallo.
Si una RC ya publicada necesita otro número, solicitar al owner la nueva
secuencia; el número de RC no sustituye al identificador estable de la fase.

**Puertas humanas obligatorias:** al terminar macOS, informar y detener la
transición a Linux hasta recibir su acceso. Al terminar Linux, hacer lo mismo
para Windows. No pedir los dos accesos por adelantado. No escanear redes ni
intentar reutilizar credenciales encontradas en el equipo.

### Qué significa «listo»

- `qualified_local`: implementación integrada en la rama de esta fase, revisión
  independiente aceptada, gates exigidos aprobados sobre los bytes finales,
  instalación limpia y rollback ensayados, evidencia persistida.
- `published`: además, autorización específica del owner, publicación realizada
  y verificación de los bytes descargados. No confundirlo con `qualified_local`.
- Se puede solicitar el siguiente acceso tras `qualified_local`; no es necesario
  publicar una release para comenzar las pruebas del siguiente host.
- La versión estable `1.0.0` y su publicación quedan fuera de este encargo.

## 2. Autoridad, alcance y límites

El owner define objetivos, acceso a máquinas, permisos de publicación y cambios
materiales de alcance. Astra conserva la responsabilidad de coordinación,
priorización y aceptación de entregables, **pero no implementa el producto**.

Al ejecutar este encargo, documentar en la instrucción de sesión y después en
`AGENTS.md` las excepciones de rol aquí solicitadas: Astra como orquestador;
Sol/Terra/Luna y, por autorización del owner (2026-09-23), Claude Opus 5.5 y
Claude Sonnet 5 como workers según tarea. La regla previa «Main Sol High» no se
reescribe silenciosamente. El host selecciona realmente el modelo principal;
el agente no puede afirmar que cambió su propio modelo. Si la sesión no está
configurada con Astra, informar y no sustituirlo por iniciativa propia.

El mandato futuro autoriza los cortes locales descritos, no hereda permisos de
push, PR, merge a la rama principal, tags, publicación de imágenes ni releases
de otros prompts históricos. Cada efecto externo necesita autorización vigente
con target concreto. La integración entre ramas privadas del encargo sí forma
parte del flujo local, una vez iniciada su ejecución.

### Incluido

- Un runtime lógico versionado con variantes por arquitectura y admisión cerrada.
- Aprovisionamiento explícito, verificable y separado de `serve` y de las tools.
- Binario nativo del servidor en cada host; ejecución del código de proyecto
  dentro del guest Linux aprobado por el Execution Gateway.
- Camino guiado de instalación/configuración y diagnóstico; reutilizar CLI y
  contratos existentes antes de proponer interfaces nuevas.
- Adaptadores de I/O, estado privado, procesos, transporte y empaquetado requeridos
  para cada host; pruebas positivas y adversas en las máquinas reales.
- Automatización de pruebas SSH con evidencia persistente y reanudación.
- Documentación pública, CI, seguridad, migración y rollback de cada fase.

### No incluido por defecto

- Ejecutar `build.rs`, proc macros, tests o analyzer directamente en el host para
  eludir el sandbox. «Windows nativo» describe el servidor y su I/O, no autoriza
  compilar o ejecutar proyectos Windows fuera del guest Linux.
- Soporte macOS Intel, Linux ARM64, Windows ARM64, filesystems de red, Docker
  remoto, contenedores Windows o targets Mach-O/PE de las tools del guest.
- Tratar WSL2 como evidencia de un servidor Windows nativo: son fronteras distintas.
- Distribuir modelos, catálogo oficial o binarios con el perfil semántico `local`
  sin la decisión y calificación adicional que ya exige el repositorio.
- Actualización general de dependencias, nuevas tools MCP, cambio de protocolo,
  HTTP remoto, reescritura del core o ampliación de la autoridad del peer.
- Compra de hardware, servicios de pago, cómputo alquilado, API de modelos,
  créditos adicionales, recargas automáticas o aumento de planes.

Si una limitación hace inviable una fase con estas fronteras, presentar un
No-go técnico con evidencia y opciones; no convertir una reducción de soporte
en cumplimiento de la fase.

## 3. Baseline y lectura inicial

Fotografía documental usada para preparar este plan: commit `755d661c`,
workspace `0.9.0-rc.1`, Rust `1.98.1`, `rmcp` `3.2.0`. Los ajustes Sonar y el
README tienen trabajo paralelo: esta no es necesariamente la futura base.
Reconciliar la denominación conversada `0.9.0-rc` con manifests y validadores
que admiten `-rc.N`; no crear ahora un tag para resolver esa diferencia.

Antes de delegar implementación, el worker técnico debe leer íntegramente la
[especificación](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/spec/rust-engineering-mcp-propuesta-v0.3.md)
(histórico a `51fa602e`),
[AGENTS.md](../AGENTS.md), el tablero
(histórico: [implementation-status.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/implementation-status.md);
vigente: [`.planning/deferred-commitments.md`](deferred-commitments.md) y
`docs/reference/compatibility.md`), las
instrucciones locales adicionales y los ADRs aplicables. Astra recibe su mapa
de discrepancias y referencias; no sustituye la evidencia por el resumen.
Astra debe leer directamente, sin implementar, este prompt, `AGENTS.md`,
ADR-087 y su sucesor, el tablero y el control/handoff vigentes. Antes de aceptar
cada puerta debe comprobar los recibos exactos, sus hashes/source inventory,
las disposiciones de revisión y las autorizaciones aplicables; el reporte de
un worker por sí solo no basta para aceptar una fase.

Revisar al menos:

- [ADR-048: frontera de artifact](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-048-0.1.0-qualification-and-artifact-boundary.md),
  [ADR-050: mutación coordinada](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-050-local-coordinated-mutation.md) y
  [ADR-087: alcance actual de hosts](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-087-1.0-host-scope.md)
  (histórico a `51fa602e`; los ADR vigentes viven en `docs/architecture/decisions.md`
  y sus capítulos).
- ADRs de I/O protegido, Execution Gateway, sandbox, journal, artifacts privados,
  contratos/freezing, distribución y admisión/aprovisionamiento M4–M6.
- [Admisión M5](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-077-m5-runtime-admission.md),
  [aprovisionamiento M6](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-082-m6-runtime-provisioning.md) y
  [admisión M6](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-085-m6-runtime-admission.md).
- [Compatibilidad](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/compatibility.md)
  (histórico; vigente: `docs/reference/compatibility.md`),
  [tools](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/tools.md)
  (histórico; vigente: `docs/reference/tools.md`),
  [modelo de seguridad](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/security-model.md)
  (histórico; vigente: `docs/architecture/execution-and-security.md`),
  [CI](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/ci.md)
  (histórico; vigente: `docs/development/testing.md`),
  [publicación](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/publication.md)
  (histórico; vigente: `docs/operations/release-verification.md`) y
  [configuración](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/client-configuration.md)
  (histórico; vigente: `docs/guides/clients.md` y `docs/guides/configuration.md`).
- [Política de evidencia](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/validation/README.md)
  (histórico a `51fa602e`), matriz/handoff vigentes M8,
  `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, tests, fixtures y workflows.

Inspeccionar `git status`, worktrees, ramas y hashes reales. Confirmar con el
owner la baseline integrada tras Sonar; no capturar a medias cambios de otros
agentes ni incorporar el README anterior sin revisar su diff. Las secciones
históricas del tablero pueden estar desactualizadas: resolver por código,
tests y recibos vigentes, sin declarar todo M8 terminado por inferencia.

Hechos de partida que requieren trabajo, no suposiciones:

1. ADR-087 restringe el soporte positivo a macOS ARM64/APFS. Un ADR sucesor,
   revisado antes del código correspondiente, debe habilitar cada ampliación.
2. Admisión global de la imagen M6 no equivale a calificación conjunta M1–M6.
   Existen comprobaciones de identidad específicas por familia de tools.
3. Hay rutas y perfiles ligados a Linux/AArch64, y adaptadores de filesystem y
   stores que fallan cerrados fuera de macOS. Cambiar `cfg` no acredita seguridad.
4. La CI inspeccionada tiene macOS ARM64 y Linux x86_64; restaurar compilación
   Windows no basta para ofrecer allí capacidades funcionales.
5. Los gates históricos no califican bytes nuevos, otro host u otra arquitectura.

Mapa inicial para asignar paquetes de implementación (revalidar en la baseline):

| Frontera | Archivos o directorios iniciales |
| --- | --- |
| I/O, captura y stores | `crates/project-adapter/src/filesystem.rs`, `filesystem/macos/`, `catalog_store.rs`, `mutation_store.rs`, `quality_artifact_store.rs`, `cargo_vendor.rs` |
| Estado y procesos del gateway | `crates/execution-adapter/src/state.rs`, `supervisor.rs`, `lsp_session.rs`, `lib.rs` |
| Admisión, metadata y perfiles por tool | `crates/execution-adapter/src/rust_gateway.rs`, `analyzer_gateway.rs`, `performance_gateway.rs`, `coverage_gateway.rs`; puertos/metadata correspondientes en application/domain |
| Provisioning y oráculos | `fixtures/rust-runtime/`, scripts de build/runtime M2–M6 y tests de integración/security |
| Gates y artifacts | `scripts/gate.py`, `release-artifact.py`, `release-smoke.py`, `.github/workflows/ci.yml`, `release-candidate.yml` |

Los paths abreviados en una fila son relativos al directorio explícito de esa
fila. No asumir que el empaquetado es portable: sus verificadores actuales
incluyen supuestos Darwin/ARM64/Mach-O y los harnesses usan primitivas POSIX.
Cada worker debe contrastar decisiones dependientes de versión con documentación
oficial vigente y con `Cargo.lock`/inventario real; guardar las fuentes usadas
en su propuesta, sin copiar ejemplos incompatibles de Internet.

## 4. Equipo y reglas de delegación

Se usan exclusivamente las suscripciones indicadas por el owner: **Codex Pro,
Claude Max 5x y Google AI Pro**. Codex mediante subagentes del host; Claude
mediante `claude`; Gemini mediante **`agy`**, no otro ejecutable por suposición.
No es obligatorio consumir los tres proveedores en todas las tareas.

| Rol | Modelo solicitado | Trabajo y esfuerzo orientativo |
| --- | --- | --- |
| Orquestador | GPT-6 Astra | Coordinar dependencias, paquetes, cuotas, evidencias y handoffs; medium ordinario, high ante bloqueos de coordinación, si el host lo permite |
| Líder técnico worker | GPT-5.6 Sol | Propuestas de ADR, contratos, diseño de adapters, debugging difícil; high |
| Implementador | GPT-5.6 Terra | Cortes delimitados con contrato aprobado; medium; high si la dificultad lo justifica |
| Worker acotado | GPT-5.6 Luna | Inventarios, fixtures simples, documentación y verificaciones deterministas; esfuerzo admitido proporcional |
| Integrador/QA | Sol o Terra, separado del autor al revisar | Integración privada, reconstrucción y gates; medium/high según riesgo |
| Worker de fronteras y seguridad | Claude Opus 5.5 | Prototipos y diseño de primitivas de seguridad por plataforma (no-follow/beneath, reparse-safe, identidad de roots, locks, durabilidad), tests adversariales, debugging complejo cross-platform, soundness de `unsafe`/FFI y borradores de decisión junto a Sol; high |
| Worker implementador Claude | Claude Sonnet 5 | Cortes Rust delimitados con contrato aprobado, tests unitarios/integración/adversariales, harnesses Python, inventarios y documentación técnica verificada contra el código; medium, high si la dificultad lo justifica |
| Revisión habitual | Claude Sonnet 5 | Review read-only de diff, tests y evidencia; esfuerzo equivalente admitido |
| Revisión de seguridad/arquitectura | Claude Opus 5.5 | Review read-only de fronteras y ADRs; esfuerzo alto admitido, sin `ultracode` |
| Revisión mecánica | Claude Haiku | Cotejo read-only de referencias, tablas y consistencia; esfuerzo bajo admitido |
| Worker Gemini | Gemini 3.8 Flash / 3.7 Flash / 3.6 Flash | Investigación, pruebas, harnesses y documentación en archivos asignados; escoger modelo y esfuerzo según capacidad real y complejidad |

Los nombres son la selección solicitada, **no prueba de disponibilidad ni de
identificador CLI**. En preflight registrar versión de `claude`/`agy`, ayuda,
modelos habilitados, IDs exactos y esfuerzos aceptados. Para Claude, el owner
selecciona Opus 5.5 y Sonnet 5 (IDs esperados `claude-opus-5-5` y
`claude-sonnet-5`, por `claude --model <id>` con `--effort`); confirmarlos en
preflight y registrar en `AGENTS.md` la excepción frente al revisor Opus 5 que
fija hoy.
No inventar flags ni asumir que `agy` expone los comandos de Gemini CLI.
Modelo requerido ausente: informar y pedir elección, sin sustitución silenciosa.
Para Gemini no deducir capacidad ni cuota del número del modelo: asignar tras
una tarea piloto pequeña y comprobable.

### Límites de Astra

- Puede leer estado, asignar tareas, solicitar revisiones, aceptar/rechazar
  entregables según evidencia, registrar decisiones y comunicar avances.
- Solo escribe metadatos de coordinación y seguimiento. No edita código,
  fixtures, Dockerfiles, workflows, ADRs de implementación ni README del producto.
- Delega propuestas técnicas, resolución de bugs, integración y ejecución de
  gates. Si no queda worker disponible, hace checkpoint y pausa; no absorbe la
  implementación. No sustituye una revisión independiente con su propio criterio.
- Sol propone decisiones; un reviewer independiente las examina; Astra verifica
  su encaje y el owner decide cuando cambia el alcance o la autoridad concedida.

El owner autoriza (2026-09-23) a Claude Opus 5.5 y Claude Sonnet 5 como
workers implementadores, además de su rol de revisión. Astra sigue siendo el
único orquestador: asigna los paquetes de Claude igual que los de Codex y
Gemini, y Claude no coordina, no asigna trabajo ni acepta entregables. Un worker
Claude edita solo el paquete disjunto asignado en su rama/worktree, sin push,
merge, tags ni integración en la rama del integrador; entrega el informe
obligatorio y el integrador (Sol o Terra) integra. Asignarlo donde aporta más
capacidad: Opus 5.5 para fronteras de seguridad, prototipos de plataforma,
adversarial testing y debugging difícil; Sonnet 5 para cortes delimitados, tests,
harnesses y documentación. Una sesión que implementó un corte no puede ser su
revisor independiente: si Claude es autor, la revisión la hace otra sesión y,
para cortes de seguridad o arquitectura, además un revisor de otro proveedor
(Sol). La cuota de Claude Max 5x es compartida: reservar capacidad para las
revisiones Opus obligatorias antes de asignar implementación a Opus. Gemini
puede editar solo el paquete disjunto asignado.

### Paquete obligatorio por tarea

```text
Task ID / phase / attempt
Objective and Definition of Done
Baseline commit / input hashes / worktree / target host
Allowed files and forbidden interfaces
Approved ADRs, contracts and security restrictions
Provider / exact model / supported effort / quota checkpoint
Dependencies / commands and tests required
Expected outputs / evidence paths / checkpoint path
Stop conditions / reviewer / integration owner
```

El informe de salida conserva `Task`, `Result`, `Files changed`, `Tests executed`,
`Evidence`, `Risks`, `Decisions`, `Open issues`, añadiendo commit, intento y
siguiente acción exacta. Un resultado de modelo sin artifacts comprobables no
habilita Done. Los workers no subdelegan sin registro y permiso del orquestador.

Concurrencia inicial: hasta **tres workers**, siempre limitada por slots reales,
cuotas y recursos. Un integrador por rama y un escritor por archivo; mientras
corre el gate final no hay writers en su checkout. Empezar con una sesión por
CLI externo; subir solo con evidencia de beneficio. No ejecutar varios full
gates simultáneamente en la misma máquina.

## 5. Estado persistente: reanudar sin depender del chat

Usar el identificador de programa `RUP-1.0` y fases `MAC`, `LINUX`, `WINDOWS`,
independientes de sus números RC. No crear un segundo backlog incompatible:
el tablero repo-visible histórico
([`docs/implementation-status.md` en 51fa602e](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/implementation-status.md))
fue retirado por la limpieza de documentación de 2026-09; sus filas de
"Decisiones Pendientes" viven ahora en `.planning/deferred-commitments.md` y
las capacidades/limitaciones vigentes en `docs/reference/compatibility.md` /
`docs/reference/limits.md`. Añadir aquí una sección RUP y enlaces al paquete
en el documento vivo aplicable al iniciar la ejecución, en vez de recrear un
tablero plano.

Al iniciar la ejecución crear este paquete, siguiendo el layout de evidencia
del repositorio y sin guardar secretos ni salidas voluminosas en Git:

```text
.planning/rup-1.0-evidence/
  matrix.md                 # Tareas, dependencias, estado, evidencia y aceptación
  handoff.md                # Checkpoint vigente, siguiente acción y bloqueos
  control.json              # Estado operativo reconstruible, schema_version=1
  providers.json            # Disponibilidad/cuotas observadas, nunca credenciales
  decisions.md              # Autorizaciones y disposiciones, referencia a ADRs
  events.jsonl              # Eventos de coordinación append-only, sin secretos
  tasks/<ID>/task.md         # Paquete de delegación
  tasks/<ID>/handoff.md      # Estado parcial y recovery de esa tarea
  tasks/<ID>/report.md       # Resultado y aceptación/rechazo
  hosts/<alias>/inventory.json
  phases/<phase>/matrix.md
  phases/<phase>/receipts/<attempt-id>.json
  phases/<phase>/handoff.md
  history/inventory.json    # Índice de intentos superados, según política vigente
```

Estos archivos **se crearán al ejecutar**; este plan no finge progreso creando
recibos vacíos. El worker de seguimiento implementará una validación sencilla
para el esquema, estados, referencias e invariantes; no un servicio permanente
ni una base de datos nueva. `control.json` es la fuente operativa; matriz y
handoff son sus vistas humanas, actualizadas en el mismo checkpoint por un único
escritor designado. Usar revisión monotónica y reemplazo atómico del control;
si un cierre interrumpe la actualización de las vistas, reconstruirlas de control
y eventos antes de delegar. No resolver discrepancias por «último chat».

### Datos mínimos

**Programa:** `schema_version`, `revision`, `updated_at_utc`, baseline y tip de
integración, fase activa, estado, autorizaciones, siguiente tarea y causas de
espera. Estados de programa: `planned`, `running`, `waiting_quota`,
`waiting_linux_access`, `waiting_windows_access`, `waiting_owner`, `complete`.

**Tarea:** ID, título, dependencias, Definition of Done, estado, proveedor/modelo/
esfuerzo observados, worker/session ID si existe, branch/worktree, archivos
reservados, intento, inicio, último checkpoint, commits, cambios pendientes,
tests/exit codes, evidencia con hashes, reviewer, findings abiertos, siguiente
comando o acción, condición de desbloqueo. Estados: `todo`, `ready`, `running`,
`checkpointed`, `waiting_quota`, `waiting_access`, `blocked`, `review`, `done`.
En el tablero general las esperas se reflejan como `Blocked` con causa explícita.

**Proveedor:** plan declarado, superficie real, modo de autenticación verificado
sin exponerlo, modelo/CLI, estado `available|low|exhausted|unknown|auth_required`,
momento y fuente de observación, ventana/unidad si se conocen, consumo restante
si se conoce, reset anunciado o `null`, tareas afectadas, próxima comprobación.
`unknown` no es cuota cero ni cuota ilimitada. No convertir tokens observados en
porcentaje de suscripción ni sumar porcentajes de ventanas diferentes.

**Job local/remoto:** `job_id`, task/attempt, host alias, boot/session identity,
PID y marcador de inicio, commit y source inventory, comando normalizado,
binario y SHA, runtime/arquitectura/perfil, start/heartbeat/end, estado, exit,
artifacts y hashes. Estados: `submitted`, `running`, `succeeded`, `failed`,
`cancelled`, `lost`, `unknown`. Un PID solo no identifica un job tras reinicio.

No versionar IPs sensibles, claves, contraseñas, tokens, cookies, sesiones CLI
crudas, rutas privadas de autenticación ni environment dumps. Usar alias SSH;
la configuración efectiva queda fuera del repositorio. Los logs crudos se
guardan en un directorio local privado persistente y explícito, no exclusivamente
en `/tmp` o `target/`; registrar hashes y ubicación mediante alias. Respetar la
política de historia al retirar intentos, conservando el material no integrado.

### Frecuencia y recuperación

Checkpoint antes de lanzar una tarea, tras cada unidad verificable, al finalizar
un comando largo, antes de integrar, al cerrar revisión y ante cuota/bloqueo.
Para tareas largas pedir checkpoint en cada subcorte y al menos cada 20–30
minutos cuando el agente esté activo; un bloqueo abrupto puede impedirlo, por
eso se registran inputs y outputs antes de la llamada. El timeout de un modelo
no convierte en fallido un build remoto que todavía está corriendo.

Un checkpoint guarda tanto commits como cambios no commiteados: inventario,
diff/patch y archivos nuevos con hashes en almacenamiento privado. Nunca usar
`reset --hard`, `clean`, checkout destructivo ni stash automático sobre trabajo
ajeno. Los commits WIP, si se necesitan, van solo en la rama privada de la tarea.

Antes de cambiar de worker, confirmar que el anterior terminó o fue detenido y
verificar que no tiene procesos escribiendo; entonces liberar la reserva de
archivos. Un bloqueo por cuota no libera automáticamente el worktree.

### Procedimiento de nueva sesión

1. Confirmar que el host configuró GPT-6 Astra; leer este prompt, instrucciones
   vigentes, control, handoff, decisions y filas de la fase actual.
2. Verificar Git, ramas, worktrees, hashes y cambios pendientes; no asumir que
   el tip, el entorno o el estado de Sonar siguen iguales.
3. Reconciliar tareas `running` con workers y jobs reales. Consultar el recibo
   remoto antes de relanzar. `unknown` exige diagnóstico, nunca éxito implícito.
4. Revalidar autenticación y cuota mediante mecanismos oficiales disponibles.
   Reutilizar sesión CLI solo si sigue siendo válida; los artifacts permiten
   iniciar otra sesión sin reconstruir toda la conversación.
5. Restaurar reservas solo para trabajo realmente vivo. Seleccionar una tarea
   `ready` de la fase autorizada o continuar el siguiente subcorte del checkpoint.
6. Delegar un paquete reducido con archivos/hashes y estado parcial; no reenviar
   todos los transcripts. Registrar el nuevo intento sin borrar el anterior.
7. Publicar un resumen al owner: último cierre, trabajo en curso, esperas y
   siguiente acción. Si falta acceso de la siguiente fase, mantener esa puerta.

## 6. Política de cuotas y continuidad entre proveedores

Las cuotas pertenecen a las cuentas, pueden compartir consumo con otras sesiones
del owner y no son presupuestos aislados por worker. No prometer una cantidad
fija de tareas por plan. El orquestador también consume la cuota Codex.

En preflight verificar que las CLI usan la suscripción y que no existe fallback
facturable activo. Si no se puede comprobar sin interacción del usuario, pedir
esa confirmación antes de consumir modelos. No modificar la cuenta, eliminar
credenciales ni imprimir secretos. No comprar créditos ni cambiar el plan.

La [documentación oficial de Codex](https://learn.chatgpt.com/docs/pricing)
explica que el consumo depende de modelo/tarea/contexto y remite a la vista de
uso; en una sesión CLI activa permite consultar `/status`. Esto no demuestra
que exista un endpoint de cuota automatizable desde este host. Para `claude` y
`agy`, inspeccionar ayuda y documentación oficial de la versión instalada y
usar su información real; si no la exponen, registrar `unknown` y pedir al
owner la lectura correspondiente. Nunca explorar endpoints privados ni hacer
llamadas de inferencia repetidas para sondear el límite.

Reglas del scheduler:

1. Antes de una tanda, observar cuota cuando esté disponible y registrar la
   ventana relevante. Reservar capacidad para un cierre/review/checkpoint;
   esa reserva es operativa, no una garantía matemática sobre cuotas opacas.
2. Con señal `low`, no abrir cortes largos: cerrar o guardar los activos y usar
   tareas pequeñas. No activar modalidades de consumo acelerado por defecto.
3. Ante un límite confirmado, guardar el error saneado, instante y reset
   declarado; marcar proveedor y tareas `waiting_quota`. No reintentar en bucle.
4. Si es un fallo transitorio, distinguirlo de auth y cuota; como máximo dos
   reintentos espaciados para una llamada idempotente. No repetir herramientas
   con efectos sin reconciliar primero sus resultados.
5. Se pueden asignar **otras tareas independientes** a proveedores disponibles.
   Una tarea parcial solo migra tras checkpoint, liberación de su escritor y
   comprobación de que el nuevo rol/modelo está autorizado y es adecuado.
6. Mantener el revisor independiente. Una revisión Opus requerida no se da por
   cumplida por Haiku ni por el autor porque se agotó cuota. Esperar o solicitar
   aprobación para otro revisor cualificado, registrando el cambio.
7. Si Astra/Codex agota cuota, no cambiar de orquestador por iniciativa propia.
   Workers ya iniciados pueden terminar su paquete y guardar evidencia; no
   iniciar cadenas de trabajo ni nuevas integraciones sin coordinación.
8. Si no queda trabajo independiente autorizado, guardar un handoff y terminar
   la sesión con el motivo y la condición de reanudación. Si se conoce reset,
   anotarlo en UTC y hora local; no inventar una fecha para `null`.

No instalar un daemon de polling de cuotas. Sin un mecanismo de programación
habilitado y autorizado, **no prometer reanudación automática**: el owner vuelve
a iniciar una sesión con este prompt. El progreso persistido debe hacer que
esa reanudación sea segura aunque el proveedor haya perdido su conversación.

## 7. Arquitectura objetivo que los workers deben concretar

Un runtime único significa **una versión lógica y un inventario controlado**, no
un binario universal ni una sola identidad válida en todas las arquitecturas:

| Fase | Binario host | Guest de ejecución |
| --- | --- | --- |
| MAC | `aarch64-apple-darwin` | Linux ARM64, variante nativa calificada |
| LINUX | `x86_64-unknown-linux-gnu` | Linux AMD64, nueva variante calificada |
| WINDOWS | `x86_64-pc-windows-msvc` | Linux AMD64 vía backend local calificado, inicialmente evaluar Docker Desktop/WSL2 |

La misma variante AMD64 puede reutilizarse en Linux y Windows, pero sus fronteras
de host, integración con Docker, filesystem y lifecycle se califican aparte.
La tabla fija el objetivo propuesto, no soporte ya existente ni aprobación de
una configuración Windows aún no probada.

El worker Sol propondrá un descriptor tipado/versionado que vincule versión
lógica, variante, manifiesto OCI, digest de imagen por plataforma, identidad
local del engine, toolchain/plugins, perfiles sandbox y capacidades calificadas.
**El digest de un manifiesto OCI y `docker inspect .Id` no son intercambiables.**
Verificar su relación y origen; no aceptar una imagen por tag mutable o porque
contenga un ejecutable con nombre esperado. La política de confianza del
descriptor debe impedir que el proyecto o el peer añadan imágenes admisibles.

No cambiar identidades a base de reemplazos masivos de strings. Inventariar y
adaptar las comprobaciones globales y por tool, metadata de resultados, snapshots,
fixtures, perfiles y harnesses. Mantener las versiones reales de cada plugin;
runtime único no implica que todos compartan versión.

Cada operación sigue usando su perfil mínimo: una sola imagen **no** unifica
grants de escritura, acceso de red, syscall permissions o límites de recursos.
Mantener deny-by-default, entorno saneado, stdout de protocolo, tiempos y
cancelación del árbol de procesos, no-follow/reparse-safe y rollback conservador.

La instalación debe llegar a esta experiencia, con sintaxis exacta definida y
probada durante MAC-04, no inventada ahora:

1. Obtener el binario nativo y el descriptor por un canal verificable.
2. Ejecutar una operación explícita de preparación que verifique o adquiera
   la única variante necesaria; mostrar descargas, tamaños y permisos.
3. Generar configuración del cliente para roots escogidas por el usuario y
   grants mínimos; no sobrescribir configuración existente ni conceder writes
   de forma implícita. Preferir generar los argumentos actuales de `serve`
   antes de introducir un nuevo almacén de autoridad/configuración.
4. Ejecutar diagnóstico y una llamada MCP real. Distinguir falta de engine,
   identidad no admitida, datos offline ausentes y falta de grant.

`serve` y las tools no descargan ni actualizan runtimes, advisories, modelos,
catálogos o dependencias. El vendor/directory source sigue siendo específico
del proyecto; ayudar a prepararlo explícitamente no lo convierte en parte
universal de la imagen. Preparar imagen distribuible y canal mínimo; Homebrew,
WinGet/Scoop y otros canales adicionales no bloquean las RC salvo elección
expresa del owner.

## 8. Fase P — Preparación y control

Las filas siguientes son paquetes que el orquestador debe descomponer si no
caben en un corte verificable. No dar fechas basadas en el número de agentes.

| ID | Dependencias | Responsable delegado | Entregable y aceptación |
| --- | --- | --- | --- |
| P-01 | Inicio autorizado | Sol + inventario Luna o Sonnet 5 | Baseline posterior a Sonar confirmada, hash, censo de tools/estabilidad, contratos y deuda; diferencias de docs resueltas o registradas |
| P-02 | P-01 | Worker de seguimiento + Astra | Paquete persistente, validación del control, inventario CLI/modelos/cuotas; simulación de checkpoint y reanudación sin pérdida |
| P-03 | P-01 | Sol, review Opus | ADR propuesto de runtime/distribución y sucesor de ADR-087 con fases, invariantes, alternativas y rollback; decisiones aceptadas antes de código afectado |
| P-04 | P-01, P-02 | Terra o Gemini | Runner local de jobs/recibos y diseño de transporte SSH; prueba local de interrupción/reconexión, sin necesitar aún hosts externos |
| P-05 | P-02, P-03 | Integrador/QA | Gate baseline proporcional y mapa de suites por capacidad; defectos heredados separados de regresiones; plan MAC listo |

La ausencia temporal de SSH Linux/Windows **no bloquea P ni MAC**. No provisionar
ni implementar los adapters de las fases posteriores antes de sus puertas.
El descriptor y el runner pueden diseñarse con esas variantes en mente sin
afirmar que ya funcionan.

## 9. Fase MAC — `1.0.0-rc.1`

| ID | Depende de | Worker sugerido | Trabajo y criterio de aceptación |
| --- | --- | --- | --- |
| MAC-01 | P-05 | Sol/Terra | Censar identidades y perfiles por familia, CLI/metadata y harnesses. Cada dependencia del runtime tiene dueño y prueba de migración |
| MAC-02 | MAC-01 | Terra, investigación Gemini; Sonnet 5 para inventario/licencias/SBOM | Construir o seleccionar imagen unificada ARM64 con inputs fijados, inventario, hashes, licencias/notices/SBOM y receta verificable. No asumir que M6 ya está calificada |
| MAC-03 | MAC-02 | Sol/Terra | Integrar descriptor/admisión y provenance por tool. Rechazar descriptor, imagen, arquitectura o plugin alterados; conservar grants/perfiles independientes |
| MAC-04 | MAC-03 | Terra o Sonnet 5; docs Gemini/Luna/Sonnet 5 | Preparación explícita, configuración de cliente y doctor. Instalación limpia sin compilar imagen ni binario para el usuario; idempotencia, paths con espacios, reintento de descarga y rollback probados |
| MAC-05 | MAC-03 | QA independiente (Sonnet 5 si no fue autor del corte) | Ejecutar suites M1–M6 y seguridad sobre la misma identidad unificada, más features/gates exigidos por la baseline. Recibos identifican cada tool/capacidad y limitaciones |
| MAC-06 | MAC-04, MAC-05 | Integrador/QA + Sonnet/Opus read-only | Cerrar findings, reconstruir binario final, core/full, clientes, recuperación y soak aplicable; contrato sin cambios no autorizados; cero P0/P1 abiertos |
| MAC-07 | MAC-06 | Integrador + documentación | Candidato rc.1, paquete de instalación verificable, migración/rollback y README; registrar `qualified_local`, informe final y puerta de acceso Linux |

Las suites actuales están ligadas a imágenes diferentes: actualizar su composición
para que MAC-05 pruebe realmente la imagen unificada. Un full verde que aún
ejecuta las imágenes antiguas no cierra esta fase. No retirar artifacts previos
hasta probar rollback y conservación del estado/journals. No cambiar formatos
persistidos sin estrategia explícita de migración y reversión.

La revisión de seguridad cubre especialmente nuevos paths de adquisición,
verificación de identidad y generación de configuración. Para distribución real
se requiere autorización del canal/registry y verificación desde descarga;
antes de ella se permite un ensayo con archivo local cuyo origen/hashes estén
registrados, etiquetado como tal, nunca «publicado».

**Mensaje obligatorio al owner al cerrar MAC-07:**

> macOS está listo para `1.0.0-rc.1` en el alcance calificado. Estado de
> publicación: [...]. Commit/binario/runtime: [...]. Gates y evidencia: [...].
> Limitaciones: [...]. El programa queda en `waiting_linux_access`.
> Puedes proporcionar ahora el alias/host SSH Linux, usuario, puerto y el
> mecanismo de autenticación local autorizado, además de la huella del host
> por un canal confiable. No envíes claves privadas ni contraseñas al repositorio.

No comenzar LINUX-01 hasta recibir ese acceso. La espera es prevista, no fallo
de implementación. Entregar el checkpoint y finalizar la sesión si corresponde.

## 10. Fase LINUX — `1.0.0-rc.2`

| ID | Depende de | Worker sugerido | Trabajo y criterio de aceptación |
| --- | --- | --- | --- |
| LINUX-01 | MAC-07 + acceso del owner | Terra/QA | Preflight SSH read-only; OS/kernel/CPU/FS/engine/cgroup/seccomp, disco disponible y permisos; inventario y configuración concreta aceptados |
| LINUX-02 | LINUX-01 | Sol u Opus 5.5 como autor; review independiente del otro proveedor + Opus 5.5 read-only si el autor fue Sol | ADR/plataforma y prototipos de I/O relativo a handles, no-follow/beneath, identidad de roots, locks y durabilidad; oráculos positivos/adversos en el host |
| LINUX-03 | LINUX-02 | Sol/Terra; Sonnet 5 para cortes delimitados y Opus 5.5 para writer/journal/recovery | Portar lectura/captura/vendor, catálogo/SQLite, estado privado, artifacts, journal y writer. Completar preview/commit/replay/recovery, sin reducir garantías silenciosamente |
| LINUX-04 | LINUX-01 + descriptor MAC | Terra/Gemini/Sonnet 5 | Variante Linux AMD64 con inventario y perfiles correctos; toolchain, rutas LLVM, helpers, scanner, analyzer, coverage y profiling recalibrados, sin emulación como evidencia nativa |
| LINUX-05 | LINUX-03, LINUX-04 | Terra/QA | Integrar gateway/state root, supervisor y sesión LSP no bloqueante, engine endpoint, cuotas/procesos y CLI/instalación. Calificar prohibición de red y cleanup bajo timeout/cancelación |
| LINUX-06 | LINUX-05 | QA + review independiente | Full nativo, contratos/clientes, instalación/upgrade/rollback; regresión macOS en el mismo tip de integración; todas las capacidades prometidas tienen evidencia |
| LINUX-07 | LINUX-06 | Integrador + documentación | Empaquetado Linux, CI y docs; candidato acumulativo rc.2; `qualified_local`, handoff y puerta Windows |

Propuesta inicial de host: distribución Linux x86_64 mantenida, filesystem local
como ext4 y Docker Engine compatible con las capacidades requeridas. La versión
mínima de distro/kernel/glibc, cgroups y filesystem se fija a partir del preflight
y de oráculos reales; no anunciar «cualquier Linux». Las dependencias adicionales
se provisionan explícitamente, fuera de `serve` y de los gates offline.

Para las primitivas Linux, evaluar APIs oficiales como `openat2`/restricciones
`RESOLVE_*` según kernel real; no convertir la mención de una API en una decisión
ya tomada. Pérdida de una garantía requerida implica fallo cerrado. Distintos
filesystems o modos rootless no heredan automáticamente la calificación.

**Mensaje obligatorio al owner al cerrar LINUX-07:**

> Linux y la regresión macOS están listos para `1.0.0-rc.2`. Estado de
> publicación, hashes, gates y limitaciones: [...]. El programa queda en
> `waiting_windows_access`. Puedes entregar ahora el acceso SSH Windows y
> su mecanismo local de autenticación, con la huella verificada del host.

## 11. Fase WINDOWS — `1.0.0-rc.3`

| ID | Depende de | Worker sugerido | Trabajo y criterio de aceptación |
| --- | --- | --- | --- |
| WINDOWS-01 | LINUX-07 + acceso del owner | Terra/QA | Preflight SSH read-only: edición/build soportada, x86_64/NTFS, ACL, MSVC/SDK, virtualización y engine local; comprobar acceso al backend desde la sesión SSH real |
| WINDOWS-02 | WINDOWS-01 | Sol/Terra; Opus 5.5 para debugging de stdio/procesos | Reproducir/corregir regresión stdio pre-initialize; EOF, cierre de stdout, buffering, framing, cancelación y procesos. Restituir CI portable Windows sin declararla soporte positivo |
| WINDOWS-03 | WINDOWS-01 | Sol u Opus 5.5 como autor; review independiente del otro proveedor + Opus 5.5 read-only si el autor fue Sol | ADR/prototipos reparse-safe, handles, identidad, ACL, locks y publicación/durabilidad; adversarial qualification sobre NTFS nativo |
| WINDOWS-04 | WINDOWS-03 | Sol/Terra; Sonnet 5 para tests adversariales de paths/NTFS | Portar filesystem, todos los stores y writer, catálogo/artifacts, journals/recovery; nombres reservados, device/UNC paths, ADS, junctions, symlinks/hardlinks, case folding, rutas largas/Unicode/CRLF y file sharing probados |
| WINDOWS-05 | WINDOWS-02, WINDOWS-04 | Sol/Terra; tests Gemini/Sonnet 5 | Endpoint Docker y frontera Windows/guest, state root, snapshots, sesión LSP, supervisión/cancelación y permisos; variante AMD64 verificada y suites de tools en este host |
| WINDOWS-06 | WINDOWS-05 | QA + reviews independientes | Full nativo Windows, instalación por usuario y rollback, clientes; regresión macOS y Linux sobre el tip final; cero P0/P1 y cero capacidades obligatorias sin evidencia |
| WINDOWS-07 | WINDOWS-06 | Integrador + documentación | Archivo/binario Windows y candidato acumulativo rc.3, hashes/notices/provenance, matriz final de soporte y handoff al owner |

No confundir acceso a un shell WSL con ejecución nativa del `.exe`. Mantener
checkouts y recibos distintos para Windows/NTFS y cualquier prueba WSL/Linux.
No usar `/mnt/c` como sustituto de una calificación explícita de cada frontera.
Un Docker Desktop operativo en el escritorio puede no estar accesible desde el
usuario SSH; comprobar sesión, permisos y arranque antes de planificar gates.
Evaluar Job Objects u otra garantía demostrada para el árbol de procesos host;
terminar el cliente Docker no prueba la terminación del contenedor guest.
Calificar ambas limpiezas. Empaquetado y smoke deben reconocer PE x86_64 y
`.exe`, con manejo explícito de permisos y paths; no renombrar un archive Mach-O.

El worker debe evaluar y documentar el transporte local del engine y su confianza;
no habilitar un daemon Docker TCP sin protección para simplificar conexión.
Si el backend o profiling no permite las capacidades exigidas, no habilitar modo
privilegiado ni `--security-opt ...=unconfined` como arreglo. Proponer No-go o un
perfil limitado explícito al owner, sin cerrar rc.3 como equivalente al objetivo.

## 12. Operación remota y recursos

Las CLI de modelos pueden permanecer en el Mac coordinador. Linux/Windows son
ejecutores de builds y pruebas; no necesitan credenciales de Codex/Claude/Gemini.
Los trabajadores usan SSH con scripts versionados y argumentos validados. No
concatenar contenido del proyecto a un shell remoto. Verificar host keys, no
desactivar esa verificación ni activar agent forwarding por conveniencia.

### Incorporación de una máquina

1. Recibir autorización, alias y alcance: usuario, directorio de trabajo exclusivo,
   recursos utilizables y permisos de instalación/reinicio. Credenciales mediante
   agente SSH/almacén local del usuario; no copiarlas a prompts, logs o Git.
2. Hacer preflight read-only. Entregar faltantes y pedir aprobación antes de
   instalar servicios, elevar privilegios, cambiar ACL/firewall o reiniciar.
3. Transferir el commit exacto mediante Git por canal autorizado o bundle; para
   inputs adicionales registrar hashes. No exigir push de cada iteración ni
   sincronizar indiscriminadamente todo el árbol local con secretos y caches.
4. Crear checkout limpio y directorios de build/evidencia propios. Validar commit,
   lockfile, toolchain y runtime en destino antes de ejecutar.
5. El runner inicia un job persistente con recibo antes de ejecutar el gate.
   Evaluar servicio de usuario en Linux y mecanismo equivalente autorizado en
   Windows; si exige privilegios nuevos, acordarlo antes. Validar primero con
   un job inocuo que sobrevive desconexión, se consulta y se cancela correctamente.
6. Conservar stdout/stderr acotados, heartbeat y exit code fuera de la conexión
   SSH. Descargar evidencia, comprobar hashes y cerrar el intento. Ningún job
   puede seguir indefinidamente: deadline y cancelación alcanzan descendientes.

### Fallos que debe soportar el runner

- Se pierde SSH: reconectar y consultar `job_id`; no lanzar una copia automática.
- Agente o proveedor pierde cuota: el job determinista autorizado puede terminar;
  persistir resultado sin necesitar una respuesta del modelo.
- Host reiniciado: comprobar identidad de arranque y markers; marcar `lost` cuando
  corresponda, conservar evidencia parcial y revisar cleanup antes de reintentar.
- Job termina pero falla descarga: repetir solo la recuperación/verificación de
  artifacts, no el gate exitoso.
- Cancelación o timeout: resultado terminal más comprobación de procesos y
  contenedores propios; no matar jobs/servicios de otros usuarios o agentes.

No se requieren por contrato **300 GB libres**. Registrar uso inicial, pico y
residual de una compilación/gate representativos en cada host. Las builds,
worktrees, caches y capas de imagen pueden crecer mucho; eso no mide el tamaño
necesario para usar el producto. Reducir concurrencia si RAM/disco son limitantes
y dimensionar con evidencia antes de pedir ampliación. GPU no forma parte del
alcance de estas pruebas del perfil core.

No ejecutar `docker system prune`, limpieza recursiva de caches compartidas ni
eliminar worktrees ajenos. La retención debe identificar por job qué artifacts
son descartables; conservar recibos, recovery y trabajo no integrado. Antes de
eliminar datos materiales, solicitar el permiso que corresponda al alcance.

## 13. Gates y definición de terminado

Cada corte sigue: contrato/tipos → pruebas discriminantes → implementación
vertical → pruebas focalizadas → revisión independiente → integración privada →
gate proporcional → documentación/evidencia. No introducir ports vacíos ni
considerar terminado un adapter por compilar.

El integrador comprueba los comandos vigentes antes de ejecutar. Baseline normal:

```text
cargo fmt --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
cargo test --workspace --doc --locked
python3 -B scripts/check-architecture.py
```

Para el cierre reconstruir explícitamente el binario release del commit final
antes de usarlo en harnesses; no asumir que `gate.py` reconstruye ese binario.
Reutilizar `scripts/gate.py core` y `full`, tras revisar su interfaz, entorno y
cobertura efectiva. Adaptar los harnesses al host con tests; no convertir
ausencia de soporte del arnés en pass ni limitarse al CI portable.

Separar aprovisionamiento autorizado con red de validación offline. `cargo audit`
y `cargo deny` se ejecutan cuando estén instalados/configurados; su ausencia se
reporta, no desencadena instalación silenciosa ni se declara éxito. Para una
puerta que los exige, esa ausencia bloquea el cierre. Una base de advisories
obsoleta necesita actualización explícita fuera del runtime y de la ejecución
offline, con identidad y freshness registradas.

### Matriz obligatoria por fase

| Área | Evidencia exigida |
| --- | --- |
| Contratos | Censo real, schemas/envelopes, CLI y versiones MCP admitidas sin cambios no autorizados; no promover preview a estable por cambiar versión |
| Runtime | Misma variante unificada para todas las suites previstas; pins, plugins y profiles verificados; imagen/descriptor alterados rechazados |
| I/O y estado | Lectura, captura, vendor, catálogo/SQLite/FTS5, artifacts y mutation journals; escapes por links/reparse, carreras, permisos y durabilidad incierta |
| Writer | Preview/commit/replay/restart, grants, TTL, idempotencia, conflictos y recuperación; sin prometer CAS ni atomicidad multiarchivo |
| Aislamiento | Código hostil build.rs/proc macro/tests: red denegada, secretos ausentes, host no escribible, outputs/cuotas acotados y proceso descendiente terminado |
| Lifecycle | Timeout, cancelación, desconexión MCP/SSH, EOF/stdout cerrado, reinicio y cleanup sin huérfanos propios |
| Instalación | Usuario limpio, binario empaquetado, datos/runtime verificados, configuración mínima, prueba MCP real, upgrade/rollback sin pérdida |
| Clientes | Harness protocolario y clientes reales requeridos por baseline, con versión/resultado; CLI usada como worker no acredita ser cliente MCP |
| Rendimiento | Medición por host y variante, frío/caliente y consumo de disco/RAM; método reproducible, no comparación directa de ARM64 vs x86_64 ni QEMU |
| Regresión acumulativa | rc.1 macOS; rc.2 macOS + Linux; rc.3 macOS + Linux + Windows, sobre el mismo source inventory final |
| Supply chain | Version/tag coherentes, checksums, SBOM/notices, trust/provenance y smoke del artifact, con publicación distinguida de ensayo local |

La suite de release hereda soak y umbrales de la baseline aceptada (incluido
M8 si sigue vigente); no acortar pruebas largas por cuota ni sustituirlas por
estimaciones. El runner permite que terminen sin mantener un modelo generando.
Las capacidades obligatorias necesitan positivo real; `skipped`, `unavailable`,
timeout del harness o «no se probó» no cuentan como pass. Lo opcional conserva
clasificación y limitación explícitas.

Un recibo incluye tiempos, host, source commit e inventario/hashes, versión y
SHA del binario ejecutado, descriptor/digest runtime, configuración de sandbox,
inputs, comandos, counts observados, exits, omisiones y referencias a artifacts.
Los recibos son inmutables. Un cambio posterior invalida la evidencia afectada;
una corrección puramente documental debe acreditar que no alteró el source
inventory del producto, no afirmar que un gate viejo corrió sobre otro commit.

Mantener sincronizados README, CHANGELOG, SECURITY, architecture, tools,
security-model, compatibility, client-configuration, ADRs y tablero cuando los
cortes los afecten. README sigue siendo guía de uso, no este plan. Expandir
soporte en documentación únicamente al cerrar su calificación.

## 14. Integración, publicación y rollback

- Trabajar en un worktree/branch de programa separado de Sonar y un worktree
  por worker que edite. No usar el checkout de otro proveedor como integración.
- Reservar los archivos centrales a un worker por tanda. Los demás pueden
  preparar fixtures/reviews en paquetes disjuntos sin cambiar contratos comunes.
- Integrador delegado revisa diff, origen y pruebas; integra localmente y corre
  regresión. Un commit de worker no es aceptación automática.
- Antes de cada RC reconciliar manifest, lockfile, metadata, docs, validadores,
  inventario de release y targets permitidos. No reutilizar el pipeline mac-only
  para anunciar Linux/Windows sin ampliar y probar su frontera de artifacts.
- Conservar binario/runtime anterior y estado respaldado de forma verificable.
  Ensayar rollback de configuración y compatibilidad de datos, incluyendo
  revocación de planes ligados a otra identidad. No hacer downgrade ciego de
  un journal ni borrar un estado `recovery_required` para obtener verde.
- Publicar solo tras autorización precisa. Después descargar por el canal real,
  verificar integridad/provenance e instalar esos bytes. Si falta autorización,
  registrar `qualified_local` y `publication_pending_owner`; no crear el tag.
- La fase completa se comunica con pruebas, limitaciones y pendientes; Astra no
  certifica una máquina futura con resultados del Mac.

## 15. Planificación de capacidad y comunicación

Orden crítico: **baseline/ADRs → MAC unificado y UX → gate MAC → acceso Linux →
adapters/AMD64/gate Linux → acceso Windows → adapters/stdio/gateway/gate Windows**.
Preparación de fixtures y revisión independiente pueden solaparse dentro de una
fase; no solapar la implementación de plataformas saltándose sus puertas.

Tras P-05 estimar por paquetes en función de la tarea piloto, complejidad y
tiempos medidos de build/gates. Registrar por separado esfuerzo de trabajo,
tiempo de pruebas, espera de cuota, espera SSH y revisión del owner. Los rangos
comentados antes en conversación no son compromisos; recalibrar al cerrar cada
plataforma y no prometer aceleración lineal por añadir agentes.

El reporte de avance al terminar cada tanda/sesión debe contener:

```text
Fase y versión objetivo:
Baseline / tip integrado:
Tareas aceptadas / total de fase (no porcentaje subjetivo):
Tareas en curso, proveedor y siguiente checkpoint:
Gates ejecutados / faltantes / findings bloqueantes:
Cuotas observadas y esperas (unknown cuando corresponda):
Accesos pendientes (Linux solo tras Mac; Windows solo tras Linux):
Publicación: no iniciada | pendiente de owner | verificada:
Siguiente acción exacta:
Ruta del handoff persistente:
```

No dejar largos periodos de actividad sin avisos al owner; informar cambios de
fase, bloqueos, riesgo de cuota y gates largos en curso. Evitar conversaciones
de modelo para polling frecuente: consultar jobs con mecanismos deterministas.

## 16. Instrucciones breves para iniciar y retomar

Cuando el owner decida comenzar, puede usar:

> Ejecuta `.planning/implement-1.0-runtime-portability-astra.md` con GPT-6
> Astra únicamente como orquestador. Empieza por verificar la baseline tras
> Sonar y ejecuta P y MAC mediante workers. Usa solo las cuotas incluidas.
> Guarda el control de avance y detente tras notificar que macOS está listo,
> hasta que entregue el acceso Linux. No publiques ni crees tags sin mi permiso.

Cuando se reinicie tras cuota o cierre de sesión:

> Retoma el mismo plan desde `.planning/rup-1.0-evidence/handoff.md` y
> `control.json`. Reconcilia workers/jobs y cambios locales antes de relanzar.
> No repitas tareas aceptadas salvo invalidación de su evidencia. Conserva las
> puertas SSH y usa solo cuota incluida, sin gasto facturable adicional ni
> repetición innecesaria de trabajo. La nueva sesión también consume cuota.

Cuando entregue el siguiente acceso:

> macOS/Linux ya fue aceptado según el handoff. El alias SSH autorizado para
> la siguiente fase es [...]; la autenticación está configurada localmente.
> Haz primero el preflight read-only y reporta instalaciones/permisos faltantes.

El resultado final del programa es una matriz de soporte reproducible y tres
entregas graduales, no solo tres números de versión. Si algún cierre no cumple
sus gates, conservarlo abierto con causa, evidencia y siguiente acción segura.
