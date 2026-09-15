# W04 — informe del worker (Claude Sonnet 5, Medium; claude 2.1.268)

Invocación: `claude -p --model sonnet --effort medium --permission-mode acceptEdits --disallowedTools Agent Task --allowedTools <lista> --no-session-persistence --output-format json < prompt-header.md`. Inicio 2026-09-14T14:50:34Z, fin 2026-09-14T14:53:34Z, exit 0, 51 turnos, 178965 ms, permission_denials: 2.

---

Clean, only the intended sentence. Everything verified. Here's the final report.

## Informe final

**Task:** W04 — correcciones V01 sobre ADR-086 §1, censo M8-01, y docs públicas (M6 `preview`), según disposición del orquestador.

**Result:** Las 6 tareas aplicadas exactamente según la disposición, sin reinterpretar. Ambas verificaciones en foreground pasan.

**Files changed (líneas):**
- `docs/adr/ADR-086-deprecation-and-freeze-policy.md` (nuevo, 150 líneas): añadida línea `Amended: 2026-09-14 (V01 F-A/F-D)`; punto 1 reescrito con 4ª clase `experimental`, definición de «consumidor real» (stock o e2e nativo, no unitario) y la obligación M8-04/RC1; nueva viñeta en `Consequences` sobre las 3 tools M3.
- `docs/compatibility.md` (+38/-1): párrafo de política ampliado a 4 clases + definición de consumidor real (+6 líneas netas sobre el original); nuevo párrafo de 5 líneas clasificando las 5 `rust.analyzer.*` como `preview` con la deuda conocida y las 31 restantes `stable`.
- `docs/client-configuration.md` (+1 párrafo, 5 líneas): nota de clase `preview`/deuda conocida tras el primer párrafo de «Configurar las tools M6».
- `README.md` (+3/-2): línea 26 corregida (M6 integrado en `main` vía PR #20/`e50c3fe`, calificado, clase `preview`, sin tag/release).
- `docs/validation/M8/01-census.json` (nuevo, 3183 líneas): `stock_client_invoked` por tool (33 `true`, 3 `false`: `rust.coverage`, `rust.semver.check`, `rust.mutation.test`) + top-level `stock_client_coverage`.
- `docs/validation/M8/01-census.md` (nuevo, 355 líneas): columna «Cliente stock» en la tabla §2 (3 `no`), nota junto a «36/36 real_consumers[]» aclarando los 3 casos; nota metodológica en §3.2 sobre la asimetría numerador/denominador.

**Evidencia:**
- `python3 -c "...stock_client_invoked..."` → `33`.
- `python3 -B scripts/docs-hygiene.py links-check` → `0 broken in living documents` (459 rotos son en "frozen records", fuera de alcance).
- Clasificación stock vs. native derivada directamente de `real_consumers[]` contra los patrones dados (`clients*.json`, `protocol.jsonl`, `17-inspector.md`, `17-codex-model.md`, `17-matrix.md` vs. `*_runtime.rs`/`*_native*.rs`): salió exactamente el split 33/3 esperado, sin forzar nada.
- `docs/validation/M6/matrix.md` §«Deuda de M6» existe y respalda la cita usada en `compatibility.md`/`client-configuration.md`.

**Risks:** Ninguno nuevo introducido; los cambios son de texto/documentación. El diff de `client-configuration.md` mostrado por `git diff` incluye ~30 líneas previas al alcance de W04 (de W01b/F13, ya en el working tree antes de esta sesión) — mi edición es solo el párrafo de clase `preview` insertado tras el bloque de código.

**Open issues:** El disposition indica que, tras W04, el material cambia de forma material (ADR-086 §1) y corresponde una re-revisión V01b (Sonnet, read-only) del diff antes de cualquier commit — no hice commit, según instrucción.
