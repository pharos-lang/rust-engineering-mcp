# W02 — disposición del orquestador (2026-09-14)

Veredicto: **aceptado**. ADR-086 materializa los nueve puntos del brief D11 sin
reinterpretarlos; índice ADR, backlog D11 (Accepted) y sección de
`docs/compatibility.md` (23 líneas) coherentes con el brief. `links-check`: los
dos rotos reportados son los enlaces anticipados de `matrix.md` al censo W01
(en curso), no del paquete W02.

| ID | Sev | Hallazgo del worker | Disposición |
| --- | --- | --- | --- |
| W02-R1 | P3 | El brief usa la clase `internal`, ausente en spec §57 (`stable/preview/experimental`) | Aceptado como divergencia terminológica deliberada: `internal` = elemento existente que **no se anuncia** (ni `tools/list` ni docs públicas), mientras que `experimental` de §57 es una tool anunciada con opt-in por namespace. Hoy ninguna tool se anuncia como experimental; si el censo M8-01 necesita esa clase, se admite `experimental` con el namespace de §57 sin cambiar ADR-086. Se deja constancia aquí; no bloquea |
