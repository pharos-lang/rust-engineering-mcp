# ADR-052 — Journal privado y autoridad de mutación local

Date: 2026-09-05

## Context

ADR-050 requiere publicación recuperable sin convertir un ID de operación en
permiso. Los ProjectRef de M1 pueden invalidarse cuando cambia un manifest.

## Decision

La primera vertical autoriza escritura desde configuración del host por root y
operación: --allow-manifest-write ROOT repetible, siempre dentro de --root y coincidente
exactamente con la raíz del workspace a editar; no concede subproyectos implícitos.
Exige la policy runtime existente para validar el candidato; no añade instalaciones. El principal local es el UID que inicia stdio; el canal stdio y su host
son la frontera de identidad, no clientInfo ni valores aportados por el peer.
Un receipt requiere reabrir el proyecto y una referencia viva autorizada por el
grant actual para la misma identidad física y operación. Revocar el grant impide
nuevos commits y consultas por MCP; no borra evidencia privada. No hay autoridad
multiusuario o remota implícita.

Preview exige la identidad de manifests observada al abrir y produce la generación
completa del source que se aprueba con el digest de commit. No exige al peer un
hash nuevo que ninguna tool M1 suministra antes del preview.

Planes en memoria: TTL 600 s monotónico, ID aleatorio de 128 bits, máximo cuatro
planes y 64 MiB de bytes agregados; se invalidan al reiniciar. Preview captura
source completo, produce candidato inmutable y digest con separación de dominio
que incluye paths/bytes antes/después, operación y contexto de validación.
Commit exige ID/digest y clave idempotente; reuso con distinto digest falla.
La pérdida de respuesta se resuelve mediante receipt del ID original.

El journal versionado conserva bytes before/after y la clave/digest antes del
primer efecto. Estado privado del UID, directorios 0700 y archivos 0600 fuera de
las roots de proyecto; no se acepta symlink/hardlink ni ownership ajeno. La primera vertical usa el hijo fijo rust-mcp-mutations-v1 del --state-root
existente del gateway, creado 0700 mediante handles protegidos. No añade una
segunda ruta de configuración ni usa HOME del peer. El proceso no requiere
privilegios ni cambia permisos de directorios ajenos. Todas las instancias que
escriben el mismo workspace deben compartir ese --state-root.

Orden de adquisición no bloqueante: global del store → device/inode del workspace.
Ambos se conservan hasta completar persistencia/cleanup; se rechaza busy sin cola.
El global serializa también workspaces distintos/anidados del mismo store.
Un lock por device/inode del workspace serializa writers MCP con
el mismo state root. Todos los procesos MCP que escriben un workspace deben usar
ese mismo state root; cambiarlo con operaciones pendientes está fuera del contrato
coordinado y se documentará en configuración. No se promete exclusión entre stores
independientes ni ante otros programas.

Primera publicación calificada: reemplazos de archivos existentes, máximo 128,
1 MiB por archivo/16 MiB source. Temporales exclusivos .rust-mcp-mut-<32 hex>.swap en la raíz exacta del workspace
(clone APFS del source admitido para preservar metadata; solo se reescriben bytes
del clone), para no
exigir que el state root esté en el mismo volumen; paths completos relativos al
handle original y renameatx_np NOFOLLOW_ANY|RESOLVE_BENEATH|SWAP. La copia before
en el journal es durable antes de crear/publicar. El temporal tras swap retiene
el inode desplazado; se verifica antes de limpiar. Solo se retira un temporal
propio identificado por inode/bytes bajo el lock después del journal durable.
El unlink usa un nombre simple en el handle original del workspace exacto, sin
componentes intermedios que puedan seguir symlinks. Éxito no deja temporales.
La creación de clone y su reescritura son fases distintas: el inode del clone se
registra duramente antes de truncarlo. Antes de cualquier publicación, ese inode
es scratch exclusivo del protocolo; una escritura parcial/ENOSPC puede retirarse
solo si el source original conserva sus bytes e inode y la fase durable no permite
que el temporal contenga source desplazado. El host y sus herramientas no editan
los nombres reservados `.rust-mcp-mut-*`, tampoco después de una interrupción.
Si el proceso cae entre clone y registro del inode, recovery solo puede adoptar
el clone con bytes before exactos, metadata/owner/nlink válidos y source intacto,
bajo el intent durable de creación exclusiva; cualquier otra observación conserva
recovery_required. Tras habilitar/publicar swap, la regla estricta de bytes/node
conocidos para el source desplazado vuelve a aplicar: no tratarlo como scratch.
Si hay recuperación pendiente no ejecutar git clean/add ni eliminar esos archivos. No rollback automático por un
conflicto externo. fsync y F_FULLFSYNC de archivo y directorio son obligatorios;
cualquier incertidumbre posterior a publicación da recovery_required.

El ID del preview se persiste como operation ID en el journal antes de publicar.
Receipt tras restart usa ese ID durable y nueva referencia viva/grant; no requiere
el plan en memoria. La recuperación se solicita explícitamente en receipt con
recover=true; un commit pendiente también debe clasificar conservadoramente. Sin
grant no se inspecciona ni repara estado por MCP: devuelve permission_denied y las
docs indican conservar cualquier evidencia pendiente y reiniciar con el mismo
grant/state-root. No se anuncia que una sesión sin grant haya comprobado recovery.

La recuperación clasifica archivos como before/after/desconocidos y solo completa
estados conocidos bajo el mismo grant/lock. Desconocidos conservan journal y
temporales, sin escritura de source. Cancelación se atiende antes del primer
efecto; después se termina la persistencia del resultado o recovery_required.
No anunciar commit successful antes de la persistencia del receipt. Un candidato
idéntico produce no_change; una operación interrumpida sin publicación produce
aborted y exige otro preview. El recibo distingue los hashes previstos del efecto
terminal registrado: committed acredita after, aborted/no_change acredita before,
y recovery_required no acredita un after. No afirma que ningún escritor posterior
haya conservado esos bytes. Consultar un recibo no promueve ni repara staging;
recover=true es la acción explícita para reconciliarlo. Las consultas y recovery
por ID autorizado permanecen disponibles ante cuota llena o entradas ajenas del
store; esas condiciones bloquean trabajo nuevo, no la recuperación conservadora.

El preview libera el registry de referencias durante el oráculo Cargo y recaptura
su generación al terminar. La admisión M2 no espera el mutex de otra mutación;
responde lock_busy. El payload MCP completo se mide antes de retener el plan.

Cuotas iniciales: 128 journals y 256 MiB totales por store, entrada serializada
máxima 48 MiB. La cuota se verifica bajo lock global antes de comenzar; no borrar
journals pendientes por TTL. La ubicación es <state-root>/rust-mcp-mutations-v1; store lleno devuelve
limit_exceeded. No borrar el directorio completo. La CLI administrativa local
`mutation list --state-root ROOT` enumera IDs/digests/estados/tamaños sin source;
`mutation prune --state-root ROOT --operation-id ID --plan-digest SHA` retira solo
un journal terminal validado, con cleanup ya acreditado en su fase durable, y exige
confirmar el digest exacto. Rechaza pendientes, staging, versión desconocida o
corrupción; no ejecuta Cargo ni toca source. La autoridad es el operador local/UID
del state root, separada del peer MCP: no es otro modo de la tool ni un ID bearer
para consultas MCP. No requiere que el workspace original todavía exista para
eliminar un recibo terminal. Conserva lock global y fsync/fullfsync del store.
La CLI no crea el store al consultarlo ni al limpiarlo. No hay retención automática.
Prune elimina evidencia e idempotencia durable de ese ID; antes de usarlo el operador
debe haber consumido el recibo y descartado sus planes. Reintentar una mutación
purgada requiere otro preview. La CLI debe calificarse antes de M2-01 Done.

## Alternatives considered

- IDs bearer: simplifican lookup pero conceden autoridad por conocimiento del ID.
- Journal en el proyecto: expone evidencia privada al contenido/código del proyecto.
- Locks por ProjectRef: permiten dos instancias sobre el mismo workspace físico.
- Estado distinto por instancia: rompe coordinación de múltiples procesos.
- Commit sin persistir before: no permite recuperación verificable.

## Consequences

La configuración por usuario debe compartir state root. Un store lleno rechaza
antes del source; no se elimina evidencia para conseguir espacio. Los receipts
durables y los artifacts efímeros M1 tienen lifecycle distinto. La creación de
Cargo.lock requerirá ampliar y calificar la primitiva antes de dependency.add.

## Status

Primera calificación nativa: macOS 26 ARM64/APFS; otros hosts rechazan antes de I/O.
El límite de 256 KiB del editor se aplica a manifests antes del límite general 1 MiB.

Accepted por el Technical Owner para M2-01. Implementación y calificación pendientes.
