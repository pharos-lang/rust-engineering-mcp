# R01 — auditoría de trazabilidad del censo M8-01 (Gemini 3.8 Flash High, read-only)

Eres un auditor independiente read-only en el repositorio `/Users/cburgosro/Projects/rust-mcp` (rama `ai/m8-stabilization`). No edites archivos. No uses red. Responde en español.

## Objeto

Audita `docs/validation/M8/01-census.json` y `docs/validation/M8/01-census.md` (censo de 36 tools MCP, Resources, CLI, modelo de errores y formatos en disco) y las decisiones del orquestador en `docs/validation/M8/01.md`, buscando **contradicciones spec → ADR → código → tests → docs/DoD**. Fuentes normativas: `docs/spec/rust-engineering-mcp-propuesta-v0.3.md` (§9, §20, §53–59, §78, §85, §116.1), `docs/adr/ADR-012-semver-compatibility.md`, `docs/adr/ADR-086-deprecation-and-freeze-policy.md`, `docs/roadmap/m8-stabilization.md` (M8-01 y «gate de superficie»), `docs/roadmap/m2-m8.md` §G1–G9, `AGENTS.md`.

## Comprobaciones obligatorias (muestrea al menos 12 tools de distintos milestones, incluidas las trece M1 y las cinco `rust.analyzer.*`)

1. Para cada tool muestreada: ¿existen `adapter_source`, cada `adr[]`, `tests.contract_snapshot` y cada `tests.protocol_tests[]`/`native_cuts[]` citados? ¿El snapshot `crates/mcp-server/tests/snapshots/<x>-tool.json` declara las `annotations` que el censo afirma? ¿Los `error_codes[]` del censo coinciden con el enum `Code` del snapshot?
2. ¿Cada `real_consumers[]` apunta a un recibo existente y la fila/ID citada realmente contiene una invocación de esa tool? (abre el recibo y comprueba).
3. ¿La clasificación `stable`/`preview` de `01.md` §2 respeta ADR-086 §1 (consumidor real + test + docs para `stable`)? ¿Hay alguna tool `stable` sin consumidor real verificable?
4. ¿Las consolidaciones descartadas en `01.md` §3 contradicen algo de spec §9/§20/§116.1 o del plan M8 («no borrar contratos usados», «~35 tools»)?
5. Verifica los 9 findings F1–F9: ¿son reales (cita línea/archivo)? ¿Falta algún finding evidente (p. ej., otro documento público con conteo de tools obsoleto, un ADR citado que no existe, un test citado que no existe)?
6. Verifica que los cuatro commits citados en `01-census.md` §10 existen (`git log -1 <hash>`) y son los merges de los PR indicados.

## Salida

Lista de findings con `id`, severidad `P0|P1|P2|P3`, archivo:línea, evidencia textual (cita corta), y acción propuesta; después un veredicto global (`Approve` / `Approve con findings` / `Block`) y la lista de tools muestreadas. **No inventes rutas, hashes ni contenidos**: si no puedes abrir un archivo, dilo. Sé conciso.
