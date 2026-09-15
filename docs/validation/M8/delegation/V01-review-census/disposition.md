# V01 — disposición del orquestador (2026-09-14)

Veredicto del revisor: **Block** (3 P1, 5 P2, 5 P3) sobre material inline (sin
acceso al JSON, al README raíz, a la spec ni a `git log`). Verificación del
orquestador finding por finding:

| ID | Sev (revisor) | Verificación | Disposición |
| --- | --- | --- | --- |
| F-A | P1 | Cierto: ADR-086 §1 no dice qué pasa con `experimental` (spec §57) | **Aceptado** (P2 de contrato): W04 enmienda ADR-086 §1 — `experimental` admitida para tools anunciadas sin calificar (namespace/opt-in §57), hoy sin uso; `internal` reservada a elementos no anunciados |
| F-D | P1 | Cierto en la letra: ADR-086 no define «consumidor real»; el encargo W01 lo definía como recibo de cliente stock **o** e2e nativo por el wire | **Aceptado con decisión**: W04 añade la definición a ADR-086 §1 (invocación registrada en recibo de cliente stock **o** test e2e nativo que atraviesa el wire MCP; un test unitario no cuenta) y la obligación de que toda tool `stable` sea ejercitada por un cliente stock en M8-04 antes de RC1 (si no pasa, se degrada a `preview` antes del freeze). Las tres M3 siguen `stable` bajo esa definición |
| F-J | P1 | **Falso**: `README.md:21` describe la release `0.3.0` («esa release registra 31 tools») y el párrafo ya dice que el checkout registra 36; W03 lo dejó con razón. El error estaba en `01.md` §1 (afirmación del orquestador demasiado amplia) | Rechazado como defecto de W03; **corregido `01.md` §1**. Hallazgo colateral del orquestador: `README.md:26` («solo M6 está en desarrollo local, sin integración remota ni release») es falso desde el PR #20 → W04 |
| F-G | P2 | Cierto: `real_consumers[]` mezcla cliente stock y e2e nativo | **Aceptado**: W04 añade `stock_client_invoked` (bool) por tool en el JSON y lo refleja en §2 del censo |
| F-K | P2 | **Falso**: `tools.md:1645` («después de las 31 tools M1–M5») y `client-configuration.md:316` (sección M5) son históricos; W03 lo justificó por sitio | Rechazado; `01.md` §1 corregido |
| F-L | P2 | **Falso**: PR #18 existe (`docs(release): record the v0.3.0 publication`, merge `1303af8`, 2026-09-11) — el revisor no tenía `git log` | Rechazado; se cita el merge en `01.md` §1 |
| F-M | P2 | Cierto: docs públicas dicen «calificadas» sin matiz `preview` | **Aceptado**: W04 añade la nota de clase `preview` y deuda conocida en `client-configuration.md` §M6 y en `compatibility.md` |
| F-B | P3 | Cierto (ambigüedad de «romper») | Backlog M8-02: definir «romper» = fallo de validación contra el snapshot 0.8.0 (`additionalProperties`/enum cerrado) en Inspector o cliente stock |
| F-C | P3 | Cierto: spec §56 dice «debe»; es requisito, no opción | **Aceptado**: `01.md` §2 lo registra como requisito a implementar/decidir en M8-02 |
| F-E, F-F | P3 | Ciertos (redacción) | Corregidos en `01.md` §3 |
| F-H | P3 | Cierto (numerador sin description/annotations) | W04 añade la nota metodológica en §3.2 del censo |
| F-I | P3 | Limitación del revisor | Sin acción; huérfanos verificados por el orquestador y por R01 |

Resultado: tras W04 el material cambia de forma material (ADR-086 §1) →
re-revisión V01b (Sonnet, read-only) del diff antes del commit.
