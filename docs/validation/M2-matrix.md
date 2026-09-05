# M2 — matriz de implementación y prerrequisitos

Fecha: 2026-09-05. Estado: **Blocked en D02**, no M2 Done.
La planificación fue commit `8b35cf6`, merge local `2f54b360e1e81f21e7efeff7c451cdd6f663a04f`.
Esta fase se inició después de ese merge en `ai/m2-write-qualification` y actualizó
primero el scope de AGENTS conforme al owner. No hubo push, PR, tag ni release.

| Elemento | Estado | Evidencia y límite |
| --- | --- | --- |
| Planificación M2–M8 | Integrada localmente | [Validación/reviews](../roadmap/planning-validation.md); no implementación |
| Scope de sesión M2 | Actualizado | AGENTS autoriza cinco tools futuras; trece M1 siguen implementadas |
| D02 investigación | No-go del candidato actual | [Prerrequisitos](../roadmap/m2-d02-host-preconditions.md), [ADR-049 Proposed](../adr/ADR-049-m2-write-boundary-qualification.md) |
| Probe nativo | 15 observaciones coincidentes; no califica writer | [Script](../../scripts/probe-m2-write-primitives.py), [JSON final](m2-d02-native-probe.json) |
| M2-01..07 | Pendientes, bloqueados antes del primer writer | D02 requiere frontera acreditada y decisión; no implementación parcial expuesta |
| M2 full/native/client gate | No ejecutado | No hay candidato de producto M2; no reutilizar 23/23 M1 como evidencia nueva |

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

## Decisión necesaria y handoff

El writer depende de una decisión explícita: mantener exclusión fuerte y diseñar/
aprovisionar broker con UID/ACL/anchor separados, o aceptar un contrato de confianza
en exclusividad del workspace declarada por el host. La segunda opción modifica
la garantía y exige ADR/spec/criterios/docs públicos; no equivale a enforcement OS.
No cambiar cuentas, ownership, ACLs, entitlements ni servicios para forzar un positivo.

Siguiente acción: resolver D02 con el owner, actualizar ADR-049 y decisiones D01/D03,
calificar positivamente la frontera y continuar M2-01. El [prompt M2](../prompts/implement-m2.md)
sigue siendo la entrada completa. No continuar a M3. No afirmar cinco tools,
journal/receipts ni M2 implementados a partir de este experimento.
