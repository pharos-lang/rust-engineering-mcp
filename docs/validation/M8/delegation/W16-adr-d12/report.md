# W16 — informe

Worker W16 (Claude Sonnet 5, `--effort medium`), rol de documentación, rama
`ai/m8-stabilization`. Sin subagentes, sin segundo plano, sin commit.

## Archivos escritos

- `docs/adr/ADR-088-migration-rollback-policy.md` (nuevo): materializa los 8
  puntos de D12 (`docs/validation/M8/03.md` §D12) sin reinterpretarlos —
  Decision 1–8, Alternatives (CLI de migración genérica, bloqueo de `serve`
  con journal pendiente, bloqueo en RustSec stale) y Consequences (incluye
  los puntos 6 y 7: prueba con dos binarios/`03-rollback.json` y el
  procedimiento de backup). `Date: 2026-09-14`, `Status: Accepted`.
- `docs/adr/README.md`: una viñeta nueva para ADR-088, mismo formato que las
  entradas de ADR-086/ADR-087.
- `docs/roadmap/adr-backlog-m2-m8.md`: §D12 `Status` pasa de `Proposed` a
  `Accepted`, con enlace a ADR-088 y a la evidencia (`03.md` §D12,
  `03-formats-analysis.md`). El resto de la sección D12 no se tocó.
- `docs/compatibility.md`: nueva sección «Upgrade, rollback y backup» (antes
  de «Gateway y capabilities M0-05/06») con la tabla de los 10 formatos
  (marcador de versión, fail-closed, cambio desde 0.3.0, recuperación),
  la corrección de `floor_or_trust_state` para RustSec/vendor tree, el
  procedimiento de rollback de binario y el procedimiento de backup/restore.
- `README.md`: nueva sección «Operación: backup, restore y rollback» (20
  líneas de contenido, ≤ 25) antes de «Seguridad», con el procedimiento
  apoyado en `doctor` y un enlace a la sección completa de
  `docs/compatibility.md`.
- `docs/validation/M8/01-census.json`: en `disk_formats[]`, el snapshot
  RustSec y el vendor tree de Cargo pasan `floor_or_trust_state` de `true` a
  `false`, cada uno con un campo `floor_or_trust_state_note` nuevo que
  explica la distinción (pin de integridad puntual vs. floor de secuencia
  persistido) y remite al único floor genuino del censo (bundle de
  confianza del catálogo). Ningún otro campo de esas dos entradas ni de
  ninguna otra se modificó.
- `CHANGELOG.md`: una viñeta nueva en la sección `0.8.0`, junto a la de
  ADR-087, resumiendo ADR-088.

No se tocó ningún archivo fuera de esta lista (en particular, no se tocó
`docs/tools.md` ni el código de `doctor`: esos quedan en W14).

## Verificación

- `python3 -B scripts/docs-hygiene.py links-check`: **0 broken en living
  documents** (2688 enlaces resueltos; 5 apuntan a evidencia excluida por
  `.gitignore`, sin relación con este cambio; 459 broken en frozen records,
  preexistente y sin relación con este cambio).
- `python3 -c "import json;json.load(open('docs/validation/M8/01-census.json'))"`:
  carga sin error.

## Notas para la revisión (V03)

- El recibo `docs/validation/M8/03-rollback.json` (W15) todavía no existe;
  se referencia como texto plano (`docs/validation/M8/03-rollback.json`, W15)
  en `docs/compatibility.md` y en ADR-088, nunca como enlace Markdown, para
  no romper `links-check` antes de que W15 lo produzca.
- La sección `mutation_journals` de `doctor` (W14) tampoco existe todavía en
  el código; se documenta en ADR-088/compatibility.md/README como el
  comportamiento decidido por D12 punto 3, consistente con cómo ADR-086
  documenta decisiones antes/junto con su implementación. Si W14 cambia el
  nombre de la sección o el comportamiento exacto al implementarla, esta
  documentación necesitará una pasada de sincronía.
