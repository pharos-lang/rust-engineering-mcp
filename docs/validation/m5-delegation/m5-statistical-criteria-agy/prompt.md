# Revisión independiente de los criterios estadísticos M5 (G8)

Eres un revisor **independiente y de solo lectura**. Usa **únicamente tu
herramienta de lectura de archivos**: no ejecutes comandos, no accedas a la red,
no escribas nada. Todo lo que necesitas está en el disco local.

## Contexto

El repositorio es `/Users/cburgosro/Projects/rust-mcp`, rama `ai/m5-performance`.
La tool `rust.benchmark.compare` compara dos datasets de benchmark y emite
`regression`, `improvement`, `no_material_change` o `inconclusive`.

Una revisión independiente anterior encontró que el intervalo de confianza
publicado medía la dispersión equivocada, y se corrigió: la unidad de remuestreo
pasó de la muestra a la ejecución (bootstrap por conglomerados). Pero quedó un
residuo: con `k = 3` ejecuciones, la cobertura entregada es ~0,84–0,89 frente al
0,95 nominal que el método publica.

El owner autorizó recalificar el método, y **congeló los criterios de aceptación
antes de medir** para que no se eligieran los parámetros después de ver qué
resultado daban.

## Qué debes leer

- `docs/adr/ADR-081-benchmark-statistical-requalification.md` — los criterios
  congelados, **incluida** su sección `#### Corrección (2026-09-09)`.
- `docs/adr/ADR-073-benchmark-method-and-dataset.md` §4 y sus correcciones
  fechadas — el método congelado.
- `crates/domain/src/benchmark_compare.rs` — la implementación real: en
  particular las constantes, `cluster_draw`, `bootstrap_ratio` y `decide`.
- `docs/validation/M5-02-method-simulation.json` — el recibo de la simulación
  (es grande; léelo por partes si hace falta).

## Qué tienes que juzgar

1. **El criterio de potencia original era imposible.** Se pedía potencia ≥ 0,80
   con un efecto real *igual* al umbral material, junto a cobertura ≥ 0,93.
   Como `regression` se emite si y solo si el extremo inferior del intervalo
   supera el umbral, con efecto verdadero igual al umbral ese evento es
   no-cobertura de un lado, luego `potencia ≤ 1 − cobertura`. **Verifica ese
   argumento tú mismo contra el código**, y di si es correcto o si se te escapa
   algo.
2. La corrección mueve la potencia a medirse con un efecto real del 10 %, el
   doble del umbral. **¿Es una corrección legítima o es relajar el criterio para
   poder aprobar?** Este es el punto más importante de tu revisión. El owner ya
   cometió un error escribiendo el criterio; puede haber cometido otro
   corrigiéndolo.
3. Los umbrales que quedan —cobertura ≥ 0,93, falsos positivos ≤ 0,01,
   `no_material_change` incorrecto ≤ 0,05, presupuesto ≤ 30 s y 512 MiB— ¿son
   defendibles, o hay alguno elegido para que algo pase?
4. El rango de deriva fijado (`τ ∈ {0,1,2,5,10} %`) ¿cubre lo que el host
   realmente muestra? El recibo y `fixtures/benchmark-datasets/README.md` tienen
   las cifras observadas.
5. ¿Hay algún criterio que falte y que un método de medición debería tener que
   cumplir antes de emitir veredictos direccionales?
6. El ADR dice que si un criterio resulta inalcanzable, se declara inalcanzable y
   los veredictos direccionales **no** se habilitan. ¿Está esa regla escrita de
   forma que no se pueda esquivar?

## Cómo revisar

Prefiere evidencia sobre prosa. Di claramente cuando algo esté bien. Una revisión
que fabrica hallazgos para parecer rigurosa es peor que inútil.

Clasifica P0/P1/P2/P3 y da **un** veredicto: **Block** o **Pass**.

## Entrega

Devuelve la revisión completa como tu último mensaje. No escribas archivos.
