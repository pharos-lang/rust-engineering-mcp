# W07 — D13 = A: ADR-087 de alcance de hosts 1.0 y aclaración de spec

Modelo solicitado: Claude Sonnet 5 (`claude -p --model sonnet --effort medium`). Rol: worker de documentación. Orquestador: Claude Fable 5.1. No puedes lanzar subagentes. **Nunca corras comandos en segundo plano.** No hagas commit. **Archivos permitidos:** `docs/adr/ADR-087-1.0-host-scope.md` (nuevo), `docs/adr/README.md` (una viñeta), `docs/roadmap/adr-backlog-m2-m8.md` (solo §D13 Status), `docs/spec/rust-engineering-mcp-propuesta-v0.3.md` (solo añadir una nota en §61 y una en §97 «1.0.0»), `README.md` (sección de plataformas/soporte), `docs/compatibility.md` (fila «Target de validación local»/«Linux / macOS x86_64 nativos» y la que corresponda), `docs/ci.md` (párrafo sobre Windows retirado: pasa a «deuda de portabilidad, no criterio 1.0»).

## Decisión del owner (2026-09-14, en sesión: «Aprobado A»)

Fuente: `docs/validation/M8/delegation/D13-scope-brief.md` (opción A) y `docs/validation/M8/02.md` decisión 8. Contenido normativo de ADR-087:
- **1.0 se califica y publica para un único host positivo: macOS ARM64 (macOS 26/APFS) con el gateway Docker Linux ARM64** para ejecución de código de proyecto; artifact 1.0 = core `aarch64-apple-darwin` (misma frontera que ADR-048, ahora para 1.0).
- **Linux x86_64 y Windows x86_64 = portabilidad de fuente/protocolo/fail-closed en CI, no calificados**: sin adapters no-follow/reparse-safe, sin oráculos nativos G4, sin artifacts; el CI de Windows, retirado el 2026-09-13 por la regresión stdio pre-`initialize`, se restaura si se corrige dentro de M8 y cuenta como **deuda de portabilidad, no como criterio 1.0**. Linux ARM64 y macOS x86_64 no se anuncian.
- El criterio «cross-platform» del checklist 1.0 (`docs/roadmap/m8-stabilization.md`) se cierra como **«resuelto por cambio de alcance aprobado por el owner»**, nunca como cumplido; una familia adicional solo entra por subprograma D13 con adapter + oráculos nativos + host real + ADR nuevo.
- Alternatives considered: B (Linux positivo en M8), C (tres familias); motivos de descarte del brief. Consequences: README/compatibility/SECURITY/ci coherentes; `docs/publication.md` y ADR-047/048 no cambian; M8-07 califica un solo target; la aspiración de spec §61 (cinco triples) se mantiene como aspiración explícitamente no cumplida en 1.0.
- `Status: Accepted` (owner, 2026-09-14). Sources: brief D13, ADR-048, spec §61/§97, `docs/ci.md`.

## Tareas

1. Escribe ADR-087 con la estructura de la casa (mira ADR-085/ADR-086). Índice en `docs/adr/README.md`. Backlog §D13: `Status: Accepted, ADR-087 (enlace relativo), 2026-09-14`.
2. Spec: en §61 («GitHub Releases») añade **al final** un párrafo corto: «Aclaración de alcance 1.0 (ADR-087, 2026-09-14): la matriz anterior sigue siendo aspiracional; 1.0 califica y publica únicamente macOS ARM64; Linux/Windows conservan CI de portabilidad sin capabilities positivas ni artifacts.» En §97 «1.0.0», junto al criterio `cross-platform`, añade una nota entre paréntesis o un párrafo tras la lista con la misma aclaración y el enlace relativo al ADR. No reescribas nada más de la spec.
3. README: localiza la sección de plataformas/soporte y deja una afirmación única y actual (macOS ARM64 positivo; Linux/Windows portable no calificado; Windows CI retirado temporalmente) enlazando ADR-087 y `docs/compatibility.md`. `docs/compatibility.md`: actualiza las filas de targets para reflejar ADR-087 (sin borrar hechos de 0.1.0/0.3.0). `docs/ci.md`: en el párrafo de Windows retirado, añade que es deuda de portabilidad y no criterio 1.0 (ADR-087).

## Verificación (foreground)

`python3 -B scripts/docs-hygiene.py links-check` → 0 rotos en documentos vivos. Informe: Task / Result / Files changed (líneas) / Texto exacto añadido a la spec / Risks / Open issues. No commit.
