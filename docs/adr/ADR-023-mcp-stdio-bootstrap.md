# ADR-023 — Bootstrap MCP stdio acotado

Date: 2026-09-03

## Context

M0-03 materializa ADR-002/003/012. `rmcp` 3.2.0 soporta discovery moderno y
initialize legacy; su `AsyncRwTransport` usa `read_until` sin límite de línea.
Todavía no existe un caso de uso de proyecto que justifique publicar una tool.

## Decision

- Incorporar `rmcp =3.2.0` con `server` y `transport-io`, sin default features;
  Tokio `=1.53.1`, tokio-util `=0.7.19` para el token de cancelación del SDK,
  tracing `=0.1.44` y tracing-subscriber `=0.3.23`. El adapter vive
  en el binario; el dominio no depende del SDK. Cargo.lock fija transitivas.
- Aceptar únicamente `serve --stdio` para servir. Mantener help/version y rechazo
  de argumentos sobrantes. No abrir HTTP ni cargar configuración del proyecto.
- Usar `ServerHandler`, `serve_server` y el transporte async I/O de rmcp. Declarar
  explícitamente las cinco versiones probadas: 2024-11-05, 2025-03-26, 2025-06-18,
  2025-11-25 y 2026-07-28. Discovery y negociación pertenecen al SDK.
- Anunciar solo capability `tools`, con lista vacía y determinista. No publicar
  placeholders. M0-04 añade la primera tool; schemas/resultados de tools siguen en
  M0-07. No añadir application/ports sin consumidores reales.
- Envolver el lector del transporte con un presupuesto de **1 MiB por línea**,
  excluyendo LF (CR cuenta). Procesar chunks de como máximo 8 KiB; no parsear JSON
  ni implementar errores JSON-RPC propios. Exceso o EOF con línea incompleta
  cierran la conexión con exit 1, sin prometer respuesta para esa línea. Este
  límite no es un presupuesto global de memoria, concurrencia o salida.
- Registrar fallos de lectura/escritura y cancelar la sesión del SDK, sin exponer datos del peer. Logging por
  tracing a stderr, solo targets del binario y sin filtro tomado del entorno.
  Los logs internos del SDK pueden incluir contenido no confiable y se suprimen.
- EOF limpio antes o después de bootstrap termina con exit 0; fallo de transporte
  o bootstrap inválido termina con exit 1 y diagnóstico fijo. Apagar el runtime
  con espera acotada para no depender de la cancelabilidad de stdin de Tokio.
- Conservar el comportamiento del SDK ante JSON inválido: ignora sintaxis inválida;
  formas de mensaje inválidas producen Invalid Request. No prometer -32700.

## Alternatives considered

- Stdio sin presupuesto: permite crecer indefinidamente el buffer de entrada.
- Parser/framing JSON-RPC propio: duplica protocolo y crea otra fuente de verdad.
- Codec limitado con otro transporte: cambia la recuperación de errores del
  transporte estándar. Un guard de bytes conserva el parser y lifecycle del SDK.
- Publicar las trece tools sin implementación: anunciaría capacidades inexistentes.

## Consequences

Se prueba el binario real con JSON independiente del SDK, moderna/legacy, EOF,
framing adverso y separación de streams. Solo el target local está acreditado;
Linux/Windows quedan para M0-11. No hay sandbox ni ejecución externa.

La cancelación de rmcp es cooperativa: M0-03 comprueba notificaciones desconocidas
sin efectos y no tiene operaciones largas. El primer request se ejecuta inline
durante bootstrap; antes de incorporar operaciones largas deberá resolverse cómo
observar cancelación en ese camino. Límites de concurrencia/salida, deadlines y
terminación de procesos requieren los cortes de ejecución posteriores.

## Status

Accepted. Gate y revisión de implementación: `docs/validation/M0-03.md`.

Sources: <https://docs.rs/rmcp/3.2.0/rmcp/>,
<https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/stdio>,
<https://modelcontextprotocol.io/specification/2026-07-28/basic/versioning>.
