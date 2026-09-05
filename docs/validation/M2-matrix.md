# M2 — matriz de implementación y prerrequisitos

Fecha: 2026-09-05. Estado: **M2-01..07 Done: cinco tools implementadas y calificadas localmente**.
La planificación fue commit `8b35cf6`, merge local `2f54b360e1e81f21e7efeff7c451cdd6f663a04f`.
La implementación `0.2.0-dev` se integra localmente desde `ai/m2-write-qualification`;
[full, clientes, reviews e integración](M2-07.md).
No hubo push, PR, tag ni release nueva. Los recibos históricos identifican sus propios bytes.

| Elemento | Estado | Evidencia y límite |
| --- | --- | --- |
| Planificación M2–M8 | Integrada localmente | [Validación/reviews](../roadmap/planning-validation.md); M3+ no implementado |
| Scope M2 | Cinco tools adicionales implementadas | [AGENTS](../../AGENTS.md), [contratos](../tools.md); 18 totales en desarrollo, 13 en release estable 0.1.0 |
| D02 | Exclusión fuerte No-go; contrato local_coordinated Accepted | [ADR-050](../adr/ADR-050-local-coordinated-mutation.md); host/editor confiables, sin broker ni exclusión OS |
| M2-01 | Done histórico | Lints preview/commit/receipt y Scratch; [revisión](../reviews/M2-01-review.md), [core 14/14](M2-02-core-gate.json), [runtime](M2-02-runtime-gate.json) |
| M2-02 | Done histórico | [fmt runtime 2/2](M2-02-runtime-gate.json), [Sonnet](../reviews/M2-02-contract-review.md), [Opus](../reviews/M2-02-native-review.md), [nativo](M2-02-native-qualification.json) |
| M2-03 | Done | [ADR-056](../adr/ADR-056-cargo-fix-isolated-loopback.md), [Fix runtime](../../crates/mcp-server/tests/inspection_runtime/fix_mutation.rs), [proc macros hostiles](../../crates/mcp-server/tests/inspection_runtime/fix_hostile.rs), [máscara socket real](M2-fix-socket-mask.json) |
| M2-04/05 | Done | [ADR-055](../adr/ADR-055-offline-cargo-data-and-lock-policy.md), [ADR-057](../adr/ADR-057-typed-manifest-and-dependency-operations.md), [runtime 4/4](M2-04-runtime-gate.json); vendor opcional y preserve_presence |
| M2-06 | Done | Cuatro familias tipadas; runtime anterior y [editor LF/CRLF/herencia](../../crates/project-adapter/tests/manifest_edit.rs) |
| M2-07 | Done | [contrato Sonnet Accepted](../reviews/M2-final-contract-review.md); [seguridad Opus Accepted](../reviews/M2-final-security-review.md) y [writer Opus Accepted](../reviews/M2-final-native-review.md); [full final](M2-full-gate.json), [runtime 17/17](M2-final-runtime.json), [cliente PASS](M2-clients.json), [ADR-059 Accepted](../reviews/M2-059-review.md), [AGY trazabilidad](../reviews/M2-closure-agy-review.md) |
| Memoria nativa | Medida, optimizada y limitada en alcance | [medición](M2-07-native-memory.json): 976,666,624 B RSS ciclo máximo, 798,769,152 B commit aislado; observaciones, no cap ni RSS MCP completo |
| Observabilidad local | Done | [ADR-058](../adr/ADR-058-local-mutation-observability.md); sin collector ni telemetría |

## Límites de la calificación

M2 es optativo y reutiliza el runtime aprobado. El publisher positivo está limitado
al adapter macOS ARM64/APFS; otros targets rechazan escritura. Los límites son
16 MiB de source, 4096 entradas, 1 MiB por archivo, 128 reemplazos, cuatro planes
con 64 MiB de bytes retenidos y journal privado con cuotas (207 MiB retenidos, 48 MiB staging y 1 MiB de crecimiento dentro de 256 MiB). La memoria total puede
ser mayor: la medición nativa máxima se aproxima a 1 GiB y no incluye todos los
planes/transportes del MCP. No se promete coste despreciable ni se instala un servicio
para ocultarlo. No hay atomicidad visible multiarchivo, exclusión de editores,
protección frente a host malicioso ni prueba de supervivencia a pérdida de energía.

Los faults de filesystem inyectados se distinguen de disco físico lleno y del
ENOSPC real del tmpfs guest. Unknown bytes/journal permanecen conservados con
recovery_required; ese resultado no equivale a recuperación automática exitosa.
Una release 0.2 requerirá sus propios artifacts, SBOM, firmas y gate de distribución;
el trabajo actual no la publica ni altera la release 0.1.0.

## Experimento reproducible

```text
python3 -B scripts/probe-m2-write-primitives.py
```

macOS 26.6.2 ARM64, Darwin 25.6.0, APFS, UID501. SDK y script identificados por SHA
en el JSON. El script verifica constantes contra el SDK antes de usar ctypes.
Solo usa fixtures privados temporales; los controles inseguros deliberados no
reciben paths de usuario ni operan sobre repositorios. El árbol temporal se elimina.

Exit 0 significa que coincidieron las observaciones esperadas, incluyendo ataques
que demuestran insuficiencia del diseño. El resultado de producto es explícitamente
`no_go_current_candidate`, no pass de M2. Exit78 significa host no calificable;
exit1 indica observación inesperada y exit70 error de infraestructura con JSON.
La [primera ejecución](m2-d02-probe-attempt1.json)
salió78 por un error del harness: `stat -f %T` en macOS no devuelve tipo de filesystem.
Se corrigió usando device de df y plist de diskutil. No fue ausencia real de APFS.
La [segunda ejecución](m2-d02-probe-attempt2.json) ya reprodujo el No-go; una revisión
Opus pidió mejorar su recibo. La ejecución final añade hashes/bytes counts/inodes,
identidad del root handle, errno simbólico, aserciones de fullfsync, flags derivados
de medición y timings monotónicos. El total y cada subprocess constan en el JSON;
el tiempo corto medido no se sustituye por una estimación del revisor.

Observaciones materiales:

- Rename SWAP+NOFOLLOW_ANY+BENEATH funciona con paths desde el root; flags inválidos
  y path absoluto se rechazan. Symlink y parent movido se niegan sin cambiar canarios;
  el control sin flags sí los cambia. El parent descriptor ya movido también permite
  cambiar el canario incluso con SWAP+NOFOLLOW_ANY+BENEATH: esos flags no reanclan
  un descriptor ajeno al root original.
- Mover el propio root después del preflight no impide usar su descriptor para
  cambiar bytes en su nueva ubicación. La primitiva no fija el namespace configurado.
- Un segundo lock cooperativo se deniega; otro proceso que ignora flock sí cambia
  el archivo. Swap publica el candidato aunque los bytes ya no coincidan con preflight.
- Swap de vuelta conserva inodes, pero desplaza una actualización posterior de la
  ruta visible hacia staging. Esto no es CAS ni demuestra rollback sin lost update.
- F_SETLEASE devuelve EPERM al proceso de prueba. Es disponibilidad observada, no
  calificación de enforcement positivo de leases. F_FULLFSYNC devuelve0 para archivo
  y directorio, lo que no demuestra supervivencia ante pérdida de energía.

Sin pruebas de EXDEV entre volúmenes, hardlinks, mmap, crash de kernel, power loss ni journal
real. Es un no-go de los mecanismos evaluados con esta autoridad, no una demostración
de imposibilidad universal ni un defecto nuevo de M1.

[Verificación externa al script](m2-d02-verification.json): SHA, reconciliación de
timings/inodes/hashes, trece snapshots y producto/manifests/workflows sin cambios.
La [revisión Opus 5 High](../reviews/M2-D02-review.md) aceptó el recibo corregido
sin P0/P1; conserva las limitaciones y mejoras de evidencia pendientes.

## Bloqueo histórico anterior a ADR-050

El writer depende de una decisión explícita: mantener exclusión fuerte y diseñar/
aprovisionar broker con UID/ACL/anchor separados, o aceptar un contrato de confianza
en exclusividad del workspace declarada por el host. La segunda opción modifica
la garantía y exige ADR/spec/criterios/docs públicos; no equivale a enforcement OS.
No cambiar cuentas, ownership, ACLs, entitlements ni servicios para forzar un positivo.

Siguiente acción histórica (sustituida por ADR-050): resolver D02 con el owner, actualizar ADR-049 y decisiones D01/D03,
calificar positivamente la frontera y continuar M2-01. El [prompt M2](../prompts/implement-m2.md)
sigue siendo la entrada completa. No continuar a M3. No afirmar cinco tools,
journal/receipts ni M2 implementados a partir de este experimento.

## Resolución del owner posterior al experimento

El owner delegó escoger la mejor decisión sin cargar instalación/uso. ADR-050
acepta local_coordinated y sustituye la exigencia de exclusión OS. La sección de
handoff anterior registra el bloqueo histórico; ya no se espera otra confirmación.
Se continúa M2-01 con pruebas positivas/negativas para el contrato nuevo. No se
reinterpretan los 15 resultados del probe como aprobación del writer.
