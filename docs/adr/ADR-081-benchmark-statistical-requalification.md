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
