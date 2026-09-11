# Fable — terminar la calificación y cerrar M5 localmente

Actúa como Technical Owner e integrador de M5 en
`/Users/cburgosro/Projects/rust-mcp`. Continúa desde este relevo, verificado el
2026-09-10; no reinicies la implementación. Ejecuta el trabajo pendiente hasta
cerrarlo con evidencia, o informa un bloqueo real sin declarar Done.

## Autorización y límites

El owner autorizó completar `docs/prompts/complete-m5.md` con commits locales en
`ai/m5-performance`. Al finalizar hacer push, PR, merge, tag, release pero no avanzar a M6.
El host configura tu modelo: no afirmes cambiarlo por tu cuenta.

Lee `AGENTS.md`, la especificación completa si no está en tu contexto fiable,
`docs/implementation-status.md`, ADR-073..081 y este relevo. El código, los tests
y los recibos prevalecen sobre estados documentales que aún estén atrasados.

Conserva las decisiones del owner:

- No ampliar SourceBundle ni cambiar sus reglas de paths.
- No modificar el umbral material del 5 %, la puerta de precisión ni los
  criterios congelados de ADR-081.
- Mantener `METHOD_QUALIFIED_FOR_DIRECTION = false`. El encargo prohíbe
  habilitar direcciones; el cierre debe declarar esta limitación.
- No reconstruir la imagen admitida:
  `sha256:e0a5ca1661b3e49d0a3d68ee3cc0963453078d08eb7fc43c30538c16b7998aac`.
- Preservar las actualizaciones de dependencias del owner ya incorporadas en
  `a3cb48c`: lancedb 0.38.0, fastembed 6.0.3, jsonschema 0.55.1 y
  tokio-rustls 0.26.5. No separarlas ni revertirlas.

## Estado exacto al relevo

- Rama principal de trabajo: `ai/m5-performance`.
- HEAD: `aa935e6e7924caac35937e95a58cb42b125ccc1d`.
- `main` sigue en `c6099f27415b0be3838e84d21d25eed903c8c312`.
- Worktree de calificación: `/private/tmp/rust-mcp-m5-closure`, detached en el
  mismo HEAD, limpio al entregar este prompt.
- No hay ningún build ni gate M5 ejecutándose al relevo. Verifícalo antes de
  iniciar uno; la conversación anterior fue interrumpida para preparar esto.
- El árbol principal tiene recibos nativos nuevos y cambios de matriz sin
  commitear. Son trabajo válido de esta sesión: no descartarlos.
- Este prompt también queda sin commitear. Revisa `git status` antes de integrar.

Los cambios pendientes principales son `docs/validation/M5/runtime.json`, los
cinco JSON históricos por corte actualizados, `M5-matrix.md`, los nuevos
`M5-01-capture-runtime.json` y `M5-native-gate.json`, y las carpetas
`m5-gate-attempts/closure-native-{sandbox-attempt,standalone}/`.

## Implementado y revisado: no rehacer

1. El volumen de captura vendor usa 512 MiB/32768 inodos y las mismas opciones
   en creación, fingerprint y cleanup; el snapshot pequeño no cambió.
2. El governor se observa mediante argv cerrado sobre CPUs del guest. Ausencia,
   heterogeneidad o datos inválidos conservan desconocido. Timeout, cancelación
   y output limit abortan; no se supone el governor físico.
3. El replay de vendor comprueba sello y SHA-256 incremental, longitud exacta y
   byte extra **antes de entregar el último bloque**. Un check en EOF posterior
   no sirve: el supervisor deja de pedir bytes al llegar a la longitud prometida.
4. Logs acotados después de normalizar UTF-8, con reemplazo y truncación
   declarados. Bloat conserva el diagnóstico de la ejecución que determinó el fallo.
5. El P2 histórico de `verify_applied` se corrigió: M5 reutiliza la matriz
   completa de `rust_applied`, incluidas ambas vistas de mounts. Ningún caller
   M1–M4 cambió. El fingerprint M5 incluye ese verificador compartido.
6. El harness de clientes ya incluye dos benchmarks reales, comparación de sus
   IDs emitidos por el store, artifacts recuperados y un turno runtime dirigido
   por modelo. Falta ejecutar esa matriz final.

Commits de referencia: `d797ae1`, `e40962d`, `b7d2c9f`, `832525c`, `333d535`,
`89ec114`, `a2464c4`. Los posteriores hasta HEAD son documentales.

Revisiones con hashes y limitaciones:

- `docs/validation/M5/delegation/closure-local-vendor/`
- `docs/validation/M5/delegation/closure-local-semantics/`
- `docs/validation/M5/delegation/closure-applied-security/`
- `docs/reviews/M5/m5-security/disposition.md`, actualización de cierre.

El audit `closure-local-semantics/g1-g9-audit.md` describe un snapshot anterior
al fix de seguridad. Consérvalo como historia y escribe una disposición final;
no vuelvas a tratar ese P2 como sin corregir ignorando los commits posteriores.

## Evidencia ya conseguida

- Captura: 12 tests focalizados aprobados.
- Performance: 68 tests aprobados.
- Verificador compartido: 11 tests aprobados.
- Oráculo Python de clientes: 64 tests aprobados.
- Gate nativo independiente: **6/6 selecciones aprobadas**, exactas, ignoradas
  y seriales. Recibo: `docs/validation/M5/native-gate.json`.
- Criterion mediante captura: tres benchmarks, tres ejecuciones y 90 muestras
  por benchmark; warmup solicitado de 3000 ms y medición de 5000 ms por ejecución.
  Pasaron digest incorrecto, escritura rechazada y cancelación de ingesta.
- Profiling: positivo, cero muestras, denegación, cancelación, descendiente
  drenado y artifact precreado rechazado. Bloat: release, LTO y target ausente.
- Los cortes que usaron Docker terminaron sin contenedores ni volúmenes propios.
- El oráculo del límite del snapshot pequeño sigue registrando dos selecciones
  `blocked` intencionales; ese contrato no aloja Criterion. No es un bloqueo de
  la captura separada, cuyo positivo ya pasó.

El gate nativo midió `a2464c4`; el diff de código, scripts, fixtures, manifests y
AGENTS hasta HEAD es vacío. Los originales están en el worktree, bajo
`target/m5-runtime/` y `target/m5-runtime-gate/`, y se copiaron sin editar al
árbol principal. Los recibos anteriores se preservaron byte por byte en
`docs/validation/M5/history/closure-history/`.

El primer intento nativo dentro del sandbox devolvió `Unavailable`. El reintento
con acceso Docker autorizado pasó completo sin cambiar código ni imagen.
Conserva ambos intentos. La sesión al relevo tiene acceso completo; respeta la
política de permisos efectiva de tu sesión.

El servidor final para clientes **ya fue compilado** en el worktree:

```text
CARGO_INCREMENTAL=0 ORT_SKIP_DOWNLOAD=1 cargo build --release --locked --offline -p rust-engineering-mcp
```

Terminó con exit 0 en 23,36 s. Binario:
`/private/tmp/rust-mcp-m5-closure/target/release/rust-engineering-mcp`.
Log: `/private/tmp/m5-final-release-build.log`. No recompilarlo si sus fuentes
siguen iguales; si modificas código, sí debes reconstruirlo.

## Lo que falta, en orden

### 1. Disponer la deuda editorial anterior sin borrarla silenciosamente

Se interrumpió una revisión read-only de la lista de «Deuda de publicación» de:

```text
git show a2464c4:docs/validation/M5/matrix.md
```

Contrasta esa lista con el código actual. En particular, el receipt agregado
conserva `provenance.run_index=1` mientras las muestras v2 identifican las tres
repeticiones; revisa y documenta su semántica/limitación real. Comprueba también
los summaries constantes, bool frente a const, orden de razones, comentario del
decoder y asociación archive/logs de la lista antigua. No supongas que todos
siguen abiertos ni que todos se corrigieron. Dispone P2 según G8 y conserva los
P3 reales con archivo y seguimiento. No inventes una revisión que no terminó.

### 2. Ejecutar la matriz de clientes final

En el worktree limpio, con el binario actual:

```text
RUST_MCP_TEST_SOCKET=/Users/cburgosro/.docker/run/docker.sock python3 -B scripts/test-m5-clients.py --run --with-runtime
```

`docs/validation/M5/clients.json` **no existe** allí intencionalmente: el recibo
anterior fue archivado y su retirada se commiteó para que el script pueda
publicar uno nuevo. No restaures el viejo en esa ruta antes de correr la matriz.
Preserva los intentos nuevos, incluidos los fallidos, y copia los resultados al
árbol principal después de inspeccionarlos.

Los dos datasets del plan tienen `run_count=1`: el oráculo correcto es
`inconclusive_reasons == ["insufficient_executions"]`, no `method_unqualified`.
La guarda global false tiene evidencia separada; ese par no alcanza esa guarda.

El turno runtime dirigido por modelo exige discovery, comparación positiva,
rechazo `NOT_A_DATASET` usando otro artifact real y lectura de una Resource real.
Las trazas anteriores confirman `mcpToolCall` y `status=failed` para un bloqueo
de tool, pero **no confirman todavía** el shape exacto de los eventos native
`list_mcp_resources`/`read_mcp_resource`. Si el oráculo no coincide con la captura,
inspecciona el evento real y corrige únicamente una expectativa incorrecta con
prueba discriminante. No debilites las acciones/resultados exigidos para lograr PASS.

Inspector 2.5.0, Codex CLI 0.153.0 y el modelo Sol configurado por el harness
estaban disponibles. No copies credenciales ni instales herramientas
silenciosamente. La autenticación se maneja como ya prescribe el harness.

### 3. Ejecutar core y full sobre las fuentes finales, en exclusiva

**Todavía no se han ejecutado para cerrar M5.** Fija primero el código final y
un HEAD limpio; cualquier fix material exige revisión y evidencia proporcional.
No mezcles compiladores ni gates concurrentes.

Desde el worktree:

```sh
export RUST_MCP_TEST_SOCKET=/Users/cburgosro/.docker/run/docker.sock
export RUST_MCP_E5_DIR=/Users/cburgosro/Projects/rust-mcp/target/m1-15-candidate/assets/model
export ORT_LIB_LOCATION=/Users/cburgosro/Library/Caches/ort.pyke.io/dfbin/aarch64-apple-darwin/612739f75438dc0a075461e1fb454226b4a1eb175e60a7271ba966bbbb972cd4
python3 -B scripts/gate.py core --report target/M5-core-gate.json
python3 -B scripts/gate.py full --report target/M5-full-gate.json
```

Detente ante un fallo para diagnosticarlo y corregirlo; no ejecutes el siguiente
gate como si el anterior hubiera pasado. Conserva logs, comandos y exit codes.
`full` ya incluye `m5-runtime`: debe producir ese recibo dentro de la ejecución
conjunta, aunque el gate nativo independiente anterior ya pasó. Conserva ambas
mediciones. No repitas además una tercera suite nativa sin una causa nueva.

Antes de core/full, integra los outputs de clientes fuera del worktree de
medición y deja este limpio en el HEAD correspondiente. Los outputs de un gate
pueden ensuciar su árbol después; eso no autoriza iniciar con fuentes sin controlar.

Los assets, targets auxiliares, vendor materializado y bundle Inspector ya están
aprovisionados en el worktree. `cargo audit` 0.22.1 y `cargo deny` 0.19.7 estaban
instalados. No instalar ni descargar dependencias durante los gates offline.

Cargo avisa que el patch vendor de LanceDB 0.31 no participa en el lock de 0.38.
Ese aviso **no** prueba un bloqueo de `verify-vendor.py`: se ejecutó y pasó
(93 archivos, solo las dos ediciones de manifests admitidas). No reviertas la
versión del owner ni modifiques el verificador por una inferencia. Atiende los
fallos reales que arrojen los gates.

### 4. Integrar evidencia y cerrar documentación

Inspecciona los receipts finales y su inventario de fuentes; verifica que las
fuentes no cambiaron durante cada gate. Conserva originales, hashes, fallos e
intentos. Los `.log` pueden estar gitignorados: revisa qué evidencia se está
staging y añade explícitamente los logs necesarios tras comprobar su contenido.

Actualiza con hechos actuales:

- `docs/validation/M5/matrix.md`, `M5-handoff.md` y el mapa/disposición G1–G9.
- `docs/implementation-status.md` y `docs/roadmap/m5-performance.md`.
- README, CHANGELOG, SECURITY, arquitectura, tools, security-model,
  compatibility, client-configuration y CI cuando sus estados/claims cambien.
- Disposición de revisiones y P3 restantes.

Los documentos aún dicen calificación/cierre pendiente en varios lugares.
Marca Done local solo cuando M5-01..05 y G1–G9 estén demostrados, incluido
profiling positivo. Distingue ese cierre de la integración remota y su smoke,
que siguen pendientes de autorización. No publiques una release implícita.

Haz commits locales coherentes de los fixes y de los recibos/documentación.
Termina con un resumen de resultados, conteos reales de gates, límites y enlaces
a la evidencia; no concluyas simplemente que «compila».

## Delegación disponible

Si el host conserva los agentes de esta sesión, sus nombres eran:
Turing (`vendor_fix`, Sol High), Faraday (`independent_reviews`, Sol Medium) y
Sartre (`environment_observation`, Terra High). Turing terminó; los otros turnos
quedaron interrumpidos. Comprueba su estado y da paquetes nuevos, pequeños y
delimitados; no asumas que siguen trabajando ni reutilices contexto obsoleto.
Solo un worker puede ejecutar Cargo/Docker/gates a la vez.

El owner limitó Claude a Sonnet 5 u Opus 5 y autorizó fallback Sol/Terra.
Sonnet respondió `Not logged in`, cero tokens y `modelUsage` vacío; el reintento
no produjo informe. Opus no fue invocado. La evidencia está en
`closure-sonnet5-semantics/`. **No hay evidencia de cuota agotada ni de una
revisión Claude del delta final.** Usa el fallback declarado; no sustituyas
modelos silenciosamente. Spark solo si está disponible y aporta una ventaja
real en una tarea pequeña; no es requisito del cierre.
