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
construyó la imagen derivada y su [recibo](../validation/M5/provisioning.json)
verifica, sobre la imagen ya construida, que los dos binarios están presentes,
que **ninguno** de los dos es alcanzable por `PATH`, que los binarios M3/M4 siguen
intactos y que el contexto de construcción no dejó residuos.

## Decision

Se admite exactamente un digest M5. La línea siguiente es la **única** de este
documento que declara cuál es, y es la que el gate lee para comprobar que la
decisión y el código no se han separado; cualquier otro digest que aparezca aquí
es historia, no admisión:

**Digest admitido:** `sha256:e0a5ca1661b3e49d0a3d68ee3cc0963453078d08eb7fc43c30538c16b7998aac`

Además, el puerto de performance exige esa imagen **y solo esa**: cualquier otro
digest devuelve `Unavailable` antes de crear contenedor alguno. Las versiones del
analizador y del helper que el resultado declara son propiedades de ese digest,
así que ejecutar una medición sobre otra imagen produciría una declaración que el
producto no puede sostener.

Las tres imágenes anteriores conservan su admisión y su alcance: M5 no las
sustituye, no las amplía y no cambia lo que ellas pueden ejecutar. Las tools M1
a M4 siguen calificadas contra sus propios digests.

### Enmienda de 2026-09-09 — el primer digest M5 queda sustituido

El digest admitido inicialmente por esta decisión fue
`sha256:0e21c561488cb917e89e42943eb5138a7ddfd73d9de2f9cd4b9a0b516bdab820`. Una
revisión independiente de containment encontró que el binario perfilado podía
sobrescribir los artifacts del propio perfilador, y la corrección cambió el
helper. La imagen que lo contiene es por tanto otra:

```text
sha256:e0a5ca1661b3e49d0a3d68ee3cc0963453078d08eb7fc43c30538c16b7998aac
```

**Sustituye, no acompaña.** La lista de admisión vuelve a tener exactamente un
digest M5. Admitir los dos dejaría admitido un runtime cuyo helper tiene el
defecto corregido, y ninguna calificación puede acreditar a los dos a la vez.
El digest anterior nunca llegó a `main`, nunca se publicó y no acreditó ninguna
release; lo que sí produjo son recibos, que **se conservan sin tocar** con el
digest que realmente midieron. El [recibo de aprovisionamiento anterior](../validation/M5/history/inventory.json)
se archiva completo junto al nuevo.

Toda la evidencia nativa capturada sobre el digest anterior queda invalidada por
esta enmienda y se vuelve a capturar sobre el nuevo, que es la consecuencia que
la propia decisión ya anunciaba. La reconstrucción es la del mismo script, sin
red (`--network=none`, `pull=false`), sobre la misma base M4 verificada por
digest: `cargo-bloat` sale byte a byte idéntico —`e3eaea0d…`, 1 644 120 bytes— y
solo cambia el helper —`70fa813d…` → `18eaac41…`—, que es exactamente lo que se
corrigió y nada más.

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
