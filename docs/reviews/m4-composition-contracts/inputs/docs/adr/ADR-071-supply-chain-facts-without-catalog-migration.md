# ADR-071 — D22, facts de supply chain con fuentes independientes

## Status

Accepted para M4-04. Implementación y calificación pendientes.

## Context

El catálogo SQLite schema 1 conserva yanked y features conocidas, pero no registra
el source ni checksum completo de cada paquete. Cargo.lock capturado sí puede
registrarlos. Mezclar ambos sin indicar la fuente convertiría una ausencia del
catálogo en una afirmación falsa. Git y registries ajenos no tienen fuentes offline
calificadas para el motor deny inicial; sus facts siguen siendo útiles.

## Decision

No hay migración SQLite ni nueva adquisición. El catálogo y sus antirollback,
firma, sequence y freshness permanecen en los ports existentes. La tool compone
una captura ProjectRef, una auditoría RustSec, una ejecución de deny cuando sus
inputs estén disponibles y facts del lock/metadata obtenidos por el adapter.
No ejecuta tools MCP desde aplicación. Cada motor conserva su estado: una falla
de deny no elimina audit ni los facts capturados; unknown/partial no produce pass.

Cada paquete conserva nombre, versión, clase de source, digest del source literal,
checksum si está registrado, duplicados y features declaradas frente a activas.
El locator Source/URL se omite por completo del resultado y artifacts; solo se
expone su clase y hash de bytes literales, sin userinfo, query ni fragmento secreto;
no se resuelve por red. Lo observado en lock no se presenta como verificación de
bytes: solo el dataset vendor autenticado puede acreditar el checksum del paquete.
Las features activas vienen exclusivamente de metadata congelada calificada;
cuando no existe, permanecen unknown. No se infieren de features del catálogo.

Yanked se consulta por nombre y versión exacta a una sola generación autenticada
del catálogo, con fingerprint/sequence/provenance/freshness. Version/crate ausente,
catálogo no disponible y no consultado por límite son unknown distintos. Las
referencias a advisories del catálogo nunca sustituyen el matcher RustSec.

Budget: trabajo Tasks 1..120 s (default 120), grafo hasta 4096 paquetes, 128 filas
visibles y 128 consultas de yanked por orden determinista. Filas/findings/consultas
omitidas se cuentan; toda reducción por el máximo de 512 KiB del resultado MCP
completo hace parcial la respuesta. El informe normalizado se publica por el store
M3, con owner/TTL/retención/provenance existentes y sin texto de diagnósticos.

La respuesta comunica facts y cobertura. No calcula un score, certificación de
seguridad ni aprobación legal. Unsafe y Miri mantienen sus preguntas independientes.

## Alternatives considered

- Migrar SQLite para replicar campos del lock: duplica autoridad sin nuevos inputs.
- Usar URLs convencionales o features del catálogo como hechos activos: inferencia
  sin evidencia del grafo ejecutado.
- Fallar todo si falta catálogo/vendor: elimina findings y facts útiles ya capturados.
- Consultar registries o Git en runtime: contradice el modelo offline y la autorización.

## Consequences

Rollback conserva los formatos persistidos y retira la tool. Deben calificarse
por separado known/unknown yanked, fuentes Git/registry, checksum declarado frente
a verificado, duplicados/features y freshness. Una futura adquisición enriquecida
o cambio de schema requiere su propio ADR y calificación; no queda autorizado aquí.
