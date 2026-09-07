# M2 — observabilidad local revisada

Claude Code CLI 2.1.260, `claude-sonnet-5`, effort medium, safe-mode/read-only,
sin tools ni MCP. [Inputs con hashes](M2-observability-inputs.json),
[respuesta íntegra](M2-observability-sonnet.json).

**Accepted; sin P0/P1 en el delta.** El revisor inspeccionó código y pruebas;
no ejecutó el runtime. El owner verificó aparte cancel/timeout/EOF de Fix con
build scripts y descendientes activos después de integrar los eventos: 1/1,
50.10 s, sin cambios host ni objetos Docker restantes. El gate conjunto conserva
la evidencia definitiva sobre el source final.

Los dos P2 se aceptan: un clone redundante acotado al finalizar el waiter y la
semántica agregada de `admitted`, que no distingue cada instante de rechazo de
autoridad. ADR-058 explicita que no acredita efectos ni permisos durables; las
métricas son locales y pueden ser null, no auditoría forense o garantía ante crash.
No se cambia ningún schema M1. Missing offline data usa unavailable conforme a
D01; la descripción add/remove requiere vendor solo para preview.

Los metadatos de Claude Code registran uso auxiliar de Haiku 4.5 además del modelo de revisión explícito. Se conserva el JSON íntegro; no se presenta ese uso auxiliar como una segunda revisión ni una sustitución del revisor solicitado.
