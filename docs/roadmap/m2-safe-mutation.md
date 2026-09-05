# M2 — Safe Mutation / 0.2.x

> Resolución posterior a la planificación: el owner delegó la decisión D02 para
> preservar instalación/uso sencillos. [ADR-050](../adr/ADR-050-local-coordinated-mutation.md)
> acepta local_coordinated: namespace host confiable, precondiciones y locks MCP,
> sin exclusión OS de editores externos. Las exigencias de exclusión fuerte y espera
> de decisión del owner que aparecen abajo son históricas y quedan sustituidas por
> ese ADR; no hay CAS, atomicidad multiarchivo ni rollback sobre bytes desconocidos.
> La [calificación positiva de M2](../validation/M2-07.md) está completada; la release 0.1.0 no cambia.


Estado: **Done: cinco tools implementadas y calificadas localmente; sin release nueva**. Spec §25/§97 M2, §21/35–47/52/69–70/74–77/103–107;
[ADR-013](../adr/ADR-013-safe-mutation.md), [ADR-024](../adr/ADR-024-project-open.md),
[ADR-031](../adr/ADR-031-rust-source-transfer.md),
[ADR-035](../adr/ADR-035-format-check.md). Aplican [G1–G9](m2-m8.md).

## Decisiones implementadas que sustituyen propuestas iniciales

El contrato vigente está en ADR-050 a ADR-059 y [matriz M2](../validation/M2-matrix.md).
Las secciones posteriores conservan la planificación histórica. D01 corresponde a
ADR-052/054 (journal privado, cuatro planes/64 MiB/600 s, retención durable sin TTL
automático y prune explícito). D03 es ADR-051/057 (toml_edit y cuatro familias).
D04 es ADR-053/056: Cargo fix usa la plantilla probada frozen/offline, allow-no-vcs,
allow-dirty y allow-staged sobre la copia sin Git. D05 es ADR-055: vendor aprobado
opcional y lock preserve_presence, sin importar CARGO_HOME ni descargar en runtime.
G3 se concreta en ADR-058, con tracing local y consultas del journal, sin collector.
ADR-059 distingue preview vigente para efectos nuevos de replay durable exacto,
y libera planes terminales en la próxima admisión.
No existe updater gestionado M2; el criterio de downgrade gestionado no aplica a
esta integración sin release. Readers v1/v2/unknown se prueban; el downgrade manual
exige reconciliar previamente y no adquiere una garantía retroactiva de 0.1.0.

La calificación de falta de disco distingue ENOSPC real en tmpfs guest de fault
injection en cada fase durable del writer con I/O APFS real; no se llena el disco
host ni se afirma power-loss. Antes/after conocido o preservación de incertidumbre
son los oráculos. La medición de memoria máxima nativa es una observación, no un
cap de RSS; la matriz publica el coste y su alcance.

## Objetivo y contrato propuesto

Un cliente puede revisar y aplicar una mutación autorizada, conocer cada archivo
afectado y recuperar el resultado tras perder la respuesta, sin que Cargo tenga
acceso de escritura al host. Cinco tools nuevas: `rust.fmt.apply`, `rust.fix.apply`,
`rust.dependency.add`, `rust.dependency.remove`, `rust.manifest.patch`. Dieciocho
tools totales solo al terminar M2; las trece M1 conservan sus schemas/semántica.
No anunciar tools vacías antes de su vertical ejecutable.

Cada tool usa DTO nuevo con discriminante `preview|commit|receipt` propuesto.
Preview solo crea candidato/diff en staging privado; commit referencia ese plan
exacto y receipt consulta el resultado sin repetir efectos. Aunque preview no
escriba source, la tool es mutable y sus annotations conservadoras no sustituyen
autorización. Si se elige separar preview en Resources/CLI en D01, debe conservarse
el mismo flujo revisable sin añadir implícitamente una sexta tool.

Preview exige ProjectRef vivo, expected source digest, operación tipada y scope.
Commit exige plan ID opaco, digest del plan aprobado, generation, clave de
idempotencia y ProjectRef/autoridad vigente. Host concede por separado roots de
escritura y operaciones; `confirm=true` aportado por peer no crea permiso.
El digest cubre bytes, rutas, before/after hashes, opciones, toolchain/runtime,
policy generation y resolución Cargo cuando aplique. TTL de plan propuesto 10 min;
revisión tardía obliga a preview nuevo. Ninguna regeneración distinta se aplica
bajo una aprobación antigua.

Receipt tipado: operation ID, estado de commit/recovery, before/after fingerprints,
archivos/rutas relativas/hashes/tamaños, diff o Resource íntegro autorizado,
omisión explícita, policy/runtime/provenance/freshness, atomicidad real y estado
lockfile/resolución. No guardar bytes secretos en audit. Query del receipt debe
seguir autorizada por root/operation host aunque la mutación invalide el ProjectRef
de manifests; D01 decide reabrir+reautorizar sin convertir operation ID en bearer
token. Se propone receipt durable 7 días/128 operaciones por root, nunca borrar
journal pendiente para cumplir TTL. Si cuota impide conservar receipt/rollback,
no comenzar commit. Defaults finales se fijan antes del primer efecto.

Resultados nuevos usan razones tipadas propias del DTO: conflict, permission_denied,
lock_busy, plan_expired, toolchain_unavailable, offline_data_missing, recovery_required. No ampliar el enum
común de errores si rompe schemas M1. Commit/no-op idempotente por key+digest;
reusar key con inputs distintos falla. Perder respuesta después del commit no
convierte el cambio en cancelled ni autoriza repetirlo.

## Alcance por operación

| Tool | Candidato permitido | Lo que se rechaza | Oráculo |
| --- | --- | --- | --- |
| fmt.apply | Rustfmt configurado sobre `.rs` capturados, solo reemplazos | Cambios Cargo.toml/lock, creación/eliminación, parse warning no entendido | Rustfmt exacto y fmt.check posterior en fixtures calificados; segunda aplicación observada, no idempotencia universal |
| fix.apply | Cargo fix cerrado en staging, selección tipada, source `.rs` | broken-code, edition migration, flags libres, sugerencias/path externo, cambios no aprobados | Cargo fix real + check del candidato; build.rs/proc macro hostiles contenidos |
| dependency.add | Edición TOML semántica de package/dependency kind con requirement explícito, alias/features/optional/default-features | Elección implícita latest, Git/registry/path externo, reemplazar clave existente ambiguamente | TOML preservado + Cargo metadata/resolución offline exacta |
| dependency.remove | Clave/kind/package exactos; lock coherente cuando el plan lo incluye | Borrado de tabla equivocado, dependencia heredada confundida con local, cleanup arbitrario | Workspace/alias/herencia y grafo Cargo antes/después |
| manifest.patch | Operaciones tagged feature.set/remove, profile.set/remove, workspace_dependency.set/remove, lint.set/remove | JSON Pointer/TOML arbitrario, package.build, target paths, patch/replace, wrappers/config; cualquier path/git/registry/registry-index introducido por patch | Comentarios conservados, TOML reparse y Cargo real sobre candidato |

Separar tres mecanismos: transformación de texto Rust, editor TOML semántico y
ejecución Cargo. El diff humano M1 de fmt.check nunca se aplica como instrucciones.
Los procesos generan cambios no confiables que el host valida contra el scope;
diagnósticos normalizados no autentican al productor. Cargo fix ejecuta check y
puede ejecutar código de proyecto ([documentación oficial](https://doc.rust-lang.org/cargo/commands/cargo-fix.html)).
No prometer que solo una sugerencia machine-applicable explique todo cambio del
staging hostil: el diff exacto aprobado y allowlist son la autoridad de commit.

## Cortes end-to-end

Primera acción de implementación, después de integrar la planificación: actualizar
el scope de AGENTS para autorizar solo M2 y conservar M3+/release fuera de alcance.

| ID | Camino observable | Depende de | Gate/evidencia | Tamaño |
| --- | --- | --- | --- | --- |
| M2-01 | Host config→preview/commit de manifest.patch mínimo→receipt/reopen→recovery | M1, D01/D02/D03 | Primera vertical real incluye transacción, no capa vacía; deny/hash/lock/crash/APFS | XL |
| M2-02 | fmt.apply preview→rustfmt guest→diff→commit único→fmt.check | 01, D04 | Parser/config/newline/no-op; exporter adversarial; bytes host exactos | L |
| M2-03 | fix.apply preview→cargo fix guest→candidate check→commit→receipt | 02, D04 | Código hostil, sockets, hijos, cancel/EOF/timeout; sin ejecución host | XL |
| M2-04 | dependency.add preview→TOML/resolución offline→manifest+lock commit | 01, D03/D05 | Cache presente/ausente, app/lib/workspace, requirement/alias/feature | XL |
| M2-05 | dependency.remove→resolución offline→commit conjunto→audit/check | 04 | Herencia, target-specific, clave ausente/no-op y lock; no red | L |
| M2-06 | manifest.patch completo→Cargo valida→diff aprobado→commit/recovery | 01/04 | Cuatro familias normativas: features, profiles, workspace deps, lints | L |
| M2-07 | Cliente real→cinco tools→fallos/races/restart→gate y handoff | 01–06 | G1–G9, 18 tools, trece snapshots M1 iguales, Sonnet+Opus High | L |

M2-01 incluye un caso mínimo de lints del manifest raíz; M2-06 completa el contrato.
Los modos aún no implementados se omiten del schema o rechazan de forma explícita,
nunca se dan por Done. Camino crítico: D02→01→02→03 y D05→04→05→06→07.
No introducir M3 tasks, M6 analyzer, tool pública de edición arbitraria de source, scaffolding, Git
commit/reset/stash, red, auto-install, otros targets positivos o publicación 0.2.

## Transacción y threat model de escritura

Activos: source/manifest/lock del usuario, directorios ajenos, journal, permisos,
contenido dirty, recepción de cambios y configuración/cache Cargo administrativos. Amenazas: peer con permiso insuficiente,
plan stale/replayed, output Cargo hostil, external writer, symlink/hardlink/reparse,
parent movido, crash/disco lleno y rollback que pise cambios ajenos.

1. Capturar bytes mediante handles originales con límites ADR-031; precondition
   usa source digest completo, no solo ProjectIdentityFingerprint de manifests.
2. Crear staging privado; Cargo/rustfmt operan allí bajo gateway. No host bind
   escribible. Copia guest escribible debe tener cuota real, no asumir que volumen
   Docker local tiene límite de disco; D04 compara tmpfs y exportador confiable.
3. Terminar/verificar todos los mutadores antes de exportar candidatos. Exporter
   acotado rechaza links/devices/path collision/extra files/cambio de permisos y
   cualquier cambio fuera del scope. No extraer un tar hostil por nombres al host.
4. Locks por identidad física del workspace, no ProjectRef. Dos refs y dos procesos
   deben competir por el mismo lock. Orden global documentado; admisión no bloquea
   recepción de cancelación. Las trece tools M1 no adquieren el lock de mutación: conservan
   admisión, presupuestos y error model existentes. Las ejecuciones ya admitidas usan
   sus bytes capturados; las nuevas capturan/validan como M1, sin prometer snapshot
   atómico mientras otra operación escribe. No introducir lock_busy ni espera nueva
   en DTOs M1. Catalog/search/explain independientes de source no toman ese lock.
   Probar cada tool durante commit con timings/códigos/schema frente al baseline.
   El receipt M2 delimita su generación, no reinterpreta resultados M1 concurrentes.
5. Revalidar root/policy/generation/bytes antes de commit. Journal durable registra
   plan/identidad/before/after y fases; preparar temporales mediante I/O protegido,
   reserva de cuota y persistencia de backups antes del primer cambio.
6. Reemplazo atómico por archivo y progreso durable. Multiarchivo es recuperable,
   **no** visibilidad atómica a lectores externos. Definir commit point durable y
   verificar estado final antes de publicar receipt. Durabilidad incierta no es pass.
7. Crash recovery conserva bytes desconocidos de terceros. Si no coinciden con
   before ni after, cuarentena/recovery_required; jamás rollback ciego.

**D02 vigente: ADR-050 local_coordinated.** El owner delegó elegir el contrato
sin añadir carga de instalación. Se confía en el host/dev y en un namespace estable
durante el commit; el código del proyecto ejecutado por el MCP sigue siendo no
confiable y queda aislado del host. Las roots originales, resolución protegida y
precondiciones se prueban; locks coordinan únicamente MCP con state root compartido.
No se anuncia exclusión OS de editores, CAS por contenido ni visibilidad atómica
multiarchivo. Los bytes desconocidos se conservan y cualquier efecto incierto queda
en recovery_required. Las pruebas de carreras delimitan estas garantías, no convierten
el contrato coordinado en enforcement frente a un host malicioso.

La investigación negativa y los requisitos fuertes originales se conservan en
[prerrequisitos históricos D02](m2-d02-host-preconditions.md) y ADR-049. Ya no se
espera otra decisión del owner ni se exige broker/UID adicional. La calificación
positiva del writer para ADR-050 sigue siendo obligatoria antes de M2-01 Done.

Revisar APIs [rustix 1.1.4](https://docs.rs/rustix/1.1.4/rustix/fs/struct.RenameFlags.html)
y [APFS](https://developer.apple.com/library/archive/documentation/FileManagement/Conceptual/APFS_Guide/ToolsandAPIs/ToolsandAPIs.html)
contra el SDK local; calificar rename/exchange, fullfsync, directory durability,
root-bound I/O y crash recovery en el target real. No extrapolar semántica SQLite
de ADR-041 al workspace compartido: su state-root privado tiene otra autoridad.

## Permisos, dirty state y resolución offline

Host concede write roots⊆read roots, operaciones separadas R3/R4 y state-root
privado protegido. Config de proyecto solo restringe; ninguna tool M1 obtiene W.
Scopes excluyen .git, target, .cargo/config, toolchain y paths externos. Dirty policy
propuesta: preservar bytes capturados con autorización explícita de esa generación,
sin reset/stash/auto-commit. No inferir clean de ausencia de .git en la captura.
Si se requiere clean Git como policy, inspeccionarlo por gateway/port aprobado y
definir submodules/worktrees; no ejecutar Git libre en host.

Cancel antes del primer cambio deja cero cambios. Después del punto irreversible
la finalización durable/recovery domina a cancelación del caller; registrar fase
real y permitir receipt lookup. Timeout de Cargo mata árbol; shutdown del servidor
espera journal/cleanup o informa incertidumbre. Defaults propuestos: 1 commit activo
por workspace, sin cola, 128 cambios, 1 MiB por archivo, 16 MiB source y 32 MiB de
staging+backups por operación, 128 KiB diff visible y 512 KiB MCP completo; exceder
cap implica preview no aplicable. Disco durable necesita reserva/enforcement propio,
no confundir cuota lógica con capacidad OS. No logs de source completo.

M1 crea CARGO_HOME vacío y no admite config vendor ni registry cache. D05 debe
elegir provisioning administrativo offline verificado separado de tools/call.
Catálogo SQLite no es un Cargo registry index/cache. dependency.add exige requirement
explícito, no selección best-effort de versión ([Cargo add](https://doc.rust-lang.org/cargo/commands/cargo-add.html)).
Propuesta: manifest-only explícito sin afirmar resolución, o resolución offline
con snapshot Cargo aprobado; nunca cambiar entre modos silenciosamente. El DoD
exige al menos un caso registry real resuelto offline y un caso ausente denegado.
App con lock: incluir lock generado en staging en el mismo plan/commit; library sin
lock: respetar policy declarada por host, no inferirla de package.lib. No modificar
lock de M1 ni relajar frozen de lecturas. `toml_edit` queda seleccionado y fijado por ADR-051; su incorporación y fixtures
forman parte de M2-01, con inventario específico antes de otra distribución.

## Tests, DoR y DoD

La [trazabilidad de cierre](../validation/M2-traceability.md) enlaza cada casilla
y G1–G9 con implementación, pruebas y límites del [full final](../validation/M2-full-gate.json).

Fixtures: dos ProjectRefs mismo workspace, dos servidores, external writer en cada
ventana, parent/root swap/ABA, symlinks/hardlinks/FIFO, permisos revocados, journal corrupto,
pérdida de respuesta y barreras/faults nativos calificados en la matriz M2.
La inyección de escritura parcial/ENOSPC del journal y los crashes calificados no
simulan disco APFS físicamente lleno, todas las primitivas ni pérdida de energía.
Cada control de contención incluye control positivo que demuestra que el ataque
se ejecuta y altera el canario con el control deliberadamente ausente en un fixture
privado: writer concurrente, parent movido, link de destino y exporter extra.
No desactivar controles en workspaces reales. Oráculo independiente registra canarios/bytes/inodes fuera de scope y exige ningún
overwrite ajeno; comparar before/after/receipt y efectuar replay. Tests APFS reales
obligatorios. Linux/Windows prueban rechazo antes de I/O hasta otro adapter.

Cargo fixtures: fmt config/skip/newline/no-op/error, fix std-only y proc macro/build
hostil; app/library/workspace con registry offline, alias/optional/target-specific,
herencia, comentarios TOML y cuatro familias patch. Output del mutador que intenta
editar manifests desde fmt, symlink al exportar o secretos en diff debe rechazarse
o permanecer privado según policy. Unit/contract/protocol/integration/security/native
y clientes G4; performance mide preview/commit/lock contention y recovery con budgets.

DoR de M2-01: baseline live, D01/D02/D03 decididos con review independiente,
frontera D02 probada, state-root aprobado y fixtures/límites definidos. D04 se
decide antes de M2-02; D05 y assets Cargo antes de M2-04. DoD:
M2-01..07, G1–G9, todas las casillas siguientes y el DoD adicional de decisiones
contractuales con receipts en futura matriz M2. Nueva
distribución no empaqueta Cargo cache/runtime/fixtures sin D05/D14 y notices/SBOM.

- [x] Cinco tools producen preview revisable, permiso explícito, commit limitado
  y receipt; trece contratos M1 permanecen iguales. Fuente: spec §25/97, ADR-013/015.
- [x] Mutación stale, replay alterado, lock concurrente y permiso ausente no cambian
  bytes en los casos calificados. I/O protegido y carreras negativas respetan el
  contrato de namespace confiable local_coordinated; no se acredita contención
  frente a un host que lo viole. Fuente: ADR-007/024/050 y M2-01/07.
- [x] Crash/cancel y faults de I/O calificados preservan bytes externos y solo
  recuperan generaciones conocidas; journal parcial/unknown conserva evidencia y
  puede exigir remediación manual con recovery_required. El alcance de inyección
  y la ausencia de prueba de power loss/disco físico lleno quedan explícitos.
  Fuente: ADR-052/054, M2-native-io-faults.json y M2-native-remediation.json.
- [x] Cargo/rustfmt reales operan en staging con red/env/hijos/quotas enforced;
  no existe write bind host ni instalación implícita. Fuente: spec §37–45/107,
  ADR-008/009/031 y M2-02/03.
- [x] Add/remove/patch preservan TOML y policy lock, resuelven un registry fixture
  offline y niegan cache ausente. Fuente: spec §25.3–25.5/105, D03/D05 y M2-04..06.
- [x] Full source-bound, native/security, Inspector y cliente stock pasan; reviews
  Sonnet y Opus High no dejan findings bloqueantes. Fuente: AGENTS, G4/G5/G8.


## Decisiones contractuales previas que D01–D05 deben concretar

**Autoridad de receipts:** propuesta D01: grant host vigente para la identidad física
original del workspace y operación de escritura ∧ principal host de la operación
igual al principal solicitante ∧ política de lectura/retención vigente. El principal
se establece por configuración/binding confiable de sesión, nunca clientInfo, un
argumento JSON ni un operation ID. Otro peer sobre la misma root no hereda acceso.
Revocación impide leer receipts existentes inmediatamente, aunque no venza TTL.
El commit devuelve before/after source y ProjectIdentityFingerprint, identidad de
root y receta `project.open` para obtener nueva referencia; no fabrica ProjectRef
vigente ni concede acceso por conocer el fingerprint. Probar B≠A y revoke-before-TTL.

**Formato:** D01 propone un payload interno de cambios de bytes existentes con
rutas/before/after y operación/provenance tipadas. El validador de cada tool autoriza
su scope; un formato capaz de representar edits no crea un editor público genérico.
M6 debe decidir su operación nueva en D25, versionar cualquier enum/formato persistido
que cambie y probar unknown-version. No reservar valores públicos vacíos en M2.
Journal, plan y receipt tienen versión independiente desde el primer writer; readers
rechazan versión desconocida antes de efectos. D12 aplica desde M2-01 a esos formatos,
aunque las migraciones generales sean M8-03. Journal pendiente impide nuevas mutaciones sobre su generación hasta
reconciliación. ADR-054 sustituye la propuesta de preflight gestionado: M2 no
incorpora updater ni launcher de downgrade. La guía exige reconciliar los journals
ante un cambio manual de checkout. No se puede modificar retroactivamente 0.1.0
para que detecte un journal M2; el binario histórico no ofrece mutaciones M2.
Probar decoders compatibles/incompatibles sin afirmar que un binario histórico
conoce un formato futuro.

**Snapshots/availability:** commit invalida todos los caches/planes ligados a su
source generation, incluyendo previews concurrentes; lectores con snapshot ya
capturado conservan su provenance original. Nunca servir un cache precommit como
fresh postcommit. Preview de fmt requiere rustfmt; fix Cargo/compiler/componentes;
add/remove/patch editor y, en modo resolución/validación, Cargo y dataset offline.
Ausencia es unavailable con razón propia del DTO M2 (toolchain_unavailable o
 offline_data_missing), no failed del proyecto ni éxito. Denegación de policy,
conflict y recuperación son operacionales; un candidato Cargo inválido es resultado
fallido observado. D01 mapea §69/70 sin modificar enums ni isError de tools M1.

**TOML/cache/config:** D03 valida valores tipados de todas las familias y rechaza
path/git/registry/registry-index, rutas absolutas/escapadas y URLs con credenciales
introducidas por patch. Candidatos resultantes deben seguir siendo capturables por
ADR-031; fixtures `../`, absoluto y symlink tienen canario externo y oracle Cargo.
D05 no importa CARGO_HOME del host en bruto. Construye config desde allowlist de
source replacement/cache offline en paths guest fijos, con hash exacto verificado
antes de cada job y presente en receipt. Prohíbe rustc-wrapper,
rustc-workspace-wrapper, linker, runner, target-dir, net/http y credentials/registries
aportados externamente. Config host malformado/adulterado se rechaza antes del job.
El gateway fija env/target-dir/red; un asset administrativo no relaja esos controles.

**Cargo fix (decisión implementada ADR-056, sustituye la propuesta D04):**
plantilla cerrada `cargo fix --workspace --all-targets --frozen --offline
--allow-no-vcs --allow-dirty --allow-staged --message-format=json --color never
--target-dir /target`. La selección es workspace/all-targets; no hay edition,
edition-idioms, broken-code ni flags libres. La captura no contiene Git y la
aceptación del diff se basa en generación exacta y permiso explícito; los flags
calificados no autorizan reset/stash ni ejecutar Git host. Los argumentos se
registran en provenance; el check VCS de Cargo no es la frontera de protección. Rustfmt no promete idempotencia universal: un segundo candidato distinto
necesita otro preview/aprobación; nunca se aplica bajo el digest anterior ni se
llama plan_expired a una mera diferencia de resultado sin TTL vencido.

DoD adicional M2-01/07, con fuentes dentro de cada criterio:

- [x] Trece snapshots y comportamiento/admisión M1 bajo commit concurrente
  conservan contrato y respetan los presupuestos existentes L09/L10; invalidación postcommit demostrada. Fuente: G1, spec §52/69/70,
  ADR-024/031, D01/D02.
- [x] Receipt owner/grant/revocación/reopen, decoder legacy v1 y unknown-format
  fail-closed se prueban. Se publica que no existe updater/downgrade gestionado;
  el operador reconcilia journals antes de volver a otro checkout y 0.1.0 no
  interpreta formato M2. Fuente: G2/G6, ADR-052/054 y guía de clientes.
- [x] tools/security-model públicos enuncian alcance real de exclusión, ausencia de
  atomicidad multiarchivo para lectores externos y lecturas M1 concurrentes, dirty
  policy, retención/autorización de receipts y ausencia de updater/downgrade gestionado;
  AGENTS refleja solo M2 autorizado en la fase de implementación. Fuente: AGENTS,
  spec §35–45/53–59/113 y D01/D02.

Handoff: commit integrado y smoke, schemas/receipts, amenazas residuales,
provisioning/rollback, DoD y limitaciones; detener antes de M3. Ningún subcorte
parcial o tool permanentemente bloqueada satisface M2 completo.
