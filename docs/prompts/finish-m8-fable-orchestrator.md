# Encargo — Terminar M8 (SonarCloud, gates de cierre, merge, RC1) en sesión limpia

Eres **Claude Fable**, orquestador exclusivo de M8 en `/Users/cburgosro/Projects/rust-mcp`,
con las mismas reglas que `docs/prompts/implement-m8-fable-orchestrator.md`
(delegación acreditada vía `claude -p` con prompt por stdin, `--disallowedTools
Agent Task`, registro en `docs/validation/M8/delegation/README.md`; Codex solo
como cliente; Gemini vía `agy`). Autorizaciones vigentes del owner (2026-09-14/15):
continuar hasta cerrar M8 asumiendo las decisiones para una primera versión
estable en **macOS ARM64**; **push y PR autorizados**; corregir lo que salga
de los checks (Windows/Linux no son requeridos, solo macOS); corregir CodeQL y
SonarCloud; **tras el merge, crear el tag `v0.9.0-rc.1`**. Nada más se publica
sin autorización.

## Estado de entrada (verifícalo live)

- Rama `ai/m8-stabilization` **pusheada**, PR **#22** abierto contra `main`
  (`gh pr view 22`). Tip esperado `d700575` (26 commits sobre `main e50c3fe`),
  árbol limpio salvo `docs/validation/M8/delegation/` (registro).
- Checks del PR: `portable / aarch64-apple-darwin`, `portable / x86_64-unknown-linux-gnu`,
  `supply chain`, `CodeQL (actions/python/rust)` **verdes**; **SonarCloud fail**
  (quality gate: `new_security_rating` E por 22 hallazgos de taint Python y
  `new_coverage` 62,7 % < 80 %). Consulta live:
  `curl -s "https://sonarcloud.io/api/qualitygates/project_status?projectKey=pharos-lang_rust-engineering-mcp&pullRequest=22"`
  y `…/api/issues/search?componentKeys=pharos-lang_rust-engineering-mcp&pullRequest=22&resolved=false`.
- Cortes M8-01…M8-08 **Done local** con recibos (`docs/validation/M8/matrix.md`,
  `checklist-1.0.md`, `handoff.md` borrador, `g-disposition.md` borrador).
  M8-09 no iniciado.
- Un reinicio del host (2026-09-17) perdió el scratchpad y mató dos workers
  (W38, W39) **sin aplicar cambios** (solo existen sus `prompt-header.md`):
  relánzalos tal cual. También se perdió la cadena `core → full → soak`.
- Gotchas: `target/release/rust-engineering-mcp` **no** lo reconstruyen los
  arneses ni el gate → `cargo build --release --locked --offline` antes de
  cualquier gate/arnés; `cargo audit --no-fetch` usa una base local que puede
  estar obsoleta → `cargo audit` con fetch antes de pushear; los workers deben
  usar `timeout` largo en Bash (nunca segundo plano) y escribir su `report.md`;
  el CLI de Claude tiene límite mensual (reset 04:30 UTC); Docker socket
  `/Users/cburgosro/.docker/run/docker.sock`; variables del `full` en
  `docs/validation/M4/full-gate-resume-driver.py`.

## Trabajo restante (en este orden)

1. **SonarCloud (bloquea el merge)**
   - Relanza **W38** (`delegation/W38-sonar-taint-coverage/prompt-header.md`):
     rutas constantes bajo `ROOT` + parámetros por stdin en `contract-freeze.py`,
     `measure-m8-performance.py`, `soak-m8.py`; exclusiones de cobertura solo
     para arneses host/Docker-only (`measure-m8-performance.py`, `soak-m8.py`,
     `test-m8-rollback.py`, `test-m8-clients.py`, `m8-inspector-session.mjs`),
     nunca `crates/**` (lo prohíbe `test-gate-reporting.py`). Ajusta la etapa
     `contract-freeze` de `gate.py` y los recibos si cambia la invocación.
   - Relanza **W39** (`delegation/W39-rust-portable-coverage/prompt-header.md`):
     tests **portables** (Linux llvm-cov) para `doctor.rs` (96/135 sin cubrir),
     `mutation_cli.rs` (31/31), `host_config.rs` (13/13), `contract_cli.rs`,
     `main.rs`; objetivo ≥ 80 % de líneas nuevas.
   - Verifica localmente (simulando el runner: sin `target/m1-17-inspector`,
     `GIT_DIR=/nonexistent`) todas las suites listadas en
     `.github/workflows/sonarcloud.yml`; commitea, pushea y repite hasta que el
     quality gate del PR esté verde. Repite el gate `core` tras cada cambio de
     `crates/`/`scripts/`.
2. **Gates de cierre sobre los bytes finales** (antes del merge; secuenciales,
   sin workers escribiendo): reconstruir `release` → `gate.py core` →
   `gate.py full` (variables host) → soak `core` 8 h / 1 000 ciclos
   (`scripts/soak-m8.py --profile core --cycles 1000 --hours 8 --sample-every 20`,
   salida `docs/validation/M8/05-soak-core.json`). Copia los recibos a
   `docs/validation/M8/` (`core-gate-final.json`, `full-gate.json`), actualiza
   `matrix.md` §Pruebas, `checklist-1.0.md`, `g-disposition.md` y `handoff.md`
   (estado final, hashes, riesgos), y commitea.
3. **Merge**: cuando los checks requeridos estén verdes (macOS, supply chain,
   CodeQL, SonarCloud), el owner mergea (`gh pr merge 22 --squash --admin` si
   el clasificador te lo bloquea, entrégale el comando). Tras el merge:
   `git checkout main && git pull --ff-only`.
4. **RC1 (M8-09)**: sobre `main` mergeado, `git tag -a v0.9.0-rc.1 -m "M8 RC1"`
   y `git push origin v0.9.0-rc.1`; sigue `.github/workflows/release-candidate.yml`
   (`gh run watch`); descarga los assets del draft, verifica `SHA256SUMS`,
   `gh attestation verify --bundle … --owner pharos-lang`, smoke desde
   descarga limpia (`scripts/release-smoke.py`), `contract --json` == manifiesto
   (`contract-freeze.py verify --strict`); registra todo en
   `docs/validation/M8/09.md` + recibo `09-rc1.json` (checklist filas 10 y 11).
   RC2 solo con autorización separada.
5. **Handoff final**: `docs/validation/M8/handoff.md` con decisión de readiness
   pendiente del owner (esperado: **not ready** hasta RC2), riesgos residuales
   (ADR-089), deuda (composición de arneses M2–M6 no repetible; Gemini CLI no
   calificado; `-32601` en `resources/read` malformado; flake
   `closed_stdout_exits_even_when_stdin_remains_open`).

Criterios inamovibles: nada que no corrió es pass; un skip/`unavailable` no es
pass; recibos source-bound; P0/P1 bloquean; no avanzar a 1.0 ni a la tarea de
paquetería.
