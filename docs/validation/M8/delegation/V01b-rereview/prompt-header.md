# V01b — re-revisión read-only del diff W04 (cambios materiales tras el Block de V01)

Modelo solicitado: Claude Sonnet 5 (`claude -p --model sonnet --effort medium --tools ""`). Rol: revisor read-only sin tools; material inline. Orquestador: Claude Fable 5.1.

Revisa únicamente el `git diff` de W04 (ADR-086 §1 enmendado, `docs/compatibility.md`, `docs/client-configuration.md`, `README.md`, `01-census.md`) y el `01.md` final, contra la disposición V01 (inline): ¿cada finding aceptado (F-A, F-D, F-G, F-M, F-C, F-E, F-F, F-H) queda cerrado tal como se dispuso? ¿La enmienda de ADR-086 §1 contradice spec §57/§58 o ADR-012? ¿Se introdujo algún hecho nuevo sin evidencia o se reescribió texto histórico? Salida: findings P0–P3 con evidencia y veredicto `Approve` / `Approve con findings` / `Block`. Breve.
