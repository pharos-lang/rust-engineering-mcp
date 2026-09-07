# M2-01 — Disposición de revisión de la primera vertical

Fecha: 2026-09-05. Estado: snapshots revisados aceptados; cierre de la vertical y
M2 pendientes de la integración y del gate sobre la fuente final.

## Paquetes y revisores

Claude Code CLI 2.1.260, sin herramientas, cambios de archivos ni sesiones
persistentes. Sonnet 5 / medium revisó contrato/aplicación/editor; Opus 5 / high
revisó autoridad, journal, writer APFS y recuperación. Los recibos `*-inputs.json`
de este directorio fijan los hashes de los archivos suministrados; los JSON de
respuesta conservan modelo efectivo, tiempos, resultado y findings originales.

- [Contrato inicial](M2-01-contract-sonnet.json),
  [recheck aceptado](M2-01-contract-recheck-sonnet.json),
  [inputs del recheck](M2-01-contract-recheck-inputs.json).
- [Native inicial](M2-01-native-opus.json),
  [recheck](M2-01-native-recheck-opus.json),
  [Scratch aceptado](M2-01-native-scratch-opus.json),
  [inputs de Scratch](M2-01-native-scratch-inputs.json).

## Disposición

| Finding | Resolución |
| --- | --- |
| Cuota/corrupción ajena impedía consultar o recuperar operaciones existentes | Consulta/recovery por ID quedan fuera del índice de admisión de commits nuevos; list/prune local explícitos para retención terminal. |
| Bytes previstos se presentaban como efecto; aborto parecía no-op | Recibos separan `intended_after` de `effect_after`; estado `aborted` exige nuevo preview. |
| Error de swap podía esconder publicación | Clasificación de pares bytes/inode antes de decidir receipt, sin rollback inverso ciego. |
| Candidato manifest demasiado grande | Límite de 256 KiB en la frontera nativa, además de límites de source y salida. |
| Grant y read root podían confundirse | Wiring comprobado a roots de escritura explícitas; autoridad exige workspace físico exacto. |
| Mutex del registro durante Cargo | Preparación posee bytes, valida fuera del registro y revalida la generación después. |
| Heurística de salida podía retener un plan no entregable | Medición de la codificación MCP completa antes de remember, con máximo de dígitos de duration. |
| Clone antes de registrar inode / rewrite parcial quedaban irrecuperables | Fase durable Scratch antes de truncar; adopción Prepared solo de clone exact-before; cleanup de scratch propio solo con original intacto. |

Los rechecks no identificaron P0/P1 pendientes en sus respectivos snapshots.
La comprobación posterior de solapamiento journal/read-root en ambas direcciones
es hardening adicional, con test del parser host. La extensión multiarchivo y la
composición genérica de tools de M2-02 son código posterior: requieren otra revisión.

## Límites de evidencia y P2

- `nlink == 1` se exige por `FileStamp::from_stat` al abrir archivos propios del
  workspace. `O_UNIQUE` no tiene probe de startup y no se acredita como garantía.
- ACL se conserva mediante la primitiva kernel CLONE_ACL; no hay comparación
  independiente de ACL. UID/GID/modo/flags/xattrs sí se verifican.
- Los SIGKILL cubren clone ya durable, Scratch y post-swap; los errores internos
  de clone/sync tienen evidencia determinista, no simulación de corte eléctrico.
- El ejemplo del reviewer que renombra temporal a Cargo.toml y luego atribuye al
  unlink del nombre temporal la eliminación de Cargo.toml no describe la semántica
  real: ese nombre temporal ya no existiría. Sí queda una carrera entre verificación
  y unlink si el host reemplaza el nombre reservado por otro inode. El contrato
  local_coordinated excluye esa intervención; no se presenta como prevención OS.
- Bytes desconocidos conservan evidencia y bloquean nuevos commits. Falta completar
  el procedimiento administrativo de reconciliación antes de M2 Done; no se sugiere
  borrar journal/temporales ni ejecutar git clean para desbloquearlos.
- La higiene del destructor del helper de crash y la medición al techo se integran
  con la calificación multiarchivo; no se acreditan como completadas aquí.

## Validación

[Gate core v2](../validation/M2-01-core-gate-v2.json): 14/14, 711 pruebas Rust y un
doctest, antes del último cambio Scratch y de M2-02. Se conserva también el primer
gate fallido por formato. No se reutiliza este gate como resultado de la fuente
final. Las pruebas nativas posteriores de Scratch pasaron 109 casos del paquete;
el nuevo gate conjunto debe fijar nuevamente fuente, runtime, conteos y resultado.
