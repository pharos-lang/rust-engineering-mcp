# W24 — disposición del orquestador (2026-09-15)

Veredicto: **aceptado**. Ensayo local `status: passed` (13 pasos): archive
core 0.8.0 construido con `release-artifact.py`, inventario/SBOM/notices,
smoke desde directorio limpio (SHA-256, `version --json` 0.8.0, `contract
--json` 36 tools con hashes iguales al manifiesto, `doctor`, `tools/list` 36,
`resources/templates/list` 2). **Defecto real hallado por el ensayo**: dos
hashes mal fijados en `TOOL_SCHEMA_SHA256` de `release-smoke.py`
(`binary.bloat`, `analyzer.action.apply`) — el verificador falló cerrado y se
corrigió; el contrato no estaba mal. Workflow RC: literal `31` → `tool_count`
del manifiesto (job `build` → output → `draft`), sin cambiar OIDC/attestations.
ADR-090 (D14) Accepted; `publication.md` con verificación offline y respuesta a
compromiso; backlog D14 Accepted. Pendientes para RC1 (sin red/tag): attestations
OIDC, redescarga desde GitHub, cadena tag/run/digest — `unavailable` honesto en
el recibo. El recibo se regenera sobre bytes commiteados (`tree_dirty: false`)
junto con los demás (V03 R-3).
