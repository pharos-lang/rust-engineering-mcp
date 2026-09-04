# ADR-013 — Mutación segura fuera de M1

Date: 2026-09-03

## Context

M1 observa y valida, aunque Cargo pueda escribir artifacts. Editar source, manifests
o dependencias amplía autoridad, locking y recuperación.

## Decision

No exponer tools que modifiquen source/config/dependencias en M1. Sus roots de source
se montan o controlan read-only cuando el sandbox lo permita; target, temp y artifact
roots son escrituras separadas. Futuras mutaciones M2 requieren permiso host
explícito, cambios estructurados, diff, precondiciones por fingerprint, write lock y
rollback/operación atómica.

## Alternatives considered

- Habilitar `cargo fix`/fmt desde M1: mejora ergonomía, pero mezcla validación y
  mutation antes de estabilizar seguridad.
- Permitir escritura general en workspace: contradice least privilege.

## Consequences

El agente modifica código con sus propias capacidades. Una tool read-only puede
escribir artifacts de build, pero debe declararlo y nunca modificar source o lockfile.

## Status

Accepted.

