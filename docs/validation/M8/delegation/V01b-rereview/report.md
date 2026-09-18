# V01b — informe del revisor (Claude Sonnet 5, Medium, read-only sin tools; claude 2.1.268)

Invocación: `claude -p --model sonnet --effort medium --tools '' --disallowedTools Agent Task --no-session-persistence --output-format json < input.md` (input sha256 `1133587c57a80dd108ea844cff4beb770ea5ac674a46b96c32e58453cd2890ed`, 38 606 bytes). Inicio 2026-09-14T14:54:08Z, fin 2026-09-14T14:55:24Z, exit 0, 75703 ms.

---

# V01b — re-revisión del diff W04

## Findings

**P1 — Hecho nuevo sin evidencia en `client-configuration.md`.** El diff introduce el flag `--allow-analyzer-action-write WORKSPACE_ROOT` como grant obligatorio para `rust.analyzer.action.apply`, con comportamiento específico (`unavailable` sin el grant, "igual que ... M2"). Ninguna fuente del material (ADR-082, ADR-085, censo, disposición V01, ADR-086) menciona este flag ni esta semántica antes de W04. Es una afirmación técnica concreta y verificable (nombre de flag, comportamiento de fallback) que no venía en el encargo de V01/W04 (que era: enmendar ADR-086 §1, añadir nota `preview`/deuda, y actualizar estado de integración de M6). Antes de integrar, el orquestador debe verificarlo contra `crates/mcp-server/src/main.rs` (que aparece modificado en el working tree) o contra el código real del gateway de M6; si el flag no existe con ese nombre/comportamiento, es un hecho fabricado en documentación pública.

**P2 — Ninguno adicional.** El resto del diff son actualizaciones de estado (integración M6, PR #20/`e50c3fe`) y la nueva sección de política de deprecación en `compatibility.md`, ambas trazables y coherentes con ADR-086 enmendado y con `01.md`.

**P3 — Redundancia menor.** La nota de clase `preview`/deuda de M6 queda duplicada casi textualmente en `client-configuration.md` y `compatibility.md`. No es un defecto, pero si difieren en el futuro divergirán; no bloquea.

## Verificación de cierres

- **F-A**: cerrado correctamente. `experimental` (spec §57, opt-in por namespace/metadata, sin uso hoy) e `internal` (no anunciados) quedan definidos sin contradecir spec §57/§58; `internal` es una categoría propia de ADR-086 para elementos no-tool, no una reinterpretación de las tres categorías de la spec.
- **F-D**: cerrado. La definición de «consumidor real» (recibo de cliente stock **o** e2e nativo por el wire) y la obligación de ejercicio por cliente stock en M8-04 antes de RC1 aparecen consistentes en ADR-086 §1, `01.md` §2 y la nueva sección de `compatibility.md`.
- **F-G**: cerrado. `stock_client_invoked` en el JSON (33 sí / 3 no, verificado por el orquestador) coincide exactamente con la columna «Cliente stock» de `01-census.md` §2.
- **F-M**: cerrado. Tanto `client-configuration.md` como `compatibility.md` añaden la nota de clase `preview` y la deuda de contrato conocida para las cinco tools M6.
- **F-C, F-E, F-F, F-H**: sin cambios pendientes en este diff; siguen resueltos en `01.md` como registró V01.
- **Sin contradicción ADR-086 §1 vs spec §57/§58 ni vs ADR-012**: no se detecta ninguna.
- **Sin reescritura de texto histórico**: los cambios en README/compatibility/client-configuration describen estado *actual* (integración de M6), no reescriben las descripciones de releases pasadas (`0.1.0`/`0.3.0` quedan intactas).

## Veredicto

**Approve con findings** (1 P1, 0 P2 adicionales, 1 P3) — condicionado a que el orquestador verifique el flag `--allow-analyzer-action-write` contra el código real (`main.rs` modificado) antes del commit; si se confirma, el diff queda limpio para integrar.
