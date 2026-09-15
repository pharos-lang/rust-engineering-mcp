# W20 — disposición del orquestador (2026-09-15)

Veredicto: **aceptado**. Verificado: `soak-m8.py` calibración 2 (30 ciclos)
`passed` (`fd_growth` 8 → 8, RSS estable); `test-m8-clients.py --preflight`
`status: ready`, `unsatisfied: []`, `pinned_versions` presente (Inspector
2.5.0, Codex 0.154.0, Claude Code 2.1.268, `agy` 1.2.2 — decisión del
orquestador: fijar la versión instalada y verificada). El primer fallo de
`fd_growth` queda explicado (reaperturas por ciclo con handles acotados por TTL,
no leak) y registrado en `05.md`.
