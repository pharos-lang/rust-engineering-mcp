# M3-06 — recibo de upgrade y rollback (G6)

Fecha: 2026-09-06. Alcance: **el único formato durable en disco que M3 introduce**,
el store privado de quality artifacts de ADR-061. M3 no añade otro estado
persistente, ni release, ni tag, ni paquete, ni target nuevo; los jobs y las Tasks
son session-local y no sobreviven a un restart, así que no tienen migración.

Este documento no reclama nada que no venga de un comando ejecutado en W6. Los
conteos, códigos de salida y hashes están en
[M3-06-rollback.json](M3-06-rollback.json), cuyo SHA-256 es
`70442cbeb3a600c1b0139c51530f72f9b8cb1108519d98158a5c1371668fb3ba`.

**Resultado: 10/10 selecciones, exit 0, exactamente un caso ejecutado por
selección.** Seis son nuevas de W6 y cuatro reutilizan oráculos existentes que ya
prueban su cláusula. Las diez son no-Docker y forman parte del `cargo test
--workspace` que el core y el full ejecutaron (1,072 pruebas, 0 fallos).

## Qué introduce M3 en disco

| Objeto | Ruta bajo `--state-root` | Versionado |
| --- | --- | --- |
| Store quality M3 | `rust-mcp-quality-artifacts-v1/` | En el **nombre del directorio** |
| Registros dentro del store | `descriptor/*.json`, `reservation/*.reserve`, `*.trunc`, `clock-watermark.json`, `quarantine/*.note` | Campo `format_version` en cada registro |
| Store mutation M2 | `rust-mcp-mutations-v1/` | Ajeno a M3; nunca se lee ni se modifica |

El formato vive en el nombre del directorio, de modo que una versión futura es un
**hermano** (`rust-mcp-quality-artifacts-v2/`), nunca una reinterpretación in-place
de v1. El binario v1 no lee, migra ni borra ese hermano.

## Cláusulas probadas

| Cláusula G6 | Oráculo | Estado |
| --- | --- | --- |
| Un `format_version` desconocido/más nuevo falla cerrado y no se reinterpreta | `an_unknown_record_version_fails_closed_and_is_never_reinterpreted` (nuevo, W6) | Probada |
| El watermark durable con versión desconocida bloquea quality sin re-basar nada | `an_unknown_watermark_version_blocks_quality_and_rebases_nothing` (nuevo, W6) | Probada |
| Un binario sin el store M3 deja el directorio intacto y no borra | `a_future_sibling_store_and_the_m2_journal_are_never_read_migrated_or_removed` (nuevo, W6) | Probada |
| `quality-artifacts recover`/`prune` se comportan como está documentado sobre objetos válidos, expirados y en quarantine | `operator_recover_and_prune_separate_valid_expired_and_unknown_objects` (nuevo, W6, nivel librería) y `prune_reclaims_only_expired_objects_and_recover_quarantines_the_unknown_one` + `a_clock_regression_blocks_prune_with_a_closed_code_until_recover_rebases_it` (nuevos, W6, nivel producto contra el binario) | Probada |
| El store M2 y sus journals quedan intactos | Los cinco casos nuevos afirman los bytes del hermano M2; ya lo afirmaban `corrupt_or_unknown_objects_are_quarantined_with_a_closed_reason` y `a_durable_clock_regression_blocks_only_quality_until_recovery` (existentes) | Probada |

### Reutilización explícita de oráculos existentes

No se duplicó lo que ya estaba probado. Estos casos previos cubren su cláusula y
se citan tal cual:

- `corrupt_or_unknown_objects_are_quarantined_with_a_closed_reason`: descriptor
  malformado, digest divergente y nombre desconocido pasan a quarantine con razón
  cerrada, y el hermano `rust-mcp-mutations-v1` conserva sus bytes.
- `a_durable_clock_regression_blocks_only_quality_until_recovery`: una regresión
  del reloj bloquea lectura, reserva, índice, prune y recover; `prune` nunca
  re-basa el reloj; sólo `recover` lo hace; el journal M2 no cambia.
- `expired_evidence_and_claims_stop_being_charged_and_leave_the_volume`: la
  reclamación de expirados libera cuota y volumen sin desalojar lo vivo.
- `an_operator_state_root_is_qualified_exactly_as_m2_qualifies_it`: el store M3
  acepta exactamente las raíces que M2 acepta.

## Comportamiento observado

### Registro con versión desconocida

Se publican dos pares válidos y se reserva un segundo job; después se reescribe
únicamente `"format_version":1` → `"format_version":2` en un descriptor y en un
reservation record. Sobre esos bytes:

- `recover` devuelve `validated=1`, `quarantined=2`, `discarded_uncommitted=0` y
  `clock_regression=false`;
- las tres notas de quarantine llevan `malformed_descriptor`;
- los bytes exactos v2 del descriptor y del reservation record siguen en el
  volumen dentro de `quarantine/`, junto con el blob asociado: **no se
  reescribieron a v1 ni se borraron**;
- el par v1 restante se sigue sirviendo con su contenido íntegro;
- una lectura del artifact rechazado devuelve `NotFound` enmascarado.

### Watermark con versión desconocida

Con `clock-watermark.json` en `format_version: 2`, `open`, `recover` y
`prune_expired` devuelven los tres `InvalidDescriptor`. Después de las tres
llamadas fallidas: el watermark conserva sus bytes v2, los directorios `blob`,
`descriptor` y `reservation` conservan exactamente sus entradas, `quarantine`
sigue vacío y el hermano M2 no cambia.

**Límite real, no una promesa:** este binario no repara un watermark que no
entiende. Un store cuyo watermark fue escrito por una versión futura sólo vuelve a
ser operable con el binario que lo escribió (roll-forward) o restaurando el
registro que este binario sí entiende. Se prefiere ese fallo cerrado a permitir
que una versión antigua juzgue expiraciones sobre un reloj que no puede leer.

### Hermano de formato futuro y M2

Con `rust-mcp-quality-artifacts-v2/descriptor/one.json` y
`rust-mcp-mutations-v1/{journal/000001.json,state.json}` presentes, un ciclo
completo v1 —`open`, `reserve`, `ingest`, `publish`, `read_chunk`,
`read_index_page`, `reconcile_recover`, más `recover` y `prune_expired` de
operador— deja ambos hermanos byte a byte idénticos, y el state root sigue
conteniendo exactamente los tres directorios.

### `quality-artifacts recover` y `prune`

Store de partida: un par válido con su claim viva, un par expirado con su claim
expirada, y un objeto `FOREIGN` que ninguna versión de este store escribió.

| Comando | Exit | Reporte | Efecto observado |
| --- | ---: | --- | --- |
| `quality-artifacts prune --state-root … --json` | 0 | `status=passed`, `removed=2`, `reclaimed_bytes>0`, `retained>=1` | Reclama sólo el par expirado y su claim expirada. No pone nada en quarantine y **no** toca `FOREIGN`. |
| `quality-artifacts recover --state-root … --json` | 0 | `status=passed`, `validated=1`, `quarantined=1`, `clock_regression=false` | Mueve `FOREIGN` a quarantine con razón `unknown_name` conservando sus bytes; el par válido se sigue leyendo. |
| `quality-artifacts prune --state-root …` (sin `--json`) | 0 | `prune: passed\n` | Una línea acotada; el reporte sólo sale con `--json`. |
| `prune` con watermark adelantado | 1 | `status=blocked`, `error_code=recovery_required`, `data=null` | `prune` nunca re-basa el reloj. |
| `recover` sobre ese mismo store | 0 | `clock_regression=true` | Re-basa el watermark; `prune` vuelve a exitir 0 después. |

En los cinco casos `rust-mcp-mutations-v1/journal/000001.json` conserva sus bytes
y su directorio conserva una sola entrada.

La CLI está integrada en `main.rs` (`quality-artifacts recover|prune`), exige
`--state-root` absoluto, acepta `--json` y escribe como máximo 16 KiB en stdout.
Esto corrige la afirmación contraria que `docs/client-configuration.md` mantenía.

## Procedimiento de rollback del binario

1. EOF/shutdown limpio: cancela los jobs vivos, espera el join del cleanup y no
   reanuda trabajo tras el restart. Los IDs anteriores quedan enmascarados.
2. Los artifacts completos pueden sobrevivir bajo su autoridad y TTL, pero no
   representan ejecución durable.
3. El binario anterior no lee `rust-mcp-quality-artifacts-v1`; el directorio queda
   intacto, no se migra ni se borra.
4. Un binario v1 frente a registros de una versión futura falla cerrado y conserva
   los bytes; no hay reinterpretación ni borrado in-place.
5. Ni el rollback ni `recover`/`prune` reducen floors antirollback, borran journals
   M2 ni tocan artifacts de otro owner.
6. Cambiar la imagen o el perfil requiere una identidad aprobada y recalibración,
   no una sustitución por tag.

## Lo que este recibo no cubre

- No hay migración v1 → v2 porque no existe un v2; sólo se prueba que un v2 futuro
  sería un hermano y que este binario lo deja en paz.
- No se prueba rollback entre binarios compilados de dos revisiones distintas: el
  oráculo son los bytes en disco, no dos ejecutables.
- La calificación positiva sigue limitada a macOS ARM64/APFS.
- Un watermark escrito por una versión futura no tiene remedio dentro de este
  binario, como se describe arriba.
