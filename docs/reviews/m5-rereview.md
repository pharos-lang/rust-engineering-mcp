# Re-revisiones independientes G8 — resumen y disposición

Fecha: 2026-09-09. Rama `ai/m5-performance`, commit revisado
`875c8902e737b28cc432ebe5f886de0bd41b0503`. Dos revisores independientes,
read-only, esfuerzo alto, uno por eje. Ninguno hizo los cambios que revisó.

G8 exige re-revisar los cambios materiales. Tras la primera ronda cambiaron: el
modelo de varianza de la comparación, el containment del profiling, el digest de
la imagen, la partición del corte M5-01 y la publicación del árbol de criterion.
Los textos íntegros están en
[containment](m5-security/findings-rereview.md) y
[método](m5-method/findings-rereview.md).

## Veredictos

| Eje | Veredicto | P0 | P1 | P2 | P3 |
| --- | --- | --- | --- | --- | --- |
| Containment y capability de profiling | **Pass** | 0 | 0 | 2 | 6 |
| Método de medición y contratos públicos | **Pass** | 0 | 0 | 6 | 11 |

Ninguno de los dos encontró algo que fabricara una medición, corrompiera datos
en reposo, escapara del containment o produjera un veredicto falso a partir de
bytes commiteados.

## Lo que los revisores verificaron en vez de creer

Vale la pena registrarlo porque es la parte que distingue una re-revisión de una
segunda lectura:

- **La imagen admitida se construyó de estas fuentes.** El revisor de containment
  volvió a ejecutar `fixtures/rust-runtime/m5/provision.py` y reprodujo el digest
  del contexto de build, `471f90954dab0f6d…`, byte a byte contra
  `M5-provisioning.json`, sobre 46 archivos y 10 796 061 bytes. Es lo más cerca
  que se puede estar de verificar la imagen sin Docker.
- **El ataque, desde el lado del hijo.** Pre-crear un artifact, los dos, symlink,
  hard-link, directorio, unlink-manteniendo-fd, doble fork, plantar durante el
  build, matar al helper: cada camino termina en un rechazo, no en una
  publicación.
- **El defecto anterior, redemostrado sobre las capturas nuevas.** Con la unidad
  de remuestreo v1, `m5/slower_125` —fuente idéntica en los dos lados— todavía
  produce una **regresión falsa** (IC `+0,0528..+0,1363`, MDR `0,0390 ≤ 0,05`);
  con v2, `inconclusive`. La cobertura de un efecto cero real pasa de 0,29 a 0,84.
- **Nada congelado se movió.** Remuestreos, semilla, derivación de semilla, nivel
  de confianza, umbral, suelo muestral, potencia y las dos constantes z son
  idénticos byte a byte al árbol previo a la corrección.
- **La semilla sigue resistiendo el «grinding»** bajo el sampler nuevo: 200
  nombres sobre el corpus real y 15 configuraciones sintéticas en el borde de
  decisión, 3 000 ejecuciones, **ningún** veredicto cambiado.
- **Las seis capturas sostienen lo que se afirma de ellas.** Digests contra el
  recibo, medianas recomputadas desde `times`/`iters`, y —lo que cierra la
  limitación que la primera revisión se puso a sí misma— el guest hasheó su
  propio `/source` antes de cada ejecución: `Cargo.lock`, `Cargo.toml` y
  `benches/perf.rs` idénticos en las seis, `src/lib.rs` distinto solo entre lados.
- **El oráculo del bloqueo M5-01 no blanquea nada.** Recomputa los tres
  predicados contra las constantes reales y falla en cuanto cualquiera deje de
  romperse; el recibo registra `blocked`.

## Findings y disposición

| Eje | Finding | Disposición |
| --- | --- | --- |
| Containment | P2 — el código del vaciado no tiene tests, ningún gate lo compila en macOS y ningún recibo lo había observado reaping | **Corregido**: workload que hace doble fork, selección que observa `descendants_reaped: 1` y `namespace_drained: true`, selección determinista de artifact pre-creado, y un paso de gate que clippya el helper para el target del guest |
| Containment | P2 — `verify_applied` sigue siendo un subconjunto de `rust_applied` | **Aceptado, diferido**; el revisor concluye que el vaciado lo hace *menos* urgente, porque `Init`/`PidMode` se detectan midiendo el pid en vez de comparando configuración |
| Containment | P3 — ADR-074 afirmaba paridad con ADR-064; la disposición dijo que se anotaría y no se anotó | **Corregido** en el ADR y en el modelo de seguridad |
| Containment | P3 — `docs/security-model.md` dos revisiones por detrás | **Corregido** |
| Containment | P3 — el handoff publicaba 192 muestras, número del smoke sobre la imagen retirada | **Corregido**: 194 de 195, del recibo generado |
| Containment | P3 — el schema publicado admitía un nombre con `-` inicial que el dominio rechaza | **Corregido** en los tres schemas y en el validador de benchmark |
| Containment | P3 — seis contadores del manifest deserializados sin invariante alguno | **Corregido** el caso imposible (`frames_unresolved > frames_total`) |
| Containment | P3 — un test arma el switch de denegación sin guarda RAII | **Abierto**, higiene de test; no puede convertir un fallo en un pase |
| Método | P2-1 — la cobertura entregada no es el `confidence_level` publicado | **Corregido**: el gate exige las tres ejecuciones del protocolo, lo que elimina la fila `k = 2`; el residuo en `k = 3` se **declara** en la constante, el ADR, el documento de contratos y el schema |
| Método | P2-2 — schema, docs y ADR dicen que los logs viven en el archivo de criterion, y no viven en ninguna parte | **Abierto** |
| Método | P2-3 — en este runtime `inconclusive` es el **único** veredicto emitido, no solo la ausencia de dirección | **Abierto** |
| Método | P2-4 — `M5-01-blocker.json` atribuía las capturas de una ejecución por lado a la imagen calificada | **Corregido** |
| Método | P2-5 — el informe no publicaba cuántas ejecuciones agrupó cada lado | **Corregido**: viajan por comparación |
| Método | P2-6 — la disposición declaraba cerrado lo que seguía abierto, en diez puntos | **Corregido**, y es el finding que más importa: una disposición en la que el siguiente revisor confía es cómo G8 deja de funcionar |
| Método | P3 ×11 | Parcialmente corregidos; los que siguen abiertos están **nombrados** en la matriz como deuda de publicación |

## Lo que queda abierto, dicho como abierto

Ninguno falsea una medición ni publica bytes no autenticados; por eso ninguno
bloquea. Están listados en [la matriz](../validation/M5-matrix.md):

1. `verify_applied` como subconjunto de `rust_applied`.
2. Los logs del harness: o se publican, o las tres afirmaciones que dicen dónde
   viven se corrigen.
3. Que `inconclusive` sea el único veredicto alcanzable en este runtime, no solo
   la ausencia de dirección.
4. La deuda de redacción: «paired», los `summary` constantes, `const: true`,
   razones sin ordenar, el comentario del decoder, `run_index` vestigial, y la
   regla publicada sobre qué repetición es el tar.
5. El vocabulario de resultado de `rust.binary.bloat`, con su propio motivo
   registrado para no corregirlo bajo el incentivo equivocado.
