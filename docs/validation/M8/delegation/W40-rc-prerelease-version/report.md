# W40 — RC1 `v0.9.0-rc.1`: versión de workspace pre-release y cadena de release `-rc.N`

Worker: Claude Sonnet 5 (`claude -p --model sonnet --effort high`). Sin subagentes,
sin commit/push/tag/`gh`/Docker/red. Todos los comandos se ejecutaron en primer
plano en este host (macOS ARM64).

## Decisión ejecutada

RC1/RC2 son pre-releases SemVer 2.0 `-rc.N`. La versión de workspace pasa de
`0.8.0` a `0.9.0-rc.1`; la cadena de release (`validate-ref`,
`release-artifact.py`, `release-smoke.py`) admite
`^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-rc\.(0|[1-9][0-9]*))?$` —
solo `-rc.N`, ningún otro pre-release ni build metadata. El paso tag == versión
del workspace sigue siendo una igualdad de cadena completa, sin recortar
sufijos (`0.9.0-rc.1` == `0.9.0-rc.1`). El freeze `0.8.0`
(`docs/validation/M8/freeze-0.8.0.json`) no cambia y sigue siendo el oráculo
para toda la serie 0.9.

## Cambios

- **`Cargo.toml`** — `[workspace.package] version = "0.9.0-rc.1"` (antes `0.8.0`).
- **`Cargo.lock`** — `cargo update --workspace --offline` actualizó exactamente
  las 8 entradas de los crates del workspace (`rust-engineering-application`,
  `-artifact`, `-catalog`, `-domain`, `-execution`, `-mcp`, `-project`,
  `-semantic`) a `0.9.0-rc.1`; `git diff --stat Cargo.lock` confirma que no se
  tocó ninguna otra entrada (16 líneas: 8 pares add/remove de `version = `).
- **`scripts/release-artifact.py`** — `TAG` admite el sufijo `-rc.N`; el
  mensaje de error de `validate_tag` pasa a «tag must be a stable semantic
  version vX.Y.Z or release-candidate vX.Y.Z-rc.N». `validate_tag` sigue
  comparando `tag[1:]` contra la versión completa del paquete raíz (sin
  recorte de sufijo), por lo que todo lo que deriva de `version = tag[1:]`
  (nombre del archive, SBOM `versionInfo`, etc.) sigue funcionando sin cambios
  adicionales.
- **`scripts/release-smoke.py`** — mismo cambio de `TAG` y mensaje de error en
  `validate_archive`; el resto de la cadena (`tag[1:]`, nombre esperado del
  archive, `version --json`, `serverInfo.version`) ya era genérico respecto al
  tag y no requirió cambios.
- **`scripts/test-release-artifact.py`** — `test_tag_must_match_stable_workspace_version`
  ahora cubre los negativos de formato (`v0.1.1-rc`, `v0.1.1-rc.01`,
  `v0.1.1-beta.1`, `v0.1.1-rc.1+build`) contra el mensaje «stable semantic»;
  nuevo `test_tag_admits_release_candidate_suffix` cubre el positivo
  (`v0.9.0-rc.1` == versión de workspace `0.9.0-rc.1`) y el caso de
  desajuste cuando el tag recorta el sufijo (`v0.9.0` contra versión
  `0.9.0-rc.1`).
- **`scripts/test-release-smoke.py`** — `inventory()`, `base_members()` y
  `write_candidate()` ahora aceptan un `tag` parametrizable (default
  `v0.1.0`, preservando los tests existentes byte a byte). Nuevo
  `test_valid_release_candidate_tag_archive` ejercita `validate_archive` de
  extremo a extremo con `v0.9.0-rc.1` sobre un archive sintético completo;
  nuevo `test_tag_format_positive_and_negative_cases` cubre el positivo
  directamente contra `smoke.TAG` y los cuatro negativos de formato
  (`v0.9.0-rc`, `v0.9.0-rc.01`, `v0.9.0-beta.1`, `v0.9.0-rc.1+build`) tanto
  contra `smoke.TAG.fullmatch` como contra `validate_archive` (mensaje
  «stable semantic»).
- **`.github/workflows/release-candidate.yml`** — únicamente el job
  `validate-ref`: el ERE de bash pasa a
  `^v[0-9]+\.[0-9]+\.[0-9]+(-rc\.[0-9]+)?$` y el mensaje de error a «Dispatch
  this workflow from an existing stable vX.Y.Z or release-candidate
  vX.Y.Z-rc.N tag.». Ningún otro job, permiso, pin o paso del workflow se
  tocó (pins de acciones, digests cruzados, `--draft --prerelease`, etc.
  intactos).
- **`CHANGELOG.md`** — nueva sección superior `## 0.9.0-rc.1 — primer
  candidato (M8-09; draft prerelease, no soportada)` que remite a las
  migration notes `0.3.0 → 0.8.0` existentes (sin cambios de contrato) y
  documenta el esquema de tags RC (`-rc.N`, un bump de versión por RC; `1.0.0`
  solo por decisión de readiness del owner). La sección `## 0.8.0` queda
  intacta debajo.
- **Docs** — `README.md:23` (bloque de tools, ahora cita `0.9.0-rc.1`
  como versión del checkout de desarrollo, freeze `0.8.0` vigente) y
  `README.md:170-172` (instrucción de instalar desde fuente, ahora aclara
  «release **soportada**» y que `v0.9.0-rc.1` es un draft prerelease, no
  soportado); `docs/compatibility.md:8` (fila «Checkout de desarrollo» ahora
  `0.9.0-rc.1`); `docs/publication.md` (el dispatch admite `vX.Y.Z` o
  `vX.Y.Z-rc.N`, mismo naming de archive/checksum y misma igualdad estricta
  tag == versión; un RC nunca es una release soportada);
  `docs/adr/ADR-090-...md:81,161` (el dispatch admite además `vX.Y.Z-rc.N`;
  un RC nunca es una release soportada); `docs/adr/ADR-047-...md` (nueva
  sección `## Amendment (2026-09-17)` datada, sin modificar la decisión
  original). `docs/ci.md` no cita el formato regex del tag (solo menciona
  «un tag de versión existente» en prosa) — no se tocó, según la condición de
  la tarea. Ningún texto histórico sobre «0.8.0 es el freeze» se reescribió.

## Verificación (todas ejecutadas en este host, sin red)

1. `cargo metadata --locked --offline --no-deps --format-version 1 | python3 -c '...'`
   → `{'0.9.0-rc.1'}`.
2. `cargo build --release --locked --offline -p rust-engineering-mcp` →
   compiló limpio, `Finished \`release\` profile [optimized] target(s) in 26.98s`.
3. `target/release/rust-engineering-mcp version --json` →
   `{"format_version":1,"operation":"version","package":"rust-engineering-mcp","version":"0.9.0-rc.1","compiled_local":false,"target_os":"macos","target_arch":"aarch64"}`.
4. `python3 -B scripts/test-release-artifact.py` → `Ran 12 tests ... OK`.
5. `python3 -B scripts/test-release-smoke.py` → `Ran 11 tests ... OK`.
6. Ensayo local completo (como W24b):
   - `python3 -B scripts/release-artifact.py --binary target/release/rust-engineering-mcp --target aarch64-apple-darwin --tag v0.9.0-rc.1 --output-dir target/w40-dist`
     → `status: passed`, `packages: 221`,
     `archive_sha256: efe2da1a4334437028562e62508afde283a239a256bacd1f5f4bf5a3c9a20a24`.
   - `python3 -B scripts/release-smoke.py --archive target/w40-dist/rust-engineering-mcp-v0.9.0-rc.1-aarch64-apple-darwin.tar.gz --sha256sums target/w40-dist/SHA256SUMS --tag v0.9.0-rc.1 --target aarch64-apple-darwin --output-receipt target/w40-dist/release-smoke-receipt.json`
     → `status: passed`, `members: 11`, `packages: 221`, `tools: 36`.
   - Recibo (`target/w40-dist/release-smoke-receipt.json`, no versionado, solo
     ensayo local): `status: passed`; `release.archive_sha256` idéntico al de
     arriba; `release.binary_sha256:
     a7c2ec1bf81732eafc9664e78ceaba6a2662abe49ccedaf328ca2d8594e62cf3`;
     `release.sha256sums_sha256:
     1eea353cd3e12fbdcdbd22d03566e687d60e49cbec7aa66f60fcfe9d492082e5`;
     `counts: {archive_members: 11, cli_calls: 3, mcp_calls: 6, packages: 221,
     tools: 36}`; `cleanup.process_group_clean: true`,
     `cleanup.temporary_installation_removed: true`.
   - Archive: `target/w40-dist/rust-engineering-mcp-v0.9.0-rc.1-aarch64-apple-darwin.tar.gz`
     (10423443 bytes); `SHA256SUMS` sha256
     `1eea353cd3e12fbdcdbd22d03566e687d60e49cbec7aa66f60fcfe9d492082e5`;
     `release-smoke-receipt.json` sha256
     `4696e8430f76992dc40e932bd32ae630e35f4e4ab68c4aac86e44c1b8050593c`.
7. `python3 -B scripts/contract-freeze.py verify --strict` →
   `{"class_changed": [], "format_errors": [], "preview_changed": [],
   "stable_changed": [], "status": "passed"}` (la versión no forma parte del
   contrato congelado).
8. `cargo test -p rust-engineering-mcp --locked --offline --test cli` →
   `test result: ok. 23 passed; 0 failed`. Ningún test fija `0.8.0` como
   versión esperada (`version_comes_from_package_metadata` compara contra
   `CARGO_PKG_VERSION` en runtime); no se editó nada bajo `crates/`.
9. `python3 -B scripts/docs-hygiene.py links-check` → exit 0; `2964 links
   resolved; 0 broken in living documents; 5 point at evidence excluded by
   .gitignore; 459 broken in frozen records` (baseline preexistente de
   records congelados, no introducido por W40; el exit code confirma que el
   check pasa).
10. `python3 -B scripts/docs-hygiene.py verify-inventories` → exit 0; `7
    inventories, 0 failures`.

## Alcance no tocado

`crates/`, snapshots de contrato, `docs/validation/M8/freeze-0.8.0.json` y
cualquier script fuera de la lista permitida permanecen sin cambios por esta
tarea. Los cambios preexistentes y no relacionados en
`crates/mcp-server/src/{doctor.rs,doctor/tests.rs,host_config.rs,mutation_cli.rs}`,
`docs/ci.md`, `scripts/{contract-freeze.py,gate.py,measure-m8-performance.py,
soak-m8.py,test-contract-freeze.py,test-m8-performance-unit.py}` y
`sonar-project.properties` (visibles en `git status` al iniciar esta sesión)
no fueron modificados por esta tarea.

Sin commit, sin push, sin tag, sin `gh`, sin Docker, sin red.
