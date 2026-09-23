# ADR-085 — Admisión del runtime M6 en el gateway

Fecha: 2026-09-11.

## Status

Accepted para la admisión de la imagen; la calificación de las tools se registra
aparte. La evidencia nativa del corte M6-01 vive en `docs/validation/M6/01*` y no
queda concedida por esta decisión.

## Context

`RustGateway::new` admite un conjunto cerrado de digests: la imagen M1
`384a1742…`, la base de seguridad M4 `95dddeb5…`, la imagen final M4
`25ed3626…` y la imagen M5 `e0a5ca16…`. Una imagen fuera de esa lista es
`ExecutionError::InvalidConfiguration`; no hay tag mutable ni descubrimiento.

M6 necesita `/opt/analyzer/bin/rust-analyzer` y `rust-src` bajo `/opt/rust`, que
no existen en ninguna de las cuatro. [ADR-082](ADR-082-m6-runtime-provisioning.md)
construyó la imagen derivada y su [recibo](../validation/M6/provisioning.json)
verifica, sobre la imagen ya construida, que ambos componentes están presentes,
que `rust-analyzer` **no** es alcanzable por `PATH`, que los binarios M3/M4/M5
siguen intactos y que el contexto de construcción no dejó residuos.

[ADR-084](ADR-084-rust-analyzer-runtime-and-lsp-lifecycle.md) fija el lifecycle
LSP que consume ese runtime, incluidas la versión, el `positionEncoding`
negociado, el oráculo de readiness, el schema de configuración y el árbol de
procesos esperado, todos marcados «subject to native calibration». Esta decisión
solo admite el digest; no acredita ninguna de esas mediciones.

## Decision

Se admite exactamente un digest M6. La línea siguiente es la **única** de este
documento que declara cuál es, y es la que el gate lee para comprobar que la
decisión y el código no se han separado; cualquier otro digest que aparezca aquí
es historia, no admisión:

**Digest admitido:** `sha256:f39a5b33ee7d54243664162eb635f8ec223d512042beb7cd18ecf071046b310c`

**Alcance de la admisión.** Ese digest entra en la lista **global** de
`RustGateway::new` —y en la del host—, igual que hizo
[ADR-077](ADR-077-m5-runtime-admission.md) con la imagen M5: la lista dice qué
imágenes puede ejecutar el gateway, no qué tool puede usar cada imagen. La
consecuencia, escrita aquí para que no quede implícita, es que un host
configurado con la imagen M6 puede invocar las tools M1–M5 sobre ella, y **eso
no está calificado**: cada una de esas tools sigue calificada contra su propio
digest, que es el que corren sus suites nativas en el gate `full`, y ninguna de
ellas se ha ejecutado sobre M6. Lo que sí se sabe de la diferencia es que es
**aditiva**: ADR-082 construyó M6 a partir del digest M5 admitido añadiendo
`/opt/analyzer` y `rust-src` bajo `/opt/rust`, y su recibo verifica sobre la
imagen construida que los binarios M3/M4/M5 siguen intactos y que
`rust-analyzer` no es alcanzable por `PATH`. Esa limitación se declara en
`docs/compatibility.md` como parte de M6-06; hasta entonces, el uso calificado
de la imagen M6 son las tools analyzer.

Además, la fase analyzer del gateway exige esa imagen **y solo esa**: cualquier
otro digest devuelve `Unavailable` antes de crear contenedor alguno. La versión
de `rust-analyzer` y el sha256 de su binario que el resultado declara son
propiedades de ese digest —la imagen se admite *por digest*, así que un binario
con otra versión es otra imagen—, de modo que ejecutar una sesión sobre otra
imagen produciría una declaración que el producto no puede sostener. Que esas dos
constantes describan el binario real es lo que comprueba la calibración nativa,
leyendo `/usr/share/doc/rust-runtime/m6/rust-analyzer-version.txt` e
`installed.json` del guest vivo y cruzándolos con el recibo de
aprovisionamiento; sin esa comprobación serían una afirmación, no una medición.

Las cuatro imágenes anteriores conservan su admisión y su alcance: M6 no las
sustituye, no las amplía y no cambia lo que ellas pueden ejecutar. Las tools M1
a M5 siguen calificadas contra sus propios digests, y una imagen M5 recibe
`unavailable` en cualquier tool analyzer, que es el resultado correcto y
declarado.

## Alternatives considered

- **Admitir por tag.** Descartado: un tag es mutable y la lista existe
  justamente para que la identidad del runtime no dependa de un nombre.
- **Sustituir la imagen M5 por la M6.** Descartado: invalidaría la calificación
  nativa M5 sin volver a ejecutarla, y G5 prohíbe que un gate anterior acredite
  bytes nuevos.
- **Permitir cualquier imagen que contenga el binario.** Descartado: convertiría
  una comprobación de identidad en una inspección de contenido, y el contenido de
  una imagen no autenticada no es evidencia.
- **Probar la versión del binario en cada llamada en lugar de fijarla.**
  Descartado: añadiría un contenedor por consulta para averiguar algo que el
  digest ya fija, y no detectaría nada que la admisión por digest no detecte
  antes. La calibración nativa sí lee el guest, que es donde esa lectura aporta
  evidencia.

## Consequences

Un host configurado con la imagen M5 sigue sirviendo las treinta y una tools
anteriores y recibirá `unavailable` en las tools analyzer cuando existan. El
rollback de M6 es apuntar el gateway al digest M5: no hay estado que migrar, la
evidencia se conserva y los planes de acción ligados a la identidad M6
(`config_digest`/`binary_sha256`, ADR-083 §6) se revocan por comparación en vez
de aplicarse contra otro runtime. Cambiar la imagen M6 exigirá un digest nuevo,
una decisión nueva y una calificación nativa nueva.
