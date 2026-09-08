# ADR-077 — Admisión del runtime M5 en el gateway

Fecha: 2026-09-08.

## Status

Accepted para la admisión de la imagen. La calificación nativa de las cuatro
tools se registra por separado en `docs/validation/M5-*` y no queda concedida por
esta decisión.

## Context

`RustGateway::new` admite un conjunto cerrado de digests: la imagen M1
`384a1742…`, la base de seguridad M4 `95dddeb5…` y la imagen final M4
`25ed3626…`. Una imagen que no esté en esa lista es
`ExecutionError::InvalidConfiguration`; no hay tag mutable ni descubrimiento.

M5 necesita `/opt/perf/bin/cargo-bloat` y `/opt/perf/bin/rust-mcp-profile-helper`,
que no existen en ninguna de las tres. [ADR-075](ADR-075-m5-runtime-provisioning.md)
construyó la imagen derivada y su [recibo](../validation/M5-provisioning.json)
verifica, sobre la imagen ya construida, que los dos binarios están presentes,
que **ninguno** de los dos es alcanzable por `PATH`, que los binarios M3/M4 siguen
intactos y que el contexto de construcción no dejó residuos.

## Decision

Se añade exactamente un digest a la lista de admisión:

```text
sha256:e9ecc40d023d9d13ac3539cccb6a944cd1022da2a8b3f86ca61356086b38a209
```

Además, el puerto de performance exige esa imagen **y solo esa**: cualquier otro
digest devuelve `Unavailable` antes de crear contenedor alguno. Las versiones del
analizador y del helper que el resultado declara son propiedades de ese digest,
así que ejecutar una medición sobre otra imagen produciría una declaración que el
producto no puede sostener.

Las tres imágenes anteriores conservan su admisión y su alcance: M5 no las
sustituye, no las amplía y no cambia lo que ellas pueden ejecutar. Las tools M1
a M4 siguen calificadas contra sus propios digests.

## Alternatives considered

- **Admitir por tag.** Descartado: un tag es mutable y la lista existe justamente
  para que la identidad del runtime no dependa de un nombre.
- **Sustituir la imagen M4 por la M5.** Descartado: invalidaría la calificación
  nativa M4 sin volver a ejecutarla, y G5 prohíbe que un gate anterior acredite
  bytes nuevos.
- **Permitir cualquier imagen que contenga los binarios.** Descartado: convertiría
  una comprobación de identidad en una inspección de contenido, y el contenido de
  una imagen no autenticada no es evidencia.

## Consequences

Un host configurado con la imagen M4 sigue sirviendo las veintisiete tools
anteriores y recibe `unavailable` en las cuatro nuevas, que es el resultado
correcto y declarado. El rollback de M5 es apuntar el gateway al digest M4: no hay
estado que migrar y la evidencia se conserva. Cambiar la imagen M5 exigirá un
digest nuevo, una decisión nueva y una calificación nativa nueva.
