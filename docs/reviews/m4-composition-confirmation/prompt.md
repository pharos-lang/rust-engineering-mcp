# Confirmación independiente de contratos de composición M4

Actúa como reviewer externo senior. Esta es una revisión estática y read-only sobre
el snapshot cerrado bajo `inputs/`; `inputs.json` enumera todos los archivos y sus
SHA-256. No leas archivos fuera de `inputs/`. No ejecutes código, comandos, tests,
red, Docker ni subagentes. No escribas ni edites archivos.

Lee primero
`inputs/docs/reviews/m4-composition-contracts/review.md`, que contiene los findings
de la revisión anterior. Después contrasta cada finding P1/P2/P3 con la fuente y
las pruebas actuales del snapshot. El objetivo es confirmar o rechazar su
disposición, no rediseñar la arquitectura ni abrir un gate.

Revisa específicamente:

1. Si `compose_security` aplica policy exactamente una vez a cada finding lógico,
   preserva la disposición en `deny.findings`, y si `SecurityFinding::apply_policy`
   es una asignación determinista e idempotente para los mismos `policy` y `now`.
2. Si las nuevas pruebas de aplicación cubren inputs deny no configurados,
   `packages > 4096`, truncación de 257 paquetes y mutación clean/failure/blocked.
3. Si los tests runtime de `supply_chain::encode_result` y
   `quality_v2::encode_result` parten de reportes completos que pasan su validador,
   demuestran que la representación MCP completa previa excede 512 KiB, y que el
   resultado recortado queda dentro de 512 KiB, con omisiones visibles,
   `complete=false` y sin `Passed`. Comprueba que el fixture supply incluye audit y
   evidencia de catálogo fresh reales; identifica cualquier artificio que reduzca
   el valor discriminante del test.
4. Si `deny_json` acepta sólo códigos de regla del conjunto cerrado, valida pero
   descarta `message`, labels y notes del guest, y emite únicamente el texto host
   fijo `cargo-deny reported rule '{rule}'`. Distingue los códigos cerrados de
   cualquier prosa libre.
5. Si existe comparación directa `deny.lock_fingerprint ==
   graph.lock_fingerprint`, incluso cuando audit no contiene lock fingerprint.
6. Si `SupplyReport::validate` ya impide declarar `complete=true` sin audit/deny
   completos, evidencia de catálogo fresh/aging y hechos de features/yanked
   conocidos.

No trates la revisión como evidencia de ejecución ni como autorización de M4 Done.
Las pruebas se ejecutan fuera del reviewer. Reporta evidencia por archivo y líneas
del snapshot. Devuelve Markdown con estas secciones exactas:

- `# Confirmación independiente — composición M4`
- `## Veredicto acotado`
- `## Disposición de findings previos` (tabla P1/P2/P3: estado, evidencia, razón)
- `## Findings actuales` (P0–P3; escribe `Ninguno` cuando corresponda)
- `## Evidencia verificada`
- `## Riesgos y backlog`
- `## Limitaciones`

No propongas cambios fuera de findings concretos. La decisión final corresponde al
Technical Owner.
