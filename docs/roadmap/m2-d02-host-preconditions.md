# D02 — prerrequisitos del host para escritura M2

> Resolución posterior a la planificación: el owner delegó la decisión D02 para
> preservar instalación/uso sencillos. [ADR-050](../adr/ADR-050-local-coordinated-mutation.md)
> acepta local_coordinated: namespace host confiable, precondiciones y locks MCP,
> sin exclusión OS de editores externos. Las exigencias de exclusión fuerte y espera
> de decisión del owner que aparecen abajo son históricas y quedan sustituidas por
> ese ADR; no hay CAS, atomicidad multiarchivo ni rollback sobre bytes desconocidos.
> La calificación positiva de M2 sigue pendiente.


Estado: investigación **read-only**, decisión **Proposed**. No es implementación
ni calificación nativa. Fecha: 2026-09-05. Complementa [M2](m2-safe-mutation.md)
y [D02](adr-backlog-m2-m8.md). La investigación independiente GPT-5.6 Sol High
inspeccionó repo, host, SDK y fuentes Apple; el Technical Owner verificó los flags
del SDK y el control de entitlement en el código oficial. No ejecutó mutaciones.

## Propiedades distintas

1. Resolver ambos paths del rename desde el handle original con restricciones
   beneath/no-follow protege la resolución frente a links/escapes descendientes.
2. No sobrescribir contenido cambiado después del preflight exige exclusión o una
   primitiva que condicione el efecto al estado esperado. El swap de nombres no
   ofrece esa comparación. Un lock que solo respeta el servidor no cubre editores.
3. Impedir que se mueva el propio root requiere un anchor/exclusión adicional;
   beneath relativo a un descriptor no fija la entrada del root en su padre.
4. Recuperabilidad durable y visibilidad multiarchivo son propiedades separadas.

## Evidencia inspeccionada

El SDK de Xcode local expone `renameatx_np` con `RENAME_SWAP`, `RENAME_EXCL`,
`RENAME_NOFOLLOW_ANY` y `RENAME_RESOLVE_BENEATH` en `usr/include/sys/stdio.h`.
`sys/fcntl.h` expone open beneath/no-follow y `F_SETLEASE`/`F_GETLEASE`.
El adaptador actual adquiere datos desde el root original; no tiene writer ni CAS.
El proceso y workspace revisados usan el mismo UID interactivo, sin broker con
ownership separado que excluya a otros procesos de ese usuario.

Fuentes Apple fijadas: [rename(2)](https://github.com/apple-oss-distributions/xnu/blob/xnu-12377.121.6/bsd/man/man2/rename.2#L120-L155),
[flock(2)](https://github.com/apple-oss-distributions/xnu/blob/xnu-12377.121.6/bsd/man/man2/flock.2#L40-L74),
[autorización de leases](https://github.com/apple-oss-distributions/xnu/blob/xnu-12377.121.6/bsd/vfs/vfs_subr.c#L12989-L13002),
[entitlement usado por tests oficiales](https://github.com/apple-oss-distributions/xnu/blob/xnu-12377.121.6/tests/file_leases.entitlements).
La fuente pública inspeccionada no coincide exactamente con el kernel instalado;
se exige una prueba nativa antes de convertir esa lectura en capability positiva.

## Alternativas revisables y criterio de decisión

| Alternativa | Qué aporta | Prerrequisito / limitación | Disposición propuesta |
| --- | --- | --- | --- |
| Rename beneath/no-follow + hash + flock | Resolución contenida, detección de algunos conflictos, serialización cooperativa | Queda intervalo entre hash y efecto; root puede moverse; escritor externo ignora flock | Insuficiente para D02 fuerte por sí sola |
| Leases XNU de archivos/directorios | Exclusión kernel candidata con break notifications | Entitlement privado `com.apple.private.vfs.file-leases`; timeout forzado; cubrir ancestros/mmap/links y kernel exacto | No disponible demostrado en producto; investigar solo con entitlement legítimo |
| Broker bajo UID separado y namespace exclusivo | Frontera DAC/ACL entre broker y editores | Aprovisionamiento privilegiado, anchor estable, operación mediante broker; cambia edición directa | Candidato a diseño y calificación si owner elige ese modelo |
| Exclusividad declarada por host | Contrato de uso quiescente, no enforcement externo | Owner debe aceptar expresamente confianza en ausencia de otros escritores; ADR/spec/docs/receipt y riesgo residual | Nunca seleccionar automáticamente ni describir como exclusión OS |
| Preview/diff únicamente | Resultado revisable sin escribir host | No satisface las cinco mutaciones normativas M2 | Puede ser trabajo preparatorio, no cierre del hito |

No se afirma imposibilidad universal de escritura segura en macOS. Falta una
frontera acreditada que satisfaga simultáneamente los requisitos fuertes de D02
bajo la configuración actual. Tampoco se declara un defecto de las lecturas M1.

## Experimento mínimo al iniciar M2, después del merge documental

Usar solo fixtures privados y desechables, nunca modificar el workspace del usuario:
control positivo/negativo de rename beneath/no-follow, parent movido, root movido,
writer que ignora flock después del preflight y `F_SETLEASE` con errno real. Registrar
kernel/SDK/filesystem, flags, hashes y resultado por caso. Un probe que demuestra
la insuficiencia del candidato no cuenta como pass de M2; determina Go/No-go y
el prerrequisito operativo exacto. No solicitar privilegios ni cambiar ownership,
ACLs, cuentas o servicios para simular un positivo.
