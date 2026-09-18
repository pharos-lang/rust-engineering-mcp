# V03b — re-revisión read-only de las correcciones V03 (W25/W26/W27/W27b/W28)

Modelo solicitado: Claude Opus 5 (`claude -p --model opus --effort medium --tools "Read,Grep,Glob"`). Rol: revisor read-only. Orquestador: Claude Fable 5.1.

Verifica contra `docs/validation/M8/delegation/V03-review-m8-03-04-05/disposition.md` que cada finding aceptado (D-1..D-6, R-1/R-2/R-4..R-7, S-1..S-4, P-1..P-9, C-1..C-7) queda cerrado tal como se dispuso, sin regresiones: snapshots de tools intactos, `contract --json` con la plantilla RFC 6570, `downgrade_blocked` por kind, controles positivos del rollback, validación de respuestas y 2-de-3 estricto, criterio `fd_after_ttl` tras TTL + apertura (expiración perezosa medida 27 → 8, ver `docs/validation/M8/05.md` §Hallazgo del soak), pins exigidos y oráculo de Codex por wire. Material: `git status`/diff inline y acceso Read/Grep/Glob al árbol. Salida: findings P0–P3 con evidencia y veredicto `Approve` / `Approve con findings` / `Block`. Breve.
