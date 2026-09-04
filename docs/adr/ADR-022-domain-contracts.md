# ADR-022 — Tipos e invariantes del dominio base

Date: 2026-09-03

## Context

M0-02 necesita modelos utilizables y pruebas, sin adelantar MCP, registro de
proyectos, hashing de workspaces ni sandbox. ADR-006/007/020 fijan semántica pero
dejan abiertos representación de identificadores, mapping de errores y umbrales.
El owner actualizó el toolchain/MSRV a 1.98.1 en `cafe721`.

## Decision

Añadir únicamente `rust-engineering-domain`. Depende de Serde 1.0.229 (derive);
serde_json 1.0.151 se usa solo en tests. Ambas versiones están publicadas, con
licencia MIT OR Apache-2.0 y MSRV inferior a 1.98.1. Cargo.lock fija el grafo.
No hay dependencias de adapters, I/O, procesos ni protocolo. JSON Schema y DTOs MCP
permanecen en M0-03/07; las pruebas de serialización base no acreditan MCP.

- `ProjectRef`: `prj_` y 32 caracteres hexadecimales minúsculos (128 bits de
  representación). Parsear no autoriza ni demuestra aleatoriedad. El generador
  criptográfico, registro y expiración se implementarán en M0-04.
- `ProjectIdentityFingerprint` y `ExecutionFingerprint`: newtypes incompatibles,
  formato `sha256:` y 64 hex minúsculos. M0-02 valida digests, no calcula hashes ni
  decide su preimagen. Esa decisión se toma en las verticales que conocen los inputs.
- Constructores y deserialización aplican las mismas invariantes; campos que
  permiten estados inválidos son privados. Los objetos rechazan campos desconocidos.
  Errores de validación son tipados y no reflejan el contenido inválido.
- `OutputEnvelope<T>` mantiene `status`, `summary`, `duration_ms`, `error_code`,
  `error_message`, `diagnostics`, `truncation`, `data` tipado y `evidence`. Los dos
  campos de error son obligatorios pero nulos para `passed`, `failed` y cancelación.
  No hay `serde_json::Value` en producción.
- Resultados de proyecto solo usan `passed`/`failed`, sin error operacional. Errores
  `TOOL_NOT_INSTALLED`/`UNSUPPORTED_PLATFORM` usan `unavailable`; los otros siete
  códigos de §69 usan `blocked`, incluido timeout. Cancelación usa `cancelled`, sin
  inventar otro error_code; el adapter futuro la tratará como error operacional.
  Solo `passed` representa éxito. JSON-RPC/internal errors no pertenecen al dominio.
- `evidence` discrimina `local` o `snapshot`; snapshot contiene provenance y
  freshness inseparables. Esta agrupación concreta el ejemplo conceptual de §17
  antes de existir un contrato MCP; no se añaden tools.
- Provenance conserva source kind, source ID, created/observed at, integridad y
  `network_used` de la fuente histórica. Timestamps son segundos UTC desde Unix
  epoch, no texto de fecha ambiguo. Observación no puede preceder a creación cuando
  ambas se conocen. Ninguna de esas marcas por sí sola demuestra integridad.
- Freshness usa edad desde `created_at` y un `Clock` inyectable consumido por la
  evaluación. `age <= fresh_for_seconds` es fresh; hasta `stale_after_seconds`
  inclusive es aging; por encima es stale. Se exige fresh_for < stale_after.
  Fecha de creación ausente o futura respecto al reloj produce unknown con edad nula, nunca
  resta saturada que simule fresh. Persistir assessed_at y policy permite validar
  de nuevo al deserializar. `network_used=true` histórico nunca convierte snapshot
  en live; no hay constructor live en el dominio snapshot M1.
  Deserializar verifica la evaluación histórica en assessed_at, no su actualidad;
  el consumidor debe reevaluar con el reloj vigente antes de tomar decisiones.
- Diagnósticos admiten ubicación ausente, múltiples spans, sugerencias multipartes
  y reemplazo vacío. Líneas/columnas son 1-based (columnas en Unicode scalar values),
  extremo final exclusivo; bytes opcionales son 0-based y final exclusivo. Rangos
  invertidos se rechazan, rangos vacíos permiten inserciones. Paths son evidencia
  textual, no autoridad filesystem. La normalización de rustc/Cargo y redacción
  operativa se implementan posteriormente.
- Truncation conserva flags de stdout/stderr y número de diagnósticos omitidos.
  Es metadata, no enforcement de límites/streaming; no altera automáticamente status.

## Alternatives considered

- Strings/JSON genérico y validación tardía: permite mezclar handles/digests y omitir
  validaciones al deserializar.
- Dos Options independientes para provenance/freshness: admite evidencia parcial.
- Freshness provista por el caller sin contraste: permite afirmar fresh/live con
  fechas o policy incompatibles.
- Añadir ahora application, adapters, schema engine y hashing completo: invade otros
  cortes y crea fronteras sin consumidor real.

## Consequences

Se pueden construir, serializar y validar resultados completos con diagnósticos y
evidencia, aunque el binario todavía solo ofrece ayuda/versión. El crate futuro de
aplicación consumirá estos tipos. M0-04 debe probar generación y autorización real;
M0-05/06 límites y sandbox; M0-07 schemas y wire output MCP. No se presenta ninguno
de esos gates como cumplido por tests del dominio.

## Status

Accepted.

Sources: <https://serde.rs/container-attrs.html>,
<https://docs.rs/serde/1.0.229/serde/>,
<https://docs.rs/serde_json/1.0.151/serde_json/>,
<https://doc.rust-lang.org/rustc/json.html>
