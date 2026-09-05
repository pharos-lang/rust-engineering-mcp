# ADR-051 — Editor semántico de manifests M2

Date: 2026-09-05

## Context

M2-01 comienza con lints del Cargo.toml raíz. La edición debe conservar texto
ajeno a la operación y permanecer separada de autorización, publicación y Cargo.
El parser `toml` de admisión M1 no conserva el formato para editar.

## Decision

Usar `toml_edit =0.25.13` (`0.25.13+spec-1.1.0`), solo features parse/display,
en project-adapter. Domain contiene valores cerrados y application un port de
edición pura consumido por la vertical de mutación. No modificar el parser M1.
Licencia MIT OR Apache-2.0; MSRV 1.85 compatible con Rust 1.98.1. La versión está
en caché local y contiene la corrección upstream de strings con largas secuencias
de comillas. No habilitar serde ni unbounded para este editor.

La primera operación admite set/remove de lints package/workspace, namespaces
rust/clippy y niveles allow/warn/deny/forbid con prioridad i64 opcional. Solo
Cargo.toml raíz; nombres ASCII alfanuméricos/underscore, 1–128 bytes. Workspace
requiere tabla workspace existente; un manifest virtual rechaza lints package; lints locales rechazan workspace=true.
No aceptar paths TOML arbitrarios. Rechazar tablas inline o claves dotted en la
ruta tocada inicialmente; no reordenar ni formatear globalmente el documento.
Un no-op devuelve los bytes originales. Set conserva decoración del valor previo;
remove elimina el comentario asociado al item eliminado, nunca los vecinos.
UTF-8, límite 256 KiB y reparse del candidato son obligatorios. El editor no
certifica semántica Cargo: el candidato aún requiere el oráculo Cargo aislado y
la transacción calificada antes de publicarse.

## Alternatives considered

- Reescribir con `toml`: pierde comentarios/formato.
- Reemplazos textuales: ambiguos frente a tablas, herencia y valores TOML.
- Editor TOML general público: amplía autoridad y superficie sin necesidad.

## Consequences

Se añaden toml_edit y toml_datetime al lock y al inventario de dependencias.
Las demás familias M2 se incorporan con tipos y fixtures propios; no se anuncian
variantes sin camino implementado. El editor no cambia permisos ni contratos M1.

## Status

Accepted por el Technical Owner. D03 inicial resuelto; calificación end-to-end pendiente.
