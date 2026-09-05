# M8 — Stabilization 0.8–0.9 y readiness 1.0

Estado: **Planned**. Fuentes: spec §53–70, §80–85, §97 M8/1.0,
[ADR-012](../adr/ADR-012-semver-compatibility.md),
[ADR-047](../adr/ADR-047-publication-license-and-delivery.md),
[ADR-048](../adr/ADR-048-0.1.0-qualification-and-artifact-boundary.md).
Aplican [G1–G9](m2-m8.md). Entrada: M6 cerrado y M7 cerrado o Deferred documentado.
No exige una release 0.7 inexistente. No añade features ilimitadas.

## Resultado y alcance

0.8 resuelve inconsistencias/compatibilidad con migraciones explícitas y congela
contratos; 0.9 conserva el freeze, ensaya operación y produce candidatos reproducibles.
1.0 es una decisión de readiness por evidencia, no un cambio de número automático.
Fuera: nuevas tools de negocio, nuevos analizadores, self-update MCP, catálogos o
modelos oficiales sin decisión, remoto si M7 Deferred y ampliación tácita de targets.

## Cortes verticales

| ID | Resultado ejecutable end-to-end | Dependencias | Evidencia/gate | Tamaño |
| --- | --- | --- | --- | --- |
| M8-01 | Inventario de invocaciones reales→censo de contratos/errores/CLI/Resources/formatos→clasificación stable/preview/internal | Entrada, D11 | Cada elemento tiene owner, fuente, test, consumidor y límite; huérfanos=0 | M |
| M8-02 | Cliente anterior→cambio 0.8.x explícito→resultado/migration guide→freeze | 01, D11, alcance D13 | Comparación schema/behavior before-after; trece M1 sin escritura implícita | L |
| M8-03 | Instalación anterior→preflight/dry-run migración→nuevo formato→rollback permitido | 01, D12 | Backup/restore/crash/disk-full/unknown-version; floors no retroceden | XL |
| M8-04 | Un cliente/version/target→discovery→flujo positivo/negativo/cancel/Resource | 02 | Matriz wire/client completa para lo anunciado; skip=no calificado | L |
| M8-05 | Carga de referencia→medición startup/dispatch/RSS→presupuestos fijados→regression gate | 01, M5 | Hardware/OS/runtime y raw samples; control de ruido y límites | L |
| M8-06 | Guía pública→instalación real core→configuración→operación→incidente/rollback | 02–05 | README/CLI/tools/security/client docs reproducidos por tercero | L |
| M8-07 | Artifact por target→inventario/notices/SBOM→firma/provenance→descarga/smoke | 03–06, D13/D14 | Matriz native-positive/artifact-qualified; cadena source/tag/run/digest | XL |
| M8-08 | Threat model completo→auditoría independiente→correcciones→retest | 02–07 | Paquete Opus High y auditoría externa de seguridad con alcance declarado | L |
| M8-09 | RC1 0.9→soak/carga/crash/upgrade→RC2→readiness | 03–08 | Dos RC consecutivos con contratos iguales y gates verdes | L |

M8-01 incluye un gate de superficie: las propuestas suman aproximadamente 35 tools
antes de 1.0. Para cada una justificar tool frente a Resource/prompt, consumidor
real y coste de contexto; registrar consolidaciones aceptadas o descartadas antes
de freeze, con análisis de compatibilidad. No borrar contratos usados para lograr
un número arbitrario. Fuente: spec §9/20/78/85/116.1; D11, G1/G4.

Camino crítico: censo→cleanup/freeze→migración→clientes/distribución→auditoría→RCs.
M8-05 puede medir mientras se validan migraciones. Cualquier adapter de plataforma
adicional se planifica como subprograma D13 con oráculos nativos antes de integrarlo;
no se disfraza de cleanup. Tamaño total XL, dominado por targets y formatos reales.

## Contratos y política propuesta de deprecación

D11 se decide antes de M8-01; M8-02 verifica su aplicación y el alcance D13 antes
del freeze. Durante 0.x toda ruptura requiere minor y migration
notes; patch no cambia nombres, required fields, defaults, enums cerrados ni
semántica de errores. Un campo opcional puede romper un cliente exhaustivo: medir
compatibilidad antes de clasificarlo como aditivo. Evitar version obligatorio en
cada tool; usar metadata/capabilities del servidor y formatos independientes.

Propuesta: deprecaciones y reemplazos en 0.8.0; conservar el comportamiento durante
toda 0.8/0.9; retirar en 1.0 solo lo anunciado desde 0.8.0 y con migración probada.
Después del freeze no renombrar ni ampliar contrato; una ruptura necesaria reinicia
freeze y ambos RC. Desde 1.0, deprecación en minor 1.x permanece funcional todo 1.x
y eliminación solo en 2.0 (spec §58). Retirada de revisión MCP requiere decisión,
clientes afectados, alternativa y avisos por releases, no solo meses. Excepción de
seguridad: deshabilitar fail-closed con advisory y recuperación documentada; nunca
reinterpretar silenciosamente resultados. No cambiar ADR-012 durante planificación.

Domain/application solo cambian por correcciones de contrato/invariantes; ports se
consolidan si tienen consumidores y semántica equivalente. Adapter MCP conserva
rmcp; compatibilidad no justifica JSON-RPC propio. CLI de migración/validación solo
se añade si el inventario identifica un formato real que migrar. No añadir comandos
vacíos por aspiración. Las herramientas de migración reutilizan filesystem seguro
y transacción M2, jamás edición arbitraria de paths desde una tool.

## Matriz wire y clientes

Cada celda registra producto, SDK exacto, revisión MCP, transporte, cliente/versión,
OS/arquitectura, perfil, resultado, test y receipt. Empezar por las cinco revisiones
M1 y stdio; HTTP solo si M7 Go/cerrado y para revisiones admitidas. Ejercitar moderna
discovery o legacy initialization según corresponda, inventario/schemas/resultados,
Resources, unknown fields/URI/errores, bootstrap costoso, cancel/EOF/backpressure y
presupuestos. No alegar conformidad completa donde solo se probó un subconjunto.

Inspector y Codex stock son obligatorios; calificar Claude Code y Gemini CLI antes
de anunciarlos. Cursor y VS Code/Copilot se incluyen si el producto los promete;
de lo contrario la documentación distingue configuración de calificación. Nunca
convertir una plantilla de configuración en prueba. Guías deben ofrecer resultados
esperados de cada modalidad unavailable/degraded.

## Migración, recovery y seguridad

Censo separa config host, receipts/journal M2, jobs/artifacts M3, analyzer state,
catálogo autoritativo y derived index. Cada formato define reader/writer compatibles,
preflight, presupuesto de disco, backup, staging/durabilidad, verificación y receipt.
Catálogo no ejecuta SQL importado. Modelo/dimensión incompatible invalida/reconstruye
índice; SQLite no pierde facts. Floors, revocaciones y trust no retroceden junto con
binario. Journal pendiente impide downgrade; estado corrupto requiere recuperación
explícita. Un backup no justifica sobrescribir cambios del usuario posteriores.

Fixtures: versión desconocida, config antigua, migration interrumpida en cada
fase, disk full, permisos revocados, backup corrupto, inode/ancestor swapped,
symlink/reparse/hardlink, crash después de commit antes de respuesta, reintento
idempotente. Oráculo compara bytes/hash/estado anterior y posterior, permisos,
floor monotónico y receipts. Aplican unit, contract, protocol, integration,
security, adversarial y native por G4. Upgrade N-1→N y rollback N→N-1 se prueban
para cada formato declarado compatible; donde no lo sea se demuestra rechazo.

Threat review incluye dependencies comprometidas, secrets en source/evidencia,
poisoning del catálogo/modelo, escapes de containment y credenciales de publicación.
Cuotas/admisión/cancelación/entorno/red son las mismas G2/G3; mayor duración del soak
no permite límites ilimitados. Auditar permisos de journals y artifacts y su
retención/borrado. Review externa distingue modelo, humano, pentest y evidencia
primaria; una revisión automática no se denomina auditoría humana.

## Performance, operación y distribución

M8-05 fija presupuestos antes de RC a partir de M5: startup cold/warm, dispatch
sin Cargo, RSS idle/pico por perfil, normalización, tamaño binario/artifact, p95
de cancel-observed y tiempo cleanup. Repetir fixtures iguales con provenance y
variabilidad; presupuestos no son SLO hasta que exista entorno/operador. Soak
propuesto: 8 h por perfil anunciado y 1,000 ciclos mixed read/job/cancel con cuotas
pequeñas y controles de crecimiento; ajustar con decisión antes de medir, jamás
después de ver fallo. No leak/objetos huérfanos, memoria retenida bajo budget y
recovery ensayado. Reiniciar contador RC tras cambios materiales.

Mantener OIDC de ADR-047/048. D14 evalúa verificación offline/bundles y respuesta
a compromiso; firma organizacional adicional solo si un requisito demuestra su
necesidad, no imponer clave nueva ni dos operadores ficticios. Separar firma de
release de Ed25519 de catálogo. Inventario target-specific, licencias/notices,
SBOM, firma, tag/source/workflow/run/digests y smoke desde descarga limpia son
obligatorios; no afirmar reproducibilidad binaria por tener provenance.

D13 debe resolver el requisito **cross-platform 1.0**: definir exactamente hosts
positivos y artifacts previstos. El mínimo normativo de las tres familias OS no
se satisface con CI portable solamente. Si el owner opta por 1.0 limitada a macOS,
requiere aclaración explícita de spec/ADR antes de readiness; no marcar ese criterio
cumplido silenciosamente. No exige todos los triples aspiracionales de §61.
Linux/Windows positivos precisan adapters no-follow/reparse-safe, sandbox y tests
nativos. Local/model/catalog distribution siguen decisiones separadas D15/D16.

## Bug bars y gates

G1–G9 completos, security/native, compatibilidad/upgrade/rollback y release rehearsal
por cada target. P0/P1 bloquean cualquier merge/RC/release; P2 de seguridad, pérdida
de datos, contrato o gate obligatorio bloquea readiness. Otros P2 requieren
disposición del owner con alcance y mitigación verificable; deuda no puede suplir
un criterio 1.0. No publicar RC/tags/releases sin autorización de esa sesión.
Rollback: detener promoción ante regresión, cuarentenar estado incierto, servir
solo versión/formato compatible y volver a ejecutar el gate que falló.

## Checklist verificable 1.0 y DoD

Cada casilla requiere receipt enlazado en la futura matriz M8, no una declaración.

- [ ] Contratos stable, errores/defaults/annotations congelados, schemas y clientes
  anteriores probados. Fuente: spec §97 1.0/§53–59, ADR-012; M8-01/02/04.
- [ ] Security model coincide con enforcement y cada capability tiene oráculo nativo;
  auditoría independiente sin P0/P1. Fuente: spec §35–45/81/97, ADR-009; M8-08/09.
- [ ] Registro de riesgos residuales con ADR de aceptación, alcance, condición de
  reevaluación y re-review M8-08; security model público los enumera. Fuente:
  spec §36/81/97, D02 y G2/G8.
- [ ] SemVer/deprecación y excepciones se documentan y prueban por release, incluyendo
  enums exhaustivos. Fuente: spec §53–59; M8-02, D11.
- [ ] Cross-platform resuelto explícitamente con evidencia positiva por familia
  anunciada o cambio de alcance aprobado, nunca portable=positivo. Fuente:
  spec §61/97 1.0 y ADR-048; M8-07, D13.
- [ ] CLI estable, exit codes/JSON/config/permissions y recuperación reproducidos
  por tercero. Fuente: spec §110/111/97; M8-03/06.
- [ ] Guías de integración probadas por cliente/version/perfil; docs públicas sin
  texto histórico que contradiga el estado actual. Fuente: spec §113/114; M8-04/06.
- [ ] Matriz protocolo/SDK/cliente conserva resultados y límites de conformidad;
  HTTP no anunciado si Deferred. Fuente: spec §8/59/97, ADR-023; M8-04.
- [ ] Upgrade/rollback y crash recovery pasan para todas las versiones declaradas;
  floors/trust no retroceden. Fuente: spec §34.16/97, ADR-041; M8-03/09.
- [ ] Signing/provenance/checksums/SBOM/notices verificados desde assets descargados,
  con fuente y target exactos. Fuente: spec §66/112/97, ADR-047/048; M8-07, D14.
- [ ] Dos RC consecutivos con mismo contrato, full gate y soak verdes dentro de
  budgets, sin skips como pass. Fuente: spec §80–82/97 M8 y G4/G5; M8-05/09.

DoR: entrada anterior verificada, censo/decisiones priorizados, entornos de migración
y clientes reales, D11–D14 preparados y todos los G9 de entrada. DoD M8: checklist
completo y decisión final de readiness del Technical Owner. Un checklist incompleto
produce **not ready**, conservando M8 abierto; 1.0 no se publica automáticamente.
Handoff final: contratos congelados, commits/hashes, receipts/reviews/dispositions,
matriz real de targets/clientes, operación/rollback y decisión readiness pendiente
o aprobada. No iniciar features posteriores a 1.0.
