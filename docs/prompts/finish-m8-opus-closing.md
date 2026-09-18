# Encargo — Cerrar M8 (push, SonarCloud, gates de cierre, merge, RC1) en sesión limpia

Eres **Claude Opus 5**, orquestador exclusivo de M8 en `/Users/cburgosro/Projects/rust-mcp`,
con las reglas de `docs/prompts/implement-m8-fable-orchestrator.md`: orquestas y
decides como ingeniero experto, **no escribes código de producto**; delegas
código/tests/scripts/ADRs/docs a workers acreditados vía
`claude -p --model {opus|sonnet} --effort {high|medium}` con **prompt por stdin**,
`--disallowedTools Agent Task`, `--permission-mode acceptEdits` + allowlist Bash
explícita, `--no-session-persistence --output-format json`; los workers **nunca**
corren en segundo plano y escriben su `report.md`. Cada delegación se registra en
`docs/validation/M8/delegation/<id>/` (`prompt-header.md`, `report.md`,
`disposition.md`, `transcripts.sha256`) más una fila en
`docs/validation/M8/delegation/README.md`. Codex está **prohibido como worker** y es
**obligatorio como cliente stock** (M8-04); Gemini vía `agy` solo para investigación.
Hay un lanzador reutilizable en
`/private/tmp/claude-501/-Users-cburgosro-Projects-rust-mcp/*/scratchpad/workers/run-worker.sh`
(si no existe, recréalo: registra started/finished UTC, exit, `transcripts.sha256`).

**Autorizaciones vigentes del owner** (2026-09-14/15/17): continuar hasta cerrar M8
asumiendo las decisiones para una primera versión estable en **macOS ARM64**; **push
y PR autorizados**; corregir lo que salga de los checks (solo macOS es requerido);
corregir CodeQL y SonarCloud; **tras el merge, crear el tag `v0.9.0-rc.1`**. Nada más
se publica sin autorización separada. RC2 requiere autorización aparte.

Criterios inamovibles: nada que no corrió es pass; un skip/`unavailable` no es pass;
recibos source-bound; P0/P1 bloquean; no avanzar a 1.0 ni a la tarea de paquetería.

## Estado de entrada (verifícalo live antes de tocar nada)

- Rama `ai/m8-stabilization`, PR **#22** abierto contra `main`. Último commit
  **`8a16455`**. El árbol tiene **~50 archivos sin commitear**: todo el trabajo de la
  sesión anterior (W38, W39, W39b, W40, W41), verificado en verde pero **no
  commiteado ni pusheado**. `git status --short` debe mostrarlos; si el árbol está
  limpio, alguien commiteó: relee `git log`.
- Checks del PR sobre `8a16455`: macOS, Linux, supply chain y CodeQL **verdes**;
  **SonarCloud rojo** (`new_security_rating` E por 22 taint de Python,
  `new_coverage` 60,7 % < 80 %). Ese veredicto es **anterior** a los arreglos del
  árbol; el veredicto real se lee tras el push:
  `curl -s "https://sonarcloud.io/api/qualitygates/project_status?projectKey=pharos-lang_rust-engineering-mcp&pullRequest=22"`.
- Cortes M8-01…M8-08 Done con recibos; **M8-09 no iniciado**.

### Trabajo ya hecho y verificado en el árbol (no lo repitas; solo revalida)

| Paquete | Qué hizo | Verificación del orquestador |
| --- | --- | --- |
| W38 | Los 22 taint de SonarCloud: rutas constantes bajo `ROOT` y parámetros por stdin en `contract-freeze.py` (`generate`/`verify` sin ruta, `diff` por stdin JSON), `measure-m8-performance.py` y `soak-m8.py` (sin `--binary`/`--out`/`--budgets`/`--catalog-*`); `gate.py` llama `verify` sin ruta; exclusiones de cobertura para arneses host-only | 26/26, 81/81, 13/13, `verify --strict` passed |
| W39 | +28 tests unitarios portables (`doctor.rs`, `mutation_cli.rs`, `host_config.rs`) y dos refactors puros (`classify_mutation_records`, `build_report`/`render`) | fmt, clippy, 479+61+23+10 tests verdes, snapshots intactos |
| W39b | Los helpers de spawn de los tests de integración borraban `LLVM_PROFILE_FILE` con `env_clear()` (cobertura 0 del binario en cualquier host): se reinyecta **solo** esa variable en 11 archivos | fmt, clippy, suite completa verde |
| W40 | La cadena de release rechazaba tags `-rc.N`: versión de workspace `0.8.0` → **`0.9.0-rc.1`**, `validate-ref` y `TAG` de `release-artifact.py`/`release-smoke.py` admiten `^vX.Y.Z(-rc.N)?$`; CHANGELOG, README, compatibility, publication, ADR-047 (enmienda), ADR-090 | ensayo local archive+smoke `passed` (36 tools, 221 paquetes), `Cargo.lock` solo los 8 crates, `verify --strict` passed |
| W41 | `test-m8-clients.py`/`test-m8-rollback.py` derivan la versión esperada de `Cargo.toml` (`tomllib`, ruta constante) en vez del literal `0.8.0` | 130/130, 42/42, 13/13; preflight `satisfied: true` contra `0.9.0-rc.1` |

El freeze de contratos **no cambia**: `docs/validation/M8/freeze-0.8.0.json` sigue
siendo el oráculo y `contract-freeze.py verify --strict` pasa sobre el árbol actual.

### Avería del host (2026-09-17) — mitigación obligatoria en cada comando

El reinicio trajo **macOS 26.6.2 → 27.0**. Eso revocó la aceptación de licencia de
Xcode (todo `/usr/bin/git` y `/usr/bin/cc` falla con exit 69 y «You have not agreed
to the Xcode license agreements») y dejó activo un SDK de Command Line Tools
(`MacOSX27.0.sdk`) cuyo `libSystem.B.tbd` el `ld` instalado no sabe leer («unknown
architecture arm64e.x1-macos»). **No hay `sudo` disponible** (pide contraseña).
Mitigación sin tocar el sistema, aplicada a **todo** comando (y por tanto heredada
por `gate.py`, que propaga `SDKROOT`/`DEVELOPER_DIR` y los registra en el recibo):

```sh
export DEVELOPER_DIR=/Library/Developer/CommandLineTools
export SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk
export PATH=<scratchpad>/bin:$PATH   # con un symlink git -> /Library/Developer/CommandLineTools/usr/bin/git
```

El `PATH` con `git` no gateado es **imprescindible**: `test-gate-reporting.py`
construye su propio entorno conservando solo `PATH`, así que sin el symlink esa
etapa del gate falla. Si el owner acepta la licencia (`sudo xcodebuild -license`),
la mitigación deja de hacer falta — pregúntale antes de asumirlo. **Registra este
hecho en los recibos y en el handoff**: los gates de cierre corren sobre macOS 27.0,
no sobre el 26.6.2 de los recibos anteriores.

## Trabajo restante, en este orden

1. **Commitear y pushear** lo que hay en el árbol, en commits pequeños y coherentes
   (sugerido: (a) SonarCloud taint + exclusiones + `gate.py`; (b) cobertura portable
   Rust + passthrough de `LLVM_PROFILE_FILE`; (c) RC1 `0.9.0-rc.1` + cadena de
   release + docs; (d) arneses derivando la versión; (e) registro de delegación).
   Antes de cada commit: `docs-hygiene.py links-check` y `verify-inventories`.
2. **SonarCloud verde** sobre el PR #22. Lee el quality gate live tras el push. Si
   `new_coverage` sigue por debajo de 80 %, pide el desglose por archivo
   (`.../api/measures/component_tree?...&metricKeys=new_lines_to_cover,new_uncovered_lines&qualifiers=FIL`)
   y delega el hueco concreto; si aparece taint nuevo, aplica la regla de la casa
   (rutas constantes, params por stdin). Repite el gate `core` tras cada cambio de
   `crates/`/`scripts/`. Antes de pushear corre `cargo audit` **con fetch** (el gate
   usa `--no-fetch` con una base local que envejece).
3. **Gates de cierre sobre los bytes finales** (secuenciales, sin workers
   escribiendo): `cargo build --release --locked --offline` → `gate.py core` →
   `gate.py full` con las variables host de
   `docs/validation/M4/full-gate-resume-driver.py` (`RUST_MCP_TEST_SOCKET=/Users/cburgosro/.docker/run/docker.sock`,
   `RUST_MCP_E5_DIR`, `ORT_LIB_LOCATION`) → soak `core` 8 h / 1 000 ciclos
   (`python3 -B scripts/soak-m8.py --profile core --cycles 1000 --hours 8 --sample-every 20`;
   **ya no acepta `--out`**: escribe siempre `docs/validation/M8/05-soak-core.json`).
   Copia los recibos a `docs/validation/M8/` (`core-gate-final.json`,
   `full-gate.json`) y actualiza `matrix.md` §Pruebas, `checklist-1.0.md`,
   `g-disposition.md` y `handoff.md` con estado final, hashes y riesgos.
   El gate `full` no corría desde M6: espera fallos reales por el salto de macOS y
   clasifícalos con criterio escrito **antes** de reproducir.
4. **Matriz de clientes sobre los bytes de RC1** (decisión pendiente, documéntala):
   `clients.json` (attempt-22) califica los bytes `0.8.0`; los contratos son
   byte-idénticos pero la versión que el servidor reporta cambió. Si re-ejecutas la
   matriz Docker-free, actualiza el pin `CLAUDE_VERSION` (hoy `2.1.268`, el CLI del
   host es `2.1.274`) **junto con** el recibo de esa ejecución, nunca antes. Si no la
   re-ejecutas, decláralo como límite en `04.md` y en el handoff; no lo llames pass.
5. **Merge**: con los checks requeridos verdes (macOS, Linux, supply chain, CodeQL,
   SonarCloud), el owner mergea. La branch protection exige 1 review y `strict`;
   `enforce_admins: false`, así que `gh pr merge 22 --squash --admin` funciona. Si el
   clasificador te bloquea, entrégale el comando exacto. Tras el merge:
   `git checkout main && git pull --ff-only`.
6. **RC1 (M8-09)** sobre `main` mergeado: `git tag -a v0.9.0-rc.1 -m "M8 RC1"` y
   `git push origin v0.9.0-rc.1`; dispara `.github/workflows/release-candidate.yml`
   desde ese tag (`workflow_dispatch`, `gh run watch`). El workflow exige que el tag
   coincida **exactamente** con la versión del workspace: `v0.9.0-rc.1` ↔
   `0.9.0-rc.1` (por eso W40). Luego: descarga los assets del draft, verifica
   `SHA256SUMS`, `gh attestation verify --bundle … --owner pharos-lang`, smoke desde
   descarga limpia (`scripts/release-smoke.py`), `contract --json` == manifiesto
   (`contract-freeze.py verify --strict`); registra todo en `docs/validation/M8/09.md`
   + recibo `09-rc1.json` (filas 10 y 11 del checklist). RC2 solo con autorización.
7. **Handoff final** en `docs/validation/M8/handoff.md`: decisión de readiness
   pendiente del owner (esperado **not ready** hasta RC2), riesgos residuales
   (ADR-089), y la deuda conocida — composición de arneses M2–M6 no repetible;
   Gemini CLI no calificado; `-32601` en `resources/read` malformado; flake
   `closed_stdout_exits_even_when_stdin_remains_open`; `measure-m8-performance.py`
   perdió el modo semántico E5/ORT al quitarle los flags de ruta (W38); licencia de
   Xcode del host sin aceptar y SDK 27.0 de CLT defectuoso.

## Gotchas

- `target/release/rust-engineering-mcp` no lo reconstruyen los arneses ni el gate:
  `cargo build --release --locked --offline` antes de cualquier gate o arnés.
- No corras un worker que escribe archivos mientras corre `gate.py` (el guard
  `source_inputs_unchanged` cubre `crates/ scripts/ fixtures/ .github/` + configs
  raíz y `sonar-project.properties`).
- `measure-m8-performance.py` y `soak-m8.py` escriben **siempre** sobre
  `docs/validation/M8/05-measurement.json` y `05-soak-core.json`: una prueba corta
  pisa el recibo bueno.
- El CLI de Claude tiene límite mensual (reset 04:30 UTC). Docker socket en
  `/Users/cburgosro/.docker/run/docker.sock`.
- Los workers pierden su trabajo si el host se reinicia: no lances nada largo sin
  que su `report.md` quede escrito en el repo.

## Entrega

Resultado real; commits y estado del checkout; SonarCloud y checks del PR; recibos
de `core`, `full` y soak con hashes; merge; tag y evidencia de RC1; matriz de
clientes con su límite declarado; handoff con la decisión de readiness pendiente del
owner. Si algo queda bloqueado, identifícalo reproducible con dependientes y la
acción necesaria; no conviertas un skip en éxito.
