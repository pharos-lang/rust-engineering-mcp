# Backup y recuperación

Qué respaldar, cómo recuperar cada store ante corrupción o interrupción, y la
regla real de compatibilidad entre versiones del binario y el estado que deja
en disco. No existe un subcomando `backup`/`restore` dedicado en ningún
store: backup/restore es un procedimiento operativo con herramientas
ordinarias del sistema de archivos, descrito abajo.

## Qué respaldar

Todo el estado de servidor que sobrevive a un reinicio vive bajo rutas que tú
eliges por flag — no hay un layout por defecto (ver
[`../guides/configuration.md`](../guides/configuration.md#rutas-de-datos)):

| Ruta (flag de host) | Contenido |
| --- | --- |
| `--state-root` | `rust-mcp-mutations-v1` (journal de mutación) y, si usas tools de calidad M3+, `rust-mcp-quality-artifacts-v1` |
| `--catalog-store` / `--catalog-trust` | Catálogo SQLite autoritativo y el par de confianza Ed25519 con su floor de secuencia |
| `--catalog-index-store` (opcional) | Índice LanceDB derivado — siempre reconstruible, no imprescindible de respaldar |

Las entradas de host puro (`--security-policy`, `--rustsec-snapshot`,
`--cargo-vendor-dir`, `--vendor-capture`) no son estado de servidor: se
re-suministran por flag en cada arranque. Respaldarlas es responsabilidad
del operador como cualquier otro archivo de configuración, pero su pérdida
no corrompe ningún store — simplemente hace que la tool que las consume
responda `unavailable` hasta que se reconfiguren.

## Procedimiento de backup/restore (operativo, sin CLI dedicada)

1. Detén el servidor (`serve --stdio`).
2. Copia `--state-root`, `--catalog-store` y `--catalog-trust` como árboles
   de archivos ordinarios (por ejemplo `cp -a` o tu herramienta de backup
   habitual, preservando permisos 0700/0600).
3. Antes de reanudar tráfico, valida la copia con
   `rust-engineering-mcp doctor --json [las mismas flags de host]`
   (ver [`../reference/cli.md`](../reference/cli.md)).
4. Reanuda `serve --stdio` normalmente.

Un backup restaurado **no autoriza sobreescribir cambios posteriores** del
workspace del usuario: el journal de mutación M2 los detecta como
`Conflict`, exactamente igual que cualquier otra escritura externa
concurrente. Restaurar un backup de `--state-root` no es una operación
privilegiada frente al modelo de confianza `local_coordinated` — ver
[`../architecture/mutation.md`](../architecture/mutation.md).

## Journal de mutación: recuperación

El journal (`rust-mcp-mutations-v1`) es la fuente autoritativa del ciclo
`preview`/`commit`/`receipt`. Recuperación disponible:

- **`rust.manifest.patch`/`.fmt.apply`/`.fix.apply`/`.dependency.add`/`.remove`/`.analyzer.action.apply`**,
  llamada `receipt` con `recover: true` — recuperación conservadora,
  opt-in explícito del cliente MCP; nunca automática en la ruta normal de
  lectura.
- **`rust-engineering-mcp mutation list --state-root PATH [--json]`** — lista
  journals por fase (pendiente/terminal) sin modificar nada.
- **`rust-engineering-mcp mutation prune --state-root PATH --operation-id ID --plan-digest sha256:ID [--json]`** —
  retira un recibo terminal ya completado.

Un journal corrupto o con un `format`/`operation_kind` desconocido bloquea el
**store compartido completo** (list/prune/commits fallan cerrado) hasta
remediación explícita — nunca se reinterpreta con un default silencioso. La
remediación documentada es: crear una **nueva** `--state-root` y copiar los
journals sanos hacia ella; los originales en cuarentena nunca se tocan
in-place. Ver el mecanismo de detección en
[`../reference/data-formats.md`](../reference/data-formats.md#journal-de-mutación-rust-mcp-mutations-v1).

### `doctor` y visibilidad de journals pendientes antes de un downgrade

`doctor --json --state-root PATH` (aceptando **solo** `--state-root`, sin el
resto de la tupla Docker, como única excepción entre todas las secciones de
`doctor`) publica `mutation_journals`: conteos `pending`/`terminal`/`kinds` y
`downgrade_blocked`:

```text
downgrade_blocked = pending > 0 ∨ existe algún kind fuera de los cinco
                     que reconoce v0.3.0 (manifest_patch, format_apply,
                     fix_apply, dependency_add, dependency_remove)
```

Un journal `analyzer_action_apply` (kind nuevo de `0.8.0`) bloquea el
downgrade a `v0.3.0` **aunque ya esté comprometido (`committed`)**, porque un
binario `v0.3.0` lo rechaza al leerlo sin importar su fase. Este chequeo de
`doctor` **no es puramente read-only**: abre el store (crea su lock file si
falta) y toma un `flock` no bloqueante — una mutación concurrente de `serve`
puede recibir `Busy` en la sección `mutation_journals` en vez de bloquearse
esperando.

**`serve` nunca bloquea su propio arranque por un journal pendiente o
desconocido** — sería una denegación de servicio contra tráfico no
relacionado con ese journal. La única protección proactiva es reactiva por
registro: cualquier tool que intente leer/journalizar sobre ese registro
concreto falla con `RecoveryRequired`, antes de tocar el workspace. La
sección `mutation_journals` de `doctor` es la forma en que el **operador**
obtiene visibilidad antes de decidir instalar un binario más antiguo.

## Store de quality artifacts (M3+): recuperación

```text
rust-engineering-mcp quality-artifacts recover --state-root PATH [--json]
rust-engineering-mcp quality-artifacts prune --state-root PATH [--json]
```

`recover` pone en cuarentena objetos corruptos o de `format_version`
desconocida sin reinterpretarlos nunca con un default; `prune` elimina
artifacts expirados por TTL (máximo 86 400 s, ver
[`../reference/limits.md`](../reference/limits.md)). Cuotas
reject-before-produce: una publicación que excede su cuota se rechaza, nunca
desaloja evidencia ya publicada — no hay "backup" separado de este store más
allá de copiar `--state-root` como se describe arriba.

## Catálogo: recuperación

No hay subcomando `catalog backup`. La guía explícita del propio CLI para un
floor de secuencia inválido o ausente es: *"Retained sequence state is
invalid or missing; restore trusted state from a verified backup without
resetting its floor"* — es decir, la recuperación asume que el operador
conserva una copia externa del bundle verificado para reimportar con
`catalog import`/`sync`. Ver
[`catalog-maintenance.md`](catalog-maintenance.md) para los subcomandos y
[`../reference/data-formats.md`](../reference/data-formats.md) para el
mecanismo de floor/activación atómica.

## Migración y rollback de versión (0.3.0 → 0.8.0)

Resumen operativo — detalle completo y por-formato en
[`../reference/data-formats.md`](../reference/data-formats.md#entradas-de-host-sin-estado-de-servidor):
comparando bytes reales (`git diff v0.3.0 HEAD`), 8 de los 10 formatos en
disco del proyecto son byte-idénticos desde `v0.3.0`; los 2 restantes (host
config, journal de mutación) cambiaron de forma estrictamente aditiva.
**No existe hoy ningún formato real que exija una migración `0.3.0 → 0.8.0`**,
por lo que no se implementó un CLI de migración nuevo — la decisión fue
medir primero y no construir una herramienta para un problema inexistente.

Verificación de esa comparación (por unidad, no end-to-end contra un binario
`v0.3.0` real instalado): `scripts/test-m8-rollback-unit.py` prueba en
aislamiento las funciones puras de la lógica de rollback descrita arriba
(la regla `downgrade_blocked` y el sniff de formato). Su estado de
integración con el gate automatizado está en
[`../development/testing.md`](../development/testing.md); a la fecha de este
documento, ese script y `scripts/test-m8-rollback.py` (equivalente contra el
binario real) existen en el repositorio pero **no están wireados como etapa
de `scripts/gate.py`** — ejecútalos manualmente si necesitas esa evidencia:

```sh
python3 -B scripts/test-m8-rollback-unit.py
python3 -B scripts/test-m8-rollback.py   # requiere binario release real
```

## Limitaciones conocidas

- No hay bloqueo proactivo de downgrade a nivel de proceso para el journal de
  mutación; la protección es enteramente reactiva por registro (deliberado,
  para no convertir un journal ajeno en una denegación de servicio contra
  tráfico no relacionado).
- **Demostrado con binarios reales `v0.3.0` ↔ `0.8.0` (4/4 escenarios), no
  solo por unidad.** Además de la comparación por unidad de arriba, los
  cuatro escenarios a–d de
  [`03-rollback.json`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/validation/M8/03-rollback.json)
  ejecutaron los dos binarios reales (`v0.3.0` y `0.8.0`) uno contra el
  estado que escribió el otro — journal de mutación con un kind desconocido
  para `v0.3.0`, artifact de calidad M3, floor de trust-bundle y catálogo —
  y los cuatro pasaron (`status: "passed"`); ver también la fila 9 de
  [`docs/validation/M8/checklist-1.0.md` en `51fa602e`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/validation/M8/checklist-1.0.md).
  Este ensayo es manual, fuera de `core`/`full` (RR-17 en
  [`../architecture/execution-and-security.md`](../architecture/execution-and-security.md#registro-de-riesgos-residuales-de-10)),
  no una etapa automatizada del gate; sus dos huecos declarados (R-7/R-2
  sobre journals/artifacts *escritos por* 0.3.0, no al revés) siguen
  vigentes.
- **Existe un fixture de "permisos revocados a mitad de una operación".**
  `crates/project-adapter/tests/support/native_mutation.rs:3279`
  (`revoked_destination_permissions_leave_a_recoverable_non_terminal_journal`)
  cubre ese caso para el journal de mutación (ADR-088 §8).
- La supervivencia a una pérdida de energía real durante una escritura del
  journal no está demostrada — solo se probó inyección de ENOSPC (disco
  lleno). Ver [`../reference/compatibility.md`](../reference/compatibility.md#limitaciones-documentadas).
