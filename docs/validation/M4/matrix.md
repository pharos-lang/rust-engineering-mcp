# M4 — matriz de implementación y calificación

Fecha: 2026-09-08. Rama de trabajo: `ai/m4-security`. Base:
`c66a3704e1ad290603a3c1d10413df90d15c2b03`.

Estado: **Done local — 2026-09-08**.
El encargo autoriza M4 y sustituye el límite histórico de AGENTS. Las 27 tools
están implementadas, con Tasks por defecto y modo síncrono hasta 60 s; GateV2
síncrono admite solo `strict` sin mutation. Versión `0.3.0-dev`, sin publicación
ni autorización M5. [Handoff](handoff.md).

## Entrada M3

El [recibo de comprobación](prerequisites.json) recalcula los 810 inputs de
`scripts/gate.py` (46027636 bytes) contra core y full M3. Las dos diferencias son
vacías; ambos gates pasaron 14/14 y 25/25 respectivamente. SHA-256 canónico del
inventario inicial: `791856ed9325c55ade73809207e971d3fd76c0bc3abd73c5b97ead54dff0ac9e`.
No se volvieron a ejecutar gates del mismo código.

La revisión final M3 anterior precede al delta de portabilidad/Sonar y al bump
de versión integrado. Se preparó una revisión Opus 5 High read-only de
`e2ec7da..c66a370`, con 50 archivos (incluido `sonar-project.properties`):
[inputs](../../reviews/m4-prerequisite/inputs.json),
[diff](../../reviews/m4-prerequisite/delta.patch) y
[prompt](../../reviews/m4-prerequisite/prompt.md).
La [revisión](../../reviews/m4-prerequisite/review.md) no detectó regresiones de
producto, pero emitió P2-1 por exclusiones Sonar sobre archivos con pruebas
portables. Se retiraron todas las exclusiones Rust, se corrigió la documentación
y se incluyó `sonar-project.properties` en el inventario del gate. Los dos
oráculos nuevos fallan contra HEAD anterior y la suite corregida pasa 11/11:
[recibo](../../reviews/m4-prerequisite/sonar-verification.json). La [confirmación Opus](../../reviews/m4-prerequisite/confirmation/review.md)
aceptó el prerrequisito sin P0/P1/P2. P3-2 (hashes documentales stale) también corregido. P3-1/3/4 se conservan
como deuda explícita para evaluar en M4-06, sin afirmar que se hayan corregido.

Los recibos M3 acreditan los bytes de producto de la base. Los scripts de gate
corregidos se verifican con el recibo focalizado anterior; los fixtures M4 nuevos
no están calificados por los gates M3. El gate M4 final debe incluir todos esos
inputs nuevos y la configuración de Sonar, sin reutilizar el inventario de 810
como si describiera la implementación futura.

## Cortes

| Corte | Estado | Evidencia |
| --- | --- | --- |
| M4-01 deny | Done | [Deny adversarial](deny-adversarial.json), [MCP/rollback](deny-mcp.json), audit único y suppressions exactas. |
| M4-02 unsafe.scan | Done | [Siete oráculos nativos](scanner-native.json), origen/omisiones, presupuesto parcial y cleanup. |
| M4-03 Miri | Done | [13 clasificaciones y 7 admisión/lifecycle](miri-native.json), warning exacto, panic ordinario y UB separados. |
| M4-04 supply_chain.inspect | Done | [Core](core-gate.json): 22 tests security/Supply, seis controles de catálogo firmado; [MCP](tools-mcp.json). |
| M4-05 strict/release | Done | Seis tests aplicación GateV2, perfiles nativos, baseline y mutation explícitos; [MCP](tools-mcp.json). |
| M4-06 hardening/cierre | Done | [Runtime 19/19](runtime.json), [mapa](hardening-map.md), gates/clientes y revisión de código pasados. |

## Puertas comunes

| Gate | Estado M4 | Evidencia / condición |
| --- | --- | --- |
| G1 contratos | Passed | [Core 19/19](core-gate.json), protocolo 44/44; cinco snapshots nuevos y 23 previos idénticos. |
| G2 autoridad/seguridad | Passed | [Runtime 19/19](runtime.json), [privacidad](privacy-runtime.json), [canarios](output-canaries.json), catálogo y parsers en core/full. |
| G3 lifecycle/budgets | Passed | [Lifecycle final](miri-task-lifecycle.json); [300/300 presupuestos](budgets.json) sobre binario histórico explícito, máximo 17044 ms; [continuidad](benchmark-source-continuity.json) y rutas síncronas actuales verificadas por G4. |
| G4 clientes/native | Passed | [Inspector 2.5.0 y Codex 0.153.0, intento 4](clients.json); 27 tools, cinco M4 reales, Resources, Tasks/negativos en Inspector, sync/modelo en Codex; [runtime 19/19](runtime.json). |
| G5 gates | Passed local | [Core 19/19](core-gate.json), [full 33/33 reanudado](full-gate.json), mismos 987 inputs. Full retiene 27 pasos aprobados y ejecuta los seis restantes tras recuperar E5 local exacto; [driver](full-gate-resume-driver.py), [recuperación](e5-local-recovery.json). Sin código cambiado, descarga ni skip. Sin CI/Sonar remotos ni calificación de host Linux/x86_64; falta linker cross local. |
| G6 rollback | Passed | [Deny MCP](deny-mcp.json): retira admisión al volver a M3, nueva llamada unavailable y mismo artifact privado v1 releído por su owner. |
| G7 inventario | Passed | [Inventario pasivo](runtime-inventory.json), seis binarios/sysroot exactos; [imagen alterada](tampered-plugin.json) rechazada y cleanup verificado. 247 inputs aprovisionados con autorización. |
| G8 review | Passed | [Sonnet](../../reviews/m4-composition-confirmation/review.md), [Gemini](../../reviews/m4-traceability/review.md), [Opus final de código](../../reviews/m4-final-closure/review.md): sin P0/P1/P2 nuevos. [Confirmación final Opus](../../reviews/m4-final-evidence/review.md) acepta cierre local, sin P0/P1/P2 abiertos. |
| G9 DoR/DoD | Passed | D19–D22 decididos; [disposición final del Technical Owner](../../reviews/m4-final-evidence/disposition.md), [verificación exacta](final-verification.json) y [handoff](handoff.md). |

## Aprovisionamiento autorizado

El owner autorizó la adquisición exacta de ADR-066. La imagen resultante es
`sha256:95dddeb5305f10b09b441e3cc4018ebb1a8a296d365c65106327d59f933c64e7`;
[provisión](provisioning.json), [Miri preparado](prepared-miri.json) y
[contención base](base-calibration.json) tienen resultados reales. El fallo
histórico de `cargo miri setup --print-sysroot` se conserva: ese comando intenta
aprovisionar aunque exista MIRI_SYSROOT; el test Miri con sysroot preaprovisionado
sí pasó y no modificó sus bytes. Ningún test básico sustituye la vertical Miri.

El [inventario](provisioning-proposal/manifest.json) fija 247 inputs; el build
se ejecutó en Docker sin red. M3 permanece inmutable. El helper D20 final deriva la imagen `25ed3626…91635`,
con provisión y recalificación separadas en ADR-068/069.

[G5](../../roadmap/m2-m8.md) exige: «Si falta una dependencia, registrar la ausencia
y aprovisionarla solo con autorización explícita separada». El [README del
fixture](../../../fixtures/rust-runtime/README.md) también exige autorización antes
de ejecutar provisioning. Esa autorización ya se recibió en esta sesión; la
aprobación técnica de imagen/sandbox depende después de resultados y revisión,
y permanece a cargo del Technical Owner.

## Handoff

[Handoff M4](handoff.md) conserva identidad, resultados finales, intentos
fallidos, límites y disposiciones. Core, full y clientes vinculan los mismos 987
inputs. Los recibos M3 preexistentes permanecen intactos. No hay commit, PR, tag,
release ni avance a M5. Rollback: imagen M3 y retirada de configuración M4,
conservando el formato del store.
