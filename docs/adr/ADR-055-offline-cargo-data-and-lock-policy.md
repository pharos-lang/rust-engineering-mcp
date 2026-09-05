# ADR-055 — Datos Cargo offline y preservación de la presencia del lock

Date: 2026-09-05

## Context

Add/remove y ciertos cambios de features necesitan resolver dependencias. El
catálogo SQLite no es un registry Cargo. M1 usa CARGO_HOME vacío y ninguna tool
puede descargar datos. D05 comparó dos fuentes reales con Cargo 1.98.1 del image
aprobado: local registry y directory source, incluyendo alias, features, optional,
transitivas y datos ausentes/corruptos.

Directory source requiere más bytes/archivos en el fixture, pero su productor
`cargo vendor` ya forma parte de Cargo. Evitar otra instalación pesa más que esa
diferencia acotada de ingest. El probe detectó que metadata puede aceptar un
Cargo.toml vendorizado alterado; por tanto no prueba integridad del dataset.

## Decision

Usar directory source provisto explícitamente por el host, optativo para las
operaciones que requieren dependencias. No afecta instalación ni uso M1, lints o
fmt. El operador prepara fuera del MCP un directorio mediante Cargo habitual:

```text
cargo vendor --locked --versioned-dirs /ruta/privada/vendor
rust-engineering-mcp cargo-vendor inspect --directory /ruta/privada/vendor --json
```

El primer comando es una instrucción administrativa para el desarrollador, no una
invocación automática del runtime MCP. Para agregar crates nuevas, el operador
incluye versiones aprobadas en un manifest de provisioning con su lock; vendor de
la lock actual solo contiene las dependencias ya resueltas. Se admite el `--sync`
habitual de Cargo para agrupar manifests de provisioning. No se añade ni copia la
configuración generada por vendor al proyecto.

El host configura el par cerrado `--cargo-vendor-dir PATH` y
`--cargo-vendor-tree-sha256 sha256:DIGEST`. La CLI inspect captura/verifica el árbol
y devuelve digest, conteos y paquetes, sin ejecutar Cargo ni descargar nada. No
retorna bytes de source. El runtime captura esos mismos bytes mediante handles
no-follow, verifica el digest esperado y conserva el valor inmutable usado por el
gateway; no valida por hash y vuelve a abrir los paths después. El dataset queda
fuera de las roots de proyecto y nunca se monta por bind desde el host.

Límite inicial: 16 MiB, 4096 archivos/directorios, 1 MiB por archivo y el mismo
subconjunto portable de paths de SourceBundle. Es una fuente selectiva aprobada,
no import de todo CARGO_HOME. Se rechazan links, hardlinks, archivos especiales,
ownership ajeno, escritura group/other, cambios durante captura y layout fuera
del contrato antes de Cargo. Directorios e inodes se revalidan; sin adapter nativo
protegido no se inspecciona el filesystem. El árbol es de datos, no de proyectos:
su captura no relaja ni reutiliza las exclusiones de configuración Cargo de M1.

Cada paquete de primer nivel contiene Cargo.toml y `.cargo-checksum.json` estricto.
Se verifica cada archivo listado, su SHA-256 y ausencia de archivos extra salvo el
checksum. Todos los paths son relativos al paquete; no duplicados ni escapes.
Se exige `package` checksum SHA-256 no nulo para paquetes crates.io; Git y registries
alternativos quedan fuera de este corte. Nombre/version de Cargo.toml deben ser
válidos, únicos como pareja, y coincidir con el directorio versionado. El digest
de árbol cubre paths ordenados y bytes con longitud u64 little-endian, igual al
probe D05. Se rechazan directorios no implícitos en los paths de archivos: así la
topología autorizada queda determinada por esos paths, sin directorios vacíos
adicionales fuera del digest. Checksum es control de integridad; la autorización de esos bytes procede
del fingerprint esperado por el host, no del checksum autoafirmado por el paquete.

El gateway ingiere los bytes aprobados en un volumen tmpfs separado acotado, con
guardian y lifecycle de ADR-053; tras ingest no quedan escritores y todos los jobs
Cargo montan los datos read-only. Solo argv/config constantes del gateway activan
crates-io→directory source. Network none, HOME/CARGO_HOME efímeros y ninguna credencial.
Resolver usa metadata offline en staging escribible y luego metadata frozen sobre
esa resolución. Un fallo descarta el candidato completo, incluso cuando Cargo
alcanzó a escribir Cargo.lock. La validación liga dataset, runtime, ambos comandos
y hash del lock resuelto, sin afirmar que resolvió lo más reciente en Internet.

Política host de M2: `preserve_presence`. Si Cargo.lock existe, actualizarlo junto
con los manifests en el mismo plan. Si no existe, usar un lock transitorio durante
resolución/validación y excluirlo explícitamente del candidato publicable, dejando
constancia de ese hecho y de su hash. Se conserva la elección observable del repo;
no se infiere application/library de targets ni se crean archivos host nuevos.
La forma de exportación agrega una entrada esperada para el lock transitorio;
un source sin lock que ya ocupe las 4096 entradas falla cerrado por límite antes
del job. Metadata completa tiene límite de salida de 1 MiB por stream; truncación
impide validar y nunca produce candidato parcial.
Cargo crea el lock nuevo con modo 0644. La entrada de decoder específica de
resolución admite ese modo únicamente para el Cargo.lock raíz transitorio, con
los mismos controles de nombre/tipo/UID/GID/checksum/límites; ese archivo se retira
del candidato antes de publicar. Un lock preexistente y todos los demás archivos
siguen exigiendo 0600. No se amplía el decoder de fmt/fix ni se normalizan permisos
de bytes desconocidos en el host.
El preview/receipt declara esta policy. La validación no promete que una futura
lectura M1 frozen sin lock pueda resolver; los comandos M1 conservan su contrato.
No se permite manifest-only silencioso cuando faltan datos: devolver un resultado
operacional `unavailable` con razón `offline_data_missing` e `isError: true` y no publicar. Datos corruptos permanecen `blocked` con `offline_data_invalid`; un fallo Cargo del candidato permanece `failed`. Esto concreta la distinción de disponibilidad de D01 sin cambiar enums ni schemas M1.

La validación de ambos documentos metadata acredita cada manifest de paquete
contra bytes capturados bajo `/source` o el dataset aprobado bajo `/rust-mcp-vendor`.
Las identidades de paquetes vendor deben coincidir con el inventario verificado;
paquetes locales y patches dentro del source siguen ligados al fingerprint completo
del proyecto. No se describe todo el grafo como procedente del vendor. Se compara
la resolución con la revalidación frozen y sus documentos quedan ligados por SHA
al fingerprint de ejecución, junto al decoder de export. La ausencia identificable
de un crate/versión en el directorio offline devuelve un motivo de datos ausentes;
no dispara descargas ni acepta un lock sin ese oráculo.

## Alternatives considered

- Local registry: compacto y calificado como fixture, pero exige otro productor o
  implementar su administración. Se conserva como evidencia, no como instalación.
- Importar CARGO_HOME: incorpora configuración, credenciales, red y estado mutable.
- Descargar automáticamente durante preview: rompe offline-first y la autorización
  explícita del host.
- Crear Cargo.lock por defecto en cualquier repo: impone una decisión al developer
  y exige ampliar la primitiva nativa de publicación. Preservar presencia evita
  ese cambio; una futura policy de creación requiere ADR/calificación propia.

## Consequences

El camino habitual mantiene cero servicios instalados adicionales. Solo quien use
resolución offline administra un dataset Cargo seleccionado. Los límites iniciales
son explícitos; no se anuncian workspaces arbitrarios ni soporte cross-platform
positivo. El dataset puede contener código ejecutable por build.rs/proc macros;
verificar sus bytes no lo convierte en confiable fuera del sandbox.

## Evidence and primary sources

- [Probe directory source](../validation/M2-D05-vendor-qualification.md) y
  [comparación resumida](../validation/M2-D05-vendor-summary.json).
- [Probe local registry](../validation/M2-D05-offline-registry-qualification.md).
- [Cargo vendor oficial](https://doc.rust-lang.org/cargo/commands/cargo-vendor.html).
- [Cargo source replacement](https://doc.rust-lang.org/cargo/reference/source-replacement.html).

## Status

Accepted. M2-04/05, captura nativa, gateway, límites y publicación conjunta
calificados en [M2](../validation/M2-07.md).
