# M5 — handoff

Fecha: 2026-09-10. Rama `ai/m5-performance`; base `main`
`c6099f27415b0be3838e84d21d25eed903c8c312`.

**Done local.** La [matriz M5](M5-matrix.md) es el estado por corte: suite
nativa 6/6, matriz de clientes, `core` 23/23 y `full` 38/38 sobre el lock con
`lancedb 0.31.0` y el workspace `0.3.0` (`45d339f`, fuentes de `34bd428`). El owner autorizó completar M5 con commits locales; no autorizó push,
PR, merge, tag, release ni M6. Las subidas de dependencias ya incorporadas en
`a3cb48c` se conservan sin separarlas ni modificarlas.

## Contratos y cambios integrados

Las decisiones ADR-073..081 gobiernan las cuatro tools de rendimiento y sus
límites. `rust.benchmark.run` consume el vendor como snapshot pequeño o captura
ADR-078. La captura real se ingiere con el volumen de 512 MiB/32768 inodos,
con creación, fingerprint y cleanup coherentes. El replay revalida el descriptor
y autentica el digest antes de entregar su último bloque. Ningún límite de
SourceBundle fue ampliado.

El gateway observa el governor dentro del guest por argv cerrado. Si no hay
observación completa y uniforme, publica desconocido; no infiere el governor
físico. La guarda `METHOD_QUALIFIED_FOR_DIRECTION=false`, el umbral material del
5 % y los criterios de precisión/potencia permanecen intactos. Una comparación
puede terminar correctamente y seguir siendo `inconclusive`.

`rust.binary.bloat` sigue ADR-079: un ranking limitado no convierte por sí solo
un análisis válido en fallo. Tamaño exacto, atribución estimada, filas omitidas y
recorte de respuesta permanecen diferenciados. Cuando falla la segunda vista,
los logs y el report describen esa misma ejecución.

Los logs de benchmarks se publican por repetición como artifacts privados. Su
límite se aplica al UTF-8 final, incluso si bytes inválidos se expanden al
reemplazarlos. Se declaran truncación y reemplazo por separado.

## Validación del candidato final

1. Pruebas focalizadas de captura y performance: **hechas** (12, 68 y 11 tests).
2. Seis selecciones nativas ignoradas, una por vez con `--exact --ignored
   --test-threads=1`: **6/6 aprobadas** ([M5-native-gate.json](M5-native-gate.json)).
3. **PASS** ([M5-clients.json](M5-clients.json), attempt-6).
   `scripts/test-m5-clients.py --run --with-runtime`: Inspector 2.5.0 como
   cliente determinista —dos benchmarks, comparación con IDs reales y lectura de
   todos los artifacts— y Claude Code 2.1.267 (`claude-sonnet-5`, restringido al
   servidor configurado y a las tools de Resources) como cliente agentic. El
   owner retiró Codex de este cierre el 2026-09-10 tras agotarse su cuota. El
   turno runtime dirigido por modelo debe abrir el proyecto, descubrir
   Resources, medir dos veces por sí mismo, comparar en positivo, obtener el
   rechazo `NOT_A_DATASET` con un artifact propio de otro tipo y leerlo como
   Resource; el turno docker-free debe obtener los cuatro rechazos declarados
   con los argumentos del plan.
4. `scripts/gate.py core`: **PASS** ([M5-core-gate.json](M5-core-gate.json)).
   `scripts/gate.py full`: **PASS** 38/38 ([M5-full-gate.json](M5-full-gate.json)),
   con `m5-runtime` dentro del conjunto. El primer `full`, sobre el lock con
   `lancedb 0.38.0`, falló en `semantic` y motivó la opción 2a
   ([intento 2](m5-gate-attempts/README.md)).
5. Disposición final G1–G9: [Done local](m5-delegation/closure-local-semantics/g1-g9-disposition.md);
   tablero sincronizado.

## Dependencias

`lancedb` vuelve a `=0.31.0` / Lance 8 según ADR-027 (decisión del owner,
opción 2a); se conservan `fastembed 6.0.3`, `jsonschema 0.55.1` y `tokio-rustls
0.26.5`. La actualización general de paquetería queda como
[tarea post-M8](../roadmap/m8-stabilization.md#tarea-post-m8--actualización-de-paquetería-decisión-del-owner-2026-09-10).

El worktree de medición es `/private/tmp/rust-mcp-m5-closure`. La imagen
admitida sigue siendo
`sha256:e0a5ca1661b3e49d0a3d68ee3cc0963453078d08eb7fc43c30538c16b7998aac`;
no se reconstruyó. La captura provisionada está en
`fixtures/criterion-vendor/capture/` y es un input local, no una descarga runtime.

## Revisión y riesgos restantes

Las revisiones [vendor](m5-delegation/closure-local-vendor/review.md) y
[semántica](m5-delegation/closure-local-semantics/review.md) detectaron y
revisaron los fixes de replay, normalización de logs y diagnóstico bloat. No
quedan P0–P2 de esos paquetes. Dos P3 quedan trazados en los informes.
La auditoría G1–G9 recuperó el P2 histórico de paridad `verify_applied`;
su corrección reutiliza `rust_applied` y requiere pruebas, re-review y full.

Claude Sonnet 5 respondió sin autenticación y el reintento no produjo informe;
Opus 5 no se invocó. Sol ejecutó el fallback autorizado. No se afirma revisión
Claude ni agotamiento de cuota. Sus [intentos](m5-delegation/closure-sonnet5-semantics/attempts.md)
se conservan.

El alcance positivo es macOS ARM64 con guest Linux ARM64. La observación del
guest no acredita control del hardware físico. Los datos estadísticos siguen
sin habilitar direcciones. Los límites de vendor cercanos al techo son política,
no una calificación de todos los extremos posibles. Las muestras provienen del
harness del proyecto, que puede falsearlas.

## Historia e integración

Los [recibos anteriores](m5-closure-history/inventory.json) se preservan sin
editar. Los gates nuevos deberán apuntar a los bytes exactos que midieron;
ningún recibo histórico acredita código posterior. El control de que Criterion
no cabe en el snapshot pequeño sigue siendo correcto para ese contrato y no
sustituye el positivo de la captura separada.

La integración remota y su smoke quedan pendientes de autorización; no forman
parte de una publicación implícita al terminar el gate local. Detener antes de M6.
