# W33 — endurecimientos V04: `persist-credentials: false` (F-05) y rutas de host dentro de roots (F-06)

Worker: Claude Sonnet 5 (`claude -p --model sonnet --effort high`), sin subagentes, sin commit, sin Docker.

## Cambios

**(1) F-05 — `persist-credentials: false` en los cuatro workflows.** Añadido
`with: persist-credentials: false` al paso `actions/checkout` en
`.github/workflows/ci.yml` (dos jobs: `portable` y `supply-chain`),
`release-candidate.yml` (job `build`), `sonarcloud.yml` (se añadió a un
`with:` ya existente con `fetch-depth: 0`) y `codeql.yml`. Ningún otro campo
tocado. Revisé los cuatro workflows completos en busca de pasos posteriores
que necesiten las credenciales de git persistidas por el checkout:
- `release-candidate.yml` usa `gh attestation verify` y `gh release create`
  con `GH_TOKEN: ${{ github.token }}` explícito por `env`, no credenciales de
  git persistidas.
- `sonarcloud.yml` usa `SONAR_TOKEN` por `env` para el scanner, sin `git
  push`/`gh`.
- `ci.yml` y `codeql.yml` no tienen ningún paso que escriba al remoto.
- El job `draft` de `release-candidate.yml` (el que sí publica el release) no
  tiene su propio `actions/checkout`, así que no le afecta este cambio.

**(2) F-06 — rutas de host dentro de roots en `host_config.rs`.**
- `--rustsec-snapshot`: el `match (audit_path, audit_fingerprint)` ahora exige
  `!config.roots.iter().any(|root| path.starts_with(root))`, mismo patrón que
  ya usa `--security-policy` (líneas ~206-214).
- `--catalog-store`, `--catalog-trust`, `--catalog-model-dir` y
  `--catalog-index-store`: el bloque que arma `config.catalog` ahora rechaza
  la configuración completa si `store`, `trust`, o el `model_dir`/
  `index_store` opcionales (cuando están presentes) caen dentro de (o son
  iguales a) cualquier `--root`, con un cierre `outside_roots` que aplica el
  mismo `path.starts_with(root)`.
- La regla del state-root (líneas ~283-293, solo se comprueba contra las
  roots cuando hay grants de escritura) no se tocó.

**(3) Tests nuevos en `crates/mcp-server/tests/cli.rs`** (uno por flag, como
pide el prompt): `rustsec_snapshot_inside_a_root_is_rejected`,
`catalog_store_inside_a_root_is_rejected`,
`catalog_trust_inside_a_root_is_rejected`,
`catalog_model_dir_inside_a_root_is_rejected`,
`catalog_index_store_inside_a_root_is_rejected`. Cada uno prueba la ruta
dentro de la root y la ruta igual a la root (exit 2, stdout vacío, stderr
idéntico al de un comando desconocido) y luego la misma ruta fuera de la
root (éxito, EOF sin salida — igual que el resto de configuraciones host
completas). Extraje dos helpers (`temp_root`/`temp_path` para las rutas
absolutas UTF-8 con el directorio temporal canonicalizado, y
`assert_root_containment_is_enforced` para no repetir cinco veces el mismo
bucle de aserciones) para no duplicar ~200 líneas casi idénticas.

Nota de implementación: la primera versión de estos tests usaba
`std::env::temp_dir()` sin `canonicalize()`. Con un `--root` real (necesario
para el caso "fuera, aceptado", que sí arranca `SecureProjects::new` y por
tanto abre el root con `NOFOLLOW_ANY`), eso falla en macOS porque `/tmp` es
un symlink a `/private/tmp`. `crates/mcp-server/tests/protocol.rs` ya
resuelve esto canonicalizando el directorio temporal antes de unir el
nombre de fixture (`project_fixtures::Fixture::new`); apliqué el mismo
patrón.

**(4) `docs/tools.md`.** La frase «Vendor y policy no pueden solaparse con
una root de proyecto» pasa a «Vendor, policy y rustsec-snapshot no pueden
solaparse con una root de proyecto», y se añadió una frase explícita después
del bloque `--catalog-store PATH --catalog-trust PATH` cubriendo las cuatro
rutas de catálogo con la misma regla.

## Arneses/fixtures que configuran estos flags

`grep -rn 'catalog-store\|rustsec-snapshot' scripts/ fixtures/` (sin
coincidencias bajo `fixtures/`; los fixtures que usan estos flags viven en
los harnesses de `scripts/` y en `crates/mcp-server/tests/`, ya cubiertos
más abajo) encontró uso en:
`scripts/test-m4-clients.py`, `scripts/measure-m8-performance.py`,
`scripts/soak-m8.py`, `scripts/test-m8-clients.py`,
`scripts/test-m8-clients-unit.py`, `scripts/test-m8-rollback.py`,
`scripts/test-codex-model-qualifier.py`. Revisé cada uno para confirmar que
`--catalog-store`/`--catalog-trust`/`--rustsec-snapshot` no caen dentro de
ningún `--root` que el mismo comando reciba:

- `scripts/test-m4-clients.py`: `store`/`trust` son hermanos de `project`/
  `cancel-project` bajo el mismo `private_root`, nunca dentro de esos dos
  roots.
- `scripts/soak-m8.py`: `store`/`trust` viven en `scratch` (un tempdir bajo
  `target/`), la root es `FIXTURE` (`fixtures/valid-basic`), directorios
  distintos.
- `scripts/measure-m8-performance.py`: la root es siempre `FIXTURE`
  (`fixtures/valid-basic`); `--catalog-store`/`--catalog-trust`/
  `--catalog-model-dir`/`--catalog-index-store` del perfil `local` son rutas
  que aporta el operador por CLI (default `None`), no rutas fijas del
  arnés — no hay una configuración fija que colisione, pero el operador
  podría en teoría apuntarlas dentro de `fixtures/valid-basic`; lo señalo
  como posible gap operativo, no de código.
- `scripts/test-m8-rollback.py`, `scripts/test-m8-clients.py`,
  `scripts/test-m8-clients-unit.py`, `scripts/test-codex-model-qualifier.py`:
  sin coincidencias de rutas fijas colisionando (el primero no usa estos
  flags; los otros tres son comentarios/lógica de aserción sobre el texto de
  rechazo, no invocaciones con rutas literales).

No rompí ningún arnés; ninguno necesitó cambios.

## Verificación

- `cargo fmt --all -- --check` → limpio.
- `cargo clippy -p rust-engineering-mcp --all-targets --locked --offline -- -D warnings` → sin warnings.
- `cargo test -p rust-engineering-mcp --locked --offline --test cli` → 20/20 OK (15 preexistentes + 5 nuevos).
- `cargo test -p rust-engineering-mcp --locked --offline --bin rust-engineering-mcp host_config` → 4/4 OK (tests unitarios preexistentes de `host_config.rs` sin cambios de comportamiento).
- `cargo test -p rust-engineering-mcp --locked --offline --test doctor --test catalog_status --test crate_search --test crate_inspect --test inspection_runtime` → todos OK (los que usan `--catalog-store`/`--catalog-trust` reales no colisionan con ninguna root; los tests de `inspection_runtime` que requieren Docker se omiten como siempre, sin Docker en este entorno).
- `python3 -B scripts/test-gate-reporting.py` → 13 tests, OK.
- `python3 -c "import yaml,sys;[yaml.safe_load(open(f)) for f in sys.argv[1:]]" .github/workflows/ci.yml .github/workflows/release-candidate.yml .github/workflows/sonarcloud.yml .github/workflows/codeql.yml` → YAML válido en los cuatro.

## Archivos tocados

`.github/workflows/ci.yml`, `.github/workflows/codeql.yml`,
`.github/workflows/release-candidate.yml`, `.github/workflows/sonarcloud.yml`,
`crates/mcp-server/src/host_config.rs`, `crates/mcp-server/tests/cli.rs`,
`docs/tools.md`. No se tocó ningún otro archivo fuera de la lista permitida
(el `git status` de partida ya traía cambios de otros workers concurrentes —
`CHANGELOG.md`, `README.md`, ADRs, `scripts/test-m8-clients*.py`, etc. — que
no toqué). Sin commit.
