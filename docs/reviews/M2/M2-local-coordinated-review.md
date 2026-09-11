# M2 — Revisión de la decisión local_coordinated

Fecha: 2026-09-05. Reviewer: Claude Code 2.1.260, modelo explícito
claude-opus-5, effort high, tools deshabilitadas. Inputs y hashes:
[M2-local-coordinated-inputs.json](M2-local-coordinated-inputs.json) y
[recheck inputs](M2-local-coordinated-recheck-inputs.json). Respuestas raw:
[inicial](M2-local-coordinated-opus.json), [recheck](M2-local-coordinated-recheck-opus.json).

La dirección fue aceptada por ambos informes; el veredicto documental fue Revise.
El primer informe señaló ambigüedades P0 de confianza/localización del journal;
el recheck confirmó esos P0 cerrados y no encontró P0. No es aprobación del writer.

Disposición del Technical Owner:

- Trust por provenance: corregido en ADR-050; código lanzado por el MCP solo gateway.
- Journal fuera de roots, binding/quotas/durabilidad/restart: especificado ADR-052.
- Root: reabrir ruta configurada y comparar dev/ino original, no descriptor consigo mismo.
- Permitir trabajo durante preview se conserva: stale se detecta al comprobar source;
  el reviewer retiró su propuesta de prohibirlo durante toda la revisión del diff.
- Locks: global → workspace, no bloqueantes, ambos durante commit/cleanup; stores
  independientes quedan fuera del contrato coordinado y no se anuncia exclusión OS.
- Scope/temporales: grant del workspace exacto; temporal simple en esa root,
  cleanup solo conocido después del journal durable, conservar durante recovery.
- ID: persiste en journal antes del primer efecto; receipt tras restart no exige plan.
- Oráculo: preview debe terminar Cargo exitosamente y su provenance queda en digest.
- Cuota/limpieza: rechazo antes de comenzar; limpieza de terminados y UX del store
  lleno son criterios pendientes de M2, no features fingidas ni motivo para borrar
  journals manualmente sin calificar su estado. No se da M2-01 por cerrado aún.
- P2 rename flags del recheck: **no se acepta la corrección sugerida**. El SDK/XNU
  efectivamente expone RENAME_NOFOLLOW_ANY=16 y RENAME_RESOLVE_BENEATH=32 para
  renameatx_np, además de SWAP=2. Lo comprueban el SDK local y el probe nativo
  versionado; no son solo atributos de openat. Se conservan ambos controles.
- P2 virtual manifest/límites: rechazo package sobre virtual explícito; 256 KiB
  del editor precede al máximo general de source.

El gate positivo y la revisión del código siguen pendientes. Ninguna revisión de
modelo sustituye tests nativos, evidencia de Cargo o el criterio del integrador.
