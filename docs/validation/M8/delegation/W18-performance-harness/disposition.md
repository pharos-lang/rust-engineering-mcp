# W18 — disposición del orquestador (2026-09-14)

Veredicto: **aceptado con incidencia de sesión** (el worker dejó la calibración
del soak en segundo plano y terminó sin informe; el orquestador verifica los
entregables directamente). Verificado: `test-m8-performance-unit.py` verde;
`05-budgets.json` con los valores de `05.md`; etapa `m8-performance-unit-tests`
en `gate.py` y línea de coverage en `sonarcloud.yml`; recibo
`05-measurement.json` (perfil `core`, N=30, binario 0.8.0 `release`): startup
cold p95 24,1 ms (presupuesto 500), warm 3,8 ms (100), dispatch
`project.open` 0,38 ms / `catalog.status` 0,06 ms (50), RSS idle 24,9 MiB
(128) / pico 24,8 MiB (256), binario 30 492 464 B (43 MB) — todo `within`;
Docker/`local`/archive `unavailable` con motivo (los mide el orquestador en
M8-09 y en el release). **Nota de ruido**: esta primera medición corrió con
otros workers compilando en el host; sirve para validar el arnés, y la
recalibración única de `05.md` se hace sobre una medición en host quieto
(M8-09/RC1), no sobre esta.
