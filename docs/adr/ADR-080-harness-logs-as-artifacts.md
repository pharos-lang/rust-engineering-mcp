# ADR-080 — Los logs del harness se publican como artifacts

Fecha: 2026-09-09.

## Status

Accepted. Implementa una capacidad que ADR-076 §5, `docs/tools.md` y el schema
publicado de `rust.benchmark.run` ya prometían y que el servidor no cumplía.

## Context

Una revisión independiente encontró que tres textos publicados —la descripción
congelada en el schema, el documento de contratos y el ADR— dicen que los logs
del harness «quedan en el artifact de criterion». No quedan ahí ni en ninguna
parte: el payload de `criterion_archive` es un export USTAR de `CRITERION_HOME`
con 35 entradas de `benchmark.json`, `estimates.json`, `sample.json` y
`tukey.json`, y ningún log. El adapter captura `stdout` y `stderr` y los
descarta.

El caso que más duele es el que más se va a dar: cuando `rust.benchmark.run`
devuelve `OBSERVED_FAILURE`, el llamador recibe dos booleanos de truncación, dos
artifacts, y una indicación de buscar el error del compilador en un archivo que
no puede contenerlo. Con `harness_unrecognized` es peor: no se publica artifact
alguno.

Había dos salidas: publicar los logs, o corregir los tres textos para decir que
no se conservan. El owner elige la primera, porque la capacidad importa para
calidad y diagnóstico.

## Decision

### 1. `stdout` y `stderr` del harness se publican como artifacts privados

Sobre el store de ADR-061, con la misma disciplina que el resto: owner-bound,
TTL, cuota reservada antes del job, y sensibilidad al menos `SourceDerived`
—salen de compilar y ejecutar código del proyecto, así que pueden contener
fragmentos de fuente y rutas del guest.

### 2. Identificados por ejecución y por repetición

Un `rust.benchmark.run` con `run_count = 3` produce tres repeticiones, y sus logs
no se concatenan en uno. Cada artifact declara a qué repetición pertenece, con el
mismo `run_index` 1-based que llevan las muestras del dataset.

### 3. Acotados, con truncación explícita

Con su propio techo, declarado en el DTO. Un log que se cortó lo dice, y dice
cuántos bytes se conservaron; no se publica un log truncado como si fuera
completo. El techo se cuenta contra el presupuesto de artifacts de la operación.

### 4. Se corrige la asociación repetición ↔ archivo ↔ logs

Hoy la regla publicada dice que el árbol de criterion retenido es «la última
repetición que exportó uno, la misma cuyo exit y logs reporta la respuesta», y es
**falsa en un caso alcanzable**: el archivo se elige con `rfind` sobre las
repeticiones que exportaron algo, mientras el exit y los logs vienen de la última
repetición sin más. Si la tercera falla sin exportar y las dos primeras
exportaron, el archivo es de la segunda y los logs de la tercera, y nada en la
respuesta permite detectarlo.

Con los logs identificados por repetición esa ambigüedad desaparece, y el
`run_index` del archivo pasa a viajar también. La regla publicada se reescribe
para decir lo que el código hace.

### 5. Nunca al `stdout` del servidor MCP

`stdout` es el transporte del protocolo. Los logs del harness van al store, no al
canal.

### 6. Se prueba desde un cliente real

La recuperación se ejercita desde la matriz de clientes, no solo desde un test
del adapter: al menos un fallo de compilación observado y un harness no
reconocido, con el artifact leído de vuelta como Resource.

## Alternatives considered

- **Borrar la promesa de la documentación.** Es la salida barata y el owner la
  descarta: la trazabilidad del fallo es justamente lo que hace usable una tool
  que compila y ejecuta código ajeno.
- **Meterlos dentro del archivo de criterion.** Descartado: ese payload es un
  export literal de `CRITERION_HOME` y su valor es ser exactamente eso. Añadirle
  entradas que criterion nunca escribió lo convierte en una fabricación.
- **Devolverlos en la respuesta.** Descartado: el presupuesto de respuesta es de
  512 KiB y los logs de una compilación fallida lo agotan; además el contrato
  dice que las evidencias van al store y no dentro del resultado.

## Consequences

`rust.benchmark.run` pasa a publicar hasta tres clases de artifact: el dataset,
el árbol de criterion y los logs por repetición. El tope de miembros de la
respuesta sube en consecuencia, y ese cambio de contrato se refleja en el schema
publicado y en su snapshot. Como toca el camino de publicación, la calificación
de M5-01 —que ya estaba bloqueada por otra razón— y la matriz de clientes se
rehacen sobre bytes finales.
