# ADR-053 — Staging de mutación acotado en el runtime existente

Date: 2026-09-05

## Context

M2-02/03 necesitan que rustfmt/Cargo modifiquen una copia, sin write bind del host.
El volumen local ordinario de M1 no limita disco. Un tmpfs por contenedor pierde
sus bytes al terminar y no sirve para exportar después de eliminar mutadores.

## Decision

Extender el único Execution Gateway con fases tipadas M2. Usar la imagen aprobada
existente y un volumen Docker local-driver con type/device tmpfs y opciones exactas
`size=64m,nr_inodes=8192,uid=65534,gid=65534,mode=0700,nosuid,nodev,noexec`.
Un guardian confiable `/usr/bin/sleep 900`, sin red y con mount read-only, mantiene
una referencia activa durante todo el job. No es un servicio instalado ni persistente.
Verificar identidad/estado running antes de transferir y después de cada mutador.
No reiniciar automáticamente el guardian: perder su mount invalida el candidato.

Orden: volumen → guardian → ingest confiable → mutador tipado → remove y ausencia
verificada de todos los writers → exporter confiable read-only → cleanup exporter,
mutadores, guardian y volumen → ausencia verificada. Cualquier incertidumbre de
cleanup pone el gateway en cuarentena. Se reutilizan identidad del daemon, imagen,
seccomp, env reconstruido, UID65534, budgets, supervisión y cancellation de M1.
Solo ingest de archivo generado y fase mutador pueden montar source RW. No añadir
comandos libres ni shell. Las trece tools M1 mantienen sus perfiles read-only.

Ingest M2 genera USTAR propio, dirs0700/files0600. Exporter fijo:
`/usr/bin/tar --create --file=- --format=ustar --sort=name --one-file-system --directory=/source .`.
Exigir exit0, stderr vacío, stdout completo y acotado24MiB. Decodificar en memoria,
nunca extraer en el host: validar checksum, tipos regular/directorio, nombres
portables y únicos, modes esperados, UID/GID65534, sin links, devices, PAX/GNU,
sparse, paths externos, colisiones ni datos extra tras fin. Admitir únicamente
prefijo generado `./` y slash terminal de directorio, sin normalización general.
El source resultante conserva exactamente paths/tipos/directorios de la captura;
el validador de operación decide los bytes cambiables. fmt/fix solo `.rs` existentes.
Crear Cargo.lock se diseñará/calificará por separado en D05 y el writer nativo.

Los 64MiB/8192inodes limitan staging y deben probarse contra source máximo de M1
(16MiB/4096entradas), rustfmt y fixtures Cargo. No implican que todos los proyectos
sean aceptados: agotamiento aborta y limpia, sin publicar un candidato parcial.
El límite interno binario es independiente del límite de texto del resultado MCP.
Las fases/argv/config/versiones y source exacto se vinculan al fingerprint de ejecución.

## Alternatives considered

- Host bind RW: permite a código de proyecto alcanzar source real; rechazado.
- Volumen Docker ordinario: no ofrece cuota de bytes/inodos acreditada; rechazado.
- Exportar desde el mutador vivo: productor hostil puede competir con export; rechazado.
- Otro runtime/helper instalado: añade distribución y mantenimiento innecesarios.
- Un tmpfs por contenedor o contenedor detenido como guardian: pierde continuidad.

## Consequences

El control plane M2 usa un deadline compartido de trabajo y una reserva de cleanup
de 10 s desde su inicio, sin renovar 10 s por comando. La reserva ignora la
cancelación del caller para intentar retirar el containment. M1 conserva su ruta
de control existente; startup/calibración se contabilizan separadamente del job.
Si una solicitud Docker mutante enviada (create/start/remove) pierde su resultado,
una ausencia inmediata no demuestra que el daemon no pueda completarla más tarde:
se conserva CleanupUncertain y cuarentena aunque el intento de cleanup parezca
vacío. Un nuevo proceso también detecta objetos etiquetados pendientes. No hay
retry ni restablecimiento implícito tras incertidumbre de ejecución.

Hay un contenedor auxiliar acotado por job y transferencia/exportación adicional.
No hay descarga, instalación, nuevo daemon ni privilegios del host. Los datos tmpfs
pueden terminar en swap; no se promete ausencia absoluta de persistencia física.
La prueba D04 acredita la primitiva y sus límites experimentales; no reemplaza
pruebas adversariales de parser, scope, proyecto hostil y cleanup de producción.

## Evidence

[Probe y recibo D04](../validation/M2-D04-native-qualification.md): Docker29.7.2
LinuxARM64,79observaciones, bytes/inodos ENOSPC y pérdida al último unmount.
Fuentes oficiales: [local-driver options](https://docs.docker.com/reference/cli/docker/volume/create/#driver-specific-options--o---opt),
[tmpfs limits](https://docs.docker.com/engine/storage/tmpfs/) y
[Moby29.7.2 local mounts](https://github.com/moby/moby/blob/docker-v29.7.2/daemon/volume/local/local.go).

## Status

Accepted para implementar D04. M2-02/03 y la calificación de producción siguen pendientes.
