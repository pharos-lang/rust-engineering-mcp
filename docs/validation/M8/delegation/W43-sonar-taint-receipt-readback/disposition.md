# W43 — disposición del orquestador

Invocación: `claude -p --model sonnet --effort high --permission-mode acceptEdits --disallowedTools Agent Task --allowedTools <lista> --no-session-persistence --output-format json < prompt-header.md` (CLI 2.1.274). Inicio 2026-09-18T13:18:36Z, fin 13:24:13Z, exit 0, 51 turnos, 336 625 ms, modelos `claude-sonnet-5` (+ auxiliar `claude-haiku-4-5`), `permission_denials: 7` (siete Bash fuera del allowlist —`py_compile`, cadenas con `&&`, copias a `/tmp`—; el worker rehízo las comprobaciones con comandos admitidos).

## Aceptado

Cuatro archivos tocados, todos dentro de los permitidos. No tocó `sonar-project.properties`, no añadió `# NOSONAR` ni excluyó nada.

Verificación repetida por el orquestador sobre el árbol resultante:

| Comprobación | Antes de W43 | Después |
| --- | --- | --- |
| `test-contract-freeze.py` | 26/26 | **31/31 OK** |
| `test-m8-performance-unit.py` | 81/81 | **85/85 OK** |
| `test-m8-rollback-unit.py` | 42/42 | 42/42 OK |
| `test-m8-clients-unit.py` | 130/130 | 130/130 OK |
| `contract-freeze.py verify --strict` | passed | `passed`, 0 cambios en las cuatro clases |
| `docs-hygiene.py links-check` | 0 rotos | 0 rotos en documentos vivos |

Los 9 tests nuevos cubren los caminos de rechazo introducidos (clave desconocida en el archivo de diff, campo con tipo incorrecto, `magnitude_id`/`verdict`/`budgets_sha256`/`profile` fuera de su dominio cerrado), como se exigió: un fallo ruidoso sin test no cuenta como hecho.

**Invariante del oráculo verificada por el orquestador** (segunda vez en la sesión): copia → `generate` → comparación campo a campo → `git checkout`. Solo difieren los campos de procedencia; `tools` idéntico objeto a objeto. `docs/validation/M8/freeze-0.8.0.json` queda sin modificar en el árbol.

## Criterio

El cambio va más allá de callar al analizador: los dos scripts pasan a **validar sus propios recibos** al releerlos en vez de confiar en que nadie los haya editado. Un `02-schema-diff.json` o un recibo de rendimiento corrupto ahora falla ruidosamente en vez de propagarse a la salida. Eso es una mejora de robustez que se sostiene sola, con o sin SonarCloud.

## Pendiente de confirmación externa

El veredicto real es el quality gate del PR #22 tras el push: `new_security_rating` debe pasar de E a A con 0 vulnerabilidades `pythonsecurity:*`. Hasta leerlo en vivo, esto **no es un pass**. Serie de la deuda de taint: 22 (W38) → 6 (W42) → 2 (W43) → pendiente.
