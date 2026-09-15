# R01 — disposición del orquestador (2026-09-14)

Veredicto del auditor: **Approve con findings** (F10–F13). Verificación del
orquestador: F10 y F11 reproducidos contra los snapshots (`project-open`,
`analyzer-*`: enums `Code` presentes y en `SCREAMING_SNAKE_CASE`; `ApplyCode`
no es `snake_case`); F12 reproducido (el recibo M3 `attempt-11/protocol.jsonl`
solo invoca `project.open` y `test.nextest`); F13 reproducido
(`client-configuration.md:406` y `compatibility.md:8` describen M6 como no
integrado). Los cuatro commits y las 36 rutas/annotations confirmados por el
auditor coinciden con la verificación propia del registro §1 y de `01.md` §1.

| ID | Sev (auditor) | Disposición | Dónde se cierra |
| --- | --- | --- | --- |
| F10 | P1 | Aceptado (P2 para el orquestador: defecto del censo, no del producto; bloquea el freeze porque el censo es la base de M8-02) | W01b: `error_codes[]` de las 5 tools desde los snapshots |
| F11 | P2 | Aceptado: 5 tools M2 en `snake_case`, 31 en `SCREAMING_SNAKE_CASE` (`analyzer.action.apply` incluida) | W01b (census) + corrección de `01.md` §4 por el orquestador |
| F12 | P1 | Aceptado (P2 para el orquestador): `rust.coverage`, `rust.semver.check`, `rust.mutation.test` solo tienen e2e nativo como consumidor; siguen `stable` por publicación en `v0.3.0` + evidencia nativa G4/G5, y **M8-04 debe ejercitarlas con Inspector y Codex stock** | W01b (census §2/§clasificación) + `01.md` §2 por el orquestador; matriz M8-04 |
| F13 | P2 | Aceptado: docs públicas con M6 «en desarrollo/no integrado» | W01b (`client-configuration.md` §M6, `compatibility.md:8`) |

Nota: el auditor cita `01.md:16-17,23` y `01-census.md:107` por número de
línea; el orquestador corrigió `01.md` directamente.
