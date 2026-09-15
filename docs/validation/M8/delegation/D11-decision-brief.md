# D11 — Política de deprecación y freeze (decisión del orquestador, 2026-09-14)

Decisor: Claude Fable 5.1 (orquestador M8) dentro del encargo; el owner puede
revocarla en sesión. Fuentes: spec §53–59 (SemVer, 0.x, sin `version` por tool,
capability document, stability, deprecación por releases), §116.1 (tool
explosion), [ADR-012](../../../adr/ADR-012-semver-compatibility.md) (se
conserva sin cambios), [m8-stabilization.md §Contratos](../../../roadmap/m8-stabilization.md),
[backlog D11](../../../roadmap/adr-backlog-m2-m8.md). Materializa: ADR-086 (W02).

## Decisión

1. **Clases de estabilidad** por elemento de contrato (tool, Resource, comando
   CLI, formato en disco): `stable`, `preview`, `internal`. La clase se asigna en
   el censo M8-01 con evidencia (consumidor real + test + docs); un elemento sin
   consumidor real o sin test no puede ser `stable`. `internal` no se anuncia en
   docs públicas ni en `tools/list`.
2. **0.x (hasta el freeze)**: ADR-012 sigue vigente — ruptura ⇒ **minor** +
   changelog + snapshots + *migration notes*; **patch** nunca cambia nombres,
   campos requeridos, defaults, enums cerrados ni semántica de errores.
3. **Aditivo no es automáticamente compatible**: un campo opcional nuevo o una
   variante nueva de enum cerrado se clasifica como aditivo solo tras medir contra
   los consumidores exhaustivos conocidos (schemas de snapshot, Inspector y la
   matriz de clientes stock); si un cliente exhaustivo rompe, es ruptura (minor +
   migration notes).
4. **0.8.0 = freeze**: en 0.8.0 se anuncian todas las deprecaciones con su
   reemplazo y migración probada por test; lo deprecado sigue funcionando durante
   toda 0.8/0.9; en 1.0 se retira **solo** lo anunciado desde 0.8.0 y con
   migración probada. Tras el freeze no se renombra ni se amplía contrato
   `stable`; una ruptura necesaria reinicia el freeze y los dos RC (M8-09).
5. **Desde 1.0** (spec §58): deprecación en minor 1.x, funcional durante toda
   1.x, eliminación solo en 2.0.
6. **Retirada de una revisión MCP** requiere decisión registrada, clientes
   afectados, alternativa y avisos por releases (no por meses).
7. **Excepción de seguridad**: se puede deshabilitar fail-closed un elemento con
   advisory y recuperación documentada; nunca se reinterpreta silenciosamente un
   resultado.
8. **Mecanismo visible**: sin parámetro `version` por tool (spec §55). La
   deprecación se expresa en la `description` de la tool (prefijo
   «Deprecated since 0.8.0 — use …»), en `docs/compatibility.md` y en el
   CHANGELOG; el contrato (schema/errores) del elemento deprecado no cambia.
   Si el censo M8-01 confirma que no existe el capability document de spec §56,
   su adopción se decide en M8-02 como parte del freeze, no como feature nueva.
9. **Gate de superficie (M8-01)**: consolidaciones aceptadas o descartadas se
   registran antes del freeze con análisis de compatibilidad; no se borra un
   contrato usado para alcanzar un número arbitrario de tools.

## Consecuencias

- El censo M8-01 entrega la clase de cada elemento y la lista de deprecaciones
  candidatas; M8-02 las congela con before/after de schema y comportamiento.
- Los dos RC de M8-09 deben publicar exactamente el mismo contrato `stable`.
- ADR-012 no se modifica (regla del plan); ADR-086 lo complementa.
