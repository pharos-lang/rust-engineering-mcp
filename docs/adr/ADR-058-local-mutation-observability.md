# ADR-058 — Observabilidad local de las mutaciones

Date: 2026-09-05

## Context

G3 exige observar admisión, rechazos, duración, cancelación, timeout, cleanup y
retención. M2 debe cumplirlo sin un colector, servicio, cuenta o configuración
adicional. Los resultados y receipts existentes describen la operación, pero no
bastan para registrar una cancelación cuya respuesta suprima el cliente.

## Decision

Emitir como máximo un evento estructurado y acotado de terminación por llamada M2 que pasó decode y conversión del DTO,
mediante el tracing/stderr existente. Campos cerrados: tool, fase pública
preview/commit/receipt/recover, estado/razón, duración total de trabajo y cleanup (incluido el join cuando el waiter sigue vivo), indicador de cleanup incierto, ID opaco producido cuando exista,
archivos cambiados y conteo/bytes de planes asignados. Una llamada rechazada
conserva su razón; no se la cuenta como admisión exitosa. El evento no representa
la terminación de cada subprocess individual ni un resultado durable perdido.

Si el SDK descarta el waiter por EOF/cancelación, un fallback unido al worker emite después de terminar el trabajo y cleanup; no afirma entrega de respuesta ni mide un join async que ya no existe. Un control de emisión única evita duplicados al competir el retorno normal y el fallback.

Si el waiter se abandona antes de que el worker entre en el trabajo, puede no existir evento de terminación: no se promete auditoría completa de admisión ni entrega ante crash. `admitted` indica entrada al worker sin rechazo de autoridad; no acredita un commit, un mutador ejecutado ni un permiso durable. El estado y razón conservan la distinción de resultado.

No registrar argumentos, referencias del cliente, rutas, source, diffs, Cargo
stdout/stderr, variables, credenciales ni idempotency keys. Los IDs solo proceden
de resultados validados. Las llamadas que no decodifican conservan el error MCP
existente y no registran el payload. No leer RUST_LOG ni habilitar logs del SDK.
Los tests de stdio verifican los campos y excluyen cualquier otra salida.

Las métricas son observaciones locales por llamada, no un servidor HTTP ni
telemetría externa. Conteo/bytes describen planes aún asignados, incluidos los
revocados y terminales retirados que esperan pruning en la próxima admisión; no son RSS ni una medición
del journal. La CLI `mutation list` ofrece el inventario durable y cuotas.
Diff y resultado excedidos se rechazan completos: no hay cambios omitidos aplicados.

El proceso no crea archivos de logs; su retención pertenece al host que consume
stderr. Los receipts permanecen en el store privado autorizado, con borrado
explícito conforme a ADR-052. Las fases internas durables y su recuperación se
verifican mediante las pruebas nativas y journal, sin exponer paths en logs.

## Alternatives considered

- Añadir un collector o daemon: carga de instalación sin necesidad para M2.
- Habilitar logs arbitrarios del SDK: puede revelar payloads del peer.
- Confiar solo en respuestas: una cancelación/EOF puede impedir su entrega.
- Nuevos schemas públicos de métricas: amplían el contrato sin mejorar la operación.

## Consequences

Los consumidores de stderr verán mensajes operativos M2 acotados. stdout y los
18 schemas permanecen iguales; el comportamiento y logs normales de M1 no cambian.
El host puede agregar eventos, pero no se prometen métricas globales persistentes,
auditoría resistente a un host malicioso ni supervivencia de logs ante crash.

## Status

Accepted. Redacción y lifecycle calificados en el [cierre M2](../validation/M2-07.md).
