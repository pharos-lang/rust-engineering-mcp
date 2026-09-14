# R01 — intentos de invocación

Registro honesto de cada invocación real. Los transcripts crudos quedan fuera
del árbol (scratchpad de la sesión); aquí se anota su SHA-256.

| # | Fecha (UTC) | Comando | Resultado | Evidencia |
| --- | --- | --- | --- | --- |
| 0 (sonda) | 2026-09-11 | `agy --model gemini-3.8-flash-high --print-timeout 3m --output-format json -p='<sonda read_url>'` | `status: SUCCESS`, `response: ""`, `denied_actions: [read_url]`. En modo `-p` sin regla de permiso, `agy` 1.2.0 deniega `read_url` automáticamente y no produce salida | transcript `agy-probe.json` sha256 `4e99b63a44ef16e9cc07fc36fdf9cb05125e2d20651b99ac0f7890ed5030c072` (fuera del árbol); 14 481 tokens |
| 1 | 2026-09-11 | `agy --model gemini-3.8-flash-high --effort high --sandbox --dangerously-skip-permissions --print-timeout 40m --output-format json -p="$(cat prompt.md)"` (en segundo plano, cwd scratchpad sin acceso al repo) | **No ejecutado**: el clasificador de auto-mode del host (Claude Code) bloqueó la orden antes de lanzarla | sin transcript |
| 2 (sonda) | 2026-09-11 | `agy --model gemini-3.8-flash-high --mode plan --print-timeout 3m --output-format json -p='<sonda read_url>'` | **No ejecutado**: bloqueado por el mismo clasificador | sin transcript |

Estado: **pendiente de decisión del owner** sobre cómo conceder a `agy` el
permiso `read_url` (regla `permissions.allow` en su `settings.json`, o
autorizar expresamente la invocación `--sandbox --dangerously-skip-permissions`
desde un cwd sin acceso al repositorio). Sin acceso web, Gemini solo podría
responder desde memoria, que el paquete prohíbe expresamente («never fill the
gap from memory»); no se lanza una investigación que no pueda citar fuentes.

## Resolución (2026-09-11, misma sesión)

La regla `read_url(*)` ya estaba en los permisos de proyecto de `agy`
(`~/.gemini/config/projects/default-cli-project.json`, junto a `command(*)`);
los intentos 0–2 la ignoraban porque el cwd era el scratchpad, fuera del
proyecto. Sondas desde `/Users/cburgosro/Projects/rust-mcp`: sin `--sandbox`
y con `--sandbox`, ambas `SUCCESS` con `denied_actions: None` y respuesta real
(`rust-analyzer.procMacro.enable`).

| # | Fecha (UTC) | Comando | Resultado |
| --- | --- | --- | --- |
| 3 | 2026-09-11 22:12–22:22 UTC | `agy --model gemini-3.8-flash-high --effort high --sandbox --print-timeout 45m --output-format json -p="$(cat prompt-header.md)"` desde el repo (`agy` 1.2.0) | `SUCCESS`, `denied_actions: None`, 586,9 s, 1 192 004 tokens (24 fuentes primarias citadas). Informe en [report.md](report.md); disposición en [disposition.md](disposition.md); hash del transcript en `transcripts.sha256` |
