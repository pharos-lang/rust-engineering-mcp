# W38 — disposición del orquestador (2026-09-17)

Invocación: `claude -p --model sonnet --effort high --permission-mode acceptEdits --disallowedTools Agent Task --allowedTools <lista> --no-session-persistence --output-format json < prompt-header.md` (CLI 2.1.274). Inicio 2026-09-17T17:19:15Z, fin 17:34:40Z, exit 0, 106 turnos, 924 175 ms, modelos `claude-sonnet-5` (+ auxiliar `claude-haiku-4-5`), `permission_denials: 4` (cuatro Bash fuera del allowlist —heredoc, `rm` en `/tmp`, cadenas con `&&`—; el worker repitió cada verificación con comandos admitidos). Relanzamiento tras la pérdida del worker original por reinicio del host (2026-09-17); el prompt no cambió.

Veredicto: **aceptado**. Verificación del orquestador sobre el árbol resultante:
`test-contract-freeze.py` 26/26, `test-m8-performance-unit.py` 81/81,
`test-gate-reporting.py` 13/13 (regla que prohíbe excluir `crates/**`),
`contract-freeze.py verify --strict` → `status: passed`, `docs-hygiene.py
links-check` 0 rotos en documentos vivos, `verify-inventories` 0 fallos.
Mapa hallazgo → cambio en [report.md](report.md): los 22 sitios señalados
(`contract-freeze.py` 77/140/141/154/296/297, `measure-m8-performance.py`
80/86/92/147/611/627/628/748/749, `soak-m8.py` 90/239/250/647/648) dejan de
recibir rutas o argv desde la CLI; `diff` toma `{base, only, out}` por stdin con
`base` validado por regex y `out` como clave de un diccionario constante.
Exclusiones de cobertura añadidas: `measure-m8-performance.py`, `soak-m8.py`,
`test-m8-rollback.py` (`test-m8-clients.py` y `m8-inspector-session.mjs` ya
estaban); `contract-freeze.py` sigue medido. El resultado real del quality gate
se comprueba en SonarCloud tras el push (registro en el README de delegación).

Efectos sobre la operación de cierre (asumidos por el orquestador):

- `gate.py` etapa `contract-freeze` llama `verify` sin ruta (ajustado por W38).
- El soak de cierre se invoca `scripts/soak-m8.py --profile core --cycles 1000
  --hours 8 --sample-every 20` y escribe **siempre** `docs/validation/M8/05-soak-core.json`;
  `measure-m8-performance.py` escribe siempre `05-measurement.json` (una prueba
  corta sobrescribiría el recibo: no se hacen pruebas cortas en el árbol de cierre).
- `measure-m8-performance.py --profile local` perdió los flags `--catalog-model-dir`/
  `--catalog-index-store` (tiempo de búsqueda semántica E5/ORT); el arnés solo medía
  el modo léxico, así que ninguna magnitud registrada cambia. Anotado como deuda.
- `docs/ci.md:378` citaba la invocación antigua de `verify` con ruta: corregido por
  el orquestador (una línea de documentación).
