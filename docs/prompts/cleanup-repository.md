# Limpieza y ordenación del repositorio — prompt de ejecución

Actúa como Technical Owner e integrador de `rust-engineering-mcp` en
`/Users/cburgosro/Projects/rust-mcp`, en una rama nueva `ai/repo-hygiene`
desde `main` (después del merge de M5 y la release `v0.3.0`). Lee `AGENTS.md`,
`docs/implementation-status.md` y este prompt. No toques código Rust, `scripts/`
de gates, `fixtures/`, `vendor/`, `Cargo.*`, ADRs ni contratos: esta tarea es de
higiene del repositorio, no de producto. Cada movimiento debe conservar la
trazabilidad de la evidencia: los recibos acreditan bytes por SHA-256, y las
matrices, handoffs, ADRs, README y tablero los enlazan por ruta.

## Punto de partida medido (2026-09-11)

- `docs/` tiene 3 599 archivos versionados; `docs/validation` 2 341 (≈100 MB),
  `docs/research` 644, `docs/reviews` 423 (126 entradas de primer nivel),
  `docs/release` 46 (8 MB), `docs/adr` 82, `docs/prompts` 16.
- Recibos de primer nivel en `docs/validation` por milestone: M0 15, M1 122,
  M2 50, M3 63, M4 57, M5 27. Solo M3 conserva 28 variantes de
  `M3-{core,full,runtime}*.json` (`attempt1..7`, `preS03`, `pre-vsec`,
  `pre-w4`, `pre-contract-pin`, `v0.2.0-dev`).
- Los intentos de clientes versionan el estado privado del store de cada
  sesión: `docs/validation/m{2,3,4,5}-clients/attempt-N/state-*/**` con
  `*.blob`, `store.lock`, `clock-watermark.json`, perfiles seccomp copiados y
  transcripts `*.jsonl`/`*.stderr`/`*.stdout` (46–61 archivos por intento).
- Hay directorios de historia duplicada: `docs/validation/m5-closure-history/`
  (con `lock-0.38.0/`), `docs/validation/m5-gate-attempts/closure-*`,
  `docs/validation/m2-pre-059/`, `docs/validation/M4-miri-native/final`.
- Archivos grandes versionados: `fixtures/hostile-reports/bundle-oversize-member.tar`
  (33 MB, fixture legítima: no tocar), `docs/validation/M2-D05-*.json` (8,7 y
  2,8 MB), `docs/release/THIRD_PARTY_NOTICES.candidate.txt` (5 MB),
  `docs/release/inventory.json` (1 MB), `docs/research/m1-16/measurement/*`.
- `.gitignore` ya excluye `*.log`, `coverage/`, `target/`, homes de clientes y
  transcripts crudos de M3; pero los `.log` de gates se han versionado como
  `.txt` en `m5-gate-attempts/`, y `.DS_Store` aparece en directorios de
  trabajo (`vendor/lancedb/.DS_Store` rompe `verify-vendor.py`).

## Objetivo

1. **Un solo paquete de evidencia por milestone**, con la misma forma para
   M0–M5: `docs/validation/M<n>/` conteniendo `matrix.md`, `handoff.md`, los
   recibos **vigentes** (`core-gate.json`, `full-gate.json`, `runtime.json`,
   `clients.json`, nativos por corte) y un `history/` con `inventory.json`
   (ruta original, SHA-256, bytes, motivo de superación) para todo intento
   fallido o recibo superado que se decida conservar. Las variantes de M3 y
   los `closure-*` de M5 pasan a ese `history/`; nada se edita, solo se mueve.
2. **Evidencia de clientes sin estado privado**: dentro de cada `attempt-N`
   conservar solo `receipt.json`, `protocol.jsonl` (metadatos), los transcripts
   del turno dirigido por modelo y la traza del harness; retirar `state-*/**`
   (blobs del store, locks, watermark, copias de seccomp) y registrar su
   retirada en el `history/inventory.json` con hash agregado. Añadir a
   `.gitignore` `docs/validation/**/state-*/` y comprobar que los scripts
   `test-m{2..5}-clients.py` no dependen de que esos directorios estén
   versionados (solo escriben en el intento nuevo).
3. **Rutas estables para lo que se cita**: antes de mover nada, extraer con un
   script todos los enlaces `](...)` de `README.md`, `CHANGELOG.md`,
   `SECURITY.md`, `docs/*.md`, `docs/adr/`, `docs/roadmap/`, `docs/prompts/`,
   `docs/reviews/` y `docs/validation/*.md`, y todas las rutas de recibos que
   leen los scripts (`grep -rn "docs/validation\|docs/release" scripts/
   .github/ crates/`); mover con `git mv` y reescribir enlaces con el mismo
   script; volver a validar que no queda ningún enlace roto ni ruta de script
   huérfana. Los scripts que leen recibos por ruta (`test-m5-runtime.py` lee
   `docs/validation/M5-provisioning.json` y `docs/adr/ADR-077`;
   `check-architecture.py` lee `M5-02-method-simulation.json`;
   `release-artifact.py` lee `docs/release/upstream-licenses/receipt.json`)
   se actualizan en el mismo commit que el movimiento, y sus tests unitarios
   deben pasar.
4. **Revisiones y prompts**: agrupar `docs/reviews/` por milestone
   (`docs/reviews/M<n>/`) conservando nombres; mover a `docs/prompts/history/`
   los prompts ya ejecutados (`complete-m5.md`, `finish-m5-fable.md`,
   `implement-m2..m4`), dejando en `docs/prompts/` solo los pendientes
   (`implement-m6/m7/m8`) y este.
5. **Raíz y artefactos de release**: `PUBLICATION-SNAPSHOT.json` se documenta o
   se mueve a `docs/release/`; `docs/release/` separa `0.1.0/` y `0.3.0/`
   (recibos locales, notices candidatos, inventarios) de `upstream-licenses/`
   y `reproduction/`, que son transversales. No borrar ningún notice ni
   licencia: son obligaciones de redistribución.
6. **Lo que no se versiona**: añadir a `.gitignore` `**/.DS_Store` (ya está
   `.DS_Store` sin prefijo; verificar que cubre subdirectorios), `*.dSYM/`,
   `*.orig`, `*.rej`, `*.swp`, `.pytest_cache/`, `node_modules/`,
   `target-*/` de sondas, `dist/` y cualquier `*.tar.gz` fuera de `fixtures/`.
   Ejecutar `git ls-files -i -c --exclude-standard` para detectar archivos ya
   versionados que ahora quedan ignorados y decidir uno a uno (no `git rm`
   masivo sin lista revisada).
7. **`docs/research/m1-16/measurement/`** (90 archivos de medición cruda):
   sustituir por un `inventory.json` con hashes y el `REPORT.md`, salvo los
   archivos que el informe cite por nombre; el resto queda fuera del árbol
   con su hash registrado.

## Reglas

- Un commit por tipo de movimiento (`chore(docs): move M3 receipts into
  docs/validation/M3/history`, etc.), con `git mv` para conservar historia;
  ninguna edición de contenido de recibos ni de ADRs aceptados; si un ADR
  cita una ruta, se actualiza solo la ruta.
- Antes de cada commit: script de enlaces sin faltantes, `python3 -B
  scripts/test-m5-clients-unit.py`, `scripts/test-release-artifact.py`,
  `scripts/test-release-smoke.py`, `scripts/test-gate-reporting.py` y
  `python3 scripts/verify-vendor.py` en verde; `cargo check --workspace
  --all-targets --locked --offline` sin cambios (no debe haber ninguno).
- Al final: `python3 -B scripts/gate.py core` sobre un worktree limpio (los
  gates inventarían `scripts/` y `.github/`; documentar el recibo en el
  paquete del milestone vigente), actualización de `docs/implementation-status.md`
  con la nueva convención de rutas, y un `docs/validation/README.md` que
  describa el layout por milestone y la política de historia.
- Entregar un resumen con: archivos movidos/retirados por categoría, bytes
  liberados del árbol, lista de enlaces reescritos, y cualquier recibo cuyo
  hash no pudo verificarse (no debe haber ninguno).
