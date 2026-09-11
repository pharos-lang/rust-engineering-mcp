# Disposición del Technical Owner — revisiones externas M5

Fecha: 2026-09-09. Dos revisiones independientes, **ninguna de la familia de
modelo que escribió el código o los criterios revisados**. Las dos devolvieron
**Block**, y las dos encontraron defectos reales que las cuatro revisiones
anteriores —Claude revisando Claude— no habían encontrado.

Eso es, por sí solo, el argumento para haberlas hecho.

## codex — semántica de resultado de `rust.binary.bloat`

Veredicto: **Block**. [Texto íntegro](m5-bloat-semantics-codex/stdout.log).

**Primero, la pregunta incómoda, que se hizo explícita en el encargo:** el defecto
se encontró donde corregirlo pone una fila de cliente en verde, así que ¿la
implementación hace lo que el ADR dice, o lo que hacía falta para aprobar? El
revisor responde que **no encontró ninguna excepción diseñada para hacer pasar la
fixture de 634 funciones**, ni ninguna aserción protectora anterior debilitada, y
que eliminar la variante `Truncated` corresponde a eliminar una causa que ya no
pertenece a ese enum. Esa respuesta vale más que el veredicto.

| Hallazgo | Disposición |
| --- | --- |
| **P2 bloqueante** — el éxito ignora el exit de la vista por crates: la tool corre el analizador **dos veces** y `exit`, `exit_code` y `termination` salían solo de la primera, así que un fallo de la segunda con JSON válido desaparecía. Con ADR-079 ese caso llega a `passed` | **Aceptado y corregido.** Verificado en el código: `output.crates.code` no se leía en ninguna parte. Ahora el exit reportado es el de la primera ejecución que no terminó limpia, y solo es `Passed` si las dos lo fueron. Con la prueba que el revisor pidió: JSON válido, falla solo la segunda |
| **P2** — un error real de publicación descarta el tamaño ya medido, así que «el tamaño sobrevive todos los caminos» no es universal | **Aceptado.** El bloqueo es correcto; lo que falla es el alcance de la afirmación. Pendiente: o se conserva la medición en el error de publicación, o se acota la frase |
| **P3** — el oráculo de clientes no comprueba los hechos nuevos que incorpora su fixture | **Aceptado**, se corrige en la matriz de clientes final |

**Honestidad del revisor que conviene registrar:** dice explícitamente que el caso
bloqueante «se deduce del flujo; no afirmo haberlo reproducido nativamente». La
reproducción la aporta ahora la prueba nueva.

## agy — criterios estadísticos y su corrección

Veredicto: **Block**. [Texto íntegro](m5-statistical-criteria-agy/stdout.log).
El [primer intento](m5-statistical-criteria-agy/attempt-1-stderr.log) falló por
sintaxis del CLI y se conserva.

El encargo le pedía juzgar **mi** corrección, no el código: yo había escrito un
criterio de potencia imposible, y quien revisa una corrección no debe ser quien la
escribió.

| Hallazgo | Disposición |
| --- | --- |
| **H-01, P1** — la puerta estadística **no existe en el código**. ADR-081 decía que los veredictos direccionales quedan deshabilitados, pero lo único que los impedía era que el adapter fija `cpu_governor: None` **por accidente**. Volver observable el entorno —trabajo que yo tenía planificado— habría abierto la puerta en silencio con un estimador reprobado | **Aceptado, es el hallazgo más importante de las dos revisiones.** Se instrumenta como guarda explícita en `decide()`, detrás de una constante que hoy vale `false` |
| **H-02, P1** — la potencia corregida sigue siendo inalcanzable, ahora porque la puerta de precisión rechaza legítimamente bajo deriva real | **Aceptado y verificado.** `MDR = 2,8016·SE`, `SE ≈ τ·sqrt(2/k)`, luego resolver el 5 % exige τ ≤ 2,19 % con k=3. Este host mide 6,1 %–28,7 %. Declarado **inalcanzable en este entorno** conforme a la regla de cierre del propio ADR |
| **H-03, P2** — cobertura y potencia se simularon solo con familia de uno; N=25 solo se midió para tiempo y memoria | **Aceptado, pendiente.** Bonferroni ensancha el intervalo, así que familia de uno es el caso más exigente para cobertura y el más laxo para potencia; medirlo con familia grande es deuda real |
| **H-04, P3** — la deriva se modela solo como gaussiana simétrica, sin colas pesadas ni asimetría térmica | **Aceptado como observación.** El recibo ya declara el modelo y qué conclusiones dependen de él |

Sobre el juicio central que se le pidió: confirma que el argumento de
imposibilidad del criterio original es **«100 % exacto e irrebatible»**, y no
objeta que moverlo a una alternativa separada sea relajarlo. Que lo diga quien no
escribió la corrección es el punto.

## Lo que cambia en el plan

El trabajo de «entorno observable» que iba a hacer a continuación **no puede
aterrizar antes** que la guarda de H-01. Se invierte el orden.

Y «calificar un entorno capaz de decidir» deja de ser una intención: el objetivo
es **deriva entre ejecuciones por debajo del 2,2 %** con el governor observado.
Un entorno que no llegue ahí no habilita direcciones por bueno que sea el método.
