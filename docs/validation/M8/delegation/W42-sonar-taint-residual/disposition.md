# W42 — disposición del orquestador

Invocación: `claude -p --model sonnet --effort high --permission-mode acceptEdits --disallowedTools Agent Task --allowedTools <lista> --no-session-persistence --output-format json < prompt-header.md` (CLI 2.1.274). Inicio 2026-09-18T12:40:54Z, fin 12:52:21Z, exit 0, 78 turnos, 686 227 ms, modelos `claude-sonnet-5` (+ auxiliar `claude-haiku-4-5`), `permission_denials: 8` (ocho Bash fuera del allowlist —heredocs, cadenas con `&&`, scripts propios en `target/`—; el worker rehízo cada comprobación con comandos admitidos).

## Aceptado

Los cinco archivos tocados están dentro de los permitidos; `scripts/test-m8-rollback-unit.py` no hizo falta. El worker **no** tocó `sonar-project.properties` ni excluyó nada, como se le exigió.

Verificación repetida por el orquestador sobre el árbol resultante (no delegada):

| Comprobación | Resultado |
| --- | --- |
| `test-contract-freeze.py` | 26/26 OK |
| `test-m8-performance-unit.py` | 81/81 OK |
| `test-m8-rollback-unit.py` | 42/42 OK |
| `test-m8-clients-unit.py` | 130/130 OK |
| `contract-freeze.py verify --strict` | `status: passed`, 0 cambios en las cuatro clases |
| `docs-hygiene.py links-check` | 0 rotos en documentos vivos |

**Restricción del oráculo, verificada por el orquestador, no por el worker.** El worker informó «byte-identical (only timestamp/head_commit differ)», que es una afirmación más débil que la exigida. Comprobado a mano: copia del manifiesto → `generate` → comparación campo a campo → restauración con `git checkout`. Resultado: los únicos campos que difieren son los de procedencia (`generated_utc`, `head_commit`, `tree_dirty`); `tools` es idéntico objeto a objeto, `tool_count` 36, `stable_count` 31, `preview_count` 5 y `canonical` idénticos. `docs/validation/M8/freeze-0.8.0.json` queda sin modificar en el árbol.

## Criterio sobre dos decisiones de diseño del worker

1. **`TOOL_NAME_PATTERN` (gramática) en vez de una lista cerrada de los 36 nombres.** El encargo pedía «conjunto cerrado de nombres conocidos». La gramática es marginalmente más débil, pero **se acepta**: incrustar los 36 nombres en el script crearía una segunda fuente de verdad que habría que sincronizar con el manifiesto cada vez que se añada una tool, justo el acoplamiento que V02 P3 ya registró como deuda. La gramática corta el flujo de taint igual (`match.group(0)` es un valor re-derivado) y un nombre fuera de ella falla ruidosamente.
2. **`next(choice for choice in PROFILE_CHOICES if choice == args.profile)`.** Idioma poco común, pero es el patrón canónico de re-derivación por pertenencia y lleva comentario explicando por qué `choices=` de argparse no basta. `argparse` garantiza la pertenencia, así que el `next()` sin default no puede lanzar `StopIteration`.

## Deuda menor aceptada (no bloqueante)

`tool_entry` llama `known_annotations(spec.get("annotations", {}), name)` pasando el **nombre** donde la firma anota `source: pathlib.Path`. Solo se usa para componer el mensaje de error, y el mensaje resultante (con el nombre de la tool) es de hecho más útil que con la ruta. Anotación de tipo inexacta; no se gasta un ciclo de worker en ella. Corregir cuando se vuelva a tocar el archivo.

## Pendiente de confirmación externa

El veredicto real es el quality gate del PR #22 tras el push: `new_security_rating` debe pasar de E a A con 0 vulnerabilidades `pythonsecurity:*`. Hasta leerlo en vivo, esto **no es un pass**.
