# W02 — ADR-086: política de deprecación y freeze (D11)

Modelo solicitado: Claude Sonnet 5 (`claude -p --model sonnet --effort medium`). Rol: worker de documentación (ADR). Orquestador: Claude Fable 5.1. No puedes lanzar subagentes. **Nunca corras comandos en segundo plano.** **Solo puedes editar**: `docs/adr/ADR-086-deprecation-and-freeze-policy.md` (nuevo), `docs/adr/README.md` (índice), `docs/roadmap/adr-backlog-m2-m8.md` (solo la sección D11), `docs/compatibility.md` (una sección nueva «Política de deprecación y freeze (0.8 → 1.0)»). No hagas commit.

## Fuente de la decisión

`docs/validation/M8/delegation/D11-decision-brief.md` (decisión ya tomada por el orquestador; no la reinterpretes ni la amplíes). Fuentes citables: spec `docs/spec/rust-engineering-mcp-propuesta-v0.3.md` §53–59 y §116.1; `docs/adr/ADR-012-semver-compatibility.md` (se conserva **sin cambios**; ADR-086 lo complementa); `docs/roadmap/m8-stabilization.md` §«Contratos y política propuesta de deprecación»; `docs/roadmap/adr-backlog-m2-m8.md` §D11.

## Tareas

1. Escribe `ADR-086-deprecation-and-freeze-policy.md` con la estructura de la casa (mira `docs/adr/ADR-085-m6-runtime-admission.md`): `Date: 2026-09-14`, `Context`, `Decision` (los 9 puntos del brief, redactados como norma), `Alternatives considered` (compatibilidad estricta; opt-in versionado por tool; ruptura libre en 0.8; política por meses), `Consequences` (incluye: M8-01 asigna clase con evidencia; M8-02 congela; dos RC con contrato idéntico; ADR-012 intacto), `Status: Accepted`, `Sources` (spec y ADR citados con rutas relativas).
2. Añade ADR-086 al índice `docs/adr/README.md` siguiendo el formato de las entradas existentes.
3. En `docs/roadmap/adr-backlog-m2-m8.md` §D11: `Status: Accepted` → enlace a ADR-086 y fecha 2026-09-14; conserva el resto de la sección.
4. En `docs/compatibility.md` añade una sección corta (≤ 25 líneas) «Política de deprecación y freeze (0.8 → 1.0)» que resuma los puntos 1, 2, 4, 5 y 8 del brief y enlace al ADR-086. No cambies otras secciones.

## Verificación (foreground)

`python3 -B scripts/docs-hygiene.py links-check` con 0 rotos en documentos vivos. Informe final: Task / Result / Files changed / Evidence (salida del links-check) / Risks / Open issues. No commit.
