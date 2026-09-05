# ADR-050 — Mutación local coordinada sin broker privilegiado

Date: 2026-09-05

## Context

El owner delegó expresamente la decisión para continuar M2 preservando la esencia
local del MCP y evitando una carga de instalación/uso. ADR-049 y su probe demostraron
que el UID interactivo no obtiene exclusión kernel frente a otros programas del
mismo usuario. Esa garantía adicional del plan D02 no era una capacidad de M1.
Exigir un servicio privilegiado, identidad separada o entitlement privado cambiaría
la operación de un repositorio ordinario y no es apropiado como requisito M2.

## Decision

Se adopta **local_coordinated**: edición local coordinada, con permisos explícitos,
precondiciones optimistas, publicación por archivo y recuperación conservadora.
Es un contrato de confianza distinto de exclusión OS, no un fallback oculto.

1. Instalación ordinaria del binario, sin daemon, cuentas adicionales, sudo, cambios
   de ownership/ACL del proyecto ni entitlement privado. La escritura permanece
   deshabilitada hasta un grant de roots/operaciones del host. Configurarlo una vez
   selecciona expresamente este modo; no hace falta un checkbox de exclusividad en
   cada llamada. El peer no puede conceder ese grant ni cambiar el modo.
2. Se confía en el host y en programas que el developer ejecuta directamente
   (editor, Git, shell), como asunción de no hostilidad. El código del proyecto
   lanzado por el MCP nunca pertenece a esa clase: solo se ejecuta en el gateway
   aislado y un grant de escritura no amplía la autoridad de ejecución. Deben mantener
   estables el namespace de roots y el estado privado del servidor, y evitar
   escrituras simultáneas sobre archivos afectados durante el commit. El
   developer puede trabajar normalmente durante el preview; el digest detecta
   diferencias hasta la lectura de validación. Una escritura entre esa lectura y
   publicación no se previene y su detección posterior tampoco es universal.
   Los locks coordinan instancias que comparten el state root privado ADR-052;
   la primera vertical mantiene además un lock global del store durante el commit
   para cubrir workspaces anidados. No bloquean IDEs, Git ni escritores externos.
3. Peer, configuración/contenido del proyecto y código ejecutado por Cargo siguen
   siendo no confiables. Root handles originales, full relative paths y flags kernel
   no-follow/beneath protegen I/O propio; hardlinks/no-regulares se rechazan.
   Código del proyecto permanece en el gateway aislado sin bind host escribible.
   No se debilitan red, env, cuotas, captura ni permisos de M1.
4. Preview produce candidato/diff/digest exactos únicamente después de que Cargo
   valide el candidato en el gateway. Commit rechaza planes cuyo digest no vincula
   ese resultado de validación exitoso; no vuelve a generar un candidato distinto. Commit valida grant/principal,
   TTL, idempotencia, source generation, root identity y bytes bajo el lock del MCP. Root identity
   compara un nuevo open protegido de la ruta host configurada con el handle
   original, antes/después de publicar; nunca compara el descriptor solo consigo mismo.
   Un conflicto detectado antes del primer efecto no escribe source. Esto no es CAS
   por contenido ni snapshot atómico frente a escritores externos.
5. Journal versionado y backups íntegros se hacen durables antes de publicar.
   [ADR-052](ADR-052-mutation-journal-and-authorization.md) fija ubicación fuera de
   roots, modo 0700/0600, binding de autoridad/idempotencia, cuotas y recuperación.
   Durable significa fsync/F_FULLFSYNC exitosos de archivo y directorio; no acredita
   supervivencia ante pérdida de energía. Registros esperados ausentes, truncados,
   corruptos o no verificables requieren recuperación conservadora, nunca replay
   con efectos basado en su ausencia.
   Reemplazo usa primitives kernel por archivo, nunca truncado in-place del source.
   No hay atomicidad visible multiarchivo, tampoco para lecturas M1 concurrentes.
   Después de publicar se verifican bytes e identidades observadas. Cambio desconocido,
   root movida o durabilidad incierta lleva a recovery_required y conserva evidencia.
6. No hacer un segundo swap automático para deshacer un conflicto con un editor
   externo. La recuperación solo modifica estados identificados como propios y
   conocidos, bajo el mismo contrato coordinado; ante bytes desconocidos se detiene.
   No se promete conservar en la ruta final una actualización concurrente tardía
   mediante un descriptor que otro programa mantenga abierto. Backups y diagnósticos
   ayudan a recuperar; no convierten esa carrera en exclusión demostrada.
7. Receipt declara modo, alcance de coordinación, before/after/provenance, cambios,
   commit/recovery y limitaciones. Las docs públicas deben explicar el permiso una
   vez, el flujo preview→commit y enumerar exactamente los archivos afectados y evitar editarlos durante commit.
   Esto incluye checkout/stash/pull/clean y mover la root durante commit. Cambiar
   archivos durante preview está permitido: exige preview nuevo si quedan stale.
   No auto-stash/reset/commit Git, auto-merge de cambios ajenos ni pérdida silenciosa
   de evidencia pendiente para cumplir TTL/cuotas.

Este ADR resuelve la **decisión de producto** D02. No califica todavía un writer ni
cierra M2: se requieren tests positivos con root estable/cooperación, y tests de
violación del contrato que demuestren rechazos observados y recuperación conservadora.
Se conserva el probe anterior como evidencia válida de las garantías que no se ofrecen.

## Alternatives considered

- Broker privilegiado con UID/namespace exclusivo: ofrece otra frontera, pero añade
  instalación administrativa, operación y restricciones al editor; fuera de M2.
- Leases privadas XNU: no disponibles para distribución ordinaria y requieren una
  calificación adicional de timeouts; descartadas como requisito.
- Swap seguido de validación presentado como CAS: incorrecto; el candidato ya fue
  visible y el rollback puede desplazar otra actualización. No se acepta ese claim.
- Preview-only como M2 final: no cumple las cinco mutaciones normativas.
- Escritura optimista sin journal, backup ni permiso: menor trabajo pero protección
  insuficiente para los efectos autorizados.

## Consequences

Se reduce explícitamente la garantía adicional del plan: no se ofrece exclusión de
escritores locales arbitrarios ni protección contra un usuario host malicioso. La
autoridad de roots y el sandbox frente a proyecto/peer siguen enforced. La UX
permanece local, con una configuración de escritura y sin servicio del sistema.
Un peer con grant vigente puede mutar el scope autorizado sin una aprobación
   humana por archivo: preview/commit vincula el plan, no prueba revisión humana.
   Cuotas de escritura/retención son independientes de M1 y rechazan, no truncan.
La duración de commit y la tasa de conflictos deben medirse; pausas largas y recovery
frecuente son defectos de UX que bloquearían el cierre, no responsabilidad ignorada.
Un backend remoto/multiusuario futuro deberá reevaluar esta TCB; no hereda la decisión.

## Status

Accepted por el Technical Owner bajo la delegación explícita del owner del 2026-09-05.
Sustituye la exigencia de exclusión OS fuerte en D02; ADR-049 conserva evidencia histórica.
Implementación y calificación positiva completadas: [cierre M2](../validation/M2-07.md).
