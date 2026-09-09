# Revisión independiente M5 — método de medición y contratos públicos

Revisor: Claude Opus 5, read-only, esfuerzo alto. Fecha: 2026-09-08.
Rama `ai/m5-performance`. Inputs congelados: [inputs.json](inputs.json).

## Veredicto del revisor

**Block.** La postura del milestone es buena —el bloqueo M5-01 es real y se
rechazó en vez de maquillarse, los límites del vendor no se ampliaron, los exits
siguen sin calificar y la separación entre tamaño exacto y atribución estimada es
lo mejor construido del milestone— pero el artefacto central, el método de
comparación, cuantifica la fuente de varianza equivocada. El revisor produjo un
veredicto `improvement` falso a partir de las capturas reales que este propio
milestone tiene commiteadas, sobre código fuente que no cambió.

## Findings

| Severidad | Finding | Disposición |
| --- | --- | --- |
| P1-1 | El intervalo y el MDR miden la dispersión *dentro* de una ejecución, no la deriva *entre* ejecuciones, que es la que decide el veredicto | **Confirmado por reproducción y en corrección** |
| P1-2 | Las cuatro tools no se anuncian; `tools/list` devuelve 27 y no hay snapshots | **No procede: árbol obsoleto** |
| P1-3 | Los dos recibos de calibración se capturaron sobre una imagen que ADR-077 prohíbe | **Aceptado** |
| P2-1 | Una dispersión degenerada vuelve vacua la puerta de precisión y da un intervalo de ancho cero | **Aceptado y en corrección** |
| P2-2 | «Unknown permanece unknown» solo es cierto de un campo; cuatro más no se comparan | **Aceptado** |
| P2-3 | La salida de compare no lleva provenance, así que «diferencias visibles» no se cumple | **Aceptado** |
| P2-4 | La fixture `control` se sigue afirmando como control 1,00x en cinco sitios | **Aceptado** |
| P2-5 | Ningún recibo M5 contiene MDR, intervalo ni veredicto observado | **Aceptado** |
| P2-6 | Bonferroni lleva el cuantil por debajo de lo que 10 000 remuestreos resuelven | **Aceptado** |

No hubo findings P0.

## P1-1 — reproducido

El revisor afirmó que comparando `m5/control` a solas, baseline
`criterion-run-2.tar` contra candidate `criterion-candidate.tar`, el método
devuelve `improvement` para código sin cambios. Se escribió el oráculo y falla
exactamente como describe:

```
m5/control alone: unchanged source produced Improvement
(effect -0.1231, interval -0.1492..-0.0756, mdr 0.0488)
```

`work_noisy` y `work_slower` son idénticos en las tres capturas; solo `work_unit`
cambia en el candidate. Las mismas dos funciones se mueven un 14,0 % y un 4,9 %
entre capturas sin que su fuente cambie, contra un umbral material del 5 %. El
intervalo no ve esa deriva porque remuestrea dentro de una sola ejecución.

Los dos oráculos nulos quedan en `criterion_dataset.rs::real_guest_datasets` como
guardia permanente.

## P1-2 — no procede

El revisor leyó un árbol anterior al registro de las tools. En el árbol actual
`list_tools` empuja las cuatro definiciones (`stdio.rs:681-690`), hay 32
snapshots (27 previos intactos desde el commit M4 + 4 nuevos + `doctor-report`),
y las seis aserciones de conteo dicen 31. La suite completa del workspace pasó
con 80 binarios de test y cero fallos sobre esos bytes.

El texto completo del revisor se conserva en [findings.md](findings.md).
