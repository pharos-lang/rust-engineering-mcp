# ADR-069 — D20, scanner AST aislado por archivo

## Status

Accepted como diseño M4-02. Implementación y calificación pendientes.

## Context

El scanner debe distinguir sintaxis real, strings/comentarios y macros sin ejecutar
código para expandirlas. Un AST recursivo en el servidor MCP permitiría que un
archivo hostil agotara su pila, incluso con 1 MiB de entrada y llaves poco profundas
(por ejemplo una cadena de operadores unarios). `catch_unwind` no contiene un
stack overflow. La captura limita 4096 entradas, 16 MiB total y 1 MiB por archivo.

La revisión de las fuentes oficiales fijadas en lock/cache confirma recursión en
`syn::buffer::TokenBuffer`, parser y Visit; `proc_macro2::FromStr` también documenta
posibles panics. Las dependencias ya adquiridas son syn 3.0.4
(checksum `e6275cddf4610d1775e6d1fe9469b2e77d0f39fd98fb7450901b821e0c53649f`)
y proc-macro2 1.0.107
(`985e7ec9bb745e6ce6535b544d84d6cd6f7ad8bd711c398938ae983b91a766d9`).

## Decision

Un helper privado dentro del runtime utiliza syn 3.0.4 `full,parsing,visit` y
proc-macro2 1.0.107 `span-locations`; sin nuevas versiones ni adquisición en runtime.
El supervisor inicia una copia del helper por archivo de forma secuencial. Esta
es una fase tipada del Execution Gateway: mismo ejecutable absoluto, un índice
validado de un manifiesto creado por el host, sin paths/flags del cliente ni shell.
El supervisor no parsea Rust. Cada hijo tiene un límite de 2 segundos; el gateway
impone el deadline global 1..120 s, memoria/CPU/PIDs/output y cleanup de todo el árbol.
Ningún caso hostil de parser se ejecuta sobre el host.

El manifiesto enumera determinísticamente archivos `.rs` capturados, con índices,
paths relativos autorizados, origen, paquete y fingerprints. El resultado interno
usa índices y spans de keyword, nunca texto libre de errores del parser. El host
valida rango, UTF-8, bytes `unsafe`/`extern`/atributo esperado, conteos, identidad y
límites. Una muerte/señal/parse error/timeout deja cobertura parcial y no elimina
resultados de otros archivos. El hijo conserva el AST hasta terminar el proceso,
evitando su drop recursivo; memoria queda contenida por el guest. El supervisor
continúa después de un archivo fallido, sin retry automático.

Se reportan unsafe block/fn/impl/extern block, extern block/fn/crate y las formas
modernas `unsafe mod` y `#[unsafe(...)]`, con span exacto del keyword. Los cuerpos
de macros son opacos y no cuentan como código expandido; se informa su omisión.
Los atributos cfg/cfg_attr propios o heredados hacen el hallazgo condicional;
no se evalúan. Línea/columna son 1-based, columna por caracteres UTF-8; bytes 0-based.
Se declaran `cfg_evaluated=false`, `macros_expanded=false` y
`generated_sources_scanned=false`. Ningún cero de findings significa seguridad.

La asignación a workspace/dependency usa raíces de paquete autenticadas de la
metadata congelada. Archivos capturados no asignables se conservan como
workspace_unowned; cobertura de dependencias requiere el dataset vendor esperado.
El resultado retiene 128 findings y conteos de omisión; máximo 512 KiB. No se usan
findings de unsafe como policy universal ni como veredicto de UB.

La tool recibe únicamente ProjectRef, timeout 1..120 (default 120) y modo de
ejecución. Reutiliza Tasks, worker único, captura y store M3. No requiere una
policy de deny ni ejecuta auditoría. El dataset vendor esperado sigue siendo
explícito del host. Metadata se obtiene con Cargo frozen/offline, sin compilación,
sobre los mounts existentes `/source`, `/rust-mcp-vendor` y `/security`.
La fase final del mismo gateway selecciona el helper absoluto y un manifiesto
`/security/scan.json` generado tras validar metadata. No hay expansión de macros.

Las raíces autenticadas asignan cada archivo a la raíz de paquete más larga;
los archivos vendor de paquetes fuera del grafo no forman parte de la selección.
El límite de 4096 archivos seleccionados abarca ambos orígenes. Los restantes
cuentan como omitidos y hacen parcial la cobertura. La respuesta agrega conteos
por origen y por resultado de archivo; cada finding conserva path relativo,
paquete, hash y span. La ausencia de findings con errores de archivo nunca produce
un resultado completo. El artifact contiene esta proyección normalizada, sin
código ni stderr del helper, y su productor Builtin se liga al digest del normalizador.

## Alternatives considered

- Regex/lexer propio: insuficiente para la sintaxis, macros y errores de Rust.
- syn en el MCP con guarda de llaves/catch_unwind: no controla la recursión real.
- Un único hijo para todo el proyecto: un archivo hostil elimina resultados ajenos.
- Expansión de macros mediante compilación: ejecución adicional fuera de este corte.

## Consequences

Un proceso por archivo tiene coste que debe medirse, sin afirmar latencia antes de
30 cold/30 warm. El helper es un nuevo asset: exige SBOM/licencias, build offline,
digest, imagen nueva y recalibración; ADR-068 no autoriza su sustitución silenciosa.
El runtime Miri ya preparado y deny permanecen inmutables hasta esa imagen derivada.
Rollback vuelve al digest previo y retira la tool sin migraciones de persistencia.
