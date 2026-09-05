# ADR-059 — Liberar planes terminales y repetir desde el journal

Date: 2026-09-05

## Context

El cliente stock completó cuatro mutaciones y encontró limit_exceeded al preview de
la quinta: los planes comprometidos seguían contando en la cuota de cuatro durante
600 s. Esa cuota protege candidatos pendientes; no debe limitar una sesión a cuatro
operaciones secuenciales. Eliminar buffers sin otra vía de replay perdería los
reintentos tras perder una respuesta, porque commit resolvía siempre un plan RAM.

## Decision

Retirar un plan de la resolución y marcar su retención liberada solo después de
un receipt terminal conocido (Committed, NoChange o Aborted) con su ID y digest.
La marca es atómica y no necesita retomar un mutex tras el efecto durable; la próxima
admisión poda los buffers antes de aplicar cuotas, igual que los previews no entregados.
Cuatro planes activos/64 MiB y TTL 600 s permanecen. Un resultado incierto o error no
se interpreta como terminal. No hay cache adicional de receipts ni cuotas ampliadas.

Cuando commit no encuentra un plan RAM válido por ausencia o TTL, puede repetir
exclusivamente una operación YA journaled mediante ID, digest e idempotency key
exactos. Una nueva operación sigue exigiendo el candidato aprobado no expirado.
El port del publisher verifica grant vivo, kind/principal, path e identidad física,
locks global/workspace y journal íntegro antes de comparar digest/key. Solo después
puede devolver el receipt terminal o usar la recuperación ya existente de ese registro.
No se crea un registro nuevo, no se carga candidato del caller y no se revalida
Cargo en replay. Un registro legacy v1 terminal puede reescribirse explícitamente
en v2 después del binding exacto, como ya hacía commit/recovery; receipt continúa
read-only. Esa migración mantiene el presupuesto de crecimiento ya calificado.
Un ID ausente no provoca efectos; un digest/key distinto falla Conflict antes de
reparación. Un journal dudoso conserva RecoveryRequired. Prune sigue retirando esa
protección de replay. Esto también permite retry explícito después de reiniciar el
servidor o vencer el TTL, bajo la autoridad actual y con los tres valores exactos.

La registry revalida una referencia viva; tras commit/replay que devuelve resultado
o RecoveryRequired la invalida como ya hace commit. Un reference viejo no se convierte
en autoridad por conocer ID/key. La tool de otra operación no adquiere acceso.
No cambian DTOs, schemas M1, formato journal ni parámetros públicos. Las descripciones
de las cinco tools M2 explicitan invalidación y uso del nuevo data.project_ref,
además de diferenciar efectos nuevos de replay durable. Las guías
explican que TTL limita comenzar efectos nuevos y que la evidencia durable gobierna
replay de efectos ya autorizados. Runtime/servicios/instalación no se amplían.

## Alternatives considered

- Aumentar cuota o esperar TTL: desplaza el fallo y aumenta carga/memoria.
- Reiniciar el servidor entre tools: oculta el defecto y empeora Agent DX.
- Cache de tombstones/receipts: otra cuota/TTL y duplicación de autoridad durable.
- Olvidar sin replay: rompe recuperación de respuesta perdida/idempotencia.
- Reejecutar candidato ausente: efecto nuevo sin preview válido, rechazado.

## Consequences

Cinco o más mutaciones secuenciales pueden continuar dentro de cuotas del journal.
Siguen limitados cuatro candidatos pendientes. Replay terminal devuelve evidencia
histórica, no certifica que el source actual siga siendo ese after. Recovery de un
registro pendiente conserva todas las precondiciones/bytes desconocidos existentes.
La ruta nueva exige pruebas nativas de key/digest/root/kind/formato/prune y pruebas
MCP de secuencia, respuesta perdida, reopen/restart/TTL, además de revisión independiente.

## Status

Accepted por el Technical Owner para corregir el P1 observado en cliente M2.
Implementación y tests calificados en el [full posterior](../validation/M2-full-gate.json);
[recheck Opus Accepted](../reviews/M2-059-review.md), cliente PASS y [cierre M2](../validation/M2-07.md).
