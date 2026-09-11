# Revisión independiente G8 — logs del harness como artifacts

Eres un revisor **independiente y de solo lectura** de
`/Users/cburgosro/Projects/rust-mcp`, rama `ai/m5-performance`, commit `1db78af49168a48d5151ff32ad03aea4b0cbefde`.

**No edites nada. No ejecutes comandos git de escritura, Docker, gates ni pruebas
`#[ignore]`.** Puedes leer todo y correr comprobaciones de solo lectura acotadas
a un paquete.

## Contexto

Tres textos publicados —la descripción congelada de `rust.benchmark.run`,
`docs/tools.md` y ADR-076— decían que los logs del harness quedan en el artifact
del árbol de criterion. No quedaban en ninguna parte: ese payload es un export
USTAR de `CRITERION_HOME` y el adapter capturaba `stdout`/`stderr` y los tiraba.
Con un fallo observado, el llamador iba a buscar el error del compilador a un
archivo que no puede contenerlo; con un harness no reconocido no se publicaba
nada.

El owner decidió implementar la capacidad en vez de borrar la promesa. La
especificación es `docs/adr/ADR-080-harness-logs-as-artifacts.md` y es
vinculante.

## Qué tienes que juzgar

1. **¿Se cumple ADR-080 §1–§6, o solo lo que hacía falta para que las pruebas
   pasaran?** Es la pregunta central.
2. **§4, la asociación repetición ↔ archivo ↔ logs.** La regla anterior era falsa
   en un caso alcanzable: el árbol se elige con `rfind` sobre las repeticiones que
   exportaron, y el exit viene de la última que corrió, así que pueden ser
   distintas. ¿Está ahora **detectable desde el cable**, o solo redactada de otra
   forma? Construye el caso: tercera repetición falla sin exportar, primera y
   segunda exportaron.
3. **Truncación.** El techo es 256 KiB por flujo y repetición, por debajo de los
   512 KiB que captura el supervisor. ¿Es alcanzable de verdad ese corte, o es
   código muerto? ¿Un log cortado puede publicarse alguna vez como completo?
   ¿El corte respeta frontera UTF-8, de modo que la declaración `Utf8LogV1` siga
   siendo cierta?
4. **§5: nunca al stdout del servidor MCP.** ¿Hay algún camino por el que un byte
   de log llegue ahí?
5. **Privacidad y límites.** Los logs salen de compilar y ejecutar código del
   proyecto. ¿La sensibilidad, el TTL, la cuota y la propiedad son las correctas?
   ¿Puede un log llevar algo que el contrato no admite publicar?
6. **Las pruebas.** Para cada una, ¿qué tendría que romperse para que fallara?
   ¿Se debilitó alguna aserción previa en vez de corregirla?

## Cómo revisar

Prefiere evidencia sobre prosa. Di claramente cuando algo esté bien; una revisión
que fabrica hallazgos para parecer rigurosa es peor que inútil.

Clasifica P0/P1/P2/P3 y da **un** veredicto: **Block** o **Pass**. Bloquean los
P0/P1 y cualquier P2 que permita publicar evidencia que no describe lo que dice
describir.

## Entrega

La revisión completa como último mensaje: veredicto, hallazgos con `archivo:línea`
y un caso concreto de fallo cada uno, y una sección de lo verificado y hallado
sólido. Nombra el commit. No escribas nada en disco.
