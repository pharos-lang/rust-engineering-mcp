# Frontera MCP y contratos de tool

Este capítulo cubre cómo el producto habla el protocolo MCP: qué posee
`rmcp` frente a qué posee el producto, el transporte stdio, el álgebra de
resultados, Resources, la admisión de workers que sostiene casi todas las
tools, y el gate de calidad de captura única. La política de SemVer,
deprecación y las clases de estabilidad (`stable`/`preview`/`experimental`/
`internal`, ADR-086) tienen su propio capítulo en
[`reference/compatibility.md`](../reference/compatibility.md); aquí solo se
señala dónde una tool nueva encaja en esa política cuando afecta al
contrato descrito en este documento.

## `rmcp` como frontera MCP

Decisiones: ADR-002.

**Decisión.** `rmcp` (pinneado `=3.2.0`, features `server` + `transport-io`)
es el único SDK MCP del producto, confinado a `crates/mcp-server/**`.
Cualquier cambio de versión exige una ADR propia más una matriz de
compatibilidad y tests — **no se implementa un stack JSON-RPC/MCP paralelo**.

**Contexto.** El protocolo MCP evoluciona rápido; el servidor necesita
negociación, lifecycle, tools, resources, cancelación y transporte sin
mantener su propia reimplementación del protocolo.

**Alternativas rechazadas que siguen explicando el límite actual.**
JSON-RPC manual tiene alto riesgo de deriva frente a las revisiones del
protocolo; un SDK no oficial añadiría riesgo de compatibilidad y de
ownership sobre una pieza crítica del transporte.

**Estado actual.** Vigente; `rmcp` solo aparece como dependencia en
`crates/mcp-server/Cargo.toml`.

**Riesgo residual.** Una actualización futura de `rmcp` podría introducir
cambios rompientes en tipos `non_exhaustive` de su API pública; se mitiga
procedimentalmente (ADR-012, revisión obligatoria antes de subir versión),
no automáticamente por el compilador.

## Stdio como único transporte y su presupuesto

Decisiones: ADR-003, ADR-023.

**Decisión.** stdio es el único transporte en M1+; el transporte remoto
(HTTP) está formalmente diferido, no simplemente no implementado (ver
`.planning/deferred-commitments.md`). **`stdout` queda reservado
exclusivamente a frames del protocolo MCP; todo log y diagnóstico va por
`stderr` vía `tracing`** — invariante no negociable repetido en `AGENTS.md`
y verificado por los tests de `crates/mcp-server/tests/protocol.rs`, que
comprueban la pureza de `stdout` del binario real.

ADR-023 fija además, concretamente:
- presupuesto de 1 MiB por línea de entrada (excluyendo el salto de línea),
  procesada en chunks de 8 KiB (`crates/mcp-server/src/stdio/budget.rs`);
- deadline de frame de 10 s;
- cinco revisiones de protocolo MCP soportadas explícitamente
  (2024-11-05…2026-07-28), negociadas por el SDK, nunca hardcodeadas;
- el anuncio inicial de la capability `tools` es determinista — estaba
  vacío en el instante de M0-03 porque todavía no había tools registradas,
  y evolucionó después según lo previsto; eso no es una contradicción con
  el estado actual de 36 tools, es una foto en el tiempo;
- EOF limpio del cliente → exit 0; fallo de transporte o de bootstrap →
  exit 1.

**Riesgo residual.** ADR-087 documenta una regresión de stdio previa a
`initialize` en Windows, que llevó a retirar la CI de Windows el
2026-09-13. Es fragilidad de robustez de un target que ya no se qualifica
como positivo (ver
[`execution-and-security.md`](execution-and-security.md)), no un cambio de
alcance del transporte stdio en sí.

**Estado actual.** Vigente; evidencia en `crates/mcp-server/src/stdio.rs`,
`stdio/budget.rs` y `crates/mcp-server/tests/{protocol,cli}.rs`.

## Álgebra de resultados y JSON Schema por tool

Decisiones: ADR-006, ADR-015.

**Decisión.** `rmcp` posee JSON-RPC, lifecycle, `tools/list`, `tools/call` y
cancelación de forma completa; el producto nunca los reimplementa. Cada tool
modela su contrato como DTOs Rust (Serde + Schemars) con raíz objeto,
`#[serde(deny_unknown_fields)]`, invariantes por newtype y JSON Schema
2020-12. `outputSchema` es **obligatorio** en el producto — más estricto que
el MCP genérico, donde es opcional.

El álgebra de estado es cerrada: `passed | failed | blocked | unavailable |
cancelled` (ver [`domain-and-application.md`](domain-and-application.md#tipos-e-invariantes-del-dominio-base)
para la tabla completa). `failed` es un **resultado de protocolo exitoso**
(`isError=false` en la respuesta JSON-RPC) porque el proyecto analizado
falló, no el servidor; los fallos operacionales (timeout, tool ausente,
sandbox denegado) llevan `isError=true` con un `OutputEnvelope` tipado cuyo
`error_code`/`error_message` son requeridos-pero-nulables sobre un enum
cerrado (`OperationalErrorCode`, sin variante `INTERNAL_ERROR` — ese código
pertenece exclusivamente a los errores JSON-RPC del SDK). Los errores
JSON-RPC quedan reservados a tool desconocida, petición malformada o fallo
interno real del servidor; los errores del compilador/lint/test del
proyecto analizado nunca se convierten en errores MCP. Un gate de calidad
requerido solo es `passed` si **todas** sus etapas son `passed` — bloqueado,
no-disponible o cancelado nunca cuentan como éxito agregado.

El tipo `Contract<I, O>` (`crates/mcp-server/src/stdio/contract.rs`) mapea
`ToolStatus` del dominio a los resultados y errores del SDK exactamente
según esta tabla, de forma centralizada para las 36 tools.

**Alternativas rechazadas que siguen explicando el límite actual.** Un
booleano de éxito pierde las distinciones anteriores; un código de salida
de proceso no-cero tratado como error MCP bloquearía la auto-reparación del
agente (que necesita poder leer un `failed` estructurado y decidir); esquemas
manuales duplicados derivan del código real con el tiempo; valores JSON
internos genéricos empujan errores de esquema a runtime, y además están
prohibidos por `scripts/check-architecture.py` fuera del borde MCP.

**Estado actual.** Vigente; evidencia `crates/domain/src/result.rs`
(`ToolStatus`, `OutputEnvelope`), `scripts/contract-freeze.py`/
`test-contract-freeze.py` y los esquemas `#[schemars(...)]` por tool.

## Resources para contexto ya computado

Decisiones: ADR-011.

**Decisión.** Las tools ejecutan trabajo; los Resources exponen información
o artifacts ya calculados. M1 implementa solo lectura mínima de artifacts
acotados vía URIs de ID opaco (`rust-artifact://prj_<hex>/art_<hex>`,
extendido luego por `rust-quality-artifact://` en M3 — nunca paths de host).
Cada lectura de Resource revalida `ProjectRef`, retención y límites de
nuevo, no confía en una autorización previa. Los `rust-project://` y
`rust-catalog://…` que la especificación sugería como ejemplo nunca se
implementaron literalmente — quedaron sustituidos por el esquema de URIs
opacas y dinámicas de artifact. Prompts (la tercera capability de MCP) está
explícitamente fuera de alcance en M1 y sigue sin usarse a través de M6:
`prompts/list` devuelve `[]` por diseño, no por omisión.

**Estado actual.** Vigente; evidencia
`crates/mcp-server/src/stdio/resources.rs` y la aserción de template en
`capability_document.rs`.

## Admisión de workers, cancelación y transporte

Decisiones: ADR-030.

**Decisión.** El runtime `rmcp` corre de forma current-thread. Un **permit
de worker único compartido, sin cola**, admite las operaciones de proyecto;
se retiene hasta que el cierre bloqueante real termina, no hasta que la
llamada async retorna. Cancelación, deadline y shutdown de sesión se
propagan dentro del worker. Operaciones costosas que llegan durante el
bootstrap del SDK (antes de que el servidor esté listo) devuelven
`SANDBOX_DENIED` fijo en vez de encolarse o bloquear. La admisión de
transporte del propio SDK es tipada: 16 requests, 16 notifications y 16
pending-sends concurrentes; IDs duplicados rechazan la sesión entera; hasta
16 leases de cancelación suprimida se retienen antes de cerrar la sesión.

**Consecuencias.** Este es el sustrato activo que reutilizan explícitamente
ADR-032/033 (`rust.project.inspect`/`.toolchain.inspect`, ver
[`domain-and-application.md`](domain-and-application.md)), ADR-042
(`rust.catalog.status`, ver [`catalog-and-search.md`](catalog-and-search.md))
y ADR-060 (MCP Tasks, ver [`jobs-and-artifacts.md`](jobs-and-artifacts.md)):
ninguno de ellos crea un segundo semáforo o cola de admisión propia.

**Estado actual.** Vigente; evidencia `crates/mcp-server/src/stdio/
workers.rs` (`run`, `run_joined`, `run_joined_with`) y
`crates/mcp-server/tests/protocol.rs`/`tests/inspection_runtime.rs`.

## El gate de calidad de captura única y sus contratos congelados

Decisiones: ADR-040, ADR-067.

**Decisión.** `rust.quality.gate` ejecuta `fmt → check → clippy → test →
audit` directamente contra los ports existentes, compartiendo un solo
worker/lease/captura de source — **sin llamadas MCP anidadas** entre las
etapas. La precedencia de agregado del resultado final es
`blocked > unavailable > failed > passed`; la publicación de logs es
agrupada (0 a 4 artifacts) con rollback all-or-nothing si falla cualquier
paso de la publicación.

ADR-067 añade `rust.quality.gate.v2` y `rust.deny` como **tools nuevas y
separadas** — nunca modifican el enum `QualityProfile`/`QualityStage` de las
13 tools M1 congeladas. `rust.quality.gate.v2` implementa los perfiles
`strict`/`release` que la especificación original describía (deny +
coverage; +semver-check + mutation opcional) bajo un nombre de tool
distinto de `rust.quality.gate` (que solo implementa `fast`/`standard`) — un
agente que necesite `strict`/`release` debe invocar la `.v2`, no la
original. `rust.deny` reutiliza el mismo `DependencyAuditPort` que
`rust.dependencies.audit` sin modificarlo; su documento de política es
host-only, cerrado, ≤64 KiB, pinneado por SHA-256; las suppresiones
requieren id/engine/rule/package/version-range/reason/owner/expiry más un
`rules_digest`.

**Patrón general de evolución de contrato.** Cada milestone posterior a M1
extiende el catálogo de tools agregando **nombres de tool nuevos**, nunca
ampliando la forma o semántica de un contrato ya congelado.

### Contratos congelados M1

Las trece tools originales conservan semántica y forma de esquema
byte-a-byte desde su implementación (verificado por tests de contrato en
cada ADR de milestone posterior, incluyendo el censo de estabilidad M8-01):

`rust.project.open`, `rust.project.inspect`, `rust.toolchain.inspect`,
`rust.check`, `rust.fmt.check`, `rust.clippy`, `rust.test`,
`rust.dependencies.audit`, `rust.diagnostics.explain`, `rust.quality.gate`,
`rust.catalog.status`, `rust.crate.search`, `rust.crate.inspect`.

Cada tool posterior (M2 mutación, M3 calidad avanzada, M4 seguridad
avanzada, M5 rendimiento, M6 analyzer) se documenta en su propio capítulo
(ver [`mutation.md`](mutation.md), [`jobs-and-artifacts.md`](jobs-and-artifacts.md),
[`execution-and-security.md`](execution-and-security.md),
[`performance.md`](performance.md), [`analyzer.md`](analyzer.md)). El
inventario completo llega a **36 tools** (13 M1 + 5 M2 + 4 M3 + 5 M4 + 4 M5 +
5 M6); la lista y esquemas exactos viven en
[`reference/tools.md`](../reference/tools.md), no aquí.

**Estado actual.** Vigente; evidencia `crates/mcp-server/src/stdio/
{quality,deny,quality_v2}.rs`, `crates/execution-adapter/src/deny_json.rs`,
`crates/application/src/security.rs`;
[`docs/validation/M4/{core-gate,full-gate,clients}.json`](https://github.com/pharos-lang/rust-engineering-mcp/tree/51fa602e/docs/validation/M4) (M4 full-gate
`status: passed`, 2026-09-08, corriendo entre otros los pasos `deny`,
`m4-runtime` y `m4-inventory` que califican `rust.deny`).

**Limitación documental (no de código).** [`docs/adr/README.md`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/README.md) marca
ADR-067 como "pendiente"; es un índice desactualizado — el cuerpo del ADR y
el código muestran implementación y calificación completas. No usar ese
índice como fuente de estado (ver [`decisions.md`](decisions.md)).

## Observabilidad, naming y disciplina de errores

`tracing`/`tracing-subscriber` se usan en todo `mcp-server`; los logs van
siempre a `stderr` bajo stdio, nunca a `stdout` (ver ADR-003 arriba). El
naming de tool sigue siempre el patrón `rust.<grupo>.<acción>`, consistente
en las 36 tools; el producto no introduce lógica específica de cliente (sin
ramas "si el cliente es X, entonces…") — la compatibilidad se logra vía MCP
y JSON Schema, verificada contra al menos dos clientes reales (MCP Inspector
y un cliente Codex-compatible) en las matrices de M1-M8. Las descripciones
de cada tool declaran cuándo usarla, qué hace, sus efectos secundarios,
costo y límites — ese contenido vive en
[`reference/tools.md`](../reference/tools.md); las descripciones en sí,
como texto de esquema `///`, son contrato congelado y esta limpieza no las
edita (ver nota en `reference/tools.md`).

No existe un parámetro `version` por llamada de tool — rechazado
explícitamente por la propia especificación (§55): costaría tokens en cada
llamada y duplicaría el SemVer del servidor sin resolver incompatibilidades
reales. El servidor expone en su lugar un capability document propio
(`document_kind`/`server_version`/`protocol`/`tools[].stability`/
`platform`/`sandbox`), distinto de `server/discover`, descrito en detalle en
[`reference/compatibility.md`](../reference/compatibility.md) junto con las
cuatro clases de estabilidad de ADR-086.

## Política de evolución de contratos (ADR-086)

[`reference/compatibility.md`](../reference/compatibility.md) resume las
cuatro clases de estabilidad y la regla base de ruptura en 0.x
(minor + changelog + notas de migración). Las reglas siguientes son las
mismas decisiones de
[ADR-086](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-086-deprecation-and-freeze-policy.md)
§3–§9; viven aquí íntegras porque gobiernan el ciclo de vida completo del
contrato público, no solo la ruptura 0.x que ya cubre `compatibility.md`.

1. **Aditivo no es automáticamente compatible (§3).** Un campo opcional nuevo
   o una variante nueva de un enum cerrado solo se clasifica como cambio
   aditivo tras medirlo contra los consumidores exhaustivos conocidos
   (schemas de snapshot, Inspector y la matriz de clientes stock). Si un
   cliente exhaustivo rompe con ese cambio, es una ruptura y sigue la regla
   de `compatibility.md` (minor + changelog + notas de migración).
2. **0.8.0 es el freeze; calendario de deprecación hasta 1.0 (§4).** En
   `0.8.0` se anuncian todas las deprecaciones con su reemplazo y una
   migración probada por test; cada elemento deprecado sigue funcionando
   durante toda la serie 0.8/0.9. En 1.0 se retira únicamente lo anunciado
   desde `0.8.0` y con migración probada; nada que no haya sido anunciado en
   `0.8.0` puede retirarse en 1.0. Tras el freeze no se renombra ni se
   amplía el contrato de un elemento `stable`; si una ruptura resulta
   necesaria, **reinicia el freeze y los dos release candidates** de M8-09
   (ver [`operations/release-verification.md`](../operations/release-verification.md)
   y [`.planning/deferred-commitments.md`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/.planning/deferred-commitments.md)
   para el estado real de esos RC).
3. **Desde 1.0 (§5).** La deprecación de un elemento estable ocurre en una
   versión minor `1.x`, permanece funcional durante toda esa serie `1.x`, y
   su eliminación solo puede ocurrir en `2.0`.
4. **Retirada de una revisión MCP (§6).** Retirar el soporte de una revisión
   del protocolo requiere una decisión registrada, la lista de clientes
   afectados, una alternativa y avisos ligados a releases — nunca solo un
   plazo en meses.
5. **Excepción de seguridad, fail-closed (§7).** Un elemento puede
   deshabilitarse fail-closed fuera de este calendario cuando existe un
   advisory de seguridad, con recuperación documentada. Esa excepción nunca
   reinterpreta en silencio el resultado de una llamada existente — un
   elemento afectado se deshabilita explícitamente, nunca degrada su
   semántica sin aviso.
6. **Mecanismo visible de deprecación (§8).** No existe un parámetro
   `version` por tool. Una deprecación se expresa en la `description` de la
   tool con el prefijo «Deprecated since 0.8.0 — use …», en
   [`reference/compatibility.md`](../reference/compatibility.md) y en el
   changelog; el contrato (schema y errores) del elemento deprecado no
   cambia mientras siga anunciado.
7. **Snapshots como parte de la exigencia de ruptura 0.x (§2/§9).** Una
   ruptura 0.x, además de minor + changelog + notas de migración, exige
   actualizar los snapshots de contrato
   (`crates/mcp-server/tests/snapshots/*-tool.json`) y el manifiesto de
   freeze regenerado y revisado
   (`tests/baselines/contract-freeze-0.8.0.json`) — ver la cadena de
   verificación de tres eslabones en
   [`reference/compatibility.md`](../reference/compatibility.md#congelación-de-contrato-080-y-verificación).
   Ninguna consolidación de superficie elimina un contrato con consumidor
   real solo para alcanzar un número arbitrario de tools.
