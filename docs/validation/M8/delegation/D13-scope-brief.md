# D13 — alcance cross-platform 1.0: brief para decisión del owner (pendiente)

Preparado por el orquestador el 2026-09-14 (M8-01). D13 exige «alcance antes
de M8-02, resolución en M8-07» ([backlog](../../../roadmap/adr-backlog-m2-m8.md#d13--calificación-por-target-para-10),
[plan M8](../../../roadmap/m8-stabilization.md) §Performance, operación y distribución).
**No se decide aquí**: el plan reserva al owner la opción de limitar 1.0 a macOS,
y esa opción exige aclaración explícita de spec §61/§97 y un ADR antes de readiness.

## Estado verificado

| Familia | Host positivo | Artifact | CI |
| --- | --- | --- | --- |
| macOS ARM64 (26/APFS + Docker Linux ARM64 gateway) | Sí, M0–M6 calificados nativamente ([ADR-048](../../../adr/ADR-048-0.1.0-qualification-and-artifact-boundary.md), gates `full`) | `v0.1.0`, `v0.3.0` core archive | portable + full local |
| Linux x86_64 | No (sin adapter no-follow/reparse-safe ni oráculos nativos) | Ninguno | portable (fmt/check/clippy/test/doctests/architecture) + SonarCloud coverage |
| Windows x86_64 | No | Ninguno | **Retirado** el 2026-09-13 (regresión stdio pre-`initialize` de M6, `docs/ci.md`) |
| Linux ARM64 / macOS x86_64 | No | Ninguno | Ninguna |

## Opciones

| Opción | Qué implica en M8 | Coste/riesgo |
| --- | --- | --- |
| **A. 1.0 limitada a macOS ARM64** (host positivo + artifact), Linux/Windows = «portable, no calificado» | ADR nuevo + nota en spec §61/§97 (aclaración de alcance, no reescritura); README/compatibility/SECURITY sin promesas de otras familias; M8-07 califica un target; restaurar Windows en CI solo como portabilidad si se corrige la regresión (deuda) | Alcance honesto y alcanzable dentro de M8; contradice la aspiración de spec §61 (tres familias) y hay que decirlo explícitamente |
| **B. 1.0 con Linux x86_64 positivo** además de macOS | Subprograma D13: adapter de filesystem no-follow (`openat2`/`RESOLVE_*`), sandbox nativo (o gateway Docker en host Linux), oráculos nativos G4, host Linux real para el gate; artifact Linux con smoke desde descarga limpia | XL; requiere hardware/host Linux y sesiones adicionales; M8-07/09 se alargan |
| **C. Tres familias positivas** | B + Windows (reparse-safe, junctions, sandbox) + corregir la regresión stdio | No alcanzable sin hosts y sin decisiones adicionales; no recomendado para 1.0 |

## Recomendación del orquestador

**A**, con la cláusula de honestidad del plan: la aclaración de spec/ADR se hace
en M8-02 (antes del freeze) y el checklist 1.0 «cross-platform» se marca como
«resuelto por cambio de alcance aprobado», nunca como cumplido. Windows/Linux
siguen como CI de portabilidad; el CI de Windows se restaura si la regresión se
corrige dentro de M8 (deuda D13, no criterio 1.0).

## Qué necesita el orquestador del owner

Una frase en sesión: «D13 = A» (o B/C) y, si A, autorización para que un worker
redacte el ADR de alcance y la nota de spec §61/§97.
