# Continuar y cerrar M2 — handoff del 2026-09-05

Asume el rol de Technical Owner, arquitecto, integrador y revisor final de Rust
Engineering MCP en `/Users/cburgosro/Projects/rust-mcp`. Completa únicamente lo que
falta de M2; no reinicies su diseño ni avances a M3+. Esta sesión continúa trabajo
ya autorizado: corregir, validar, documentar, hacer commits coherentes y merge
local a `main`. No hagas push, PR, tag, release, instalaciones silenciosas ni
operaciones en YouTrack. Preserva todos los cambios e históricos existentes.

## Prioridad del owner

Mantén la esencia del MCP y una instalación/operación razonables: arquitectura
hexagonal, contratos tipados, deny-by-default, cinco permisos de escritura
explícitos, preview revisable, commit por digest, journal y recuperación.
Conserva ADR-050 `local_coordinated`: cooperación del host/editor y namespace
estable, sin broker privilegiado, daemon nuevo, cuenta o collector obligatorio.
Reutiliza el gateway Docker existente. Vendor es opcional y explícito; el runtime
no descarga catálogos, advisories, modelos ni datos Cargo. No prometas exclusión OS,
CAS, publicación multiarchivo visible atómica ni resistencia a power loss.

## Inicio obligatorio

1. Lee `AGENTS.md` y completamente la especificación
   `docs/spec/rust-engineering-mcp-propuesta-v0.3.md`; revisa los documentos públicos,
   `docs/implementation-status.md`, `docs/validation/M2-matrix.md`,
   `docs/validation/M2-07.md`, roadmap M2 y ADRs relevantes, especialmente 049–059.
2. Inspecciona estado Git, árbol, manifests, lockfile, tests y CI. Comprueba este
   handoff contra archivos reales; los documentos de cierre siguen provisionales.
3. La autorización final del adjunto original prevalece sobre su instrucción
   inicial de solo planificación: pidió commit/merge del plan y ejecutar M2 hasta
   terminar. La planificación M2–M8 ya está integrada. No repitas esa planificación.

## Agentes de ChatGPT/Codex, Claude y AGY

Esta política hace explícitas las instrucciones del owner para la continuación;
no exige reconstruir el equipo anterior ni repetir paquetes ya aceptados.
El principal conserva arquitectura, alcance, contratos públicos, seguridad,
integración y decisión de cierre. Ningún dictamen de modelo sustituye las pruebas.

- **Principal:** configuración solicitada GPT-5.6 Sol, High. La configura el host;
  no afirmes cambiar tu propio modelo o esfuerzo. Si la sesión usa otra
  configuración, informa la diferencia sin inventar una selección.
- **Workers ChatGPT/Codex:** usa el mínimo con trabajo independiente útil. Sol es
  el predeterminado; el owner también autorizó Terra ("tierra") y Luna. Selecciona
  entre los modelos realmente disponibles: Medium para inspección documental,
  inventarios y análisis de resultados; High para debugging difícil o análisis de
  fronteras. Extra High solo con necesidad demostrada. Elige por dificultad y
  registra modelo/esfuerzo; no conviertas disponibilidad en motivo para delegar.
- **Claude Code CLI:** Sonnet 5 para revisión habitual de contratos, documentación
  o evidencia; Opus 5 High para cambios complejos de arquitectura, seguridad o
  recuperación. Verifica versión, opciones y modelo explícito antes de invocar;
  no uses `ultracode`. Revisión read-only, sin implementación, cambios de archivos,
  commits ni merges. Los paquetes ya Accepted se conservan; solicita revisión
  adicional únicamente de cambios posteriores o brechas concretas de cierre.
- **AGY:** Gemini 3.8 High mediante `agy`, si sigue disponible, para auditoría
  independiente de trazabilidad spec → ADR → implementación → pruebas → DoD M2 y
  contradicciones en los documentos finales. Acota el paquete a M2 y al delta de
  cierre; no reabras la planificación M2–M8. Reutiliza una auditoría previa solo
  si cubre los mismos archivos/hashes y afirmaciones. No afirmes que AGY auditó
  este cierre si no existe una ejecución y un resultado verificables.
- **Escalamiento excepcional:** el owner mencionó Claude Fable 5.1 únicamente
  ante incertidumbre o una decisión compleja que permanezca sin resolver. No es
  un gate rutinario. Verifica su identificador/disponibilidad antes de usarlo;
  no supongas que el nombre corresponde a un modelo accesible.

Verifica las herramientas locales y opciones oficiales antes de construir
invocaciones de Claude/AGY; no inventes flags ni sustituyas silenciosamente modelos
o proveedores. Si falta uno, registra la limitación y continúa trabajo independiente
con los medios autorizados. No marques una revisión obligatoria ausente como
realizada ni declares cerrado un criterio que dependa de ella.

Antes de cada delegación define objetivo y DoD, archivos permitidos, fronteras
prohibidas, restricciones, pruebas y salida esperada. Los workers que editen
deben poseer archivos disjuntos; prefiere read-only para revisión y evidencia.
Pide siempre: Task, Result, Files changed, Tests executed, Evidence, Risks,
Decisions y Open issues. Para revisiones externas añade versión/modelo/esfuerzo,
archivos y hashes revisados, findings por severidad, evidencia por archivo y
disposición del owner. Conserva prompts, inputs y resultados sin sobrescribir
ejecuciones previas. No atribuyas bytes nuevos a una revisión histórica.

Distribución sugerida para lo que resta: el principal ejecuta e integra el full;
un worker puede verificar documentación/hashes en paralelo sin editar inputs del
gate. Claude revisa solo deltas que lo requieran y AGY audita trazabilidad del
paquete final. No lances otro gate Docker en paralelo, no mantengas workers ociosos
y no delegues el commit/merge ni la declaración Done. La calificación de Claude
como cliente MCP y su revisión estática son evidencias distintas.

## Estado capturado (verificar live)

- Rama: `ai/m2-write-qualification`.
- HEAD: `331d1630da5a2c1d8d8c596305c0c1167a7896cb`, primer corte M2-01/02.
- `main`: `2f54b360e1e81f21e7efeff7c451cdd6f663a04f`, merge de planificación.
- Hay numerosos cambios de implementación, tests, documentos y artifacts sin
  commit, incluidos archivos nuevos. No hagas reset, checkout destructivo ni clean.
- Workspace `0.2.0-dev`: 18 tools, las 13 M1 más `rust.manifest.patch`,
  `rust.fmt.apply`, `rust.fix.apply`, `rust.dependency.add`,
  `rust.dependency.remove`. La release pública 0.1.0 no cambia.
- Contratos M1 conservados byte por byte contra baseline pública `aa61bce`:
  `docs/validation/M2-m1-contract-preservation.json`.
- Las cinco operaciones están implementadas con preview/commit/receipt/recovery,
  grants, confinamiento, resolución offline, conflictos, cancelación, cuotas y
  observabilidad local. No falta otra vertical funcional conocida.

## Corrección más reciente: ADR-059

El cliente encontró que cuatro commits consumían las cuatro ranuras RAM hasta el
TTL y bloqueaban el quinto preview. Se corrigió retirando planes terminales en la
siguiente admisión y habilitando replay desde un journal existente para un plan
ausente/expirado, con autorización viva y binding exacto ID/digest/idempotency key.
No se aumentó la cuota ni se introdujo un caché alternativo. Errores y estados de
recuperación pendiente conservan su protección. Replay puede usar recuperación o
migración explícita v1→v2 existente; no es necesariamente una lectura sin escrituras,
pero nunca crea una operación nueva sin candidato.

El commit invalida su `project_ref`. Las cinco descripciones MCP y el prompt de
cliente v2 explicitan reabrir y usar el nuevo `data.project_ref` en TODAS las
llamadas posteriores, incluso receipt/recovery. No cambiaron campos DTO ni los
schemas M1. Cinco snapshots M2 cambiaron metadata descriptiva.

Evidencia focal: `M2-059-regression.json` (snapshot histórico de esa ejecución),
application 18 tests, native replay 3, nuevo runtime real 1; luego protocol 38/38 y
Clippy MCP pasaron. Hubo mejoras posteriores solo en oráculos de tests nativos y
mock de replay; el nuevo full debe ejecutar archivos completos, no solo filtros.
El runtime nuevo cubre cinco commits, retry, restart, identidad equivocada y source
avanzado. No simula pérdida real del paquete de respuesta ni espera 600 s por TTL;
la expiración se cubre por composición con pruebas de reloj y la rama compartida.

## Gate de clientes: PASS estricto, ya finalizado

- `docs/validation/M2-clients.json`, schema v2, status `passed`.
- SHA-256 del receipt:
  `61958b34778bd8a62c52bff14135a1c0976a71d5ef7dfdd2ec139e4254509150`.
- Binario local calificado, 22,647,376 bytes:
  `41d256a606538d52df3c574e812b5fdd2df006cc659a335a447d4935f45e685c`.
- `scripts/test-m2-clients.py` SHA:
  `f18397e4694521b62d6764c3baa8dc52188e8feb8ad09e1321a91e51ba1905cc`.
- Inspector 2.5.0: discovery 18, 13 snapshots M1, open positivo y cinco denegaciones
  explícitas. Su exit 5 con `isError=true` es esperado para esas denegaciones.
- Claude Code 2.1.260, Sonnet 5 medium, cliente stock restringido a MCP: 17 llamadas
  y 17 resultados passed; cinco preview+commit, seis opens, receipt final committed;
  árboles finales exactos y Docker sin recursos propios restantes.
- Prueba PASS en `docs/validation/m2-clients/attempt-5/`. El prompt v2 explicita
  la referencia vigente; no afirmar que bastaron las descripciones por sí solas,
  pase al primer intento o una tasa general de fiabilidad.
- Intentos 1–4 permanecen fallidos y preservados: 1 expectativa incorrecta del
  harness sobre exit 5; 2 defecto de cuota corregido por ADR-059; 3 y 4 uso de ref
  vieja por el modelo y continuación contraria al criterio de abortar. El servidor
  rechazó correctamente el ref viejo. No convertir esos intentos en pases.
- ATENCIÓN: `M2-client-binary.json` todavía registra el binario anterior `3ab4be…`.
  Actualiza ese recibo con build/hash actuales y binding al NUEVO full al cerrar.

## Revisión independiente: aceptada; falta consolidación

Contrato, seguridad, writer, observabilidad y documentación tienen informes
`docs/reviews/M2-final-*-review.md` y `M2-observability-review.md`, con paquetes e
históricos. Lee sus residuales; no atribuyas bytes posteriores al reviewer previo.

La revisión ADR-059 inicial fue Revise por un P2 de texto NotFound, ya corregido.
El recheck terminó **Accepted**, sin P0/P1 pendientes en el delta:
`docs/reviews/M2-059-recheck-opus.json`, SHA
`e98457b081cc9ea1a19b82a7855f65303f792275a687b2285eb3ec701a29518c`.
Inputs: `M2-059-recheck-inputs.json`; primera revisión `M2-059-opus.json` e inputs.
Consolida `docs/reviews/M2-059-review.md` con findings/disposición y límites.
Fue Opus 5 high, read-only; metadata CLI registra auxiliar Haiku 4.5, que no debe
presentarse como otro reviewer ni sustitución de Opus. No es certificación del full.
La frase del recheck sobre clientes aún fallidos refleja su paquete anterior al
intento5: el recibo posterior PASS es la evidencia de cliente actual.

## Lo pendiente, en orden

1. Revisa el diff final como owner y congela código, scripts, fixtures y config.
   Consolida el recheck ADR-059. No repitas revisiones aceptadas sin un delta que
   lo justifique. Aplica la sección de agentes anterior para cualquier revisión
   adicional y la auditoría de trazabilidad AGY; registra cobertura o limitación.
2. Ejecuta NUEVO gate completo posterior a ADR-059. El actual
   `docs/validation/M2-full-gate.json` es PASS 24/24 de ANTES de ADR-059:
   830 resultados Rust + 1 doctest, 68 Python, 16 runtime M2/9 selecciones,
   573 inputs sin cambios. No acredita el código final.
   Sus JSON y logs ya están preservados bajo `docs/validation/m2-pre-059/` con
   README e inventario; conserva también `M2-full-attempt1.json` y attempt2.
3. El nuevo runtime debe ejecutar 17 casos en 10 selecciones, incluido
   `terminal_plan_runtime::terminal_plans_free_quota_and_replay_only_from_exact_durable_identity`.
   Obtén los demás conteos del resultado real, no los predigas. El full incluye
   las suites completas application/native y resuelve la petición del reviewer.
4. Conserva logs e inventarios del nuevo full, actualiza `M2-final-runtime.json` y
   `M2-final-log-inventory.json`, verifica source inventory sin cambios. No arrastres
   logs viejos de doctor: ya ocurrió con streams cancelados de otra ejecución.
5. Reconstruye release core locked/offline. Si SHA sigue siendo el `41d256…` del
   cliente, registra igualdad y vincula ambos gates sin repetirlo sin motivo.
   Si cambia, identifica el motivo y recalifica cliente cuando corresponda.
6. Sincroniza README, CHANGELOG, SECURITY, architecture, tools, security-model,
   compatibility, client-configuration, ADRs 050–059, implementation-status,
   M2-matrix, M2-07 y roadmap M2/M2–M8. Marca Done solo con evidencia final real.
   El cierre provisional contiene referencias/timings anteriores: reescríbelo
   coherentemente sin borrar históricos ni inflar claims. M3+ sigue planificado.
7. Comprueba links/fences, trazabilidad DoD, 13 contratos M1 idénticos, hashes,
   ausencia de recursos Docker propios y diff final. Incluye evidencia necesaria
   en Git: `*.log` está ignorado y requiere add explícito selectivo, no un force-add
   indiscriminado. Incluye también el raw completado `M2-02-native-recheck-opus.json`,
   que accidentalmente quedó vacío en 331d163; no reescribas ese commit.
8. Haz commits coherentes y merge local no-ff de la rama a main. Conserva la rama.
   Verifica igualdad de inputs calificados después del merge, smoke proporcional,
   registra hashes de integración y deja checkout limpio. No publiques nada.
9. Entrega cierre breve en español con resultado, commits/merge, gates/reviews,
   links a evidencia y límites operativos. Detente en M2; no inicies M3.

## Comando y entorno del full

macOS 26.6.2 ARM64/APFS, Docker Desktop existente. No ejecutes otros trabajos
Docker simultáneos con el gate. Duración previa aproximada: 20 minutos.

```sh
env RUST_MCP_TEST_SOCKET=/Users/cburgosro/.docker/run/docker.sock \
  RUST_MCP_E5_DIR=/private/tmp/rust-mcp-e5-m009/onnx \
  ORT_LIB_LOCATION=/Users/cburgosro/Library/Caches/ort.pyke.io/dfbin/aarch64-apple-darwin/612739f75438dc0a075461e1fb454226b4a1eb175e60a7271ba966bbbb972cd4 \
  python3 scripts/gate.py full --report docs/validation/M2-full-gate.json \
  > /tmp/M2-full-gate.log 2>&1
```

Verifica las rutas antes de usar; no instales si faltan. Imagen aprobada existente:
`sha256:8fac70723a8d04b6ec9633ab721806b8a55f4f083a1b3f988c61bf6a00fa1909`.
Cargo audit/deny estaban instalados y configurados offline. Build:
`cargo build --release --locked --offline -p rust-engineering-mcp`.
Servidor: `target/release/rust-engineering-mcp serve --stdio`.

## Límites que deben seguir visibles

- Solo macOS ARM64/APFS está calificado para publicación nativa positiva; no
  conviertas CI portable en soporte binario/capabilities Linux o Windows.
- Namespace estable/cooperación del host; proceso hijo contenido por sandbox.
- Journal parcial puede bloquear el store compartido. Remediación P2 aceptada:
  detener instancias, preservar originales y crear copia física revisada de cada
  workspace necesario + state privado nuevo + grants nuevos. Nunca reconectar
  roots en cuarentena ni borrar/recuperar bytes desconocidos automáticamente.
- Source 16 MiB/4096 entradas/1 MiB por archivo, 128 reemplazos; cuatro propuestas
  pendientes y 64 MiB. Journal 128 registros/256 MiB: 207 retenidos, 48 staging,
  1 MiB crecimiento. El margen nuevo no es retroactivo a stores dev de 208 MiB.
- RSS histórico nativo máximo optimizado 976,666,624 B; no mide todo el MCP ni
  representa cap. No se volvió a medir tras cada cambio; preserva esa limitación.
- Inyección ENOSPC/escritura parcial y crash por fases está acotada a los fixtures;
  no equivale a disco APFS físicamente lleno o pérdida eléctrica.
- Telemetría local vía tracing/stderr, sin garantía forense ni envío externo.
- Cargo Fix ejecuta código del proyecto aislado, permite loopback TCP dentro del
  guest sin red externa y hace postcheck independiente. El diff aprobado es la
  autoridad, no una supuesta procedencia exclusiva del compilador.

No hay workers ni gate Docker que debas heredar: cliente y recheck terminaron.
El usuario pidió este handoff antes de lanzar el nuevo full; continúa desde aquí.
