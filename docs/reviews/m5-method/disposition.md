# Disposición del Technical Owner — revisión de método y contratos M5

Fecha: 2026-09-08. Revisor: Claude Opus 5, read-only. Rama `ai/m5-performance`.

La revisión encontró un defecto real en el corazón del milestone y lo demostró
con las capturas que este propio repositorio tiene commiteadas. Se acepta el
veredicto de bloqueo.

## P1-1 — el intervalo mide la varianza equivocada

**Confirmado por reproducción, no solo aceptado.** Se escribió el oráculo que el
revisor describe y falla exactamente como dice:

```
m5/control alone: unchanged source produced Improvement
(effect -0.1231, interval -0.1492..-0.0756, mdr 0.0488)
```

`work_noisy` y `work_slower` son idénticos en las tres capturas; solo `work_unit`
cambia en el candidate. El método devuelve dirección para código que no cambió, y
la puerta de precisión no lo impide porque el MDR se calcula de la misma
dispersión intra-ejecución. Las mismas dos funciones se mueven un 14,0 % y un
4,9 % entre capturas sin cambio de fuente, contra un umbral material del 5 %.

El diagnóstico del revisor es correcto y su control τ = 0 lo aísla: con deriva
entre ejecuciones nula, el SE creído coincide con el observado y la tasa de falsos
es exactamente cero. El método es correcto para una sola ejecución homogénea y
equivocado precisamente para la comparación de dos ejecuciones que existe para
hacer.

Corrección en curso, siguiendo la opción (a) del revisor y no la (c): la unidad
de remuestreo pasa a ser la ejecución (bootstrap por conglomerados), `RawSample`
gana su `run_index`, el formato del dataset sube a v2 —nunca se publicó—, y una
sola ejecución por lado deja de admitir dirección alguna, con una razón nueva que
lo dice. La opción (c) habría hecho los veredictos honestos sin hacerlos
correctos; no es suficiente.

Consecuencia que se acepta explícitamente: las tres capturas commiteadas son una
ejecución cada una, así que el oráculo de «regresión de dirección conocida» sobre
`m5/reference` deja de poder afirmar una dirección. Es la respuesta correcta —con
una ejecución por lado no se puede separar un efecto del código del 12,8 % de una
deriva de máquina del mismo orden, y esa deriva está medida— y el oráculo se
reescribe para afirmar lo honesto conservando la medición.

## P1-2 — no procede: árbol obsoleto

El revisor leyó el árbol antes de que terminara el registro de las tools. En el
árbol actual `list_tools` empuja las cuatro definiciones, existen 32 snapshots
—los 27 previos intactos desde el commit M4, cuatro nuevos y `doctor-report`— y
las seis aserciones de conteo dicen 31. La suite completa del workspace pasó con
80 binarios de test y cero fallos sobre esos bytes. La invariancia que importaba
se cumple: ningún schema publicado anterior cambió.

## P1-3 — recibos capturados sobre una imagen no admitida

**Aceptado.** Es cierto y lo sabía: las calibraciones se corrieron sobre
`e9ecc40d…` antes de que existiera la imagen final `0e21c561…`. Barrí el digest
en código y documentación pero no volví a capturar. Por la regla del propio
ADR-077, una medición sobre otra imagen es una declaración que el producto no
puede sostener.

Se corrige recapturando ambas calibraciones sobre la imagen admitida final, no
editando los recibos. Como la corrección del helper (P1 de la otra revisión)
cambia la imagen otra vez, la recaptura se hace sobre el digest definitivo, en
una sola pasada, junto con la requalificación nativa. La afirmación errónea de
`M5-01-blocker.json:53` se corrige en la misma pasada.

## P2-1 — dispersión degenerada

**Aceptado y en corrección.** El revisor tiene razón en el mecanismo y en por qué
importa: es exactamente la amenaza que el plan nombra —un benchmark que falsifica
su propia salida— y una dispersión observada de cero no es precisión infinita
sino ausencia de información sobre la dispersión. Se añade una razón nueva y se
devuelve `inconclusive`.

## P2-2 — «unknown permanece unknown» solo de un campo

**Aceptado.** El revisor detectó una contradicción interna de ADR-073: §3 dice
que cualquier campo de hardware no observable bloquea la comparación y §5
enumera un conjunto que solo bloquea por `cpu_model`; el código implementa §5.
`cpu_governor` es el caso más agudo, porque es permanentemente desconocido dentro
del contenedor y es el parámetro ambiental más capaz de fabricar una regresión.
`configuration_fingerprint` es el otro: está documentado como el digest de la
configuración congelada y no se consulta nunca.

Pendiente. La corrección correcta es ampliar el conjunto bloqueante y el enum de
razones, no relajar §3; hasta entonces §3 no debe leerse como una garantía.

## P2-3 — compare no lleva provenance

**Aceptado y pendiente.** Un llamador al que se le dice `["cpu_model"]` no puede
ver qué dos CPUs, y no puede obtenerlas de esta tool. El plan pide diferencias
visibles.

## P2-4 — la fixture `control` seguía afirmándose como control

**Aceptado y corregido.** El revisor tiene razón en que la retractación vivía en
un README mientras la fuente —el artefacto primario— seguía afirmando lo
contrario. Corregido en `lib.rs`, en la tabla y la prosa del README, en la matriz
y en el handoff, que antes no lo mencionaba en absoluto.

## P2-5 — ningún recibo lleva MDR, intervalo ni veredicto observado

**Aceptado y pendiente.** Es un criterio de aceptación del plan y no se cumple.
Se corrige en la recaptura de P1-3, registrando veredicto, intervalo y MDR
observados para los tres oráculos. Los ratios de host sin recibo que la matriz
cita se anotarán o se retirarán.

## P2-6 — Bonferroni y el número de remuestreos

**Aceptado y pendiente.** El análisis es correcto: desde familias de ~50 los
extremos del intervalo son estadísticos de orden extremos, y el comentario que
justifica los 10 000 remuestreos está redactado para el nivel sin ajustar.

## P3

Todos aceptados como observaciones correctas. Los de redacción —«paired» donde el
remuestreo es independiente, los `summary` constantes que afirman resultados que
no ocurrieron, `const: true` en los schemas, el orden de la puerta MDR, las
razones sin ordenar, el comentario que sitúa el decoder en el adapter— se
corrigen con el resto. Las discrepancias de estado entre matriz y handoff que el
revisor señala se corrigen en el cierre. La cita circular de ADR-076 sobre
cargo-bloat y el desacuerdo de tres formas sobre el mecanismo `--profile` se
anotan: el resultado observado está bien recibido, la explicación publicada no
coincide con lo que el recibo registra.

## Sobre lo verificado

Se conserva íntegro y es la parte más valiosa: la exactitud del CDF normal
inverso comprobada contra AS241 sobre 400 000 puntos, Bonferroni aplicado tanto
al intervalo como al MDR, el PRNG resistente al «seed grinding» probado con 200
nombres distintos, las vallas de Tukey sin descarte a posteriori, el orden de
`decide()` sin caídas incorrectas, el versionado del dataset cerrado en cuatro
puntos independientes, la separación exacto/estimado en `rust.binary.bloat`, la
ausencia de causalidad en cualquier cadena publicada, y la corrección de haber
rechazado ampliar el límite del vendor.
