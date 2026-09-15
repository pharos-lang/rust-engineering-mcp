# ADR-088 — Política de migración, downgrade y backup/restore (0.3.0 → 0.8.0)

Date: 2026-09-14

## Context

D12 (`docs/roadmap/adr-backlog-m2-m8.md` §D12) exige, para M8-03, formatos
versionados, preflight/backup y upgrade/rollback explícitos, con evidencia de
versión desconocida, crash, disk full, floor antirollback y journal
pendiente. El worker W13 comparó por archivo (`git diff v0.3.0 HEAD`) los diez
formatos en disco del censo M8-01
([`01-census.json`](../validation/M8/01-census.json) `disk_formats[]`)
contra el mismo inventario en `v0.3.0`
([`03-formats-analysis.md`](../validation/M8/03-formats-analysis.md)). El
resultado: ningún formato requiere una migración de bytes 0.3.0 → 0.8.0 — ocho
son byte-idénticos y los otros dos (host config, journal M2) cambiaron de
forma estrictamente aditiva (§(b) del análisis). Esta decisión, tomada por el
orquestador bajo la autorización del owner en sesión (2026-09-14, continuar
M8 asumiendo las decisiones necesarias para una primera versión estable en
macOS), se registra en
[`docs/validation/M8/03.md`](../validation/M8/03.md) §D12 (8 puntos) y se
materializa aquí sin reinterpretarla.

## Decision

1. **No se añade un CLI de migración en M8.** El análisis §(b) confirma que
   ningún formato en disco tiene bytes que migrar entre `0.3.0` y `0.8.0`; el
   plan prohíbe comandos vacíos por aspiración.
2. **Política de formatos por dos categorías.** Todo formato **con estado del
   servidor** (journal/receipts M2, artifacts M3, catálogo SQLite, bundle de
   confianza + floor, índice LanceDB) lleva marcador de versión propio y
   falla cerrado antes de cualquier efecto ante una versión desconocida — ya
   cumplido por los cinco, con evidencia citada en el análisis. Los **inputs
   de host** (config de `serve`, política de seguridad, snapshot RustSec,
   vendor tree de Cargo) se verifican por pin SHA-256 o schema en cada
   invocación y no tienen estado de servidor que pueda migrar o retroceder.
   El censo se corrige: el snapshot RustSec y el vendor tree pasan de
   `floor_or_trust_state: true` a `false`, con una nota que distingue
   integridad puntual re-suministrada de un floor de secuencia persistido
   (único floor genuino del censo: el bundle de confianza del catálogo,
   §6 del análisis).
3. **Downgrade con journal pendiente o de kind desconocido para 0.3.0.** La
   protección existente es fail-closed por registro: un binario anterior que
   no reconoce una variante de operación (p. ej. `analyzer_action_apply`)
   devuelve `RecoveryRequired` antes de tocar el workspace o el store — sea
   cual sea la fase del journal en el binario que lo escribió, porque el
   binario anterior nunca llega a interpretar el campo `operation`. Se añade
   un preflight pasivo en `doctor`, sección `mutation_journals`, que lista
   los journals por fase y kind (`kinds{kind → pending, terminal}`) —
   incluidas las entradas de formato o kind desconocido para este binario
   (`unknown_format`) — para que el operador vea el bloqueo antes de
   intentar un downgrade.
   `downgrade_blocked` es `pending > 0 ∨ existe un registro cuyo kind no está
   entre los cinco que `0.3.0` reconoce (`manifest_patch`, `format_apply`,
   `fix_apply`, `dependency_add`, `dependency_remove`)`: un journal
   `analyzer_action_apply` ya **committed** por `0.8.0` bloquea el downgrade
   igual que uno pendiente, porque `0.3.0` lo rechaza en cuanto lo lee,
   independientemente de su fase; `downgrade_blocking_kinds` nombra esos
   kinds. `serve` no bloquea el arranque por un journal pendiente o de kind
   desconocido: bloquear ahí sería una denegación de servicio activada por un
   journal ajeno a la operación que el cliente intenta realizar.
4. **Compatibilidad diferenciada por TTL.** Los formatos sin TTL (journal,
   catálogo, floor del bundle) exigen lector legado más migración explícita,
   igual que el patrón v1→v2 ya probado del journal M2. Los formatos con TTL
   (artifacts M3) pueden retirar lectores legados con fail-closed más
   expiración; la asimetría actual (el bump histórico `BenchmarkDatasetV2`
   rompió sin lector legado) se acepta como política, no como deuda.
5. **El snapshot RustSec sigue degradando, no bloqueando, ante edad
   desconocida o stale.** Cambiar `AuditIssue::SnapshotUnknownAge`/`Stale` a
   un error duro rompería el contrato M1 congelado de
   `rust.dependencies.audit` ([ADR-086](ADR-086-deprecation-and-freeze-policy.md)
   §4); la frescura se comunica al llamador, que decide.
6. **Rollback y upgrade se prueban una vez con dos binarios reales.** Se
   construye `v0.3.0` desde el tag en un worktree separado y se ejecuta junto
   al `0.8.0` actual sobre estado real: (a) un journal pendiente
   `analyzer_action_apply` escrito por `0.8.0` hace que `0.3.0 mutation list`
   falle cerrado; (b) estado M3/catálogo escrito por `0.8.0` es legible por
   `0.3.0` (formatos idénticos, §(b)); (c) un bundle de secuencia menor es
   rechazado por el floor persistido incluso leído por `0.3.0` (el floor no
   retrocede); (d) estado escrito por `0.3.0` es legible por `0.8.0`
   (upgrade). El resultado se registra en un recibo dedicado,
   `docs/validation/M8/03-rollback.json`, generado por W15; esta prueba no
   entra en el gate `core` porque requiere construir un segundo binario.
7. **Backup y restore sin CLI nueva.** El procedimiento es operativo: parar el
   servidor, copiar `--state-root`, `--catalog-store` y `--catalog-trust`
   como árboles de archivos ordinarios, y validar el estado restaurado con
   `doctor` antes de reanudar `serve`. Un backup restaurado no autoriza
   sobrescribir cambios posteriores del usuario en el workspace: el journal
   M2 los detecta como `Conflict` igual que cualquier otra escritura externa
   concurrente, porque el backup no es más que un estado de servidor anterior
   restaurado sobre un workspace que pudo seguir cambiando.
8. **Fixtures faltantes.** Se añade un fixture de permisos revocados a mitad
   de operación del journal M2 — tras `Published` (el swap ya es durable) y
   antes de que el commit complete: en ese punto el store necesita permisos
   de escritura en el workspace para retirar el clon temporal ya huérfano y
   persistir la transición `Committed`, así que la revocación deja un
   journal `RecoveryRequired` recuperable en vez de perder o corromper la
   fuente (`revoked_destination_permissions_leave_a_recoverable_non_terminal_journal`,
   `crates/project-adapter/tests/support/native_mutation.rs`) — hueco real
   identificado por el análisis. «Crash tras commit antes de responder al
   cliente» queda cubierto a nivel de journal/filesystem por los tests de
   kill de proceso existentes y se declara así, sin fixture adicional a nivel
   de protocolo. «Backup corrupto» no es un concepto distinto de «estado
   corrupto»: un backup restaurado que resulte corrupto se comporta como
   cualquier estado corrupto en cuarentena, ya cubierto por los tests
   existentes de M2 y M3.

## Alternatives considered

- **CLI de migración genérica ahora.** Descartada: no existe un formato real
  que migrar entre `0.3.0` y `0.8.0` (análisis §(b)); el plan prohíbe
  comandos vacíos por aspiración, y añadir uno crearía superficie de
  contrato sin consumidor ni caso de uso verificable.
- **Bloquear `serve` cuando hay un journal de mutación pendiente.**
  Descartada: convierte un journal ajeno a la operación solicitada en una
  denegación de servicio para el resto del tráfico del servidor. La
  visibilidad en `doctor` y el fail-closed por registro ya existente cumplen
  el mandato «journal pendiente impide downgrade» sin ese coste.
- **Bloquear en snapshot RustSec stale/unknown age.** Descartada: el
  contrato de `rust.dependencies.audit` está congelado desde M1
  ([ADR-086](ADR-086-deprecation-and-freeze-policy.md) §4); cambiar su
  severidad de degradación a error duro sería una ruptura de contrato fuera
  del alcance de M8-03.

## Consequences

- El censo M8-01 (`docs/validation/M8/01-census.json`) corrige
  `floor_or_trust_state` a `false` para el snapshot RustSec y el vendor tree
  de Cargo, con nota de integridad puntual vs. floor de secuencia.
- `doctor` gana la sección `mutation_journals` (W14), preflight pasivo para
  el operador antes de un downgrade; `serve` no cambia su comportamiento de
  arranque.
- W15 produce un test nativo de rollback/upgrade con dos binarios reales y el
  recibo `docs/validation/M8/03-rollback.json`; ese recibo se referencia
  desde `docs/compatibility.md` y no entra en el gate `core`.
- `docs/compatibility.md` gana la sección «Upgrade, rollback y backup» con la
  tabla de los diez formatos y el procedimiento de rollback de binario;
  `README.md` gana un procedimiento operativo breve de backup/restore/
  rollback apoyado en `doctor`.
- No se abre ningún subcomando `catalog backup`/`mutation backup` nuevo; la
  recuperación sigue dependiendo de que el operador conserve una copia
  externa verificada, tal como documentan ya `catalog_cli.rs` y el CLI de
  mutaciones.

## Status

Accepted.

## Sources

- `docs/validation/M8/03.md` §D12 — decisión del orquestador que este ADR
  materializa (8 puntos).
- [`docs/validation/M8/03-formats-analysis.md`](../validation/M8/03-formats-analysis.md)
  (W13) — comparación de los diez formatos contra `v0.3.0`, huecos §(c) y
  fixtures §(d).
- [`docs/validation/M8/01-census.json`](../validation/M8/01-census.json)
  `disk_formats[]` — inventario de formatos y corrección de
  `floor_or_trust_state`.
- `docs/roadmap/m8-stabilization.md` §M8-03 y §«Migración, recovery y
  seguridad».
- `docs/roadmap/adr-backlog-m2-m8.md` §D12.
- [ADR-086](ADR-086-deprecation-and-freeze-policy.md) §4 — contrato M1
  congelado de `rust.dependencies.audit`, razón de la decisión 5.
