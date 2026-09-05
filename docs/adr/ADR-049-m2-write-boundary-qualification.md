# ADR-049 — Calificación previa de la frontera de escritura M2

Date: 2026-09-05

## Context

El owner autorizó M2 después de integrar la planificación. ADR-013 requiere permisos,
diff, precondiciones, locks y recuperación; D02 del plan exige distinguir containment
de paths, exclusión de escritores externos y durabilidad. El workspace macOS/APFS
normal pertenece al UID interactivo; todavía no existe un writer ni broker separado.

## Decision

**Proposed, no Accepted:** antes de M2-01 ejecutar un probe nativo limitado a
fixtures desechables para comprobar rename beneath/no-follow, conflictos que
ignoran flock, movimiento de root y disponibilidad de leases. Registrar por
separado comportamiento observado y criterio de producto. Un pass del experimento
puede demostrar No-go para una garantía: no acredita M2 ni habilita tools nuevas.

Si no existe exclusión acreditada, el owner debe decidir entre aprovisionar una
frontera de identidad/namespace separada o aceptar expresamente otro contrato de
concurrencia con riesgos residuales. No presentar swap seguido de validación como
CAS: el efecto ocurre antes de detectar conflicto y rollback también puede competir.

## Alternatives considered

- Rename root-relative + hashes + flock: útil para containment y cooperación;
  insuficiente por sí solo para negar escritores externos.
- File leases XNU: candidato con entitlement privado y timeout; comprobar el host
  real sin intentar autootorgar privilegios. El EPERM nativo por sí solo acredita
  indisponibilidad del proceso; el requisito de entitlement procede de la fuente
  Apple inspeccionada, no se deduce únicamente de ese errno.
- Broker con UID/ACL/anchor exclusivo: requiere diseño, aprovisionamiento y gate
  positivos adicionales; no cambiar ownership del repositorio automáticamente.
- Contrato optimista con backups o exclusividad declarada: requiere decisión
  explícita de producto, documentación y criterios diferentes, no fallback.

## Consequences

El probe no modifica código de producción, manifests, schemas, workflows ni datos
del usuario. Solo escribe dentro de su directorio temporal privado y emite JSON.
No-go mantiene pendientes M2-01..07 y los dependientes; pueden prepararse decisiones
y fixtures independientes sin anunciar mutaciones implementadas.

## Status

Proposed. Pendiente evidencia nativa y decisión de frontera del owner.

Evidencia nativa obtenida: [probe](../validation/m2-d02-native-probe.json), No-go
del candidato, sin calificar el writer. La decisión del owner sigue pendiente.
Fuente Apple de leases inspeccionada el 2026-09-05:
[vfs_subr.c, autorización y EPERM](https://github.com/apple-oss-distributions/xnu/blob/xnu-12377.121.6/bsd/vfs/vfs_subr.c#L12989-L13002)
y [entitlement del test oficial](https://github.com/apple-oss-distributions/xnu/blob/xnu-12377.121.6/tests/file_leases.entitlements).
Tag público xnu-12377.121.6; no se presenta como fuente idéntica al kernel instalado.
