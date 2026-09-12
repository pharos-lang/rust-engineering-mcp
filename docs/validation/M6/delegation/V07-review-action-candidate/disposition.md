# Disposición — revisión independiente V07 del candidato de acción (W07)

Fecha: 2026-09-12. Revisor: Claude Opus 5 (read-only, diff por stdin, 269 s),
distinto del worker. [Texto íntegro](claude-opus-5-review.md); hashes en
[inputs.sha256](inputs.sha256). Veredicto: **Approve con P3** — sin P0-P2. Las
propiedades críticas de la frontera de escritura se sostienen: digest inyectivo
e independiente del orden, edits fail-closed en adapter y dominio, todos los
`match` de `MutationKind` actualizados, un solo writer, el journal rechaza kind
desconocido/ajeno/reetiquetado; el plan queda ligado por digest (que cubre
`validation`).

Verificaciones del orquestador de los puntos que el revisor no veía en el diff:
`CodeActionKind::to_lsp` es inyectivo (7 cadenas distintas); **`apply_edits` ya
rechaza inserts coincidentes** (`pair[0].0 == pair[1].0`), así que el P3-1 era
infundado (la postcondición vive en el dominio, no solo en el adapter); los
rechazos del codec (Command/snippet/resource-op/external-uri/version-mismatch)
están cubiertos por los tests de W06; `mutation_digest` cubre `validation`.

| P3 | Disposición |
| --- | --- |
| Inserts coincidentes / postcondición | **Ya seguro** en el dominio (`apply_edits`). La inconsistencia listado↔apply (el listado no corre `structural_rejection`) se cierra en W08 |
| Sin round-trip encode→decode entre crates para el framing de `validation` | Aceptado → W08 añade el test cruzado |
| El writer no ata versión de `validation` al kind (defensa en profundidad; el digest ya liga ambos) | Aceptado → W08 usa la vista solo para el kind AnalyzerActionApply; el nivel-writer queda como defensa aceptada (digest-bound) |
| Prelude re-implementado | **Verificado paritario** por el orquestador (el camino de candidato añade `authorize` sobre los mismos pasos) |
| Quarantine se salta en el fallo temprano de `analyzer_source_fingerprint` | Aceptado → W08 lo mueve dentro del alcance de quarantine |
| Provenance con constantes (platform/rust/cargo) no observadas por el decoder | Aceptado como en `m2-fmt-apply-v1`; sin cambio |
| Digest doc sobrestima cobertura (no liga image_id/configuration_fingerprint) | Aceptado → W08 ajusta el doc; los edits sí van ligados |
| Cut nativo estrecho (no ejercita el puerto de producción ni `analyzer_action_candidate` ni el framing; sin caso Stale nativo) | Aceptado → el e2e nativo de W08 (aplicar por la tool → archivo cambia → receipt → invalidación de generación) lo cubre |
| Alcance: una acción puede reescribir hasta 128 `.rs` (incl. build.rs/vendored) | **Decisión de política**: W08 hace prominente la lista de archivos tocados en el preview; el diff exacto es la superficie de revisión (Opción A) |

Sin P0-P2. Los P3 materiales se cierran en W08 (que construye la tool y su
e2e); los verificados/infundados no requieren cambio. W07 no se integra solo
(no expone tool): se commitea junto con W08 como el vertical M6-04/05.
