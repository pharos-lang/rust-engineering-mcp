# W01 — disposición del orquestador (2026-09-14)

Veredicto: **aceptado con ampliación**. Censo completo (36/36 contrastado con
la sonda live del orquestador), 0 huérfanos, 2 Resources dinámicas, 15 comandos
CLI, 10 formatos en disco, 9 findings (4 P2, 5 P3, 0 P0/P1). Reconciliación de
`m2-m8.md` con hashes de merge verificados en `main`.

| ID | Sev | Hallazgo | Disposición |
| --- | --- | --- | --- |
| W01-D1 | P3 | F1 subestimado: además de `docs/tools.md:3-4`, README.md:21, docs/architecture.md:291/333, docs/client-configuration.md:316/418 y tools.md:138/1645 describen el checkout actual con 31 tools | Ampliado; a W03 |
| W01-D2 | P3 | Clase `preview` de las cinco tools M6 propuesta con el motivo «nunca publicadas en un tag»; ese motivo no es criterio de ADR-086 | Clase aceptada con motivo distinto (deuda que afecta al contrato); ver [01.md §2](../../01.md) |
| W01-D3 | P3 | Recomendación de «diseñar un contrato sucesor unificado» para quality.gate v1/v2 antes del freeze | Rechazada: sería tool nueva (fuera de M8); consolidación descartada con análisis en [01.md §3](../../01.md) |
| W01-D4 | — | 14 `permission_denials` (awk/mkdir/redirecciones a `/tmp`/bucles) | Sin impacto: el worker reformuló y completó; el allowlist se mantiene estrecho |
| W01-D5 | — | Scripts auxiliares en `target/m8-census/` (gitignored) | No forman parte de la evidencia; el censo cita el método en §1 |

Las decisiones de clase, consolidación y findings están en [01.md](../../01.md).
