# W02 — informe del worker (Claude Sonnet 5, Medium; claude 2.1.268)

Invocación: `claude -p --model sonnet --effort medium --permission-mode acceptEdits --disallowedTools Agent Task --allowedTools <lista> --no-session-persistence --output-format json < prompt-header.md`. Inicio 2026-09-14T13:44:33Z, fin 2026-09-14T13:46:19Z, exit 0, 28 turnos, 104768 ms, modelos ['claude-haiku-4-5-20251001', 'claude-sonnet-5'], permission_denials: 2 (dos Bash fuera del allowlist: `awk … | wc -l` y `git status --short`; ninguna afectó al resultado).

---

No commit made, as instructed.

## Task
Materializar ADR-086 (D11 — política de deprecación y freeze 0.8→1.0) a partir de la decisión ya tomada en el decision-brief del orquestador, y propagarla al índice de ADRs, al backlog D11 y a `docs/compatibility.md`.

## Result
- Creado `docs/adr/ADR-086-deprecation-and-freeze-policy.md`: los 9 puntos del brief redactados como norma (Decision), 4 alternativas descartadas con su motivo, Consequences (M8-01 asigna clase con evidencia, M8-02 congela, dos RC con contrato idéntico, ADR-012 intacto), Status Accepted, Sources con rutas relativas a spec §53–59/§116.1, ADR-012, roadmap M8 y backlog D11.
- `docs/adr/README.md`: añadida la entrada ADR-086 al final del índice, mismo formato que las entradas M6/M5 existentes.
- `docs/roadmap/adr-backlog-m2-m8.md` §D11: `Status` cambiado de `Proposed` a `Accepted, ADR-086 (enlace relativo), 2026-09-14`; el resto de la sección (Context/Decision propuesta/Alternatives/Consequences/Fecha límite/Evidencia) queda intacto.
- `docs/compatibility.md`: nueva sección «Política de deprecación y freeze (0.8 → 1.0)» (23 líneas), resume clases de estabilidad con evidencia (punto 1), regla 0.x (punto 2), freeze en 0.8.0 y retiro en 1.0 (punto 4), regla desde 1.0 (punto 5) y el mecanismo visible sin `version` por tool (punto 8), enlazando a ADR-086.

## Files changed
- `docs/adr/ADR-086-deprecation-and-freeze-policy.md` (nuevo)
- `docs/adr/README.md`
- `docs/roadmap/adr-backlog-m2-m8.md`
- `docs/compatibility.md`

## Evidence
`python3 -B scripts/docs-hygiene.py links-check`:
```
links-check: 2506 links resolved; 2 broken in living documents; 5 point at evidence excluded by .gitignore; 459 broken in frozen records
  BROKEN docs/validation/M8/matrix.md:35 -> 01-census.md
  BROKEN docs/validation/M8/matrix.md:35 -> 01-census.json
```
Los 2 rotos en documentos vivos son preexistentes (censo M8-01 aún no escrito) y no fueron tocados en esta tarea; ninguno de los enlaces que añadí (ADR-086, brief D11, spec, ADR-012) aparece en la lista de rotos.

## Risks
- El brief usa la clase `internal`, que no existe literalmente en spec §57 (`stable/preview/experimental`); lo señalé en el ADR citando §57 sin forzar equivalencia, pero es una divergencia terminológica entre spec y decisión que M8-01/M8-02 deberán resolver al aplicar la clase.

## Open issues
- Ninguno bloqueante para W02. No se hizo commit, según instrucción.
