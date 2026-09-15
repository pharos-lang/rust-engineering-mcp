# ADR-086 — Política de deprecación y freeze (0.8 → 1.0)

Date: 2026-09-14
Amended: 2026-09-14 (V01 F-A/F-D)

## Context

D11 (`docs/roadmap/adr-backlog-m2-m8.md` §D11) exige, antes de M8-01, fijar la
política de deprecación y freeze de contratos que el censo de superficie y el
resto de M8 deben aplicar. [ADR-012](ADR-012-semver-compatibility.md) ya fija
SemVer y la existencia de `docs/compatibility.md`, pero no dice cómo se
clasifica la estabilidad de un elemento de contrato ni cómo se anuncia y retira
una deprecación entre 0.x y 1.0; esa frontera es la que este ADR cierra, sin
tocar ADR-012. La spec exige SemVer con reglas cerradas por dígito
(`docs/spec/rust-engineering-mcp-propuesta-v0.3.md` §53.1), libertad acotada
durante 0.x con changelog/fixtures/migration notes obligatorios (§54), ningún
parámetro `version` por tool —la versión relevante vive en metadata/capabilities
del servidor— (§55), un capability document propio con `stability` por tool
(§56), tres categorías de estabilidad (`stable`, `preview`, `experimental`, §57),
deprecación de una tool estable únicamente en una versión MINOR con la tool
funcionando durante todo ese ciclo mayor (§58) y la existencia obligatoria de
`docs/compatibility.md` (§59). §116.1 advierte del riesgo de *tool explosion* y
recomienda tools composables y parámetros tipados en vez de acumular contratos
que después haya que deprecar uno a uno. La decisión ya fue tomada por el
orquestador M8 en
[`docs/validation/M8/delegation/D11-decision-brief.md`](../validation/M8/delegation/D11-decision-brief.md)
y se materializa aquí sin reinterpretarla; `docs/roadmap/m8-stabilization.md`
§«Contratos y política propuesta de deprecación» la resume como encargo previo
a M8-01/M8-02.

## Decision

1. **Clases de estabilidad por elemento de contrato.** Cada tool, Resource,
   comando CLI y formato en disco recibe una clase —`stable`, `preview`,
   `experimental` o `internal`— asignada en el censo M8-01 con evidencia:
   consumidor real, test y documentación. Un elemento sin consumidor real o
   sin test no puede clasificarse `stable`. `experimental` es la categoría de
   spec §57 para tools anunciadas en `tools/list` pero aún sin calificar, con
   opt-in por namespace o metadata; hoy no tiene uso, ninguna tool del
   catálogo se anuncia `experimental`. `internal` designa elementos existentes
   que no se anuncian —comandos CLI y formatos en disco privados— y no se
   anuncia en documentación pública ni en `tools/list`.

   **Consumidor real.** Es, indistintamente, (a) una invocación registrada en
   un recibo de cliente stock (Inspector, Codex, Claude Code, Gemini CLI) o
   (b) un test end-to-end nativo que atraviesa el wire MCP contra el servidor
   real. Un test unitario o de contrato (snapshot de schema, validación
   aislada) no cuenta como consumidor real.

   Toda tool `stable` debe además ser ejercitada por al menos un cliente
   stock en la matriz M8-04 antes de RC1; si no lo supera, se degrada a
   `preview` antes del freeze, con constancia en el changelog.
2. **0.x hasta el freeze.** Mientras no exista freeze, toda ruptura de un
   elemento requiere minor release, changelog, snapshots y migration notes
   (ADR-012). Un patch nunca cambia nombres, campos requeridos, defaults, enums
   cerrados ni la semántica de errores de un elemento ya publicado.
3. **Aditivo no es automáticamente compatible.** Un campo opcional nuevo o una
   variante nueva de un enum cerrado solo se clasifica como cambio aditivo tras
   medirlo contra los consumidores exhaustivos conocidos (schemas de snapshot,
   Inspector y la matriz de clientes stock). Si un cliente exhaustivo rompe con
   ese cambio, es una ruptura y sigue la regla del punto 2 (minor + migration
   notes).
4. **0.8.0 es el freeze.** En 0.8.0 se anuncian todas las deprecaciones con su
   reemplazo y una migración probada por test; cada elemento deprecado sigue
   funcionando durante toda la serie 0.8/0.9. En 1.0 se retira únicamente lo
   anunciado desde 0.8.0 y con migración probada; nada que no haya sido
   anunciado en 0.8.0 puede retirarse en 1.0. Tras el freeze no se renombra ni
   se amplía el contrato de un elemento `stable`; si una ruptura resulta
   necesaria, reinicia el freeze y los dos release candidates de M8-09.
5. **Desde 1.0.** La deprecación de un elemento estable ocurre en una versión
   minor 1.x, permanece funcional durante toda esa serie 1.x y su eliminación
   solo puede ocurrir en 2.0 (spec §58).
6. **Retirada de una revisión MCP.** Retirar el soporte de una revisión del
   protocolo requiere una decisión registrada, la lista de clientes afectados,
   una alternativa y avisos ligados a releases, nunca solo a un plazo en meses.
7. **Excepción de seguridad.** Un elemento puede deshabilitarse fail-closed
   fuera de este calendario cuando existe un advisory de seguridad, con
   recuperación documentada. Esa excepción nunca reinterpreta en silencio el
   resultado de una llamada existente.
8. **Mecanismo visible de deprecación.** No se introduce un parámetro `version`
   por tool (spec §55). Una deprecación se expresa en la `description` de la
   tool con el prefijo «Deprecated since 0.8.0 — use …», en
   `docs/compatibility.md` y en el changelog; el contrato (schema y errores) del
   elemento deprecado no cambia mientras siga anunciado. Si el censo M8-01
   confirma que el capability document de spec §56 no existe en el checkout, su
   adopción se decide en M8-02 como parte del freeze, no como feature nueva
   fuera de esa gate.
9. **Gate de superficie en M8-01.** Toda consolidación de tools aceptada o
   descartada por el censo se registra antes del freeze junto con su análisis
   de compatibilidad; un contrato usado no se elimina solo para alcanzar un
   número arbitrario de tools (spec §116.1).

## Alternatives considered

- **Compatibilidad estricta (nunca romper nada antes de 1.0).** Descartada:
  contradice spec §54, que permite libertad acotada durante 0.x siempre que se
  documenten breaking changes y se publiquen migration notes; congelar todo
  desde ahora impediría la propia consolidación de superficie que D11/M8-01
  necesitan evaluar.
- **Opt-in versionado por tool (parámetro `version` en cada llamada).**
  Descartada explícitamente por spec §55: incrementa tokens, obliga al agente a
  conocer una versión interna, duplica el SemVer del servidor y no resuelve
  incompatibilidades reales; la versión relevante ya vive en
  metadata/capabilities.
- **Ruptura libre hasta 0.8 sin anuncio previo.** Descartada: sin clases de
  estabilidad ni ventana de anuncio en 0.8.0, un cliente exhaustivo no tiene
  forma de distinguir un cambio aditivo real de una ruptura silenciosa, y
  contradice el punto 3 de la decisión y la evidencia exigida por spec §54.
- **Política de deprecación basada exclusivamente en meses.** Descartada por
  spec §58, que asocia la compatibilidad a releases, no a un calendario; un
  plazo en meses no es verificable contra el estado real del checkout ni contra
  los dos release candidates de M8-09.

## Consequences

- El censo M8-01 debe entregar, por cada elemento de contrato, su clase de
  estabilidad con evidencia y la lista de deprecaciones candidatas hacia
  0.8.0.
- M8-02 congela esas deprecaciones con un before/after explícito de schema y
  comportamiento por elemento, y decide la adopción del capability document de
  spec §56 si el censo confirma que falta.
- Los dos release candidates de M8-09 deben publicar exactamente el mismo
  contrato `stable`; una ruptura necesaria después del freeze reinicia ambos
  RC.
- [ADR-012](ADR-012-semver-compatibility.md) no se modifica: sigue fijando
  SemVer, la baseline de protocolo/SDK y la existencia de
  `docs/compatibility.md`; ADR-086 lo complementa con clases de estabilidad y
  el calendario de deprecación 0.8 → 1.0.
- `docs/compatibility.md` gana una sección que resume esta política para
  lectores que no consultan los ADR directamente.
- Las tres tools M3 (`rust.coverage`, `rust.semver.check`,
  `rust.mutation.test`) hoy solo tienen e2e nativo como consumidor y quedan
  condicionadas a M8-04.

## Status

Accepted.

## Sources

- `docs/spec/rust-engineering-mcp-propuesta-v0.3.md` §53–59 (estrategia de
  actualizaciones, versionado 0.x, `version` por tool, capability document,
  tool stability, deprecación, compatibilidad MCP) y §116.1 (tool explosion).
- [ADR-012](ADR-012-semver-compatibility.md) — SemVer y compatibilidad MCP
  (sin cambios).
- `docs/roadmap/m8-stabilization.md` §«Contratos y política propuesta de
  deprecación».
- `docs/roadmap/adr-backlog-m2-m8.md` §D11.
- [`docs/validation/M8/delegation/D11-decision-brief.md`](../validation/M8/delegation/D11-decision-brief.md)
  — decisión del orquestador que este ADR materializa.
