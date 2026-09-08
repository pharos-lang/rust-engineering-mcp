Revisión independiente READ-ONLY Claude Sonnet 5 Medium de los contratos y la composición de supply chain y quality gate v2. Lee exclusivamente `docs/reviews/m4-composition-contracts/inputs/` e `inputs.json`. Usa solo Read, Grep y Glob. No ejecutes comandos, no edites, no uses Docker, red, MCP ni subagentes. El snapshot es la única evidencia autorizada; no inspecciones otros archivos del checkout.

Revisa seguridad, correctness y cierre de contratos de `rust.supply_chain.inspect` y `rust.quality.gate.v2` end-to-end dentro del paquete. Verifica con referencias concretas de archivo y línea:

1. que cada composición use una sola captura candidata y exactamente una auditoría RustSec, comparta esa observación sin duplicarla ni recapturar, y revalide authority/owner antes y durante publicación;
2. que strict rechace baseline, release exija y capture/revalide un baseline separado, y que la etapa SemVer no confunda generaciones;
3. que fuentes ausentes, freshness stale/unknown, catálogo/vendor/policy/deny/audit parcial o unavailable, filas omitidas y evidencia truncada nunca produzcan pass ni borren hechos independientes ya acreditados;
4. que suppressions y policy se apliquen una sola vez, con identidad exacta, sin duplicar findings ni reparar una auditoría incompleta;
5. que status/verdict/issue de cada etapa y agregados sean conservadores ante fallos ordinarios, timeout, cancelación, cleanup incierto, publicación fallida y revalidación perdida;
6. que inputs/schemas sean cerrados, mantengan cardinalidades y límites de 512 KiB, 4096 paquetes, 128 filas/findings/lookups, timeout supply 1..120 default 120 y gate v2 1..3600 default 300; mutation es opt-in y su presupuesto derivado más 300 s debe caber en el timeout global;
7. que artifacts/resources queden ligados al owner/ProjectRef vivo, con publication durable y sin locators/source/raw prose;
8. que las pruebas incluidas realmente discriminen una captura/audit, orden de etapas, strict/release/baseline, unknown/partial/no-pass, duplicación de policy/suppression, schemas, cardinalidades, budgets y catálogo exact-version/freshness. Señala cualquier hueco de prueba material.

Contexto acotado: las 22 tools M1-M3 están congeladas y fuera de esta revisión. Las dos nuevas tools conservan `ADVERTISEMENT_READY=false`; no trates ese switch como bug ni afirmes que están publicadas. Main observó que un caso MCP strict pasó y que una fixture release con lock incompleto quedó bloqueada; una fixture positiva sigue en progreso. Son contexto, no evidencia incluida ni cierre. La revisión no comparte responsabilidad arquitectónica: el Technical Owner decidirá cada disposición.

Entrega en Markdown: veredicto acotado; cobertura; findings P0-P3 ordenados por severidad, cada uno con archivo/líneas, explicación, escenario discriminante y corrección mínima; invariantes que verificaste como correctos; limitaciones explícitas; pruebas faltantes; y una conclusión que diga expresamente que esta revisión por sí sola no abre gates ni acredita M4 Done. No sugieras nuevas dependencias salvo necesidad demostrada.
