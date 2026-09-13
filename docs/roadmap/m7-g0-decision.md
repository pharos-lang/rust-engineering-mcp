# M7-G0 — Acta de decisión de puerta: **Deferred (no-go)**

Estado: **Deferred con decisión**. Fecha: 2026-09-13. Responsable/owner:
Cesar Burgos Rodríguez. Fuentes normativas: [m7-remote.md](m7-remote.md) (M7-G0),
[m2-m8.md §M7](m2-m8.md), spec §8.1 / §97 M7,
[ADR-003](../adr/ADR-003-stdio-first.md), [ADR-023](../adr/ADR-023-mcp-stdio-bootstrap.md).

## Decisión

**No-go a la ejecución remota de M7 (0.7.x); M7 queda `Deferred` con decisión
registrada.** No se implementa ninguno de los cortes M7-01..06 ni se inicia
diseño de HTTP/OAuth/tenancy/executor remoto. El trabajo pasa a **M8**
(estabilización local hacia 1.0).

## Motivo

El roadmap define M7 como `Conditional` y su ejecución/release como `Deferred`
hasta un **Go**, que exige (M7-G0) un expediente con **caso remoto real
aprobado**. Ese expediente **no existe**: no hay owner/operador ni tarea y
frecuencia reales, ni usuarios, datos/residencia, concurrencia medida, costo y
límites, SLO propuesto, ni una comparativa que **refute** que las alternativas
`stdio` / SSH / devcontainer / runner administrado satisfacen la tarea. Por el
criterio explícito de M7-G0 —*"Go exige todos los elementos; ausencia de uno
mantiene Deferred"*— y por *"No-go actual por falta de evidencia"*, la decisión
correcta es **Deferred**, no una release 0.7.x forzada.

No se inventan usuarios, presupuesto, IdP, infraestructura ni fechas (prohibido
por M7-G0). Esta acta registra la **ausencia** de caso, que es precisamente lo
que mantiene la puerta cerrada.

## Aceptación del owner

Aceptación explícita del owner (requisito de M7-G0): instrucción directa de
Cesar Burgos Rodríguez en la sesión del 2026-09-13 —generar el acta Deferred,
integrarla a `main` y preparar el arranque de M8—. Este documento es el acta de
esa decisión.

## Consecuencias

- **No se toca** la superficie remota: sin `serve --http`, sin Authorization
  Server, sin multi-tenancy, sin executor remoto. `stdio` (ADR-003/023) sigue
  siendo el único transporte.
- **M8 procede**: su DoR *"M6 cerrado y M7 cerrado o Deferred con decisión"*
  queda satisfecho por (a) el cierre local calificado de M6 —gate `full`
  `sha256:69a0be14…`, matriz de clientes `sha256:cb11315e…`, G1–G9 dispuestas
  (ver [docs/validation/M6/handoff.md](../validation/M6/handoff.md))— y (b) esta
  acta. M8 depende de la **decisión**, no de un Go.
- **Sin release 0.7.x**: no hay 0.7.x; la numeración salta de 0.6.x (M6) a la
  línea de estabilización 0.8/0.9 → 1.0 de M8, sin release ficticia de M7.

## Reversibilidad

`Deferred` **no** implica que el remoto sea imposible. Una reevaluación futura
requiere el expediente M7-G0 completo (caso real, métricas medidas, comparativa
con alternativas, SLO/cuotas/retención, IdP/executor reales) y una nueva
aceptación explícita del owner; recién entonces se decidiría Go y se ejecutarían
M7-01..06 con G1–G9. Nada en M8 crea dependencia con disponibilidad remota
supuesta.

## Alcance de M6 al momento de esta acta

M6 (analyzer, 0.6.x) está **Done local y calificado** en la rama
`ai/m6-analyzer`: cinco tools del analyzer (inventario 36), gate `full` verde
sobre bytes finales (42 etapas, `source_inputs_unchanged: true`), matriz de
clientes stock verde, G1–G9 satisfechas, sin P0/P1/P2 abiertos. Esta acta se
integra a `main` junto con el cierre de M6 para dejar una base limpia de M8.
