# W22 — informe

Worker W22 (Claude Opus 5, High), rol de seguridad (documentación normativa;
solo lectura del código), rama `ai/m8-stabilization`. Sin subagentes, sin
segundo plano, sin commit.

## Task

M8-08: threat model completo de 0.8.0/1.0 sobre macOS ARM64 con gateway Docker
Linux ARM64 y registro de riesgos residuales (ADR-089), con la sección
«Riesgos residuales 1.0» en el security model público.

## Result

- Threat model con las 7 tablas pedidas: actores y activos; 8 fronteras;
  amenazas/controles/oráculo/residual por frontera; los cinco temas del plan;
  permisos, retención y borrado; capability ↔ oráculo; riesgos `RR-n`.
- ADR-089 `Accepted`: registra RR-01…RR-18 con severidad, alcance, mitigación,
  aceptación y condición de reevaluación. Incluye los puntos obligatorios:
  - (a) RR-01: la auditoría es una revisión de modelo, no humana ni pentest.
  - (b) RR-02: Linux y Windows no calificados.
  - (c) RR-03: deuda M6.
  - (d) RR-04: RustSec stale degrada sin bloquear.
  - (e) RR-05…RR-18.

  También fija la re-review obligatoria en M8-08 y las alternativas
  (auditoría humana; no publicar 1.0).
- Cada control cita `archivo:línea` o un test verificado en el árbol; los
  controles sin oráculo se declaran así.

## Files changed

- `docs/validation/M8/08-threat-model.md` (nuevo).
- `docs/adr/ADR-089-residual-risk-register.md` (nuevo).
- `docs/adr/README.md`: una viñeta para ADR-089.
- `docs/security-model.md`:
  - Sección nueva «Riesgos residuales 1.0» con RR-01…RR-18, enlazada al ADR y
    al threat model.
  - Correcciones de hechos desactualizados, cada una con cita:
    - M3-01/nextest ya no está bloqueado (ADR-064, `nextest_runtime.rs`,
      matriz M3), en dos lugares.
    - 31 → 36 tools (ADR-086).
    - M5-03 está Done local (matriz M5), no «en recalificación».
    - M6 ya no tiene la calificación nativa pendiente: gate `full`, matriz M6,
      en dos lugares.
- `SECURITY.md`, solo donde contradecía el estado actual:
  - «Estado actual» describía únicamente M0; se añade el estado 0.8.0 con
    enlaces.
  - «No existe todavía una versión binaria soportada» contradecía la release
    `v0.1.0` publicada (`docs/publication.md`).
- `docs/validation/M8/delegation/W22-threat-model/report.md` (este informe).

## Conteo

| Métrica | Valor |
| --- | --- |
| Fronteras | 8 (B1–B8) |
| Controles evaluados | 53 |
| — con oráculo nativo | 31 |
| — con oráculo unit/contract/protocol | 12 |
| — con evidencia histórica/configuración | 4 |
| — sin oráculo | 6 |
| Riesgos residuales | 18 (RR-01…RR-18) |
| Capabilities positivas del security model sin oráculo | 0 |

## Verificación

- `python3 -B scripts/docs-hygiene.py links-check`: 2810 enlaces resueltos,
  **0 rotos en documentos vivos**. Además, 5 apuntan a evidencia excluida por
  `.gitignore` y 459 están rotos en registros congelados; ambas cifras son
  preexistentes.
- `python3 -B scripts/docs-hygiene.py verify-inventories`: 7 inventarios, 0 fallos.
- No se ejecutó ningún test Rust ni gate: el encargo es documental y no toca
  `crates/`.

## Risks

- El conteo de 114–128 syscalls y la ausencia de `mount`/`unshare`/`ptrace`/`bpf`
  en los seis perfiles seccomp es una inspección estática de este worker, no un
  test del repositorio. El oráculo que la respalda es `gateway.rs:185` más las
  calibraciones.
- La clasificación N/U/H/— de cada control es un juicio del worker; la
  auditoría independiente debe revisarla.
- Varias citas de tests de journal M2 (`native_mutation.rs`) se toman del
  análisis W13 (`03-formats-analysis.md`). No se re-leyeron línea a línea, salvo
  `:3279` y el `#[path]` en `filesystem/macos/mutation.rs:2948`.

## Open issues (fuera de los archivos permitidos)

1. `.github/workflows/release-candidate.yml:219` exige `counts.tools == 31`;
   `scripts/release-smoke.py:35-72` publica 36. Un draft 0.8.x falla cerrado →
   M8-07.
2. La branch protection registrada (`docs/validation/M1/public-ci-live-33928952807.json:43-55`)
   exige el check de Windows, retirado del CI el 2026-09-13 (`docs/ci.md:119`).
   No se consultó en vivo; hay que re-observarla antes de RC1 (RR-12).
3. Los mensajes `#[ignore]` de `crates/execution-adapter/tests/semver_runtime.rs:203-376`
   dicen «pending M3-04 calibration» con M3 ya calificado: texto desactualizado.
4. `docs/security-model.md` conserva el encabezado histórico «Escritura local M2
   en desarrollo», aunque su propio cuerpo declara la calificación M2
   completada. No se cambió por ser un título de sección enlazable.
5. La casilla del checklist «Registro de riesgos residuales» solo puede marcarse
   tras la re-review de M8-08 posterior a la auditoría (ADR-089, decisión 3).
