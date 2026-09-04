# ADR-019 — Embeddings locales y reproducibles

Date: 2026-09-03

## Context

La query semantic debe embedderse sin API externa y funcionar en español e inglés.
El runtime/modelo afecta licencia, distribución, CPU, RAM y soporte de targets.

## Decision

Definir `EmbeddingProvider` en ports y un `LocalEmbeddingProvider` adapter. Baseline
M1: `fastembed` 6.0.2 con `intfloat/multilingual-e5-small` (MIT, 94 idiomas), ejecución
ONNX CPU. Deshabilitar la feature de descarga `hf-hub` en el runtime del servidor y
cargar archivos user-defined únicamente desde el model artifact validado por el
manifest. Usar prefijos `query:`/`passage:` y registrar dimensión/normalización.

El model ID incluye repositorio, revision inmutable, hashes de tokenizer/config/model,
runtime y parámetros. El artifact se distribuye/importa por separado del binario.
Antes de estabilizar M1 debe superar un benchmark ES/EN de búsquedas Rust, startup,
latencia, RAM, tamaño y CI x86_64/ARM64 en los OS anunciados. Si falla, este ADR se
sustituye; no se cambia el modelo silenciosamente. Ausencia/incompatibilidad degrada
a búsqueda lexical con provenance explícita.

## Alternatives considered

- BGE-small-en-v1.5: default ligero de fastembed, pero no cubre bien consultas ES.
- all-MiniLM-L6-v2: pequeño y Apache-2.0, principalmente inglés.
- BGE-M3/modelos grandes: más capacidad/multilingüe, mayor costo de distribución.
- APIs externas: mejor operación administrada, incompatible con el core offline.

## Consequences

La inferencia síncrona se ejecuta fuera del reactor Tokio con concurrencia acotada.
El modelo no se descarga durante `tools/call`; doctor explica cómo importarlo.

## Status

Accepted, condicionado al gate de benchmark y plataforma antes del RC.

Sources: <https://docs.rs/fastembed/6.0.2/fastembed/>,
<https://huggingface.co/intfloat/multilingual-e5-small>

