# M2 — auditoría independiente de trazabilidad final

**Accepted; cero P0/P1 nuevos.** AGY se ejecutó con el identificador disponible
`gemini-3.8-flash-high`, esfuerzo `high`, sobre un paquete autocontenido M2.
El [recibo de ejecución](M2-closure-agy-run.json) registra exit 0, un turno y
`SUCCESS`; el [resultado bruto](M2-closure-agy.json) conserva el dictamen completo.
[Inputs](M2-closure-agy-inputs.json) y [prompt exacto](m2-closure-packages/M2-closure-agy-prompt.txt)
identifican 37 fuentes y delimitan extractos, resúmenes derivados e índices de
pruebas. No atribuyen lectura completa del repositorio ni de la especificación.

La [identidad del CLI y opciones verificadas](m2-closure-packages/agy-tooling.json)
incluye hash del ejecutable, help y lista de modelos. No se deduce una versión
instalada del encabezado del changelog. El modelo solicitado figura en argv y en
la respuesta; el JSON no ofrece una atestación independiente del backend.

## Alcance y restricciones observadas

Task: auditar spec→ADR→implementación→pruebas→nueve criterios DoD/G1–G9 y coherencia
final, sin reabrir M2–M8. Files changed: ninguno por el auditor. Tests executed:
ninguno. Se usó un directorio temporal aislado del checkout, sandbox y un prompt
que prohibía tools, MCP, terminal, red, subagentes y cambios. El informe declara
haber analizado solo el paquete; el JSON no aporta un timeline independiente de
herramientas. Los inputs productivos permanecen iguales al full.

El stderr advierte que `--mode plan` no tiene efecto con
`--disable-slash-commands`. Se conserva esa advertencia y no se presenta plan mode
como enforcement aplicado; read-only fue la instrucción del paquete, reforzada
por el directorio separado y `--sandbox`. No se usó bypass de permisos.

## Findings y disposición del Technical Owner

- Los cinco P2 reiteran límites existentes: disponibilidad del store corrupto,
  coste RSS/clone, loopback de fix, contrato local_coordinated y headroom no
  retroactivo. Se mantienen con la remediación y límites ya publicados; ninguno
  es una nueva petición de cambio productivo ni un P0/P1 abierto.
- Los P3 conservan el alcance del cliente con prompt v2, composición de TTL,
  inyección APFS limitada e integración pendiente en el paquete recibido. El
  commit/merge y smoke se ejecutan después y tienen evidencia propia; no se
  atribuyen retrospectivamente a AGY.
- El dictamen acepta trazabilidad y consistencia de claims recibidos. Los hashes,
  bytes M1 y comandos se verificaron por el owner y el verificador reproducible;
  AGY no los recalculó ni ejecutó gates. Su tabla omite fuentes y dice 29 aunque
  el manifest contiene 37; el manifest exacto es la referencia de cobertura.

## Precisiones del dictamen que no se adoptan

La redacción del auditor contiene generalizaciones que no cambian el contrato:

- D02 es No-go de primitivas evaluadas con esa autoridad, no una demostración
  universal de imposibilidad de exclusión. El adapter Darwin usa sus flags
  no-follow/beneath, no el flag Linux `RESOLVE_BENEATH` nombrado en la respuesta.
- El journal corrupto sigue pudiendo bloquear el store: la remediación permite
  continuar en copias físicas y state nuevos; no significa que el fallo haya
  adquirido recuperación automática. No se poda un journal pendiente para
  eludir reconciliación ni se recrea/borra el original en cuarentena.
- `0.1.0` no interpreta journals M2 ni ofrece sus tools. No se ha demostrado ni
  se exige que ese binario histórico detecte/rechace un formato futuro.
- Los intentos cliente 1–4 tienen causas distintas: 1 expectativa del harness
  sobre exit 5, 2 cuota terminal, 3–4 refs viejas/continuación indebida. No se
  reducen todos a cuota o referencias. M2 usa stdio; la composición de replay
  no acredita pérdida física de paquetes o un enlace TCP del cliente.
- La cifra RSS es observación histórica, no techo enforced. Solo macOS ARM64/APFS
  tiene writer positivo; no se atribuye calificación binaria Linux/Windows.

Decisions: mantener ADR-050..059 y avanzar únicamente al merge local autorizado.
Open issues del paquete: registrar integración/smoke, resueltos en el
[cierre del owner](../validation/M2-07.md#integración). M3+ sigue sin autorización.
Los enlaces añadidos y registros de integración posteriores tienen revisión del
owner y verificación mecánica propias, sin atribuir sus nuevos hashes a AGY.
