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

**Aceptado y corregido.** Es cierto y lo sabía: las calibraciones se corrieron sobre
`e9ecc40d…` antes de que existiera la imagen final `0e21c561…`. Barrí el digest
en código y documentación pero no volví a capturar. Por la regla del propio
ADR-077, una medición sobre otra imagen es una declaración que el producto no
puede sostener.

Se corrigió recapturando sobre la imagen admitida, no editando recibos. La
corrección del helper (P1 de la otra revisión) cambió la imagen otra vez, así que
la recaptura se hizo sobre el digest definitivo `sha256:e0a5ca16…`, junto con la
requalificación nativa completa. La calibración de benchmark se rehízo con seis
capturas —tres ejecuciones por lado, que el método v2 necesita— y con un script
que se niega a medir sobre cualquier otra imagen, que es lo que evita que el
recibo vuelva a derivar. La afirmación errónea de `M5-01-blocker.json` se
corrigió en la misma pasada.

## P2-1 — dispersión degenerada

**Aceptado y en corrección.** El revisor tiene razón en el mecanismo y en por qué
importa: es exactamente la amenaza que el plan nombra —un benchmark que falsifica
su propia salida— y una dispersión observada de cero no es precisión infinita
sino ausencia de información sobre la dispersión. Se añade una razón nueva y se
devuelve `inconclusive`.

## P2-2 — «unknown permanece unknown» solo de un campo

**Aceptado y corregido.** El revisor detectó una contradicción interna de
ADR-073: §3 dice que cualquier campo de hardware no observable bloquea la
comparación y §5 enumeraba un conjunto que solo bloqueaba por `cpu_model`; el
código implementaba §5. `cpu_governor` es el caso más agudo, porque es
permanentemente desconocido dentro del contenedor y es el parámetro ambiental más
capaz de fabricar una regresión. `configuration_fingerprint` es el otro: estaba
documentado como el digest de la configuración congelada y no se consultaba
nunca.

Se amplió el conjunto bloqueante, no se relajó §3, y se separaron tres
situaciones que antes se confundían: conocido y distinto es incompatible;
observado en un solo lado es incompatible; y la misma ceguera en los dos lados
deja los datasets comparables pero no admite dirección
(`inconclusive` / `unobservable_hardware`). Las dos secciones del ADR ya dicen lo
mismo.

**Lo que cuesta, dicho aquí y publicado en `docs/tools.md`:** dentro del
contenedor el governor no es legible nunca, así que **ninguna comparación de este
runtime emite dirección alguna**. Es la consecuencia correcta del hallazgo del
revisor, no un efecto colateral que convenga esconder.

## P2-3 — compare no lleva provenance

**Aceptado y corregido.** Un llamador al que se le decía `["cpu_model"]` no podía
ver qué dos CPUs, y no había otra tool a la que pedírselas. La respuesta lleva
ahora las dos provenances comparadas —exactamente los campos que la comprobación
de compatibilidad consulta— dentro del mismo presupuesto de 512 KiB.

## P2-4 — la fixture `control` seguía afirmándose como control

**Aceptado y corregido.** El revisor tiene razón en que la retractación vivía en
un README mientras la fuente —el artefacto primario— seguía afirmando lo
contrario. Corregido en `lib.rs`, en la tabla y la prosa del README, en la matriz
y en el handoff, que antes no lo mencionaba en absoluto.

## P2-5 — ningún recibo lleva MDR, intervalo ni veredicto observado

**Aceptado y corregido donde importa.** La recaptura sobre la imagen admitida
publica las seis capturas con sus medianas y la deriva entre ejecuciones, y el
oráculo `criterion_dataset::admitted_image_datasets` fija veredicto, efecto y MDR
observados sobre datos reales: efecto +24,4 % para el único benchmark cuya fuente
cambia, MDR por encima del umbral, veredicto retenido. Los ratios de host sin
recibo se retiraron de la matriz.

## P2-6 — Bonferroni y el número de remuestreos

**Aceptado y corregido.** El análisis es correcto: desde familias de ~50 los
extremos del intervalo son estadísticos de orden extremos, y el comentario que
justificaba los 10 000 remuestreos estaba redactado para el nivel sin ajustar.

De las tres salidas —subir los remuestreos con la familia, acotar la familia, o
declarar el límite y negarse más allá— se eligió la tercera, porque es la única
que no cambia lo que el método afirma cuando sí afirma algo.
`MAX_RESOLVABLE_FAMILY_SIZE = 25` se deriva de los remuestreos, la confianza y un
mínimo de diez sorteos en la cola, y un test recomputa esa derivación. Una
familia mayor describe las dos medidas pero no corre bootstrap, no afirma
intervalo y no admite dirección.

## P3

Todos aceptados como observaciones correctas.

**Corrección de este documento (2026-09-09).** La versión anterior de esta
sección decía que los P3 de redacción «se corrigen con el resto». Una re-revisión
independiente los comprobó uno a uno contra el árbol y encontró que la mayoría
seguían abiertos: «paired» seguía en el comentario del bootstrap, los `summary`
constantes seguían igual, `const: true` seguía sin estar en los schemas, las
razones seguían sin ordenar, el comentario del decoder seguía situándolo en el
adapter, y la retractación del control `control` había llegado a `lib.rs` pero
**no** a la tabla ni a la prosa de su README. También encontró que «los ratios de
host se retiraron de la matriz» era falso: siguen impresos, anotados.

Esa frase era el error que más importa de todo este documento. Una disposición
que declara cerrado lo que sigue abierto convierte a G8 en un trámite: el
siguiente revisor confía en ella y deja de mirar. Lo que se corrige ahora se dice
como corregido; lo que sigue abierto se nombra:

- **Corregidos ahora**: la tabla y la prosa del README de la fixture sobre
  `control`; los identificadores `v1` que quedaron sueltos en
  `quality_artifact.rs`, ADR-076 y el handoff; la atribución errónea de las
  capturas en `M5-01-blocker.json`; el rango de deriva publicado como «15–29 %»,
  que era solo el lado baseline cuando el recibo registra 6,1–28,7 % en los dos;
  y los ratios de host, que ahora dicen que son del host y no acreditan el
  runtime, en vez de afirmarse retirados.
- **Siguen abiertos y se nombran como tales**: el comentario «paired», los
  `summary` constantes, `const: true`, el orden de las razones, el comentario del
  decoder, `provenance.run_index` vestigial, y la regla publicada sobre qué
  repetición es el tar, que es falsa en un caso alcanzable. Ninguno falsea una
  medición; todos son deuda de publicación y están registrados en la matriz.

La cita circular de ADR-076 sobre cargo-bloat y el desacuerdo sobre el mecanismo
`--profile` **sí** se corrigieron: la explicación publicada dice ahora que está
inferida de la fuente del analizador y no medida, y el recibo registra el fallo
observado.

## Sobre lo verificado

Se conserva íntegro y es la parte más valiosa: la exactitud del CDF normal
inverso comprobada contra AS241 sobre 400 000 puntos, Bonferroni aplicado tanto
al intervalo como al MDR, el PRNG resistente al «seed grinding» probado con 200
nombres distintos, las vallas de Tukey sin descarte a posteriori, el orden de
`decide()` sin caídas incorrectas, el versionado del dataset cerrado en cuatro
puntos independientes, la separación exacto/estimado en `rust.binary.bloat`, la
ausencia de causalidad en cualquier cadena publicada, y la corrección de haber
rechazado ampliar el límite del vendor.
