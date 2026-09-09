# ADR-081 — Criterios congelados para recalificar el método de comparación

Fecha: 2026-09-09.

## Status

Accepted **como criterio**, no como resultado. Este documento se escribe
deliberadamente **antes** de correr una sola simulación nueva y antes de tocar el
método. Nada de lo que dice puede cambiarse después de ver qué parámetros
producen el resultado deseado; si un criterio resulta inalcanzable, se declara
inalcanzable y el veredicto direccional **no** se habilita.

## Context

ADR-073 corrigió la unidad de remuestreo —de la muestra a la ejecución— y con eso
la cobertura de un efecto cero real pasó de 0,29 a ~0,84 bajo el modelo de deriva
que usó la revisión independiente. Pero 0,84–0,89 con tres ejecuciones **no es**
el 0,95 que el método publica.

Documentarlo mejora la transparencia y no corrige la subcobertura. El mecanismo
es conocido y no depende del modelo: un bootstrap por conglomerados estimado
sobre `k` clústeres subestima el error estándar por `sqrt(k/(k−1))`, y los
extremos percentiles no llevan corrección `t_{k−1}`. Con `k = 3` eso es 1,22×.

El owner autoriza revisar el método y el número de ejecuciones, con revisión
estadística independiente. Este ADR congela contra qué se juzgará esa revisión.

## Decision

### 1. Criterios de aceptación, fijados antes de medir

Un método solo habilita veredictos direccionales (`regression`, `improvement`) y
`no_material_change` si cumple **todos**:

| Criterio | Umbral | Cómo se mide |
| --- | --- | --- |
| **Cobertura** del efecto verdadero por el intervalo publicado | ≥ 0,93 en todo el rango de deriva evaluado | Simulación, ≥ 10 000 réplicas por punto |
| **Falsos positivos direccionales** bajo nulo verdadero | ≤ 0,01 | La misma simulación, contando `regression`+`improvement` |
| **Potencia** para detectar un efecto igual al umbral material del 5 % | ≥ 0,80 | Simulación con efecto real +5 % |
| **`no_material_change` incorrecto** cuando el efecto real supera el umbral | ≤ 0,05 | Simulación con efecto real ±10 % |
| **Presupuesto** de una comparación | ≤ 30 s y ≤ 512 MiB, sin subir el techo de `compare` | Medición real, no estimación |

La cobertura pedida es 0,93 y no 0,95 a propósito: exigir exactamente el nominal
invita a elegir el estimador que lo alcanza por casualidad en el punto medido. Lo
que se exige es que el intervalo **no sea más confiado que lo que declara** por
más de dos puntos, en todo el rango, y que el nivel publicado sea el entregado o
se publique el entregado.

#### Corrección (2026-09-09) — la fila de potencia era imposible, y el error es mío

La primera simulación contra estos criterios encontró que **ningún candidato**
puede cumplirlos, y la razón no es el método: **las filas de cobertura y de
potencia de la tabla de arriba son incompatibles entre sí tal como las escribí**.

La prueba es de dos líneas y sale de la regla del propio producto. `regression`
se emite si y solo si el extremo inferior del intervalo supera el umbral:
`low > MATERIAL_THRESHOLD_RATIO`. Si el efecto verdadero es **exactamente** el
umbral, ese evento es idéntico a «el intervalo quedó entero por encima del valor
verdadero», que es un caso de no-cobertura. Por tanto, para cualquier intervalo:

```text
potencia(Δ = umbral) ≤ 1 − cobertura(Δ = umbral)
```

Exigir cobertura ≥ 0,93 obliga a potencia ≤ 0,07. Pedir 0,80 a la vez es pedir
un imposible aritmético, y la simulación lo confirma en los datos: la mejor
potencia observada en cualquiera de las 75 celdas es 0,0539, contra su propia
cota de no-cobertura en la misma celda.

El defecto está en el criterio, no en el estimador. Un criterio de potencia se
evalúa contra una alternativa **separada** del borde de decisión; medirla
justo en el umbral pregunta al método si resuelve el punto que él mismo declara
como el límite de lo material, y la respuesta correcta ahí es no resolverlo.

**Se corrige la fila, y se corrige antes de volver a puntuar a nadie.** Este
párrafo se commitea sin haber consultado todavía las cifras de potencia en la
alternativa nueva, por la misma razón que existía la congelación original.

| Criterio | Antes | Ahora |
| --- | --- | --- |
| Potencia | ≥ 0,80 con efecto real **igual** al umbral del 5 % | ≥ 0,80 con efecto real del **10 %**, el doble del umbral |

Todo lo demás de §1 y §4 queda igual. En particular el umbral material **sigue
siendo el 5 %**: lo que cambia es en qué alternativa se mide la potencia, no qué
se considera material. Si con la alternativa separada tampoco se alcanza 0,80,
se declara inalcanzable y los veredictos direccionales siguen deshabilitados, que
es la regla original y no se toca.

Dos lecturas alternativas de «detectar» que la simulación también midió —que el
intervalo excluya el cero, que es la convención del MDR de ADR-073, y que el
veredicto no sea `no_material_change`— **no** se adoptan como criterio. Ambas son
más laxas y elegir una después de ver los resultados sería exactamente lo que
este documento existe para impedir.

#### Corrección (2026-09-09, segunda) — qué exige de verdad emitir una dirección

Una revisión independiente externa —Gemini 3.8 vía `agy`, deliberadamente de otra
familia de modelo que quien escribió esto— devolvió **Block** con dos P1, y los
dos son correctos.

**La puerta estadística no existía en el código.** Este documento decía que los
veredictos direccionales quedan deshabilitados hasta pasar la recalificación, y
eso era una frase en Markdown. Lo único que impedía una dirección era que el
adapter fija `cpu_governor: None`, es decir **un accidente del entorno**, no un
control. Volver observable el entorno —trabajo que estaba planificado— habría
abierto la puerta en silencio con un estimador que reprueba su propia cobertura.
Se instrumenta como guarda explícita en `decide()`, detrás de una constante que
hoy vale `false`.

**Y la potencia sigue siendo inalcanzable, ahora por una razón distinta y sana.**
La puerta de precisión rechaza cuando `MDR > 5 %`, y

```text
MDR = 2,8016 · SE        SE ≈ τ · sqrt(2/k)
```

luego `MDR ≤ 0,05` exige `SE ≤ 0,01785`, es decir:

| Ejecuciones por lado | Deriva máxima admisible (τ) |
| --- | --- |
| k = 3 | **2,19 %** |
| k = 5 | 2,82 % |
| k = 10 | 3,99 % |
| k = 20 | 5,64 % |
| k = 60 | 9,78 % |

Este host mide entre ejecuciones del **mismo** código un recorrido de 6,1 % a
28,7 %. Así que con el protocolo actual ningún candidato alcanza 0,80 de potencia,
y **eso no es un defecto del estimador**: es la puerta de precisión funcionando.
Quitarla o subir el umbral sería exactamente el fraude que §4 prohíbe.

**Conclusión, conforme a la regla de cierre de este documento:** el criterio de
potencia se declara **inalcanzable en este entorno**, y los veredictos
direccionales y `no_material_change` **siguen deshabilitados**. No se elige otro
estimador para esquivarlo, porque ninguno lo esquiva.

Lo que sí queda fijado es el objetivo del trabajo de entorno, que deja de ser una
intención y pasa a ser un número: para decidir a un umbral del 5 % con tres
ejecuciones por lado hace falta **deriva entre ejecuciones por debajo del 2,2 %**,
con el governor observado y no supuesto. Un entorno que no llegue ahí no habilita
direcciones por mucho que mejore el estimador.

### 2. El rango de deriva está fijado aquí

`τ ∈ {0, 1, 2, 5, 10} %` de desviación típica entre ejecuciones, que es el rango
que cubre lo medido en este host (6,1 %–28,7 % de recorrido observado entre
ejecuciones del mismo código). Un método que solo cumpla en `τ = 0` no cumple.

### 3. Controles reales, no solo simulación

Además de la simulación, la recalificación exige, sobre capturas reales del guest
en la imagen admitida:

- **Positivos reproducibles** de `regression`, `improvement` y
  `no_material_change`, cada uno con su efecto real conocido por construcción.
- **Negativos** por ruido mayor que el efecto, por incompatibilidad y por datos
  insuficientes.
- `no_material_change` **exige evidencia suficiente**: no es la respuesta por
  defecto cuando no se alcanza a decidir; esa sigue siendo `inconclusive`.

### 4. Lo que no se toca para conseguir verde

- El **umbral material del 5 %** no se mueve.
- Los controles de seguridad, la contención y el aislamiento de red no se
  relajan.
- Los outliers se siguen contando con vallas de Tukey y **no** se eliminan.
- La semilla y su derivación siguen fijas y auditables.

### 5. Qué se autoriza cambiar

El número de ejecuciones por lado, el estimador del intervalo (por ejemplo BCa,
una corrección `t_{k−1}`, o un modelo de efectos aleatorios explícito), y el
número de remuestreos. Cualquiera de esos cambios sube la versión del método,
porque los intervalos de dos estimadores distintos no son comparables.

### 6. Revisión independiente obligatoria

La recalificación no se acepta sin una revisión estadística independiente que no
haya hecho el cambio, con acceso a las simulaciones y a los controles reales, y
con mandato explícito de comprobar que los criterios de §1 se fijaron antes y no
después. Este ADR, commiteado antes de la primera simulación, es la evidencia de
ese orden.

## Alternatives considered

- **Publicar la cobertura entregada y dejar el método como está.** Es lo que se
  hizo como medida inmediata y es honesto, pero el owner tiene razón en que no
  corrige la subcobertura: un intervalo más confiado de lo que declara sigue
  produciendo direcciones que la evidencia no sostiene.
- **Subir el número de ejecuciones hasta que la cobertura salga.** Descartado
  como criterio único: es exactamente elegir el parámetro después de ver el
  resultado. Subir `k` es una de las palancas autorizadas, pero se juzga contra
  los umbrales de §1 fijados aquí, junto con su coste en presupuesto.
- **Bajar el umbral material.** Descartado explícitamente por el owner.

## Consequences

Hasta que esta recalificación pase, `rust.benchmark.compare` **no habilita**
veredictos direccionales ni `no_material_change`, con independencia de que el
entorno pase a ser observable. Es decir: la puerta de hardware y la puerta
estadística son independientes y ambas deben abrirse.
