# V01 — revisión independiente read-only del censo M8-01 y de sus decisiones

Modelo solicitado: Claude Sonnet 5 (`claude -p --model sonnet --effort high --tools "" --restricted`). Rol: revisor read-only (sin Edit/Write/Bash). Orquestador: Claude Fable 5.1. El material a revisar viene inline a continuación del encabezado (archivos completos y `git diff`); no puedes abrir otros archivos: si necesitas uno que no está, dilo como limitación.

## Qué revisar

1. `docs/validation/M8/01.md` (decisiones del orquestador): ¿la clasificación `stable`(31)/`preview`(5) respeta ADR-086 §1? ¿Los motivos de `preview` de las cinco `rust.analyzer.*` son verificables? ¿Las seis consolidaciones descartadas están justificadas con análisis de compatibilidad real (G1: trece M1 congeladas; G2: grants; ADR-086 §9)? ¿Se descarta algo que el plan M8 exigiría aceptar, o se acepta algo prohibido?
2. `docs/validation/M8/01-census.md` (W01): coherencia interna (conteos, tablas, §3 gate de superficie, §9 findings), afirmaciones sin evidencia, y si «huérfanos = 0» está demostrado o solo declarado.
3. `docs/adr/ADR-086-deprecation-and-freeze-policy.md`: contradicciones con spec §53–58 o ADR-012; ambigüedades que un cliente exhaustivo no podría aplicar.
4. Diff W03 (`--help` con 36, conteos «31»→36, índice ADR, línea de estado del roadmap): ¿se reescribió algún hecho histórico? ¿queda algún conteo obsoleto en el texto mostrado? ¿el paréntesis del `--help` sigue siendo exacto?

## Salida

Findings por archivo con severidad P0–P3 (P0/P1 bloquean; P2 de contrato/gate bloquea readiness), evidencia (cita corta) y acción propuesta; veredicto `Approve` / `Approve con findings` / `Block`. Sin implementación. Sé preciso y breve.
