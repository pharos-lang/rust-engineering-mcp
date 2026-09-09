# ADR-078 — Captura de vendor offline grande, separada de `SourceBundle`

Fecha: 2026-09-09.

## Status

Accepted como decisión de contrato. **No** concede límites todavía: los límites
concretos se fijan con las mediciones que §4 exige, y hasta entonces M5-01 sigue
bloqueado.

## Context

`rust.benchmark.run` resuelve su harness offline desde un `CargoVendorSnapshot`,
que es un `SourceBundle`. El cierre de `criterion 0.8.2` para
`aarch64-unknown-linux-gnu` son 52 paquetes, 6 014 archivos, 779 directorios y
**156 267 469 bytes**, con cuatro archivos por encima del límite por archivo. Un
`SourceBundle` admite 4 096 entradas, 16 MiB en total y 1 MiB por archivo
([ADR-055](ADR-055-offline-cargo-data-and-lock-policy.md)), así que **los tres
límites se rompen a la vez**.

Podar no sirve, y está medido: los cuatro archivos que exceden el límite por
archivo pertenecen a paquetes solo-Windows, y Cargo exige que todo paquete del
lockfile esté presente en un directory source. Quitarlos produce
`failed to read root of directory source`. La evidencia completa está en
[M5-01-blocker.json](../validation/M5-01-blocker.json).

Los límites de `SourceBundle` **no se suben**. Pertenecen al contrato de datos
offline calificado en M2/M4 y los comparten todos los flujos que llevan datos del
host al guest; ampliarlos para poner en verde una prueba debilitaría una frontera
de seguridad calificada sin decisión ni recalificación.

## Decision

Se crea un contrato **distinto**, con su propio nombre, sus propios límites y su
propio threat model, para una clase de dato que `SourceBundle` no fue diseñado
para llevar: un árbol de vendor grande, inmutable y aprovisionado explícitamente.

### 1. Capturado, no montado

La primera solución **no** es montar un directorio mutable del host de solo
lectura en el guest. Que el guest lo vea de solo lectura no impide que otro
proceso del host lo cambie mientras se usa, ni durante la propia captura. La
autenticación tiene que venir del host, antes de que el guest vea nada.

Cargo es explícito en que `.cargo-checksum.json` **no** protege frente a
modificaciones maliciosas del árbol vendorizado, así que la integridad no puede
delegarse en él.

### 2. Inmutable y direccionada por digest

La captura se identifica por el digest de su contenido, y ese digest es la
identidad que viaja en la provenance de cualquier medición que la use. Una
captura ya existente se reutiliza por digest sin volver a capturar. Una captura
cuyo digest no coincide con el declarado no se usa: se rechaza.

### 3. Aprovisionada explícitamente

Igual que el resto de runtimes y plugins de este proyecto: con autorización
separada, versiones, hashes, licencias y procedencia registradas, y **sin
descargas durante un gate**. La tool nunca captura por su cuenta como efecto
secundario de una medición.

### 4. Límites propios, justificados por medición

Los límites de este contrato **no** se copian de `SourceBundle` ni se eligen
porque el cierre de criterion quepa. Se fijan con mediciones registradas de:

- **memoria** máxima residente durante captura y durante ingesta;
- **disco** ocupado por la captura y por su materialización en el guest;
- **tiempo** de captura, de verificación de digest y de ingesta;
- **concurrencia**: qué ocurre con dos capturas simultáneas y con una captura
  mientras corre una medición.

Hasta que esas cuatro mediciones existan y estén en un recibo, este ADR no
autoriza ningún número.

### 5. Lectura incremental

Ni la captura ni la ingesta cargan el árbol completo en memoria. Se leen y se
verifican por partes, con el digest calculado de forma incremental. Un contrato
cuyo tamaño se mide en cientos de MiB no puede tener un camino que materialice
todo en RAM.

### 6. Controles que el contrato exige

- **Cuotas** de tamaño total, número de entradas y tamaño por entrada, aplicadas
  durante la lectura y no después.
- **Rechazo de enlaces**: ni symlinks ni hard links; una entrada que no sea
  archivo o directorio regular es un rechazo, no un salto.
- **Rechazo de cambios durante la captura**: si el árbol se modifica mientras se
  captura, la captura falla. No se publica una captura de un árbol que se movió.
- **Montaje de solo lectura** en el guest, sobre la captura autenticada y nunca
  sobre el directorio original.
- **Limpieza tras cancelación**: una captura interrumpida no deja residuo ni un
  artefacto a medias que otra ejecución pueda tomar por completo.

## Alternatives considered

- **Subir los límites de `SourceBundle`.** Descartado, y es la razón de que este
  ADR exista.
- **Montar el directorio del host de solo lectura.** Descartado como primera
  solución por lo dicho en §1: el modo de montaje del guest no dice nada sobre lo
  que el host puede hacerle al directorio.
- **Confiar en `.cargo-checksum.json`.** Descartado: la propia documentación de
  Cargo dice que no protege frente a modificación maliciosa.
- **Vendorizar solo el cierre del target Linux.** Medido y descartado: Cargo
  exige todo paquete del lockfile en un directory source.

## Consequences

M5-01 sigue **bloqueado** hasta que este contrato esté implementado, medido,
calificado nativamente y revisado de forma independiente. `rust.benchmark.compare`
sigue sin positivo de cliente por dependencia, porque la única tool que publica un
dataset es la que no puede correr. Ninguno de los dos estados cambia por
documentar esta decisión.
