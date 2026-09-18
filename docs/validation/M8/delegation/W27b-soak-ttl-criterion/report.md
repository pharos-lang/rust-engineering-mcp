# W27b — soak: criterio `fd_after_ttl` tras TTL + una apertura (expiración perezosa)

## Cambios

`scripts/soak-m8.py`:

- `run_open_churn`: cuando `--ttl-wait-seconds > 0`, tras la espera y el
  muestreo (`fd_count_after_ttl_wait`) se hace **una apertura adicional**
  (`rust.project.open`) del mismo `path` y se vuelve a muestrear FDs en
  `fd_count_after_reclaim_open`. Con `--ttl-wait-seconds 0` ambos campos
  quedan `None` (sin cambios de comportamiento respecto al caso ya cubierto).
- `evaluate_fd_after_ttl(plateau_fds, fd_count_after_ttl_wait,
  fd_count_after_reclaim_open)`: el criterio pasa/falla ahora se evalúa sobre
  `fd_count_after_reclaim_open ≤ plateau + FD_GROWTH_MARGIN (10)`, no sobre
  la muestra tras el TTL. Se añade el campo informativo
  `retained_until_next_open = fd_count_after_ttl_wait - plateau_fds`
  (`None` si no hay muestra tras el TTL). `applicable` requiere `plateau_fds`
  y `fd_count_after_reclaim_open`.
- `run_core_soak`: la nota de `fd_after_ttl` en `notes[]` reporta ambas
  muestras (retención tras el TTL sin apertura, y el valor tras la apertura
  de recolección) y el resultado del criterio. Se añade una nota nueva que
  describe la expiración perezosa como propiedad de diseño acotada (TTL +
  límite de sesión de 16 slots, ADR-030), sin afirmar nada sobre workloads no
  medidos (muchos paths distintos, nunca reabiertos).
- Docstring del módulo actualizado para reflejar el criterio nuevo.

`scripts/test-m8-performance-unit.py`:

- `OpenChurnTests`: el test de espera existente se extendió para cubrir la
  apertura de recolección (`fd_count_after_reclaim_open`); el test de
  `ttl_wait_seconds=0` verifica que ambos campos nuevos queden `None`.
- `EvaluateFdAfterTtlTests`: series sintéticas para
  - sin datos de churn → no aplicable, pasa por defecto;
  - sin muestra de reclamo (incluye el caso `ttl_wait_seconds=0`) → no
    aplicable;
  - **caso de expiración perezosa observado en calibración**: muestra tras
    TTL alta (27, por encima de meseta+10) pero `fd_count_after_reclaim_open`
    dentro de margen (10) → el criterio **pasa** (antes habría fallado sobre
    la muestra tras el TTL);
  - `fd_count_after_reclaim_open` por encima de margen → falla;
  - `retained_until_next_open` es `None` cuando no hay muestra tras el TTL.

## Ejecución real

```
python3 -B scripts/soak-m8.py --profile core --cycles 20 --hours 0.05 \
  --sample-every 5 --out target/m8-soak-smoke-2.json
```

Resultado: `PASSED m8 soak (core) written to .../target/m8-soak-smoke-2.json`.

`criteria` (recibo completo):

```json
{
  "rss_growth": { "passed": true, "plateau_mib": 28.27, "final_mib": 15.5, "limit_mib": 33.92 },
  "fd_growth": { "passed": true, "plateau_fds": 8, "final_fds": 8, "limit_fds": 18 },
  "state_root_orphans": { "applicable": false, "passed": true },
  "cycle_overrun": { "passed": true, "count": 0 },
  "fd_after_ttl": {
    "applicable": true,
    "plateau_fds": 8,
    "limit_fds": 18,
    "measured_fds": 8,
    "retained_until_next_open": 19,
    "passed": true
  },
  "catalog_store_orphans": { "passed": true, "orphans": [] }
}
```

`open_churn`:

```json
{
  "count": 20,
  "ttl_wait_seconds": 35.0,
  "fd_count_before": 8,
  "fd_count_after": 27,
  "fd_count_after_ttl_wait": 27,
  "fd_count_after_reclaim_open": 8
}
```

`status`: **`passed`**.

La medición reproduce el patrón del experimento del orquestador (`docs/validation/M8/05.md`
"Hallazgo del soak": 29 → 29 tras espera → 10 tras una apertura más): aquí,
27 tras el TTL (sin recolección) → **8** tras la apertura de recolección,
por debajo del límite de meseta+10 (18). `retained_until_next_open = 19`
queda registrado como evidencia informativa de la retención acotada por el
TTL, sin participar en el pasa/falla.

## Unit tests

```
python3 -B scripts/test-m8-performance-unit.py
```

`Ran 78 tests in 0.007s` — `OK`.

## No commit

Sin cambios de git realizados; archivos tocados: `scripts/soak-m8.py`,
`scripts/test-m8-performance-unit.py`, este reporte.
