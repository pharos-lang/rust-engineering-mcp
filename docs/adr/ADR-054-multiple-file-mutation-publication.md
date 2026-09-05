# ADR-054 — Publicación de varios archivos existentes

Date: 2026-09-05

## Context

M2-01 califica un lint del manifest raíz. fmt.apply necesita reemplazar varios
`.rs`, incluidos descendientes. Guardar un journal completo por cada archivo y
fase multiplicaría innecesariamente el I/O; el journal ya contiene bytes y nodos
suficientes para reconocer cada lado de un swap tras un crash.

## Decision

Generalizar el publisher nativo a una lista ordenada de hasta128 reemplazos de
archivos existentes. Cada elemento liga path, source node, staged node y temporal
exclusivo `.rust-mcp-mut-<id>-<index>.swap` situado en la raíz exacta autorizada.
Los destinos descendientes se resuelven completos desde ese root original con
NOFOLLOW_ANY/BENEATH; cleanup solo usa hojas de temporales en ese mismo handle.
El writer queda ligado a una operación autorizada: manifest_patch o format_apply.
Un grant de formato no concede patch de manifest ni consulta de sus receipts.

El kind de formato verifica alcance, tamaños y bytes aprobados en la frontera
nativa; no interpreta semántica Rust ni acredita por sí solo el origen rustfmt.
Esa evidencia procede del productor aislado y su postcheck, ligados al digest por
la aplicación. SourceBundle ordena paths y SourceFile impone 1 MiB por archivo;
son invariantes del dominio, adicionales a las comprobaciones del publisher.

Fases globales: Prepared → Scratch → Staged → Applying → Published → Committed.
Aborted/NoChange y RecoveryRequired conservan la semántica de ADR-052.

1. Capturar full source y verificar generación, operación y todos los nodos.
   Reservar cuota y persistir plan/before/after/paths/temp intents antes de efectos.
2. Crear todos los clones exact-before sin truncarlos. Persistir todos sus inodes
   en Scratch antes de reescribir cualquiera. Prepared interrumpido puede adoptar
   solo clones exact-before bajo las reglas de ADR-052; si falta un clone, no hay
   obligación de crearlo durante recovery: conservar original y abortar/limpiar.
3. Reescribir/verificar todos los clones y persistir Staged. En Scratch, con todos
   los originales intactos y nodes propios durables, los temporales son scratch.
4. Verificar nuevamente la generación completa excluyendo únicamente temporales
   propios de esta operación por nombre exacto + inode. Persistir Applying antes
   del primer swap; publicar en orden y verificar cada pareja de bytes/inodes.
5. Cuando todo source coincide con after, persistir Published antes de limpiar
   originales desplazados. Retirar solo temporales verificados; persistir Committed
   después de cleanup durable. El recibo no anuncia éxito antes de ese punto.

No hace falta reescribir el journal completo tras cada swap: los nodos registrados
antes del primer efecto y cada pareja activa/temporal distinguen un prefijo after
y un sufijo before. Tras una interrupción sin efectos se aborta sin publicar.
Con un prefijo ya publicado, recovery puede completar el sufijo aprobado únicamente
cuando toda la generación lógica coincide con ese estado mixto conocido, incluidos
archivos no editados. No se vuelve a ejecutar rustfmt/Cargo ni se calcula otro diff.
Unknown, orden incompatible, source ajeno modificado, nodo cambiado o temporal
perdido antes de Published dejan RecoveryRequired y no activan rollback ni avances.
En Published, temporales ausentes representan cleanup ya efectuado y se conservan
los checks de source after y nodos antes de retirar los restantes.

Cancelación anterior al primer efecto aborta. Tras el primer efecto se termina
publicación/cleanup o se conserva recovery_required; no se devuelve cancelled por
una operación que ya publicó. Los locks global/shard se mantienen hasta finalizar.
La captura lógica excluyente es privada del writer y exige los nodos exactos del
journal: no altera ProjectSourceBackend::source ni el comportamiento M1 y no acepta
patrones de exclusión aportados por el peer.

Usar journal v2 para nuevas operaciones. Conservar un decoder estricto de v1 y su
checksum sobre el cuerpo v1 original; solo después de verificarlo se representa
internamente como una operación manifest de un archivo. Receipt/list no migran.
La recuperación explícita o replay de commit autorizado pueden persistir ese
registro en v2 bajo los mismos locks; antes de ello se conservan las reglas v1
de reconocimiento, especialmente Staged que pudo publicar antes de interrumpirse.
Un formato desconocido nunca provoca efectos ni limpieza. No se admite downgrade
del writer experimental después de escribir v2. Ninguna release publicada contiene
aún writer. Los registros experimentales deben consumirse/prunearse antes de
volver a un checkout antiguo; no hay migración automática al iniciar el MCP.

El host concede formato mediante `--allow-fmt-write WORKSPACE_ROOT`, separado de
`--allow-manifest-write`. Ambos requieren el runtime aprobado y journal externo a
las roots. Cada instancia del publisher liga exactamente un MutationKind; la
lectura/recuperación MCP verifica también ese kind. La CLI administrativa local
puede enumerar/prunear registros terminales de ambos tipos bajo su autoridad UID.
El payload completo se escribe por fase global, no por archivo: costo de journal
acotado por fases × bytes del plan. Medirlo con source/cantidad máximos antes de Done.

La reserva se calcula con la codificación del peor estado de fase (todos los nodos
temporales presentes, coordenadas/tamaño decimal máximos, contador de secuencia
máximo y nombre de fase más largo), no con un margen constante sobre Prepared.
Se rechaza antes de crear journal/temporales si ese estado no cabe en 48 MiB o
si sus dos copias transitorias no caben en la cuota del store.

## Alternatives considered

- Un commit independiente por archivo: oculta efectos parciales al contrato del plan.
- Reescribir todos los bytes de journal después de cada archivo: I/O proporcional
  a archivos × tamaño total, innecesario cuando el estado de swaps es reconocible.
- Rollback inverso automático: podría desplazar bytes externos; rechazado.
- Ignorar globalmente `.rust-mcp-mut-*` durante source capture: ocultaría archivos
  ajenos y cambiaría M1; solo se excluyen nodes propios exactos en captura interna.
- Publicar archivos nuevos/Cargo.lock: requiere otra primitiva, fuera de este corte.

## Consequences

La recuperación puede completar efectos ya aprobados en un plan parcialmente
publicado; se conserva la promesa de no publicar una generación desconocida.
La operación no es visible atómicamente a IDEs ni otros lectores. El namespace
confiable y nombres reservados de ADR-050/052 siguen siendo condiciones del host.
Sin la nueva captura lógica y pruebas de crash por prefijo no se habilita fmt.apply.

## Status

Accepted. M2-02, compatibilidad de journal y gate calificados en [M2](../validation/M2-07.md).
