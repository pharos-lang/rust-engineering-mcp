# W21 — disposición del orquestador (2026-09-15)

Veredicto: **aceptado, pendiente de V03**. Causa raíz confirmada con el schema
del SDK TS (2026-07-28 exige `ttlMs`/`cacheScope` en los cuatro listados;
rmcp `Default` los omite). Producto: `list_resources`/`list_resource_templates`/
`list_prompts` con `ttlMs 0` + `cacheScope private`; dos plantillas desde las
constantes de `resources.rs`; `mimeType` solo donde es uniforme (decisión
razonada, aceptada). Verificado por el orquestador: snapshots de tools
intactos (solo `doctor-report.json` de W14), `contract-freeze verify --strict`
passed, `test-m8-clients-unit.py` 66/66. Recibo Docker-free
`docs/validation/M8/clients.json` (`attempt-9`): **Inspector `passed`
(contract equality 36/36, 31 negativos, 4 genéricos, `resources/list` vacío)
y Codex `passed`** (obligatorios); Claude Code y Gemini `unavailable` por
causas del arnés/entorno (pin 2.1.267 en el validador compartido de M6;
`agy` sin credenciales por `HOME` aislado) → W23. Cuatro defectos del arnés
corregidos con causa raíz citada (incluido `rust.catalog.status` como
observación, no refusal: expectativa incorrecta del plan, no del producto).
