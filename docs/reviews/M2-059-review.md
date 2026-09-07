# M2 — retiro terminal y replay durable (ADR-059)

Claude Code CLI 2.1.260, modelo explícito `claude-opus-5`, esfuerzo high,
read-only, sin tools ni MCP. [Inputs iniciales](M2-059-inputs.json),
[dictamen inicial](M2-059-opus.json), [inputs de recheck](M2-059-recheck-inputs.json)
y [recheck íntegro](M2-059-recheck-opus.json). Los prompts originales se conservan
en [paquete inicial](m2-059-packages/M2-059-opus-prompt.txt) y
[paquete de recheck](m2-059-packages/M2-059-recheck-opus-prompt.txt), con SHA iguales
a los registrados en los inputs. El recheck tiene SHA-256
`e98457b081cc9ea1a19b82a7855f65303f792275a687b2285eb3ec701a29518c`.

**Accepted en recheck, sin P0/P1 pendientes en el delta.** El Technical Owner
acepta el dictamen y las disposiciones siguientes. La revisión estática no
certifica el full, la ejecución nativa ni la calificación de Claude como cliente.
Los metadatos registran uso auxiliar de Haiku 4.5; no es otro reviewer ni una
sustitución de Opus 5.

| Finding inicial | Disposición verificada en recheck |
| --- | --- |
| P2: descripciones y NotFound contradicen replay tras TTL/reinicio | Corregido en las cinco descripciones M2, snapshots y mensaje: efectos nuevos requieren preview vigente; replay exige journal e ID/digest/key exactos. Reabrir y usar el nuevo `data.project_ref` después de commit. |
| P3: migración v1→v2 omitida del ADR | ADR-059 explicita la posible reescritura tras binding exacto. Receipt sigue read-only; replay puede recuperar o migrar un registro existente. |
| P3: staging válido/no creciente y daño ajeno | Oráculos nativos añadidos, con snapshots de árboles y preservación de bytes desconocidos. |
| P3: mock confunde receipt/replay | Vector de replay separado con ID/digest/key y aserción de que receipt no fue llamado. |
| P3: stats incluyen terminales retirados | ADR-058 aclara que son asignaciones hasta la siguiente poda, no ranuras disponibles ni RSS. |
| P3 condicional: auditoría tardía | Recheck de workers demuestra que el join conserva el resultado tardío; no se confirmó el fallo alcanzable. |
| P3 adyacente: invalidación recover | La revalidación lazy de identidad/source es preexistente; no se amplía autoridad. |

La expiración se cubre por composición: reloj inyectable de planes y rama común
`NotFound | Expired`. El runtime hace cinco commits, retry, restart, identidad
errónea y source avanzado. No simula pérdida real del paquete de respuesta ni
espera 600 s en wire. La reserva de crecimiento v1→v2 conserva su prueba nativa;
no se atribuye una medición nueva al recheck.

El owner verificó el 2026-09-05 que todos los hashes `current_sha256` del recheck
coincidían con el checkout antes de congelar el full posterior. La consolidación
documental de cierre es posterior y no se atribuye a este reviewer. El nuevo
[full](../validation/M2-full-gate.json) ejecuta archivos completos de application
y native, además del [runtime](../validation/M2-final-runtime.json); solo su
resultado final acredita esa ejecución, no este informe.

La frase histórica del recheck sobre clientes fallidos describe su paquete previo.
El [recibo posterior](../validation/M2-clients.json) es PASS estricto en intento 5:
Claude stock con prompt v2 que exige explícitamente renovar referencias. No prueba
que basten las descripciones ni éxito al primer intento. Los intentos 1–4 permanecen
fallidos en [histórico](../validation/m2-clients/). No se repiten dictámenes Accepted
sin un delta concreto que lo requiera.
