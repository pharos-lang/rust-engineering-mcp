# Disposición del Technical Owner — trazabilidad M4

La revisión Gemini 3.8 Flash High (agy 1.1.27) es documental y auxiliar.
El paquete fue pasado íntegro como prompt, sin acceso necesario a herramientas;
la CLI advirtió que `--mode plan` no tiene efecto con slash commands desactivados.
No se atribuye a ese flag una barrera de permisos. El cwd fue temporal fuera del
repositorio y el paquete quedó conservado con hashes. La review no cierra M4.

- P2 imagen deny: los cuatro casos ahora seleccionan la imagen final25ed;
  el caso MCP incluye además retirada del plugin mediante retorno explícito a M3.
  El [runtime final 19/19](../../validation/M4-runtime.json) verifica esos casos.
- P2 revocación: se añade al caso MCP el restart en M3, nueva admisión deny
  rechazada y relectura de los mismos bytes privados con un ProjectRef nuevo del
  mismo owner. Se conserva también la retirada de policy dentro de la sesión; el
  [recibo final deny MCP](../../validation/M4-deny-mcp.json) pasó esos controles.
- P2 canarios: [privacidad final](../../validation/M4-privacy-runtime.json) pasó.
  El canario host quedó fuera de HTML/diff/streams; el canario de source autorizado
  apareció en HTML y un diff real como control positivo. No se promete redacción universal
  de source en los artifacts heredados de M3. SECURITY documenta la diferencia.
- P3 vendor vacío: limitación existente D05, documentada también para workspaces
  sin dependencias. Puede usarse el dataset autorizado no vacío aunque ninguno
  de sus paquetes participe en el grafo. No requiere un paquete dummy fabricado.
  Owner: Technical Owner; extensión de D05 fuera de M4.
- P3 envelope Miri: ADR-072 distingue ahora el límite del parser (OutputLimit)
  del rechazo del envelope compartido (InvalidMetadata). Permanece fail-closed.

El roadmap permanece Planned según el maestro; el cierre se registra en tablero
 y matriz de implementación, sin retroeditar la planificación histórica.
