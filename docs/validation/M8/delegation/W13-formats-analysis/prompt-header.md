# W13 — M8-03 (preparación D12): análisis de formatos en disco y marcadores de versión

Modelo solicitado: Claude Sonnet 5 (`claude -p --model sonnet --effort high`). Rol: worker de análisis (solo lectura del código; escribe **un** documento). Orquestador: Claude Fable 5.1. No puedes lanzar subagentes. **Nunca corras comandos en segundo plano.** No hagas commit. No hagas Docker. **Único archivo que puedes escribir:** `docs/validation/M8/03-formats-analysis.md`.

## Contexto

Rama `ai/m8-stabilization` (M8-02 freeze 0.8.0 commiteado; tip `ea3bf2a`). M8-03 (plan `docs/roadmap/m8-stabilization.md` §M8-03 y §«Migración, recovery y seguridad») exige: censo de formatos con reader/writer, preflight/dry-run, backup/staging, verificación y receipt; floors/revocaciones/trust no retroceden; journal pendiente impide downgrade; estado corrupto exige recuperación explícita; upgrade N-1→N y rollback N→N-1 probados por formato compatible; **«CLI de migración/validación solo se añade si el inventario identifica un formato real que migrar»**. D12 (`docs/roadmap/adr-backlog-m2-m8.md` §D12) está Proposed. El censo M8-01 (`docs/validation/M8/01-census.json` → `disk_formats[]`, 10 formatos) ya identifica ubicación, reader/writer, ADR y tests por formato; el finding F7 señala que el journal M2 no tiene `format_version` en registro.

## Tarea

Para **cada uno de los 10 formatos** del censo determina, leyendo el código (`crates/project-adapter`, `crates/artifact-adapter`, `crates/catalog-adapter`, `crates/semantic-adapter`, `crates/execution-adapter`, `crates/mcp-server/src/host_config.rs`, …) y `git diff v0.3.0 HEAD -- <rutas>`:

1. **Marcador de versión**: cómo detecta el lector la versión (campo `format_version`/`schema_version`, nombre de directorio, tabla `PRAGMA user_version`/tabla de migraciones SQLite, cabecera del archivo, ninguno). Cita archivo:línea.
2. **Comportamiento ante versión desconocida/futura** (fail-closed antes de efectos, ignora, sobrescribe): cita.
3. **¿Cambió el formato entre `v0.3.0` y HEAD?** (`git diff v0.3.0 HEAD` sobre los módulos reader/writer y sus tests; distingue cambio de bytes/estructura frente a refactor). Conclusión: `unchanged` / `changed-compatible` / `changed-incompatible`, con evidencia.
4. **Estado que no debe retroceder** (floor antirollback del catálogo, revocaciones/trust, grants, journal pendiente): dónde vive y cómo se protege hoy; qué pasa en un downgrade de binario 0.8.0 → 0.3.0 con ese estado.
5. **Recuperación**: qué CLI/tool existente cubre backup/restore/recover/prune para ese formato (`quality-artifacts recover/prune`, `mutation list/prune`, `catalog …`, `doctor`), y qué falta.
6. **Tests existentes** de upgrade/rollback/versión desconocida/corrupción (cita nombres de test).

Cierra con: (a) tabla resumen 10 × {marcador, unknown-version, cambio desde 0.3.0, no-retroceso, recuperación, tests}; (b) lista de **formatos que realmente requieren migración** 0.3.0 → 0.8.0 (esperado: pocos o ninguno — no lo fuerces); (c) huecos para D12 en orden de riesgo (p. ej. formatos sin marcador, sin fail-closed ante versión desconocida, sin test de rollback) con la propuesta mínima por hueco y su coste relativo (S/M/L); (d) qué fixtures de la lista del plan (versión desconocida, config antigua, migración interrumpida, disk full, permisos revocados, backup corrupto, inode/ancestor swapped, symlink/reparse/hardlink, crash tras commit, reintento idempotente) ya existen en el repo (cita rutas) y cuáles faltan. Sin inventar rutas: si no encuentras algo, dilo.

## Verificación

`python3 -B scripts/docs-hygiene.py links-check` → 0 rotos. Escribe el informe final también como última respuesta (Task / Result / Files changed / Conteos / Risks / Open issues). No commit.
