# Mutación local coordinada

M1 no incluía tools de mutación; M2 las añadió bajo autorización explícita
del owner (`AGENTS.md`), después de que una investigación empírica (D02)
mostrara que la exclusión a nivel de sistema operativo entre programas del
mismo usuario interactivo no existe en este entorno. Este capítulo describe
el modelo de confianza resultante — `local_coordinated` — y los seis tools
de escritura que lo implementan: `rust.fmt.apply`, `rust.fix.apply`,
`rust.dependency.add`, `rust.dependency.remove`, `rust.manifest.patch` y,
desde M6, `rust.analyzer.action.apply` (que reutiliza este mismo writer, ver
[`analyzer.md`](analyzer.md)).

## Por qué no hay exclusión a nivel de sistema operativo (D02)

Decisiones: ADR-049.

**Decisión (histórica, nunca `Accepted` en sí misma — quedó `Proposed`, su
evidencia negativa sigue siendo válida).** ADR-049 probó empíricamente que
el UID interactivo del desarrollador **no tiene exclusión a nivel kernel**
frente a otros programas lanzados por el mismo usuario (por ejemplo, un
editor guardando el mismo archivo a la vez). Esto forzó al owner a decidir
explícitamente entre una frontera de identidad separada (broker
privilegiado) o un contrato de concurrencia distinto con riesgo residual
aceptado.

**Estado actual.** Sustituido por ADR-050 — la propia evidencia negativa
([`docs/validation/M2/d02-native-probe.json`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/validation/M2/d02-native-probe.json)) sigue siendo histórica y
válida; el índice [`docs/adr/README.md`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/README.md) lo marca correctamente como
"Proposed; No-go histórico, requisito sustituido por ADR-050".

## `local_coordinated` y sus límites explícitos

Decisiones: ADR-050.

**Decisión.** El owner delegó y el Technical Owner adoptó el modelo de
confianza **`local_coordinated`**: instalación de binario ordinaria, sin
daemon, sin `sudo`, sin cambios de ownership ni entitlements privados. El
producto confía en el host y en los programas que el propio developer
lanza (editor, Git, shell), pero **nunca** en código de proyecto lanzado por
el MCP mismo — ese código permanece siempre confinado al gateway aislado
(ver [`execution-and-security.md`](execution-and-security.md)). El flujo es
siempre preview → commit: el commit solo ocurre después de que Cargo valide
el candidato dentro del gateway; el commit en sí revalida grant, principal,
TTL, idempotencia, generación de source e identidad de root, todo bajo el
lock del propio MCP. Journal y backups son durables (`fsync`/
`F_FULLFSYNC`, **no a prueba de pérdida de energía real** — solo se probó
inyección de ENOSPC) antes de publicar cualquier efecto. **No hay rollback
automático** ante un conflicto con un editor externo que modificó el mismo
archivo entre preview y commit.

**Lo que `local_coordinated` explícitamente NO es:**
- **NO es CAS** (compare-and-swap con contenido) — no hay comparación
  atómica de bytes contra lo que existía cuando se generó el preview más
  allá de la revalidación de identidad/generación descrita arriba.
- **NO es exclusión de sistema operativo** sobre editores externos — un
  editor puede escribir el mismo archivo entre preview y commit sin que el
  sistema operativo lo impida.
- **NO es atomicidad multiarchivo visible** para otros lectores — un lector
  externo puede observar un estado intermedio entre archivos de una misma
  operación multiarchivo.

**Alternativas rechazadas que siguen explicando el límite actual.** Un
broker privilegiado con UID o namespace exclusivo habría resuelto la
exclusión real, pero su carga de instalación/administración quedó fuera de
alcance para M2 por decisión del owner. Leases de archivo XNU privados
exigirían un entitlement que el producto no puede auto-otorgarse.
Presentar el patrón real ("swap y luego validar") como si fuera CAS se
rechazó explícitamente como una afirmación incorrecta — la documentación no
debe describirlo así en ningún punto.

**Riesgo residual (RR-09, ver el registro completo en
[`execution-and-security.md`](execution-and-security.md#registro-de-riesgos-residuales-de-10)).**
Sin CAS, sin exclusión de sistema operativo de editores externos, sin
atomicidad multiarchivo visible; supervivencia a pérdida de energía real no
demostrada; un journal corrupto bloquea el store compartido completo. Es un
riesgo aceptado y documentado explícitamente, nunca oculto.

**Estado actual.** Vigente; decisión fundacional de todo este capítulo.
Evidencia: `crates/mcp-server/src/host_config.rs` (flags
`--allow-*-write`), `crates/project-adapter/src/filesystem/macos/
mutation.rs`, `crates/application/src/mutation.rs`; tests
`tests/inspection_runtime/{mutation,mutation_concurrency,format_mutation,
fix_mutation,dependency_mutation,fix_hostile}.rs`.

## Mutación segura, fuera de M1 (histórico en su alcance; principios continuados)

Decisiones: ADR-013.

**Decisión histórica.** M1 no incluyó tools de mutación; se dejaron roots de
source en solo lectura donde el sandbox lo permitiera. La ADR proponía que
cualquier mutación futura exigiera permiso explícito, diff, precondiciones
por fingerprint, un "write lock" y rollback atómico.

**Qué sigue vigente y qué fue sustituido.** Los principios de permiso
explícito, precondición por fingerprint, diff previo y rollback avanzaron a
M2 tal cual. El mecanismo **literal** de "write lock" con exclusión de
sistema operativo queda **sustituido** por el modelo de confianza de
ADR-050 (sin esa exclusión). El alcance "sin mutación" de M1 es histórico:
M2 implementó mutación bajo autorización explícita del owner; no es una
limitación permanente del producto.

## Journal privado, autorización y retirada de planes

Decisiones: ADR-052, ADR-059.

**Decisión (ADR-052).** La escritura está autorizada por root y por
operación vía flags de host (`--allow-manifest-write`, etc.) que deben
coincidir **exactamente** con una root de workspace ya registrada. El
principal de cada operación es el **UID que arrancó el proceso stdio** —
**nunca** `clientInfo` ni ningún valor suministrado por el peer MCP, que no
es confiable para autoridad. Los planes viven en memoria con TTL de 600 s,
un ID aleatorio de 128 bits, y un límite de 4 planes/64 MiB agregados por
sesión. El journal es durable **antes del primer efecto** en disco, con
permisos `0700`/`0600`, sin symlinks, hardlinks ni ownership ajeno. El orden
de locks es fijo: lock global del store → lock de workspace por
device/inode. Cuotas: 128 journals/256 MiB por store, 48 MiB por entrada.
**Un journal corrupto o parcial bloquea el store compartido entero**
(`list`/`prune`/commits fallan cerrado) — existe un procedimiento de
remediación documentado (crear una nueva state-root con copias frescas,
nunca tocar los originales en cuarentena).

**Decisión (ADR-059, corrección de un P1 real observado en producción).**
Un plan se retira de la resolución en memoria — liberando su cupo de los 4
planes — solo tras un recibo **terminal** conocido (`Committed`/
`NoChange`/`Aborted`) con ID y digest coincidentes. Los límites de 4
planes/64 MiB/TTL 600 s **no cambian**. Cuando un commit no encuentra su
plan en RAM (por ausencia o por TTL vencido), puede **repetir** —nunca
crear de nuevo ni re-ejecutar— exactamente una operación ya journaled, vía
ID + digest + idempotency-key, revalidando grant, kind, principal, path,
identidad física, locks e integridad del journal primero. Un replay
**nunca** carga bytes de candidato suministrados por el caller y **nunca**
re-valida vía Cargo — solo repite el efecto ya probado y journaled.

Esto corrige un defecto real observado: un cliente real chocaba con
`limit_exceeded` en su quinta preview porque 4 planes ya comprometidos
seguían contando contra una cuota diseñada para acotar candidatos
*pendientes*, no para tapar una sesión completa a solo 4 operaciones
secuenciales de por vida.

**Estado actual.** Ambos vigentes; ADR-059 refina la retención de ADR-052
sin contradecirla. Evidencia: `crates/application/src/mutation.rs`
(`replay`, `replay_mutation`, `retire_if_terminal`); tests
`tests/inspection_runtime/{mutation,mutation_concurrency,mutation_digest,
mutation_store}.rs`; [`docs/validation/M2/full-gate.json`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/validation/M2/full-gate.json);
[`docs/reviews/M2/M2-059-review.md`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/reviews/M2/M2-059-review.md) (revisión independiente: Accepted).

## Editor semántico de manifiestos y operaciones tipadas

Decisiones: ADR-051, ADR-057.

**Decisión (ADR-051).** `toml_edit = 0.25.13` (solo parse/display) para un
editor que preserva formato en vez de reescribir el archivo entero. El
primer corte se limita a lints de package/workspace del `Cargo.toml` raíz
únicamente. **Rechaza tablas inline y claves dotted** en el path tocado por
la edición — el editor no intenta reformatear estructuras que no entiende
por completo.

**Decisión (ADR-057, excepción acotada + extensión).** Añade la única
excepción a la regla anterior: la **remoción** (nunca edición) de una
entrada de dependencia completa puede conservar su representación propia en
tabla inline o clave dotted — quitar una entrada entera no exige entender
su formato interno. ADR-057 extiende además con operaciones tipadas
**cerradas** para features, profiles y workspace-dependencies, más
`rust.dependency.add`/`rust.dependency.remove`, restringidos a **miembros de
workspace verificados contra la metadata de Cargo ya capturada** — un
`Cargo.toml` que solo esté bajo una root de lectura no se vuelve
automáticamente un miembro escribible por eso.

M3 y la edición arbitraria de TOML quedan **fuera de alcance por diseño**,
no por una brecha pendiente — el editor nunca pretendió ser un editor TOML
genérico.

**Estado actual.** Ambos vigentes (ADR-057 es una ampliación acotada, no
una contradicción). Evidencia: `crates/project-adapter/src/manifest_edit.rs`,
`crates/domain/src/manifest_edit.rs`,
`crates/execution-adapter/src/project_metadata.rs`; `Cargo.lock`
(`toml_edit 0.25.13+spec-1.1.0`); tests
`tests/inspection_runtime/{mutation,dependency_mutation}.rs`.

## Publicación multiarchivo y datos de Cargo offline

Decisiones: ADR-054, ADR-055.

**Decisión (ADR-054).** El publicador nativo generaliza a hasta **128**
reemplazos de archivo existente, con una máquina de fases explícita:
`Prepared → Scratch → Staged → Applying → Published → Committed`, cada una
con sus propias reglas de recuperación. El writer está ligado a
**exactamente un** tipo de operación autorizado por llamada
(`manifest_patch` o `format_apply` — nunca intercambiables dentro de la
misma operación).

**Decisión (ADR-055).** La resolución de dependencias offline usa un
directory-source preparado por el host con `cargo vendor` — **nunca
auto-descargado** por el producto — verificado por SHA-256 de árbol
pinneado por el host. La política es `preserve_presence` para `Cargo.lock`:
**nunca se crea uno ausente** en el candidato publicable; si es necesario
resolver, se usa como máximo un lock transitorio durante la resolución,
excluido siempre del candidato final.

**Estado actual.** Ambos vigentes. ADR-078 (ver
[`execution-and-security.md`](execution-and-security.md#captura-de-vendor-a-gran-escala-separada-de-sourcebundle))
reafirma explícitamente estos límites de vendor sin subirlos, tratándolos
como frontera de seguridad ya calificada, no como un techo a ampliar.
Evidencia: `crates/project-adapter/src/filesystem/macos/mutation.rs`,
`crates/project-adapter/src/mutation_state.rs`,
`crates/execution-adapter/src/resolution_gateway.rs`.

## `cargo fix` en su propio loopback aislado

Decisiones: ADR-056.

Ver la descripción completa del perfil seccomp dedicado en
[`execution-and-security.md`](execution-and-security.md#staging-y-fix-de-mutación-dentro-del-mismo-gateway):
la variante `Fix` reutiliza el staging/publisher de M2 (esta ADR); el
comando fijo es `cargo fix --workspace --all-targets --frozen --offline
...`; el éxito exige `exit 0` + JSON válido + un `cargo check` independiente
posterior. No se altera el perfil baseline M1.

## Observabilidad local de mutación

Decisiones: ADR-058.

**Decisión.** Como máximo **un** evento estructurado acotado por llamada M2
completada, emitido por el mecanismo de `tracing`/stderr ya existente (sin
un colector nuevo). Los campos son cerrados: tool, fase pública, status/
reason, duración, un flag de cleanup incierto y un ID opaco. El evento
**nunca** registra argumentos, paths, source, diffs, stdout de Cargo,
variables de entorno ni credenciales ni claves de idempotencia. Una ruta de
fallback garantiza la emisión del evento tras el cleanup incluso cuando el
SDK suprime la respuesta al cliente.

**Estado actual.** Vigente; evidencia
`crates/mcp-server/src/stdio/mutation/audit.rs` (schema
`rust-mcp-mutation-event-v1`), verificado indirectamente por los tests de
fase de `mutation.rs`/`mutation_concurrency.rs`.

## Los cinco tools de escritura M2, y el sexto reutilizado por el analyzer

`rust.fmt.apply` (idempotente, restringido al workspace, devuelve archivos
modificados + diff), `rust.fix.apply` (ejecuta `cargo fix` bajo la política
controlada de arriba, requiere permiso explícito), `rust.dependency.add`/
`.remove` (edición estructurada de manifiesto, nunca concatenación de
strings) y `rust.manifest.patch` (solo las propiedades permitidas por
ADR-057 — nunca un editor TOML genérico) son los cinco tools M2. Cada uno
exige su propio flag de host (`--allow-fmt-write`, `--allow-fix-write`,
`--allow-dependency-add`, `--allow-dependency-remove`,
`--allow-manifest-write`) — el flag es por operación, no un interruptor
único de "permitir mutación".

`rust.analyzer.action.apply` (M6, ver [`analyzer.md`](analyzer.md)) es el
sexto tool de escritura, pero **no** introduce un segundo lock, journal ni
staging de filesystem: reutiliza verbatim este mismo writer M2
(`MutationCandidate{kind: AnalyzerActionApply}`), con el mismo journal,
replay, receipt y recovery descritos arriba. Una entrada de journal
preexistente con un `kind` desconocido — por ejemplo escrita por un binario
más nuevo o más viejo que reconoce un kind que este binario no — debe
rechazarse **antes de cualquier efecto**, nunca interpretarse con un
default silencioso (test obligatorio G6 de ADR-083).
