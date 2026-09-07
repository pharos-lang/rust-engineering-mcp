# M2 D02 — revisión independiente del experimento

Estado: **aceptado como evidencia negativa del candidato**, no gate M2 aprobado.
Fecha: 2026-09-05. Claude Code 2.1.260, `claude-opus-5`, `--effort high`, read-only:
print, tools vacías, MCP estricto sin servidores, sin persistencia de sesión.
Metadata de respuestas confirma el modelo; no se usó sustituto ni se delegó decisión
de arquitectura. El revisor no ejecutó pruebas ni calificó capabilities de producto.

Primera revisión: [inputs/SHA](M2-D02-inputs.json), [respuesta](M2-D02-opus.json).
Pidió corregir tres P1 de recibo: fullfsync siempre true, claim de inodes sin medición,
y flags hardcoded sin publicar hashes observados. No rechazó el No-go acotado.

Correcciones: fullfsync compara errno0; snapshots publican SHA/size/dev/ino; flags
se derivan de bytes; root handle guarda identidad antes/después; errnos exactos y
simbólicos; tiempos monotónicos por subprocess; JSON+exit70 de infraestructura;
límites hardlinks/mmap/EXDEV/power-loss; no assert eliminable ni falso éxito sin errno.
El probe se repitió sobre APFS y produjo 15 observaciones coincidentes, salida0 y
`no_go_current_candidate`. La primera ejecución fallida de detección de FS y el
recibo previo se conservan, sin hacerlos pasar por calificación final.

Re-review: [inputs/SHA](M2-D02-recheck-inputs.json), [respuesta aceptada](M2-D02-opus-recheck.json).
No quedan P0/P1. Disposición de P2 no bloqueantes para evidencia negativa:

| Finding | Disposición del Technical Owner |
| --- | --- |
| P2-1 predicado y snapshot leen dos veces | Aceptado como limitación del harness actual: fixtures controladas, hijo terminado antes de ambas lecturas, sin actor asíncrono entre ellas. No acredita observación atómica bajo escritor externo persistente. Captura única será obligatoria si se reutiliza para un gate positivo M2-01. |
| P2-2 descriptor movido aun con flags | Matriz corregida para decir explícitamente que no-follow/beneath no reanclan ese descriptor. |
| P2-3 fuente entitlement | ADR-049 enlaza fuente Apple/tag/fecha; EPERM solo acredita indisponibilidad. |
| P2-4 versión intérprete | El recibo no la capturó en la ejecución. Verificación posterior registra su propio intérprete, sin atribuir retroactivamente metadata al probe. Agregar identidad del intérprete antes de reutilizar harness en gate positivo M2-01. |
| P2-5 timings nombrados por binario | Cuatro nombres distintos en esta ejecución, verificados sin colisión. Usar ID de paso explícito si se añaden llamadas repetidas en M2-01. |

El Technical Owner verificó por separado [SHA, timings, inodes y ausencia de cambios
de producto](../validation/m2-d02-verification.json). Los cambios posteriores al
manifest de re-review son aclaraciones documentales; no se atribuye al revisor
lectura de esos bytes posteriores. No se ejecuta Cargo/full por un probe aislado
sin modificaciones Rust, manifests, schemas o workflows; ese gate sigue pendiente
para un candidato M2 real.
