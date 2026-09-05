# M2 — revisión e integración del Technical Owner

Continuación del handoff del 2026-09-05 desde `331d163`, rama
`ai/m2-write-qualification`. Se preservaron todos los cambios previos. Esta sesión
conserva arquitectura, scope M2, decisiones ADR-050..059 y contratos M1; no añadió
código de producción ni cambió scripts, fixtures o configuración del gate.

## Revisión y decisiones

Se leyó la especificación completa y se contrastaron manifests, lockfile, CI,
contratos, código de mutación/resolución y evidencia real. Las dependencias
normales siguen siendo domain→Serde y application→domain; rmcp, Cargo/Docker,
TOML y filesystem permanecen en adapters. El delta del lock respecto de `331d163`
solo cambia la versión de los ocho paquetes propios a `0.2.0-dev`.

La retirada terminal usa una marca de la asignación concreta y poda antes de
admitir nuevos planes. Replay conserva grant vivo, root física, tipo, ID/digest/key,
locks y journal verificado antes de recovery o migración; no acepta un candidato
del caller ni interpreta ausencia como permiso para un efecto nuevo.
Application y publisher repiten la comprobación de scope, y Cargo/fmt/fix actúan
en staging sin write bind host. No se encontró motivo para cambiar estos contratos.

Se conservan los paquetes Accepted de contrato, seguridad, writer y observabilidad.
El [inventario comparativo](M2-closure-review-coverage.json) registra sus hashes y
los deltas posteriores; no extiende automáticamente su cobertura a bytes nuevos.
El [recheck ADR-059](M2-059-review.md) cubre replay/retención/descripciones/oráculos.
La auditoría Sol/Medium detectó el delta posterior de resolución; se extrajo y
verificó el source anterior del prompt histórico y se obtuvo una
[revisión Opus acotada](M2-closure-resolution-review.md), Accepted sin P0/P1/P2.

El worker documental Sol/Medium fue read-only: no ejecutó Cargo, Docker ni edits.
Detectó el texto de plan no vencido en SECURITY, tablas de cliente obsoletas,
recibos pre-059 aún usados como finales, estados pendientes y atribución de hashes
anteriores al cierre. El owner corrigió cada grupo usando recibos propios, sin
convertir intentos fallidos en pases. Los prompts originales disponibles de las
siete revisiones finales y ambas ADR-059 se copiaron con SHA iguales a sus inputs.
La auditoría AGY final y la integración tienen recibos separados en el cierre.

## Límites de revisión

La sesión principal se identifica como GPT-6; el host no expone aquí confirmación
de la configuración solicitada Sol/High y no se afirma haberla cambiado. El worker
sí se solicitó explícitamente como GPT-5.6 Sol/Medium. Claude Code 2.1.260 se invocó
con Opus 5/high, sin tools ni MCP, y registró ese modelo más uso auxiliar Haiku.
Los modelos aportan revisión estática, no certificación de ejecución.

El [cierre verificable](../validation/M2-07.md) registra el resultado final del full,
cliente, build core y smoke. El owner conserva los residuales de taxonomía, coste,
observabilidad y remediación del store dentro de sus límites publicados. No se
inicia otro corte de mantenimiento, M3 ni distribución en esta continuación.
