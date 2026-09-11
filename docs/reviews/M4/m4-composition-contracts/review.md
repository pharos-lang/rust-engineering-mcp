# Revisión independiente — Contratos de composición M4 (`rust.supply_chain.inspect` y `rust.quality.gate.v2`)

**Revisor:** Claude Sonnet 5 (read-only) — evidencia exclusiva: `docs/reviews/M4/m4-composition-contracts/inputs/` e `inputs.json`.

## Veredicto acotado

Sobre el snapshot revisado, las invariantes centrales de composición (una captura, un audit, revalidación de authority, strict/release/baseline, no-pass ante evidencia parcial/unavailable, cierre de esquemas, cardinalidades y presupuestos) están **implementadas y en su mayoría probadas correctamente**. Encontré **un defecto de correctness verificable en el código provisto** (doble aplicación de policy/suppressions en `compose_security`) y **varios huecos de prueba materiales**, sobre todo en rutas "no configurado"/cardinalidad-límite/mutación exitosa/truncación 512 KiB en runtime. No encontré ninguna ruta que produzca `Passed` con evidencia demostrablemente parcial dentro de lo que el snapshot permite verificar. Esta revisión **no** abre gates ni acredita M4 Done por sí sola (ver Conclusión).

## Cobertura

Leídos y correlacionados: ambos ADR (067, 071); `domain::{supply_chain, quality_v2}`; `application::{security, supply_chain, quality_v2}`; `mcp-server/stdio::{supply_chain(+schemas), quality_v2(+schemas)}`; tests de aplicación (`security.rs`, `quality_v2.rs`), tests de contrato MCP (`security_contract_tests.rs`) y tests de catálogo (`catalog/supply_tests.rs`). **No** se leyó `domain/src/security.rs` (no forma parte del snapshot); es la fuente real de `SecurityFinding::apply_policy`, `SecurityPolicy`, `DenyOptions`, etc. Esto limita la verificación de algunos hallazgos (marcado explícitamente abajo).

---

## Findings

### P1 — Doble aplicación de policy/suppressions sobre findings de deny

**Archivo/líneas:** `crates/application/src/security.rs:337-343` (función `compose_security`, usada por ambas tools vía `application/src/supply_chain.rs:182-191` y `application/src/quality_v2.rs:325-332`).

```rust
for row in &mut deny.findings {
    row.apply_policy(policy, assessed_at.0);      // 1ª aplicación
}
findings.extend(deny.findings.iter().cloned());
for row in &mut findings {
    row.apply_policy(policy, assessed_at.0);      // 2ª aplicación (incluye los mismos deny findings)
}
```

**Explicación:** El primer bucle es necesario para que `deny.findings` (conservado en `SecurityObservation.deny` / persistido en el artifact) refleje la disposición de suppression. Pero el segundo bucle reaplica `apply_policy` sobre **todos** los findings combinados, incluidos los clones de `deny.findings` que ya fueron procesados. Esto contradice literalmente el requisito "que suppressions y policy se apliquen una sola vez, con identidad exacta". No puedo confirmar si `apply_policy` es idempotente porque `domain/src/security.rs` no está en el snapshot autorizado.

**Escenario discriminante:** Un fixture donde `apply_policy` tuviera cualquier efecto no puramente funcional/determinista sobre el mismo `SecuritySuppression` (p. ej. contadores de uso, elección no determinista entre reglas superpuestas, mutación de `expires_at` derivada, etc.) produciría un resultado distinto entre una sola aplicación y dos. Los tests actuales (`exact_suppression_preserves_original_and_does_not_repair_missing_audit_data`) sólo comprueban el resultado final, no el número de invocaciones — no discriminan este defecto si el efecto es idempotente pero tampoco lo prueban si no lo es.

**Corrección mínima:** Aplicar `apply_policy` una sola vez: hacer el segundo bucle sólo sobre los findings de audit (antes del `extend`), o eliminar el primer bucle y, tras el `extend`+segunda pasada, volver a escribir la disposición resultante en `deny.findings` desde `findings` (por identidad) en vez de mutar dos veces el mismo objeto lógico.

---

### P2 — Huecos de prueba: rutas "no configurado" (Deny `NotConfigured`)

**Archivos:** `application/tests/quality_v2.rs` (todo el fichero) y `application/tests/security.rs` (tests de `supply_chain_durable`).

Ningún test invoca `quality_gate_v2` con `vendor: None`/`policy: None`, ni `supply_chain_durable` con `SupplyInputs{vendor: None, policy: None, ..}`. Estas rutas existen explícitamente en el código:
- `application/src/quality_v2.rs:363-365` → `security_failure(kind, SecurityError::MissingOfflineData)`.
- `application/src/supply_chain.rs:140-142` → `SupplyAvailability::NotConfigured`.

**Escenario discriminante:** Un regreso accidental (`Ok`/`Passed`/`Available`) en vez de `Unavailable`/`NotConfigured` cuando el host no entrega vendor/policy no sería detectado por ninguna prueba incluida.

**Corrección mínima:** Añadir un test por tool que ejecute la composición sin vendor/policy y aserte `report.status == Unavailable` (gate v2) / `deny_availability == NotConfigured && !report.complete` (supply chain).

---

### P2 — Huecos de prueba: cardinalidades de runtime (4096/128) y truncación 512 KiB

**Archivos:** `application/src/supply_chain.rs:164-174` (rechazo si `graph.packages.len() > 4096`; truncado a 128 con `packages_omitted`), `mcp-server/src/stdio/supply_chain.rs:403-440` y `mcp-server/src/stdio/quality_v2.rs:425-476` (bucle de `trim_one` bajo presupuesto de 512 KiB).

Los tests de `security_contract_tests.rs` sólo verifican que un **fixture estático** con 129 elementos falla la validación de **esquema** (`assert_invalid`), no que la ruta de **ejecución real** (`supply_chain_durable`/`quality_gate_v2` → `encode_result`) trunque correctamente, incremente `*_omitted`, marque `complete=false` y jamás devuelva `Passed` cuando el payload serializado excede 512 KiB. No hay ningún test que fuerce `graph.packages.len() > 4096` para ejercitar el `InvalidMetadata` de `supply_chain.rs:165`.

**Escenario discriminante:** Una regresión en `trim_one()` o en el bucle `loop { … }` de `encode_result` (p. ej. no volver a fijar `complete=false`, o encode "Passed" antes de medir tamaño) pasaría inadvertida.

**Corrección mínima:** Un test de aplicación con un grafo de 4200 paquetes fixture (para el rechazo `>4096`) y un test de integración en `mcp-server` que fuerce un reporte grande (muchos `findings`/`packages` dentro de los límites de cardinalidad pero con payload textual amplio) y verifique que el resultado final serializado ≤512 KiB, `complete=false`, y `status != Passed`.

---

### P2 — Hueco de prueba: mutación nunca se ejecuta con éxito

**Archivo:** `application/tests/quality_v2.rs:505-515` — `ProjectMutationTestPort::run` del `Executor` de test siempre devuelve `Err(InspectionError::Internal)`.

Ningún test ejercita las ramas `value.clean()`, `value.conclusive_failure()`, o el `Blocked` intermedio en `application/src/quality_v2.rs:444-479`, ni la clasificación `MutationBaseline::{Passed,Failed,Missing}`. El único test relacionado con mutation (`invalid_mutation_budget_is_rejected_before_candidate_capture`) sólo cubre el rechazo de admisión por presupuesto, no la ejecución.

**Corrección mínima:** Añadir casos con un `ProjectMutationTestPort` que devuelva observaciones "clean", "conclusive failure" y "inconclusive/blocked" para validar el mapeo a `ToolStatus` y la integración en `report.status`/`report.complete`.

---

### P2 — Provenance del texto libre en `SecurityFinding`/`DenyObservation` no verificable en el snapshot

**Archivo:** `application/src/security.rs:131-135` (`DenyObservation::validate` acota `f.rule.len() > 96 || f.message.len() > 512`, pero no elimina contenido).

ADR-067 exige "Elimina todos los mensajes, labels, notes y campos libres del guest" para el artifact de deny. El snapshot no incluye `domain/src/security.rs` (donde se construye `SecurityFinding`/`DenyObservation` desde la salida real de cargo-deny), así que no puedo confirmar si `message`/`rule` son texto canónico fijo por regla o texto derivado del guest. Los límites de longitud (96/512) sugieren capacidad para texto variable, lo cual sería inconsistente con el requisito "sin campos libres del guest".

**Corrección mínima (si aplica):** El Technical Owner debe confirmar en el adapter de deny (fuera de este snapshot) que `message`/`rule` provienen de un mapeo cerrado código→texto fijo, no de `stderr`/`labels` crudos del plugin.

---

### P3 — `deny.lock_fingerprint` no se compara directamente contra `graph.lock_fingerprint`

**Archivo:** `application/src/supply_chain.rs:164-169` compara `audit.lock_fingerprint` contra `graph.lock_fingerprint`; `compose_security` (`security.rs:293-296`) compara `audit.lock_fingerprint` contra `deny.lock_fingerprint`. La equivalencia `deny.lock_fingerprint == graph.lock_fingerprint` sólo se sostiene transitivamente cuando `audit.lock_fingerprint` es `Some`. Si audit es `Unavailable` (`lock_fingerprint: None`), ambas comparaciones se omiten y no hay verificación directa deny↔graph. Actualmente no es explotable porque ambos derivan del mismo `source_fingerprint` capturado una sola vez, pero es una dependencia implícita frágil.

**Corrección mínima:** Añadir comparación directa `deny.lock_fingerprint != graph.lock_fingerprint → InvalidMetadata` en `supply_chain.rs`, independiente del estado de audit.

### P3 — Reset redundante de `complete` en el stdio de supply chain

**Archivo:** `mcp-server/src/stdio/supply_chain.rs:439` (`data.observation.report.complete = false;`) es redundante: `SupplyReport::trim_one` (`domain/src/supply_chain.rs:196-199`) ya fija `self.complete = false` internamente. No es dañino, sólo ruido de mantenimiento.

---

## Invariantes verificadas correctas (con evidencia directa)

- **Una sola captura candidata y un solo audit por composición**, contado con `AtomicUsize` en tests (`security.rs:466`, `quality_v2.rs:751`).
- **Strict rechaza baseline / Release lo exige**, `QualityV2Options::validate` (`application/src/quality_v2.rs:22-34`), probado en `invalid_mutation_budget_is_rejected_before_candidate_capture` y en la construcción de `opened()`.
- **Baseline capturado/inspeccionado por separado y revalidado al publicar** (`application/src/quality_v2.rs:207-220,532-539`); SemVer nunca confunde generaciones (aserción explícita `assert_eq!(baseline, &source(false)); assert_eq!(candidate, &source(true))` en `quality_v2.rs:483-484`).
- **Revalidación de fingerprints deny↔source/runtime/vendor/policy** antes de componer y de publicar, con `InvalidMetadata` en cada mismatch (`security.rs:289-299`, `supply_chain.rs:144-158`), probado exhaustivamente (`source_runtime_lock_vendor_and_policy_mismatches_are_rejected`, `supply_rejects_graph_lock_and_deny_runtime_mismatches_before_publication`).
- **Expiración de policy verificada antes, durante y después de cada etapa**, y en publicación (`policy_expiry_during_any_engine_stage_rejects_publication`).
- **Revocación de owner/authority antes/después de publicar nunca produce éxito** (`durable_deny_rejects_revocation_and_policy_expiry_during_publication`, `publication_error_and_revocation_after_publisher_revalidation_never_return_evidence`, `supply_rejects_owner_revocation_and_publication_failure`).
- **Audit/deny/catálogo stale, unknown o parcial nunca marcan `complete=true`** (`stale_or_unknown_audit_cannot_pass_a_clean_deny`, `stale_or_unknown_supply_evidence_never_marks_report_complete`).
- **Omisiones de findings nunca desaparecen del cómputo de completitud** (`audit_or_deny_omissions_never_false_pass`).
- **Suppressions exigen identidad exacta (engine+rule+package+source+versión) y no reparan auditorías incompletas** (`suppression_requires_exact_engine_rule_package_source_and_version`, `exact_suppression_preserves_original_and_does_not_repair_missing_audit_data`).
- **Yanked exact-version/catálogo**: distingue `Yanked/NotYanked/VersionAbsent/CrateAbsent/CatalogUnavailable/NotApplicable/NotConsulted`, detecta manipulación de firma/hash/secuencia, reevalúa freshness fresh→stale sobre la misma generación autenticada, y el provider fija una generación por request (`catalog/supply_tests.rs`, cobertura sólida).
- **Esquemas cerrados** (`deny_unknown_fields` en inputs/outputs, `additionalProperties:false`) con pruebas adversariales explícitas rechazando campos extra, enums forjados y arrays sobre cardinalidad (`security_contract_tests.rs`).
- **Presupuestos de timeout**: input schemas fuerzan 1..120 (supply, default 120) y 1..3600 (gate v2, default 300); el presupuesto derivado de mutation + 300s se valida contra el timeout global antes de capturar (`QualityV2Options::validate`).
- **Un resultado recortado nunca "pasa"**: la lógica de `encode_result` en ambos stdio (`supply_chain.rs`, `quality_v2.rs`) recalcula el outcome tras cada `trim_one`, siempre degradando a `Blocked`, aunque la ruta de ejecución completa con payload realmente sobredimensionado no está cubierta por tests (ver P2).

## Limitaciones explícitas

1. **`domain/src/security.rs` está fuera del snapshot.** No pude inspeccionar `SecurityFinding::apply_policy`, `SecurityPolicy`, `DenyOptions`, ni la construcción real de `message`/`rule`. El hallazgo P1 y la observación P2 sobre texto libre dependen de ese archivo para confirmación definitiva.
2. No se leyeron `execution-adapter`, `domain/src/lib.rs`, `application/src/lib.rs` ni el resto de tools M1-M3 (fuera del alcance acordado); cualquier invariante que dependa de esos contratos (p. ej. semántica exacta de `ToolStatus`/`QualityIssue`, `SecurityPolicy::fingerprint()`) se acepta tal como se usa en el snapshot, sin verificación independiente.
3. No se ejecutó código ni se corrieron los tests; el análisis es puramente estático sobre el texto fuente provisto.
4. El contexto de main (caso MCP strict pasado, fixture release bloqueada por lock incompleto, fixture positiva en progreso) se trató como contexto, no como evidencia; no se usó para ajustar el veredicto.

## Pruebas faltantes (resumen accionable)

- Deny `NotConfigured`/`MissingOfflineData` en ambas tools (aplicación).
- Rechazo por `packages_total > 4096` en `supply_chain_durable`.
- Truncación real bajo presupuesto de 512 KiB en el runtime de `encode_result` de ambas tools (no sólo el rechazo de esquema sobre fixtures estáticos).
- Ejecución de mutation con resultados `clean`/`conclusive_failure`/inconcluso.
- Un test que verifique explícitamente que la disposición de suppression es idéntica si `apply_policy` se invocara una sola vez vs. dos (para blindar contra el hallazgo P1 independientemente de si hoy es idempotente).

## Conclusión

Esta revisión identifica un defecto de correctness verificable (P1, doble aplicación de policy) y varios huecos de prueba materiales (P2) en las rutas "no configurado", cardinalidad límite, truncación 512 KiB y mutación exitosa. Las invariantes centrales de captura única, auditoría única, revalidación de authority, y no-pass ante evidencia parcial están correctamente implementadas y en su mayoría probadas. **Esta revisión, por sí sola, no abre gates ni acredita M4 Done**; la disposición final de cada hallazgo corresponde al Technical Owner, incluida la verificación de los puntos que dependen de `domain/src/security.rs`, fuera del alcance de este snapshot.
