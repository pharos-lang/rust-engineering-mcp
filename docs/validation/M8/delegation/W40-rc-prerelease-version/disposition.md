# W40 — disposición del orquestador (2026-09-17)

Invocación: `claude -p --model sonnet --effort high --permission-mode acceptEdits --disallowedTools Agent Task --allowedTools <lista> --no-session-persistence --output-format json < prompt-header.md` (CLI 2.1.274). Inicio 2026-09-17T17:43:34Z, fin 19:25:29Z, exit 0, 98 turnos, 1 048 689 ms, `permission_denials: 9` (redirecciones a `/tmp` y comandos `git` durante la avería de licencia de Xcode descrita en [W39b](../W39b-llvm-profile-passthrough/disposition.md)).

Origen del encargo (decisión del orquestador, no del worker): el owner autorizó
crear el tag `v0.9.0-rc.1` tras el merge. La cadena de release lo habría rechazado
en el primer job — `validate-ref` exigía `^v[0-9]+\.[0-9]+\.[0-9]+$`, el paso
«Verify tag matches every workspace package» exige igualdad exacta con la versión
del workspace (`0.8.0`), y `TAG` de `release-artifact.py`/`release-smoke.py`
rechazaba cualquier sufijo. Decisión: RC1/RC2 son pre-releases SemVer 2.0 `-rc.N`;
la versión del workspace pasa a `0.9.0-rc.1` y la cadena admite **solo** ese sufijo.

Veredicto: **aceptado**. Verificación del orquestador sobre el árbol resultante:

- `Cargo.toml` `[workspace.package] version = "0.9.0-rc.1"`; `git diff --numstat
  Cargo.lock` = 8 líneas añadidas / 8 borradas (exactamente los ocho crates del
  workspace, ninguna dependencia tocada).
- `cargo build --release --locked --offline -p rust-engineering-mcp` limpio;
  `target/release/rust-engineering-mcp version --json` → `"version":"0.9.0-rc.1"`.
- `test-release-artifact.py` 12/12, `test-release-smoke.py` 11/11 (positivos
  `v0.9.0-rc.1`; negativos `v0.9.0-rc`, `v0.9.0-rc.01`, `v0.9.0-beta.1`,
  `v0.9.0-rc.1+build`).
- `contract-freeze.py verify --strict` → `passed`: **el freeze 0.8.0 sigue siendo
  el oráculo y ningún contrato cambia** (la versión no forma parte del contrato).
- Ensayo local completo del worker (archive + smoke desde directorio limpio) con
  el tag `v0.9.0-rc.1`: `passed`, 11 miembros, 221 paquetes, 36 tools.
- `validate-ref` del workflow y los mensajes de error actualizados; el resto del
  workflow (pins, permisos, digests cruzados, `--draft --prerelease`) intacto.
- Docs: README, `compatibility.md`, `publication.md`, ADR-047 (enmienda datada),
  ADR-090 y CHANGELOG describen el esquema `-rc.N` y que un RC **nunca** es una
  release soportada.

Efecto colateral detectado por el orquestador y corregido en
[W41](../W41-harness-version-source/prompt-header.md): dos arneses host-only
(`test-m8-clients.py`, `test-m8-rollback.py`) exigían el literal `0.8.0` al binario
del árbol.
