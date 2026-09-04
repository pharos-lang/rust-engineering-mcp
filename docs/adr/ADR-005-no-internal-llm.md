# ADR-005 — Sin LLM interno en el core

Date: 2026-09-03

## Context

El consumidor ya es un agente. Un LLM interno duplicaría razonamiento, agregaría red,
credenciales, costo y resultados difíciles de reproducir.

## Decision

No incorporar inferencia generativa ni APIs de OpenAI, Anthropic, Gemini u otros LLM
en el core o M1. El servidor entrega evidencia y operaciones deterministas. Los
embeddings locales son una capacidad de retrieval delimitada, no un LLM generativo.

## Alternatives considered

- LLM remoto para explicar/recomendar: mejor lenguaje natural, pero rompe offline,
  privacidad y determinismo.
- Modelo generativo local: evita red, pero aumenta mucho distribución y recursos.

## Consequences

Las explicaciones provienen de rustc y los facts de fuentes locales. Un LLM futuro
sería adapter opcional, open-world y requeriría ADR separado.

## Status

Accepted.

