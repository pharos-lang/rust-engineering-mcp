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

### Corrección (2026-09-09) — no son tres límites, y uno no es un límite

Las mediciones que §4 exige encontraron dos cosas que este documento decía mal, y
la segunda cambia qué hay que decidir.

**Son cuatro límites, no tres.** Además de entradas, bytes totales y tamaño por
archivo, el cierre rompe `SOURCE_MAX_PATH_BYTES`: **123 rutas** pasan de 100
bytes, la mayor de 112.

**Y trece rutas no rompen ningún límite: rompen la gramática.**
`validate_source_path` admite `[A-Za-z0-9._/-]` y nada más, así que rutas como
`zerocopy-derive-0.8.56/src/output_tests/expected/into_bytes_enum.repr(i8).expected.rs`
—paréntesis— devuelven `SourceError::Invalid`, no `Limits`. Verificado de forma
independiente sobre el árbol real: 13 archivos.

Eso es lo que cambia la decisión. **Ninguna cuota mueve esas trece rutas.** Un
contrato nuevo con límites más generosos, por bien medidos que estén, seguiría sin
poder ingerir este árbol. Así que el contrato tiene que decidir explícitamente
sobre el **juego de caracteres de las rutas**, y no solo sobre cuántas hay y cómo
de grandes son.

Las opciones no son equivalentes y ninguna se elige aquí todavía:

- **Ampliar el alfabeto** a lo que un `.crate` publicado en crates.io puede
  contener legítimamente. Toca una frontera de seguridad —el alfabeto existe para
  que una ruta no pueda expresar cosas que el guest interprete—, así que exige su
  propio análisis de qué se vuelve expresable.
- **Codificar la ruta** en la captura y reconstruirla en el guest, dejando el
  alfabeto intacto. Mueve el problema a la fidelidad de la codificación.
- **Rechazar el paquete** que las contiene. Medido y ya descartado por otra vía:
  Cargo exige todo paquete del lockfile en un directory source.

**Y una tercera cosa, observada sobre nuestro propio fixture.** El árbol
materializado había acumulado un `.DS_Store` de 6 148 bytes que el fixture nunca
escribió, git-ignorado y por tanto invisible para todo lo que mira el repo. Medir
sobre él habría publicado 6 015 archivos y 156 273 617 bytes como forma del
cierre, en vez de 6 014 y 156 267 469.

Es exactamente la deriva de la que hablan §1 y §6 —un árbol del host que cambia
sin que nadie lo pida— pero observada en casa y sin malicia de por medio. Refuerza
la decisión de §1: capturar y autenticar, en vez de montar un directorio mutable y
confiar en que nadie lo toque.

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

## Los límites, ahora que las mediciones existen (2026-09-09)

§4 no autorizaba ningún número hasta tener memoria, disco, tiempo y concurrencia
medidos. Están en
[el recibo](../validation/M5-01-vendor-capture-measurements.json). Estos son los
números y de dónde sale cada uno.

**Lo primero que dicen las mediciones es que la máquina no es la restricción.**
Con lectura incremental el pico de RSS sobre el suelo del intérprete es 2,7 MB
—sigue el tamaño del buffer, no el del árbol— frente a 171 MB si se residencia el
árbol y 341 MB si se residencia el artifact. Captura, verificación e ingesta del
cierre de 156 MB suman **1,00 s**. Dos capturas concurrentes cuestan 1,16× de
reloj cada una y **no cambian ningún resultado**: los diez artifacts concurrentes
dieron un único digest, idéntico al serie.

Eso obliga a ser honesto sobre qué justifica cada límite. Decir «lo medimos» de un
número que la medición no obliga sería el razonamiento circular que este ADR
prohíbe. Así que se separan:

| Límite | Valor | Qué lo justifica |
| --- | --- | --- |
| Buffer de lectura | 64 KiB | **Medido.** Es lo que fija el pico de memoria; con 1 MiB el pico sube a 4,4 MB sobre el suelo sin ganar tiempo (0,3706 s frente a 0,3792 s, dentro del ruido) |
| Bytes totales | 512 MiB | **Política, informada por medición.** A 156 MB el ciclo completo es 1,0 s; proyectado a 512 MiB son ~3,5 s de reloj y ~1,1 GB de disco del host entre captura y árbol ingerido. Es el punto donde el coste sigue siendo el de una operación interactiva |
| Entradas | 32 768 | **Política.** El cierre medido usa 6 793. El factor de holgura es deliberado y no se justifica por la medición: se justifica por no querer volver a esta decisión con el siguiente harness |
| Bytes por archivo | 8 MiB | **Argumentado, no elegido.** La curva acumulada del cierre real es 96,5 % de los bytes en archivos ≤ 1 MiB y el mayor es 1 670 630 B. Un límite derivado de la mediana (5 344 B) y uno derivado del máximo difieren en tres órdenes de magnitud, así que ninguno de los dos sirve. 8 MiB es ~5× el mayor archivo observado: deja pasar el cierre medido con margen y sigue rechazando un archivo que ningún vendor legítimo produce |
| Bytes por ruta | 200 | **Medido.** El máximo observado es 112 y el p99 es 104. El doble del máximo observado |
| Profundidad | 16 | **Medido.** El máximo observado es 9 |

Ninguno de estos números se eligió para que el cierre de criterion quepa. Se
comprueba al revés: **el cierre cabría también con la mitad de las entradas y un
tercio de los bytes totales.** Que quepa es consecuencia, no criterio.

### El alfabeto de rutas, que no es un límite

Trece rutas del cierre —las de `zerocopy-derive` con paréntesis— no rompen ninguna
cuota: rompen la gramática. **Se decide ampliar el alfabeto**, no codificar ni
rechazar el paquete, y se decide con su coste declarado:

- El alfabeto pasa a admitir además `()+,=@[]{}~` y espacio, que es lo que un
  `.crate` publicado puede contener legítimamente en nombres de archivo de tests.
- **Lo que no se admite sigue siendo lo que importa**: ningún byte de control,
  ningún byte no ASCII, ningún `\`, ninguna `:` , ningún componente vacío, `.` o
  `..`, ninguna ruta absoluta. La razón por la que el alfabeto existe —que una
  ruta no pueda expresar algo que el guest interprete como otra cosa— se conserva
  entera.
- **La ampliación es del contrato nuevo, no de `SourceBundle`.** `validate_source_path`
  no se toca: los flujos M2/M4 calificados siguen con su alfabeto cerrado.

La alternativa de codificar la ruta se descarta porque mueve el problema a la
fidelidad de la codificación y añade un camino donde el nombre que ve el guest no
es el que viajó. La de rechazar el paquete ya estaba descartada por medición:
Cargo exige todo paquete del lockfile.

### Dos decisiones que la implementación necesitó y este ADR no nombraba

Ninguna amplía ni estrecha un límite; las dos existen para que el único límite que
actúe sea el que este documento fija.

**El encuadre del artifact.** USTAR sin extensiones no puede expresar una ruta
cuyo último componente pase de 100 bytes, y el límite de 200 bytes por ruta que
fija la tabla de arriba admite un nombre de archivo de 150. Rechazarlo habría sido
añadir un límite que este ADR no nombra. El artifact es entonces USTAR con una
cabecera extendida pax `path=` para exactamente esas rutas, y con un nombre de
respaldo **deliberadamente inservible** (`PaxLongPath/<n>`): un extractor que
ignore pax falla de forma ruidosa en vez de producir un árbol plausible con
nombres equivocados. El cierre de criterion no llega nunca a esa rama; está para
que el alfabeto sea la única frontera.

**El orden canónico es por componentes, no por bytes.** En orden de bytes `a-b`
cae entre `a` y `a/b`, lo que rompería a cualquier lector que mantenga un
directorio en una pila mientras llega su subárbol. El orden por componentes es el
que ya produce un recorrido en profundidad ordenado, mantiene la memoria del
verificador en O(profundidad), y está fijado por su propia prueba.

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
