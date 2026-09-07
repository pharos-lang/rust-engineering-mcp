# M2 — revisión final del writer nativo

Claude Code CLI 2.1.260, `claude-opus-5`, effort high, safe-mode/read-only,
sin tools ni MCP. [Inputs iniciales](M2-final-native-inputs.json),
[primera revisión](M2-final-native-opus.json),
[inputs del recheck](M2-final-native-recheck-inputs.json) y
[recheck íntegro](M2-final-native-recheck-opus.json).

**Accepted; cero P0/P1 pendientes en este alcance.** El revisor leyó el paquete;
no ejecutó las pruebas ni certificó el gate conjunto o M2 Done.

El P1 inicial sobre un journal parcial que bloquea el store compartido se cierra
como P1 y se acepta como P2 residual de disponibilidad. El owner publicó el límite
y la remediación manual de ADR-052; la [prueba reproducible](../validation/M2-native-remediation.json)
fija propagación a otro workspace, receipt intacto por ID, rechazo de la root
original en el store nuevo, continuación en una copia física nueva y preservación
de nombres/bytes originales. Se detienen todas las instancias; cada workspace que
deba continuar recibe una copia física revisada, grants nuevos y state privado nuevo.
No se repara ni certifica el journal original, se elimina evidencia, se transfiere
idempotencia ni se conecta una root en cuarentena al store nuevo.

Los P2 de headroom y pico de admisión se resolvieron: 207 MiB retenidos, 48 MiB
staging y 1 MiB de crecimiento; admisión antes de codificar el buffer. Las pruebas
miden 6044 bytes de crecimiento máximo v2 y 124 de conversión legacy, inferiores
a 8 KiB por registro. El techo no concede holgura retroactiva a stores de desarrollo
poblados bajo 208 MiB. Las dos precisiones documentales solicitadas en el recheck
(continuación por cada workspace y no retroactividad) están en ADR-052 y la guía.

La [medición histórica de memoria](../validation/M2-07-native-memory.json) aclara
que permanece el clone de JournalBody de peor caso. El reordenamiento posterior no
se presenta como una nueva medición RSS. El enum de fault productivo no-op se acepta
como P2. Los [faults nativos](../validation/M2-native-io-faults.json) son inyecciones
acotadas, no disco APFS físicamente lleno, fallos de todas las primitivas ni corte
de energía. El helper abrupt_checkpoint_helper requiere su variable de entorno;
un retorno vacío en el gate normal no es evidencia de un crash adicional.

No se promete exclusión OS de editores ni atomicidad visible multiarchivo. Se
mantiene el contrato local_coordinated y no se instala un broker o servicio nuevo.
Los metadatos de Claude Code incluyen uso auxiliar de Haiku 4.5; se conserva el
JSON, sin presentarlo como segunda revisión o sustitución del revisor explícito.
